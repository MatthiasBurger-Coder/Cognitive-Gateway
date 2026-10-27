//! Complete scoped snapshot DATA. This does not admit effects, establish current
//! authority, prove a goal, or assert causal After acquisition. The future live
//! consumer must supply authenticated acquisitions and persist Before legally.
pub(in crate::sessions) mod codec;
mod identity;
mod types;
mod validation;
use types::SnapshotData;
pub use types::*;

/// A structural completeness proof for the exact authenticated data supplied by
/// its owner. No Deserialize or caller-supplied aggregate digest can mint it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteScopedSnapshot {
    data: SnapshotData,
    digest: gateway_domain::ContentDigest,
    object_versions: Vec<SnapshotObjectVersion>,
}
impl CompleteScopedSnapshot {
    pub fn validate(
        binding: SnapshotBinding,
        universe: SnapshotUniverse,
        profile: SnapshotProfile,
        acquisitions: Vec<SnapshotAcquisition>,
        object_reads: Vec<SnapshotObjectRead>,
        consistency: SnapshotConsistency,
        now_ms: u64,
    ) -> Result<Self, SnapshotError> {
        let data = SnapshotData {
            binding,
            universe,
            profile,
            acquisitions,
            object_reads,
            consistency,
        };
        let object_versions = validation::validate(&data, now_ms)?;
        let bytes = codec::canonical_input(&data)?;
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(SnapshotError::LimitExceeded);
        }
        let digest = gateway_domain::context_content_digest(&bytes);
        Ok(Self {
            data,
            digest,
            object_versions,
        })
    }
    pub fn binding(&self) -> &SnapshotBinding {
        &self.data.binding
    }
    pub fn universe(&self) -> &SnapshotUniverse {
        &self.data.universe
    }
    pub fn profile(&self) -> &SnapshotProfile {
        &self.data.profile
    }
    pub fn acquisitions(&self) -> &[SnapshotAcquisition] {
        &self.data.acquisitions
    }
    pub fn object_reads(&self) -> &[SnapshotObjectRead] {
        &self.data.object_reads
    }
    pub fn consistency(&self) -> &SnapshotConsistency {
        &self.data.consistency
    }
    pub fn digest(&self) -> &gateway_domain::ContentDigest {
        &self.digest
    }
    pub fn object_versions(&self) -> &[SnapshotObjectVersion] {
        &self.object_versions
    }
}
