//! Genuine native ContextApplication -> private Coordinator preparation ->
//! immutable record -> authenticated reload. No MCP/model/effect is installed.
use crate::{
    complete_snapshot_fixture as snapshot, fixture, goal_observation_harness::MemoryJournal,
    prepared_before_fixture,
};
use gateway_application::{
    PlanningCapabilitySnapshot,
    closed_loop::{ClosedLoop, LoopRules, VerifiedOutcomeReceipt},
    context_application::CompiledStep,
    sessions::{scoped_snapshots::*, *},
};
use gateway_domain::*;
use serde_json::{Value, json};
use std::{
    cell::Cell,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy)]
enum Mode {
    Atomic,
    Versioned,
    Bracketed,
    WrongOwner,
    WrongFence,
    After,
    ForeignRuntime,
    LateLease,
    LateAge,
    FinalLease,
    FinalAge,
    CommitLease,
    GoalSatisfied,
    ForeignGoal,
    NoData,
    ReadOutage,
}
struct Host<'a> {
    owner: OwnerBinding,
    goal: SupportedConnectorGoal,
    compiled: &'a CompiledStep,
    now: Cell<u64>,
    calls: Cell<usize>,
    mode: Mode,
    captured: Cell<bool>,
    validation_clock_read: Cell<bool>,
    capture_clock_reads: Cell<usize>,
}
impl InteractionAuthorityPort for Host<'_> {
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
impl StructuredSessionHost for Host<'_> {
    fn now_ms(&self) -> Result<u64, SessionError> {
        if self.captured.get() {
            let reads = self.capture_clock_reads.get() + 1;
            self.capture_clock_reads.set(reads);
            match self.mode {
                Mode::FinalLease if reads >= 5 => return Ok(self.now.get() + 31_000),
                Mode::FinalAge if reads >= 5 => return Ok(self.now.get() + 2_000),
                Mode::CommitLease if reads >= 3 => return Ok(self.now.get() + 31_000),
                _ => {}
            }
        }
        if self.captured.get() && self.validation_clock_read.replace(true) {
            match self.mode {
                Mode::LateLease => return Ok(self.now.get() + 31_000),
                Mode::LateAge => return Ok(self.now.get() + 2_000),
                _ => {}
            }
        }
        Ok(self.now.get())
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
        owner: &OwnerBinding,
        intent: Intent,
        _: RequestedExecution,
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
        if owner != &self.owner || &intent != self.goal.intent() {
            return Err(SessionError::ScopeDenied);
        }
        Ok((
            SupportedSessionGoal::Connector(self.goal.clone()),
            SessionBudget {
                actions: 0,
                retries: 0,
                max_actions: 1,
                max_retries: 0,
                deadline_ms: self.now.get() + 120_000,
            },
        ))
    }
    fn goal_observation_runtime(&self) -> Option<&dyn ReadonlyGoalObservationRuntimePort> {
        Some(self)
    }
    fn prepare(&self, _: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        panic!("artifact preparation is not this path")
    }
    fn disclose(&self, owner: &OwnerBinding, _: &SessionId) -> Result<(), SessionError> {
        if owner == &self.owner {
            Ok(())
        } else {
            Err(SessionError::ScopeDenied)
        }
    }
    fn issuer(&self, _: &OwnerBinding, _: &PrincipalId) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}
fn amounts() -> SessionResourceAmounts {
    SessionResourceAmounts {
        connector_calls: 5,
        retrieval_results: 32,
        retrieval_tokens: 10000,
        ..Default::default()
    }
}
impl ReadonlyGoalObservationRuntimePort for Host<'_> {
    fn validate_before_basis(
        &self,
        _: &SessionCheckpoint,
        _: &CapturedBefore,
    ) -> Result<(), SessionError> {
        // Component fixture only; the installed runtime rechecks actual live definitions.
        Ok(())
    }

    fn resource_limits(
        &self,
        _: &SessionCheckpoint,
    ) -> Result<SessionResourceLimits, SessionError> {
        Ok(SessionResourceLimits {
            cost_unit: ReferenceId::new("LOCAL").unwrap(),
            amounts: amounts(),
        })
    }
    fn preparation_budget(
        &self,
        _: &SessionCheckpoint,
    ) -> Result<SessionResourceAmounts, SessionError> {
        Ok(amounts())
    }
    fn prepare_readonly(
        &self,
        cp: &SessionCheckpoint,
        phase: &mut SessionPhaseResources,
    ) -> Result<Option<ReadonlyPreparation>, SessionError> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(cp.snapshot.state, SessionState::Dispatching);
        let allocation = phase.allocate(amounts())?;
        if matches!(self.mode, Mode::NoData) {
            return Ok(None);
        }
        if matches!(self.mode, Mode::ReadOutage) {
            return Err(SessionError::StorageUnavailable);
        }
        if matches!(self.mode, Mode::GoalSatisfied | Mode::ForeignGoal) {
            let binding = PreExecutionGoalBinding {
                acquisition: allocation.binding().clone(),
                reservation: allocation.reservation_id().clone(),
                claim_revision: allocation.claim_revision(),
                task: self
                    .compiled
                    .context()
                    .execution_context()
                    .task()
                    .id()
                    .clone(),
                execution: NonEmptyText::new(
                    self.compiled.context().execution_context().id().as_str(),
                )
                .unwrap(),
                context_digest: context_content_digest(self.compiled.to_json().unwrap().as_bytes()),
            };
            let data = observed::data(&self.goal, binding, true, self.now.get());
            return Ok(observed::candidate(data, &self.goal, goal_rules())
                .map(|c| ReadonlyPreparation::GoalSatisfied(Box::new(c))));
        }
        let mut f = match self.mode {
            Mode::Versioned => snapshot::Fixture::versioned(),
            Mode::Bracketed => snapshot::Fixture::bracketed(),
            _ => snapshot::Fixture::atomic(),
        };
        if matches!(self.mode, Mode::LateLease | Mode::LateAge | Mode::FinalAge) {
            f.profile.max_age_seconds = if matches!(self.mode, Mode::LateAge | Mode::FinalAge) {
                1
            } else {
                3600
            };
            f.repin_profile();
        }
        let runtime = snapshot::pin(
            "native-before-runtime",
            "cg.connector-snapshot-runtime",
            &json!({"schema_version":"1.0","scope":snapshot::scope(),"native_execution":self.compiled.context().execution_context().id()}),
        );
        let mut captured = prepared_before_fixture::capture(
            &mut f,
            &allocation,
            self.compiled,
            runtime,
            self.now.get(),
        );
        match self.mode {
            Mode::WrongOwner => {
                f.binding.capture.owner.project = ProjectId::new("foreign").unwrap();
                captured.snapshot = f.validate().unwrap();
            }
            Mode::WrongFence => {
                f.binding.reservation.as_mut().unwrap().fence =
                    FenceToken::new(allocation.fence().value() + 1).unwrap();
                captured.snapshot = f.validate().unwrap();
            }
            Mode::After => {
                f.binding.phase = SnapshotPhase::After;
                captured.snapshot = f.validate().unwrap();
            }
            Mode::ForeignRuntime => {
                captured.runtime.scope = ContextScopeId::new("foreign").unwrap()
            }
            _ => {}
        }
        self.captured.set(true);
        Ok(Some(ReadonlyPreparation::BeforeCaptured(Box::new(
            captured,
        ))))
    }
    fn verify_retained_goal(
        &self,
        cp: &SessionCheckpoint,
        data: &PreExecutionGoalData,
    ) -> Result<Option<VerifiedOutcomeReceipt>, SessionError> {
        ClosedLoop::verify_retained(
            data.execution()?,
            cp.goal.scope().clone(),
            if matches!(self.mode, Mode::ForeignGoal) {
                observed::goal(cp.goal.scope().clone()).intent().clone()
            } else {
                cp.goal.intent().clone()
            },
            data.readset(),
            goal_rules(),
        )
        .map_err(|_| SessionError::InvalidInteraction)
    }
}
fn native() -> fixture::Fixture {
    let mut input = fixture::input();
    input.scope = snapshot::scope();
    input.plan_scope = snapshot::scope();
    input.situation_scope = snapshot::scope();
    fixture::Fixture::from_input(input)
}
fn coordinator<'a>(
    compiled: &'a CompiledStep,
    native: &fixture::Fixture,
    mode: Mode,
) -> SessionCoordinator<BoundaryJournal, Host<'a>> {
    let input = native.resolved.snapshot.input();
    let goal = SupportedConnectorGoal::from_host_registration(
        Intent::new(
            IntentId::new("native-before-intent").unwrap(),
            input.desired.clone(),
        ),
        snapshot::scope(),
        content_reference(
            "native-before-plan",
            input.plan.to_json().unwrap().as_bytes(),
        )
        .unwrap(),
        vec![compiled.policy().step.clone()],
        input
            .desired
            .conditions()
            .iter()
            .map(|condition| {
                ConditionObservationMapping::new(
                    condition.id().clone(),
                    SourceId::new("shared-state-provider").unwrap(),
                    CapabilityId::new("snapshot.read").unwrap(),
                    gateway_application::connectors::ConnectorOperationClass::Read,
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap();
    let owner = OwnerBinding {
        principal: PrincipalId::new("before-owner").unwrap(),
        workspace: WorkspaceId::new("workspace").unwrap(),
        project: ProjectId::new("project").unwrap(),
        binding: BindingId::new("binding").unwrap(),
        client_owner: ClientOwnerId::new("client").unwrap(),
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    SessionCoordinator::new(
        BoundaryJournal {
            inner: MemoryJournal::default(),
            fault: Cell::new(ReadFault::None),
            claim_reads: Cell::new(0),
            marker_reads: Cell::new(0),
        },
        Host {
            owner,
            goal,
            compiled,
            now: Cell::new(now),
            calls: Cell::new(0),
            mode,
            captured: Cell::new(false),
            validation_clock_read: Cell::new(false),
            capture_clock_reads: Cell::new(0),
        },
    )
}
fn begin<J: SessionRepositoryPort>(c: &SessionCoordinator<J, Host<'_>>) -> SessionSnapshot {
    c.execute(
        &c.host.owner,
        SessionCommand::Start {
            command: CommandId::new("start-before").unwrap(),
            intent: c.host.goal.intent().clone(),
            execution: RequestedExecution {
                mode: OperatingMode::Development,
                profile: ExecutionProfile::FullPath,
            },
        },
    )
    .unwrap()
}
fn acquire<J: SessionRepositoryPort>(
    c: &SessionCoordinator<J, Host<'_>>,
    start: &SessionSnapshot,
) -> Result<SessionSnapshot, SessionError> {
    c.execute(
        &c.host.owner,
        SessionCommand::Continue {
            at: Mutation {
                session: start.session.clone(),
                expected_revision: start.revision,
                command: CommandId::new("capture-before").unwrap(),
            },
        },
    )
}
fn stored<J: SessionRepositoryPort>(
    c: &SessionCoordinator<J, Host<'_>>,
    session: &SessionId,
) -> FencedSession {
    c.repository.load(&c.host.owner, session).unwrap()
}
fn records(repository: &BoundaryJournal) -> Vec<(OwnerBinding, RecordWrite)> {
    repository.inner.0.lock().unwrap().records.clone()
}
fn tamper_record(
    repository: &BoundaryJournal,
    reference: &RecordRef,
    alter: impl FnOnce(&mut RecordWrite),
) {
    let mut rows = repository.inner.0.lock().unwrap();
    alter(
        &mut rows
            .records
            .iter_mut()
            .find(|(_, r)| &r.reference == reference)
            .unwrap()
            .1,
    );
}
#[test]
fn native_before_persistence_reloads_all_profiles_as_data_and_never_completes_goal() {
    let native = native();
    let compiled = native.compile().unwrap();
    for mode in [Mode::Atomic, Mode::Versioned, Mode::Bracketed] {
        let c = coordinator(&compiled, &native, mode);
        let start = begin(&c);
        let result = acquire(&c, &start).unwrap();
        assert_eq!(result.state, SessionState::Runnable);
        assert_eq!(result.dispatch_knowledge, DispatchKnowledge::Prepared);
        assert!(result.final_evidence.is_none());
        let cp = stored(&c, &result.session).checkpoint;
        let reference = cp.prepared_before.as_ref().unwrap();
        assert_eq!(
            reference.binding.capture.revision.next().unwrap(),
            reference.committed_revision
        );
        let records = records(&c.repository);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].1.kind, RecordKind::CompleteBefore);
        let loaded = load_prepared_before_current(&c.repository, &cp, c.host.now.get()).unwrap();
        assert_eq!(loaded.digest(), &reference.snapshot_digest);
        assert_eq!(loaded.universe().objects.len(), 2);
        assert_eq!(
            decode_before_record(reference, &records[0].1.bytes, reference.validated_at_ms)
                .unwrap(),
            loaded
        );
        assert!(
            cp.artifact.is_none()
                && cp.pre_execution_goal.is_none()
                && cp.accepted_consent.is_none()
        );
        c.host.now.set(c.host.now.get() + 90_000);
        assert_eq!(
            load_prepared_before_historical(&c.repository, &cp).unwrap(),
            loaded
        );
        assert_eq!(
            load_prepared_before_current(&c.repository, &cp, c.host.now.get()),
            Err(SessionError::ExpiredInteraction)
        );
        assert_eq!(
            c.execute(
                &c.host.owner,
                SessionCommand::Continue {
                    at: Mutation {
                        session: result.session.clone(),
                        expected_revision: result.revision,
                        command: CommandId::new("no-reacquire").unwrap()
                    }
                }
            ),
            Err(SessionError::UnsupportedCapability)
        );
        assert_eq!(c.host.calls.get(), 1);
    }
}
#[test]
fn coordinator_rejects_foreign_capture_fence_after_phase_and_runtime_scope_before_record_append() {
    let native = native();
    let compiled = native.compile().unwrap();
    for mode in [
        Mode::WrongOwner,
        Mode::WrongFence,
        Mode::After,
        Mode::ForeignRuntime,
    ] {
        let c = coordinator(&compiled, &native, mode);
        let start = begin(&c);
        assert!(acquire(&c, &start).is_err());
        assert!(records(&c.repository).is_empty());
        assert!(
            stored(&c, &start.session)
                .checkpoint
                .prepared_before
                .is_none()
        );
        assert_eq!(c.host.calls.get(), 1);
    }
}
#[test]
fn authenticated_record_hash_metadata_and_closed_snapshot_payload_cannot_be_substituted() {
    let native = native();
    let compiled = native.compile().unwrap();
    let c = coordinator(&compiled, &native, Mode::Atomic);
    let result = acquire(&c, &begin(&c)).unwrap();
    let cp = stored(&c, &result.session).checkpoint;
    let reference = cp.prepared_before.as_ref().unwrap();
    let bytes = records(&c.repository)[0].1.bytes.clone();
    for path in [
        "/binding/execution",
        "/runtime/revision",
        "/universe/revision",
        "/profile/revision",
        "/native_context_digest",
        "/snapshot_digest",
    ] {
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        *value.pointer_mut(path).unwrap() = if path.ends_with("digest") {
            serde_json::to_value(context_content_digest(b"foreign")).unwrap()
        } else {
            json!("foreign")
        };
        let malformed = serde_json::to_vec(&value).unwrap();
        let mut expected = reference.clone();
        expected.record = content_reference(reference.record.id().as_str(), &malformed).unwrap();
        assert_eq!(
            decode_before_record(&expected, &malformed, expected.validated_at_ms),
            Err(SessionError::InvalidInteraction)
        );
    }
    for path in [
        "",
        "/binding",
        "/snapshot/data/acquisitions/0/observation/metadata",
    ] {
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        value
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("forged_grant".into(), json!(true));
        let malformed = serde_json::to_vec(&value).unwrap();
        let mut expected = reference.clone();
        expected.record = content_reference(reference.record.id().as_str(), &malformed).unwrap();
        assert_eq!(
            decode_before_record(&expected, &malformed, expected.validated_at_ms),
            Err(SessionError::InvalidInteraction)
        );
    }
    assert_eq!(
        decode_before_record(reference, &bytes, reference.validated_at_ms - 1),
        Err(SessionError::InvalidInteraction)
    );
    let mut duplicate = String::from_utf8(bytes.clone()).unwrap();
    duplicate = duplicate.replacen("\"contract\":", "\"contract\":null,\"contract\":", 1);
    let mut expected = reference.clone();
    expected.record =
        content_reference(reference.record.id().as_str(), duplicate.as_bytes()).unwrap();
    assert_eq!(
        decode_before_record(&expected, duplicate.as_bytes(), expected.validated_at_ms),
        Err(SessionError::InvalidInteraction)
    );
    tamper_record(&c.repository, &reference.record, |record| {
        record.bytes.push(b' ')
    });
    assert_eq!(
        load_prepared_before_historical(&c.repository, &cp),
        Err(SessionError::InvalidInteraction)
    );
}

#[test]
fn coherent_header_edits_do_not_replace_the_sealed_snapshot_digest_and_record_kind() {
    let native = native();
    let compiled = native.compile().unwrap();
    let c = coordinator(&compiled, &native, Mode::Atomic);
    let result = acquire(&c, &begin(&c)).unwrap();
    let cp = stored(&c, &result.session).checkpoint;
    let expected = cp.prepared_before.as_ref().unwrap();
    let bytes = records(&c.repository)[0].1.bytes.clone();
    let mut envelope: Value = serde_json::from_slice(&bytes).unwrap();
    let mut altered = expected.clone();
    altered.snapshot_digest = context_content_digest(b"forged complete snapshot digest");
    envelope["snapshot_digest"] = serde_json::to_value(&altered.snapshot_digest).unwrap();
    let forged = serde_json::to_vec(&envelope).unwrap();
    altered.record = content_reference(expected.record.id().as_str(), &forged).unwrap();
    assert_eq!(
        decode_before_record(&altered, &forged, altered.validated_at_ms),
        Err(SessionError::InvalidInteraction)
    );
    assert_eq!(
        decode_before_record(
            expected,
            &vec![b' '; MAX_BEFORE_RECORD_BYTES + 1],
            expected.validated_at_ms
        ),
        Err(SessionError::LimitExceeded)
    );
    tamper_record(&c.repository, &expected.record, |record| {
        record.kind = RecordKind::Evidence
    });
    assert_eq!(
        load_prepared_before_historical(&c.repository, &cp),
        Err(SessionError::InvalidInteraction)
    );
}

#[test]
fn lease_or_evidence_expiry_after_initial_validation_does_not_append_a_before_record() {
    let native = native();
    let compiled = native.compile().unwrap();
    for mode in [Mode::LateLease, Mode::LateAge] {
        let c = coordinator(&compiled, &native, mode);
        let start = begin(&c);
        let outcome = acquire(&c, &start);
        assert!(outcome.is_err() || outcome.unwrap().state == SessionState::Failed);
        let current = stored(&c, &start.session).checkpoint;
        assert!(current.prepared_before.is_none());
        assert!(records(&c.repository).is_empty());
        assert_eq!(current.budget.actions, 1);
        assert_eq!(
            current
                .resource_ledger
                .as_ref()
                .unwrap()
                .reservations()
                .len(),
            1
        );
        assert_eq!(c.host.calls.get(), 1);
    }
}

#[derive(Clone, Copy)]
enum ReadFault {
    None,
    ClaimFence,
    CommittedFence,
    RecoveryFence,
    MissingCommittedBytes,
}
struct BoundaryJournal {
    inner: MemoryJournal,
    fault: Cell<ReadFault>,
    claim_reads: Cell<usize>,
    marker_reads: Cell<usize>,
}
impl SessionJournalPort for BoundaryJournal {
    fn load(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
    ) -> Result<FencedSession, SessionError> {
        let mut current = self.inner.load(owner, session)?;
        let mut substitute = false;
        if current.checkpoint.snapshot.state == SessionState::Dispatching {
            let n = self.claim_reads.get() + 1;
            self.claim_reads.set(n);
            substitute = matches!(self.fault.get(), ReadFault::ClaimFence) && n == 2;
        }
        if current.checkpoint.prepared_before.is_some() {
            let n = self.marker_reads.get() + 1;
            self.marker_reads.set(n);
            substitute = matches!(self.fault.get(), ReadFault::CommittedFence)
                || (matches!(self.fault.get(), ReadFault::RecoveryFence) && n == 2);
        }
        if substitute {
            // A fresh storage read reports a competing claim fence; the old
            // capture may remain data but cannot be returned as current work.
            current.fence = FenceToken::new(current.fence.value() + 1)?;
        }
        Ok(current)
    }
    fn command_outcome(&self, key: &CommandKey) -> Result<CommandOutcome, SessionError> {
        self.inner.command_outcome(key)
    }
    fn create(
        &self,
        command: &CommandId,
        checkpoint: SessionCheckpoint,
    ) -> Result<FencedSession, SessionError> {
        self.inner.create(command, checkpoint)
    }
    fn append(
        &self,
        owner: &OwnerBinding,
        change: JournalAppend,
    ) -> Result<FencedSession, SessionError> {
        self.inner.append(owner, change)
    }
}
impl ArtifactStoragePort for BoundaryJournal {
    fn load_artifact(
        &self,
        owner: &OwnerBinding,
        reference: &RecordRef,
    ) -> Result<Vec<u8>, SessionError> {
        if matches!(self.fault.get(), ReadFault::MissingCommittedBytes) {
            return Err(SessionError::Unavailable);
        }
        self.inner.load_artifact(owner, reference)
    }
}
impl SessionRepositoryPort for BoundaryJournal {
    fn record_kind(
        &self,
        owner: &OwnerBinding,
        reference: &RecordRef,
    ) -> Result<RecordKind, SessionError> {
        self.inner.record_kind(owner, reference)
    }
}
#[test]
fn coordinator_fences_precommit_postcommit_and_historical_storage_callbacks() {
    let native = native();
    let compiled = native.compile().unwrap();
    for fault in [
        ReadFault::ClaimFence,
        ReadFault::CommittedFence,
        ReadFault::MissingCommittedBytes,
        ReadFault::RecoveryFence,
    ] {
        let c = coordinator(&compiled, &native, Mode::Atomic);
        c.repository
            .fault
            .set(if matches!(fault, ReadFault::RecoveryFence) {
                ReadFault::None
            } else {
                fault
            });
        let start = begin(&c);
        let result = acquire(&c, &start);
        if matches!(fault, ReadFault::RecoveryFence) {
            let captured = result.unwrap();
            c.repository.fault.set(fault);
            c.repository.marker_reads.set(0);
            assert_eq!(
                c.recover(&c.host.owner, &captured.session),
                Err(SessionError::StaleRevision)
            );
        } else {
            let expected = if matches!(fault, ReadFault::MissingCommittedBytes) {
                SessionError::Unavailable
            } else {
                SessionError::StaleRevision
            };
            assert_eq!(result, Err(expected));
        }
        let actual = c
            .repository
            .inner
            .load(&c.host.owner, &start.session)
            .unwrap();
        assert_eq!(
            actual.checkpoint.prepared_before.is_some(),
            !matches!(fault, ReadFault::ClaimFence)
        );
        assert_eq!(
            records(&c.repository).len(),
            usize::from(!matches!(fault, ReadFault::ClaimFence))
        );
        assert_eq!(actual.checkpoint.budget.actions, 1);
        assert!(actual.checkpoint.snapshot.final_evidence.is_none());
        assert_eq!(c.host.calls.get(), 1);
    }
}
#[test]
fn final_return_clock_and_evidence_expiry_do_not_erase_committed_historical_data() {
    let native = native();
    let compiled = native.compile().unwrap();
    for mode in [Mode::CommitLease, Mode::FinalLease, Mode::FinalAge] {
        let c = coordinator(&compiled, &native, mode);
        let start = begin(&c);
        let result = acquire(&c, &start);
        let current = stored(&c, &start.session);
        if matches!(mode, Mode::CommitLease) {
            assert_eq!(result.unwrap().state, SessionState::Failed);
            assert!(current.checkpoint.prepared_before.is_none());
            assert!(records(&c.repository).is_empty());
        } else {
            assert_eq!(result, Err(SessionError::ExpiredInteraction));
            assert!(current.checkpoint.prepared_before.is_some());
            assert_eq!(current.checkpoint.snapshot.state, SessionState::Runnable);
            load_prepared_before_historical(&c.repository, &current.checkpoint).unwrap();
            assert_eq!(records(&c.repository).len(), 1);
        }
        assert!(current.checkpoint.snapshot.final_evidence.is_none());
        assert_eq!(c.host.calls.get(), 1);
    }
}

#[path = "pre_execution_goal.rs"]
mod observed;
fn goal_rules() -> LoopRules {
    let f = native();
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
#[test]
fn the_same_native_preparation_owner_preserves_goal_success_none_and_transport_outage() {
    let native = native();
    let compiled = native.compile().unwrap();
    for mode in [
        Mode::GoalSatisfied,
        Mode::ForeignGoal,
        Mode::NoData,
        Mode::ReadOutage,
    ] {
        let c = coordinator(&compiled, &native, mode);
        let start = begin(&c);
        let result = acquire(&c, &start);
        let cp = stored(&c, &start.session).checkpoint;
        assert!(cp.prepared_before.is_none());
        match mode {
            Mode::GoalSatisfied => {
                assert_eq!(result.unwrap().state, SessionState::Completed);
                assert!(cp.pre_execution_goal.is_some());
                assert!(cp.snapshot.final_evidence.is_some());
            }
            Mode::ForeignGoal => {
                assert_eq!(result.unwrap().state, SessionState::Failed);
                assert!(cp.pre_execution_goal.is_some());
                assert!(cp.snapshot.final_evidence.is_none());
            }
            Mode::NoData => {
                assert_eq!(result, Err(SessionError::UnsupportedCapability));
                assert_eq!(cp.snapshot.state, SessionState::Failed);
                assert!(records(&c.repository).is_empty());
            }
            Mode::ReadOutage => {
                assert_eq!(result, Err(SessionError::StorageUnavailable));
                assert_eq!(cp.snapshot.state, SessionState::Dispatching);
                assert!(records(&c.repository).is_empty());
                c.host.now.set(cp.lease_until_ms.unwrap());
                assert_eq!(
                    c.recover(&c.host.owner, &start.session).unwrap().state,
                    SessionState::Failed
                );
            }
            _ => unreachable!(),
        }
        assert_eq!(c.host.calls.get(), 1);
        assert_eq!(cp.budget.actions, 1);
    }
}
