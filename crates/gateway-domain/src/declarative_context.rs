//! CG-06 declarative context and situation foundations.
//!
//! This module owns the stable, provider-independent value objects and
//! aggregate boundaries for the declarative layer. Intent, observation,
//! normalization, assessment and serialization semantics are added by later
//! CG-06 slices; they must build on these identities rather than redefining
//! them in application or adapter crates.

use std::{fmt, str::FromStr};

use crate::{
    identifiers::{DeclarativeContextId, ObservedStateId, SituationId},
    normalization::{NormalizationDiagnostic, NormalizedStateEntry},
    situation::{Assessment, Risk, SituationDiagnostic, SituationReference},
    validation::ValidationError,
    version::SchemaVersion,
};

/// The currently supported Declarative Context / Situation IR version.
pub const DECLARATIVE_CONTEXT_IR_VERSION: DeclarativeContextVersion = DeclarativeContextVersion::V1;

/// A version of the declarative context and situation contract.
///
/// The value object can represent a syntactically valid future version so
/// callers can report it precisely. Aggregate constructors accept only the
/// supported v1 contract and fail closed for every other version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct DeclarativeContextVersion(SchemaVersion);

impl DeclarativeContextVersion {
    /// The first supported declarative context / situation contract.
    pub const V1: Self = Self(SchemaVersion::V1);

    /// Creates a syntactically valid declarative contract version.
    pub fn new(major: u16, minor: u16) -> Result<Self, ValidationError> {
        match SchemaVersion::new(major, minor) {
            Ok(version) => Ok(Self(version)),
            Err(error) => Err(error),
        }
    }

    /// Returns the major version component.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.0.major()
    }

    /// Returns the minor version component.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.0.minor()
    }

    /// Rejects versions that are not supported by this IR implementation.
    pub fn ensure_supported(self) -> Result<(), ValidationError> {
        if self == Self::V1 {
            Ok(())
        } else {
            Err(ValidationError::UnsupportedSchemaVersion {
                expected: "1.0",
                actual: self.to_string(),
            })
        }
    }
}

impl fmt::Display for DeclarativeContextVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for DeclarativeContextVersion {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        SchemaVersion::from_str(value).map(Self)
    }
}

impl TryFrom<SchemaVersion> for DeclarativeContextVersion {
    type Error = ValidationError;

    fn try_from(value: SchemaVersion) -> Result<Self, Self::Error> {
        Ok(Self(value))
    }
}

/// The root declarative input aggregate.
///
/// It deliberately contains only the version and identity at this stage.
/// Later CG-06 slices add intent and external-context members without moving
/// ownership into `gateway-context`, which remains reserved for CG-10
/// compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarativeContext {
    version: DeclarativeContextVersion,
    id: DeclarativeContextId,
}

impl DeclarativeContext {
    /// Creates a v1 declarative context.
    #[must_use]
    pub fn new_v1(id: DeclarativeContextId) -> Self {
        Self {
            version: DeclarativeContextVersion::V1,
            id,
        }
    }

    /// Creates a declarative context after validating the IR version.
    pub fn new(
        version: DeclarativeContextVersion,
        id: DeclarativeContextId,
    ) -> Result<Self, ValidationError> {
        version.ensure_supported()?;
        Ok(Self { version, id })
    }

    /// Returns the validated IR version.
    #[must_use]
    pub const fn version(&self) -> DeclarativeContextVersion {
        self.version
    }

    /// Returns the context identity.
    #[must_use]
    pub fn id(&self) -> &DeclarativeContextId {
        &self.id
    }
}

/// The normalized observed-state aggregate boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedState {
    version: DeclarativeContextVersion,
    id: ObservedStateId,
    entries: Vec<NormalizedStateEntry>,
    diagnostics: Vec<NormalizationDiagnostic>,
}

impl ObservedState {
    /// Creates a v1 observed-state snapshot.
    #[must_use]
    pub fn new_v1(id: ObservedStateId) -> Self {
        Self {
            version: DeclarativeContextVersion::V1,
            id,
            entries: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Creates a v1 snapshot with deterministic normalized state entries.
    #[must_use]
    pub fn new_v1_with_entries(
        id: ObservedStateId,
        entries: Vec<NormalizedStateEntry>,
        diagnostics: Vec<NormalizationDiagnostic>,
    ) -> Self {
        Self {
            version: DeclarativeContextVersion::V1,
            id,
            entries,
            diagnostics,
        }
    }

    /// Creates an observed-state snapshot after validating the IR version.
    pub fn new(
        version: DeclarativeContextVersion,
        id: ObservedStateId,
    ) -> Result<Self, ValidationError> {
        version.ensure_supported()?;
        Ok(Self {
            version,
            id,
            entries: Vec::new(),
            diagnostics: Vec::new(),
        })
    }

    /// Returns the validated IR version.
    #[must_use]
    pub const fn version(&self) -> DeclarativeContextVersion {
        self.version
    }

    /// Returns the observed-state identity.
    #[must_use]
    pub fn id(&self) -> &ObservedStateId {
        &self.id
    }

    /// Returns normalized state entries in canonical subject order.
    #[must_use]
    pub fn entries(&self) -> &[NormalizedStateEntry] {
        &self.entries
    }

    /// Returns normalization diagnostics in deterministic input order.
    #[must_use]
    pub fn diagnostics(&self) -> &[NormalizationDiagnostic] {
        &self.diagnostics
    }
}

/// Alias used by later APIs that call the normalized snapshot `CurrentState`.
pub type CurrentState = ObservedState;

/// The complete operational situation aggregate boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Situation {
    version: DeclarativeContextVersion,
    id: SituationId,
    observed_state_id: Option<ObservedStateId>,
    assessments: Vec<Assessment>,
    risks: Vec<Risk>,
    diagnostics: Vec<SituationDiagnostic>,
    references: Vec<SituationReference>,
}

impl Situation {
    /// Creates a v1 situation snapshot.
    #[must_use]
    pub fn new_v1(id: SituationId) -> Self {
        Self {
            version: DeclarativeContextVersion::V1,
            id,
            observed_state_id: None,
            assessments: Vec::new(),
            risks: Vec::new(),
            diagnostics: Vec::new(),
            references: Vec::new(),
        }
    }

    /// Creates a situation snapshot after validating the IR version.
    pub fn new(
        version: DeclarativeContextVersion,
        id: SituationId,
    ) -> Result<Self, ValidationError> {
        version.ensure_supported()?;
        Ok(Self {
            version,
            id,
            observed_state_id: None,
            assessments: Vec::new(),
            risks: Vec::new(),
            diagnostics: Vec::new(),
            references: Vec::new(),
        })
    }

    pub(crate) fn from_parts(
        version: DeclarativeContextVersion,
        id: SituationId,
        observed_state_id: ObservedStateId,
        assessments: Vec<Assessment>,
        risks: Vec<Risk>,
        diagnostics: Vec<SituationDiagnostic>,
        references: Vec<SituationReference>,
    ) -> Self {
        Self {
            version,
            id,
            observed_state_id: Some(observed_state_id),
            assessments,
            risks,
            diagnostics,
            references,
        }
    }

    /// Returns the validated IR version.
    #[must_use]
    pub const fn version(&self) -> DeclarativeContextVersion {
        self.version
    }

    /// Returns the situation identity.
    #[must_use]
    pub fn id(&self) -> &SituationId {
        &self.id
    }

    /// Returns the normalized observed-state snapshot identity, if assembled.
    #[must_use]
    pub fn observed_state_id(&self) -> Option<&ObservedStateId> {
        self.observed_state_id.as_ref()
    }

    /// Returns derived assessments in canonical identity order.
    #[must_use]
    pub fn assessments(&self) -> &[Assessment] {
        &self.assessments
    }

    /// Returns derived risks in canonical identity order.
    #[must_use]
    pub fn risks(&self) -> &[Risk] {
        &self.risks
    }

    /// Returns unresolved conflicts, questions and data-quality diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[SituationDiagnostic] {
        &self.diagnostics
    }

    /// Returns external/runtime state references in canonical order.
    #[must_use]
    pub fn references(&self) -> &[SituationReference] {
        &self.references
    }

    /// Projects deterministic human-readable explanations from the stored
    /// assessments and risks without introducing new semantics.
    pub fn explainability(&self) -> Vec<crate::situation::ExplainabilityTrace> {
        self.assessments
            .iter()
            .map(crate::situation::ExplainabilityTrace::for_assessment)
            .chain(
                self.risks
                    .iter()
                    .map(crate::situation::ExplainabilityTrace::for_risk),
            )
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn version_is_explicit_and_fail_closed() {
        assert_eq!(DeclarativeContextVersion::V1.to_string(), "1.0");
        assert_eq!(
            DeclarativeContextVersion::from_str("1.0").unwrap(),
            DeclarativeContextVersion::V1
        );
        assert_eq!(
            DeclarativeContextVersion::from_str("2.0").unwrap().major(),
            2
        );
        assert!(DeclarativeContextVersion::from_str("v1").is_err());
        assert!(DeclarativeContextVersion::new(0, 1).is_err());
        assert!(
            DeclarativeContextVersion::from_str("2.0")
                .unwrap()
                .ensure_supported()
                .is_err()
        );
    }

    #[test]
    fn aggregate_construction_validates_versions_and_preserves_identity() {
        let context_id = DeclarativeContextId::new("context-1").unwrap();
        let context = DeclarativeContext::new_v1(context_id.clone());
        assert_eq!(context.version(), DeclarativeContextVersion::V1);
        assert_eq!(context.id(), &context_id);

        let observed_id = ObservedStateId::new("state-1").unwrap();
        let observed = ObservedState::new_v1(observed_id.clone());
        assert_eq!(observed.id(), &observed_id);

        let situation_id = SituationId::new("situation-1").unwrap();
        let situation = Situation::new_v1(situation_id.clone());
        assert_eq!(situation.id(), &situation_id);

        let unsupported = DeclarativeContextVersion::new(2, 0).unwrap();
        assert!(matches!(
            DeclarativeContext::new(unsupported, context_id),
            Err(ValidationError::UnsupportedSchemaVersion { .. })
        ));
    }
}
