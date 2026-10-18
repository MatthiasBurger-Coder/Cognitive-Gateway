//! Versioned run-wide invocation accounting. No provider or lifecycle authority.
use crate::{SchemaVersion, prompt_ir::InvocationIrIdentity, prompt_rendering::TokenizerProfile};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationRunLimits {
    pub schema_version: SchemaVersion,
    pub max_attempts: u16,
    pub timeout_ms: u32,
    pub invocation_input_tokens: u64,
    pub invocation_output_tokens: u64,
    pub run_input_tokens: u64,
    pub run_output_tokens: u64,
    /// Integer micro-units in one host-selected currency for the entire run.
    pub cost_microunits: u64,
    pub output_bytes: u64,
    /// Absolute input-token discrepancy that must be visible in telemetry.
    pub reconciliation_tolerance: u64,
}
impl InvocationRunLimits {
    pub fn validate(&self) -> Result<(), InvocationRunDiagnostic> {
        if self.schema_version != SchemaVersion::V1 {
            return Err(InvocationRunDiagnostic::IncompatibleVersion);
        }
        if self.max_attempts == 0
            || self.timeout_ms == 0
            || self.invocation_input_tokens == 0
            || self.invocation_output_tokens == 0
            || self.run_input_tokens == 0
            || self.run_output_tokens == 0
            || self.output_bytes == 0
        {
            return Err(InvocationRunDiagnostic::InvalidLimits);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InvocationRunDiagnostic {
    IncompatibleVersion,
    InvalidLimits,
    InvalidBinding,
    AuthorityMismatch,
    LifecycleBlocked,
    Cancelled,
    DeadlineExceeded,
    AttemptBudgetExceeded,
    TokenCountUnavailable,
    InputTokenBudgetExceeded,
    OutputTokenBudgetExceeded,
    ContextWindowExceeded,
    CostBudgetExceeded,
    OutputBudgetExceeded,
    UsageReconciliationMismatch,
    UncertainSideEffect,
    HardFailure,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageProvenance {
    LocalExact,
    LocalEstimated,
    ProviderReported,
}
/// Adapters must correlate usage to this exact dispatch, not just the model name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptBinding {
    pub schema_version: SchemaVersion,
    pub attempt: u16,
    pub invocation: InvocationIrIdentity,
    pub profile: TokenizerProfile,
    pub payload_digest: crate::ContentDigest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderUsage {
    pub binding: AttemptBinding,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_microunits: u64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct InvocationRunUsage {
    pub attempts: u16,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_microunits: u64,
    pub output_bytes: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct AttemptUsageRecord {
    pub binding: AttemptBinding,
    pub local_input_tokens: u64,
    pub local_provenance: UsageProvenance,
    pub reserved_output_tokens: u64,
    pub reserved_cost_microunits: u64,
    pub reported: Option<ProviderUsage>,
    pub reported_provenance: Option<UsageProvenance>,
    pub diagnostics: Vec<InvocationRunDiagnostic>,
}
/// A transport failure with uncertain effects is always terminal, even if an
/// outer lifecycle controller requested retry. No connector replay occurs here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InvocationAttemptFailure {
    Transient,
    Hard,
    UncertainSideEffect,
}
