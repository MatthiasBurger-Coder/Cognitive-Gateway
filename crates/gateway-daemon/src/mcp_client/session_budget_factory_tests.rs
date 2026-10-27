use super::*;
use crate::{
    mcp_client::{
        ConnectorMapping, HostConfig, Limits, ProtocolInventory, ProtocolResult, WireOperation,
        transport::{Transport, TransportFactory},
    },
    session_resource_test_support::with_preparation_resources,
};
use gateway_application::connector_manifest::*;
use gateway_application::sessions::SessionResourcePlan;
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

fn text(value: &str) -> NonEmptyText {
    NonEmptyText::new(value).unwrap()
}
fn amounts() -> SessionResourceAmounts {
    SessionResourceAmounts {
        connector_calls: 2,
        cost_microunits: 2,
        retrieval_rounds: 2,
        ..Default::default()
    }
}
fn context() -> CallContext {
    CallContext::new(
        text("factory-discovery"),
        Instant::now() + Duration::from_secs(5),
    )
}
struct NoIo(Arc<AtomicUsize>);
impl TransportFactory for NoIo {
    fn open(&self, _: &CallContext, _: &Limits) -> Result<Box<dyn Transport>, Error> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(Error::Unavailable)
    }
}
struct Denied;
impl ConnectorAdmissionPort for Denied {
    fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        Err(failure(
            ConnectorFailureCode::PermissionDenied,
            &text("host-denied"),
            ConnectorEffect::NotDispatched,
        ))
    }
    fn authorize(
        &self,
        r: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorDispatch, ConnectorFailure> {
        Err(failure(
            ConnectorFailureCode::PermissionDenied,
            &r.correlation,
            ConnectorEffect::NotDispatched,
        ))
    }
    fn revalidate(
        &self,
        d: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        Err(failure(
            ConnectorFailureCode::PermissionDenied,
            &d.request.correlation,
            ConnectorEffect::NotDispatched,
        ))
    }
}
impl ConnectorMapping for Denied {
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
fn host(scope: gateway_domain::ContextScopeId, io: Arc<AtomicUsize>) -> Arc<McpConnectorHost> {
    Arc::new(
        McpConnectorHost::new(
            HostConfig {
                target: ConnectorTarget {
                    connector: text("example.records"),
                    scope,
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
            },
            Box::new(NoIo(io)),
            Arc::new(Denied),
            Box::new(Denied),
            Arc::new(()),
        )
        .unwrap(),
    )
}
struct Source {
    view: ReadonlyAllocationView,
    audits: AtomicUsize,
    validation_hook: Option<Arc<dyn Fn() -> Result<(), SessionError> + Send + Sync>>,
}
impl Source {
    fn audit(&self) -> gateway_domain::ContentDigest {
        self.audits.fetch_add(1, Ordering::SeqCst);
        self.view.allocation().reservation_id().clone()
    }
}
impl SessionResourceAuthorityPort for Source {
    fn validate_allocation(&self, a: &SessionResourceAllocation) -> Result<(), SessionError> {
        if std::ptr::eq(a, self.view.allocation()) {
            if let Some(hook) = &self.validation_hook {
                hook()?;
            }
            Ok(())
        } else {
            Err(SessionError::AuthorityDenied)
        }
    }
    fn validate_connector_discovery(
        &self,
        a: &SessionResourceAllocation,
        t: &ConnectorTarget,
    ) -> Result<(), SessionError> {
        self.validate_allocation(a)?;
        if t.scope == self.view.allocation().binding().scope {
            Ok(())
        } else {
            Err(SessionError::ScopeDenied)
        }
    }
}
impl SessionConnectorQuotePort for Source {
    fn quote_discovery(
        &self,
        a: &SessionResourceAllocation,
        _: &ConnectorTarget,
    ) -> Result<SessionConnectorQuote, SessionError> {
        self.validate_allocation(a)?;
        Ok(SessionConnectorQuote {
            amounts: SessionResourceAmounts {
                connector_calls: 1,
                cost_microunits: 1,
                retrieval_rounds: 1,
                ..Default::default()
            },
            cost_unit: a.cost_unit().clone(),
        })
    }
    fn quote_request(
        &self,
        _: &SessionResourceAllocation,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<SessionConnectorQuote, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn validate_inventory(
        &self,
        _: &SessionResourceAllocation,
        _: &SessionConnectorQuote,
        _: &ConnectorInventory,
    ) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn validate_observation(
        &self,
        _: &SessionResourceAllocation,
        _: &SessionConnectorQuote,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
        _: &ConnectorObservation,
    ) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}
fn components(
    view: ReadonlyAllocationView,
    scope: gateway_domain::ContextScopeId,
    io: Arc<AtomicUsize>,
) -> (SessionConnectorComponents, Arc<Source>) {
    let source = Arc::new(Source {
        view,
        audits: AtomicUsize::new(0),
        validation_hook: None,
    });
    (
        SessionConnectorComponents {
            pure_quote: None,
            full_join: None,
            host: host(scope, io),
            authority: source.clone(),
            quotes: source.clone(),
        },
        source,
    )
}
#[test]
fn factory_admission_checks_real_claim_before_callback_and_rechecks_after_callbacks() {
    for case in [
        "revoked-before",
        "cancelled-before",
        "mixed-slice",
        "no-connector",
        "revoke-callback",
        "cancel-callback",
        "foreign-host",
        "expired-context",
        "child-denied",
        "revoke-child-validation",
        "cancel-child-validation",
    ] {
        with_preparation_resources(
            gateway_domain::ContextScopeId::new("factory-scope").unwrap(),
            SessionResourcePlan {
                preparation: SessionResourceAmounts {
                    model_attempts: 1,
                    ..amounts()
                },
                ..Default::default()
            },
            |resources, _, base| {
                let io = Arc::new(AtomicUsize::new(0));
                let mut called = false;
                let mut context = context();
                let cancel = context.clone();
                if case == "revoked-before" {
                    base.revoke();
                }
                if case == "cancelled-before" {
                    context.cancel();
                }
                if case == "expired-context" {
                    context.deadline = Instant::now();
                }
                let slice = match case {
                    "mixed-slice" => SessionResourceAmounts {
                        model_attempts: 1,
                        ..amounts()
                    },
                    "no-connector" => SessionResourceAmounts {
                        retrieval_rounds: 1,
                        ..Default::default()
                    },
                    _ => amounts(),
                };
                let result = SessionConnectorRun::with_factory(
                    resources.allocate(slice).unwrap(),
                    Arc::new(base.clone()),
                    context,
                    |view| {
                        called = true;
                        if case == "revoke-callback" {
                            base.revoke();
                        }
                        if case == "cancel-callback" {
                            cancel.cancel();
                        }
                        let scope = if case == "foreign-host" {
                            gateway_domain::ContextScopeId::new("foreign").unwrap()
                        } else {
                            view.allocation().binding().scope.clone()
                        };
                        let (mut parts, _) = components(view.clone(), scope, io.clone());
                        if matches!(
                            case,
                            "child-denied" | "revoke-child-validation" | "cancel-child-validation"
                        ) {
                            let base = base.clone();
                            let cancel = cancel.clone();
                            parts.authority = Arc::new(Source {
                                view,
                                audits: AtomicUsize::new(0),
                                validation_hook: Some(Arc::new(move || {
                                    if case == "child-denied" {
                                        return Err(SessionError::AuthorityDenied);
                                    }
                                    if case == "revoke-child-validation" {
                                        base.revoke();
                                    }
                                    if case == "cancel-child-validation" {
                                        cancel.cancel();
                                    }
                                    Ok(())
                                })),
                            });
                        }
                        Ok(parts)
                    },
                );
                assert!(result.is_err(), "{case}");
                assert_eq!(
                    called,
                    matches!(
                        case,
                        "revoke-callback"
                            | "cancel-callback"
                            | "foreign-host"
                            | "child-denied"
                            | "revoke-child-validation"
                            | "cancel-child-validation"
                    ),
                    "{case}"
                );
                assert_eq!(io.load(Ordering::SeqCst), 0, "{case}");
            },
        );
    }
}
#[test]
fn readonly_source_shares_exact_allocation_and_cannot_replace_live_base_checks() {
    with_preparation_resources(
        gateway_domain::ContextScopeId::new("factory-scope").unwrap(),
        SessionResourcePlan {
            preparation: amounts(),
            ..Default::default()
        },
        |resources, _, base| {
            let allocation = resources.allocate(amounts()).unwrap();
            let initial_context = context();
            let deadline = allocation.deadline().min(initial_context.deadline);
            let reservation = allocation.reservation_id().clone();
            let io = Arc::new(AtomicUsize::new(0));
            let mut retained = None;
            let run = SessionConnectorRun::with_factory(
                allocation,
                Arc::new(base.clone()),
                initial_context,
                |view| {
                    let scope = view.allocation().binding().scope.clone();
                    assert_eq!(view.allocation().reservation_id(), &reservation);
                    let (parts, source) = components(view.clone(), scope, io.clone());
                    assert_eq!(source.audit(), reservation);
                    retained = Some(source);
                    Ok(parts)
                },
            )
            .unwrap();
            assert_eq!(run.context.deadline, deadline);
            assert!(
                run.pure_quote.is_none(),
                "injected generic factory quotes cannot obtain a sealed pure quote proof"
            );
            let source = retained.unwrap();
            assert!(std::ptr::eq(
                run.allocation.as_ref(),
                source.view.allocation()
            ));
            assert_eq!(source.audit(), reservation);
            let target = run.host.configured_target().clone();
            // The child still recognizes this immutable view after base revocation;
            // the run must independently deny the now-invalid durable claim.
            base.revoke();
            assert!(source.validate_allocation(source.view.allocation()).is_ok());
            assert_eq!(
                run.discover(&target).unwrap_err().effect,
                ConnectorEffect::NotDispatched
            );
            assert_eq!(run.usage().unwrap().connector_calls, 0);
            assert_eq!(io.load(Ordering::SeqCst), 0);
            drop(run);
            assert_eq!(
                resources.remaining().connector_calls,
                0,
                "dropping the run never remints its consumed child slice"
            );
            assert_eq!(
                source.audit(),
                reservation,
                "metadata remains auditable after run drop without yielding a spending cursor"
            );
        },
    );
}

#[test]
fn sealed_joined_guards_still_reject_revocation_inside_child_validation() {
    for final_only in [false, true] {
        with_preparation_resources(
            gateway_domain::ContextScopeId::new("factory-scope").unwrap(),
            SessionResourcePlan {
                preparation: amounts(),
                ..Default::default()
            },
            |resources, _, base| {
                let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let io = Arc::new(AtomicUsize::new(0));
                let run = SessionConnectorRun::with_factory(
                    resources.allocate(amounts()).unwrap(),
                    Arc::new(base.clone()),
                    context(),
                    |view| {
                        let scope = view.allocation().binding().scope.clone();
                        let (mut parts, _) = components(view.clone(), scope, io.clone());
                        let base = base.clone();
                        let armed = armed.clone();
                        parts.authority = Arc::new(Source {
                            view,
                            audits: AtomicUsize::new(0),
                            validation_hook: Some(Arc::new(move || {
                                if armed.load(Ordering::SeqCst) {
                                    base.revoke();
                                }
                                Ok(())
                            })),
                        });
                        Ok(parts)
                    },
                )
                .unwrap();
                assert!(matches!(run.authority.as_ref(), RunAuthority::Joined(_)));
                let target = run.host.configured_target().clone();
                let quote = run
                    .quotes
                    .quote_discovery(&run.allocation, &target)
                    .unwrap();
                armed.store(true, Ordering::SeqCst);
                let operation = Operation::Discovery(target);
                let guard = if final_only {
                    run.final_guard(operation)
                } else {
                    run.operation_guard(operation, quote)
                };
                assert!(
                    guard.check().is_err(),
                    "current base must be checked after the child callback"
                );
                assert_eq!(io.load(Ordering::SeqCst), 0);
                assert_eq!(run.usage().unwrap().connector_calls, 0);
            },
        );
    }
}

impl QualifiedConnectorAuthority for Source {}

#[test]
fn qualified_join_retains_both_current_authorities_and_refuses_unsupported_requests() {
    with_preparation_resources(
        gateway_domain::ContextScopeId::new("factory-scope").unwrap(),
        SessionResourcePlan {
            preparation: amounts(),
            ..Default::default()
        },
        |resources, _, base| {
            let io = Arc::new(AtomicUsize::new(0));
            let run = SessionConnectorRun::with_factory(
                resources.allocate(amounts()).unwrap(),
                Arc::new(base.clone()),
                context(),
                |view| {
                    let source = Arc::new(Source {
                        view: view.clone(),
                        audits: AtomicUsize::new(0),
                        validation_hook: None,
                    });
                    let bundle = super::super::scalar_quotes::tests::host(
                        view,
                        true,
                        Limits::default(),
                        io.clone(),
                    )
                    .unwrap();
                    Ok(SessionConnectorComponents::from_qualified_joined(
                        bundle, source,
                    ))
                },
            )
            .unwrap();
            let target = run.host.configured_target().clone();
            run.authority
                .validate_connector_discovery(&run.allocation, &target)
                .unwrap();
            let request = ConnectorRequest {
                target,
                capability: gateway_domain::CapabilityId::new("records.read").unwrap(),
                class: ConnectorOperationClass::Read,
                inventory_revision: gateway_domain::ContentDigest::new("a".repeat(64)).unwrap(),
                arguments: Default::default(),
                task: gateway_domain::TaskId::new("task").unwrap(),
                step: run.allocation.binding().step.clone(),
                execution: text("exec"),
                correlation: text("request"),
            };
            assert_eq!(
                run.authority
                    .validate_connector_allocation(&run.allocation, &request, None),
                Err(SessionError::UnsupportedCapability)
            );
            base.revoke();
            assert!(
                run.authority
                    .validate_connector_discovery(&run.allocation, &request.target)
                    .is_err()
            );
            assert_eq!(io.load(Ordering::SeqCst), 0);
        },
    );
}

#[test]
fn constructor_scope_resources_discovery_scope_and_poison_fail_before_io() {
    for case in ["mixed", "foreign", "discovery-scope", "poison"] {
        with_preparation_resources(
            gateway_domain::ContextScopeId::new("factory-scope").unwrap(),
            SessionResourcePlan {
                preparation: SessionResourceAmounts {
                    model_attempts: 1,
                    ..amounts()
                },
                ..Default::default()
            },
            |resources, _, base| {
                let io = Arc::new(AtomicUsize::new(0));
                if case == "mixed" || case == "foreign" {
                    let allocation = resources
                        .allocate(if case == "mixed" {
                            SessionResourceAmounts {
                                model_attempts: 1,
                                ..amounts()
                            }
                        } else {
                            amounts()
                        })
                        .unwrap();
                    let h = host(
                        if case == "foreign" {
                            gateway_domain::ContextScopeId::new("foreign").unwrap()
                        } else {
                            allocation.binding().scope.clone()
                        },
                        io.clone(),
                    );
                    let quote = Arc::new(Denied);
                    let result = SessionConnectorRun::new(
                        h,
                        allocation,
                        Arc::new(base.clone()),
                        quote,
                        context(),
                    );
                    assert!(matches!(
                        result,
                        Err(SessionError::InvalidInput | SessionError::ScopeDenied)
                    ));
                } else {
                    let run = SessionConnectorRun::with_factory(
                        resources.allocate(amounts()).unwrap(),
                        Arc::new(base.clone()),
                        context(),
                        |view| {
                            let scope = view.allocation().binding().scope.clone();
                            Ok(components(view, scope, io.clone()).0)
                        },
                    )
                    .unwrap();
                    let mut target = run.host.configured_target().clone();
                    if case == "discovery-scope" {
                        target.scope = gateway_domain::ContextScopeId::new("wrong").unwrap();
                        assert_eq!(
                            run.discover(&target).unwrap_err().code,
                            ConnectorFailureCode::ScopeMismatch
                        );
                    } else {
                        assert!(
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                let _held = run.usage.lock().unwrap();
                                panic!("test poisoned allowance");
                            }))
                            .is_err()
                        );
                        assert_eq!(
                            run.discover(&target).unwrap_err().code,
                            ConnectorFailureCode::Unavailable
                        );
                    }
                }
                assert_eq!(io.load(Ordering::SeqCst), 0);
            },
        );
    }
    for (error, expected) in [
        (
            SessionError::ScopeDenied,
            ConnectorFailureCode::ScopeMismatch,
        ),
        (SessionError::Unavailable, ConnectorFailureCode::Unavailable),
        (
            SessionError::StorageUnavailable,
            ConnectorFailureCode::Unavailable,
        ),
    ] {
        assert_eq!(session_code(error), expected);
    }
}

impl SessionConnectorQuotePort for Denied {
    fn quote_discovery(
        &self,
        _: &SessionResourceAllocation,
        _: &ConnectorTarget,
    ) -> Result<SessionConnectorQuote, SessionError> {
        panic!("invalid constructor requested a quote")
    }
    fn quote_request(
        &self,
        _: &SessionResourceAllocation,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<SessionConnectorQuote, SessionError> {
        panic!("invalid constructor requested a quote")
    }
    fn validate_inventory(
        &self,
        _: &SessionResourceAllocation,
        _: &SessionConnectorQuote,
        _: &ConnectorInventory,
    ) -> Result<(), SessionError> {
        panic!("invalid constructor received inventory")
    }
    fn validate_observation(
        &self,
        _: &SessionResourceAllocation,
        _: &SessionConnectorQuote,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
        _: &ConnectorObservation,
    ) -> Result<(), SessionError> {
        panic!("invalid constructor received an observation")
    }
}

#[test]
fn terminal_inner_attempt_preserves_uncertainty_before_quotes_or_admission() {
    with_preparation_resources(
        gateway_domain::ContextScopeId::new("factory-scope").unwrap(),
        SessionResourcePlan {
            preparation: amounts(),
            ..Default::default()
        },
        |resources, _, base| {
            let io = Arc::new(AtomicUsize::new(0));
            let run = SessionConnectorRun::with_factory(
                resources.allocate(amounts()).unwrap(),
                Arc::new(base.clone()),
                context(),
                |view| {
                    let scope = view.allocation().binding().scope.clone();
                    Ok(components(view, scope, io.clone()).0)
                },
            )
            .unwrap();
            let request = ConnectorRequest {
                target: run.host.configured_target().clone(),
                capability: gateway_domain::CapabilityId::new("records.read").unwrap(),
                class: ConnectorOperationClass::Read,
                inventory_revision: gateway_domain::ContentDigest::new("a".repeat(64)).unwrap(),
                arguments: Default::default(),
                task: gateway_domain::TaskId::new("task").unwrap(),
                step: run.allocation.binding().step.clone(),
                execution: text("exec"),
                correlation: text("request"),
            };
            let terminal = failure(
                ConnectorFailureCode::Timeout,
                &request.correlation,
                ConnectorEffect::Unknown,
            );
            let mut usage = run.usage.lock().unwrap();
            usage.terminal = Some(terminal.clone());
            let context = context();
            assert_eq!(
                run.request_once(
                    &mut usage,
                    &request,
                    None,
                    &context,
                    &std::cell::Cell::new(context.deadline)
                ),
                Err(terminal)
            );
            assert_eq!(usage.amounts.connector_calls, 0);
            assert_eq!(io.load(Ordering::SeqCst), 0);
        },
    );
}

#[test]
fn unavailable_factory_fails_without_constructing_an_effect_host() {
    with_preparation_resources(
        gateway_domain::ContextScopeId::new("factory-scope").unwrap(),
        SessionResourcePlan {
            preparation: amounts(),
            ..Default::default()
        },
        |resources, _, base| {
            let result = SessionConnectorRun::with_factory(
                resources.allocate(amounts()).unwrap(),
                Arc::new(base.clone()),
                context(),
                |_| Err(SessionError::StorageUnavailable),
            );
            assert!(matches!(result, Err(SessionError::StorageUnavailable)));
        },
    );
}
