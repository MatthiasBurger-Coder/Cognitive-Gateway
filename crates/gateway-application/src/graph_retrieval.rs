//! Deterministic advisory graph traversal over a validated derived projection.
use gateway_domain::knowledge_graph::{
    GraphEdge, GraphError, GraphNode, GraphNodeId, GraphProjection, GraphProjectionId,
    GraphSourceManifest,
};
use gateway_domain::{ContextScopeId, FreshnessStatus, SensitivityClass, TrustClass};
use std::collections::{BTreeSet, VecDeque};

/// Replaceable storage for derived projections. The caller, not this store,
/// establishes which source snapshots are current.
pub trait GraphProjectionPort {
    fn load(&self, id: &GraphProjectionId) -> Result<Option<GraphProjection>, GraphError>;
    fn replace(&self, projection: GraphProjection) -> Result<(), GraphError>;
    fn invalidate(&self, id: &GraphProjectionId) -> Result<(), GraphError>;
    fn current_sources(&self) -> Result<GraphSourceManifest, GraphError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphTraversalBudget {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_results: usize,
    pub max_elapsed_ms: u64,
    /// Unit cost per traversed edge. Zero permits seed-only results.
    pub max_cost: u64,
}
impl GraphTraversalBudget {
    pub fn validate(self) -> Result<Self, GraphError> {
        if self.max_nodes == 0 || self.max_results == 0 || self.max_elapsed_ms == 0 {
            return Err(GraphError::InvalidBudget);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphTraversalRequest {
    pub scope: ContextScopeId,
    /// Explicit additional scopes. The request scope is always allowed.
    pub permitted_scopes: BTreeSet<ContextScopeId>,
    pub seeds: BTreeSet<GraphNodeId>,
    pub accepted_trust: BTreeSet<TrustClass>,
    pub maximum_sensitivity: SensitivityClass,
    pub require_fresh: bool,
    pub budget: GraphTraversalBudget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPathStep {
    pub edge: GraphEdge,
    pub node: GraphNode,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPath {
    pub root: GraphNode,
    pub steps: Vec<GraphPathStep>,
}
impl GraphPath {
    pub fn terminal(&self) -> &GraphNode {
        self.steps.last().map_or(&self.root, |step| &step.node)
    }
    pub fn is_inferred(&self) -> bool {
        self.steps.iter().any(|step| {
            step.edge.basis == gateway_domain::knowledge_graph::RelationshipBasis::Inferred
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphStopReason {
    Complete,
    DepthLimit,
    NodeLimit,
    EdgeLimit,
    ResultLimit,
    TimeLimit,
    CostLimit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphTraversalOutcome {
    pub paths: Vec<GraphPath>,
    pub visited_nodes: usize,
    pub visited_edges: usize,
    pub cost: u64,
    pub reason: GraphStopReason,
}

fn eligible(
    scope: &ContextScopeId,
    quality: gateway_domain::QualityMetadata,
    request: &GraphTraversalRequest,
) -> bool {
    (scope == &request.scope || request.permitted_scopes.contains(scope))
        && request.accepted_trust.contains(&quality.trust())
        && quality.sensitivity() <= request.maximum_sensitivity
        && (!request.require_fresh || quality.freshness() == FreshnessStatus::Fresh)
}

/// `elapsed_ms` is a monotonic clock supplied by the runtime. A fixed clock
/// stream and fixed projection produce identical paths and stop reasons.
pub fn traverse(
    projection: &GraphProjection,
    request: &GraphTraversalRequest,
    mut elapsed_ms: impl FnMut() -> u64,
) -> Result<GraphTraversalOutcome, GraphError> {
    request.budget.validate()?;
    if request.seeds.is_empty() || request.permitted_scopes.contains(&request.scope) {
        return Err(GraphError::InvalidProjection);
    }
    let mut queue = VecDeque::new();
    let mut visited = BTreeSet::new();
    for id in &request.seeds {
        let node = projection
            .nodes()
            .get(id)
            .ok_or(GraphError::DanglingReference)?;
        if node.fragment.scope != request.scope {
            return Err(GraphError::ScopeMismatch);
        }
        if eligible(&node.fragment.scope, node.fragment.quality, request) {
            queue.push_back(GraphPath {
                root: node.clone(),
                steps: Vec::new(),
            });
            visited.insert(id.clone());
        }
    }
    let mut output = GraphTraversalOutcome {
        paths: Vec::new(),
        visited_nodes: 0,
        visited_edges: 0,
        cost: 0,
        reason: GraphStopReason::Complete,
    };
    while let Some(path) = queue.pop_front() {
        if elapsed_ms() >= request.budget.max_elapsed_ms {
            output.reason = GraphStopReason::TimeLimit;
            break;
        }
        if output.visited_nodes >= request.budget.max_nodes {
            output.reason = GraphStopReason::NodeLimit;
            break;
        }
        output.visited_nodes += 1;
        let terminal = path.terminal().id.clone();
        let depth = path.steps.len();
        output.paths.push(path.clone());
        if output.paths.len() >= request.budget.max_results {
            output.reason = GraphStopReason::ResultLimit;
            break;
        }
        for edge in projection
            .edges()
            .values()
            .filter(|edge| edge.from == terminal)
        {
            if visited.contains(&edge.to) {
                continue;
            }
            let node = projection
                .nodes()
                .get(&edge.to)
                .ok_or(GraphError::DanglingReference)?;
            if !eligible(&edge.scope, edge.quality, request)
                || !eligible(&node.fragment.scope, node.fragment.quality, request)
            {
                continue;
            }
            if depth >= request.budget.max_depth {
                output.reason = GraphStopReason::DepthLimit;
                continue;
            }
            if elapsed_ms() >= request.budget.max_elapsed_ms {
                output.reason = GraphStopReason::TimeLimit;
                return Ok(output);
            }
            if output.visited_edges >= request.budget.max_edges {
                output.reason = GraphStopReason::EdgeLimit;
                return Ok(output);
            }
            if output.cost >= request.budget.max_cost {
                output.reason = GraphStopReason::CostLimit;
                return Ok(output);
            }
            output.visited_edges += 1;
            output.cost += 1;
            visited.insert(edge.to.clone());
            let mut next = path.clone();
            next.steps.push(GraphPathStep {
                edge: edge.clone(),
                node: node.clone(),
            });
            queue.push_back(next);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_domain::knowledge_graph::{
        GraphEdgeId, GraphRelation, GraphVersion, RelationshipBasis,
    };
    use gateway_domain::{
        Confidence, ContentDigest, NonEmptyText, Provenance, ProvenanceId, ReferenceId,
        RetrievedFragment, SourceId, SourceKind, Uncertainty,
    };

    fn scope(value: &str) -> ContextScopeId {
        ContextScopeId::new(value).unwrap()
    }
    fn source() -> SourceId {
        SourceId::new("source").unwrap()
    }
    fn digest() -> ContentDigest {
        ContentDigest::new("a".repeat(64)).unwrap()
    }
    fn provenance(value: &str) -> Provenance {
        Provenance::new(
            ProvenanceId::new(value).unwrap(),
            SourceKind::Repository,
            source(),
            value,
        )
        .unwrap()
    }
    fn quality(uncertainty: Uncertainty) -> gateway_domain::QualityMetadata {
        gateway_domain::QualityMetadata::new(
            TrustClass::RetrievedContent,
            SensitivityClass::Normal,
            Confidence::Unknown,
            FreshnessStatus::Fresh,
            uncertainty,
        )
    }
    fn node(id: &str, project: &str) -> GraphNode {
        GraphNode {
            id: GraphNodeId::new(id).unwrap(),
            version: GraphVersion::V1,
            fragment: RetrievedFragment {
                id: ReferenceId::new(id).unwrap(),
                scope: scope(project),
                content: NonEmptyText::new(id).unwrap(),
                provenance: provenance(id),
                snapshot: digest(),
                quality: quality(Uncertainty::None),
                evidence: BTreeSet::new(),
            },
        }
    }
    fn edge(id: &str, from: &str, to: &str, project: &str) -> GraphEdge {
        GraphEdge {
            id: GraphEdgeId::new(id).unwrap(),
            version: GraphVersion::V1,
            from: GraphNodeId::new(from).unwrap(),
            to: GraphNodeId::new(to).unwrap(),
            relation: GraphRelation::References,
            basis: RelationshipBasis::Observed,
            scope: scope(project),
            provenance: provenance(id),
            snapshot: digest(),
            quality: quality(Uncertainty::None),
        }
    }
    fn projection(reverse: bool) -> GraphProjection {
        let mut nodes = vec![node("a", "one"), node("b", "one"), node("c", "one")];
        let mut edges = vec![
            edge("ab", "a", "b", "one"),
            edge("ba", "b", "a", "one"),
            edge("ac", "a", "c", "one"),
        ];
        if reverse {
            nodes.reverse();
            edges.reverse();
        }
        GraphProjection::new(
            GraphProjectionId::new("graph").unwrap(),
            GraphVersion::V1,
            [((scope("one"), source()), digest())].into(),
            nodes,
            edges,
        )
        .unwrap()
    }
    fn budget() -> GraphTraversalBudget {
        GraphTraversalBudget {
            max_depth: 3,
            max_nodes: 10,
            max_edges: 10,
            max_results: 10,
            max_elapsed_ms: 100,
            max_cost: 10,
        }
    }
    fn request() -> GraphTraversalRequest {
        GraphTraversalRequest {
            scope: scope("one"),
            permitted_scopes: BTreeSet::new(),
            seeds: [GraphNodeId::new("a").unwrap()].into(),
            accepted_trust: [TrustClass::RetrievedContent].into(),
            maximum_sensitivity: SensitivityClass::Normal,
            require_fresh: true,
            budget: budget(),
        }
    }
    #[test]
    fn cycle_terminates_with_stable_paths_and_lineage() {
        let first = traverse(&projection(false), &request(), || 0).unwrap();
        let second = traverse(&projection(true), &request(), || 0).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.reason, GraphStopReason::Complete);
        assert_eq!(first.paths.len(), 3);
        assert_eq!(
            first.paths[1].steps[0].edge.provenance.source_reference(),
            "ab"
        );
        assert_eq!(first.paths[1].terminal().id.as_str(), "b");
        assert_eq!(first.visited_edges, 2);
        assert!(!first.paths[1].is_inferred());
    }
    #[test]
    fn each_budget_has_a_stable_stop_reason() {
        let cases = [
            (
                GraphTraversalBudget {
                    max_depth: 0,
                    ..budget()
                },
                GraphStopReason::DepthLimit,
            ),
            (
                GraphTraversalBudget {
                    max_nodes: 1,
                    ..budget()
                },
                GraphStopReason::NodeLimit,
            ),
            (
                GraphTraversalBudget {
                    max_edges: 0,
                    ..budget()
                },
                GraphStopReason::EdgeLimit,
            ),
            (
                GraphTraversalBudget {
                    max_results: 1,
                    ..budget()
                },
                GraphStopReason::ResultLimit,
            ),
            (
                GraphTraversalBudget {
                    max_cost: 0,
                    ..budget()
                },
                GraphStopReason::CostLimit,
            ),
        ];
        for (limit, reason) in cases {
            let mut request = request();
            request.budget = limit;
            assert_eq!(
                traverse(&projection(false), &request, || 0).unwrap().reason,
                reason
            );
        }
        let mut timed = request();
        timed.budget.max_elapsed_ms = 1;
        assert_eq!(
            traverse(&projection(false), &timed, || 1).unwrap().reason,
            GraphStopReason::TimeLimit
        );
        assert_eq!(
            GraphTraversalBudget {
                max_nodes: 0,
                ..budget()
            }
            .validate(),
            Err(GraphError::InvalidBudget)
        );
    }
    #[test]
    fn cross_scope_is_explicit_and_retains_original_scope() {
        let graph = GraphProjection::new(
            GraphProjectionId::new("cross").unwrap(),
            GraphVersion::V1,
            [
                ((scope("one"), source()), digest()),
                ((scope("two"), source()), digest()),
            ]
            .into(),
            vec![node("a", "one"), node("b", "two")],
            vec![edge("ab", "a", "b", "one")],
        )
        .unwrap();
        let request = request();
        assert_eq!(traverse(&graph, &request, || 0).unwrap().paths.len(), 1);
        let mut permitted = request.clone();
        permitted.permitted_scopes.insert(scope("two"));
        let paths = traverse(&graph, &permitted, || 0).unwrap().paths;
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[1].terminal().fragment.scope, scope("two"));
        assert_eq!(paths[1].steps[0].edge.scope, scope("one"));
        let mut bad_seed = request;
        bad_seed.seeds = [GraphNodeId::new("b").unwrap()].into();
        assert_eq!(
            traverse(&graph, &bad_seed, || 0),
            Err(GraphError::ScopeMismatch)
        );
    }
}
