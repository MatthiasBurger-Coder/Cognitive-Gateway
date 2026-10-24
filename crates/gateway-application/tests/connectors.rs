//! Compile conformance for independently replaceable adapters, not live MCP proof.
use gateway_application::{
    connectors::*,
    ports::{inbound::ConnectorApplicationPort, outbound::ConnectorRuntimePort},
};
use gateway_domain::{
    CapabilityId, ContentDigest, ContextScopeId, NonEmptyText, PlanStepId, PolicyId, SourceId,
    TaskId, UnixTimestamp,
};
use std::collections::BTreeMap;

struct ProcessFixture;
struct RemoteFixture;

fn denied() -> ConnectorFailure {
    ConnectorFailure {
        code: ConnectorFailureCode::PermissionDenied,
        effect: ConnectorEffect::NotDispatched,
        correlation: NonEmptyText::new("correlation-1").unwrap(),
    }
}

// Two different outer types use exactly the same driven contract.
macro_rules! runtime_fixture {
    ($adapter:ty) => {
        impl ConnectorRuntimePort for $adapter {
            fn discover(
                &self,
                _: &ConnectorTarget,
            ) -> Result<ConnectorInventory, ConnectorFailure> {
                Err(denied())
            }
            fn invoke(
                &self,
                _: &ConnectorDispatch,
            ) -> Result<ConnectorObservation, ConnectorFailure> {
                Err(denied())
            }
            fn read_resource(
                &self,
                _: &ConnectorDispatch,
                _: &SourceId,
            ) -> Result<ConnectorObservation, ConnectorFailure> {
                Err(denied())
            }
            fn lifecycle(
                &self,
                _: &ConnectorTarget,
                _: ConnectorLifecycleCommand,
            ) -> Result<ConnectorLifecycleState, ConnectorFailure> {
                Err(denied())
            }
        }
    };
}
runtime_fixture!(ProcessFixture);
runtime_fixture!(RemoteFixture);

// A driving implementation accepts proposals, never an adapter or wire object.
struct DenyingApplication;
impl ConnectorApplicationPort for DenyingApplication {
    fn discover(&self, _: &ConnectorTarget) -> Result<ConnectorInventory, ConnectorFailure> {
        Err(denied())
    }
    fn invoke(&self, _: &ConnectorRequest) -> Result<ConnectorObservation, ConnectorFailure> {
        Err(denied())
    }
    fn read_resource(
        &self,
        _: &ConnectorRequest,
        _: &SourceId,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        Err(denied())
    }
    fn lifecycle(
        &self,
        _: &ConnectorTarget,
        _: ConnectorLifecycleCommand,
    ) -> Result<ConnectorLifecycleState, ConnectorFailure> {
        Err(denied())
    }
}

fn request() -> ConnectorRequest {
    ConnectorRequest {
        target: ConnectorTarget {
            connector: NonEmptyText::new("configured-instance").unwrap(),
            scope: ContextScopeId::new("scope-1").unwrap(),
        },
        capability: CapabilityId::new("external.record.read").unwrap(),
        class: ConnectorOperationClass::Read,
        inventory_revision: ContentDigest::new("a".repeat(64)).unwrap(),
        arguments: BTreeMap::new(),
        task: TaskId::new("task-1").unwrap(),
        step: PlanStepId::new("step-1").unwrap(),
        execution: NonEmptyText::new("execution-1").unwrap(),
        correlation: NonEmptyText::new("correlation-1").unwrap(),
    }
}

#[test]
fn independent_runtime_types_compile_against_all_driven_operations() {
    let dispatch = ConnectorDispatch {
        request: request(),
        policy: PolicyId::new("policy-1").unwrap(),
        policy_revision: ContentDigest::new("b".repeat(64)).unwrap(),
        consent_receipt: None,
        budget_reservation: NonEmptyText::new("reservation-1").unwrap(),
        deadline: UnixTimestamp::new(123),
    };
    let source = SourceId::new("source-1").unwrap();
    for runtime in [&ProcessFixture as &dyn ConnectorRuntimePort, &RemoteFixture] {
        assert_eq!(runtime.discover(&dispatch.request.target), Err(denied()));
        assert_eq!(runtime.invoke(&dispatch), Err(denied()));
        assert_eq!(runtime.read_resource(&dispatch, &source), Err(denied()));
        assert_eq!(
            runtime.lifecycle(&dispatch.request.target, ConnectorLifecycleCommand::Connect),
            Err(denied())
        );
    }
}

#[test]
fn driving_operations_use_canonical_proposals_and_preserve_typed_denial() {
    let application: &dyn ConnectorApplicationPort = &DenyingApplication;
    let request = request();
    assert_eq!(application.discover(&request.target), Err(denied()));
    assert_eq!(application.invoke(&request), Err(denied()));
    assert_eq!(
        application.read_resource(&request, &SourceId::new("source-1").unwrap()),
        Err(denied())
    );
    assert_eq!(
        application.lifecycle(&request.target, ConnectorLifecycleCommand::Disable),
        Err(denied())
    );
}
