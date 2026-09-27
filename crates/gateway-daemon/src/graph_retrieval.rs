//! Replaceable in-memory graph projection and CG-16 graph source adapter.
use gateway_application::graph_retrieval::{
    GraphProjectionPort, GraphTraversalBudget, GraphTraversalOutcome, GraphTraversalRequest,
    traverse,
};
use gateway_application::retrieval_pipeline::{HybridCandidate, RetrievalSourceAdapter};
use gateway_domain::knowledge_graph::{
    GraphError, GraphNodeId, GraphProjection, GraphProjectionId, GraphSourceManifest,
};
use gateway_domain::{
    ContextScopeId, FreshnessRequirement, QualityMetadata, RetrievalError, RetrievalRequest,
    RetrievalResult, RetrievalSourceId, RetrievalStrategyId, TrustClass, Uncertainty,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

/// A derived read model. Call `set_current_sources` when authoritative source
/// snapshots change, then rebuild or invalidate the stale projection.
pub struct InMemoryGraphStore {
    projections: RefCell<BTreeMap<GraphProjectionId, GraphProjection>>,
    current: RefCell<GraphSourceManifest>,
}
impl InMemoryGraphStore {
    pub fn new(current: GraphSourceManifest) -> Self {
        Self {
            projections: RefCell::new(BTreeMap::new()),
            current: RefCell::new(current),
        }
    }
    pub fn set_current_sources(&self, current: GraphSourceManifest) {
        *self.current.borrow_mut() = current;
    }
}
impl GraphProjectionPort for InMemoryGraphStore {
    fn load(&self, id: &GraphProjectionId) -> Result<Option<GraphProjection>, GraphError> {
        Ok(self.projections.borrow().get(id).cloned())
    }
    fn replace(&self, projection: GraphProjection) -> Result<(), GraphError> {
        projection.ensure_current(&self.current.borrow())?;
        self.projections
            .borrow_mut()
            .insert(projection.id().clone(), projection);
        Ok(())
    }
    fn invalidate(&self, id: &GraphProjectionId) -> Result<(), GraphError> {
        self.projections.borrow_mut().remove(id);
        Ok(())
    }
    fn current_sources(&self) -> Result<GraphSourceManifest, GraphError> {
        Ok(self.current.borrow().clone())
    }
}

fn retrieval_error(error: GraphError) -> RetrievalError {
    match error {
        GraphError::StaleProjection => RetrievalError::StaleIndex,
        GraphError::ScopeMismatch => RetrievalError::ScopeMismatch,
        GraphError::InvalidBudget => RetrievalError::InvalidBudget,
        _ => RetrievalError::InvalidResult,
    }
}

pub struct GraphRetrievalAdapter<'a> {
    store: &'a dyn GraphProjectionPort,
    projection: GraphProjectionId,
    scope: ContextScopeId,
    source: RetrievalSourceId,
    strategy: RetrievalStrategyId,
    budget: GraphTraversalBudget,
}
impl<'a> GraphRetrievalAdapter<'a> {
    pub fn new(
        store: &'a dyn GraphProjectionPort,
        projection: GraphProjectionId,
        scope: ContextScopeId,
        source: RetrievalSourceId,
        strategy: RetrievalStrategyId,
        budget: GraphTraversalBudget,
    ) -> Result<Self, RetrievalError> {
        budget.validate().map_err(retrieval_error)?;
        Ok(Self {
            store,
            projection,
            scope,
            source,
            strategy,
            budget,
        })
    }

    /// Explicit graph use case. Additional scopes must be named in the request;
    /// the CG-15 adapter below always uses an empty additional-scope set.
    pub fn retrieve_paths(
        &self,
        request: &GraphTraversalRequest,
    ) -> Result<GraphTraversalOutcome, RetrievalError> {
        if request.scope != self.scope {
            return Err(RetrievalError::ScopeMismatch);
        }
        if request.budget.max_depth > self.budget.max_depth
            || request.budget.max_nodes > self.budget.max_nodes
            || request.budget.max_edges > self.budget.max_edges
            || request.budget.max_results > self.budget.max_results
            || request.budget.max_elapsed_ms > self.budget.max_elapsed_ms
            || request.budget.max_cost > self.budget.max_cost
        {
            return Err(RetrievalError::BudgetExceeded);
        }
        let projection = self
            .store
            .load(&self.projection)
            .map_err(retrieval_error)?
            .ok_or(RetrievalError::ServiceUnavailable)?;
        let current = self.store.current_sources().map_err(retrieval_error)?;
        projection
            .ensure_current(&current)
            .map_err(retrieval_error)?;
        let started = Instant::now();
        traverse(&projection, request, || {
            started.elapsed().as_millis() as u64
        })
        .map_err(retrieval_error)
    }
}

impl RetrievalSourceAdapter for GraphRetrievalAdapter<'_> {
    fn source(&self) -> &RetrievalSourceId {
        &self.source
    }
    fn strategy(&self) -> &RetrievalStrategyId {
        &self.strategy
    }
    fn retrieve(&self, request: &RetrievalRequest) -> Result<Vec<HybridCandidate>, RetrievalError> {
        let input = request.input();
        if input.scope != self.scope {
            return Err(RetrievalError::ScopeMismatch);
        }
        if !input.sources.iter().any(|source| {
            source.id == self.source && source.kind == gateway_domain::RetrievalSourceKind::Graph
        }) {
            return Err(RetrievalError::UnsupportedSource);
        }
        if !input.strategies.iter().any(|strategy| {
            strategy.id == self.strategy
                && strategy.kind == gateway_domain::RetrievalStrategyKind::GraphTraversal
        }) {
            return Err(RetrievalError::UnsupportedStrategy);
        }
        let projection = self
            .store
            .load(&self.projection)
            .map_err(retrieval_error)?
            .ok_or(RetrievalError::ServiceUnavailable)?;
        projection
            .ensure_current(&self.store.current_sources().map_err(retrieval_error)?)
            .map_err(retrieval_error)?;
        let queries: Vec<_> = input
            .queries
            .iter()
            .map(|query| query.0.as_str().trim().to_lowercase())
            .collect();
        let seeds: BTreeSet<GraphNodeId> = projection
            .nodes()
            .values()
            .filter(|node| node.fragment.scope == self.scope)
            .filter(|node| {
                queries.iter().any(|query| {
                    node.id.as_str().to_lowercase().contains(query)
                        || node
                            .fragment
                            .content
                            .as_str()
                            .to_lowercase()
                            .contains(query)
                })
            })
            .map(|node| node.id.clone())
            .collect();
        if seeds.is_empty() {
            return Ok(Vec::new());
        }
        let graph_request = GraphTraversalRequest {
            scope: self.scope.clone(),
            permitted_scopes: BTreeSet::new(),
            seeds,
            accepted_trust: input.required.accepted_trust.clone(),
            maximum_sensitivity: input.required.maximum_sensitivity,
            require_fresh: input.required.requirements.freshness() == FreshnessRequirement::Fresh,
            budget: self.budget,
        };
        let started = Instant::now();
        let outcome = traverse(&projection, &graph_request, || {
            started.elapsed().as_millis() as u64
        })
        .map_err(retrieval_error)?;
        let mut candidates = Vec::new();
        for path in outcome.paths {
            let mut fragment = path.terminal().fragment.clone();
            if path.is_inferred() {
                // Inference does not establish evidence or canonical trust.
                fragment.evidence.clear();
                fragment.quality = QualityMetadata::new(
                    TrustClass::DerivedAssessment,
                    fragment.quality.sensitivity(),
                    fragment.quality.confidence(),
                    fragment.quality.freshness(),
                    Uncertainty::Probabilistic,
                );
                if !input
                    .required
                    .accepted_trust
                    .contains(&TrustClass::DerivedAssessment)
                {
                    continue;
                }
            }
            let score = (1_000_000u32 / (path.steps.len() as u32 + 1)).max(1);
            let result = RetrievalResult {
                fragment,
                source: self.source.clone(),
                strategy: self.strategy.clone(),
                score: i64::from(score),
            };
            let mut candidate = HybridCandidate::new(result, None, false, None, Some(score))?;
            candidate.graph_paths.push(path);
            candidates.push(candidate);
        }
        Ok(candidates)
    }
}
