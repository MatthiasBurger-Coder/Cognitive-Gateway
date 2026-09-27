//! CG-20 data quality and governed, reference-only learning handoff.
use crate::memory::{MemoryApplication, MemoryError, MemoryStore};
use gateway_domain::{
    ContextScopeId, ReferenceId, SensitivityClass, TrustClass, UnixTimestamp,
    memory::{ExperienceRecord, MEMORY_SCHEMA_VERSION, MemoryEligibilityReference, MemoryPayload},
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const CURATED_SNAPSHOT_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DataDefect {
    MissingLabel,
    MissingOutcome,
    DuplicateSnapshot,
    NearDuplicate,
    Conflict,
    Stale,
    Untrusted,
    InvalidSchema,
    SensitiveInline,
}
impl DataDefect {
    pub const ALL: [Self; 9] = [
        Self::MissingLabel,
        Self::MissingOutcome,
        Self::DuplicateSnapshot,
        Self::NearDuplicate,
        Self::Conflict,
        Self::Stale,
        Self::Untrusted,
        Self::InvalidSchema,
        Self::SensitiveInline,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataQualityProfile {
    pub records: usize,
    pub defects: BTreeMap<DataDefect, BTreeSet<ReferenceId>>,
    /// Near-duplicate applicability is inline payload count; all other
    /// denominators use the scoped input record count.
    pub denominators: BTreeMap<DataDefect, usize>,
    /// No imputation: empty corpora have no numeric distribution.
    pub age_range_seconds: Option<(i64, i64)>,
    pub sensitivity_counts: BTreeMap<SensitivityClass, usize>,
}

/// Diagnostics retain IDs and counts, never raw payloads.
pub fn profile(records: &[ExperienceRecord], at: UnixTimestamp) -> DataQualityProfile {
    let mut defects: BTreeMap<DataDefect, BTreeSet<ReferenceId>> = BTreeMap::new();
    let mut snapshots = BTreeMap::new();
    let mut normalized = BTreeMap::new();
    let mut ages = Vec::new();
    let mut sensitivity_counts = BTreeMap::new();
    let mut inline_count = 0;
    for record in records {
        let id = record.id.clone();
        let mut mark = |defect| {
            defects.entry(defect).or_default().insert(id.clone());
        };
        if record.label_basis.is_none() {
            mark(DataDefect::MissingLabel);
        }
        if record.outcome.is_none() {
            mark(DataDefect::MissingOutcome);
        }
        if record.schema_version != MEMORY_SCHEMA_VERSION || record.validate().is_err() {
            mark(DataDefect::InvalidSchema);
        }
        if record.quality.conflict() != gateway_domain::ConflictStatus::None {
            mark(DataDefect::Conflict);
        }
        if record.quality.freshness() != gateway_domain::FreshnessStatus::Fresh
            || at.seconds() < record.observed_at.seconds()
            || at.seconds().saturating_sub(record.observed_at.seconds())
                > i64::try_from(record.max_age_seconds).unwrap_or(i64::MAX)
        {
            mark(DataDefect::Stale);
        }
        if record.quality.trust() != TrustClass::DerivedAssessment {
            mark(DataDefect::Untrusted);
        }
        if record.quality.sensitivity() >= SensitivityClass::Confidential
            && matches!(record.payload, Some(MemoryPayload::Inline(_)))
        {
            mark(DataDefect::SensitiveInline);
        }
        if let Some(previous) =
            snapshots.insert((&record.scope, &record.source_snapshot), id.clone())
        {
            defects
                .entry(DataDefect::DuplicateSnapshot)
                .or_default()
                .extend([previous, id.clone()]);
        }
        if let Some(MemoryPayload::Inline(value)) = &record.payload {
            inline_count += 1;
            let text = value
                .as_str()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            if let Some(previous) = normalized.insert((&record.scope, text), id.clone()) {
                defects
                    .entry(DataDefect::NearDuplicate)
                    .or_default()
                    .extend([previous, id.clone()]);
            }
        }
        *sensitivity_counts
            .entry(record.quality.sensitivity())
            .or_insert(0) += 1;
        ages.push(at.seconds().saturating_sub(record.observed_at.seconds()));
    }
    let age_range_seconds =
        (!ages.is_empty()).then(|| (*ages.iter().min().unwrap(), *ages.iter().max().unwrap()));
    let denominators = DataDefect::ALL
        .into_iter()
        .map(|defect| {
            (
                defect,
                if defect == DataDefect::NearDuplicate {
                    inline_count
                } else {
                    records.len()
                },
            )
        })
        .collect();
    DataQualityProfile {
        records: records.len(),
        defects,
        denominators,
        age_range_seconds,
        sensitivity_counts,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratedItem {
    pub eligibility: MemoryEligibilityReference,
    pub observed_at: UnixTimestamp,
    pub created_at: UnixTimestamp,
    pub validation: ReferenceId,
    pub label_basis: ReferenceId,
    /// Sensitive outcome text stays behind the governed memory boundary.
    pub outcome: Option<String>,
    pub sensitivity: SensitivityClass,
    /// Sensitive data and ordinary payloads are omitted from export alike.
    pub payload_reference: Option<ReferenceId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratedSnapshot {
    pub version: u16,
    pub scope: ContextScopeId,
    pub source_revision: String,
    pub generated_at: UnixTimestamp,
    pub items: Vec<CuratedItem>,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    Memory(MemoryError),
    InvalidSourceRevision,
    Empty,
    Revoked,
}

/// Export only current learning-eligible references. Consumers must call
/// `revalidate_snapshot` immediately before use; the digest is not a grant.
pub fn export_snapshot<S: MemoryStore>(
    memory: &MemoryApplication<S>,
    scope: &ContextScopeId,
    at: UnixTimestamp,
    source_revision: &str,
) -> Result<CuratedSnapshot, ExportError> {
    if source_revision.trim().is_empty() {
        return Err(ExportError::InvalidSourceRevision);
    }
    let mut items = Vec::new();
    for entry in memory.store().list(scope).map_err(ExportError::Memory)? {
        if entry.learning_reasons(at) != [gateway_domain::memory::MemoryReason::Eligible] {
            continue;
        }
        let eligibility = memory
            .eligibility_reference(scope, &entry.record.id, at)
            .map_err(ExportError::Memory)?;
        let record = entry.record;
        items.push(CuratedItem {
            eligibility,
            observed_at: record.observed_at,
            created_at: record.created_at,
            validation: record.validation.expect("learning eligible"),
            label_basis: record.label_basis.expect("learning eligible"),
            outcome: (record.quality.sensitivity() < SensitivityClass::Confidential).then(|| {
                record
                    .outcome
                    .expect("learning eligible")
                    .as_str()
                    .to_owned()
            }),
            sensitivity: record.quality.sensitivity(),
            payload_reference: match record.payload {
                Some(MemoryPayload::Reference(id)) => Some(id),
                _ => None,
            },
        });
    }
    items.sort_by(|a, b| a.eligibility.id.cmp(&b.eligibility.id));
    if items.is_empty() {
        return Err(ExportError::Empty);
    }
    let mut snapshot = CuratedSnapshot {
        version: CURATED_SNAPSHOT_VERSION,
        scope: scope.clone(),
        source_revision: source_revision.to_owned(),
        generated_at: at,
        items,
        digest: String::new(),
    };
    snapshot.digest = snapshot_digest(&snapshot);
    Ok(snapshot)
}

pub fn revalidate_snapshot<S: MemoryStore>(
    memory: &MemoryApplication<S>,
    snapshot: &CuratedSnapshot,
    at: UnixTimestamp,
) -> Result<(), ExportError> {
    if snapshot.version != CURATED_SNAPSHOT_VERSION
        || snapshot.items.is_empty()
        || snapshot.digest != snapshot_digest(snapshot)
        || snapshot
            .items
            .windows(2)
            .any(|pair| pair[0].eligibility.id >= pair[1].eligibility.id)
        || snapshot
            .items
            .iter()
            .any(|item| item.eligibility.scope != snapshot.scope)
    {
        return Err(ExportError::Revoked);
    }
    for item in &snapshot.items {
        if !memory
            .revalidate_reference(&item.eligibility, at)
            .map_err(ExportError::Memory)?
        {
            return Err(ExportError::Revoked);
        }
        let entry = memory
            .store()
            .get(&snapshot.scope, &item.eligibility.id)
            .map_err(ExportError::Memory)?
            .ok_or(ExportError::Revoked)?;
        let record = entry.record;
        if record.observed_at != item.observed_at
            || record.created_at != item.created_at
            || record.validation.as_ref() != Some(&item.validation)
            || record.label_basis.as_ref() != Some(&item.label_basis)
            || if item.sensitivity >= SensitivityClass::Confidential {
                item.outcome.is_some() || record.outcome.is_none()
            } else {
                record.outcome.as_ref().map(|value| value.as_str()) != item.outcome.as_deref()
            }
            || record.quality.sensitivity() != item.sensitivity
            || match (&record.payload, &item.payload_reference) {
                (Some(MemoryPayload::Reference(actual)), Some(expected)) => actual != expected,
                (Some(MemoryPayload::Inline(_)), None) => false,
                _ => true,
            }
        {
            return Err(ExportError::Revoked);
        }
    }
    Ok(())
}

fn snapshot_digest(snapshot: &CuratedSnapshot) -> String {
    let mut hasher = Sha256::new();
    hasher.update(snapshot.version.to_be_bytes());
    put(&mut hasher, snapshot.scope.as_str());
    put(&mut hasher, &snapshot.source_revision);
    hasher.update(snapshot.generated_at.seconds().to_be_bytes());
    for item in &snapshot.items {
        put(&mut hasher, item.eligibility.id.as_str());
        hasher.update(item.eligibility.revision.to_be_bytes());
        hasher.update(item.eligibility.eligibility_version.to_be_bytes());
        put(&mut hasher, item.eligibility.source_snapshot.as_str());
        put(&mut hasher, item.eligibility.source_digest.as_str());
        hasher.update(item.observed_at.seconds().to_be_bytes());
        hasher.update(item.created_at.seconds().to_be_bytes());
        put(&mut hasher, item.validation.as_str());
        put(&mut hasher, item.label_basis.as_str());
        put(&mut hasher, item.outcome.as_deref().unwrap_or(""));
        put(&mut hasher, item.sensitivity.as_str());
        put(
            &mut hasher,
            item.payload_reference
                .as_ref()
                .map_or("", ReferenceId::as_str),
        );
    }
    format!("{:x}", hasher.finalize())
}

fn put(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}
