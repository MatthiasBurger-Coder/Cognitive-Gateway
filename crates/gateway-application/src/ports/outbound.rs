use gateway_domain::{
    CapabilityId, ContextCacheEntryId, ContextScopeId, KnowledgeQuery, RetrievedKnowledge,
};

use crate::{
    context::ProjectContext,
    external_context::{
        CacheCapabilities, CacheEntry, ContextScope, IngestionResult, ScopedObservationBatch,
        SourceSnapshot,
    },
};

/// A retrieval request with an explicit optional consuming-project scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnowledgeRequest<'a> {
    query: &'a KnowledgeQuery,
    project_context: Option<&'a ProjectContext>,
}

impl<'a> KnowledgeRequest<'a> {
    /// Creates a Gateway-catalog-only retrieval request.
    #[must_use]
    pub const fn new(query: &'a KnowledgeQuery) -> Self {
        Self {
            query,
            project_context: None,
        }
    }

    /// Adds request-scoped consuming-project context to the retrieval scope.
    #[must_use]
    pub const fn with_project_context(mut self, context: &'a ProjectContext) -> Self {
        self.project_context = Some(context);
        self
    }

    /// Returns the semantic retrieval query.
    #[must_use]
    pub const fn query(&self) -> &KnowledgeQuery {
        self.query
    }

    /// Returns the optional consuming-project retrieval scope.
    #[must_use]
    pub const fn project_context(&self) -> Option<&ProjectContext> {
        self.project_context
    }
}

pub trait KnowledgePort {
    type Error;

    fn retrieve(
        &self,
        request: &KnowledgeRequest<'_>,
    ) -> Result<Vec<RetrievedKnowledge>, Self::Error>;
}

pub trait CapabilityPort {
    type Error;

    fn is_available(&self, capability: &CapabilityId) -> Result<bool, Self::Error>;
}

pub trait ExecutionRuntimePort {
    type Error;

    fn runtime_id(&self) -> Result<String, Self::Error>;
}

pub trait EvidencePort {
    type Error;

    fn record(&self, event: &str) -> Result<(), Self::Error>;
}

/// Provider-neutral source adapter contract for repository, Git, CI, runtime
/// or retrieval-style inputs.
pub trait ScopedContextSource {
    type Error;

    fn source_snapshot(&self, scope: &ContextScope) -> Result<SourceSnapshot, Self::Error>;

    fn collect(
        &self,
        scope: &ContextScope,
        snapshot: &SourceSnapshot,
    ) -> Result<ScopedObservationBatch, Self::Error>;
}

/// Derived, scoped and explicitly invalidatable external-context cache port.
pub trait CachePort {
    type Error;

    fn capabilities(&self) -> CacheCapabilities;

    fn put(&self, entry: CacheEntry) -> Result<IngestionResult, Self::Error>;

    fn get(
        &self,
        scope: &ContextScopeId,
        id: &ContextCacheEntryId,
    ) -> Result<CacheEntry, Self::Error>;

    fn invalidate(
        &self,
        scope: &ContextScopeId,
        id: &ContextCacheEntryId,
    ) -> Result<bool, Self::Error>;

    fn clear_scope(&self, scope: &ContextScopeId) -> Result<usize, Self::Error>;
}

/// Plans advisory information acquisition from an explicit immutable request.
/// Implementations must preserve the request's scope, requirements, limits and order.
pub trait RetrievalPlanner {
    fn plan(
        &self,
        request: &gateway_domain::RetrievalRequest,
    ) -> Result<gateway_domain::RetrievalPlan, gateway_domain::RetrievalError>;
}

/// Executes one bounded round. Implementations reserve usage before dispatch,
/// validate cumulative accounting and return explicit unavailable/degraded outcomes.
pub trait KnowledgeRetrievalPort {
    fn retrieve(
        &self,
        plan: &gateway_domain::RetrievalPlan,
        round: gateway_domain::RetrievalRound,
        usage: &gateway_domain::BudgetUsage,
    ) -> Result<gateway_domain::RetrievalBatch, gateway_domain::RetrievalError>;

    /// A failed dispatch still consumes a round. Concrete adapters can
    /// override this to report measured elapsed time and other consumed work.
    fn retrieve_measured(
        &self,
        plan: &gateway_domain::RetrievalPlan,
        round: gateway_domain::RetrievalRound,
        usage: &gateway_domain::BudgetUsage,
    ) -> Result<gateway_domain::RetrievalBatch, RetrievalAttemptFailure> {
        self.retrieve(plan, round, usage).map_err(|error| {
            let mut consumed = usage.clone();
            consumed.rounds = round.0.get();
            RetrievalAttemptFailure {
                error,
                usage: consumed,
            }
        })
    }
}

/// Measurement returned for a failed or cancelled retrieval attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalAttemptFailure {
    pub error: gateway_domain::RetrievalError,
    pub usage: gateway_domain::BudgetUsage,
}

/// Produces derived vectors with source snapshot and model/version lineage.
pub trait EmbeddingPort {
    fn embed(
        &self,
        request: &gateway_domain::EmbeddingRequest,
    ) -> Result<gateway_domain::EmbeddingResult, gateway_domain::RetrievalError>;
}

/// Replaceable tokenizer/estimator boundary. Unknown counts must stay unknown.
pub trait TokenEstimatorPort {
    fn estimate(
        &self,
        request: &gateway_domain::TokenEstimateRequest,
    ) -> Result<gateway_domain::TokenEstimate, gateway_domain::RetrievalError>;

    /// Counts one semantic context section for the named target. Implementations
    /// that only support retrieved fragments fail closed through this default.
    fn estimate_context(
        &self,
        _request: &ContextTokenEstimateRequest<'_>,
    ) -> Result<gateway_domain::TokenEstimate, gateway_domain::RetrievalError> {
        Err(gateway_domain::RetrievalError::InvalidEstimate)
    }
}

pub struct ContextTokenEstimateRequest<'a> {
    pub scope: &'a gateway_domain::ContextScopeId,
    pub target: &'a gateway_domain::NonEmptyText,
    pub content: &'a str,
}

/// Optional non-authoritative compaction boundary. The selector validates each
/// derived artifact and its source lineage before it can enter compilation.
pub trait ContextCompactionPort {
    fn compact(
        &self,
        sources: &[gateway_context::ContextFragment],
        target: &gateway_domain::NonEmptyText,
    ) -> Result<Vec<gateway_context::budgeted::CompactedCandidate>, gateway_domain::RetrievalError>;
}
