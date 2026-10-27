//! Shared application component tests. Durable/executable acceptance is separate.
use gateway_application::{
    codex::CompileCommand, connector_proposals::ProposalError, connectors::ConnectorEffect,
    sessions::*,
};
use gateway_context::ContextDisclosurePolicy;
use gateway_domain::*;
use gateway_policy::*;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};
#[allow(dead_code)]
#[path = "support/composition.rs"]
mod composition;
#[path = "connector_session_turn_validation.rs"]
mod connector_turn_fixtures;
#[allow(dead_code)]
#[path = "support/context_fixture.rs"]
mod fixture;
#[path = "support/pre_execution_goal.rs"]
mod pre_execution_goal_fixture;
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
#[derive(Default, Clone)]
struct Repository {
    rows: Arc<Mutex<Rows>>,
    appends: Arc<AtomicUsize>,
    fail_at: Arc<AtomicUsize>,
    corrupt: Arc<AtomicUsize>,
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
    now: Arc<AtomicU64>,
    prepares: AtomicUsize,
    change_at: AtomicUsize,
    change: AtomicUsize,
    sources: bool,
    grant: Mutex<Option<VerifiedConsent>>,
    revoked: AtomicUsize,
    consent: AtomicUsize,
    connector_registered: bool,
    resource_admission: bool,
    resource_step_limit: u64,
    connector_goal: Option<SupportedConnectorGoal>,
    connector_runtime: ConnectorStartRuntime,
    connector_grant: Mutex<Option<VerifiedConnectorConsent>>,
    connector_revoked: AtomicUsize,
    connector_live_revoked: Arc<AtomicUsize>,
    connector_live_advance_ms: AtomicU64,
}
impl Host {
    fn new(sources: bool) -> Self {
        Self {
            now: Arc::new(AtomicU64::new(100)),
            prepares: AtomicUsize::new(0),
            change_at: AtomicUsize::new(0),
            change: AtomicUsize::new(0),
            sources,
            grant: Mutex::new(None),
            revoked: AtomicUsize::new(0),
            consent: AtomicUsize::new(0),
            connector_registered: false,
            resource_admission: true,
            resource_step_limit: 8,
            connector_goal: None,
            connector_runtime: ConnectorStartRuntime::default(),
            connector_grant: Mutex::new(None),
            connector_revoked: AtomicUsize::new(0),
            connector_live_revoked: Arc::new(AtomicUsize::new(0)),
            connector_live_advance_ms: AtomicU64::new(0),
        }
    }

    fn connector(sources: bool) -> Self {
        let mut host = Self::new(sources);
        host.connector_registered = true;
        host
    }

    fn connector_session(
        goal: SupportedConnectorGoal,
        prepared: gateway_application::sessions::PreparedConnectorTurn,
        dispatches: Arc<AtomicUsize>,
        uncertain_on_second: bool,
        capture_after_on_error: bool,
    ) -> Self {
        let mut host = Self::new(false);
        host.connector_registered = true;
        host.connector_goal = Some(goal);
        host.connector_runtime = ConnectorStartRuntime {
            preparation_calls: Arc::new(AtomicUsize::new(0)),
            resume_calls: Arc::new(AtomicUsize::new(0)),
            resume_entries: Arc::new(AtomicUsize::new(0)),
            revalidation_hook: Mutex::new(None),
            pre_goal_mode: 0,
            pre_goal_reads: Arc::new(AtomicUsize::new(0)),
            pre_goal_verifications: Arc::new(AtomicUsize::new(0)),
            pre_goal_clock: None,
            pre_goal_verify_hook: Mutex::new(None),
            pre_goal_verify_error: Arc::new(AtomicUsize::new(0)),
            pre_goal_definition_drift: Arc::new(AtomicUsize::new(0)),
            preparation_clock: None,
            advance_preparation_ms: 0,
            plan_override: None,
            prepared: Some(prepared),
            dispatches,
            uncertain_on_second,
            capture_after_on_error,
            unknown_without_after_on_second: false,
            invalid_turn_on_first: false,
            not_dispatched_on_first: false,
            consent_required_first: false,
            resume_seen: Arc::new(Mutex::new(None)),
            revalidations: Arc::new(AtomicUsize::new(0)),
            duplicate_resume_record: false,
            producer_input_mode: 0,
            tamper_resume_record: false,
            wrong_connector_consent_issuer: false,
            issue_connector_receipt: false,
            wrong_connector_receipt_execution: false,
            wrong_smaller_connector_goal: false,
            omit_connector_goal_member: false,
            redundant_connector_goal_batches: false,
        };
        host
    }
}

#[derive(Default)]
struct ConnectorStartRuntime {
    preparation_calls: Arc<AtomicUsize>,
    resume_calls: Arc<AtomicUsize>,
    resume_entries: Arc<AtomicUsize>,
    revalidation_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    pre_goal_mode: usize,
    pre_goal_reads: Arc<AtomicUsize>,
    pre_goal_verifications: Arc<AtomicUsize>,
    pre_goal_clock: Option<Arc<AtomicU64>>,
    pre_goal_verify_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    pre_goal_verify_error: Arc<AtomicUsize>,
    pre_goal_definition_drift: Arc<AtomicUsize>,
    preparation_clock: Option<Arc<AtomicU64>>,
    advance_preparation_ms: u64,
    plan_override: Option<SessionResourcePlan>,
    prepared: Option<gateway_application::sessions::PreparedConnectorTurn>,
    dispatches: Arc<AtomicUsize>,
    uncertain_on_second: bool,
    capture_after_on_error: bool,
    unknown_without_after_on_second: bool,
    invalid_turn_on_first: bool,
    not_dispatched_on_first: bool,
    consent_required_first: bool,
    resume_seen: Arc<Mutex<Option<ConnectorConsentResumeMaterial>>>,
    revalidations: Arc<AtomicUsize>,
    duplicate_resume_record: bool,
    producer_input_mode: usize,
    tamper_resume_record: bool,
    wrong_connector_consent_issuer: bool,
    issue_connector_receipt: bool,
    wrong_connector_receipt_execution: bool,
    wrong_smaller_connector_goal: bool,
    omit_connector_goal_member: bool,
    redundant_connector_goal_batches: bool,
}

struct DefaultsOnlyConnectorRuntime;
impl ConnectorSessionRuntimePort for DefaultsOnlyConnectorRuntime {
    fn initial_assessment(&self, _: &SessionCheckpoint) -> Result<InitialAssessment, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn prepare_step(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
    ) -> Result<PreparedConnectorTurn, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn execute_step(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
        _resources: &mut SessionPhaseResources,
        _: &DispatchId,
        _: &PreparedConnectorTurn,
    ) -> Result<ConnectorSessionTurn, SessionError> {
        Err(SessionError::UnsupportedCapability)
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

impl ConnectorSessionRuntimePort for ConnectorStartRuntime {
    fn resource_plan(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
    ) -> Result<SessionResourcePlan, SessionError> {
        let mut plan = self
            .plan_override
            .clone()
            .unwrap_or_else(connector_resource_plan);
        if self.pre_goal_mode != 0 {
            plan.preparation.connector_calls = 1;
            plan.preparation.retrieval_results = 2;
        }
        Ok(plan)
    }

    fn initial_assessment(
        &self,
        checkpoint: &SessionCheckpoint,
    ) -> Result<InitialAssessment, SessionError> {
        Ok(InitialAssessment {
            goal: content_reference(
                "supported-goal",
                &serde_json::to_vec(checkpoint.goal.intent())
                    .map_err(|_| SessionError::InvalidInput)?,
            )?,
            authority: reference("connector-authority"),
            outcome: "UNKNOWN".into(),
        })
    }

    fn prepare_step(
        &self,
        checkpoint: &SessionCheckpoint,
        step: &PlanStepId,
    ) -> Result<PreparedConnectorTurn, SessionError> {
        let Some(prepared) = &self.prepared else {
            return Err(SessionError::UnsupportedCapability);
        };
        let scope = checkpoint.goal.scope().clone();
        PreparedConnectorTurn::new(
            prepared.invocation().clone(),
            prepared.verified_output().clone(),
            ScopedConnectorSnapshotPayload::new(
                scope,
                serde_json::to_vec(&serde_json::json!({"step":step.as_str(),"phase":"before"}))
                    .map_err(|_| SessionError::InvalidInput)?,
            )?,
        )
    }

    fn prepare_authorized_step(
        &self,
        checkpoint: &SessionCheckpoint,
        step: &PlanStepId,
        resources: &mut SessionPhaseResources,
        context: ConnectorPreparationContext<'_>,
    ) -> Result<ConnectorPreparation, SessionError> {
        if self.pre_goal_mode != 0 {
            let source = resources.allocate(SessionResourceAmounts {
                connector_calls: 1,
                cost_microunits: 1,
                retrieval_rounds: 1,
                retrieval_results: 2,
                retrieval_tokens: 25,
                ..Default::default()
            })?;
            self.pre_goal_reads.fetch_add(1, Ordering::SeqCst);
            let source_revision = source.claim_revision();
            let binding = PreExecutionGoalBinding {
                acquisition: source.binding().clone(),
                reservation: source.reservation_id().clone(),
                claim_revision: source_revision,
                task: TaskId::new("native-read-task").unwrap(),
                execution: NonEmptyText::new("native-read-context").unwrap(),
                context_digest: context_content_digest(b"native-inspect-context"),
            };
            let goal = checkpoint.goal.as_connector().unwrap();
            let data = pre_execution_goal_fixture::data(
                goal,
                binding,
                self.pre_goal_mode == 1,
                self.pre_goal_clock.as_ref().unwrap().load(Ordering::SeqCst),
            );
            if let Some(candidate) =
                pre_execution_goal_fixture::candidate(data, goal, native_pre_goal_rules())
            {
                return Ok(ConnectorPreparation::GoalAlreadySatisfied(Box::new(
                    candidate,
                )));
            }
            // Unsatisfied observed data stays in this same real preparing claim.
            let model = resources.allocate(SessionResourceAmounts {
                model_attempts: 1,
                input_tokens: 100,
                output_tokens: 50,
                output_bytes: 1024,
                cost_microunits: 6,
                ..Default::default()
            })?;
            assert_eq!(model.claim_revision(), source_revision);
            self.preparation_calls.fetch_add(1, Ordering::SeqCst);
            return self
                .prepare_step(checkpoint, step)
                .map(|turn| ConnectorPreparation::Ready(Box::new(turn)));
        }
        let ConnectorPreparationContext {
            revision,
            pending,
            dispatch,
            expires_at_ms,
        } = context;
        assert_eq!(resources.phase(), SessionResourcePhase::Preparation);
        resources.allocate(connector_resource_plan().preparation)?;
        self.preparation_calls.fetch_add(1, Ordering::SeqCst);
        // The runtime observes only already-journaled resource admission.
        assert_eq!(checkpoint.snapshot.state, SessionState::Preparing);
        assert!(
            checkpoint
                .resource_ledger
                .as_ref()
                .unwrap()
                .reservations()
                .iter()
                .any(|r| r.binding().dispatch == *dispatch && r.binding().step == *step)
        );
        if let Some(clock) = &self.preparation_clock {
            clock.fetch_add(self.advance_preparation_ms, Ordering::SeqCst);
        }
        if self.consent_required_first {
            let prepared = self.prepare_step(checkpoint, step)?;
            let invocation = prepared.invocation();
            let capability = invocation
                .prompt()
                .tools()
                .values()
                .next()
                .ok_or(SessionError::InvalidInteraction)?
                .binding
                .clone();
            let raw_output = serde_json::to_vec(&serde_json::json!({
                "schema_version": "1.0",
                "invocation": invocation.identity(),
                "result": "entity",
                "references": [],
                "tools": [{"capability": capability, "arguments": {"value": "proposal-0"}}]
            }))
            .map_err(|_| SessionError::InvalidInput)?;
            let producer_name = format!(
                "resume-producer-pin-{}-{}",
                checkpoint.snapshot.run.as_str(),
                revision.value()
            );
            let producer_bytes = producer_name.as_bytes().to_vec();
            let producer_pin = content_reference(&producer_name, &producer_bytes)?;
            let material = ConnectorConsentResumeMaterial::new(
                invocation.identity().clone(),
                0,
                raw_output,
                vec![producer_pin.clone()],
            )?;
            let bytes = serde_json::to_vec(&material).map_err(|_| SessionError::InvalidInput)?;
            let output_record = content_reference("connector-resume-material", &bytes)?;
            let resume = ConnectorConsentResumeBinding {
                invocation: material.invocation.clone(),
                proposal_ordinal: 0,
                output_record: output_record.clone(),
                output_digest: material.output_digest.clone(),
                producer_pins: material.producer_pins.clone(),
            };
            let goal = checkpoint
                .goal
                .as_connector()
                .ok_or(SessionError::InvalidState)?;
            let turn = connector_turn_fixtures::connector_turn_for_step(goal, &prepared, step);
            let request = &turn
                .proposal_batch
                .results
                .first()
                .ok_or(SessionError::InvalidInteraction)?
                .request;
            let mut binding = ConnectorConsentBinding::new(
                checkpoint.owner.clone(),
                goal.scope().clone(),
                checkpoint.snapshot.session.clone(),
                checkpoint.snapshot.run.clone(),
                revision,
                pending.clone(),
                step.clone(),
                dispatch.clone(),
                request,
                resume,
                PolicyId::new("connector-policy").map_err(|_| SessionError::InvalidInput)?,
                ContentDigest::new("e".repeat(64)).map_err(|_| SessionError::InvalidInput)?,
                expires_at_ms,
                checkpoint.owner.principal.clone(),
            )?;
            if self.wrong_connector_consent_issuer {
                binding.issuer =
                    PrincipalId::new("untrusted-issuer").map_err(|_| SessionError::InvalidInput)?;
            }
            // Default ports remain fail-closed even when called with a real
            // coordinator-minted capability (they cannot acquire authority).
            let fallback = DefaultsOnlyConnectorRuntime;
            assert!(matches!(
                fallback.resource_plan(checkpoint, step),
                Err(SessionError::UnsupportedCapability)
            ));
            assert!(matches!(
                fallback.resource_envelope(checkpoint, step),
                Err(SessionError::UnsupportedCapability)
            ));
            assert!(matches!(
                fallback.prepare_authorized_step(
                    checkpoint,
                    step,
                    resources,
                    ConnectorPreparationContext {
                        revision,
                        pending,
                        dispatch,
                        expires_at_ms
                    }
                ),
                Err(SessionError::UnsupportedCapability)
            ));
            assert!(matches!(
                fallback.resume_approved_step(checkpoint, step, resources, &binding, &material),
                Err(SessionError::UnsupportedCapability)
            ));
            assert!(matches!(
                fallback.capture_after_snapshot(checkpoint, step, resources, dispatch, &prepared),
                Err(SessionError::Unavailable)
            ));
            *self.resume_seen.lock().unwrap() = Some(material);
            let mut record_bytes = bytes;
            if self.tamper_resume_record {
                record_bytes.push(b' ');
            }
            let record = RecordWrite {
                reference: output_record.clone(),
                kind: RecordKind::ConnectorProposal,
                bytes: record_bytes,
            };
            let producer_record = RecordWrite {
                reference: producer_pin,
                kind: RecordKind::ConnectorProducerInput,
                bytes: producer_bytes,
            };
            let mut records = vec![record.clone(), producer_record.clone()];
            match self.producer_input_mode {
                1 => {
                    records.pop();
                }
                2 => records.push(producer_record.clone()),
                3 => records.push(RecordWrite {
                    reference: reference("orphan-producer-input"),
                    kind: RecordKind::ConnectorProducerInput,
                    bytes: b"orphan-producer-input".to_vec(),
                }),
                4 => records.last_mut().unwrap().kind = RecordKind::Artifact,
                5 => records.last_mut().unwrap().bytes.push(b' '),
                _ => {}
            }
            if self.duplicate_resume_record {
                records.push(record);
            }
            return Ok(ConnectorPreparation::ConsentRequired {
                binding: Box::new(binding),
                records,
            });
        }
        Ok(ConnectorPreparation::Ready(Box::new(
            self.prepare_step(checkpoint, step)?,
        )))
    }

    fn resume_approved_step(
        &self,
        checkpoint: &SessionCheckpoint,
        step: &PlanStepId,
        resources: &mut SessionPhaseResources,
        binding: &ConnectorConsentBinding,
        material: &ConnectorConsentResumeMaterial,
    ) -> Result<PreparedConnectorTurn, SessionError> {
        self.resume_entries.fetch_add(1, Ordering::SeqCst);
        assert_eq!(checkpoint.snapshot.state, SessionState::Preparing);
        assert_eq!(resources.phase(), SessionResourcePhase::ApprovedResume);
        assert!(matches!(
            resources.allocate(SessionResourceAmounts {
                model_attempts: 1,
                ..Default::default()
            }),
            Err(SessionError::LimitExceeded)
        ));
        assert!(matches!(
            resources.allocate(connector_resource_envelope()),
            Err(SessionError::LimitExceeded)
        ));
        resources.allocate(connector_resource_plan().approved_resume)?;
        self.resume_calls.fetch_add(1, Ordering::SeqCst);
        material.matches(&binding.resume)?;
        *self.resume_seen.lock().unwrap() = Some(material.clone());
        self.prepare_step(checkpoint, step)
    }

    fn verify_pre_execution_goal(
        &self,
        checkpoint: &SessionCheckpoint,
        data: &PreExecutionGoalData,
    ) -> Result<Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>, SessionError>
    {
        self.pre_goal_verifications.fetch_add(1, Ordering::SeqCst);
        if self.pre_goal_definition_drift.load(Ordering::SeqCst) != 0
            || data
                .definition_pins()
                .iter()
                .map(|pin| serde_json::to_vec(pin).unwrap())
                .collect::<std::collections::BTreeSet<_>>()
                != pre_execution_goal_fixture::definitions(checkpoint.goal.scope())
                    .iter()
                    .map(|pin| serde_json::to_vec(pin).unwrap())
                    .collect::<std::collections::BTreeSet<_>>()
        {
            return Err(SessionError::AuthorityDenied);
        }
        if data.reads().iter().any(|r| r.max_age_seconds != 60) {
            return Err(SessionError::AuthorityDenied);
        }
        if self.pre_goal_verify_error.load(Ordering::SeqCst) != 0 {
            return Err(SessionError::StorageUnavailable);
        }
        if let Some(hook) = self.pre_goal_verify_hook.lock().unwrap().as_ref() {
            hook()
        }
        let goal = checkpoint.goal.as_connector().unwrap();
        gateway_application::closed_loop::ClosedLoop::verify_retained(
            data.execution()?,
            goal.scope().clone(),
            goal.intent().clone(),
            data.readset(),
            native_pre_goal_rules(),
        )
        .map_err(|_| SessionError::InvalidInteraction)
    }

    fn revalidate_consent_binding(
        &self,
        _checkpoint: &SessionCheckpoint,
        binding: &ConnectorConsentBinding,
        material: &ConnectorConsentResumeMaterial,
    ) -> Result<(), SessionError> {
        material.matches(&binding.resume)?;
        self.revalidations.fetch_add(1, Ordering::SeqCst);
        if let Some(hook) = self.revalidation_hook.lock().unwrap().as_ref() {
            hook();
        }
        Ok(())
    }

    fn execute_step(
        &self,
        checkpoint: &SessionCheckpoint,
        step: &PlanStepId,
        resources: &mut SessionPhaseResources,
        _: &DispatchId,
        prepared: &PreparedConnectorTurn,
    ) -> Result<ConnectorSessionTurn, SessionError> {
        assert_eq!(resources.phase(), SessionResourcePhase::Execution);
        resources.allocate(connector_resource_plan().execution)?;
        let call = self.dispatches.fetch_add(1, Ordering::SeqCst) + 1;
        if self.uncertain_on_second && call == 2 {
            return Err(SessionError::OutcomeUnknown);
        }
        let goal = checkpoint
            .goal
            .as_connector()
            .ok_or(SessionError::InvalidState)?;
        let mut turn = connector_turn_fixtures::connector_turn_for_step(goal, prepared, step);
        if self.unknown_without_after_on_second && call == 2 {
            turn.after_snapshot = None;
            turn.independent_readbacks.clear();
            for result in &mut turn.proposal_batch.results {
                result.observation.metadata.effect = ConnectorEffect::Unknown;
            }
            return Ok(turn);
        }
        if goal.steps().last() == Some(step) {
            turn = final_goal_readbacks(goal, step, turn, self.redundant_connector_goal_batches)?;
        }
        if self.invalid_turn_on_first && call == 1 {
            turn.after_snapshot = Some(ScopedConnectorSnapshotPayload::new(
                ContextScopeId::new("foreign-project").unwrap(),
                br#"{"phase":"after"}"#.to_vec(),
            )?);
        }
        if self.not_dispatched_on_first && call == 1 {
            turn.proposal_batch.all_calls_returned = false;
            turn.proposal_batch.results.clear();
            turn.proposal_batch.failure = Some((0, ProposalError::PolicyDenied));
            turn.after_snapshot = None;
            turn.independent_readbacks.clear();
            turn.goal_verification_readbacks.clear();
        }
        Ok(turn)
    }

    fn capture_after_snapshot(
        &self,
        checkpoint: &SessionCheckpoint,
        step: &PlanStepId,
        resources: &mut SessionPhaseResources,
        _: &DispatchId,
        _: &PreparedConnectorTurn,
    ) -> Result<ScopedConnectorSnapshotPayload, SessionError> {
        assert_eq!(
            resources.phase(),
            SessionResourcePhase::AfterSnapshotRecovery
        );
        resources.allocate(connector_resource_plan().after_snapshot_recovery)?;
        if !self.capture_after_on_error {
            return Err(SessionError::Unavailable);
        }
        ScopedConnectorSnapshotPayload::new(
            checkpoint.goal.scope().clone(),
            serde_json::to_vec(&serde_json::json!({"step":step.as_str(),"phase":"after"}))
                .map_err(|_| SessionError::InvalidInput)?,
        )
    }

    fn verify_goal(
        &self,
        checkpoint: &SessionCheckpoint,
        step: &PlanStepId,
        dispatch: &DispatchId,
        readbacks: &[PersistedConnectorReadback],
    ) -> Result<Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>, SessionError>
    {
        if self.issue_connector_receipt || self.wrong_connector_receipt_execution {
            Ok(Some(connector_goal_receipt(
                checkpoint,
                step,
                dispatch,
                readbacks,
                self.wrong_connector_receipt_execution,
                self.wrong_smaller_connector_goal,
                self.omit_connector_goal_member,
            )?))
        } else {
            Ok(None)
        }
    }
}

fn final_goal_readbacks(
    goal: &SupportedConnectorGoal,
    step: &PlanStepId,
    turn: ConnectorSessionTurn,
    redundant_batches: bool,
) -> Result<ConnectorSessionTurn, SessionError> {
    use gateway_application::{ScopedObservationBatch, SourceSnapshot};
    let mut turn = connector_turn_fixtures::add_goal_verification_readback(goal, step, turn)?;
    if goal.intent().desired_state().conditions().len() == 1 {
        return Ok(turn);
    }
    let template = turn.goal_verification_readbacks[0].clone();
    let mut groups: BTreeMap<(SourceId, CapabilityId), Vec<(usize, DesiredCondition)>> =
        BTreeMap::new();
    for (ordinal, condition) in goal
        .intent()
        .desired_state()
        .conditions()
        .iter()
        .enumerate()
    {
        let mapping = goal
            .observations()
            .iter()
            .find(|m| m.condition() == condition.id())
            .ok_or(SessionError::InvalidInput)?;
        groups
            .entry((mapping.source().clone(), mapping.capability().clone()))
            .or_default()
            .push((ordinal, condition.clone()));
    }
    turn.goal_verification_readbacks.clear();
    for (group, ((source, capability), conditions)) in groups.into_iter().enumerate() {
        let provenance = Provenance::new(
            ProvenanceId::new(format!("goal-provenance-{group}")).unwrap(),
            SourceKind::Tool,
            source.clone(),
            format!("connector://goal/{source}"),
        )
        .unwrap()
        .with_producer(template.observation.metadata.authenticated_source.as_str())
        .unwrap()
        .with_acquired_at(
            SourceTimestamp::new(template.observation.acquired_at.seconds().to_string()).unwrap(),
        )
        .with_source_timestamp(SourceTimestamp::new("goal-revision-1").unwrap());
        let mut observations = Vec::new();
        let mut facts = Vec::new();
        let mut evidence = Vec::new();
        let observed_conditions = if redundant_batches {
            goal.intent()
                .desired_state()
                .conditions()
                .iter()
                .cloned()
                .enumerate()
                .collect::<Vec<_>>()
        } else {
            conditions.clone()
        };
        for (ordinal, condition) in &observed_conditions {
            let observation = Observation::new(
                ObservationId::new(format!("goal-observation-{group}-{ordinal}")).unwrap(),
                condition.subject().clone(),
                TypedValue::Boolean(true),
                provenance.id().clone(),
            )
            .unwrap();
            let fact = Fact::new(
                FactId::new(format!("goal-fact-{group}-{ordinal}")).unwrap(),
                condition.subject().clone(),
                TypedValue::Boolean(true),
                AssertionPolarity::Affirmed,
                vec![observation.id().clone()],
            )
            .unwrap();
            evidence.push(
                Evidence::new(
                    EvidenceId::new(format!("goal-evidence-{group}-{ordinal}")).unwrap(),
                    EvidenceKind::Report,
                    "Independent goal source",
                    EvidenceContent::inline("The registered condition is true.").unwrap(),
                    provenance.id().clone(),
                    vec![EvidenceLink::new(
                        fact.id().clone(),
                        EvidenceRelation::Supports,
                    )],
                )
                .unwrap(),
            );
            observations.push(observation);
            facts.push(fact);
        }
        let records =
            ObservationEvidenceSet::new(vec![provenance], observations, facts, evidence).unwrap();
        let digest = context_content_digest(records.to_json().unwrap().as_bytes());
        let quality = QualityMetadata::new(
            TrustClass::ObservedEvidence,
            SensitivityClass::Public,
            Confidence::score(0.95).unwrap(),
            FreshnessStatus::Fresh,
            Uncertainty::None,
        );
        let mut batch = ScopedObservationBatch::new(
            goal.scope().clone(),
            SourceSnapshot::new(
                source.clone(),
                SourceKind::Tool,
                Some(SourceTimestamp::new("goal-revision-1").unwrap()),
                Some(digest.clone()),
            )
            .unwrap(),
            records.clone(),
        )
        .unwrap();
        for (_, condition) in &observed_conditions {
            batch = batch.with_quality_metadata(condition.subject().clone(), vec![quality]);
        }
        let mut read_request = template.read_request.clone();
        read_request.capability = capability;
        read_request.execution = NonEmptyText::new(format!("goal-read-execution-{group}")).unwrap();
        read_request.correlation =
            NonEmptyText::new(format!("goal-read-correlation-{group}")).unwrap();
        let mut observation = template.observation.clone();
        observation.source = source.clone();
        observation.source_revision = Some(SourceTimestamp::new("goal-revision-1").unwrap());
        observation.content_digest = digest.clone();
        observation.records = records;
        observation.correlation = read_request.correlation.clone();
        observation.metadata.execution = read_request.execution.clone();
        observation.metadata.object_reference =
            NonEmptyText::new(format!("connector://goal/{source}")).unwrap();
        observation.metadata.source_digest = Some(digest);
        observation.metadata.quality = quality;
        for (_, condition) in conditions {
            turn.goal_verification_readbacks
                .push(GoalVerificationReadback {
                    condition: condition.id().clone(),
                    read_request: read_request.clone(),
                    observation: observation.clone(),
                    batch: batch.clone(),
                });
        }
    }
    Ok(turn)
}

fn connector_goal_receipt(
    checkpoint: &SessionCheckpoint,
    step: &PlanStepId,
    dispatch: &DispatchId,
    readbacks: &[PersistedConnectorReadback],
    wrong_execution: bool,
    smaller_goal: bool,
    omit_member: bool,
) -> Result<gateway_application::closed_loop::VerifiedOutcomeReceipt, SessionError> {
    use gateway_application::{
        PlanningCapabilitySnapshot,
        closed_loop::{ClosedLoop, LoopRules},
    };
    let goal = checkpoint
        .goal
        .as_connector()
        .ok_or(SessionError::InvalidState)?;
    let readset = if omit_member {
        use gateway_application::retained_observations::{
            RetainedObservationMember, RetainedObservationReadset,
        };
        let readback = readbacks.first().ok_or(SessionError::InvalidInteraction)?;
        let reference = checkpoint
            .connector_observations
            .iter()
            .find(|r| {
                r.role == ConnectorObservationRole::GoalVerificationReadback
                    && r.goal_condition == readback.goal_condition
                    && r.binding == readback.binding
            })
            .ok_or(SessionError::InvalidInteraction)?;
        let binding = context_content_digest(
            &serde_json::to_vec(&("cg.session-goal-verification-member/2.0", reference)).unwrap(),
        );
        let version_domain = context_content_digest(
            &serde_json::to_vec(&(
                "cg.connector-source-object/1.0",
                goal.scope(),
                readback.observation.target.connector.as_str(),
                readback.observation.source.as_str(),
                readback.observation.metadata.source_kind.as_str(),
                readback.observation.metadata.object_reference.as_str(),
            ))
            .unwrap(),
        );
        RetainedObservationReadset::new(
            goal.scope().clone(),
            vec![RetainedObservationMember {
                binding,
                version_domain,
                batch: readback.batch.clone(),
            }],
        )
        .map_err(|_| SessionError::InvalidInteraction)?
    } else {
        build_goal_verification_readset(checkpoint, step, readbacks)?
    };
    let input = fixture::input();
    let rules = LoopRules {
        capabilities: PlanningCapabilitySnapshot::new(
            input.index.clone(),
            "connector-goal-runtime",
            PlanningIrVersion::V1,
        )
        .map_err(|_| SessionError::InvalidInput)?,
        requirements: CapabilityRequirementRules::default()
            .with_observation(
                CapabilityId::new("architecture.dependency-analysis")
                    .map_err(|_| SessionError::InvalidInput)?,
            )
            .with_evidence_acquisition(
                CapabilityId::new("architecture.dependency-analysis")
                    .map_err(|_| SessionError::InvalidInput)?,
            ),
        planner: PlannerRules::default(),
        max_iterations: 2,
        max_retries: 1,
    };
    let execution = if wrong_execution {
        format!("{}-different-run-execution-1", dispatch.as_str())
    } else {
        format!("{}-execution-1", dispatch.as_str())
    };
    let intent = if smaller_goal {
        let desired = goal.intent().desired_state();
        let condition = desired.conditions()[0].clone();
        Intent::new(
            goal.intent().id().clone(),
            DesiredState::new(
                desired.id().clone(),
                vec![condition.clone()],
                ConditionExpression::condition(condition.id().clone()),
                vec![],
                vec![],
            )
            .map_err(|_| SessionError::InvalidInput)?,
        )
    } else {
        goal.intent().clone()
    };
    ClosedLoop::verify_retained(
        ReferenceId::new(execution).map_err(|_| SessionError::InvalidInput)?,
        goal.scope().clone(),
        intent,
        &readset,
        rules,
    )
    .map_err(|_| SessionError::InvalidInteraction)?
    .ok_or(SessionError::InvalidInteraction)
}

fn two_condition_connector_goal(
    shared_source: bool,
) -> (SupportedConnectorGoal, PreparedConnectorTurn) {
    let (original, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
    let first = original.intent().desired_state().conditions()[0].clone();
    let second = DesiredCondition::new(
        ConditionId::new("secondary-ready").unwrap(),
        SubjectPath::new(["connector", "secondary", "ready"]).unwrap(),
        ComparisonOperator::Equals,
        Some(TypedValue::Boolean(true)),
    )
    .unwrap();
    let desired = DesiredState::new(
        original.intent().desired_state().id().clone(),
        vec![first.clone(), second.clone()],
        ConditionExpression::all(vec![
            ConditionExpression::condition(first.id().clone()),
            ConditionExpression::condition(second.id().clone()),
        ])
        .unwrap(),
        vec![],
        vec![],
    )
    .unwrap();
    let mapping = &original.observations()[0];
    let goal = SupportedConnectorGoal::from_host_registration(
        Intent::new(original.intent().id().clone(), desired),
        original.scope().clone(),
        original.plan().clone(),
        original.steps().to_vec(),
        vec![
            mapping.clone(),
            ConditionObservationMapping::new(
                second.id().clone(),
                if shared_source {
                    mapping.source().clone()
                } else {
                    SourceId::new("connector-second-state").unwrap()
                },
                mapping.capability().clone(),
                gateway_application::connectors::ConnectorOperationClass::Read,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    (goal, prepared)
}

fn connector_goal_from_artifact(goal: &SupportedArtifactGoal) -> SupportedConnectorGoal {
    SupportedConnectorGoal::from_host_registration(
        goal.intent().clone(),
        goal.basis().scope.clone(),
        goal.basis().plan.clone(),
        vec![goal.basis().step.clone()],
        goal.intent()
            .desired_state()
            .conditions()
            .iter()
            .map(|condition| {
                ConditionObservationMapping::new(
                    condition.id().clone(),
                    SourceId::new("connector-state-source").unwrap(),
                    CapabilityId::new("connector.state.read").unwrap(),
                    gateway_application::connectors::ConnectorOperationClass::Read,
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap()
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
    fn load_connector_consent(
        &self,
        _: &OwnerBinding,
        r: &ConnectorConsentRecordRef,
    ) -> Result<ConnectorConsentStatus, SessionError> {
        match self.connector_revoked.load(Ordering::SeqCst) {
            1 => return Ok(ConnectorConsentStatus::Denied),
            2 => return Ok(ConnectorConsentStatus::Withdrawn),
            _ => {}
        }
        match self.connector_grant.lock().unwrap().as_ref() {
            Some(grant) if grant.record() == r => {
                Ok(ConnectorConsentStatus::Granted(Box::new(grant.clone())))
            }
            _ => Err(SessionError::Unavailable),
        }
    }
    fn live_connector_consent(
        &self,
        grant: &VerifiedConnectorConsent,
    ) -> Result<ConnectorConsentStatus, SessionError> {
        let advance = self.connector_live_advance_ms.load(Ordering::SeqCst);
        if advance > 0 {
            self.now.fetch_add(advance, Ordering::SeqCst);
        }
        match self.connector_live_revoked.load(Ordering::SeqCst) {
            1 => return Ok(ConnectorConsentStatus::Denied),
            2 => return Ok(ConnectorConsentStatus::Withdrawn),
            _ => {}
        }
        self.load_connector_consent(&grant.binding().owner, grant.record())
    }
}
fn connector_resource_plan() -> SessionResourcePlan {
    let read = SessionResourceAmounts {
        cost_microunits: 1,
        retrieval_rounds: 1,
        retrieval_results: 1,
        retrieval_tokens: 25,
        ..Default::default()
    };
    SessionResourcePlan {
        preparation: SessionResourceAmounts {
            model_attempts: 1,
            input_tokens: 100,
            output_tokens: 50,
            output_bytes: 1024,
            cost_microunits: 7,
            ..read
        },
        approved_resume: read,
        execution: read,
        after_snapshot_recovery: read,
        goal_verification: SessionResourceAmounts::default(),
    }
}
fn connector_resource_envelope() -> SessionResourceAmounts {
    SessionResourceAmounts {
        model_attempts: 1,
        input_tokens: 100,
        output_tokens: 50,
        output_bytes: 1024,
        cost_microunits: 10,
        retrieval_rounds: 4,
        retrieval_results: 4,
        retrieval_tokens: 100,
        ..Default::default()
    }
}
impl StructuredSessionHost for Host {
    fn resource_limits(
        &self,
        _: &OwnerBinding,
        goal: &SupportedSessionGoal,
    ) -> Result<Option<SessionResourceLimits>, SessionError> {
        if !self.resource_admission {
            return Ok(None);
        }
        let n = self.resource_step_limit;
        Ok(goal.as_connector().map(|_| SessionResourceLimits {
            cost_unit: ReferenceId::new("test-credit").unwrap(),
            amounts: SessionResourceAmounts {
                connector_calls: if self.connector_runtime.pre_goal_mode != 0 {
                    n
                } else {
                    0
                },
                model_attempts: n,
                input_tokens: 100 * n,
                output_tokens: 50 * n,
                output_bytes: 1024 * n,
                cost_microunits: 10 * n,
                retrieval_rounds: 4 * n,
                retrieval_results: 4 * n,
                retrieval_tokens: 100 * n,
                ..Default::default()
            },
        }))
    }

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
    fn connector_runtime(&self) -> Result<&dyn ConnectorSessionRuntimePort, SessionError> {
        if self.connector_registered {
            Ok(&self.connector_runtime)
        } else {
            Err(SessionError::UnsupportedCapability)
        }
    }
    fn register(
        &self,
        authenticated: &OwnerBinding,
        intent: Intent,
        execution: RequestedExecution,
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
        self.disclose(authenticated, &SessionId::new("start").unwrap())?;
        let registered_goal = if let Some(connector_goal) = &self.connector_goal {
            if intent != *connector_goal.intent() || execution != selection() {
                return Err(SessionError::UnsupportedCapability);
            }
            SupportedSessionGoal::connector(connector_goal.clone())
        } else {
            let artifact_goal = goal(self.sources);
            if intent != *artifact_goal.intent() || execution != selection() {
                return Err(SessionError::UnsupportedCapability);
            }
            if self.connector_registered {
                SupportedSessionGoal::connector(connector_goal_from_artifact(&artifact_goal))
            } else {
                SupportedSessionGoal::artifact(artifact_goal)
            }
        };
        Ok((
            registered_goal,
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
fn start_rejects_duplicate_commands_and_inspect_can_resolve_command_outcome() {
    let c = coordinator(false);
    let started = start(&c);
    let inspected = c
        .inspect(
            &owner(),
            InspectTarget::Command(CommandId::new("start").unwrap()),
        )
        .unwrap();
    assert_eq!(inspected.session, started.session);
    assert!(inspected.command_outcome.is_some());
    assert_eq!(
        c.execute(
            &owner(),
            SessionCommand::Start {
                command: CommandId::new("start").unwrap(),
                intent: goal(false).intent().clone(),
                execution: selection(),
            },
        )
        .unwrap_err(),
        SessionError::Duplicate,
    );
}

#[test]
fn start_refuses_a_budget_that_is_already_expired() {
    let c = coordinator(false);
    c.host.now.store(1_000_000, Ordering::SeqCst);
    assert_eq!(
        c.execute(
            &owner(),
            SessionCommand::Start {
                command: CommandId::new("expired-start").unwrap(),
                intent: goal(false).intent().clone(),
                execution: selection(),
            },
        )
        .unwrap_err(),
        SessionError::LimitExceeded,
    );
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
fn clarification_selects_only_a_registered_source_and_resumes_runnable_session() {
    let coordinator = coordinator(true);
    let pending = start(&coordinator);
    let checkpoint = coordinator.details(&owner(), &pending.session).unwrap();
    let Some(PendingInteraction::Source(question)) = checkpoint.pending else {
        panic!("source selection should be pending")
    };
    let selected = question.alternatives[0].clone();
    let clarified = coordinator
        .execute(
            &owner(),
            SessionCommand::Clarify {
                at: mutation(&pending, "clarify-registered-source"),
                pending: question.pending.id.clone(),
                answer: ClarificationAnswer::SelectSource(selected.clone()),
            },
        )
        .unwrap();
    assert_eq!(clarified.state, SessionState::Runnable);
    assert!(
        coordinator
            .details(&owner(), &pending.session)
            .unwrap()
            .selected_source
            == Some(selected)
    );
}

#[test]
fn cancel_connector_consent_pause_stops_before_external_dispatch() {
    let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let mut host =
        Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
    host.connector_runtime.consent_required_first = true;
    let coordinator = SessionCoordinator::new(Repository::default(), host);
    let started = start_connector_goal(&coordinator, &goal, "cancel-consent-start");
    let paused = continue_as(&coordinator, &started, "cancel-consent-pause");
    let cancelled = coordinator
        .execute(
            &owner(),
            SessionCommand::Cancel {
                at: mutation(&paused, "cancel-consent"),
            },
        )
        .unwrap();
    assert_eq!(cancelled.state, SessionState::Cancelled);
    assert_eq!(cancelled.dispatch_knowledge, DispatchKnowledge::Stopped);
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);
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
    let connector_approval = json!({
        "schema_version":"2.0",
        "scope":{"workspace_id":"w","project_id":"p","binding_id":"b"},
        "operation":"session.approve",
        "correlation":{"request_id":"r"},
        "execution":{"operating_mode":"HARDENING","execution_profile":"FULL_PATH"},
        "input":{
            "session_id":"session-a","command_id":"approve-a","expected_revision":2,
            "pending_id":"pending-a",
            "consent":{"contract":"cg.connector-consent-record","contract_version":"1.0",
                "reference":{"id":"grant-a","revision":"1","digest":format!("sha256:{}", "a".repeat(64))}}
        }
    });
    assert!(matches!(
        boundary::decode("session.approve", &connector_approval),
        Ok(boundary::Decoded::Command(
            SessionCommand::ApproveConnector { .. }
        ))
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

    // Exercise every v2 operation projection independently of coordinator
    // behavior. These branches are part of the public MCP wire contract.
    assert_eq!(boundary::tools().len(), 6);
    let envelope = |operation: &str, input: serde_json::Value| {
        json!({
            "schema_version":"2.0", "scope":{"workspace_id":"w","project_id":"p","binding_id":"b"},
            "operation":operation, "correlation":{"request_id":"r"},
            "execution":{"operating_mode":"HARDENING","execution_profile":"FULL_PATH"}, "input":input
        })
    };
    assert!(matches!(
        boundary::decode(
            "session.inspect",
            &envelope("session.inspect", json!({"session_id":"session-a"}))
        ),
        Ok(boundary::Decoded::Inspect(InspectTarget::Session(_)))
    ));
    assert!(matches!(
        boundary::decode(
            "session.inspect",
            &envelope("session.inspect", json!({"command_id":"command-a"}))
        ),
        Ok(boundary::Decoded::Inspect(InspectTarget::Command(_)))
    ));
    let reference =
        json!({"id":"record-a","revision":"1","digest":format!("sha256:{}", "a".repeat(64))});
    let mutation = |operation: &str, extra: serde_json::Value| {
        let mut input =
            json!({"session_id":"session-a","command_id":"command-a","expected_revision":1});
        input
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        envelope(operation, input)
    };
    assert!(matches!(
        boundary::decode(
            "session.clarify",
            &mutation(
                "session.clarify",
                json!({
                    "pending_id":"pending-a","answer":{"contract":"cg.clarification-answer","contract_version":"2.0","selected_source":reference}
                })
            )
        ),
        Ok(boundary::Decoded::Command(SessionCommand::Clarify { .. }))
    ));
    assert!(matches!(
        boundary::decode(
            "session.approve",
            &mutation(
                "session.approve",
                json!({
                    "pending_id":"pending-a","consent":{"contract":"cg.consent-record","contract_version":"2.0","reference":reference}
                })
            )
        ),
        Ok(boundary::Decoded::Command(SessionCommand::Approve { .. }))
    ));
    assert!(matches!(
        boundary::decode("session.continue", &mutation("session.continue", json!({}))),
        Ok(boundary::Decoded::Command(SessionCommand::Continue { .. }))
    ));
    assert!(matches!(
        boundary::decode("session.cancel", &mutation("session.cancel", json!({}))),
        Ok(boundary::Decoded::Command(SessionCommand::Cancel { .. }))
    ));
    assert!(matches!(
        boundary::decode(
            "session.inspect",
            &envelope("session.inspect", json!({"session_id":"bad id"}))
        ),
        Err("CG_INVALID_INPUT")
    ));
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

#[test]
fn connector_start_uses_registered_connector_assessment_without_artifact_preparation() {
    let host = Host::connector(false);
    let repository = Repository::default();
    let coordinator = SessionCoordinator::new(repository, host);
    let intent = goal(false).intent().clone();
    let snapshot = coordinator
        .execute(
            &owner(),
            SessionCommand::Start {
                command: CommandId::new("connector-start").unwrap(),
                intent,
                execution: selection(),
            },
        )
        .unwrap();

    assert_eq!(snapshot.state, SessionState::Runnable);
    let checkpoint = coordinator.details(&owner(), &snapshot.session).unwrap();
    assert!(checkpoint.goal.as_connector().is_some());
    assert_eq!(checkpoint.initial_assessment.unwrap().outcome, "UNKNOWN");
    assert_eq!(coordinator.host.prepares.load(Ordering::SeqCst), 0);
}

#[test]
fn reserved_connector_dispatch_recovers_as_typed_unknown_without_replay() {
    let host = Host::connector(false);
    let repository = Repository::default();
    let coordinator = SessionCoordinator::new(repository, host);
    let started = coordinator
        .execute(
            &owner(),
            SessionCommand::Start {
                command: CommandId::new("connector-start-recovery").unwrap(),
                intent: goal(false).intent().clone(),
                execution: selection(),
            },
        )
        .unwrap();
    let dispatch = DispatchId::new("connector-dispatch-recovery").unwrap();
    let checkpoint = {
        let mut rows = coordinator.repository.rows.lock().unwrap();
        let Rows {
            session, records, ..
        } = &mut *rows;
        let fenced = session.as_mut().unwrap();
        let connector_goal = fenced.checkpoint.goal.as_connector().unwrap();
        let step = connector_goal.steps()[0].clone();
        let binding = ConnectorStepBinding {
            owner: owner(),
            scope: connector_goal.scope().clone(),
            session: started.session.clone(),
            run: started.run.clone(),
            revision: Revision::new(3).unwrap(),
            task: TaskId::new("connector-task").unwrap(),
            step,
            dispatch: dispatch.clone(),
            invocation: gateway_domain::prompt_ir::InvocationIrIdentity {
                schema_version: SchemaVersion::V1,
                invocation_id: ReferenceId::new("connector-invocation").unwrap(),
                invocation_digest: ContentDigest::new("a".repeat(64)).unwrap(),
                handoff_digest: ContentDigest::new("b".repeat(64)).unwrap(),
                tools_digest: ContentDigest::new("c".repeat(64)).unwrap(),
                ir_digest: ContentDigest::new("d".repeat(64)).unwrap(),
            },
        };
        fenced
            .checkpoint
            .resource_ledger
            .as_mut()
            .unwrap()
            .reserve(
                ResourceReservationBinding {
                    owner: owner(),
                    scope: binding.scope.clone(),
                    session: started.session.clone(),
                    run: started.run.clone(),
                    revision: Revision::new(2).unwrap(),
                    step: binding.step.clone(),
                    dispatch: dispatch.clone(),
                },
                connector_resource_envelope(),
            )
            .unwrap();
        fenced.checkpoint.snapshot.revision = Revision::new(3).unwrap();
        let bytes = br#"{"contract":"before"}"#.to_vec();
        let before = content_reference("connector-before-recovery", &bytes).unwrap();
        fenced.checkpoint.snapshot.state = SessionState::Dispatching;
        fenced.checkpoint.snapshot.dispatch = Some(dispatch.clone());
        fenced.checkpoint.snapshot.dispatch_knowledge = DispatchKnowledge::Reserved;
        fenced.checkpoint.lease_until_ms = Some(0);
        fenced
            .checkpoint
            .connector_step_snapshots
            .push(ConnectorStepSnapshotRef {
                binding,
                phase: ConnectorSnapshotPhase::Before,
                record: before.clone(),
            });
        records.push((
            owner(),
            RecordWrite {
                reference: before,
                kind: RecordKind::ConnectorSnapshot,
                bytes,
            },
        ));
        fenced.checkpoint.clone()
    };
    checkpoint.validate().unwrap();

    let recovered = coordinator.recover(&owner(), &started.session).unwrap();
    assert_eq!(recovered.state, SessionState::OutcomeUnknown);
    assert_eq!(recovered.dispatch_knowledge, DispatchKnowledge::Unknown);
    let after_first_recovery = coordinator.details(&owner(), &started.session).unwrap();
    assert_eq!(after_first_recovery.connector_results.len(), 1);
    assert_eq!(
        coordinator
            .repository
            .record_kind(&owner(), &after_first_recovery.connector_results[0].record)
            .unwrap(),
        RecordKind::ConnectorResult
    );

    let second = coordinator.recover(&owner(), &started.session).unwrap();
    let after_second_recovery = coordinator.details(&owner(), &started.session).unwrap();
    assert_eq!(second.state, SessionState::OutcomeUnknown);
    assert_eq!(after_second_recovery.connector_results.len(), 1);
    assert_eq!(coordinator.host.prepares.load(Ordering::SeqCst), 0);
}

fn two_step_connector_goal() -> (
    SupportedConnectorGoal,
    gateway_application::sessions::PreparedConnectorTurn,
) {
    connector_turn_fixtures::goal_and_prepared_for_steps(vec![
        PlanStepId::new("connector-step-one").unwrap(),
        PlanStepId::new("connector-step-two").unwrap(),
    ])
}

#[test]
fn connector_consent_approval_resumes_exact_material_before_single_dispatch() {
    let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let mut host =
        Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
    host.connector_runtime.consent_required_first = true;
    let coordinator = SessionCoordinator::new(Repository::default(), host);
    let started = start_connector_goal(&coordinator, &goal, "consent-cycle-start");

    let paused = continue_as(&coordinator, &started, "consent-cycle-prepare");
    assert_eq!(paused.state, SessionState::PendingConsent);
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    assert_eq!(paused.dispatch_knowledge, DispatchKnowledge::Prepared);
    let checkpoint = coordinator.details(&owner(), &paused.session).unwrap();
    let Some(PendingInteraction::ConnectorConsent(binding)) = checkpoint.pending.as_ref() else {
        panic!("connector consent should be pending")
    };
    let Some(pending_ref) = paused.pending.as_ref() else {
        panic!("pending identity should be retained")
    };
    let boundary_request = serde_json::json!({
        "schema_version":"2.0",
        "scope":{"workspace_id":"w","project_id":"p","binding_id":"b"},
        "operation":"session.continue",
        "correlation":{"request_id":"consent-cycle"}
    });
    let boundary_response = boundary::response(&boundary_request, paused.clone(), &checkpoint);
    assert_eq!(
        boundary_response["result"]["interaction"]["kind"],
        "connector_consent"
    );
    assert_eq!(
        boundary_response["result"]["interaction"]["payload"]["resume"],
        serde_json::to_value(&binding.resume).unwrap()
    );
    let response_schema = boundary::artifact("response.schema.json").unwrap();
    let common_schema = boundary::artifact("common.schema.json").unwrap();
    assert!(gateway_application::codex::contracts::valid(
        &boundary_response,
        &response_schema,
        &common_schema,
    ));
    let mut unknown_resume_field = boundary_response.clone();
    unknown_resume_field["result"]["interaction"]["payload"]["resume"]["unexpected"] =
        serde_json::json!(true);
    assert!(!gateway_application::codex::contracts::valid(
        &unknown_resume_field,
        &response_schema,
        &common_schema,
    ));
    let material = coordinator
        .host
        .connector_runtime
        .resume_seen
        .lock()
        .unwrap()
        .clone()
        .unwrap();
    material.matches(&binding.resume).unwrap();
    let fallback = DefaultsOnlyConnectorRuntime;
    assert_eq!(
        fallback.revalidate_consent_binding(&checkpoint, binding, &material),
        Err(SessionError::UnsupportedCapability)
    );

    assert_eq!(
        fallback.preflight_retained_consent_binding(&checkpoint, binding, &material),
        Err(SessionError::UnsupportedCapability)
    );
    let revalidations = coordinator
        .host
        .connector_runtime
        .revalidations
        .load(Ordering::SeqCst);
    coordinator
        .host
        .connector_runtime
        .preflight_retained_consent_binding(&checkpoint, binding, &material)
        .unwrap();
    assert_eq!(
        coordinator
            .host
            .connector_runtime
            .revalidations
            .load(Ordering::SeqCst),
        revalidations + 1,
        "default preflight must invoke the existing strict legacy owner hook"
    );

    let record = ConnectorConsentRecordRef(reference("connector-consent-cycle"));
    let grant =
        VerifiedConnectorConsent::from_trusted_record(record.clone(), binding.as_ref().clone())
            .unwrap();
    *coordinator.host.connector_grant.lock().unwrap() = Some(grant);
    let approved = coordinator
        .execute(
            &owner(),
            SessionCommand::ApproveConnector {
                at: mutation(&paused, "consent-cycle-approve"),
                pending: pending_ref.id.clone(),
                consent: record,
            },
        )
        .unwrap();
    assert_eq!(approved.state, SessionState::Runnable);
    assert_eq!(approved.dispatch_knowledge, DispatchKnowledge::Prepared);
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);

    let completed = continue_as(&coordinator, &approved, "consent-cycle-resume");
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    assert_eq!(
        *coordinator
            .host
            .connector_runtime
            .resume_seen
            .lock()
            .unwrap(),
        Some(material)
    );
    assert!(
        coordinator
            .host
            .connector_runtime
            .revalidations
            .load(Ordering::SeqCst)
            >= 2
    );
    assert_eq!(completed.state, SessionState::Failed);
}

#[test]
fn connector_goal_completion_uses_a_closed_loop_receipt_and_appends_evidence() {
    let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let mut host =
        Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
    host.connector_runtime.issue_connector_receipt = true;
    let coordinator = SessionCoordinator::new(Repository::default(), host);
    let started = start_connector_goal(&coordinator, &goal, "closed-loop-connector-start");
    let completed = continue_as(&coordinator, &started, "closed-loop-connector-complete");

    assert_eq!(completed.state, SessionState::Completed);
    assert_eq!(completed.dispatch_knowledge, DispatchKnowledge::Verified);
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    let evidence = completed
        .final_evidence
        .as_ref()
        .expect("closed loop evidence should be committed");
    assert_eq!(
        coordinator
            .repository
            .record_kind(&owner(), evidence)
            .unwrap(),
        RecordKind::Evidence,
    );
    let bytes = coordinator
        .repository
        .load_artifact(&owner(), evidence)
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["contract"], "cg.connector-goal-verification");
    assert_eq!(value["contract_version"], "2.0");
    assert_eq!(
        value["goal_digest"],
        gateway_application::closed_loop::ClosedLoop::goal_digest(goal.intent()).as_str()
    );
    assert!(value["readset_digest"].is_string());
    assert!(value["goal_readbacks"][0]["binding"].is_object());
    assert!(value["goal_readbacks"][0]["record"]["revision"].is_string());
    assert_eq!(
        value["execution"],
        format!("{}-execution-1", completed.dispatch.unwrap().as_str())
    );
}

#[test]
fn connector_goal_readset_covers_multiple_sources_and_shared_batches_with_exact_persisted_pins() {
    for shared_source in [false, true] {
        let (goal, prepared) = two_condition_connector_goal(shared_source);
        let dispatches = Arc::new(AtomicUsize::new(0));
        let mut host =
            Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
        host.connector_runtime.issue_connector_receipt = true;
        let coordinator = SessionCoordinator::new(Repository::default(), host);
        let started = start_connector_goal(
            &coordinator,
            &goal,
            if shared_source {
                "shared-source-start"
            } else {
                "multi-source-start"
            },
        );
        let completed = continue_as(&coordinator, &started, "readset-complete");
        assert_eq!(completed.state, SessionState::Completed);
        let checkpoint = coordinator.details(&owner(), &completed.session).unwrap();
        let references = checkpoint
            .connector_observations
            .iter()
            .filter(|r| r.role == ConnectorObservationRole::GoalVerificationReadback)
            .collect::<Vec<_>>();
        assert_eq!(references.len(), 2);
        let mut readbacks = references
            .iter()
            .map(|r| {
                decode_goal_verification_readback(&coordinator.repository, &owner(), r, &goal)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let readset =
            build_goal_verification_readset(&checkpoint, &goal.steps()[0], &readbacks).unwrap();
        assert_eq!(
            readbacks[0].covered_conditions.len(),
            if shared_source { 2 } else { 1 }
        );
        readbacks.reverse();
        let mut reordered = checkpoint.clone();
        reordered.connector_observations.reverse();
        assert_eq!(
            build_goal_verification_readset(&reordered, &goal.steps()[0], &readbacks)
                .unwrap()
                .digest(),
            readset.digest()
        );
        assert!(
            build_goal_verification_readset(&checkpoint, &goal.steps()[0], &readbacks[..1])
                .is_err()
        );
        let mut changed = checkpoint.clone();
        let reference = changed
            .connector_observations
            .iter_mut()
            .find(|r| r.role == ConnectorObservationRole::GoalVerificationReadback)
            .unwrap();
        reference.record = RecordRef::new(
            reference.record.id().clone(),
            RecordId::new("changed-revision").unwrap(),
            reference.record.digest().to_owned(),
        )
        .unwrap();
        assert_ne!(
            build_goal_verification_readset(&changed, &goal.steps()[0], &readbacks)
                .unwrap()
                .digest(),
            readset.digest()
        );
        let original = coordinator
            .repository
            .load_artifact(&owner(), completed.final_evidence.as_ref().unwrap())
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        assert_eq!(value["readset_digest"], readset.digest().as_str());
        assert_eq!(value["goal_readbacks"].as_array().unwrap().len(), 2);
        let preparations = coordinator
            .host
            .connector_runtime
            .preparation_calls
            .load(Ordering::SeqCst);
        {
            let mut rows = coordinator.repository.rows.lock().unwrap();
            let cp = &mut rows.session.as_mut().unwrap().checkpoint;
            cp.snapshot.state = SessionState::Runnable;
            cp.snapshot.dispatch_knowledge = DispatchKnowledge::Observed;
            cp.snapshot.final_evidence = None;
        }
        let restarted = coordinator.recover(&owner(), &completed.session).unwrap();
        assert_eq!(restarted.state, SessionState::Completed);
        assert_eq!(
            coordinator
                .repository
                .load_artifact(&owner(), restarted.final_evidence.as_ref().unwrap())
                .unwrap(),
            original
        );
        assert_eq!(
            coordinator
                .host
                .connector_runtime
                .preparation_calls
                .load(Ordering::SeqCst),
            preparations
        );
        assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn connector_goal_rejects_native_receipt_for_smaller_goal_or_omitted_registered_source() {
    for smaller_goal in [true, false] {
        let (goal, prepared) = two_condition_connector_goal(false);
        let dispatches = Arc::new(AtomicUsize::new(0));
        let mut host =
            Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
        host.connector_runtime.issue_connector_receipt = true;
        host.connector_runtime.wrong_smaller_connector_goal = smaller_goal;
        host.connector_runtime.omit_connector_goal_member = !smaller_goal;
        // One real source can report both subjects. The second registered
        // source remains a mandatory retained member even when its facts agree.
        host.connector_runtime.redundant_connector_goal_batches = !smaller_goal;
        let coordinator = SessionCoordinator::new(Repository::default(), host);
        let started = start_connector_goal(
            &coordinator,
            &goal,
            if smaller_goal {
                "smaller-goal-start"
            } else {
                "omitted-source-start"
            },
        );
        let failed = continue_as(&coordinator, &started, "wrong-proof-complete");
        assert_eq!(failed.state, SessionState::Failed);
        assert!(failed.final_evidence.is_none());
        assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn coordinator_rejects_a_real_closed_loop_receipt_with_wrong_execution_identity() {
    let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let mut host =
        Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
    host.connector_runtime.wrong_connector_receipt_execution = true;
    let coordinator = SessionCoordinator::new(Repository::default(), host);
    let started = start_connector_goal(&coordinator, &goal, "wrong-receipt-start");
    let failed = continue_as(&coordinator, &started, "wrong-receipt-complete");
    assert_eq!(failed.state, SessionState::Failed);
    assert_eq!(failed.dispatch_knowledge, DispatchKnowledge::Stopped);
    assert!(failed.final_evidence.is_none());
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
}

#[test]
fn recovery_finishes_persisted_connector_readbacks_without_redispatch() {
    let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let mut host =
        Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
    host.connector_runtime.issue_connector_receipt = true;
    let coordinator = SessionCoordinator::new(Repository::default(), host);
    let started = start_connector_goal(&coordinator, &goal, "recover-goal-start");
    let completed = continue_as(&coordinator, &started, "recover-goal-commit");
    assert_eq!(completed.state, SessionState::Completed);
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);

    // Recreate the committed state immediately before the final evidence append:
    // all immutable connector records exist, while coordinator completion does not.
    {
        let mut rows = coordinator.repository.rows.lock().unwrap();
        let fenced = rows.session.as_mut().unwrap();
        fenced.checkpoint.snapshot.state = SessionState::Runnable;
        fenced.checkpoint.snapshot.dispatch_knowledge = DispatchKnowledge::Observed;
        fenced.checkpoint.snapshot.final_evidence = None;
    }
    let original_evidence = coordinator
        .repository
        .load_artifact(&owner(), completed.final_evidence.as_ref().unwrap())
        .unwrap();
    let preparations = coordinator
        .host
        .connector_runtime
        .preparation_calls
        .load(Ordering::SeqCst);
    let recovered = coordinator.recover(&owner(), &started.session).unwrap();
    assert_eq!(recovered.state, SessionState::Completed);
    assert_eq!(
        coordinator
            .repository
            .load_artifact(&owner(), recovered.final_evidence.as_ref().unwrap())
            .unwrap(),
        original_evidence
    );
    assert_eq!(
        coordinator
            .host
            .connector_runtime
            .preparation_calls
            .load(Ordering::SeqCst),
        preparations
    );
    assert_eq!(recovered.dispatch_knowledge, DispatchKnowledge::Verified);
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    assert!(recovered.final_evidence.is_some());
}

#[test]
fn completed_legacy_connector_evidence_remains_readable_without_silent_upgrade() {
    let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let mut host =
        Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
    host.connector_runtime.issue_connector_receipt = true;
    let coordinator = SessionCoordinator::new(Repository::default(), host);
    let started = start_connector_goal(&coordinator, &goal, "legacy-history-start");
    let completed = continue_as(&coordinator, &started, "legacy-history-complete");
    let bytes = coordinator
        .repository
        .load_artifact(&owner(), completed.final_evidence.as_ref().unwrap())
        .unwrap();
    let mut legacy: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    legacy["contract_version"] = serde_json::json!("1.0");
    legacy.as_object_mut().unwrap().remove("goal_digest");
    legacy.as_object_mut().unwrap().remove("readset_digest");
    legacy["goal_readbacks"]=serde_json::Value::Array(legacy["goal_readbacks"].as_array().unwrap().iter().map(|r|serde_json::json!({"condition":r["goal_condition"],"record":r["record"]["id"],"digest":r["record"]["digest"]})).collect());
    let bytes = serde_json::to_vec(&legacy).unwrap();
    let reference = content_reference("legacy-final-evidence", &bytes).unwrap();
    {
        let mut rows = coordinator.repository.rows.lock().unwrap();
        rows.records.push((
            owner(),
            RecordWrite {
                reference: reference.clone(),
                kind: RecordKind::Evidence,
                bytes: bytes.clone(),
            },
        ));
        rows.session
            .as_mut()
            .unwrap()
            .checkpoint
            .snapshot
            .final_evidence = Some(reference.clone());
    }
    let appends = coordinator.repository.appends.load(Ordering::SeqCst);
    let preparations = coordinator
        .host
        .connector_runtime
        .preparation_calls
        .load(Ordering::SeqCst);
    let historical = coordinator.recover(&owner(), &completed.session).unwrap();
    assert_eq!(historical.final_evidence.as_ref(), Some(&reference));
    assert_eq!(
        coordinator
            .repository
            .load_artifact(&owner(), &reference)
            .unwrap(),
        bytes
    );
    assert_eq!(
        coordinator.repository.appends.load(Ordering::SeqCst),
        appends
    );
    assert_eq!(
        coordinator
            .host
            .connector_runtime
            .preparation_calls
            .load(Ordering::SeqCst),
        preparations
    );
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
}

#[test]
fn denied_withdrawn_or_missing_connector_consent_cannot_resume_or_dispatch() {
    for status in [1, 2, 3, 4, 5] {
        let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
        let dispatches = Arc::new(AtomicUsize::new(0));
        let mut host =
            Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
        host.connector_runtime.consent_required_first = true;
        let coordinator = SessionCoordinator::new(Repository::default(), host);
        let started = start_connector_goal(
            &coordinator,
            &goal,
            &format!("consent-status-{status}-start"),
        );
        let paused = continue_as(
            &coordinator,
            &started,
            &format!("consent-status-{status}-prepare"),
        );
        let checkpoint = coordinator.details(&owner(), &paused.session).unwrap();
        let Some(PendingInteraction::ConnectorConsent(binding)) = checkpoint.pending else {
            panic!("connector consent should be pending")
        };
        let pending = paused.pending.as_ref().unwrap().id.clone();
        let record =
            ConnectorConsentRecordRef(reference(&format!("connector-consent-status-{status}")));
        let material_record = binding.resume.output_record.clone();
        let grant =
            VerifiedConnectorConsent::from_trusted_record(record.clone(), *binding).unwrap();
        *coordinator.host.connector_grant.lock().unwrap() = Some(grant);
        let expected = match status {
            1 | 2 => {
                coordinator
                    .host
                    .connector_revoked
                    .store(status, Ordering::SeqCst);
                SessionError::AuthorityDenied
            }
            3 => {
                coordinator
                    .repository
                    .rows
                    .lock()
                    .unwrap()
                    .records
                    .retain(|(_, row)| row.reference != material_record);
                SessionError::Unavailable
            }
            5 => SessionError::AuthorityDenied,
            _ => {
                coordinator.repository.corrupt.store(2, Ordering::SeqCst);
                SessionError::InvalidInteraction
            }
        };
        let approval = coordinator.execute(
            &owner(),
            SessionCommand::ApproveConnector {
                at: mutation(&paused, &format!("consent-status-{status}-approve")),
                pending,
                consent: record,
            },
        );
        if status == 5 {
            let approved = approval.unwrap();
            coordinator
                .host
                .connector_revoked
                .store(2, Ordering::SeqCst);
            assert_eq!(
                coordinator.execute(
                    &owner(),
                    SessionCommand::Continue {
                        at: mutation(&approved, "consent-revoked-after-approval"),
                    }
                ),
                Err(SessionError::AuthorityDenied),
            );
        } else {
            assert_eq!(approval, Err(expected));
        }
        assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn journal_requires_one_record_for_every_new_pending_producer_reference() {
    let (coordinator, paused, _, _, _) = connector_consent_pause("journal-producer-inputs");
    let current = coordinator
        .repository
        .load(&owner(), &paused.session)
        .unwrap();
    for copies in [0, 2] {
        let mut next = current.checkpoint.clone();
        next.snapshot.revision = next.snapshot.revision.next().unwrap();
        let pin = reference("new-producer-input");
        let Some(PendingInteraction::ConnectorConsent(binding)) = &mut next.pending else {
            panic!("pending connector consent")
        };
        binding.resume.producer_pins = vec![pin.clone()];
        binding.issued_revision = next.snapshot.revision;
        next.snapshot.pending.as_mut().unwrap().issued_revision = next.snapshot.revision;
        assert_eq!(next.validate(), Ok(()));
        let record = RecordWrite {
            reference: pin,
            kind: RecordKind::ConnectorProducerInput,
            bytes: b"new-producer-input".to_vec(),
        };
        let transition = JournalAppend {
            session: paused.session.clone(),
            expected_revision: current.checkpoint.snapshot.revision,
            fence: current.fence,
            command: None,
            next,
            records: vec![record; copies],
        };
        assert_eq!(
            transition.validate_against(&current),
            Err(SessionError::InvalidState),
            "new pending reference cannot have {copies} matching inputs"
        );
    }
}

#[test]
fn pending_connector_consent_requires_exact_producer_inputs_without_orphans() {
    for mode in 1..=5 {
        let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
        let dispatches = Arc::new(AtomicUsize::new(0));
        let mut host =
            Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
        host.connector_runtime.consent_required_first = true;
        host.connector_runtime.producer_input_mode = mode;
        let coordinator = SessionCoordinator::new(Repository::default(), host);
        let started = start_connector_goal(&coordinator, &goal, "producer-record-negative");
        assert_eq!(
            coordinator.execute(
                &owner(),
                SessionCommand::Continue {
                    at: mutation(&started, "invalid-producer-record")
                }
            ),
            Err(SessionError::InvalidInteraction),
            "invalid producer-input mode {mode}"
        );
        assert_eq!(dispatches.load(Ordering::SeqCst), 0);
        assert_eq!(
            coordinator
                .host
                .connector_runtime
                .resume_entries
                .load(Ordering::SeqCst),
            0
        );
    }
}

#[test]
fn connector_consent_requires_one_intact_resume_record() {
    for duplicate in [false, true] {
        let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
        let dispatches = Arc::new(AtomicUsize::new(0));
        let mut host =
            Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
        host.connector_runtime.consent_required_first = true;
        host.connector_runtime.duplicate_resume_record = duplicate;
        host.connector_runtime.tamper_resume_record = !duplicate;
        let coordinator = SessionCoordinator::new(Repository::default(), host);
        let started = start_connector_goal(
            &coordinator,
            &goal,
            &format!("consent-record-{duplicate}-start"),
        );
        assert_eq!(
            coordinator.execute(
                &owner(),
                SessionCommand::Continue {
                    at: mutation(&started, &format!("consent-record-{duplicate}-prepare")),
                },
            ),
            Err(SessionError::InvalidInteraction),
        );
        assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn connector_consent_runtime_binding_must_match_current_authority() {
    let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let mut host =
        Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
    host.connector_runtime.consent_required_first = true;
    host.connector_runtime.wrong_connector_consent_issuer = true;
    let coordinator = SessionCoordinator::new(Repository::default(), host);
    let started = start_connector_goal(&coordinator, &goal, "wrong-consent-issuer-start");
    assert_eq!(
        coordinator.execute(
            &owner(),
            SessionCommand::Continue {
                at: mutation(&started, "wrong-consent-issuer-prepare"),
            },
        ),
        Err(SessionError::AuthorityDenied)
    );
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);
}

#[test]
fn connector_consent_rejects_wrong_interaction_grant_and_stale_resume_storage() {
    let artifact = coordinator(false);
    artifact.host.consent.store(1, Ordering::SeqCst);
    let artifact_start = start(&artifact);
    let artifact_pause = continue_task(&artifact, &artifact_start).unwrap();
    let wrong_kind = artifact.execute(
        &owner(),
        SessionCommand::ApproveConnector {
            at: mutation(&artifact_pause, "approve-connector-on-artifact-consent"),
            pending: artifact_pause.pending.as_ref().unwrap().id.clone(),
            consent: ConnectorConsentRecordRef(reference("wrong-kind-consent")),
        },
    );
    assert_eq!(wrong_kind, Err(SessionError::InvalidInteraction));

    let (coordinator, paused, binding, record, dispatches) =
        connector_consent_pause("mismatched-consent-grant");
    let mut mismatched = binding.clone();
    mismatched.policy_revision = ContentDigest::new("f".repeat(64)).unwrap();
    *coordinator.host.connector_grant.lock().unwrap() = Some(grant_for(record.clone(), mismatched));
    assert_eq!(
        coordinator.execute(
            &owner(),
            SessionCommand::ApproveConnector {
                at: mutation(&paused, "approve-mismatched-connector-grant"),
                pending: paused.pending.as_ref().unwrap().id.clone(),
                consent: record,
            },
        ),
        Err(SessionError::AuthorityDenied)
    );
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);

    let (coordinator, paused, binding, record, dispatches) =
        connector_consent_pause("corrupt-resume-storage");
    *coordinator.host.connector_grant.lock().unwrap() = Some(grant_for(record.clone(), binding));
    coordinator.repository.corrupt.store(1, Ordering::SeqCst);
    assert_eq!(
        coordinator.execute(
            &owner(),
            SessionCommand::ApproveConnector {
                at: mutation(&paused, "approve-corrupt-resume-storage"),
                pending: paused.pending.as_ref().unwrap().id.clone(),
                consent: record,
            },
        ),
        Err(SessionError::StorageUnavailable)
    );
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);
}

#[test]
fn connector_consent_rechecks_live_status_and_expiry_after_resume_validation() {
    for (case, expected) in [
        ("live-withdrawal", SessionError::AuthorityDenied),
        (
            "expires-during-live-check",
            SessionError::ExpiredInteraction,
        ),
    ] {
        let (coordinator, paused, binding, record, dispatches) = connector_consent_pause(case);
        *coordinator.host.connector_grant.lock().unwrap() =
            Some(grant_for(record.clone(), binding.clone()));
        if case == "live-withdrawal" {
            coordinator
                .host
                .connector_live_revoked
                .store(2, Ordering::SeqCst);
        } else {
            let now = coordinator.host.now.load(Ordering::SeqCst);
            coordinator
                .host
                .connector_live_advance_ms
                .store(binding.expires_at_ms.saturating_sub(now), Ordering::SeqCst);
        }
        assert_eq!(
            coordinator.execute(
                &owner(),
                SessionCommand::ApproveConnector {
                    at: mutation(&paused, &format!("approve-{case}")),
                    pending: paused.pending.as_ref().unwrap().id.clone(),
                    consent: record,
                },
            ),
            Err(expected)
        );
        assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn retained_consent_preflight_revocation_or_expiry_never_enters_resume() {
    for approve in [true, false] {
        for expire in [false, true] {
            let (coordinator, paused, binding, record, dispatches) =
                connector_consent_pause("preflight-current-grant");
            *coordinator.host.connector_grant.lock().unwrap() =
                Some(grant_for(record.clone(), binding.clone()));
            let ready = if approve {
                paused.clone()
            } else {
                coordinator
                    .execute(
                        &owner(),
                        SessionCommand::ApproveConnector {
                            at: mutation(&paused, "approve-before-preflight-change"),
                            pending: paused.pending.as_ref().unwrap().id.clone(),
                            consent: record.clone(),
                        },
                    )
                    .unwrap()
            };
            let before = coordinator.details(&owner(), &ready.session).unwrap();
            let before_ledger = before.resource_ledger.clone();
            let revoked = coordinator.host.connector_live_revoked.clone();
            let now = coordinator.host.now.clone();
            let expires_at = binding.expires_at_ms;
            *coordinator
                .host
                .connector_runtime
                .revalidation_hook
                .lock()
                .unwrap() = Some(Arc::new(move || {
                if expire {
                    now.store(expires_at, Ordering::SeqCst);
                } else {
                    revoked.store(2, Ordering::SeqCst);
                }
            }));
            let before_checks = coordinator
                .host
                .connector_runtime
                .revalidations
                .load(Ordering::SeqCst);
            let command = if approve {
                SessionCommand::ApproveConnector {
                    at: mutation(&ready, "approval-preflight-revoked"),
                    pending: ready.pending.as_ref().unwrap().id.clone(),
                    consent: record,
                }
            } else {
                SessionCommand::Continue {
                    at: mutation(&ready, "continue-preflight-revoked"),
                }
            };
            assert_eq!(
                coordinator.execute(&owner(), command),
                Err(if expire {
                    SessionError::ExpiredInteraction
                } else {
                    SessionError::AuthorityDenied
                })
            );
            assert_eq!(
                coordinator
                    .host
                    .connector_runtime
                    .revalidations
                    .load(Ordering::SeqCst),
                before_checks + 1,
                "default preflight delegates to legacy strict callback"
            );
            assert_eq!(
                coordinator
                    .host
                    .connector_runtime
                    .resume_entries
                    .load(Ordering::SeqCst),
                0,
                "no resume cursor or fresh read reaches the runtime after preflight revocation"
            );
            assert_eq!(
                coordinator
                    .host
                    .connector_runtime
                    .resume_calls
                    .load(Ordering::SeqCst),
                0
            );
            assert_eq!(dispatches.load(Ordering::SeqCst), 0);
            assert_eq!(
                coordinator
                    .details(&owner(), &ready.session)
                    .unwrap()
                    .resource_ledger,
                before_ledger
            );
        }
    }
}

#[test]
fn approved_connector_consent_cannot_continue_after_store_binding_drift() {
    let (coordinator, paused, binding, record, dispatches) =
        connector_consent_pause("accepted-grant-drift");
    *coordinator.host.connector_grant.lock().unwrap() =
        Some(grant_for(record.clone(), binding.clone()));
    let approved = coordinator
        .execute(
            &owner(),
            SessionCommand::ApproveConnector {
                at: mutation(&paused, "approve-before-grant-drift"),
                pending: paused.pending.as_ref().unwrap().id.clone(),
                consent: record.clone(),
            },
        )
        .unwrap();

    let mut changed_binding = binding;
    changed_binding.policy_revision = ContentDigest::new("f".repeat(64)).unwrap();
    *coordinator.host.connector_grant.lock().unwrap() = Some(grant_for(record, changed_binding));
    assert_eq!(
        coordinator.execute(
            &owner(),
            SessionCommand::Continue {
                at: mutation(&approved, "continue-after-grant-drift"),
            },
        ),
        Err(SessionError::AuthorityDenied)
    );
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);
}

#[test]
fn connector_dispatch_stops_at_the_session_deadline_without_invoking_runtime() {
    let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let host = Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
    let coordinator = SessionCoordinator::new(Repository::default(), host);
    let started = start_connector_goal(&coordinator, &goal, "connector-deadline-start");
    let checkpoint = coordinator.details(&owner(), &started.session).unwrap();
    coordinator
        .host
        .now
        .store(checkpoint.budget.deadline_ms, Ordering::SeqCst);
    let failed = coordinator
        .execute(
            &owner(),
            SessionCommand::Continue {
                at: mutation(&started, "connector-deadline-continue"),
            },
        )
        .unwrap();
    assert_eq!(failed.state, SessionState::Failed);
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);
}

fn start_connector_goal(
    coordinator: &SessionCoordinator<Repository, Host>,
    goal: &SupportedConnectorGoal,
    command: &str,
) -> SessionSnapshot {
    coordinator
        .execute(
            &owner(),
            SessionCommand::Start {
                command: CommandId::new(command).unwrap(),
                intent: goal.intent().clone(),
                execution: selection(),
            },
        )
        .unwrap()
}

fn continue_as(
    coordinator: &SessionCoordinator<Repository, Host>,
    snapshot: &SessionSnapshot,
    command: &str,
) -> SessionSnapshot {
    coordinator
        .execute(
            &owner(),
            SessionCommand::Continue {
                at: mutation(snapshot, command),
            },
        )
        .unwrap()
}

fn connector_consent_pause(
    name: &str,
) -> (
    SessionCoordinator<Repository, Host>,
    SessionSnapshot,
    ConnectorConsentBinding,
    ConnectorConsentRecordRef,
    Arc<AtomicUsize>,
) {
    let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let mut host =
        Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
    host.connector_runtime.consent_required_first = true;
    let coordinator = SessionCoordinator::new(Repository::default(), host);
    let started = start_connector_goal(&coordinator, &goal, &format!("{name}-start"));
    let paused = continue_as(&coordinator, &started, &format!("{name}-prepare"));
    let checkpoint = coordinator.details(&owner(), &paused.session).unwrap();
    let Some(PendingInteraction::ConnectorConsent(binding)) = checkpoint.pending else {
        panic!("connector consent should be pending")
    };
    let record = ConnectorConsentRecordRef(reference(&format!("{name}-record")));
    (
        coordinator,
        paused,
        binding.as_ref().clone(),
        record,
        dispatches,
    )
}

fn grant_for(
    record: ConnectorConsentRecordRef,
    binding: ConnectorConsentBinding,
) -> VerifiedConnectorConsent {
    VerifiedConnectorConsent::from_trusted_record(record, binding).unwrap()
}

#[test]
fn two_step_connector_session_persists_four_scoped_snapshots_and_separate_goal_readback() {
    let (goal, prepared) = two_step_connector_goal();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let repository = Repository::default();
    let host = Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
    let coordinator = SessionCoordinator::new(repository, host);
    let started = start_connector_goal(&coordinator, &goal, "two-step-start");

    let first = continue_as(&coordinator, &started, "connector-step-one-command");
    assert_eq!(first.state, SessionState::Runnable);
    assert_eq!(
        coordinator.recover(&owner(), &first.session).unwrap(),
        first
    );
    let finished = continue_as(&coordinator, &first, "connector-step-two-command");
    // A trusted runtime must return a CG-14 receipt to complete the goal. This
    // synthetic runtime deliberately returns None after persisting its read.
    assert_eq!(finished.state, SessionState::Failed);
    assert_eq!(dispatches.load(Ordering::SeqCst), 2);

    let checkpoint = coordinator.details(&owner(), &finished.session).unwrap();
    assert_eq!(checkpoint.connector_step_snapshots.len(), 4);
    for step in goal.steps() {
        let step_snapshots = checkpoint
            .connector_step_snapshots
            .iter()
            .filter(|snapshot| &snapshot.binding.step == step)
            .collect::<Vec<_>>();
        assert_eq!(step_snapshots.len(), 2);
        assert_eq!(
            step_snapshots
                .iter()
                .filter(|snapshot| snapshot.phase == ConnectorSnapshotPhase::Before)
                .count(),
            1
        );
        assert_eq!(
            step_snapshots
                .iter()
                .filter(|snapshot| snapshot.phase == ConnectorSnapshotPhase::After)
                .count(),
            1
        );
    }
    for reference in &checkpoint.connector_step_snapshots {
        assert_eq!(
            coordinator
                .repository
                .record_kind(&owner(), &reference.record)
                .unwrap(),
            RecordKind::ConnectorSnapshot
        );
        let bytes = coordinator
            .repository
            .load_artifact(&owner(), &reference.record)
            .unwrap();
        let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(record["binding"]["owner"]["project"], "p");
        assert_eq!(record["binding"]["scope"], goal.scope().as_str());
        assert_eq!(record["binding"]["session"], finished.session.as_str());
        assert_eq!(record["binding"]["run"], finished.run.as_str());
        assert_eq!(
            record["binding"]["revision"],
            serde_json::json!(reference.binding.revision.value())
        );
        assert_eq!(record["binding"]["step"], reference.binding.step.as_str());
        assert_eq!(
            record["binding"]["dispatch"],
            reference.binding.dispatch.as_str()
        );
    }
    let goal_readbacks = checkpoint
        .connector_observations
        .iter()
        .filter(|record| record.role == ConnectorObservationRole::GoalVerificationReadback)
        .collect::<Vec<_>>();
    assert_eq!(goal_readbacks.len(), 1);
    assert_eq!(
        goal_readbacks[0].goal_condition.as_ref().unwrap(),
        &ConditionId::new("ready").unwrap()
    );
    let bytes = coordinator
        .repository
        .load_artifact(&owner(), &goal_readbacks[0].record)
        .unwrap();
    let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(record["contract"], CONNECTOR_GOAL_READBACK_CONTRACT);
    assert!(checkpoint.snapshot.final_evidence.is_none());
}

#[test]
fn later_connector_uncertainty_keeps_first_step_records_and_restart_recovery_never_redispatches() {
    let (goal, prepared) = two_step_connector_goal();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let repository = Repository::default();
    let host = Host::connector_session(
        goal.clone(),
        prepared.clone(),
        dispatches.clone(),
        true,
        true,
    );
    let coordinator = SessionCoordinator::new(repository.clone(), host);
    let started = start_connector_goal(&coordinator, &goal, "uncertain-two-step-start");
    let first = continue_as(&coordinator, &started, "uncertain-step-one-command");
    assert_eq!(first.state, SessionState::Runnable);
    let uncertain = continue_as(&coordinator, &first, "uncertain-step-two-command");
    assert_eq!(uncertain.state, SessionState::OutcomeUnknown);
    assert_eq!(dispatches.load(Ordering::SeqCst), 2);

    let checkpoint = coordinator.details(&owner(), &uncertain.session).unwrap();
    assert_eq!(checkpoint.connector_step_snapshots.len(), 4);
    let first_step_after = checkpoint
        .connector_step_snapshots
        .iter()
        .find(|snapshot| {
            snapshot.phase == ConnectorSnapshotPhase::After
                && snapshot.binding.step == goal.steps()[0]
        })
        .unwrap();
    assert_eq!(
        coordinator
            .repository
            .record_kind(&owner(), &first_step_after.record)
            .unwrap(),
        RecordKind::ConnectorSnapshot
    );
    assert!(checkpoint.connector_results.iter().any(|result| {
        result.binding.step == goal.steps()[0]
            && result.binding.dispatch == first_step_after.binding.dispatch
    }));
    let second_step_after = checkpoint
        .connector_step_snapshots
        .iter()
        .find(|snapshot| {
            snapshot.phase == ConnectorSnapshotPhase::After
                && snapshot.binding.step == goal.steps()[1]
        })
        .unwrap();
    assert_eq!(
        checkpoint
            .connector_results
            .last()
            .unwrap()
            .after_snapshot_state,
        ConnectorAfterSnapshotState::Captured
    );
    assert_eq!(
        coordinator
            .repository
            .record_kind(&owner(), &second_step_after.record)
            .unwrap(),
        RecordKind::ConnectorSnapshot
    );

    // A fresh coordinator and host use the persisted journal state and the
    // same instrumented connector boundary. Unknown outcomes are inspect-only.
    drop(coordinator);
    let restarted_host = Host::connector_session(goal, prepared, dispatches.clone(), true, true);
    let restarted = SessionCoordinator::new(repository, restarted_host);
    let recovered = restarted.recover(&owner(), &uncertain.session).unwrap();
    assert_eq!(recovered.state, SessionState::OutcomeUnknown);
    assert_eq!(dispatches.load(Ordering::SeqCst), 2);
    let after_recovery = restarted.details(&owner(), &uncertain.session).unwrap();
    assert_eq!(after_recovery.connector_step_snapshots.len(), 4);
    assert_eq!(after_recovery.connector_results.len(), 2);
}

#[test]
fn missing_after_capture_is_typed_and_prevents_goal_completion() {
    let (goal, prepared) = two_step_connector_goal();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let repository = Repository::default();
    let mut host =
        Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
    host.connector_runtime.unknown_without_after_on_second = true;
    let coordinator = SessionCoordinator::new(repository, host);
    let started = start_connector_goal(&coordinator, &goal, "missing-after-start");
    let first = continue_as(&coordinator, &started, "missing-after-step-one");
    let uncertain = continue_as(&coordinator, &first, "missing-after-step-two");

    assert_eq!(uncertain.state, SessionState::OutcomeUnknown);
    assert_ne!(uncertain.state, SessionState::Completed);
    let checkpoint = coordinator.details(&owner(), &uncertain.session).unwrap();
    assert_eq!(checkpoint.connector_step_snapshots.len(), 3);
    assert!(!checkpoint.connector_step_snapshots.iter().any(|snapshot| {
        snapshot.phase == ConnectorSnapshotPhase::After && snapshot.binding.step == goal.steps()[1]
    }));
    assert_eq!(
        checkpoint
            .connector_results
            .last()
            .unwrap()
            .after_snapshot_state,
        ConnectorAfterSnapshotState::Missing
    );
    assert!(checkpoint.snapshot.final_evidence.is_none());
    assert_eq!(dispatches.load(Ordering::SeqCst), 2);
}

#[test]
fn returned_unknown_batch_without_after_captures_independent_scoped_snapshot() {
    let (goal, prepared) = two_step_connector_goal();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let repository = Repository::default();
    let mut host = Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, true);
    host.connector_runtime.unknown_without_after_on_second = true;
    let coordinator = SessionCoordinator::new(repository, host);
    let started = start_connector_goal(&coordinator, &goal, "unknown-batch-without-after-start");
    let first = continue_as(&coordinator, &started, "unknown-batch-step-one");
    let uncertain = continue_as(&coordinator, &first, "unknown-batch-step-two");

    assert_eq!(uncertain.state, SessionState::OutcomeUnknown);
    let checkpoint = coordinator.details(&owner(), &uncertain.session).unwrap();
    assert_eq!(checkpoint.connector_step_snapshots.len(), 4);
    assert!(checkpoint.connector_step_snapshots.iter().any(|snapshot| {
        snapshot.phase == ConnectorSnapshotPhase::After && snapshot.binding.step == goal.steps()[1]
    }));
    assert_eq!(
        checkpoint
            .connector_results
            .last()
            .unwrap()
            .after_snapshot_state,
        ConnectorAfterSnapshotState::Captured
    );
    assert!(checkpoint.snapshot.final_evidence.is_none());
    assert_eq!(dispatches.load(Ordering::SeqCst), 2);
}

#[test]
fn invalid_runtime_turn_is_persisted_as_unknown_with_independent_after_capture() {
    let (goal, prepared) = connector_turn_fixtures::goal_and_prepared_for_steps(vec![
        PlanStepId::new("connector-step-one").unwrap(),
    ]);
    let dispatches = Arc::new(AtomicUsize::new(0));
    let mut host = Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, true);
    host.connector_runtime.invalid_turn_on_first = true;
    let coordinator = SessionCoordinator::new(Repository::default(), host);
    let started = start_connector_goal(&coordinator, &goal, "invalid-turn-start");
    let outcome = continue_as(&coordinator, &started, "invalid-turn-continue");

    assert_eq!(outcome.state, SessionState::OutcomeUnknown);
    assert_eq!(outcome.dispatch_knowledge, DispatchKnowledge::Unknown);
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    let checkpoint = coordinator.details(&owner(), &outcome.session).unwrap();
    assert_eq!(checkpoint.connector_results.len(), 1);
    assert_eq!(
        checkpoint.connector_results[0].after_snapshot_state,
        ConnectorAfterSnapshotState::Captured
    );
    let bytes = coordinator
        .repository
        .load_artifact(&owner(), &checkpoint.connector_results[0].record)
        .unwrap();
    let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        record["payload"]["failure"]["code"],
        "invalid_runtime_result"
    );
}

#[test]
fn explicit_pre_dispatch_denial_is_stopped_without_after_snapshot() {
    let (goal, prepared) = connector_turn_fixtures::goal_and_prepared_for_steps(vec![
        PlanStepId::new("connector-step-one").unwrap(),
    ]);
    let dispatches = Arc::new(AtomicUsize::new(0));
    let mut host = Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, true);
    host.connector_runtime.not_dispatched_on_first = true;
    let coordinator = SessionCoordinator::new(Repository::default(), host);
    let started = start_connector_goal(&coordinator, &goal, "denied-turn-start");
    let outcome = continue_as(&coordinator, &started, "denied-turn-continue");

    assert_eq!(outcome.state, SessionState::Failed);
    assert_eq!(outcome.dispatch_knowledge, DispatchKnowledge::Stopped);
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    let checkpoint = coordinator.details(&owner(), &outcome.session).unwrap();
    assert_eq!(checkpoint.connector_results.len(), 1);
    assert_eq!(
        checkpoint.connector_results[0].after_snapshot_state,
        ConnectorAfterSnapshotState::NotRequired
    );
    assert_eq!(checkpoint.connector_step_snapshots.len(), 1);
    assert_eq!(checkpoint.snapshot.final_evidence, None);
}

#[test]
fn connector_resources_are_persisted_before_preparation_and_failed_append_releases_nothing() {
    let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let host = Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
    let preparations = host.connector_runtime.preparation_calls.clone();
    let repository = Repository::default();
    repository.fail_at.store(1, Ordering::SeqCst);
    let coordinator = SessionCoordinator::new(repository, host);
    let started = start_connector_goal(&coordinator, &goal, "resource-append-start");
    assert_eq!(
        coordinator.execute(
            &owner(),
            SessionCommand::Continue {
                at: mutation(&started, "resource-append-continue"),
            }
        ),
        Err(SessionError::StorageUnavailable)
    );
    assert_eq!(preparations.load(Ordering::SeqCst), 0);
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    let retained = coordinator.details(&owner(), &started.session).unwrap();
    assert_eq!(retained.snapshot, started);
    assert!(retained.resource_ledger.unwrap().reservations().is_empty());
}

#[test]
fn connector_resources_require_admission_and_never_refill_between_steps() {
    for admitted in [false, true] {
        let (goal, prepared) = two_step_connector_goal();
        let dispatches = Arc::new(AtomicUsize::new(0));
        let mut host =
            Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
        host.resource_admission = admitted;
        host.resource_step_limit = 1;
        let preparations = host.connector_runtime.preparation_calls.clone();
        let coordinator = SessionCoordinator::new(Repository::default(), host);
        let started = start_connector_goal(&coordinator, &goal, "resource-cap-start");
        let current = if admitted {
            let first = continue_as(&coordinator, &started, "resource-first");
            assert_eq!(first.state, SessionState::Runnable);
            first
        } else {
            started
        };
        assert_eq!(
            coordinator.execute(
                &owner(),
                SessionCommand::Continue {
                    at: mutation(&current, "resource-denied"),
                }
            ),
            Err(if admitted {
                SessionError::LimitExceeded
            } else {
                SessionError::UnsupportedCapability
            })
        );
        assert_eq!(preparations.load(Ordering::SeqCst), usize::from(admitted));
        assert_eq!(dispatches.load(Ordering::SeqCst), usize::from(admitted));
        let retained = coordinator.details(&owner(), &current.session).unwrap();
        if admitted {
            let ledger = retained.resource_ledger.unwrap();
            assert_eq!(*ledger.used(), connector_resource_envelope());
            assert_eq!(ledger.reservations().len(), 1);
        } else {
            assert!(retained.resource_ledger.is_none());
        }
    }
}

#[test]
fn interrupted_preparation_retains_charge_and_recovery_or_cancel_never_repeats_model_call() {
    for cancel in [false, true] {
        let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
        let dispatches = Arc::new(AtomicUsize::new(0));
        let host =
            Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
        let preparations = host.connector_runtime.preparation_calls.clone();
        let repository = Repository::default();
        // Reservation commits, preparation runs, BEFORE/send reservation fails.
        repository.fail_at.store(2, Ordering::SeqCst);
        let coordinator = SessionCoordinator::new(repository, host);
        let started = start_connector_goal(&coordinator, &goal, "prepare-interrupted-start");
        assert_eq!(
            coordinator.execute(
                &owner(),
                SessionCommand::Continue {
                    at: mutation(&started, "prepare-interrupted-command"),
                }
            ),
            Err(SessionError::StorageUnavailable)
        );
        let retained = coordinator.details(&owner(), &started.session).unwrap();
        assert_eq!(retained.snapshot.state, SessionState::Preparing);
        assert_eq!(
            retained.snapshot.dispatch_knowledge,
            DispatchKnowledge::Prepared
        );
        assert_eq!(retained.budget.actions, 0);
        assert!(retained.connector_step_snapshots.is_empty());
        assert_eq!(preparations.load(Ordering::SeqCst), 1);
        let ledger = retained.resource_ledger.clone().unwrap();
        assert_eq!(*ledger.used(), connector_resource_envelope());
        assert_eq!(
            coordinator.execute(
                &owner(),
                SessionCommand::Continue {
                    at: mutation(&retained.snapshot, "prepare-interrupted-command"),
                }
            ),
            Err(SessionError::Duplicate)
        );
        assert_eq!(
            coordinator.recover(&owner(), &started.session),
            Err(SessionError::InvalidState)
        );
        let stopped = if cancel {
            coordinator
                .execute(
                    &owner(),
                    SessionCommand::Cancel {
                        at: mutation(&retained.snapshot, "cancel-preparation"),
                    },
                )
                .unwrap()
        } else {
            coordinator
                .host
                .now
                .store(retained.lease_until_ms.unwrap(), Ordering::SeqCst);
            coordinator.recover(&owner(), &started.session).unwrap()
        };
        assert_eq!(
            stopped.state,
            if cancel {
                SessionState::Cancelled
            } else {
                SessionState::Failed
            }
        );
        assert_eq!(stopped.dispatch_knowledge, DispatchKnowledge::Stopped);
        assert_eq!(
            coordinator.recover(&owner(), &started.session).unwrap(),
            stopped
        );
        assert_eq!(
            coordinator
                .details(&owner(), &started.session)
                .unwrap()
                .resource_ledger,
            Some(ledger)
        );
        assert_eq!(preparations.load(Ordering::SeqCst), 1);
        assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn preparation_that_consumes_deadline_retains_resources_and_cannot_dispatch_or_pause_consent() {
    for consent in [false, true] {
        let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
        let dispatches = Arc::new(AtomicUsize::new(0));
        let mut host =
            Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
        host.connector_runtime.preparation_clock = Some(host.now.clone());
        host.connector_runtime.advance_preparation_ms = 1_000_000;
        host.connector_runtime.consent_required_first = consent;
        let preparations = host.connector_runtime.preparation_calls.clone();
        let coordinator = SessionCoordinator::new(Repository::default(), host);
        let started = start_connector_goal(&coordinator, &goal, "preparation-deadline-start");
        let result = coordinator.execute(
            &owner(),
            SessionCommand::Continue {
                at: mutation(&started, "preparation-deadline-continue"),
            },
        );
        if consent {
            assert_eq!(result, Err(SessionError::ExpiredInteraction));
        } else {
            assert_eq!(result.unwrap().state, SessionState::Failed);
        }
        let retained = coordinator.details(&owner(), &started.session).unwrap();
        assert_eq!(retained.snapshot.state, SessionState::Failed);
        assert!(retained.pending.is_none());
        assert_eq!(retained.budget.actions, 0);
        assert_eq!(
            *retained.resource_ledger.unwrap().used(),
            connector_resource_envelope()
        );
        assert_eq!(preparations.load(Ordering::SeqCst), 1);
        assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn consent_pause_and_approved_resume_use_one_persisted_resource_reservation() {
    let (coordinator, paused, binding, record, dispatches) =
        connector_consent_pause("resource-consent");
    let before = coordinator
        .details(&owner(), &paused.session)
        .unwrap()
        .resource_ledger
        .unwrap();
    assert_eq!(before.reservations().len(), 1);
    *coordinator.host.connector_grant.lock().unwrap() = Some(grant_for(record.clone(), binding));
    let approved = coordinator
        .execute(
            &owner(),
            SessionCommand::ApproveConnector {
                at: mutation(&paused, "resource-consent-approve"),
                pending: paused.pending.as_ref().unwrap().id.clone(),
                consent: record,
            },
        )
        .unwrap();
    let after = continue_as(&coordinator, &approved, "resource-consent-dispatch");
    assert_eq!(
        coordinator
            .details(&owner(), &after.session)
            .unwrap()
            .resource_ledger,
        Some(before)
    );
    assert_eq!(
        coordinator
            .host
            .connector_runtime
            .preparation_calls
            .load(Ordering::SeqCst),
        1
    );
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
}

#[test]
fn two_step_consent_consumes_each_approval_and_reserves_each_step_separately() {
    let (goal, prepared) = two_step_connector_goal();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let mut host =
        Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
    host.connector_runtime.consent_required_first = true;
    let coordinator = SessionCoordinator::new(Repository::default(), host);
    let mut current = start_connector_goal(&coordinator, &goal, "two-consents-start");
    for ordinal in 0..2 {
        let paused = continue_as(
            &coordinator,
            &current,
            &format!("two-consents-prepare-{ordinal}"),
        );
        assert_eq!(paused.state, SessionState::PendingConsent);
        let checkpoint = coordinator.details(&owner(), &paused.session).unwrap();
        let Some(PendingInteraction::ConnectorConsent(binding)) = checkpoint.pending else {
            panic!("expected step consent")
        };
        assert_eq!(
            checkpoint.resource_ledger.unwrap().reservations().len(),
            ordinal + 1
        );
        assert_eq!(dispatches.load(Ordering::SeqCst), ordinal);
        let record = ConnectorConsentRecordRef(reference(&format!("two-consents-grant-{ordinal}")));
        *coordinator.host.connector_grant.lock().unwrap() =
            Some(grant_for(record.clone(), *binding));
        let approved = coordinator
            .execute(
                &owner(),
                SessionCommand::ApproveConnector {
                    at: mutation(&paused, &format!("two-consents-approve-{ordinal}")),
                    pending: paused.pending.as_ref().unwrap().id.clone(),
                    consent: record,
                },
            )
            .unwrap();
        current = continue_as(
            &coordinator,
            &approved,
            &format!("two-consents-dispatch-{ordinal}"),
        );
        assert!(
            coordinator
                .details(&owner(), &current.session)
                .unwrap()
                .accepted_connector_consent
                .is_none()
        );
    }
    let checkpoint = coordinator.details(&owner(), &current.session).unwrap();
    assert_eq!(checkpoint.resource_ledger.unwrap().reservations().len(), 2);
    assert_eq!(
        coordinator
            .host
            .connector_runtime
            .preparation_calls
            .load(Ordering::SeqCst),
        2
    );
    assert_eq!(dispatches.load(Ordering::SeqCst), 2);
    assert_eq!(checkpoint.connector_step_snapshots.len(), 4);
}

#[test]
fn expired_connector_consent_recovery_stops_without_preparing_again_or_refunding_resources() {
    let (coordinator, paused, binding, _, dispatches) =
        connector_consent_pause("expired-resource-consent");
    let ledger = coordinator
        .details(&owner(), &paused.session)
        .unwrap()
        .resource_ledger;
    coordinator
        .host
        .now
        .store(binding.expires_at_ms, Ordering::SeqCst);
    let stopped = coordinator.recover(&owner(), &paused.session).unwrap();
    assert_eq!(stopped.state, SessionState::Failed);
    assert_eq!(stopped.dispatch_knowledge, DispatchKnowledge::Stopped);
    assert!(stopped.pending.is_none());
    assert_eq!(
        coordinator
            .details(&owner(), &paused.session)
            .unwrap()
            .resource_ledger,
        ledger
    );
    assert_eq!(
        coordinator
            .host
            .connector_runtime
            .preparation_calls
            .load(Ordering::SeqCst),
        1
    );
    assert_eq!(coordinator.host.prepares.load(Ordering::SeqCst), 0);
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    assert_eq!(
        coordinator.recover(&owner(), &paused.session).unwrap(),
        stopped
    );
}

#[test]
fn expired_approved_consent_is_rejected_before_any_runtime_resume_read() {
    let (coordinator, paused, binding, record, dispatches) =
        connector_consent_pause("expired-approved-resource");
    *coordinator.host.connector_grant.lock().unwrap() =
        Some(grant_for(record.clone(), binding.clone()));
    let approved = coordinator
        .execute(
            &owner(),
            SessionCommand::ApproveConnector {
                at: mutation(&paused, "expired-approved-resource-approve"),
                pending: paused.pending.as_ref().unwrap().id.clone(),
                consent: record,
            },
        )
        .unwrap();
    let revalidations = coordinator
        .host
        .connector_runtime
        .revalidations
        .load(Ordering::SeqCst);
    coordinator
        .host
        .now
        .store(binding.expires_at_ms, Ordering::SeqCst);
    assert_eq!(
        coordinator.execute(
            &owner(),
            SessionCommand::Continue {
                at: mutation(&approved, "expired-approved-resource-continue"),
            }
        ),
        Err(SessionError::ExpiredInteraction)
    );
    assert_eq!(
        coordinator
            .host
            .connector_runtime
            .revalidations
            .load(Ordering::SeqCst),
        revalidations
    );
    assert_eq!(
        coordinator
            .host
            .connector_runtime
            .resume_calls
            .load(Ordering::SeqCst),
        0
    );
    assert_eq!(
        coordinator
            .details(&owner(), &approved.session)
            .unwrap()
            .snapshot,
        approved
    );
    assert_eq!(dispatches.load(Ordering::SeqCst), 0);
}

#[test]
fn consent_resume_claim_is_persisted_before_reads_and_cannot_repeat_after_interrupt() {
    for fail_claim in [true, false] {
        let (coordinator, paused, binding, record, dispatches) =
            connector_consent_pause("resource-resume-claim");
        *coordinator.host.connector_grant.lock().unwrap() =
            Some(grant_for(record.clone(), binding));
        let approved = coordinator
            .execute(
                &owner(),
                SessionCommand::ApproveConnector {
                    at: mutation(&paused, "resource-resume-claim-approve"),
                    pending: paused.pending.as_ref().unwrap().id.clone(),
                    consent: record,
                },
            )
            .unwrap();
        let ledger = coordinator
            .details(&owner(), &approved.session)
            .unwrap()
            .resource_ledger;
        let next_append = coordinator.repository.appends.load(Ordering::SeqCst) + 1;
        coordinator
            .repository
            .fail_at
            .store(next_append + usize::from(!fail_claim), Ordering::SeqCst);
        assert_eq!(
            coordinator.execute(
                &owner(),
                SessionCommand::Continue {
                    at: mutation(&approved, "resource-resume-claim-continue"),
                }
            ),
            Err(SessionError::StorageUnavailable)
        );
        assert_eq!(
            coordinator
                .host
                .connector_runtime
                .resume_calls
                .load(Ordering::SeqCst),
            usize::from(!fail_claim)
        );
        let checkpoint = coordinator.details(&owner(), &approved.session).unwrap();
        assert_eq!(checkpoint.resource_ledger, ledger);
        assert_eq!(dispatches.load(Ordering::SeqCst), 0);
        if fail_claim {
            assert_eq!(checkpoint.snapshot, approved);
        } else {
            assert_eq!(checkpoint.snapshot.state, SessionState::Preparing);
            assert_eq!(
                coordinator.execute(
                    &owner(),
                    SessionCommand::Continue {
                        at: mutation(&approved, "competing-resume-continue"),
                    }
                ),
                Err(SessionError::StaleRevision)
            );
            assert_eq!(
                coordinator.execute(
                    &owner(),
                    SessionCommand::Continue {
                        at: mutation(&checkpoint.snapshot, "fresh-resume-continue"),
                    }
                ),
                Err(SessionError::InvalidState)
            );
            coordinator
                .host
                .now
                .store(checkpoint.lease_until_ms.unwrap(), Ordering::SeqCst);
            assert_eq!(
                coordinator
                    .recover(&owner(), &approved.session)
                    .unwrap()
                    .state,
                SessionState::Failed
            );
            assert_eq!(
                coordinator
                    .host
                    .connector_runtime
                    .resume_calls
                    .load(Ordering::SeqCst),
                1
            );
        }
    }
}

#[test]
fn phase_plan_refuses_unbounded_recovery_verification_and_over_cap_partitions_before_io() {
    for verification_io in [true, false] {
        let (goal, prepared) = connector_turn_fixtures::one_proposal_goal_and_prepared();
        let dispatches = Arc::new(AtomicUsize::new(0));
        let mut host =
            Host::connector_session(goal.clone(), prepared, dispatches.clone(), false, false);
        host.resource_step_limit = 1;
        let mut plan = connector_resource_plan();
        if verification_io {
            plan.goal_verification.retrieval_rounds = 1;
        } else {
            plan.execution.cost_microunits += 1;
        }
        host.connector_runtime.plan_override = Some(plan);
        let preparations = host.connector_runtime.preparation_calls.clone();
        let coordinator = SessionCoordinator::new(Repository::default(), host);
        let started = start_connector_goal(&coordinator, &goal, "phase-admission-start");
        assert_eq!(
            coordinator.execute(
                &owner(),
                SessionCommand::Continue {
                    at: mutation(&started, "phase-admission-continue"),
                }
            ),
            Err(if verification_io {
                SessionError::UnsupportedCapability
            } else {
                SessionError::LimitExceeded
            })
        );
        let retained = coordinator.details(&owner(), &started.session).unwrap();
        assert_eq!(retained.snapshot, started);
        assert!(retained.resource_ledger.unwrap().reservations().is_empty());
        assert_eq!(preparations.load(Ordering::SeqCst), 0);
        assert_eq!(dispatches.load(Ordering::SeqCst), 0);
    }
}

fn native_pre_goal_rules() -> gateway_application::closed_loop::LoopRules {
    let fixture = fixture::Fixture::new();
    gateway_application::closed_loop::LoopRules {
        capabilities: gateway_application::PlanningCapabilitySnapshot::new(
            fixture.resolved.snapshot.input().index.clone(),
            fixture.resolved.report.basis.registry_fingerprint.as_str(),
            PlanningIrVersion::V1,
        )
        .unwrap(),
        requirements: CapabilityRequirementRules::default(),
        planner: PlannerRules::default(),
        max_iterations: 2,
        max_retries: 1,
    }
}
fn pre_goal_coordinator() -> SessionCoordinator<Repository, Host> {
    let mut host = Host::connector(false);
    host.connector_goal = Some(pre_execution_goal_fixture::goal(
        ContextScopeId::new("pre-goal-scope").unwrap(),
    ));
    host.connector_runtime.pre_goal_mode = 1;
    host.connector_runtime.pre_goal_clock = Some(host.now.clone());
    SessionCoordinator::new(Repository::default(), host)
}
#[test]
fn already_observed_full_goal_completes_without_model_invocation_or_after_records() {
    let c = pre_goal_coordinator();
    let goal = c.host.connector_goal.as_ref().unwrap();
    let started = start_connector_goal(&c, goal, "already-observed-start");
    let completed = continue_as(&c, &started, "already-observed-continue");
    assert_eq!(completed.state, SessionState::Completed);
    let cp = c.details(&owner(), &completed.session).unwrap();
    assert!(cp.pre_execution_goal.is_some());
    assert!(cp.connector_step_snapshots.is_empty());
    assert!(cp.connector_results.is_empty());
    assert!(cp.connector_observations.is_empty());
    assert_eq!(
        c.host
            .connector_runtime
            .pre_goal_reads
            .load(Ordering::SeqCst),
        1
    );
    assert_eq!(
        c.host
            .connector_runtime
            .preparation_calls
            .load(Ordering::SeqCst),
        0
    );
    assert_eq!(
        c.host.connector_runtime.dispatches.load(Ordering::SeqCst),
        0
    );
    let evidence = c
        .repository
        .load_artifact(&owner(), cp.snapshot.final_evidence.as_ref().unwrap())
        .unwrap();
    let evidence: serde_json::Value = serde_json::from_slice(&evidence).unwrap();
    assert_eq!(evidence["purpose"], "PRE_EXECUTION_GOAL");
    assert!(evidence.get("invocation").is_none());
    c.recover(&owner(), &completed.session).unwrap();
    assert_eq!(
        c.execute(
            &owner(),
            SessionCommand::Continue {
                at: mutation(&started, "already-observed-continue"),
            },
        ),
        Err(SessionError::Duplicate),
    );
    assert_eq!(
        c.details(&owner(), &completed.session).unwrap().snapshot.state,
        SessionState::Completed,
    );
    assert_eq!(
        c.host
            .connector_runtime
            .pre_goal_reads
            .load(Ordering::SeqCst),
        1
    );
}
#[test]
fn unsatisfied_before_read_keeps_original_preparing_claim_for_model_route() {
    let mut c = pre_goal_coordinator();
    c.host.connector_runtime.pre_goal_mode = 2;
    let started = start_connector_goal(
        &c,
        c.host.connector_goal.as_ref().unwrap(),
        "unsatisfied-before-start",
    );
    assert_eq!(
        c.execute(
            &owner(),
            SessionCommand::Continue {
                at: mutation(&started, "unsatisfied-before-model")
            }
        ),
        Err(SessionError::UnsupportedCapability)
    );
    let cp = c.details(&owner(), &started.session).unwrap();
    assert!(cp.pre_execution_goal.is_none());
    assert_eq!(
        c.host
            .connector_runtime
            .pre_goal_reads
            .load(Ordering::SeqCst),
        1
    );
    assert_eq!(
        c.host
            .connector_runtime
            .preparation_calls
            .load(Ordering::SeqCst),
        1
    );
    assert_eq!(
        c.host.connector_runtime.dispatches.load(Ordering::SeqCst),
        0
    );
}

fn committed_pre_goal_candidate() -> (SessionCoordinator<Repository, Host>, SessionSnapshot) {
    let c = pre_goal_coordinator();
    c.host
        .connector_runtime
        .pre_goal_verify_error
        .store(1, Ordering::SeqCst);
    let started = start_connector_goal(
        &c,
        c.host.connector_goal.as_ref().unwrap(),
        "goal-read-commit-start",
    );
    assert_eq!(
        c.execute(
            &owner(),
            SessionCommand::Continue {
                at: mutation(&started, "goal-read-commit")
            }
        ),
        Err(SessionError::StorageUnavailable)
    );
    let cp = c.details(&owner(), &started.session).unwrap();
    assert_eq!(cp.snapshot.state, SessionState::Runnable);
    assert!(cp.pre_execution_goal.is_some());
    (c, cp.snapshot)
}
#[test]
fn persisted_no_model_goal_candidate_recovers_without_repeating_any_acquisition() {
    let (c, marker) = committed_pre_goal_candidate();
    c.host
        .connector_runtime
        .pre_goal_verify_error
        .store(0, Ordering::SeqCst);
    let reads = c
        .host
        .connector_runtime
        .pre_goal_reads
        .load(Ordering::SeqCst);
    let restarted = SessionCoordinator::new(c.repository.clone(), c.host);
    let completed = restarted.recover(&owner(), &marker.session).unwrap();
    assert_eq!(completed.state, SessionState::Completed);
    assert_eq!(
        restarted
            .host
            .connector_runtime
            .pre_goal_reads
            .load(Ordering::SeqCst),
        reads
    );
    assert_eq!(
        restarted
            .host
            .connector_runtime
            .preparation_calls
            .load(Ordering::SeqCst),
        0
    );
    assert_eq!(
        restarted
            .host
            .connector_runtime
            .dispatches
            .load(Ordering::SeqCst),
        0
    );
}
#[test]
fn no_model_goal_marker_blocks_effects_on_corruption_staleness_and_definition_drift() {
    for case in ["corrupt", "kind", "expired-observation", "definition"] {
        let (c, marker) = committed_pre_goal_candidate();
        c.host
            .connector_runtime
            .pre_goal_verify_error
            .store(0, Ordering::SeqCst);
        let cp = c.details(&owner(), &marker.session).unwrap();
        let reference = cp.pre_execution_goal.as_ref().unwrap();
        match case {
            "corrupt" => {
                let mut rows = c.repository.rows.lock().unwrap();
                let (_, record) = rows
                    .records
                    .iter_mut()
                    .find(|(_, r)| r.reference == reference.record)
                    .unwrap();
                record.bytes.push(b' ');
            }
            "kind" => {
                let mut rows = c.repository.rows.lock().unwrap();
                let (_, record) = rows
                    .records
                    .iter_mut()
                    .find(|(_, r)| r.reference == reference.record)
                    .unwrap();
                record.kind = RecordKind::Artifact;
            }
            "expired-observation" => c.host.now.store(reference.valid_until_ms, Ordering::SeqCst),
            _ => c
                .host
                .connector_runtime
                .pre_goal_definition_drift
                .store(1, Ordering::SeqCst),
        }
        let result = c.execute(
            &owner(),
            SessionCommand::Continue {
                at: mutation(&marker, "goal-marker-replay"),
            },
        );
        assert!(
            result.is_err() || result.as_ref().unwrap().state == SessionState::Failed,
            "{case}"
        );
        assert_ne!(
            c.details(&owner(), &marker.session).unwrap().snapshot.state,
            SessionState::Completed
        );
        assert_eq!(
            c.host
                .connector_runtime
                .pre_goal_reads
                .load(Ordering::SeqCst),
            1
        );
        assert_eq!(
            c.host
                .connector_runtime
                .preparation_calls
                .load(Ordering::SeqCst),
            0
        );
        assert_eq!(
            c.host.connector_runtime.dispatches.load(Ordering::SeqCst),
            0
        );
    }
}
#[test]
fn no_model_goal_checks_clock_and_fence_after_native_verification() {
    for expire in [false, true] {
        let (c, marker) = committed_pre_goal_candidate();
        c.host
            .connector_runtime
            .pre_goal_verify_error
            .store(0, Ordering::SeqCst);
        let now = c.host.now.clone();
        let rows = c.repository.rows.clone();
        *c.host
            .connector_runtime
            .pre_goal_verify_hook
            .lock()
            .unwrap() = Some(Arc::new(move || {
            if expire {
                now.store(60_000, Ordering::SeqCst)
            } else {
                let mut rows = rows.lock().unwrap();
                let current = rows.session.as_mut().unwrap();
                current.checkpoint.snapshot.revision =
                    current.checkpoint.snapshot.revision.next().unwrap();
                current.checkpoint.snapshot.state = SessionState::Cancelled;
                current.checkpoint.snapshot.dispatch_knowledge = DispatchKnowledge::Stopped;
                current.fence = FenceToken::new(current.fence.value() + 1).unwrap();
            }
        }));
        let outcome = c.recover(&owner(), &marker.session);
        if expire {
            assert_eq!(outcome.unwrap().state, SessionState::Failed)
        } else {
            assert_eq!(outcome, Err(SessionError::StaleRevision));
        }
        assert_eq!(
            c.host
                .connector_runtime
                .pre_goal_reads
                .load(Ordering::SeqCst),
            1
        );
        assert_eq!(
            c.host.connector_runtime.dispatches.load(Ordering::SeqCst),
            0
        );
    }
}

#[test]
fn invocation_free_goal_codec_rejects_weak_unmapped_incomplete_and_expired_reads() {
    let (c, marker) = committed_pre_goal_candidate();
    let cp = c.details(&owner(), &marker.session).unwrap();
    let goal = cp.goal.as_connector().unwrap();
    let data =
        load_pre_execution_goal(&c.repository, &cp, c.host.now.load(Ordering::SeqCst)).unwrap();
    for case in [
        "ack",
        "unknown",
        "partial",
        "conflict",
        "unmapped",
        "incomplete",
        "expired",
    ] {
        let mut observations = data
            .reads()
            .iter()
            .map(|r| r.observation.clone())
            .collect::<Vec<_>>();
        let mut requests = data
            .reads()
            .iter()
            .map(|r| gateway_application::connectors::ConnectorRequest {
                target: r.observation.target.clone(),
                capability: CapabilityId::new(r.read_request.capability.clone()).unwrap(),
                class: gateway_application::connectors::ConnectorOperationClass::Read,
                inventory_revision: r.observation.inventory_revision.clone(),
                arguments: BTreeMap::new(),
                task: r.observation.metadata.task.clone(),
                step: r.observation.metadata.step.clone(),
                execution: r.observation.metadata.execution.clone(),
                correlation: r.observation.correlation.clone(),
            })
            .collect::<Vec<_>>();
        match case {
            "ack" => observations[0].basis =
                gateway_application::connectors::ConnectorObservationBasis::ProviderAcknowledgement,
            "unknown" => {
                observations[0].metadata.quality = QualityMetadata::new(
                    TrustClass::ObservedEvidence,
                    SensitivityClass::Public,
                    Confidence::Unknown,
                    FreshnessStatus::Fresh,
                    Uncertainty::Unknown,
                )
            }
            "partial" => {
                observations[0].metadata.partial = true;
                observations[0].metadata.quality = QualityMetadata::new(
                    TrustClass::ObservedEvidence,
                    SensitivityClass::Public,
                    Confidence::NotApplicable,
                    FreshnessStatus::Fresh,
                    Uncertainty::Incomplete,
                )
            }
            "conflict" => {
                observations[0].metadata.quality = observations[0]
                    .metadata
                    .quality
                    .with_conflict(ConflictStatus::Unresolved)
            }
            "unmapped" => requests[0].capability = CapabilityId::new("foreign.read").unwrap(),
            _ => {}
        }
        let count = if case == "incomplete" {
            1
        } else {
            data.reads().len()
        };
        let inputs = (0..count)
            .map(|i| PreExecutionGoalReadInput {
                condition: &data.reads()[i].condition,
                read_request: &requests[i],
                observation: &observations[i],
                batch: &data.reads()[i].batch,
                max_age_seconds: 60,
            })
            .collect::<Vec<_>>();
        let now = if case == "expired" { 60_000 } else { 100 };
        assert!(
            PreExecutionGoalData::new(
                goal,
                data.binding().clone(),
                data.definition_pins().to_vec(),
                &inputs,
                now
            )
            .is_err(),
            "{case}"
        );
    }
    let defaults = DefaultsOnlyConnectorRuntime;
    assert_eq!(
        defaults.verify_pre_execution_goal(&cp, &data),
        Err(SessionError::UnsupportedCapability)
    );
}
#[test]
fn native_receipt_for_reduced_goal_cannot_select_full_goal_candidate() {
    let (c, marker) = committed_pre_goal_candidate();
    let cp = c.details(&owner(), &marker.session).unwrap();
    let data = load_pre_execution_goal(&c.repository, &cp, 100).unwrap();
    let goal = cp.goal.as_connector().unwrap();
    let first = goal.intent().desired_state().conditions()[0].clone();
    let desired = DesiredState::new(
        DesiredStateId::new("reduced-goal").unwrap(),
        vec![first.clone()],
        ConditionExpression::condition(first.id().clone()),
        vec![],
        vec![],
    )
    .unwrap();
    let reduced = Intent::new(goal.intent().id().clone(), desired);
    let receipt = gateway_application::closed_loop::ClosedLoop::verify_retained(
        data.execution().unwrap(),
        goal.scope().clone(),
        reduced,
        data.readset(),
        native_pre_goal_rules(),
    )
    .unwrap()
    .unwrap();
    assert!(PreExecutionGoalCandidate::new(data, &receipt).is_err());
}
#[test]
fn invocation_free_goal_reference_rejects_cross_owner_scope_run_and_claim() {
    let (c, marker) = committed_pre_goal_candidate();
    let cp = c.details(&owner(), &marker.session).unwrap();
    for case in 0..6 {
        let mut changed = cp.clone();
        let r = changed.pre_execution_goal.as_mut().unwrap();
        match case {
            0 => r.binding.acquisition.owner.principal = PrincipalId::new("foreign").unwrap(),
            1 => r.binding.acquisition.scope = ContextScopeId::new("foreign").unwrap(),
            2 => r.binding.acquisition.run = RunId::new("foreign").unwrap(),
            3 => r.binding.claim_revision = r.committed_revision,
            4 => r.binding.acquisition.dispatch = DispatchId::new("foreign").unwrap(),
            _ => r.binding.acquisition.step = PlanStepId::new("foreign").unwrap(),
        }
        assert!(load_pre_execution_goal(&c.repository, &changed, 100).is_err());
    }
}
