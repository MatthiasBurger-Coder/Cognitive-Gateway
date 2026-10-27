//! Closed, bounded persistence of snapshot DATA. Decoding rechecks completeness;
//! neither an encoded digest nor this codec confers live acquisition authority.
use super::*;
use crate::{
    ScopedObservationBatch, SourceSnapshot, connector_results::ConnectorResultMetadata,
    connectors::*, sessions::ResourceReservationBinding,
};
use gateway_domain::*;
use serde::{
    Deserialize, Serialize,
    de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

const CONTRACT: &str = "cg.complete-scoped-snapshot";
const ACQUISITION_CONTRACT: &str = "cg.scoped-snapshot-acquisition";
type Result<T> = std::result::Result<T, SnapshotError>;

impl CompleteScopedSnapshot {
    /// Canonical immutable data only; the future live consumer owns authority.
    pub fn encode(&self) -> Result<Vec<u8>> {
        if context_content_digest(&canonical_data(self)?) != self.digest {
            return Err(SnapshotError::DigestMismatch);
        }
        bounded(canonical(&Envelope {
            contract: CONTRACT.into(),
            contract_version: SchemaVersion::V1,
            digest: self.digest.clone(),
            data: DataWire::from_data(&self.data)?,
        })?)
    }
    /// Reload against independently selected current bindings and definitions.
    /// The supplied bytes cannot choose their owner, universe or provider profile.
    pub fn decode(
        bytes: &[u8],
        expected_binding: &SnapshotBinding,
        expected_universe: &SnapshotUniverse,
        expected_profile: &SnapshotProfile,
        now_ms: u64,
    ) -> Result<Self> {
        let (data, digest) = decode_data(
            bytes,
            expected_binding,
            &expected_universe.pin,
            &expected_profile.pin,
        )?;
        if &data.universe != expected_universe {
            return Err(SnapshotError::UniverseMismatch);
        }
        if &data.profile != expected_profile {
            return Err(SnapshotError::ProfileMismatch);
        }
        finish_decode(data, digest, bytes, now_ms)
    }
    /// Reload DATA selected by independently authenticated full definition pins.
    /// The owning journal must authenticate these pins and the record bytes;
    /// this function neither authenticates a caller nor grants current authority.
    pub fn decode_bound(
        bytes: &[u8],
        expected_binding: &SnapshotBinding,
        expected_universe: &SnapshotPin,
        expected_profile: &SnapshotPin,
        now_ms: u64,
    ) -> Result<Self> {
        let (data, digest) =
            decode_data(bytes, expected_binding, expected_universe, expected_profile)?;
        finish_decode(data, digest, bytes, now_ms)
    }
}

fn decode_data(
    bytes: &[u8],
    binding: &SnapshotBinding,
    universe: &SnapshotPin,
    profile: &SnapshotPin,
) -> Result<(SnapshotData, ContentDigest)> {
    let value = strict_value(bytes)?;
    let wire: Envelope =
        serde_json::from_value(value).map_err(|_| SnapshotError::InvalidEncoding)?;
    if wire.contract != CONTRACT || wire.contract_version != SchemaVersion::V1 {
        return Err(SnapshotError::InvalidEncoding);
    }
    let data = wire.data.into_data()?;
    if &data.binding != binding {
        return Err(SnapshotError::BindingMismatch);
    }
    if &data.universe.pin != universe {
        return Err(SnapshotError::UniverseMismatch);
    }
    if &data.profile.pin != profile {
        return Err(SnapshotError::ProfileMismatch);
    }
    Ok((data, wire.digest))
}
fn finish_decode(
    data: SnapshotData,
    digest: ContentDigest,
    bytes: &[u8],
    now_ms: u64,
) -> Result<CompleteScopedSnapshot> {
    let complete = CompleteScopedSnapshot::validate(
        data.binding,
        data.universe,
        data.profile,
        data.acquisitions,
        data.object_reads,
        data.consistency,
        now_ms,
    )?;
    if complete.digest != digest {
        return Err(SnapshotError::DigestMismatch);
    }
    // A single persisted representation also covers native CG06 JSON strings.
    if complete.encode()?.as_slice() != bytes {
        return Err(SnapshotError::InvalidEncoding);
    }
    Ok(complete)
}

// SnapshotBinding is evidence DATA. Only this owning wire conversion defines
// its fence/step representation; no live allocation or grant gains serde.
impl Serialize for SnapshotBinding {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        BindingWire::from_binding(self).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for SnapshotBinding {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> std::result::Result<Self, D::Error> {
        BindingWire::deserialize(decoder)?
            .into_binding()
            .map_err(|_| D::Error::custom("invalid snapshot binding"))
    }
}

pub(super) fn canonical_data(snapshot: &CompleteScopedSnapshot) -> Result<Vec<u8>> {
    canonical_input(&snapshot.data)
}
pub(super) fn canonical_input(data: &SnapshotData) -> Result<Vec<u8>> {
    bounded(canonical(&Input {
        contract: CONTRACT,
        contract_version: SchemaVersion::V1,
        data: DataWire::from_data(data)?,
    })?)
}
pub(super) fn canonical_acquisition(value: &SnapshotAcquisition) -> Result<Vec<u8>> {
    bounded(canonical(&AcquisitionInput {
        contract: ACQUISITION_CONTRACT,
        contract_version: SchemaVersion::V1,
        acquisition: AcquisitionWire::from_acquisition(value)?,
    })?)
}
fn bounded(bytes: Vec<u8>) -> Result<Vec<u8>> {
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        Err(SnapshotError::LimitExceeded)
    } else {
        Ok(bytes)
    }
}
pub(in crate::sessions) fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let value = serde_json::to_value(value).map_err(|_| SnapshotError::InvalidEncoding)?;
    serde_json::to_vec(&ordered(value)).map_err(|_| SnapshotError::InvalidEncoding)
}
fn ordered(value: Value) -> Value {
    match value {
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(k, v)| (k, ordered(v)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(ordered).collect()),
        other => other,
    }
}

#[derive(Serialize)]
struct Input<'a> {
    contract: &'a str,
    contract_version: SchemaVersion,
    data: DataWire,
}
#[derive(Serialize)]
struct AcquisitionInput<'a> {
    contract: &'a str,
    contract_version: SchemaVersion,
    acquisition: AcquisitionWire,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    contract: String,
    contract_version: SchemaVersion,
    digest: ContentDigest,
    data: DataWire,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DataWire {
    binding: BindingWire,
    universe: UniverseWire,
    profile: ProfileWire,
    acquisitions: Vec<AcquisitionWire>,
    object_reads: Vec<ObjectReadWire>,
    consistency: ConsistencyWire,
}
impl DataWire {
    fn from_data(data: &SnapshotData) -> Result<Self> {
        Ok(Self {
            binding: BindingWire::from_binding(&data.binding),
            universe: UniverseWire::from_universe(&data.universe),
            profile: ProfileWire::from_profile(&data.profile),
            acquisitions: data
                .acquisitions
                .iter()
                .map(AcquisitionWire::from_acquisition)
                .collect::<Result<_>>()?,
            object_reads: data
                .object_reads
                .iter()
                .map(ObjectReadWire::from_read)
                .collect(),
            consistency: ConsistencyWire::from_consistency(&data.consistency),
        })
    }
    fn into_data(self) -> Result<SnapshotData> {
        Ok(SnapshotData {
            binding: self.binding.into_binding()?,
            universe: self.universe.into_universe(),
            profile: self.profile.into_profile(),
            acquisitions: self
                .acquisitions
                .into_iter()
                .map(AcquisitionWire::into_acquisition)
                .collect::<Result<_>>()?,
            object_reads: self
                .object_reads
                .into_iter()
                .map(ObjectReadWire::into_read)
                .collect(),
            consistency: self.consistency.into_consistency(),
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum PhaseWire {
    Before,
    After,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReservationWire {
    reservation: ContentDigest,
    fence: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingWire {
    capture: ResourceReservationBinding,
    phase: PhaseWire,
    task: TaskId,
    execution: NonEmptyText,
    reservation: Option<ReservationWire>,
}
impl BindingWire {
    fn from_binding(v: &SnapshotBinding) -> Self {
        Self {
            capture: v.capture.clone(),
            phase: match v.phase {
                SnapshotPhase::Before => PhaseWire::Before,
                SnapshotPhase::After => PhaseWire::After,
            },
            task: v.task.clone(),
            execution: v.execution.clone(),
            reservation: v.reservation.as_ref().map(|r| ReservationWire {
                reservation: r.reservation.clone(),
                fence: r.fence.value(),
            }),
        }
    }
    fn into_binding(self) -> Result<SnapshotBinding> {
        Ok(SnapshotBinding {
            capture: self.capture,
            phase: match self.phase {
                PhaseWire::Before => SnapshotPhase::Before,
                PhaseWire::After => SnapshotPhase::After,
            },
            task: self.task,
            execution: self.execution,
            reservation: self
                .reservation
                .map(|r| {
                    Ok(SnapshotReservation {
                        reservation: r.reservation,
                        fence: crate::sessions::FenceToken::new(r.fence)
                            .map_err(|_| SnapshotError::InvalidEncoding)?,
                    })
                })
                .transpose()?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ClassWire {
    Read,
    Search,
    Inspect,
    Mutate,
    Admin,
}
impl ClassWire {
    fn from_class(v: ConnectorOperationClass) -> Self {
        match v {
            ConnectorOperationClass::Read => Self::Read,
            ConnectorOperationClass::Search => Self::Search,
            ConnectorOperationClass::Inspect => Self::Inspect,
            ConnectorOperationClass::Mutate => Self::Mutate,
            ConnectorOperationClass::Admin => Self::Admin,
        }
    }
    fn into_class(self) -> ConnectorOperationClass {
        match self {
            Self::Read => ConnectorOperationClass::Read,
            Self::Search => ConnectorOperationClass::Search,
            Self::Inspect => ConnectorOperationClass::Inspect,
            Self::Mutate => ConnectorOperationClass::Mutate,
            Self::Admin => ConnectorOperationClass::Admin,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetWire {
    connector: NonEmptyText,
    scope: ContextScopeId,
}
impl TargetWire {
    fn from_target(v: &ConnectorTarget) -> Self {
        Self {
            connector: v.connector.clone(),
            scope: v.scope.clone(),
        }
    }
    fn into_target(self) -> ConnectorTarget {
        ConnectorTarget {
            connector: self.connector,
            scope: self.scope,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteWire {
    pin: SnapshotPin,
    authority_domain: SnapshotPin,
    target: TargetWire,
    capability: CapabilityId,
    class: ClassWire,
    arguments: BTreeMap<NonEmptyText, TypedValue>,
    object_reference: NonEmptyText,
    epoch_argument: Option<NonEmptyText>,
}
impl RouteWire {
    fn from_route(v: &SnapshotReadRoute) -> Self {
        Self {
            pin: v.pin.clone(),
            authority_domain: v.authority_domain.clone(),
            target: TargetWire::from_target(&v.target),
            capability: v.capability.clone(),
            class: ClassWire::from_class(v.class),
            arguments: v.arguments.clone(),
            object_reference: v.object_reference.clone(),
            epoch_argument: v.epoch_argument.clone(),
        }
    }
    fn into_route(self) -> SnapshotReadRoute {
        SnapshotReadRoute {
            pin: self.pin,
            authority_domain: self.authority_domain,
            target: self.target.into_target(),
            capability: self.capability,
            class: self.class.into_class(),
            arguments: self.arguments,
            object_reference: self.object_reference,
            epoch_argument: self.epoch_argument,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectWire {
    object: SnapshotPin,
    source: SourceId,
    source_kind: SourceKind,
    object_reference: NonEmptyText,
    subjects: BTreeSet<SubjectPath>,
    version_subject: SubjectPath,
    route: RouteWire,
}
impl ObjectWire {
    fn from_object(v: &SnapshotObject) -> Self {
        Self {
            object: v.object.clone(),
            source: v.source.clone(),
            source_kind: v.source_kind,
            object_reference: v.object_reference.clone(),
            subjects: v.subjects.clone(),
            version_subject: v.version_subject.clone(),
            route: RouteWire::from_route(&v.route),
        }
    }
    fn into_object(self) -> SnapshotObject {
        SnapshotObject {
            object: self.object,
            source: self.source,
            source_kind: self.source_kind,
            object_reference: self.object_reference,
            subjects: self.subjects,
            version_subject: self.version_subject,
            route: self.route.into_route(),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UniverseWire {
    pin: SnapshotPin,
    objects: Vec<ObjectWire>,
}
impl UniverseWire {
    fn from_universe(v: &SnapshotUniverse) -> Self {
        Self {
            pin: v.pin.clone(),
            objects: v.objects.iter().map(ObjectWire::from_object).collect(),
        }
    }
    fn into_universe(self) -> SnapshotUniverse {
        SnapshotUniverse {
            pin: self.pin,
            objects: self
                .objects
                .into_iter()
                .map(ObjectWire::into_object)
                .collect(),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ProfileKindWire {
    AtomicCompleteExport,
    SnapshotScopedReads,
    BracketedNonReusedAnchor,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileWire {
    pin: SnapshotPin,
    authority_domain: SnapshotPin,
    kind: ProfileKindWire,
    anchor_source: SourceId,
    anchor_source_kind: SourceKind,
    anchor_route: RouteWire,
    epoch_subject: SubjectPath,
    membership_subject: SubjectPath,
    max_age_seconds: u64,
}
impl ProfileWire {
    fn from_profile(v: &SnapshotProfile) -> Self {
        Self {
            pin: v.pin.clone(),
            authority_domain: v.authority_domain.clone(),
            kind: match v.kind {
                SnapshotProfileKind::AtomicCompleteExport => ProfileKindWire::AtomicCompleteExport,
                SnapshotProfileKind::SnapshotScopedReads => ProfileKindWire::SnapshotScopedReads,
                SnapshotProfileKind::BracketedNonReusedAnchor => {
                    ProfileKindWire::BracketedNonReusedAnchor
                }
            },
            anchor_source: v.anchor_source.clone(),
            anchor_source_kind: v.anchor_source_kind,
            anchor_route: RouteWire::from_route(&v.anchor_route),
            epoch_subject: v.epoch_subject.clone(),
            membership_subject: v.membership_subject.clone(),
            max_age_seconds: v.max_age_seconds,
        }
    }
    fn into_profile(self) -> SnapshotProfile {
        SnapshotProfile {
            pin: self.pin,
            authority_domain: self.authority_domain,
            kind: match self.kind {
                ProfileKindWire::AtomicCompleteExport => SnapshotProfileKind::AtomicCompleteExport,
                ProfileKindWire::SnapshotScopedReads => SnapshotProfileKind::SnapshotScopedReads,
                ProfileKindWire::BracketedNonReusedAnchor => {
                    SnapshotProfileKind::BracketedNonReusedAnchor
                }
            },
            anchor_source: self.anchor_source,
            anchor_source_kind: self.anchor_source_kind,
            anchor_route: self.anchor_route.into_route(),
            epoch_subject: self.epoch_subject,
            membership_subject: self.membership_subject,
            max_age_seconds: self.max_age_seconds,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectReadWire {
    source: SourceId,
    source_kind: SourceKind,
    object: SnapshotPin,
    acquisition: ContentDigest,
    version_observation: ObservationId,
}
impl ObjectReadWire {
    fn from_read(v: &SnapshotObjectRead) -> Self {
        Self {
            source: v.source.clone(),
            source_kind: v.source_kind,
            object: v.object.clone(),
            acquisition: v.acquisition.clone(),
            version_observation: v.version_observation.clone(),
        }
    }
    fn into_read(self) -> SnapshotObjectRead {
        SnapshotObjectRead {
            source: self.source,
            source_kind: self.source_kind,
            object: self.object,
            acquisition: self.acquisition,
            version_observation: self.version_observation,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnchorWire {
    domain: SnapshotPin,
    epoch: SourceTimestamp,
    membership: ContentDigest,
    acquisition: ContentDigest,
    epoch_observation: ObservationId,
    membership_observation: ObservationId,
}
impl AnchorWire {
    fn from_anchor(v: &SnapshotAnchor) -> Self {
        Self {
            domain: v.domain.clone(),
            epoch: v.epoch.clone(),
            membership: v.membership.clone(),
            acquisition: v.acquisition.clone(),
            epoch_observation: v.epoch_observation.clone(),
            membership_observation: v.membership_observation.clone(),
        }
    }
    fn into_anchor(self) -> SnapshotAnchor {
        SnapshotAnchor {
            domain: self.domain,
            epoch: self.epoch,
            membership: self.membership,
            acquisition: self.acquisition,
            epoch_observation: self.epoch_observation,
            membership_observation: self.membership_observation,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
enum ConsistencyWire {
    Atomic {
        anchor: AnchorWire,
    },
    SnapshotScoped {
        anchor: AnchorWire,
    },
    Bracketed {
        before: AnchorWire,
        after: Box<AnchorWire>,
    },
}
impl ConsistencyWire {
    fn from_consistency(v: &SnapshotConsistency) -> Self {
        match v {
            SnapshotConsistency::Atomic { anchor } => Self::Atomic {
                anchor: AnchorWire::from_anchor(anchor),
            },
            SnapshotConsistency::SnapshotScoped { anchor } => Self::SnapshotScoped {
                anchor: AnchorWire::from_anchor(anchor),
            },
            SnapshotConsistency::Bracketed { before, after } => Self::Bracketed {
                before: AnchorWire::from_anchor(before),
                after: Box::new(AnchorWire::from_anchor(after)),
            },
        }
    }
    fn into_consistency(self) -> SnapshotConsistency {
        match self {
            Self::Atomic { anchor } => SnapshotConsistency::Atomic {
                anchor: anchor.into_anchor(),
            },
            Self::SnapshotScoped { anchor } => SnapshotConsistency::SnapshotScoped {
                anchor: anchor.into_anchor(),
            },
            Self::Bracketed { before, after } => SnapshotConsistency::Bracketed {
                before: before.into_anchor(),
                after: Box::new((*after).into_anchor()),
            },
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestWire {
    target: TargetWire,
    capability: CapabilityId,
    class: ClassWire,
    inventory_revision: ContentDigest,
    arguments: BTreeMap<NonEmptyText, TypedValue>,
    task: TaskId,
    step: String,
    execution: NonEmptyText,
    correlation: NonEmptyText,
}
impl RequestWire {
    fn from_request(v: &ConnectorRequest) -> Self {
        Self {
            target: TargetWire::from_target(&v.target),
            capability: v.capability.clone(),
            class: ClassWire::from_class(v.class),
            inventory_revision: v.inventory_revision.clone(),
            arguments: v.arguments.clone(),
            task: v.task.clone(),
            step: v.step.as_str().into(),
            execution: v.execution.clone(),
            correlation: v.correlation.clone(),
        }
    }
    fn into_request(self) -> Result<ConnectorRequest> {
        Ok(ConnectorRequest {
            target: self.target.into_target(),
            capability: self.capability,
            class: self.class.into_class(),
            inventory_revision: self.inventory_revision,
            arguments: self.arguments,
            task: self.task,
            step: PlanStepId::new(self.step).map_err(|_| SnapshotError::InvalidEncoding)?,
            execution: self.execution,
            correlation: self.correlation,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum EffectWire {
    NotDispatched,
    NoMutation,
    Acknowledged,
    Unknown,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum BasisWire {
    ProviderAcknowledgement,
    SourceRead,
    IndependentPostEffectRead,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetadataWire {
    task: TaskId,
    step: String,
    execution: NonEmptyText,
    object_reference: NonEmptyText,
    source_kind: SourceKind,
    source_digest: Option<ContentDigest>,
    observed_at: Option<i64>,
    authenticated_source: NonEmptyText,
    quality: QualityMetadata,
    effect: EffectWire,
    partial: bool,
    verifies: Option<RequestWire>,
    non_authoritative_raw: Option<ReferenceId>,
}
impl MetadataWire {
    fn from_metadata(v: &ConnectorResultMetadata) -> Self {
        Self {
            task: v.task.clone(),
            step: v.step.as_str().into(),
            execution: v.execution.clone(),
            object_reference: v.object_reference.clone(),
            source_kind: v.source_kind,
            source_digest: v.source_digest.clone(),
            observed_at: v.observed_at.map(UnixTimestamp::seconds),
            authenticated_source: v.authenticated_source.clone(),
            quality: v.quality,
            effect: match v.effect {
                ConnectorEffect::NotDispatched => EffectWire::NotDispatched,
                ConnectorEffect::NoMutation => EffectWire::NoMutation,
                ConnectorEffect::Acknowledged => EffectWire::Acknowledged,
                ConnectorEffect::Unknown => EffectWire::Unknown,
            },
            partial: v.partial,
            verifies: v.verifies.as_ref().map(RequestWire::from_request),
            non_authoritative_raw: v.non_authoritative_raw.clone(),
        }
    }
    fn into_metadata(self) -> Result<ConnectorResultMetadata> {
        Ok(ConnectorResultMetadata {
            task: self.task,
            step: PlanStepId::new(self.step).map_err(|_| SnapshotError::InvalidEncoding)?,
            execution: self.execution,
            object_reference: self.object_reference,
            source_kind: self.source_kind,
            source_digest: self.source_digest,
            observed_at: self.observed_at.map(UnixTimestamp::new),
            authenticated_source: self.authenticated_source,
            quality: self.quality,
            effect: match self.effect {
                EffectWire::NotDispatched => ConnectorEffect::NotDispatched,
                EffectWire::NoMutation => ConnectorEffect::NoMutation,
                EffectWire::Acknowledged => ConnectorEffect::Acknowledged,
                EffectWire::Unknown => ConnectorEffect::Unknown,
            },
            partial: self.partial,
            verifies: self.verifies.map(RequestWire::into_request).transpose()?,
            non_authoritative_raw: self.non_authoritative_raw,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservationWire {
    metadata: MetadataWire,
    target: TargetWire,
    inventory_revision: ContentDigest,
    correlation: NonEmptyText,
    source: SourceId,
    source_revision: Option<SourceTimestamp>,
    content_digest: ContentDigest,
    records: String,
    acquired_at: i64,
    basis: BasisWire,
}
impl ObservationWire {
    fn from_observation(v: &ConnectorObservation) -> Result<Self> {
        Ok(Self {
            metadata: MetadataWire::from_metadata(&v.metadata),
            target: TargetWire::from_target(&v.target),
            inventory_revision: v.inventory_revision.clone(),
            correlation: v.correlation.clone(),
            source: v.source.clone(),
            source_revision: v.source_revision.clone(),
            content_digest: v.content_digest.clone(),
            records: v
                .records
                .to_json()
                .map_err(|_| SnapshotError::InvalidEncoding)?,
            acquired_at: v.acquired_at.seconds(),
            basis: match v.basis {
                ConnectorObservationBasis::ProviderAcknowledgement => {
                    BasisWire::ProviderAcknowledgement
                }
                ConnectorObservationBasis::SourceRead => BasisWire::SourceRead,
                ConnectorObservationBasis::IndependentPostEffectRead => {
                    BasisWire::IndependentPostEffectRead
                }
            },
        })
    }
    fn into_observation(self) -> Result<ConnectorObservation> {
        Ok(ConnectorObservation {
            metadata: self.metadata.into_metadata()?,
            target: self.target.into_target(),
            inventory_revision: self.inventory_revision,
            correlation: self.correlation,
            source: self.source,
            source_revision: self.source_revision,
            content_digest: self.content_digest,
            records: decode_records(&self.records)?,
            acquired_at: UnixTimestamp::new(self.acquired_at),
            basis: match self.basis {
                BasisWire::ProviderAcknowledgement => {
                    ConnectorObservationBasis::ProviderAcknowledgement
                }
                BasisWire::SourceRead => ConnectorObservationBasis::SourceRead,
                BasisWire::IndependentPostEffectRead => {
                    ConnectorObservationBasis::IndependentPostEffectRead
                }
            },
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QualityWire {
    subject: SubjectPath,
    metadata: Vec<QualityMetadata>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchWire {
    scope: ContextScopeId,
    source: SourceId,
    source_kind: SourceKind,
    change_token: Option<SourceTimestamp>,
    digest: Option<ContentDigest>,
    records: String,
    quality_metadata: Vec<QualityWire>,
}
impl BatchWire {
    fn from_batch(v: &ScopedObservationBatch) -> Result<Self> {
        Ok(Self {
            scope: v.scope().clone(),
            source: v.snapshot().source_id().clone(),
            source_kind: v.snapshot().source_kind(),
            change_token: v.snapshot().change_token().cloned(),
            digest: v.snapshot().digest().cloned(),
            records: v
                .records()
                .to_json()
                .map_err(|_| SnapshotError::InvalidEncoding)?,
            quality_metadata: v
                .quality_metadata()
                .iter()
                .map(|(subject, metadata)| QualityWire {
                    subject: subject.clone(),
                    metadata: metadata.clone(),
                })
                .collect(),
        })
    }
    fn into_batch(self) -> Result<ScopedObservationBatch> {
        let snapshot = SourceSnapshot::new(
            self.source,
            self.source_kind,
            self.change_token,
            self.digest,
        )
        .map_err(|_| SnapshotError::InvalidEvidence)?;
        let records = decode_records(&self.records)?;
        let mut batch = ScopedObservationBatch::new(self.scope, snapshot, records)
            .map_err(|_| SnapshotError::InvalidEvidence)?;
        let mut subjects = BTreeSet::new();
        for quality in self.quality_metadata {
            if !subjects.insert(quality.subject.clone()) {
                return Err(SnapshotError::InvalidEncoding);
            }
            batch = batch.with_quality_metadata(quality.subject, quality.metadata);
        }
        Ok(batch)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcquisitionWire {
    request: RequestWire,
    observation: ObservationWire,
    batch: BatchWire,
}
impl AcquisitionWire {
    fn from_acquisition(v: &SnapshotAcquisition) -> Result<Self> {
        Ok(Self {
            request: RequestWire::from_request(&v.request),
            observation: ObservationWire::from_observation(&v.observation)?,
            batch: BatchWire::from_batch(&v.batch)?,
        })
    }
    fn into_acquisition(self) -> Result<SnapshotAcquisition> {
        Ok(SnapshotAcquisition {
            request: self.request.into_request()?,
            observation: self.observation.into_observation()?,
            batch: self.batch.into_batch()?,
        })
    }
}
fn decode_records(raw: &str) -> Result<ObservationEvidenceSet> {
    strict_value(raw.as_bytes())?;
    ObservationEvidenceSet::from_json(raw).map_err(|_| SnapshotError::InvalidEvidence)
}

/// Duplicate keys are rejected before serde can collapse them, including inside
/// embedded native CG06 record strings. The same bound applies at every parse.
pub(in crate::sessions) fn strict_value(bytes: &[u8]) -> Result<Value> {
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(SnapshotError::LimitExceeded);
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let value = Node(0)
        .deserialize(&mut decoder)
        .map_err(|_| SnapshotError::InvalidEncoding)?;
    decoder.end().map_err(|_| SnapshotError::InvalidEncoding)?;
    Ok(value)
}
struct Node(usize);
impl<'de> DeserializeSeed<'de> for Node {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        de: D,
    ) -> std::result::Result<Value, D::Error> {
        if self.0 > MAX_SNAPSHOT_DEPTH {
            return Err(D::Error::custom("snapshot depth exceeded"));
        }
        de.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Node {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded snapshot JSON with unique object fields")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut input: A) -> std::result::Result<Value, A::Error> {
        let mut object = Map::new();
        while let Some(key) = input.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(A::Error::custom("duplicate snapshot field"));
            }
            object.insert(key, input.next_value_seed(Node(self.0 + 1))?);
        }
        Ok(Value::Object(object))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut input: A) -> std::result::Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = input.next_element_seed(Node(self.0 + 1))? {
            values.push(value)
        }
        Ok(Value::Array(values))
    }
    fn visit_bool<E: Error>(self, v: bool) -> std::result::Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E: Error>(self, v: i64) -> std::result::Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_u64<E: Error>(self, v: u64) -> std::result::Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_f64<E: Error>(self, v: f64) -> std::result::Result<Value, E> {
        Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("invalid snapshot number"))
    }
    fn visit_str<E: Error>(self, v: &str) -> std::result::Result<Value, E> {
        Ok(Value::String(v.into()))
    }
    fn visit_string<E: Error>(self, v: String) -> std::result::Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_unit<E: Error>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
}
