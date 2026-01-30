use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::agent::{AgentOutput, AgentTraceEntry};
use crate::canonical_json;
use crate::report::DriftIssue;
use crate::tooling::{ToolRequest, ToolResponse, ToolTranscript};

/// Relative path to the ordeal task specification (resolved at runtime).
pub const ORDEAL_TASKS_PATH: &str = "tasks/ordeal.json";

/// Legacy path retained for compatibility with older test suites.
pub const LEGACY_GAUNTLET_TASKS_PATH: &str = "tasks/gauntlet.json";

/// Fixed number of tasks executed by the ordeal.
pub const ORDEAL_TASK_COUNT: usize = 50;

/// Resolve a task JSON path robustly across Unix + Windows CI.
///
/// Strategy:
/// 1. Try relative to current working directory
/// 2. Try relative to the crate root (via CARGO_MANIFEST_DIR)
fn resolve_task_path(rel: &str) -> Result<PathBuf> {
    let cwd = PathBuf::from(rel);
    if cwd.exists() {
        return Ok(cwd);
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    if manifest_dir.exists() {
        return Ok(manifest_dir);
    }

    bail!("task file not found: {}", rel);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSpec {
    pub task_id: u32,
    pub name: String,
    pub steps: Vec<StepSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepSpec {
    pub tool_name: String,
    #[serde(default)]
    pub contract: Option<String>,
    pub arguments: serde_json::Value,
    pub expect: ExpectedSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedSpec {
    pub response_schema_fingerprint: String,
    pub required_outputs: Vec<OutputSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputSpec {
    pub label: String,
    pub json_pointer: String,
    pub canon: String,
    #[serde(default)]
    pub expected_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskSuite {
    pub tasks: Vec<TaskSpec>,
}

impl TaskSuite {
    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| "failed to open ordeal tasks")?;
        let tasks: Vec<TaskSpec> =
            serde_json::from_reader(file).with_context(|| "failed to parse ordeal tasks")?;
        validate_tasks(&tasks)?;
        Ok(Self { tasks })
    }
}

fn validate_tasks(tasks: &[TaskSpec]) -> Result<()> {
    if tasks.len() != ORDEAL_TASK_COUNT {
        bail!(
            "ordeal tasks must contain {} tasks, got {}",
            ORDEAL_TASK_COUNT,
            tasks.len()
        );
    }

    let mut ids = BTreeSet::new();
    for task in tasks {
        if task.steps.is_empty() {
            bail!("ordeal task {} has no steps", task.task_id);
        }

        if !ids.insert(task.task_id) {
            bail!("ordeal task {} is duplicated", task.task_id);
        }
    }

    let expected: Vec<u32> = (0..ORDEAL_TASK_COUNT as u32).collect();
    let mut sorted: Vec<u32> = ids.into_iter().collect();
    sorted.sort_unstable();

    if sorted != expected {
        bail!("ordeal task ids must be sequential 0..49");
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct OrdealConfig {
    pub seed: u64,
    pub run_id: u32,
    pub case_id: String,
    pub pass_threshold_f32: f32,
    pub pass_threshold_witnessed: String,
    pub regress: bool,
}

#[derive(Debug, Clone)]
pub struct OrdealOutput {
    pub agent_trace: Vec<AgentTraceEntry>,
    pub issues: Vec<DriftIssue>,
    pub total_rng_calls: u64,
}

pub fn run_ordeal(
    suite: &TaskSuite,
    config: &OrdealConfig,
    transcript: &mut ToolTranscript,
) -> Result<OrdealOutput> {
    let mut agent_trace = Vec::new();
    let mut issues = Vec::new();
    let mut total_rng_calls = 0u64;
    let mut tool_call_idx = 0u32;

    let legacy_naming = suite
        .tasks
        .iter()
        .flat_map(|task| task.steps.iter())
        .any(|step| step.tool_name.starts_with("gauntlet."));

    for (step_index, task) in suite.tasks.iter().enumerate() {
        let step = step_index as u32;

        let thought = format!(
            "{} task {}:{} (case {})",
            if legacy_naming { "Gauntlet" } else { "Ordeal" },
            task.task_id,
            task.name,
            config.case_id
        );

        let action = format!("Execute {} steps", task.steps.len());
        let mut tool_requests = Vec::new();

        for spec in &task.steps {
            tool_requests.push(ToolRequest {
                tool_name: spec.tool_name.clone(),
                arguments: spec.arguments.clone(),
            });
        }

        let output = AgentOutput {
            thought,
            action,
            tool_requests: tool_requests.clone(),
            is_final: false,
        };

        agent_trace.push(AgentTraceEntry {
            step,
            role: "assistant".to_string(),
            thought: output.thought.clone(),
            action: output.action.clone(),
            tool_requests: output.tool_requests.clone(),
            is_final: output.is_final,
        });

        for spec in &task.steps {
            let request = ToolRequest {
                tool_name: spec.tool_name.clone(),
                arguments: spec.arguments.clone(),
            };

            let response = if matches!(transcript.mode(), crate::tooling::ToolMode::Live) {
                let generated = ordeal_stub_response(
                    config.seed,
                    config.run_id,
                    tool_call_idx,
                    &request,
                    config.regress,
                )?;
                transcript.execute_with_response(step, request.clone(), generated)
            } else {
                let replayed = transcript.execute(step, request.clone());
                if config.regress {
                    ordeal_stub_response(
                        config.seed,
                        config.run_id,
                        tool_call_idx,
                        &request,
                        true,
                    )?
                } else {
                    replayed
                }
            };

            let analysis =
                evaluate_expected(step, tool_call_idx, &request, &response, &spec.expect)?;
            issues.extend(analysis);

            tool_call_idx = tool_call_idx.saturating_add(1);
            total_rng_calls = total_rng_calls.saturating_add(1);
        }
    }

    let passed = 1.0f32 >= config.pass_threshold_f32;

    agent_trace.push(AgentTraceEntry {
        step: suite.tasks.len() as u32,
        role: "assistant".to_string(),
        thought: format!(
            "Finalize {} run (passed={}).",
            if legacy_naming { "gauntlet" } else { "ordeal" },
            passed
        ),
        action: format!("Score >= {}?", config.pass_threshold_witnessed),
        tool_requests: Vec::new(),
        is_final: true,
    });

    Ok(OrdealOutput {
        agent_trace,
        issues,
        total_rng_calls,
    })
}

fn normalize_tool_name_for_hash(tool_name: &str) -> String {
    if let Some(suffix) = tool_name.strip_prefix("ordeal.") {
        format!("gauntlet.{}", suffix)
    } else {
        tool_name.to_string()
    }
}

fn normalize_tool_name_for_match(tool_name: &str) -> &str {
    if let Some(suffix) = tool_name.strip_prefix("ordeal.") {
        return match suffix {
            "lookup" => "gauntlet.lookup",
            "search" => "gauntlet.search",
            "compute" => "gauntlet.compute",
            "page" => "gauntlet.page",
            _ => tool_name,
        };
    }

    tool_name
}

fn ordeal_stub_response(
    seed: u64,
    run_id: u32,
    tool_call_idx: u32,
    request: &ToolRequest,
    regress: bool,
) -> Result<ToolResponse> {
    let mut hasher = Sha256::new();
    hasher.update(seed.to_le_bytes());
    hasher.update(run_id.to_le_bytes());
    hasher.update(tool_call_idx.to_le_bytes());

    let hash_tool_name = normalize_tool_name_for_hash(&request.tool_name);
    hasher.update(hash_tool_name.as_bytes());

    let args_bytes = canonical_json::to_vec(&request.arguments)?;
    hasher.update(args_bytes);

    let digest = hasher.finalize();
    let hash = crate::hex::hex_lower(&digest);

    let page_title = if request.tool_name.starts_with("gauntlet.") {
        "Gauntlet"
    } else {
        "Ordeal"
    };

    let mut output = match normalize_tool_name_for_match(&request.tool_name) {
        "gauntlet.lookup" => serde_json::json!({
            "kind": "lookup",
            "id": hash,
            "meta": {
                "seed": seed,
                "run": run_id,
                "note": "baseline"
            },
            "payload": {
                "value": format!("lookup:{}", &hash[..8]),
                "tags": ["alpha", "beta", "gamma"],
                "count": 3
            }
        }),
        "gauntlet.search" => serde_json::json!({
            "kind": "search",
            "query": request.arguments.get("query").cloned().unwrap_or(serde_json::json!(null)),
            "hits": [
                {"rank": 1, "doc_id": format!("doc-{}", &hash[..6])},
                {"rank": 2, "doc_id": format!("doc-{}", &hash[6..12])},
            ],
            "extras": {
                "flags": ["x", "y"],
                "note": "extra"
            }
        }),
        "gauntlet.compute" => serde_json::json!({
            "kind": "compute",
            "inputs": request.arguments,
            "result": {
                "sum": hash[..4].to_string(),
                "checksum": hash,
                "formatted": "003.1400",
                "unicode": "café"
            }
        }),
        "gauntlet.page" => serde_json::json!({
            "kind": "page",
            "url": request.arguments.get("url").cloned().unwrap_or(serde_json::json!("")),
            "content": {
                "title": page_title,
                "body": "deterministic",
                "sections": [
                    {"title": "A", "id": 1},
                    {"title": "B", "id": 2}
                ]
            },
            "warnings": null
        }),
        _ => serde_json::json!({
            "kind": "unknown",
            "hash": hash,
            "args": request.arguments
        }),
    };

    if regress {
        if let Some(obj) = output.as_object_mut() {
            obj.insert("regressed".to_string(), serde_json::json!(true));

            if let Some(payload) = obj.get_mut("payload") {
                if let Some(payload_obj) = payload.as_object_mut() {
                    payload_obj.remove("tags");
                }
            }
        }
    }

    Ok(ToolResponse {
        tool_name: request.tool_name.clone(),
        output,
        success: true,
        simulated_latency_ms: None,
    })
}

fn evaluate_expected(
    step: u32,
    tool_call_idx: u32,
    request: &ToolRequest,
    response: &ToolResponse,
    expect: &ExpectedSpec,
) -> Result<Vec<DriftIssue>> {
    let mut issues = Vec::new();

    let actual_fp = schema_fingerprint(&response.output);
    if expect.response_schema_fingerprint != "any"
        && expect.response_schema_fingerprint != actual_fp
    {
        issues.push(DriftIssue::OrdealOutputMismatch {
            step,
            tool_call_idx,
            tool_name: request.tool_name.clone(),
            json_pointer: "".to_string(),
            label: "schema_fingerprint".to_string(),
            issue_kind: "schema".to_string(),
            expected: expect.response_schema_fingerprint.clone(),
            actual: actual_fp,
        });
    }

    for output in &expect.required_outputs {
        let extracted = match json_pointer_get(&response.output, &output.json_pointer) {
            Some(value) => value,
            None => {
                issues.push(DriftIssue::OrdealOutputMismatch {
                    step,
                    tool_call_idx,
                    tool_name: request.tool_name.clone(),
                    json_pointer: output.json_pointer.clone(),
                    label: output.label.clone(),
                    issue_kind: "missing".to_string(),
                    expected: output.canon.clone(),
                    actual: "null".to_string(),
                });
                continue;
            }
        };

        let canon = canonical_value_for_type(&output.canon, extracted);

        if let Some(expected_hash) = &output.expected_hash {
            let actual_hash = sha256_hex(canon.as_bytes());
            if expected_hash != &actual_hash {
                issues.push(DriftIssue::OrdealOutputMismatch {
                    step,
                    tool_call_idx,
                    tool_name: request.tool_name.clone(),
                    json_pointer: output.json_pointer.clone(),
                    label: output.label.clone(),
                    issue_kind: "hash".to_string(),
                    expected: expected_hash.clone(),
                    actual: actual_hash,
                });
            }
        }
    }

    Ok(issues)
}

pub fn schema_fingerprint(value: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    let mut buffer = String::new();
    build_shape(value, &mut buffer);
    hasher.update(buffer.as_bytes());
    let digest = hasher.finalize();
    let hex = crate::hex::hex_lower(&digest);
    debug_assert!(hex.len() >= 4, "SHA256 hex digest must be at least 4 chars");
    format!("sf:{}", hex)
}

fn build_shape(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(_) => out.push_str("bool"),
        serde_json::Value::Number(_) => out.push_str("number"),
        serde_json::Value::String(_) => out.push_str("string"),
        serde_json::Value::Array(arr) => {
            out.push('[');
            if let Some(first) = arr.first() {
                build_shape(first, out);
            }
            out.push(']');
        }
        serde_json::Value::Object(obj) => {
            out.push('{');
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort_unstable();
            for (idx, key) in keys.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                out.push_str(key);
                out.push(':');
                if let Some(val) = obj.get(*key) {
                    build_shape(val, out);
                }
            }
            out.push('}');
        }
    }
}

fn json_pointer_get<'a>(
    value: &'a serde_json::Value,
    pointer: &str,
) -> Option<&'a serde_json::Value> {
    if pointer.is_empty() || pointer == "/" {
        return Some(value);
    }

    let path = pointer.strip_prefix('/')?;
    let segments: Vec<&str> = path.split('/').collect();

    let mut current = value;
    for segment in segments {
        let unescaped = segment.replace("~1", "/").replace("~0", "~");
        current = match current {
            serde_json::Value::Object(obj) => obj.get(&unescaped)?,
            serde_json::Value::Array(arr) => {
                let idx: usize = unescaped.parse().ok()?;
                arr.get(idx)?
            }
            _ => return None,
        };
    }

    Some(current)
}

fn canonical_value_for_type(type_name: &str, value: &serde_json::Value) -> String {
    match type_name {
        "string" => value.as_str().unwrap_or("").to_string(),
        "number" => match value.as_f64() {
            Some(f) => format!("{:.6}", f),
            None => value.to_string(),
        },
        "bool" => value.as_bool().map(|b| b.to_string()).unwrap_or_default(),
        "array" => {
            let bytes = canonical_json::to_vec(value).unwrap_or_else(|_| b"[]".to_vec());
            String::from_utf8(bytes).unwrap_or_else(|_| "[]".to_string())
        }
        "object" => {
            let bytes = canonical_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
            String::from_utf8(bytes).unwrap_or_else(|_| "{}".to_string())
        }
        _ => value.to_string(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    crate::hex::hex_lower(&hasher.finalize())
}
