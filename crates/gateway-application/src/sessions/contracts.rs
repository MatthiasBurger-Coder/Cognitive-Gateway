use gateway_domain::{ContextScopeId, ExecutionProfile, Intent, OperatingMode, PlanStepId};
use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_REVISION: u64 = 9_007_199_254_740_991;

/// The frozen boundary token alphabet, independent of any transport crate.
fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
}

macro_rules! identity {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, SessionError> {
                let value = value.into();
                if !token(&value) { return Err(SessionError::InvalidInput); }
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str { &self.0 }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Self::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }
    )+};
}
identity!(
    PrincipalId,
    WorkspaceId,
    ProjectId,
    BindingId,
    ClientOwnerId,
    SessionId,
    RunId,
    CommandId,
    DispatchId,
    PendingId,
    RecordId
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct Revision(u64);
impl Revision {
    pub fn new(value: u64) -> Result<Self, SessionError> {
        if value > MAX_REVISION {
            return Err(SessionError::LimitExceeded);
        }
        Ok(Self(value))
    }
    pub fn value(self) -> u64 {
        self.0
    }
    pub fn next(self) -> Result<Self, SessionError> {
        Self::new(self.0.checked_add(1).ok_or(SessionError::LimitExceeded)?)
    }
}
impl<'de> Deserialize<'de> for Revision {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(u64::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// Immutable authenticated ownership; connection and mapping revisions are absent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerBinding {
    pub principal: PrincipalId,
    pub workspace: WorkspaceId,
    pub project: ProjectId,
    pub binding: BindingId,
    pub client_owner: ClientOwnerId,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandKey {
    pub owner: OwnerBinding,
    pub command: CommandId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedExecution {
    pub mode: OperatingMode,
    pub profile: ExecutionProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mutation {
    pub session: SessionId,
    pub command: CommandId,
    pub expected_revision: Revision,
}

/// A pinned canonical record, with validated identity and SHA-256 digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordRef {
    id: RecordId,
    revision: RecordId,
    digest: String,
}
impl RecordRef {
    pub fn new(id: RecordId, revision: RecordId, digest: String) -> Result<Self, SessionError> {
        if digest.len() != 71
            || !digest.starts_with("sha256:")
            || !digest[7..]
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(SessionError::InvalidInput);
        }
        Ok(Self {
            id,
            revision,
            digest,
        })
    }
    pub fn id(&self) -> &RecordId {
        &self.id
    }
    pub fn revision(&self) -> &RecordId {
        &self.revision
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
}
impl<'de> Deserialize<'de> for RecordRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            id: RecordId,
            revision: RecordId,
            digest: String,
        }
        let wire = Wire::deserialize(d)?;
        Self::new(wire.id, wire.revision, wire.digest).map_err(serde::de::Error::custom)
    }
}

/// Trusted registration of the supported input set. A client cannot replace it
/// by wrapping an arbitrary desired state in a session request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactGoalBasis {
    pub scope: ContextScopeId,
    pub plan: RecordRef,
    #[serde(with = "step_id")]
    pub step: PlanStepId,
    pub projection: RecordRef,
    pub sources: Vec<RecordRef>,
}

mod step_id {
    use super::*;
    pub fn serialize<S: serde::Serializer>(id: &PlanStepId, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(id.as_str())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PlanStepId, D::Error> {
        let value = String::deserialize(d)?;
        if !token(&value) {
            return Err(serde::de::Error::custom("invalid step identity"));
        }
        PlanStepId::new(value).map_err(serde::de::Error::custom)
    }
}

/// Validated goal, deliberately not client-deserializable. The subject denotes
/// the trusted verifier's verdict over the exact persisted basis, never a model fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedArtifactGoal {
    intent: Intent,
    basis: ArtifactGoalBasis,
}
impl SupportedArtifactGoal {
    pub fn validate(intent: Intent, basis: ArtifactGoalBasis) -> Result<Self, SessionError> {
        let desired =
            serde_json::to_value(intent.desired_state()).map_err(|_| SessionError::InvalidInput)?;
        let expected = serde_json::json!({
            "schema_version":"1.0", "id":desired["id"],
            "conditions":[{"id":"context-verified", "subject":format!("cg.context.{}.verified", basis.projection.id().as_str()),
                "operator":"EQUALS", "expected":{"kind":"BOOLEAN", "value":true}}],
            "expression":{"kind":"CONDITION", "value":"context-verified"},
            "acceptance_criteria":[], "constraints":[]
        });
        if desired != expected
            || basis.sources.len() > 256
            || basis
                .sources
                .iter()
                .enumerate()
                .any(|(i, r)| basis.sources[..i].iter().any(|p| p.id() == r.id()))
        {
            return Err(SessionError::UnsupportedCapability);
        }
        Ok(Self { intent, basis })
    }
    pub fn intent(&self) -> &Intent {
        &self.intent
    }
    pub fn basis(&self) -> &ArtifactGoalBasis {
        &self.basis
    }
}

/// Only the registered non-authority answer shape is supported initially.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClarificationAnswer {
    SelectSource(RecordRef),
}

/// This is a reference to be loaded from the trusted consent store, not a grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentRecordRef(pub RecordRef);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCommand {
    Start {
        command: CommandId,
        intent: Intent,
        execution: RequestedExecution,
    },
    Clarify {
        at: Mutation,
        pending: PendingId,
        answer: ClarificationAnswer,
    },
    Approve {
        at: Mutation,
        pending: PendingId,
        consent: ConsentRecordRef,
    },
    Continue {
        at: Mutation,
    },
    Cancel {
        at: Mutation,
    },
}
impl SessionCommand {
    pub fn command_id(&self) -> &CommandId {
        match self {
            Self::Start { command, .. } => command,
            Self::Clarify { at, .. }
            | Self::Approve { at, .. }
            | Self::Continue { at }
            | Self::Cancel { at } => &at.command,
        }
    }
    pub fn mutation(&self) -> Option<&Mutation> {
        match self {
            Self::Start { .. } => None,
            Self::Clarify { at, .. }
            | Self::Approve { at, .. }
            | Self::Continue { at }
            | Self::Cancel { at } => Some(at),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectTarget {
    Session(SessionId),
    Command(CommandId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Runnable,
    Dispatching,
    PendingClarification,
    PendingConsent,
    Cancelling,
    OutcomeUnknown,
    Completed,
    Failed,
    Cancelled,
}
impl SessionState {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchKnowledge {
    None,
    /// Identity reserved for exact consent; invocation has not been released.
    Prepared,
    Reserved,
    Verified,
    Unknown,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingRef {
    pub id: PendingId,
    pub issued_revision: Revision,
    pub expires_at_ms: u64,
}
impl PendingRef {
    pub fn check(
        &self,
        id: &PendingId,
        revision: Revision,
        now_ms: u64,
    ) -> Result<(), SessionError> {
        if self.issued_revision.value() == 0
            || self.expires_at_ms == 0
            || self.expires_at_ms > MAX_REVISION
        {
            return Err(SessionError::InvalidInteraction);
        }
        if &self.id != id || self.issued_revision != revision {
            return Err(SessionError::InvalidInteraction);
        }
        if now_ms >= self.expires_at_ms {
            return Err(SessionError::ExpiredInteraction);
        }
        Ok(())
    }
}

/// Disclosure-only snapshot; the service retains ownership internally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSnapshot {
    pub session: SessionId,
    pub run: RunId,
    pub revision: Revision,
    pub state: SessionState,
    pub pending: Option<PendingRef>,
    pub dispatch: Option<DispatchId>,
    pub dispatch_knowledge: DispatchKnowledge,
    pub command_outcome: Option<CommandOutcome>,
    pub final_evidence: Option<RecordRef>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandOutcome {
    pub command: CommandId,
    pub session: SessionId,
    pub revision: Revision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    InvalidInput,
    UnsupportedCapability,
    ScopeDenied,
    Unavailable,
    Duplicate,
    StaleRevision,
    InvalidState,
    InvalidInteraction,
    ExpiredInteraction,
    ConsentRequired,
    AuthorityDenied,
    LimitExceeded,
    OutcomeUnknown,
    StorageUnavailable,
}
impl SessionError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidInput => "CG_INVALID_INPUT",
            Self::UnsupportedCapability => "CG_UNSUPPORTED_CAPABILITY",
            Self::ScopeDenied => "CG_SCOPE_DENIED",
            Self::Unavailable => "CG_SESSION_UNAVAILABLE",
            Self::Duplicate => "CG_DUPLICATE_COMMAND",
            Self::StaleRevision => "CG_STALE_REVISION",
            Self::InvalidState => "CG_INVALID_SESSION_STATE",
            Self::InvalidInteraction => "CG_INVALID_INTERACTION",
            Self::ExpiredInteraction => "CG_EXPIRED_INTERACTION",
            Self::ConsentRequired => "CG_CONSENT_REQUIRED",
            Self::AuthorityDenied => "CG_POLICY_DENIED",
            Self::LimitExceeded => "CG_LIMIT_EXCEEDED",
            Self::OutcomeUnknown => "CG_OUTCOME_UNKNOWN",
            Self::StorageUnavailable => "CG_STORAGE_UNAVAILABLE",
        }
    }
}
impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for SessionError {}

/// The same driving application port is consumed by CLI and MCP adapters.
pub trait SessionApplicationPort {
    fn execute(
        &self,
        owner: &OwnerBinding,
        command: SessionCommand,
    ) -> Result<SessionSnapshot, SessionError>;
    fn inspect(
        &self,
        owner: &OwnerBinding,
        target: InspectTarget,
    ) -> Result<SessionSnapshot, SessionError>;
}
