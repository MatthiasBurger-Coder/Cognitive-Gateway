use std::{fmt, str::FromStr};

use crate::validation::ValidationError;

/// The project lifecycle in which an execution takes place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum OperatingMode {
    Development,
    Hardening,
    ReleaseQualification,
}

impl OperatingMode {
    /// Returns the canonical wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Development => "DEVELOPMENT",
            Self::Hardening => "HARDENING",
            Self::ReleaseQualification => "RELEASE_QUALIFICATION",
        }
    }
}

impl fmt::Display for OperatingMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for OperatingMode {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "DEVELOPMENT" => Ok(Self::Development),
            "HARDENING" => Ok(Self::Hardening),
            "RELEASE_QUALIFICATION" => Ok(Self::ReleaseQualification),
            value => Err(ValidationError::UnknownDomainValue {
                field: "operating_mode",
                value: value.to_owned(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::OperatingMode;
    use crate::ValidationError;

    #[test]
    fn uses_canonical_values_and_round_trips_them() {
        for mode in [
            OperatingMode::Development,
            OperatingMode::Hardening,
            OperatingMode::ReleaseQualification,
        ] {
            assert_eq!(OperatingMode::from_str(mode.as_str()).unwrap(), mode);
            assert_eq!(mode.to_string(), mode.as_str());
        }
    }

    #[test]
    fn rejects_unknown_and_malformed_values() {
        assert!(matches!(
            OperatingMode::from_str("development"),
            Err(ValidationError::UnknownDomainValue {
                field: "operating_mode",
                ..
            })
        ));
        assert!(OperatingMode::from_str("").is_err());
    }
}
