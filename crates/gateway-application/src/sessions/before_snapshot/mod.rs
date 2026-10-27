//! Coordinator-owned immutable Before references; these data never grant execution.
use super::scoped_snapshots::{
    CompleteScopedSnapshot, SnapshotBinding, SnapshotPhase, SnapshotPin,
};
use super::*;
use gateway_domain::{ContentDigest, SchemaVersion};
use serde::{Deserialize, Serialize};
mod codec;
pub use codec::*;

pub const MAX_BEFORE_RECORD_BYTES: usize = 1_000_000;
/// Validated captured data, supplied only by the trusted readonly preparation facet.
/// The coordinator alone chooses validation time, persists and mints the reference.
pub struct CapturedBefore {
    pub snapshot: CompleteScopedSnapshot,
    pub runtime: SnapshotPin,
    pub native_context_digest: ContentDigest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedBeforeRef {
    pub binding: SnapshotBinding,
    pub universe: SnapshotPin,
    pub profile: SnapshotPin,
    pub runtime: SnapshotPin,
    pub native_context_digest: ContentDigest,
    pub snapshot_digest: ContentDigest,
    pub record: RecordRef,
    pub committed_revision: Revision,
    pub validated_at_ms: u64,
}
impl PreparedBeforeRef {
    pub(crate) fn validate_for(&self, cp: &SessionCheckpoint) -> Result<(), SessionError> {
        let goal = cp.goal.as_connector().ok_or(SessionError::InvalidState)?;
        let b = &self.binding.capture;
        let reservation = self
            .binding
            .reservation
            .as_ref()
            .ok_or(SessionError::InvalidInteraction)?;
        if self.binding.phase != SnapshotPhase::Before
            || b.owner != cp.owner
            || b.scope != *goal.scope()
            || b.session != cp.snapshot.session
            || b.run != cp.snapshot.run
            || !goal.steps().contains(&b.step)
            || b.revision.value() == 0
            || self.committed_revision != b.revision.next()?
            || self.committed_revision > cp.snapshot.revision
            || cp.snapshot.dispatch.as_ref() != Some(&b.dispatch)
            || self.validated_at_ms == 0
            || self.validated_at_ms >= cp.budget.deadline_ms
            || [&self.universe, &self.profile, &self.runtime]
                .iter()
                .any(|p| p.scope != b.scope || p.contract_version != SchemaVersion::V1)
            || !cp.resource_ledger.as_ref().is_some_and(|l| {
                l.reservations()
                    .iter()
                    .any(|r| r.id() == &reservation.reservation && r.binding() == b)
            })
        {
            return Err(SessionError::InvalidInteraction);
        }
        Ok(())
    }
}

/// Recheck time after intervening storage lookups without rewriting the data or
/// repeating expensive encoding. The sealed snapshot already validated quality.
pub(crate) fn before_fresh_at(
    snapshot: &CompleteScopedSnapshot,
    now: u64,
) -> Result<(), SessionError> {
    let age = snapshot
        .profile()
        .max_age_seconds
        .checked_mul(1000)
        .ok_or(SessionError::LimitExceeded)?;
    for acquisition in snapshot.acquisitions() {
        let metadata = &acquisition.observation.metadata;
        let observed = u64::try_from(
            metadata
                .observed_at
                .ok_or(SessionError::InvalidInteraction)?
                .seconds(),
        )
        .map_err(|_| SessionError::ExpiredInteraction)?
        .checked_mul(1000)
        .ok_or(SessionError::LimitExceeded)?;
        let acquired = u64::try_from(acquisition.observation.acquired_at.seconds())
            .map_err(|_| SessionError::ExpiredInteraction)?
            .checked_mul(1000)
            .ok_or(SessionError::LimitExceeded)?;
        let until = observed
            .checked_add(age)
            .ok_or(SessionError::LimitExceeded)?;
        if now < acquired || now >= until {
            return Err(SessionError::ExpiredInteraction);
        }
    }
    Ok(())
}
