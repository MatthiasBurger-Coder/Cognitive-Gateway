use super::*;
use gateway_application::connector_manifest::*;
use gateway_domain::*;
use std::collections::BTreeSet;

#[test]
fn tool_schema_lock_failures_have_stable_content_free_projections() {
    assert_eq!(
        super::tool_schema_lock_error(ConnectorFailureCode::Backpressure),
        Error::InvalidState
    );
    assert_eq!(
        super::tool_schema_lock_error(ConnectorFailureCode::Timeout),
        Error::Timeout
    );
    assert_eq!(
        super::tool_schema_lock_error(ConnectorFailureCode::PermissionDenied),
        Error::Unavailable
    );
}

struct Deny;
fn denied() -> ConnectorFailure {
    Error::Unsupported.failure(
        &NonEmptyText::new("deny").unwrap(),
        ConnectorEffect::NotDispatched,
    )
}
impl ConnectorAdmission for Deny {
    fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        Err(denied())
    }
    fn authorize(
        &self,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorDispatch, ConnectorFailure> {
        Err(denied())
    }
    fn revalidate(
        &self,
        _: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        Err(denied())
    }
}
impl ConnectorMapping for Deny {
    fn inventory(
        &self,
        _: &ConnectorTarget,
        _: &ConnectorManifest,
        _: &ProtocolInventory,
    ) -> Result<ConnectorInventory, Error> {
        Err(Error::Unsupported)
    }
    fn operation(
        &self,
        _: &ConnectorDispatch,
        _: Option<&SourceId>,
        _: &ProtocolInventory,
    ) -> Result<WireOperation, Error> {
        Err(Error::Unsupported)
    }
    fn normalize(
        &self,
        _: &ConnectorDispatch,
        _: Option<&SourceId>,
        _: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        Err(Error::Unsupported)
    }
}
impl TransportFactory for Deny {
    fn open(
        &self,
        _: &CallContext,
        _: &Limits,
    ) -> Result<Box<dyn super::super::transport::Transport>, Error> {
        Err(Error::Unavailable)
    }
}
fn configuration() -> HostConfig {
    HostConfig {
        target: ConnectorTarget {
            connector: NonEmptyText::new("example.records").unwrap(),
            scope: ContextScopeId::new("scope").unwrap(),
        },
        manifest: ConnectorManifest::from_json(include_bytes!(
            "../../../../tests/fixtures/connector-manifest-v1/valid-stdio.json"
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
    }
}

#[test]
fn protocol_result_secret_scan_covers_nested_tool_and_resource_values() {
    let tool = ProtocolResult::Tool {
        content: vec![serde_json::json!({"text":"token-canary"})],
        structured: Some(serde_json::json!({"nested":["nested-secret"]})),
        is_error: false,
    };
    let values = super::protocol_result_text(&tool);
    assert!(values.contains("token-canary"));
    assert!(values.contains("nested-secret"));
    let split = ProtocolResult::Tool {
        content: vec![
            serde_json::json!({"text":"token-can"}),
            serde_json::json!({"text":"ary"}),
        ],
        structured: None,
        is_error: false,
    };
    assert!(super::protocol_result_text(&split).contains("token-canary"));
    let resource = ProtocolResult::Resource {
        contents: vec![serde_json::json!({"text":"resource-secret"})],
    };
    assert!(super::protocol_result_text(&resource).contains("resource-secret"));
}
fn build(config: HostConfig) -> Result<McpConnectorHost, Error> {
    McpConnectorHost::new(
        config,
        Box::new(Deny),
        Arc::new(Deny),
        Box::new(Deny),
        Arc::new(()),
    )
}
#[test]
fn configuration_and_driven_port_are_fail_closed() {
    let mut c = configuration();
    c.target.connector = NonEmptyText::new("wrong").unwrap();
    assert!(matches!(build(c), Err(Error::Configuration)));
    let mut c = configuration();
    c.profile.transports.clear();
    assert!(matches!(build(c), Err(Error::Configuration)));
    let mut c = configuration();
    c.limits.pages = 0;
    assert!(matches!(build(c), Err(Error::Configuration)));
    let host = build(configuration()).unwrap();
    let request = ConnectorRequest {
        target: configuration().target,
        capability: CapabilityId::new("read").unwrap(),
        class: ConnectorOperationClass::Read,
        inventory_revision: ContentDigest::new("a".repeat(64)).unwrap(),
        arguments: BTreeMap::new(),
        task: TaskId::new("task").unwrap(),
        step: PlanStepId::new("step").unwrap(),
        execution: NonEmptyText::new("execution").unwrap(),
        correlation: NonEmptyText::new("correlation").unwrap(),
    };
    let dispatch = ConnectorDispatch {
        request,
        policy: PolicyId::new("policy").unwrap(),
        policy_revision: ContentDigest::new("b".repeat(64)).unwrap(),
        consent_receipt: None,
        budget_reservation: NonEmptyText::new("reservation").unwrap(),
        deadline: UnixTimestamp::new(i64::MAX),
    };
    let runtime: &dyn ConnectorRuntimePort = &host.runtime;
    assert!(runtime.invoke(&dispatch).is_err());
    assert!(
        runtime
            .read_resource(&dispatch, &SourceId::new("source").unwrap())
            .is_err()
    );
    assert!(
        runtime
            .lifecycle(&configuration().target, ConnectorLifecycleCommand::Connect)
            .is_err()
    );
}

#[test]
fn connector_mappings_without_a_tool_schema_are_unavailable_by_default() {
    let config = configuration();
    let capability = CapabilityId::new("records.read").unwrap();
    let revision = ContentDigest::new("a".repeat(64)).unwrap();
    assert_eq!(
        Deny.tool_schema(
            &config.target,
            &config.manifest,
            &ProtocolInventory::default(),
            &capability,
            &revision,
        ),
        Err(Error::Unsupported)
    );
}

#[test]
fn tool_schema_lookup_is_scope_bound_and_requires_current_discovery() {
    let mut host = build(configuration()).unwrap();
    let capability = CapabilityId::new("records.read").unwrap();
    let revision = ContentDigest::new("a".repeat(64)).unwrap();
    assert_eq!(
        host.tool_schema(&configuration().target, &capability, &revision),
        Err(Error::SessionDenied(ConnectorFailureCode::ProtocolError))
    );
    host.runtime.admission = Arc::new(MetadataOnly);
    let mut wrong_scope = configuration().target;
    wrong_scope.scope = ContextScopeId::new("other-scope").unwrap();
    assert_eq!(
        host.tool_schema(&wrong_scope, &capability, &revision),
        Err(Error::Configuration)
    );
    assert_eq!(
        host.tool_schema(&configuration().target, &capability, &revision),
        Err(Error::InvalidState)
    );
}

// Metadata tests explicitly admit the configured target while keeping all
// dispatch authority denied.
struct MetadataOnly;
impl ConnectorAdmission for MetadataOnly {
    fn admit_target(&self, target: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        if target == &configuration().target {
            Ok(())
        } else {
            Err(denied())
        }
    }
    fn authorize(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorDispatch, ConnectorFailure> {
        Deny.authorize(request, source)
    }
    fn revalidate(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        Deny.revalidate(dispatch, source)
    }
}

struct SchemaMapping {
    revoke: Option<Arc<gateway_application::connector_admission::ScopedConnectorAdmission>>,
    wrong_binding: bool,
    delay: std::time::Duration,
}
impl ConnectorMapping for SchemaMapping {
    fn inventory(
        &self,
        _: &ConnectorTarget,
        _: &ConnectorManifest,
        _: &ProtocolInventory,
    ) -> Result<ConnectorInventory, Error> {
        Err(Error::Unsupported)
    }

    fn operation(
        &self,
        _: &ConnectorDispatch,
        _: Option<&SourceId>,
        _: &ProtocolInventory,
    ) -> Result<WireOperation, Error> {
        Err(Error::Unsupported)
    }

    fn normalize(
        &self,
        _: &ConnectorDispatch,
        _: Option<&SourceId>,
        _: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        Err(Error::Unsupported)
    }

    fn tool_schema(
        &self,
        target: &ConnectorTarget,
        _: &ConnectorManifest,
        _: &ProtocolInventory,
        capability: &CapabilityId,
        inventory_revision: &ContentDigest,
    ) -> Result<super::super::mapping::MappedToolSchema, Error> {
        if let Some(admission) = &self.revoke {
            admission.revoke();
        }
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        Ok(super::super::mapping::MappedToolSchema {
            target: target.clone(),
            capability: if self.wrong_binding {
                CapabilityId::new("other.capability").unwrap()
            } else {
                capability.clone()
            },
            inventory_revision: inventory_revision.clone(),
            argument_schema_digest: ContentDigest::new("a".repeat(64)).unwrap(),
            wire_argument_schema_digest: ContentDigest::new("b".repeat(64)).unwrap(),
            result_schema_digest: ContentDigest::new("c".repeat(64)).unwrap(),
            argument_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
            result_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        })
    }
}

fn ready_host(wrong_binding: bool) -> McpConnectorHost {
    ready_host_with_delay(wrong_binding, std::time::Duration::ZERO)
}

fn ready_host_with_delay(wrong_binding: bool, delay: std::time::Duration) -> McpConnectorHost {
    let config = configuration();
    let target = config.target.clone();
    let revision = ContentDigest::new("a".repeat(64)).unwrap();
    let capability = CapabilityId::new("records.read").unwrap();
    let host = McpConnectorHost::new(
        config,
        Box::new(Deny),
        Arc::new(MetadataOnly),
        Box::new(SchemaMapping {
            revoke: None,
            wrong_binding,
            delay,
        }),
        Arc::new(()),
    )
    .unwrap();
    {
        let mut state = host.runtime.state.lock().unwrap();
        state.session.state = SessionState::Ready;
        state.session.inventory = Some(ProtocolInventory {
            tools: vec![json!({"name":"records.read"})],
            ..ProtocolInventory::default()
        });
        state.canonical = Some(ConnectorInventory {
            target,
            revision,
            capabilities: vec![ConnectorCapability {
                capability,
                class: ConnectorOperationClass::Read,
                argument_schema_digest: ContentDigest::new("a".repeat(64)).unwrap(),
                result_schema_digest: ContentDigest::new("c".repeat(64)).unwrap(),
            }],
            resources: vec![],
        });
    }
    host
}

#[test]
fn tool_schema_checks_deadline_after_mapping_returns() {
    let mut config = configuration();
    config.limits.call_timeout = std::time::Duration::from_millis(200);
    let target = config.target.clone();
    let revision = ContentDigest::new("a".repeat(64)).unwrap();
    let capability = CapabilityId::new("records.read").unwrap();
    let host = McpConnectorHost::new(
        config,
        Box::new(Deny),
        Arc::new(MetadataOnly),
        Box::new(SchemaMapping {
            revoke: None,
            wrong_binding: false,
            delay: std::time::Duration::from_millis(250),
        }),
        Arc::new(()),
    )
    .unwrap();
    {
        let mut state = host.runtime.state.lock().unwrap();
        state.session.state = SessionState::Ready;
        state.session.inventory = Some(ProtocolInventory {
            tools: vec![json!({"name":"records.read"})],
            ..ProtocolInventory::default()
        });
        state.canonical = Some(ConnectorInventory {
            target: target.clone(),
            revision: revision.clone(),
            capabilities: vec![ConnectorCapability {
                capability: capability.clone(),
                class: ConnectorOperationClass::Read,
                argument_schema_digest: ContentDigest::new("a".repeat(64)).unwrap(),
                result_schema_digest: ContentDigest::new("c".repeat(64)).unwrap(),
            }],
            resources: vec![],
        });
    }
    assert_eq!(
        host.tool_schema(&target, &capability, &revision),
        Err(Error::Timeout)
    );
}

#[test]
fn tool_schema_uses_the_current_discovery_revision_and_checks_mapping_response_binding() {
    let host = ready_host(false);
    let target = configuration().target;
    let capability = CapabilityId::new("records.read").unwrap();
    let revision = ContentDigest::new("a".repeat(64)).unwrap();
    let schema = host.tool_schema(&target, &capability, &revision).unwrap();
    assert_eq!(schema.target, target);
    assert_eq!(schema.capability, capability);
    assert_eq!(schema.inventory_revision, revision);

    assert_eq!(
        host.tool_schema(
            &configuration().target,
            &CapabilityId::new("records.read").unwrap(),
            &ContentDigest::new("d".repeat(64)).unwrap(),
        ),
        Err(Error::InventoryChanged)
    );

    let wrong = ready_host(true);
    assert_eq!(
        wrong.tool_schema(
            &configuration().target,
            &CapabilityId::new("records.read").unwrap(),
            &ContentDigest::new("a".repeat(64)).unwrap(),
        ),
        Err(Error::InventoryChanged)
    );
}
#[test]
fn typed_arguments_preserve_scalars_and_reject_unqualified_shapes() {
    assert_eq!(typed(&TypedValue::Boolean(true)), Ok(json!(true)));
    assert_eq!(typed(&TypedValue::Integer(42)), Ok(json!(42)));
    assert_eq!(
        typed(&TypedValue::symbol("alpha").unwrap()),
        Ok(json!("alpha"))
    );
    assert_eq!(
        typed(&TypedValue::Decimal(DecimalValue::new(1, 1).unwrap())),
        Err(Error::Schema)
    );
    assert_eq!(
        typed(&TypedValue::Set(vec![TypedValue::Integer(1)])),
        Err(Error::Schema)
    );
    assert_eq!(typed(&TypedValue::Set(vec![])), Err(Error::Schema));
}

struct DiscoveryAdmission(Arc<AtomicBool>);
impl ConnectorAdmission for DiscoveryAdmission {
    fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        if self.0.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(denied())
        }
    }
    fn authorize(
        &self,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorDispatch, ConnectorFailure> {
        Err(denied())
    }
    fn revalidate(
        &self,
        _: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        Err(denied())
    }
}
struct DiscoveryPeer {
    paginate: bool,
    methods: Arc<Mutex<Vec<String>>>,
    response: Option<Vec<u8>>,
}
impl super::super::transport::Transport for DiscoveryPeer {
    fn send(&mut self, frame: &[u8], context: &CallContext) -> Result<(), Error> {
        context.check()?;
        let request: Value = serde_json::from_slice(frame).unwrap();
        let method = request["method"].as_str().unwrap();
        self.methods.lock().unwrap().push(method.to_owned());
        if let Some(id) = request.get("id") {
            let result = match method {
                "initialize" => json!({
                    "protocolVersion":super::super::MCP_VERSION,
                    "serverInfo":{"name":"peer","version":"1.0.0"},
                    "capabilities":{"tools":{}}
                }),
                "tools/list" if self.paginate => json!({"tools":[],"nextCursor":"next-page"}),
                "tools/list" => json!({"tools":[]}),
                _ => panic!("unexpected discovery method"),
            };
            self.response = Some(
                serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"result":result})).unwrap(),
            );
        }
        Ok(())
    }
    fn receive(&mut self, context: &CallContext) -> Result<Vec<u8>, Error> {
        context.check()?;
        self.response.take().ok_or(Error::Timeout)
    }
    fn close(&mut self) {}
}
struct DiscoveryFactory {
    target_allowed: Arc<AtomicBool>,
    revoke_on_open: bool,
    paginate: bool,
    methods: Arc<Mutex<Vec<String>>>,
}
impl TransportFactory for DiscoveryFactory {
    fn open(
        &self,
        context: &CallContext,
        _: &Limits,
    ) -> Result<Box<dyn super::super::transport::Transport>, Error> {
        context.check()?;
        if self.revoke_on_open {
            self.target_allowed.store(false, Ordering::Release);
        }
        Ok(Box::new(DiscoveryPeer {
            paginate: self.paginate,
            methods: self.methods.clone(),
            response: None,
        }))
    }
}
struct DiscoveryMapping(Arc<AtomicBool>);
impl ConnectorMapping for DiscoveryMapping {
    fn inventory(
        &self,
        target: &ConnectorTarget,
        _: &ConnectorManifest,
        _: &ProtocolInventory,
    ) -> Result<ConnectorInventory, Error> {
        self.0.store(false, Ordering::Release);
        Ok(ConnectorInventory {
            target: target.clone(),
            revision: ContentDigest::new("a".repeat(64)).unwrap(),
            capabilities: vec![],
            resources: vec![],
        })
    }
    fn operation(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
        inventory: &ProtocolInventory,
    ) -> Result<WireOperation, Error> {
        Deny.operation(dispatch, source, inventory)
    }
    fn normalize(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
        result: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        Deny.normalize(dispatch, source, result)
    }
}
struct RevokeDiscoveryTarget {
    allowed: Arc<AtomicBool>,
    after_first_page: bool,
}
impl Observer for RevokeDiscoveryTarget {
    fn record(&self, event: Event) {
        if (!self.after_first_page && event.kind == EventKind::State(SessionState::Ready))
            || (self.after_first_page
                && event.kind == EventKind::Completed
                && event.request_id == Some(2))
        {
            self.allowed.store(false, Ordering::Release);
        }
    }
}

#[test]
fn guarded_discovery_reports_late_revocation_and_rechecks_target_before_frames() {
    // Each case revokes a different authority without changing the other.
    // This covers empty early rejection, factory callbacks before initialize,
    // completed handshake before list, and resource revocation after mapping.
    for stage in ["early", "factory", "ready", "page", "mapping"] {
        let target_allowed = Arc::new(AtomicBool::new(stage != "early"));
        let resource_allowed = Arc::new(AtomicBool::new(true));
        let methods = Arc::new(Mutex::new(Vec::new()));
        let config = configuration();
        let target = config.target.clone();
        let observer: Arc<dyn Observer> = if matches!(stage, "ready" | "page") {
            Arc::new(RevokeDiscoveryTarget {
                allowed: target_allowed.clone(),
                after_first_page: stage == "page",
            })
        } else {
            Arc::new(())
        };
        let host = McpConnectorHost::new(
            config,
            Box::new(DiscoveryFactory {
                target_allowed: target_allowed.clone(),
                revoke_on_open: stage == "factory",
                paginate: stage == "page",
                methods: methods.clone(),
            }),
            Arc::new(DiscoveryAdmission(target_allowed)),
            Box::new(DiscoveryMapping(resource_allowed.clone())),
            observer,
        )
        .unwrap();
        let context = CallContext::new(
            NonEmptyText::new("discovery-revocation").unwrap(),
            Instant::now() + Duration::from_secs(1),
        )
        .with_final_guard(Arc::new(move || {
            if resource_allowed.load(Ordering::Acquire) {
                Ok(())
            } else {
                Err(Error::SessionDenied(ConnectorFailureCode::PermissionDenied))
            }
        }));
        let error = host
            .discover_with_context_and_preflight(&target, &context, &|| Ok(()))
            .unwrap_err();
        let sent = methods.lock().unwrap();
        if matches!(stage, "early" | "factory") {
            assert_eq!(error.effect, ConnectorEffect::NotDispatched, "{stage}");
            assert!(sent.is_empty(), "{stage}: {sent:?}");
        } else {
            assert_eq!(error.effect, ConnectorEffect::NoMutation, "{stage}");
            assert_eq!(sent[0], "initialize");
            assert_eq!(sent[1], "notifications/initialized");
            assert_eq!(
                sent.iter().filter(|m| *m == "tools/list").count(),
                usize::from(matches!(stage, "page" | "mapping")),
            );
        }
        assert!(host.runtime.state.lock().unwrap().canonical.is_none());
    }
}

#[test]
fn absent_recovery_contract_is_explicitly_unsafe_and_disabled_health_is_unavailable() {
    let request = ConnectorRequest {
        target: configuration().target,
        capability: CapabilityId::new("read").unwrap(),
        class: ConnectorOperationClass::Read,
        inventory_revision: ContentDigest::new("a".repeat(64)).unwrap(),
        arguments: BTreeMap::new(),
        task: TaskId::new("task").unwrap(),
        step: PlanStepId::new("step").unwrap(),
        execution: NonEmptyText::new("execution").unwrap(),
        correlation: NonEmptyText::new("correlation").unwrap(),
    };
    assert_eq!(
        Deny.recovery_support(&request, None, &ProtocolInventory::default())
            .unwrap(),
        gateway_application::connector_resilience::RecoverySupport::default()
    );
    let host = build(configuration()).unwrap();
    host.lifecycle(&request.target, ConnectorLifecycleCommand::Disable)
        .unwrap();
    assert_eq!(
        host.health_snapshot().health,
        gateway_application::connector_resilience::ConnectorHealth::Unavailable
    );
}

#[test]
fn tool_schema_does_not_release_metadata_revoked_inside_mapping() {
    use gateway_application::connector_admission::*;
    let config = configuration();
    let binding = ConnectorScopeBinding {
        project: NonEmptyText::new("project").unwrap(),
        scope: config.target.scope.clone(),
        session: None,
        request: None,
    };
    let registry = ConnectorRegistry::rebuild(
        vec![ConnectorRegistration {
            binding: binding.clone(),
            manifest: config.manifest,
            enabled: true,
        }],
        &config.profile,
    )
    .unwrap();
    let admission = registry
        .bind(&binding, &config.target, Arc::new(MetadataOnly))
        .unwrap();
    let mut host = ready_host(false);
    host.runtime.admission = admission.clone();
    host.runtime.mapping = Box::new(SchemaMapping {
        revoke: Some(admission),
        wrong_binding: false,
        delay: std::time::Duration::ZERO,
    });
    assert_eq!(
        host.tool_schema(
            &config.target,
            &CapabilityId::new("records.read").unwrap(),
            &ContentDigest::new("a".repeat(64)).unwrap()
        ),
        Err(Error::SessionDenied(ConnectorFailureCode::PermissionDenied))
    );
}
