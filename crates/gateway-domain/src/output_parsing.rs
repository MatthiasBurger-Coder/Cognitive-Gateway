//! Provider-neutral stream termination and non-executable repair contracts.
use crate::{SchemaVersion, output_verification::VerificationBinding};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OutputCompletion {
    Complete,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RepairError {
    InvalidBudget,
    BudgetExhausted,
    RepairNotPermitted,
}

/// Audit/proposal only; this record grants no admission or dispatch authority.
/// The original invocation pins scope, sensitivity, schema and exposed inventory.
#[derive(Debug, Clone, Serialize)]
pub struct OutputRepairProposal {
    pub schema_version: SchemaVersion,
    pub failed_output: VerificationBinding,
    pub diagnostic: crate::output_verification::VerificationError,
    pub attempt: u8,
    pub max_output_bytes: usize,
}

/// Shared response ceiling for complete and streamed parsing.
pub const MAX_OUTPUT_BYTES: usize = 1_048_576;

pub fn validate_repair_budget(
    max_attempts: u8,
    max_output_bytes: usize,
) -> Result<(), RepairError> {
    if max_attempts == 0
        || max_attempts > 3
        || max_output_bytes == 0
        || max_output_bytes > MAX_OUTPUT_BYTES
    {
        return Err(RepairError::InvalidBudget);
    }
    Ok(())
}
