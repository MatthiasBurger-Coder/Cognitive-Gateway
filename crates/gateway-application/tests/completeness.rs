use gateway_application::interpretation::*;
use gateway_domain::{interpretation::*, *};
use std::cell::Cell;

fn id(s: &str) -> ReferenceId {
    ReferenceId::new(s).unwrap()
}
fn time(s: i64) -> UnixTimestamp {
    UnixTimestamp::new(s)
}
fn pin(s: &str) -> InterpretationPin {
    InterpretationPin {
        id: id(s),
        scope: ContextScopeId::new("project-p").unwrap(),
        contract: id("test-record"),
        contract_version: SchemaVersion::V1,
        revision: id("r1"),
        digest: ContentDigest::new("a".repeat(64)).unwrap(),
    }
}
fn target(s: &str) -> SemanticTarget {
    SemanticTarget {
        kind: TargetKind::Service,
        reference: pin(s),
    }
}
fn provenance(s: &str) -> Provenance {
    Provenance::new(
        ProvenanceId::new(s).unwrap(),
        SourceKind::Caller,
        SourceId::new("source").unwrap(),
        s,
    )
    .unwrap()
}
fn quality(fresh: FreshnessStatus) -> QualityMetadata {
    QualityMetadata::new(
        TrustClass::CanonicalReference,
        SensitivityClass::Normal,
        Confidence::Unknown,
        fresh,
        Uncertainty::None,
    )
}
fn fact(s: &str, entity: &str, tier: InterpretationTier) -> InterpretationFact {
    InterpretationFact {
        id: id(s),
        field: "target".parse().unwrap(),
        tier,
        value: InterpretationValue::Target(target(entity)),
        source: pin(if tier == InterpretationTier::CurrentInput {
            "input"
        } else {
            "source"
        }),
        provenance: provenance(s),
        quality: quality(FreshnessStatus::Fresh),
        valid_until: time(200),
        relevance: NonEmptyText::new("target binding").unwrap(),
        span: Some((0, 10)),
        subject: None,
        protected: false,
        required_pin: false,
        supersedes: None,
    }
}
fn data(facts: Vec<InterpretationFact>) -> InterpretationContextData {
    InterpretationContextData {
        version: SchemaVersion::V1,
        id: id("context"),
        revision: id("capture-1"),
        binding: InterpretationBinding {
            scope: pin("input").scope,
            request: id("request"),
            input: pin("input"),
            conversation: Some(pin("conversation")),
            task: Some(ResolvedReference {
                id: TaskId::new("task").unwrap(),
                scope: pin("task").scope,
                contract: id("test-record"),
                contract_version: SchemaVersion::V1,
                revision: id("r1"),
                digest: pin("task").digest,
            }),
            host_basis: pin("host"),
        },
        captured_at: time(100),
        expires_at: time(200),
        basis_refs: vec![
            pin("input"),
            pin("host"),
            pin("source"),
            pin("conversation"),
            pin("task"),
        ],
        permitted_sources: [
            InterpretationTier::VerifiedMemory,
            InterpretationTier::RetrievedEvidence,
        ]
        .into_iter()
        .map(|tier| InterpretationSource {
            source: pin("source"),
            tier,
            sensitivity_ceiling: SensitivityClass::Normal,
            trust: TrustClass::CanonicalReference,
            max_results: 10,
        })
        .collect(),
        relevant_facts: facts,
        aliases: vec![],
        temporal_anchors: vec![],
    }
}
#[derive(Default)]
struct Host {
    denied: Cell<bool>,
    changed: Cell<bool>,
    rejected: Option<ReferenceId>,
}
impl InterpretationHost for Host {
    fn validate_projection(&self, _: &InterpretationContextData) -> bool {
        !self.denied.get()
    }
    fn validate_query(&self, _: &InterpretationBinding, query: &InterpretationQuery) -> bool {
        query.provenance.id() == &ProvenanceId::new("query").unwrap()
    }
    fn authenticate(&self, binding: &InterpretationBinding) -> Result<(), InterpretationError> {
        if self.denied.get()
            || binding
                .conversation
                .as_ref()
                .is_some_and(|p| p.id != id("conversation"))
        {
            Err(InterpretationError::Admission)
        } else {
            Ok(())
        }
    }
    fn validate_basis(&self, _: &InterpretationPin, _: UnixTimestamp) -> bool {
        !self.changed.get()
    }
    fn validate_fact(
        &self,
        f: &InterpretationFact,
        _: UnixTimestamp,
    ) -> Result<(), SemanticGapReason> {
        if self.rejected.as_ref() == Some(&f.id) {
            Err(SemanticGapReason::InvalidReference)
        } else {
            Ok(())
        }
    }
}
#[derive(Default)]
struct Lookup(Vec<InterpretationFact>);
impl InterpretationLookup for Lookup {
    fn lookup(
        &self,
        _: &ContextScopeId,
        source: &InterpretationSource,
        _: &EntityQuery,
    ) -> Vec<InterpretationFact> {
        self.0
            .iter()
            .filter(|f| f.tier == source.tier)
            .cloned()
            .collect()
    }
}
fn query() -> InterpretationQuery {
    InterpretationQuery {
        field: "target".parse().unwrap(),
        expected: ExpectedMeaning::Target(TargetKind::Service),
        expression: ReferenceExpression::Focus,
        provenance: provenance("query"),
        span: (0, 10),
        subject: None,
    }
}

use gateway_application::completeness::*;

impl CompletenessHost for Host {
    fn validate_requirements(
        &self,
        _: &InterpretationBinding,
        revision: &ContentDigest,
        fields: &[SemanticField],
    ) -> bool {
        revision == &revision_id()
            && fields.iter().any(|f| {
                f.query.field.to_string() == "target"
                    && f.requirement == SemanticFieldRequirement::Mandatory
            })
    }
}
impl SemanticReferenceValidator for Host {
    fn validate(
        &self,
        _: &SemanticTaskData,
        _: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        if self.rejected.is_some() {
            Err(ValidationError::InvalidStateCombination {
                reason: "reference denied",
            })
        } else {
            Ok(())
        }
    }
}
fn revision_id() -> ContentDigest {
    ContentDigest::new("b".repeat(64)).unwrap()
}
fn field(name: &str, dependencies: &[&str]) -> SemanticField {
    let mut q = query();
    q.field = name.parse().unwrap();
    SemanticField {
        query: q,
        requirement: SemanticFieldRequirement::Mandatory,
        depends_on: dependencies.iter().map(|s| s.parse().unwrap()).collect(),
    }
}
fn validator<'a>(
    host: &'a Host,
    lookup: &'a Lookup,
) -> CompletenessValidator<'a, Host, Lookup, Lookup> {
    CompletenessValidator {
        resolver: DeterministicInterpretationResolver {
            host,
            registry: lookup,
            project: lookup,
        },
    }
}
fn check(d: InterpretationContextData, fields: &[SemanticField]) -> CompletenessResult {
    let host = Host::default();
    let lookup = Lookup::default();
    let context = InterpretationContext::admit(d, &host, time(100)).unwrap();
    validator(&host, &lookup)
        .validate(&context, &revision_id(), fields, time(100))
        .unwrap()
}
fn task() -> SemanticTaskIR {
    let source = include_str!("../../../tests/fixtures/semantic-task-v1/minimal.json")
        .replace("project-x", "project-p")
        .replace("task-inspect", "task");
    SemanticTaskIR::from_json(&source).unwrap()
}

#[test]
fn missing_facts_are_gaps_and_dependency_frontier_is_minimal() {
    let fields = vec![
        field("goal", &["target"]),
        field("target", &[]),
        field("evidence", &["goal"]),
    ];
    let result = check(data(vec![]), &fields);
    assert!(!result.is_complete());
    assert_eq!(result.gaps.len(), 3);
    assert_eq!(result.clarification.questions.len(), 1);
    assert_eq!(
        result.clarification.questions[0].field.to_string(),
        "target"
    );
    assert_eq!(
        result.gaps[0].diagnostic_code(),
        "SEM_MISSING_REQUIRED_FIELD"
    );
    let mut reversed = fields.clone();
    reversed.reverse();
    assert_eq!(result, check(data(vec![]), &reversed));
    assert_eq!(
        result.canonical_bytes(),
        check(data(vec![]), &reversed).canonical_bytes()
    );
    let mut d = data(vec![]);
    d.revision = id("capture-2");
    assert_ne!(
        result.clarification.questions[0].id,
        check(d, &fields).clarification.questions[0].id
    );
    let mut d = data(vec![]);
    d.binding.request = id("other-request");
    assert_ne!(
        result.clarification.questions[0].id,
        check(d, &fields).clarification.questions[0].id
    );
    let wire = serde_json::to_value(&result.clarification).unwrap();
    assert_eq!(wire["version"], "1.0");
    assert_eq!(wire["binding"]["task"]["id"], "task");
    assert_eq!(wire["semantic_revision"], "b".repeat(64));
}

#[test]
fn deterministic_context_memory_and_retrieval_precede_questions() {
    for tier in [
        InterpretationTier::CurrentInput,
        InterpretationTier::Situation,
        InterpretationTier::VerifiedMemory,
        InterpretationTier::RetrievedEvidence,
    ] {
        let result = check(
            data(vec![fact("known", "service", tier)]),
            &[field("target", &[])],
        );
        assert!(result.is_complete());
        assert!(result.clarification.questions.is_empty());
    }
    let host = Host::default();
    let lookup = Lookup(vec![fact(
        "lookup",
        "service",
        InterpretationTier::RetrievedEvidence,
    )]);
    let context = InterpretationContext::admit(data(vec![]), &host, time(100)).unwrap();
    let mut f = field("target", &[]);
    f.query.expression = ReferenceExpression::Entity(EntityQuery::Alias {
        label: NonEmptyText::new("service").unwrap(),
        kind: TargetKind::Service,
    });
    let result = validator(&host, &lookup)
        .validate(&context, &revision_id(), &[f], time(100))
        .unwrap();
    assert!(result.is_complete());
    assert_eq!(result.decisions[0].lookup_facts.len(), 1);
}

#[test]
fn advisory_gaps_do_not_ask_or_block_but_invalid_input_requires_repair() {
    let mut advisory = field("history", &[]);
    advisory.requirement = SemanticFieldRequirement::Advisory;
    let result = check(
        data(vec![fact(
            "known",
            "service",
            InterpretationTier::Situation,
        )]),
        &[field("target", &[]), advisory],
    );
    assert!(result.is_complete());
    assert_eq!(result.gaps.len(), 1);
    assert!(result.clarification.questions.is_empty());
    let mut f = field("target", &[]);
    f.query.expression = ReferenceExpression::Entity(EntityQuery::Canonical {
        id: id("absent"),
        kind: TargetKind::Service,
    });
    let result = check(data(vec![]), &[f]);
    assert!(!result.is_complete());
    assert!(result.clarification.questions.is_empty());
    assert_eq!(
        result.gaps[0].required_resolution_class(),
        KnowledgeResolutionClass::InputRepair
    );
}

#[test]
fn ambiguous_and_conflicting_meanings_are_never_selected() {
    for (tier, reason) in [
        (
            InterpretationTier::ConversationFocus,
            SemanticGapReason::AmbiguousReference,
        ),
        (
            InterpretationTier::Situation,
            SemanticGapReason::ConflictingContext,
        ),
    ] {
        let mut f = field("target", &[]);
        f.query.subject = Some(target("subject"));
        let result = check(
            data(vec![fact("one", "a", tier), fact("two", "b", tier)]),
            &[f],
        );
        assert!(!result.is_complete());
        assert_eq!(result.gaps[0].reason(), reason);
        assert!(matches!(
            result.gaps[0].subject(),
            KnowledgeGapSubject::Target(_)
        ));
    }
}

#[test]
fn malformed_or_downgraded_manifests_fail_before_resolution() {
    let host = Host::default();
    let lookup = Lookup::default();
    let context = InterpretationContext::admit(data(vec![]), &host, time(100)).unwrap();
    let v = validator(&host, &lookup);
    let mut advisory = field("goal", &[]);
    advisory.requirement = SemanticFieldRequirement::Advisory;
    for fields in [
        vec![],
        vec![field("target", &[]); 2],
        vec![field("target", &["absent"])],
        vec![field("target", &["target"])],
        vec![field("target", &["goal"]), field("goal", &["target"])],
        vec![field("target", &["goal"]), advisory],
        vec![field("goal", &[])],
    ] {
        assert_eq!(
            v.validate(&context, &revision_id(), &fields, time(100)),
            Err(CompletenessError::InvalidRequirements)
        );
    }
    assert_eq!(
        v.validate(
            &context,
            &revision_id(),
            &vec![field("target", &[]); 129],
            time(100)
        ),
        Err(CompletenessError::ResourceLimit)
    );
    let mut f = field("target", &[]);
    f.query.span = (1, 1);
    assert_eq!(
        v.validate(&context, &revision_id(), &[f], time(100)),
        Err(CompletenessError::Interpretation(
            InterpretationError::InvalidQuery
        ))
    );
}

#[test]
fn resolved_child_stays_blocked_until_prerequisite_is_known() {
    let mut known = fact("goal-fact", "service", InterpretationTier::Situation);
    known.field = "goal".parse().unwrap();
    let result = check(
        data(vec![known]),
        &[field("target", &[]), field("goal", &["target"])],
    );
    assert_eq!(result.gaps.len(), 2);
    assert_eq!(result.clarification.questions.len(), 1);
}

#[test]
fn clarified_request_reenters_full_validation_and_normal_compiler() {
    let host = Host::default();
    let lookup = Lookup::default();
    let v = validator(&host, &lookup);
    let fields = [field("target", &[])];
    let context = InterpretationContext::admit(data(vec![]), &host, time(100)).unwrap();
    let (_, compiled) = v
        .compile(&context, &revision_id(), &fields, time(100), |_| {
            panic!("must not compile missing semantics")
        })
        .unwrap();
    assert!(compiled.is_none());
    let mut d = data(vec![fact(
        "reply",
        "service",
        InterpretationTier::CurrentInput,
    )]);
    d.revision = id("reply-capture");
    let fresh = InterpretationContext::admit(d, &host, time(101)).unwrap();
    let (result, compiled) = v
        .compile(&fresh, &revision_id(), &fields, time(101), |decisions| {
            let mut source: serde_json::Value =
                serde_json::from_str(include_str!("../../../examples/cgsl-v1/minimal.cgsl"))
                    .unwrap();
            source["id"] = serde_json::json!("task");
            source["output"]["schema"]["scope"] = serde_json::json!("project-p");
            if let InterpretationValue::Target(target) =
                decisions[0].resolution.require_resolved().unwrap()
            {
                source["target"] = serde_json::to_value(target).unwrap();
            }
            let parsed = gateway_cgsl::compile("clarified-input", source.to_string().as_bytes())
                .map_err(|_| CompletenessError::Compilation)?;
            parsed
                .task_shape()
                .cloned()
                .ok_or(CompletenessError::Compilation)
        })
        .unwrap();
    assert!(result.is_complete());
    assert_eq!(compiled.unwrap().data().target.reference.id, id("service"));
    host.changed.set(true);
    assert_eq!(
        v.compile(&fresh, &revision_id(), &fields, time(101), |_| panic!()),
        Err(CompletenessError::Interpretation(
            InterpretationError::ChangedBasis
        ))
    );
    host.changed.set(false);
    assert_eq!(
        v.compile(&fresh, &revision_id(), &fields, time(200), |_| panic!()),
        Err(CompletenessError::Interpretation(
            InterpretationError::Expired
        ))
    );
    host.denied.set(true);
    assert!(
        v.compile(&fresh, &revision_id(), &fields, time(101), |_| panic!())
            .is_err()
    );
}

#[test]
fn compiler_errors_reference_failures_and_correlation_mismatch_fail_closed() {
    let host = Host::default();
    let lookup = Lookup::default();
    let v = validator(&host, &lookup);
    let context = InterpretationContext::admit(
        data(vec![fact(
            "known",
            "service",
            InterpretationTier::Situation,
        )]),
        &host,
        time(100),
    )
    .unwrap();
    let fields = [field("target", &[])];
    assert_eq!(
        v.compile(&context, &revision_id(), &fields, time(100), |_| Err(
            CompletenessError::Compilation
        )),
        Err(CompletenessError::Compilation)
    );
    for change in [0, 1] {
        assert_eq!(
            v.compile(&context, &revision_id(), &fields, time(100), |_| {
                let mut t = task().data().clone();
                if change == 0 {
                    t.id = TaskId::new("other").unwrap();
                } else {
                    t.target.reference.scope = ContextScopeId::new("foreign").unwrap();
                    t.output_contract.schema.scope = ContextScopeId::new("foreign").unwrap();
                }
                Ok(SemanticTaskIR::new(t).unwrap())
            }),
            Err(CompletenessError::Compilation)
        );
    }
    assert_eq!(
        v.compile(&context, &revision_id(), &fields, time(100), |_| {
            host.changed.set(true);
            Ok(task())
        }),
        Err(CompletenessError::Interpretation(
            InterpretationError::ChangedBasis
        ))
    );
    let host = Host {
        rejected: Some(id("different")),
        ..Host::default()
    };
    let v = validator(&host, &lookup);
    assert_eq!(
        v.compile(&context, &revision_id(), &fields, time(100), |_| Ok(task())),
        Err(CompletenessError::Compilation)
    );
}

#[test]
fn independent_questions_are_sorted_and_advisory_diagnostics_keep_lineage() {
    let result = check(data(vec![]), &[field("target", &[]), field("goal", &[])]);
    assert_eq!(
        result
            .clarification
            .questions
            .iter()
            .map(|q| q.field.to_string())
            .collect::<Vec<_>>(),
        vec!["goal", "target"]
    );
    let value: serde_json::Value = serde_json::from_slice(&result.canonical_bytes()).unwrap();
    assert_eq!(value["gaps"][0]["diagnostic"], "SEM_MISSING_REQUIRED_FIELD");
    assert_eq!(
        value["gaps"][0]["provenance"][0],
        serde_json::to_value(provenance("query")).unwrap()
    );
    assert_eq!(value["decisions"].as_array().unwrap().len(), 2);
    let mut result = result;
    result.gaps.push(
        KnowledgeGap::new(
            KnowledgeGapSubject::Path("goal".parse().unwrap()),
            "goal".parse().unwrap(),
            SemanticGapReason::MissingRequiredField,
            vec![provenance("query")],
            KnowledgeResolutionClass::HumanClarification,
            SemanticFieldRequirement::Advisory,
        )
        .unwrap(),
    );
    assert!(!result.canonical_bytes().is_empty());
    let mut f = field("target", &[]);
    f.query.subject = Some(target("subject"));
    assert!(!check(data(vec![]), &[f]).canonical_bytes().is_empty());
}

#[test]
fn plans_require_task_correlation_and_compile_rechecks_fresh_resolution() {
    let host = Host::default();
    let lookup = Lookup::default();
    let mut d = data(vec![]);
    d.binding.task = None;
    let context = InterpretationContext::admit(d, &host, time(100)).unwrap();
    assert_eq!(
        validator(&host, &lookup).validate(
            &context,
            &revision_id(),
            &[field("target", &[])],
            time(100)
        ),
        Err(CompletenessError::InvalidRequirements)
    );
}
