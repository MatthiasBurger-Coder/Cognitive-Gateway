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

/// The recovery adapter persists each field in its own bounded/versioned format.
/// Rebuilding this typed checkpoint requires the supported-goal validator, full
/// pending payload and consumed budgets, not the redacted CG-14 diagnostic JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCheckpoint {
    pub owner: OwnerBinding,
    pub goal: SupportedArtifactGoal,
    pub execution: RequestedExecution,
    pub snapshot: SessionSnapshot,
    pub budget: SessionBudget,
    pub pending: Option<PendingInteraction>,
    pub accepted_consent: Option<VerifiedConsent>,
    pub authority_events: Vec<AuthorityEvent>,
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
        if self.authority_events.len() > 4096 {
            return Err(SessionError::LimitExceeded);
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
                    || question.basis != self.goal.basis().projection
                    || question
                        .alternatives
                        .iter()
                        .any(|r| !self.goal.basis().sources.contains(r))
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
