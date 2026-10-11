//! EPIC-05.04 interpretation contracts. These records are not executable IR.
//! Source admission, pinned snapshot integrity and precedence are application
//! responsibilities specified by Interpretation Context 1.0.

use serde::{Deserialize, Serialize};

use crate::{Provenance, ReferenceId, SemanticTarget, SubjectPath, ValidationError};

/// Stable categories and diagnostic codes. Codes are never reused for another meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SemanticGapReason {
    MissingRequiredField,
    UnknownEntity,
    AmbiguousEntity,
    AmbiguousReference,
    ConflictingContext,
    StaleReference,
    UnsupportedIntent,
    InsufficientConfidence,
    InvalidReference,
}

impl SemanticGapReason {
    pub const fn diagnostic_code(self) -> &'static str {
        match self {
            Self::MissingRequiredField => "SEM_MISSING_REQUIRED_FIELD",
            Self::UnknownEntity => "SEM_UNKNOWN_ENTITY",
            Self::AmbiguousEntity => "SEM_AMBIGUOUS_ENTITY",
            Self::AmbiguousReference => "SEM_AMBIGUOUS_REFERENCE",
            Self::ConflictingContext => "SEM_CONFLICTING_CONTEXT",
            Self::StaleReference => "SEM_STALE_REFERENCE",
            Self::UnsupportedIntent => "SEM_UNSUPPORTED_INTENT",
            Self::InsufficientConfidence => "SEM_INSUFFICIENT_CONFIDENCE",
            Self::InvalidReference => "SEM_INVALID_REFERENCE",
        }
    }

    pub const fn resolution_state(self) -> SemanticResolutionState {
        match self {
            Self::AmbiguousEntity | Self::AmbiguousReference => SemanticResolutionState::Ambiguous,
            Self::ConflictingContext => SemanticResolutionState::Conflicting,
            Self::UnsupportedIntent | Self::InvalidReference => SemanticResolutionState::Invalid,
            Self::MissingRequiredField
            | Self::UnknownEntity
            | Self::StaleReference
            | Self::InsufficientConfidence => SemanticResolutionState::Unresolved,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SemanticResolutionState {
    Resolved,
    Unresolved,
    Ambiguous,
    Conflicting,
    Invalid,
}

fn invalid(reason: &'static str) -> ValidationError {
    ValidationError::InvalidStateCombination { reason }
}

fn lineage(mut values: Vec<Provenance>) -> Result<Vec<Provenance>, ValidationError> {
    if values.is_empty() {
        return Err(invalid("semantic resolution requires source provenance"));
    }
    values.sort_by(|a, b| a.id().cmp(b.id()));
    if values.windows(2).any(|pair| pair[0].id() == pair[1].id()) {
        return Err(invalid("duplicate semantic provenance identity"));
    }
    Ok(values)
}

/// A typed possible meaning with all supporting lineage, never a confidence winner.
/// T owns its value invariants (e.g. SemanticTarget or a validated CG-06 literal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionCandidate<T> {
    value: T,
    provenance: Vec<Provenance>,
}

impl<T> ResolutionCandidate<T> {
    pub fn new(value: T, provenance: Vec<Provenance>) -> Result<Self, ValidationError> {
        Ok(Self {
            value,
            provenance: lineage(provenance)?,
        })
    }

    pub fn value(&self) -> &T {
        &self.value
    }
    pub fn provenance(&self) -> &[Provenance] {
        &self.provenance
    }
}

/// Two or more distinct exact meanings. Input order carries no preference.
/// Equivalent assertions must be grouped with combined lineage before construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionCandidates<T>(Vec<ResolutionCandidate<T>>);

impl<T: PartialEq> ResolutionCandidates<T> {
    pub fn new(values: Vec<ResolutionCandidate<T>>) -> Result<Self, ValidationError> {
        if values.len() < 2 {
            return Err(invalid(
                "ambiguity or conflict requires at least two meanings",
            ));
        }
        for (index, candidate) in values.iter().enumerate() {
            if values[..index]
                .iter()
                .any(|other| other.value == candidate.value)
            {
                return Err(invalid("duplicate semantic candidate meaning"));
            }
        }
        Ok(Self(values))
    }
}

impl<T> ResolutionCandidates<T> {
    pub fn candidates(&self) -> &[ResolutionCandidate<T>] {
        &self.0
    }
}

/// Invalid explicit stale pins differ from excluded stale implicit context:
/// use InvalidReference for the former and StaleReference for the latter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionFailure {
    reason: SemanticGapReason,
    provenance: Vec<Provenance>,
}

impl ResolutionFailure {
    pub fn new(
        reason: SemanticGapReason,
        provenance: Vec<Provenance>,
    ) -> Result<Self, ValidationError> {
        if matches!(
            reason.resolution_state(),
            SemanticResolutionState::Ambiguous | SemanticResolutionState::Conflicting
        ) {
            return Err(invalid(
                "ambiguity and conflict require inspectable candidates",
            ));
        }
        Ok(Self {
            reason,
            provenance: lineage(provenance)?,
        })
    }

    pub const fn reason(&self) -> SemanticGapReason {
        self.reason
    }
    pub fn provenance(&self) -> &[Provenance] {
        &self.provenance
    }
}

/// Ambiguity is multiple possible referents; conflict is incompatible assertions.
/// No conversion to SemanticTaskIR or default-value operation is provided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticResolution<T> {
    Resolved(ResolutionCandidate<T>),
    Unresolved(ResolutionFailure),
    Ambiguous(ResolutionCandidates<T>),
    Conflicting(ResolutionCandidates<T>),
    Invalid(ResolutionFailure),
}

impl<T> SemanticResolution<T> {
    /// Validates failure/state agreement, including directly constructed enum values.
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Unresolved(failure)
                if failure.reason.resolution_state() != SemanticResolutionState::Unresolved =>
            {
                Err(invalid("unresolved reason does not match resolution state"))
            }
            Self::Invalid(failure)
                if failure.reason.resolution_state() != SemanticResolutionState::Invalid =>
            {
                Err(invalid("invalid reason does not match resolution state"))
            }
            _ => Ok(()),
        }
    }

    pub const fn state(&self) -> SemanticResolutionState {
        match self {
            Self::Resolved(_) => SemanticResolutionState::Resolved,
            Self::Unresolved(_) => SemanticResolutionState::Unresolved,
            Self::Ambiguous(_) => SemanticResolutionState::Ambiguous,
            Self::Conflicting(_) => SemanticResolutionState::Conflicting,
            Self::Invalid(_) => SemanticResolutionState::Invalid,
        }
    }

    /// Extract mandatory meaning only from RESOLVED. Even a single proposed value
    /// with insufficient confidence remains a failure until deterministically validated.
    pub fn require_resolved(&self) -> Result<&T, &'static str> {
        if self.validate().is_err() {
            return Err("SEM_INVALID_RESOLUTION_CONTRACT");
        }
        match self {
            Self::Resolved(candidate) => Ok(candidate.value()),
            Self::Unresolved(failure) | Self::Invalid(failure) => {
                Err(failure.reason.diagnostic_code())
            }
            Self::Ambiguous(_) => Err("SEM_AMBIGUOUS_REFERENCE"),
            Self::Conflicting(_) => Err("SEM_CONFLICTING_CONTEXT"),
        }
    }
}

/// An unresolved target is attached to its request, never a fabricated target ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnowledgeGapSubject {
    Target(SemanticTarget),
    Path(SubjectPath),
    Request(ReferenceId),
}

/// Required meaning includes any declared input/state/evidence dependency, even
/// when its containing IR member is structurally optional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SemanticFieldRequirement {
    Mandatory,
    Advisory,
}

/// A requested resolution route, never an acquisition or consent grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum KnowledgeResolutionClass {
    CurrentState,
    OrganisationalMemory,
    PermittedRetrieval,
    HumanClarification,
    InputRepair,
    Unsupported,
}

/// Immutable missing knowledge attached to a typed field and original sources.
/// Advisory gaps remain in interpretation diagnostics, outside executable IR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeGap {
    subject: KnowledgeGapSubject,
    missing_field: SubjectPath,
    reason: SemanticGapReason,
    provenance: Vec<Provenance>,
    required_resolution_class: KnowledgeResolutionClass,
    requirement: SemanticFieldRequirement,
}

impl KnowledgeGap {
    pub fn new(
        subject: KnowledgeGapSubject,
        missing_field: SubjectPath,
        reason: SemanticGapReason,
        provenance: Vec<Provenance>,
        required_resolution_class: KnowledgeResolutionClass,
        requirement: SemanticFieldRequirement,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            subject,
            missing_field,
            reason,
            provenance: lineage(provenance)?,
            required_resolution_class,
            requirement,
        })
    }

    pub fn subject(&self) -> &KnowledgeGapSubject {
        &self.subject
    }
    pub fn missing_field(&self) -> &SubjectPath {
        &self.missing_field
    }
    pub const fn reason(&self) -> SemanticGapReason {
        self.reason
    }
    pub fn provenance(&self) -> &[Provenance] {
        &self.provenance
    }
    pub const fn required_resolution_class(&self) -> KnowledgeResolutionClass {
        self.required_resolution_class
    }
    pub const fn requirement(&self) -> SemanticFieldRequirement {
        self.requirement
    }
    pub const fn diagnostic_code(&self) -> &'static str {
        self.reason.diagnostic_code()
    }
    pub fn blocks_compilation(&self) -> bool {
        self.requirement == SemanticFieldRequirement::Mandatory
    }
}
