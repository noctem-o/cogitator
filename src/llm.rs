use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::canonical_json;
use crate::tooling::{ToolRequest, ToolResponse};

pub const LLM_REQUEST_SCHEMA_VERSION: u32 = 1;
pub const LLM_RESPONSE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LlmRequest {
    pub schema_version: u32,
    pub model: String,
    pub messages: Vec<LlmMessage>,
    pub temperature: Option<u32>,
    pub max_tokens: Option<u32>,
    pub seed: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LlmResponse {
    pub schema_version: u32,
    pub content: String,
}

impl LlmRequest {
    pub fn tool_name() -> &'static str {
        "llm.generate"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum LlmBackendKind {
    Stub,
}

pub trait LlmBackend {
    fn generate(&self, req: &LlmRequest) -> Result<LlmResponse>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StubLlmBackend;

impl LlmBackend for StubLlmBackend {
    fn generate(&self, req: &LlmRequest) -> Result<LlmResponse> {
        let bytes = canonical_json::to_vec(req).context("serialize llm request")?;
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hasher.finalize();
        let hash = hex_string(&digest);
        let short = &hash[..16.min(hash.len())];
        Ok(LlmResponse {
            schema_version: LLM_RESPONSE_SCHEMA_VERSION,
            content: format!(
                "STUB:{} model={} msgs={}",
                short,
                req.model,
                req.messages.len()
            ),
        })
    }
}

pub fn make_tool_request(req: &LlmRequest) -> Result<ToolRequest> {
    let arguments = canonical_json::to_value(req).context("llm request to value")?;
    Ok(ToolRequest {
        tool_name: LlmRequest::tool_name().to_string(),
        arguments,
    })
}

pub fn parse_tool_request(request: &ToolRequest) -> Result<LlmRequest> {
    if request.tool_name != LlmRequest::tool_name() {
        bail!("unexpected llm tool name: {}", request.tool_name);
    }
    let parsed: LlmRequest =
        serde_json::from_value(request.arguments.clone()).context("parse llm request")?;
    if parsed.schema_version != LLM_REQUEST_SCHEMA_VERSION {
        bail!("unsupported llm request schema: {}", parsed.schema_version);
    }
    Ok(parsed)
}

#[allow(dead_code)]
pub fn parse_tool_response(response: &ToolResponse) -> Result<LlmResponse> {
    if response.tool_name != LlmRequest::tool_name() {
        bail!("unexpected llm tool name: {}", response.tool_name);
    }
    let parsed: LlmResponse =
        serde_json::from_value(response.output.clone()).context("parse llm response")?;
    if parsed.schema_version != LLM_RESPONSE_SCHEMA_VERSION {
        bail!("unsupported llm response schema: {}", parsed.schema_version);
    }
    Ok(parsed)
}

pub fn response_to_tool_output(response: &LlmResponse) -> Result<serde_json::Value> {
    canonical_json::to_value(response).context("llm response to value")
}

fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{:02x}", byte));
    }
    out
}
