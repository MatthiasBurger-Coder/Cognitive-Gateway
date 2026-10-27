use super::*;
use crate::mcp_client::mapping::*;
use gateway_application::connector_manifest::InventoryKind;
use serde_json::{Value, json};

#[derive(Clone)]
struct Collector(Acquisition);
impl AcquisitionPort for Collector {
    fn collect(&self, _: &ConnectorDispatch) -> Result<Acquisition, Error> {
        Ok(self.0.clone())
    }
}
// This fixture collector returns immutable metadata without provider I/O.
impl QualifiedAcquisitionPort for Collector {}
fn text(s: &str) -> NonEmptyText {
    NonEmptyText::new(s).unwrap()
}
fn dispatch() -> ConnectorDispatch {
    ConnectorDispatch {
        request: ConnectorRequest {
            target: ConnectorTarget {
                connector: text("fixture.connector"),
                scope: ContextScopeId::new("scope.one").unwrap(),
            },
            capability: CapabilityId::new("records.read").unwrap(),
            class: ConnectorOperationClass::Read,
            inventory_revision: ContentDigest::new("a".repeat(64)).unwrap(),
            arguments: BTreeMap::new(),
            task: TaskId::new("task.one").unwrap(),
            step: PlanStepId::new("step.one").unwrap(),
            execution: text("read.execution"),
            correlation: text("read.correlation"),
        },
        policy: PolicyId::new("policy.one").unwrap(),
        policy_revision: ContentDigest::new("b".repeat(64)).unwrap(),
        consent_receipt: None,
        budget_reservation: text("budget.one"),
        deadline: UnixTimestamp::new(200),
    }
}
fn acquisition() -> Acquisition {
    Acquisition {
        acquired_at: UnixTimestamp::new(100),
        observed_at: Some(UnixTimestamp::new(90)),
        evaluated_at: UnixTimestamp::new(100),
        max_age: FreshnessPolicy::new(20),
        authenticated_source: text("authenticated.collector"),
        sensitivity: SensitivityClass::Confidential,
        confidence: Confidence::Unknown,
        uncertainty: Uncertainty::Unknown,
        conflict: ConflictStatus::None,
        partial: false,
        basis: ConnectorObservationBasis::SourceRead,
        effect: ConnectorEffect::NoMutation,
        verifies: None,
        non_authoritative_raw: Some(ReferenceId::new("archive.raw").unwrap()),
    }
}
fn profile() -> NormalizationProfile {
    NormalizationProfile {
        source: SourceId::new("records.source").unwrap(),
        source_kind: SourceKind::Tool,
        object_field: "object".into(),
        revision_field: Some("revision".into()),
        digest_field: Some("digest".into()),
        fields: [
            (
                "title".into(),
                SubjectPath::new("issue.title".split('.')).unwrap(),
            ),
            (
                "open".into(),
                SubjectPath::new("issue.open".split('.')).unwrap(),
            ),
            (
                "count".into(),
                SubjectPath::new("issue.count".split('.')).unwrap(),
            ),
        ]
        .into(),
    }
}
fn mapping() -> MappingRecord {
    let schema = json!({"type":"object", "properties":{"object":{"type":"string"},"revision":{"type":"string"},"digest":{"type":"string"},
        "title":{"type":"string"},"open":{"type":"boolean"},"count":{"type":"integer"}},
        "required":["object","revision","digest","title","open","count"], "additionalProperties":false});
    MappingRecord {
        reference: "mapping.records.v1".into(),
        kind: InventoryKind::Tool,
        name: "read".into(),
        capability: dispatch().request.capability,
        source: Some(profile().source),
        class: OperationClass::Read,
        side_effects: SideEffects::None,
        idempotency: Idempotency::Idempotent,
        idempotency_key: None,
        retry_safety: RetrySafety::ReadOnly,
        consent: ConsentHint::Policy,
        result_schema: Some(schema),
    }
}
fn fixtures() -> Value {
    serde_json::from_slice(include_bytes!(
        "../../../../tests/fixtures/connector-result-v1/cases.json"
    ))
    .unwrap()
}
fn payload() -> Value {
    fixtures()["valid"].clone()
}

fn tool(value: Value) -> ProtocolResult {
    ProtocolResult::Tool {
        content: vec![],
        structured: Some(value),
        is_error: false,
    }
}
fn normalizer(a: Acquisition) -> CanonicalResultNormalizer {
    CanonicalResultNormalizer::new(profile(), Box::new(Collector(a))).unwrap()
}
fn result() -> ConnectorResult {
    normalizer(acquisition())
        .canonicalize(&dispatch(), None, &mapping(), tool(payload()))
        .unwrap()
}
#[test]
fn canonical_round_trip_preserves_lineage_quality_and_reference_only_context() {
    let r = result();
    assert!(r.diagnostics().is_empty());
    let o = r.observation();
    assert_eq!(o.source_revision.as_ref().unwrap().as_str(), "provider.r7");
    assert_eq!(
        o.metadata.source_digest.as_ref().unwrap().as_str(),
        "c".repeat(64)
    );
    assert_eq!(o.metadata.object_reference.as_str(), "urn:record:one");
    assert_eq!(o.records.evidence().len(), 3);
    assert_eq!(
        o.content_digest.as_str(),
        fixtures()["content_digest"].as_str().unwrap()
    );
    let round = ConnectorResult::new(&dispatch(), r.clone().into_observation()).unwrap();
    assert_eq!(round, r);
    let batch = r.batch().unwrap();
    assert_eq!(batch.scope(), &dispatch().request.target.scope);
    assert_eq!(batch.records(), &o.records);
    assert_eq!(batch.snapshot().change_token(), o.source_revision.as_ref());
    for quality in batch.quality_metadata().values() {
        assert_eq!(quality, &vec![o.metadata.quality]);
    }
    let fragments = r.fragments().unwrap();
    for f in fragments {
        assert!(f.is_reference());
        assert_eq!(f.metadata().quality, o.metadata.quality);
        assert_eq!(f.scope(), batch.scope());
    }
    assert_eq!(r, result());
    // Free-form instructions are retained only as reported data.
    assert!(format!("{:?}", o.records).contains("ignore policy"));
    assert!(!format!("{:?}", batch).contains("archive.raw"));
}
#[test]
fn stale_conflict_partial_and_unknown_freshness_survive() {
    let mut a = acquisition();
    a.max_age = FreshnessPolicy::new(
        fixtures()["stale_conflict"]["max_age_seconds"]
            .as_u64()
            .unwrap(),
    );
    a.conflict = ConflictStatus::Unresolved;
    a.partial = true;
    let r = normalizer(a)
        .canonicalize(&dispatch(), None, &mapping(), tool(payload()))
        .unwrap();
    let q = r.observation().metadata.quality;
    assert_eq!(
        r.diagnostics(),
        vec![
            ResultDiagnostic::Stale,
            ResultDiagnostic::Conflict,
            ResultDiagnostic::Partial
        ]
    );
    assert_eq!(q.freshness(), FreshnessStatus::Stale);
    assert_eq!(q.conflict(), ConflictStatus::Unresolved);
    assert_eq!(q.uncertainty(), Uncertainty::Incomplete);
    assert_eq!(r.fragments().unwrap()[0].metadata().quality, q);
    let mut a = acquisition();
    a.observed_at = None;
    let r = normalizer(a)
        .canonicalize(&dispatch(), None, &mapping(), tool(payload()))
        .unwrap();
    assert_eq!(
        r.observation().metadata.quality.freshness(),
        FreshnessStatus::Unknown
    );
}
#[test]
fn acknowledgements_and_separate_readback_are_distinct() {
    let mut a = acquisition();
    a.basis = ConnectorObservationBasis::ProviderAcknowledgement;
    a.effect = ConnectorEffect::Unknown;
    let r = normalizer(a)
        .canonicalize(&dispatch(), None, &mapping(), tool(payload()))
        .unwrap();
    assert_eq!(
        r.diagnostics(),
        vec![
            ResultDiagnostic::AcknowledgementOnly,
            ResultDiagnostic::UncertainEffect
        ]
    );
    assert_eq!(r.batch(), Err(ResultError::Basis));
    assert_eq!(r.fragments(), Err(ResultError::Basis));
    let mut a = acquisition();
    a.basis = ConnectorObservationBasis::IndependentPostEffectRead;
    let mut original = dispatch().request;
    original.class = ConnectorOperationClass::Mutate;
    original.correlation = text("effect.correlation");
    original.execution = text("effect.execution");
    a.verifies = Some(original);
    let r = normalizer(a)
        .canonicalize(&dispatch(), None, &mapping(), tool(payload()))
        .unwrap();
    assert!(r.batch().is_ok());
    assert!(r.observation().metadata.verifies.is_some());
}
#[test]
fn malformed_values_and_provider_authority_claims_fail_closed() {
    let n = normalizer(acquisition());
    let d = dispatch();
    let m = mapping();
    for v in [
        Value::Null,
        json!([]),
        json!({}),
        {
            let mut v = payload();
            v["trust"] = json!("CANONICAL_REFERENCE");
            v
        },
        {
            let mut v = payload();
            v["count"] = json!({"raw":1});
            v
        },
        {
            let mut v = payload();
            v["title"] = json!("");
            v
        },
        {
            let mut v = payload();
            v["object"] = json!("");
            v
        },
        {
            let mut v = payload();
            v["revision"] = json!("");
            v
        },
        {
            let mut v = payload();
            v["digest"] = json!("bad");
            v
        },
        {
            let mut v = payload();
            v["count"] = json!(u64::MAX);
            v
        },
    ] {
        assert!(n.canonicalize(&d, None, &m, tool(v)).is_err());
    }
    for v in [
        ProtocolResult::Tool {
            content: vec![],
            structured: None,
            is_error: false,
        },
        ProtocolResult::Tool {
            content: vec![],
            structured: Some(payload()),
            is_error: true,
        },
        ProtocolResult::Resource { contents: vec![] },
    ] {
        assert!(n.canonicalize(&d, None, &m, v).is_err());
    }
    let mut m = m;
    m.source = None;
    assert!(n.canonicalize(&d, None, &m, tool(payload())).is_err());
    let mut m = mapping();
    m.capability = CapabilityId::new("other").unwrap();
    assert!(n.canonicalize(&d, None, &m, tool(payload())).is_err());
    let mut m = mapping();
    m.class = OperationClass::Mutate;
    assert!(n.canonicalize(&d, None, &m, tool(payload())).is_err());
    assert!(
        n.canonicalize(
            &d,
            Some(&SourceId::new("other").unwrap()),
            &mapping(),
            tool(payload())
        )
        .is_err()
    );
    let mut m = mapping();
    m.result_schema = None;
    assert!(n.canonicalize(&d, None, &m, tool(payload())).is_err());
}
#[test]
fn invalid_profiles_times_and_bounds_are_rejected() {
    for p in [
        {
            let mut p = profile();
            p.fields.clear();
            p
        },
        {
            let mut p = profile();
            p.object_field = "title".into();
            p
        },
        {
            let mut p = profile();
            p.object_field = "".into();
            p
        },
        {
            let mut p = profile();
            p.fields.insert(
                "extra".into(),
                SubjectPath::new("issue.title".split('.')).unwrap(),
            );
            p
        },
    ] {
        assert!(CanonicalResultNormalizer::new(p, Box::new(Collector(acquisition()))).is_err());
    }
    for a in [
        {
            let mut a = acquisition();
            a.evaluated_at = UnixTimestamp::new(50);
            a
        },
        {
            let mut a = acquisition();
            a.observed_at = Some(UnixTimestamp::new(101));
            a
        },
        {
            let mut a = acquisition();
            a.acquired_at = UnixTimestamp::new(201);
            a.evaluated_at = UnixTimestamp::new(201);
            a
        },
    ] {
        assert!(
            normalizer(a)
                .canonicalize(&dispatch(), None, &mapping(), tool(payload()))
                .is_err()
        );
    }
    let mut v = payload();
    v["title"] = json!("x".repeat(262_144));
    assert_eq!(
        normalizer(acquisition()).canonicalize(&dispatch(), None, &mapping(), tool(v)),
        Err(Error::Limit)
    );
}
#[test]
fn public_records_are_revalidated_against_dispatch_lineage_and_basis() {
    let base = result().into_observation();
    let d = dispatch();
    let mut cases = vec![];
    macro_rules! bad {
        ($field:ident, $value:expr) => {{
            let mut o = base.clone();
            o.$field = $value;
            cases.push(o);
        }};
    }
    bad!(correlation, text("wrong"));
    bad!(
        inventory_revision,
        ContentDigest::new("d".repeat(64)).unwrap()
    );
    bad!(
        target,
        ConnectorTarget {
            connector: text("wrong"),
            scope: d.request.target.scope.clone()
        }
    );
    bad!(source, SourceId::new("wrong").unwrap());
    bad!(source_revision, None);
    bad!(acquired_at, UnixTimestamp::new(99));
    bad!(
        records,
        ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap()
    );
    for index in 0..10 {
        let mut o = base.clone();
        let m = &mut o.metadata;
        match index {
            0 => m.task = TaskId::new("wrong").unwrap(),
            1 => m.step = PlanStepId::new("wrong").unwrap(),
            2 => m.execution = text("wrong"),
            3 => m.object_reference = text("wrong"),
            4 => m.authenticated_source = text("wrong"),
            5 => m.source_kind = SourceKind::Model,
            6 => {
                m.quality = QualityMetadata::new(
                    TrustClass::CanonicalReference,
                    SensitivityClass::Public,
                    Confidence::Unknown,
                    FreshnessStatus::Fresh,
                    Uncertainty::None,
                )
            }
            7 => {
                m.partial = true;
                m.quality = QualityMetadata::new(
                    TrustClass::ObservedEvidence,
                    SensitivityClass::Public,
                    Confidence::Unknown,
                    FreshnessStatus::Fresh,
                    Uncertainty::None,
                )
            }
            8 => m.effect = ConnectorEffect::Acknowledged,
            _ => m.verifies = Some(d.request.clone()),
        }
        cases.push(o);
    }
    for o in cases {
        assert!(ConnectorResult::new(&d, o).is_err());
    }
    let mut mutate = d.clone();
    mutate.request.class = ConnectorOperationClass::Mutate;
    assert!(ConnectorResult::new(&mutate, base.clone()).is_err());
    for index in 0..8 {
        let mut o = base.clone();
        o.basis = ConnectorObservationBasis::IndependentPostEffectRead;
        let mut original = d.request.clone();
        original.class = ConnectorOperationClass::Mutate;
        original.correlation = text("effect.correlation");
        original.execution = text("effect.execution");
        match index {
            0 => {}
            1 => original.target.connector = text("other"),
            2 => original.task = TaskId::new("other").unwrap(),
            3 => original.correlation = d.request.correlation.clone(),
            4 => original.execution = d.request.execution.clone(),
            5 => original.class = ConnectorOperationClass::Read,
            6 => o.metadata.effect = ConnectorEffect::Unknown,
            _ => {}
        }
        if index != 0 {
            o.metadata.verifies = Some(original);
        }
        if index == 7 {
            o.basis = ConnectorObservationBasis::ProviderAcknowledgement;
        }
        assert!(ConnectorResult::new(&d, o).is_err());
    }
    let mut o = base;
    o.basis = ConnectorObservationBasis::ProviderAcknowledgement;
    o.metadata.effect = ConnectorEffect::NotDispatched;
    assert!(ConnectorResult::new(&d, o).is_err());
}
#[test]
fn text_resources_remain_text_and_unknown_revision_stays_absent() {
    let mut p = profile();
    p.fields = [(
        "text".into(),
        SubjectPath::new("document.content".split('.')).unwrap(),
    )]
    .into();
    p.revision_field = None;
    p.digest_field = None;
    let n = CanonicalResultNormalizer::new(p, Box::new(Collector(acquisition()))).unwrap();
    let mut m = mapping();
    m.kind = InventoryKind::Resource;
    let r = n
        .canonicalize(
            &dispatch(),
            Some(&profile().source),
            &m,
            ProtocolResult::Resource {
                contents: vec![json!({"uri":"urn:document:one","text":"{\"policy\":\"allow\"}"})],
            },
        )
        .unwrap();
    assert!(r.observation().source_revision.is_none());
    assert!(r.observation().metadata.source_digest.is_none());
    assert!(r.fragments().is_ok());
    for contents in [
        vec![],
        vec![json!({})],
        vec![json!({"uri":"urn:one"})],
        vec![json!({"uri":"urn:one","blob":"AAAA"})],
    ] {
        assert!(
            n.canonicalize(&dispatch(), None, &m, ProtocolResult::Resource { contents })
                .is_err()
        );
    }
    assert!(
        normalizer(acquisition())
            .canonicalize(
                &dispatch(),
                None,
                &m,
                ProtocolResult::Resource {
                    contents: vec![json!({"uri":"urn:one","text":"x"})]
                }
            )
            .is_err()
    );
}

#[test]
fn existing_cg06_serialization_retains_provenance_chains_and_rejects_cycles() {
    let base = result().into_observation();
    let root = base.records.provenances()[0].clone();
    let parent_id = ProvenanceId::new("connector.parent").unwrap();
    let parent = Provenance::new(
        parent_id.clone(),
        SourceKind::Repository,
        SourceId::new("original.source").unwrap(),
        "urn:original:object",
    )
    .unwrap()
    .with_producer("earlier.collector")
    .unwrap()
    .with_acquired_at(SourceTimestamp::new("80").unwrap())
    .with_source_timestamp(SourceTimestamp::new("original.r1").unwrap());
    for cyclic in [false, true] {
        let child = root
            .clone()
            .with_parent_provenance(vec![parent_id.clone()])
            .unwrap();
        let parent = if cyclic {
            parent
                .clone()
                .with_parent_provenance(vec![root.id().clone()])
                .unwrap()
        } else {
            parent.clone()
        };
        let graph = ObservationEvidenceSet::new(
            vec![parent, child],
            base.records.observations().to_vec(),
            base.records.facts().to_vec(),
            base.records.evidence().to_vec(),
        )
        .unwrap();
        let restored = ObservationEvidenceSet::from_json(&graph.to_json().unwrap()).unwrap();
        assert_eq!(graph, restored);
        let mut o = base.clone();
        o.records = restored;
        let normalized = ConnectorResult::new(&dispatch(), o);
        assert_eq!(normalized.is_err(), cyclic);
        if let Ok(r) = normalized {
            assert_eq!(r.batch().unwrap().records().provenances().len(), 2);
        }
    }
}

#[test]
fn competing_results_keep_distinct_lineage_in_the_existing_scoped_store() {
    use gateway_application::{
        InMemoryContextStore,
        ports::inbound::{ObservationEvidenceInputPort, ScopeLifecyclePort},
    };
    let mut a = acquisition();
    a.conflict = ConflictStatus::Unresolved;
    let n = normalizer(a);
    let first = n
        .canonicalize(&dispatch(), None, &mapping(), tool(payload()))
        .unwrap();
    let mut changed = payload();
    changed["open"] = json!(false);
    let second = n
        .canonicalize(&dispatch(), None, &mapping(), tool(changed))
        .unwrap();
    assert_ne!(
        first.observation().content_digest,
        second.observation().content_digest
    );
    let store = InMemoryContextStore::new();
    let scope = store.open_scope(dispatch().request.target.scope).unwrap();
    store
        .ingest_observations(&scope, first.batch().unwrap())
        .unwrap();
    store
        .ingest_observations(&scope, second.batch().unwrap())
        .unwrap();
    let snapshot = store.snapshot(&scope).unwrap();
    assert_eq!(snapshot.batches().len(), 2);
    assert_ne!(
        snapshot.batches()[0].records(),
        snapshot.batches()[1].records()
    );
    for batch in snapshot.batches() {
        for quality in batch.quality_metadata().values() {
            assert_eq!(quality[0].conflict(), ConflictStatus::Unresolved);
        }
    }
}

#[test]
fn optional_provider_revision_and_digest_remain_absent_when_not_supplied() {
    let mut m = mapping();
    m.result_schema.as_mut().unwrap()["required"] = json!(["object", "title", "open", "count"]);
    let mut v = payload();
    v.as_object_mut().unwrap().remove("revision");
    v.as_object_mut().unwrap().remove("digest");
    let r = normalizer(acquisition())
        .canonicalize(&dispatch(), None, &m, tool(v))
        .unwrap();
    assert!(r.observation().source_revision.is_none());
    assert!(r.observation().metadata.source_digest.is_none());
    assert!(r.batch().unwrap().snapshot().change_token().is_none());
}

#[test]
fn quality_requires_explicit_collector_assessment_and_partial_cannot_claim_certainty() {
    let unassessed = normalizer(acquisition())
        .canonicalize(&dispatch(), None, &mapping(), tool(payload()))
        .unwrap();
    assert_eq!(
        unassessed.observation().metadata.quality.confidence(),
        Confidence::Unknown
    );
    assert_eq!(
        unassessed.observation().metadata.quality.uncertainty(),
        Uncertainty::Unknown
    );
    for partial in [false, true] {
        let mut qualified = acquisition();
        qualified.confidence = Confidence::NotApplicable;
        qualified.uncertainty = Uncertainty::None;
        qualified.partial = partial;
        let result = normalizer(qualified)
            .canonicalize(&dispatch(), None, &mapping(), tool(payload()))
            .unwrap();
        let quality = result.observation().metadata.quality;
        assert_eq!(quality.confidence(), Confidence::NotApplicable);
        assert_eq!(
            quality.uncertainty(),
            if partial {
                Uncertainty::Incomplete
            } else {
                Uncertainty::None
            }
        );
        let batch = result.batch().unwrap();
        assert!(
            batch
                .quality_metadata()
                .values()
                .all(|items| items == &[quality])
        );
        assert!(
            result
                .fragments()
                .unwrap()
                .iter()
                .all(|fragment| fragment.metadata().quality == quality)
        );
    }
}

#[test]
fn scalar_accounting_requires_qualified_collector_and_bounds_canonical_typed_values() {
    assert!(matches!(
        normalizer(acquisition()).resource_bounds(),
        Err(Error::Unsupported)
    ));
    let normalizer =
        CanonicalResultNormalizer::new_qualified(profile(), Box::new(Collector(acquisition())))
            .unwrap();
    let bound = normalizer.resource_bounds().unwrap();
    assert_eq!(bound.source(), &profile().source);
    assert_eq!(
        bound.subjects(),
        &profile().fields.values().cloned().collect()
    );
    assert_eq!(bound.value_bytes(), 262_144 + 32 * 3);
    let mut input = payload();
    input["title"] = json!("é\\\"".repeat(2_000));
    let result = normalizer
        .canonicalize(&dispatch(), None, &mapping(), tool(input))
        .unwrap();
    let actual: usize = result
        .batch()
        .unwrap()
        .records()
        .facts()
        .iter()
        .map(|fact| serde_json::to_vec(fact.value()).unwrap().len())
        .sum();
    assert!(actual as u64 <= bound.value_bytes());
    let mut oversized = payload();
    oversized["title"] = json!("x".repeat(262_145));
    assert!(matches!(
        normalizer.canonicalize(&dispatch(), None, &mapping(), tool(oversized)),
        Err(Error::Limit)
    ));
}

#[test]
fn exact_argument_routes_select_matching_native_subjects_and_resource_bounds() {
    let mut first = profile();
    first.fields = BTreeMap::from([(
        "title".into(),
        SubjectPath::new(["object", "one", "title"]).unwrap(),
    )]);
    let mut second = profile();
    second.fields = BTreeMap::from([
        (
            "open".into(),
            SubjectPath::new(["object", "two", "open"]).unwrap(),
        ),
        (
            "count".into(),
            SubjectPath::new(["object", "two", "count"]).unwrap(),
        ),
    ]);
    let args = |id: &str| BTreeMap::from([(text("query"), TypedValue::string(id).unwrap())]);
    let cap = dispatch().request.capability;
    let source = first.source.clone();
    let normalizer = CanonicalResultNormalizer::new_qualified_routes(
        vec![
            NormalizationRoute {
                capability: cap.clone(),
                source: source.clone(),
                arguments: args("one"),
                profile: first,
            },
            NormalizationRoute {
                capability: cap.clone(),
                source: source.clone(),
                arguments: args("two"),
                profile: second,
            },
        ],
        Box::new(Collector(acquisition())),
    )
    .unwrap();
    assert!(
        normalizer.resource_bounds().is_err(),
        "routed proof must never fall back to first profile"
    );
    for (id, expected) in [
        ("one", vec!["object.one.title"]),
        ("two", vec!["object.two.open", "object.two.count"]),
    ] {
        let mut d = dispatch();
        d.request.arguments = args(id);
        let mut value = payload();
        value["object"] = json!(id);
        let result = normalizer
            .canonicalize(&d, None, &mapping(), tool(value))
            .unwrap();
        let observed = result.observation();
        let bounds = normalizer
            .resource_bounds_for_route(&cap, &d.request.arguments, &source)
            .unwrap();
        assert_eq!(observed.records.facts().len(), expected.len());
        assert_eq!(
            observed
                .records
                .facts()
                .iter()
                .map(|f| f.subject().clone())
                .collect::<BTreeSet<_>>(),
            *bounds.subjects()
        );
        assert_eq!(
            *bounds.subjects(),
            expected
                .into_iter()
                .map(|s| SubjectPath::new(s.split('.')).unwrap())
                .collect()
        );
    }
    assert!(
        normalizer
            .resource_bounds_for_route(&cap, &args("unknown"), &source)
            .is_err()
    );
    assert!(
        normalizer
            .resource_bounds_for_route(
                &cap,
                &args("one"),
                &SourceId::new("foreign.source").unwrap()
            )
            .is_err()
    );
    let mut unknown = dispatch();
    unknown.request.arguments = args("unknown");
    assert!(matches!(
        normalizer.canonicalize(&unknown, None, &mapping(), tool(payload())),
        Err(Error::Schema)
    ));
}

#[test]
fn normalization_route_construction_rejects_ambiguous_or_foreign_selectors() {
    let cap = dispatch().request.capability;
    let source = profile().source;
    let route = |id: &str| NormalizationRoute {
        capability: cap.clone(),
        source: source.clone(),
        arguments: BTreeMap::from([(text("query"), TypedValue::string(id).unwrap())]),
        profile: profile(),
    };
    assert!(matches!(
        CanonicalResultNormalizer::new_qualified_routes(
            vec![route("same"), route("same")],
            Box::new(Collector(acquisition()))
        ),
        Err(Error::Configuration)
    ));
    let mut foreign = route("two");
    foreign.capability = CapabilityId::new("foreign.capability").unwrap();
    assert!(matches!(
        CanonicalResultNormalizer::new_qualified_routes(
            vec![route("one"), foreign],
            Box::new(Collector(acquisition()))
        ),
        Err(Error::Configuration)
    ));
    let mut foreign = route("two");
    foreign.profile.source = SourceId::new("foreign.source").unwrap();
    assert!(matches!(
        CanonicalResultNormalizer::new_qualified_routes(
            vec![route("one"), foreign],
            Box::new(Collector(acquisition()))
        ),
        Err(Error::Configuration)
    ));
}
