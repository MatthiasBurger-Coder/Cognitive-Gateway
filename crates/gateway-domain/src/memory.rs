//! Versioned, scoped experience records. Memory is derived information only.
use crate::{
    ConflictStatus, ContentDigest, ContextScopeId, FreshnessPolicy, FreshnessStatus, NonEmptyText,
    ProvenanceId, QualityMetadata, ReferenceId, SensitivityClass, TrustClass, UnixTimestamp,
    ValidationError,
};
use serde::{Deserialize, Serialize};

pub const MEMORY_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurationState {
    Pending,
    Validated,
    Rejected,
    Invalidated,
    Superseded,
    Forgotten,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryPayload {
    Inline(NonEmptyText),
    Reference(ReferenceId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExperienceRecord {
    pub schema_version: u16,
    pub id: ReferenceId,
    pub scope: ContextScopeId,
    pub provenance: ProvenanceId,
    pub source_snapshot: ReferenceId,
    pub source_version: NonEmptyText,
    pub source_digest: ContentDigest,
    pub created_at: UnixTimestamp,
    pub observed_at: UnixTimestamp,
    pub valid_from: UnixTimestamp,
    pub expires_at: UnixTimestamp,
    pub max_age_seconds: u64,
    pub quality: QualityMetadata,
    pub validation: Option<ReferenceId>,
    pub outcome: Option<NonEmptyText>,
    pub label_basis: Option<ReferenceId>,
    pub payload: Option<MemoryPayload>,
}

impl ExperienceRecord {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != MEMORY_SCHEMA_VERSION
            || self.observed_at > self.created_at
            || self.valid_from > self.expires_at
            || self.quality.trust() != TrustClass::DerivedAssessment
            || self.payload.is_none()
        {
            return Err(ValidationError::InvalidDeclarativeValue {
                reason: "invalid experience time or trust",
            });
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, crate::SerializationError> {
        self.validate()?;
        Ok(serde_json::to_string(&WireExperienceRecord::from(self))?)
    }

    pub fn from_json(json: &str) -> Result<Self, crate::SerializationError> {
        let wire: WireExperienceRecord = serde_json::from_str(json)?;
        Ok(Self::try_from(wire)?)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "SCREAMING_SNAKE_CASE")]
enum WireMemoryPayload {
    Inline(String),
    Reference(ReferenceId),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireExperienceRecord {
    schema_version: u16,
    id: ReferenceId,
    scope: ContextScopeId,
    provenance: ProvenanceId,
    source_snapshot: ReferenceId,
    source_version: String,
    source_digest: ContentDigest,
    created_at: UnixTimestamp,
    observed_at: UnixTimestamp,
    valid_from: UnixTimestamp,
    expires_at: UnixTimestamp,
    max_age_seconds: u64,
    quality: QualityMetadata,
    validation: Option<ReferenceId>,
    outcome: Option<String>,
    label_basis: Option<ReferenceId>,
    payload: Option<WireMemoryPayload>,
}

impl From<&ExperienceRecord> for WireExperienceRecord {
    fn from(record: &ExperienceRecord) -> Self {
        Self {
            schema_version: record.schema_version,
            id: record.id.clone(),
            scope: record.scope.clone(),
            provenance: record.provenance.clone(),
            source_snapshot: record.source_snapshot.clone(),
            source_version: record.source_version.as_str().to_owned(),
            source_digest: record.source_digest.clone(),
            created_at: record.created_at,
            observed_at: record.observed_at,
            valid_from: record.valid_from,
            expires_at: record.expires_at,
            max_age_seconds: record.max_age_seconds,
            quality: record.quality,
            validation: record.validation.clone(),
            outcome: record.outcome.as_ref().map(|text| text.as_str().to_owned()),
            label_basis: record.label_basis.clone(),
            payload: record.payload.as_ref().map(|payload| match payload {
                MemoryPayload::Inline(text) => WireMemoryPayload::Inline(text.as_str().to_owned()),
                MemoryPayload::Reference(reference) => {
                    WireMemoryPayload::Reference(reference.clone())
                }
            }),
        }
    }
}

impl TryFrom<WireExperienceRecord> for ExperienceRecord {
    type Error = ValidationError;

    fn try_from(wire: WireExperienceRecord) -> Result<Self, Self::Error> {
        let record = Self {
            schema_version: wire.schema_version,
            id: wire.id,
            scope: wire.scope,
            provenance: wire.provenance,
            source_snapshot: wire.source_snapshot,
            source_version: NonEmptyText::new_for_field(wire.source_version, "source_version")?,
            source_digest: wire.source_digest,
            created_at: wire.created_at,
            observed_at: wire.observed_at,
            valid_from: wire.valid_from,
            expires_at: wire.expires_at,
            max_age_seconds: wire.max_age_seconds,
            quality: wire.quality,
            validation: wire.validation,
            outcome: wire
                .outcome
                .map(|text| NonEmptyText::new_for_field(text, "outcome"))
                .transpose()?,
            label_basis: wire.label_basis,
            payload: wire
                .payload
                .map(|payload| match payload {
                    WireMemoryPayload::Inline(text) => Ok(MemoryPayload::Inline(
                        NonEmptyText::new_for_field(text, "payload")?,
                    )),
                    WireMemoryPayload::Reference(reference) => {
                        Ok(MemoryPayload::Reference(reference))
                    }
                })
                .transpose()?,
        };
        record.validate()?;
        Ok(record)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryEntry {
    pub schema_version: u16,
    pub revision: u64,
    pub record: ExperienceRecord,
    pub state: CurationState,
    pub superseded_by: Option<ReferenceId>,
    /// Persisted revocation sequence; consumers must revalidate against it.
    pub eligibility_version: u64,
    /// Forgotten entries retain only non-payload tombstone metadata.
    pub payload_forgotten: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryReason {
    Eligible,
    PendingValidation,
    Rejected,
    Invalidated,
    Superseded,
    Forgotten,
    NotYetValid,
    Expired,
    Stale,
    Uncertain,
    Conflict,
    MissingValidation,
    MissingOutcome,
    MissingLabelBasis,
    UnknownConfidence,
    SensitiveReferenceRequired,
}

impl MemoryReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Eligible => "MEMORY_ELIGIBLE",
            Self::PendingValidation => "MEMORY_PENDING_VALIDATION",
            Self::Rejected => "MEMORY_REJECTED",
            Self::Invalidated => "MEMORY_INVALIDATED",
            Self::Superseded => "MEMORY_SUPERSEDED",
            Self::Forgotten => "MEMORY_FORGOTTEN",
            Self::NotYetValid => "MEMORY_NOT_YET_VALID",
            Self::Expired => "MEMORY_EXPIRED",
            Self::Stale => "MEMORY_STALE",
            Self::Uncertain => "MEMORY_UNCERTAIN",
            Self::Conflict => "MEMORY_CONFLICT",
            Self::MissingValidation => "MEMORY_MISSING_VALIDATION",
            Self::MissingOutcome => "MEMORY_MISSING_OUTCOME",
            Self::MissingLabelBasis => "MEMORY_MISSING_LABEL_BASIS",
            Self::UnknownConfidence => "MEMORY_UNKNOWN_CONFIDENCE",
            Self::SensitiveReferenceRequired => "MEMORY_SENSITIVE_REFERENCE_REQUIRED",
        }
    }
}

impl MemoryEntry {
    pub fn new(record: ExperienceRecord) -> Result<Self, ValidationError> {
        record.validate()?;
        Ok(Self {
            schema_version: MEMORY_SCHEMA_VERSION,
            revision: 1,
            record,
            state: CurationState::Pending,
            superseded_by: None,
            eligibility_version: 1,
            payload_forgotten: false,
        })
    }

    pub fn reasons(&self, at: UnixTimestamp) -> Vec<MemoryReason> {
        let mut reasons = Vec::new();
        match self.state {
            CurationState::Pending => reasons.push(MemoryReason::PendingValidation),
            CurationState::Rejected => reasons.push(MemoryReason::Rejected),
            CurationState::Invalidated => reasons.push(MemoryReason::Invalidated),
            CurationState::Superseded => reasons.push(MemoryReason::Superseded),
            CurationState::Forgotten => reasons.push(MemoryReason::Forgotten),
            CurationState::Validated => {}
        }
        if at < self.record.valid_from {
            reasons.push(MemoryReason::NotYetValid);
        }
        if at > self.record.expires_at {
            reasons.push(MemoryReason::Expired);
        }
        if self.record.quality.freshness() != FreshnessStatus::Fresh
            || crate::evaluate_freshness(
                Some(self.record.observed_at),
                Some(at),
                FreshnessPolicy::new(self.record.max_age_seconds),
            ) != Ok(FreshnessStatus::Fresh)
        {
            reasons.push(MemoryReason::Stale);
        }
        if self.record.quality.uncertainty() != crate::Uncertainty::None {
            reasons.push(MemoryReason::Uncertain);
        }
        if self.record.quality.conflict() != ConflictStatus::None {
            reasons.push(MemoryReason::Conflict);
        }
        if self.record.validation.is_none() {
            reasons.push(MemoryReason::MissingValidation);
        }
        if self.record.quality.confidence().as_fraction().is_none() {
            reasons.push(MemoryReason::UnknownConfidence);
        }
        if self.record.quality.sensitivity() >= SensitivityClass::Confidential
            && matches!(self.record.payload, Some(MemoryPayload::Inline(_)))
        {
            reasons.push(MemoryReason::SensitiveReferenceRequired);
        }
        if reasons.is_empty() {
            reasons.push(MemoryReason::Eligible);
        }
        reasons
    }

    pub fn learning_reasons(&self, at: UnixTimestamp) -> Vec<MemoryReason> {
        let mut reasons = self.reasons(at);
        if self.record.outcome.is_none() {
            reasons.push(MemoryReason::MissingOutcome);
        }
        if self.record.label_basis.is_none() {
            reasons.push(MemoryReason::MissingLabelBasis);
        }
        if reasons.len() > 1 {
            reasons.retain(|reason| *reason != MemoryReason::Eligible);
        }
        reasons
    }

    pub fn is_current(&self, at: UnixTimestamp) -> bool {
        self.reasons(at) == [MemoryReason::Eligible]
    }
}

/// A dataset manifest pins this reference; it is never an eternal grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryEligibilityReference {
    pub schema_version: u16,
    pub scope: ContextScopeId,
    pub id: ReferenceId,
    pub revision: u64,
    pub eligibility_version: u64,
    pub source_snapshot: ReferenceId,
    pub source_digest: ContentDigest,
}
