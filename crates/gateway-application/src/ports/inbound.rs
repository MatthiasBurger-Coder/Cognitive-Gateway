use gateway_domain::{ContextScopeId, Intent, execution_context::ExecutionContext};

use crate::{
    context::ExecutionRequest,
    external_context::{ContextScope, IngestionReceipt, IngestionResult, ScopedObservationBatch},
};

pub trait ResolveExecutionContext {
    type Error;

    fn resolve(&self, request: &ExecutionRequest<'_>) -> Result<ExecutionContext, Self::Error>;
}

/// Driving port for one structured Intent/DesiredState submission.
pub trait DeclarativeIntentInputPort {
    type Error;

    fn submit_intent(
        &self,
        scope: &ContextScope,
        intent: Intent,
    ) -> Result<IngestionResult, Self::Error>;
}

/// Driving port for validated observation/evidence/provenance ingestion.
pub trait ObservationEvidenceInputPort {
    type Error;

    fn ingest_observations(
        &self,
        scope: &ContextScope,
        batch: ScopedObservationBatch,
    ) -> Result<IngestionReceipt, Self::Error>;
}

/// Driving port for explicit transient-scope lifecycle management.
pub trait ScopeLifecyclePort {
    type Error;

    fn open_scope(&self, id: ContextScopeId) -> Result<ContextScope, Self::Error>;

    fn seal_scope(&self, scope: &ContextScope) -> Result<ContextScope, Self::Error>;

    fn close_scope(&self, scope: &ContextScope) -> Result<ContextScope, Self::Error>;
}
