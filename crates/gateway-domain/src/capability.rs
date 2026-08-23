//! Validated capability declarations used by execution planning.

use std::{collections::BTreeSet, fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{
    CapabilityConstraint, CapabilityDomain, CapabilityInputKind, CapabilityOutputKind,
    CapabilityPrecondition, CapabilityTag, NonEmptyText, ValidationError,
    relationships::unique_relationships,
};

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
    domain: CapabilityDomain,
    description: NonEmptyText,
    input_kinds: Vec<CapabilityInputKind>,
    output_kinds: Vec<CapabilityOutputKind>,
    preconditions: Vec<CapabilityPrecondition>,
    constraints: Vec<CapabilityConstraint>,
    applicability_tags: Vec<CapabilityTag>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireCapabilityDefinition {
    id: String,
    class: String,
    domain: String,
    description: String,
    input_kinds: Vec<String>,
    output_kinds: Vec<String>,
    preconditions: Vec<String>,
    constraints: Vec<String>,
    applicability_tags: Vec<String>,
}

impl CapabilityDefinition {
    /// Creates a capability with an explicit safety class.
    ///
    /// This compatibility constructor creates a valid minimal declaration.
    /// [`Self::new_with_contract`] or the `with_*` methods provide the
    /// complete machine-resolvable metadata.
    pub fn new(id: CapabilityId, class: CapabilityClass) -> Self {
        let domain = CapabilityDomain::new("general")
            .expect("the built-in general capability domain must be valid");
        let description = NonEmptyText::new_for_field(id.as_str(), "description")
            .expect("a validated capability ID is valid description text");
        Self {
            id,
            class,
            domain,
            description,
            input_kinds: Vec::new(),
            output_kinds: Vec::new(),
            preconditions: Vec::new(),
            constraints: Vec::new(),
            applicability_tags: Vec::new(),
        }
    }

    /// Fallible constructor for symmetry with parsing-boundary domain types.
    pub fn try_new(id: CapabilityId, class: CapabilityClass) -> Result<Self, ValidationError> {
        Ok(Self::new(id, class))
    }

    /// Creates a capability with its complete reusable contract metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_contract(
        id: CapabilityId,
        class: CapabilityClass,
        domain: impl Into<String>,
        description: impl Into<String>,
        input_kinds: impl IntoIterator<Item = impl Into<String>>,
        output_kinds: impl IntoIterator<Item = impl Into<String>>,
        preconditions: impl IntoIterator<Item = impl Into<String>>,
        constraints: impl IntoIterator<Item = impl Into<String>>,
        applicability_tags: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ValidationError> {
        Self::new(id, class)
            .with_domain(domain)?
            .with_description(description)?
            .with_input_kinds(input_kinds)?
            .with_output_kinds(output_kinds)?
            .with_preconditions(preconditions)?
            .with_constraints(constraints)?
            .with_applicability_tags(applicability_tags)
    }

    /// Fallible alias for [`Self::new_with_contract`] at parsing boundaries.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new_with_contract(
        id: CapabilityId,
        class: CapabilityClass,
        domain: impl Into<String>,
        description: impl Into<String>,
        input_kinds: impl IntoIterator<Item = impl Into<String>>,
        output_kinds: impl IntoIterator<Item = impl Into<String>>,
        preconditions: impl IntoIterator<Item = impl Into<String>>,
        constraints: impl IntoIterator<Item = impl Into<String>>,
        applicability_tags: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ValidationError> {
        Self::new_with_contract(
            id,
            class,
            domain,
            description,
            input_kinds,
            output_kinds,
            preconditions,
            constraints,
            applicability_tags,
        )
    }

    /// Alias for [`Self::new_with_contract`] using metadata terminology.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_metadata(
        id: CapabilityId,
        class: CapabilityClass,
        domain: impl Into<String>,
        description: impl Into<String>,
        input_kinds: impl IntoIterator<Item = impl Into<String>>,
        output_kinds: impl IntoIterator<Item = impl Into<String>>,
        preconditions: impl IntoIterator<Item = impl Into<String>>,
        constraints: impl IntoIterator<Item = impl Into<String>>,
        applicability_tags: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ValidationError> {
        Self::new_with_contract(
            id,
            class,
            domain,
            description,
            input_kinds,
            output_kinds,
            preconditions,
            constraints,
            applicability_tags,
        )
    }

    /// Sets the reusable capability domain.
    pub fn with_domain(mut self, domain: impl Into<String>) -> Result<Self, ValidationError> {
        self.domain = CapabilityDomain::new(domain)?;
        Ok(self)
    }

    /// Sets the reusable responsibility or purpose statement.
    pub fn with_description(
        mut self,
        description: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        self.description = NonEmptyText::new_for_field(description, "description")?;
        Ok(self)
    }

    /// Sets the typed input or context kinds accepted by the capability.
    pub fn with_input_kinds(
        mut self,
        input_kinds: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ValidationError> {
        self.input_kinds = unique_relationships(
            input_kinds
                .into_iter()
                .map(|value| CapabilityInputKind::new(value))
                .collect::<Result<Vec<_>, _>>()?,
            "input_kinds",
        )?;
        Ok(self)
    }

    /// Sets the typed result or output kinds produced by the capability.
    pub fn with_output_kinds(
        mut self,
        output_kinds: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ValidationError> {
        self.output_kinds = unique_relationships(
            output_kinds
                .into_iter()
                .map(|value| CapabilityOutputKind::new(value))
                .collect::<Result<Vec<_>, _>>()?,
            "output_kinds",
        )?;
        Ok(self)
    }

    /// Sets intrinsic preconditions that are part of the capability contract.
    pub fn with_preconditions(
        mut self,
        preconditions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ValidationError> {
        self.preconditions = unique_relationships(
            preconditions
                .into_iter()
                .map(|value| CapabilityPrecondition::new(value))
                .collect::<Result<Vec<_>, _>>()?,
            "preconditions",
        )?;
        Ok(self)
    }

    /// Sets intrinsic limitations that do not grant or deny authority.
    pub fn with_constraints(
        mut self,
        constraints: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ValidationError> {
        self.constraints = unique_relationships(
            constraints
                .into_iter()
                .map(|value| CapabilityConstraint::new(value))
                .collect::<Result<Vec<_>, _>>()?,
            "constraints",
        )?;
        Ok(self)
    }

    /// Sets deterministic tags/selectors used for applicability matching.
    pub fn with_applicability_tags(
        mut self,
        applicability_tags: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ValidationError> {
        self.applicability_tags = unique_relationships(
            applicability_tags
                .into_iter()
                .map(|value| CapabilityTag::new(value))
                .collect::<Result<Vec<_>, _>>()?,
            "applicability_tags",
        )?;
        Ok(self)
    }

    /// Alias for [`Self::with_applicability_tags`].
    pub fn with_applicability_selectors(
        self,
        selectors: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ValidationError> {
        self.with_applicability_tags(selectors)
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

    /// Returns the reusable capability domain.
    #[must_use]
    pub fn domain(&self) -> &CapabilityDomain {
        &self.domain
    }

    /// Returns the responsibility or purpose statement.
    #[must_use]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }

    /// Alias emphasizing that the description states the capability purpose.
    #[must_use]
    pub fn purpose(&self) -> &str {
        self.description()
    }

    /// Returns the typed input or context kinds accepted by the capability.
    #[must_use]
    pub fn input_kinds(&self) -> &[CapabilityInputKind] {
        &self.input_kinds
    }

    /// Returns the typed result or output kinds produced by the capability.
    #[must_use]
    pub fn output_kinds(&self) -> &[CapabilityOutputKind] {
        &self.output_kinds
    }

    /// Returns intrinsic preconditions of the capability.
    #[must_use]
    pub fn preconditions(&self) -> &[CapabilityPrecondition] {
        &self.preconditions
    }

    /// Returns intrinsic limitations of the capability.
    #[must_use]
    pub fn constraints(&self) -> &[CapabilityConstraint] {
        &self.constraints
    }

    /// Returns deterministic applicability tags/selectors.
    #[must_use]
    pub fn applicability_tags(&self) -> &[CapabilityTag] {
        &self.applicability_tags
    }

    /// Alias for callers that use the shorter capability vocabulary.
    #[must_use]
    pub fn inputs(&self) -> &[CapabilityInputKind] {
        self.input_kinds()
    }

    /// Alias for callers that use the shorter capability vocabulary.
    #[must_use]
    pub fn outputs(&self) -> &[CapabilityOutputKind] {
        self.output_kinds()
    }

    /// Alias for input kinds when callers model them as context classes.
    #[must_use]
    pub fn input_context_kinds(&self) -> &[CapabilityInputKind] {
        self.input_kinds()
    }

    /// Alias for output kinds when callers model them as result classes.
    #[must_use]
    pub fn result_kinds(&self) -> &[CapabilityOutputKind] {
        self.output_kinds()
    }

    /// Alias distinguishing intrinsic constraints from policy.
    #[must_use]
    pub fn intrinsic_constraints(&self) -> &[CapabilityConstraint] {
        self.constraints()
    }

    /// Alias for applicability tags used as deterministic selectors.
    #[must_use]
    pub fn applicability_selectors(&self) -> &[CapabilityTag] {
        self.applicability_tags()
    }

    /// Returns whether policy consent is required by the capability class.
    #[must_use]
    pub const fn requires_mutation_policy(&self) -> bool {
        matches!(self.class, CapabilityClass::Mutate)
    }
}

/// Short name for a capability declaration in planning APIs.
pub type Capability = CapabilityDefinition;

/// Compatibility name emphasizing the declarative contract semantics.
pub type CapabilityContract = CapabilityDefinition;

/// Validates the provider-local capability declarations while retaining their
/// declared order for deterministic serialization and resolution.
pub(crate) fn unique_capabilities(
    capabilities: impl IntoIterator<Item = CapabilityDefinition>,
) -> Result<Vec<CapabilityDefinition>, ValidationError> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for capability in capabilities {
        if !seen.insert(capability.id().clone()) {
            return Err(ValidationError::DuplicateRelationship {
                field: "provided_capabilities",
            });
        }
        result.push(capability);
    }
    Ok(result)
}

impl Serialize for CapabilityDefinition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireCapabilityDefinition {
            id: self.id.to_string(),
            class: self.class.to_string(),
            domain: self.domain.to_string(),
            description: self.description().to_owned(),
            input_kinds: self.input_kinds.iter().map(ToString::to_string).collect(),
            output_kinds: self.output_kinds.iter().map(ToString::to_string).collect(),
            preconditions: self.preconditions.iter().map(ToString::to_string).collect(),
            constraints: self.constraints.iter().map(ToString::to_string).collect(),
            applicability_tags: self
                .applicability_tags
                .iter()
                .map(ToString::to_string)
                .collect(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CapabilityDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WireCapabilityDefinition::deserialize(deserializer)?;
        Self::new_with_contract(
            CapabilityId::new(wire.id).map_err(D::Error::custom)?,
            CapabilityClass::from_str(&wire.class).map_err(D::Error::custom)?,
            wire.domain,
            wire.description,
            wire.input_kinds,
            wire.output_kinds,
            wire.preconditions,
            wire.constraints,
            wire.applicability_tags,
        )
        .map_err(D::Error::custom)
    }
}

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

    #[test]
    fn creates_and_validates_a_machine_resolvable_contract() {
        let capability = CapabilityDefinition::new_with_contract(
            CapabilityId::new("architecture.dependency-analysis").unwrap(),
            CapabilityClass::Inspect,
            "architecture",
            "Analyze dependency direction and boundary relationships",
            ["repository.snapshot"],
            ["architecture.dependency-graph"],
            ["repository.available"],
            ["read-only"],
            ["architecture", "dependency-analysis"],
        )
        .unwrap();

        assert_eq!(capability.domain().as_str(), "architecture");
        assert_eq!(
            capability.description(),
            "Analyze dependency direction and boundary relationships"
        );
        assert_eq!(capability.inputs()[0].as_str(), "repository.snapshot");
        assert_eq!(
            capability.outputs()[0].as_str(),
            "architecture.dependency-graph"
        );
        assert_eq!(
            capability.preconditions()[0].as_str(),
            "repository.available"
        );
        assert_eq!(capability.intrinsic_constraints()[0].as_str(), "read-only");
        assert_eq!(capability.applicability_tags().len(), 2);
    }

    #[test]
    fn covers_contract_aliases_and_typed_accessors() {
        let capability = CapabilityDefinition::new(
            CapabilityId::new("quality.coverage").unwrap(),
            CapabilityClass::Mutate,
        )
        .with_domain("quality")
        .unwrap()
        .with_description("Publish a coverage report")
        .unwrap()
        .with_input_kinds(["repository.snapshot"])
        .unwrap()
        .with_output_kinds(["quality.coverage-report"])
        .unwrap()
        .with_preconditions(["repository.available"])
        .unwrap()
        .with_constraints(["requires-approval"])
        .unwrap()
        .with_applicability_selectors(["quality", "coverage"])
        .unwrap();

        assert_eq!(capability.purpose(), "Publish a coverage report");
        assert_eq!(capability.input_kinds(), capability.inputs());
        assert_eq!(capability.input_kinds(), capability.input_context_kinds());
        assert_eq!(capability.output_kinds(), capability.outputs());
        assert_eq!(capability.output_kinds(), capability.result_kinds());
        assert_eq!(capability.constraints(), capability.intrinsic_constraints());
        assert_eq!(
            capability.applicability_tags(),
            capability.applicability_selectors()
        );
        assert!(capability.requires_mutation_policy());

        let via_try = CapabilityDefinition::try_new_with_contract(
            CapabilityId::new("architecture.inspect").unwrap(),
            CapabilityClass::Inspect,
            "architecture",
            "Inspect architecture",
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
        )
        .unwrap();
        let via_metadata = CapabilityDefinition::new_with_metadata(
            CapabilityId::new("security.inspect").unwrap(),
            CapabilityClass::Inspect,
            "security",
            "Inspect security posture",
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
        )
        .unwrap();
        assert_eq!(via_try.domain().as_str(), "architecture");
        assert_eq!(via_metadata.domain().as_str(), "security");
    }

    #[test]
    fn rejects_invalid_or_duplicate_contract_selectors() {
        let capability = CapabilityDefinition::new(
            CapabilityId::new("inspect").unwrap(),
            CapabilityClass::Inspect,
        );
        assert!(matches!(
            capability.clone().with_domain("bad domain"),
            Err(ValidationError::InvalidIdentifierCharacter { character: ' ' })
        ));
        assert!(matches!(
            capability.clone().with_description("\0"),
            Err(ValidationError::ControlCharacter {
                field: "description"
            })
        ));
        assert!(matches!(
            capability.with_input_kinds(["repository.snapshot", "repository.snapshot"]),
            Err(ValidationError::DuplicateRelationship {
                field: "input_kinds"
            })
        ));
    }

    #[test]
    fn capability_contracts_round_trip_through_direct_serde() {
        let capability = CapabilityDefinition::new_with_contract(
            CapabilityId::new("quality.test-coverage-analysis").unwrap(),
            CapabilityClass::Inspect,
            "quality",
            "Analyze test coverage evidence",
            ["repository.snapshot"],
            ["quality.coverage-report"],
            ["repository.available"],
            ["read-only"],
            ["quality", "coverage"],
        )
        .unwrap();
        let json = serde_json::to_string(&capability).unwrap();
        assert_eq!(
            serde_json::from_str::<CapabilityDefinition>(&json).unwrap(),
            capability
        );
        assert!(
            serde_json::from_str::<CapabilityDefinition>(
                &json.replace("\"domain\"", "\"unexpected\"")
            )
            .is_err()
        );
    }
}
