use super::*;
use crate::{
    mcp_client::{
        CallContext, Limits,
        normalization::*,
        transport::{Transport, TransportFactory},
    },
    session_resource_test_support::with_preparation_resources,
};
use gateway_application::{connector_manifest::*, sessions::SessionResourcePlan};
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
fn text(v: &str) -> NonEmptyText {
    NonEmptyText::new(v).unwrap()
}
struct Collector;
impl AcquisitionPort for Collector {
    fn collect(&self, _: &ConnectorDispatch) -> Result<Acquisition, Error> {
        Ok(Acquisition {
            acquired_at: UnixTimestamp::new(10),
            observed_at: Some(UnixTimestamp::new(10)),
            evaluated_at: UnixTimestamp::new(10),
            max_age: FreshnessPolicy::new(10),
            authenticated_source: text("bounded.local.fixture"),
            sensitivity: SensitivityClass::Public,
            confidence: Confidence::NotApplicable,
            uncertainty: Uncertainty::None,
            conflict: ConflictStatus::None,
            partial: false,
            basis: ConnectorObservationBasis::SourceRead,
            effect: ConnectorEffect::NoMutation,
            verifies: None,
            non_authoritative_raw: None,
        })
    }
}
impl QualifiedAcquisitionPort for Collector {}
struct Membership;
impl ScalarResponseAdmission for Membership {
    fn validate(
        &self,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
        _: &ProtocolResult,
    ) -> Result<(), ConnectorFailure> {
        Ok(())
    }
}
struct Denied;
impl ConnectorAdmissionPort for Denied {
    fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        Err(ConnectorFailure {
            code: ConnectorFailureCode::PermissionDenied,
            correlation: text("denied"),
            effect: ConnectorEffect::NotDispatched,
        })
    }
    fn authorize(
        &self,
        r: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorDispatch, ConnectorFailure> {
        Err(ConnectorFailure {
            code: ConnectorFailureCode::PermissionDenied,
            correlation: r.correlation.clone(),
            effect: ConnectorEffect::NotDispatched,
        })
    }
    fn revalidate(
        &self,
        d: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        self.admit_target(&d.request.target)
    }
}
struct NoIo(Arc<AtomicUsize>);
impl TransportFactory for NoIo {
    fn open(&self, _: &CallContext, _: &Limits) -> Result<Box<dyn Transport>, Error> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(Error::Unavailable)
    }
}
fn document() -> MappingDocument {
    let mut v: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../tests/fixtures/connector-mapping-v1/mapping.json"
    ))
    .unwrap();
    v["records"][0]["source"] = json!("records.tool");
    v["records"][0]["result_schema"]["properties"]["object"] = json!({"type":"string"});
    v["records"][0]["result_schema"]["properties"]["v1"] = json!({"type":"string"});
    v["records"][0]["result_schema"]["required"] = json!(["value", "object"]);
    MappingDocument::from_json(&serde_json::to_vec(&v).unwrap()).unwrap()
}
fn profile(source: SourceId, fields: usize) -> NormalizationProfile {
    NormalizationProfile {
        source,
        source_kind: SourceKind::Tool,
        object_field: "object".into(),
        revision_field: None,
        digest_field: None,
        fields: (0..fields)
            .map(|n| {
                (
                    if n == 0 {
                        "value".into()
                    } else {
                        format!("v{n}")
                    },
                    SubjectPath::new([format!("field{n}")]).unwrap(),
                )
            })
            .collect(),
    }
}
fn normalizers(qualified: bool) -> BTreeMap<CapabilityId, CanonicalResultNormalizer> {
    document()
        .records()
        .iter()
        .map(|r| {
            let p = profile(r.source.clone().unwrap(), 1);
            (
                r.capability.clone(),
                if qualified {
                    CanonicalResultNormalizer::new_qualified(p, Box::new(Collector)).unwrap()
                } else {
                    CanonicalResultNormalizer::new(p, Box::new(Collector)).unwrap()
                },
            )
        })
        .collect()
}
fn target(scope: ContextScopeId) -> ConnectorTarget {
    ConnectorTarget {
        connector: text("example.records"),
        scope,
    }
}
pub(in crate::mcp_client) fn host(
    allocation: super::super::session_budget::ReadonlyAllocationView,
    qualified: bool,
    limits: Limits,
    io: Arc<AtomicUsize>,
) -> Result<QualifiedScalarHost, Error> {
    qualified_scalar_host(
        QualifiedScalarConfig {
            allocation: allocation.clone(),
            host: HostConfig {
                target: target(allocation.allocation().binding().scope.clone()),
                manifest: ConnectorManifest::from_json(include_bytes!(
                    "../../../../tests/fixtures/connector-mapping-v1/manifest.json"
                ))
                .unwrap(),
                profile: ConnectorCompatibilityProfile {
                    gateway: ConnectorVersion {
                        major: 0,
                        minor: 1,
                        patch: 0,
                    },
                    transports: BTreeSet::from([ConnectorTransport::Stdio]),
                },
                limits,
            },
            document: document(),
            normalizers: normalizers(qualified),
            tariff: FixedLocalTariff::operator_fixed_no_embedding(
                ReferenceId::new("test-credit").unwrap(),
                2,
                3,
            ),
        },
        Box::new(NoIo(io)),
        Arc::new(Denied),
        Arc::new(Membership),
        Arc::new(()),
    )
}

fn request(a: &SessionResourceAllocation) -> ConnectorRequest {
    ConnectorRequest {
        target: target(a.binding().scope.clone()),
        capability: CapabilityId::new("records.read").unwrap(),
        class: ConnectorOperationClass::Read,
        inventory_revision: ContentDigest::new("a".repeat(64)).unwrap(),
        arguments: [(
            text("selector"),
            TypedValue::string("quotes\"\\ ä🔑").unwrap(),
        )]
        .into(),
        task: TaskId::new("fixture-task").unwrap(),
        step: a.binding().step.clone(),
        execution: text("exec"),
        correlation: text("corr"),
    }
}
fn amounts() -> SessionResourceAmounts {
    SessionResourceAmounts {
        connector_calls: 5,
        retrieval_rounds: 5,
        retrieval_results: 20,
        retrieval_tokens: 3_000_000,
        cost_microunits: 20,
        ..Default::default()
    }
}
#[test]
fn native_scalar_serialization_envelopes_and_maximum_profile_prove_output_bound() {
    for raw in [
        json!(true),
        json!(i64::MIN),
        json!("\"\\är🔑"),
        json!("🔑".repeat(16_384)),
    ] {
        let typed = if let Some(s) = raw.as_str() {
            TypedValue::string(s).unwrap()
        } else if let Some(b) = raw.as_bool() {
            TypedValue::Boolean(b)
        } else {
            TypedValue::Integer(raw.as_i64().unwrap())
        };
        assert!(
            serde_json::to_vec(&typed).unwrap().len()
                <= serde_json::to_vec(&raw).unwrap().len() + 32
        );
    }
    let n = CanonicalResultNormalizer::new_qualified(
        profile(SourceId::new("s").unwrap(), 256),
        Box::new(Collector),
    )
    .unwrap();
    assert_eq!(
        n.resource_bounds().unwrap().value_bytes(),
        262_144 + 32 * 256
    );
    assert!(
        CanonicalResultNormalizer::new_qualified(
            profile(SourceId::new("s").unwrap(), 257),
            Box::new(Collector)
        )
        .is_err()
    );
    let unassessed = CanonicalResultNormalizer::new(
        profile(SourceId::new("s").unwrap(), 1),
        Box::new(Collector),
    )
    .unwrap();
    assert!(matches!(
        unassessed.resource_bounds(),
        Err(Error::Unsupported)
    ));
}
#[test]
fn sealed_factory_rejects_unknown_collectors_and_invalid_actual_limits_without_io() {
    with_preparation_resources(
        ContextScopeId::new("scope").unwrap(),
        SessionResourcePlan {
            preparation: amounts(),
            ..Default::default()
        },
        |resources, _, authority| {
            let io = Arc::new(AtomicUsize::new(0));
            super::super::session_budget::SessionConnectorRun::with_factory(
                resources.allocate(amounts()).unwrap(),
                Arc::new(authority.clone()),
                CallContext::new(text("factory"), Instant::now() + Duration::from_secs(2)),
                |view| {
                    assert!(matches!(
                        host(view.clone(), false, Limits::default(), io.clone()),
                        Err(Error::Configuration)
                    ));
                    assert!(matches!(
                        host(
                            view.clone(),
                            true,
                            Limits {
                                frame_bytes: 0,
                                ..Limits::default()
                            },
                            io.clone()
                        ),
                        Err(Error::Configuration)
                    ));
                    let parts = host(view, true, Limits::default(), io.clone()).unwrap();
                    Ok(
                        super::super::session_budget::SessionConnectorComponents::from_qualified(
                            parts,
                            Arc::new(authority.clone()),
                        ),
                    )
                },
            )
            .unwrap();
            assert_eq!(io.load(Ordering::SeqCst), 0);
        },
    );
}
#[test]
fn actual_allocation_quotes_count_input_plus_all_values_and_refuse_repricing_and_foreign_source() {
    with_preparation_resources(
        ContextScopeId::new("scope").unwrap(),
        SessionResourcePlan {
            preparation: amounts().checked_add(&amounts()).unwrap(),
            ..Default::default()
        },
        |resources, _, authority| {
            let other = resources.allocate(amounts()).unwrap();
            let a = resources.allocate(amounts()).unwrap();
            super::super::session_budget::SessionConnectorRun::with_factory(a,Arc::new(authority.clone()),CallContext::new(text("factory"),Instant::now()+Duration::from_secs(3)),|view| {
            let a=view.allocation();
            let io = Arc::new(AtomicUsize::new(0));
            let parts = host(
                view.clone(),
                true,
                Limits::default(),
                io.clone(),
            )
            .unwrap();
            let r = request(a);
            let s = SourceId::new("records.tool").unwrap();
            let q = parts.quotes.quote_request(a, &r, Some(&s)).unwrap();
            assert_eq!(
                q.amounts.retrieval_tokens,
                argument_bytes(&r).unwrap() + 262_144 + 32
            );
            assert_eq!(q.amounts.retrieval_results, 1);
            assert_eq!(q.amounts.cost_microunits, 3);
            let mut longer = r.clone();
            longer.arguments.insert(
                text("another-query"),
                TypedValue::string("x".repeat(1024)).unwrap(),
            );
            assert!(
                parts
                    .quotes
                    .quote_request(a, &longer, Some(&s))
                    .unwrap()
                    .amounts
                    .retrieval_tokens
                    > q.amounts.retrieval_tokens
            );
            assert!(
                parts
                    .quotes
                    .quote_request(a, &r, Some(&SourceId::new("foreign").unwrap()))
                    .is_err()
            );
            let d = ConnectorDispatch {
                request: r.clone(),
                policy: PolicyId::new("p").unwrap(),
                policy_revision: ContentDigest::new("b".repeat(64)).unwrap(),
                consent_receipt: None,
                budget_reservation: text(a.reservation_id().as_str()),
                deadline: UnixTimestamp::new(100),
            };
            let n = normalizers(true).remove(&r.capability).unwrap();
            let map = document();
            let record = map
                .records()
                .iter()
                .find(|m| m.capability == r.capability)
                .unwrap();
            let observation = n
                .canonicalize(
                    &d,
                    Some(&s),
                    record,
                    ProtocolResult::Tool {
                        content: vec![],
                        structured: Some(json!({"object":"o","value":"\"\\ä🔑"})),
                        is_error: false,
                    },
                )
                .unwrap()
                .into_observation();
            parts
                .quotes
                .validate_observation(a, &q, &r, Some(&s), &observation)
                .unwrap();
            let profile = profile(s.clone(), 256);
            let mut payload = serde_json::Map::from_iter([("object".into(), json!("o"))]);
            let mut properties =
                serde_json::Map::from_iter([("object".into(), json!({"type":"string"}))]);
            for field in profile.fields.keys() {
                payload.insert(field.clone(), json!("\\\"ä".repeat(160)));
                properties.insert(field.clone(), json!({"type":"string"}));
            }
            let input_size = serde_json::to_vec(&payload).unwrap().len();
            assert!(input_size < 262_144);
            let mut mapped = record.clone();
            mapped.result_schema = Some(
                json!({"type":"object","properties":properties,"required":payload.keys().collect::<Vec<_>>(),"additionalProperties":false}),
            );
            let wide =
                CanonicalResultNormalizer::new_qualified(profile, Box::new(Collector)).unwrap();
            let normalized = wide
                .canonicalize(
                    &d,
                    Some(&s),
                    &mapped,
                    ProtocolResult::Tool {
                        content: vec![],
                        structured: Some(json!(payload)),
                        is_error: false,
                    },
                )
                .unwrap()
                .into_observation();
            assert_eq!(normalized.records.facts().len(), 256);
            assert!(
                value_bytes(normalized.records.facts().iter().map(Fact::value)).unwrap()
                    <= wide.resource_bounds().unwrap().value_bytes()
            );
            let discovery = parts.quotes.quote_discovery(a, &r.target).unwrap();
            let mut inventory = ConnectorInventory {
                target: r.target.clone(),
                revision: r.inventory_revision.clone(),
                capabilities: document()
                    .records()
                    .iter()
                    .map(|m| ConnectorCapability {
                        capability: m.capability.clone(),
                        class: m.class.operation_class(),
                        argument_schema_digest: r.inventory_revision.clone(),
                        result_schema_digest: r.inventory_revision.clone(),
                    })
                    .collect(),
                resources: document()
                    .records()
                    .iter()
                    .filter_map(|m| m.source.clone())
                    .collect(),
            };
            inventory.resources.sort();
            assert_eq!(discovery.amounts.retrieval_results, 4);
            parts
                .quotes
                .validate_inventory(a, &discovery, &inventory)
                .unwrap();
            inventory.capabilities[1] = inventory.capabilities[0].clone();
            assert!(
                parts
                    .quotes
                    .validate_inventory(a, &discovery, &inventory)
                    .is_err()
            );
            let mut forged = q.clone();
            forged.amounts.retrieval_tokens = 100;
            assert!(
                parts
                    .quotes
                    .validate_observation(a, &forged, &r, Some(&s), &observation)
                    .is_err()
            );
            let mut foreign = r.clone();
            foreign.target.scope = ContextScopeId::new("foreign").unwrap();
            assert!(parts.quotes.quote_request(a, &foreign, Some(&s)).is_err());
            assert_eq!(parts.host.configured_target(), &r.target);
            assert_eq!(parts.quotes.quote_request(&other,&r,Some(&s)).unwrap_err(),SessionError::AuthorityDenied);
            assert_eq!(parts.quotes.quote_discovery(&other,&r.target).unwrap_err(),SessionError::AuthorityDenied);
            assert_eq!(parts.quotes.validate_observation(&other,&q,&r,Some(&s),&observation).unwrap_err(),SessionError::AuthorityDenied);
            assert_eq!(parts.quotes.validate_inventory(&other,&discovery,&inventory).unwrap_err(),SessionError::AuthorityDenied);
            assert_eq!(io.load(Ordering::SeqCst), 0);
            Ok(super::super::session_budget::SessionConnectorComponents::from_qualified(parts,Arc::new(authority.clone())))
            }).unwrap();
        },
    );
}
#[test]
fn insufficient_real_slice_stops_before_host_or_transport_instead_of_accepting_numeric_quote() {
    let mut low = amounts();
    low.retrieval_tokens = 100;
    with_preparation_resources(
        ContextScopeId::new("scope").unwrap(),
        SessionResourcePlan {
            preparation: low,
            ..Default::default()
        },
        |resources, _, authority| {
            let a = resources.allocate(low).unwrap();
            let r = request(&a);
            authority.allow_connector_request(&r, None);
            let io = Arc::new(AtomicUsize::new(0));
            let run = super::super::session_budget::SessionConnectorRun::with_factory(
                a,
                Arc::new(authority.clone()),
                CallContext::new(text("corr"), Instant::now() + Duration::from_secs(2)),
                |view| {
                    let parts = host(view, true, Limits::default(), io.clone()).unwrap();
                    Ok(
                        super::super::session_budget::SessionConnectorComponents::from_qualified(
                            parts,
                            Arc::new(authority.clone()),
                        ),
                    )
                },
            )
            .unwrap();
            let failure = run.invoke(&r).unwrap_err();
            assert_eq!(failure.code, ConnectorFailureCode::BudgetExhausted);
            assert_eq!(failure.effect, ConnectorEffect::NotDispatched);
            assert_eq!(io.load(Ordering::SeqCst), 0);
        },
    );
}

#[test]
fn consuming_bundle_keeps_same_view_different_normalizers_paired_before_io() {
    let mut low = amounts();
    // Empty native arguments + single-field ceiling fits; two fields do not.
    low.retrieval_tokens = 2 + 262_144 + 32;
    with_preparation_resources(
        ContextScopeId::new("scope").unwrap(),
        SessionResourcePlan {
            preparation: low,
            ..Default::default()
        },
        |resources, _, authority| {
            let allocation = resources.allocate(low).unwrap();
            let mut r = request(&allocation);
            r.arguments.clear();
            authority.allow_connector_request(&r, None);
            let io = Arc::new(AtomicUsize::new(0));
            let run = super::super::session_budget::SessionConnectorRun::with_factory(
                allocation,
                Arc::new(authority.clone()),
                CallContext::new(text("corr"), Instant::now() + Duration::from_secs(3)),
                |view| {
                    let a = host(view.clone(), true, Limits::default(), io.clone()).unwrap();
                    let mut ns = normalizers(true);
                    let two = CanonicalResultNormalizer::new_qualified(
                        profile(SourceId::new("records.tool").unwrap(), 2),
                        Box::new(Collector),
                    )
                    .unwrap();
                    // Both profiles have a genuinely satisfiable native mapping;
                    // the second output is not an unreachable schema branch.
                    let dispatch = ConnectorDispatch {
                        request: r.clone(),
                        policy: PolicyId::new("p").unwrap(),
                        policy_revision: ContentDigest::new("b".repeat(64)).unwrap(),
                        consent_receipt: None,
                        budget_reservation: text(view.allocation().reservation_id().as_str()),
                        deadline: UnixTimestamp::new(100),
                    };
                    let doc = document();
                    let mapping = doc
                        .records()
                        .iter()
                        .find(|m| m.capability == r.capability)
                        .unwrap();
                    assert_eq!(
                        two.canonicalize(
                            &dispatch,
                            None,
                            mapping,
                            ProtocolResult::Tool {
                                content: vec![],
                                structured: Some(json!({"object":"o","value":"one","v1":"two"})),
                                is_error: false
                            }
                        )
                        .unwrap()
                        .observation()
                        .records
                        .facts()
                        .len(),
                        2
                    );
                    ns.insert(r.capability.clone(), two);
                    let b = qualified_scalar_host(
                        QualifiedScalarConfig {
                            allocation: view.clone(),
                            host: HostConfig {
                                target: r.target.clone(),
                                manifest: ConnectorManifest::from_json(include_bytes!(
                                    "../../../../tests/fixtures/connector-mapping-v1/manifest.json"
                                ))
                                .unwrap(),
                                profile: ConnectorCompatibilityProfile {
                                    gateway: ConnectorVersion {
                                        major: 0,
                                        minor: 1,
                                        patch: 0,
                                    },
                                    transports: BTreeSet::from([ConnectorTransport::Stdio]),
                                },
                                limits: Limits::default(),
                            },
                            document: document(),
                            normalizers: ns,
                            tariff: FixedLocalTariff::operator_fixed_no_embedding(
                                ReferenceId::new("test-credit").unwrap(),
                                2,
                                3,
                            ),
                        },
                        Box::new(NoIo(io.clone())),
                        Arc::new(Denied),
                        Arc::new(Membership),
                        Arc::new(()),
                    )
                    .unwrap();
                    assert_eq!(
                        a.quotes
                            .quote_request(view.allocation(), &r, None)
                            .unwrap()
                            .amounts
                            .retrieval_tokens,
                        low.retrieval_tokens
                    );
                    assert_eq!(
                        b.quotes
                            .quote_request(view.allocation(), &r, None)
                            .unwrap()
                            .amounts
                            .retrieval_tokens,
                        low.retrieval_tokens + 32
                    );
                    // Production callers can consume B only as an opaque whole. They
                    // cannot extract A's cheaper quotes or replace B's normalizer host.
                    Ok(
                        super::super::session_budget::SessionConnectorComponents::from_qualified(
                            b,
                            Arc::new(authority.clone()),
                        ),
                    )
                },
            )
            .unwrap();
            assert_eq!(
                run.invoke(&r).unwrap_err().code,
                ConnectorFailureCode::BudgetExhausted
            );
            assert_eq!(io.load(Ordering::SeqCst), 0);
        },
    );
}
struct DiscoveryOnly(ConnectorTarget);
impl ConnectorAdmissionPort for DiscoveryOnly {
    fn admit_target(&self, t: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        if t == &self.0 {
            Ok(())
        } else {
            Denied.admit_target(t)
        }
    }
    fn authorize(
        &self,
        r: &ConnectorRequest,
        s: Option<&SourceId>,
    ) -> Result<ConnectorDispatch, ConnectorFailure> {
        Denied.authorize(r, s)
    }
    fn revalidate(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        Denied.revalidate(d, s)
    }
}
struct PeerFactory(Arc<AtomicUsize>);
struct Peer {
    queue: std::collections::VecDeque<Vec<u8>>,
    sends: Arc<AtomicUsize>,
}
impl TransportFactory for PeerFactory {
    fn open(&self, c: &CallContext, _: &Limits) -> Result<Box<dyn Transport>, Error> {
        c.check()?;
        Ok(Box::new(Peer {
            queue: Default::default(),
            sends: self.0.clone(),
        }))
    }
}
impl Transport for Peer {
    fn send(&mut self, frame: &[u8], c: &CallContext) -> Result<(), Error> {
        c.check()?;
        self.sends.fetch_add(1, Ordering::SeqCst);
        let v: serde_json::Value = serde_json::from_slice(frame).unwrap();
        let Some(id) = v.get("id") else { return Ok(()) };
        let wire: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../../tests/fixtures/connector-mapping-v1/discovery.json"
        ))
        .unwrap();
        let result = match v["method"].as_str().unwrap() {
            "initialize" => {
                json!({"protocolVersion":super::super::MCP_VERSION,"capabilities":{"tools":{},"resources":{},"prompts":{}},"serverInfo":{"name":"records","version":"1.0.0"}})
            }
            "tools/list" => json!({"tools":wire["tools"]}),
            "resources/list" => json!({"resources":wire["resources"]}),
            "resources/templates/list" => json!({"resourceTemplates":wire["templates"]}),
            "prompts/list" => json!({"prompts":wire["prompts"]}),
            _ => panic!("unexpected provider method"),
        };
        self.queue.push_back(
            serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"result":result})).unwrap(),
        );
        Ok(())
    }
    fn receive(&mut self, c: &CallContext) -> Result<Vec<u8>, Error> {
        c.check()?;
        self.queue.pop_front().ok_or(Error::Timeout)
    }
    fn receive_with_wait_deadline(
        &mut self,
        c: &CallContext,
        _: Instant,
    ) -> Result<Vec<u8>, Error> {
        self.receive(c)
    }
    fn close(&mut self) {}
}
#[test]
fn metadata_retains_only_actual_budgeted_checked_snapshot_and_never_keeps_host_alive() {
    with_preparation_resources(
        ContextScopeId::new("scope").unwrap(),
        SessionResourcePlan {
            preparation: amounts(),
            ..Default::default()
        },
        |resources, _, authority| {
            let allocation = resources.allocate(amounts()).unwrap();
            let target = target(allocation.binding().scope.clone());
            authority.allow_connector_target(&target);
            let sends = Arc::new(AtomicUsize::new(0));
            let mut witness = None;
            let run = super::super::session_budget::SessionConnectorRun::with_factory(
                allocation,
                Arc::new(authority.clone()),
                CallContext::new(text("discovery"), Instant::now() + Duration::from_secs(3)),
                |view| {
                    let mut definition: serde_json::Value = serde_json::from_slice(include_bytes!(
                        "../../../../tests/fixtures/connector-mapping-v1/manifest.json"
                    ))
                    .unwrap();
                    definition["manifest"]["inventory"][0]["source_mapping_ref"] =
                        json!("mapping.records.read.v1");
                    let manifest = ConnectorManifest::from_definition_json(
                        &serde_json::to_vec(&definition["manifest"]).unwrap(),
                    )
                    .unwrap();
                    let bundle = qualified_scalar_host(
                        QualifiedScalarConfig {
                            allocation: view,
                            host: HostConfig {
                                target: target.clone(),
                                manifest,
                                profile: ConnectorCompatibilityProfile {
                                    gateway: ConnectorVersion {
                                        major: 0,
                                        minor: 1,
                                        patch: 0,
                                    },
                                    transports: BTreeSet::from([ConnectorTransport::Stdio]),
                                },
                                limits: Limits::default(),
                            },
                            document: document(),
                            normalizers: normalizers(true),
                            tariff: FixedLocalTariff::operator_fixed_no_embedding(
                                ReferenceId::new("test-credit").unwrap(),
                                2,
                                3,
                            ),
                        },
                        Box::new(PeerFactory(sends.clone())),
                        Arc::new(DiscoveryOnly(target.clone())),
                        Arc::new(Membership),
                        Arc::new(()),
                    )
                    .unwrap();
                    let metadata = bundle.metadata();
                    let synthetic = ConnectorInventory {
                        target: target.clone(),
                        revision: ContentDigest::new("a".repeat(64)).unwrap(),
                        capabilities: vec![],
                        resources: vec![],
                    };
                    assert!(matches!(
                        metadata.snapshot_for(&synthetic),
                        Err(Error::InventoryChanged)
                    ));
                    assert!(
                        metadata
                            .tool_schema(
                                &target,
                                &CapabilityId::new("records.read").unwrap(),
                                &synthetic.revision
                            )
                            .is_err()
                    );
                    assert_eq!(sends.load(Ordering::SeqCst), 0);
                    witness = Some(metadata);
                    Ok(
                        super::super::session_budget::SessionConnectorComponents::from_qualified(
                            bundle,
                            Arc::new(authority.clone()),
                        ),
                    )
                },
            )
            .unwrap();
            let metadata = witness.take().unwrap();
            let summary = run.discover(&target).unwrap();
            let count = sends.load(Ordering::SeqCst);
            assert!(count >= 5);
            let snapshot = metadata.snapshot_for(&summary).unwrap();
            assert_eq!(snapshot.inventory(), &summary);
            assert_eq!(snapshot.entries().len(), 2);
            assert!(!snapshot.canonical_json().is_empty());
            let mut altered = summary.clone();
            altered.resources.clear();
            assert!(matches!(
                metadata.snapshot_for(&altered),
                Err(Error::InventoryChanged)
            ));
            let mut foreign = summary.clone();
            foreign.target.scope = ContextScopeId::new("elsewhere").unwrap();
            assert!(metadata.snapshot_for(&foreign).is_err());
            let capability = CapabilityId::new("records.read").unwrap();
            assert_eq!(
                metadata
                    .tool_schema(&target, &capability, &summary.revision)
                    .unwrap()
                    .inventory_revision,
                summary.revision
            );
            assert_eq!(sends.load(Ordering::SeqCst), count);
            run.lifecycle(&target, ConnectorLifecycleCommand::Shutdown)
                .unwrap();
            // Historical data is still authentic; it is no current dispatch grant.
            assert!(metadata.snapshot_for(&summary).is_ok());
            assert!(
                metadata
                    .tool_schema(&target, &capability, &summary.revision)
                    .is_err()
            );
            assert_eq!(sends.load(Ordering::SeqCst), count);
            drop(run);
            assert!(matches!(
                metadata.snapshot_for(&summary),
                Err(Error::Unavailable)
            ));
            assert!(matches!(
                metadata.tool_schema(&target, &capability, &summary.revision),
                Err(Error::Unavailable)
            ));
        },
    );
}

struct RejectMembership;
impl ScalarResponseAdmission for RejectMembership {
    fn validate(
        &self,
        request: &ConnectorRequest,
        _: Option<&SourceId>,
        _: &ProtocolResult,
    ) -> Result<(), ConnectorFailure> {
        Err(ConnectorFailure {
            code: ConnectorFailureCode::PermissionDenied,
            effect: ConnectorEffect::NoMutation,
            correlation: request.correlation.clone(),
        })
    }
}
#[test]
fn routed_scalar_normalization_preserves_membership_denials_and_refuses_missing_normalizers() {
    with_preparation_resources(
        ContextScopeId::new("routed-scalar").unwrap(),
        SessionResourcePlan {
            preparation: amounts(),
            ..Default::default()
        },
        |resources, _, authority| {
            super::super::session_budget::SessionConnectorRun::with_factory(
                resources.allocate(amounts()).unwrap(),
                Arc::new(authority.clone()),
                CallContext::new(text("routed"), Instant::now() + Duration::from_secs(3)),
                |view| {
                    let mut shared = Arc::new(ScalarQuotes {
                        allocation: view.clone(),
                        target: target(view.allocation().binding().scope.clone()),
                        document: document(),
                        normalizers: normalizers(true),
                        tariff: FixedLocalTariff::operator_fixed_no_embedding(
                            ReferenceId::new("test-credit").unwrap(),
                            2,
                            3,
                        ),
                        response_admission: Arc::new(Membership),
                    });
                    let request = request(view.allocation());
                    let source = SourceId::new("records.tool").unwrap();
                    let dispatch = ConnectorDispatch {
                        request,
                        policy: PolicyId::new("p").unwrap(),
                        policy_revision: ContentDigest::new("a".repeat(64)).unwrap(),
                        consent_receipt: None,
                        budget_reservation: text(view.allocation().reservation_id().as_str()),
                        deadline: UnixTimestamp::new(100),
                    };
                    let document = document();
                    let mapping = document
                        .records()
                        .iter()
                        .find(|record| record.capability == dispatch.request.capability)
                        .unwrap();
                    let result = || ProtocolResult::Tool {
                        content: vec![],
                        structured: Some(json!({"object":"o","value":"safe"})),
                        is_error: false,
                    };
                    let observation = Routed(shared.clone())
                        .normalize(&dispatch, Some(&source), mapping, result())
                        .unwrap();
                    assert_eq!(observation.source, source);
                    Arc::get_mut(&mut shared).unwrap().normalizers.clear();
                    assert_eq!(
                        Routed(shared.clone()).normalize(
                            &dispatch,
                            Some(&source),
                            mapping,
                            result()
                        ),
                        Err(Error::Unsupported)
                    );
                    Arc::get_mut(&mut shared).unwrap().response_admission =
                        Arc::new(RejectMembership);
                    assert_eq!(
                        Routed(shared).normalize(&dispatch, Some(&source), mapping, result()),
                        Err(Error::SessionDenied(ConnectorFailureCode::PermissionDenied))
                    );
                    let bundle =
                        host(view, true, Limits::default(), Arc::new(AtomicUsize::new(0))).unwrap();
                    Ok(
                        super::super::session_budget::SessionConnectorComponents::from_qualified(
                            bundle,
                            Arc::new(authority.clone()),
                        ),
                    )
                },
            )
            .unwrap();
        },
    );
}
