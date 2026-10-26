//! Shared component harness. Allocations originate only in the actual session
//! coordinator, and assertions run inside its one live observation callback.
use gateway_application::sessions::*;
use gateway_domain::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Default)]
struct Rows {
    current: Option<FencedSession>,
    commands: BTreeMap<CommandKey, CommandOutcome>,
    records: Vec<(OwnerBinding, RecordWrite)>,
}
#[derive(Default)]
struct MemoryJournal(Mutex<Rows>);
impl SessionJournalPort for MemoryJournal {
    fn load(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
    ) -> Result<FencedSession, SessionError> {
        self.0
            .lock()
            .unwrap()
            .current
            .clone()
            .filter(|c| c.checkpoint.owner == *owner && c.checkpoint.snapshot.session == *session)
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
        if rows.current.is_some() {
            return Err(SessionError::Duplicate);
        }
        let key = CommandKey {
            owner: checkpoint.owner.clone(),
            command: command.clone(),
        };
        rows.commands
            .insert(key, checkpoint.snapshot.command_outcome.clone().unwrap());
        let current = FencedSession {
            checkpoint,
            fence: FenceToken::new(1)?,
        };
        rows.current = Some(current.clone());
        Ok(current)
    }
    fn append(
        &self,
        owner: &OwnerBinding,
        change: JournalAppend,
    ) -> Result<FencedSession, SessionError> {
        let mut rows = self.0.lock().unwrap();
        let current = rows.current.as_ref().ok_or(SessionError::Unavailable)?;
        if &current.checkpoint.owner != owner {
            return Err(SessionError::ScopeDenied);
        }
        change.validate_against(current)?;
        for record in &change.records {
            if content_reference(record.reference.id().as_str(), &record.bytes)? != record.reference
            {
                return Err(SessionError::InvalidInteraction);
            }
        }
        let next = FencedSession {
            fence: FenceToken::new(current.fence.value() + 1)?,
            checkpoint: change.next,
        };
        if let Some(command) = change.command {
            rows.commands.insert(
                CommandKey {
                    owner: owner.clone(),
                    command,
                },
                next.checkpoint.snapshot.command_outcome.clone().unwrap(),
            );
        }
        rows.records
            .extend(change.records.into_iter().map(|r| (owner.clone(), r)));
        rows.current = Some(next.clone());
        Ok(next)
    }
}
impl ArtifactStoragePort for MemoryJournal {
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
            .find(|(o, r)| o == owner && &r.reference == reference)
            .map(|(_, r)| r.bytes.clone())
            .ok_or(SessionError::Unavailable)
    }
}
impl SessionRepositoryPort for MemoryJournal {
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
            .find(|(o, r)| o == owner && &r.reference == reference)
            .map(|(_, r)| r.kind)
            .ok_or(SessionError::Unavailable)
    }
}
struct CallbackHost<F> {
    owner: OwnerBinding,
    goal: SupportedConnectorGoal,
    limits: SessionResourceLimits,
    until: u64,
    callback: RefCell<Option<F>>,
    completed: Cell<bool>,
}
impl<F> InteractionAuthorityPort for CallbackHost<F> {
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
impl<F: FnOnce(&SessionCheckpoint, &mut SessionPhaseResources) -> Result<(), SessionError>>
    StructuredSessionHost for CallbackHost<F>
{
    fn now_ms(&self) -> Result<u64, SessionError> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| SessionError::Unavailable)?
            .as_millis();
        u64::try_from(millis).map_err(|_| SessionError::LimitExceeded)
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
                deadline_ms: self.until,
            },
        ))
    }
    fn prepare(&self, _: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        Err(SessionError::UnsupportedCapability)
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
    fn goal_observation_runtime(&self) -> Option<&dyn ReadonlyGoalObservationRuntimePort> {
        Some(self)
    }
}
impl<F: FnOnce(&SessionCheckpoint, &mut SessionPhaseResources) -> Result<(), SessionError>>
    ReadonlyGoalObservationRuntimePort for CallbackHost<F>
{
    fn resource_limits(
        &self,
        _: &SessionCheckpoint,
    ) -> Result<SessionResourceLimits, SessionError> {
        Ok(self.limits.clone())
    }
    fn preparation_budget(
        &self,
        _: &SessionCheckpoint,
    ) -> Result<SessionResourceAmounts, SessionError> {
        Ok(self.limits.amounts)
    }
    fn observe_goal(
        &self,
        checkpoint: &SessionCheckpoint,
        resources: &mut SessionPhaseResources,
    ) -> Result<Option<PreExecutionGoalCandidate>, SessionError> {
        assert_eq!(checkpoint.snapshot.state, SessionState::Dispatching);
        assert_eq!(resources.claim_revision(), checkpoint.snapshot.revision);
        (self
            .callback
            .borrow_mut()
            .take()
            .expect("preparation callback must not repeat"))(checkpoint, resources)?;
        self.completed.set(true);
        Ok(None)
    }
}
/// Executes the callback while the actual private-minted preparation cursor is
/// live. No allocation escapes as a return value; no goal receipt is fabricated.
pub fn with_preparation_resources(
    scope: ContextScopeId,
    cost_unit: ReferenceId,
    amounts: SessionResourceAmounts,
    callback: impl FnOnce(&SessionCheckpoint, &mut SessionPhaseResources) -> Result<(), SessionError>,
) {
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
    let goal = SupportedConnectorGoal::from_host_registration(
        Intent::new(IntentId::new("resource-task").unwrap(), desired),
        scope,
        content_reference("resource-test-plan", b"resource-test-plan-data").unwrap(),
        vec![PlanStepId::new("read-step").unwrap()],
        vec![
            ConditionObservationMapping::new(
                condition.id().clone(),
                SourceId::new("source").unwrap(),
                CapabilityId::new("read").unwrap(),
                gateway_application::connectors::ConnectorOperationClass::Read,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    with_preparation_resources_for_goal(goal, cost_unit, amounts, callback);
}
/// Same actual coordinator path with a host-registered native plan/goal. This
/// never relocates a native policy input to an unrelated step identifier.
pub fn with_preparation_resources_for_goal(
    goal: SupportedConnectorGoal,
    cost_unit: ReferenceId,
    amounts: SessionResourceAmounts,
    callback: impl FnOnce(&SessionCheckpoint, &mut SessionPhaseResources) -> Result<(), SessionError>,
) {
    let owner = OwnerBinding {
        principal: PrincipalId::new("resource-owner").unwrap(),
        workspace: WorkspaceId::new("workspace").unwrap(),
        project: ProjectId::new("project").unwrap(),
        binding: BindingId::new("binding").unwrap(),
        client_owner: ClientOwnerId::new("client").unwrap(),
    };
    let now = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let host = CallbackHost {
        owner: owner.clone(),
        goal,
        limits: SessionResourceLimits { cost_unit, amounts },
        until: now.checked_add(30_000).unwrap(),
        callback: RefCell::new(Some(callback)),
        completed: Cell::new(false),
    };
    let coordinator = SessionCoordinator::new(MemoryJournal::default(), host);
    let execution = RequestedExecution {
        mode: OperatingMode::Development,
        profile: ExecutionProfile::FullPath,
    };
    let start = coordinator
        .execute(
            &owner,
            SessionCommand::Start {
                command: CommandId::new("start-resources").unwrap(),
                intent: coordinator.host.goal.intent().clone(),
                execution,
            },
        )
        .unwrap();
    assert_eq!(
        coordinator.execute(
            &owner,
            SessionCommand::Continue {
                at: Mutation {
                    session: start.session.clone(),
                    command: CommandId::new("use-resources").unwrap(),
                    expected_revision: start.revision
                }
            }
        ),
        Err(SessionError::UnsupportedCapability)
    );
    assert!(
        coordinator.host.completed.get(),
        "resource assertions must complete inside the callback"
    );
    let final_cp = coordinator.details(&owner, &start.session).unwrap();
    assert_eq!(final_cp.snapshot.state, SessionState::Failed);
    assert_eq!(final_cp.resource_ledger.as_ref().unwrap().used(), &amounts);
}
