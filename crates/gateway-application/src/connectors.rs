//! EPIC-07.01 v1 architectural ports. No transport or authorization implementation.
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
pub const CONNECTOR_PORT_VERSION: &str = "1";

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
