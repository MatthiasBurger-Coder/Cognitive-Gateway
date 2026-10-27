use super::*;
use crate::goal_observation_test_harness as harness;
use crate::mcp_client::{
    CallContext, Limits,
    normalization::*,
    session_budget::{SessionConnectorComponents, SessionConnectorRun},
    transport::{Transport, TransportFactory},
};
use gateway_application::{
    connector_manifest::*,
    sessions::{SessionCheckpoint, SessionResourceAuthorityPort},
};
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};
#[derive(Clone)]
struct CurrentClaim {
    cp: SessionCheckpoint,
    revoked: Arc<AtomicBool>,
}
impl SessionResourceAuthorityPort for CurrentClaim {
    fn validate_allocation(&self, a: &SessionResourceAllocation) -> Result<(), SessionError> {
        let b = a.binding();
        if self.revoked.load(Ordering::SeqCst)
            || b.owner != self.cp.owner
            || b.scope != *self.cp.goal.scope()
            || b.session != self.cp.snapshot.session
            || b.run != self.cp.snapshot.run
            || b.revision != self.cp.snapshot.revision
            || self.cp.snapshot.dispatch.as_ref() != Some(&b.dispatch)
            || a.claim_revision() != self.cp.snapshot.revision
            || Instant::now() >= a.deadline()
        {
            return Err(SessionError::AuthorityDenied);
        }
        Ok(())
    }
}
fn current(cp: &SessionCheckpoint) -> CurrentClaim {
    CurrentClaim {
        cp: cp.clone(),
        revoked: Arc::new(AtomicBool::new(false)),
    }
}
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
fn host(
    allocation: super::super::session_budget::ReadonlyAllocationView,
    qualified: bool,
    limits: Limits,
    io: Arc<AtomicUsize>,
) -> Result<QualifiedScalarHost, Error> {
    let expected = target(allocation.allocation().binding().scope.clone());
    host_for_target(allocation, qualified, limits, io, expected)
}
fn host_for_target(
    allocation: super::super::session_budget::ReadonlyAllocationView,
    qualified: bool,
    limits: Limits,
    io: Arc<AtomicUsize>,
    expected: ConnectorTarget,
) -> Result<QualifiedScalarHost, Error> {
    qualified_scalar_host(
        QualifiedScalarConfig {
            allocation: allocation.clone(),
            host: HostConfig {
                target: expected,
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
fn real_child_quote_binds_exact_allocation_source_unit_and_all_input_output_bytes() {
    let total = amounts().checked_add(&amounts()).unwrap();
    harness::with_preparation_resources(
        ContextScopeId::new("quote-scope").unwrap(),
        ReferenceId::new("test-credit").unwrap(),
        total,
        |cp, resources| {
            let other = resources.allocate(amounts())?;
            let allocation = resources.allocate(amounts())?;
            let authority = Arc::new(current(cp));
            let io = Arc::new(AtomicUsize::new(0));
            SessionConnectorRun::with_factory(
                allocation,
                authority.clone(),
                CallContext::new(text("quote"), Instant::now() + Duration::from_secs(5)),
                |view| {
                    let bundle = host(view.clone(), true, Limits::default(), io.clone()).unwrap();
                    let a = view.allocation();
                    let r = request(a);
                    let source = SourceId::new("records.tool").unwrap();
                    let quote = bundle.quotes.quote_request(a, &r, Some(&source))?;
                    assert_eq!(
                        quote.amounts.retrieval_tokens,
                        argument_bytes(&r)? + 262_144 + 32
                    );
                    assert_eq!(quote.amounts.retrieval_results, 1);
                    assert_eq!(quote.amounts.cost_microunits, 3);
                    let mut longer = r.clone();
                    longer.arguments.insert(
                        text("extra-selector"),
                        TypedValue::string("é".repeat(512)).unwrap(),
                    );
                    assert!(
                        bundle
                            .quotes
                            .quote_request(a, &longer, Some(&source))?
                            .amounts
                            .retrieval_tokens
                            > quote.amounts.retrieval_tokens
                    );
                    assert_eq!(
                        bundle
                            .quotes
                            .quote_request(&other, &r, Some(&source))
                            .unwrap_err(),
                        SessionError::AuthorityDenied
                    );
                    assert_eq!(
                        bundle
                            .quotes
                            .quote_discovery(&other, &r.target)
                            .unwrap_err(),
                        SessionError::AuthorityDenied
                    );
                    assert!(
                        bundle
                            .quotes
                            .quote_request(a, &r, Some(&SourceId::new("foreign").unwrap()))
                            .is_err()
                    );
                    let mut foreign = r.clone();
                    foreign.target.scope = ContextScopeId::new("foreign").unwrap();
                    assert_eq!(
                        bundle
                            .quotes
                            .quote_request(a, &foreign, Some(&source))
                            .unwrap_err(),
                        SessionError::ScopeDenied
                    );
                    let dispatch = ConnectorDispatch {
                        request: r.clone(),
                        policy: PolicyId::new("fixture-policy").unwrap(),
                        policy_revision: ContentDigest::new("b".repeat(64)).unwrap(),
                        consent_receipt: None,
                        budget_reservation: text(a.reservation_id().as_str()),
                        deadline: UnixTimestamp::new(i64::MAX),
                    };
                    let maps = document();
                    let record = maps
                        .records()
                        .iter()
                        .find(|m| m.capability == r.capability)
                        .unwrap();
                    let observation = normalizers(true)
                        .remove(&r.capability)
                        .unwrap()
                        .canonicalize(
                            &dispatch,
                            Some(&source),
                            record,
                            ProtocolResult::Tool {
                                content: vec![],
                                structured: Some(
                                    json!({"object":"fixture-object","value":"escaped\" é"}),
                                ),
                                is_error: false,
                            },
                        )
                        .unwrap()
                        .into_observation();
                    bundle.quotes.validate_observation(
                        a,
                        &quote,
                        &r,
                        Some(&source),
                        &observation,
                    )?;
                    let mut forged = quote.clone();
                    forged.amounts.retrieval_tokens = 1;
                    assert!(
                        bundle
                            .quotes
                            .validate_observation(a, &forged, &r, Some(&source), &observation)
                            .is_err()
                    );
                    assert_eq!(
                        bundle
                            .quotes
                            .validate_observation(&other, &quote, &r, Some(&source), &observation)
                            .unwrap_err(),
                        SessionError::AuthorityDenied
                    );
                    let discovery = bundle.quotes.quote_discovery(a, &r.target)?;
                    assert_eq!(discovery.amounts.cost_microunits, 2);
                    assert_eq!(discovery.amounts.retrieval_results, 4);
                    assert_eq!(
                        bundle
                            .quotes
                            .validate_inventory(
                                &other,
                                &discovery,
                                &ConnectorInventory {
                                    target: r.target.clone(),
                                    revision: r.inventory_revision.clone(),
                                    capabilities: vec![],
                                    resources: vec![]
                                }
                            )
                            .unwrap_err(),
                        SessionError::AuthorityDenied
                    );
                    Ok(SessionConnectorComponents::from_qualified(
                        bundle,
                        authority.clone(),
                    ))
                },
            )?;
            assert_eq!(io.load(Ordering::SeqCst), 0);
            Ok(())
        },
    );
}
#[test]
fn real_factory_rejects_unknown_bounds_revocation_cancel_and_foreign_target_before_transport() {
    for fault in ["cancel", "revoke", "scope", "unknown"] {
        harness::with_preparation_resources(
            ContextScopeId::new("factory-scope").unwrap(),
            ReferenceId::new("test-credit").unwrap(),
            amounts(),
            |cp, resources| {
                let allocation = resources.allocate(amounts())?;
                let authority = Arc::new(current(cp));
                let io = Arc::new(AtomicUsize::new(0));
                let called = std::cell::Cell::new(false);
                let context =
                    CallContext::new(text("factory"), Instant::now() + Duration::from_secs(5));
                if fault == "cancel" {
                    context.cancel()
                }
                let result = SessionConnectorRun::with_factory(
                    allocation,
                    authority.clone(),
                    context,
                    |view| {
                        called.set(true);
                        if fault == "unknown" {
                            assert!(matches!(
                                host(view.clone(), false, Limits::default(), io.clone()),
                                Err(Error::Configuration)
                            ))
                        }
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
                        let bundle = if fault == "scope" {
                            host_for_target(
                                view,
                                true,
                                Limits::default(),
                                io.clone(),
                                target(ContextScopeId::new("foreign-scope").unwrap()),
                            )
                            .unwrap()
                        } else {
                            host(view, true, Limits::default(), io.clone()).unwrap()
                        };
                        if fault == "revoke" {
                            authority.revoked.store(true, Ordering::SeqCst);
                        }
                        Ok(SessionConnectorComponents::from_qualified(
                            bundle,
                            authority.clone(),
                        ))
                    },
                );
                if fault == "unknown" {
                    assert!(result.is_ok())
                } else {
                    assert!(result.is_err(), "{fault}")
                }
                assert_eq!(called.get(), fault != "cancel");
                assert_eq!(io.load(Ordering::SeqCst), 0);
                Ok(())
            },
        );
    }
}
#[test]
fn preparation_run_rejects_all_mutation_and_admin_before_quotes_or_transport() {
    harness::with_preparation_resources(
        ContextScopeId::new("readonly-scope").unwrap(),
        ReferenceId::new("test-credit").unwrap(),
        amounts(),
        |cp, resources| {
            let a = resources.allocate(amounts())?;
            let mut r = request(&a);
            let authority = Arc::new(current(cp));
            let io = Arc::new(AtomicUsize::new(0));
            let run = SessionConnectorRun::with_factory(
                a,
                authority.clone(),
                CallContext::new(text("read-only"), Instant::now() + Duration::from_secs(5)),
                |view| {
                    Ok(SessionConnectorComponents::from_qualified(
                        host(view, true, Limits::default(), io.clone()).unwrap(),
                        authority.clone(),
                    ))
                },
            )?;
            for class in [
                ConnectorOperationClass::Mutate,
                ConnectorOperationClass::Admin,
            ] {
                r.class = class;
                let err = run.invoke(&r).unwrap_err();
                assert_eq!(err.effect, ConnectorEffect::NotDispatched);
            }
            assert_eq!(run.usage().unwrap(), SessionResourceAmounts::default());
            assert_eq!(io.load(Ordering::SeqCst), 0);
            Ok(())
        },
    );
}

fn routed_normalizers() -> BTreeMap<CapabilityId, CanonicalResultNormalizer> {
    let mut values = normalizers(true);
    let doc = document();
    let record = doc
        .records()
        .iter()
        .find(|r| r.capability.as_str() == "records.read")
        .unwrap();
    let routes = [("one", 1usize), ("two", 2usize)]
        .into_iter()
        .map(|(id, fields)| {
            let mut p = profile(record.source.clone().unwrap(), fields);
            p.fields = p
                .fields
                .into_iter()
                .enumerate()
                .map(|(n, (key, _))| {
                    (
                        key,
                        SubjectPath::new(["object", id, &format!("field{n}")]).unwrap(),
                    )
                })
                .collect();
            NormalizationRoute {
                capability: record.capability.clone(),
                source: record.source.clone().unwrap(),
                arguments: BTreeMap::from([(text("query"), TypedValue::string(id).unwrap())]),
                profile: p,
            }
        })
        .collect();
    values.insert(
        record.capability.clone(),
        CanonicalResultNormalizer::new_qualified_routes(routes, Box::new(Collector)).unwrap(),
    );
    values
}

#[test]
fn route_quotes_and_actual_native_records_share_profiles_from_a_real_phase_child() {
    harness::with_preparation_resources(
        ContextScopeId::new("route-scope").unwrap(),
        ReferenceId::new("test-credit").unwrap(),
        amounts(),
        |cp, resources| {
            let allocation = resources.allocate(amounts())?;
            let base = request(&allocation);
            let authority = Arc::new(current(cp));
            let io = Arc::new(AtomicUsize::new(0));
            let run = SessionConnectorRun::with_factory(
                allocation,
                authority.clone(),
                CallContext::new(
                    text("routed-quotes"),
                    Instant::now() + Duration::from_secs(5),
                ),
                |view| {
                    // One concrete Arc owns BOTH the quote path and actual native normalizer.
                    let shared = Arc::new(ScalarQuotes {
                        allocation: view.clone(),
                        target: base.target.clone(),
                        document: document(),
                        normalizers: routed_normalizers(),
                        tariff: FixedLocalTariff::operator_fixed_no_embedding(
                            ReferenceId::new("test-credit").unwrap(),
                            1,
                            1,
                        ),
                        response_admission: Arc::new(Membership),
                    });
                    let mapping = shared
                        .document
                        .records()
                        .iter()
                        .find(|r| r.capability == base.capability)
                        .unwrap();
                    let mut quotes = vec![];
                    for (id, fields) in [("one", 1usize), ("two", 2usize)] {
                        let mut r = base.clone();
                        r.arguments =
                            BTreeMap::from([(text("query"), TypedValue::string(id).unwrap())]);
                        r.correlation = text(id);
                        let q = shared.quote_request(view.allocation(), &r, None)?;
                        let d = ConnectorDispatch {
                            request: r.clone(),
                            policy: PolicyId::new("test-policy").unwrap(),
                            policy_revision: context_content_digest(b"current-fixture-policy"),
                            consent_receipt: None,
                            budget_reservation: text("test-reservation"),
                            deadline: UnixTimestamp::new(20),
                        };
                        let mut payload = json!({"object":id,"value":"actual value"});
                        if fields == 2 {
                            payload["v1"] = json!("another actual value");
                        }
                        let observation = Routed(shared.clone())
                            .normalize(
                                &d,
                                None,
                                mapping,
                                ProtocolResult::Tool {
                                    content: vec![],
                                    structured: Some(payload),
                                    is_error: false,
                                },
                            )
                            .unwrap();
                        shared.validate_observation(
                            view.allocation(),
                            &q,
                            &r,
                            None,
                            &observation,
                        )?;
                        assert_eq!(q.amounts.retrieval_results, fields as u64);
                        assert_eq!(observation.records.facts().len(), fields);
                        assert!(observation.records.facts().iter().all(|f| {
                            f.subject()
                                .to_string()
                                .starts_with(&format!("object.{id}."))
                        }));
                        quotes.push(q);
                    }
                    assert_ne!(
                        quotes[0].amounts, quotes[1].amounts,
                        "different routes must retain their own native hard bounds"
                    );
                    let mut unknown = base.clone();
                    unknown.arguments =
                        BTreeMap::from([(text("query"), TypedValue::string("unknown").unwrap())]);
                    assert!(
                        shared
                            .quote_request(view.allocation(), &unknown, None)
                            .is_err()
                    );
                    assert_eq!(io.load(Ordering::SeqCst), 0);
                    let bundle = qualified_scalar_host(
                        QualifiedScalarConfig {
                            allocation: view,
                            host: HostConfig {
                                target: base.target.clone(),
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
                            normalizers: routed_normalizers(),
                            tariff: FixedLocalTariff::operator_fixed_no_embedding(
                                ReferenceId::new("test-credit").unwrap(),
                                1,
                                1,
                            ),
                        },
                        Box::new(NoIo(io.clone())),
                        Arc::new(Denied),
                        Arc::new(Membership),
                        Arc::new(()),
                    )
                    .unwrap();
                    Ok(SessionConnectorComponents::from_qualified(
                        bundle,
                        authority.clone(),
                    ))
                },
            )?;
            assert_eq!(run.usage()?, SessionResourceAmounts::default());
            assert_eq!(io.load(Ordering::SeqCst), 0);
            Ok(())
        },
    );
}
