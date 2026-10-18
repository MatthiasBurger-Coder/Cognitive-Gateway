//! Immutable invocation correlation contracts. Content authentication belongs to
//! the trusted application host, never to a renderer or a model.
use crate::{ContentDigest, ReferenceId, ResolvedReference, SchemaVersion, TaskId};
use serde::{Deserialize, Serialize};

pub const INVOCATION_IR_VERSION: SchemaVersion = SchemaVersion::V1;

/// Exact producer pins; schema and verification *contents* have independent pins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptBindings {
    pub task: ResolvedReference<TaskId>,
    pub requirements: ResolvedReference<ReferenceId>,
    pub plan: ResolvedReference<ReferenceId>,
    pub context: ResolvedReference<ReferenceId>,
    pub execution_projection: ResolvedReference<ReferenceId>,
    pub examples: Option<ResolvedReference<ReferenceId>>,
    pub output: ResolvedReference<ReferenceId>,
    pub verification: ResolvedReference<ReferenceId>,
}

/// Current host checks fail closed. A missing store/decision is a denial, not
/// permission to trust the supplied pin or treat a model response as evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InvocationIrError {
    IncompatibleVersion,
    InvalidBinding,
    StaleInput,
    AuthorityDenied,
    DisclosureDenied,
    IncompleteTask,
    AuthenticationUnavailable,
}

/// Correlates downstream rendering/verification with the complete admitted IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvocationIrIdentity {
    pub schema_version: SchemaVersion,
    pub invocation_id: ReferenceId,
    pub invocation_digest: ContentDigest,
    pub handoff_digest: ContentDigest,
    pub tools_digest: ContentDigest,
    pub ir_digest: ContentDigest,
}

impl PromptBindings {
    /// Validate exact revision/digest syntax and scope without asserting source
    /// authenticity. The host still authenticates every referenced artifact.
    pub fn validate(&self) -> Result<(), InvocationIrError> {
        use crate::model_invocation::validate_model_binding;
        validate_model_binding(&self.task).map_err(|_| InvocationIrError::InvalidBinding)?;
        for pin in [
            &self.requirements,
            &self.plan,
            &self.context,
            &self.execution_projection,
            &self.output,
            &self.verification,
        ]
        .into_iter()
        .chain(self.examples.iter())
        {
            validate_model_binding(pin).map_err(|_| InvocationIrError::InvalidBinding)?;
            if pin.scope != self.task.scope {
                return Err(InvocationIrError::InvalidBinding);
            }
        }
        Ok(())
    }
}
