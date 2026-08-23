//! Validated constraints that can influence execution planning.

use std::{fmt, str::FromStr};

use crate::{ConstraintId, ExecutionProfile, OperatingMode, ValidationError};

/// The supported semantic effects of an execution constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ConstraintKind {
    /// No feature additions are permitted in the current run.
    FeatureFreeze,
    /// Mutating capabilities require explicit consent before use.
    LiveMutationRequiresConsent,
    /// Release qualification must use the deepest execution profile.
    RequireFullPathForReleaseQualification,
}

impl ConstraintKind {
    /// Returns the canonical wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FeatureFreeze => "FEATURE_FREEZE",
            Self::LiveMutationRequiresConsent => "LIVE_MUTATION_REQUIRES_CONSENT",
            Self::RequireFullPathForReleaseQualification => {
                "REQUIRE_FULL_PATH_FOR_RELEASE_QUALIFICATION"
            }
        }
    }
}

impl fmt::Display for ConstraintKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ConstraintKind {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "FEATURE_FREEZE" => Ok(Self::FeatureFreeze),
            "LIVE_MUTATION_REQUIRES_CONSENT" => Ok(Self::LiveMutationRequiresConsent),
            "REQUIRE_FULL_PATH_FOR_RELEASE_QUALIFICATION" => {
                Ok(Self::RequireFullPathForReleaseQualification)
            }
            value => Err(ValidationError::UnknownDomainValue {
                field: "constraint_kind",
                value: value.to_owned(),
            }),
        }
    }
}

/// A named, provider-independent execution constraint.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Constraint {
    id: ConstraintId,
    kind: ConstraintKind,
}

impl Constraint {
    /// Creates a constraint with a validated identity and semantic effect.
    #[must_use]
    pub fn new(id: ConstraintId, kind: ConstraintKind) -> Self {
        Self { id, kind }
    }

    /// Fallible constructor for callers at a parsing boundary.
    pub fn try_new(id: ConstraintId, kind: ConstraintKind) -> Result<Self, ValidationError> {
        Ok(Self::new(id, kind))
    }

    /// Returns the typed constraint identity.
    #[must_use]
    pub fn id(&self) -> &ConstraintId {
        &self.id
    }

    /// Returns the semantic effect of this constraint.
    #[must_use]
    pub const fn kind(&self) -> ConstraintKind {
        self.kind
    }

    /// Checks the constraint against the independent mode/profile dimensions.
    pub fn validate_for(
        &self,
        operating_mode: OperatingMode,
        execution_profile: ExecutionProfile,
    ) -> Result<(), ValidationError> {
        if matches!(
            self.kind,
            ConstraintKind::RequireFullPathForReleaseQualification
        ) && matches!(operating_mode, OperatingMode::ReleaseQualification)
            && !matches!(execution_profile, ExecutionProfile::FullPath)
        {
            return Err(ValidationError::InvalidStateCombination {
                reason: "release qualification requires FULL_PATH under this constraint",
            });
        }

        Ok(())
    }
}

/// Alias used when constraints are declared as profile definitions.
pub type ConstraintDefinition = Constraint;

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{Constraint, ConstraintKind};
    use crate::{ConstraintId, ExecutionProfile, OperatingMode, ValidationError};

    #[test]
    fn parses_constraint_kinds_without_coercion() {
        for kind in [
            ConstraintKind::FeatureFreeze,
            ConstraintKind::LiveMutationRequiresConsent,
            ConstraintKind::RequireFullPathForReleaseQualification,
        ] {
            assert_eq!(ConstraintKind::from_str(kind.as_str()).unwrap(), kind);
            assert_eq!(kind.to_string(), kind.as_str());
        }
        assert!(matches!(
            ConstraintKind::from_str("feature_freeze"),
            Err(ValidationError::UnknownDomainValue {
                field: "constraint_kind",
                ..
            })
        ));
    }

    #[test]
    fn validates_named_constraints_against_execution_dimensions() {
        let constraint = Constraint::new(
            ConstraintId::new("release-depth").unwrap(),
            ConstraintKind::RequireFullPathForReleaseQualification,
        );
        assert_eq!(constraint.id().as_str(), "release-depth");
        assert_eq!(
            constraint.kind(),
            ConstraintKind::RequireFullPathForReleaseQualification
        );
        assert!(
            Constraint::try_new(
                ConstraintId::new("feature-freeze").unwrap(),
                ConstraintKind::FeatureFreeze,
            )
            .is_ok()
        );
        assert!(
            constraint
                .validate_for(OperatingMode::Development, ExecutionProfile::FastPath)
                .is_ok()
        );
        assert!(
            constraint
                .validate_for(
                    OperatingMode::ReleaseQualification,
                    ExecutionProfile::FullPath
                )
                .is_ok()
        );
        assert!(
            constraint
                .validate_for(
                    OperatingMode::ReleaseQualification,
                    ExecutionProfile::NormalPath
                )
                .is_err()
        );
    }
}
