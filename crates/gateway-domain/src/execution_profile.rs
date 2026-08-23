use std::{fmt, str::FromStr};

use crate::validation::ValidationError;

/// The verification/execution depth for one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ExecutionProfile {
    FastPath,
    NormalPath,
    FullPath,
}

impl ExecutionProfile {
    /// Returns the canonical wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FastPath => "FAST_PATH",
            Self::NormalPath => "NORMAL_PATH",
            Self::FullPath => "FULL_PATH",
        }
    }
}

impl fmt::Display for ExecutionProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ExecutionProfile {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "FAST_PATH" => Ok(Self::FastPath),
            "NORMAL_PATH" => Ok(Self::NormalPath),
            "FULL_PATH" => Ok(Self::FullPath),
            value => Err(ValidationError::UnknownDomainValue {
                field: "execution_profile",
                value: value.to_owned(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::ExecutionProfile;
    use crate::ValidationError;

    #[test]
    fn uses_canonical_values_and_round_trips_them() {
        for profile in [
            ExecutionProfile::FastPath,
            ExecutionProfile::NormalPath,
            ExecutionProfile::FullPath,
        ] {
            assert_eq!(
                ExecutionProfile::from_str(profile.as_str()).unwrap(),
                profile
            );
            assert_eq!(profile.to_string(), profile.as_str());
        }
    }

    #[test]
    fn rejects_unknown_and_malformed_values() {
        assert!(matches!(
            ExecutionProfile::from_str("FULL"),
            Err(ValidationError::UnknownDomainValue {
                field: "execution_profile",
                ..
            })
        ));
        assert!(ExecutionProfile::from_str("").is_err());
    }
}
