//! CG-29 immutable, advisory cognitive work contracts. No lifecycle commands.
use crate::{ContextScopeId, ReferenceId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const WORK_VERSION: u16 = 1;
pub const MAX_SNAPSHOT_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkKind {
    Retrieval,
    Pattern,
    Evaluation,
    Model,
}

/// All resource reservations apply to each attempt, including failed attempts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkBudget {
    pub max_attempts: u16,
    pub lease_ms: u64,
    pub retry_delay_ms: u64,
    pub deadline_ms: u64,
    pub memory_bytes: u64,
    pub compute_units: u64,
    pub max_result_bytes: usize,
}

/// Bytes are copied into the item; no live source lookup is permitted in workers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkSpec {
    pub version: u16,
    pub scope: ContextScopeId,
    pub trace: ReferenceId,
    /// Distinguishes intentional repeated operations over identical inputs.
    pub operation: ReferenceId,
    pub kind: WorkKind,
    pub snapshot: Vec<u8>,
    pub sources: BTreeSet<ReferenceId>,
    /// Exact runtime/model/artifact revision, never an unversioned alias.
    pub runtime: ReferenceId,
    pub model: Option<ReferenceId>,
    pub budget: WorkBudget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkItem {
    id: String,
    snapshot_digest: String,
    spec: WorkSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FabricError {
    InvalidContract,
    Backpressure,
    UnknownWork,
    ScopeMismatch,
    InvalidWorker,
    StaleLease,
    InvalidResult,
    ClockRegression,
    CounterExhausted,
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl WorkItem {
    pub fn new(spec: WorkSpec) -> Result<Self, FabricError> {
        Self::validate_spec(&spec)?;
        let snapshot_digest = digest(&spec.snapshot);
        let id = digest(&serde_json::to_vec(&spec).map_err(|_| FabricError::InvalidContract)?);
        let item = Self {
            id,
            snapshot_digest,
            spec,
        };
        item.validate()?;
        Ok(item)
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn snapshot_digest(&self) -> &str {
        &self.snapshot_digest
    }
    pub fn spec(&self) -> &WorkSpec {
        &self.spec
    }
    // Check sizes before hashing or allocating canonical serialized bytes.
    fn validate_spec(spec: &WorkSpec) -> Result<(), FabricError> {
        let b = &spec.budget;
        if spec.version != WORK_VERSION
            || spec.snapshot.is_empty()
            || spec.snapshot.len() > MAX_SNAPSHOT_BYTES
            || spec.sources.is_empty()
            || spec.sources.len() > 64
            || b.max_attempts == 0
            || b.max_attempts > 64
            || b.lease_ms == 0
            || b.deadline_ms == 0
            || b.memory_bytes == 0
            || b.compute_units == 0
            || b.max_result_bytes == 0
            || b.max_result_bytes > MAX_SNAPSHOT_BYTES
            || b.compute_units
                .checked_mul(u64::from(b.max_attempts))
                .is_none()
            || (spec.kind == WorkKind::Model && spec.model.is_none())
        {
            return Err(FabricError::InvalidContract);
        }
        Ok(())
    }
    /// Every transport admission revalidates identity; Deserialize is not trust.
    pub fn validate(&self) -> Result<(), FabricError> {
        Self::validate_spec(&self.spec)?;
        if self.snapshot_digest != digest(&self.spec.snapshot)
            || self.id
                != digest(
                    &serde_json::to_vec(&self.spec).map_err(|_| FabricError::InvalidContract)?,
                )
        {
            return Err(FabricError::InvalidContract);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerAdvertisement {
    pub worker: ReferenceId,
    pub node: ReferenceId,
    /// Reference adapter is dedicated to a single project, even when idle.
    pub scope: ContextScopeId,
    pub kinds: BTreeSet<WorkKind>,
    pub runtimes: BTreeSet<ReferenceId>,
    pub models: BTreeSet<ReferenceId>,
    pub slots: usize,
    pub memory_bytes: u64,
    pub compute_units: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkLease {
    pub item: WorkItem,
    pub worker: ReferenceId,
    pub node: ReferenceId,
    /// Fencing token: new on every assignment; old attempts cannot commit.
    pub token: u64,
    pub attempt: u16,
    pub expires_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkProvenance {
    pub worker: ReferenceId,
    pub node: ReferenceId,
    pub runtime: ReferenceId,
    pub model: Option<ReferenceId>,
    pub sources: BTreeSet<ReferenceId>,
    pub snapshot_digest: String,
}

/// Opaque proposal bytes. Acceptance never changes Process/Policy/promotion state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkResult {
    pub work_id: String,
    pub scope: ContextScopeId,
    pub trace: ReferenceId,
    pub token: u64,
    pub attempt: u16,
    pub proposal: Vec<u8>,
    pub compute_units: u64,
    pub provenance: WorkProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureReason {
    Timeout,
    WorkerLost,
    Execution,
    InvalidResult,
    Deadline,
    AttemptsExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkState {
    Queued {
        ready_ms: u64,
    },
    Leased {
        worker: ReferenceId,
        token: u64,
        expires_ms: u64,
    },
    Completed,
    Failed {
        reason: FailureReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkStatus {
    pub work_id: String,
    pub scope: ContextScopeId,
    pub trace: ReferenceId,
    pub snapshot_digest: String,
    pub state: WorkState,
    pub attempts: u16,
    /// Reservations count in full even if a node disappears without telemetry.
    pub reserved_compute_units: u64,
    pub last_failure: Option<FailureReason>,
    pub result: Option<WorkResult>,
}
