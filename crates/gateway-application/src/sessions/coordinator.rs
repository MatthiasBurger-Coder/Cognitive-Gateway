//! One shared application coordinator for the registered pure CG-10 artifact task.
//! No semantic/model/connector dispatch is provided by this baseline.
use super::*;
use crate::{
    codex::CompileCommand,
    context_application::{CompileStepInput, ContextApplication},
    policy_application::PolicyApplication,
};
use gateway_domain::Intent;
use gateway_policy::{Approval, PolicyDecision, PolicyReason, StepPolicyReport};
use sha2::{Digest, Sha256};

pub struct PreparedTask {
    pub input: CompileCommand,
    pub authority: RecordRef,
    pub action_policy: StepPolicyReport,
}
pub trait StructuredSessionHost: InteractionAuthorityPort {
    fn now_ms(&self) -> Result<u64, SessionError>;
    fn register(
        &self,
        owner: &OwnerBinding,
        intent: Intent,
        execution: RequestedExecution,
    ) -> Result<(SupportedArtifactGoal, SessionBudget), SessionError>;
    /// Additive trusted registration; legacy artifact hosts need no new implementation.
    fn register_goal(
        &self,
        owner: &OwnerBinding,
        intent: Intent,
        execution: RequestedExecution,
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
        self.register(owner, intent, execution)
            .map(|(goal, budget)| (goal.into(), budget))
    }
    fn goal_observation_runtime(&self) -> Option<&dyn ReadonlyGoalObservationRuntimePort> {
        None
    }
    fn prepare(&self, checkpoint: &SessionCheckpoint) -> Result<PreparedTask, SessionError>;
    fn disclose(&self, owner: &OwnerBinding, session: &SessionId) -> Result<(), SessionError>;
    /// Authenticated operator boundary, separate from client SessionCommand DTOs.
    fn issuer(&self, owner: &OwnerBinding, issuer: &PrincipalId) -> Result<(), SessionError>;
}

pub fn content_reference(id: &str, bytes: &[u8]) -> Result<RecordRef, SessionError> {
    RecordRef::new(
        RecordId::new(id)?,
        RecordId::new("1")?,
        format!("sha256:{:x}", Sha256::digest(bytes)),
    )
}
fn encoded<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, SessionError> {
    serde_json::to_vec(value).map_err(|_| SessionError::InvalidInput)
}
fn identity(owner: &OwnerBinding, command: &CommandId) -> Result<String, SessionError> {
    Ok(format!("{:x}", Sha256::digest(encoded(&(owner, command))?)))
}
pub struct SessionCoordinator<J, H> {
    pub repository: J,
    pub host: H,
}
impl<J: SessionRepositoryPort, H: StructuredSessionHost> SessionCoordinator<J, H> {
    pub fn new(repository: J, host: H) -> Self {
        Self { repository, host }
    }
    fn append(
        &self,
        current: &FencedSession,
        command: Option<CommandId>,
        mut next: SessionCheckpoint,
        records: Vec<RecordWrite>,
    ) -> Result<FencedSession, SessionError> {
        next.snapshot.revision = current.checkpoint.snapshot.revision.next()?;
        if let Some(id) = &command {
            next.snapshot.command_outcome = Some(CommandOutcome {
                command: id.clone(),
                session: next.snapshot.session.clone(),
                revision: next.snapshot.revision,
            });
        }
        self.repository.append(
            &current.checkpoint.owner,
            JournalAppend {
                session: current.checkpoint.snapshot.session.clone(),
                expected_revision: current.checkpoint.snapshot.revision,
                fence: current.fence,
                command,
                next,
                records,
            },
        )
    }
    fn load(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
    ) -> Result<FencedSession, SessionError> {
        self.host.disclose(owner, session)?;
        let result = self.repository.load(owner, session)?;
        result.checkpoint.validate()?;
        Ok(result)
    }
    fn duplicate(&self, owner: &OwnerBinding, command: &CommandId) -> Result<bool, SessionError> {
        match self.repository.command_outcome(&CommandKey {
            owner: owner.clone(),
            command: command.clone(),
        }) {
            Ok(_) => Ok(true),
            Err(SessionError::Unavailable) => Ok(false),
            Err(error) => Err(error),
        }
    }
    pub fn details(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
    ) -> Result<SessionCheckpoint, SessionError> {
        Ok(self.load(owner, session)?.checkpoint)
    }
    fn binding(
        &self,
        checkpoint: &SessionCheckpoint,
        prepared: &PreparedTask,
        issued: Revision,
        pending: PendingId,
        dispatch: DispatchId,
        expires: u64,
    ) -> Result<ActionBinding, SessionError> {
        let arguments = encoded(&(
            checkpoint.goal.artifact_basis()?,
            checkpoint.execution,
            &checkpoint.selected_source,
            crate::resolution_encoding::basis_json(&prepared.input.resolved.report.basis),
        ))?;
        Ok(ActionBinding {
            owner: checkpoint.owner.clone(),
            session: checkpoint.snapshot.session.clone(),
            run: checkpoint.snapshot.run.clone(),
            pending,
            issued_revision: issued,
            dispatch,
            step: RecordId::new(checkpoint.goal.artifact_basis()?.step.as_str())?,
            action: content_reference(
                "compile-context",
                b"CG-10.ContextApplication.compile_step/1.0",
            )?,
            arguments: content_reference("compile-arguments", &arguments)?,
            artifact_basis: content_reference(
                "artifact-basis",
                &encoded(checkpoint.goal.artifact_basis()?)?,
            )?,
            authority: prepared.authority.clone(),
            expires_at_ms: expires,
        })
    }
    fn question(
        &self,
        next: &mut SessionCheckpoint,
        revision: Revision,
        now: u64,
    ) -> Result<(), SessionError> {
        let reference = PendingRef {
            id: PendingId::new(format!(
                "question-{}-{}",
                next.snapshot.run.as_str(),
                revision.value()
            ))?,
            issued_revision: revision,
            expires_at_ms: now
                .checked_add(300_000)
                .ok_or(SessionError::LimitExceeded)?
                .min(next.budget.deadline_ms),
        };
        next.snapshot.state = SessionState::PendingClarification;
        next.snapshot.pending = Some(reference.clone());
        next.pending = Some(PendingInteraction::Source(SourceQuestion {
            pending: reference,
            basis: next.goal.artifact_basis()?.projection.clone(),
            alternatives: next.goal.artifact_basis()?.sources.clone(),
        }));
        Ok(())
    }
    fn needs_consent(&self, prepared: &PreparedTask) -> Result<bool, SessionError> {
        let input = &prepared.input;
        let report = PolicyApplication
            .evaluate(&input.resolved, &input.authority, &input.policy_context)
            .map_err(|_| SessionError::AuthorityDenied)?;
        let step = report
            .steps()
            .get(&input.projection.mapping.step)
            .ok_or(SessionError::InvalidInput)?;
        // Missing authorization/evidence has no supported answer that can grant it.
        if step.findings.iter().any(|f| {
            f.decision != PolicyDecision::Allow && f.reason != PolicyReason::ConsentMissing
        }) {
            return Err(SessionError::AuthorityDenied);
        }
        match prepared.action_policy.decision {
            PolicyDecision::Allow | PolicyDecision::RequireConsent => Ok(step.decision
                == PolicyDecision::RequireConsent
                || prepared.action_policy.decision == PolicyDecision::RequireConsent),
            _ => Err(SessionError::AuthorityDenied),
        }
    }
    fn consent_pause(
        &self,
        current: &FencedSession,
        command: Option<CommandId>,
        prepared: &PreparedTask,
        now: u64,
    ) -> Result<FencedSession, SessionError> {
        let mut next = current.checkpoint.clone();
        let revision = next.snapshot.revision.next()?;
        let pending = PendingId::new(format!(
            "consent-{}-{}",
            next.snapshot.run.as_str(),
            revision.value()
        ))?;
        let dispatch = DispatchId::new(format!(
            "compile-{}-{}",
            next.snapshot.run.as_str(),
            revision.value()
        ))?;
        let expires = now
            .checked_add(300_000)
            .ok_or(SessionError::LimitExceeded)?
            .min(next.budget.deadline_ms);
        let binding = self.binding(
            &next,
            prepared,
            revision,
            pending.clone(),
            dispatch.clone(),
            expires,
        )?;
        next.snapshot.state = SessionState::PendingConsent;
        next.snapshot.pending = Some(PendingRef {
            id: pending,
            issued_revision: revision,
            expires_at_ms: expires,
        });
        next.snapshot.dispatch = Some(dispatch);
        next.snapshot.dispatch_knowledge = DispatchKnowledge::Prepared;
        next.pending = Some(PendingInteraction::Consent(Box::new(binding)));
        next.accepted_consent = None;
        next.artifact = None;
        next.lease_until_ms = None;
        self.append(current, command, next, vec![])
    }
    fn valid_grant(
        &self,
        checkpoint: &SessionCheckpoint,
        prepared: &PreparedTask,
        now: u64,
    ) -> Result<bool, SessionError> {
        let Some(grant) = &checkpoint.accepted_consent else {
            return Ok(false);
        };
        let old = grant.binding();
        let current = self.binding(
            checkpoint,
            prepared,
            old.issued_revision,
            old.pending.clone(),
            old.dispatch.clone(),
            old.expires_at_ms,
        )?;
        let live = self.host.live_status(grant)?;
        let granted = matches!(&live, ConsentStatus::Granted(g) if g.as_ref() == grant);
        // Reservations/outcome events advance revision without rebinding consent.
        Ok(grant
            .check_approval(&current, old.issued_revision, now, !granted)
            .is_ok())
    }
    fn compile(
        &self,
        checkpoint: &SessionCheckpoint,
        mut prepared: PreparedTask,
        now: u64,
    ) -> Result<(crate::context_application::CompiledStep, PreparedTask), SessionError> {
        if self.needs_consent(&prepared)? {
            if !self.valid_grant(checkpoint, &prepared, now)? {
                return Err(SessionError::ConsentRequired);
            }
            // Exact, live, action-bound consent supplies only consent facts.
            // It cannot repair absent authorizations or other Process/Policy gates.
            let facts = prepared
                .input
                .policy_context
                .steps
                .get_mut(&prepared.input.projection.mapping.step)
                .ok_or(SessionError::AuthorityDenied)?;
            for id in prepared.input.authority.capabilities.keys() {
                facts.consents.insert(id.clone(), Approval::Granted);
            }
        }
        let input = &prepared.input;
        let compiled = ContextApplication
            .compile_step(CompileStepInput {
                resolved: &input.resolved,
                authority: &input.authority,
                policy_context: &input.policy_context,
                catalog: &input.catalog,
                projection: &input.projection,
                candidates: &input.candidates,
                selected: &input.selected,
            })
            .map_err(|_| SessionError::AuthorityDenied)?;
        if compiled.basis().scope != checkpoint.goal.artifact_basis()?.scope
            || input.projection.mapping.step != checkpoint.goal.artifact_basis()?.step
        {
            return Err(SessionError::ScopeDenied);
        }
        Ok((compiled, prepared))
    }
    fn verify_observed_goal(
        &self,
        current: FencedSession,
        command: Option<CommandId>,
    ) -> Result<SessionSnapshot, SessionError> {
        if current.checkpoint.snapshot.state != SessionState::Runnable
            || current.checkpoint.pre_execution_goal.is_none()
        {
            return Err(SessionError::InvalidState);
        }
        let now = self.host.now_ms()?;
        if now >= current.checkpoint.budget.deadline_ms {
            return self.fail_with_command(&current, SessionState::Failed, command);
        }
        let data = match load_pre_execution_goal(&self.repository, &current.checkpoint, now) {
            Ok(data) => data,
            Err(_) => return self.fail_with_command(&current, SessionState::Failed, command),
        };
        let reference = current.checkpoint.pre_execution_goal.as_ref().unwrap();
        let Some(receipt) = self
            .host
            .goal_observation_runtime()
            .ok_or(SessionError::UnsupportedCapability)?
            .verify_retained_goal(&current.checkpoint, &data)?
        else {
            return self.fail_with_command(&current, SessionState::Failed, command);
        };
        if !super::pre_execution_goal::receipt_matches(&receipt, reference, data.readset())? {
            return self.fail_with_command(&current, SessionState::Failed, command);
        }
        let latest = self.load(
            &current.checkpoint.owner,
            &current.checkpoint.snapshot.session,
        )?;
        if latest != current {
            return Err(SessionError::StaleRevision);
        }
        let final_now = self.host.now_ms()?;
        if final_now >= current.checkpoint.budget.deadline_ms
            || data.validate_fresh(final_now).is_err()
        {
            return self.fail_with_command(&current, SessionState::Failed, command);
        }
        let bytes=serde_json::to_vec(&serde_json::json!({"contract":"cg.connector-pre-execution-goal-verification","contract_version":"1.0","purpose":"PRE_EXECUTION_GOAL","owner":current.checkpoint.owner,"scope":current.checkpoint.goal.scope(),"session":current.checkpoint.snapshot.session,"run":current.checkpoint.snapshot.run,"read_manifest":reference,"goal_digest":receipt.goal_digest(),"readset_digest":receipt.readset_digest(),"execution":receipt.execution(),"source_digest":receipt.source_digest(),"validation":receipt.validation(),"facts":receipt.facts().iter().map(|id|id.as_str()).collect::<Vec<_>>(),"evidence":receipt.evidence().iter().map(|id|id.as_str()).collect::<Vec<_>>()})).map_err(|_|SessionError::InvalidInput)?;
        let evidence = RecordWrite {
            reference: content_reference(
                &format!("pre-goal-proof-{}", &receipt.source_digest().as_str()[..24]),
                &bytes,
            )?,
            kind: RecordKind::Evidence,
            bytes,
        };
        let mut next = current.checkpoint.clone();
        next.snapshot.state = SessionState::Completed;
        next.snapshot.dispatch_knowledge = DispatchKnowledge::Verified;
        next.snapshot.final_evidence = Some(evidence.reference.clone());
        Ok(self
            .append(&current, command, next, vec![evidence])?
            .checkpoint
            .snapshot)
    }
    fn continue_observed_goal(
        &self,
        current: FencedSession,
        command: CommandId,
    ) -> Result<SessionSnapshot, SessionError> {
        if current.checkpoint.prepared_before.is_some() {
            return Err(SessionError::UnsupportedCapability);
        }
        if current.checkpoint.pre_execution_goal.is_some() {
            return self.verify_observed_goal(current, Some(command));
        }
        let runtime = self
            .host
            .goal_observation_runtime()
            .ok_or(SessionError::UnsupportedCapability)?;
        let limits = runtime.resource_limits(&current.checkpoint)?;
        let amounts = runtime.preparation_budget(&current.checkpoint)?;
        amounts.validate_readonly()?;
        if amounts.connector_calls == 0 {
            return Err(SessionError::InvalidInput);
        }
        let lease_started = std::time::Instant::now();
        let claim_started_ms = self.host.now_ms()?;
        let now = claim_started_ms;
        let mut next = current.checkpoint.clone();
        next.budget.reserve(now, false)?;
        let revision = next.snapshot.revision.next()?;
        let step = next
            .goal
            .as_connector()
            .ok_or(SessionError::InvalidState)?
            .steps()[0]
            .clone();
        let dispatch = DispatchId::new(format!(
            "observe-{}-{}",
            next.snapshot.run.as_str(),
            revision.value()
        ))?;
        let binding = ResourceReservationBinding {
            owner: next.owner.clone(),
            scope: next.goal.scope().clone(),
            session: next.snapshot.session.clone(),
            run: next.snapshot.run.clone(),
            revision,
            step,
            dispatch: dispatch.clone(),
        };
        let ledger = next
            .resource_ledger
            .as_mut()
            .ok_or(SessionError::InvalidState)?;
        if limits != *ledger.limits() {
            return Err(SessionError::AuthorityDenied);
        }
        let reservation = ledger.reserve(binding, amounts)?;
        let unit = ledger.limits().cost_unit.clone();
        next.snapshot.state = SessionState::Dispatching;
        next.snapshot.dispatch = Some(dispatch);
        next.snapshot.dispatch_knowledge = DispatchKnowledge::Reserved;
        next.lease_until_ms = Some(
            now.checked_add(30_000)
                .ok_or(SessionError::LimitExceeded)?
                .min(next.budget.deadline_ms),
        );
        let claim = self.append(&current, Some(command), next, vec![])?;
        let now = self.host.now_ms()?;
        let until = claim
            .checkpoint
            .lease_until_ms
            .ok_or(SessionError::InvalidState)?;
        if now >= until {
            return self.fail(&claim, SessionState::Failed);
        }
        // Keep the original monotonic claim lifetime even if the wall clock
        // moves backward while persistence is in progress.
        let deadline = lease_started
            .checked_add(std::time::Duration::from_millis(until - claim_started_ms))
            .ok_or(SessionError::LimitExceeded)?
            .min(
                std::time::Instant::now()
                    .checked_add(std::time::Duration::from_millis(until - now))
                    .ok_or(SessionError::LimitExceeded)?,
            );
        let mut resources = SessionPhaseResources::mint(
            &reservation,
            SessionResourcePhase::Preparation,
            unit,
            claim.checkpoint.snapshot.revision,
            claim.fence,
            deadline,
        )?;
        let Some(preparation) = runtime.prepare_readonly(&claim.checkpoint, &mut resources)? else {
            self.fail(&claim, SessionState::Failed)?;
            return Err(SessionError::UnsupportedCapability);
        };
        if self.load(&claim.checkpoint.owner, &claim.checkpoint.snapshot.session)? != claim {
            return Err(SessionError::StaleRevision);
        }
        let now = self.host.now_ms()?;
        if now >= until || std::time::Instant::now() >= deadline {
            return self.fail(&claim, SessionState::Failed);
        }
        let candidate = match preparation {
            ReadonlyPreparation::GoalSatisfied(candidate) => candidate,
            ReadonlyPreparation::BeforeCaptured(captured) => {
                let (reference, record) = prepare_before_record(&claim, &captured, now)?;
                // Encoding and native validation can take time. The recorded
                // validation instant stays immutable; admission checks use NOW.
                decode_before_record(&reference, &record.bytes, self.host.now_ms()?)?;
                if self.load(&claim.checkpoint.owner, &claim.checkpoint.snapshot.session)? != claim
                {
                    return Err(SessionError::StaleRevision);
                }
                let commit_now = self.host.now_ms()?;
                if commit_now >= until || std::time::Instant::now() >= deadline {
                    return self.fail(&claim, SessionState::Failed);
                }
                before_fresh_at(&captured.snapshot, commit_now)?;
                let mut next = claim.checkpoint.clone();
                next.snapshot.state = SessionState::Runnable;
                next.snapshot.dispatch_knowledge = DispatchKnowledge::Prepared;
                next.lease_until_ms = None;
                next.prepared_before = Some(reference);
                let persisted = self.append(&claim, None, next, vec![record])?;
                // A commit acknowledgement alone cannot establish readable data.
                // Decode the actual owner/kind/hash-linked stored record now.
                let reloaded = load_prepared_before_current(
                    &self.repository,
                    &persisted.checkpoint,
                    self.host.now_ms()?,
                )?;
                if self.load(
                    &persisted.checkpoint.owner,
                    &persisted.checkpoint.snapshot.session,
                )? != persisted
                {
                    return Err(SessionError::StaleRevision);
                }
                let final_now = self.host.now_ms()?;
                if final_now >= until || std::time::Instant::now() >= deadline {
                    return Err(SessionError::ExpiredInteraction);
                }
                // No storage callback follows the final fence/time checks.
                before_fresh_at(&reloaded, final_now)?;
                return Ok(persisted.checkpoint.snapshot);
            }
        };
        let (marker, record) = candidate.into_record(&claim.checkpoint, now)?;
        let mut next = claim.checkpoint.clone();
        next.snapshot.state = SessionState::Runnable;
        // The actual read completed with known NoMutation. Only native goal
        // verification remains; there is no unresolved external effect.
        next.snapshot.dispatch_knowledge = DispatchKnowledge::Prepared;
        next.lease_until_ms = None;
        next.pre_execution_goal = Some(marker);
        let persisted = self.append(&claim, None, next, vec![record])?;
        self.verify_observed_goal(persisted, None)
    }
    fn continue_task(
        &self,
        current: FencedSession,
        command: CommandId,
        _now: u64,
    ) -> Result<SessionSnapshot, SessionError> {
        if current.checkpoint.goal.as_connector().is_some() {
            return self.continue_observed_goal(current, command);
        }
        let prepared = self.host.prepare(&current.checkpoint)?;
        let now = self.host.now_ms()?;
        if self.needs_consent(&prepared)?
            && !self.valid_grant(&current.checkpoint, &prepared, now)?
        {
            return Ok(self
                .consent_pause(&current, Some(command), &prepared, now)?
                .checkpoint
                .snapshot);
        }
        let mut next = current.checkpoint.clone();
        next.budget.reserve(now, false)?;
        next.snapshot.state = SessionState::Dispatching;
        next.snapshot.pending = None;
        next.pending = None;
        if next.snapshot.dispatch.is_none() {
            next.snapshot.dispatch = Some(DispatchId::new(format!(
                "compile-{}-{}",
                next.snapshot.run.as_str(),
                next.snapshot.revision.next()?.value()
            ))?);
        }
        next.snapshot.dispatch_knowledge = DispatchKnowledge::Reserved;
        next.lease_until_ms = Some(
            now.checked_add(30_000)
                .ok_or(SessionError::LimitExceeded)?
                .min(next.budget.deadline_ms),
        );
        let reserved = self.append(&current, Some(command), next, vec![])?;
        self.finish(reserved)
    }
    fn finish(&self, current: FencedSession) -> Result<SessionSnapshot, SessionError> {
        let now = self.host.now_ms()?;
        if now >= current.checkpoint.budget.deadline_ms {
            return self.fail(&current, SessionState::Failed);
        }
        let prepared = self.host.prepare(&current.checkpoint)?;
        let now = self.host.now_ms()?;
        if now >= current.checkpoint.budget.deadline_ms {
            return self.fail(&current, SessionState::Failed);
        }
        let (compiled, prepared) = match self.compile(&current.checkpoint, prepared, now) {
            Ok(result) => result,
            Err(SessionError::ConsentRequired) => {
                let prepared = self.host.prepare(&current.checkpoint)?;
                return Ok(self
                    .consent_pause(&current, None, &prepared, now)?
                    .checkpoint
                    .snapshot);
            }
            Err(_) => return self.fail(&current, SessionState::Failed),
        };
        let current = if current.checkpoint.artifact.is_none() {
            let bytes = canonical_artifact(&compiled)?;
            let reference = content_reference(
                current
                    .checkpoint
                    .snapshot
                    .dispatch
                    .as_ref()
                    .ok_or(SessionError::InvalidState)?
                    .as_str(),
                &bytes,
            )?;
            let mut next = current.checkpoint.clone();
            next.artifact = Some(reference.clone());
            self.append(
                &current,
                None,
                next,
                vec![RecordWrite {
                    reference,
                    kind: RecordKind::Artifact,
                    bytes,
                }],
            )?
        } else {
            current
        };
        // Independent read from the committed store, current recompilation and CG-14.
        let fresh = self.host.prepare(&current.checkpoint)?;
        let now = self.host.now_ms()?;
        if now >= current.checkpoint.budget.deadline_ms {
            return self.fail(&current, SessionState::Failed);
        }
        let (expected, fresh) = match self.compile(&current.checkpoint, fresh, now) {
            Ok(result) => result,
            Err(SessionError::ConsentRequired) => {
                let prepared = self.host.prepare(&current.checkpoint)?;
                return Ok(self
                    .consent_pause(&current, None, &prepared, now)?
                    .checkpoint
                    .snapshot);
            }
            Err(_) => return self.fail(&current, SessionState::Failed),
        };
        if prepared.authority != fresh.authority {
            return self.fail(&current, SessionState::Failed);
        }
        let evidence = verify_artifact(
            &self.repository,
            &current.checkpoint,
            &expected,
            &fresh.input,
        )?;
        if self.host.now_ms()? >= current.checkpoint.budget.deadline_ms {
            return self.fail(&current, SessionState::Failed);
        }
        let mut next = current.checkpoint.clone();
        next.snapshot.state = SessionState::Completed;
        next.snapshot.dispatch_knowledge = DispatchKnowledge::Verified;
        next.snapshot.final_evidence = Some(evidence.reference.clone());
        next.lease_until_ms = None;
        Ok(self
            .append(&current, None, next, vec![evidence])?
            .checkpoint
            .snapshot)
    }
    fn fail(
        &self,
        current: &FencedSession,
        state: SessionState,
    ) -> Result<SessionSnapshot, SessionError> {
        self.fail_with_command(current, state, None)
    }
    fn fail_with_command(
        &self,
        current: &FencedSession,
        state: SessionState,
        command: Option<CommandId>,
    ) -> Result<SessionSnapshot, SessionError> {
        let mut next = current.checkpoint.clone();
        next.snapshot.state = state;
        next.pending = None;
        next.snapshot.pending = None;
        if next.snapshot.dispatch.is_some() {
            next.snapshot.dispatch_knowledge = DispatchKnowledge::Stopped;
        }
        next.lease_until_ms = None;
        Ok(self
            .append(current, command, next, vec![])?
            .checkpoint
            .snapshot)
    }
    /// Explicit trusted recovery event, never an Inspect side effect. The baseline
    /// invokes only the pure context compiler. Fence takeover prevents the old
    /// host committing; committed artifacts are read back, never external retries.
    pub fn recover(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
    ) -> Result<SessionSnapshot, SessionError> {
        let current = self.load(owner, session)?;
        if current.checkpoint.goal.as_connector().is_some() {
            if current.checkpoint.prepared_before.is_some() {
                load_prepared_before_historical(&self.repository, &current.checkpoint)?;
                if self.load(owner, session)? != current {
                    return Err(SessionError::StaleRevision);
                }
                return Ok(current.checkpoint.snapshot);
            }
            if current.checkpoint.snapshot.state == SessionState::Runnable
                && current.checkpoint.pre_execution_goal.is_some()
            {
                return self.verify_observed_goal(current, None);
            }
            if current.checkpoint.snapshot.state == SessionState::Dispatching
                && self.host.now_ms()?
                    >= current
                        .checkpoint
                        .lease_until_ms
                        .ok_or(SessionError::InvalidState)?
            {
                return self.fail(&current, SessionState::Failed);
            }
            return Ok(current.checkpoint.snapshot);
        }
        let now = self.host.now_ms()?;
        if current
            .checkpoint
            .snapshot
            .pending
            .as_ref()
            .is_some_and(|pending| now >= pending.expires_at_ms)
        {
            if now >= current.checkpoint.budget.deadline_ms {
                return self.fail(&current, SessionState::Failed);
            }
            if matches!(
                current.checkpoint.pending,
                Some(PendingInteraction::Source(_))
            ) {
                let mut next = current.checkpoint.clone();
                let revision = next.snapshot.revision.next()?;
                self.question(&mut next, revision, now)?;
                return Ok(self
                    .append(&current, None, next, vec![])?
                    .checkpoint
                    .snapshot);
            }
            let prepared = self.host.prepare(&current.checkpoint)?;
            return Ok(self
                .consent_pause(&current, None, &prepared, now)?
                .checkpoint
                .snapshot);
        }
        if !matches!(
            current.checkpoint.snapshot.state,
            SessionState::Dispatching | SessionState::Cancelling | SessionState::OutcomeUnknown
        ) {
            return Ok(current.checkpoint.snapshot);
        }
        if current
            .checkpoint
            .lease_until_ms
            .is_some_and(|until| now < until)
        {
            return Err(SessionError::InvalidState);
        }
        let mut next = current.checkpoint.clone();
        if next.snapshot.state == SessionState::Cancelling {
            return self.fail(&current, SessionState::Cancelled);
        }
        if next.artifact.is_none() {
            // For this pure compiler the only released effect is the atomic
            // artifact append. Its absence plus the conditional replacement fence
            // proves no result was released; stop without dispatching a retry.
            return self.fail(&current, SessionState::Failed);
        }
        next.snapshot.state = SessionState::Dispatching;
        next.snapshot.dispatch_knowledge = DispatchKnowledge::Reserved;
        next.lease_until_ms = Some(now.checked_add(30_000).ok_or(SessionError::LimitExceeded)?);
        let fenced = self.append(&current, None, next, vec![])?;
        self.finish(fenced)
    }
}
impl<J: SessionRepositoryPort, H: StructuredSessionHost> SessionApplicationPort
    for SessionCoordinator<J, H>
{
    fn inspect(
        &self,
        owner: &OwnerBinding,
        target: InspectTarget,
    ) -> Result<SessionSnapshot, SessionError> {
        let (session, outcome) = match target {
            InspectTarget::Session(id) => (id, None),
            InspectTarget::Command(command) => {
                let result = self.repository.command_outcome(&CommandKey {
                    owner: owner.clone(),
                    command,
                })?;
                (result.session.clone(), Some(result))
            }
        };
        let mut snapshot = self.load(owner, &session)?.checkpoint.snapshot;
        if outcome.is_some() {
            snapshot.command_outcome = outcome;
        }
        Ok(snapshot)
    }
    fn execute(
        &self,
        owner: &OwnerBinding,
        command: SessionCommand,
    ) -> Result<SessionSnapshot, SessionError> {
        let now = self.host.now_ms()?;
        if let SessionCommand::Start {
            command,
            intent,
            execution,
        } = command
        {
            if self.duplicate(owner, &command)? {
                return Err(SessionError::Duplicate);
            }
            let (goal, budget) = self.host.register_goal(owner, intent, execution)?;
            budget.validate()?;
            if now >= budget.deadline_ms {
                return Err(SessionError::LimitExceeded);
            }
            let id = identity(owner, &command)?;
            let session = SessionId::new(format!("session-{id}"))?;
            let mut initial = SessionCheckpoint {
                owner: owner.clone(),
                goal,
                execution,
                snapshot: SessionSnapshot {
                    session: session.clone(),
                    run: RunId::new(format!("run-{id}"))?,
                    revision: Revision::new(1)?,
                    state: SessionState::Runnable,
                    pending: None,
                    dispatch: None,
                    dispatch_knowledge: DispatchKnowledge::None,
                    command_outcome: Some(CommandOutcome {
                        command: command.clone(),
                        session,
                        revision: Revision::new(1)?,
                    }),
                    final_evidence: None,
                },
                budget,
                pending: None,
                accepted_consent: None,
                authority_events: vec![],
                selected_source: None,
                artifact: None,
                lease_until_ms: None,
                initial_assessment: None,
                resource_ledger: None,
                pre_execution_goal: None,
                prepared_before: None,
            };
            if let Some(goal) = initial.goal.as_connector() {
                let runtime = self
                    .host
                    .goal_observation_runtime()
                    .ok_or(SessionError::UnsupportedCapability)?;
                let limits = runtime.resource_limits(&initial)?;
                initial.resource_ledger = Some(SessionResourceLedger::new(limits)?);
                initial.initial_assessment = Some(InitialAssessment {
                    goal: content_reference("supported-goal", &encoded(initial.goal.intent())?)?,
                    authority: goal.plan().clone(),
                    outcome: "UNKNOWN".into(),
                });
            } else {
                let prepared = self.host.prepare(&initial)?;
                initial.initial_assessment = Some(assess_initial_goal(
                    &initial,
                    &prepared.input,
                    &prepared.authority,
                )?);
                if !initial.goal.artifact_basis()?.sources.is_empty() {
                    self.question(&mut initial, Revision::new(1)?, now)?;
                }
            }
            return Ok(self
                .repository
                .create(&command, initial)?
                .checkpoint
                .snapshot);
        }
        let at = command.mutation().ok_or(SessionError::InvalidState)?;
        let current = self.load(owner, &at.session)?;
        SessionAdmission {
            owner: &current.checkpoint.owner,
            session: &current.checkpoint.snapshot.session,
            revision: current.checkpoint.snapshot.revision,
            state: current.checkpoint.snapshot.state,
            pending: current.checkpoint.snapshot.pending.as_ref(),
        }
        .check(
            owner,
            &command,
            self.duplicate(owner, command.command_id())?,
            now,
        )?;
        let mut next = current.checkpoint.clone();
        match command {
            SessionCommand::Clarify {
                at,
                pending,
                answer,
            } => {
                let Some(PendingInteraction::Source(question)) = &next.pending else {
                    return Err(SessionError::InvalidInteraction);
                };
                next.selected_source =
                    Some(question.validate_answer(&pending, at.expected_revision, now, &answer)?);
                self.host.prepare(&next)?;
                next.pending = None;
                next.snapshot.pending = None;
                next.snapshot.state = SessionState::Runnable;
                Ok(self
                    .append(&current, Some(at.command), next, vec![])?
                    .checkpoint
                    .snapshot)
            }
            SessionCommand::Approve { at, consent, .. } => {
                let Some(PendingInteraction::Consent(binding)) = &next.pending else {
                    return Err(SessionError::InvalidInteraction);
                };
                let ConsentStatus::Granted(grant) = self.host.load_consent(owner, &consent)? else {
                    return Err(SessionError::AuthorityDenied);
                };
                let prepared = self.host.prepare(&next)?;
                let fresh = self.binding(
                    &next,
                    &prepared,
                    binding.issued_revision,
                    binding.pending.clone(),
                    binding.dispatch.clone(),
                    binding.expires_at_ms,
                )?;
                grant.check_approval(&fresh, at.expected_revision, self.host.now_ms()?, false)?;
                next.accepted_consent = Some(*grant);
                next.pending = None;
                next.snapshot.pending = None;
                next.snapshot.state = SessionState::Runnable;
                Ok(self
                    .append(&current, Some(at.command), next, vec![])?
                    .checkpoint
                    .snapshot)
            }
            SessionCommand::Continue { at } => self.continue_task(current, at.command, now),
            SessionCommand::Cancel { at } => {
                // Fenced pure compiler cannot release any result after this append.
                next.snapshot.state = SessionState::Cancelled;
                next.pending = None;
                next.snapshot.pending = None;
                next.lease_until_ms = None;
                if next.snapshot.dispatch.is_some() {
                    next.snapshot.dispatch_knowledge = DispatchKnowledge::Stopped;
                }
                Ok(self
                    .append(&current, Some(at.command), next, vec![])?
                    .checkpoint
                    .snapshot)
            }
            SessionCommand::Start { .. } => unreachable!(),
        }
    }
}
