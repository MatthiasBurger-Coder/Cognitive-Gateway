//! Real subprocess conformance tests. Authority/mapping are explicit test adapters;
//! these tests do not qualify the future production policy service or providers.
use gateway_application::{connector_manifest::*, connectors::*};
use gateway_daemon::mcp_client::{transport::*, *};
use gateway_domain::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
fn text(s: &str) -> NonEmptyText {
    NonEmptyText::new(s).unwrap()
}
fn digest() -> ContentDigest {
    ContentDigest::new("a".repeat(64)).unwrap()
}
fn target() -> ConnectorTarget {
    ConnectorTarget {
        connector: text("example.records"),
        scope: ContextScopeId::new("scope-one").unwrap(),
    }
}
fn request() -> ConnectorRequest {
    ConnectorRequest {
        target: target(),
        capability: CapabilityId::new("records.read").unwrap(),
        class: ConnectorOperationClass::Read,
        inventory_revision: digest(),
        arguments: [(text("query"), TypedValue::string("hello").unwrap())].into(),
        task: TaskId::new("task").unwrap(),
        step: PlanStepId::new("step").unwrap(),
        execution: text("exec"),
        correlation: text("call-one"),
    }
}
fn failure() -> ConnectorFailure {
    ConnectorFailure {
        code: ConnectorFailureCode::PermissionDenied,
        effect: ConnectorEffect::NotDispatched,
        correlation: text("call-one"),
    }
}
#[derive(Default)]
struct Authority {
    deny: AtomicUsize,
    validations: AtomicUsize,
}
impl ConnectorAdmission for Authority {
    fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        if self.deny.load(Ordering::SeqCst) == 1 {
            Err(failure())
        } else {
            Ok(())
        }
    }
    fn authorize(
        &self,
        request: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorDispatch, ConnectorFailure> {
        if self.deny.load(Ordering::SeqCst) == 2 {
            return Err(failure());
        }
        let mut request = request.clone();
        if self.deny.load(Ordering::SeqCst) == 4 {
            request.execution = text("forged");
        }
        Ok(ConnectorDispatch {
            request,
            policy: PolicyId::new("policy").unwrap(),
            policy_revision: digest(),
            consent_receipt: None,
            budget_reservation: text("reservation"),
            deadline: UnixTimestamp::new(if self.deny.load(Ordering::SeqCst) == 5 {
                0
            } else {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
                    + 10
            }),
        })
    }
    fn revalidate(
        &self,
        _: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        self.validations.fetch_add(1, Ordering::SeqCst);
        if self.deny.load(Ordering::SeqCst) == 3 {
            Err(failure())
        } else {
            Ok(())
        }
    }
}
struct Mapping {
    mode: &'static str,
}
impl ConnectorMapping for Mapping {
    fn inventory(
        &self,
        target: &ConnectorTarget,
        _: &ConnectorManifest,
        wire: &ProtocolInventory,
    ) -> Result<ConnectorInventory, Error> {
        if self.mode == "mapping-error" {
            return Err(Error::Schema);
        }
        let mut inventory = ConnectorInventory {
            target: target.clone(),
            revision: digest(),
            capabilities: [
                ConnectorOperationClass::Read,
                ConnectorOperationClass::Mutate,
            ]
            .into_iter()
            .map(|class| ConnectorCapability {
                capability: CapabilityId::new(if class == ConnectorOperationClass::Mutate {
                    "records.z-mutate"
                } else {
                    "records.read"
                })
                .unwrap(),
                class,
                argument_schema_digest: digest(),
                result_schema_digest: digest(),
            })
            .collect(),
            resources: vec![SourceId::new("one").unwrap()],
        };
        if self.mode == "bad-inventory" {
            inventory.target.scope = ContextScopeId::new("other").unwrap();
        }
        if self.mode == "pages" {
            assert_eq!(wire.tools[0]["name"], "records.read");
            assert_eq!(wire.tools[1]["name"], "z");
        }
        Ok(inventory)
    }
    fn operation(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
        _: &ProtocolInventory,
    ) -> Result<WireOperation, Error> {
        if self.mode == "operation-error" {
            return Err(Error::Schema);
        }
        if source.is_some() {
            return Ok(WireOperation::Resource {
                uri: if self.mode == "unlisted" {
                    "arbitrary://escape"
                } else {
                    "records://one"
                }
                .into(),
            });
        }
        let mut arguments = dispatch.request.arguments.clone();
        if self.mode == "changed-arguments" {
            arguments.clear();
        }
        Ok(WireOperation::Tool {
            name: if self.mode == "unlisted" {
                "forbidden"
            } else {
                "records.read"
            }
            .into(),
            arguments,
        })
    }
    fn normalize(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
        result: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        if self.mode == "normalize-error" {
            return Err(Error::Schema);
        }
        match result {
            ProtocolResult::Tool {
                content,
                structured,
                is_error,
            } => {
                assert_eq!(is_error, self.mode == "tool-error");
                assert_eq!(content[0]["type"], "text");
                assert!(structured.is_some());
            }
            ProtocolResult::Resource { contents } => {
                assert_eq!(contents[0]["text"], "fixture value")
            }
        }
        let source_id = source.cloned().unwrap_or(SourceId::new("one").unwrap());
        let pid = gateway_domain::ProvenanceId::new("fixture.p").unwrap();
        let oid = gateway_domain::ObservationId::new("fixture.o").unwrap();
        let fid = gateway_domain::FactId::new("fixture.f").unwrap();
        let subject = gateway_domain::SubjectPath::new("fixture.value".split('.')).unwrap();
        let value = gateway_domain::TypedValue::string("fixture").unwrap();
        let records = ObservationEvidenceSet::new(
            vec![
                gateway_domain::Provenance::new(
                    pid.clone(),
                    gateway_domain::SourceKind::Tool,
                    source_id.clone(),
                    "fixture.object",
                )
                .unwrap()
                .with_producer("fixture.collector")
                .unwrap()
                .with_acquired_at(gateway_domain::SourceTimestamp::new("1").unwrap()),
            ],
            vec![
                gateway_domain::Observation::new(
                    oid.clone(),
                    subject.clone(),
                    value.clone(),
                    pid.clone(),
                )
                .unwrap(),
            ],
            vec![
                gateway_domain::Fact::new(
                    fid.clone(),
                    subject,
                    value,
                    gateway_domain::AssertionPolarity::Affirmed,
                    vec![oid],
                )
                .unwrap(),
            ],
            vec![
                gateway_domain::Evidence::new(
                    gateway_domain::EvidenceId::new("fixture.e").unwrap(),
                    gateway_domain::EvidenceKind::ToolOutput,
                    "fixture",
                    gateway_domain::EvidenceContent::inline("fixture").unwrap(),
                    pid,
                    vec![gateway_domain::EvidenceLink::new(
                        fid,
                        gateway_domain::EvidenceRelation::Supports,
                    )],
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let mut observation = ConnectorObservation {
            metadata: gateway_application::connector_results::ConnectorResultMetadata {
                task: dispatch.request.task.clone(),
                step: dispatch.request.step.clone(),
                execution: dispatch.request.execution.clone(),
                object_reference: text("fixture.object"),
                source_kind: gateway_domain::SourceKind::Tool,
                source_digest: None,
                observed_at: None,
                authenticated_source: text("fixture.collector"),
                quality: gateway_domain::QualityMetadata::new(
                    gateway_domain::TrustClass::ObservedEvidence,
                    gateway_domain::SensitivityClass::Internal,
                    gateway_domain::Confidence::Unknown,
                    gateway_domain::FreshnessStatus::Unknown,
                    gateway_domain::Uncertainty::Unknown,
                ),
                effect: if source.is_some() {
                    ConnectorEffect::NoMutation
                } else {
                    ConnectorEffect::Acknowledged
                },
                partial: false,
                verifies: None,
                non_authoritative_raw: None,
            },
            target: dispatch.request.target.clone(),
            inventory_revision: dispatch.request.inventory_revision.clone(),
            correlation: dispatch.request.correlation.clone(),
            source: source.cloned().unwrap_or(SourceId::new("one").unwrap()),
            source_revision: None,
            content_digest: digest(),
            acquired_at: UnixTimestamp::new(1),
            records,
            basis: if source.is_some() {
                ConnectorObservationBasis::SourceRead
            } else {
                ConnectorObservationBasis::ProviderAcknowledgement
            },
        };
        if self.mode == "bad-observation" {
            observation.correlation = text("wrong");
        }
        Ok(observation)
    }
}
#[derive(Default)]
struct Events(Mutex<Vec<Event>>);
impl Observer for Events {
    fn record(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }
}
fn config(mode: &str) -> StdioConfig {
    StdioConfig {
        executable: PathBuf::from("/usr/bin/python3"),
        arguments: vec![
            "-I".into(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/mcp-client-v1/server.py")
                .display()
                .to_string(),
            mode.into(),
        ],
        directory: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        environment: BTreeMap::new(),
    }
}
fn build(
    mode: &'static str,
    limits: Limits,
    authority: Arc<Authority>,
    events: Arc<Events>,
) -> McpConnectorHost {
    McpConnectorHost::new(
        HostConfig {
            target: target(),
            manifest: ConnectorManifest::from_json(include_bytes!(
                "../../../tests/fixtures/connector-manifest-v1/valid-stdio.json"
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
        Box::new(config(mode)),
        authority,
        Box::new(Mapping { mode }),
        events,
    )
    .unwrap()
}
fn host(mode: &'static str) -> McpConnectorHost {
    build(
        mode,
        Limits::default(),
        Arc::new(Authority::default()),
        Arc::new(Events::default()),
    )
}
fn ready(host: &McpConnectorHost) {
    assert_eq!(
        host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
            .unwrap(),
        ConnectorLifecycleState::Ready
    );
    host.discover(&target()).unwrap();
}
#[test]
fn stdio_handshake_discovery_dispatch_health_and_reconnect() {
    let events = Arc::new(Events::default());
    let host = build(
        "pages",
        Limits::default(),
        Arc::new(Authority::default()),
        events.clone(),
    );
    assert_eq!(host.session_state().unwrap(), SessionState::Disconnected);
    ready(&host);
    assert_eq!(
        host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
            .unwrap(),
        ConnectorLifecycleState::Ready
    );
    assert_eq!(
        host.invoke(&request()).unwrap().correlation,
        request().correlation
    );
    assert_eq!(
        host.read_resource(&request(), &SourceId::new("one").unwrap())
            .unwrap()
            .basis,
        ConnectorObservationBasis::SourceRead
    );
    assert_eq!(
        host.lifecycle(&target(), ConnectorLifecycleCommand::Health)
            .unwrap(),
        ConnectorLifecycleState::Ready
    );
    assert_eq!(
        host.lifecycle(&target(), ConnectorLifecycleCommand::Shutdown)
            .unwrap(),
        ConnectorLifecycleState::Disconnected
    );
    ready(&host);
    let ids: Vec<_> = events
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e.kind == EventKind::Sent)
        .map(|e| e.request_id.unwrap())
        .collect();
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    host.lifecycle(&target(), ConnectorLifecycleCommand::Disable)
        .unwrap();
    assert_eq!(host.session_state().unwrap(), SessionState::Disabled);
    assert!(
        host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
            .is_err()
    );
    assert_eq!(
        host.invoke(&request()).unwrap_err().effect,
        ConnectorEffect::NotDispatched
    );
}
#[test]
fn fails_closed_for_negotiation_and_pre_ready_calls() {
    for mode in ["version", "wrong-id", "slow-init"] {
        let host = build(
            mode,
            Limits {
                call_timeout: Duration::from_millis(100),
                shutdown: Duration::from_millis(5),
                ..Limits::default()
            },
            Arc::new(Authority::default()),
            Arc::new(Events::default()),
        );
        assert!(host.invoke(&request()).is_err());
        let error = host
            .lifecycle(&target(), ConnectorLifecycleCommand::Connect)
            .unwrap_err();
        assert_eq!(
            error.code,
            match mode {
                "version" => ConnectorFailureCode::IncompatibleVersion,
                "slow-init" => ConnectorFailureCode::Timeout,
                _ => ConnectorFailureCode::ProtocolError,
            }
        );
        assert_eq!(host.session_state().unwrap(), SessionState::Degraded);
    }
}
#[test]
fn optional_capabilities_and_protocol_denials() {
    for mode in [
        "empty",
        "server-request",
        "tool-error",
        "output-schema",
        "no-iserror",
    ] {
        let host = host(mode);
        ready(&host);
        if mode == "empty" {
            assert!(host.invoke(&request()).is_err());
        } else {
            host.invoke(&request()).unwrap();
        }
    }
    for (mode, code) in [
        ("duplicate", ConnectorFailureCode::SchemaMismatch),
        ("cycle", ConnectorFailureCode::SchemaMismatch),
        ("mapping-error", ConnectorFailureCode::SchemaMismatch),
    ] {
        let host = host(mode);
        host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
            .unwrap();
        assert_eq!(host.discover(&target()).unwrap_err().code, code);
    }
    for (mode, code) in [
        ("eof", ConnectorFailureCode::Unavailable),
        ("remote", ConnectorFailureCode::ProtocolError),
        ("malformed", ConnectorFailureCode::ProtocolError),
        ("invalidated", ConnectorFailureCode::InventoryChanged),
        ("oversize", ConnectorFailureCode::BudgetExhausted),
        ("flood", ConnectorFailureCode::BudgetExhausted),
    ] {
        let events = Arc::new(Events::default());
        let host = build(
            mode,
            Limits {
                frame_bytes: 2048,
                events_per_request: 8,
                ..Limits::default()
            },
            Arc::new(Authority::default()),
            events.clone(),
        );
        ready(&host);
        assert_eq!(host.invoke(&request()).unwrap_err().code, code, "{mode}");
        assert_eq!(host.session_state().unwrap(), SessionState::Degraded);
        assert!(!format!("{:?}", events.0.lock().unwrap()).contains("SECRET"));
        assert!(host.invoke(&request()).is_err());
        host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
            .unwrap();
        // Reconnect has no executable inventory until rediscovery.
        assert_eq!(
            host.invoke(&request()).unwrap_err().code,
            ConnectorFailureCode::InventoryChanged
        );
    }
}
#[test]
fn admission_scope_binding_mapping_and_revision_are_checked_before_effects() {
    for denial in 1..=5 {
        let authority = Arc::new(Authority::default());
        let host = build(
            "normal",
            Limits::default(),
            authority.clone(),
            Arc::new(Events::default()),
        );
        ready(&host);
        authority.deny.store(denial, Ordering::SeqCst);
        assert_eq!(
            host.invoke(&request()).unwrap_err().effect,
            ConnectorEffect::NotDispatched
        );
    }
    for mode in [
        "operation-error",
        "changed-arguments",
        "unlisted",
        "normalize-error",
        "bad-observation",
        "bad-inventory",
    ] {
        let host = host(mode);
        host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
            .unwrap();
        if mode == "bad-inventory" {
            assert!(host.discover(&target()).is_err());
            continue;
        }
        host.discover(&target()).unwrap();
        assert_eq!(
            host.invoke(&request()).unwrap_err().code,
            ConnectorFailureCode::SchemaMismatch,
            "{mode}"
        );
        if mode == "unlisted" {
            assert_eq!(
                host.read_resource(&request(), &SourceId::new("one").unwrap())
                    .unwrap_err()
                    .effect,
                ConnectorEffect::NotDispatched
            );
        }
    }
    let host = host("normal");
    ready(&host);
    let mut r = request();
    r.target.scope = ContextScopeId::new("other").unwrap();
    assert_eq!(
        host.invoke(&r).unwrap_err().code,
        ConnectorFailureCode::ScopeMismatch
    );
    r = request();
    r.inventory_revision = ContentDigest::new("b".repeat(64)).unwrap();
    assert_eq!(
        host.invoke(&r).unwrap_err().code,
        ConnectorFailureCode::InventoryChanged
    );
    r = request();
    r.capability = CapabilityId::new("unknown").unwrap();
    assert!(host.invoke(&r).is_err());
    assert!(
        host.read_resource(&request(), &SourceId::new("unknown").unwrap())
            .is_err()
    );
    let ctx = CallContext::new(text("different"), Instant::now() + Duration::from_secs(1));
    assert!(host.invoke_with_context(&request(), None, &ctx).is_err());
}
#[test]
fn cancellation_timeout_backpressure_disable_and_effect_uncertainty() {
    for cancel in [false, true] {
        let events = Arc::new(Events::default());
        let host = Arc::new(build(
            "slow",
            Limits {
                shutdown: Duration::from_millis(10),
                ..Limits::default()
            },
            Arc::new(Authority::default()),
            events.clone(),
        ));
        ready(&host);
        let context = CallContext::new(
            request().correlation,
            Instant::now() + Duration::from_millis(if cancel { 1000 } else { 50 }),
        );
        let runner = host.clone();
        let control = context.clone();
        let thread = std::thread::spawn(move || {
            let mut r = request();
            r.class = ConnectorOperationClass::Mutate;
            r.capability = CapabilityId::new("records.z-mutate").unwrap();
            runner.invoke_with_context(&r, None, &control)
        });
        let until = Instant::now() + Duration::from_secs(2);
        while !events
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.kind == EventKind::Sent && e.correlation == request().correlation)
        {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(host.session_state().is_err());
        assert_eq!(
            host.invoke(&request()).unwrap_err().code,
            ConnectorFailureCode::Backpressure
        );
        if cancel {
            context.cancel();
        }
        let failure = thread.join().unwrap().unwrap_err();
        assert_eq!(
            failure.code,
            if cancel {
                ConnectorFailureCode::Cancelled
            } else {
                ConnectorFailureCode::Timeout
            }
        );
        assert_eq!(failure.effect, ConnectorEffect::Unknown);
        // No automatic replay, even after an uncertain effect.
        assert_eq!(
            host.invoke(&request()).unwrap_err().effect,
            ConnectorEffect::NotDispatched
        );
    }
    let events = Arc::new(Events::default());
    let host = Arc::new(build(
        "slow",
        Limits {
            shutdown: Duration::from_millis(10),
            ..Limits::default()
        },
        Arc::new(Authority::default()),
        events.clone(),
    ));
    ready(&host);
    let runner = host.clone();
    let thread = std::thread::spawn(move || runner.invoke(&request()));
    let until = Instant::now() + Duration::from_secs(2);
    while !events
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|e| e.kind == EventKind::Sent && e.correlation == request().correlation)
    {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        host.lifecycle(&target(), ConnectorLifecycleCommand::Disable)
            .unwrap(),
        ConnectorLifecycleState::Disabled
    );
    assert_eq!(
        thread.join().unwrap().unwrap_err().code,
        ConnectorFailureCode::Cancelled
    );
}
#[test]
fn deadlines_limits_and_stdio_configuration_are_bounded() {
    let host = host("normal");
    ready(&host);
    let ctx = CallContext::new(request().correlation, Instant::now());
    assert_eq!(
        host.invoke_with_context(&request(), None, &ctx)
            .unwrap_err()
            .code,
        ConnectorFailureCode::Timeout
    );
    let ctx = CallContext::new(
        request().correlation,
        Instant::now() + Duration::from_secs(1),
    );
    ctx.cancel();
    assert_eq!(
        host.invoke_with_context(&request(), None, &ctx)
            .unwrap_err()
            .code,
        ConnectorFailureCode::Cancelled
    );
    for limits in [
        Limits {
            pages: 1,
            ..Limits::default()
        },
        Limits {
            items: 1,
            ..Limits::default()
        },
        Limits {
            inventory_bytes: 1,
            ..Limits::default()
        },
    ] {
        let host = build(
            "pages",
            limits,
            Arc::new(Authority::default()),
            Arc::new(Events::default()),
        );
        host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
            .unwrap();
        assert_eq!(
            host.discover(&target()).unwrap_err().code,
            ConnectorFailureCode::BudgetExhausted
        );
    }
    let context = CallContext::new(text("transport"), Instant::now() + Duration::from_secs(1));
    let mut c = config("normal");
    c.executable = "relative".into();
    assert!(matches!(
        c.open(&context, &Limits::default()),
        Err(Error::Configuration)
    ));
    c.executable = "/does-not-exist".into();
    assert!(matches!(
        c.open(&context, &Limits::default()),
        Err(Error::Unavailable)
    ));
    let mut t = config("normal").open(&context, &Limits::default()).unwrap();
    assert_eq!(t.send(b"bad\nframe", &context), Err(Error::Limit));
    t.close();
    assert_eq!(t.send(b"{}", &context), Err(Error::Unavailable));
}

#[test]
fn typed_result_validation_rejects_bad_envelopes_and_schemas() {
    for mode in [
        "output-mismatch",
        "bad-iserror",
        "bad-structured",
        "bad-resource",
    ] {
        let host = host(mode);
        ready(&host);
        let error = if mode == "bad-resource" {
            host.read_resource(&request(), &SourceId::new("one").unwrap())
                .unwrap_err()
        } else {
            host.invoke(&request()).unwrap_err()
        };
        assert_eq!(error.code, ConnectorFailureCode::SchemaMismatch, "{mode}");
    }
}
#[test]
fn stdio_owns_child_environment_working_directory_and_reaps_on_close() {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let log = std::env::temp_dir().join(format!(
        "cg-mcp-peer-{}-{}.jsonl",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut c = config("normal");
    c.arguments.push(log.display().to_string());
    c.environment
        .insert("ADMITTED_MARKER".into(), "allowed".into());
    let context = CallContext::new(text("child"), Instant::now() + Duration::from_secs(2));
    let mut transport = c.open(&context, &Limits::default()).unwrap();
    transport
        .send(
            br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            &context,
        )
        .unwrap();
    transport.receive(&context).unwrap();
    transport.close();
    let log_bytes = std::fs::read_to_string(&log).unwrap();
    let startup: serde_json::Value =
        serde_json::from_str(log_bytes.lines().next().unwrap()).unwrap();
    assert_eq!(startup["environment"]["ADMITTED_MARKER"], "allowed");
    assert!(startup["environment"].get("PATH").is_none());
    assert_eq!(
        std::fs::canonicalize(startup["directory"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(&c.directory).unwrap()
    );
    #[cfg(target_os = "linux")]
    assert!(!std::path::Path::new(&format!("/proc/{}", startup["pid"].as_u64().unwrap())).exists());
    std::fs::remove_file(log).unwrap();
}

struct FixtureNormalizer;
impl mapping::ConnectorResultNormalizer for FixtureNormalizer {
    fn normalize(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
        _: &mapping::MappingRecord,
        result: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        Mapping { mode: "normal" }.normalize(dispatch, source, result)
    }
}
fn canonical_host(authority: Arc<Authority>, mode: &'static str) -> McpConnectorHost {
    canonical_host_with_normalizer(authority, mode, Box::new(FixtureNormalizer))
}
fn canonical_host_with_normalizer(
    authority: Arc<Authority>,
    mode: &'static str,
    normalizer: Box<dyn mapping::ConnectorResultNormalizer>,
) -> McpConnectorHost {
    let manifest = ConnectorManifest::from_json(include_bytes!(
        "../../../tests/fixtures/connector-mapping-v1/manifest.json"
    ))
    .unwrap();
    let mapper = mapping::DiscoveryMapper::new(
        target(),
        manifest.clone(),
        mapping::MappingDocument::from_json(include_bytes!(
            "../../../tests/fixtures/connector-mapping-v1/mapping.json"
        ))
        .unwrap(),
    )
    .unwrap();
    McpConnectorHost::new(
        HostConfig {
            target: target(),
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
        Box::new(config(mode)),
        authority,
        Box::new(mapping::CanonicalConnectorMapping::new(mapper, normalizer)),
        Arc::new(()),
    )
    .unwrap()
}
#[test]
fn canonical_mapping_drives_real_tool_and_resource_calls_but_never_grants_authority() {
    let authority = Arc::new(Authority::default());
    let host = canonical_host(authority.clone(), "normal");
    host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
        .unwrap();
    let inventory = host.discover(&target()).unwrap();
    assert_eq!(
        inventory.revision.as_str(),
        include_str!("../../../tests/fixtures/connector-mapping-v1/revision.txt").trim()
    );
    let mut request = request();
    request.inventory_revision = inventory.revision;
    host.invoke(&request).unwrap();
    request.capability = CapabilityId::new("records.resource.read").unwrap();
    request.arguments.clear();
    host.read_resource(&request, &SourceId::new("records.one").unwrap())
        .unwrap();
    assert_eq!(
        host.read_resource(&request, &SourceId::new("unmapped").unwrap())
            .unwrap_err()
            .effect,
        ConnectorEffect::NotDispatched
    );
    authority.deny.store(2, Ordering::SeqCst);
    assert_eq!(
        host.read_resource(&request, &SourceId::new("records.one").unwrap())
            .unwrap_err()
            .code,
        ConnectorFailureCode::PermissionDenied
    );
    authority.deny.store(3, Ordering::SeqCst);
    assert_eq!(
        host.read_resource(&request, &SourceId::new("records.one").unwrap())
            .unwrap_err()
            .code,
        ConnectorFailureCode::PermissionDenied
    );
    authority.deny.store(1, Ordering::SeqCst);
    assert_eq!(
        host.discover(&target()).unwrap_err().code,
        ConnectorFailureCode::PermissionDenied
    );
}
#[test]
fn canonical_mapping_blocks_unmapped_discovery_and_bad_output_before_normalization() {
    // Additional discovered tools cannot silently broaden a configured inventory.
    let host = canonical_host(Arc::new(Authority::default()), "pages");
    host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
        .unwrap();
    assert_eq!(
        host.discover(&target()).unwrap_err().code,
        ConnectorFailureCode::SchemaMismatch
    );
    // Missing advertised outputSchema still uses the explicit Gateway-side contract.
    let host = canonical_host(Arc::new(Authority::default()), "bad-structured");
    host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
        .unwrap();
    let inventory = host.discover(&target()).unwrap();
    let mut request = request();
    request.inventory_revision = inventory.revision;
    assert_eq!(
        host.invoke(&request).unwrap_err().code,
        ConnectorFailureCode::SchemaMismatch
    );
}

#[test]
fn real_stdio_resource_crosses_canonical_normalization_ingestion_and_context_boundary() {
    use gateway_application::ports::inbound::{ObservationEvidenceInputPort, ScopeLifecyclePort};
    use gateway_application::{InMemoryContextStore, connector_results::ConnectorResult};
    use gateway_daemon::mcp_client::normalization::*;
    struct Collector;
    impl AcquisitionPort for Collector {
        fn collect(&self, _: &ConnectorDispatch) -> Result<Acquisition, Error> {
            Ok(Acquisition {
                acquired_at: UnixTimestamp::new(100),
                observed_at: Some(UnixTimestamp::new(100)),
                evaluated_at: UnixTimestamp::new(100),
                max_age: FreshnessPolicy::new(60),
                authenticated_source: text("test.admitted.stdio.peer"),
                sensitivity: SensitivityClass::Internal,
                conflict: ConflictStatus::Unresolved,
                partial: false,
                basis: ConnectorObservationBasis::SourceRead,
                effect: ConnectorEffect::NoMutation,
                verifies: None,
                non_authoritative_raw: Some(ReferenceId::new("test.raw.archive").unwrap()),
            })
        }
    }
    let normalizer = CanonicalResultNormalizer::new(
        NormalizationProfile {
            source: SourceId::new("records.one").unwrap(),
            source_kind: SourceKind::Tool,
            object_field: "uri".into(),
            revision_field: None,
            digest_field: None,
            fields: [("text".into(), "record.content".parse().unwrap())].into(),
        },
        Box::new(Collector),
    )
    .unwrap();
    let authority = Arc::new(Authority::default());
    let host = canonical_host_with_normalizer(authority.clone(), "normal", Box::new(normalizer));
    host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
        .unwrap();
    let inventory = host.discover(&target()).unwrap();
    let mut r = request();
    r.inventory_revision = inventory.revision;
    r.capability = CapabilityId::new("records.resource.read").unwrap();
    r.arguments.clear();
    let observation = host
        .read_resource(&r, &SourceId::new("records.one").unwrap())
        .unwrap();
    let dispatch = authority.authorize(&r, Some(&observation.source)).unwrap();
    let result = ConnectorResult::new(&dispatch, observation).unwrap();
    assert_eq!(
        result.observation().metadata.object_reference.as_str(),
        "records://one"
    );
    assert_eq!(
        result.observation().records.observations()[0].value(),
        &TypedValue::string("fixture value").unwrap()
    );
    let store = InMemoryContextStore::new();
    let scope = store.open_scope(r.target.scope.clone()).unwrap();
    let batch = result.batch().unwrap();
    store.ingest_observations(&scope, batch.clone()).unwrap();
    assert_eq!(store.snapshot(&scope).unwrap().batches(), &[batch]);
    let fragments = result.fragments().unwrap();
    assert!(fragments[0].is_reference());
    assert_eq!(
        fragments[0].metadata().quality.conflict(),
        ConflictStatus::Unresolved
    );
    host.lifecycle(&target(), ConnectorLifecycleCommand::Shutdown)
        .unwrap();
}
