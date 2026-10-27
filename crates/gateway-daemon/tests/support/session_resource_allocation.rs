//! Component harness: allocations are minted only by the real coordinator's
//! persisted preparation transition. No public factory or synthetic claim token.
use gateway_application::{
    connectors::{ConnectorOperationClass, ConnectorRequest, ConnectorTarget},
    sessions::*,
};
use gateway_domain::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

#[derive(Default)]
struct Rows {
    session: Option<FencedSession>,
    commands: BTreeMap<CommandKey, CommandOutcome>,
    records: Vec<(OwnerBinding, RecordWrite)>,
}

#[derive(Clone, Default)]
struct Repository(Arc<Mutex<Rows>>);
impl SessionJournalPort for Repository {
    fn load(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
    ) -> Result<FencedSession, SessionError> {
        self.0
            .lock()
            .unwrap()
            .session
            .clone()
            .filter(|stored| {
                stored.checkpoint.owner == *owner && stored.checkpoint.snapshot.session == *session
            })
            .ok_or(SessionError::ScopeDenied)
    }
    fn command_outcome(&self, key: &CommandKey) -> Result<CommandOutcome, SessionError> {
        self.0
            .lock()
            .unwrap()
            .commands
            .get(key)
            .cloned()
            .ok_or(SessionError::Unavailable)
    }
    fn create(
        &self,
        command: &CommandId,
        checkpoint: SessionCheckpoint,
    ) -> Result<FencedSession, SessionError> {
        checkpoint.validate()?;
        let mut rows = self.0.lock().unwrap();
        let key = CommandKey {
            owner: checkpoint.owner.clone(),
            command: command.clone(),
        };
        if rows.commands.contains_key(&key) || rows.session.is_some() {
            return Err(SessionError::Duplicate);
        }
        rows.commands
            .insert(key, checkpoint.snapshot.command_outcome.clone().unwrap());
        let stored = FencedSession {
            checkpoint,
            fence: FenceToken::new(1)?,
        };
        rows.session = Some(stored.clone());
        Ok(stored)
    }
    fn append(
        &self,
        owner: &OwnerBinding,
        append: JournalAppend,
    ) -> Result<FencedSession, SessionError> {
        let mut rows = self.0.lock().unwrap();
        let old = rows.session.as_ref().ok_or(SessionError::Unavailable)?;
        if old.checkpoint.owner != *owner {
            return Err(SessionError::ScopeDenied);
        }
        append.validate_against(old)?;
        if append.command.as_ref().is_some_and(|command| {
            rows.commands.contains_key(&CommandKey {
                owner: owner.clone(),
                command: command.clone(),
            })
        }) {
            return Err(SessionError::Duplicate);
        }
        let stored = FencedSession {
            checkpoint: append.next,
            fence: FenceToken::new(old.fence.value() + 1)?,
        };
        if let Some(command) = append.command {
            rows.commands.insert(
                CommandKey {
                    owner: owner.clone(),
                    command,
                },
                stored.checkpoint.snapshot.command_outcome.clone().unwrap(),
            );
        }
        rows.records.extend(
            append
                .records
                .into_iter()
                .map(|record| (owner.clone(), record)),
        );
        rows.session = Some(stored.clone());
        Ok(stored)
    }
}
impl ArtifactStoragePort for Repository {
    fn load_artifact(
        &self,
        owner: &OwnerBinding,
        reference: &RecordRef,
    ) -> Result<Vec<u8>, SessionError> {
        self.0
            .lock()
            .unwrap()
            .records
            .iter()
            .find(|(record_owner, record)| record_owner == owner && record.reference == *reference)
            .map(|(_, record)| record.bytes.clone())
            .ok_or(SessionError::Unavailable)
    }
}
impl SessionRepositoryPort for Repository {
    fn record_kind(
        &self,
        owner: &OwnerBinding,
        reference: &RecordRef,
    ) -> Result<RecordKind, SessionError> {
        self.0
            .lock()
            .unwrap()
            .records
            .iter()
            .find(|(record_owner, record)| record_owner == owner && record.reference == *reference)
            .map(|(_, record)| record.kind)
            .ok_or(SessionError::Unavailable)
    }
}

struct ExpectedInvocation {
    original: gateway_application::prompt_ir::InvocationIr,
    claim_revision: Revision,
    fence: FenceToken,
    reservation: ContentDigest,
}

struct ExpectedPhaseClaim {
    revision: Revision,
    fence: FenceToken,
    reservation: ContentDigest,
    phase: SessionResourcePhase,
}
impl ExpectedPhaseClaim {
    fn matches(&self, allocation: &SessionResourceAllocation) -> bool {
        self.revision == allocation.claim_revision()
            && self.fence == allocation.fence()
            && &self.reservation == allocation.reservation_id()
            && self.phase == allocation.phase()
    }
}
struct ExpectedConnectorRequest {
    request: ConnectorRequest,
    source: Option<SourceId>,
    claim: ExpectedPhaseClaim,
}
struct ExpectedConnectorTarget {
    target: ConnectorTarget,
    claim: ExpectedPhaseClaim,
}

#[derive(Clone)]
pub struct PreparationAuthority {
    repository: Repository,
    owner: OwnerBinding,
    now_ms: Arc<AtomicU64>,
    revoked: Arc<AtomicBool>,
    qualified_profiles: Arc<Mutex<Vec<gateway_domain::prompt_rendering::TokenizerProfile>>>,
    expected_invocation: Arc<Mutex<Option<ExpectedInvocation>>>,
    phase: SessionResourcePhase,
    connector_requests: Arc<Mutex<Vec<ExpectedConnectorRequest>>>,
    connector_targets: Arc<Mutex<Vec<ExpectedConnectorTarget>>>,
}
impl PreparationAuthority {
    pub fn snapshot(&self) -> FencedSession {
        self.repository.0.lock().unwrap().session.clone().unwrap()
    }
    pub fn revoke(&self) {
        self.revoked.store(true, Ordering::SeqCst);
    }
    fn current_claim(&self) -> ExpectedPhaseClaim {
        let current = self.snapshot();
        let reservation = current
            .checkpoint
            .resource_ledger
            .as_ref()
            .unwrap()
            .reservations()
            .iter()
            .find(|reservation| {
                current.checkpoint.snapshot.dispatch.as_ref()
                    == Some(&reservation.binding().dispatch)
            })
            .unwrap()
            .id()
            .clone();
        ExpectedPhaseClaim {
            revision: current.checkpoint.snapshot.revision,
            fence: current.fence,
            reservation,
            phase: self.phase,
        }
    }
    /// Trusted test registration, never inferred from an incoming operation.
    pub fn allow_connector_request(&self, request: &ConnectorRequest, source: Option<&SourceId>) {
        let current = self.snapshot();
        assert_eq!(&request.target.scope, current.checkpoint.goal.scope());
        assert!(
            current
                .checkpoint
                .goal
                .as_connector()
                .unwrap()
                .steps()
                .contains(&request.step)
        );
        if let Some(before) = current
            .checkpoint
            .connector_step_snapshots
            .iter()
            .find(|snapshot| {
                snapshot.phase == ConnectorSnapshotPhase::Before
                    && current.checkpoint.snapshot.dispatch.as_ref()
                        == Some(&snapshot.binding.dispatch)
            })
        {
            assert_eq!(request.task, before.binding.task);
        }
        self.connector_requests
            .lock()
            .unwrap()
            .push(ExpectedConnectorRequest {
                request: request.clone(),
                source: source.cloned(),
                claim: self.current_claim(),
            });
    }
    pub fn allow_connector_target(&self, target: &ConnectorTarget) {
        assert_eq!(&target.scope, self.snapshot().checkpoint.goal.scope());
        self.connector_targets
            .lock()
            .unwrap()
            .push(ExpectedConnectorTarget {
                target: target.clone(),
                claim: self.current_claim(),
            });
    }
    /// Explicit trusted producer admission joined to this exact live claim.
    /// Incoming invocations never install or refresh their own expected basis.
    pub fn allow_invocation(&self, original: &gateway_application::prompt_ir::InvocationIr) {
        let current = self.snapshot();
        assert_eq!(current.checkpoint.snapshot.state, SessionState::Preparing);
        assert_eq!(
            original.prepared().invocation().data().task.scope,
            *current.checkpoint.goal.scope()
        );
        let reservation = current
            .checkpoint
            .resource_ledger
            .as_ref()
            .unwrap()
            .reservations()
            .iter()
            .find(|reservation| {
                current.checkpoint.snapshot.dispatch.as_ref()
                    == Some(&reservation.binding().dispatch)
            })
            .unwrap()
            .id()
            .clone();
        let mut admitted = self.expected_invocation.lock().unwrap();
        assert!(
            admitted.is_none(),
            "producer admission is immutable for this claim"
        );
        *admitted = Some(ExpectedInvocation {
            original: original.clone(),
            claim_revision: current.checkpoint.snapshot.revision,
            fence: current.fence,
            reservation,
        });
    }
    pub fn revoke_connector_requests(&self) {
        self.connector_requests.lock().unwrap().clear();
    }
    pub fn revoke_connector_targets(&self) {
        self.connector_targets.lock().unwrap().clear();
    }
    /// Explicit trusted qualification of one complete model/tokenizer/rendering
    /// profile for the independently checked established byte-bound method.
    pub fn allow_qualified_bounds(
        &self,
        profile: gateway_domain::prompt_rendering::TokenizerProfile,
    ) {
        self.qualified_profiles.lock().unwrap().push(profile);
    }
    pub fn revoke_qualified_bounds(&self) {
        self.qualified_profiles.lock().unwrap().clear();
    }
    pub fn expire(&self) {
        self.now_ms.store(
            self.snapshot().checkpoint.budget.deadline_ms,
            Ordering::SeqCst,
        );
    }
    pub fn advance_fence(&self) {
        let mut rows = self.repository.0.lock().unwrap();
        let stored = rows.session.as_mut().unwrap();
        stored.fence = FenceToken::new(stored.fence.value() + 1).unwrap();
    }
    pub fn cancel(&self) {
        let current = self.snapshot();
        let coordinator = SessionCoordinator::new(
            self.repository.clone(),
            ControlHost {
                owner: self.owner.clone(),
                now_ms: self.now_ms.clone(),
            },
        );
        let cancelled = coordinator
            .execute(
                &self.owner,
                SessionCommand::Cancel {
                    at: Mutation {
                        session: current.checkpoint.snapshot.session,
                        expected_revision: current.checkpoint.snapshot.revision,
                        command: CommandId::new("model-allocation-cancel").unwrap(),
                    },
                },
            )
            .unwrap();
        assert!(matches!(
            cancelled.state,
            SessionState::Cancelled | SessionState::Cancelling
        ));
    }
}
impl SessionResourceAuthorityPort for PreparationAuthority {
    fn validate_connector_allocation(
        &self,
        allocation: &SessionResourceAllocation,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<(), SessionError> {
        self.validate_allocation(allocation)?;
        if request.target.scope != allocation.binding().scope
            || request.step != allocation.binding().step
            || (matches!(
                request.class,
                ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
            ) && allocation.phase() != SessionResourcePhase::Execution)
        {
            return Err(SessionError::AuthorityDenied);
        }
        let current = self.snapshot();
        if let Some(before) = current
            .checkpoint
            .connector_step_snapshots
            .iter()
            .find(|snapshot| {
                snapshot.phase == ConnectorSnapshotPhase::Before
                    && current.checkpoint.snapshot.dispatch.as_ref()
                        == Some(&snapshot.binding.dispatch)
            })
            && request.task != before.binding.task
        {
            return Err(SessionError::AuthorityDenied);
        }
        if !self.connector_requests.lock().unwrap().iter().any(|entry| {
            entry.claim.matches(allocation)
                && entry.request == *request
                && entry.source.as_ref() == source
        }) {
            return Err(SessionError::AuthorityDenied);
        }
        Ok(())
    }
    fn validate_connector_discovery(
        &self,
        allocation: &SessionResourceAllocation,
        target: &ConnectorTarget,
    ) -> Result<(), SessionError> {
        self.validate_allocation(allocation)?;
        if target.scope != allocation.binding().scope
            || !self
                .connector_targets
                .lock()
                .unwrap()
                .iter()
                .any(|entry| entry.claim.matches(allocation) && entry.target == *target)
        {
            return Err(SessionError::AuthorityDenied);
        }
        Ok(())
    }
    fn validate_invocation_allocation(
        &self,
        allocation: &SessionResourceAllocation,
        ir: &gateway_application::prompt_ir::InvocationIr,
    ) -> Result<(), SessionError> {
        self.validate_allocation(allocation)?;
        let admitted = self.expected_invocation.lock().unwrap();
        let expected = admitted.as_ref().ok_or(SessionError::AuthorityDenied)?;
        let mut proposed = ir.prepared().invocation().data().clone();
        proposed.capability = expected
            .original
            .prepared()
            .invocation()
            .data()
            .capability
            .clone();
        if expected.claim_revision != allocation.claim_revision()
            || expected.fence != allocation.fence()
            || &expected.reservation != allocation.reservation_id()
            || proposed != *expected.original.prepared().invocation().data()
            || ir.prepared().invocation().data().task.scope != allocation.binding().scope
            || ir.identity().handoff_digest != expected.original.identity().handoff_digest
            || ir.prompt().handoff() != expected.original.prompt().handoff()
            || ir.prompt().bindings() != expected.original.prompt().bindings()
            || ir.prompt().tools() != expected.original.prompt().tools()
            || ir.prepared().provider().data().locality
                != expected.original.prepared().provider().data().locality
        {
            return Err(SessionError::AuthorityDenied);
        }
        Ok(())
    }
    fn validate_token_bound(
        &self,
        allocation: &SessionResourceAllocation,
        count: &gateway_domain::prompt_rendering::TokenCountResult,
        payload: &str,
    ) -> Result<(), SessionError> {
        use gateway_domain::prompt_rendering::CountMode;
        self.validate_allocation(allocation)?;
        let provenance = ResolvedReference {
            id: ReferenceId::new("utf8-wire-byte-upper-bound").unwrap(),
            scope: allocation.binding().scope.clone(),
            contract: ReferenceId::new("utf8-wire-byte-upper-bound").unwrap(),
            contract_version: SchemaVersion::V1,
            revision: ReferenceId::new("1").unwrap(),
            digest: context_content_digest(b"UTF8 byte fallback plus 256 chat framing tokens/v1"),
        };
        let lower_bound = u64::try_from(payload.len())
            .ok()
            .and_then(|bytes| bytes.checked_add(256))
            .ok_or(SessionError::LimitExceeded)?;
        if count.mode != CountMode::Estimated
            || !self
                .qualified_profiles
                .lock()
                .unwrap()
                .contains(&count.profile)
            || count.provenance != provenance
            || count.payload_digest != context_content_digest(payload.as_bytes())
            || count
                .usage
                .total()
                .is_none_or(|counted| counted < lower_bound)
        {
            return Err(SessionError::AuthorityDenied);
        }
        Ok(())
    }
    fn validate_allocation(
        &self,
        allocation: &SessionResourceAllocation,
    ) -> Result<(), SessionError> {
        if self.revoked.load(Ordering::SeqCst) {
            return Err(SessionError::AuthorityDenied);
        }
        if allocation.phase() != self.phase {
            return Err(SessionError::InvalidState);
        }
        let current = self.snapshot();
        let checkpoint = &current.checkpoint;
        checkpoint.validate()?;
        let now = self.now_ms.load(Ordering::SeqCst);
        if now >= checkpoint.budget.deadline_ms
            || Instant::now() >= allocation.deadline()
            || checkpoint.lease_until_ms.is_none_or(|until| now >= until)
        {
            return Err(SessionError::LimitExceeded);
        }
        if current.fence != allocation.fence()
            || checkpoint.snapshot.revision != allocation.claim_revision()
        {
            return Err(SessionError::StaleRevision);
        }
        let binding = allocation.binding();
        if checkpoint.owner != binding.owner
            || checkpoint.goal.scope() != &binding.scope
            || checkpoint.snapshot.session != binding.session
            || checkpoint.snapshot.run != binding.run
            || checkpoint.snapshot.dispatch.as_ref() != Some(&binding.dispatch)
            || checkpoint
                .goal
                .as_connector()
                .is_none_or(|goal| !goal.steps().contains(&binding.step))
        {
            return Err(SessionError::ScopeDenied);
        }
        let phase_current = match allocation.phase() {
            SessionResourcePhase::Preparation => {
                checkpoint.snapshot.state == SessionState::Preparing
                    && checkpoint.accepted_connector_consent.is_none()
            }
            SessionResourcePhase::ApprovedResume => {
                checkpoint.snapshot.state == SessionState::Preparing
                    && checkpoint.accepted_connector_consent.is_some()
            }
            SessionResourcePhase::Execution | SessionResourcePhase::AfterSnapshotRecovery => {
                checkpoint.snapshot.state == SessionState::Dispatching
            }
            SessionResourcePhase::GoalVerification => false,
        };
        if !phase_current {
            return Err(SessionError::InvalidState);
        }
        let ledger = checkpoint
            .resource_ledger
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        let reservation = ledger
            .reservations()
            .iter()
            .find(|reservation| reservation.id() == allocation.reservation_id())
            .ok_or(SessionError::AuthorityDenied)?;
        if reservation.binding() != binding
            || ledger.limits().cost_unit != *allocation.cost_unit()
            || reservation.plan().is_none_or(|plan| {
                !allocation
                    .amounts()
                    .within(plan.amounts(allocation.phase()))
            })
        {
            return Err(SessionError::AuthorityDenied);
        }
        Ok(())
    }
}

struct ControlHost {
    owner: OwnerBinding,
    now_ms: Arc<AtomicU64>,
}
impl InteractionAuthorityPort for ControlHost {
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
impl StructuredSessionHost for ControlHost {
    fn now_ms(&self) -> Result<u64, SessionError> {
        Ok(self.now_ms.load(Ordering::SeqCst))
    }
    fn register(
        &self,
        _: &OwnerBinding,
        _: Intent,
        _: RequestedExecution,
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn prepare(&self, _: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn disclose(&self, owner: &OwnerBinding, _: &SessionId) -> Result<(), SessionError> {
        if *owner == self.owner {
            Ok(())
        } else {
            Err(SessionError::ScopeDenied)
        }
    }
    fn issuer(&self, owner: &OwnerBinding, _: &PrincipalId) -> Result<(), SessionError> {
        self.disclose(owner, &SessionId::new("authority").unwrap())
    }
}

struct Runtime<F> {
    callback: RefCell<F>,
    invocations: Cell<usize>,
    plan: SessionResourcePlan,
    authority: PreparationAuthority,
    prepared: Option<PreparedConnectorTurn>,
}
impl<F: FnMut(&mut SessionPhaseResources, &SessionCheckpoint, &PreparationAuthority)>
    ConnectorSessionRuntimePort for Runtime<F>
{
    fn resource_plan(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
    ) -> Result<SessionResourcePlan, SessionError> {
        Ok(self.plan.clone())
    }
    fn initial_assessment(
        &self,
        checkpoint: &SessionCheckpoint,
    ) -> Result<InitialAssessment, SessionError> {
        Ok(InitialAssessment {
            goal: content_reference(
                "supported-goal",
                &serde_json::to_vec(checkpoint.goal.intent()).unwrap(),
            )?,
            authority: checkpoint.goal.as_connector().unwrap().plan().clone(),
            outcome: "UNKNOWN".into(),
        })
    }
    fn prepare_step(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
    ) -> Result<PreparedConnectorTurn, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn prepare_authorized_step(
        &self,
        checkpoint: &SessionCheckpoint,
        _: &PlanStepId,
        resources: &mut SessionPhaseResources,
        _: ConnectorPreparationContext<'_>,
    ) -> Result<ConnectorPreparation, SessionError> {
        assert_eq!(checkpoint.snapshot.state, SessionState::Preparing);
        assert_eq!(checkpoint, &self.authority.snapshot().checkpoint);
        assert_eq!(resources.phase(), SessionResourcePhase::Preparation);
        if let Some(prepared) = &self.prepared {
            return Ok(ConnectorPreparation::Ready(Box::new(prepared.clone())));
        }
        self.invocations.set(self.invocations.get() + 1);
        (self.callback.borrow_mut())(resources, checkpoint, &self.authority);
        // This harness qualifies the preparation boundary; no connector effect
        // follows its callback and no fabricated VerifiedOutput is returned.
        Err(SessionError::UnsupportedCapability)
    }
    fn execute_step(
        &self,
        checkpoint: &SessionCheckpoint,
        _: &PlanStepId,
        resources: &mut SessionPhaseResources,
        _: &DispatchId,
        _: &PreparedConnectorTurn,
    ) -> Result<ConnectorSessionTurn, SessionError> {
        if self.authority.phase == SessionResourcePhase::Execution {
            assert_eq!(checkpoint.snapshot.state, SessionState::Dispatching);
            assert_eq!(checkpoint, &self.authority.snapshot().checkpoint);
            assert_eq!(resources.phase(), SessionResourcePhase::Execution);
            self.invocations.set(self.invocations.get() + 1);
            (self.callback.borrow_mut())(resources, checkpoint, &self.authority);
        }
        Err(SessionError::UnsupportedCapability)
    }
    fn capture_after_snapshot(
        &self,
        checkpoint: &SessionCheckpoint,
        _: &PlanStepId,
        resources: &mut SessionPhaseResources,
        _: &DispatchId,
        _: &PreparedConnectorTurn,
    ) -> Result<ScopedConnectorSnapshotPayload, SessionError> {
        if self.authority.phase == SessionResourcePhase::AfterSnapshotRecovery {
            assert_eq!(checkpoint.snapshot.state, SessionState::Dispatching);
            assert_eq!(checkpoint, &self.authority.snapshot().checkpoint);
            assert_eq!(
                resources.phase(),
                SessionResourcePhase::AfterSnapshotRecovery
            );
            self.invocations.set(self.invocations.get() + 1);
            (self.callback.borrow_mut())(resources, checkpoint, &self.authority);
        }
        Err(SessionError::Unavailable)
    }
    fn verify_goal(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
        _: &DispatchId,
        _: &[PersistedConnectorReadback],
    ) -> Result<Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>, SessionError>
    {
        Err(SessionError::UnsupportedCapability)
    }
}

struct Host<F> {
    control: ControlHost,
    goal: SupportedConnectorGoal,
    runtime: Runtime<F>,
}
impl<F> InteractionAuthorityPort for Host<F> {
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
impl<F: FnMut(&mut SessionPhaseResources, &SessionCheckpoint, &PreparationAuthority)>
    StructuredSessionHost for Host<F>
{
    fn now_ms(&self) -> Result<u64, SessionError> {
        self.control.now_ms()
    }
    fn register(
        &self,
        owner: &OwnerBinding,
        intent: Intent,
        execution: RequestedExecution,
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
        self.control
            .disclose(owner, &SessionId::new("register").unwrap())?;
        if intent != *self.goal.intent() || execution != execution_selection() {
            return Err(SessionError::AuthorityDenied);
        }
        Ok((
            SupportedSessionGoal::connector(self.goal.clone()),
            SessionBudget {
                actions: 0,
                retries: 0,
                max_actions: 1,
                max_retries: 8,
                deadline_ms: 1_000_000,
            },
        ))
    }
    fn prepare(&self, _: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn resource_limits(
        &self,
        _: &OwnerBinding,
        _: &SupportedSessionGoal,
    ) -> Result<Option<SessionResourceLimits>, SessionError> {
        Ok(Some(SessionResourceLimits {
            cost_unit: ReferenceId::new("test-credit").unwrap(),
            amounts: self.runtime.plan.total()?,
        }))
    }
    fn connector_runtime(&self) -> Result<&dyn ConnectorSessionRuntimePort, SessionError> {
        Ok(&self.runtime)
    }
    fn disclose(&self, owner: &OwnerBinding, session: &SessionId) -> Result<(), SessionError> {
        self.control.disclose(owner, session)
    }
    fn issuer(&self, owner: &OwnerBinding, issuer: &PrincipalId) -> Result<(), SessionError> {
        self.control.issuer(owner, issuer)
    }
}

fn execution_selection() -> RequestedExecution {
    RequestedExecution {
        mode: OperatingMode::Development,
        profile: ExecutionProfile::FullPath,
    }
}

pub fn with_preparation_resources<
    F: FnMut(&mut SessionPhaseResources, &SessionCheckpoint, &PreparationAuthority),
>(
    scope: ContextScopeId,
    plan: SessionResourcePlan,
    callback: F,
) -> SessionCheckpoint {
    with_phase_resources(
        scope,
        plan,
        None,
        SessionResourcePhase::Preparation,
        callback,
    )
}

pub fn with_execution_resources<
    F: FnMut(&mut SessionPhaseResources, &SessionCheckpoint, &PreparationAuthority),
>(
    scope: ContextScopeId,
    plan: SessionResourcePlan,
    prepared: PreparedConnectorTurn,
    callback: F,
) -> SessionCheckpoint {
    with_phase_resources(
        scope,
        plan,
        Some(prepared),
        SessionResourcePhase::Execution,
        callback,
    )
}

pub fn with_recovery_resources<
    F: FnMut(&mut SessionPhaseResources, &SessionCheckpoint, &PreparationAuthority),
>(
    scope: ContextScopeId,
    plan: SessionResourcePlan,
    prepared: PreparedConnectorTurn,
    callback: F,
) -> SessionCheckpoint {
    with_phase_resources(
        scope,
        plan,
        Some(prepared),
        SessionResourcePhase::AfterSnapshotRecovery,
        callback,
    )
}

fn with_phase_resources<
    F: FnMut(&mut SessionPhaseResources, &SessionCheckpoint, &PreparationAuthority),
>(
    scope: ContextScopeId,
    plan: SessionResourcePlan,
    prepared: Option<PreparedConnectorTurn>,
    phase: SessionResourcePhase,
    callback: F,
) -> SessionCheckpoint {
    let condition = DesiredCondition::new(
        ConditionId::new("model-proposal-observed").unwrap(),
        SubjectPath::new(["model", "proposal", "observed"]).unwrap(),
        ComparisonOperator::Equals,
        Some(TypedValue::Boolean(true)),
    )
    .unwrap();
    let intent = Intent::new(
        IntentId::new("model-allocation-task").unwrap(),
        DesiredState::new(
            DesiredStateId::new("model-allocation-goal").unwrap(),
            vec![condition],
            ConditionExpression::condition(ConditionId::new("model-proposal-observed").unwrap()),
            vec![],
            vec![],
        )
        .unwrap(),
    );
    let goal = SupportedConnectorGoal::from_host_registration(
        intent,
        scope,
        content_reference("model-allocation-plan", b"trusted-model-allocation-plan").unwrap(),
        vec![PlanStepId::new("model-allocation-step").unwrap()],
        vec![
            ConditionObservationMapping::new(
                ConditionId::new("model-proposal-observed").unwrap(),
                SourceId::new("model-observation").unwrap(),
                CapabilityId::new("model-observation-read").unwrap(),
                ConnectorOperationClass::Read,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let owner = OwnerBinding {
        principal: PrincipalId::new("model-operator").unwrap(),
        workspace: WorkspaceId::new("model-workspace").unwrap(),
        project: ProjectId::new("model-project").unwrap(),
        binding: BindingId::new("model-binding").unwrap(),
        client_owner: ClientOwnerId::new("model-client").unwrap(),
    };
    let repository = Repository::default();
    let now_ms = Arc::new(AtomicU64::new(100));
    let authority = PreparationAuthority {
        repository: repository.clone(),
        owner: owner.clone(),
        now_ms: now_ms.clone(),
        revoked: Arc::new(AtomicBool::new(false)),
        qualified_profiles: Arc::new(Mutex::new(Vec::new())),
        expected_invocation: Arc::new(Mutex::new(None)),
        phase,
        connector_requests: Arc::new(Mutex::new(Vec::new())),
        connector_targets: Arc::new(Mutex::new(Vec::new())),
    };
    let host = Host {
        control: ControlHost {
            owner: owner.clone(),
            now_ms,
        },
        goal,
        runtime: Runtime {
            callback: RefCell::new(callback),
            invocations: Cell::new(0),
            plan,
            authority,
            prepared,
        },
    };
    let intent = host.goal.intent().clone();
    let coordinator = SessionCoordinator::new(repository, host);
    let started = coordinator
        .execute(
            &owner,
            SessionCommand::Start {
                command: CommandId::new("model-allocation-start").unwrap(),
                intent,
                execution: execution_selection(),
            },
        )
        .unwrap();
    let result = coordinator.execute(
        &owner,
        SessionCommand::Continue {
            at: Mutation {
                session: started.session.clone(),
                expected_revision: started.revision,
                command: CommandId::new("model-allocation-continue").unwrap(),
            },
        },
    );
    if phase == SessionResourcePhase::Preparation {
        assert!(result.is_err());
    } else if let Ok(snapshot) = result {
        assert_eq!(snapshot.state, SessionState::OutcomeUnknown);
    }
    assert_eq!(coordinator.host.runtime.invocations.get(), 1);
    coordinator.details(&owner, &started.session).unwrap()
}
