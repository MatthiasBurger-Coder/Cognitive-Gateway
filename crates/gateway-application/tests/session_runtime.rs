//! Shared application component tests. Durable/executable acceptance is separate.
use gateway_application::{codex::CompileCommand, sessions::*};
use gateway_context::ContextDisclosurePolicy;
use gateway_domain::*;
use gateway_policy::*;
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};
#[allow(dead_code)]
#[path = "support/composition.rs"]
mod composition;
#[allow(dead_code)]
#[path = "support/context_fixture.rs"]
mod fixture;
mod support;

fn owner() -> OwnerBinding {
    OwnerBinding {
        principal: PrincipalId::new("operator").unwrap(),
        workspace: WorkspaceId::new("w").unwrap(),
        project: ProjectId::new("p").unwrap(),
        binding: BindingId::new("b").unwrap(),
        client_owner: ClientOwnerId::new("stable").unwrap(),
    }
}
fn reference(name: &str) -> RecordRef {
    content_reference(name, name.as_bytes()).unwrap()
}
fn goal(sources: bool) -> SupportedArtifactGoal {
    let f = fixture::Fixture::new();
    let condition = DesiredCondition::new(
        ConditionId::new("context-verified").unwrap(),
        SubjectPath::new(["cg", "context", "projection", "verified"]).unwrap(),
        ComparisonOperator::Equals,
        Some(TypedValue::Boolean(true)),
    )
    .unwrap();
    let desired = DesiredState::new(
        DesiredStateId::new("artifact").unwrap(),
        vec![condition],
        ConditionExpression::condition(ConditionId::new("context-verified").unwrap()),
        vec![],
        vec![],
    )
    .unwrap();
    SupportedArtifactGoal::validate(
        Intent::new(IntentId::new("artifact-task").unwrap(), desired),
        ArtifactGoalBasis {
            scope: f.resolved.report.basis.scope.clone(),
            plan: reference("plan"),
            step: f.projection.mapping.step,
            projection: reference("projection"),
            sources: if sources {
                vec![reference("source")]
            } else {
                vec![]
            },
        },
    )
    .unwrap()
}
#[derive(Default)]
struct Rows {
    session: Option<FencedSession>,
    commands: BTreeMap<CommandKey, CommandOutcome>,
    records: Vec<(OwnerBinding, RecordWrite)>,
}
#[derive(Default)]
struct Repository {
    rows: Mutex<Rows>,
    appends: AtomicUsize,
    fail_at: AtomicUsize,
    corrupt: AtomicUsize,
}
impl SessionJournalPort for Repository {
    fn load(&self, owner: &OwnerBinding, id: &SessionId) -> Result<FencedSession, SessionError> {
        self.rows
            .lock()
            .unwrap()
            .session
            .clone()
            .filter(|s| s.checkpoint.owner == *owner && s.checkpoint.snapshot.session == *id)
            .ok_or(SessionError::ScopeDenied)
    }
    fn command_outcome(&self, key: &CommandKey) -> Result<CommandOutcome, SessionError> {
        self.rows
            .lock()
            .unwrap()
            .commands
            .get(key)
            .cloned()
            .ok_or(SessionError::Unavailable)
    }
    fn create(
        &self,
        cmd: &CommandId,
        cp: SessionCheckpoint,
    ) -> Result<FencedSession, SessionError> {
        cp.validate()?;
        let mut rows = self.rows.lock().unwrap();
        let key = CommandKey {
            owner: cp.owner.clone(),
            command: cmd.clone(),
        };
        if rows.commands.contains_key(&key) {
            return Err(SessionError::Duplicate);
        }
        rows.commands
            .insert(key, cp.snapshot.command_outcome.clone().unwrap());
        let fenced = FencedSession {
            checkpoint: cp,
            fence: FenceToken::new(1)?,
        };
        rows.session = Some(fenced.clone());
        Ok(fenced)
    }
    fn append(
        &self,
        owner: &OwnerBinding,
        transition: JournalAppend,
    ) -> Result<FencedSession, SessionError> {
        let mut rows = self.rows.lock().unwrap();
        let old = rows.session.as_ref().unwrap();
        if old.checkpoint.owner != *owner {
            return Err(SessionError::ScopeDenied);
        }
        transition.validate_against(old)?;
        let n = self.appends.fetch_add(1, Ordering::SeqCst) + 1;
        if n == self.fail_at.load(Ordering::SeqCst) {
            return Err(SessionError::StorageUnavailable);
        }
        let fenced = FencedSession {
            checkpoint: transition.next,
            fence: FenceToken::new(old.fence.value() + 1)?,
        };
        if let Some(command) = transition.command {
            rows.commands.insert(
                CommandKey {
                    owner: owner.clone(),
                    command,
                },
                fenced.checkpoint.snapshot.command_outcome.clone().unwrap(),
            );
        }
        rows.records
            .extend(transition.records.into_iter().map(|r| (owner.clone(), r)));
        rows.session = Some(fenced.clone());
        Ok(fenced)
    }
}
impl ArtifactStoragePort for Repository {
    fn load_artifact(
        &self,
        owner: &OwnerBinding,
        reference: &RecordRef,
    ) -> Result<Vec<u8>, SessionError> {
        let mut bytes = self
            .rows
            .lock()
            .unwrap()
            .records
            .iter()
            .find(|(o, r)| o == owner && &r.reference == reference)
            .map(|(_, r)| r.bytes.clone())
            .ok_or(SessionError::Unavailable)?;
        if self.corrupt.load(Ordering::SeqCst) == 1 {
            bytes.push(b' ');
        }
        Ok(bytes)
    }
}
impl SessionRepositoryPort for Repository {
    fn record_kind(
        &self,
        owner: &OwnerBinding,
        reference: &RecordRef,
    ) -> Result<RecordKind, SessionError> {
        if self.corrupt.load(Ordering::SeqCst) == 2 {
            return Ok(RecordKind::Evidence);
        }
        self.rows
            .lock()
            .unwrap()
            .records
            .iter()
            .find(|(o, r)| o == owner && &r.reference == reference)
            .map(|(_, r)| r.kind)
            .ok_or(SessionError::Unavailable)
    }
}
struct Host {
    now: AtomicU64,
    prepares: AtomicUsize,
    change_at: AtomicUsize,
    change: AtomicUsize,
    sources: bool,
    grant: Mutex<Option<VerifiedConsent>>,
    revoked: AtomicUsize,
    consent: AtomicUsize,
}
impl Host {
    fn new(sources: bool) -> Self {
        Self {
            now: AtomicU64::new(100),
            prepares: AtomicUsize::new(0),
            change_at: AtomicUsize::new(0),
            change: AtomicUsize::new(0),
            sources,
            grant: Mutex::new(None),
            revoked: AtomicUsize::new(0),
            consent: AtomicUsize::new(0),
        }
    }
}
impl InteractionAuthorityPort for Host {
    fn load_consent(
        &self,
        _: &OwnerBinding,
        r: &ConsentRecordRef,
    ) -> Result<ConsentStatus, SessionError> {
        let grant = self.grant.lock().unwrap();
        if self.revoked.load(Ordering::SeqCst) == 1 {
            return Ok(ConsentStatus::Denied);
        }
        if self.revoked.load(Ordering::SeqCst) == 2 {
            return Ok(ConsentStatus::Withdrawn);
        }
        match grant.as_ref() {
            Some(g) if g.record() == r => Ok(ConsentStatus::Granted(Box::new(g.clone()))),
            _ => Err(SessionError::Unavailable),
        }
    }
    fn live_status(&self, g: &VerifiedConsent) -> Result<ConsentStatus, SessionError> {
        self.load_consent(&g.binding().owner, g.record())
    }
}
impl StructuredSessionHost for Host {
    fn now_ms(&self) -> Result<u64, SessionError> {
        Ok(self.now.load(Ordering::SeqCst))
    }
    fn disclose(&self, authenticated: &OwnerBinding, _: &SessionId) -> Result<(), SessionError> {
        if authenticated != &owner() {
            Err(SessionError::ScopeDenied)
        } else {
            Ok(())
        }
    }
    fn issuer(&self, authenticated: &OwnerBinding, _: &PrincipalId) -> Result<(), SessionError> {
        self.disclose(authenticated, &SessionId::new("authority").unwrap())
    }
    fn register(
        &self,
        authenticated: &OwnerBinding,
        intent: Intent,
        execution: RequestedExecution,
    ) -> Result<(SupportedArtifactGoal, SessionBudget), SessionError> {
        self.disclose(authenticated, &SessionId::new("start").unwrap())?;
        let goal = goal(self.sources);
        if intent != *goal.intent() || execution != selection() {
            return Err(SessionError::UnsupportedCapability);
        }
        Ok((
            goal,
            SessionBudget {
                actions: 0,
                retries: 0,
                max_actions: 2,
                max_retries: 1,
                deadline_ms: 1_000_000,
            },
        ))
    }
    fn prepare(&self, _: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        let n = self.prepares.fetch_add(1, Ordering::SeqCst) + 1;
        let changed =
            n >= self.change_at.load(Ordering::SeqCst) && self.change_at.load(Ordering::SeqCst) > 0;
        let mut f = fixture::Fixture::new();
        let change = if changed {
            self.change.load(Ordering::SeqCst)
        } else {
            0
        };
        if change == 1 {
            f.projection.mapping.step = PlanStepId::new("wrong").unwrap();
        }
        if change == 2 {
            f.policy
                .steps
                .values_mut()
                .for_each(|facts| facts.authorizations.clear());
        }
        if change == 3 {
            self.now.store(1_000_001, Ordering::SeqCst);
        }
        let required = self.consent.load(Ordering::SeqCst) == 1 || change == 4;
        let action_policy = StepPolicyReport {
            schema_version: 1,
            step: f.projection.mapping.step.clone(),
            decision: if required {
                PolicyDecision::RequireConsent
            } else {
                PolicyDecision::Allow
            },
            policies: Default::default(),
            capability_classes: Default::default(),
            findings: Default::default(),
        };
        let input = CompileCommand {
            resolved: f.resolved,
            authority: f.authority,
            policy_context: f.policy,
            catalog: f.catalog,
            projection: f.projection,
            candidates: vec![],
            selected: Default::default(),
            disclosure: ContextDisclosurePolicy {
                maximum_sensitivity: SensitivityClass::Public,
                include_caller_input: true,
                include_external_content: false,
            },
        };
        Ok(PreparedTask {
            input,
            authority: reference(if change == 5 { "changed" } else { "authority" }),
            action_policy,
        })
    }
}
fn selection() -> RequestedExecution {
    RequestedExecution {
        mode: OperatingMode::Hardening,
        profile: ExecutionProfile::FullPath,
    }
}
fn coordinator(sources: bool) -> SessionCoordinator<Repository, Host> {
    SessionCoordinator::new(Repository::default(), Host::new(sources))
}
fn start(c: &SessionCoordinator<Repository, Host>) -> SessionSnapshot {
    c.execute(
        &owner(),
        SessionCommand::Start {
            command: CommandId::new("start").unwrap(),
            intent: goal(c.host.sources).intent().clone(),
            execution: selection(),
        },
    )
    .unwrap()
}
fn mutation(snapshot: &SessionSnapshot, cmd: &str) -> Mutation {
    Mutation {
        session: snapshot.session.clone(),
        command: CommandId::new(cmd).unwrap(),
        expected_revision: snapshot.revision,
    }
}
fn continue_task(
    c: &SessionCoordinator<Repository, Host>,
    s: &SessionSnapshot,
) -> Result<SessionSnapshot, SessionError> {
    c.execute(
        &owner(),
        SessionCommand::Continue {
            at: mutation(s, "continue"),
        },
    )
}
fn approve(c: &SessionCoordinator<Repository, Host>, s: &SessionSnapshot) -> SessionSnapshot {
    let cp = c.details(&owner(), &s.session).unwrap();
    let Some(PendingInteraction::Consent(binding)) = cp.pending else {
        panic!("consent pause required")
    };
    let reference = ConsentRecordRef(reference("grant"));
    let grant =
        VerifiedConsent::from_trusted_record(reference.clone(), *binding, owner().principal);
    *c.host.grant.lock().unwrap() = Some(grant);
    c.execute(
        &owner(),
        SessionCommand::Approve {
            at: mutation(s, "approve"),
            pending: s.pending.as_ref().unwrap().id.clone(),
            consent: reference,
        },
    )
    .unwrap()
}

#[test]
fn real_coordinator_verifies_goal_and_rejects_corrupt_stored_artifacts() {
    for fault in 0..3 {
        let c = coordinator(false);
        let s = start(&c);
        c.repository.corrupt.store(fault, Ordering::SeqCst);
        let result = continue_task(&c, &s);
        if fault == 0 {
            assert_eq!(result.unwrap().state, SessionState::Completed);
        } else {
            assert_eq!(result, Err(SessionError::InvalidInput));
            assert_ne!(
                c.details(&owner(), &s.session).unwrap().snapshot.state,
                SessionState::Completed
            );
        }
    }
}
#[test]
fn failure_before_and_after_artifact_commit_is_reconciled_without_budget_reset() {
    for point in 2..=3 {
        let c = coordinator(false);
        let s = start(&c);
        c.repository.fail_at.store(point, Ordering::SeqCst);
        assert_eq!(continue_task(&c, &s), Err(SessionError::StorageUnavailable));
        let old = c.details(&owner(), &s.session).unwrap();
        assert_eq!(old.budget.actions, 1);
        assert_eq!(
            c.recover(&owner(), &s.session),
            Err(SessionError::InvalidState)
        );
        c.host.now.store(31_000, Ordering::SeqCst);
        let recovered = c.recover(&owner(), &s.session).unwrap();
        assert_eq!(
            recovered.state,
            if point == 2 {
                SessionState::Failed
            } else {
                SessionState::Completed
            }
        );
        assert_eq!(c.details(&owner(), &s.session).unwrap().budget, old.budget);
        assert_eq!(c.recover(&owner(), &s.session).unwrap(), recovered);
    }
}
#[test]
fn current_inputs_policy_authority_and_deadline_are_checked_after_reservation() {
    for (point, change, state) in [
        (3, 2, SessionState::Failed),
        (3, 4, SessionState::PendingConsent),
        (4, 4, SessionState::PendingConsent),
        (4, 5, SessionState::Failed),
        (3, 3, SessionState::Failed),
        (4, 3, SessionState::Failed),
    ] {
        let c = coordinator(false);
        let s = start(&c);
        c.host.change_at.store(point, Ordering::SeqCst);
        c.host.change.store(change, Ordering::SeqCst);
        let result = continue_task(&c, &s).unwrap();
        assert_eq!(result.state, state);
        assert_eq!(c.details(&owner(), &s.session).unwrap().budget.actions, 1);
    }
}
#[test]
fn expired_questions_are_renewed_only_by_explicit_recovery() {
    for source in [false, true] {
        let c = coordinator(source);
        c.host.consent.store(1, Ordering::SeqCst);
        let initial = start(&c);
        let pending = if source {
            initial
        } else {
            continue_task(&c, &initial).unwrap()
        };
        c.host.now.store(300_101, Ordering::SeqCst);
        assert_eq!(
            c.inspect(&owner(), InspectTarget::Session(pending.session.clone()))
                .unwrap(),
            pending
        );
        let renewed = c.recover(&owner(), &pending.session).unwrap();
        assert_eq!(renewed.state, pending.state);
        assert_ne!(
            renewed.pending.as_ref().unwrap().id,
            pending.pending.as_ref().unwrap().id
        );
        assert!(renewed.revision > pending.revision);
        c.host.now.store(1_000_001, Ordering::SeqCst);
        assert_eq!(
            c.recover(&owner(), &pending.session).unwrap().state,
            SessionState::Failed
        );
    }
}
#[test]
fn accepted_consent_is_checked_live_and_again_after_current_authority_changes() {
    for revoked in 0..=2 {
        let c = coordinator(false);
        c.host.consent.store(1, Ordering::SeqCst);
        let initial = start(&c);
        let pending = continue_task(&c, &initial).unwrap();
        let runnable = approve(&c, &pending);
        c.host.revoked.store(revoked, Ordering::SeqCst);
        let result = c
            .execute(
                &owner(),
                SessionCommand::Continue {
                    at: mutation(&runnable, "run"),
                },
            )
            .unwrap();
        assert_eq!(
            result.state,
            if revoked == 0 {
                SessionState::Completed
            } else {
                SessionState::PendingConsent
            }
        );
    }
    let c = coordinator(false);
    c.host.consent.store(1, Ordering::SeqCst);
    let s = start(&c);
    let p = continue_task(&c, &s).unwrap();
    let ready = approve(&c, &p);
    c.host.change_at.store(1, Ordering::SeqCst);
    c.host.change.store(5, Ordering::SeqCst);
    assert_eq!(
        c.execute(
            &owner(),
            SessionCommand::Continue {
                at: mutation(&ready, "changed")
            }
        )
        .unwrap()
        .state,
        SessionState::PendingConsent
    );
}

#[test]
fn v2_boundary_refuses_wrong_versions_claims_and_invalid_projections() {
    use serde_json::json;
    let request = json!({"schema_version":"2.0","scope":{"workspace_id":"w","project_id":"p","binding_id":"b"},"operation":"session.start","correlation":{"request_id":"r"},"execution":{"operating_mode":"HARDENING","execution_profile":"FULL_PATH"},"input":{"command_id":"start","intent":{"kind":"document","contract":"cg.intent","contract_version":"1.0","document":goal(false).intent()}}});
    assert!(boundary::decode("session.start", &request).is_ok());
    let mut invalid = request.clone();
    invalid["schema_version"] = json!("3.0");
    assert!(matches!(
        boundary::decode("session.start", &invalid),
        Err("CG_UNSUPPORTED_VERSION")
    ));
    assert!(matches!(
        boundary::decode("session.approve", &request),
        Err("CG_INVALID_INPUT")
    ));
    let mut invalid = request.clone();
    invalid["input"]["intent"]["document"]["unexpected"] = json!(true);
    assert!(matches!(
        boundary::decode("session.start", &invalid),
        Err("CG_INVALID_INPUT")
    ));
    let mut invalid = request.clone();
    invalid["input"]["intent"]["document"]["original_input"] = json!("password=boundary-sentinel");
    assert!(matches!(
        boundary::decode("session.start", &invalid),
        Err("CG_SENSITIVITY_DENIED")
    ));
    assert!(boundary::artifact("missing").is_none());
    assert!(boundary::artifact("catalog.schema.json").is_some());
    let c = coordinator(false);
    let s = start(&c);
    let cp = c.details(&owner(), &s.session).unwrap();
    let mut invalid = request.clone();
    invalid["scope"] = json!({});
    assert_eq!(
        boundary::response(&invalid, s, &cp)["diagnostics"][0]["code"],
        "CG_INTERNAL_ERROR"
    );
    assert_eq!(boundary::execution(&json!({})), Err("CG_INVALID_INPUT"));
    assert_eq!(
        boundary::execution(
            &json!({"execution":{"operating_mode":"DEVELOPMENT","execution_profile":"UNKNOWN"}})
        ),
        Err("CG_INVALID_INPUT")
    );
}

#[test]
fn initial_goal_assessment_is_real_retained_and_cannot_claim_observed_success() {
    let c = coordinator(false);
    let s = start(&c);
    let cp = c.details(&owner(), &s.session).unwrap();
    let assessment = cp.initial_assessment.as_ref().unwrap();
    assert_eq!(assessment.outcome, "UNKNOWN");
    assert_eq!(
        assessment.goal,
        content_reference(
            "supported-goal",
            &serde_json::to_vec(cp.goal.intent()).unwrap()
        )
        .unwrap()
    );
    for fault in 0..2 {
        let mut altered = cp.clone();
        let report = altered.initial_assessment.as_mut().unwrap();
        if fault == 0 {
            report.outcome = "SATISFIED".into();
        } else {
            report.goal = reference("foreign-goal");
        }
        assert_eq!(altered.validate(), Err(SessionError::InvalidState));
    }
    let done = continue_task(&c, &s).unwrap();
    assert_eq!(
        c.details(&owner(), &done.session)
            .unwrap()
            .initial_assessment,
        cp.initial_assessment
    );
}
