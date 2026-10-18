//! Versioned verification records. Acceptance grants no execution authority.
use crate::prompt_ir::InvocationIrIdentity;
use crate::semantic_output::{CheckOutcome, ResultValidity};
use crate::{CapabilityId, ContentDigest, ReferenceId, ResolvedReference, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerificationError {
    OutputParseFailed,
    OutputLimitExceeded,
    IncompatibleVersion,
    CorrelationMismatch,
    OutputSchemaInvalid,
    UnsupportedReference,
    InvalidToolProposal,
    StaleContext,
    AuthorityDenied,
    VerificationUnavailable,
    ConstraintViolation,
    SemanticVerificationFailed,
}

/// Pins are attached to actual JSON-pointer locations in the result payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputReference {
    pub pointer: String,
    pub binding: ResolvedReference<ReferenceId>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCallProposal {
    pub capability: ResolvedReference<CapabilityId>,
    pub arguments: Value,
}
/// Strict wire envelope, always provisional until the application verifies it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputProposalData {
    pub schema_version: SchemaVersion,
    pub invocation: InvocationIrIdentity,
    pub result: Value,
    pub references: Vec<OutputReference>,
    pub tools: Vec<ToolCallProposal>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationBinding {
    pub invocation: InvocationIrIdentity,
    pub output_digest: ContentDigest,
}
/// Audit output only: consumers must use the application result, not deserialize
/// an acceptance claim from model/transport input.
#[derive(Debug, Clone, Serialize)]
pub struct VerificationRecord {
    pub admitted_invocation: InvocationIrIdentity,
    pub schema_version: SchemaVersion,
    pub binding: VerificationBinding,
    pub validity: ResultValidity,
    pub diagnostic: Option<VerificationError>,
    pub critics: Vec<CheckOutcome>,
}

/// Structural verification cannot establish entity existence or current policy.
/// Those checks are supplied by the trusted application host after this gate.
pub fn validate_output_proposal(
    expected: &InvocationIrIdentity,
    contract: &crate::semantic_output::OutputContract,
    data: &OutputProposalData,
) -> Result<(), VerificationError> {
    use VerificationError as E;
    if &data.invocation != expected {
        return Err(E::CorrelationMismatch);
    }
    if !contract.shape.accepts(&data.result) {
        return Err(E::OutputSchemaInvalid);
    }
    let mut pointers = std::collections::BTreeSet::new();
    for reference in &data.references {
        let pin = &reference.binding;
        if crate::model_invocation::validate_model_binding(pin).is_err()
            || pin.scope != contract.schema.scope
            || !pointers.insert(&reference.pointer)
            || data
                .result
                .pointer(&reference.pointer)
                .and_then(Value::as_str)
                != Some(pin.id.as_str())
        {
            return Err(E::UnsupportedReference);
        }
    }
    Ok(())
}
