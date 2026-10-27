//! Deterministic aggregation of retained scoped evidence, never an effect grant.
use crate::{ScopedObservationBatch, SourceSnapshot};
use gateway_domain::*;
use serde_json::json;
use std::collections::BTreeMap;

pub const MAX_RETAINED_OBSERVATION_MEMBERS: usize = 256;

/// Binding hashes the complete authenticated member identity. Version domain
/// identifies the real source/object, independently of its version or condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedObservationMember {
    pub binding: ContentDigest,
    pub version_domain: ContentDigest,
    pub batch: ScopedObservationBatch,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetainedObservationError {
    Empty,
    Limit,
    ScopeMismatch,
    DuplicateBinding,
    DuplicateIdentity,
    InconsistentVersion,
    InvalidRecords,
    InvalidQuality,
}

/// Only construction from actual validated members can establish this digest.
/// No deserialization or caller-supplied aggregate digest is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedObservationReadset {
    scope: ContextScopeId,
    digest: ContentDigest,
    batch: ScopedObservationBatch,
}
impl RetainedObservationReadset {
    pub fn new(
        scope: ContextScopeId,
        members: Vec<RetainedObservationMember>,
    ) -> Result<Self, RetainedObservationError> {
        use RetainedObservationError as E;
        if members.is_empty() {
            return Err(E::Empty);
        }
        if members.len() > MAX_RETAINED_OBSERVATION_MEMBERS {
            return Err(E::Limit);
        }
        let mut unique = BTreeMap::new();
        for member in members {
            if member.batch.scope() != &scope {
                return Err(E::ScopeMismatch);
            }
            if let Some(previous) = unique.insert(member.binding.clone(), member.clone()) {
                if previous != member {
                    return Err(E::DuplicateBinding);
                }
            }
        }
        let mut versions = BTreeMap::new();
        let mut provenances = BTreeMap::new();
        let mut observations = BTreeMap::new();
        let mut facts = BTreeMap::new();
        let mut evidence = BTreeMap::new();
        let mut quality: BTreeMap<SubjectPath, Vec<QualityMetadata>> = BTreeMap::new();
        let mut manifest = Vec::new();
        for member in unique.values() {
            let batch = &member.batch;
            batch.records().validate().map_err(|_| E::InvalidRecords)?;
            // Each member must qualify its own facts. A fresh claim from
            // another source cannot fill an absent subject-quality declaration.
            if batch.records().facts().iter().any(|fact| {
                batch
                    .quality_metadata()
                    .get(fact.subject())
                    .is_none_or(Vec::is_empty)
            }) {
                return Err(E::InvalidQuality);
            }
            let source = batch.snapshot();
            let domain = (
                source.source_kind().as_str(),
                source.source_id().clone(),
                member.version_domain.clone(),
            );
            if let Some(previous) = versions.insert(domain, source.clone()) {
                let consistent = match (previous.change_token(), source.change_token()) {
                    (Some(old), Some(current)) => old == current,
                    (None, None) => previous.digest() == source.digest(),
                    _ => false,
                };
                if !consistent {
                    return Err(E::InconsistentVersion);
                }
            }
            for item in batch.records().provenances() {
                insert(&mut provenances, item.id().clone(), item)?;
            }
            for item in batch.records().observations() {
                insert(&mut observations, item.id().clone(), item)?;
            }
            for item in batch.records().facts() {
                insert(&mut facts, item.id().clone(), item)?;
            }
            for item in batch.records().evidence() {
                insert(&mut evidence, item.id().clone(), item)?;
            }
            for (subject, metadata) in batch.quality_metadata() {
                if metadata.iter().any(|item| {
                    item.trust() != TrustClass::ObservedEvidence
                        || item.freshness() != FreshnessStatus::Fresh
                        || item.conflict() == ConflictStatus::Unresolved
                        || item.uncertainty() == Uncertainty::Unknown
                        || item.sensitivity() == SensitivityClass::Secret
                }) {
                    return Err(E::InvalidQuality);
                }
                quality
                    .entry(subject.clone())
                    .or_default()
                    .extend(metadata.iter().cloned());
            }
            let records: serde_json::Value =
                serde_json::from_str(&batch.records().to_json().map_err(|_| E::InvalidRecords)?)
                    .map_err(|_| E::InvalidRecords)?;
            manifest.push(json!({"binding":member.binding,"version_domain":member.version_domain,
                "source":source.source_id(),"source_kind":source.source_kind().as_str(),
                "change_token":source.change_token(),"source_digest":source.digest(),"records":records,
                "quality":batch.quality_metadata().iter().map(|(subject,metadata)| json!({
                    "subject":subject.segments().iter().map(NonEmptyText::as_str).collect::<Vec<_>>(),
                    "metadata":metadata})).collect::<Vec<_>>() }));
        }
        let records = ObservationEvidenceSet::new(
            provenances.into_values().collect(),
            observations.into_values().collect(),
            facts.into_values().collect(),
            evidence.into_values().collect(),
        )
        .map_err(|_| E::InvalidRecords)?;
        let digest = context_content_digest(&serde_json::to_vec(&json!({"contract":"cg.retained-observation-readset","version":1,"scope":scope,"members":manifest})).expect("native readset serializes"));
        let snapshot = SourceSnapshot::new(
            SourceId::new(format!("retained-readset-{}", digest.as_str()))
                .expect("digest forms valid source ID"),
            SourceKind::Runtime,
            None,
            Some(digest.clone()),
        )
        .expect("computed digest is a source marker");
        let mut batch = ScopedObservationBatch::new(scope.clone(), snapshot, records)
            .map_err(|_| E::InvalidRecords)?;
        for (subject, metadata) in quality {
            batch = batch.with_quality_metadata(subject, metadata);
        }
        Ok(Self {
            scope,
            digest,
            batch,
        })
    }
    pub fn scope(&self) -> &ContextScopeId {
        &self.scope
    }
    pub fn digest(&self) -> &ContentDigest {
        &self.digest
    }
    pub fn batch(&self) -> &ScopedObservationBatch {
        &self.batch
    }
}
fn insert<K: Ord, T: Clone + PartialEq>(
    items: &mut BTreeMap<K, T>,
    id: K,
    value: &T,
) -> Result<(), RetainedObservationError> {
    if items.get(&id).is_some_and(|previous| previous != value) {
        return Err(RetainedObservationError::DuplicateIdentity);
    }
    items.insert(id, value.clone());
    Ok(())
}
