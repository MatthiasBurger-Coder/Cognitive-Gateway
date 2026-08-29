//! Explicit information-quality, handling and freshness semantics.

use crate::{task::TaskConfidence, validation::ValidationError};

/// Provider-independent trust classification; none of these classes grants authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum TrustClass {
    CanonicalReference,
    ObservedEvidence,
    RetrievedContent,
    CallerInput,
    DerivedAssessment,
    SyntheticData,
    Mixed,
}

impl TrustClass {
    /// Returns the stable machine-readable trust class.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CanonicalReference => "CANONICAL_REFERENCE",
            Self::ObservedEvidence => "OBSERVED_EVIDENCE",
            Self::RetrievedContent => "RETRIEVED_CONTENT",
            Self::CallerInput => "CALLER_INPUT",
            Self::DerivedAssessment => "DERIVED_ASSESSMENT",
            Self::SyntheticData => "SYNTHETIC_DATA",
            Self::Mixed => "MIXED",
        }
    }
}

/// Provider-independent information handling classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum SensitivityClass {
    Public,
    Normal,
    Internal,
    Confidential,
    Secret,
}

impl SensitivityClass {
    /// Returns the stable machine-readable classification.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "PUBLIC",
            Self::Normal => "NORMAL",
            Self::Internal => "INTERNAL",
            Self::Confidential => "CONFIDENTIAL",
            Self::Secret => "SECRET",
        }
    }
    /// Returns the stronger handling classification.
    #[must_use]
    pub const fn strongest(self, other: Self) -> Self {
        if self as u8 >= other as u8 {
            self
        } else {
            other
        }
    }
}

/// Confidence is explicit and keeps unknown/not-applicable distinct from zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum Confidence {
    Unknown,
    NotApplicable,
    Score(TaskConfidence),
}

impl Confidence {
    /// Creates a bounded confidence score.
    pub fn score(value: f64) -> Result<Self, ValidationError> {
        Ok(Self::Score(TaskConfidence::new(value)?))
    }

    /// Returns the score, or None for unknown/not-applicable.
    #[must_use]
    pub fn as_fraction(self) -> Option<f64> {
        match self {
            Self::Score(value) => Some(value.as_fraction()),
            Self::Unknown | Self::NotApplicable => None,
        }
    }

    /// Returns the stable semantic category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::NotApplicable => "NOT_APPLICABLE",
            Self::Score(_) => "SCORE",
        }
    }
}

/// Explicit uncertainty about completeness or probabilistic interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum Uncertainty {
    None,
    Incomplete,
    Probabilistic,
    Unknown,
}

impl Uncertainty {
    /// Returns the stable machine-readable uncertainty category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Incomplete => "INCOMPLETE",
            Self::Probabilistic => "PROBABILISTIC",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Explicit conflict status for quality propagation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ConflictStatus {
    None,
    Unresolved,
}

impl ConflictStatus {
    /// Returns the stable machine-readable conflict category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Unresolved => "UNRESOLVED",
        }
    }
}

/// A deterministic, explicit evaluation time in Unix seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct UnixTimestamp(i64);

impl UnixTimestamp {
    /// Creates an explicit time point; no current time is read.
    #[must_use]
    pub const fn new(seconds: i64) -> Self {
        Self(seconds)
    }

    /// Returns Unix seconds.
    #[must_use]
    pub const fn seconds(self) -> i64 {
        self.0
    }
}

/// A validity interval with explicit inclusive endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ValidityInterval {
    not_before: Option<UnixTimestamp>,
    not_after: Option<UnixTimestamp>,
}

impl ValidityInterval {
    /// Creates an interval and rejects an impossible ordering.
    pub fn new(
        not_before: Option<UnixTimestamp>,
        not_after: Option<UnixTimestamp>,
    ) -> Result<Self, ValidationError> {
        if let (Some(start), Some(end)) = (not_before, not_after) {
            if start > end {
                return Err(ValidationError::InvalidDeclarativeValue {
                    reason: "validity interval starts after it ends",
                });
            }
        }
        Ok(Self {
            not_before,
            not_after,
        })
    }

    /// Returns whether an explicit time lies inside this interval.
    #[must_use]
    pub fn contains(self, time: UnixTimestamp) -> bool {
        self.not_before.is_none_or(|start| time >= start)
            && self.not_after.is_none_or(|end| time <= end)
    }

    /// Returns the inclusive lower bound.
    #[must_use]
    pub const fn not_before(self) -> Option<UnixTimestamp> {
        self.not_before
    }

    /// Returns the inclusive upper bound.
    #[must_use]
    pub const fn not_after(self) -> Option<UnixTimestamp> {
        self.not_after
    }
}

/// An explicit maximum-age policy for freshness evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct FreshnessPolicy {
    max_age_seconds: u64,
}

impl FreshnessPolicy {
    /// Creates a finite maximum-age policy.
    #[must_use]
    pub const fn new(max_age_seconds: u64) -> Self {
        Self { max_age_seconds }
    }

    /// Returns the maximum permitted age.
    #[must_use]
    pub const fn max_age_seconds(self) -> u64 {
        self.max_age_seconds
    }
}

/// Result of evaluating freshness with explicit timestamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum FreshnessStatus {
    Fresh,
    Stale,
    Unknown,
}

impl FreshnessStatus {
    /// Returns the stable machine-readable freshness category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "FRESH",
            Self::Stale => "STALE",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Evaluates freshness without accessing an ambient clock.
pub fn evaluate_freshness(
    observed_at: Option<UnixTimestamp>,
    evaluation_time: Option<UnixTimestamp>,
    policy: FreshnessPolicy,
) -> Result<FreshnessStatus, ValidationError> {
    let (Some(observed_at), Some(evaluation_time)) = (observed_at, evaluation_time) else {
        return Ok(FreshnessStatus::Unknown);
    };
    if observed_at > evaluation_time {
        return Err(ValidationError::InvalidDeclarativeValue {
            reason: "observed time is later than explicit evaluation time",
        });
    }
    let age = u64::try_from(
        evaluation_time
            .seconds()
            .checked_sub(observed_at.seconds())
            .ok_or(ValidationError::InvalidDeclarativeValue {
                reason: "freshness age cannot be represented",
            })?,
    )
    .map_err(|_| ValidationError::InvalidDeclarativeValue {
        reason: "freshness age cannot be represented",
    })?;
    Ok(if age <= policy.max_age_seconds {
        FreshnessStatus::Fresh
    } else {
        FreshnessStatus::Stale
    })
}

/// Quality metadata propagated through normalized state without granting authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct QualityMetadata {
    trust: TrustClass,
    sensitivity: SensitivityClass,
    confidence: Confidence,
    freshness: FreshnessStatus,
    uncertainty: Uncertainty,
    conflict: ConflictStatus,
}

impl QualityMetadata {
    /// Creates explicit quality metadata.
    #[must_use]
    pub const fn new(
        trust: TrustClass,
        sensitivity: SensitivityClass,
        confidence: Confidence,
        freshness: FreshnessStatus,
        uncertainty: Uncertainty,
    ) -> Self {
        Self {
            trust,
            sensitivity,
            confidence,
            freshness,
            uncertainty,
            conflict: ConflictStatus::None,
        }
    }

    /// Marks the quality basis as unresolved without resolving it.
    #[must_use]
    pub const fn with_conflict(mut self, conflict: ConflictStatus) -> Self {
        self.conflict = conflict;
        self
    }

    /// Merges explicit metadata conservatively and deterministically.
    #[must_use]
    pub fn merge(values: &[Self]) -> Option<Self> {
        let first = *values.first()?;
        let trust = if values.iter().all(|value| value.trust == first.trust) {
            first.trust
        } else {
            TrustClass::Mixed
        };
        let sensitivity = values
            .iter()
            .map(|value| value.sensitivity)
            .fold(SensitivityClass::Public, SensitivityClass::strongest);
        let confidence = if values
            .iter()
            .all(|value| value.confidence == first.confidence)
        {
            first.confidence
        } else {
            Confidence::Unknown
        };
        let freshness = if values
            .iter()
            .any(|value| value.freshness == FreshnessStatus::Stale)
        {
            FreshnessStatus::Stale
        } else if values
            .iter()
            .all(|value| value.freshness == FreshnessStatus::Fresh)
        {
            FreshnessStatus::Fresh
        } else {
            FreshnessStatus::Unknown
        };
        let uncertainty = if values
            .iter()
            .any(|value| value.uncertainty == Uncertainty::Unknown)
        {
            Uncertainty::Unknown
        } else if values
            .iter()
            .any(|value| value.uncertainty == Uncertainty::Incomplete)
        {
            Uncertainty::Incomplete
        } else if values
            .iter()
            .any(|value| value.uncertainty == Uncertainty::Probabilistic)
        {
            Uncertainty::Probabilistic
        } else {
            Uncertainty::None
        };
        let conflict = if values
            .iter()
            .any(|value| value.conflict == ConflictStatus::Unresolved)
        {
            ConflictStatus::Unresolved
        } else {
            ConflictStatus::None
        };
        Some(Self {
            trust,
            sensitivity,
            confidence,
            freshness,
            uncertainty,
            conflict,
        })
    }

    /// Returns the trust classification.
    #[must_use]
    pub const fn trust(self) -> TrustClass {
        self.trust
    }

    /// Returns the strongest sensitivity classification.
    #[must_use]
    pub const fn sensitivity(self) -> SensitivityClass {
        self.sensitivity
    }

    /// Returns confidence, including unknown/not-applicable semantics.
    #[must_use]
    pub const fn confidence(self) -> Confidence {
        self.confidence
    }

    /// Returns freshness status.
    #[must_use]
    pub const fn freshness(self) -> FreshnessStatus {
        self.freshness
    }

    /// Returns uncertainty classification.
    #[must_use]
    pub const fn uncertainty(self) -> Uncertainty {
        self.uncertainty
    }

    /// Returns unresolved-conflict status.
    #[must_use]
    pub const fn conflict(self) -> ConflictStatus {
        self.conflict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(
        trust: TrustClass,
        sensitivity: SensitivityClass,
        confidence: Confidence,
        freshness: FreshnessStatus,
        uncertainty: Uncertainty,
    ) -> QualityMetadata {
        QualityMetadata::new(trust, sensitivity, confidence, freshness, uncertainty)
    }

    #[test]
    fn trust_sensitivity_confidence_and_uncertainty_are_distinct() {
        for trust in [
            TrustClass::CanonicalReference,
            TrustClass::ObservedEvidence,
            TrustClass::RetrievedContent,
            TrustClass::CallerInput,
            TrustClass::DerivedAssessment,
            TrustClass::SyntheticData,
            TrustClass::Mixed,
        ] {
            assert!(!trust.as_str().is_empty());
        }
        for sensitivity in [
            SensitivityClass::Public,
            SensitivityClass::Normal,
            SensitivityClass::Internal,
            SensitivityClass::Confidential,
            SensitivityClass::Secret,
        ] {
            assert!(!sensitivity.as_str().is_empty());
        }
        assert_eq!(TrustClass::RetrievedContent.as_str(), "RETRIEVED_CONTENT");
        assert_eq!(TrustClass::Mixed.as_str(), "MIXED");
        assert_eq!(SensitivityClass::Secret.as_str(), "SECRET");
        assert_eq!(
            SensitivityClass::Public.strongest(SensitivityClass::Confidential),
            SensitivityClass::Confidential
        );
        assert_eq!(Confidence::Unknown.as_fraction(), None);
        assert_eq!(Confidence::NotApplicable.as_fraction(), None);
        assert_eq!(Confidence::score(0.98).unwrap().as_fraction(), Some(0.98));
        assert_eq!(Confidence::Unknown.as_str(), "UNKNOWN");
        assert_eq!(Confidence::NotApplicable.as_str(), "NOT_APPLICABLE");
        assert_eq!(Confidence::score(0.98).unwrap().as_str(), "SCORE");
        assert!(Confidence::score(-0.01).is_err());
        assert!(Confidence::score(1.01).is_err());
        assert_eq!(Uncertainty::None.as_str(), "NONE");
        assert_eq!(Uncertainty::Incomplete.as_str(), "INCOMPLETE");
        assert_eq!(Uncertainty::Unknown.as_str(), "UNKNOWN");
        assert_eq!(Uncertainty::Probabilistic.as_str(), "PROBABILISTIC");
        assert_eq!(ConflictStatus::None.as_str(), "NONE");
        assert_eq!(ConflictStatus::Unresolved.as_str(), "UNRESOLVED");
    }

    #[test]
    fn intervals_and_freshness_use_only_explicit_time() {
        let start = UnixTimestamp::new(100);
        let end = UnixTimestamp::new(200);
        let interval = ValidityInterval::new(Some(start), Some(end)).unwrap();
        assert!(interval.contains(UnixTimestamp::new(100)));
        assert!(!interval.contains(UnixTimestamp::new(201)));
        assert_eq!(interval.not_before(), Some(start));
        assert_eq!(interval.not_after(), Some(end));
        assert!(ValidityInterval::new(Some(end), Some(start)).is_err());
        let policy = FreshnessPolicy::new(50);
        assert_eq!(policy.max_age_seconds(), 50);
        assert_eq!(
            evaluate_freshness(
                Some(UnixTimestamp::new(100)),
                Some(UnixTimestamp::new(150)),
                policy
            )
            .unwrap(),
            FreshnessStatus::Fresh
        );
        assert_eq!(
            evaluate_freshness(
                Some(UnixTimestamp::new(100)),
                Some(UnixTimestamp::new(151)),
                policy
            )
            .unwrap(),
            FreshnessStatus::Stale
        );
        assert_eq!(
            evaluate_freshness(None, Some(UnixTimestamp::new(151)), policy).unwrap(),
            FreshnessStatus::Unknown
        );
        assert!(
            evaluate_freshness(
                Some(UnixTimestamp::new(151)),
                Some(UnixTimestamp::new(150)),
                policy
            )
            .is_err()
        );
        assert!(
            evaluate_freshness(
                Some(UnixTimestamp::new(i64::MIN)),
                Some(UnixTimestamp::new(i64::MAX)),
                policy
            )
            .is_err()
        );
        assert_eq!(FreshnessStatus::Fresh.as_str(), "FRESH");
        assert_eq!(FreshnessStatus::Stale.as_str(), "STALE");
        assert_eq!(FreshnessStatus::Unknown.as_str(), "UNKNOWN");
    }

    #[test]
    fn quality_merge_is_conservative_and_preserves_strongest_sensitivity() {
        let first = metadata(
            TrustClass::ObservedEvidence,
            SensitivityClass::Internal,
            Confidence::score(0.98).unwrap(),
            FreshnessStatus::Fresh,
            Uncertainty::Probabilistic,
        );
        let second = metadata(
            TrustClass::RetrievedContent,
            SensitivityClass::Secret,
            Confidence::Unknown,
            FreshnessStatus::Stale,
            Uncertainty::Incomplete,
        )
        .with_conflict(ConflictStatus::Unresolved);
        let merged = QualityMetadata::merge(&[first, second]).unwrap();
        assert_eq!(merged.trust(), TrustClass::Mixed);
        assert_eq!(merged.sensitivity(), SensitivityClass::Secret);
        assert_eq!(merged.confidence(), Confidence::Unknown);
        assert_eq!(merged.freshness(), FreshnessStatus::Stale);
        assert_eq!(merged.uncertainty(), Uncertainty::Incomplete);
        assert_eq!(merged.conflict(), ConflictStatus::Unresolved);
        assert!(QualityMetadata::merge(&[]).is_none());
        let same = QualityMetadata::merge(&[first, first]).unwrap();
        assert_eq!(same.trust(), TrustClass::ObservedEvidence);
        assert_eq!(same.sensitivity(), SensitivityClass::Internal);
        assert_eq!(same.confidence(), first.confidence());
        assert_eq!(same.freshness(), FreshnessStatus::Fresh);
        assert_eq!(same.uncertainty(), Uncertainty::Probabilistic);
        assert_eq!(same.conflict(), ConflictStatus::None);
        assert_eq!(first.trust(), TrustClass::ObservedEvidence);
        assert_eq!(first.sensitivity(), SensitivityClass::Internal);
        assert_eq!(first.confidence().as_fraction(), Some(0.98));
    }
}
