//! EPIC-07 v2 architectural ports with canonical result metadata.
//!
//! Requests are proposals until the application checks current policy. Public
//! binding records are audit data, not unforgeable grants. See the normative
//! `docs/mcp-connector-runtime.md` preconditions for every implementation.

use std::collections::BTreeMap;

use gateway_domain::{
    CapabilityId, ContentDigest, ContextScopeId, NonEmptyText, ObservationEvidenceSet, PlanStepId,
    PolicyId, SourceId, SourceTimestamp, TaskId, TypedValue, UnixTimestamp,
};

/// Architecture contract revision; independent of the external protocol version.
pub const CONNECTOR_PORT_VERSION: &str = "2";

/// Opaque operator-managed reference to credentials. This identifier is safe to
/// persist; it must never contain a credential value or be treated as authority.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CredentialProfileRef(NonEmptyText);

impl CredentialProfileRef {
    pub fn new(value: NonEmptyText) -> Self {
        Self(value)
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Narrow credential use to one configured connector/server and optional tools.
/// Empty tool set means no tool is authorized; wildcard expansion is forbidden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialScope {
    pub connector: NonEmptyText,
    pub server: NonEmptyText,
    pub tools: std::collections::BTreeSet<NonEmptyText>,
}
impl CredentialScope {
    pub fn permits(&self, connector: &str, server: &str, tool: &str) -> bool {
        self.connector.as_str() == connector
            && self.server.as_str() == server
            && self.tools.iter().any(|allowed| allowed.as_str() == tool)
    }
}

/// Runtime-only resolved material. Kept in the outer adapter and deliberately
/// has no Debug/serde representation. Never copy into requests or observations.
pub struct ResolvedCredential {
    pub environment: BTreeMap<String, String>,
    pub scope: CredentialScope,
    pub generation: u64,
}
impl ResolvedCredential {
    pub fn new(
        environment: BTreeMap<String, String>,
        scope: CredentialScope,
        generation: u64,
    ) -> Self {
        Self {
            environment,
            scope,
            generation,
        }
    }
}

/// Environment/keyring/secret-manager adapters implement this outer boundary.
/// Resolution is performed per connection so rotation takes effect on reconnect.
pub trait ConnectorSecretResolver: Send + Sync {
    fn resolve(
        &self,
        profile: &CredentialProfileRef,
    ) -> Result<ResolvedCredential, ConnectorFailureCode>;
}

/// A mapped operation class. Unknown mappings have no executable representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorOperationClass {
    Read,
    Search,
    Inspect,
    Mutate,
    Admin,
}

/// Identity of one admitted connector instance in one application scope.
/// Manifest/version compatibility and secret resolution remain outer concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorTarget {
    pub connector: NonEmptyText,
    pub scope: ContextScopeId,
}

/// Deterministically mapped entry; neither discovery nor classification grants access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorCapability {
    pub capability: CapabilityId,
    pub class: ConnectorOperationClass,
    /// Digest of the executable argument schema; descriptive metadata is
    /// excluded, while the complete inventory revision binds the full schema.
    pub argument_schema_digest: ContentDigest,
    pub result_schema_digest: ContentDigest,
}

/// Bounded complete inventory, sorted by canonical capability ID, with no duplicates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorInventory {
    pub target: ConnectorTarget,
    pub revision: ContentDigest,
    pub capabilities: Vec<ConnectorCapability>,
    pub resources: Vec<SourceId>,
}

/// Canonical typed arguments; no tool names, wire URIs, SDK objects or secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorRequest {
    pub target: ConnectorTarget,
    pub capability: CapabilityId,
    pub class: ConnectorOperationClass,
    pub inventory_revision: ContentDigest,
    pub arguments: BTreeMap<NonEmptyText, TypedValue>,
    pub task: TaskId,
    pub step: PlanStepId,
    pub execution: NonEmptyText,
    pub correlation: NonEmptyText,
}

/// Application-issued bindings to current authorization and aggregate accounting.
/// Implementations MUST validate them against trusted current state; possession
/// or construction of this record alone never authorizes dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorDispatch {
    pub request: ConnectorRequest,
    pub policy: PolicyId,
    pub policy_revision: ContentDigest,
    pub consent_receipt: Option<NonEmptyText>,
    pub budget_reservation: NonEmptyText,
    pub deadline: UnixTimestamp,
}

/// Candidate lineage for CG-06 validation; no completion or trust assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorObservation {
    pub metadata: crate::connector_results::ConnectorResultMetadata,
    pub target: ConnectorTarget,
    pub inventory_revision: ContentDigest,
    pub correlation: NonEmptyText,
    pub source: SourceId,
    pub source_revision: Option<SourceTimestamp>,
    pub content_digest: ContentDigest,
    pub acquired_at: UnixTimestamp,
    pub records: ObservationEvidenceSet,
    pub basis: ConnectorObservationBasis,
}

/// Acknowledgement and post-effect verification must remain distinguishable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorObservationBasis {
    ProviderAcknowledgement,
    SourceRead,
    IndependentPostEffectRead,
}

/// Effect uncertainty survives failures, including cancellation and disconnection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorEffect {
    NotDispatched,
    NoMutation,
    Acknowledged,
    Unknown,
}

/// Content-free application diagnostic vocabulary; raw errors stay in outer adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorFailureCode {
    Unavailable,
    AuthenticationFailed,
    CredentialUnavailable,
    CredentialScopeDenied,
    SecretDisclosure,
    PermissionDenied,
    Timeout,
    Cancelled,
    ProtocolError,
    IncompatibleVersion,
    SchemaMismatch,
    RateLimited,
    NotFound,
    StaleResult,
    ConflictingResult,
    MutationUncertain,
    RetryUnsafe,
    ScopeMismatch,
    InventoryChanged,
    BudgetExhausted,
    Backpressure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorFailure {
    pub code: ConnectorFailureCode,
    pub effect: ConnectorEffect,
    pub correlation: NonEmptyText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorLifecycleCommand {
    Connect,
    Health,
    Disable,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorLifecycleState {
    Disconnected,
    Ready,
    Degraded,
    Disabled,
}

/// Driving application port. All calls require admitted scope; operation proposals
/// additionally require current policy, consent, schema and budget checks.
pub trait ConnectorApplicationPort {
    fn discover(&self, target: &ConnectorTarget) -> Result<ConnectorInventory, ConnectorFailure>;
    fn invoke(&self, request: &ConnectorRequest) -> Result<ConnectorObservation, ConnectorFailure>;
    fn read_resource(
        &self,
        request: &ConnectorRequest,
        source: &SourceId,
    ) -> Result<ConnectorObservation, ConnectorFailure>;
    fn lifecycle(
        &self,
        target: &ConnectorTarget,
        command: ConnectorLifecycleCommand,
    ) -> Result<ConnectorLifecycleState, ConnectorFailure>;
}

/// Driven runtime port implemented by outer MCP adapters. It cannot bypass the
/// driving application port; lifecycle/discovery require configured admission.
///
/// A proposal cannot be passed where a bound dispatch is required:
///
/// ```compile_fail
/// use gateway_application::connectors::{ConnectorRequest, ConnectorRuntimePort};
/// fn dispatch_proposal(runtime: &dyn ConnectorRuntimePort, proposal: &ConnectorRequest) {
///     let _ = runtime.invoke(proposal);
/// }
/// ```
pub trait ConnectorRuntimePort {
    fn discover(&self, target: &ConnectorTarget) -> Result<ConnectorInventory, ConnectorFailure>;
    fn invoke(
        &self,
        dispatch: &ConnectorDispatch,
    ) -> Result<ConnectorObservation, ConnectorFailure>;
    fn read_resource(
        &self,
        dispatch: &ConnectorDispatch,
        source: &SourceId,
    ) -> Result<ConnectorObservation, ConnectorFailure>;
    fn lifecycle(
        &self,
        target: &ConnectorTarget,
        command: ConnectorLifecycleCommand,
    ) -> Result<ConnectorLifecycleState, ConnectorFailure>;
}

/// Trusted host authority injected at composition, never deserialized from a
/// proposal. Implementations own current policy/consent, scope, source binding
/// and durable aggregate budget checks. There is deliberately no permissive
/// default. The runtime calls `revalidate` immediately before transport effects.
pub trait ConnectorAdmissionPort: Send + Sync {
    /// Bounded, read-only current target admission, also used during transport
    /// liveness polling. Must not dispatch effects, acquire grants or record
    /// audit decisions; authorization owns those decision boundaries.
    fn admit_target(&self, target: &ConnectorTarget) -> Result<(), ConnectorFailure>;
    fn authorize(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorDispatch, ConnectorFailure>;
    fn revalidate(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure>;
    /// Validation-only current admission for bounded transport liveness polls.
    /// Implementations must read current facts without recording a decision or
    /// acquiring a new grant. Legacy admission sources deny this guarded route
    /// until they explicitly implement a current validation boundary.
    fn validate_current_dispatch(
        &self,
        dispatch: &ConnectorDispatch,
        _source: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        Err(ConnectorFailure {
            code: ConnectorFailureCode::PermissionDenied,
            effect: ConnectorEffect::NotDispatched,
            correlation: dispatch.request.correlation.clone(),
        })
    }
}
