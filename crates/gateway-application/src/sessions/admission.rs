//! Pure shared command admission. No adapter owns or advances lifecycle here.
use super::*;

/// Authenticated, journal-loaded state used by the coordinator's admission gate.
/// Construction is application/host work; this is never a client command DTO.
pub struct SessionAdmission<'a> {
    pub owner: &'a OwnerBinding,
    pub session: &'a SessionId,
    pub revision: Revision,
    pub state: SessionState,
    pub pending: Option<&'a PendingRef>,
}
impl SessionAdmission<'_> {
    /// The caller performs owner-scoped lookup and ledger lookup before invoking
    /// this gate. Rejections consume neither command keys nor session revisions.
    pub fn check(
        &self,
        authenticated: &OwnerBinding,
        command: &SessionCommand,
        committed_duplicate: bool,
        now_ms: u64,
    ) -> Result<(), SessionError> {
        if authenticated != self.owner {
            return Err(SessionError::ScopeDenied);
        }
        let at = command.mutation().ok_or(SessionError::InvalidState)?;
        if &at.session != self.session {
            return Err(SessionError::Unavailable);
        }
        if committed_duplicate {
            return Err(SessionError::Duplicate);
        }
        if at.expected_revision != self.revision {
            return Err(SessionError::StaleRevision);
        }
        if self.state.terminal() {
            return Err(SessionError::InvalidState);
        }
        // No accepting a command whose resulting revision would wrap or lose
        // precision in the public JSON boundary.
        self.revision.next()?;
        match command {
            SessionCommand::Clarify { pending, .. }
                if self.state == SessionState::PendingClarification =>
            {
                self.pending.ok_or(SessionError::InvalidInteraction)?.check(
                    pending,
                    self.revision,
                    now_ms,
                )
            }
            SessionCommand::Approve { pending, .. }
                if self.state == SessionState::PendingConsent =>
            {
                self.pending.ok_or(SessionError::InvalidInteraction)?.check(
                    pending,
                    self.revision,
                    now_ms,
                )
            }
            SessionCommand::Continue { .. } if self.state == SessionState::Runnable => Ok(()),
            SessionCommand::Cancel { .. }
                if matches!(
                    self.state,
                    SessionState::Runnable
                        | SessionState::PendingClarification
                        | SessionState::PendingConsent
                        | SessionState::Dispatching
                ) =>
            {
                Ok(())
            }
            _ if self.state == SessionState::OutcomeUnknown => Err(SessionError::OutcomeUnknown),
            _ => Err(SessionError::InvalidState),
        }
    }
}

impl SessionSnapshot {
    /// A lifecycle projection cannot claim success, cancellation or failure while
    /// an effect remains unresolved. Validating shape does not verify evidence.
    pub fn validate(&self) -> Result<(), SessionError> {
        if self.revision.value() == 0 {
            return Err(SessionError::InvalidState);
        }
        let pending_state = matches!(
            self.state,
            SessionState::PendingClarification | SessionState::PendingConsent
        );
        if pending_state != self.pending.is_some()
            || self
                .pending
                .as_ref()
                .is_some_and(|p| p.issued_revision != self.revision)
            || (self.dispatch.is_none() != (self.dispatch_knowledge == DispatchKnowledge::None))
            || ((self.state == SessionState::Completed) != self.final_evidence.is_some())
            || self.command_outcome.as_ref().is_some_and(|c| {
                c.session != self.session || c.revision.value() == 0 || c.revision > self.revision
            })
        {
            return Err(SessionError::InvalidState);
        }
        let unresolved = matches!(
            self.dispatch_knowledge,
            DispatchKnowledge::Reserved | DispatchKnowledge::Unknown
        );
        match self.state {
            SessionState::Completed | SessionState::Failed | SessionState::Cancelled
                if self.dispatch_knowledge == DispatchKnowledge::Prepared =>
            {
                Err(SessionError::InvalidState)
            }
            SessionState::Completed | SessionState::Failed | SessionState::Cancelled
                if unresolved =>
            {
                Err(SessionError::OutcomeUnknown)
            }
            SessionState::OutcomeUnknown
                if self.dispatch_knowledge != DispatchKnowledge::Unknown =>
            {
                Err(SessionError::InvalidState)
            }
            SessionState::Dispatching | SessionState::Cancelling if !unresolved => {
                Err(SessionError::InvalidState)
            }
            SessionState::Runnable
            | SessionState::PendingClarification
            | SessionState::PendingConsent
                if unresolved =>
            {
                Err(SessionError::InvalidState)
            }
            _ => Ok(()),
        }
    }
}
