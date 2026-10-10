//! Exact task authority bindings and provider-neutral outbound contracts.
use super::*;
use serde::{Deserialize, Serialize};

/// Digests of the registered serializer's complete action/arguments and current
/// process/catalog/policy context. A raw arbitrary JSON hash is not a substitute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionBinding {
    pub owner: OwnerBinding,
    pub session: SessionId,
    pub run: RunId,
    pub pending: PendingId,
    pub issued_revision: Revision,
    pub dispatch: DispatchId,
    pub step: RecordId,
    pub action: RecordRef,
    pub arguments: RecordRef,
    pub artifact_basis: RecordRef,
    pub authority: RecordRef,
    pub expires_at_ms: u64,
}

/// Verified only through the trusted CG issuer/store boundary. A request is
/// deliberately a different type from this record. No client Deserialize impl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedConsent {
    record: ConsentRecordRef,
    binding: ActionBinding,
    issuer: PrincipalId,
}
impl VerifiedConsent {
    /// Called by the trusted interaction authority after authenticating issuance
    /// and loading the pinned store record, never directly from a client payload.
    pub fn from_trusted_record(
        record: ConsentRecordRef,
        binding: ActionBinding,
        issuer: PrincipalId,
    ) -> Self {
        Self {
            record,
            binding,
            issuer,
        }
    }
    pub fn record(&self) -> &ConsentRecordRef {
        &self.record
    }
    pub fn issuer(&self) -> &PrincipalId {
        &self.issuer
    }
    pub fn binding(&self) -> &ActionBinding {
        &self.binding
    }
    pub fn check_approval(
        &self,
        current: &ActionBinding,
        revision: Revision,
        now_ms: u64,
        revoked: bool,
    ) -> Result<(), SessionError> {
        if revoked {
            return Err(SessionError::AuthorityDenied);
        }
        if now_ms >= self.binding.expires_at_ms {
            return Err(SessionError::ExpiredInteraction);
        }
        if &self.binding != current || revision != current.issued_revision {
            return Err(SessionError::InvalidInteraction);
        }
        Ok(())
    }
    /// Approval advances the session revision; it never rewrites the issuance
    /// revision inside the grant. Live revocation and current basis still apply.
    pub fn check_dispatch(
        &self,
        current: &ActionBinding,
        accepted_revision: Revision,
        now_ms: u64,
        revoked: bool,
    ) -> Result<(), SessionError> {
        self.check_approval(current, current.issued_revision, now_ms, revoked)?;
        if accepted_revision != current.issued_revision.next()? {
            return Err(SessionError::StaleRevision);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsentStatus {
    Granted(Box<VerifiedConsent>),
    Denied,
    Withdrawn,
}

/// An actual missing structured-input choice from the trusted validator. The
/// exact parser/version and admitted alternatives survive restart unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceQuestion {
    pub pending: PendingRef,
    pub basis: RecordRef,
    pub alternatives: Vec<RecordRef>,
}
impl SourceQuestion {
    pub fn validate_answer(
        &self,
        id: &PendingId,
        revision: Revision,
        now_ms: u64,
        answer: &ClarificationAnswer,
    ) -> Result<RecordRef, SessionError> {
        self.pending.check(id, revision, now_ms)?;
        let ClarificationAnswer::SelectSource(selected) = answer;
        if !self.alternatives.contains(selected) {
            return Err(SessionError::InvalidInput);
        }
        Ok(selected.clone())
    }
}

/// Scope/policy admission is separate from immutable ownership and client claims.
pub trait CurrentAuthorityPort {
    fn admit(
        &self,
        owner: &OwnerBinding,
        execution: RequestedExecution,
    ) -> Result<RecordRef, SessionError>;
    fn disclose(&self, owner: &OwnerBinding, session: &SessionId) -> Result<(), SessionError>;
}

pub trait InteractionAuthorityPort {
    fn load_consent(
        &self,
        owner: &OwnerBinding,
        record: &ConsentRecordRef,
    ) -> Result<ConsentStatus, SessionError>;
    /// Must be checked again immediately before the fenced invocation.
    fn live_status(&self, consent: &VerifiedConsent) -> Result<ConsentStatus, SessionError>;
}

/// Storage provides bytes; application validation supplies the verdict. This
/// port intentionally cannot return a client-asserted boolean success.
pub trait ArtifactStoragePort {
    fn load_artifact(
        &self,
        owner: &OwnerBinding,
        reference: &RecordRef,
    ) -> Result<Vec<u8>, SessionError>;
}

/// Persistent cumulative baseline units. Optional inference/connector resources
/// have no implied zero usage; those capabilities remain unsupported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionBudget {
    pub actions: u32,
    pub retries: u32,
    pub max_actions: u32,
    pub max_retries: u32,
    pub deadline_ms: u64,
}
impl SessionBudget {
    pub fn validate(&self) -> Result<(), SessionError> {
        if self.max_actions == 0
            || self.max_actions > 10_000
            || self.max_retries > 10_000
            || self.actions > self.max_actions
            || self.retries > self.max_retries
            || self.deadline_ms == 0
            || self.deadline_ms > MAX_REVISION
        {
            return Err(SessionError::LimitExceeded);
        }
        Ok(())
    }
    /// Reservation is committed with journal intent before any invocation.
    /// Failed validation leaves all counters intact.
    pub fn reserve(&mut self, now_ms: u64, retry: bool) -> Result<(), SessionError> {
        self.validate()?;
        if now_ms >= self.deadline_ms
            || self.actions == self.max_actions
            || (retry && self.retries == self.max_retries)
        {
            return Err(SessionError::LimitExceeded);
        }
        self.actions += 1;
        if retry {
            self.retries += 1;
        }
        Ok(())
    }
}
