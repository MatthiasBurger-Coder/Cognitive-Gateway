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
