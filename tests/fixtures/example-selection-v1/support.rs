use gateway_domain::memory::CurationState;
use gateway_domain::*;
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;
pub fn id(s: &str) -> ReferenceId {
    ReferenceId::new(s).unwrap()
}
pub fn text(s: &str) -> NonEmptyText {
    NonEmptyText::new(s).unwrap()
}
pub fn scope() -> ContextScopeId {
    ContextScopeId::new("project-x").unwrap()
}
pub fn task() -> SemanticTaskIR {
    SemanticTaskIR::from_json(include_str!("../semantic-task-v1/minimal.json")).unwrap()
}
pub fn pin<I>(name: I) -> ResolvedReference<I> {
    ResolvedReference {
        id: name,
        scope: scope(),
        contract: id("example"),
        contract_version: SchemaVersion::V1,
        revision: id("r1"),
        digest: context_content_digest(b"placeholder"),
    }
}
pub fn quality() -> QualityMetadata {
    QualityMetadata::new(
        TrustClass::RetrievedContent,
        SensitivityClass::Normal,
        Confidence::Unknown,
        FreshnessStatus::Fresh,
        Uncertainty::None,
    )
}
pub fn context_requirement() -> ContextRequirement {
    ContextRequirement {
        id: id("examples"),
        class: ContextClass::Examples,
        purpose: text("Demonstrate inspection"),
        necessity: ContextNecessity::Optional,
        permitted_sources: vec![
            ContextSourceClass::Examples,
            ContextSourceClass::Repository,
            ContextSourceClass::Rag,
            ContextSourceClass::Memory,
        ],
        information: RequiredInformation {
            description: text("Validated demonstrations"),
            requirements: InformationRequirements::new(
                FreshnessRequirement::Fresh,
                None,
                vec![],
                vec![],
            )
            .unwrap(),
            accepted_trust: BTreeSet::from([
                TrustClass::RetrievedContent,
                TrustClass::SyntheticData,
            ]),
            maximum_sensitivity: SensitivityClass::Internal,
        },
        budget: RetrievalBudget {
            results: ResultBudget(NonZeroU64::new(3).unwrap()),
            rounds: RoundBudget(NonZeroU64::MIN),
            latency: LatencyBudget(NonZeroU64::MIN),
            cost: CostBudget {
                maximum: 0,
                unit: text("credits"),
            },
            tokens: TokenBudget(200),
            context: ContextBudget::new(
                TokenBudget(200),
                BTreeMap::from([(ContextBudgetClass::Knowledge, TokenBudget(200))]),
            )
            .unwrap(),
        },
        stop: StopCondition::BudgetExhausted,
        priority: 10,
    }
}
pub fn requirements(r: ContextRequirement) -> ContextRequirementsIR {
    let t = task();
    let mut task_pin = pin(t.data().id.clone());
    task_pin.digest = t.content_digest().unwrap();
    ContextRequirementsIR::new(
        SchemaVersion::V1,
        id("r1"),
        ContextPlanningBasis {
            task: task_pin,
            situation: None,
            plan: None,
        },
        vec![r],
        TokenBudget(200),
    )
    .unwrap()
}
pub fn policy() -> ExamplePolicy {
    ExamplePolicy {
        sources: BTreeSet::from([
            ExampleSourceClass::Curated,
            ExampleSourceClass::Repository,
            ExampleSourceClass::Retrieval,
            ExampleSourceClass::GovernedExperience,
            ExampleSourceClass::PromotedProcedure,
        ]),
        reuse: BTreeMap::new(),
        architecture: Some(pin(id("hexagonal"))),
        tags: BTreeSet::from([id("rust"), id("testing")]),
        minimum_relevance: 500,
    }
}
pub fn requirement() -> ExampleRequirement {
    ExampleRequirement::from_context(
        &requirements(context_requirement()),
        &task(),
        &id("examples"),
        policy(),
    )
    .unwrap()
    .unwrap()
}
pub fn fragment(name: &str, content: &str) -> RetrievedFragment {
    RetrievedFragment {
        id: id(name),
        scope: scope(),
        content: text(content),
        provenance: Provenance::new(
            ProvenanceId::new(format!("lineage-{name}")).unwrap(),
            SourceKind::Repository,
            SourceId::new("repo").unwrap(),
            "source://examples/r1",
        )
        .unwrap(),
        snapshot: context_content_digest(content.as_bytes()),
        quality: quality(),
        evidence: BTreeSet::new(),
    }
}
pub fn estimate(tokens: u64) -> TokenEstimate {
    TokenEstimate {
        estimator: TokenEstimatorId::new("fixture").unwrap(),
        version: TokenEstimatorVersion::new("1").unwrap(),
        count: TokenCount::Estimated {
            tokens,
            upper_bound: Some(tokens),
            semantics: text("Conservative full pair bound"),
        },
    }
}
pub fn candidate(name: &str) -> ExampleCandidate {
    let mut c = ExampleCandidate {
        pin: pin(id(name)),
        source_class: ExampleSourceClass::Repository,
        input: fragment(&format!("{name}-input"), "Inspect this Rust module"),
        expected_output: Some(fragment(
            &format!("{name}-output"),
            "Return a typed report with tests",
        )),
        compatibility: requirement().compatibility().clone(),
        validation: CurationState::Validated,
        validation_ref: Some(pin(id("validation"))),
        validated_digest: None,
        successful: true,
        tokens: estimate(40),
    };
    seal(&mut c);
    c
}
pub fn seal(c: &mut ExampleCandidate) {
    c.pin.digest = c.content_digest();
    c.validated_digest = Some(c.pin.digest.clone());
    if let Some(v) = c.validation_ref.as_mut() {
        v.scope = c.pin.scope.clone();
    }
}
pub fn change_pair(c: &mut ExampleCandidate, input: &str, output: &str) {
    c.input.content = text(input);
    c.input.snapshot = context_content_digest(input.as_bytes());
    if let Some(f) = c.expected_output.as_mut() {
        f.content = text(output);
        f.snapshot = context_content_digest(output.as_bytes());
    }
    seal(c);
}
pub fn decision(c: &ExampleCandidate, reason: ExampleReason) -> ExampleDecision {
    ExampleDecision {
        candidate: c.pin.clone(),
        reason,
        relevance: 1000,
        advisory_score: None,
    }
}
pub fn set(c: ExampleCandidate) -> ExampleSet {
    ExampleSet::new(
        id("s1"),
        requirement(),
        vec![c.clone()],
        vec![decision(&c, ExampleReason::Selected)],
    )
    .unwrap()
}
