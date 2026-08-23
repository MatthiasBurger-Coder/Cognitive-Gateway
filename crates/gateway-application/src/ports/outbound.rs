use gateway_domain::{CapabilityId, KnowledgeQuery, RetrievedKnowledge};

use crate::context::ProjectContext;

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
