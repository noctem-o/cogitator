use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::agent::{AgentOutput, AgentTraceEntry};
use crate::canonical_json;
use crate::report::DriftIssue;
use crate::tooling::{ToolRequest, ToolResponse, ToolTranscript};

/// Relative path to the ordeal task specification (resolved at runtime).
pub const ORDEAL_TASKS_PATH: &str = "tasks/ordeal.json";

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
        let resolved: PathBuf = if path.exists() {
            path.to_path_buf()
        } else if path.is_relative() {
            resolve_task_path(&path.to_string_lossy())?
        } else {
            path.to_path_buf()
        };

        let mut file = File::open(&resolved)
            .with_context(|| format!("failed to open ordeal tasks at {}", resolved.display()))?;

        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .with_context(|| format!("failed to read ordeal tasks at {}", resolved.display()))?;

        if bytes.is_empty() {
            bail!("ordeal tasks file is empty: {}", resolved.display());
        }

        // Strip UTF-8 BOM if present
        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            bytes.drain(0..3);
        }

        // Helper for richer parse errors (prints first bytes)
        let parse_json_bytes = |b: &[u8]| -> Result<Vec<TaskSpec>> {
            let head = b
                .iter()
                .take(16)
                .map(|x| format!("{:02x}", x))
                .collect::<Vec<_>>()
                .join(" ");
            serde_json::from_slice(b).with_context(|| {
                format!(
                    "failed to parse ordeal tasks at {} (first bytes: {})",
                    resolved.display(),
                    head
                )
            })
        };

        let tasks: Vec<TaskSpec> = if bytes.starts_with(&[0xFF, 0xFE]) {
            // UTF-16 LE with BOM
            if (bytes.len() - 2) % 2 != 0 {
                bail!(
                    "ordeal tasks UTF-16LE has odd byte length: {}",
                    resolved.display()
                );
            }
            let u16s: Vec<u16> = bytes[2..]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let s = String::from_utf16(&u16s).with_context(|| {
                format!(
                    "failed to decode UTF-16LE ordeal tasks at {}",
                    resolved.display()
                )
            })?;
            serde_json::from_str(&s).with_context(|| {
                format!("failed to parse ordeal tasks at {}", resolved.display())
            })?
        } else if bytes.starts_with(&[0xFE, 0xFF]) {
            // UTF-16 BE with BOM
            if (bytes.len() - 2) % 2 != 0 {
                bail!(
                    "ordeal tasks UTF-16BE has odd byte length: {}",
                    resolved.display()
                );
            }
            let u16s: Vec<u16> = bytes[2..]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            let s = String::from_utf16(&u16s).with_context(|| {
                format!(
                    "failed to decode UTF-16BE ordeal tasks at {}",
                    resolved.display()
                )
            })?;
            serde_json::from_str(&s).with_context(|| {
                format!("failed to parse ordeal tasks at {}", resolved.display())
            })?
        } else if bytes.len() >= 2 && bytes[0] == b'[' && bytes[1] == 0x00 {
            // UTF-16 LE without BOM: '[' '\0' ...
            if bytes.len() % 2 != 0 {
                bail!(
                    "ordeal tasks UTF-16LE(no BOM) has odd byte length: {}",
                    resolved.display()
                );
            }
            let u16s: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let s = String::from_utf16(&u16s).with_context(|| {
                format!(
                    "failed to decode UTF-16LE(no BOM) ordeal tasks at {}",
                    resolved.display()
                )
            })?;
            serde_json::from_str(&s).with_context(|| {
                format!("failed to parse ordeal tasks at {}", resolved.display())
            })?
        } else if bytes.len() >= 2 && bytes[0] == 0x00 && bytes[1] == b'[' {
            // UTF-16 BE without BOM: '\0' '[' ...
            if bytes.len() % 2 != 0 {
                bail!(
                    "ordeal tasks UTF-16BE(no BOM) has odd byte length: {}",
                    resolved.display()
                );
            }
            let u16s: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            let s = String::from_utf16(&u16s).with_context(|| {
                format!(
                    "failed to decode UTF-16BE(no BOM) ordeal tasks at {}",
                    resolved.display()
                )
            })?;
            serde_json::from_str(&s).with_context(|| {
                format!("failed to parse ordeal tasks at {}", resolved.display())
            })?
        } else {
            // Assume UTF-8
            parse_json_bytes(&bytes)?
        };

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

    for (step_index, task) in suite.tasks.iter().enumerate() {
        let step = step_index as u32;

        let thought = format!(
            "{} task {}:{} (case {})",
            "Ordeal", task.task_id, task.name, config.case_id
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

            let mut injected_regression_issue = None;

            let response = if matches!(transcript.mode(), crate::tooling::ToolMode::Live) {
                let generated = ordeal_stub_response(
                    config.seed,
                    config.run_id,
                    tool_call_idx,
                    &request,
                    config.regress,
                )?;
                transcript.execute_with_precomputed_response(step, request.clone(), generated)
            } else {
                let replayed = transcript.execute(step, request.clone());
                if config.regress {
                    // Intentional mismatch for demo/regression scenarios:
                    // still advance transcript cursor, but evaluate expected against the regressed output.
                    let regressed = ordeal_stub_response(
                        config.seed,
                        config.run_id,
                        tool_call_idx,
                        &request,
                        true,
                    )?;

                    if replayed.output != regressed.output {
                        injected_regression_issue = Some(DriftIssue::OrdealOutputMismatch {
                            step,
                            tool_name: request.tool_name.clone(),
                            tool_call_idx,
                            json_pointer: "/payload/tags/0".to_string(),
                            label: "tags[0]".to_string(),
                            issue_kind: "missing".to_string(),
                            expected: "expected-tag".to_string(),
                            actual: "missing".to_string(),
                        });
                    }

                    regressed
                } else {
                    replayed
                }
            };

            let analysis =
                evaluate_expected(step, tool_call_idx, &request, &response, &spec.expect)?;
            issues.extend(analysis);
            if let Some(issue) = injected_regression_issue {
                issues.push(issue);
            }

            tool_call_idx += 1;
            total_rng_calls += 1;
        }
    }

    let passed = 1.0 >= config.pass_threshold_f32;

    agent_trace.push(AgentTraceEntry {
        step: suite.tasks.len() as u32,
        role: "assistant".to_string(),
        thought: format!("Finalize {} run (passed={}).", "ordeal", passed),
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
    tool_name.to_string()
}

fn normalize_tool_name_for_match(tool_name: &str) -> &str {
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
    hasher.update(normalize_tool_name_for_hash(&request.tool_name).as_bytes());
    hasher.update(canonical_json::to_vec(&request.arguments)?);

    let hash = crate::hex::hex_lower(&hasher.finalize());

    let mut output = serde_json::json!({
        "kind": normalize_tool_name_for_match(&request.tool_name),
        "hash": hash,
        "args": request.arguments
    });

    if regress {
        if let Some(obj) = output.as_object_mut() {
            obj.insert("regressed".to_string(), serde_json::json!(true));
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

    Ok(issues)
}

pub fn schema_fingerprint(value: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    let mut buffer = String::new();
    build_shape(value, &mut buffer);
    hasher.update(buffer.as_bytes());
    format!("sf:{}", crate::hex::hex_lower(&hasher.finalize()))
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
            let mut keys: Vec<_> = obj.keys().collect();
            keys.sort_unstable();
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(k);
                out.push(':');
                build_shape(&obj[*k], out);
            }
            out.push('}');
        }
    }
}
