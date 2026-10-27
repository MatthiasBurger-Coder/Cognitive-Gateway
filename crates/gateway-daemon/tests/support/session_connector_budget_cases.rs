//! Reserved-connector component qualification through the real coordinator,
//! CG-09 engine, mapped MCP host and an independent stdio protocol peer.
use super::{Mapping, config, digest, text};
use gateway_application::{
    connector_manifest::*,
    connector_policy::*,
    connectors::*,
    output_parsing::parse_invocation_output,
    output_verification::{ConstraintReport, OutputVerificationPort, ParsedOutput, verify_output},
    prompt_ir::{InvocationAdmissionPort, InvocationIr, admit_invocation_ir},
    sessions::*,
};
use gateway_daemon::mcp_client::{
    CallContext, ConnectorMapping, Error, HostConfig, Limits, McpConnectorHost, ProtocolInventory,
    ProtocolResult, WireOperation, session_budget::*, transport::*,
};
use gateway_domain::*;
use gateway_policy::*;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[path = "session_resource_allocation.rs"]
#[allow(dead_code)]
mod allocation_fixture;
#[path = "../../../../tests/fixtures/prompt-rendering-v1/support.rs"]
#[allow(dead_code)]
mod model_fixture;

type Hook = Arc<dyn Fn() + Send + Sync>;
struct Producer;
impl SemanticReferenceValidator for Producer {
    fn validate(
        &self,
        _: &SemanticTaskData,
        _: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl InvocationAdmissionPort for Producer {
    fn validate_current(
        &self,
        _: &InvocationIr,
    ) -> Result<(), gateway_domain::prompt_ir::InvocationIrError> {
        Ok(())
    }
}
impl OutputVerificationPort for Producer {
    fn resolve_reference(
        &self,
        _: &InvocationIr,
        _: &gateway_domain::output_verification::OutputReference,
    ) -> Result<(), gateway_domain::output_verification::VerificationError> {
        Ok(())
    }
    fn evaluate(
        &self,
        ir: &InvocationIr,
        output: &ParsedOutput,
    ) -> Result<ConstraintReport, gateway_domain::output_verification::VerificationError> {
        use gateway_domain::{
            semantic_output::CheckOutcome, semantic_task::SemanticVerificationCheck,
        };
        Ok(ConstraintReport {
            binding: output.binding(),
            policy: CheckOutcome::Passed,
            process: CheckOutcome::Passed,
            task_completeness: CheckOutcome::Passed,
            semantic_output: CheckOutcome::Passed,
            checks: ir
                .prompt()
                .handoff()
                .task()
                .data()
                .verification_contract
                .checks
                .iter()
                .filter(|check| **check != SemanticVerificationCheck::OutputSchemaValid)
                .map(|check| (*check, CheckOutcome::Passed))
                .collect(),
        })
    }
}
fn prepared() -> PreparedConnectorTurn {
    let handoff = model_fixture::handoff();
    let invocation = admit_invocation_ir(
        SchemaVersion::V1,
        handoff.clone(),
        model_fixture::prepared(&handoff, 100_000, false),
        BTreeMap::new(),
        &Producer,
    )
    .unwrap();
    let raw=serde_json::to_vec(&json!({"schema_version":"1.0","invocation":invocation.identity(),"result":"entity","references":[],"tools":[]})).unwrap();
    let parsed = parse_invocation_output(&invocation, &raw).unwrap();
    let verified = verify_output(&invocation, parsed, &Producer, vec![]).unwrap();
    let scope = invocation.prepared().invocation().data().task.scope.clone();
    PreparedConnectorTurn::new(
        invocation,
        verified,
        ScopedConnectorSnapshotPayload::new(scope, br#"{"snapshot":"before"}"#.to_vec()).unwrap(),
    )
    .unwrap()
}
fn slice(calls: u64) -> SessionResourceAmounts {
    SessionResourceAmounts {
        connector_calls: calls,
        cost_microunits: calls,
        retrieval_rounds: calls,
        retrieval_results: 10 * calls,
        retrieval_tokens: 20_000 * calls,
        ..Default::default()
    }
}
fn plan(phase: SessionResourcePhase, calls: u64) -> SessionResourcePlan {
    match phase {
        SessionResourcePhase::Preparation => SessionResourcePlan {
            preparation: slice(calls),
            ..Default::default()
        },
        SessionResourcePhase::Execution => SessionResourcePlan {
            execution: slice(calls),
            ..Default::default()
        },
        SessionResourcePhase::AfterSnapshotRecovery => SessionResourcePlan {
            after_snapshot_recovery: slice(calls),
            ..Default::default()
        },
        _ => panic!("unsupported test phase"),
    }
}
fn in_phase(
    phase: SessionResourcePhase,
    calls: u64,
    mut callback: impl FnMut(
        &mut SessionPhaseResources,
        &SessionCheckpoint,
        &allocation_fixture::PreparationAuthority,
    ),
) {
    let turn = prepared();
    let scope = turn
        .invocation()
        .prepared()
        .invocation()
        .data()
        .task
        .scope
        .clone();
    match phase {
        SessionResourcePhase::Preparation => {
            allocation_fixture::with_preparation_resources(
                scope,
                plan(phase, calls),
                &mut callback,
            );
        }
        SessionResourcePhase::Execution => {
            allocation_fixture::with_execution_resources(
                scope,
                plan(phase, calls),
                turn,
                &mut callback,
            );
        }
        SessionResourcePhase::AfterSnapshotRecovery => {
            allocation_fixture::with_recovery_resources(
                scope,
                plan(phase, calls),
                turn,
                &mut callback,
            );
        }
        _ => panic!("unsupported test phase"),
    }
}
fn target(resources: &SessionPhaseResources) -> ConnectorTarget {
    ConnectorTarget {
        connector: text("example.records"),
        scope: resources.binding().scope.clone(),
    }
}
fn request(
    resources: &SessionPhaseResources,
    checkpoint: &SessionCheckpoint,
    class: ConnectorOperationClass,
) -> ConnectorRequest {
    let task = checkpoint
        .connector_step_snapshots
        .last()
        .map(|r| r.binding.task.clone())
        .unwrap_or_else(|| TaskId::new("preparation-task").unwrap());
    ConnectorRequest {
        target: target(resources),
        capability: CapabilityId::new(if class == ConnectorOperationClass::Mutate {
            "records.z-mutate"
        } else {
            "records.read"
        })
        .unwrap(),
        class,
        inventory_revision: digest(),
        arguments: BTreeMap::from([(text("query"), TypedValue::string("hello").unwrap())]),
        task,
        step: resources.binding().step.clone(),
        execution: text("connector-execution"),
        correlation: text("reserved-call"),
    }
}
struct Policy {
    reservation: String,
    deadline: UnixTimestamp,
    deny: AtomicBool,
    wrong_reservation: AtomicBool,
    audits: AtomicUsize,
    audit_hook: Mutex<Option<Hook>>,
}
impl Policy {
    fn snapshot(&self, request: &ConnectorRequest) -> ConnectorPolicySnapshot {
        let cap = request.capability.clone();
        let class = if matches!(
            request.class,
            ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
        ) {
            CapabilityClass::Mutate
        } else {
            CapabilityClass::Inspect
        };
        ConnectorPolicySnapshot {
            step: request.step.clone(),
            resolved: true,
            has_prerequisites: false,
            constraints: BTreeSet::new(),
            preconditions: BTreeSet::new(),
            authority: PolicyAuthority {
                policies: vec![
                    PolicyDefinition::new(
                        PolicyId::new("policy").unwrap(),
                        "connector budget fixture",
                        [cap.clone()],
                    )
                    .unwrap(),
                ],
                capabilities: BTreeMap::from([(
                    cap.clone(),
                    CapabilityDefinition::new(cap.clone(), class),
                )]),
                ..Default::default()
            },
            facts: StepFacts {
                authorizations: BTreeMap::from([(
                    cap.clone(),
                    if self.deny.load(Ordering::SeqCst) {
                        Approval::Denied
                    } else {
                        Approval::Granted
                    },
                )]),
                consents: BTreeMap::from([(cap, Approval::Granted)]),
                ..Default::default()
            },
            operating_mode: OperatingMode::Development,
            execution_profile: ExecutionProfile::FullPath,
            process: ProcessReadiness::Eligible,
            consent_required: false,
            administrative_capabilities: BTreeSet::new(),
            policy: PolicyId::new("policy").unwrap(),
            policy_revision: ContentDigest::new("b".repeat(64)).unwrap(),
            consent_receipt: if class == CapabilityClass::Mutate {
                Some(text("fixture-consent"))
            } else {
                None
            },
            budget_reservation: text(if self.wrong_reservation.load(Ordering::SeqCst) {
                "foreign-reservation"
            } else {
                &self.reservation
            }),
            deadline: self.deadline,
        }
    }
}
impl ConnectorPolicySource for Policy {
    fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        Ok(())
    }
    fn authorize(
        &self,
        r: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        Ok(self.snapshot(r))
    }
    fn revalidate(
        &self,
        d: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        Ok(self.snapshot(&d.request))
    }
    fn record_decision(&self, _: &ConnectorPolicyEvaluation<'_>) -> Result<(), ConnectorFailure> {
        self.audits.fetch_add(1, Ordering::SeqCst);
        let hook = self.audit_hook.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        Ok(())
    }
}
struct PolicyRef(Arc<Policy>);
impl ConnectorPolicySource for PolicyRef {
    fn admit_target(&self, t: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        self.0.admit_target(t)
    }
    fn authorize(
        &self,
        r: &ConnectorRequest,
        s: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.0.authorize(r, s)
    }
    fn revalidate(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.0.revalidate(d, s)
    }
    fn record_decision(
        &self,
        evaluation: &ConnectorPolicyEvaluation<'_>,
    ) -> Result<(), ConnectorFailure> {
        self.0.record_decision(evaluation)
    }
}
#[derive(Default)]
struct Quotes {
    mode: AtomicUsize,
    quote_hook: Mutex<Option<Hook>>,
    result_hook: Mutex<Option<Hook>>,
    quote_count: AtomicUsize,
    result_deny: AtomicBool,
}
impl Quotes {
    fn quote(&self) -> Result<SessionConnectorQuote, SessionError> {
        self.quote_count.fetch_add(1, Ordering::SeqCst);
        let hook = self.quote_hook.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        let mut quote = SessionConnectorQuote {
            amounts: slice(1),
            cost_unit: ReferenceId::new("test-credit").unwrap(),
        };
        match self.mode.load(Ordering::SeqCst) {
            1 => quote.cost_unit = ReferenceId::new("foreign-credit").unwrap(),
            2 => quote.amounts.connector_calls = 0,
            3 => quote.amounts.model_attempts = 1,
            4 => quote.amounts.retrieval_rounds = 0,
            5 => quote.amounts.cost_microunits = 1000,
            6 => return Err(SessionError::UnsupportedCapability),
            7 => quote.amounts.cost_microunits = 0,
            8 => return Err(SessionError::OutcomeUnknown),
            _ => {}
        }
        Ok(quote)
    }
}
impl SessionConnectorQuotePort for Quotes {
    fn quote_discovery(
        &self,
        _: &SessionResourceAllocation,
        _: &ConnectorTarget,
    ) -> Result<SessionConnectorQuote, SessionError> {
        self.quote()
    }
    fn quote_request(
        &self,
        _: &SessionResourceAllocation,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<SessionConnectorQuote, SessionError> {
        self.quote()
    }
    fn validate_inventory(
        &self,
        _: &SessionResourceAllocation,
        q: &SessionConnectorQuote,
        i: &ConnectorInventory,
    ) -> Result<(), SessionError> {
        let hook = self.result_hook.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        if self.result_deny.load(Ordering::SeqCst)
            || (i.capabilities.len() + i.resources.len()) as u64 > q.amounts.retrieval_results
        {
            Err(SessionError::LimitExceeded)
        } else {
            Ok(())
        }
    }
    fn validate_observation(
        &self,
        _: &SessionResourceAllocation,
        q: &SessionConnectorQuote,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
        o: &ConnectorObservation,
    ) -> Result<(), SessionError> {
        let hook = self.result_hook.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        if self.result_deny.load(Ordering::SeqCst)
            || o.records.facts().len() as u64 > q.amounts.retrieval_results
        {
            Err(SessionError::LimitExceeded)
        } else {
            Ok(())
        }
    }
}
struct HookMapping {
    recovery: Mutex<gateway_application::connector_resilience::RecoverySupport>,
    operation: Mutex<Option<Hook>>,
    normalize: Mutex<Option<Hook>>,
}
impl ConnectorMapping for HookMapping {
    fn recovery_support(
        &self,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
        _: &ProtocolInventory,
    ) -> Result<gateway_application::connector_resilience::RecoverySupport, Error> {
        Ok(self.recovery.lock().unwrap().clone())
    }
    fn inventory(
        &self,
        t: &ConnectorTarget,
        m: &ConnectorManifest,
        w: &ProtocolInventory,
    ) -> Result<ConnectorInventory, Error> {
        Mapping { mode: "normal" }.inventory(t, m, w)
    }
    fn operation(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
        w: &ProtocolInventory,
    ) -> Result<WireOperation, Error> {
        let hook = self.operation.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        Mapping { mode: "normal" }.operation(d, s, w)
    }
    fn normalize(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
        r: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        let result = Mapping { mode: "normal" }.normalize(d, s, r)?;
        let hook = self.normalize.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        Ok(result)
    }
}
struct Factory {
    inner: StdioConfig,
    permits: Mutex<Option<Hook>>,
}
impl TransportFactory for Factory {
    fn open(&self, c: &CallContext, l: &Limits) -> Result<Box<dyn Transport>, Error> {
        self.inner.open(c, l)
    }
    fn permits_tool(&self, _: &str, _: &str, _: &str) -> Result<(), Error> {
        let hook = self.permits.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        Ok(())
    }
}
struct Setup {
    host: Arc<McpConnectorHost>,
    policy: Arc<Policy>,
    quotes: Arc<Quotes>,
    mapping: Arc<HookMapping>,
    factory: Arc<Factory>,
    log: PathBuf,
}
struct MappingRef(Arc<HookMapping>);
impl ConnectorMapping for MappingRef {
    fn recovery_support(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
        wire: &ProtocolInventory,
    ) -> Result<gateway_application::connector_resilience::RecoverySupport, Error> {
        self.0.recovery_support(request, source, wire)
    }
    fn inventory(
        &self,
        t: &ConnectorTarget,
        m: &ConnectorManifest,
        w: &ProtocolInventory,
    ) -> Result<ConnectorInventory, Error> {
        self.0.inventory(t, m, w)
    }
    fn operation(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
        w: &ProtocolInventory,
    ) -> Result<WireOperation, Error> {
        self.0.operation(d, s, w)
    }
    fn normalize(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
        r: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        self.0.normalize(d, s, r)
    }
}
struct FactoryRef(Arc<Factory>);
impl TransportFactory for FactoryRef {
    fn open(&self, c: &CallContext, l: &Limits) -> Result<Box<dyn Transport>, Error> {
        self.0.open(c, l)
    }
    fn permits_tool(&self, c: &str, s: &str, t: &str) -> Result<(), Error> {
        self.0.permits_tool(c, s, t)
    }
}
fn setup(resources: &SessionPhaseResources, mode: &str) -> Setup {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let log = std::env::temp_dir().join(format!(
        "cg-reserved-connector-{}-{}.jsonl",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let mut transport = config(mode);
    transport.arguments.push(log.display().to_string());
    let factory = Arc::new(Factory {
        inner: transport,
        permits: Mutex::new(None),
    });
    let mapping = Arc::new(HookMapping {
        recovery: Mutex::new(Default::default()),
        operation: Mutex::new(None),
        normalize: Mutex::new(None),
    });
    let policy = Arc::new(Policy {
        reservation: resources.reservation_id().as_str().into(),
        deadline: UnixTimestamp::new(
            i64::try_from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + if mode == "policy-expired" {
                        0
                    } else if mode == "rate-policy-short" {
                        3
                    } else {
                        60
                    },
            )
            .unwrap(),
        ),
        deny: AtomicBool::new(false),
        wrong_reservation: AtomicBool::new(false),
        audits: AtomicUsize::new(0),
        audit_hook: Mutex::new(None),
    });
    let host = McpConnectorHost::new(
        HostConfig {
            target: target(resources),
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
            limits: Limits {
                frame_bytes: 8192,
                inventory_bytes: 8192,
                items: 8,
                pages: 4,
                ..Limits::default()
            },
        },
        Box::new(FactoryRef(factory.clone())),
        Arc::new(Cg09ConnectorAdmission::new(PolicyRef(policy.clone()))),
        Box::new(MappingRef(mapping.clone())),
        Arc::new(()),
    )
    .unwrap();
    Setup {
        host: Arc::new(host),
        policy,
        quotes: Arc::new(Quotes::default()),
        mapping,
        factory,
        log,
    }
}
fn run(
    resources: &mut SessionPhaseResources,
    authority: &allocation_fixture::PreparationAuthority,
    s: &Setup,
    calls: u64,
) -> SessionConnectorRun {
    let target = target(resources);
    authority.allow_connector_target(&target);
    SessionConnectorRun::new(
        s.host.clone(),
        resources.allocate(slice(calls)).unwrap(),
        Arc::new(authority.clone()),
        s.quotes.clone(),
        CallContext::new(
            text("reserved-discovery"),
            Instant::now() + Duration::from_secs(5),
        ),
    )
    .unwrap()
}
fn wire(log: &PathBuf, method: &str) -> usize {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|v| v["method"] == method)
        .count()
}
fn cleanup(run: &SessionConnectorRun, target: &ConnectorTarget, s: &Setup) {
    let _ = run.lifecycle(target, ConnectorLifecycleCommand::Shutdown);
    let _ = std::fs::remove_file(&s.log);
}

#[test]
fn reserved_connector_real_coordinator_cg09_discovery_tool_resource_and_exhaustion() {
    in_phase(
        SessionResourcePhase::Execution,
        3,
        |resources, cp, authority| {
            let s = setup(resources, "normal");
            let target = target(resources);
            let mut req = request(resources, cp, ConnectorOperationClass::Read);
            authority.allow_connector_request(&req, None);
            let run = run(resources, authority, &s, 3);
            let inventory = run.discover(&target).unwrap();
            assert_eq!(inventory.target, target);
            let observation = run.invoke(&req).unwrap();
            assert_eq!(observation.correlation, req.correlation);
            req.correlation = text("reserved-resource");
            let source = SourceId::new("one").unwrap();
            authority.allow_connector_request(&req, Some(&source));
            run.read_resource(&req, &source).unwrap();
            assert_eq!(run.usage().unwrap().connector_calls, 3);
            assert_eq!(run.usage().unwrap().cost_microunits, 3);
            let count = wire(&s.log, "resources/read");
            assert_eq!(
                run.read_resource(&req, &source).unwrap_err().code,
                ConnectorFailureCode::BudgetExhausted
            );
            assert_eq!(wire(&s.log, "resources/read"), count);
            assert_eq!(wire(&s.log, "tools/call"), 1);
            // Discovery has no dispatch audit; each tool/resource operation
            // audits authorize + explicit pre-effect revalidation exactly once.
            assert_eq!(s.policy.audits.load(Ordering::SeqCst), 4);
            cleanup(&run, &target, &s);
        },
    );
}

#[test]
fn reserved_connector_denials_and_quote_errors_have_zero_transport() {
    for mode in 1..=6 {
        in_phase(
            SessionResourcePhase::Preparation,
            1,
            |resources, cp, authority| {
                let s = setup(resources, "normal");
                let target = target(resources);
                let req = request(resources, cp, ConnectorOperationClass::Read);
                authority.allow_connector_request(&req, None);
                let run = run(resources, authority, &s, 1);
                s.quotes.mode.store(mode, Ordering::SeqCst);
                let failure = run.discover(&target).unwrap_err();
                assert_eq!(failure.effect, ConnectorEffect::NotDispatched);
                assert_eq!(wire(&s.log, "initialize"), 0);
                assert_eq!(run.usage().unwrap().connector_calls, 0);
                cleanup(&run, &target, &s);
            },
        );
    }
    in_phase(
        SessionResourcePhase::Preparation,
        1,
        |resources, cp, authority| {
            let s = setup(resources, "normal");
            let target = target(resources);
            let req = request(resources, cp, ConnectorOperationClass::Mutate);
            authority.allow_connector_request(&req, None);
            let run = run(resources, authority, &s, 1);
            assert_eq!(
                run.invoke(&req).unwrap_err().effect,
                ConnectorEffect::NotDispatched
            );
            assert_eq!(wire(&s.log, "initialize"), 0);
            cleanup(&run, &target, &s);
        },
    );
}

#[test]
fn reserved_connector_constructor_refuses_non_connector_slice_and_cancelled_context() {
    for cancelled in [false, true] {
        in_phase(
            SessionResourcePhase::Preparation,
            1,
            |resources, _, authority| {
                let s = setup(resources, "normal");
                let context =
                    CallContext::new(text("constructor"), Instant::now() + Duration::from_secs(5));
                if cancelled {
                    context.cancel();
                }
                let amounts = if cancelled {
                    slice(1)
                } else {
                    SessionResourceAmounts {
                        retrieval_rounds: 1,
                        ..Default::default()
                    }
                };
                let error = SessionConnectorRun::new(
                    s.host.clone(),
                    resources.allocate(amounts).unwrap(),
                    Arc::new(authority.clone()),
                    s.quotes.clone(),
                    context,
                )
                .err()
                .unwrap();
                assert_eq!(
                    error,
                    if cancelled {
                        SessionError::AuthorityDenied
                    } else {
                        SessionError::LimitExceeded
                    }
                );
                assert_eq!(wire(&s.log, "initialize"), 0);
            },
        );
    }
}

#[test]
fn reserved_connector_discovery_revalidates_exact_target_and_preserves_spend() {
    for case in ["quote-target", "inventory-revoked", "inventory-metrics"] {
        in_phase(
            SessionResourcePhase::Preparation,
            1,
            |resources, cp, authority| {
                let s = setup(resources, "normal");
                let target = target(resources);
                let run = run(resources, authority, &s, 1);
                let a = authority.clone();
                match case {
                    "quote-target" => {
                        *s.quotes.quote_hook.lock().unwrap() =
                            Some(Arc::new(move || a.revoke_connector_targets()))
                    }
                    "inventory-revoked" => {
                        *s.quotes.result_hook.lock().unwrap() =
                            Some(Arc::new(move || a.revoke_connector_targets()))
                    }
                    _ => s.quotes.result_deny.store(true, Ordering::SeqCst),
                }
                let failure = run.discover(&target).unwrap_err();
                assert_eq!(
                    failure.effect,
                    if case == "quote-target" {
                        ConnectorEffect::NotDispatched
                    } else {
                        ConnectorEffect::NoMutation
                    }
                );
                assert_eq!(run.usage().unwrap().connector_calls, 1);
                assert_eq!(
                    wire(&s.log, "initialize"),
                    usize::from(case != "quote-target")
                );
                // A revoked grant and exhausted allowance cannot prevent cleanup.
                authority.revoke();
                if case == "inventory-metrics" {
                    let frames = std::fs::read(&s.log).unwrap();
                    s.quotes.result_deny.store(false, Ordering::SeqCst);
                    assert_eq!(run.discover(&target).unwrap_err(), failure);
                    let req = request(resources, cp, ConnectorOperationClass::Read);
                    assert_eq!(run.invoke(&req).unwrap_err(), failure);
                    assert_eq!(std::fs::read(&s.log).unwrap(), frames);
                }
                run.lifecycle(&target, ConnectorLifecycleCommand::Disable)
                    .unwrap();
                cleanup(&run, &target, &s);
            },
        );
    }
}

#[test]
fn reserved_connector_wrong_cg09_reservation_policy_and_late_mapping_denials_send_no_tool() {
    for case in [
        "reservation",
        "policy",
        "mapping",
        "factory",
        "audit",
        "pre-effect-audit",
        "quote-policy",
    ] {
        in_phase(
            SessionResourcePhase::Execution,
            2,
            |resources, cp, authority| {
                let s = setup(resources, "normal");
                let target = target(resources);
                let req = request(resources, cp, ConnectorOperationClass::Mutate);
                authority.allow_connector_request(&req, None);
                let run = run(resources, authority, &s, 2);
                run.discover(&target).unwrap();
                match case {
                    "reservation" => s.policy.wrong_reservation.store(true, Ordering::SeqCst),
                    "policy" => s.policy.deny.store(true, Ordering::SeqCst),
                    "mapping" => {
                        let a = authority.clone();
                        *s.mapping.operation.lock().unwrap() =
                            Some(Arc::new(move || a.revoke_connector_requests()));
                    }
                    "factory" => {
                        let a = authority.clone();
                        *s.factory.permits.lock().unwrap() =
                            Some(Arc::new(move || a.revoke_connector_requests()));
                    }
                    "pre-effect-audit" => {
                        let policy = s.policy.clone();
                        let a = authority.clone();
                        *s.factory.permits.lock().unwrap() = Some(Arc::new(move || {
                            let a = a.clone();
                            *policy.audit_hook.lock().unwrap() = Some(Arc::new(move || a.revoke()));
                        }));
                    }
                    "quote-policy" => {
                        let policy = s.policy.clone();
                        let quotes = s.quotes.clone();
                        *s.factory.permits.lock().unwrap() = Some(Arc::new(move || {
                            let policy = policy.clone();
                            *quotes.quote_hook.lock().unwrap() = Some(Arc::new(move || {
                                policy.deny.store(true, Ordering::SeqCst);
                            }));
                        }));
                    }
                    _ => {
                        let a = authority.clone();
                        *s.policy.audit_hook.lock().unwrap() = Some(Arc::new(move || a.revoke()));
                    }
                }
                let f = run.invoke(&req).unwrap_err();
                assert_eq!(f.effect, ConnectorEffect::NotDispatched, "{case}");
                assert_eq!(wire(&s.log, "tools/call"), 0, "{case}");
                assert_eq!(run.usage().unwrap().connector_calls, 2);
                cleanup(&run, &target, &s);
            },
        );
    }
}

#[test]
fn reserved_connector_post_effect_revocation_retains_unknown_and_forbids_direct_retry() {
    for class in [
        ConnectorOperationClass::Read,
        ConnectorOperationClass::Mutate,
    ] {
        in_phase(
            SessionResourcePhase::Execution,
            3,
            |resources, cp, authority| {
                let s = setup(resources, "normal");
                let target = target(resources);
                let req = request(resources, cp, class);
                authority.allow_connector_request(&req, None);
                let run = run(resources, authority, &s, 3);
                run.discover(&target).unwrap();
                let a = authority.clone();
                *s.mapping.normalize.lock().unwrap() =
                    Some(Arc::new(move || a.revoke_connector_requests()));
                let first = run.invoke(&req).unwrap_err();
                assert_eq!(
                    first.effect,
                    if class == ConnectorOperationClass::Mutate {
                        ConnectorEffect::Unknown
                    } else {
                        ConnectorEffect::NoMutation
                    }
                );
                assert_eq!(wire(&s.log, "tools/call"), 1);
                assert_eq!(run.usage().unwrap().connector_calls, 2);
                let next = run.invoke(&req).unwrap_err();
                if class == ConnectorOperationClass::Mutate {
                    assert_eq!(next, first);
                }
                assert_eq!(wire(&s.log, "tools/call"), 1);
                cleanup(&run, &target, &s);
            },
        );
    }
}

#[test]
fn standalone_host_rechecks_policy_revoked_by_pre_effect_audit_before_sending() {
    in_phase(SessionResourcePhase::Execution, 1, |resources, cp, _| {
        let s = setup(resources, "normal");
        let target = target(resources);
        let req = request(resources, cp, ConnectorOperationClass::Mutate);
        s.host
            .lifecycle(&target, ConnectorLifecycleCommand::Connect)
            .unwrap();
        s.host.discover(&target).unwrap();
        // Arm revocation after initial authorization. This exercises the
        // standalone API, which has no session or typed-preflight guards.
        let policy = s.policy.clone();
        *s.factory.permits.lock().unwrap() = Some(Arc::new(move || {
            let policy_for_audit = policy.clone();
            *policy.audit_hook.lock().unwrap() = Some(Arc::new(move || {
                policy_for_audit.deny.store(true, Ordering::SeqCst);
            }));
        }));
        let context = CallContext::new(
            req.correlation.clone(),
            Instant::now() + Duration::from_secs(5),
        );
        let failure = s
            .host
            .invoke_with_context(&req, None, &context)
            .unwrap_err();
        assert_eq!(failure.code, ConnectorFailureCode::PermissionDenied);
        assert_eq!(failure.effect, ConnectorEffect::NotDispatched);
        assert_eq!(wire(&s.log, "tools/call"), 0);
        assert_eq!(s.policy.audits.load(Ordering::SeqCst), 2);
        s.host
            .lifecycle(&target, ConnectorLifecycleCommand::Shutdown)
            .unwrap();
        let _ = std::fs::remove_file(&s.log);
    });
}

#[test]
fn reserved_connector_late_quote_changes_cannot_reprice_an_admitted_operation() {
    for mode in [7, 1, 8] {
        in_phase(
            SessionResourcePhase::Execution,
            2,
            |resources, cp, authority| {
                let s = setup(resources, "normal");
                let target = target(resources);
                let req = request(resources, cp, ConnectorOperationClass::Mutate);
                authority.allow_connector_request(&req, None);
                let run = run(resources, authority, &s, 2);
                run.discover(&target).unwrap();
                let quotes = s.quotes.clone();
                *s.factory.permits.lock().unwrap() = Some(Arc::new(move || {
                    quotes.mode.store(mode, Ordering::SeqCst);
                }));
                let failure = run.invoke(&req).unwrap_err();
                assert_eq!(
                    failure.code,
                    if mode == 8 {
                        ConnectorFailureCode::MutationUncertain
                    } else {
                        ConnectorFailureCode::BudgetExhausted
                    }
                );
                assert_eq!(failure.effect, ConnectorEffect::NotDispatched);
                assert_eq!(wire(&s.log, "tools/call"), 0);
                assert_eq!(run.usage().unwrap().connector_calls, 2);
                assert_eq!(run.usage().unwrap().cost_microunits, 2);
                cleanup(&run, &target, &s);
            },
        );
    }
}

#[test]
fn reserved_connector_quote_callback_cannot_keep_exact_permission_after_factory_guard() {
    in_phase(
        SessionResourcePhase::Execution,
        2,
        |resources, cp, authority| {
            let s = setup(resources, "normal");
            let target = target(resources);
            let req = request(resources, cp, ConnectorOperationClass::Mutate);
            authority.allow_connector_request(&req, None);
            let run = run(resources, authority, &s, 2);
            run.discover(&target).unwrap();
            let q = s.quotes.clone();
            let a = authority.clone();
            *s.factory.permits.lock().unwrap() = Some(Arc::new(move || {
                let a = a.clone();
                *q.quote_hook.lock().unwrap() =
                    Some(Arc::new(move || a.revoke_connector_requests()));
            }));
            let f = run.invoke(&req).unwrap_err();
            assert_eq!(f.effect, ConnectorEffect::NotDispatched);
            assert_eq!(wire(&s.log, "tools/call"), 0);
            cleanup(&run, &target, &s);
        },
    );
}

#[test]
fn reserved_connector_result_metric_failure_and_cancellation_keep_spent_budget() {
    for mutation in [false, true] {
        in_phase(
            SessionResourcePhase::Execution,
            3,
            |resources, cp, authority| {
                let s = setup(resources, "normal");
                let target = target(resources);
                let req = request(
                    resources,
                    cp,
                    if mutation {
                        ConnectorOperationClass::Mutate
                    } else {
                        ConnectorOperationClass::Read
                    },
                );
                authority.allow_connector_request(&req, None);
                let run = run(resources, authority, &s, 3);
                run.discover(&target).unwrap();
                s.quotes.result_deny.store(true, Ordering::SeqCst);
                let f = run.invoke(&req).unwrap_err();
                assert_eq!(
                    f.effect,
                    if mutation {
                        ConnectorEffect::Unknown
                    } else {
                        ConnectorEffect::NoMutation
                    }
                );
                assert_eq!(run.usage().unwrap().connector_calls, 2);
                let frames = std::fs::read(&s.log).unwrap();
                s.quotes.result_deny.store(false, Ordering::SeqCst);
                assert_eq!(run.invoke(&req).unwrap_err(), f);
                authority.revoke();
                assert_eq!(run.invoke(&req).unwrap_err(), f);
                assert_eq!(run.discover(&target).unwrap_err(), f);
                assert_eq!(std::fs::read(&s.log).unwrap(), frames);
                run.cancel();
                assert_eq!(run.invoke(&req).unwrap_err(), f);
                assert_eq!(wire(&s.log, "tools/call"), 1);
                cleanup(&run, &target, &s);
            },
        );
    }
}

#[test]
fn reserved_connector_reentrant_calls_refuse_and_invalid_cleanup_does_not_cancel_valid_run() {
    in_phase(
        SessionResourcePhase::Execution,
        2,
        |resources, cp, authority| {
            let s = setup(resources, "normal");
            let target = target(resources);
            let req = request(resources, cp, ConnectorOperationClass::Read);
            authority.allow_connector_request(&req, None);
            let run = Arc::new(run(resources, authority, &s, 2));
            let mut wrong = target.clone();
            wrong.connector = text("foreign");
            assert_eq!(
                run.lifecycle(&wrong, ConnectorLifecycleCommand::Disable)
                    .unwrap_err()
                    .code,
                ConnectorFailureCode::ScopeMismatch
            );
            assert_eq!(
                run.lifecycle(&target, ConnectorLifecycleCommand::Health)
                    .unwrap_err()
                    .effect,
                ConnectorEffect::NotDispatched
            );
            run.discover(&target).unwrap();
            let observed = Arc::new(AtomicBool::new(false));
            let seen = observed.clone();
            let weak = Arc::downgrade(&run);
            let nested = req.clone();
            *s.quotes.quote_hook.lock().unwrap() = Some(Arc::new(move || {
                let run = weak.upgrade().unwrap();
                assert_eq!(
                    run.invoke(&nested).unwrap_err().code,
                    ConnectorFailureCode::Backpressure
                );
                assert_eq!(run.usage(), Err(SessionError::Unavailable));
                seen.store(true, Ordering::SeqCst);
            }));
            run.invoke(&req).unwrap();
            assert!(observed.load(Ordering::SeqCst));
            assert_eq!(wire(&s.log, "tools/call"), 1);
            cleanup(&run, &target, &s);
        },
    );
}

#[test]
fn reserved_connector_readonly_preparation_and_uncertain_recovery_have_distinct_phase_allowances() {
    for phase in [
        SessionResourcePhase::Preparation,
        SessionResourcePhase::AfterSnapshotRecovery,
    ] {
        in_phase(phase, 2, |resources, cp, authority| {
            let s = setup(resources, "normal");
            let target = target(resources);
            let read = request(resources, cp, ConnectorOperationClass::Read);
            authority.allow_connector_request(&read, None);
            let mut mutation = read.clone();
            mutation.class = ConnectorOperationClass::Mutate;
            mutation.capability = CapabilityId::new("records.z-mutate").unwrap();
            authority.allow_connector_request(&mutation, None);
            let run = run(resources, authority, &s, 2);
            assert_eq!(
                run.invoke(&mutation).unwrap_err().effect,
                ConnectorEffect::NotDispatched
            );
            run.discover(&target).unwrap();
            run.invoke(&read).unwrap();
            assert_eq!(wire(&s.log, "tools/call"), 1);
            assert_eq!(run.usage().unwrap().connector_calls, 2);
            cleanup(&run, &target, &s);
        });
    }
}

#[test]
fn connector_retries_are_bounded_and_spend_the_original_aggregate_slice() {
    use gateway_application::connector_resilience::*;
    for (mode, calls, expected_wires, expected_usage) in [
        ("rate-once", 3, 2, 3),
        ("rate-limit", 10, 3, 4),
        ("rate-limit", 2, 1, 2),
    ] {
        in_phase(
            SessionResourcePhase::Execution,
            calls,
            |resources, cp, authority| {
                let s = setup(resources, mode);
                s.mapping.recovery.lock().unwrap().retry = RetrySafety::ReadOnly;
                let target = target(resources);
                let req = request(resources, cp, ConnectorOperationClass::Read);
                authority.allow_connector_request(&req, None);
                let run = run(resources, authority, &s, calls);
                run.discover(&target).unwrap();
                let result = run.invoke(&req);
                if mode == "rate-once" {
                    assert!(result.is_ok(), "{result:?}");
                } else {
                    assert_eq!(
                        result.unwrap_err().code,
                        if calls == 2 {
                            ConnectorFailureCode::BudgetExhausted
                        } else {
                            ConnectorFailureCode::RateLimited
                        }
                    );
                }
                assert_eq!(wire(&s.log, "tools/call"), expected_wires);
                assert_eq!(run.usage().unwrap().connector_calls, expected_usage);
                assert_eq!(run.usage().unwrap().cost_microunits, expected_usage);
                assert!(run.retry_attempts().unwrap() > 0);
                let retained = run.usage().unwrap();
                run.cancel();
                assert_eq!(
                    run.invoke(&req).unwrap_err().code,
                    ConnectorFailureCode::Cancelled
                );
                assert_eq!(run.usage().unwrap(), retained);
                cleanup(&run, &target, &s);
            },
        );
    }
}
#[test]
fn uncertain_mutation_blocks_retries_even_with_provider_idempotency() {
    use gateway_application::connector_resilience::*;
    in_phase(
        SessionResourcePhase::Execution,
        5,
        |resources, cp, authority| {
            let s = setup(resources, "rate-once");
            *s.mapping.recovery.lock().unwrap() = RecoverySupport {
                retry: RetrySafety::Idempotent,
                idempotency: ProviderIdempotency::Idempotent,
                read_back: ReadBackSupport::Unsupported,
            };
            let target = target(resources);
            let req = request(resources, cp, ConnectorOperationClass::Mutate);
            authority.allow_connector_request(&req, None);
            let run = run(resources, authority, &s, 5);
            run.discover(&target).unwrap();
            let first = run.invoke(&req).unwrap_err();
            assert_eq!(first.code, ConnectorFailureCode::RateLimited);
            assert_eq!(first.effect, ConnectorEffect::Unknown);
            assert_eq!(run.invoke(&req).unwrap_err(), first);
            assert_eq!(wire(&s.log, "tools/call"), 1);
            assert_eq!(run.usage().unwrap().connector_calls, 2);
            assert_eq!(run.retry_attempts().unwrap(), 0);
            cleanup(&run, &target, &s);
        },
    );
}
#[test]
fn cancellation_interrupts_backoff_without_spending_another_call() {
    use gateway_application::connector_resilience::*;
    in_phase(
        SessionResourcePhase::Execution,
        5,
        |resources, cp, authority| {
            let s = setup(resources, "rate-long");
            s.mapping.recovery.lock().unwrap().retry = RetrySafety::ReadOnly;
            let target = target(resources);
            let req = request(resources, cp, ConnectorOperationClass::Read);
            authority.allow_connector_request(&req, None);
            let run = Arc::new(run(resources, authority, &s, 5));
            run.discover(&target).unwrap();
            let worker = {
                let run = run.clone();
                let req = req.clone();
                std::thread::spawn(move || run.invoke(&req))
            };
            let until = Instant::now() + Duration::from_secs(3);
            while s.host.health_snapshot().retry_after.is_zero() {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(2));
            }
            run.cancel();
            assert_eq!(
                worker.join().unwrap().unwrap_err().code,
                ConnectorFailureCode::Cancelled
            );
            assert_eq!(run.usage().unwrap().connector_calls, 2);
            assert_eq!(wire(&s.log, "tools/call"), 1);
            cleanup(&run, &target, &s);
        },
    );
}

#[test]
fn recovery_port_exposes_exact_provider_support_without_granting_mutation_replay() {
    use gateway_application::connector_resilience::*;
    in_phase(
        SessionResourcePhase::Execution,
        2,
        |resources, cp, authority| {
            let s = setup(resources, "normal");
            let target = target(resources);
            let req = request(resources, cp, ConnectorOperationClass::Read);
            let mutate = request(resources, cp, ConnectorOperationClass::Mutate);
            authority.allow_connector_request(&req, None);
            let run = run(resources, authority, &s, 2);
            assert_eq!(
                s.host.recovery_support(&req, None).unwrap_err().code,
                ConnectorFailureCode::InventoryChanged
            );
            run.discover(&target).unwrap();
            let port: &dyn ConnectorRecoveryPort = s.host.as_ref();
            assert_eq!(
                port.recovery_support(&req, None).unwrap(),
                RecoverySupport::default()
            );
            let mut stale = req.clone();
            stale.inventory_revision = ContentDigest::new("b".repeat(64)).unwrap();
            assert_eq!(
                port.recovery_support(&stale, None).unwrap_err().code,
                ConnectorFailureCode::InventoryChanged
            );
            let mut wrong = req.clone();
            wrong.target.scope = ContextScopeId::new("wrong").unwrap();
            assert!(port.recovery_support(&wrong, None).is_err());
            assert!(
                port.recovery_support(&req, Some(&SourceId::new("missing").unwrap()))
                    .is_err()
            );
            s.mapping.recovery.lock().unwrap().retry = RetrySafety::ReadOnly;
            assert_eq!(
                port.recovery_support(&mutate, None).unwrap_err().code,
                ConnectorFailureCode::SchemaMismatch
            );
            *s.mapping.recovery.lock().unwrap() = RecoverySupport {
                retry: RetrySafety::Idempotent,
                idempotency: ProviderIdempotency::Keyed {
                    argument: text("missing"),
                },
                read_back: ReadBackSupport::Unsupported,
            };
            assert_eq!(
                port.recovery_support(&req, None).unwrap_err().code,
                ConnectorFailureCode::SchemaMismatch
            );
            s.mapping.recovery.lock().unwrap().idempotency = ProviderIdempotency::Keyed {
                argument: text("query"),
            };
            assert!(port.recovery_support(&req, None).is_ok());
            for capability in ["missing", "records.z-mutate"] {
                s.mapping.recovery.lock().unwrap().read_back =
                    ReadBackSupport::Capability(CapabilityId::new(capability).unwrap());
                assert_eq!(
                    port.recovery_support(&req, None).unwrap_err().code,
                    ConnectorFailureCode::SchemaMismatch
                );
            }
            s.mapping.recovery.lock().unwrap().read_back =
                ReadBackSupport::Capability(CapabilityId::new("records.read").unwrap());
            assert!(matches!(
                port.recovery_support(&req, None).unwrap().read_back,
                ReadBackSupport::Capability(_)
            ));
            assert_eq!(wire(&s.log, "tools/call"), 0);
            assert_eq!(run.usage().unwrap().connector_calls, 1);
            cleanup(&run, &target, &s);
        },
    );
}

#[test]
fn authority_revocation_during_backoff_keeps_original_spend_and_sends_no_retry() {
    use gateway_application::connector_resilience::*;
    in_phase(
        SessionResourcePhase::Execution,
        5,
        |resources, cp, authority| {
            let s = setup(resources, "rate-long");
            s.mapping.recovery.lock().unwrap().retry = RetrySafety::ReadOnly;
            let target = target(resources);
            let req = request(resources, cp, ConnectorOperationClass::Read);
            authority.allow_connector_request(&req, None);
            let run = Arc::new(run(resources, authority, &s, 5));
            run.discover(&target).unwrap();
            let worker = {
                let run = run.clone();
                let req = req.clone();
                std::thread::spawn(move || run.invoke(&req))
            };
            let until = Instant::now() + Duration::from_secs(3);
            while s.host.health_snapshot().retry_after.is_zero() {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(2));
            }
            authority.revoke_connector_requests();
            assert_eq!(
                worker.join().unwrap().unwrap_err().code,
                ConnectorFailureCode::PermissionDenied
            );
            assert_eq!(run.usage().unwrap().connector_calls, 2);
            assert_eq!(wire(&s.log, "tools/call"), 1);
            cleanup(&run, &target, &s);
        },
    );
}

#[test]
fn retry_reconnect_and_rediscovery_each_consume_the_same_remaining_budget() {
    use gateway_application::connector_resilience::*;
    in_phase(
        SessionResourcePhase::Execution,
        6,
        |resources, cp, authority| {
            let s = setup(resources, "eof");
            s.mapping.recovery.lock().unwrap().retry = RetrySafety::ReadOnly;
            let target = target(resources);
            let req = request(resources, cp, ConnectorOperationClass::Read);
            authority.allow_connector_request(&req, None);
            let run = run(resources, authority, &s, 6);
            run.discover(&target).unwrap();
            let error = run.invoke(&req).unwrap_err();
            assert_eq!(error.code, ConnectorFailureCode::Unavailable);
            assert_eq!(error.effect, ConnectorEffect::NoMutation);
            assert_eq!(run.usage().unwrap().connector_calls, 6);
            assert_eq!(run.usage().unwrap().cost_microunits, 6);
            assert_eq!(run.retry_attempts().unwrap(), 2);
            assert_eq!(wire(&s.log, "tools/call"), 3);
            assert_eq!(wire(&s.log, "initialize"), 3);
            assert_eq!(wire(&s.log, "tools/list"), 3);
            assert_eq!(
                run.invoke(&req).unwrap_err().code,
                ConnectorFailureCode::BudgetExhausted
            );
            assert_eq!(wire(&s.log, "tools/call"), 3);
            cleanup(&run, &target, &s);
        },
    );
}

#[test]
fn retry_sequence_retains_the_narrower_original_cg09_deadline() {
    use gateway_application::connector_resilience::*;
    in_phase(
        SessionResourcePhase::Execution,
        5,
        |resources, cp, authority| {
            let s = setup(resources, "rate-policy-short");
            s.mapping.recovery.lock().unwrap().retry = RetrySafety::ReadOnly;
            let target = target(resources);
            let req = request(resources, cp, ConnectorOperationClass::Read);
            authority.allow_connector_request(&req, None);
            let run = run(resources, authority, &s, 5);
            run.discover(&target).unwrap();
            let started = Instant::now();
            let error = run.invoke(&req).unwrap_err();
            assert!(matches!(
                error.code,
                ConnectorFailureCode::RateLimited | ConnectorFailureCode::Timeout
            ));
            assert!(started.elapsed() < Duration::from_secs(1));
            assert_eq!(run.retry_attempts().unwrap(), 0);
            assert_eq!(
                wire(&s.log, "tools/call"),
                if error.code == ConnectorFailureCode::RateLimited {
                    1
                } else {
                    0
                }
            );
            assert_eq!(run.usage().unwrap().connector_calls, 2);
            cleanup(&run, &target, &s);
        },
    );
}

#[test]
fn expired_original_dispatch_deadline_stops_without_retry_or_wire_send() {
    use gateway_application::connector_resilience::*;
    in_phase(
        SessionResourcePhase::Execution,
        4,
        |resources, cp, authority| {
            let s = setup(resources, "policy-expired");
            s.mapping.recovery.lock().unwrap().retry = RetrySafety::ReadOnly;
            let target = target(resources);
            let req = request(resources, cp, ConnectorOperationClass::Read);
            authority.allow_connector_request(&req, None);
            let run = run(resources, authority, &s, 4);
            run.discover(&target).unwrap();
            let error = run.invoke(&req).unwrap_err();
            assert_eq!(
                (error.code, error.effect),
                (
                    ConnectorFailureCode::Timeout,
                    ConnectorEffect::NotDispatched
                )
            );
            assert_eq!(run.usage().unwrap().connector_calls, 2);
            assert_eq!(run.retry_attempts().unwrap(), 0);
            assert_eq!(wire(&s.log, "tools/call"), 0);
            cleanup(&run, &target, &s);
        },
    );
}
