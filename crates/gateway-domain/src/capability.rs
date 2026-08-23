//! Validated capability declarations used by execution planning.

use std::{fmt, str::FromStr};

use crate::ValidationError;

pub use crate::identifiers::CapabilityId;

/// The safety class of a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum CapabilityClass {
    /// Reads or inspects state without changing it.
    Inspect,
    /// May change state and therefore requires policy evaluation.
    Mutate,
}

impl CapabilityClass {
    /// Returns the canonical wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inspect => "INSPECT",
            Self::Mutate => "MUTATE",
        }
    }
}

impl fmt::Display for CapabilityClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CapabilityClass {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "INSPECT" => Ok(Self::Inspect),
            "MUTATE" => Ok(Self::Mutate),
            value => Err(ValidationError::UnknownDomainValue {
                field: "capability_class",
                value: value.to_owned(),
            }),
        }
    }
}

/// A provider-independent capability declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDefinition {
    id: CapabilityId,
    class: CapabilityClass,
}

impl CapabilityDefinition {
    /// Creates a capability with an explicit safety class.
    pub fn new(id: CapabilityId, class: CapabilityClass) -> Self {
        Self { id, class }
    }

    /// Fallible constructor for symmetry with parsing-boundary domain types.
    pub fn try_new(id: CapabilityId, class: CapabilityClass) -> Result<Self, ValidationError> {
        Ok(Self::new(id, class))
    }

    /// Returns the typed capability identity.
    #[must_use]
    pub fn id(&self) -> &CapabilityId {
        &self.id
    }

    /// Returns the capability safety class.
    #[must_use]
    pub const fn class(&self) -> CapabilityClass {
        self.class
    }

    /// Returns whether policy consent is required by the capability class.
    #[must_use]
    pub const fn requires_mutation_policy(&self) -> bool {
        matches!(self.class, CapabilityClass::Mutate)
    }
}

/// Short name for a capability declaration in planning APIs.
pub type Capability = CapabilityDefinition;

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{CapabilityClass, CapabilityDefinition};
    use crate::{CapabilityId, ValidationError};

    #[test]
    fn exposes_capability_identity_and_class() {
        let inspect = CapabilityDefinition::new(
            CapabilityId::new("repository.read").unwrap(),
            CapabilityClass::Inspect,
        );
        let mutate = CapabilityDefinition::new(
            CapabilityId::new("repository.write").unwrap(),
            CapabilityClass::Mutate,
        );

        assert_eq!(inspect.id().as_str(), "repository.read");
        assert_eq!(inspect.class(), CapabilityClass::Inspect);
        assert!(!inspect.requires_mutation_policy());
        assert_eq!(mutate.class().as_str(), "MUTATE");
        assert!(mutate.requires_mutation_policy());
        assert_eq!(
            CapabilityClass::from_str("INSPECT").unwrap(),
            CapabilityClass::Inspect
        );
        assert_eq!(CapabilityClass::Mutate.to_string(), "MUTATE");
        assert!(matches!(
            CapabilityClass::from_str("WRITE"),
            Err(ValidationError::UnknownDomainValue {
                field: "capability_class",
                ..
            })
        ));
        assert!(
            CapabilityDefinition::try_new(
                CapabilityId::new("quality.run").unwrap(),
                CapabilityClass::Inspect,
            )
            .is_ok()
        );
    }
}
