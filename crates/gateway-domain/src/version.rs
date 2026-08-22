//! Version value objects shared by versioned domain contracts.

use std::{fmt, str::FromStr};

use crate::validation::ValidationError;

/// A major/minor version for a domain schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SchemaVersion {
    major: u16,
    minor: u16,
}

impl SchemaVersion {
    /// The first schema version used by the gateway contracts.
    pub const V1: Self = Self { major: 1, minor: 0 };

    /// Creates a schema version. Major version zero is reserved for drafts.
    pub const fn new(major: u16, minor: u16) -> Result<Self, ValidationError> {
        if major == 0 {
            Err(ValidationError::InvalidSchemaVersion)
        } else {
            Ok(Self { major, minor })
        }
    }

    /// Returns the major version component.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    /// Returns the minor version component.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for SchemaVersion {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (major, minor) = value
            .split_once('.')
            .ok_or(ValidationError::InvalidSchemaVersionFormat)?;
        if major.is_empty()
            || minor.is_empty()
            || !major.chars().all(|character| character.is_ascii_digit())
            || !minor.chars().all(|character| character.is_ascii_digit())
            || (major.len() > 1 && major.starts_with('0'))
            || (minor.len() > 1 && minor.starts_with('0'))
        {
            return Err(ValidationError::InvalidSchemaVersionFormat);
        }

        let major = major
            .parse::<u16>()
            .map_err(|_| ValidationError::InvalidSchemaVersionFormat)?;
        let minor = minor
            .parse::<u16>()
            .map_err(|_| ValidationError::InvalidSchemaVersionFormat)?;
        Self::new(major, minor)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::SchemaVersion;

    #[test]
    fn exposes_and_formats_v1() {
        assert_eq!(SchemaVersion::V1.major(), 1);
        assert_eq!(SchemaVersion::V1.minor(), 0);
        assert_eq!(SchemaVersion::V1.to_string(), "1.0");
    }

    #[test]
    fn rejects_invalid_versions() {
        assert!(SchemaVersion::new(0, 1).is_err());
        assert!(SchemaVersion::from_str("1").is_err());
        assert!(SchemaVersion::from_str("one.0").is_err());
        assert!(SchemaVersion::from_str("1.0.0").is_err());
        assert!(SchemaVersion::from_str("01.0").is_err());
        assert_eq!(SchemaVersion::from_str("2.3").unwrap().major(), 2);
    }
}
