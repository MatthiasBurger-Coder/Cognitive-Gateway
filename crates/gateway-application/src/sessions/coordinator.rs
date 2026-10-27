//! One shared application coordinator for registered structured artifact and
//! connector tasks. Connector execution is fail-closed unless a trusted runtime
//! is installed by the host composition root.
use super::*;
use crate::{
    codex::CompileCommand,
    connectors::ConnectorEffect,
    context_application::{CompileStepInput, ContextApplication},
    experience_patterns::OutcomeClass,
    policy_application::PolicyApplication,
    sessions::connector_execution::{
        MAX_CONNECTOR_STEP_COMMIT_BYTES, MAX_CONNECTOR_STEP_COMMIT_RECORDS, after_snapshot_record,
        aggregate_proposal_effect, before_snapshot_record, proposal_batch_record,
    },
    sessions::readback_storage::{
        GoalVerificationReadbackInput, build_goal_verification_readset,
        decode_goal_verification_readback, encode_connector_readback,
        encode_goal_verification_input, goal_verification_manifest,
    },
};
use gateway_domain::Intent;
use gateway_policy::{Approval, PolicyDecision, PolicyReason, StepPolicyReport};
use sha2::{Digest, Sha256};

fn connector_goal_verification_evidence(
    checkpoint: &SessionCheckpoint,
    last_step: &gateway_domain::PlanStepId,
    receipt: &crate::closed_loop::VerifiedOutcomeReceipt,
) -> Result<Vec<u8>, SessionError> {
    let goal = checkpoint
        .goal
        .as_connector()
        .ok_or(SessionError::InvalidState)?;
    let manifest = goal_verification_manifest(checkpoint, last_step)?;
    let binding = &manifest[0].binding;
    let readset_digest = receipt
        .readset_digest()
        .ok_or(SessionError::InvalidInteraction)?;
    serde_json::to_vec(&serde_json::json!({
        "contract":"cg.connector-goal-verification", "contract_version":"2.0",
        "owner":checkpoint.owner, "scope":goal.scope(),
        "session":checkpoint.snapshot.session, "run":checkpoint.snapshot.run,
        "step":last_step.as_str(), "dispatch":binding.dispatch, "invocation":binding.invocation,
        "execution":receipt.execution(), "source_snapshot":receipt.source_snapshot(),
        "source_digest":receipt.source_digest(), "validation":receipt.validation(),
        "goal_digest":receipt.goal_digest(), "readset_digest":readset_digest,
        "facts":receipt.facts().iter().map(|v| v.as_str()).collect::<Vec<_>>(),
        "evidence":receipt.evidence().iter().map(|v| v.as_str()).collect::<Vec<_>>(),
        "goal_readbacks":manifest,
    }))
    .map_err(|_| SessionError::InvalidInput)
}

fn account_connector_step_record_bytes(
    total: &mut usize,
    bytes: &[u8],
) -> Result<(), SessionError> {
    *total = total
        .checked_add(bytes.len())
        .ok_or(SessionError::LimitExceeded)?;
    if *total > MAX_CONNECTOR_STEP_COMMIT_BYTES {
        return Err(SessionError::LimitExceeded);
    }
    Ok(())
}

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
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError>;
    fn prepare(&self, checkpoint: &SessionCheckpoint) -> Result<PreparedTask, SessionError>;
    /// Trusted immutable session admission, never a client-provided allowance.
    fn resource_limits(
        &self,
        _owner: &OwnerBinding,
        _goal: &SupportedSessionGoal,
    ) -> Result<Option<SessionResourceLimits>, SessionError> {
        Ok(None)
    }
    /// Optional fail-closed connector execution composition. The default keeps
    /// existing structured/artifact hosts unchanged and makes connector goals
    /// explicitly unsupported until a trusted runtime is installed.
    fn connector_runtime(&self) -> Result<&dyn ConnectorSessionRuntimePort, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
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
    fn connector_resume_material(
        &self,
        owner: &OwnerBinding,
        binding: &ConnectorConsentBinding,
    ) -> Result<ConnectorConsentResumeMaterial, SessionError> {
        let reference = &binding.resume.output_record;
        if self.repository.record_kind(owner, reference)? != RecordKind::ConnectorProposal {
            return Err(SessionError::InvalidInteraction);
        }
        let bytes = self.repository.load_artifact(owner, reference)?;
        if content_reference(reference.id().as_str(), &bytes)? != *reference {
            return Err(SessionError::StorageUnavailable);
        }
        let material: ConnectorConsentResumeMaterial =
            serde_json::from_slice(&bytes).map_err(|_| SessionError::StorageUnavailable)?;
        material.matches(&binding.resume)?;
        Ok(material)
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
        let goal = checkpoint
            .goal
            .as_artifact()
            .ok_or(SessionError::UnsupportedCapability)?;
        let arguments = encoded(&(
            goal.basis(),
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
            step: RecordId::new(goal.basis().step.as_str())?,
            action: content_reference(
                "compile-context",
                b"CG-10.ContextApplication.compile_step/1.0",
            )?,
            arguments: content_reference("compile-arguments", &arguments)?,
            artifact_basis: content_reference("artifact-basis", &encoded(goal.basis())?)?,
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
        let goal = next
            .goal
            .as_artifact()
            .ok_or(SessionError::UnsupportedCapability)?;
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
            basis: goal.basis().projection.clone(),
            alternatives: goal.basis().sources.clone(),
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
        let goal = checkpoint
            .goal
            .as_artifact()
            .ok_or(SessionError::UnsupportedCapability)?;
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
        if compiled.basis().scope != goal.basis().scope
            || input.projection.mapping.step != goal.basis().step
        {
            return Err(SessionError::ScopeDenied);
        }
        Ok((compiled, prepared))
    }
    fn next_connector_step(
        checkpoint: &SessionCheckpoint,
    ) -> Result<gateway_domain::PlanStepId, SessionError> {
        let goal = checkpoint
            .goal
            .as_connector()
            .ok_or(SessionError::UnsupportedCapability)?;
        goal.steps()
            .iter()
            .find(|step| {
                !checkpoint.connector_step_snapshots.iter().any(|snapshot| {
                    snapshot.phase == ConnectorSnapshotPhase::After
                        && snapshot.binding.step == **step
                })
            })
            .cloned()
            .ok_or(SessionError::InvalidState)
    }

    fn connector_binding(
        checkpoint: &SessionCheckpoint,
        revision: Revision,
        step: &gateway_domain::PlanStepId,
        dispatch: &DispatchId,
        invocation: &crate::prompt_ir::InvocationIr,
    ) -> Result<ConnectorStepBinding, SessionError> {
        let goal = checkpoint
            .goal
            .as_connector()
            .ok_or(SessionError::UnsupportedCapability)?;
        if !goal.steps().contains(step) {
            return Err(SessionError::InvalidInput);
        }
        Ok(ConnectorStepBinding {
            owner: checkpoint.owner.clone(),
            scope: goal.scope().clone(),
            session: checkpoint.snapshot.session.clone(),
            run: checkpoint.snapshot.run.clone(),
            revision,
            task: invocation.prepared().invocation().data().task.id.clone(),
            step: step.clone(),
            dispatch: dispatch.clone(),
            invocation: invocation.identity(),
        })
    }

    fn record_reference(
        prefix: &str,
        binding: &ConnectorStepBinding,
        bytes: &[u8],
    ) -> Result<RecordRef, SessionError> {
        let id = format!("{prefix}-{:x}", Sha256::digest(encoded(binding)?));
        content_reference(&id[..prefix.len() + 1 + 32], bytes)
    }

    fn phase_resources(
        &self,
        current: &FencedSession,
        step: &gateway_domain::PlanStepId,
        phase: SessionResourcePhase,
    ) -> Result<SessionPhaseResources, SessionError> {
        let latest = self.load(
            &current.checkpoint.owner,
            &current.checkpoint.snapshot.session,
        )?;
        if latest != *current {
            return Err(SessionError::StaleRevision);
        }
        let checkpoint = &current.checkpoint;
        let prepared = checkpoint.snapshot.state == SessionState::Preparing
            && checkpoint.snapshot.dispatch_knowledge == DispatchKnowledge::Prepared;
        let executing = checkpoint.snapshot.state == SessionState::Dispatching
            && checkpoint.snapshot.dispatch_knowledge == DispatchKnowledge::Reserved;
        let valid_phase = match phase {
            SessionResourcePhase::Preparation => {
                prepared && checkpoint.accepted_connector_consent.is_none()
            }
            SessionResourcePhase::ApprovedResume => {
                prepared && checkpoint.accepted_connector_consent.is_some()
            }
            SessionResourcePhase::Execution | SessionResourcePhase::AfterSnapshotRecovery => {
                executing
            }
            SessionResourcePhase::GoalVerification => false,
        };
        if !valid_phase {
            return Err(SessionError::InvalidState);
        }
        let ledger = checkpoint
            .resource_ledger
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        let reservation = ledger
            .reservations()
            .iter()
            .find(|reservation| {
                reservation.binding().step == *step
                    && checkpoint.snapshot.dispatch.as_ref()
                        == Some(&reservation.binding().dispatch)
            })
            .ok_or(SessionError::AuthorityDenied)?;
        let until = checkpoint
            .lease_until_ms
            .ok_or(SessionError::InvalidState)?
            .min(checkpoint.budget.deadline_ms);
        let remaining = until
            .checked_sub(self.host.now_ms()?)
            .filter(|remaining| *remaining > 0)
            .ok_or(SessionError::LimitExceeded)?;
        let deadline = std::time::Instant::now()
            .checked_add(std::time::Duration::from_millis(remaining))
            .ok_or(SessionError::LimitExceeded)?;
        SessionPhaseResources::mint(
            reservation,
            phase,
            ledger.limits().cost_unit.clone(),
            checkpoint.snapshot.revision,
            current.fence,
            deadline,
        )
    }

    fn mark_connector_unknown(
        &self,
        current: &FencedSession,
        binding: &ConnectorStepBinding,
        safe_code: &'static str,
        after: Option<ScopedConnectorSnapshotPayload>,
    ) -> Result<SessionSnapshot, SessionError> {
        let mut next = current.checkpoint.clone();
        let mut result_binding = binding.clone();
        result_binding.revision = next.snapshot.revision.next()?;
        let captured_after = after
            .filter(|payload| payload.scope() == &result_binding.scope)
            .and_then(|payload| {
                after_snapshot_record(&result_binding, &payload)
                    .ok()
                    .map(|bytes| (bytes, payload))
            });
        let after_snapshot_state = if captured_after.is_some() {
            ConnectorAfterSnapshotState::Captured
        } else {
            ConnectorAfterSnapshotState::Missing
        };
        let bytes = serde_json::to_vec(&serde_json::json!({
            "contract":"cg.connector-proposal-batch-result",
            "contract_version":"1.0",
            "binding":result_binding,
            "payload":{
                "all_calls_returned":false,
                "effect":"unknown",
                "after_snapshot_state":after_snapshot_state,
                "results":[],
                "failure":{"ordinal":0,"code":safe_code,"effect":"unknown"}
            }
        }))
        .map_err(|_| SessionError::InvalidInput)?;
        let reference = Self::record_reference("connector-result", &result_binding, &bytes)?;
        let mut records = vec![RecordWrite {
            reference: reference.clone(),
            kind: RecordKind::ConnectorResult,
            bytes,
        }];
        if let Some((after_bytes, _payload)) = &captured_after {
            let after_reference =
                Self::record_reference("connector-after", &result_binding, after_bytes)?;
            next.connector_step_snapshots
                .push(ConnectorStepSnapshotRef {
                    binding: result_binding.clone(),
                    phase: ConnectorSnapshotPhase::After,
                    record: after_reference.clone(),
                });
            records.push(RecordWrite {
                reference: after_reference,
                kind: RecordKind::ConnectorSnapshot,
                bytes: after_bytes.clone(),
            });
        }
        next.connector_results.push(ConnectorResultRef {
            binding: result_binding,
            record: reference.clone(),
            after_snapshot_state,
        });
        next.snapshot.state = SessionState::OutcomeUnknown;
        next.snapshot.dispatch_knowledge = DispatchKnowledge::Unknown;
        next.lease_until_ms = None;
        Ok(self
            .append(current, None, next, records)?
            .checkpoint
            .snapshot)
    }

    fn continue_connector_task(
        &self,
        mut current: FencedSession,
        command: CommandId,
        now: u64,
    ) -> Result<SessionSnapshot, SessionError> {
        if current.checkpoint.pre_execution_goal.is_some() {
            return self.verify_pre_execution_goal(current);
        }
        let goal = current
            .checkpoint
            .goal
            .as_connector()
            .cloned()
            .ok_or(SessionError::UnsupportedCapability)?;
        if now >= current.checkpoint.budget.deadline_ms {
            return self.fail(&current, SessionState::Failed);
        }
        let step = Self::next_connector_step(&current.checkpoint)?;
        let runtime = self.host.connector_runtime()?;
        let next_revision = current.checkpoint.snapshot.revision.next()?;
        let default_dispatch_digest = format!(
            "{:x}",
            Sha256::digest(encoded(&(
                &current.checkpoint.snapshot.run,
                step.as_str(),
                next_revision,
            ))?)
        );
        let default_dispatch =
            DispatchId::new(format!("connector-{}", &default_dispatch_digest[..32]))?;
        let mut command = Some(command);
        if current.checkpoint.accepted_connector_consent.is_none() {
            // The envelope covers model preparation and all independent reads,
            // not merely the later connector mutation. A failed append releases
            // no model/read call. Consume the command key with this first append.
            let mut next = current.checkpoint.clone();
            let mut baseline = next.budget.clone();
            baseline.reserve(now, false)?;
            let ledger = next
                .resource_ledger
                .as_mut()
                .ok_or(SessionError::UnsupportedCapability)?;
            let plan = runtime.resource_plan(&current.checkpoint, &step)?;
            // Verification is repeatable pure computation over persisted reads.
            // Nonzero acquisition here needs a separate durable one-shot claim.
            if !plan.goal_verification.is_zero() {
                return Err(SessionError::UnsupportedCapability);
            }
            ledger.reserve_plan(
                ResourceReservationBinding {
                    owner: next.owner.clone(),
                    scope: goal.scope().clone(),
                    session: next.snapshot.session.clone(),
                    run: next.snapshot.run.clone(),
                    revision: next_revision,
                    step: step.clone(),
                    dispatch: default_dispatch.clone(),
                },
                plan,
            )?;
            next.snapshot.state = SessionState::Preparing;
            next.snapshot.pending = None;
            next.pending = None;
            next.snapshot.dispatch = Some(default_dispatch.clone());
            next.snapshot.dispatch_knowledge = DispatchKnowledge::Prepared;
            next.lease_until_ms = Some(
                now.checked_add(30_000)
                    .ok_or(SessionError::LimitExceeded)?
                    .min(next.budget.deadline_ms),
            );
            current = self.append(&current, command.take(), next, vec![])?;
        } else {
            let binding = &current
                .checkpoint
                .accepted_connector_consent
                .as_ref()
                .ok_or(SessionError::InvalidInteraction)?
                .binding;
            if now >= binding.expires_at_ms {
                return Err(SessionError::ExpiredInteraction);
            }
            if binding.owner != current.checkpoint.owner
                || binding.scope != *goal.scope()
                || binding.session != current.checkpoint.snapshot.session
                || binding.run != current.checkpoint.snapshot.run
                || binding.step != step
            {
                return Err(SessionError::AuthorityDenied);
            }
            if current
                .checkpoint
                .resource_ledger
                .as_ref()
                .is_none_or(|ledger| {
                    !ledger.reservations().iter().any(|reservation| {
                        reservation.binding().dispatch == binding.dispatch
                            && reservation.binding().step == step
                    })
                })
            {
                return Err(SessionError::AuthorityDenied);
            }
            let ConnectorConsentStatus::Granted(grant) = self.host.load_connector_consent(
                &current.checkpoint.owner,
                &current
                    .checkpoint
                    .accepted_connector_consent
                    .as_ref()
                    .ok_or(SessionError::InvalidInteraction)?
                    .record,
            )?
            else {
                return Err(SessionError::AuthorityDenied);
            };
            if grant.binding() != binding
                || !matches!(self.host.live_connector_consent(&grant)?,
                ConnectorConsentStatus::Granted(live) if live.as_ref() == grant.as_ref())
            {
                return Err(SessionError::AuthorityDenied);
            }
            // Reconstructing a retained proposal can perform reads too. Claim
            // this exact reservation under the fence before resuming, so two
            // concurrent Continues cannot spend it twice. No new allowance.
            let mut next = current.checkpoint.clone();
            next.snapshot.state = SessionState::Preparing;
            next.snapshot.dispatch_knowledge = DispatchKnowledge::Prepared;
            next.lease_until_ms = Some(
                now.checked_add(30_000)
                    .ok_or(SessionError::LimitExceeded)?
                    .min(next.budget.deadline_ms),
            );
            current = self.append(&current, command.take(), next, vec![])?;
        }
        let next_revision = current.checkpoint.snapshot.revision.next()?;
        let prepared = if let Some(accepted) = &current.checkpoint.accepted_connector_consent {
            let binding = &accepted.binding;
            if binding.owner != current.checkpoint.owner
                || binding.scope != *goal.scope()
                || binding.session != current.checkpoint.snapshot.session
                || binding.run != current.checkpoint.snapshot.run
                || binding.step != step
            {
                return Err(SessionError::AuthorityDenied);
            }
            let ConnectorConsentStatus::Granted(grant) = self
                .host
                .load_connector_consent(&current.checkpoint.owner, &accepted.record)?
            else {
                return Err(SessionError::AuthorityDenied);
            };
            if grant.binding() != binding
                || !matches!(
                    self.host.live_connector_consent(&grant)?,
                    ConnectorConsentStatus::Granted(current) if current.as_ref() == grant.as_ref()
                )
            {
                return Err(SessionError::AuthorityDenied);
            }
            if self.host.now_ms()? >= binding.expires_at_ms {
                self.fail(&current, SessionState::Failed)?;
                return Err(SessionError::ExpiredInteraction);
            }
            let material = self.connector_resume_material(&current.checkpoint.owner, binding)?;
            runtime.preflight_retained_consent_binding(&current.checkpoint, binding, &material)?;
            // Historical-data preflight may take time or observe revocation.
            // Recheck the exact live grant before exposing a resume allowance.
            if !matches!(
                self.host.live_connector_consent(&grant)?,
                ConnectorConsentStatus::Granted(live) if live.as_ref() == grant.as_ref()
            ) {
                return Err(SessionError::AuthorityDenied);
            }
            if self.host.now_ms()? >= binding.expires_at_ms {
                self.fail(&current, SessionState::Failed)?;
                return Err(SessionError::ExpiredInteraction);
            }
            let mut resources =
                self.phase_resources(&current, &step, SessionResourcePhase::ApprovedResume)?;
            runtime.resume_approved_step(
                &current.checkpoint,
                &step,
                &mut resources,
                binding,
                &material,
            )?
        } else {
            let pending = PendingId::new(format!(
                "connector-consent-{}-{}",
                current.checkpoint.snapshot.run.as_str(),
                next_revision.value()
            ))?;
            let expires = now
                .checked_add(300_000)
                .ok_or(SessionError::LimitExceeded)?
                .min(current.checkpoint.budget.deadline_ms);
            let mut resources =
                self.phase_resources(&current, &step, SessionResourcePhase::Preparation)?;
            let preparation = runtime.prepare_authorized_step(
                &current.checkpoint,
                &step,
                &mut resources,
                ConnectorPreparationContext {
                    revision: next_revision,
                    pending: &pending,
                    dispatch: &default_dispatch,
                    expires_at_ms: expires,
                },
            );
            let preparation = match preparation {
                Ok(preparation) => preparation,
                Err(error) => {
                    // Retain the admitted upper bound after an attempted model
                    // call, even when usage is absent. Recovery never repeats it.
                    self.fail(&current, SessionState::Failed)?;
                    return Err(error);
                }
            };
            match preparation {
                ConnectorPreparation::GoalAlreadySatisfied(candidate) => {
                    let now = self.host.now_ms()?;
                    if now >= current.checkpoint.budget.deadline_ms
                        || current
                            .checkpoint
                            .lease_until_ms
                            .is_none_or(|until| now >= until)
                    {
                        return self.fail(&current, SessionState::Failed);
                    }
                    let (reference, record) = candidate.into_record(&current.checkpoint, now)?;
                    let mut next = current.checkpoint.clone();
                    next.snapshot.state = SessionState::Runnable;
                    next.snapshot.dispatch_knowledge = DispatchKnowledge::Prepared;
                    next.lease_until_ms = None;
                    next.pre_execution_goal = Some(reference);
                    let committed = self.append(&current, None, next, vec![record])?;
                    return self.verify_pre_execution_goal(committed);
                }
                ConnectorPreparation::Ready(prepared) => *prepared,
                ConnectorPreparation::ConsentRequired { binding, records } => {
                    if self.host.now_ms()? >= expires {
                        self.fail(&current, SessionState::Failed)?;
                        return Err(SessionError::ExpiredInteraction);
                    }
                    binding.validate()?;
                    if binding.owner != current.checkpoint.owner
                        || binding.scope != *goal.scope()
                        || binding.session != current.checkpoint.snapshot.session
                        || binding.run != current.checkpoint.snapshot.run
                        || binding.issued_revision != next_revision
                        || binding.pending != pending
                        || binding.step != step
                        || binding.dispatch != default_dispatch
                        || binding.expires_at_ms != expires
                        || binding.issuer != current.checkpoint.owner.principal
                    {
                        return Err(SessionError::AuthorityDenied);
                    }
                    for pin in &binding.resume.producer_pins {
                        let inputs = records
                            .iter()
                            .filter(|record| {
                                record.kind == RecordKind::ConnectorProducerInput
                                    && &record.reference == pin
                            })
                            .collect::<Vec<_>>();
                        if inputs.len() != 1
                            || content_reference(pin.id().as_str(), &inputs[0].bytes)? != *pin
                        {
                            return Err(SessionError::InvalidInteraction);
                        }
                    }
                    if records.iter().any(|record| {
                        record.kind == RecordKind::ConnectorProducerInput
                            && !binding.resume.producer_pins.contains(&record.reference)
                    }) {
                        return Err(SessionError::InvalidInteraction);
                    }
                    let resume_records = records
                        .iter()
                        .filter(|record| {
                            record.kind == RecordKind::ConnectorProposal
                                && record.reference == binding.resume.output_record
                        })
                        .collect::<Vec<_>>();
                    if resume_records.len() != 1
                        || content_reference(
                            binding.resume.output_record.id().as_str(),
                            &resume_records[0].bytes,
                        )? != binding.resume.output_record
                    {
                        return Err(SessionError::InvalidInteraction);
                    }
                    let material: ConnectorConsentResumeMaterial =
                        serde_json::from_slice(&resume_records[0].bytes)
                            .map_err(|_| SessionError::InvalidInteraction)?;
                    material.matches(&binding.resume)?;
                    let mut next = current.checkpoint.clone();
                    next.snapshot.state = SessionState::PendingConsent;
                    next.snapshot.pending = Some(PendingRef {
                        id: pending,
                        issued_revision: next_revision,
                        expires_at_ms: expires,
                    });
                    next.snapshot.dispatch = Some(default_dispatch);
                    next.snapshot.dispatch_knowledge = DispatchKnowledge::Prepared;
                    next.pending = Some(PendingInteraction::ConnectorConsent(binding));
                    next.accepted_connector_consent = None;
                    next.lease_until_ms = None;
                    return Ok(self
                        .append(&current, command, next, records)?
                        .checkpoint
                        .snapshot);
                }
            }
        };
        if prepared.before_snapshot().scope() != goal.scope()
            || prepared
                .invocation()
                .prepared()
                .invocation()
                .data()
                .task
                .scope
                != *goal.scope()
            || prepared.verified_output().record().admitted_invocation
                != prepared.invocation().identity()
            || prepared.verified_output().output().data().invocation
                != prepared.invocation().identity()
        {
            return Err(SessionError::StaleRevision);
        }

        let dispatch = if let Some(accepted) = &current.checkpoint.accepted_connector_consent {
            let binding = &accepted.binding;
            if binding.owner != current.checkpoint.owner
                || binding.scope != *goal.scope()
                || binding.session != current.checkpoint.snapshot.session
                || binding.run != current.checkpoint.snapshot.run
                || binding.step != step
            {
                return Err(SessionError::AuthorityDenied);
            }
            let ConnectorConsentStatus::Granted(grant) = self
                .host
                .load_connector_consent(&current.checkpoint.owner, &accepted.record)?
            else {
                return Err(SessionError::AuthorityDenied);
            };
            if grant.binding() != binding {
                return Err(SessionError::AuthorityDenied);
            }
            if !matches!(
                self.host.live_connector_consent(&grant)?,
                ConnectorConsentStatus::Granted(current) if current.as_ref() == grant.as_ref()
            ) {
                return Err(SessionError::AuthorityDenied);
            }
            binding.dispatch.clone()
        } else {
            default_dispatch
        };
        // Preparation and consent revalidation can take time. Never dispatch
        // against the timestamp captured before a model call or consent check.
        let now = self.host.now_ms()?;
        if now >= current.checkpoint.budget.deadline_ms {
            return self.fail(&current, SessionState::Failed);
        }
        if current
            .checkpoint
            .accepted_connector_consent
            .as_ref()
            .is_some_and(|accepted| now >= accepted.binding.expires_at_ms)
        {
            return Err(SessionError::ExpiredInteraction);
        }
        let before_binding = Self::connector_binding(
            &current.checkpoint,
            next_revision,
            &step,
            &dispatch,
            prepared.invocation(),
        )?;
        let before_bytes = before_snapshot_record(&before_binding, prepared.before_snapshot())?;
        let before_ref =
            Self::record_reference("connector-before", &before_binding, &before_bytes)?;
        let mut next = current.checkpoint.clone();
        next.budget.reserve(now, false)?;
        next.snapshot.state = SessionState::Dispatching;
        next.snapshot.pending = None;
        next.pending = None;
        next.snapshot.dispatch = Some(dispatch.clone());
        next.snapshot.dispatch_knowledge = DispatchKnowledge::Reserved;
        next.lease_until_ms = Some(
            now.checked_add(30_000)
                .ok_or(SessionError::LimitExceeded)?
                .min(next.budget.deadline_ms),
        );
        next.connector_step_snapshots
            .push(ConnectorStepSnapshotRef {
                binding: before_binding,
                phase: ConnectorSnapshotPhase::Before,
                record: before_ref.clone(),
            });
        let reserved = self.append(
            &current,
            command,
            next,
            vec![RecordWrite {
                reference: before_ref,
                kind: RecordKind::ConnectorSnapshot,
                bytes: before_bytes,
            }],
        )?;

        let reserved_binding = reserved
            .checkpoint
            .connector_step_snapshots
            .last()
            .ok_or(SessionError::InvalidState)?
            .binding
            .clone();
        let mut execution_resources =
            self.phase_resources(&reserved, &step, SessionResourcePhase::Execution)?;
        let mut recovery_resources = self.phase_resources(
            &reserved,
            &step,
            SessionResourcePhase::AfterSnapshotRecovery,
        )?;
        let turn = match runtime.execute_step(
            &reserved.checkpoint,
            &step,
            &mut execution_resources,
            &dispatch,
            &prepared,
        ) {
            Ok(turn) => turn,
            Err(_) => {
                let after = runtime
                    .capture_after_snapshot(
                        &reserved.checkpoint,
                        &step,
                        &mut recovery_resources,
                        &dispatch,
                        &prepared,
                    )
                    .ok();
                return self.mark_connector_unknown(
                    &reserved,
                    &reserved_binding,
                    "runtime_outcome_unknown",
                    after,
                );
            }
        };
        if turn.validate(&goal, &step, &prepared).is_err() {
            let after = runtime
                .capture_after_snapshot(
                    &reserved.checkpoint,
                    &step,
                    &mut recovery_resources,
                    &dispatch,
                    &prepared,
                )
                .ok();
            return self.mark_connector_unknown(
                &reserved,
                &reserved_binding,
                "invalid_runtime_result",
                after,
            );
        }
        let result_revision = reserved.checkpoint.snapshot.revision.next()?;
        let binding = Self::connector_binding(
            &reserved.checkpoint,
            result_revision,
            &step,
            &dispatch,
            prepared.invocation(),
        )?;
        let batch_bytes = proposal_batch_record(&binding, &turn.proposal_batch)?;
        let batch_ref = Self::record_reference("connector-result", &binding, &batch_bytes)?;
        let effect = aggregate_proposal_effect(&turn.proposal_batch);
        let after_payload = if let Some(after) = &turn.after_snapshot {
            Some(after.clone())
        } else if effect != ConnectorEffect::NotDispatched {
            runtime
                .capture_after_snapshot(
                    &reserved.checkpoint,
                    &step,
                    &mut recovery_resources,
                    &dispatch,
                    &prepared,
                )
                .ok()
        } else {
            None
        };
        // Capture scope, JSON shape and serialized size are all checked here.
        // Invalid/unavailable capture becomes an explicit Missing status and
        // can never leave a successful-looking runnable checkpoint.
        let after_record =
            after_payload.and_then(|after| after_snapshot_record(&binding, &after).ok());
        let mut next = reserved.checkpoint.clone();
        next.connector_results.push(ConnectorResultRef {
            binding: binding.clone(),
            record: batch_ref.clone(),
            after_snapshot_state: if after_record.is_some() {
                ConnectorAfterSnapshotState::Captured
            } else if effect == ConnectorEffect::NotDispatched {
                ConnectorAfterSnapshotState::NotRequired
            } else {
                ConnectorAfterSnapshotState::Missing
            },
        });
        let mut records = vec![RecordWrite {
            reference: batch_ref,
            kind: RecordKind::ConnectorResult,
            bytes: batch_bytes,
        }];
        let mut record_bytes = 0usize;
        account_connector_step_record_bytes(&mut record_bytes, &records[0].bytes)?;
        if let Some(bytes) = after_record {
            account_connector_step_record_bytes(&mut record_bytes, &bytes)?;
            let reference = Self::record_reference("connector-after", &binding, &bytes)?;
            next.connector_step_snapshots
                .push(ConnectorStepSnapshotRef {
                    binding: binding.clone(),
                    phase: ConnectorSnapshotPhase::After,
                    record: reference.clone(),
                });
            records.push(RecordWrite {
                reference,
                kind: RecordKind::ConnectorSnapshot,
                bytes,
            });
        }
        for readback in &turn.independent_readbacks {
            let result = turn
                .proposal_batch
                .results
                .iter()
                .find(|result| result.ordinal == readback.proposal_ordinal as usize)
                .ok_or(SessionError::InvalidInteraction)?;
            let bytes = encode_connector_readback(
                &binding,
                readback.proposal_ordinal,
                &result.request,
                &readback.read_request,
                &readback.observation,
                &readback.batch,
                &goal,
            )?;
            account_connector_step_record_bytes(&mut record_bytes, &bytes)?;
            let reference = Self::record_reference("connector-readback", &binding, &bytes)?;
            next.connector_observations.push(ConnectorObservationRef {
                binding: binding.clone(),
                proposal_ordinal: Some(readback.proposal_ordinal),
                goal_condition: None,
                role: ConnectorObservationRole::PostEffectReadback,
                record: reference.clone(),
            });
            records.push(RecordWrite {
                reference,
                kind: RecordKind::ConnectorObservation,
                bytes,
            });
        }
        for readback in &turn.goal_verification_readbacks {
            let bytes = encode_goal_verification_input(
                &binding,
                GoalVerificationReadbackInput {
                    condition: &readback.condition,
                    read_request: &readback.read_request,
                    observation: &readback.observation,
                    batch: &readback.batch,
                },
                &goal,
            )?;
            account_connector_step_record_bytes(&mut record_bytes, &bytes)?;
            let reference = Self::record_reference("connector-goal-readback", &binding, &bytes)?;
            next.connector_observations.push(ConnectorObservationRef {
                binding: binding.clone(),
                proposal_ordinal: None,
                goal_condition: Some(readback.condition.clone()),
                role: ConnectorObservationRole::GoalVerificationReadback,
                record: reference.clone(),
            });
            records.push(RecordWrite {
                reference,
                kind: RecordKind::ConnectorObservation,
                bytes,
            });
        }
        if records.len() > MAX_CONNECTOR_STEP_COMMIT_RECORDS {
            return Err(SessionError::LimitExceeded);
        }
        next.lease_until_ms = None;
        let incomplete = matches!(
            effect,
            ConnectorEffect::Acknowledged | ConnectorEffect::NoMutation
        ) && next.connector_results.last().is_some_and(|result| {
            result.after_snapshot_state != ConnectorAfterSnapshotState::Captured
        });
        let unknown = effect == ConnectorEffect::Unknown || incomplete;
        let denied = effect == ConnectorEffect::NotDispatched;
        if unknown {
            next.snapshot.state = SessionState::OutcomeUnknown;
            next.snapshot.dispatch_knowledge = DispatchKnowledge::Unknown;
        } else if denied {
            next.snapshot.state = SessionState::Failed;
            next.snapshot.dispatch_knowledge = DispatchKnowledge::Stopped;
        } else {
            next.snapshot.state = SessionState::Runnable;
            next.snapshot.dispatch_knowledge = DispatchKnowledge::Observed;
            // An exact-step approval is consumed by this observation. The next
            // step needs its own resource envelope and current consent decision.
            next.accepted_connector_consent = None;
        }
        let committed = self.append(&reserved, None, next, records)?;
        if unknown || denied {
            return Ok(committed.checkpoint.snapshot);
        }

        let complete = goal.steps().iter().all(|planned| {
            committed
                .checkpoint
                .connector_step_snapshots
                .iter()
                .any(|snapshot| {
                    snapshot.phase == ConnectorSnapshotPhase::After
                        && snapshot.binding.step == *planned
                })
        });
        if !complete {
            return Ok(committed.checkpoint.snapshot);
        }
        let final_refs = committed
            .checkpoint
            .connector_observations
            .iter()
            .filter(|r| {
                r.role == ConnectorObservationRole::GoalVerificationReadback
                    && r.binding.step == step
                    && r.binding.dispatch == dispatch
                    && r.binding.revision == committed.checkpoint.snapshot.revision
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut persisted = Vec::new();
        for reference in &final_refs {
            match decode_goal_verification_readback(
                &self.repository,
                &committed.checkpoint.owner,
                reference,
                &goal,
            ) {
                Ok(readback) => persisted.push(readback),
                Err(_) => return self.fail(&committed, SessionState::Failed),
            }
        }
        let readset =
            match build_goal_verification_readset(&committed.checkpoint, &step, &persisted) {
                Ok(readset) => readset,
                Err(_) => return self.fail(&committed, SessionState::Failed),
            };
        let Some(receipt) =
            runtime.verify_goal(&committed.checkpoint, &step, &dispatch, &persisted)?
        else {
            return self.fail(&committed, SessionState::Failed);
        };
        let expected_execution =
            gateway_domain::ReferenceId::new(format!("{}-execution-1", dispatch.as_str()))
                .map_err(|_| SessionError::InvalidInteraction)?;
        if receipt.scope() != goal.scope()
            || receipt.execution() != &expected_execution
            || receipt.outcome() != OutcomeClass::Success
            || receipt.facts().is_empty()
            || receipt.evidence().is_empty()
            || receipt.readset_digest() != Some(readset.digest())
            || receipt.goal_digest() != &crate::closed_loop::ClosedLoop::goal_digest(goal.intent())
        {
            return self.fail(&committed, SessionState::Failed);
        }
        let bytes = connector_goal_verification_evidence(&committed.checkpoint, &step, &receipt)?;
        let evidence = RecordWrite {
            reference: content_reference(
                &format!(
                    "connector-verification-{}",
                    &receipt.source_digest().as_str()[..24]
                ),
                &bytes,
            )?,
            kind: RecordKind::Evidence,
            bytes,
        };
        let mut verified = committed.checkpoint.clone();
        verified.snapshot.state = SessionState::Completed;
        verified.snapshot.dispatch_knowledge = DispatchKnowledge::Verified;
        verified.snapshot.final_evidence = Some(evidence.reference.clone());
        Ok(self
            .append(&committed, None, verified, vec![evidence])?
            .checkpoint
            .snapshot)
    }

    fn continue_task(
        &self,
        current: FencedSession,
        command: CommandId,
        _now: u64,
    ) -> Result<SessionSnapshot, SessionError> {
        if current.checkpoint.goal.as_connector().is_some() {
            return self.continue_connector_task(current, command, _now);
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
    fn verify_pre_execution_goal(
        &self,
        current: FencedSession,
    ) -> Result<SessionSnapshot, SessionError> {
        if current.checkpoint.snapshot.state != SessionState::Runnable
            || current.checkpoint.pre_execution_goal.is_none()
        {
            return Err(SessionError::InvalidState);
        }
        let now = self.host.now_ms()?;
        if now >= current.checkpoint.budget.deadline_ms {
            return self.fail(&current, SessionState::Failed);
        }
        let data = match load_pre_execution_goal(&self.repository, &current.checkpoint, now) {
            Ok(data) => data,
            Err(_) => return self.fail(&current, SessionState::Failed),
        };
        let reference = current.checkpoint.pre_execution_goal.as_ref().unwrap();
        let Some(receipt) = self
            .host
            .connector_runtime()?
            .verify_pre_execution_goal(&current.checkpoint, &data)?
        else {
            return self.fail(&current, SessionState::Failed);
        };
        if !super::pre_execution_goal::receipt_matches(&receipt, reference, data.readset())? {
            return self.fail(&current, SessionState::Failed);
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
            return self.fail(&current, SessionState::Failed);
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
            .append(&current, None, next, vec![evidence])?
            .checkpoint
            .snapshot)
    }
    fn fail(
        &self,
        current: &FencedSession,
        state: SessionState,
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
            .append(current, None, next, vec![])?
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
                Some(PendingInteraction::ConnectorConsent(_))
            ) {
                // Connector consent pins retained verified proposals. Expiry
                // cannot rebuild a proposal through artifact/model preparation
                // or replenish the already-consumed preparation allowance.
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
        if current.checkpoint.pre_execution_goal.is_some()
            && current.checkpoint.snapshot.state == SessionState::Runnable
        {
            return self.verify_pre_execution_goal(current);
        }
        if current.checkpoint.goal.as_connector().is_some()
            && current.checkpoint.snapshot.state == SessionState::Runnable
        {
            let goal = current
                .checkpoint
                .goal
                .as_connector()
                .ok_or(SessionError::InvalidState)?;
            let complete =
                goal.steps().iter().all(|planned| {
                    current.checkpoint.connector_step_snapshots.iter().any(|s| {
                        s.phase == ConnectorSnapshotPhase::After && s.binding.step == *planned
                    })
                });
            if complete {
                return self.verify_persisted_connector_goal(current);
            }
            return Ok(current.checkpoint.snapshot);
        }
        if current.checkpoint.snapshot.state == SessionState::Preparing {
            if current
                .checkpoint
                .lease_until_ms
                .is_some_and(|until| now < until)
            {
                return Err(SessionError::InvalidState);
            }
            // A model/read preparation may have consumed its entire allowance,
            // but no connector mutation was released. Stop without repeating it.
            return self.fail(&current, SessionState::Failed);
        }
        if !matches!(
            current.checkpoint.snapshot.state,
            SessionState::Dispatching | SessionState::Cancelling | SessionState::OutcomeUnknown
        ) {
            return Ok(current.checkpoint.snapshot);
        }
        if current.checkpoint.snapshot.state == SessionState::OutcomeUnknown {
            // Connector uncertainty is terminal until an explicit reconciliation
            // contract supplies new trusted evidence. Inspect/recover never retries it.
            return Ok(current.checkpoint.snapshot);
        }
        if current
            .checkpoint
            .lease_until_ms
            .is_some_and(|until| now < until)
        {
            return Err(SessionError::InvalidState);
        }
        if current.checkpoint.goal.as_connector().is_some() {
            let dispatch = current
                .checkpoint
                .snapshot
                .dispatch
                .as_ref()
                .ok_or(SessionError::InvalidState)?;
            let binding = current
                .checkpoint
                .connector_step_snapshots
                .iter()
                .rev()
                .find(|snapshot| {
                    snapshot.phase == ConnectorSnapshotPhase::Before
                        && &snapshot.binding.dispatch == dispatch
                })
                .map(|snapshot| snapshot.binding.clone())
                .ok_or(SessionError::InvalidInteraction)?;
            return self.mark_connector_unknown(
                &current,
                &binding,
                "recovery_after_dispatch_reservation",
                None,
            );
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

    fn verify_persisted_connector_goal(
        &self,
        current: FencedSession,
    ) -> Result<SessionSnapshot, SessionError> {
        let goal = current
            .checkpoint
            .goal
            .as_connector()
            .ok_or(SessionError::InvalidState)?;
        let last_step = goal
            .steps()
            .last()
            .ok_or(SessionError::InvalidInteraction)?;
        let refs = current
            .checkpoint
            .connector_observations
            .iter()
            .filter(|r| {
                r.role == ConnectorObservationRole::GoalVerificationReadback
                    && r.binding.step == *last_step
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut readbacks = Vec::new();
        for reference in &refs {
            match decode_goal_verification_readback(
                &self.repository,
                &current.checkpoint.owner,
                reference,
                goal,
            ) {
                Ok(readback) => readbacks.push(readback),
                Err(_) => return self.fail(&current, SessionState::Failed),
            }
        }
        let readset =
            match build_goal_verification_readset(&current.checkpoint, last_step, &readbacks) {
                Ok(readset) => readset,
                Err(_) => return self.fail(&current, SessionState::Failed),
            };
        let dispatch = refs
            .first()
            .ok_or(SessionError::InvalidInteraction)?
            .binding
            .dispatch
            .clone();
        let binding = refs
            .first()
            .ok_or(SessionError::InvalidInteraction)?
            .binding
            .clone();
        let has_result = current
            .checkpoint
            .connector_results
            .iter()
            .any(|r| r.binding == binding);
        let has_after_snapshot = current
            .checkpoint
            .connector_step_snapshots
            .iter()
            .any(|r| r.phase == ConnectorSnapshotPhase::After && r.binding == binding);
        if !has_result
            || !has_after_snapshot
            || refs
                .iter()
                .any(|r| r.binding.dispatch != dispatch || r.binding != binding)
        {
            return self.fail(&current, SessionState::Failed);
        }
        let runtime = self.host.connector_runtime()?;
        let Some(receipt) =
            runtime.verify_goal(&current.checkpoint, last_step, &dispatch, &readbacks)?
        else {
            return self.fail(&current, SessionState::Failed);
        };
        let execution =
            gateway_domain::ReferenceId::new(format!("{}-execution-1", dispatch.as_str()))
                .map_err(|_| SessionError::InvalidInteraction)?;
        if receipt.scope() != goal.scope()
            || receipt.execution() != &execution
            || receipt.outcome() != OutcomeClass::Success
            || receipt.facts().is_empty()
            || receipt.evidence().is_empty()
            || receipt.readset_digest() != Some(readset.digest())
            || receipt.goal_digest() != &crate::closed_loop::ClosedLoop::goal_digest(goal.intent())
        {
            return self.fail(&current, SessionState::Failed);
        }
        let bytes = connector_goal_verification_evidence(&current.checkpoint, last_step, &receipt)?;
        let evidence = RecordWrite {
            reference: content_reference(
                &format!(
                    "connector-verification-{}",
                    &receipt.source_digest().as_str()[..24]
                ),
                &bytes,
            )?,
            kind: RecordKind::Evidence,
            bytes,
        };
        let mut verified = current.checkpoint.clone();
        verified.snapshot.state = SessionState::Completed;
        verified.snapshot.dispatch_knowledge = DispatchKnowledge::Verified;
        verified.snapshot.final_evidence = Some(evidence.reference.clone());
        Ok(self
            .append(&current, None, verified, vec![evidence])?
            .checkpoint
            .snapshot)
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
            let (goal, budget) = self.host.register(owner, intent, execution)?;
            budget.validate()?;
            let resource_ledger = self
                .host
                .resource_limits(owner, &goal)?
                .map(SessionResourceLedger::new)
                .transpose()?;
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
                resource_ledger,
                pending: None,
                accepted_consent: None,
                accepted_connector_consent: None,
                authority_events: vec![],
                selected_source: None,
                artifact: None,
                lease_until_ms: None,
                initial_assessment: None,
                connector_step_snapshots: vec![],
                connector_results: vec![],
                connector_observations: vec![],
                pre_execution_goal: None,
            };
            initial.initial_assessment = Some(match initial.goal.as_artifact() {
                Some(_) => {
                    let prepared = self.host.prepare(&initial)?;
                    assess_initial_goal(&initial, &prepared.input, &prepared.authority)?
                }
                None => self
                    .host
                    .connector_runtime()?
                    .initial_assessment(&initial)?,
            });
            if initial
                .goal
                .as_artifact()
                .is_some_and(|goal| !goal.basis().sources.is_empty())
            {
                self.question(&mut initial, Revision::new(1)?, now)?;
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
            SessionCommand::ApproveConnector {
                at,
                pending,
                consent,
            } => {
                let Some(PendingInteraction::ConnectorConsent(binding)) = &next.pending else {
                    return Err(SessionError::InvalidInteraction);
                };
                let pending_ref = next
                    .snapshot
                    .pending
                    .as_ref()
                    .ok_or(SessionError::InvalidInteraction)?;
                pending_ref.check(&pending, at.expected_revision, self.host.now_ms()?)?;
                binding.validate()?;
                if binding.pending != pending
                    || binding.owner != *owner
                    || binding.issued_revision != pending_ref.issued_revision
                    || binding.expires_at_ms != pending_ref.expires_at_ms
                {
                    return Err(SessionError::InvalidInteraction);
                }
                let ConnectorConsentStatus::Granted(grant) =
                    self.host.load_connector_consent(owner, &consent)?
                else {
                    return Err(SessionError::AuthorityDenied);
                };
                if grant.record() != &consent || grant.binding() != binding.as_ref() {
                    return Err(SessionError::AuthorityDenied);
                }
                let runtime = self.host.connector_runtime()?;
                let material = self.connector_resume_material(owner, binding)?;
                runtime.preflight_retained_consent_binding(&next, binding, &material)?;
                let live = self.host.live_connector_consent(&grant)?;
                if !matches!(live, ConnectorConsentStatus::Granted(current) if current.as_ref() == grant.as_ref())
                {
                    return Err(SessionError::AuthorityDenied);
                }
                if self.host.now_ms()? >= binding.expires_at_ms {
                    return Err(SessionError::ExpiredInteraction);
                }
                next.accepted_connector_consent = Some(AcceptedConnectorConsent {
                    record: consent,
                    binding: binding.as_ref().clone(),
                });
                next.pending = None;
                next.snapshot.pending = None;
                next.snapshot.state = SessionState::Runnable;
                // Keep the prepared dispatch reservation intact. Approval does
                // not invoke the connector or advance dispatch knowledge.
                Ok(self
                    .append(&current, Some(at.command), next, vec![])?
                    .checkpoint
                    .snapshot)
            }
            SessionCommand::Continue { at } => self.continue_task(current, at.command, now),
            SessionCommand::Cancel { at } => {
                if next.goal.as_connector().is_some()
                    && next.snapshot.state == SessionState::Dispatching
                {
                    // The connector runtime may already have crossed its effect
                    // boundary. Keep the reservation unresolved until recovery
                    // records an explicit Unknown outcome; never report cancellation
                    // as if it proved that no external change happened.
                    next.snapshot.state = SessionState::Cancelling;
                } else {
                    // The pure compiler cannot release an external result after
                    // this append, so cancellation remains a terminal stop.
                    next.snapshot.state = SessionState::Cancelled;
                    next.pending = None;
                    next.snapshot.pending = None;
                    next.lease_until_ms = None;
                    if next.snapshot.dispatch.is_some() {
                        next.snapshot.dispatch_knowledge = DispatchKnowledge::Stopped;
                    }
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
