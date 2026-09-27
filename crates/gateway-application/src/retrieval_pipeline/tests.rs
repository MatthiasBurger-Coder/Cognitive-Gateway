use super::*;
use crate::graph_retrieval::GraphPath;
use gateway_domain::knowledge_graph::{GraphNode, GraphNodeId, GraphVersion};
use gateway_domain::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
};

fn text(value: &str) -> NonEmptyText {
    NonEmptyText::new(value).unwrap()
}
fn candidate(
    id: &str,
    content: &str,
    source: &str,
    exact: bool,
    lexical: Option<u32>,
    semantic: Option<u32>,
) -> HybridCandidate {
    let fragment = RetrievedFragment {
        id: ReferenceId::new(id).unwrap(),
        scope: ContextScopeId::new("scope-a").unwrap(),
        content: text(content),
        provenance: Provenance::new(
            ProvenanceId::new("origin").unwrap(),
            SourceKind::Repository,
            SourceId::new(source).unwrap(),
            "docs/adr.md",
        )
        .unwrap(),
        snapshot: ContentDigest::new("a".repeat(64)).unwrap(),
        quality: QualityMetadata::new(
            TrustClass::RetrievedContent,
            SensitivityClass::Public,
            Confidence::Unknown,
            FreshnessStatus::Fresh,
            Uncertainty::None,
        ),
        evidence: BTreeSet::new(),
    };
    HybridCandidate::new(
        RetrievalResult {
            fragment,
            source: RetrievalSourceId::new(source).unwrap(),
            strategy: RetrievalStrategyId::new("lexical").unwrap(),
            score: 1,
        },
        None,
        exact,
        lexical,
        semantic,
    )
    .unwrap()
}

#[test]
fn exact_identifier_hit_survives_semantic_score_and_duplicate_merge() {
    let policy = FusionPolicy {
        lexical_weight: 1,
        semantic_weight: 1_000_000,
        exact_match_floor: 900_000,
    };
    let exact = candidate(
        "exact",
        " THE ADR   DEFINES RETRIEVAL ",
        "repo",
        true,
        Some(100_000),
        None,
    );
    let mut semantic = candidate(
        "semantic",
        "The ADR defines retrieval",
        "vector",
        false,
        None,
        Some(1_000_000),
    );
    semantic.result.fragment.provenance = exact.result.fragment.provenance.clone();
    let output = fuse_candidates([semantic, exact], policy).unwrap();
    assert_eq!(output.len(), 1);
    assert!(output[0].exact_match);
    assert!(output[0].result.score >= 900_000);
    // Duplicate collapse retains one complete source fragment and its lineage.
    assert!(
        output[0]
            .result
            .fragment
            .provenance
            .source_reference()
            .contains("docs/adr.md")
    );
}

#[test]
fn duplicate_fusion_retains_every_inspectable_graph_path() {
    let policy = FusionPolicy {
        lexical_weight: 1,
        semantic_weight: 1,
        exact_match_floor: 900_000,
    };
    let mut first = candidate("one", "same fact", "repo", false, Some(100), None);
    let mut second = candidate("two", "SAME FACT", "repo", true, Some(200), None);
    let mut third = candidate("three", "same fact", "repo", false, Some(50), None);
    for (id, hit) in [
        ("one", &mut first),
        ("two", &mut second),
        ("three", &mut third),
    ] {
        hit.graph_paths.push(GraphPath {
            root: GraphNode {
                id: GraphNodeId::new(id).unwrap(),
                version: GraphVersion::V1,
                fragment: hit.result.fragment.clone(),
            },
            steps: Vec::new(),
        });
    }
    let output = fuse_candidates([first, second, third], policy).unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].result().fragment.id.as_str(), "two");
    assert_eq!(output[0].graph_paths.len(), 3);
    assert_eq!(
        output[0]
            .graph_paths
            .iter()
            .map(|path| path.root.id.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "three", "two"]
    );
}

#[test]
fn invalid_fusion_policy_fails_closed() {
    assert_eq!(
        FusionPolicy {
            lexical_weight: 0,
            semantic_weight: 0,
            exact_match_floor: 0
        }
        .validate(),
        Err(RetrievalError::InvalidPlan)
    );
    let valid = candidate("x", "text", "repo", false, Some(1), None);
    assert!(
        HybridCandidate::new(valid.result.clone(), None, false, Some(1_000_001), None).is_err()
    );
}

fn n(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}
fn plan() -> RetrievalPlan {
    let input = RetrievalRequestInput {
        version: RetrievalVersion::V1,
        scope: ContextScopeId::new("scope-a").unwrap(),
        provenance: ProvenanceId::new("request").unwrap(),
        situation: None,
        step: None,
        purpose: RetrievalPurpose::ArchitectureEvidence,
        required: RequiredInformation {
            description: text("knowledge"),
            requirements: InformationRequirements::new(
                FreshnessRequirement::Fresh,
                None,
                vec![],
                vec![],
            )
            .unwrap(),
            accepted_trust: BTreeSet::from([TrustClass::RetrievedContent]),
            maximum_sensitivity: SensitivityClass::Public,
        },
        queries: BTreeSet::from([RetrievalQuery(text("ADR"))]),
        sources: vec![
            RetrievalSource {
                priority: 0,
                id: RetrievalSourceId::new("repo").unwrap(),
                kind: RetrievalSourceKind::Document,
                optional: false,
            },
            RetrievalSource {
                priority: 1,
                id: RetrievalSourceId::new("vector").unwrap(),
                kind: RetrievalSourceKind::VectorIndex,
                optional: true,
            },
        ],
        strategies: vec![
            RetrievalStrategy {
                priority: 0,
                id: RetrievalStrategyId::new("lexical").unwrap(),
                kind: RetrievalStrategyKind::Lexical,
                optional: false,
            },
            RetrievalStrategy {
                priority: 1,
                id: RetrievalStrategyId::new("semantic").unwrap(),
                kind: RetrievalStrategyKind::Semantic,
                optional: true,
            },
        ],
        budget: RetrievalBudget {
            results: ResultBudget(n(4)),
            rounds: RoundBudget(n(1)),
            latency: LatencyBudget(n(100)),
            cost: CostBudget {
                maximum: 1,
                unit: text("unit"),
            },
            tokens: TokenBudget(100),
            context: ContextBudget::new(
                TokenBudget(100),
                BTreeMap::from([(ContextBudgetClass::Knowledge, TokenBudget(100))]),
            )
            .unwrap(),
        },
        stop: Some(StopCondition::BudgetExhausted),
    };
    let support = RetrievalSupport {
        sources: input
            .sources
            .iter()
            .map(|source| (source.id.clone(), source.kind))
            .collect(),
        strategies: input
            .strategies
            .iter()
            .map(|strategy| (strategy.id.clone(), strategy.kind))
            .collect(),
    };
    RetrievalPlan::new(
        RetrievalPlanId::new("plan").unwrap(),
        RetrievalRequest::new(input).unwrap(),
        &support,
    )
    .unwrap()
}

struct Adapter {
    source: RetrievalSourceId,
    strategy: RetrievalStrategyId,
    reply: Result<Vec<HybridCandidate>, RetrievalError>,
}
impl RetrievalSourceAdapter for Adapter {
    fn source(&self) -> &RetrievalSourceId {
        &self.source
    }
    fn strategy(&self) -> &RetrievalStrategyId {
        &self.strategy
    }
    fn retrieve(&self, _: &RetrievalRequest) -> Result<Vec<HybridCandidate>, RetrievalError> {
        self.reply.clone()
    }
}
fn adapter(
    source: &str,
    strategy: &str,
    reply: Result<Vec<HybridCandidate>, RetrievalError>,
) -> Adapter {
    Adapter {
        source: RetrievalSourceId::new(source).unwrap(),
        strategy: RetrievalStrategyId::new(strategy).unwrap(),
        reply,
    }
}

#[test]
fn federation_reports_partial_failures_and_rejects_mislabelled_results() {
    let valid = adapter(
        "repo",
        "lexical",
        Ok(vec![candidate(
            "a",
            "ADR",
            "repo",
            true,
            Some(900_000),
            None,
        )]),
    );
    let outage = adapter("vector", "semantic", Err(RetrievalError::StaleIndex));
    let output = federate(&plan(), &[&valid, &outage]);
    assert_eq!(output.candidates.len(), 1);
    assert!(output.failures.contains(&(
        outage.source.clone(),
        outage.strategy.clone(),
        RetrievalError::StaleIndex
    )));

    let wrong_scope = {
        let mut item = candidate("b", "other", "vector", false, None, Some(50));
        item.result.fragment.scope = ContextScopeId::new("scope-b").unwrap();
        item.result.strategy = RetrievalStrategyId::new("semantic").unwrap();
        adapter("vector", "semantic", Ok(vec![item]))
    };
    let output = federate(&plan(), &[&valid, &wrong_scope]);
    assert_eq!(output.candidates.len(), 1);
    assert!(
        output
            .failures
            .iter()
            .any(|entry| entry.2 == RetrievalError::InvalidResult)
    );

    let duplicate = federate(&plan(), &[&valid, &valid, &outage]);
    assert!(
        duplicate
            .failures
            .iter()
            .any(|entry| entry.2 == RetrievalError::DuplicateIdentity)
    );
    let missing = federate(&plan(), &[&valid]);
    assert!(
        missing
            .failures
            .iter()
            .any(|entry| entry.2 == RetrievalError::ServiceUnavailable)
    );
    let unselected = adapter("unused", "lexical", Ok(Vec::new()));
    assert_eq!(
        federate(&plan(), &[&valid, &outage, &unselected])
            .candidates
            .len(),
        1
    );
}

struct Model {
    id: &'static str,
    version: &'static str,
    answer: Result<Vec<(ReferenceId, u32)>, RetrievalError>,
}
impl RetrievalReranker for Model {
    fn model(&self) -> &str {
        self.id
    }
    fn version(&self) -> &str {
        self.version
    }
    fn rank(&self, _: &[HybridCandidate]) -> Result<Vec<(ReferenceId, u32)>, RetrievalError> {
        self.answer.clone()
    }
}

#[test]
fn reranker_keeps_scores_and_model_lineage_or_explicit_fallback() {
    let items = vec![
        candidate("a", "ADR", "repo", true, Some(900_000), None),
        candidate("b", "Guide", "vector", false, None, Some(700_000)),
    ];
    let model = Model {
        id: "model",
        version: "v1",
        answer: Ok(vec![
            (ReferenceId::new("b").unwrap(), 1_000_000),
            (ReferenceId::new("a").unwrap(), 1),
        ]),
    };
    let outcome = rerank(items.clone(), &model);
    assert_eq!(outcome.ranked[0].candidate.result.fragment.id.as_str(), "a");
    assert_eq!(
        outcome.ranked[0].score_origin,
        ScoreOrigin::Model {
            id: text("model"),
            version: text("v1")
        }
    );
    assert_eq!(outcome.ranked[1].score, 1_000_000);
    assert_eq!(outcome.failure, None);
    for bad in [
        Model {
            id: "",
            version: "v1",
            answer: model.answer.clone(),
        },
        Model {
            id: "model",
            version: "v1",
            answer: Err(RetrievalError::ServiceUnavailable),
        },
        Model {
            id: "model",
            version: "v1",
            answer: Ok(vec![]),
        },
        Model {
            id: "model",
            version: "v1",
            answer: Ok(vec![
                (ReferenceId::new("a").unwrap(), 2_000_000),
                (ReferenceId::new("b").unwrap(), 1),
            ]),
        },
        Model {
            id: "model",
            version: "v1",
            answer: Ok(vec![
                (ReferenceId::new("a").unwrap(), 1),
                (ReferenceId::new("a").unwrap(), 2),
            ]),
        },
    ] {
        let outcome = rerank(items.clone(), &bad);
        assert_eq!(outcome.failure, Some(RetrievalError::ServiceUnavailable));
        assert_eq!(outcome.ranked[0].score_origin, ScoreOrigin::Fusion);
    }
}

#[test]
fn fusion_uses_stable_ties_and_never_merges_across_scopes() {
    let policy = FusionPolicy {
        lexical_weight: 1,
        semantic_weight: 0,
        exact_match_floor: 0,
    };
    let a = candidate("a", "Same text", "repo", true, Some(100), None);
    let mut b = candidate("b", "same  TEXT", "vector", true, Some(100), None);
    b.result.fragment.provenance = a.result.fragment.provenance.clone();
    let first = fuse_candidates([b.clone(), a.clone()], policy).unwrap();
    let second = fuse_candidates([a, b], policy).unwrap();
    assert_eq!(first, second);
    assert_eq!(first[0].result.source.as_str(), "repo");
    assert_eq!(first[0].contributors().len(), 1);
    let mut other_scope = candidate("c", "Same text", "repo", true, Some(100), None);
    other_scope.result.fragment.scope = ContextScopeId::new("scope-b").unwrap();
    assert_eq!(
        fuse_candidates([first[0].clone(), other_scope], policy)
            .unwrap()
            .len(),
        2
    );
    let distinct_lineage = candidate("d", "Same text", "external", true, Some(100), None);
    assert_eq!(
        fuse_candidates([first[0].clone(), distinct_lineage], policy)
            .unwrap()
            .len(),
        2
    );
}
