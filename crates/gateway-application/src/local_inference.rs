//! Optional cognitive service boundary. Returned JSON is a proposal and must pass
//! the caller's semantic, policy and authority validation before use.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalInferenceRequest {
    pub schema_version: String,
    pub role: String,
    pub input_contract: String,
    pub output_contract: String,
    pub prompt: String,
    pub output_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalInferenceProposal {
    pub schema_version: String,
    pub kind: String,
    pub model_id: String,
    pub artifact_digest: String,
    pub proposal: Value,
    pub metrics: Value,
    pub provenance: LocalInferenceProvenance,
}

/// Immutable model and invocation inputs retained with every signal. This
/// records origin, not permission to apply the proposed output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalInferenceProvenance {
    pub model_version: String,
    pub runtime: String,
    pub runtime_version: String,
    pub runtime_configuration: Value,
    pub prompt_version: String,
    pub template_digest: String,
    pub system_digest: String,
    pub input_contract: String,
    pub output_contract: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalInferenceError {
    Unavailable,
    InvalidProposal,
    InvalidRequest,
}

pub trait LocalInferencePort {
    fn infer(
        &self,
        request: &LocalInferenceRequest,
    ) -> Result<LocalInferenceProposal, LocalInferenceError>;
}
