//! Provider-neutral accounting for a complete rendered invocation (#264).
use crate::{ContentDigest, ReferenceId, ResolvedReference, SchemaVersion};

pub const PROMPT_RENDERING_VERSION: SchemaVersion = SchemaVersion::V1;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenizerProfile {
    pub schema_version: SchemaVersion,
    pub model: ResolvedReference<ReferenceId>,
    pub generation_profile: ContentDigest,
    pub tokenizer: ReferenceId,
    pub tokenizer_version: ReferenceId,
    pub renderer: ReferenceId,
    pub template_version: ReferenceId,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CountMode {
    Exact,
    Estimated,
}
/// The tokenizer allocates the full payload count across components. Framing
/// includes roles, wrappers, bindings, planning, examples and verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenUsage {
    pub instruction_tokens: u64,
    pub task_tokens: u64,
    pub context_tokens: u64,
    pub evidence_tokens: u64,
    pub tool_schema_tokens: u64,
    pub framing_tokens: u64,
}
impl TokenUsage {
    pub fn total(&self) -> Option<u64> {
        [
            self.instruction_tokens,
            self.task_tokens,
            self.context_tokens,
            self.evidence_tokens,
            self.tool_schema_tokens,
            self.framing_tokens,
        ]
        .into_iter()
        .try_fold(0u64, u64::checked_add)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenCountResult {
    pub profile: TokenizerProfile,
    pub payload_digest: ContentDigest,
    pub mode: CountMode,
    /// Exact-tokenizer revision or documented conservative-bound method revision.
    pub provenance: ResolvedReference<ReferenceId>,
    pub usage: TokenUsage,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenBudgetSnapshot {
    pub count: TokenCountResult,
    pub reserved_output_tokens: u64,
    pub total_input_tokens: u64,
    pub context_limit: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RenderDiagnostic {
    InvalidBinding,
    UnsupportedRenderer,
    TokenizerUnavailable,
    InvalidAccounting,
    RenderingFailed,
}
