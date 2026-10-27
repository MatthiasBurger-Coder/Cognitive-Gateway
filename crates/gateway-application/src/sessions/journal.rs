//! Transactional storage responsibilities owned by #276. The port specifies
//! acknowledgement/fencing semantics; it is not an in-memory recovery substitute.
use super::*;

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
    pub pending: Option<PendingInteraction>,
    pub accepted_consent: Option<VerifiedConsent>,
    pub authority_events: Vec<AuthorityEvent>,
    pub selected_source: Option<RecordRef>,
    pub artifact: Option<RecordRef>,
    pub lease_until_ms: Option<u64>,
    pub initial_assessment: Option<InitialAssessment>,
    pub resource_ledger: Option<SessionResourceLedger>,
    pub pre_execution_goal: Option<PreExecutionGoalRef>,
    pub prepared_before: Option<PreparedBeforeRef>,
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
}

impl SessionCheckpoint {
    pub fn validate(&self) -> Result<(), SessionError> {
        self.snapshot.validate()?;
        self.budget.validate()?;
        match (&self.goal, &self.resource_ledger) {
            (SupportedSessionGoal::Artifact(_), None)
                if self.pre_execution_goal.is_none() && self.prepared_before.is_none() => {}
            (SupportedSessionGoal::Connector(goal), Some(ledger)) => {
                ledger.validate()?;
                if ledger.reservations().len() > 1
                    || (self.snapshot.state == SessionState::Completed
                        && self.pre_execution_goal.is_none())
                    || self
                        .lease_until_ms
                        .is_some_and(|d| d == 0 || d > self.budget.deadline_ms)
                {
                    return Err(SessionError::InvalidState);
                }
                if self.pending.is_some()
                    || self.accepted_consent.is_some()
                    || self.selected_source.is_some()
                    || self.artifact.is_some()
                {
                    return Err(SessionError::InvalidState);
                }
                for r in ledger.reservations() {
                    let b = r.binding();
                    if b.owner != self.owner
                        || b.scope != *goal.scope()
                        || b.session != self.snapshot.session
                        || b.run != self.snapshot.run
                        || !goal.steps().contains(&b.step)
                        || b.revision > self.snapshot.revision
                    {
                        return Err(SessionError::InvalidState);
                    }
                }
                if self.snapshot.state == SessionState::Dispatching {
                    let r = ledger
                        .reservations()
                        .last()
                        .ok_or(SessionError::InvalidState)?;
                    if self.lease_until_ms.is_none()
                        || self.snapshot.dispatch.as_ref() != Some(&r.binding().dispatch)
                        || r.binding().revision != self.snapshot.revision
                        || self.pre_execution_goal.is_some()
                        || self.prepared_before.is_some()
                    {
                        return Err(SessionError::InvalidState);
                    }
                }
                if let Some(before) = &self.prepared_before {
                    before.validate_for(self)?;
                    if self.pre_execution_goal.is_some()
                        || self.snapshot.final_evidence.is_some()
                        || !matches!(
                            self.snapshot.state,
                            SessionState::Runnable | SessionState::Failed | SessionState::Cancelled
                        )
                        || self.lease_until_ms.is_some()
                        || (self.snapshot.state == SessionState::Runnable
                            && self.snapshot.dispatch_knowledge != DispatchKnowledge::Prepared)
                    {
                        return Err(SessionError::InvalidState);
                    }
                }
                if let Some(marker) = &self.pre_execution_goal {
                    marker.validate_for(self)?;
                    if !matches!(
                        self.snapshot.state,
                        SessionState::Runnable
                            | SessionState::Completed
                            | SessionState::Failed
                            | SessionState::Cancelled
                    ) || self.lease_until_ms.is_some()
                    {
                        return Err(SessionError::InvalidState);
                    }
                }
            }
            _ => return Err(SessionError::InvalidState),
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
        match (&self.pending, self.snapshot.state, &self.snapshot.pending) {
            (
                Some(PendingInteraction::Source(question)),
                SessionState::PendingClarification,
                Some(reference),
            ) => {
                let artifact_basis = self.goal.artifact_basis()?;
                if &question.pending != reference
                    || question.alternatives.is_empty()
                    || question.alternatives.len() > 256
                    || question.basis != self.goal.artifact_basis()?.projection
                    || question
                        .alternatives
                        .iter()
                        .any(|r| !artifact_basis.sources.contains(r))
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
        if self.authority_events.iter().any(|event| {
            let revision = match event {
                AuthorityEvent::Denied { revision, .. }
                | AuthorityEvent::Withdrawn { revision, .. } => *revision,
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
    PreExecutionGoal,
    CompleteBefore,
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
        match (&old.resource_ledger, &self.next.resource_ledger) {
            (None, None) => {}
            (Some(old_ledger), Some(new_ledger)) => {
                new_ledger.validate_successor(old_ledger)?;
                let added = new_ledger.reservations().len() - old_ledger.reservations().len();
                if added > 1
                    || (added == 1
                        && (old.snapshot.state != SessionState::Runnable
                            || old.pre_execution_goal.is_some()
                            || old.prepared_before.is_some()
                            || self.next.snapshot.state != SessionState::Dispatching
                            || new_ledger.reservations().last().unwrap().binding().revision
                                != self.next.snapshot.revision))
                {
                    return Err(SessionError::InvalidState);
                }
            }
            _ => return Err(SessionError::InvalidState),
        }
        if old.goal.as_connector().is_some()
            && old.snapshot.state == SessionState::Dispatching
            && self.next.snapshot.state == SessionState::Runnable
            && self.next.pre_execution_goal.is_none()
            && self.next.prepared_before.is_none()
        {
            return Err(SessionError::InvalidState);
        }
        match (&old.pre_execution_goal, &self.next.pre_execution_goal) {
            (Some(a), Some(b)) if a == b => {}
            (None, None) => {}
            (None, Some(marker))
                if old.snapshot.state == SessionState::Dispatching
                    && self.next.snapshot.state == SessionState::Runnable
                    && marker.binding.claim_revision == old.snapshot.revision
                    && marker.committed_revision == self.next.snapshot.revision =>
            {
                if self
                    .records
                    .iter()
                    .filter(|r| {
                        r.kind == RecordKind::PreExecutionGoal
                            && r.reference == marker.record
                            && content_reference(r.reference.id().as_str(), &r.bytes)
                                .ok()
                                .as_ref()
                                == Some(&r.reference)
                    })
                    .count()
                    != 1
                {
                    return Err(SessionError::InvalidState);
                }
            }
            _ => return Err(SessionError::InvalidState),
        }
        if self.records.iter().any(|r| {
            r.kind == RecordKind::PreExecutionGoal
                && (old.pre_execution_goal.is_some()
                    || self
                        .next
                        .pre_execution_goal
                        .as_ref()
                        .is_none_or(|m| m.record != r.reference))
        }) {
            return Err(SessionError::InvalidState);
        }
        match (&old.prepared_before, &self.next.prepared_before) {
            (Some(a), Some(b)) if a == b => {}
            (None, None) => {}
            (None, Some(before))
                if old.snapshot.state == SessionState::Dispatching
                    && self.next.snapshot.state == SessionState::Runnable
                    && before.binding.capture.revision == old.snapshot.revision
                    && before
                        .binding
                        .reservation
                        .as_ref()
                        .is_some_and(|r| r.fence == current.fence)
                    && before.committed_revision == self.next.snapshot.revision =>
            {
                let matching = self
                    .records
                    .iter()
                    .filter(|r| {
                        r.kind == RecordKind::CompleteBefore && r.reference == before.record
                    })
                    .collect::<Vec<_>>();
                if matching.len() != 1 {
                    return Err(SessionError::InvalidState);
                }
                decode_before_record(before, &matching[0].bytes, before.validated_at_ms)?;
            }
            _ => return Err(SessionError::InvalidState),
        }
        if self.records.iter().any(|record| {
            record.kind == RecordKind::CompleteBefore
                && (old.prepared_before.is_some()
                    || self
                        .next
                        .prepared_before
                        .as_ref()
                        .is_none_or(|r| r.record != record.reference))
        }) {
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
