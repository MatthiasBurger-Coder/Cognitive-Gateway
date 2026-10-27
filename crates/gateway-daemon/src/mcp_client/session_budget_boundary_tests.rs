//! Public legacy facade + actual native CG09, bounded normalization and stdio.
//! The spending cursor always originates inside the actual coordinator callback.
use super::*;
use crate::mcp_client::{mapping::*, normalization::*, transport::*, *};
use crate::{
    goal_observation_test_harness as harness, native_policy_test_support as native_fixture,
};
use gateway_application::{
    connector_manifest::*, connector_policy::*, policy_application::PolicyApplication, sessions::*,
};
use gateway_domain::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
fn text(s: &str) -> NonEmptyText {
    NonEmptyText::new(s).unwrap()
}
fn amounts(calls: u64) -> SessionResourceAmounts {
    SessionResourceAmounts {
        connector_calls: calls,
        retrieval_rounds: calls,
        retrieval_results: 100,
        retrieval_tokens: 5_000_000,
        cost_microunits: 100,
        ..Default::default()
    }
}
fn native(scope: ContextScopeId) -> native_fixture::NativePolicy {
    let condition = DesiredCondition::new(
        ConditionId::new("required").unwrap(),
        SubjectPath::new(["state", "required"]).unwrap(),
        ComparisonOperator::Equals,
        Some(TypedValue::Boolean(true)),
    )
    .unwrap();
    let desired = DesiredState::new(
        DesiredStateId::new("required-state").unwrap(),
        vec![condition.clone()],
        ConditionExpression::condition(condition.id().clone()),
        vec![],
        vec![],
    )
    .unwrap();
    let seed = SupportedConnectorGoal::from_host_registration(
        Intent::new(IntentId::new("budget-native-task").unwrap(), desired),
        scope,
        content_reference("plan-seed", b"plan-seed").unwrap(),
        vec![PlanStepId::new("seed").unwrap()],
        vec![
            ConditionObservationMapping::new(
                condition.id().clone(),
                SourceId::new("native.source").unwrap(),
                CapabilityId::new("architecture.dependency-analysis").unwrap(),
                ConnectorOperationClass::Read,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    native_fixture::native(seed, OperatingMode::Development)
}
struct ExpectedAllocationIdentity {
    reservation: ContentDigest,
    binding: ResourceReservationBinding,
    amounts: SessionResourceAmounts,
    cost_unit: ReferenceId,
    claim_revision: Revision,
    fence: FenceToken,
    deadline: Instant,
}
struct State {
    checkpoint: SessionCheckpoint,
    live: AtomicBool,
    request_live: AtomicBool,
    allowed: Mutex<Vec<(ConnectorRequest, Option<SourceId>)>>,
    expected: Mutex<Option<ExpectedAllocationIdentity>>,
    mode: Mutex<&'static str>,
    audits: AtomicUsize,
    quote_calls: AtomicUsize,
    open: AtomicUsize,
}
impl State {
    fn new(cp: &SessionCheckpoint) -> Self {
        Self {
            checkpoint: cp.clone(),
            live: AtomicBool::new(true),
            request_live: AtomicBool::new(true),
            allowed: Mutex::new(vec![]),
            expected: Mutex::new(None),
            mode: Mutex::new("normal"),
            audits: AtomicUsize::new(0),
            quote_calls: AtomicUsize::new(0),
            open: AtomicUsize::new(0),
        }
    }
    fn allow(&self, r: &ConnectorRequest, s: Option<&SourceId>) {
        self.allowed.lock().unwrap().push((r.clone(), s.cloned()));
    }
    fn bind(&self, a: &SessionResourceAllocation) {
        *self.expected.lock().unwrap() = Some(ExpectedAllocationIdentity {
            reservation: a.reservation_id().clone(),
            binding: a.binding().clone(),
            amounts: *a.amounts(),
            cost_unit: a.cost_unit().clone(),
            claim_revision: a.claim_revision(),
            fence: a.fence(),
            deadline: a.deadline(),
        });
    }
}
struct Authority(Arc<State>);
impl SessionResourceAuthorityPort for Authority {
    fn validate_allocation(&self, a: &SessionResourceAllocation) -> Result<(), SessionError> {
        let expected = self.0.expected.lock().unwrap();
        let e = expected.as_ref().ok_or(SessionError::InvalidState)?;
        if !self.0.live.load(Ordering::SeqCst)
            || a.reservation_id() != &e.reservation
            || a.binding() != &e.binding
            || a.amounts() != &e.amounts
            || a.cost_unit() != &e.cost_unit
            || a.claim_revision() != e.claim_revision
            || a.fence() != e.fence
            || a.deadline() != e.deadline
            || a.binding().revision != self.0.checkpoint.snapshot.revision
            || Instant::now() >= a.deadline()
        {
            return Err(SessionError::AuthorityDenied);
        }
        Ok(())
    }
    fn validate_connector_discovery(
        &self,
        a: &SessionResourceAllocation,
        t: &ConnectorTarget,
    ) -> Result<(), SessionError> {
        self.validate_allocation(a)?;
        if t != &target(a.binding().scope.clone()) {
            return Err(SessionError::ScopeDenied);
        }
        Ok(())
    }
    fn validate_connector_allocation(
        &self,
        a: &SessionResourceAllocation,
        r: &ConnectorRequest,
        s: Option<&SourceId>,
    ) -> Result<(), SessionError> {
        self.validate_allocation(a)?;
        if !self.0.request_live.load(Ordering::SeqCst)
            || !self
                .0
                .allowed
                .lock()
                .unwrap()
                .contains(&(r.clone(), s.cloned()))
        {
            return Err(SessionError::ScopeDenied);
        }
        Ok(())
    }
}
struct Policy {
    state: Arc<State>,
    native: Arc<native_fixture::NativePolicy>,
}
impl Policy {
    fn snapshot(
        &self,
        r: &ConnectorRequest,
        s: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        if !self
            .state
            .allowed
            .lock()
            .unwrap()
            .contains(&(r.clone(), s.cloned()))
        {
            return Err(failure(
                ConnectorFailureCode::ScopeMismatch,
                &r.correlation,
                ConnectorEffect::NotDispatched,
            ));
        }
        let n = PolicyApplication
            .step_input(&self.native.resolved, &self.native.context, &r.step)
            .map_err(|_| {
                failure(
                    ConnectorFailureCode::PermissionDenied,
                    &r.correlation,
                    ConnectorEffect::NotDispatched,
                )
            })?;
        let e = self.state.expected.lock().unwrap();
        let reservation = e.as_ref().unwrap().reservation.as_str();
        Ok(ConnectorPolicySnapshot {
            step: n.step,
            resolved: n.resolved,
            has_prerequisites: n.has_prerequisites,
            constraints: n.constraints,
            preconditions: n.preconditions,
            authority: self.native.authority.clone(),
            facts: n.facts,
            operating_mode: n.operating_mode,
            execution_profile: n.execution_profile,
            process: n.process,
            consent_required: false,
            administrative_capabilities: BTreeSet::new(),
            policy: self.native.authority.policies[0].id().clone(),
            policy_revision: context_content_digest(
                // Fixture-owner policy input record: all native public fields,
                // without serializing a policy authority or using Debug text.
                &serde_json::to_vec(
                    &self
                        .native
                        .authority
                        .policies
                        .iter()
                        .map(|p| {
                            (
                                p.id(),
                                p.description(),
                                p.allowed_capability_ids(),
                                p.denied_capability_ids(),
                            )
                        })
                        .collect::<Vec<_>>(),
                )
                .unwrap(),
            ),
            consent_receipt: None,
            budget_reservation: text(if *self.state.mode.lock().unwrap() == "wrong-reservation" {
                "foreign-reservation"
            } else {
                reservation
            }),
            deadline: UnixTimestamp::new(if *self.state.mode.lock().unwrap() == "expired-policy" {
                0
            } else {
                (self.state.checkpoint.lease_until_ms.unwrap() / 1000) as i64
            }),
        })
    }
}
impl ConnectorPolicySource for Policy {
    fn admit_target(&self, t: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        if t == &target(self.state.checkpoint.goal.scope().clone()) {
            Ok(())
        } else {
            Err(failure(
                ConnectorFailureCode::ScopeMismatch,
                &text("target"),
                ConnectorEffect::NotDispatched,
            ))
        }
    }
    fn authorize(
        &self,
        r: &ConnectorRequest,
        s: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        let snapshot = self.snapshot(r, s)?;
        if *self.state.mode.lock().unwrap() == "revoke-authorize" {
            self.state.request_live.store(false, Ordering::SeqCst)
        }
        Ok(snapshot)
    }
    fn revalidate(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.snapshot(&d.request, s)
    }
    fn record_decision(&self, _: &ConnectorPolicyEvaluation<'_>) -> Result<(), ConnectorFailure> {
        self.state.audits.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
struct Collector;
impl AcquisitionPort for Collector {
    fn collect(&self, _: &ConnectorDispatch) -> Result<Acquisition, Error> {
        let now = UnixTimestamp::new(
            (SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()) as i64,
        );
        Ok(Acquisition {
            acquired_at: now,
            observed_at: Some(now),
            evaluated_at: now,
            max_age: FreshnessPolicy::new(60),
            authenticated_source: text("fixture.native.reader"),
            sensitivity: SensitivityClass::Internal,
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
struct Normalizer {
    actual: Arc<CanonicalResultNormalizer>,
    state: Arc<State>,
}
impl ConnectorResultNormalizer for Normalizer {
    fn normalize(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
        m: &MappingRecord,
        r: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        let result = self.actual.canonicalize(d, s, m, r)?.into_observation();
        if *self.state.mode.lock().unwrap() == "revoke-result" {
            self.state.request_live.store(false, Ordering::SeqCst)
        }
        Ok(result)
    }
}
struct Quotes {
    actual: Arc<CanonicalResultNormalizer>,
    state: Arc<State>,
    unit: ReferenceId,
}
impl Quotes {
    fn quote(
        &self,
        a: &SessionResourceAllocation,
        r: Option<&ConnectorRequest>,
    ) -> Result<SessionConnectorQuote, SessionError> {
        self.state.quote_calls.fetch_add(1, Ordering::SeqCst);
        let mut q = SessionConnectorQuote {
            cost_unit: self.unit.clone(),
            amounts: SessionResourceAmounts {
                connector_calls: 1,
                retrieval_rounds: 1,
                retrieval_results: if r.is_some() { 1 } else { 2 },
                retrieval_tokens: match r {
                    None => 4096,
                    Some(r) => {
                        self.actual
                            .resource_bounds()
                            .map_err(|_| SessionError::OutcomeUnknown)?
                            .value_bytes()
                            + serde_json::to_vec(&r.arguments)
                                .map_err(|_| SessionError::InvalidInput)?
                                .len() as u64
                    }
                },
                cost_microunits: 1,
                ..Default::default()
            },
        };
        Authority(self.state.clone()).validate_allocation(a)?;
        match *self.state.mode.lock().unwrap() {
            "unit" => q.cost_unit = ReferenceId::new("other-unit").unwrap(),
            "zero-calls" => q.amounts.connector_calls = 0,
            "model" => q.amounts.model_attempts = 1,
            "no-round" => q.amounts.retrieval_rounds = 0,
            "expensive" => q.amounts.cost_microunits = 101,
            "unknown-bound" => return Err(SessionError::OutcomeUnknown),
            "reprice" => q.amounts.cost_microunits = 2,
            "max-cost" => q.amounts.cost_microunits = u64::MAX,
            _ => (),
        };
        Ok(q)
    }
}
impl SessionConnectorQuotePort for Quotes {
    fn quote_discovery(
        &self,
        a: &SessionResourceAllocation,
        _: &ConnectorTarget,
    ) -> Result<SessionConnectorQuote, SessionError> {
        self.quote(a, None)
    }
    fn quote_request(
        &self,
        a: &SessionResourceAllocation,
        r: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<SessionConnectorQuote, SessionError> {
        self.quote(a, Some(r))
    }
    fn validate_inventory(
        &self,
        _: &SessionResourceAllocation,
        _: &SessionConnectorQuote,
        _: &ConnectorInventory,
    ) -> Result<(), SessionError> {
        if *self.state.mode.lock().unwrap() == "inventory-metrics" {
            Err(SessionError::OutcomeUnknown)
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
        if *self.state.mode.lock().unwrap() == "result-metrics" {
            return Err(SessionError::OutcomeUnknown);
        }
        if o.records.facts().len() as u64 > q.amounts.retrieval_results {
            return Err(SessionError::LimitExceeded);
        }
        Ok(())
    }
}
struct Factory {
    stdio: StdioConfig,
    state: Arc<State>,
}
impl TransportFactory for Factory {
    fn open(&self, c: &CallContext, l: &Limits) -> Result<Box<dyn Transport>, Error> {
        self.state.open.fetch_add(1, Ordering::SeqCst);
        self.stdio.open(c, l)
    }
    fn permits_tool(&self, _: &str, _: &str, _: &str) -> Result<(), Error> {
        if *self.state.mode.lock().unwrap() == "change-at-permit" {
            *self.state.mode.lock().unwrap() = "reprice";
        }
        Ok(())
    }
}
fn target(scope: ContextScopeId) -> ConnectorTarget {
    ConnectorTarget {
        connector: text("example.records"),
        scope,
    }
}
struct Fixture {
    host: Arc<McpConnectorHost>,
    quotes: Arc<Quotes>,
    state: Arc<State>,
    root: PathBuf,
    native: Arc<native_fixture::NativePolicy>,
}
impl Fixture {
    fn new(cp: &SessionCheckpoint, native: Arc<native_fixture::NativePolicy>) -> Self {
        Self::new_scoped(cp, native, None)
    }
    fn new_scoped(
        cp: &SessionCheckpoint,
        native: Arc<native_fixture::NativePolicy>,
        scope: Option<ContextScopeId>,
    ) -> Self {
        let configured = target(scope.unwrap_or_else(|| cp.goal.scope().clone()));
        let state = Arc::new(State::new(cp));
        let root = std::env::temp_dir().join(format!(
            "cg-budget-native-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let script = root.join("peer.py");
        std::fs::write(&script,r#"import json,sys,pathlib
root=pathlib.Path(sys.argv[1])
calls=0
for line in sys.stdin:
 r=json.loads(line)
 with (root/'wire.jsonl').open('a') as f:f.write(json.dumps(r)+'\n')
 if 'id' not in r:continue
 m=r['method']
 if m=='initialize':v={'protocolVersion':'2025-11-25','serverInfo':{'name':'bounded','version':'1.0.0'},'capabilities':{'tools':{'listChanged':True}}}
 elif m=='tools/list':v={'tools':[{'name':'records.read','inputSchema':{'type':'object','properties':{'query':{'type':'string'}},'required':['query'],'additionalProperties':False}}]}
 elif m=='tools/call':
  calls+=1
  mode=(root/'fault').read_text() if (root/'fault').exists() else 'normal'
  if mode=='eof':sys.exit(0)
  if mode in ('rate-once','rate-limit','rate-long') and (mode!='rate-once' or calls==1):
   print(json.dumps({'jsonrpc':'2.0','id':r['id'],'error':{'code':-32029,'message':'fixture','data':{'retryAfterMs':1000 if mode=='rate-long' else 10}}}),flush=True)
   continue
  v={'content':[],'structuredContent':{'object':r['params']['arguments']['query'],'value':True},'isError':False}
 else:v={}
 print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':v}),flush=True)
"#).unwrap();
        let tool = serde_json::json!({"name":"records.read","inputSchema":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}});
        let mut manifest: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../../tests/fixtures/connector-mapping-v1/manifest.json"
        ))
        .unwrap();
        let mut manifest = manifest["manifest"].take();
        manifest["inventory"].as_array_mut().unwrap().truncate(1);
        manifest["inventory"][0]["schema_digest"] =
            serde_json::json!(discovery_schema_digest(InventoryKind::Tool, &tool));
        manifest["inventory"][0]["source_mapping_ref"] =
            serde_json::json!("mapping.records.read.v1");
        let manifest =
            ConnectorManifest::from_definition_json(&serde_json::to_vec(&manifest).unwrap())
                .unwrap();
        let mut document: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../../tests/fixtures/connector-mapping-v1/mapping.json"
        ))
        .unwrap();
        document["records"].as_array_mut().unwrap().truncate(1);
        document["records"][0]["capability"] = serde_json::json!(native.capability);
        document["records"][0]["source"] = serde_json::json!("native.source");
        document["records"][0]["result_schema"] = serde_json::json!({"type":"object","properties":{"object":{"type":"string"},"value":{"type":"boolean"}},"required":["object","value"],"additionalProperties":false});
        let document = MappingDocument::from_json(&serde_json::to_vec(&document).unwrap()).unwrap();
        let normalizer = Arc::new(
            CanonicalResultNormalizer::new_qualified(
                NormalizationProfile {
                    source: SourceId::new("native.source").unwrap(),
                    source_kind: SourceKind::Tool,
                    object_field: "object".into(),
                    revision_field: None,
                    digest_field: None,
                    fields: [(
                        "value".into(),
                        SubjectPath::new(["state", "required"]).unwrap(),
                    )]
                    .into(),
                },
                Box::new(Collector),
            )
            .unwrap(),
        );
        let mapper = CanonicalConnectorMapping::new(
            DiscoveryMapper::new(configured.clone(), manifest.clone(), document).unwrap(),
            Box::new(Normalizer {
                actual: normalizer.clone(),
                state: state.clone(),
            }),
        );
        let host = Arc::new(
            McpConnectorHost::new(
                HostConfig {
                    target: configured.clone(),
                    manifest,
                    profile: ConnectorCompatibilityProfile {
                        gateway: ConnectorVersion {
                            major: 0,
                            minor: 1,
                            patch: 0,
                        },
                        transports: [ConnectorTransport::Stdio].into(),
                    },
                    limits: Limits::default(),
                },
                Box::new(Factory {
                    stdio: StdioConfig {
                        executable: "/usr/bin/python3".into(),
                        arguments: vec![
                            "-I".into(),
                            script.to_string_lossy().into_owned(),
                            root.to_string_lossy().into_owned(),
                        ],
                        directory: root.clone(),
                        environment: BTreeMap::new(),
                    },
                    state: state.clone(),
                }),
                Arc::new(Cg09ConnectorAdmission::new(Policy {
                    state: state.clone(),
                    native: native.clone(),
                })),
                Box::new(mapper),
                Arc::new(()),
            )
            .unwrap(),
        );
        Self {
            host,
            quotes: Arc::new(Quotes {
                actual: normalizer,
                state: state.clone(),
                unit: ReferenceId::new("budget-unit").unwrap(),
            }),
            state,
            root,
            native,
        }
    }
    fn run(&self, a: SessionResourceAllocation) -> Result<SessionConnectorRun, SessionError> {
        self.state.bind(&a);
        SessionConnectorRun::new(
            self.host.clone(),
            a,
            Arc::new(Authority(self.state.clone())),
            self.quotes.clone(),
            CallContext::new(
                text("budget-discovery"),
                Instant::now() + Duration::from_secs(60),
            ),
        )
    }
    fn request(&self, inventory: &ConnectorInventory, cp: &SessionCheckpoint) -> ConnectorRequest {
        ConnectorRequest {
            target: inventory.target.clone(),
            capability: self.native.capability.clone(),
            class: ConnectorOperationClass::Read,
            inventory_revision: inventory.revision.clone(),
            arguments: [(text("query"), TypedValue::string("bounded-object").unwrap())].into(),
            task: TaskId::new(self.native.goal.intent().id().as_str()).unwrap(),
            step: cp.goal.as_connector().unwrap().steps()[0].clone(),
            execution: text("native-inspection"),
            correlation: text("budget-read"),
        }
    }
    fn calls(&self) -> usize {
        std::fs::read_to_string(self.root.join("wire.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter(|line| {
                serde_json::from_str::<serde_json::Value>(line).unwrap()["method"] == "tools/call"
            })
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.host.lifecycle(
            &target(self.state.checkpoint.goal.scope().clone()),
            ConnectorLifecycleCommand::Shutdown,
        );
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn exercise(
    callback: impl FnOnce(
        &SessionCheckpoint,
        &mut SessionPhaseResources,
        Arc<native_fixture::NativePolicy>,
    ) -> Result<(), SessionError>,
) {
    exercise_with_ceiling(amounts(10), callback);
}
fn exercise_with_ceiling(
    ceiling: SessionResourceAmounts,
    callback: impl FnOnce(
        &SessionCheckpoint,
        &mut SessionPhaseResources,
        Arc<native_fixture::NativePolicy>,
    ) -> Result<(), SessionError>,
) {
    let native = Arc::new(native(ContextScopeId::new("native-budget-scope").unwrap()));
    harness::with_preparation_resources_for_goal(
        native.goal.clone(),
        ReferenceId::new("budget-unit").unwrap(),
        ceiling,
        |cp, r| callback(cp, r, native),
    );
}
#[test]
fn real_native_legacy_run_charges_discovery_and_read_and_audits_only_real_decisions() {
    exercise(|cp, resources, native| {
        let f = Fixture::new(cp, native);
        let run = f.run(resources.allocate(amounts(4))?)?;
        let inventory = run.discover(&target(cp.goal.scope().clone())).unwrap();
        let r = f.request(&inventory, cp);
        f.state.allow(&r, None);
        let observation = run.invoke(&r).unwrap();
        assert_eq!(observation.metadata.effect, ConnectorEffect::NoMutation);
        assert_eq!(observation.metadata.task, r.task);
        assert_eq!(f.calls(), 1);
        assert_eq!(run.usage()?.connector_calls, 2);
        assert_eq!(f.state.audits.load(Ordering::SeqCst), 2);
        assert!(f.state.quote_calls.load(Ordering::SeqCst) > 4);
        // Actual native source pins include source option; a resource API cannot
        // smuggle a tool mapping through with an unadmitted source identity.
        assert!(
            run.read_resource(&r, &SourceId::new("foreign").unwrap())
                .is_err()
        );
        assert_eq!(f.calls(), 1);
        Ok(())
    });
}
#[test]
fn malformed_unknown_or_over_slice_quotes_fail_before_bootstrap_without_spending() {
    for mode in [
        "unit",
        "zero-calls",
        "model",
        "no-round",
        "expensive",
        "unknown-bound",
    ] {
        exercise(|cp, resources, native| {
            let f = Fixture::new(cp, native);
            let run = f.run(resources.allocate(amounts(3))?)?;
            *f.state.mode.lock().unwrap() = mode;
            let failure = run.discover(&target(cp.goal.scope().clone())).unwrap_err();
            assert_eq!(failure.effect, ConnectorEffect::NotDispatched, "{mode}");
            assert_eq!(run.usage()?, SessionResourceAmounts::default(), "{mode}");
            assert_eq!(f.state.open.load(Ordering::SeqCst), 0, "{mode}");
            Ok(())
        });
    }
}
#[test]
fn current_request_revocation_changed_quote_and_wrong_native_reservation_send_zero_calls() {
    for mode in ["revoke-authorize", "change-at-permit", "wrong-reservation"] {
        exercise(|cp, resources, native| {
            let f = Fixture::new(cp, native);
            let run = f.run(resources.allocate(amounts(4))?)?;
            let inventory = run.discover(&target(cp.goal.scope().clone())).unwrap();
            let r = f.request(&inventory, cp);
            f.state.allow(&r, None);
            *f.state.mode.lock().unwrap() = mode;
            let failure = run.invoke(&r).unwrap_err();
            assert_eq!(failure.effect, ConnectorEffect::NotDispatched, "{mode}");
            assert_eq!(f.calls(), 0, "{mode}");
            assert_eq!(
                run.usage()?.connector_calls,
                2,
                "quote stays held after rejection: {mode}"
            );
            Ok(())
        });
    }
}
#[test]
fn post_result_accounting_failure_is_sticky_before_later_permission_or_budget_checks() {
    for mode in ["inventory-metrics", "result-metrics"] {
        exercise(|cp, resources, native| {
            let f = Fixture::new(cp, native);
            let run = f.run(resources.allocate(amounts(5))?)?;
            let (failure, r) = if mode == "inventory-metrics" {
                *f.state.mode.lock().unwrap() = mode;
                let failure = run.discover(&target(cp.goal.scope().clone())).unwrap_err();
                let inventory = ConnectorInventory {
                    target: target(cp.goal.scope().clone()),
                    revision: context_content_digest(b"unavailable-summary"),
                    capabilities: vec![],
                    resources: vec![],
                };
                (failure, f.request(&inventory, cp))
            } else {
                let inventory = run.discover(&target(cp.goal.scope().clone())).unwrap();
                let r = f.request(&inventory, cp);
                f.state.allow(&r, None);
                *f.state.mode.lock().unwrap() = mode;
                (run.invoke(&r).unwrap_err(), r)
            };
            assert_eq!(failure.effect, ConnectorEffect::NoMutation);
            let used = run.usage()?;
            let calls = f.calls();
            let quotes = f.state.quote_calls.load(Ordering::SeqCst);
            f.state.live.store(false, Ordering::SeqCst);
            *f.state.mode.lock().unwrap() = "expensive";
            assert_eq!(run.invoke(&r).unwrap_err(), failure);
            assert_eq!(
                run.discover(&target(ContextScopeId::new("foreign").unwrap()))
                    .unwrap_err(),
                failure
            );
            assert_eq!(f.calls(), calls);
            assert_eq!(run.usage()?, used);
            assert_eq!(f.state.quote_calls.load(Ordering::SeqCst), quotes);
            Ok(())
        });
    }
}
#[test]
fn confirmed_readonly_result_loses_permission_without_inventing_not_dispatched_or_refund() {
    exercise(|cp, resources, native| {
        let f = Fixture::new(cp, native);
        let run = f.run(resources.allocate(amounts(5))?)?;
        let inventory = run.discover(&target(cp.goal.scope().clone())).unwrap();
        let r = f.request(&inventory, cp);
        f.state.allow(&r, None);
        *f.state.mode.lock().unwrap() = "revoke-result";
        let err = run.invoke(&r).unwrap_err();
        assert_eq!(err.effect, ConnectorEffect::NoMutation);
        assert_eq!(f.calls(), 1);
        assert_eq!(run.usage()?.connector_calls, 2);
        // Honest hard bound remains valid: confirmed reads may retry only after the
        // trusted source re-admits them, with another debit inside the same ceiling.
        *f.state.mode.lock().unwrap() = "normal";
        f.state.request_live.store(true, Ordering::SeqCst);
        run.invoke(&r).unwrap();
        assert_eq!(f.calls(), 2);
        assert_eq!(run.usage()?.connector_calls, 3);
        Ok(())
    });
}
#[test]
fn context_and_native_slice_construction_denials_never_open_transport() {
    for mode in ["no-calls", "cancelled", "expired", "authority"] {
        exercise(|cp, resources, native| {
            let f = Fixture::new(cp, native);
            let a = if mode == "no-calls" {
                resources.allocate(SessionResourceAmounts {
                    retrieval_rounds: 1,
                    ..Default::default()
                })?
            } else {
                resources.allocate(amounts(2))?
            };
            f.state.bind(&a);
            let context = CallContext::new(
                text("construction"),
                if mode == "expired" {
                    Instant::now()
                } else {
                    Instant::now() + Duration::from_secs(60)
                },
            );
            if mode == "cancelled" {
                context.cancel()
            }
            if mode == "authority" {
                f.state.live.store(false, Ordering::SeqCst)
            }
            let result = SessionConnectorRun::new(
                f.host.clone(),
                a,
                Arc::new(Authority(f.state.clone())),
                f.quotes.clone(),
                context,
            );
            assert!(result.is_err(), "{mode}");
            assert_eq!(f.state.open.load(Ordering::SeqCst), 0);
            Ok(())
        });
    }
}
#[test]
fn nested_poisoned_usage_and_cleanup_preserve_safety_and_do_not_require_fresh_effect_grants() {
    exercise(|cp, resources, native| {
        let f = Fixture::new(cp, native);
        let run = f.run(resources.allocate(amounts(4))?)?;
        {
            let _held = run.lock(&text("nested")).unwrap();
            assert_eq!(
                run.discover(&target(cp.goal.scope().clone()))
                    .unwrap_err()
                    .code,
                ConnectorFailureCode::Backpressure
            );
            assert_eq!(run.usage().unwrap_err(), SessionError::Unavailable);
        }
        assert_eq!(f.state.open.load(Ordering::SeqCst), 0);
        std::thread::scope(|scope| {
            assert!(
                scope
                    .spawn(|| {
                        let _guard = run.usage.lock().unwrap();
                        panic!("controlled poison of usage serialization")
                    })
                    .join()
                    .is_err()
            );
        });
        assert_eq!(
            run.discover(&target(cp.goal.scope().clone()))
                .unwrap_err()
                .code,
            ConnectorFailureCode::Unavailable
        );
        assert_eq!(
            run.lifecycle(
                &target(cp.goal.scope().clone()),
                ConnectorLifecycleCommand::Connect
            )
            .unwrap_err()
            .effect,
            ConnectorEffect::NotDispatched
        );
        assert_eq!(
            run.lifecycle(
                &target(ContextScopeId::new("foreign").unwrap()),
                ConnectorLifecycleCommand::Shutdown
            )
            .unwrap_err()
            .code,
            ConnectorFailureCode::ScopeMismatch
        );
        f.state.live.store(false, Ordering::SeqCst);
        run.lifecycle(
            &target(cp.goal.scope().clone()),
            ConnectorLifecycleCommand::Shutdown,
        )
        .unwrap();
        assert_eq!(f.state.open.load(Ordering::SeqCst), 0);
        Ok(())
    });
}
#[test]
fn local_context_cancellation_is_shared_and_never_creates_a_new_deadline_or_allowance() {
    exercise(|cp, resources, native| {
        let f = Fixture::new(cp, native);
        let a = resources.allocate(amounts(3))?;
        let native_deadline = a.deadline();
        let run = f.run(a)?;
        assert_eq!(run.context.deadline, native_deadline);
        let child = run.context(&text("other-correlation"));
        run.cancel();
        assert_eq!(child.check(), Err(Error::Cancelled));
        assert!(run.discover(&target(cp.goal.scope().clone())).is_err());
        assert_eq!(run.usage()?, SessionResourceAmounts::default());
        assert_eq!(f.state.open.load(Ordering::SeqCst), 0);
        Ok(())
    });
}

#[test]
fn default_coordinator_slice_cannot_be_attached_to_a_foreign_configured_host() {
    let scope = ContextScopeId::new("constructor-owner-scope").unwrap();
    harness::with_preparation_resources(
        scope,
        ReferenceId::new("budget-unit").unwrap(),
        amounts(3),
        |cp, resources| {
            let policy = Arc::new(native(cp.goal.scope().clone()));
            let fixture = Fixture::new_scoped(
                cp,
                policy,
                Some(ContextScopeId::new("foreign-configured-scope").unwrap()),
            );
            let allocation = resources.allocate(amounts(2))?;
            assert!(matches!(
                fixture.run(allocation),
                Err(SessionError::ScopeDenied)
            ));
            assert_eq!(fixture.state.open.load(Ordering::SeqCst), 0);
            assert_eq!(fixture.calls(), 0);
            Ok(())
        },
    );
}

#[test]
fn cumulative_cost_overflow_cannot_wrap_into_an_apparently_affordable_read() {
    let mut ceiling = amounts(4);
    ceiling.cost_microunits = u64::MAX;
    exercise_with_ceiling(ceiling, |cp, resources, native| {
        let f = Fixture::new(cp, native);
        let run = f.run(resources.allocate(ceiling)?)?;
        let inventory = run.discover(&target(cp.goal.scope().clone())).unwrap();
        let request = f.request(&inventory, cp);
        f.state.allow(&request, None);
        let held = run.usage()?;
        // This is an intentionally conservative fixed local price bound, not
        // an underestimated result bound. The previous discovery already held1.
        *f.state.mode.lock().unwrap() = "max-cost";
        let failure = run.invoke(&request).unwrap_err();
        assert_eq!(failure.code, ConnectorFailureCode::BudgetExhausted);
        assert_eq!(failure.effect, ConnectorEffect::NotDispatched);
        assert_eq!(run.usage()?, held);
        assert_eq!(f.calls(), 0);
        assert_eq!(f.state.audits.load(Ordering::SeqCst), 0);
        Ok(())
    });
}

#[test]
fn readonly_slice_rejects_mutation_step_or_target_substitution_before_quote_and_wire() {
    exercise(|cp, resources, native| {
        let f = Fixture::new(cp, native);
        let run = f.run(resources.allocate(amounts(4))?)?;
        let inventory = run.discover(&target(cp.goal.scope().clone())).unwrap();
        let request = f.request(&inventory, cp);
        let held = run.usage()?;
        let quoted = f.state.quote_calls.load(Ordering::SeqCst);
        for class in [
            ConnectorOperationClass::Mutate,
            ConnectorOperationClass::Admin,
        ] {
            let mut other = request.clone();
            other.class = class;
            f.state.allow(&other, None);
            let failure = run.invoke(&other).unwrap_err();
            assert_eq!(failure.code, ConnectorFailureCode::ScopeMismatch);
            assert_eq!(failure.effect, ConnectorEffect::NotDispatched);
        }
        let mut other = request.clone();
        other.step = PlanStepId::new("unrelated-step").unwrap();
        f.state.allow(&other, None);
        assert_eq!(
            run.invoke(&other).unwrap_err().code,
            ConnectorFailureCode::ScopeMismatch
        );
        let mut foreign = request.target.clone();
        foreign.scope = ContextScopeId::new("unrelated-scope").unwrap();
        other = request;
        other.target = foreign.clone();
        f.state.allow(&other, None);
        assert_eq!(
            run.invoke(&other).unwrap_err().code,
            ConnectorFailureCode::ScopeMismatch
        );
        assert_eq!(
            run.discover(&foreign).unwrap_err().code,
            ConnectorFailureCode::ScopeMismatch
        );
        assert_eq!(run.usage()?, held);
        assert_eq!(f.state.quote_calls.load(Ordering::SeqCst), quoted);
        assert_eq!(f.calls(), 0);
        Ok(())
    });
}

#[test]
fn retries_and_reconnects_charge_the_same_native_session_slice() {
    for (mode, ceiling, expected_calls, expected_usage) in [
        ("rate-once", 3, 2, 3),
        ("rate-limit", 10, 3, 4),
        ("rate-limit", 2, 1, 2),
        ("eof", 6, 3, 6),
    ] {
        exercise_with_ceiling(amounts(ceiling), |cp, resources, native| {
            let f = Fixture::new(cp, native);
            std::fs::write(f.root.join("fault"), mode).unwrap();
            let run = f.run(resources.allocate(amounts(ceiling))?)?;
            let inventory = run.discover(&target(cp.goal.scope().clone())).unwrap();
            let r = f.request(&inventory, cp);
            f.state.allow(&r, None);
            let result = run.invoke(&r);
            if mode == "rate-once" {
                assert!(result.is_ok(), "{result:?}");
            } else {
                assert_eq!(
                    result.unwrap_err().code,
                    match (mode, ceiling) {
                        ("eof", _) => ConnectorFailureCode::Unavailable,
                        (_, 2) => ConnectorFailureCode::BudgetExhausted,
                        _ => ConnectorFailureCode::RateLimited,
                    }
                );
            }
            assert_eq!(f.calls(), expected_calls);
            assert_eq!(run.usage()?.connector_calls, expected_usage);
            assert_eq!(run.usage()?.cost_microunits, expected_usage);
            assert!(run.retry_attempts()? > 0);
            let retained = run.usage()?;
            run.cancel();
            assert_eq!(
                run.invoke(&r).unwrap_err().code,
                ConnectorFailureCode::Cancelled
            );
            assert_eq!(run.usage()?, retained);
            Ok(())
        });
    }
}

#[test]
fn cancellation_and_authority_revocation_interrupt_native_backoff_without_another_send() {
    for cancel in [true, false] {
        exercise(|cp, resources, native| {
            let f = Fixture::new(cp, native);
            std::fs::write(f.root.join("fault"), "rate-long").unwrap();
            let run = f.run(resources.allocate(amounts(5))?)?;
            let inventory = run.discover(&target(cp.goal.scope().clone())).unwrap();
            let r = f.request(&inventory, cp);
            f.state.allow(&r, None);
            std::thread::scope(|scope| {
                let worker = scope.spawn(|| run.invoke(&r));
                let until = Instant::now() + Duration::from_secs(5);
                while f.host.health_snapshot().retry_after.is_zero() {
                    assert!(Instant::now() < until);
                    std::thread::sleep(Duration::from_millis(2));
                }
                if cancel {
                    run.cancel();
                } else {
                    f.state.request_live.store(false, Ordering::SeqCst);
                }
                assert_eq!(
                    worker.join().unwrap().unwrap_err().code,
                    if cancel {
                        ConnectorFailureCode::Cancelled
                    } else {
                        ConnectorFailureCode::ScopeMismatch
                    }
                );
            });
            assert_eq!(f.calls(), 1);
            assert_eq!(run.usage()?.connector_calls, 2);
            assert_eq!(run.retry_attempts()?, 0);
            Ok(())
        });
    }
}

#[test]
fn expired_native_policy_deadline_never_spends_a_retry_or_sends_a_tool() {
    exercise(|cp, resources, native| {
        let f = Fixture::new(cp, native);
        *f.state.mode.lock().unwrap() = "expired-policy";
        let run = f.run(resources.allocate(amounts(4))?)?;
        let inventory = run.discover(&target(cp.goal.scope().clone())).unwrap();
        let r = f.request(&inventory, cp);
        f.state.allow(&r, None);
        let error = run.invoke(&r).unwrap_err();
        assert_eq!(
            (error.code, error.effect),
            (
                ConnectorFailureCode::Timeout,
                ConnectorEffect::NotDispatched
            )
        );
        assert_eq!(run.usage()?.connector_calls, 2);
        assert_eq!(run.retry_attempts()?, 0);
        assert_eq!(f.calls(), 0);
        Ok(())
    });
}
