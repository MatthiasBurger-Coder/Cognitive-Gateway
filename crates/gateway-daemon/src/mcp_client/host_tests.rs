use super::*;
use gateway_application::connector_manifest::*;
use gateway_domain::*;
use std::collections::BTreeSet;
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
