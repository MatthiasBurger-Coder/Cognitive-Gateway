use gateway_application::ports::outbound::{EmbeddingPort, KnowledgeRetrievalPort};
use gateway_application::retrieval_pipeline::{
    FusionPolicy, HybridCandidate, RetrievalReranker, RetrievalSourceAdapter,
};
use gateway_daemon::retrieval::{
    FederatedRetrievalPort, RepositoryLexicalAdapter, VectorIndexAdapter,
};
use gateway_domain::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    num::NonZeroU64,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn text(value: &str) -> NonEmptyText {
    NonEmptyText::new(value).unwrap()
}
fn n(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}
fn scope() -> ContextScopeId {
    ContextScopeId::new("project-a").unwrap()
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cg16-{}-{nanos}", std::process::id()));
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("docs/ADR-011.md"),
            "# Retrieval\nADR-011 keeps exact identifiers first.\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/other.md"),
            "# Runtime\nA different topic.\n",
        )
        .unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn plan(query: &str, semantic: bool) -> RetrievalPlan {
    let mut sources = vec![RetrievalSource {
        priority: 0,
        id: RetrievalSourceId::new("repo").unwrap(),
        kind: RetrievalSourceKind::Document,
        optional: false,
    }];
    let mut strategies = vec![RetrievalStrategy {
        priority: 0,
        id: RetrievalStrategyId::new("lexical").unwrap(),
        kind: RetrievalStrategyKind::Lexical,
        optional: false,
    }];
    if semantic {
        sources.push(RetrievalSource {
            priority: 1,
            id: RetrievalSourceId::new("vector").unwrap(),
            kind: RetrievalSourceKind::VectorIndex,
            optional: true,
        });
        strategies.push(RetrievalStrategy {
            priority: 1,
            id: RetrievalStrategyId::new("semantic").unwrap(),
            kind: RetrievalStrategyKind::Semantic,
            optional: true,
        });
    }
    let input = RetrievalRequestInput {
        version: RetrievalVersion::V1,
        scope: scope(),
        provenance: ProvenanceId::new("request").unwrap(),
        situation: None,
        step: None,
        purpose: RetrievalPurpose::ArchitectureEvidence,
        required: RequiredInformation {
            description: text("Find architecture rules"),
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
        queries: BTreeSet::from([RetrievalQuery(text(query))]),
        sources,
        strategies,
        budget: RetrievalBudget {
            results: ResultBudget(n(4)),
            rounds: RoundBudget(n(1)),
            latency: LatencyBudget(n(10_000)),
            cost: CostBudget {
                maximum: 1,
                unit: text("unit"),
            },
            tokens: TokenBudget(100_000),
            context: ContextBudget::new(
                TokenBudget(100_000),
                BTreeMap::from([(ContextBudgetClass::Knowledge, TokenBudget(100_000))]),
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

fn usage() -> BudgetUsage {
    BudgetUsage {
        results: 0,
        rounds: 0,
        elapsed_ms: 0,
        cost: 0,
        cost_unit: text("unit"),
        tokens: 0,
        context: BTreeMap::new(),
    }
}

fn repository(root: &PathBuf) -> RepositoryLexicalAdapter {
    RepositoryLexicalAdapter::new(
        scope(),
        RetrievalSourceId::new("repo").unwrap(),
        RetrievalStrategyId::new("lexical").unwrap(),
        root,
        SensitivityClass::Public,
    )
    .unwrap()
}

struct TwoDimensionalEmbedding(EmbeddingModel);
impl EmbeddingPort for TwoDimensionalEmbedding {
    fn embed(&self, request: &EmbeddingRequest) -> Result<EmbeddingResult, RetrievalError> {
        let vectors = request
            .fragments
            .iter()
            .map(|fragment| {
                let values = if fragment
                    .content
                    .as_str()
                    .to_lowercase()
                    .contains("retrieval")
                    || fragment.content.as_str().contains("ADR-011")
                {
                    vec![1.0, 0.0]
                } else {
                    vec![0.5, 0.5]
                };
                EmbeddingVector {
                    fragment: fragment.id.clone(),
                    values,
                }
            })
            .collect();
        EmbeddingResult::new(request, self.0.clone(), vectors)
    }
}
fn model() -> EmbeddingModel {
    EmbeddingModel {
        id: EmbeddingModelId::new("fixture-model").unwrap(),
        version: EmbeddingModelVersion::new("v1").unwrap(),
        digest: None,
        dimensions: n(2),
    }
}
fn fusion() -> FusionPolicy {
    FusionPolicy {
        lexical_weight: 500_000,
        semantic_weight: 500_000,
        exact_match_floor: 900_000,
    }
}

#[test]
fn repository_git_result_reaches_validated_retrieval_batch() {
    let fixture = Fixture::new();
    assert!(
        Command::new("git")
            .args(["-C", fixture.0.to_str().unwrap(), "init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args(["-C", fixture.0.to_str().unwrap(), "add", "."])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args([
                "-C",
                fixture.0.to_str().unwrap(),
                "-c",
                "user.name=CG Test",
                "-c",
                "user.email=cg@example.test",
                "commit",
                "-qm",
                "fixture"
            ])
            .status()
            .unwrap()
            .success()
    );
    let repo = repository(&fixture.0);
    let port = FederatedRetrievalPort {
        adapters: vec![&repo],
        fusion: fusion(),
        reranker: None,
    };
    let batch = port
        .retrieve(&plan("ADR-011", false), RetrievalRound(n(1)), &usage())
        .unwrap();
    assert_eq!(batch.input().status, RetrievalStatus::Complete);
    assert_eq!(batch.input().results.len(), 1);
    let result = &batch.input().results[0];
    assert!(
        result
            .fragment
            .provenance
            .source_reference()
            .contains("docs/ADR-011.md@")
    );
    assert_eq!(result.fragment.provenance.source_kind(), SourceKind::Git);
    assert_eq!(result.fragment.scope, scope());
    assert_eq!(result.score, 1_000_000);
    let contextual = repo.retrieve(plan("ADR-011", false).request()).unwrap();
    assert_eq!(
        contextual[0]
            .location
            .as_ref()
            .unwrap()
            .section
            .as_ref()
            .unwrap()
            .as_str(),
        "# Retrieval"
    );
}

#[test]
fn vector_index_detects_stale_content_and_scope_mismatch() {
    let fixture = Fixture::new();
    let repo = repository(&fixture.0);
    let embeddings = TwoDimensionalEmbedding(model());
    let vector = VectorIndexAdapter::new(
        RetrievalSourceId::new("vector").unwrap(),
        RetrievalStrategyId::new("semantic").unwrap(),
        &repo,
        &embeddings,
        model(),
    );
    vector.build().unwrap();
    assert_eq!(vector.metadata().unwrap().model, model());
    let found = vector.retrieve(plan("retrieval", true).request()).unwrap();
    assert_eq!(found.len(), 2);
    fs::write(
        fixture.0.join("docs/ADR-011.md"),
        "# Changed\nADR-011 changed.\n",
    )
    .unwrap();
    assert_eq!(
        vector.retrieve(plan("retrieval", true).request()),
        Err(RetrievalError::StaleIndex)
    );
    vector.rebuild().unwrap();
    assert!(vector.retrieve(plan("ADR-011", true).request()).is_ok());
    vector.invalidate();
    assert_eq!(
        vector.retrieve(plan("ADR-011", true).request()),
        Err(RetrievalError::ServiceUnavailable)
    );
    let mut wrong = plan("ADR-011", true).request().input().clone();
    wrong.scope = ContextScopeId::new("project-b").unwrap();
    assert_eq!(
        vector.retrieve(&RetrievalRequest::new(wrong).unwrap()),
        Err(RetrievalError::ScopeMismatch)
    );
}

struct PreferOther;
impl RetrievalReranker for PreferOther {
    fn model(&self) -> &str {
        "fixture-reranker"
    }
    fn version(&self) -> &str {
        "v1"
    }
    fn rank(
        &self,
        candidates: &[HybridCandidate],
    ) -> Result<Vec<(ReferenceId, u32)>, RetrievalError> {
        Ok(candidates
            .iter()
            .map(|candidate| {
                let score = if candidate
                    .result()
                    .fragment
                    .content
                    .as_str()
                    .contains("different topic")
                {
                    1_000_000
                } else {
                    1
                };
                (candidate.result().fragment.id.clone(), score)
            })
            .collect())
    }
}

#[test]
fn hybrid_result_keeps_exact_identifier_ahead_of_model_reranking() {
    let fixture = Fixture::new();
    let repo = repository(&fixture.0);
    let embeddings = TwoDimensionalEmbedding(model());
    let vector = VectorIndexAdapter::new(
        RetrievalSourceId::new("vector").unwrap(),
        RetrievalStrategyId::new("semantic").unwrap(),
        &repo,
        &embeddings,
        model(),
    );
    vector.build().unwrap();
    let reranker = PreferOther;
    let port = FederatedRetrievalPort {
        adapters: vec![&repo, &vector],
        fusion: fusion(),
        reranker: Some(&reranker),
    };
    let batch = port
        .retrieve(&plan("ADR-011", true), RetrievalRound(n(1)), &usage())
        .unwrap();
    assert_eq!(batch.input().status, RetrievalStatus::Complete);
    assert_eq!(batch.input().results.len(), 2);
    assert_eq!(batch.input().results[0].source.as_str(), "repo");
    assert_eq!(batch.input().results[0].score, 1_000_000);
    assert!(
        batch
            .input()
            .explanations
            .iter()
            .any(|item| item.detail.as_str().contains("fixture-reranker@v1"))
    );
    assert!(
        batch
            .input()
            .explanations
            .iter()
            .any(|item| item.detail.as_str().contains("duplicate from vector"))
    );
}

#[test]
fn optional_vector_outage_preserves_lexical_result_and_reports_degradation() {
    let fixture = Fixture::new();
    let repo = repository(&fixture.0);
    let port = FederatedRetrievalPort {
        adapters: vec![&repo],
        fusion: fusion(),
        reranker: None,
    };
    let batch = port
        .retrieve(&plan("ADR-011", true), RetrievalRound(n(1)), &usage())
        .unwrap();
    assert_eq!(batch.input().status, RetrievalStatus::Degraded);
    assert_eq!(batch.input().results.len(), 1);
    assert!(
        batch
            .input()
            .explanations
            .iter()
            .any(|item| item.reason == RetrievalReason::ServiceUnavailable)
    );
}

struct OfflineReranker;
impl RetrievalReranker for OfflineReranker {
    fn model(&self) -> &str {
        "offline"
    }
    fn version(&self) -> &str {
        "v1"
    }
    fn rank(&self, _: &[HybridCandidate]) -> Result<Vec<(ReferenceId, u32)>, RetrievalError> {
        Err(RetrievalError::ServiceUnavailable)
    }
}

#[test]
fn unavailable_reranker_and_budget_exhaustion_are_explicit() {
    let fixture = Fixture::new();
    let repo = repository(&fixture.0);
    let offline = OfflineReranker;
    let port = FederatedRetrievalPort {
        adapters: vec![&repo],
        fusion: fusion(),
        reranker: Some(&offline),
    };
    let batch = port
        .retrieve(&plan("ADR-011", false), RetrievalRound(n(1)), &usage())
        .unwrap();
    assert_eq!(batch.input().status, RetrievalStatus::Degraded);
    assert!(
        batch
            .input()
            .explanations
            .iter()
            .any(|item| item.target == RetrievalExplanationTarget::Reranker)
    );
    assert_eq!(batch.input().results[0].source.as_str(), "repo");

    let original = plan("ADR-011", false);
    let mut budgeted = original.request().input().clone();
    budgeted.budget.context = ContextBudget::new(
        TokenBudget(1),
        BTreeMap::from([(ContextBudgetClass::Knowledge, TokenBudget(0))]),
    )
    .unwrap();
    let support = RetrievalSupport {
        sources: budgeted
            .sources
            .iter()
            .map(|source| (source.id.clone(), source.kind))
            .collect(),
        strategies: budgeted
            .strategies
            .iter()
            .map(|strategy| (strategy.id.clone(), strategy.kind))
            .collect(),
    };
    let budgeted = RetrievalPlan::new(
        original.id().clone(),
        RetrievalRequest::new(budgeted).unwrap(),
        &support,
    )
    .unwrap();
    let port = FederatedRetrievalPort {
        adapters: vec![&repo],
        fusion: fusion(),
        reranker: None,
    };
    assert_eq!(
        port.retrieve(&budgeted, RetrievalRound(n(1)), &usage()),
        Err(RetrievalError::BudgetExceeded)
    );
}

#[test]
fn no_match_required_source_failure_and_multi_round_accounting() {
    let fixture = Fixture::new();
    let repo = repository(&fixture.0);
    let port = FederatedRetrievalPort {
        adapters: vec![&repo],
        fusion: fusion(),
        reranker: None,
    };
    let no_match = port
        .retrieve(&plan("zxqv-unknown", false), RetrievalRound(n(1)), &usage())
        .unwrap();
    assert_eq!(no_match.input().reason, RetrievalReason::NoMatches);
    assert!(no_match.input().results.is_empty());

    let empty = FederatedRetrievalPort {
        adapters: vec![],
        fusion: fusion(),
        reranker: None,
    };
    assert_eq!(
        empty.retrieve(&plan("ADR-011", false), RetrievalRound(n(1)), &usage()),
        Err(RetrievalError::ServiceUnavailable)
    );
    let measured = empty
        .retrieve_measured(&plan("ADR-011", false), RetrievalRound(n(1)), &usage())
        .unwrap_err();
    assert_eq!(measured.error, RetrievalError::ServiceUnavailable);
    assert_eq!(measured.usage.rounds, 1);
    assert!(measured.usage.tokens >= "ADR-011".len() as u64);

    let original = plan("ADR-011", false);
    let mut input = original.request().input().clone();
    input.budget.rounds = RoundBudget(n(2));
    let old_support = RetrievalSupport {
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
    let old_multi = RetrievalPlan::new(
        original.id().clone(),
        RetrievalRequest::new(input.clone()).unwrap(),
        &old_support,
    )
    .unwrap();
    assert_eq!(
        port.retrieve(&old_multi, RetrievalRound(n(1)), &usage()),
        Err(RetrievalError::InvalidPlan)
    );
    input.version = RetrievalVersion::V2;
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
    let multi = RetrievalPlan::new(
        original.id().clone(),
        RetrievalRequest::new(input).unwrap(),
        &support,
    )
    .unwrap();
    let first = port
        .retrieve(&multi, RetrievalRound(n(1)), &usage())
        .unwrap();
    assert_eq!(first.input().status, RetrievalStatus::Partial);
    assert_eq!(first.input().reason, RetrievalReason::MoreInformationNeeded);
    let second = port
        .retrieve(&multi, RetrievalRound(n(2)), &first.input().usage)
        .unwrap();
    assert_eq!(second.input().status, RetrievalStatus::Complete);
    assert_eq!(second.input().reason, RetrievalReason::BudgetReached);
    assert_eq!(second.input().usage.rounds, 2);
    assert_eq!(second.input().usage.results, 2);
    assert!(second.input().usage.tokens > first.input().usage.tokens);
    assert_eq!(
        port.retrieve(&multi, RetrievalRound(n(3)), &second.input().usage),
        Err(RetrievalError::InvalidPlan)
    );
}

#[test]
fn repository_handles_partial_terms_nontext_and_scope_boundaries() {
    let fixture = Fixture::new();
    let single_file = fixture.0.join("docs/ADR-011.md");
    assert!(matches!(
        RepositoryLexicalAdapter::new(
            scope(),
            RetrievalSourceId::new("repo").unwrap(),
            RetrievalStrategyId::new("lexical").unwrap(),
            &single_file,
            SensitivityClass::Public
        ),
        Err(RetrievalError::ServiceUnavailable)
    ));
    fs::write(fixture.0.join("docs/empty.md"), " \n ").unwrap();
    fs::write(fixture.0.join("docs/binary.md"), [0xff, 0xfe]).unwrap();
    #[cfg(unix)]
    {
        let other = Fixture::new();
        fs::write(other.0.join("docs/private.md"), "external-secret-token").unwrap();
        std::os::unix::fs::symlink(&other.0, fixture.0.join("docs/external")).unwrap();
        assert!(
            repository(&fixture.0)
                .retrieve(plan("external-secret-token", false).request())
                .unwrap()
                .is_empty()
        );
    }
    let repo = repository(&fixture.0);
    let found = repo
        .retrieve(plan("ADR-011 absent", false).request())
        .unwrap();
    assert_eq!(found.len(), 1);
    assert!(!found[0].exact_match);
    assert!(found[0].lexical_score.unwrap() < 1_000_000);
    let mut wrong = plan("ADR-011", false).request().input().clone();
    wrong.scope = ContextScopeId::new("project-b").unwrap();
    assert_eq!(
        repo.retrieve(&RetrievalRequest::new(wrong).unwrap()),
        Err(RetrievalError::ScopeMismatch)
    );

    let restricted = RepositoryLexicalAdapter::new(
        scope(),
        RetrievalSourceId::new("repo").unwrap(),
        RetrievalStrategyId::new("lexical").unwrap(),
        &fixture.0,
        SensitivityClass::Internal,
    )
    .unwrap();
    assert!(
        restricted
            .retrieve(plan("ADR-011", false).request())
            .unwrap()
            .is_empty()
    );
    let embeddings = TwoDimensionalEmbedding(model());
    let vector = VectorIndexAdapter::new(
        RetrievalSourceId::new("vector").unwrap(),
        RetrievalStrategyId::new("semantic").unwrap(),
        &restricted,
        &embeddings,
        model(),
    );
    vector.build().unwrap();
    assert!(
        vector
            .retrieve(plan("ADR-011", true).request())
            .unwrap()
            .is_empty()
    );
}
