//! Actual PG owner/journal lifecycle with native full-goal data, never a minted receipt or IR.
use super::*;
use gateway_application::closed_loop::{ClosedLoop, LoopRules, VerifiedOutcomeReceipt};
use gateway_domain::{NonEmptyText, ReferenceId, TaskId};
use std::cell::Cell;
#[path = "../../gateway-application/tests/support/pre_execution_goal.rs"]
mod observed;

type PreparationProbe =
    dyn Fn(&SessionCheckpoint, &mut SessionPhaseResources) -> Result<(), SessionError>;
struct GoalHost {
    goal: SupportedConnectorGoal,
    rules: LoopRules,
    now: Cell<u64>,
    verifier_down: Cell<bool>,
    definitions_changed: Cell<bool>,
    acquisitions: Cell<u64>,
    probe: Option<Box<PreparationProbe>>,
}
fn amounts() -> SessionResourceAmounts {
    SessionResourceAmounts {
        connector_calls: 2,
        retrieval_rounds: 2,
        retrieval_results: 2,
        retrieval_tokens: 4096,
        ..Default::default()
    }
}
impl GoalHost {
    fn new(scope: ContextScopeId) -> Self {
        let registry = gateway_registry::Registry::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog"),
        )
        .unwrap();
        Self {
            goal: observed::goal(scope),
            rules: LoopRules {
                capabilities: gateway_application::PlanningCapabilitySnapshot::new(
                    registry.capability_index().unwrap(),
                    "pg-native-catalog",
                    gateway_domain::PlanningIrVersion::V1,
                )
                .unwrap(),
                requirements: Default::default(),
                planner: Default::default(),
                max_iterations: 2,
                max_retries: 0,
            },
            now: Cell::new(100_000),
            verifier_down: Cell::new(true),
            definitions_changed: Cell::new(false),
            acquisitions: Cell::new(0),
            probe: None,
        }
    }
}
impl InteractionAuthorityPort for GoalHost {
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
impl StructuredSessionHost for GoalHost {
    fn now_ms(&self) -> Result<u64, SessionError> {
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
        actual: &OwnerBinding,
        intent: Intent,
        _: RequestedExecution,
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
        if actual != &owner() || intent != *self.goal.intent() {
            return Err(SessionError::ScopeDenied);
        }
        Ok((
            SupportedSessionGoal::connector(self.goal.clone()),
            SessionBudget {
                actions: 0,
                retries: 0,
                max_actions: 2,
                max_retries: 0,
                deadline_ms: self
                    .now
                    .get()
                    .checked_add(900_000)
                    .ok_or(SessionError::LimitExceeded)?,
            },
        ))
    }
    fn goal_observation_runtime(&self) -> Option<&dyn ReadonlyGoalObservationRuntimePort> {
        Some(self)
    }
    fn prepare(&self, _: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        panic!("goal-before route must never compile an artifact/model")
    }
    fn disclose(&self, actual: &OwnerBinding, _: &SessionId) -> Result<(), SessionError> {
        if actual == &owner() {
            Ok(())
        } else {
            Err(SessionError::ScopeDenied)
        }
    }
    fn issuer(&self, _: &OwnerBinding, _: &PrincipalId) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}
impl ReadonlyGoalObservationRuntimePort for GoalHost {
    fn resource_limits(
        &self,
        _: &SessionCheckpoint,
    ) -> Result<SessionResourceLimits, SessionError> {
        Ok(SessionResourceLimits {
            cost_unit: ReferenceId::new("pg-read-units").unwrap(),
            amounts: amounts(),
        })
    }
    fn preparation_budget(
        &self,
        _: &SessionCheckpoint,
    ) -> Result<SessionResourceAmounts, SessionError> {
        Ok(amounts())
    }
    fn observe_goal(
        &self,
        cp: &SessionCheckpoint,
        resources: &mut SessionPhaseResources,
    ) -> Result<Option<PreExecutionGoalCandidate>, SessionError> {
        self.acquisitions.set(self.acquisitions.get() + 1);
        if let Some(probe) = &self.probe {
            probe(cp, resources)?;
            return Ok(None);
        }
        let allocation = resources.allocate(amounts())?;
        let binding = PreExecutionGoalBinding {
            acquisition: allocation.binding().clone(),
            reservation: allocation.reservation_id().clone(),
            claim_revision: cp.snapshot.revision,
            task: TaskId::new("pg-qualified-read-task").unwrap(),
            execution: NonEmptyText::new("pg-qualified-read-execution").unwrap(),
            context_digest: gateway_domain::context_content_digest(b"pg-qualified-read-context"),
        };
        let data = observed::data(&self.goal, binding, true, self.now.get());
        Ok(observed::candidate(data, &self.goal, self.rules.clone()))
    }
    fn verify_retained_goal(
        &self,
        cp: &SessionCheckpoint,
        data: &PreExecutionGoalData,
    ) -> Result<Option<VerifiedOutcomeReceipt>, SessionError> {
        if self.verifier_down.get() {
            return Err(SessionError::StorageUnavailable);
        }
        if self.definitions_changed.get()
            || data.definition_pins() != observed::definitions(cp.goal.scope())
        {
            return Err(SessionError::StaleRevision);
        }
        ClosedLoop::verify_retained(
            data.execution()?,
            cp.goal.scope().clone(),
            cp.goal.intent().clone(),
            data.readset(),
            self.rules.clone(),
        )
        .map_err(|_| SessionError::InvalidInput)
    }
}
fn observed_marker(
    repo: PostgresSessionStore,
    id: &str,
) -> (
    SessionCoordinator<PostgresSessionStore, GoalHost>,
    FencedSession,
) {
    let host = GoalHost::new(repo.store.scope().clone());
    let coordinator = SessionCoordinator::new(repo, host);
    let initial = coordinator
        .execute(
            &owner(),
            SessionCommand::Start {
                command: CommandId::new(id).unwrap(),
                intent: coordinator.host.goal.intent().clone(),
                execution: RequestedExecution {
                    mode: OperatingMode::Development,
                    profile: ExecutionProfile::FullPath,
                },
            },
        )
        .unwrap();
    assert_eq!(
        coordinator.execute(
            &owner(),
            SessionCommand::Continue {
                at: Mutation {
                    session: initial.session.clone(),
                    command: CommandId::new(format!("continue-{id}")).unwrap(),
                    expected_revision: initial.revision
                }
            }
        ),
        Err(SessionError::StorageUnavailable)
    );
    let marker = coordinator
        .repository
        .load(&owner(), &initial.session)
        .unwrap();
    assert_eq!(marker.checkpoint.snapshot.state, SessionState::Runnable);
    assert!(marker.checkpoint.pre_execution_goal.is_some());
    assert!(marker.checkpoint.artifact.is_none());
    assert_eq!(coordinator.host.acquisitions.get(), 1);
    (coordinator, marker)
}
fn reopen(scope: &ContextScopeId) -> PostgresSessionStore {
    PostgresSessionStore::new(
        CognitiveStore::connect(
            &std::env::var("CG_COGNITIVE_TEST_DATABASE").unwrap(),
            scope.clone(),
        )
        .unwrap(),
    )
}
#[test]
fn postgres_observed_goal_reopens_native_marker_and_completes_without_reacquisition() {
    let Some(repo) = repository() else { return };
    let (coordinator, marker) = observed_marker(repo, "goal-roundtrip");
    let reader = reopen(coordinator.repository.store.scope());
    let reopened = reader
        .load(&owner(), &marker.checkpoint.snapshot.session)
        .unwrap();
    assert_eq!(reopened, marker);
    let reference = reopened.checkpoint.pre_execution_goal.as_ref().unwrap();
    assert_eq!(
        reader.record_kind(&owner(), &reference.record).unwrap(),
        RecordKind::PreExecutionGoal
    );
    let data = load_pre_execution_goal(&reader, &reopened.checkpoint, 100_001).unwrap();
    assert_eq!(data.reads().len(), 2);
    assert_eq!(data.readset().digest(), &reference.readset_digest);
    assert!(
        load_pre_execution_goal(&reader, &reopened.checkpoint, reference.valid_until_ms).is_err()
    );
    coordinator.host.verifier_down.set(false);
    let recovered = SessionCoordinator::new(reader, coordinator.host);
    assert_eq!(
        recovered
            .recover(&owner(), &marker.checkpoint.snapshot.session)
            .unwrap()
            .state,
        SessionState::Completed
    );
    assert_eq!(recovered.host.acquisitions.get(), 1);
    assert!(
        recovered
            .repository
            .load(&owner(), &marker.checkpoint.snapshot.session)
            .unwrap()
            .checkpoint
            .artifact
            .is_none()
    );
}
#[test]
fn postgres_observed_goal_expiry_and_current_definition_drift_do_not_reacquire_or_complete() {
    for fault in ["expired", "definitions"] {
        let Some(repo) = repository() else { return };
        let (coordinator, marker) = observed_marker(repo, fault);
        coordinator.host.verifier_down.set(false);
        if fault == "expired" {
            coordinator.host.now.set(
                marker
                    .checkpoint
                    .pre_execution_goal
                    .as_ref()
                    .unwrap()
                    .valid_until_ms,
            )
        } else {
            coordinator.host.definitions_changed.set(true)
        }
        let result = coordinator.recover(&owner(), &marker.checkpoint.snapshot.session);
        assert!(result.is_err() || result.unwrap().state == SessionState::Failed);
        assert_ne!(
            coordinator
                .repository
                .load(&owner(), &marker.checkpoint.snapshot.session)
                .unwrap()
                .checkpoint
                .snapshot
                .state,
            SessionState::Completed
        );
        assert_eq!(coordinator.host.acquisitions.get(), 1);
    }
}
#[test]
fn postgres_observed_goal_exact_owner_kind_and_bytes_are_required_after_reopen() {
    let Some(repo) = repository() else { return };
    let (coordinator, marker) = observed_marker(repo, "goal-integrity");
    let repo = &coordinator.repository;
    let original = repo
        .run(|j| serde_json::to_value(&*j).map_err(|_| SessionError::InvalidInput))
        .unwrap();
    let reference = &marker
        .checkpoint
        .pre_execution_goal
        .as_ref()
        .unwrap()
        .record;
    let index = original["records"]
        .as_array()
        .unwrap()
        .iter()
        .position(|r| r["reference"] == serde_json::to_value(reference).unwrap())
        .unwrap();
    for fault in ["missing", "kind", "owner", "hash"] {
        let mut wire = original.clone();
        match fault {
            "missing" => {
                wire["records"].as_array_mut().unwrap().remove(index);
            }
            "kind" => {
                wire["records"][index]["kind"] = serde_json::to_value(RecordKind::Artifact).unwrap()
            }
            "owner" => {
                wire["records"][index]["owner"]["project"] = serde_json::json!("foreign-project")
            }
            _ => wire["records"][index]["bytes"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!(33)),
        }
        repo.store
            .transact::<serde_json::Value, _, SessionError>(
                "task-sessions-v2",
                &serde_json::Value::Null,
                |stored| {
                    *stored = wire;
                    Ok(())
                },
            )
            .unwrap();
        assert!(
            reopen(repo.store.scope())
                .load(&owner(), &marker.checkpoint.snapshot.session)
                .is_err(),
            "{fault}"
        );
        repo.store
            .transact::<serde_json::Value, _, SessionError>(
                "task-sessions-v2",
                &serde_json::Value::Null,
                |stored| {
                    *stored = original.clone();
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(
            repo.load(&owner(), &marker.checkpoint.snapshot.session)
                .unwrap(),
            marker
        );
    }
    let mut foreign = owner();
    foreign.project = ProjectId::new("foreign-project").unwrap();
    assert!(repo.load_artifact(&foreign, reference).is_err());
    assert!(
        repo.load(&foreign, &marker.checkpoint.snapshot.session)
            .is_err()
    );
}

#[path = "session_store_audit_tests.rs"]
mod audit_tests;
