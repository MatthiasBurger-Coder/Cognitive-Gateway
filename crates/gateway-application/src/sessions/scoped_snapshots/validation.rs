//! Local consistency and exact coverage checks over supplied observation DATA.
use super::{types::SnapshotData, *};
use crate::{connector_results::ConnectorResult, connectors::*};
use gateway_domain::*;
use std::collections::{BTreeMap, BTreeSet};

type Domain = (SourceKind, SourceId, ReferenceId);
fn domain(object: &SnapshotObject) -> Domain {
    (
        object.source_kind,
        object.source.clone(),
        object.object.id.clone(),
    )
}
fn strong(q: QualityMetadata) -> bool {
    q.trust() == TrustClass::ObservedEvidence
        && q.freshness() == FreshnessStatus::Fresh
        && q.uncertainty() == Uncertainty::None
        && q.conflict() == ConflictStatus::None
        && q.confidence() == Confidence::NotApplicable
        && q.sensitivity() != SensitivityClass::Secret
}
fn value<'a>(
    a: &'a SnapshotAcquisition,
    id: &ObservationId,
    subject: &SubjectPath,
) -> Result<&'a str, SnapshotError> {
    let observation = a
        .batch
        .records()
        .observations()
        .iter()
        .find(|o| o.id() == id)
        .ok_or(SnapshotError::InvalidEvidence)?;
    if observation.subject() != subject {
        return Err(SnapshotError::InvalidEvidence);
    }
    match observation.value() {
        TypedValue::String(value) if !value.as_str().is_empty() => Ok(value.as_str()),
        _ => Err(SnapshotError::InvalidEvidence),
    }
}
fn route(
    a: &SnapshotAcquisition,
    route: &SnapshotReadRoute,
    epoch: &SourceTimestamp,
) -> Result<(), SnapshotError> {
    let mut arguments = route.arguments.clone();
    if let Some(key) = &route.epoch_argument {
        if arguments.contains_key(key) {
            return Err(SnapshotError::ProfileMismatch);
        }
        arguments.insert(
            key.clone(),
            TypedValue::string(epoch.as_str()).map_err(|_| SnapshotError::InvalidEncoding)?,
        );
    }
    if a.request.target != route.target
        || a.request.capability != route.capability
        || a.request.class != route.class
        || a.request.arguments != arguments
        || a.observation.metadata.object_reference != route.object_reference
    {
        return Err(SnapshotError::ProfileMismatch);
    }
    Ok(())
}
struct CaptureInterval {
    observed: u64,
    acquired: u64,
}
fn time(a: &SnapshotAcquisition, now: u64, max_age: u64) -> Result<CaptureInterval, SnapshotError> {
    let observed = a
        .observation
        .metadata
        .observed_at
        .ok_or(SnapshotError::InvalidEvidence)?
        .seconds();
    let observed = u64::try_from(observed)
        .map_err(|_| SnapshotError::Expired)?
        .checked_mul(1000)
        .ok_or(SnapshotError::LimitExceeded)?;
    let acquired = u64::try_from(a.observation.acquired_at.seconds())
        .map_err(|_| SnapshotError::Expired)?
        .checked_mul(1000)
        .ok_or(SnapshotError::LimitExceeded)?;
    let end = observed
        .checked_add(
            max_age
                .checked_mul(1000)
                .ok_or(SnapshotError::LimitExceeded)?,
        )
        .ok_or(SnapshotError::LimitExceeded)?;
    if observed > acquired || acquired > now || now >= end {
        return Err(SnapshotError::Expired);
    }
    Ok(CaptureInterval { observed, acquired })
}
fn anchor(
    data: &SnapshotData,
    anchor: &SnapshotAnchor,
    refs: &BTreeMap<ContentDigest, usize>,
) -> Result<usize, SnapshotError> {
    if anchor.domain != data.profile.authority_domain
        || anchor.membership != data.universe.membership_digest()?
    {
        return Err(SnapshotError::Inconsistent);
    }
    let index = *refs
        .get(&anchor.acquisition)
        .ok_or(SnapshotError::InvalidEvidence)?;
    let a = &data.acquisitions[index];
    if a.observation.source != data.profile.anchor_source
        || a.observation.metadata.source_kind != data.profile.anchor_source_kind
    {
        return Err(SnapshotError::ProfileMismatch);
    }
    route(a, &data.profile.anchor_route, &anchor.epoch)?;
    if value(a, &anchor.epoch_observation, &data.profile.epoch_subject)? != anchor.epoch.as_str()
        || value(
            a,
            &anchor.membership_observation,
            &data.profile.membership_subject,
        )? != anchor.membership.as_str()
    {
        return Err(SnapshotError::Inconsistent);
    }
    Ok(index)
}
fn bounds(data: &SnapshotData) -> Result<(), SnapshotError> {
    if data.universe.objects.is_empty()
        || data.universe.objects.len() > MAX_SNAPSHOT_OBJECTS
        || data.acquisitions.is_empty()
        || data.acquisitions.len() > MAX_SNAPSHOT_ACQUISITIONS
        || data.object_reads.len() > MAX_SNAPSHOT_OBJECTS
        || data
            .universe
            .objects
            .iter()
            .map(|o| o.subjects.len())
            .sum::<usize>()
            > MAX_SNAPSHOT_SUBJECTS
    {
        return Err(SnapshotError::LimitExceeded);
    }
    if data.profile.max_age_seconds == 0
        || data.profile.max_age_seconds > 3600
        || data.profile.epoch_subject == data.profile.membership_subject
        || matches!(
            data.profile.anchor_source_kind,
            SourceKind::Model | SourceKind::Caller | SourceKind::Synthetic
        )
    {
        return Err(SnapshotError::ProfileMismatch);
    }
    let mut size = data
        .universe
        .body_bytes()?
        .len()
        .checked_add(data.profile.body_bytes()?.len())
        .ok_or(SnapshotError::LimitExceeded)?;
    for acquisition in &data.acquisitions {
        size = size
            .checked_add(super::codec::canonical_acquisition(acquisition)?.len())
            .ok_or(SnapshotError::LimitExceeded)?;
        if size > MAX_SNAPSHOT_BYTES {
            return Err(SnapshotError::LimitExceeded);
        }
    }
    Ok(())
}
fn direct_values(records: &ObservationEvidenceSet) -> Result<(), SnapshotError> {
    let mut subjects = BTreeMap::new();
    for observation in records.observations() {
        if subjects
            .insert(observation.subject().clone(), observation.value().clone())
            .is_some_and(|old| old != *observation.value())
        {
            return Err(SnapshotError::Conflict);
        }
    }
    let mut supported = BTreeSet::new();
    for evidence in records.evidence() {
        if matches!(
            evidence.kind(),
            EvidenceKind::ModelOutput
                | EvidenceKind::CallerStatement
                | EvidenceKind::SyntheticFixture
        ) {
            return Err(SnapshotError::InvalidEvidence);
        }
        for link in evidence.links() {
            if link.relation() != EvidenceRelation::Supports {
                return Err(SnapshotError::Conflict);
            }
            supported.insert(link.fact().clone());
        }
    }
    for fact in records.facts() {
        if fact.polarity() != AssertionPolarity::Affirmed
            || !supported.contains(fact.id())
            || fact.observations().iter().any(|id| {
                records
                    .observations()
                    .iter()
                    .find(|o| o.id() == id)
                    .is_none_or(|o| o.subject() != fact.subject() || o.value() != fact.value())
            })
        {
            return Err(SnapshotError::Conflict);
        }
    }
    Ok(())
}
fn evidence(
    data: &SnapshotData,
    now: u64,
) -> Result<(BTreeMap<ContentDigest, usize>, Vec<CaptureInterval>), SnapshotError> {
    let mut refs = BTreeMap::new();
    let mut times = Vec::new();
    let b = &data.binding;
    let mut identities = BTreeMap::new();
    for (index, a) in data.acquisitions.iter().enumerate() {
        let o = &a.observation;
        let m = &o.metadata;
        if a.request.target.scope != b.capture.scope
            || a.request.task != b.task
            || a.request.step != b.capture.step
            || a.request.execution != b.execution
        {
            return Err(SnapshotError::BindingMismatch);
        }
        if o.basis != ConnectorObservationBasis::SourceRead
            || m.effect != ConnectorEffect::NoMutation
            || m.partial
            || m.verifies.is_some()
            || m.non_authoritative_raw.is_some()
            || matches!(
                m.source_kind,
                SourceKind::Model | SourceKind::Caller | SourceKind::Synthetic
            )
            || !strong(m.quality)
            || ConnectorResult::validate_for_request(&a.request, o).is_err()
            || a.batch.scope() != &b.capture.scope
            || a.batch.records() != &o.records
            || a.batch.snapshot().source_id() != &o.source
            || a.batch.snapshot().source_kind() != m.source_kind
            || a.batch.snapshot().change_token() != o.source_revision.as_ref()
            || a.batch.snapshot().digest() != Some(&o.content_digest)
        {
            return Err(SnapshotError::InvalidEvidence);
        }
        let subjects: BTreeSet<_> = o
            .records
            .observations()
            .iter()
            .map(|o| o.subject().clone())
            .collect();
        if a.batch
            .quality_metadata()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            != subjects
            || a.batch
                .quality_metadata()
                .values()
                .any(|q| q.is_empty() || q.iter().any(|q| !strong(*q)))
        {
            return Err(SnapshotError::InvalidEvidence);
        }
        direct_values(&o.records)?;
        times.push(time(a, now, data.profile.max_age_seconds)?);
        if refs.insert(a.reference()?, index).is_some() {
            return Err(SnapshotError::InvalidEvidence);
        }
        // Native IDs cannot mean different records in different acquisitions.
        let records: serde_json::Value = serde_json::from_str(
            &o.records
                .to_json()
                .map_err(|_| SnapshotError::InvalidEvidence)?,
        )
        .map_err(|_| SnapshotError::InvalidEvidence)?;
        for (kind, items) in records.as_object().ok_or(SnapshotError::InvalidEvidence)? {
            if let Some(items) = items.as_array() {
                for item in items {
                    let key = (
                        kind.clone(),
                        item.get("id")
                            .cloned()
                            .ok_or(SnapshotError::InvalidEvidence)?
                            .to_string(),
                    );
                    if identities
                        .insert(key, item.clone())
                        .is_some_and(|old| old != *item)
                    {
                        return Err(SnapshotError::Conflict);
                    }
                }
            }
        }
    }
    Ok((refs, times))
}

pub(super) fn validate(
    data: &SnapshotData,
    now_ms: u64,
) -> Result<Vec<SnapshotObjectVersion>, SnapshotError> {
    bounds(data)?;
    super::identity::validate_identity(data)?;
    let mut universe = BTreeMap::new();
    let mut definitions = BTreeMap::new();
    let mut ownership = BTreeMap::new();
    for object in &data.universe.objects {
        if object.subjects.is_empty()
            || !object.subjects.contains(&object.version_subject)
            || object.subjects.contains(&data.profile.epoch_subject)
            || object.subjects.contains(&data.profile.membership_subject)
            || matches!(
                object.source_kind,
                SourceKind::Model | SourceKind::Caller | SourceKind::Synthetic
            )
            || universe.insert(domain(object), object).is_some()
            || definitions
                .insert(object.object.id.clone(), object.object.clone())
                .is_some_and(|old| old != object.object)
        {
            return Err(SnapshotError::UniverseMismatch);
        }
        for subject in &object.subjects {
            if ownership
                .insert(subject.clone(), object.object.clone())
                .is_some_and(|prior| prior != object.object)
            {
                return Err(SnapshotError::UniverseMismatch);
            }
        }
    }
    if universe.len() != data.object_reads.len() {
        return Err(SnapshotError::Incomplete);
    }
    let (refs, times) = evidence(data, now_ms)?;
    let (global, before, after) = match (&data.profile.kind, &data.consistency) {
        (SnapshotProfileKind::AtomicCompleteExport, SnapshotConsistency::Atomic { anchor: a }) => {
            let i = anchor(data, a, &refs)?;
            if data.acquisitions.len() != 1 {
                return Err(SnapshotError::Inconsistent);
            }
            (a, i, i)
        }
        (
            SnapshotProfileKind::SnapshotScopedReads,
            SnapshotConsistency::SnapshotScoped { anchor: a },
        ) => {
            let i = anchor(data, a, &refs)?;
            (a, i, i)
        }
        (
            SnapshotProfileKind::BracketedNonReusedAnchor,
            SnapshotConsistency::Bracketed {
                before: a,
                after: b,
            },
        ) => {
            let i = anchor(data, a, &refs)?;
            let j = anchor(data, b, &refs)?;
            if a.domain != b.domain
                || a.epoch != b.epoch
                || a.membership != b.membership
                || i >= j
                || times[i].acquired > times[j].observed
            {
                return Err(SnapshotError::Inconsistent);
            }
            (a, i, j)
        }
        _ => return Err(SnapshotError::ProfileMismatch),
    };
    let mut seen = BTreeSet::new();
    let mut expected: BTreeMap<usize, BTreeSet<SubjectPath>> = BTreeMap::new();
    for index in [before, after] {
        expected.entry(index).or_default().extend([
            data.profile.epoch_subject.clone(),
            data.profile.membership_subject.clone(),
        ]);
    }
    let mut versions = Vec::new();
    for read in &data.object_reads {
        let key = (
            read.source_kind,
            read.source.clone(),
            read.object.id.clone(),
        );
        let object = universe.get(&key).ok_or(SnapshotError::Incomplete)?;
        if object.object != read.object || !seen.insert(key) {
            return Err(SnapshotError::Incomplete);
        }
        let index = *refs
            .get(&read.acquisition)
            .ok_or(SnapshotError::InvalidEvidence)?;
        let a = &data.acquisitions[index];
        if a.observation.source != object.source
            || a.observation.metadata.source_kind != object.source_kind
        {
            return Err(SnapshotError::InvalidEvidence);
        }
        route(a, &object.route, &global.epoch)?;
        match data.profile.kind {
            SnapshotProfileKind::AtomicCompleteExport => {
                if index != before || object.route != data.profile.anchor_route {
                    return Err(SnapshotError::Inconsistent);
                }
            }
            SnapshotProfileKind::SnapshotScopedReads => {
                if object.route.epoch_argument.is_none()
                    || index < before
                    || times[index].acquired < times[before].acquired
                {
                    return Err(SnapshotError::Inconsistent);
                }
            }
            SnapshotProfileKind::BracketedNonReusedAnchor => {
                if index <= before
                    || index >= after
                    || times[index].observed < times[before].acquired
                    || times[index].acquired > times[after].observed
                {
                    return Err(SnapshotError::Inconsistent);
                }
            }
        }
        let actual = value(a, &read.version_observation, &object.version_subject)?;
        let version = SourceTimestamp::new(actual).map_err(|_| SnapshotError::InvalidEvidence)?;
        versions.push(SnapshotObjectVersion {
            object: read.object.clone(),
            source: read.source.clone(),
            source_kind: read.source_kind,
            version,
        });
        expected
            .entry(index)
            .or_default()
            .extend(object.subjects.iter().cloned());
    }
    if expected.len() != data.acquisitions.len() {
        return Err(SnapshotError::Incomplete);
    }
    let metadata: BTreeSet<_> = data
        .universe
        .objects
        .iter()
        .map(|o| o.version_subject.clone())
        .chain([
            data.profile.epoch_subject.clone(),
            data.profile.membership_subject.clone(),
        ])
        .collect();
    let mut world = BTreeMap::new();
    for (index, subjects) in expected {
        let a = &data.acquisitions[index];
        let observations: BTreeSet<_> = a
            .batch
            .records()
            .observations()
            .iter()
            .map(|o| o.subject().clone())
            .collect();
        let facts: BTreeSet<_> = a
            .batch
            .records()
            .facts()
            .iter()
            .map(|o| o.subject().clone())
            .collect();
        if observations != subjects || facts != subjects {
            return Err(SnapshotError::Incomplete);
        }
        for fact in a.batch.records().facts() {
            if !metadata.contains(fact.subject())
                && world
                    .insert(fact.subject().clone(), fact.value().clone())
                    .is_some_and(|old| old != *fact.value())
            {
                return Err(SnapshotError::Conflict);
            }
        }
    }
    Ok(versions)
}
