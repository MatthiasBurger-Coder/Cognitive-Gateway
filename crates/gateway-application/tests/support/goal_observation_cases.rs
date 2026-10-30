//! Coordinator-owned, invocation-free native evidence component tests.
use super::*;
use gateway_application::{
    PlanningCapabilitySnapshot,
    closed_loop::{ClosedLoop, LoopRules, VerifiedOutcomeReceipt},
};
#[path = "pre_execution_goal.rs"]
mod observed;
struct ObservationHost {
    legacy: Host,
    registered: SupportedConnectorGoal,
    reads: AtomicUsize,
    verifies: AtomicUsize,
    outage: AtomicUsize,
    drift: AtomicUsize,
    weak: bool,
    invalid_budget: bool,
    observe_outage: bool,
    verify_expire: bool,
    verify_hook: Mutex<Option<Box<dyn Fn()>>>,
    observe_hook: Mutex<Option<Box<dyn Fn()>>>,
    changed_unit: bool,
    zero_calls: bool,
    no_receipt: bool,
    wrong_receipt: bool,
    observe_expire: bool,
}
impl ObservationHost {
    fn new() -> Self {
        Self {
            legacy: Host::new(false),
            registered: observed::goal(ContextScopeId::new("project-a").unwrap()),
            reads: AtomicUsize::new(0),
            verifies: AtomicUsize::new(0),
            outage: AtomicUsize::new(0),
            drift: AtomicUsize::new(0),
            weak: false,
            invalid_budget: false,
            observe_outage: false,
            verify_expire: false,
            verify_hook: Mutex::new(None),
            observe_hook: Mutex::new(None),
            changed_unit: false,
            zero_calls: false,
            no_receipt: false,
            wrong_receipt: false,
            observe_expire: false,
        }
    }
    fn amounts(&self) -> SessionResourceAmounts {
        SessionResourceAmounts {
            connector_calls: 1,
            retrieval_rounds: 1,
            retrieval_results: 2,
            retrieval_tokens: 10000,
            cost_microunits: 1,
            ..Default::default()
        }
    }
    fn rules(&self) -> LoopRules {
        let f = fixture::Fixture::new();
        LoopRules {
            capabilities: PlanningCapabilitySnapshot::new(
                f.resolved.snapshot.input().index.clone(),
                f.resolved.report.basis.registry_fingerprint.as_str(),
                PlanningIrVersion::V1,
            )
            .unwrap(),
            requirements: CapabilityRequirementRules::default(),
            planner: PlannerRules::default(),
            max_iterations: 2,
            max_retries: 0,
        }
    }
}
impl InteractionAuthorityPort for ObservationHost {
    fn load_consent(
        &self,
        _: &OwnerBinding,
        _: &ConsentRecordRef,
    ) -> Result<ConsentStatus, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn live_status(&self, _: &VerifiedConsent) -> Result<ConsentStatus, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}
impl StructuredSessionHost for ObservationHost {
    fn now_ms(&self) -> Result<u64, SessionError> {
        self.legacy.now_ms()
    }
    fn register(
        &self,
        _: &OwnerBinding,
        _: Intent,
        _: RequestedExecution,
    ) -> Result<(SupportedArtifactGoal, SessionBudget), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn register_goal(
        &self,
        o: &OwnerBinding,
        i: Intent,
        e: RequestedExecution,
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
        self.disclose(o, &SessionId::new("registration").unwrap())?;
        if &i != self.registered.intent() || e != selection() {
            return Err(SessionError::UnsupportedCapability);
        }
        Ok((
            SupportedSessionGoal::Connector(self.registered.clone()),
            SessionBudget {
                actions: 0,
                retries: 0,
                max_actions: 2,
                max_retries: 1,
                deadline_ms: 1_000_000,
            },
        ))
    }
    fn prepare(&self, cp: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        self.legacy.prepare(cp)
    }
    fn disclose(&self, o: &OwnerBinding, s: &SessionId) -> Result<(), SessionError> {
        self.legacy.disclose(o, s)
    }
    fn issuer(&self, _: &OwnerBinding, _: &PrincipalId) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn goal_observation_runtime(&self) -> Option<&dyn ReadonlyGoalObservationRuntimePort> {
        Some(self)
    }
}
impl ReadonlyGoalObservationRuntimePort for ObservationHost {
    fn resource_limits(
        &self,
        _: &SessionCheckpoint,
    ) -> Result<SessionResourceLimits, SessionError> {
        Ok(SessionResourceLimits {
            cost_unit: ReferenceId::new(if self.changed_unit {
                "other-unit"
            } else {
                "credits"
            })
            .unwrap(),
            amounts: self.amounts(),
        })
    }
    fn preparation_budget(
        &self,
        _: &SessionCheckpoint,
    ) -> Result<SessionResourceAmounts, SessionError> {
        let mut a = self.amounts();
        if self.invalid_budget {
            a.model_attempts = 1;
        }
        if self.zero_calls {
            a.connector_calls = 0;
        }
        Ok(a)
    }
    fn observe_goal(
        &self,
        cp: &SessionCheckpoint,
        resources: &mut SessionPhaseResources,
    ) -> Result<Option<PreExecutionGoalCandidate>, SessionError> {
        assert_eq!(cp.snapshot.state, SessionState::Dispatching);
        assert_eq!(resources.claim_revision(), cp.snapshot.revision);
        assert_eq!(resources.phase(), SessionResourcePhase::Preparation);
        assert_eq!(
            cp.snapshot.dispatch.as_ref(),
            Some(&resources.binding().dispatch)
        );
        let allocation = resources.allocate(self.amounts())?;
        assert!(resources.remaining().is_zero());
        assert_eq!(
            resources.allocate(self.amounts()).unwrap_err(),
            SessionError::LimitExceeded
        );
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.observe_outage {
            return Err(SessionError::StorageUnavailable);
        }
        let binding = PreExecutionGoalBinding {
            acquisition: allocation.binding().clone(),
            reservation: allocation.reservation_id().clone(),
            claim_revision: allocation.claim_revision(),
            task: TaskId::new("actual-goal-read-task").unwrap(),
            execution: NonEmptyText::new("native-read-context").unwrap(),
            context_digest: context_content_digest(b"native-read-context"),
        };
        let data = observed::data(&self.registered, binding, !self.weak, self.now_ms()?);
        let candidate = observed::candidate(data, &self.registered, self.rules());
        if self.observe_expire {
            self.legacy
                .now
                .store(cp.lease_until_ms.unwrap(), Ordering::SeqCst);
        }
        if let Some(hook) = self.observe_hook.lock().unwrap().take() {
            hook();
        }
        Ok(candidate)
    }
    fn verify_retained_goal(
        &self,
        cp: &SessionCheckpoint,
        data: &PreExecutionGoalData,
    ) -> Result<Option<VerifiedOutcomeReceipt>, SessionError> {
        self.verifies.fetch_add(1, Ordering::SeqCst);
        if self.no_receipt {
            return Ok(None);
        }
        if self.outage.load(Ordering::SeqCst) > 0 {
            return Err(SessionError::StorageUnavailable);
        }
        if self.drift.load(Ordering::SeqCst) > 0
            || data.definition_pins() != observed::definitions(self.registered.scope())
        {
            return Err(SessionError::StaleRevision);
        }
        let intent = if self.wrong_receipt {
            let desired = cp.goal.intent().desired_state();
            let condition = desired.conditions()[0].clone();
            Intent::new(
                cp.goal.intent().id().clone(),
                DesiredState::new(
                    desired.id().clone(),
                    vec![condition.clone()],
                    ConditionExpression::condition(condition.id().clone()),
                    vec![],
                    vec![],
                )
                .unwrap(),
            )
        } else {
            cp.goal.intent().clone()
        };
        let proof = ClosedLoop::verify_retained(
            data.execution()?,
            self.registered.scope().clone(),
            intent,
            data.readset(),
            self.rules(),
        )
        .map_err(|_| SessionError::InvalidInteraction)?;
        if self.verify_expire {
            self.legacy.now.store(
                cp.pre_execution_goal.as_ref().unwrap().valid_until_ms,
                Ordering::SeqCst,
            );
        }
        if let Some(hook) = self.verify_hook.lock().unwrap().take() {
            hook();
        }
        Ok(proof)
    }
}
fn coordinator() -> SessionCoordinator<Repository, ObservationHost> {
    SessionCoordinator::new(Repository::default(), ObservationHost::new())
}
fn start_observation(c: &SessionCoordinator<Repository, ObservationHost>) -> SessionSnapshot {
    c.execute(
        &owner(),
        SessionCommand::Start {
            command: CommandId::new("start-observation").unwrap(),
            intent: c.host.registered.intent().clone(),
            execution: selection(),
        },
    )
    .unwrap()
}
fn continue_observation(
    c: &SessionCoordinator<Repository, ObservationHost>,
    s: &SessionSnapshot,
) -> Result<SessionSnapshot, SessionError> {
    c.execute(
        &owner(),
        SessionCommand::Continue {
            at: Mutation {
                session: s.session.clone(),
                command: CommandId::new("observe").unwrap(),
                expected_revision: s.revision,
            },
        },
    )
}
#[test]
fn observation_kernel_completes_native_fullgoal_without_artifact_or_model_preparation() {
    let c = coordinator();
    let started = start_observation(&c);
    let done = continue_observation(&c, &started).unwrap();
    assert_eq!(done.state, SessionState::Completed);
    let cp = c.details(&owner(), &done.session).unwrap();
    assert!(cp.pre_execution_goal.is_some());
    assert!(cp.artifact.is_none());
    assert_eq!(c.host.legacy.prepares.load(Ordering::SeqCst), 0);
    assert_eq!(c.host.reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        cp.resource_ledger.as_ref().unwrap().used(),
        &c.host.amounts()
    );
    assert_eq!(c.recover(&owner(), &done.session).unwrap(), done);
    let evidence = c
        .repository
        .load_artifact(&owner(), cp.snapshot.final_evidence.as_ref().unwrap())
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&evidence).unwrap();
    assert_eq!(value["purpose"], "PRE_EXECUTION_GOAL");
    assert!(value.get("invocation").is_none());
}
#[test]
fn persisted_runnable_marker_replays_native_verify_without_new_allocation_or_read() {
    let c = coordinator();
    c.host.outage.store(1, Ordering::SeqCst);
    let started = start_observation(&c);
    assert_eq!(
        continue_observation(&c, &started),
        Err(SessionError::StorageUnavailable)
    );
    let cp = c.details(&owner(), &started.session).unwrap();
    assert_eq!(cp.snapshot.state, SessionState::Runnable);
    assert!(cp.pre_execution_goal.is_some());
    let ledger = cp.resource_ledger.clone();
    c.host.outage.store(0, Ordering::SeqCst);
    assert_eq!(
        c.recover(&owner(), &started.session).unwrap().state,
        SessionState::Completed
    );
    assert_eq!(c.host.reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        c.details(&owner(), &started.session)
            .unwrap()
            .resource_ledger,
        ledger
    );
}
#[test]
fn weak_goal_or_model_budget_cannot_complete_or_refill_preparation() {
    for invalid_budget in [false, true] {
        let mut c = coordinator();
        c.host.weak = true;
        c.host.invalid_budget = invalid_budget;
        let started = start_observation(&c);
        assert_eq!(
            continue_observation(&c, &started),
            Err(SessionError::UnsupportedCapability)
        );
        let cp = c.details(&owner(), &started.session).unwrap();
        assert!(cp.pre_execution_goal.is_none());
        assert_eq!(
            c.host.reads.load(Ordering::SeqCst),
            usize::from(!invalid_budget)
        );
        if !invalid_budget {
            assert_eq!(cp.snapshot.state, SessionState::Failed);
            assert_eq!(cp.resource_ledger.as_ref().unwrap().reservations().len(), 1);
        }
        assert_eq!(c.host.legacy.prepares.load(Ordering::SeqCst), 0);
    }
}
#[test]
fn stale_corrupt_wrongkind_or_definition_drift_marker_never_reacquires_or_completes() {
    for case in ["bytes", "kind", "age", "definitions"] {
        let c = coordinator();
        c.host.outage.store(1, Ordering::SeqCst);
        let started = start_observation(&c);
        assert_eq!(
            continue_observation(&c, &started),
            Err(SessionError::StorageUnavailable)
        );
        let cp = c.details(&owner(), &started.session).unwrap();
        let marker = cp.pre_execution_goal.unwrap();
        c.host.outage.store(0, Ordering::SeqCst);
        match case {
            "bytes" | "kind" => {
                let mut rows = c.repository.rows.lock().unwrap();
                let record = &mut rows
                    .records
                    .iter_mut()
                    .find(|(_, r)| r.reference == marker.record)
                    .unwrap()
                    .1;
                if case == "bytes" {
                    record.bytes.push(b' ');
                } else {
                    record.kind = RecordKind::Artifact;
                }
            }
            "age" => c
                .host
                .legacy
                .now
                .store(marker.valid_until_ms, Ordering::SeqCst),
            _ => c.host.drift.store(1, Ordering::SeqCst),
        }
        let result = c.recover(&owner(), &started.session);
        assert!(result.is_err() || result.unwrap().state == SessionState::Failed);
        assert_ne!(
            c.details(&owner(), &started.session)
                .unwrap()
                .snapshot
                .state,
            SessionState::Completed
        );
        assert_eq!(c.host.reads.load(Ordering::SeqCst), 1);
    }
}
#[test]
fn actual_fence_and_observation_age_are_checked_after_pure_verification() {
    for expire in [true, false] {
        let mut c = coordinator();
        c.host.verify_expire = expire;
        if !expire {
            let rows = std::sync::Arc::clone(&c.repository.rows);
            *c.host.verify_hook.lock().unwrap() = Some(Box::new(move || {
                let mut rows = rows.lock().unwrap();
                let f = rows.session.as_mut().unwrap();
                f.fence = FenceToken::new(f.fence.value() + 1).unwrap();
            }));
        }
        let started = start_observation(&c);
        let result = continue_observation(&c, &started);
        if expire {
            assert_eq!(result.unwrap().state, SessionState::Failed);
        } else {
            assert_eq!(result, Err(SessionError::StaleRevision));
        }
        assert_ne!(
            c.details(&owner(), &started.session)
                .unwrap()
                .snapshot
                .state,
            SessionState::Completed
        );
        assert_eq!(c.host.reads.load(Ordering::SeqCst), 1);
    }
}

fn data_binding(goal: &SupportedConnectorGoal) -> PreExecutionGoalBinding {
    PreExecutionGoalBinding {
        acquisition: ResourceReservationBinding {
            owner: owner(),
            scope: goal.scope().clone(),
            session: SessionId::new("codec-session").unwrap(),
            run: RunId::new("codec-run").unwrap(),
            revision: Revision::new(2).unwrap(),
            step: goal.steps()[0].clone(),
            dispatch: DispatchId::new("codec-dispatch").unwrap(),
        },
        reservation: context_content_digest(b"codec-data-not-a-grant"),
        claim_revision: Revision::new(2).unwrap(),
        task: TaskId::new("native-task").unwrap(),
        execution: NonEmptyText::new("native-context").unwrap(),
        context_digest: context_content_digest(b"context"),
    }
}
fn request_from_read(
    read: &PersistedPreExecutionGoalRead,
) -> gateway_application::connectors::ConnectorRequest {
    use gateway_application::connectors::*;
    ConnectorRequest {
        target: ConnectorTarget {
            connector: NonEmptyText::new(read.read_request.connector.clone()).unwrap(),
            scope: ContextScopeId::new(read.read_request.scope.clone()).unwrap(),
        },
        capability: CapabilityId::new(read.read_request.capability.clone()).unwrap(),
        class: ConnectorOperationClass::Read,
        inventory_revision: ContentDigest::new(read.read_request.inventory_revision.clone())
            .unwrap(),
        arguments: BTreeMap::new(),
        task: TaskId::new(read.read_request.task.clone()).unwrap(),
        step: PlanStepId::new(read.read_request.step.clone()).unwrap(),
        execution: NonEmptyText::new(read.read_request.execution.clone()).unwrap(),
        correlation: NonEmptyText::new(read.read_request.correlation.clone()).unwrap(),
    }
}
#[test]
fn invocation_free_codec_rejects_acknowledgements_partial_unknown_quality_and_mismatched_records() {
    use gateway_application::connectors::*;
    let goal = observed::goal(ContextScopeId::new("project-a").unwrap());
    let data = observed::data(&goal, data_binding(&goal), true, 1000);
    let read = &data.reads()[0];
    let request = request_from_read(read);
    assert_eq!(
        connector_request_digest(&request).unwrap().as_str(),
        read.read_request.arguments_digest
    );
    for case in [
        "ack",
        "ack-effect",
        "unknown-effect",
        "not-dispatched",
        "post-effect-basis",
        "mutating-request",
        "administrative-request",
        "partial",
        "unknown",
        "missing-quality",
        "different-batch",
        "future-acquired",
        "expired",
        "omitted-condition",
    ] {
        let mut request = request.clone();
        let mut observation = read.observation.clone();
        let mut batch = read.batch.clone();
        let mut now = 1000;
        match case {
            "ack" => observation.basis = ConnectorObservationBasis::ProviderAcknowledgement,
            "ack-effect" => observation.metadata.effect = ConnectorEffect::Acknowledged,
            "unknown-effect" => observation.metadata.effect = ConnectorEffect::Unknown,
            "not-dispatched" => observation.metadata.effect = ConnectorEffect::NotDispatched,
            "post-effect-basis" => {
                observation.basis = ConnectorObservationBasis::IndependentPostEffectRead
            }
            "mutating-request" => request.class = ConnectorOperationClass::Mutate,
            "administrative-request" => request.class = ConnectorOperationClass::Admin,
            "partial" => observation.metadata.partial = true,
            "unknown" => {
                observation.metadata.quality = QualityMetadata::new(
                    TrustClass::ObservedEvidence,
                    SensitivityClass::Public,
                    Confidence::Unknown,
                    FreshnessStatus::Fresh,
                    Uncertainty::None,
                )
            }
            "missing-quality" => {
                batch = batch.with_quality_metadata(
                    goal.intent().desired_state().conditions()[0]
                        .subject()
                        .clone(),
                    vec![],
                )
            }
            "different-batch" => {
                batch = gateway_application::ScopedObservationBatch::new(
                    goal.scope().clone(),
                    batch.snapshot().clone(),
                    ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap(),
                )
                .unwrap()
            }
            "future-acquired" => {
                let future = observed::data(&goal, data_binding(&goal), true, 2000);
                observation = future.reads()[0].observation.clone();
                observation.metadata.observed_at = Some(UnixTimestamp::new(1));
                batch = future.reads()[0].batch.clone();
            }
            "expired" => now = 61000,
            _ => (),
        }
        let mut inputs = goal
            .intent()
            .desired_state()
            .conditions()
            .iter()
            .map(|c| PreExecutionGoalReadInput {
                condition: c.id(),
                read_request: &request,
                observation: &observation,
                batch: &batch,
                max_age_seconds: 60,
            })
            .collect::<Vec<_>>();
        if case == "omitted-condition" {
            inputs.pop();
        }
        assert!(
            PreExecutionGoalData::new(
                &goal,
                data_binding(&goal),
                observed::definitions(goal.scope()),
                &inputs,
                now
            )
            .is_err(),
            "{case}"
        );
    }
}
#[test]
fn opaque_receipt_for_a_smaller_goal_cannot_select_the_registered_full_goal() {
    let host = ObservationHost::new();
    let data = observed::data(&host.registered, data_binding(&host.registered), true, 1000);
    let original = host.registered.intent().desired_state();
    let condition = original.conditions()[0].clone();
    let smaller = DesiredState::new(
        original.id().clone(),
        vec![condition.clone()],
        ConditionExpression::condition(condition.id().clone()),
        vec![],
        vec![],
    )
    .unwrap();
    let intent = Intent::new(host.registered.intent().id().clone(), smaller);
    let receipt = ClosedLoop::verify_retained(
        data.execution().unwrap(),
        host.registered.scope().clone(),
        intent,
        data.readset(),
        host.rules(),
    )
    .unwrap()
    .unwrap();
    assert!(PreExecutionGoalCandidate::new(data, &receipt).is_err());
}
#[test]
fn journal_rejects_orphan_marker_and_preparing_rollback_without_replenishing_budget() {
    let c = coordinator();
    c.host.outage.store(1, Ordering::SeqCst);
    let start = start_observation(&c);
    assert_eq!(
        continue_observation(&c, &start),
        Err(SessionError::StorageUnavailable)
    );
    let old = c.repository.load(&owner(), &start.session).unwrap();
    let marker = old.checkpoint.pre_execution_goal.as_ref().unwrap();
    let bytes = c
        .repository
        .load_artifact(&owner(), &marker.record)
        .unwrap();
    let mut next = old.checkpoint.clone();
    next.snapshot.revision = next.snapshot.revision.next().unwrap();
    let orphan = JournalAppend {
        session: start.session.clone(),
        expected_revision: old.checkpoint.snapshot.revision,
        fence: old.fence,
        command: None,
        next,
        records: vec![RecordWrite {
            reference: marker.record.clone(),
            kind: RecordKind::PreExecutionGoal,
            bytes,
        }],
    };
    assert_eq!(
        orphan.validate_against(&old),
        Err(SessionError::InvalidState)
    );
}
#[test]
fn readonly_resource_units_are_exact_checked_and_cannot_include_model_or_workers() {
    for unit in ["calls", "rounds", "results", "tokens", "cost"] {
        let mut amounts = SessionResourceAmounts::default();
        match unit {
            "calls" => amounts.connector_calls = 1,
            "rounds" => amounts.retrieval_rounds = 1,
            "results" => amounts.retrieval_results = 1,
            "tokens" => amounts.retrieval_tokens = 1,
            _ => amounts.cost_microunits = 1,
        }
        assert!(!amounts.is_zero());
        assert!(!amounts.within(&SessionResourceAmounts::default()));
        assert_eq!(
            amounts.checked_sub(&amounts).unwrap(),
            SessionResourceAmounts::default()
        );
        let mut full = amounts;
        match unit {
            "calls" => full.connector_calls = u64::MAX,
            "rounds" => full.retrieval_rounds = u64::MAX,
            "results" => full.retrieval_results = u64::MAX,
            "tokens" => full.retrieval_tokens = u64::MAX,
            _ => full.cost_microunits = u64::MAX,
        }
        assert_eq!(full.checked_add(&amounts), Err(SessionError::LimitExceeded));
        assert_eq!(
            SessionResourceAmounts::default().checked_sub(&amounts),
            Err(SessionError::LimitExceeded)
        );
    }
    for unit in [
        "model", "input", "output", "bytes", "repair", "worker", "compute",
    ] {
        let mut a = SessionResourceAmounts::default();
        match unit {
            "model" => a.model_attempts = 1,
            "input" => a.input_tokens = 1,
            "output" => a.output_tokens = 1,
            "bytes" => a.output_bytes = 1,
            "repair" => a.repair_attempts = 1,
            "worker" => a.worker_attempts = 1,
            _ => a.worker_compute_units = 1,
        }
        assert_eq!(
            a.validate_readonly(),
            Err(SessionError::UnsupportedCapability)
        );
    }
}

#[test]
fn durable_resource_ledger_refuses_replay_cross_run_and_rewritten_history() {
    let goal = observed::goal(ContextScopeId::new("project-a").unwrap());
    let first = data_binding(&goal).acquisition;
    let amount = SessionResourceAmounts {
        connector_calls: 1,
        ..Default::default()
    };
    let limits = SessionResourceLimits {
        cost_unit: ReferenceId::new("credits").unwrap(),
        amounts: SessionResourceAmounts {
            connector_calls: 3,
            ..Default::default()
        },
    };
    let mut ledger = SessionResourceLedger::new(limits.clone()).unwrap();
    let empty = ledger.clone();
    assert_eq!(
        ledger.reserve(first.clone(), SessionResourceAmounts::default()),
        Err(SessionError::InvalidInput)
    );
    let reservation = ledger.reserve(first.clone(), amount).unwrap();
    assert_eq!(reservation.binding(), &first);
    assert_eq!(reservation.amounts(), &amount);
    let once = ledger.clone();
    assert_eq!(
        ledger.reserve(first.clone(), amount),
        Err(SessionError::Duplicate)
    );
    let mut next = first.clone();
    next.dispatch = DispatchId::new("second-dispatch").unwrap();
    assert_eq!(
        ledger.reserve(next.clone(), amount),
        Err(SessionError::StaleRevision)
    );
    next.revision = next.revision.next().unwrap();
    for key in ["owner", "scope", "session", "run"] {
        let mut foreign = next.clone();
        match key {
            "owner" => foreign.owner.project = ProjectId::new("other").unwrap(),
            "scope" => foreign.scope = ContextScopeId::new("other").unwrap(),
            "session" => foreign.session = SessionId::new("other").unwrap(),
            _ => foreign.run = RunId::new("other").unwrap(),
        }
        assert_eq!(
            ledger.reserve(foreign, amount),
            Err(SessionError::ScopeDenied)
        );
        assert_eq!(ledger, once);
    }
    let excessive = SessionResourceAmounts {
        connector_calls: 3,
        ..Default::default()
    };
    assert_eq!(
        ledger.reserve(next.clone(), excessive),
        Err(SessionError::LimitExceeded)
    );
    assert_eq!(ledger, once);
    ledger.reserve(next, amount).unwrap();
    ledger.validate_successor(&once).unwrap();
    assert_eq!(
        once.validate_successor(&ledger),
        Err(SessionError::InvalidState)
    );
    assert_eq!(
        empty.validate_successor(&ledger),
        Err(SessionError::InvalidState)
    );
    let mut replacement = SessionResourceLedger::new(limits.clone()).unwrap();
    let mut changed = first;
    changed.dispatch = DispatchId::new("replacement").unwrap();
    replacement.reserve(changed, amount).unwrap();
    assert_eq!(
        replacement.validate_successor(&once),
        Err(SessionError::InvalidState)
    );
    let mut changed_unit = limits;
    changed_unit.cost_unit = ReferenceId::new("other-unit").unwrap();
    assert_eq!(
        SessionResourceLedger::new(changed_unit)
            .unwrap()
            .validate_successor(&empty),
        Err(SessionError::InvalidState)
    );
    let wire = serde_json::to_value(&ledger).unwrap();
    assert_eq!(
        serde_json::from_value::<SessionResourceLedger>(wire.clone()).unwrap(),
        ledger
    );
    for key in ["used", "reservation", "admission", "binding", "duplicate"] {
        let mut corrupt = wire.clone();
        match key {
            "used" => corrupt["used"]["connector_calls"] = serde_json::json!(0),
            "reservation" => {
                corrupt["reservations"][0]["id"] =
                    serde_json::json!(context_content_digest(b"wrong"))
            }
            "admission" => corrupt["limits"]["cost_unit"] = serde_json::json!("other-unit"),
            "binding" => corrupt["reservations"][0]["binding"]["revision"] = serde_json::json!(0),
            _ => {
                let repeated = corrupt["reservations"][0].clone();
                corrupt["reservations"]
                    .as_array_mut()
                    .unwrap()
                    .push(repeated);
                corrupt["used"]["connector_calls"] = serde_json::json!(3);
            }
        }
        assert!(
            serde_json::from_value::<SessionResourceLedger>(corrupt).is_err(),
            "{key}"
        );
    }
}

#[test]
fn an_unconfigured_observation_port_cannot_supply_limits_reads_or_a_goal_receipt() {
    struct Unconfigured;
    impl ReadonlyGoalObservationRuntimePort for Unconfigured {}
    let c = coordinator();
    let started = start_observation(&c);
    let cp = c.details(&owner(), &started.session).unwrap();
    let goal = cp.goal.as_connector().unwrap();
    let data = observed::data(goal, data_binding(goal), true, 1000);
    assert_eq!(
        Unconfigured.resource_limits(&cp),
        Err(SessionError::UnsupportedCapability)
    );
    assert_eq!(
        Unconfigured.preparation_budget(&cp),
        Err(SessionError::UnsupportedCapability)
    );
    assert!(matches!(
        Unconfigured.verify_retained_goal(&cp, &data),
        Err(SessionError::UnsupportedCapability)
    ));
    super::goal_observation_harness::with_preparation_resources(
        goal.scope().clone(),
        ReferenceId::new("credits").unwrap(),
        SessionResourceAmounts {
            connector_calls: 1,
            ..Default::default()
        },
        |actual_cp, resources| {
            assert!(matches!(
                Unconfigured.observe_goal(actual_cp, resources),
                Err(SessionError::UnsupportedCapability)
            ));
            assert_eq!(resources.remaining().connector_calls, 1);
            Ok(())
        },
    );
}

#[test]
fn registered_connector_goals_require_complete_bounded_readonly_condition_bindings() {
    use gateway_application::connectors::ConnectorOperationClass;
    let goal = observed::goal(ContextScopeId::new("project-a").unwrap());
    let register = |steps, observations| {
        SupportedConnectorGoal::from_host_registration(
            goal.intent().clone(),
            goal.scope().clone(),
            goal.plan().clone(),
            steps,
            observations,
        )
    };
    assert_eq!(
        register(vec![], goal.observations().to_vec()),
        Err(SessionError::InvalidInput)
    );
    assert_eq!(
        register(
            (0..=MAX_CONNECTOR_GOAL_STEPS)
                .map(|i| PlanStepId::new(format!("step-{i}")).unwrap())
                .collect(),
            goal.observations().to_vec()
        ),
        Err(SessionError::LimitExceeded)
    );
    assert_eq!(
        register(
            vec![goal.steps()[0].clone(); 2],
            goal.observations().to_vec()
        ),
        Err(SessionError::InvalidInput)
    );
    assert_eq!(
        register(goal.steps().to_vec(), vec![]),
        Err(SessionError::InvalidInput)
    );
    assert_eq!(
        register(
            goal.steps().to_vec(),
            vec![goal.observations()[0].clone(); goal.observations().len()]
        ),
        Err(SessionError::InvalidInput)
    );
    let mut foreign = goal.observations().to_vec();
    foreign[0] = ConditionObservationMapping::new(
        ConditionId::new("undeclared").unwrap(),
        SourceId::new("source").unwrap(),
        CapabilityId::new("read").unwrap(),
        ConnectorOperationClass::Read,
    )
    .unwrap();
    assert_eq!(
        register(goal.steps().to_vec(), foreign),
        Err(SessionError::InvalidInput)
    );
    let mut reversed = goal.observations().to_vec();
    reversed.reverse();
    assert_eq!(register(goal.steps().to_vec(), reversed).unwrap(), goal);
    for class in [
        ConnectorOperationClass::Mutate,
        ConnectorOperationClass::Admin,
    ] {
        assert_eq!(
            ConditionObservationMapping::new(
                goal.observations()[0].condition().clone(),
                SourceId::new("source").unwrap(),
                CapabilityId::new("read").unwrap(),
                class
            ),
            Err(SessionError::UnsupportedCapability)
        );
    }
    for class in [
        ConnectorOperationClass::Read,
        ConnectorOperationClass::Search,
        ConnectorOperationClass::Inspect,
    ] {
        let binding = ConditionObservationMapping::new(
            goal.observations()[0].condition().clone(),
            SourceId::new("source").unwrap(),
            CapabilityId::new("read").unwrap(),
            class,
        )
        .unwrap();
        assert_eq!(binding.operation_class(), class);
    }
}

#[test]
fn interrupted_preparing_is_never_reinvoked_by_continue_or_recovery() {
    let mut c = coordinator();
    c.host.observe_outage = true;
    let start = start_observation(&c);
    assert_eq!(
        continue_observation(&c, &start),
        Err(SessionError::StorageUnavailable)
    );
    let cp = c.details(&owner(), &start.session).unwrap();
    assert_eq!(cp.snapshot.state, SessionState::Dispatching);
    let ledger = cp.resource_ledger.clone();
    assert_eq!(c.recover(&owner(), &start.session).unwrap(), cp.snapshot);
    assert_eq!(
        continue_observation(&c, &cp.snapshot),
        Err(SessionError::Duplicate)
    );
    c.host
        .legacy
        .now
        .store(cp.lease_until_ms.unwrap(), Ordering::SeqCst);
    assert_eq!(
        c.recover(&owner(), &start.session).unwrap().state,
        SessionState::Failed
    );
    assert_eq!(c.host.reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        c.details(&owner(), &start.session).unwrap().resource_ledger,
        ledger
    );
}

#[test]
fn readonly_continue_refuses_zero_quotes_or_changed_units_without_reserving_or_reading() {
    assert!(Host::new(false).goal_observation_runtime().is_none());
    for zero in [true, false] {
        let mut c = coordinator();
        let started = start_observation(&c);
        let before = c.details(&owner(), &started.session).unwrap();
        c.host.zero_calls = zero;
        c.host.changed_unit = !zero;
        assert_eq!(
            continue_observation(&c, &started),
            Err(if zero {
                SessionError::InvalidInput
            } else {
                SessionError::AuthorityDenied
            })
        );
        assert_eq!(c.details(&owner(), &started.session).unwrap(), before);
        assert_eq!(c.host.reads.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn storage_lookup_failure_cannot_be_treated_as_an_unused_command_id() {
    let c = coordinator();
    c.repository.lookup_failure.store(1, Ordering::SeqCst);
    assert_eq!(
        c.execute(
            &owner(),
            SessionCommand::Start {
                command: CommandId::new("lookup-failure").unwrap(),
                intent: c.host.registered.intent().clone(),
                execution: selection()
            }
        ),
        Err(SessionError::StorageUnavailable)
    );
    assert!(c.repository.rows.lock().unwrap().session.is_none());
    assert!(c.repository.rows.lock().unwrap().commands.is_empty());
}

#[test]
fn lease_expiry_at_storage_acknowledgement_stops_before_minting_or_reading() {
    let c = coordinator();
    let started = start_observation(&c);
    let now = c.host.legacy.now.clone();
    *c.repository.dispatch_append_hook.lock().unwrap() = Some(Box::new(move || {
        now.fetch_add(30_000, Ordering::SeqCst);
    }));
    let failed = continue_observation(&c, &started).unwrap();
    assert_eq!(failed.state, SessionState::Failed);
    assert_eq!(c.host.reads.load(Ordering::SeqCst), 0);
    assert_eq!(
        c.details(&owner(), &started.session)
            .unwrap()
            .resource_ledger
            .unwrap()
            .reservations()
            .len(),
        1
    );
}

#[test]
fn a_read_result_cannot_commit_after_lease_expiry_or_competing_claim() {
    for expire in [true, false] {
        let mut c = coordinator();
        c.host.observe_expire = expire;
        if !expire {
            let rows = c.repository.rows.clone();
            *c.host.observe_hook.lock().unwrap() = Some(Box::new(move || {
                let mut rows = rows.lock().unwrap();
                let current = rows.session.as_mut().unwrap();
                current.fence = FenceToken::new(current.fence.value() + 1).unwrap();
            }));
        }
        let started = start_observation(&c);
        let result = continue_observation(&c, &started);
        if expire {
            assert_eq!(result.unwrap().state, SessionState::Failed);
        } else {
            assert_eq!(result, Err(SessionError::StaleRevision));
        }
        let stored = c.details(&owner(), &started.session).unwrap();
        assert!(stored.pre_execution_goal.is_none());
        assert!(stored.snapshot.final_evidence.is_none());
        assert_eq!(stored.resource_ledger.unwrap().reservations().len(), 1);
    }
}

#[test]
fn missing_or_smaller_native_receipt_cannot_complete_a_persisted_full_goal() {
    for missing in [true, false] {
        let mut c = coordinator();
        c.host.no_receipt = missing;
        c.host.wrong_receipt = !missing;
        let started = start_observation(&c);
        assert_eq!(
            continue_observation(&c, &started).unwrap().state,
            SessionState::Failed
        );
        let stored = c.details(&owner(), &started.session).unwrap();
        assert!(stored.pre_execution_goal.is_some());
        assert!(stored.snapshot.final_evidence.is_none());
        assert_eq!(c.host.reads.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn marker_only_continue_atomically_records_its_command_for_success_and_failure() {
    for missing in [true, false] {
        let mut c = coordinator();
        c.host.outage.store(1, Ordering::SeqCst);
        let started = start_observation(&c);
        assert_eq!(
            continue_observation(&c, &started),
            Err(SessionError::StorageUnavailable)
        );
        let marker = c.details(&owner(), &started.session).unwrap();
        assert!(marker.pre_execution_goal.is_some());
        c.host.outage.store(0, Ordering::SeqCst);
        c.host.no_receipt = missing;
        let id = CommandId::new("verify-retained-marker").unwrap();
        let command = SessionCommand::Continue {
            at: Mutation {
                session: started.session.clone(),
                command: id.clone(),
                expected_revision: marker.snapshot.revision,
            },
        };
        let result = c.execute(&owner(), command.clone()).unwrap();
        assert_eq!(
            result.state,
            if missing {
                SessionState::Failed
            } else {
                SessionState::Completed
            }
        );
        let observed = c
            .inspect(&owner(), InspectTarget::Command(id.clone()))
            .unwrap();
        let outcome = observed.command_outcome.unwrap();
        assert_eq!(outcome.command, id);
        assert_eq!(outcome.revision, result.revision);
        assert_eq!(outcome.session, started.session);
        assert_eq!(c.execute(&owner(), command), Err(SessionError::Duplicate));
        let after = c.details(&owner(), &started.session).unwrap();
        assert_eq!(after.resource_ledger, marker.resource_ledger);
        assert_eq!(after.snapshot.revision, result.revision);
        assert_eq!(c.host.reads.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn checkpoint_and_journal_reject_marker_ledger_and_role_substitution() {
    let c = coordinator();
    let started = start_observation(&c);
    assert_eq!(
        continue_observation(&c, &started).unwrap().state,
        SessionState::Completed
    );
    let completed = c.details(&owner(), &started.session).unwrap();
    for case in [
        "missing-marker",
        "missing-ledger",
        "artifact-role",
        "selected-source",
        "zero-lease",
        "marker-lease",
    ] {
        let mut bad = completed.clone();
        match case {
            "missing-marker" => bad.pre_execution_goal = None,
            "missing-ledger" => bad.resource_ledger = None,
            "artifact-role" => bad.goal = goal(false).into(),
            "selected-source" => bad.selected_source = Some(reference("unselected-source")),
            "zero-lease" => bad.lease_until_ms = Some(0),
            _ => bad.lease_until_ms = Some(1000),
        }
        assert!(bad.validate().is_err(), "{case}");
    }
    let reservation = &completed.resource_ledger.as_ref().unwrap().reservations()[0];
    for field in ["owner", "scope", "session", "run", "step", "revision"] {
        let mut binding = reservation.binding().clone();
        match field {
            "owner" => binding.owner.project = ProjectId::new("other-project").unwrap(),
            "scope" => binding.scope = ContextScopeId::new("other-scope").unwrap(),
            "session" => binding.session = SessionId::new("other-session").unwrap(),
            "run" => binding.run = RunId::new("other-run").unwrap(),
            "step" => binding.step = PlanStepId::new("other-step").unwrap(),
            _ => binding.revision = completed.snapshot.revision.next().unwrap(),
        }
        let mut ledger = SessionResourceLedger::new(
            completed.resource_ledger.as_ref().unwrap().limits().clone(),
        )
        .unwrap();
        ledger.reserve(binding, *reservation.amounts()).unwrap();
        let mut bad = completed.clone();
        bad.resource_ledger = Some(ledger);
        assert_eq!(bad.validate(), Err(SessionError::InvalidState), "{field}");
    }
    let c = coordinator();
    c.host.outage.store(1, Ordering::SeqCst);
    let started = start_observation(&c);
    assert_eq!(
        continue_observation(&c, &started),
        Err(SessionError::StorageUnavailable)
    );
    let old = c.repository.load(&owner(), &started.session).unwrap();
    let mut next = old.checkpoint.clone();
    next.snapshot.revision = next.snapshot.revision.next().unwrap();
    next.pre_execution_goal = None;
    let append = JournalAppend {
        session: started.session,
        expected_revision: old.checkpoint.snapshot.revision,
        fence: old.fence,
        command: None,
        next,
        records: vec![],
    };
    assert_eq!(
        append.validate_against(&old),
        Err(SessionError::InvalidState)
    );
}

#[test]
fn absolute_deadline_stops_new_tasks_and_retained_marker_verification_without_io() {
    let c = coordinator();
    c.host.legacy.now.store(1_000_000, Ordering::SeqCst);
    assert_eq!(
        c.execute(
            &owner(),
            SessionCommand::Start {
                command: CommandId::new("expired-at-start").unwrap(),
                intent: c.host.registered.intent().clone(),
                execution: selection(),
            }
        ),
        Err(SessionError::LimitExceeded)
    );
    assert!(c.repository.rows.lock().unwrap().session.is_none());
    let c = coordinator();
    c.host.outage.store(1, Ordering::SeqCst);
    let started = start_observation(&c);
    assert_eq!(
        continue_observation(&c, &started),
        Err(SessionError::StorageUnavailable)
    );
    let marker = c.details(&owner(), &started.session).unwrap();
    let verifies = c.host.verifies.load(Ordering::SeqCst);
    c.host
        .legacy
        .now
        .store(marker.budget.deadline_ms, Ordering::SeqCst);
    assert_eq!(
        c.recover(&owner(), &started.session).unwrap().state,
        SessionState::Failed
    );
    assert_eq!(c.host.verifies.load(Ordering::SeqCst), verifies);
    assert_eq!(c.host.reads.load(Ordering::SeqCst), 1);
}

#[test]
fn restoring_resource_ledgers_rejects_empty_admission_and_classifies_overspend() {
    let limits = SessionResourceLimits {
        cost_unit: ReferenceId::new("credits").unwrap(),
        amounts: SessionResourceAmounts {
            connector_calls: 3,
            ..Default::default()
        },
    };
    let mut ledger = SessionResourceLedger::new(limits.clone()).unwrap();
    let mut empty_admission = serde_json::to_value(&ledger).unwrap();
    empty_admission["limits"]["amounts"] =
        serde_json::to_value(SessionResourceAmounts::default()).unwrap();

    let goal = observed::goal(ContextScopeId::new("project-a").unwrap());
    ledger
        .reserve(data_binding(&goal).acquisition, limits.amounts)
        .unwrap();
    let wire = serde_json::to_value(&ledger).unwrap();
    // Consuming the exact admitted allowance remains a valid durable ledger.
    assert_eq!(
        serde_json::from_value::<SessionResourceLedger>(wire.clone()).unwrap(),
        ledger
    );
    let mut overspent = wire;
    overspent["used"]["connector_calls"] = serde_json::json!(4);
    for (case, invalid) in [
        ("empty admission", empty_admission),
        ("overspent", overspent),
    ] {
        let error = serde_json::from_value::<SessionResourceLedger>(invalid).unwrap_err();
        assert!(
            error
                .to_string()
                .contains(SessionError::LimitExceeded.code()),
            "{case}: {error}"
        );
    }
}
