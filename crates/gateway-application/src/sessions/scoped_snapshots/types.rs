//! Snapshot inputs are evidence data, never current execution authority.
use crate::{ScopedObservationBatch, connectors::*, sessions::*};
use gateway_domain::*;
use std::collections::{BTreeMap, BTreeSet};

pub type SnapshotPin = ResolvedReference<ReferenceId>;
pub const MAX_SNAPSHOT_BYTES: usize = 8 * 1_048_576;
pub const MAX_SNAPSHOT_DEPTH: usize = 64;
pub const MAX_SNAPSHOT_OBJECTS: usize = 256;
pub const MAX_SNAPSHOT_ACQUISITIONS: usize = 258;
pub const MAX_SNAPSHOT_SUBJECTS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    InvalidEncoding,
    LimitExceeded,
    BindingMismatch,
    UniverseMismatch,
    ProfileMismatch,
    DigestMismatch,
    InvalidEvidence,
    Incomplete,
    Inconsistent,
    Expired,
    Conflict,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotPhase {
    Before,
    After,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotReservation {
    pub reservation: ContentDigest,
    pub fence: FenceToken,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotBinding {
    pub capture: ResourceReservationBinding,
    pub phase: SnapshotPhase,
    pub task: TaskId,
    pub execution: NonEmptyText,
    pub reservation: Option<SnapshotReservation>,
}
/// A static read definition excludes runtime correlation/session coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotReadRoute {
    pub pin: SnapshotPin,
    pub authority_domain: SnapshotPin,
    pub target: ConnectorTarget,
    pub capability: CapabilityId,
    pub class: ConnectorOperationClass,
    pub arguments: BTreeMap<NonEmptyText, TypedValue>,
    pub object_reference: NonEmptyText,
    pub epoch_argument: Option<NonEmptyText>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotObject {
    pub object: SnapshotPin,
    pub source: SourceId,
    pub source_kind: SourceKind,
    /// Leaf identity, distinct from the actual atomic export root.
    pub object_reference: NonEmptyText,
    pub subjects: BTreeSet<SubjectPath>,
    /// The object token must be present in actual native observation records.
    pub version_subject: SubjectPath,
    pub route: SnapshotReadRoute,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotUniverse {
    pub pin: SnapshotPin,
    pub objects: Vec<SnapshotObject>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotProfileKind {
    AtomicCompleteExport,
    SnapshotScopedReads,
    BracketedNonReusedAnchor,
}
/// These are explicit qualified-provider DATA contracts. They do not authenticate
/// an arbitrary caller or establish that a live provider implements the profile.
/// The actual qualified acquisition owner is a mandatory separate consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotProfile {
    pub pin: SnapshotPin,
    pub authority_domain: SnapshotPin,
    pub kind: SnapshotProfileKind,
    pub anchor_source: SourceId,
    pub anchor_source_kind: SourceKind,
    pub anchor_route: SnapshotReadRoute,
    pub epoch_subject: SubjectPath,
    pub membership_subject: SubjectPath,
    pub max_age_seconds: u64,
}
/// One actual acquisition can cover multiple objects. Its native provenance and
/// export root remain intact; no per-object connector observations are invented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotAcquisition {
    pub request: ConnectorRequest,
    pub observation: ConnectorObservation,
    pub batch: ScopedObservationBatch,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotObjectRead {
    pub object: SnapshotPin,
    pub source: SourceId,
    pub source_kind: SourceKind,
    pub acquisition: ContentDigest,
    pub version_observation: ObservationId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotAnchor {
    pub domain: SnapshotPin,
    pub epoch: SourceTimestamp,
    pub membership: ContentDigest,
    pub acquisition: ContentDigest,
    pub epoch_observation: ObservationId,
    pub membership_observation: ObservationId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotConsistency {
    Atomic {
        anchor: SnapshotAnchor,
    },
    SnapshotScoped {
        anchor: SnapshotAnchor,
    },
    Bracketed {
        before: SnapshotAnchor,
        after: Box<SnapshotAnchor>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotObjectVersion {
    pub object: SnapshotPin,
    pub source: SourceId,
    pub source_kind: SourceKind,
    pub version: SourceTimestamp,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SnapshotData {
    pub binding: SnapshotBinding,
    pub universe: SnapshotUniverse,
    pub profile: SnapshotProfile,
    pub acquisitions: Vec<SnapshotAcquisition>,
    pub object_reads: Vec<SnapshotObjectRead>,
    pub consistency: SnapshotConsistency,
}
