//! Exact task authority bindings and provider-neutral outbound contracts.
use super::*;
use crate::{connectors::ConnectorRequest, sessions::readback_storage::ConnectorRequestIdentity};
use gateway_domain::{
    ContentDigest, ContextScopeId, PlanStepId, PolicyId, prompt_ir::InvocationIrIdentity,
};
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

/// Exact, immutable consent basis for one connector proposal. This record is
/// separate from artifact compilation consent: it binds the full authenticated
/// owner (including project), session/run/revision, step/dispatch, canonical
/// connector request identity, and the current CG-09 policy revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorConsentResumeBinding {
    pub invocation: InvocationIrIdentity,
    pub proposal_ordinal: u32,
    pub output_record: RecordRef,
    pub output_digest: ContentDigest,
    pub producer_pins: Vec<RecordRef>,
}

impl ConnectorConsentResumeBinding {
    pub fn validate(&self) -> Result<(), SessionError> {
        if self.proposal_ordinal as usize
            >= crate::sessions::connector_execution::MAX_CONNECTOR_PROPOSALS_PER_STEP
            || self.producer_pins.is_empty()
            || self.producer_pins.len() > 128
            || self
                .producer_pins
                .iter()
                .enumerate()
                .any(|(i, pin)| self.producer_pins[..i].contains(pin))
        {
            return Err(SessionError::InvalidInteraction);
        }
        Ok(())
    }
}

/// Bounded raw provider output retained for consent resume. The adapter must
/// parse and run this output through current `verify_output` after rebuilding
/// the exact invocation; this value is data, never a verified grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorConsentResumeMaterial {
    pub invocation: InvocationIrIdentity,
    pub proposal_ordinal: u32,
    pub raw_output: Vec<u8>,
    pub output_digest: ContentDigest,
    pub producer_pins: Vec<RecordRef>,
}

impl ConnectorConsentResumeMaterial {
    pub fn new(
        invocation: InvocationIrIdentity,
        proposal_ordinal: u32,
        raw_output: Vec<u8>,
        producer_pins: Vec<RecordRef>,
    ) -> Result<Self, SessionError> {
        if raw_output.is_empty() || raw_output.len() > 1_048_576 {
            return Err(SessionError::LimitExceeded);
        }
        let parsed: serde_json::Value =
            serde_json::from_slice(&raw_output).map_err(|_| SessionError::InvalidInput)?;
        if !crate::codex::security::credential_free(&parsed) {
            return Err(SessionError::AuthorityDenied);
        }
        let material = Self {
            invocation,
            proposal_ordinal,
            output_digest: gateway_domain::context_content_digest(&raw_output),
            raw_output,
            producer_pins,
        };
        material.validate()?;
        Ok(material)
    }

    pub fn validate(&self) -> Result<(), SessionError> {
        if self.raw_output.is_empty() || self.raw_output.len() > 1_048_576 {
            return Err(SessionError::LimitExceeded);
        }
        if self.proposal_ordinal as usize
            >= crate::sessions::connector_execution::MAX_CONNECTOR_PROPOSALS_PER_STEP
        {
            return Err(SessionError::InvalidInteraction);
        }
        if self.producer_pins.is_empty()
            || self.producer_pins.len() > 128
            || self
                .producer_pins
                .iter()
                .enumerate()
                .any(|(i, pin)| self.producer_pins[..i].contains(pin))
        {
            return Err(SessionError::InvalidInteraction);
        }
        let parsed: serde_json::Value =
            serde_json::from_slice(&self.raw_output).map_err(|_| SessionError::InvalidInput)?;
        if !crate::codex::security::credential_free(&parsed) {
            return Err(SessionError::AuthorityDenied);
        }
        if gateway_domain::context_content_digest(&self.raw_output) != self.output_digest {
            return Err(SessionError::InvalidInteraction);
        }
        let parsed_output = crate::output_verification::parse_output(&self.raw_output)
            .map_err(|_| SessionError::InvalidInteraction)?;
        if parsed_output.binding().invocation != self.invocation
            || parsed_output
                .data()
                .tools
                .get(self.proposal_ordinal as usize)
                .is_none()
        {
            return Err(SessionError::StaleRevision);
        }
        Ok(())
    }

    pub fn matches(&self, binding: &ConnectorConsentResumeBinding) -> Result<(), SessionError> {
        self.validate()?;
        binding.validate()?;
        if self.invocation != binding.invocation
            || self.proposal_ordinal != binding.proposal_ordinal
            || self.output_digest != binding.output_digest
            || self.producer_pins != binding.producer_pins
        {
            return Err(SessionError::StaleRevision);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectorConsentBinding {
    pub owner: OwnerBinding,
    pub scope: ContextScopeId,
    pub session: SessionId,
    pub run: RunId,
    pub issued_revision: Revision,
    pub pending: PendingId,
    #[serde(with = "connector_consent_step_id")]
    pub step: PlanStepId,
    pub dispatch: DispatchId,
    pub request: ConnectorRequestIdentity,
    pub resume: ConnectorConsentResumeBinding,
    pub policy: PolicyId,
    pub policy_revision: ContentDigest,
    pub expires_at_ms: u64,
    pub issuer: PrincipalId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectorConsentBindingWire {
    owner: OwnerBinding,
    scope: ContextScopeId,
    session: SessionId,
    run: RunId,
    issued_revision: Revision,
    pending: PendingId,
    #[serde(with = "connector_consent_step_id")]
    step: PlanStepId,
    dispatch: DispatchId,
    request: ConnectorRequestIdentity,
    resume: ConnectorConsentResumeBinding,
    policy: PolicyId,
    policy_revision: ContentDigest,
    expires_at_ms: u64,
    issuer: PrincipalId,
}

impl<'de> Deserialize<'de> for ConnectorConsentBinding {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ConnectorConsentBindingWire::deserialize(deserializer)?;
        let binding = Self {
            owner: wire.owner,
            scope: wire.scope,
            session: wire.session,
            run: wire.run,
            issued_revision: wire.issued_revision,
            pending: wire.pending,
            step: wire.step,
            dispatch: wire.dispatch,
            request: wire.request,
            resume: wire.resume,
            policy: wire.policy,
            policy_revision: wire.policy_revision,
            expires_at_ms: wire.expires_at_ms,
            issuer: wire.issuer,
        };
        binding.validate().map_err(serde::de::Error::custom)?;
        Ok(binding)
    }
}

mod connector_consent_step_id {
    use super::PlanStepId;
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
        PlanStepId::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl ConnectorConsentBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        owner: OwnerBinding,
        scope: ContextScopeId,
        session: SessionId,
        run: RunId,
        issued_revision: Revision,
        pending: PendingId,
        step: PlanStepId,
        dispatch: DispatchId,
        request: &ConnectorRequest,
        resume: ConnectorConsentResumeBinding,
        policy: PolicyId,
        policy_revision: ContentDigest,
        expires_at_ms: u64,
        issuer: PrincipalId,
    ) -> Result<Self, SessionError> {
        let binding = Self {
            owner,
            scope,
            session,
            run,
            issued_revision,
            pending,
            step,
            dispatch,
            request: ConnectorRequestIdentity::from_request(request)?,
            resume,
            policy,
            policy_revision,
            expires_at_ms,
            issuer,
        };
        binding.validate()?;
        Ok(binding)
    }

    /// Validate all identities which can be checked without the full request.
    /// Runtime use must additionally call `matches_current` with the actual
    /// typed request and current policy identity.
    pub fn validate(&self) -> Result<(), SessionError> {
        if self.issued_revision.value() == 0
            || self.expires_at_ms == 0
            || self.expires_at_ms > MAX_REVISION
            || self.request.scope != self.scope.as_str()
            || self.request.step != self.step.as_str()
        {
            return Err(SessionError::InvalidInteraction);
        }
        self.request.validate()?;
        self.resume.validate()?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn matches_current(
        &self,
        owner: &OwnerBinding,
        scope: &ContextScopeId,
        session: &SessionId,
        run: &RunId,
        revision: Revision,
        step: &PlanStepId,
        dispatch: &DispatchId,
        request: &ConnectorRequest,
        policy: &PolicyId,
        policy_revision: &ContentDigest,
    ) -> Result<(), SessionError> {
        self.validate()?;
        let current_request = ConnectorRequestIdentity::from_request(request)?;
        if &self.owner != owner
            || &self.scope != scope
            || &self.session != session
            || &self.run != run
            || self.issued_revision != revision
            || &self.step != step
            || &self.dispatch != dispatch
            || self.request != current_request
            || &self.policy != policy
            || &self.policy_revision != policy_revision
        {
            return Err(SessionError::StaleRevision);
        }
        Ok(())
    }
}

/// Content-addressed reference to a connector consent decision. It is not a
/// grant until loaded from the trusted store and reconstructed below.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConnectorConsentRecordRef(pub RecordRef);

/// Durable reference and immutable identity accepted by an operator. The
/// record is reloaded for current status before any connector dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedConnectorConsent {
    pub record: ConnectorConsentRecordRef,
    pub binding: ConnectorConsentBinding,
}

/// A connector grant reconstructed only by a trusted consent-store adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedConnectorConsent {
    record: ConnectorConsentRecordRef,
    binding: ConnectorConsentBinding,
}

impl VerifiedConnectorConsent {
    /// Trusted store constructor. Callers must authenticate the issuer, verify
    /// the content-addressed decision record, and load current grant status.
    pub fn from_trusted_record(
        record: ConnectorConsentRecordRef,
        binding: ConnectorConsentBinding,
    ) -> Result<Self, SessionError> {
        binding.validate()?;
        if record.0.id().as_str().is_empty() {
            return Err(SessionError::InvalidInteraction);
        }
        Ok(Self { record, binding })
    }

    pub fn record(&self) -> &ConnectorConsentRecordRef {
        &self.record
    }

    pub fn binding(&self) -> &ConnectorConsentBinding {
        &self.binding
    }

    /// Revalidate exact request and all current session/policy identities
    /// immediately before dispatch. Revocation and expiry always deny.
    #[allow(clippy::too_many_arguments)]
    pub fn check_dispatch(
        &self,
        owner: &OwnerBinding,
        scope: &ContextScopeId,
        session: &SessionId,
        run: &RunId,
        revision: Revision,
        step: &PlanStepId,
        dispatch: &DispatchId,
        request: &ConnectorRequest,
        policy: &PolicyId,
        policy_revision: &ContentDigest,
        now_ms: u64,
        revoked: bool,
    ) -> Result<(), SessionError> {
        if revoked {
            return Err(SessionError::AuthorityDenied);
        }
        if now_ms >= self.binding.expires_at_ms {
            return Err(SessionError::ExpiredInteraction);
        }
        self.binding.matches_current(
            owner,
            scope,
            session,
            run,
            revision,
            step,
            dispatch,
            request,
            policy,
            policy_revision,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectorConsentStatus {
    Granted(Box<VerifiedConnectorConsent>),
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
    fn load_connector_consent(
        &self,
        _owner: &OwnerBinding,
        _record: &ConnectorConsentRecordRef,
    ) -> Result<ConnectorConsentStatus, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn live_connector_consent(
        &self,
        _consent: &VerifiedConnectorConsent,
    ) -> Result<ConnectorConsentStatus, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
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
