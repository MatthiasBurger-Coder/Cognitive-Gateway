//! Transactional storage responsibilities owned by #276. The port specifies
//! acknowledgement/fencing semantics; it is not an in-memory recovery substitute.
use super::*;
use gateway_domain::{PlanStepId, TaskId, prompt_ir::InvocationIrIdentity};

/// Immutable identity shared by the before/after snapshots and observations of
/// one connector step. The project is part of authenticated `owner`; scope is
/// recorded separately because it is the domain authorization boundary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorStepBinding {
    pub owner: OwnerBinding,
    pub scope: gateway_domain::ContextScopeId,
    pub session: SessionId,
    pub run: RunId,
    /// Session revision at which this immutable record was captured.
    pub revision: Revision,
    pub task: TaskId,
    #[serde(with = "connector_step_id")]
    pub step: PlanStepId,
    pub dispatch: DispatchId,
    pub invocation: InvocationIrIdentity,
}

mod connector_step_id {
    use super::*;
    use serde::Deserialize;

    pub fn serialize<S: serde::Serializer>(
        value: &PlanStepId,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(value.as_str())
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<PlanStepId, D::Error> {
        let value = String::deserialize(deserializer)?;
        PlanStepId::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorSnapshotPhase {
    Before,
    After,
}

/// Owner-scoped immutable snapshot record pointer. Bytes are stored separately
/// through `RecordWrite` and loaded through the existing artifact storage port.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorStepSnapshotRef {
    pub binding: ConnectorStepBinding,
    pub phase: ConnectorSnapshotPhase,
    pub record: RecordRef,
}

/// Immutable serialized ordered proposal-batch outcome for one connector step.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorResultRef {
    pub binding: ConnectorStepBinding,
    pub record: RecordRef,
    /// Explicit capture state for the step's independent after snapshot. The
    /// legacy default is Unknown so old checkpoints never imply completeness.
    #[serde(default)]
    pub after_snapshot_state: ConnectorAfterSnapshotState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorAfterSnapshotState {
    Captured,
    Missing,
    NotRequired,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorObservationRole {
    PostEffectReadback,
    GoalVerificationReadback,
}

/// Connector observations are their own immutable records and are never the
/// independent goal-verification receipt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorObservationRef {
    pub binding: ConnectorStepBinding,
    /// Present only when the observation follows a specific dispatched proposal.
    #[serde(default)]
    pub proposal_ordinal: Option<u32>,
    /// Present only for a final goal-condition read. This is an observation
    /// input, never a `VerifiedOutcomeReceipt` or final Evidence record.
    #[serde(default)]
    pub goal_condition: Option<gateway_domain::ConditionId>,
    pub role: ConnectorObservationRole,
    pub record: RecordRef,
}

impl ConnectorStepBinding {
    fn validate_for(&self, checkpoint: &SessionCheckpoint) -> Result<(), SessionError> {
        let connector_goal = checkpoint
            .goal
            .as_connector()
            .ok_or(SessionError::UnsupportedCapability)?;
        if self.owner != checkpoint.owner
            || self.scope != *connector_goal.scope()
            || !connector_goal.steps().contains(&self.step)
            || self.session != checkpoint.snapshot.session
            || self.run != checkpoint.snapshot.run
            || self.revision.value() == 0
            || self.revision > checkpoint.snapshot.revision
            || self.invocation.schema_version != gateway_domain::prompt_ir::INVOCATION_IR_VERSION
        {
            return Err(SessionError::InvalidInteraction);
        }
        Ok(())
    }

    fn same_step(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.scope == other.scope
            && self.session == other.session
            && self.run == other.run
            && self.task == other.task
            && self.step == other.step
            && self.dispatch == other.dispatch
            && self.invocation == other.invocation
    }
}

/// Storage-owned fencing generation. A lost/ambiguous commit cannot be retried
/// by guessing a token or reusing transport connection identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FenceToken(u64);
impl FenceToken {
    pub fn new(value: u64) -> Result<Self, SessionError> {
        if value == 0 || value > MAX_REVISION {
            return Err(SessionError::InvalidInput);
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingInteraction {
    Source(SourceQuestion),
    Consent(Box<ActionBinding>),
    ConnectorConsent(Box<ConnectorConsentBinding>),
}

/// CG-14's initial exact-goal verdict and the trusted input authority it assessed.
/// It is retained history, never an authorization or completed-result receipt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialAssessment {
    pub goal: RecordRef,
    pub authority: RecordRef,
    pub outcome: String,
}

/// The recovery adapter persists each field in its own bounded/versioned format.
/// Rebuilding this typed checkpoint requires the supported-goal validator, full
/// pending payload and consumed budgets, not the redacted CG-14 diagnostic JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCheckpoint {
    pub owner: OwnerBinding,
    pub goal: SupportedSessionGoal,
    pub execution: RequestedExecution,
    pub snapshot: SessionSnapshot,
    pub budget: SessionBudget,
    /// Immutable admission and conservative whole-step charges, persisted before
    /// resource-consuming preparation. Absent on legacy artifact sessions.
    pub resource_ledger: Option<SessionResourceLedger>,
    pub pending: Option<PendingInteraction>,
    pub accepted_consent: Option<VerifiedConsent>,
    pub accepted_connector_consent: Option<AcceptedConnectorConsent>,
    pub authority_events: Vec<AuthorityEvent>,
    pub selected_source: Option<RecordRef>,
    pub artifact: Option<RecordRef>,
    pub lease_until_ms: Option<u64>,
    pub initial_assessment: Option<InitialAssessment>,
    /// Append-only references to immutable connector step snapshots.
    pub connector_step_snapshots: Vec<ConnectorStepSnapshotRef>,
    /// Append-only references to serialized ordered ProposalBatch results.
    pub connector_results: Vec<ConnectorResultRef>,
    /// Append-only connector observations, distinct from final goal evidence.
    pub connector_observations: Vec<ConnectorObservationRef>,
    /// Invocation-free read evidence, pending pure verification while Runnable.
    pub pre_execution_goal: Option<PreExecutionGoalRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityEvent {
    Denied {
        pending: PendingId,
        revision: Revision,
    },
    Withdrawn {
        record: ConsentRecordRef,
        revision: Revision,
    },
    ConnectorWithdrawn {
        record: ConnectorConsentRecordRef,
        revision: Revision,
    },
}

impl SessionCheckpoint {
    pub fn validate(&self) -> Result<(), SessionError> {
        self.snapshot.validate()?;
        self.budget.validate()?;
        if let Some(ledger) = &self.resource_ledger {
            ledger.validate()?;
            for reservation in ledger.reservations() {
                let binding = reservation.binding();
                let registered_step = match self.goal.as_connector() {
                    Some(goal) => goal.steps().contains(&binding.step),
                    None => self
                        .goal
                        .as_artifact()
                        .is_some_and(|goal| goal.basis().step == binding.step),
                };
                if binding.owner != self.owner
                    || binding.scope != *self.goal.scope()
                    || binding.session != self.snapshot.session
                    || binding.run != self.snapshot.run
                    || binding.revision > self.snapshot.revision
                    || !registered_step
                {
                    return Err(SessionError::InvalidInteraction);
                }
            }
        }
        if self.snapshot.state == SessionState::Preparing
            && (self.goal.as_connector().is_none()
                || self
                    .lease_until_ms
                    .is_none_or(|until| until == 0 || until > self.budget.deadline_ms)
                || self.resource_ledger.as_ref().is_none_or(|ledger| {
                    !ledger.reservations().iter().any(|reservation| {
                        self.snapshot.dispatch.as_ref() == Some(&reservation.binding().dispatch)
                            && (reservation.binding().revision == self.snapshot.revision
                                || self.accepted_connector_consent.as_ref().is_some_and(
                                    |accepted| {
                                        accepted.binding.dispatch == reservation.binding().dispatch
                                            && accepted.binding.step == reservation.binding().step
                                            && reservation.binding().revision
                                                < accepted.binding.issued_revision
                                            && accepted.binding.issued_revision
                                                < self.snapshot.revision
                                    },
                                ))
                    })
                })
                || self.connector_step_snapshots.iter().any(|snapshot| {
                    self.snapshot.dispatch.as_ref() == Some(&snapshot.binding.dispatch)
                }))
        {
            return Err(SessionError::InvalidState);
        }
        if let Some(reference) = &self.pre_execution_goal {
            reference.validate_for(self)?;
            if !matches!(
                self.snapshot.state,
                SessionState::Runnable
                    | SessionState::Completed
                    | SessionState::Failed
                    | SessionState::Cancelled
            ) || self.pending.is_some()
                || self.accepted_connector_consent.is_some()
                || self.accepted_consent.is_some()
                || self.lease_until_ms.is_some()
                || self
                    .connector_results
                    .iter()
                    .any(|r| r.binding.dispatch == reference.binding.acquisition.dispatch)
                || self
                    .connector_step_snapshots
                    .iter()
                    .any(|r| r.binding.dispatch == reference.binding.acquisition.dispatch)
                || self
                    .connector_observations
                    .iter()
                    .any(|r| r.binding.dispatch == reference.binding.acquisition.dispatch)
                || (self.snapshot.state == SessionState::Runnable
                    && self.snapshot.dispatch_knowledge != DispatchKnowledge::Prepared)
            {
                return Err(SessionError::InvalidState);
            }
        }
        if let Some(assessment) = &self.initial_assessment {
            if assessment.goal
                != content_reference(
                    "supported-goal",
                    &serde_json::to_vec(self.goal.intent())
                        .map_err(|_| SessionError::InvalidInput)?,
                )?
                || !["UNKNOWN", "INSUFFICIENT_EVIDENCE", "UNRESOLVED_INPUT"]
                    .contains(&assessment.outcome.as_str())
            {
                return Err(SessionError::InvalidState);
            }
        }
        if self.authority_events.len() > 4096 {
            return Err(SessionError::LimitExceeded);
        }
        if self.connector_step_snapshots.len()
            + self.connector_results.len()
            + self.connector_observations.len()
            > 4096
        {
            return Err(SessionError::LimitExceeded);
        }
        let mut record_refs = Vec::with_capacity(
            self.connector_step_snapshots.len()
                + self.connector_results.len()
                + self.connector_observations.len(),
        );
        for (index, snapshot) in self.connector_step_snapshots.iter().enumerate() {
            snapshot.binding.validate_for(self)?;
            if self.resource_ledger.as_ref().is_some_and(|ledger| {
                !ledger.reservations().iter().any(|reservation| {
                    reservation.binding().dispatch == snapshot.binding.dispatch
                        && reservation.binding().step == snapshot.binding.step
                        && reservation.binding().revision < snapshot.binding.revision
                })
            }) {
                return Err(SessionError::InvalidInteraction);
            }
            if record_refs.contains(&snapshot.record)
                || self.artifact.as_ref() == Some(&snapshot.record)
                || self.snapshot.final_evidence.as_ref() == Some(&snapshot.record)
                || self.connector_step_snapshots[..index].iter().any(|prior| {
                    prior.binding.same_step(&snapshot.binding) && prior.phase == snapshot.phase
                })
            {
                return Err(SessionError::InvalidInteraction);
            }
            record_refs.push(snapshot.record.clone());
            if snapshot.phase == ConnectorSnapshotPhase::After
                && !self.connector_step_snapshots.iter().any(|prior| {
                    prior.phase == ConnectorSnapshotPhase::Before
                        && prior.binding.same_step(&snapshot.binding)
                        && prior.binding.revision < snapshot.binding.revision
                })
            {
                return Err(SessionError::InvalidInteraction);
            }
        }
        for result in &self.connector_results {
            result.binding.validate_for(self)?;
            if record_refs.contains(&result.record)
                || self.artifact.as_ref() == Some(&result.record)
                || self.snapshot.final_evidence.as_ref() == Some(&result.record)
                || !self.connector_step_snapshots.iter().any(|snapshot| {
                    snapshot.phase == ConnectorSnapshotPhase::Before
                        && snapshot.binding.same_step(&result.binding)
                        && snapshot.binding.revision <= result.binding.revision
                })
            {
                return Err(SessionError::InvalidInteraction);
            }
            let matching_after = self.connector_step_snapshots.iter().any(|snapshot| {
                snapshot.phase == ConnectorSnapshotPhase::After
                    && snapshot.binding.same_step(&result.binding)
                    && snapshot.binding.revision == result.binding.revision
            });
            match result.after_snapshot_state {
                ConnectorAfterSnapshotState::Captured if !matching_after => {
                    return Err(SessionError::InvalidInteraction);
                }
                ConnectorAfterSnapshotState::Missing | ConnectorAfterSnapshotState::NotRequired
                    if matching_after =>
                {
                    return Err(SessionError::InvalidInteraction);
                }
                ConnectorAfterSnapshotState::Captured
                | ConnectorAfterSnapshotState::Missing
                | ConnectorAfterSnapshotState::NotRequired
                | ConnectorAfterSnapshotState::Unknown => {}
            }
            record_refs.push(result.record.clone());
        }
        for (index, observation) in self.connector_observations.iter().enumerate() {
            observation.binding.validate_for(self)?;
            let role_link_valid = match observation.role {
                ConnectorObservationRole::PostEffectReadback => {
                    observation.proposal_ordinal.is_some() && observation.goal_condition.is_none()
                }
                ConnectorObservationRole::GoalVerificationReadback => {
                    observation.proposal_ordinal.is_none()
                        && observation
                            .goal_condition
                            .as_ref()
                            .is_some_and(|condition| {
                                self.goal.as_connector().is_some_and(|goal| {
                                    goal.observations()
                                        .iter()
                                        .any(|mapping| mapping.condition() == condition)
                                        && goal.steps().last() == Some(&observation.binding.step)
                                })
                            })
                }
            };
            if record_refs.contains(&observation.record)
                || self.artifact.as_ref() == Some(&observation.record)
                || self.snapshot.final_evidence.as_ref() == Some(&observation.record)
                || !role_link_valid
                || self.connector_observations[..index].iter().any(|prior| {
                    prior.proposal_ordinal == observation.proposal_ordinal
                        && prior.goal_condition == observation.goal_condition
                        && prior.role == observation.role
                        && prior.binding.same_step(&observation.binding)
                })
                || !self.connector_results.iter().any(|result| {
                    result.binding.same_step(&observation.binding)
                        && result.binding.revision <= observation.binding.revision
                })
                || !self.connector_step_snapshots.iter().any(|snapshot| {
                    snapshot.phase == ConnectorSnapshotPhase::Before
                        && snapshot.binding.same_step(&observation.binding)
                        && snapshot.binding.revision <= observation.binding.revision
                })
            {
                return Err(SessionError::InvalidInteraction);
            }
            record_refs.push(observation.record.clone());
        }
        match (&self.pending, self.snapshot.state, &self.snapshot.pending) {
            (
                Some(PendingInteraction::Source(question)),
                SessionState::PendingClarification,
                Some(reference),
            ) => {
                if &question.pending != reference
                    || question.alternatives.is_empty()
                    || question.alternatives.len() > 256
                    || self
                        .goal
                        .as_artifact()
                        .is_none_or(|goal| question.basis != goal.basis().projection)
                    || question.alternatives.iter().any(|r| {
                        self.goal
                            .as_artifact()
                            .is_none_or(|goal| !goal.basis().sources.contains(r))
                    })
                    || question
                        .alternatives
                        .iter()
                        .enumerate()
                        .any(|(i, r)| question.alternatives[..i].contains(r))
                {
                    return Err(SessionError::InvalidInteraction);
                }
            }
            (
                Some(PendingInteraction::Consent(binding)),
                SessionState::PendingConsent,
                Some(reference),
            ) => {
                if binding.owner != self.owner
                    || binding.session != self.snapshot.session
                    || binding.run != self.snapshot.run
                    || binding.pending != reference.id
                    || binding.issued_revision != reference.issued_revision
                    || binding.expires_at_ms != reference.expires_at_ms
                    || self.snapshot.dispatch.as_ref() != Some(&binding.dispatch)
                {
                    return Err(SessionError::InvalidInteraction);
                }
            }
            (
                Some(PendingInteraction::ConnectorConsent(binding)),
                SessionState::PendingConsent,
                Some(reference),
            ) => {
                binding.validate()?;
                if binding.owner != self.owner
                    || binding.session != self.snapshot.session
                    || binding.run != self.snapshot.run
                    || binding.pending != reference.id
                    || binding.issued_revision != reference.issued_revision
                    || binding.expires_at_ms != reference.expires_at_ms
                    || self.snapshot.dispatch.as_ref() != Some(&binding.dispatch)
                    || self.goal.as_connector().is_none_or(|goal| {
                        goal.scope() != &binding.scope || !goal.steps().contains(&binding.step)
                    })
                {
                    return Err(SessionError::InvalidInteraction);
                }
            }
            (None, _, None) => {}
            _ => return Err(SessionError::InvalidInteraction),
        }
        if let Some(consent) = &self.accepted_consent {
            let binding = consent.binding();
            if binding.owner != self.owner
                || binding.session != self.snapshot.session
                || binding.run != self.snapshot.run
                || self.snapshot.dispatch.as_ref() != Some(&binding.dispatch)
                || binding.issued_revision >= self.snapshot.revision
            {
                return Err(SessionError::InvalidInteraction);
            }
        }
        if let Some(consent) = &self.accepted_connector_consent {
            consent.binding.validate()?;
            if consent.binding.owner != self.owner
                || consent.binding.session != self.snapshot.session
                || consent.binding.run != self.snapshot.run
                || self.snapshot.dispatch.as_ref() != Some(&consent.binding.dispatch)
                || consent.binding.issued_revision >= self.snapshot.revision
            {
                return Err(SessionError::InvalidInteraction);
            }
        }
        if self.authority_events.iter().any(|event| {
            let revision = match event {
                AuthorityEvent::Denied { revision, .. }
                | AuthorityEvent::Withdrawn { revision, .. }
                | AuthorityEvent::ConnectorWithdrawn { revision, .. } => *revision,
            };
            revision.value() == 0 || revision > self.snapshot.revision
        }) {
            return Err(SessionError::InvalidState);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FencedSession {
    pub checkpoint: SessionCheckpoint,
    pub fence: FenceToken,
}

/// Conditional transition metadata is independent of the next state. Storage
/// checks all predecessor conditions atomically, including the global owner
/// command ledger. Autonomous application events have no client command key.
pub struct JournalAppend {
    pub session: SessionId,
    pub expected_revision: Revision,
    pub fence: FenceToken,
    pub command: Option<CommandId>,
    pub next: SessionCheckpoint,
    pub records: Vec<RecordWrite>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordWrite {
    pub reference: RecordRef,
    pub kind: RecordKind,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Artifact,
    Evidence,
    ConnectorSnapshot,
    ConnectorResult,
    ConnectorObservation,
    ConnectorProposal,
    /// Retained producer DATA referenced by exact pending/accepted consent.
    ConnectorProducerInput,
    PreExecutionGoal,
}

pub trait SessionRepositoryPort: SessionJournalPort + ArtifactStoragePort {
    fn record_kind(
        &self,
        owner: &OwnerBinding,
        reference: &RecordRef,
    ) -> Result<RecordKind, SessionError>;
}
impl JournalAppend {
    /// The adapter additionally checks the current fence and ledger while locked.
    /// No state/budget/event change may escape a failed append.
    pub fn validate_against(&self, current: &FencedSession) -> Result<(), SessionError> {
        current.checkpoint.validate()?;
        if self.next.owner != current.checkpoint.owner {
            return Err(SessionError::ScopeDenied);
        }
        if self.fence != current.fence
            || self.session != current.checkpoint.snapshot.session
            || self.expected_revision != current.checkpoint.snapshot.revision
        {
            return Err(SessionError::StaleRevision);
        }
        if current.checkpoint.snapshot.state.terminal() {
            return Err(SessionError::InvalidState);
        }
        self.next.validate()?;
        let old = &current.checkpoint;
        match (&self.next.resource_ledger, &old.resource_ledger) {
            (Some(next), Some(old)) => {
                next.validate_successor(old)?;
                let added = &next.reservations()[old.reservations().len()..];
                if added.len() > 1
                    || added.iter().any(|reservation| {
                        reservation.binding().revision != self.next.snapshot.revision
                            || self.next.snapshot.state != SessionState::Preparing
                            || self.next.snapshot.dispatch.as_ref()
                                != Some(&reservation.binding().dispatch)
                    })
                {
                    return Err(SessionError::InvalidState);
                }
            }
            (None, None) => {}
            _ => return Err(SessionError::InvalidState),
        }
        if self.next.goal != old.goal
            || self.next.initial_assessment != old.initial_assessment
            || self.next.execution != old.execution
            || self.next.snapshot.session != old.snapshot.session
            || self.next.snapshot.run != old.snapshot.run
            || self.next.snapshot.revision != self.expected_revision.next()?
            || self.next.budget.max_actions != old.budget.max_actions
            || self.next.budget.max_retries != old.budget.max_retries
            || self.next.budget.deadline_ms != old.budget.deadline_ms
            || self.next.budget.actions < old.budget.actions
            || self.next.budget.retries < old.budget.retries
            || !self
                .next
                .authority_events
                .starts_with(&old.authority_events)
        {
            return Err(SessionError::InvalidState);
        }
        if !self
            .next
            .connector_step_snapshots
            .starts_with(&old.connector_step_snapshots)
            || !self
                .next
                .connector_results
                .starts_with(&old.connector_results)
            || !self
                .next
                .connector_observations
                .starts_with(&old.connector_observations)
        {
            return Err(SessionError::InvalidState);
        }
        if old.pre_execution_goal.is_some()
            && self.next.pre_execution_goal != old.pre_execution_goal
        {
            return Err(SessionError::InvalidState);
        }
        if self.next.pre_execution_goal != old.pre_execution_goal {
            let candidate = self
                .next
                .pre_execution_goal
                .as_ref()
                .ok_or(SessionError::InvalidState)?;
            if old.snapshot.state != SessionState::Preparing
                || self.next.snapshot.state != SessionState::Runnable
                || candidate.committed_revision != self.next.snapshot.revision
                || candidate.binding.claim_revision != old.snapshot.revision
                || self
                    .records
                    .iter()
                    .filter(|r| {
                        r.kind == RecordKind::PreExecutionGoal && r.reference == candidate.record
                    })
                    .count()
                    != 1
            {
                return Err(SessionError::InvalidState);
            }
        }
        let added_snapshots =
            &self.next.connector_step_snapshots[old.connector_step_snapshots.len()..];
        let added_results = &self.next.connector_results[old.connector_results.len()..];
        let added_observations =
            &self.next.connector_observations[old.connector_observations.len()..];
        if added_snapshots.iter().any(|snapshot| {
            snapshot.binding.revision != self.next.snapshot.revision
                || self.next.snapshot.dispatch.as_ref() != Some(&snapshot.binding.dispatch)
                || self
                    .records
                    .iter()
                    .filter(|record| {
                        record.kind == RecordKind::ConnectorSnapshot
                            && record.reference == snapshot.record
                    })
                    .count()
                    != 1
        }) || added_results.iter().any(|result| {
            result.binding.revision != self.next.snapshot.revision
                || self.next.snapshot.dispatch.as_ref() != Some(&result.binding.dispatch)
                || self
                    .records
                    .iter()
                    .filter(|record| {
                        record.kind == RecordKind::ConnectorResult
                            && record.reference == result.record
                    })
                    .count()
                    != 1
        }) || added_observations.iter().any(|observation| {
            observation.binding.revision != self.next.snapshot.revision
                || self.next.snapshot.dispatch.as_ref() != Some(&observation.binding.dispatch)
                || self
                    .records
                    .iter()
                    .filter(|record| {
                        record.kind == RecordKind::ConnectorObservation
                            && record.reference == observation.record
                    })
                    .count()
                    != 1
        }) || self.records.iter().any(|record| match record.kind {
            RecordKind::ConnectorSnapshot => !added_snapshots
                .iter()
                .any(|snapshot| snapshot.record == record.reference),
            RecordKind::ConnectorResult => !added_results
                .iter()
                .any(|result| result.record == record.reference),
            RecordKind::ConnectorObservation => !added_observations
                .iter()
                .any(|observation| observation.record == record.reference),
            RecordKind::PreExecutionGoal => self.next.pre_execution_goal.as_ref().is_none_or(|r|Some(r)==old.pre_execution_goal.as_ref() || r.record!=record.reference),
            RecordKind::ConnectorProducerInput => {
                let pending = self.next.pending.as_ref().is_some_and(|pending| matches!(pending,
                    PendingInteraction::ConnectorConsent(binding) if binding.resume.producer_pins.contains(&record.reference)));
                let accepted = self.next.accepted_connector_consent.as_ref().is_some_and(|accepted| accepted.binding.resume.producer_pins.contains(&record.reference));
                !pending && !accepted
            }
            RecordKind::ConnectorProposal => self.next.pending.as_ref().is_none_or(|pending| {
                !matches!(pending, PendingInteraction::ConnectorConsent(binding)
                        if binding.resume.output_record == record.reference)
            }),
            RecordKind::Artifact | RecordKind::Evidence => false,
        }) {
            return Err(SessionError::InvalidState);
        }
        if let Some(PendingInteraction::ConnectorConsent(binding)) = &self.next.pending
            && self.next.pending != old.pending
            && binding.resume.producer_pins.iter().any(|pin| {
                self.records
                    .iter()
                    .filter(|record| {
                        record.kind == RecordKind::ConnectorProducerInput
                            && &record.reference == pin
                    })
                    .count()
                    != 1
            })
        {
            return Err(SessionError::InvalidState);
        }
        match (&self.command, &self.next.snapshot.command_outcome) {
            (Some(command), Some(outcome))
                if command == &outcome.command
                    && outcome.revision == self.next.snapshot.revision =>
            {
                Ok(())
            }
            (None, outcome) if outcome == &old.snapshot.command_outcome => Ok(()),
            _ => Err(SessionError::InvalidState),
        }
    }
}

pub trait SessionJournalPort {
    /// Owner-scoped lookup must not reveal a different owner's task existence.
    /// Neither read changes revision, resumes effects, or resets a budget.
    fn load(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
    ) -> Result<FencedSession, SessionError>;
    fn command_outcome(&self, key: &CommandKey) -> Result<CommandOutcome, SessionError>;
    /// Validate and atomically insert revision 1, immutable goal/owner/run and a
    /// globally owner-unique command outcome. Acknowledge only after durable commit.
    fn create(
        &self,
        command: &CommandId,
        initial: SessionCheckpoint,
    ) -> Result<FencedSession, SessionError>;
    /// Atomically compare revision/fence, refuse a duplicate owner command key,
    /// and persist next state, outcome, budget and any reservation together.
    /// Ambiguous commit returns OutcomeUnknown; inspect before another mutation.
    fn append(
        &self,
        owner: &OwnerBinding,
        transition: JournalAppend,
    ) -> Result<FencedSession, SessionError>;
}
