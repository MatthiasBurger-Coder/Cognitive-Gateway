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
fn resolve(d: InterpretationContextData, q: &InterpretationQuery) -> InterpretationDecision {
    let host = Host::default();
    let context = InterpretationContext::admit(d, &host, time(100)).unwrap();
    DeterministicInterpretationResolver {
        host: &host,
        registry: &Lookup::default(),
        project: &Lookup::default(),
    }
    .resolve(&context, q, time(100))
    .unwrap()
}
fn entity_query(alias: bool) -> InterpretationQuery {
    InterpretationQuery {
        expression: ReferenceExpression::Entity(if alias {
            EntityQuery::Alias {
                label: NonEmptyText::new("payments").unwrap(),
                kind: TargetKind::Service,
            }
        } else {
            EntityQuery::Canonical {
                id: id("A"),
                kind: TargetKind::Service,
            }
        }),
        ..query()
    }
}
fn aliases(entities: &[&str]) -> InterpretationContextData {
    let mut d = data(vec![fact(
        "focus",
        "A",
        InterpretationTier::ConversationFocus,
    )]);
    d.aliases = entities
        .iter()
        .enumerate()
        .map(|(i, entity)| InterpretationAlias {
            label: NonEmptyText::new("payments").unwrap(),
            kind: TargetKind::Service,
            fact: fact(
                &format!("alias-{i}"),
                entity,
                InterpretationTier::VerifiedMemory,
            ),
        })
        .collect();
    d
}

#[test]
fn every_precedence_tier_and_permutation_is_deterministic() {
    use InterpretationTier::*;
    let tiers = [
        CurrentInput,
        TaskState,
        Situation,
        ConversationFocus,
        VerifiedMemory,
        RetrievedEvidence,
    ];
    for start in 0..tiers.len() {
        let facts: Vec<_> = tiers[start..]
            .iter()
            .enumerate()
            .map(|(i, tier)| fact(&format!("f-{i}"), &format!("entity-{i}"), *tier))
            .collect();
        let d = data(facts);
        let a = resolve(d.clone(), &query());
        assert_eq!(a.selected_tier, Some(tiers[start]));
        assert_eq!(
            a.resolution.require_resolved().unwrap(),
            &InterpretationValue::Target(target("entity-0"))
        );
        let mut reversed = d;
        reversed.relevant_facts.reverse();
        reversed.basis_refs.reverse();
        reversed.permitted_sources.reverse();
        assert_eq!(a, resolve(reversed, &query()));
        assert!(
            a.trace
                .iter()
                .all(|t| t.rule == "selected-tier" || t.rule == "superseded-tier")
        );
    }
}

#[test]
fn ambiguity_conflict_deduplication_and_provenance() {
    use InterpretationTier::*;
    for tier in [CurrentInput, TaskState, ConversationFocus] {
        let result = resolve(
            data(vec![
                fact("a", "A", tier),
                fact("b", "B", tier),
                fact("memory", "A", VerifiedMemory),
            ]),
            &query(),
        );
        assert_eq!(
            result.resolution.state(),
            if tier <= TaskState {
                SemanticResolutionState::Conflicting
            } else {
                SemanticResolutionState::Ambiguous
            }
        );
    }
    let result = resolve(
        data(vec![
            fact("a", "A", ConversationFocus),
            fact("b", "A", ConversationFocus),
        ]),
        &query(),
    );
    let SemanticResolution::Resolved(candidate) = result.resolution else {
        panic!()
    };
    assert_eq!(candidate.provenance().len(), 2);
    let a = fact("a", "A", ConversationFocus);
    let mut b = fact("b", "A", ConversationFocus);
    if let InterpretationValue::Target(t) = &mut b.value {
        t.reference.digest = ContentDigest::new("b".repeat(64)).unwrap();
    }
    assert_eq!(
        resolve(data(vec![a, b]), &query()).resolution.state(),
        SemanticResolutionState::Conflicting
    );
}

#[test]
fn explicit_alias_never_falls_back_or_uses_case_folding() {
    for (entities, state) in [
        (vec![], SemanticResolutionState::Unresolved),
        (vec!["A"], SemanticResolutionState::Resolved),
        (vec!["A", "B"], SemanticResolutionState::Ambiguous),
    ] {
        let result = resolve(aliases(&entities), &entity_query(true));
        assert_eq!(result.resolution.state(), state);
        assert_eq!(result.selected_tier, Some(InterpretationTier::CurrentInput));
    }
    let mut d = aliases(&["A"]);
    d.aliases[0].label = NonEmptyText::new("Payments").unwrap();
    assert_eq!(
        resolve(d, &entity_query(true))
            .resolution
            .require_resolved(),
        Err("SEM_UNKNOWN_ENTITY")
    );
    assert_eq!(
        resolve(aliases(&["B"]), &entity_query(false))
            .resolution
            .require_resolved(),
        Err("SEM_INVALID_REFERENCE")
    );
}

#[test]
fn scoped_registry_and_project_ports_resolve_exact_ids_and_capture_results() {
    let host = Host::default();
    let context = InterpretationContext::admit(aliases(&["B"]), &host, time(100)).unwrap();
    let lookup = Lookup(vec![fact(
        "registry",
        "A",
        InterpretationTier::VerifiedMemory,
    )]);
    let empty = Lookup::default();
    for (registry, project) in [(&lookup, &empty), (&empty, &lookup)] {
        let decision = DeterministicInterpretationResolver {
            host: &host,
            registry,
            project,
        }
        .resolve(&context, &entity_query(false), time(100))
        .unwrap();
        assert_eq!(
            decision.resolution.require_resolved().unwrap(),
            &InterpretationValue::Target(target("A"))
        );
        assert_eq!(decision.lookup_facts, lookup.0);
        let SemanticResolution::Resolved(candidate) = decision.resolution else {
            panic!()
        };
        assert_eq!(candidate.provenance().len(), 2);
    }
}

#[test]
fn corrections_are_explicit_and_protected_conflicts_survive_precedence() {
    let a = fact("a", "A", InterpretationTier::CurrentInput);
    let mut b = fact("b", "B", InterpretationTier::CurrentInput);
    b.supersedes = Some(a.id.clone());
    let decision = resolve(data(vec![a.clone(), b]), &query());
    assert_eq!(
        decision.resolution.require_resolved().unwrap(),
        &InterpretationValue::Target(target("B"))
    );
    assert!(
        decision
            .trace
            .iter()
            .any(|t| t.rule == "explicit-correction")
    );
    let mut protected = fact("task", "B", InterpretationTier::TaskState);
    protected.protected = true;
    assert_eq!(
        resolve(data(vec![a, protected]), &query())
            .resolution
            .state(),
        SemanticResolutionState::Conflicting
    );
}

#[test]
fn stale_implicit_sources_are_excluded_but_explicit_pins_block() {
    for tier in [
        InterpretationTier::CurrentInput,
        InterpretationTier::TaskState,
        InterpretationTier::VerifiedMemory,
    ] {
        let mut a = fact("a", "A", tier);
        a.valid_until = time(100);
        let b = fact("b", "B", InterpretationTier::RetrievedEvidence);
        let result = resolve(data(vec![a.clone(), b.clone()]), &query());
        assert_eq!(
            result.resolution.state(),
            if tier <= InterpretationTier::TaskState {
                SemanticResolutionState::Invalid
            } else {
                SemanticResolutionState::Resolved
            }
        );
        assert_eq!(
            result.trace[0].reason,
            Some(SemanticGapReason::StaleReference)
        );
        a.required_pin = true;
        assert_eq!(
            resolve(data(vec![a, b]), &query()).resolution.state(),
            SemanticResolutionState::Invalid
        );
    }
    let mut a = fact("a", "A", InterpretationTier::ConversationFocus);
    a.quality = quality(FreshnessStatus::Unknown);
    assert_eq!(
        resolve(data(vec![a]), &query())
            .resolution
            .require_resolved(),
        Err("SEM_STALE_REFERENCE")
    );
}

#[test]
fn missing_denied_conflicted_and_target_dependent_context_never_supply_guesses() {
    assert_eq!(
        resolve(data(vec![]), &query())
            .resolution
            .require_resolved(),
        Err("SEM_MISSING_REQUIRED_FIELD")
    );
    let mut f = fact("a", "A", InterpretationTier::VerifiedMemory);
    let mut d = data(vec![f.clone()]);
    d.permitted_sources.clear();
    assert_eq!(
        resolve(d, &query()).resolution.state(),
        SemanticResolutionState::Unresolved
    );
    f.quality = f.quality.with_conflict(ConflictStatus::Unresolved);
    assert_eq!(
        resolve(data(vec![f]), &query()).trace[0].reason,
        Some(SemanticGapReason::ConflictingContext)
    );
    let mut f = fact("dependent", "A", InterpretationTier::ConversationFocus);
    f.subject = Some(target("A"));
    let mut q = query();
    q.subject = Some(target("B"));
    assert_eq!(
        resolve(data(vec![f.clone()]), &q).resolution.state(),
        SemanticResolutionState::Unresolved
    );
    q.subject = Some(target("A"));
    assert_eq!(
        resolve(data(vec![f]), &q).resolution.state(),
        SemanticResolutionState::Resolved
    );
}

fn temporal() -> (InterpretationContextData, InterpretationQuery) {
    let mut f = fact("anchor", "A", InterpretationTier::ConversationFocus);
    f.value = InterpretationValue::Instant(time(1791712800)); // 2026-10-11 10:00 UTC
    let mut d = data(vec![]);
    d.temporal_anchors.push(TemporalAnchor {
        name: id("now"),
        fact: f,
        timezone: Some(NonEmptyText::new("UTC").unwrap()),
        calendar: Some(NonEmptyText::new("Gregorian").unwrap()),
        yesterday: Some((time(1791590400), time(1791676800))),
    });
    (
        d,
        InterpretationQuery {
            expected: ExpectedMeaning::Temporal,
            expression: ReferenceExpression::Yesterday { anchor: id("now") },
            ..query()
        },
    )
}

#[test]
fn temporal_resolution_uses_pinned_instant_and_calendar_not_machine_time() {
    let (d, mut q) = temporal();
    assert_eq!(
        resolve(d.clone(), &q)
            .resolution
            .require_resolved()
            .unwrap(),
        &InterpretationValue::Interval {
            start: time(1791590400),
            end: time(1791676800)
        }
    );
    q.expression = ReferenceExpression::RelativeSeconds {
        anchor: id("now"),
        seconds: -60,
    };
    assert_eq!(
        resolve(d.clone(), &q)
            .resolution
            .require_resolved()
            .unwrap(),
        &InterpretationValue::Instant(time(1791712740))
    );
    q.expression = ReferenceExpression::RelativeSeconds {
        anchor: id("now"),
        seconds: i64::MAX,
    };
    assert_eq!(
        resolve(d, &q).resolution.state(),
        SemanticResolutionState::Unresolved
    );
    for missing in 0..4 {
        let (mut d, q) = temporal();
        match missing {
            0 => d.temporal_anchors[0].timezone = None,
            1 => d.temporal_anchors[0].calendar = None,
            2 => d.temporal_anchors[0].yesterday = None,
            _ => d.temporal_anchors.clear(),
        }
        assert_eq!(
            resolve(d, &q).resolution.state(),
            SemanticResolutionState::Unresolved
        );
    }
    let (mut d, q) = temporal();
    let mut second = d.temporal_anchors[0].clone();
    second.fact.id = id("other-anchor");
    second.fact.provenance = provenance("other-anchor");
    second.yesterday = Some((time(1), time(2)));
    d.temporal_anchors.push(second);
    assert_eq!(
        resolve(d, &q).resolution.state(),
        SemanticResolutionState::Ambiguous
    );
}

#[test]
fn wire_admission_canonical_bytes_expiry_and_changed_basis() {
    let host = Host::default();
    let (d, _) = temporal();
    let context = InterpretationContext::admit(d.clone(), &host, time(100)).unwrap();
    let replay =
        InterpretationContext::admit_json(context.canonical_bytes(), &host, time(100)).unwrap();
    assert_eq!(context.digest(), replay.digest());
    assert_eq!(context.canonical_bytes(), replay.canonical_bytes());
    assert_eq!(
        context.revalidate(&host, time(99)),
        Err(InterpretationError::Expired)
    );
    assert_eq!(
        context.revalidate(&host, time(200)),
        Err(InterpretationError::Expired)
    );
    assert!(context.revalidate(&host, time(199)).is_ok());
    host.changed.set(true);
    assert_eq!(
        context.revalidate(&host, time(100)),
        Err(InterpretationError::ChangedBasis)
    );
    host.denied.set(true);
    assert_eq!(
        context.revalidate(&host, time(100)),
        Err(InterpretationError::Admission)
    );
    assert!(InterpretationContext::admit_json(b"{}", &Host::default(), time(100)).is_err());
    let mut json = serde_json::to_value(d).unwrap();
    json["transcript"] = "untyped history".into();
    assert!(
        InterpretationContext::admit_json(
            &serde_json::to_vec(&json).unwrap(),
            &Host::default(),
            time(100)
        )
        .is_err()
    );
}

#[test]
fn scope_session_and_snapshot_integrity_fail_closed() {
    let host = Host::default();
    let base = data(vec![fact("a", "A", InterpretationTier::ConversationFocus)]);
    for case in 0..16 {
        let mut d = base.clone();
        match case {
            0 => d.version = "2.0".parse().unwrap(),
            1 => d.expires_at = d.captured_at,
            2 => d.basis_refs.clear(),
            3 => d.basis_refs[0].scope = ContextScopeId::new("foreign").unwrap(),
            4 => d.basis_refs[0].contract_version = "2.0".parse().unwrap(),
            5 => {
                let mut p = d.basis_refs[0].clone();
                p.digest = ContentDigest::new("b".repeat(64)).unwrap();
                d.basis_refs.push(p);
            }
            6 => d.permitted_sources[0].max_results = 0,
            7 => d.relevant_facts[0].source = pin("missing-source"),
            8 => d.relevant_facts[0].span = Some((10, 1)),
            9 => d.binding.conversation = None,
            10 => {
                let mut f = d.relevant_facts[0].clone();
                f.value = InterpretationValue::Target(target("B"));
                d.relevant_facts.push(f);
            }
            11 => {
                let mut f = d.relevant_facts[0].clone();
                f.id = id("b");
                f.provenance = Provenance::new(
                    f.provenance.id().clone(),
                    SourceKind::Caller,
                    SourceId::new("other").unwrap(),
                    "other",
                )
                .unwrap();
                d.relevant_facts.push(f);
            }
            12 => d.relevant_facts[0].supersedes = Some(id("missing")),
            13 => d.relevant_facts[0].supersedes = Some(id("a")),
            14 => d.binding.conversation = Some(pin("foreign-session")),
            _ => {
                if let InterpretationValue::Target(t) = &mut d.relevant_facts[0].value {
                    t.reference.scope = ContextScopeId::new("foreign").unwrap();
                }
            }
        }
        assert!(
            InterpretationContext::admit(d, &host, time(100)).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn malformed_alias_anchor_correction_and_task_input_are_rejected() {
    let host = Host::default();
    for case in 0..8 {
        let mut d = aliases(&["A"]);
        match case {
            0 => d.aliases[0].kind = TargetKind::Artifact,
            1 => {
                let mut f = fact("input", "A", InterpretationTier::CurrentInput);
                f.span = None;
                d.relevant_facts.push(f);
            }
            2 => {
                d.binding.task = None;
                d.relevant_facts
                    .push(fact("task", "A", InterpretationTier::TaskState));
            }
            3 => {
                d = temporal().0;
                d.temporal_anchors[0].fact.value = InterpretationValue::Target(target("A"));
            }
            4 => {
                d = temporal().0;
                d.temporal_anchors[0].yesterday = Some((time(2), time(1)));
            }
            5 => {
                d.relevant_facts[0].value = InterpretationValue::Interval {
                    start: time(2),
                    end: time(1),
                };
            }
            6 => {
                let mut a = fact("a", "A", InterpretationTier::CurrentInput);
                let mut b = fact("b", "B", InterpretationTier::CurrentInput);
                a.supersedes = Some(b.id.clone());
                b.supersedes = Some(a.id.clone());
                d.relevant_facts = vec![a, b];
            }
            _ => {
                d.relevant_facts[0].protected = true;
                let mut correction = fact("correction", "B", InterpretationTier::ConversationFocus);
                correction.supersedes = Some(d.relevant_facts[0].id.clone());
                d.relevant_facts.push(correction);
            }
        }
        assert!(
            InterpretationContext::admit(d, &host, time(100)).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn adapters_cannot_return_foreign_wrong_kind_wrong_id_or_unbounded_results() {
    let host = Host::default();
    let mut d = data(vec![]);
    d.permitted_sources[0].max_results = 1;
    let context = InterpretationContext::admit(d, &host, time(100)).unwrap();
    for case in 0..6 {
        let mut f = fact("lookup", "A", InterpretationTier::VerifiedMemory);
        match case {
            0 => {
                if let InterpretationValue::Target(t) = &mut f.value {
                    t.reference.scope = ContextScopeId::new("foreign").unwrap();
                }
            }
            1 => {
                if let InterpretationValue::Target(t) = &mut f.value {
                    t.kind = TargetKind::Artifact;
                }
            }
            2 => f.value = InterpretationValue::Target(target("wrong-id")),
            3 => f.value = InterpretationValue::Instant(time(100)),
            4 => f.source = pin("input"),
            _ => {}
        }
        let mut lookup = Lookup(vec![f]);
        if case == 5 {
            lookup
                .0
                .push(fact("extra", "A", InterpretationTier::VerifiedMemory));
        }
        let resolver = DeterministicInterpretationResolver {
            host: &host,
            registry: &lookup,
            project: &Lookup::default(),
        };
        assert!(
            resolver
                .resolve(&context, &entity_query(false), time(100))
                .is_err(),
            "case {case}"
        );
    }
}

#[test]
fn expected_type_reference_literals_and_host_rejections() {
    let mut f = fact("f", "A", InterpretationTier::ConversationFocus);
    f.value = InterpretationValue::Reference(pin("record"));
    let mut q = query();
    q.expected = ExpectedMeaning::Reference(id("test-record"));
    assert_eq!(
        resolve(data(vec![f.clone()]), &q)
            .resolution
            .require_resolved()
            .unwrap(),
        &f.value
    );
    q.expected = ExpectedMeaning::Literal;
    assert_eq!(
        resolve(data(vec![f.clone()]), &q).resolution.state(),
        SemanticResolutionState::Unresolved
    );
    f.value = InterpretationValue::Literal(TypedValue::Boolean(true));
    assert_eq!(
        resolve(data(vec![f]), &q).resolution.state(),
        SemanticResolutionState::Resolved
    );
    let host = Host {
        rejected: Some(id("f")),
        ..Host::default()
    };
    let d = data(vec![fact("f", "A", InterpretationTier::ConversationFocus)]);
    let context = InterpretationContext::admit(d, &host, time(100)).unwrap();
    let resolver = DeterministicInterpretationResolver {
        host: &host,
        registry: &Lookup::default(),
        project: &Lookup::default(),
    };
    assert_eq!(
        resolver
            .resolve(&context, &query(), time(100))
            .unwrap()
            .resolution
            .state(),
        SemanticResolutionState::Unresolved
    );
    let mut q = query();
    q.span = (2, 1);
    assert_eq!(
        resolver.resolve(&context, &q, time(100)),
        Err(InterpretationError::InvalidQuery)
    );
}

#[test]
fn decision_wire_and_handoff_revalidate_exact_lookup_basis_and_time() {
    let host = Host::default();
    let d = data(vec![]);
    let q = entity_query(false);
    let ReferenceExpression::Entity(entity) = q.expression.clone() else {
        panic!()
    };
    let adapter = CapturedInterpretationLookup::new(vec![CapturedLookupEntry {
        scope: d.binding.scope.clone(),
        source: d.permitted_sources[0].clone(),
        query: entity,
        facts: vec![fact("registry", "A", InterpretationTier::VerifiedMemory)],
    }]);
    let context = InterpretationContext::admit(d, &host, time(100)).unwrap();
    let empty = CapturedInterpretationLookup::default();
    let resolver = DeterministicInterpretationResolver {
        host: &host,
        registry: &adapter,
        project: &empty,
    };
    let decision = resolver.resolve(&context, &q, time(100)).unwrap();
    assert_eq!(
        resolver.revalidate_decision(&context, &decision, time(100)),
        Ok(())
    );
    assert_eq!(
        resolver.revalidate_decision(&context, &decision, time(101)),
        Err(InterpretationError::ChangedBasis)
    );
    let json: serde_json::Value = serde_json::from_slice(&decision.canonical_bytes()).unwrap();
    assert_eq!(json["state"], "RESOLVED");
    assert_eq!(json["request"], "request");
    assert_eq!(
        json["candidates"][0]["provenance"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let missing = DeterministicInterpretationResolver {
        host: &host,
        registry: &empty,
        project: &empty,
    };
    assert_eq!(
        missing.revalidate_decision(&context, &decision, time(100)),
        Err(InterpretationError::ChangedBasis)
    );
    let invalid = missing.resolve(&context, &q, time(100)).unwrap();
    assert_eq!(
        missing.revalidate_decision(&context, &invalid, time(100)),
        Err(InterpretationError::InvalidQuery)
    );
    host.changed.set(true);
    assert_eq!(
        resolver.revalidate_decision(&context, &decision, time(100)),
        Err(InterpretationError::ChangedBasis)
    );
    for entities in [vec![], vec!["A", "B"]] {
        let d = resolve(aliases(&entities), &entity_query(true));
        let json: serde_json::Value = serde_json::from_slice(&d.canonical_bytes()).unwrap();
        assert_eq!(json["version"], "1.0");
        assert_eq!(json["candidates"].as_array().unwrap().len(), entities.len());
    }
}

#[test]
fn frozen_alias_pronoun_temporal_and_scope_fixtures() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/interpretation-v1");
    if std::env::var_os("CG_UPDATE_INTERPRETATION_FIXTURES").is_some() {
        std::fs::create_dir_all(&root).unwrap();
        let mut foreign = aliases(&["A"]);
        if let InterpretationValue::Target(t) = &mut foreign.aliases[0].fact.value {
            t.reference.scope = ContextScopeId::new("foreign").unwrap();
        }
        for (name, context) in [
            ("alias", aliases(&["A", "B"])),
            (
                "pronoun",
                data(vec![fact(
                    "focus",
                    "A",
                    InterpretationTier::ConversationFocus,
                )]),
            ),
            ("temporal", temporal().0),
            ("scope-conflict", foreign),
        ] {
            std::fs::write(
                root.join(format!("{name}.json")),
                serde_json::to_vec_pretty(&context).unwrap(),
            )
            .unwrap();
        }
    }
    let host = Host::default();
    for (name, q, state) in [
        (
            "alias",
            entity_query(true),
            SemanticResolutionState::Ambiguous,
        ),
        ("pronoun", query(), SemanticResolutionState::Resolved),
        ("temporal", temporal().1, SemanticResolutionState::Resolved),
    ] {
        let context = InterpretationContext::admit_json(
            &std::fs::read(root.join(format!("{name}.json"))).unwrap(),
            &host,
            time(100),
        )
        .unwrap();
        let resolver = DeterministicInterpretationResolver {
            host: &host,
            registry: &Lookup::default(),
            project: &Lookup::default(),
        };
        assert_eq!(
            resolver
                .resolve(&context, &q, time(100))
                .unwrap()
                .resolution
                .state(),
            state
        );
    }
    assert_eq!(
        InterpretationContext::admit_json(
            &std::fs::read(root.join("scope-conflict.json")).unwrap(),
            &host,
            time(100)
        )
        .unwrap_err(),
        InterpretationError::Scope
    );
}

#[test]
fn literal_sets_and_permission_identities_are_canonical() {
    let mut f = fact("literal", "A", InterpretationTier::CurrentInput);
    f.value = InterpretationValue::Literal(TypedValue::Set(vec![
        TypedValue::Integer(2),
        TypedValue::Integer(1),
    ]));
    let q = InterpretationQuery {
        expected: ExpectedMeaning::Literal,
        ..query()
    };
    let first = resolve(data(vec![f.clone()]), &q);
    f.value = InterpretationValue::Literal(TypedValue::Set(vec![
        TypedValue::Integer(1),
        TypedValue::Integer(2),
    ]));
    assert_eq!(first, resolve(data(vec![f.clone()]), &q));
    f.value = InterpretationValue::Literal(TypedValue::Set(vec![
        TypedValue::Integer(1),
        TypedValue::Integer(1),
    ]));
    assert!(InterpretationContext::admit(data(vec![f]), &Host::default(), time(100)).is_err());
    let mut d = data(vec![]);
    let mut duplicate = d.permitted_sources[0].clone();
    duplicate.max_results += 1;
    d.permitted_sources.push(duplicate);
    assert_eq!(
        InterpretationContext::admit(d, &Host::default(), time(100)).unwrap_err(),
        InterpretationError::InvalidCapture
    );
}

#[test]
fn explicit_lookup_cannot_override_protected_meaning_or_use_it_as_fallback() {
    let mut d = aliases(&["A"]);
    let mut protected = fact("immutable", "B", InterpretationTier::TaskState);
    protected.protected = true;
    d.relevant_facts.push(protected);
    assert_eq!(
        resolve(d.clone(), &entity_query(true)).resolution.state(),
        SemanticResolutionState::Conflicting
    );
    d.aliases.clear();
    assert_eq!(
        resolve(d, &entity_query(true))
            .resolution
            .require_resolved(),
        Err("SEM_UNKNOWN_ENTITY")
    );
}

#[test]
fn new_target_invalidates_old_task_facts_without_hiding_explicit_required_pins() {
    let mut old = fact("old", "A", InterpretationTier::TaskState);
    old.subject = Some(target("A"));
    let mut fresh = fact("fresh", "B", InterpretationTier::Situation);
    fresh.subject = Some(target("B"));
    let q = InterpretationQuery {
        subject: Some(target("B")),
        ..query()
    };
    let decision = resolve(data(vec![old.clone(), fresh.clone()]), &q);
    assert_eq!(
        decision.resolution.require_resolved().unwrap(),
        &fresh.value
    );
    assert!(
        decision
            .trace
            .iter()
            .any(|t| t.rule == "target-invalidated")
    );
    old.required_pin = true;
    assert_eq!(
        resolve(data(vec![old, fresh]), &q).resolution.state(),
        SemanticResolutionState::Invalid
    );
}

#[path = "support/semantic_conformance.rs"]
mod semantic_conformance;
