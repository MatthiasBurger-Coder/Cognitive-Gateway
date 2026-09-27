use gateway_application::graph_retrieval::{
    GraphProjectionPort, GraphTraversalBudget, GraphTraversalRequest,
};
use gateway_application::ports::outbound::KnowledgeRetrievalPort;
use gateway_application::retrieval_pipeline::FusionPolicy;
use gateway_application::retrieval_pipeline::{RetrievalSourceAdapter, federate};
use gateway_daemon::graph_retrieval::{GraphRetrievalAdapter, InMemoryGraphStore};
use gateway_daemon::retrieval::FederatedRetrievalPort;
use gateway_domain::knowledge_graph::*;
use gateway_domain::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
};

fn text(value: &str) -> NonEmptyText {
    NonEmptyText::new(value).unwrap()
}
fn scope() -> ContextScopeId {
    ContextScopeId::new("project").unwrap()
}
fn source() -> SourceId {
    SourceId::new("repo").unwrap()
}
fn digest(value: char) -> ContentDigest {
    ContentDigest::new(value.to_string().repeat(64)).unwrap()
}
fn manifest(value: char) -> GraphSourceManifest {
    [((scope(), source()), digest(value))].into()
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
fn quality(uncertainty: Uncertainty) -> QualityMetadata {
    QualityMetadata::new(
        TrustClass::RetrievedContent,
        SensitivityClass::Public,
        Confidence::Unknown,
        FreshnessStatus::Fresh,
        uncertainty,
    )
}
fn node(id: &str, content: &str) -> GraphNode {
    GraphNode {
        id: GraphNodeId::new(id).unwrap(),
        version: GraphVersion::V1,
        fragment: RetrievedFragment {
            id: ReferenceId::new(id).unwrap(),
            scope: scope(),
            content: text(content),
            provenance: provenance(id),
            snapshot: digest('a'),
            quality: quality(Uncertainty::None),
            evidence: BTreeSet::new(),
        },
    }
}
fn projection() -> GraphProjection {
    GraphProjection::new(
        GraphProjectionId::new("graph").unwrap(),
        GraphVersion::V1,
        manifest('a'),
        vec![node("a", "seed"), node("b", "target")],
        vec![GraphEdge {
            id: GraphEdgeId::new("ab").unwrap(),
            version: GraphVersion::V1,
            from: GraphNodeId::new("a").unwrap(),
            to: GraphNodeId::new("b").unwrap(),
            relation: GraphRelation::DependsOn,
            basis: RelationshipBasis::Inferred,
            scope: scope(),
            provenance: provenance("edge"),
            snapshot: digest('a'),
            quality: quality(Uncertainty::Probabilistic),
        }],
    )
    .unwrap()
}
fn budget() -> GraphTraversalBudget {
    GraphTraversalBudget {
        max_depth: 2,
        max_nodes: 3,
        max_edges: 2,
        max_results: 3,
        max_elapsed_ms: 1000,
        max_cost: 2,
    }
}
fn request() -> RetrievalRequest {
    RetrievalRequest::new(RetrievalRequestInput {
        version: RetrievalVersion::V1,
        scope: scope(),
        provenance: ProvenanceId::new("request").unwrap(),
        situation: None,
        step: None,
        purpose: RetrievalPurpose::TaskKnowledge,
        required: RequiredInformation {
            description: text("graph relationships"),
            requirements: InformationRequirements::new(
                FreshnessRequirement::Fresh,
                None,
                vec![],
                vec![],
            )
            .unwrap(),
            accepted_trust: [TrustClass::RetrievedContent, TrustClass::DerivedAssessment].into(),
            maximum_sensitivity: SensitivityClass::Public,
        },
        queries: [RetrievalQuery(text("seed"))].into(),
        sources: vec![RetrievalSource {
            priority: 0,
            id: RetrievalSourceId::new("graph-source").unwrap(),
            kind: RetrievalSourceKind::Graph,
            optional: true,
        }],
        strategies: vec![RetrievalStrategy {
            priority: 0,
            id: RetrievalStrategyId::new("traversal").unwrap(),
            kind: RetrievalStrategyKind::GraphTraversal,
            optional: false,
        }],
        budget: RetrievalBudget {
            results: ResultBudget(NonZeroU64::new(3).unwrap()),
            rounds: RoundBudget(NonZeroU64::new(1).unwrap()),
            latency: LatencyBudget(NonZeroU64::new(1000).unwrap()),
            cost: CostBudget {
                maximum: 10,
                unit: text("unit"),
            },
            tokens: TokenBudget(1000),
            context: ContextBudget::new(
                TokenBudget(1000),
                BTreeMap::from([(ContextBudgetClass::Knowledge, TokenBudget(1000))]),
            )
            .unwrap(),
        },
        stop: Some(StopCondition::BudgetExhausted),
    })
    .unwrap()
}

#[test]
fn graph_adapter_keeps_path_and_inference_advisory() {
    let store = InMemoryGraphStore::new(manifest('a'));
    store.replace(projection()).unwrap();
    let adapter = GraphRetrievalAdapter::new(
        &store,
        GraphProjectionId::new("graph").unwrap(),
        scope(),
        RetrievalSourceId::new("graph-source").unwrap(),
        RetrievalStrategyId::new("traversal").unwrap(),
        budget(),
    )
    .unwrap();
    let results = adapter.retrieve(&request()).unwrap();
    assert_eq!(results.len(), 2);
    let inferred = results
        .iter()
        .find(|hit| hit.result().fragment.content.as_str() == "target")
        .unwrap();
    assert_eq!(
        inferred.result().fragment.quality.trust(),
        TrustClass::DerivedAssessment
    );
    assert!(inferred.result().fragment.evidence.is_empty());
    assert_eq!(
        inferred.graph_paths[0].steps[0]
            .edge
            .provenance
            .source_reference(),
        "edge"
    );
    assert_eq!(
        inferred.graph_paths[0].steps[0].edge.quality.uncertainty(),
        Uncertainty::Probabilistic
    );
    let mut restricted = request().input().clone();
    restricted
        .required
        .accepted_trust
        .remove(&TrustClass::DerivedAssessment);
    assert_eq!(
        adapter
            .retrieve(&RetrievalRequest::new(restricted).unwrap())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn stale_projection_is_excluded_until_rebuilt_and_outage_is_reported() {
    let store = InMemoryGraphStore::new(manifest('a'));
    store.replace(projection()).unwrap();
    let adapter = GraphRetrievalAdapter::new(
        &store,
        GraphProjectionId::new("graph").unwrap(),
        scope(),
        RetrievalSourceId::new("graph-source").unwrap(),
        RetrievalStrategyId::new("traversal").unwrap(),
        budget(),
    )
    .unwrap();
    store.set_current_sources(manifest('b'));
    assert_eq!(
        adapter.retrieve(&request()),
        Err(RetrievalError::StaleIndex)
    );
    assert_eq!(
        store.replace(projection()),
        Err(GraphError::StaleProjection)
    );
    let input = request();
    let plan = RetrievalPlan::new(
        RetrievalPlanId::new("plan").unwrap(),
        input,
        &RetrievalSupport {
            sources: [(
                RetrievalSourceId::new("graph-source").unwrap(),
                RetrievalSourceKind::Graph,
            )]
            .into(),
            strategies: [(
                RetrievalStrategyId::new("traversal").unwrap(),
                RetrievalStrategyKind::GraphTraversal,
            )]
            .into(),
        },
    )
    .unwrap();
    let federated = federate(&plan, &[&adapter]);
    assert!(federated.candidates.is_empty());
    assert!(
        federated
            .failures
            .iter()
            .any(|failure| failure.2 == RetrievalError::StaleIndex)
    );
    store.set_current_sources(manifest('a'));
    assert_eq!(adapter.retrieve(&request()).unwrap().len(), 2);
    store
        .invalidate(&GraphProjectionId::new("graph").unwrap())
        .unwrap();
    assert_eq!(
        adapter.retrieve(&request()),
        Err(RetrievalError::ServiceUnavailable)
    );
}

#[test]
fn explicit_path_api_enforces_scope_budget_and_current_sources() {
    let store = InMemoryGraphStore::new(manifest('a'));
    store.replace(projection()).unwrap();
    let adapter = GraphRetrievalAdapter::new(
        &store,
        GraphProjectionId::new("graph").unwrap(),
        scope(),
        RetrievalSourceId::new("graph-source").unwrap(),
        RetrievalStrategyId::new("traversal").unwrap(),
        budget(),
    )
    .unwrap();
    let mut graph_request = GraphTraversalRequest {
        scope: scope(),
        permitted_scopes: BTreeSet::new(),
        seeds: [GraphNodeId::new("a").unwrap()].into(),
        accepted_trust: [TrustClass::RetrievedContent].into(),
        maximum_sensitivity: SensitivityClass::Public,
        require_fresh: true,
        budget: budget(),
    };
    let result = adapter.retrieve_paths(&graph_request).unwrap();
    assert_eq!(result.paths.len(), 2);
    assert_eq!(
        result.paths[1].steps[0].edge.provenance.source_reference(),
        "edge"
    );
    graph_request.scope = ContextScopeId::new("other").unwrap();
    assert_eq!(
        adapter.retrieve_paths(&graph_request),
        Err(RetrievalError::ScopeMismatch)
    );
    graph_request.scope = scope();
    graph_request.budget.max_cost += 1;
    assert_eq!(
        adapter.retrieve_paths(&graph_request),
        Err(RetrievalError::BudgetExceeded)
    );
    graph_request.budget = budget();
    store.set_current_sources(manifest('b'));
    assert_eq!(
        adapter.retrieve_paths(&graph_request),
        Err(RetrievalError::StaleIndex)
    );
    store
        .invalidate(&GraphProjectionId::new("graph").unwrap())
        .unwrap();
    assert_eq!(
        adapter.retrieve_paths(&graph_request),
        Err(RetrievalError::ServiceUnavailable)
    );
}

#[test]
fn cg15_adapter_rejects_wrong_selection_and_empty_matches() {
    let store = InMemoryGraphStore::new(manifest('a'));
    store.replace(projection()).unwrap();
    let adapter = GraphRetrievalAdapter::new(
        &store,
        GraphProjectionId::new("graph").unwrap(),
        scope(),
        RetrievalSourceId::new("graph-source").unwrap(),
        RetrievalStrategyId::new("traversal").unwrap(),
        budget(),
    )
    .unwrap();
    let mut input = request().input().clone();
    input.scope = ContextScopeId::new("other").unwrap();
    assert_eq!(
        adapter.retrieve(&RetrievalRequest::new(input).unwrap()),
        Err(RetrievalError::ScopeMismatch)
    );
    let mut input = request().input().clone();
    input.sources[0].kind = RetrievalSourceKind::Document;
    assert_eq!(
        adapter.retrieve(&RetrievalRequest::new(input).unwrap()),
        Err(RetrievalError::UnsupportedSource)
    );
    let mut input = request().input().clone();
    input.strategies[0].kind = RetrievalStrategyKind::Semantic;
    assert_eq!(
        adapter.retrieve(&RetrievalRequest::new(input).unwrap()),
        Err(RetrievalError::UnsupportedStrategy)
    );
    let mut input = request().input().clone();
    input.queries = [RetrievalQuery(text("absent"))].into();
    assert!(
        adapter
            .retrieve(&RetrievalRequest::new(input).unwrap())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        GraphRetrievalAdapter::new(
            &store,
            GraphProjectionId::new("graph").unwrap(),
            scope(),
            RetrievalSourceId::new("graph-source").unwrap(),
            RetrievalStrategyId::new("traversal").unwrap(),
            GraphTraversalBudget {
                max_results: 0,
                ..budget()
            }
        )
        .err(),
        Some(RetrievalError::InvalidBudget)
    );
}

#[test]
fn graph_paths_survive_federation_as_inspectable_batch_explanations() {
    let store = InMemoryGraphStore::new(manifest('a'));
    store.replace(projection()).unwrap();
    let adapter = GraphRetrievalAdapter::new(
        &store,
        GraphProjectionId::new("graph").unwrap(),
        scope(),
        RetrievalSourceId::new("graph-source").unwrap(),
        RetrievalStrategyId::new("traversal").unwrap(),
        budget(),
    )
    .unwrap();
    let request = request();
    let plan = RetrievalPlan::new(
        RetrievalPlanId::new("plan").unwrap(),
        request,
        &RetrievalSupport {
            sources: [(
                RetrievalSourceId::new("graph-source").unwrap(),
                RetrievalSourceKind::Graph,
            )]
            .into(),
            strategies: [(
                RetrievalStrategyId::new("traversal").unwrap(),
                RetrievalStrategyKind::GraphTraversal,
            )]
            .into(),
        },
    )
    .unwrap();
    let port = FederatedRetrievalPort {
        adapters: vec![&adapter],
        fusion: FusionPolicy {
            lexical_weight: 0,
            semantic_weight: 1,
            exact_match_floor: 0,
        },
        reranker: None,
    };
    let batch = port
        .retrieve(
            &plan,
            RetrievalRound(NonZeroU64::new(1).unwrap()),
            &BudgetUsage {
                results: 0,
                rounds: 0,
                elapsed_ms: 0,
                cost: 0,
                cost_unit: text("unit"),
                tokens: 0,
                context: BTreeMap::new(),
            },
        )
        .unwrap();
    assert_eq!(batch.input().results.len(), 2);
    let target = batch
        .input()
        .results
        .iter()
        .find(|result| result.fragment.id.as_str() == "b")
        .unwrap();
    assert_eq!(
        target.fragment.quality.trust(),
        TrustClass::DerivedAssessment
    );
    let detail = batch
        .input()
        .explanations
        .iter()
        .find(|explanation| {
            explanation.target == RetrievalExplanationTarget::Result(ReferenceId::new("b").unwrap())
        })
        .unwrap()
        .detail
        .as_str();
    assert!(detail.contains("graph root a -> b"));
    assert!(detail.contains("edge"));
    assert!(detail.contains("PROBABILISTIC"));
}
