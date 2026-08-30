//! CG-07.01 versioned Delta and declarative Plan contracts.
//!
//! This module defines the provider-independent planning IR.  It deliberately
//! contains outcomes, references and graph metadata only.  Concrete
//! ProcessDefinitions, Agents and Skills are resolved by CG-08; policy and
//! process lifecycle authority remain owned by CG-09 and CG-04 respectively.

use std::{collections::BTreeSet, fmt, str::FromStr};

use crate::{
    identifiers::{
        AssessmentId, CapabilityConstraint, CapabilityId, CapabilityPrecondition,
        CapabilityRequirementId, ConditionId, CurrentStateId, DeltaId, DeltaItemId, DesiredStateId,
        EvidenceId, FactId, ObservationId, PlanId, PlanStepId, ProvenanceId, SituationId,
    },
    intent::{DesiredState, SubjectPath, TypedValue},
    validation::{NonEmptyText, ValidationError},
    version::SchemaVersion,
};

/// The currently supported Delta/Plan IR version.
pub const DECLARATIVE_PLANNING_IR_VERSION: PlanningIrVersion = PlanningIrVersion::V1;

/// A version of the provider-independent Delta and declarative Plan contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PlanningIrVersion(SchemaVersion);

impl PlanningIrVersion {
    /// The first supported planning IR version.
    pub const V1: Self = Self(SchemaVersion::V1);

    /// Creates a syntactically valid planning IR version.
    pub fn new(major: u16, minor: u16) -> Result<Self, ValidationError> {
        SchemaVersion::new(major, minor).map(Self)
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

    /// Rejects versions that this implementation does not understand.
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

impl fmt::Display for PlanningIrVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for PlanningIrVersion {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        SchemaVersion::from_str(value).map(Self)
    }
}

/// The semantic classification of one desired-vs-current item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum DeltaKind {
    /// The desired condition is already true and needs no action.
    Satisfied,
    /// The desired condition is definitely not true.
    UnsatisfiedCondition,
    /// The current state explicitly violates a desired restriction.
    Violation,
    /// A required state or change is absent.
    MissingState,
    /// The condition cannot be decided because required evidence is absent.
    MissingEvidence,
    /// The subject has not been observed sufficiently to decide the condition.
    UnknownState,
    /// Current assertions conflict and require explicit resolution.
    Conflict,
    /// An explicit caller input or answer is needed before planning can proceed.
    UnresolvedInput,
    /// The condition/value combination is not supported or comparable.
    UnsupportedComparison,
}

impl DeltaKind {
    /// Returns the stable machine-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Satisfied => "SATISFIED",
            Self::UnsatisfiedCondition => "UNSATISFIED_CONDITION",
            Self::Violation => "VIOLATION",
            Self::MissingState => "MISSING_STATE",
            Self::MissingEvidence => "MISSING_EVIDENCE",
            Self::UnknownState => "UNKNOWN_STATE",
            Self::Conflict => "CONFLICT",
            Self::UnresolvedInput => "UNRESOLVED_INPUT",
            Self::UnsupportedComparison => "UNSUPPORTED_COMPARISON",
        }
    }
}

impl fmt::Display for DeltaKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for DeltaKind {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "SATISFIED" => Ok(Self::Satisfied),
            "UNSATISFIED_CONDITION" => Ok(Self::UnsatisfiedCondition),
            "VIOLATION" => Ok(Self::Violation),
            "MISSING_STATE" => Ok(Self::MissingState),
            "MISSING_EVIDENCE" => Ok(Self::MissingEvidence),
            "UNKNOWN_STATE" => Ok(Self::UnknownState),
            "CONFLICT" => Ok(Self::Conflict),
            "UNRESOLVED_INPUT" => Ok(Self::UnresolvedInput),
            "UNSUPPORTED_COMPARISON" => Ok(Self::UnsupportedComparison),
            value => Err(ValidationError::UnknownDomainValue {
                field: "delta_kind",
                value: value.to_owned(),
            }),
        }
    }
}

/// The stable machine-readable reason attached to one Delta item.
///
/// `DeltaKind` classifies the gap at the planning boundary.  This companion
/// code retains the semantic reason emitted by comparison so diagnostics and
/// machine consumers are projections of the same result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum DeltaReasonCode {
    ConditionSatisfied,
    ValueMismatch,
    ExplicitViolation,
    SubjectNotObserved,
    StateUnknown,
    StateConflict,
    MissingEvidence,
    StaleEvidence,
    FreshnessUnknown,
    IncompleteInformation,
    IncompatibleTypes,
    UnsupportedOperation,
    NegatedAssertionNotComparable,
    MissingState,
    UnresolvedInput,
}

impl DeltaReasonCode {
    /// Returns the stable machine-readable reason name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConditionSatisfied => "CONDITION_SATISFIED",
            Self::ValueMismatch => "VALUE_MISMATCH",
            Self::ExplicitViolation => "EXPLICIT_VIOLATION",
            Self::SubjectNotObserved => "SUBJECT_NOT_OBSERVED",
            Self::StateUnknown => "STATE_UNKNOWN",
            Self::StateConflict => "STATE_CONFLICT",
            Self::MissingEvidence => "MISSING_EVIDENCE",
            Self::StaleEvidence => "STALE_EVIDENCE",
            Self::FreshnessUnknown => "FRESHNESS_UNKNOWN",
            Self::IncompleteInformation => "INCOMPLETE_INFORMATION",
            Self::IncompatibleTypes => "INCOMPATIBLE_TYPES",
            Self::UnsupportedOperation => "UNSUPPORTED_OPERATION",
            Self::NegatedAssertionNotComparable => "NEGATED_ASSERTION_NOT_COMPARABLE",
            Self::MissingState => "MISSING_STATE",
            Self::UnresolvedInput => "UNRESOLVED_INPUT",
        }
    }
}

impl fmt::Display for DeltaReasonCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for DeltaReasonCode {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "CONDITION_SATISFIED" => Ok(Self::ConditionSatisfied),
            "VALUE_MISMATCH" => Ok(Self::ValueMismatch),
            "EXPLICIT_VIOLATION" => Ok(Self::ExplicitViolation),
            "SUBJECT_NOT_OBSERVED" => Ok(Self::SubjectNotObserved),
            "STATE_UNKNOWN" => Ok(Self::StateUnknown),
            "STATE_CONFLICT" => Ok(Self::StateConflict),
            "MISSING_EVIDENCE" => Ok(Self::MissingEvidence),
            "STALE_EVIDENCE" => Ok(Self::StaleEvidence),
            "FRESHNESS_UNKNOWN" => Ok(Self::FreshnessUnknown),
            "INCOMPLETE_INFORMATION" => Ok(Self::IncompleteInformation),
            "INCOMPATIBLE_TYPES" => Ok(Self::IncompatibleTypes),
            "UNSUPPORTED_OPERATION" => Ok(Self::UnsupportedOperation),
            "NEGATED_ASSERTION_NOT_COMPARABLE" => Ok(Self::NegatedAssertionNotComparable),
            "MISSING_STATE" => Ok(Self::MissingState),
            "UNRESOLVED_INPUT" => Ok(Self::UnresolvedInput),
            value => Err(ValidationError::UnknownDomainValue {
                field: "delta_reason_code",
                value: value.to_owned(),
            }),
        }
    }
}

impl DeltaReasonCode {
    const fn from_kind(kind: DeltaKind) -> Self {
        match kind {
            DeltaKind::Satisfied => Self::ConditionSatisfied,
            DeltaKind::UnsatisfiedCondition => Self::ValueMismatch,
            DeltaKind::Violation => Self::ExplicitViolation,
            DeltaKind::MissingState => Self::MissingState,
            DeltaKind::MissingEvidence => Self::MissingEvidence,
            DeltaKind::UnknownState => Self::StateUnknown,
            DeltaKind::Conflict => Self::StateConflict,
            DeltaKind::UnresolvedInput => Self::UnresolvedInput,
            DeltaKind::UnsupportedComparison => Self::UnsupportedOperation,
        }
    }
}

/// The generic outcome a Delta item or PlanStep requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum RequiredOutcomeKind {
    /// A domain or project state must be changed.
    DomainChange,
    /// Sufficient evidence must be acquired.
    EvidenceAcquisition,
    /// An unknown subject must be observed or reassessed.
    Observation,
    /// A caller must provide an explicit answer or input.
    InputAcquisition,
    /// Conflicting assertions must be resolved by explicit evidence or input.
    ConflictResolution,
    /// A derived condition must be assessed or verified.
    Assessment,
    /// No work is required because the goal is already satisfied.
    NoOp,
}

impl RequiredOutcomeKind {
    /// Returns the stable machine-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DomainChange => "DOMAIN_CHANGE",
            Self::EvidenceAcquisition => "EVIDENCE_ACQUISITION",
            Self::Observation => "OBSERVATION",
            Self::InputAcquisition => "INPUT_ACQUISITION",
            Self::ConflictResolution => "CONFLICT_RESOLUTION",
            Self::Assessment => "ASSESSMENT",
            Self::NoOp => "NO_OP",
        }
    }
}

impl fmt::Display for RequiredOutcomeKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RequiredOutcomeKind {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "DOMAIN_CHANGE" => Ok(Self::DomainChange),
            "EVIDENCE_ACQUISITION" => Ok(Self::EvidenceAcquisition),
            "OBSERVATION" => Ok(Self::Observation),
            "INPUT_ACQUISITION" => Ok(Self::InputAcquisition),
            "CONFLICT_RESOLUTION" => Ok(Self::ConflictResolution),
            "ASSESSMENT" => Ok(Self::Assessment),
            "NO_OP" => Ok(Self::NoOp),
            value => Err(ValidationError::UnknownDomainValue {
                field: "required_outcome_kind",
                value: value.to_owned(),
            }),
        }
    }
}

/// A typed, provider-independent outcome description.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct RequiredOutcome {
    kind: RequiredOutcomeKind,
    description: NonEmptyText,
    subject: Option<SubjectPath>,
    expected: Option<TypedValue>,
}

impl RequiredOutcome {
    /// Creates an outcome without assuming how an adapter will achieve it.
    pub fn new(
        kind: RequiredOutcomeKind,
        description: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            kind,
            description: NonEmptyText::new_for_field(description, "required_outcome")?,
            subject: None,
            expected: None,
        })
    }

    /// Attaches the explicitly typed subject affected by the outcome.
    #[must_use]
    pub fn with_subject(mut self, subject: SubjectPath) -> Self {
        self.subject = Some(subject);
        self
    }

    /// Attaches an explicitly typed expected value.
    pub fn with_expected(mut self, expected: TypedValue) -> Result<Self, ValidationError> {
        expected.validate()?;
        self.expected = Some(expected);
        Ok(self)
    }

    /// Returns the outcome kind.
    #[must_use]
    pub const fn kind(&self) -> RequiredOutcomeKind {
        self.kind
    }

    /// Returns the human-readable outcome description.
    #[must_use]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }

    /// Returns the optional affected subject.
    #[must_use]
    pub fn subject(&self) -> Option<&SubjectPath> {
        self.subject.as_ref()
    }

    /// Returns the optional expected value.
    #[must_use]
    pub fn expected(&self) -> Option<&TypedValue> {
        self.expected.as_ref()
    }
}

/// Explicit current-state and evidence lineage used to explain a Delta item.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct DeltaBasis {
    situation: Option<SituationId>,
    current_state: Option<CurrentStateId>,
    state_subjects: Vec<SubjectPath>,
    facts: Vec<FactId>,
    observations: Vec<ObservationId>,
    evidence: Vec<EvidenceId>,
    provenances: Vec<ProvenanceId>,
    assessments: Vec<AssessmentId>,
}

impl DeltaBasis {
    /// Creates a lineage basis and canonicalizes all reference collections.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        situation: Option<SituationId>,
        current_state: Option<CurrentStateId>,
        mut state_subjects: Vec<SubjectPath>,
        mut facts: Vec<FactId>,
        mut observations: Vec<ObservationId>,
        mut evidence: Vec<EvidenceId>,
        mut provenances: Vec<ProvenanceId>,
        mut assessments: Vec<AssessmentId>,
    ) -> Result<Self, ValidationError> {
        sort_unique(&mut state_subjects, "delta_basis.state_subjects")?;
        sort_unique(&mut facts, "delta_basis.facts")?;
        sort_unique(&mut observations, "delta_basis.observations")?;
        sort_unique(&mut evidence, "delta_basis.evidence")?;
        sort_unique(&mut provenances, "delta_basis.provenances")?;
        sort_unique(&mut assessments, "delta_basis.assessments")?;
        Ok(Self {
            situation,
            current_state,
            state_subjects,
            facts,
            observations,
            evidence,
            provenances,
            assessments,
        })
    }

    /// Creates an empty basis for a Delta item whose references are supplied later.
    pub fn empty() -> Self {
        Self {
            situation: None,
            current_state: None,
            state_subjects: Vec::new(),
            facts: Vec::new(),
            observations: Vec::new(),
            evidence: Vec::new(),
            provenances: Vec::new(),
            assessments: Vec::new(),
        }
    }

    /// Returns the optional Situation identity.
    #[must_use]
    pub fn situation(&self) -> Option<&SituationId> {
        self.situation.as_ref()
    }

    /// Returns the optional CurrentState identity.
    #[must_use]
    pub fn current_state(&self) -> Option<&CurrentStateId> {
        self.current_state.as_ref()
    }

    /// Returns affected state subjects in canonical order.
    #[must_use]
    pub fn state_subjects(&self) -> &[SubjectPath] {
        &self.state_subjects
    }

    /// Returns fact references in canonical order.
    #[must_use]
    pub fn facts(&self) -> &[FactId] {
        &self.facts
    }

    /// Returns observation references in canonical order.
    #[must_use]
    pub fn observations(&self) -> &[ObservationId] {
        &self.observations
    }

    /// Returns evidence references in canonical order.
    #[must_use]
    pub fn evidence(&self) -> &[EvidenceId] {
        &self.evidence
    }

    /// Returns provenance references in canonical order.
    #[must_use]
    pub fn provenances(&self) -> &[ProvenanceId] {
        &self.provenances
    }

    /// Returns assessment references in canonical order.
    #[must_use]
    pub fn assessments(&self) -> &[AssessmentId] {
        &self.assessments
    }
}

/// One explicit semantic gap between a DesiredState and a Situation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct DeltaItem {
    id: DeltaItemId,
    desired_state: DesiredStateId,
    condition: ConditionId,
    kind: DeltaKind,
    reason: DeltaReasonCode,
    basis: DeltaBasis,
    required_outcome: RequiredOutcome,
    rationale: NonEmptyText,
}

impl DeltaItem {
    /// Creates one traceable Delta item.
    pub fn new(
        id: DeltaItemId,
        desired_state: DesiredStateId,
        condition: ConditionId,
        kind: DeltaKind,
        basis: DeltaBasis,
        required_outcome: RequiredOutcome,
        rationale: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        Self::new_with_reason(
            id,
            desired_state,
            condition,
            kind,
            DeltaReasonCode::from_kind(kind),
            basis,
            required_outcome,
            rationale,
        )
    }

    /// Creates one traceable Delta item with an explicit semantic reason.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_reason(
        id: DeltaItemId,
        desired_state: DesiredStateId,
        condition: ConditionId,
        kind: DeltaKind,
        reason: DeltaReasonCode,
        basis: DeltaBasis,
        required_outcome: RequiredOutcome,
        rationale: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            id,
            desired_state,
            condition,
            kind,
            reason,
            basis,
            required_outcome,
            rationale: NonEmptyText::new_for_field(rationale, "delta_item.rationale")?,
        })
    }

    /// Returns the stable Delta item identity.
    #[must_use]
    pub fn id(&self) -> &DeltaItemId {
        &self.id
    }

    /// Returns the owning DesiredState identity.
    #[must_use]
    pub fn desired_state(&self) -> &DesiredStateId {
        &self.desired_state
    }

    /// Returns the originating DesiredCondition identity.
    #[must_use]
    pub fn condition(&self) -> &ConditionId {
        &self.condition
    }

    /// Returns the semantic Delta classification.
    #[must_use]
    pub const fn kind(&self) -> DeltaKind {
        self.kind
    }

    /// Returns the stable semantic reason for this item.
    #[must_use]
    pub const fn reason(&self) -> DeltaReasonCode {
        self.reason
    }

    /// Returns whether this item represents actionable work.
    #[must_use]
    pub const fn is_actionable(&self) -> bool {
        !matches!(self.kind, DeltaKind::Satisfied)
    }

    /// Returns current-state, Situation and evidence lineage.
    #[must_use]
    pub const fn basis(&self) -> &DeltaBasis {
        &self.basis
    }

    /// Returns the typed outcome required to close the gap.
    #[must_use]
    pub const fn required_outcome(&self) -> &RequiredOutcome {
        &self.required_outcome
    }

    /// Returns the stable rationale for the item.
    #[must_use]
    pub fn rationale(&self) -> &str {
        self.rationale.as_str()
    }
}

/// A complete, versioned comparison result prepared for later derivation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delta {
    version: PlanningIrVersion,
    id: DeltaId,
    desired_state: DesiredStateId,
    situation: Option<SituationId>,
    items: Vec<DeltaItem>,
}

impl Delta {
    /// Creates a supported v1 Delta with canonical item ordering.
    pub fn new(
        id: DeltaId,
        desired_state: DesiredStateId,
        situation: Option<SituationId>,
        items: Vec<DeltaItem>,
    ) -> Result<Self, ValidationError> {
        Self::new_with_version(
            DECLARATIVE_PLANNING_IR_VERSION,
            id,
            desired_state,
            situation,
            items,
        )
    }

    /// Creates a Delta after validating its explicit IR version and references.
    pub fn new_with_version(
        version: PlanningIrVersion,
        id: DeltaId,
        desired_state: DesiredStateId,
        situation: Option<SituationId>,
        mut items: Vec<DeltaItem>,
    ) -> Result<Self, ValidationError> {
        version.ensure_supported()?;
        items.sort_by(|left, right| left.id.cmp(&right.id));
        ensure_unique_ids(&items, "delta_item")?;
        if items
            .iter()
            .any(|item| item.desired_state() != &desired_state)
        {
            return Err(ValidationError::InvalidStateCombination {
                reason: "Delta items must reference the owning desired state",
            });
        }
        if situation.is_some()
            && items.iter().any(|item| {
                item.basis()
                    .situation()
                    .is_some_and(|basis| Some(basis) != situation.as_ref())
            })
        {
            return Err(ValidationError::InvalidStateCombination {
                reason: "Delta item Situation basis must match the owning Delta",
            });
        }
        Ok(Self {
            version,
            id,
            desired_state,
            situation,
            items,
        })
    }

    /// Returns the planning IR version.
    #[must_use]
    pub const fn version(&self) -> PlanningIrVersion {
        self.version
    }

    /// Returns the stable Delta identity.
    #[must_use]
    pub fn id(&self) -> &DeltaId {
        &self.id
    }

    /// Returns the DesiredState identity being evaluated.
    #[must_use]
    pub fn desired_state(&self) -> &DesiredStateId {
        &self.desired_state
    }

    /// Returns the optional Situation identity being compared.
    #[must_use]
    pub fn situation(&self) -> Option<&SituationId> {
        self.situation.as_ref()
    }

    /// Returns Delta items in canonical identity order.
    #[must_use]
    pub fn items(&self) -> &[DeltaItem] {
        &self.items
    }

    /// Returns whether this Delta contains no actionable gap.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.items.is_empty() || self.items.iter().all(|item| !item.is_actionable())
    }

    /// Returns only the items that require follow-up work.
    #[must_use]
    pub fn actionable_items(&self) -> Vec<&DeltaItem> {
        self.items
            .iter()
            .filter(|item| item.is_actionable())
            .collect()
    }

    /// Returns whether the Delta contains an item with the supplied identity.
    #[must_use]
    pub fn contains_item(&self, id: &DeltaItemId) -> bool {
        self.items.iter().any(|item| item.id() == id)
    }

    /// Validates condition references against the originating DesiredState.
    pub fn validate_against_desired_state(
        &self,
        desired_state: &DesiredState,
    ) -> Result<(), ValidationError> {
        self.version.ensure_supported()?;
        if self.desired_state != *desired_state.id() {
            return Err(ValidationError::InvalidStateCombination {
                reason: "Delta and DesiredState must have matching identities",
            });
        }
        for item in &self.items {
            if !desired_state
                .conditions()
                .iter()
                .any(|condition| condition.id() == item.condition())
            {
                return Err(ValidationError::MissingDeclarativeIdentity {
                    kind: "condition",
                    id: item.condition().to_string(),
                });
            }
        }
        Ok(())
    }
}

/// Cardinality semantics for one abstract capability requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum RequirementCardinality {
    /// The plan cannot be considered executable without this requirement.
    Mandatory,
    /// The requirement is useful but not necessary for the supported plan.
    Optional,
}

impl RequirementCardinality {
    /// Returns the stable machine-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mandatory => "MANDATORY",
            Self::Optional => "OPTIONAL",
        }
    }
}

impl fmt::Display for RequirementCardinality {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RequirementCardinality {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "MANDATORY" => Ok(Self::Mandatory),
            "OPTIONAL" => Ok(Self::Optional),
            value => Err(ValidationError::UnknownDomainValue {
                field: "requirement_cardinality",
                value: value.to_owned(),
            }),
        }
    }
}

/// One abstract capability need, without a concrete executor selection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct CapabilityRequirement {
    id: CapabilityRequirementId,
    capability: CapabilityId,
    cardinality: RequirementCardinality,
    originating_delta_item: DeltaItemId,
    preconditions: Vec<CapabilityPrecondition>,
    constraints: Vec<CapabilityConstraint>,
    rationale: NonEmptyText,
}

impl CapabilityRequirement {
    /// Creates a mandatory or optional abstract capability requirement.
    pub fn new(
        id: CapabilityRequirementId,
        capability: CapabilityId,
        cardinality: RequirementCardinality,
        originating_delta_item: DeltaItemId,
        rationale: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        Self::new_with_metadata(
            id,
            capability,
            cardinality,
            originating_delta_item,
            Vec::new(),
            Vec::new(),
            rationale,
        )
    }

    /// Creates a capability requirement with explicit intrinsic metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_metadata(
        id: CapabilityRequirementId,
        capability: CapabilityId,
        cardinality: RequirementCardinality,
        originating_delta_item: DeltaItemId,
        mut preconditions: Vec<CapabilityPrecondition>,
        mut constraints: Vec<CapabilityConstraint>,
        rationale: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        sort_unique(&mut preconditions, "capability_requirement.preconditions")?;
        sort_unique(&mut constraints, "capability_requirement.constraints")?;
        Ok(Self {
            id,
            capability,
            cardinality,
            originating_delta_item,
            preconditions,
            constraints,
            rationale: NonEmptyText::new_for_field(rationale, "capability_requirement.rationale")?,
        })
    }

    /// Returns the stable requirement identity.
    #[must_use]
    pub fn id(&self) -> &CapabilityRequirementId {
        &self.id
    }

    /// Returns the canonical abstract capability identity.
    #[must_use]
    pub fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    /// Alias emphasizing the reference is not a selected executor.
    #[must_use]
    pub fn capability_id(&self) -> &CapabilityId {
        self.capability()
    }

    /// Returns mandatory/optional semantics.
    #[must_use]
    pub const fn cardinality(&self) -> RequirementCardinality {
        self.cardinality
    }

    /// Returns the originating Delta item identity.
    #[must_use]
    pub fn originating_delta_item(&self) -> &DeltaItemId {
        &self.originating_delta_item
    }

    /// Alias for the originating Delta reference.
    #[must_use]
    pub fn delta_item_id(&self) -> &DeltaItemId {
        self.originating_delta_item()
    }

    /// Returns explicit capability preconditions in canonical order.
    #[must_use]
    pub fn preconditions(&self) -> &[CapabilityPrecondition] {
        &self.preconditions
    }

    /// Returns explicit intrinsic capability constraints in canonical order.
    #[must_use]
    pub fn constraints(&self) -> &[CapabilityConstraint] {
        &self.constraints
    }

    /// Returns the stable requirement rationale.
    #[must_use]
    pub fn rationale(&self) -> &str {
        self.rationale.as_str()
    }
}

/// A generic lifecycle shape or process-template hint for CG-08/CG-04.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum LifecycleRequirementKind {
    /// A change must be followed by a distinct verification outcome.
    VerificationAfterChange,
    /// Evidence must be available before a state-changing outcome.
    EvidenceBeforeChange,
    /// The outcome needs independent verification.
    IndependentVerification,
    /// The outcome needs explicit human input or review.
    HumanInput,
}

impl LifecycleRequirementKind {
    /// Returns the stable machine-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VerificationAfterChange => "VERIFICATION_AFTER_CHANGE",
            Self::EvidenceBeforeChange => "EVIDENCE_BEFORE_CHANGE",
            Self::IndependentVerification => "INDEPENDENT_VERIFICATION",
            Self::HumanInput => "HUMAN_INPUT",
        }
    }
}

impl fmt::Display for LifecycleRequirementKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for LifecycleRequirementKind {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "VERIFICATION_AFTER_CHANGE" => Ok(Self::VerificationAfterChange),
            "EVIDENCE_BEFORE_CHANGE" => Ok(Self::EvidenceBeforeChange),
            "INDEPENDENT_VERIFICATION" => Ok(Self::IndependentVerification),
            "HUMAN_INPUT" => Ok(Self::HumanInput),
            value => Err(ValidationError::UnknownDomainValue {
                field: "lifecycle_requirement_kind",
                value: value.to_owned(),
            }),
        }
    }
}

/// A declarative lifecycle requirement, never a selected ProcessDefinition.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct LifecycleRequirement {
    kind: LifecycleRequirementKind,
    description: NonEmptyText,
}

impl LifecycleRequirement {
    /// Creates a generic lifecycle/process-template hint.
    pub fn new(
        kind: LifecycleRequirementKind,
        description: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            kind,
            description: NonEmptyText::new_for_field(description, "lifecycle_requirement")?,
        })
    }

    /// Returns the generic lifecycle requirement kind.
    #[must_use]
    pub const fn kind(&self) -> LifecycleRequirementKind {
        self.kind
    }

    /// Returns the requirement description.
    #[must_use]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }
}

/// The condition used to decide whether a PlanStep is complete or verified.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PlanCondition {
    /// Re-evaluate an existing DesiredState condition.
    DesiredCondition(ConditionId),
    /// Evaluate an explicit typed outcome condition.
    Outcome(RequiredOutcome),
}

impl PlanCondition {
    /// References an existing DesiredState condition.
    #[must_use]
    pub fn desired_condition(id: ConditionId) -> Self {
        Self::DesiredCondition(id)
    }

    /// Creates an explicit outcome condition.
    #[must_use]
    pub fn outcome(outcome: RequiredOutcome) -> Self {
        Self::Outcome(outcome)
    }
}

/// The semantic role of a declarative PlanStep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PlanStepKind {
    /// An explicit empty/no-op step used for a satisfied plan.
    NoOp,
    /// A state-changing or remediation outcome.
    Change,
    /// A distinct verification outcome.
    Verification,
    /// An evidence or information acquisition outcome.
    EvidenceAcquisition,
    /// An explicit caller-input or clarification outcome.
    InputAcquisition,
    /// An observation or reassessment outcome.
    Observation,
    /// An explicit conflict-resolution outcome.
    ConflictResolution,
}

impl PlanStepKind {
    /// Returns the stable machine-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoOp => "NO_OP",
            Self::Change => "CHANGE",
            Self::Verification => "VERIFICATION",
            Self::EvidenceAcquisition => "EVIDENCE_ACQUISITION",
            Self::InputAcquisition => "INPUT_ACQUISITION",
            Self::Observation => "OBSERVATION",
            Self::ConflictResolution => "CONFLICT_RESOLUTION",
        }
    }
}

impl fmt::Display for PlanStepKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for PlanStepKind {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "NO_OP" => Ok(Self::NoOp),
            "CHANGE" => Ok(Self::Change),
            "VERIFICATION" => Ok(Self::Verification),
            "EVIDENCE_ACQUISITION" => Ok(Self::EvidenceAcquisition),
            "INPUT_ACQUISITION" => Ok(Self::InputAcquisition),
            "OBSERVATION" => Ok(Self::Observation),
            "CONFLICT_RESOLUTION" => Ok(Self::ConflictResolution),
            value => Err(ValidationError::UnknownDomainValue {
                field: "plan_step_kind",
                value: value.to_owned(),
            }),
        }
    }
}

/// One explicit declarative Plan graph node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PlanStep {
    id: PlanStepId,
    kind: PlanStepKind,
    outcome: RequiredOutcome,
    dependencies: Vec<PlanStepId>,
    capability_requirements: Vec<CapabilityRequirementId>,
    delta_items: Vec<DeltaItemId>,
    completion: PlanCondition,
    verification: Option<PlanCondition>,
    lifecycle_requirement: Option<LifecycleRequirement>,
    rationale: NonEmptyText,
}

impl PlanStep {
    /// Creates a PlanStep with empty optional reference collections.
    pub fn new(
        id: PlanStepId,
        kind: PlanStepKind,
        outcome: RequiredOutcome,
        completion: PlanCondition,
        rationale: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            id,
            kind,
            outcome,
            dependencies: Vec::new(),
            capability_requirements: Vec::new(),
            delta_items: Vec::new(),
            completion,
            verification: None,
            lifecycle_requirement: None,
            rationale: NonEmptyText::new_for_field(rationale, "plan_step.rationale")?,
        })
    }

    /// Adds canonical predecessor references and rejects self-dependencies.
    pub fn with_dependencies(
        mut self,
        mut dependencies: Vec<PlanStepId>,
    ) -> Result<Self, ValidationError> {
        if dependencies.iter().any(|dependency| dependency == &self.id) {
            return Err(ValidationError::SelfReference {
                field: "plan_step.dependencies",
            });
        }
        sort_unique(&mut dependencies, "plan_step.dependencies")?;
        self.dependencies = dependencies;
        Ok(self)
    }

    /// Adds canonical abstract capability requirement references.
    pub fn with_capability_requirements(
        mut self,
        mut requirements: Vec<CapabilityRequirementId>,
    ) -> Result<Self, ValidationError> {
        sort_unique(&mut requirements, "plan_step.capability_requirements")?;
        self.capability_requirements = requirements;
        Ok(self)
    }

    /// Adds canonical Delta trace references.
    pub fn with_delta_items(
        mut self,
        mut delta_items: Vec<DeltaItemId>,
    ) -> Result<Self, ValidationError> {
        sort_unique(&mut delta_items, "plan_step.delta_items")?;
        self.delta_items = delta_items;
        Ok(self)
    }

    /// Adds a separate verification condition.
    #[must_use]
    pub fn with_verification(mut self, verification: PlanCondition) -> Self {
        self.verification = Some(verification);
        self
    }

    /// Adds an optional generic lifecycle/process-template requirement.
    #[must_use]
    pub fn with_lifecycle_requirement(mut self, requirement: LifecycleRequirement) -> Self {
        self.lifecycle_requirement = Some(requirement);
        self
    }

    /// Returns the stable PlanStep identity.
    #[must_use]
    pub fn id(&self) -> &PlanStepId {
        &self.id
    }

    /// Returns the semantic step kind.
    #[must_use]
    pub const fn kind(&self) -> PlanStepKind {
        self.kind
    }

    /// Returns the outcome this step is required to achieve.
    #[must_use]
    pub const fn outcome(&self) -> &RequiredOutcome {
        &self.outcome
    }

    /// Returns predecessor references in canonical order.
    #[must_use]
    pub fn dependencies(&self) -> &[PlanStepId] {
        &self.dependencies
    }

    /// Returns abstract capability requirement references in canonical order.
    #[must_use]
    pub fn capability_requirements(&self) -> &[CapabilityRequirementId] {
        &self.capability_requirements
    }

    /// Returns Delta trace references in canonical order.
    #[must_use]
    pub fn delta_items(&self) -> &[DeltaItemId] {
        &self.delta_items
    }

    /// Returns the completion condition.
    #[must_use]
    pub const fn completion(&self) -> &PlanCondition {
        &self.completion
    }

    /// Returns the optional separate verification condition.
    #[must_use]
    pub const fn verification(&self) -> Option<&PlanCondition> {
        self.verification.as_ref()
    }

    /// Returns the optional generic lifecycle requirement.
    #[must_use]
    pub const fn lifecycle_requirement(&self) -> Option<&LifecycleRequirement> {
        self.lifecycle_requirement.as_ref()
    }

    /// Returns the stable rationale for the step.
    #[must_use]
    pub fn rationale(&self) -> &str {
        self.rationale.as_str()
    }
}

/// A validated, versioned declarative Plan handed to CG-08.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    version: PlanningIrVersion,
    id: PlanId,
    desired_state: DesiredStateId,
    delta: DeltaId,
    capability_requirements: Vec<CapabilityRequirement>,
    steps: Vec<PlanStep>,
}

impl Plan {
    /// Creates a supported v1 Plan and validates internal references.
    pub fn new(
        id: PlanId,
        desired_state: DesiredStateId,
        delta: DeltaId,
        capability_requirements: Vec<CapabilityRequirement>,
        steps: Vec<PlanStep>,
    ) -> Result<Self, ValidationError> {
        Self::new_with_version(
            DECLARATIVE_PLANNING_IR_VERSION,
            id,
            desired_state,
            delta,
            capability_requirements,
            steps,
        )
    }

    /// Creates a Plan after validating its explicit IR version.
    pub fn new_with_version(
        version: PlanningIrVersion,
        id: PlanId,
        desired_state: DesiredStateId,
        delta: DeltaId,
        mut capability_requirements: Vec<CapabilityRequirement>,
        mut steps: Vec<PlanStep>,
    ) -> Result<Self, ValidationError> {
        version.ensure_supported()?;
        capability_requirements.sort_by(|left, right| left.id.cmp(&right.id));
        steps.sort_by(|left, right| left.id.cmp(&right.id));
        ensure_unique_ids(&capability_requirements, "capability_requirement")?;
        ensure_unique_ids(&steps, "plan_step")?;
        let requirement_ids = capability_requirements
            .iter()
            .map(|requirement| requirement.id.clone())
            .collect::<BTreeSet<_>>();
        let step_ids = steps
            .iter()
            .map(|step| step.id.clone())
            .collect::<BTreeSet<_>>();
        for step in &steps {
            for dependency in step.dependencies() {
                if !step_ids.contains(dependency) {
                    return Err(ValidationError::MissingDeclarativeIdentity {
                        kind: "plan_step",
                        id: dependency.to_string(),
                    });
                }
            }
            for requirement in step.capability_requirements() {
                if !requirement_ids.contains(requirement) {
                    return Err(ValidationError::MissingDeclarativeIdentity {
                        kind: "capability_requirement",
                        id: requirement.to_string(),
                    });
                }
            }
        }
        Ok(Self {
            version,
            id,
            desired_state,
            delta,
            capability_requirements,
            steps,
        })
    }

    /// Validates references from this Plan against its originating Delta.
    pub fn validate_against_delta(&self, delta: &Delta) -> Result<(), ValidationError> {
        self.version.ensure_supported()?;
        if self.delta != *delta.id() {
            return Err(ValidationError::InvalidStateCombination {
                reason: "Plan must reference the supplied Delta identity",
            });
        }
        if self.desired_state != *delta.desired_state() {
            return Err(ValidationError::InvalidStateCombination {
                reason: "Plan and Delta must reference the same desired state",
            });
        }
        for requirement in &self.capability_requirements {
            if !delta.contains_item(requirement.originating_delta_item()) {
                return Err(ValidationError::MissingDeclarativeIdentity {
                    kind: "delta_item",
                    id: requirement.originating_delta_item().to_string(),
                });
            }
        }
        for step in &self.steps {
            for item in step.delta_items() {
                if !delta.contains_item(item) {
                    return Err(ValidationError::MissingDeclarativeIdentity {
                        kind: "delta_item",
                        id: item.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Validates DesiredState condition references used by this Plan.
    pub fn validate_against_desired_state(
        &self,
        desired_state: &DesiredState,
    ) -> Result<(), ValidationError> {
        self.version.ensure_supported()?;
        if self.desired_state != *desired_state.id() {
            return Err(ValidationError::InvalidStateCombination {
                reason: "Plan and DesiredState must have matching identities",
            });
        }
        for step in &self.steps {
            validate_plan_condition(step.completion(), desired_state)?;
            if let Some(verification) = step.verification() {
                validate_plan_condition(verification, desired_state)?;
            }
        }
        Ok(())
    }

    /// Returns the planning IR version.
    #[must_use]
    pub const fn version(&self) -> PlanningIrVersion {
        self.version
    }

    /// Returns the stable Plan identity.
    #[must_use]
    pub fn id(&self) -> &PlanId {
        &self.id
    }

    /// Returns the DesiredState identity being planned.
    #[must_use]
    pub fn desired_state(&self) -> &DesiredStateId {
        &self.desired_state
    }

    /// Returns the originating Delta identity.
    #[must_use]
    pub fn delta(&self) -> &DeltaId {
        &self.delta
    }

    /// Returns abstract capability requirements in canonical identity order.
    #[must_use]
    pub fn capability_requirements(&self) -> &[CapabilityRequirement] {
        &self.capability_requirements
    }

    /// Returns PlanSteps in canonical identity order.
    #[must_use]
    pub fn steps(&self) -> &[PlanStep] {
        &self.steps
    }

    /// Returns whether the Plan contains no work.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.steps.is_empty()
            || self
                .steps
                .iter()
                .all(|step| step.kind() == PlanStepKind::NoOp)
    }
}

fn sort_unique<T>(values: &mut [T], field: &'static str) -> Result<(), ValidationError>
where
    T: Ord,
{
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ValidationError::DuplicateRelationship { field });
    }
    Ok(())
}

fn validate_plan_condition(
    condition: &PlanCondition,
    desired_state: &DesiredState,
) -> Result<(), ValidationError> {
    let PlanCondition::DesiredCondition(condition_id) = condition else {
        return Ok(());
    };
    if desired_state
        .conditions()
        .iter()
        .any(|declared| declared.id() == condition_id)
    {
        Ok(())
    } else {
        Err(ValidationError::MissingDeclarativeIdentity {
            kind: "condition",
            id: condition_id.to_string(),
        })
    }
}

fn ensure_unique_ids<T>(values: &[T], kind: &'static str) -> Result<(), ValidationError>
where
    T: HasPlanningId,
{
    for pair in values.windows(2) {
        if pair[0].planning_id() == pair[1].planning_id() {
            return Err(ValidationError::DuplicateDeclarativeIdentity {
                kind,
                id: pair[0].planning_id().to_string(),
            });
        }
    }
    Ok(())
}

trait HasPlanningId {
    type Id: Ord + fmt::Display;

    fn planning_id(&self) -> &Self::Id;
}

impl HasPlanningId for DeltaItem {
    type Id = DeltaItemId;

    fn planning_id(&self) -> &Self::Id {
        &self.id
    }
}

impl HasPlanningId for CapabilityRequirement {
    type Id = CapabilityRequirementId;

    fn planning_id(&self) -> &Self::Id {
        &self.id
    }
}

impl HasPlanningId for PlanStep {
    type Id = PlanStepId;

    fn planning_id(&self) -> &Self::Id {
        &self.id
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{ComparisonOperator, ConditionExpression, DesiredCondition};

    use super::*;

    fn ids() -> (
        DesiredStateId,
        ConditionId,
        SituationId,
        CurrentStateId,
        DeltaId,
        DeltaItemId,
        CapabilityRequirementId,
        PlanId,
        PlanStepId,
    ) {
        (
            DesiredStateId::new("desired-1").unwrap(),
            ConditionId::new("condition-1").unwrap(),
            SituationId::new("situation-1").unwrap(),
            CurrentStateId::new("state-1").unwrap(),
            DeltaId::new("delta-1").unwrap(),
            DeltaItemId::new("delta-item-1").unwrap(),
            CapabilityRequirementId::new("requirement-1").unwrap(),
            PlanId::new("plan-1").unwrap(),
            PlanStepId::new("step-1").unwrap(),
        )
    }

    fn outcome(kind: RequiredOutcomeKind) -> RequiredOutcome {
        RequiredOutcome::new(kind, "achieve the declared outcome").unwrap()
    }

    fn basis(situation: &SituationId, state: &CurrentStateId) -> DeltaBasis {
        DeltaBasis::new(
            Some(situation.clone()),
            Some(state.clone()),
            vec![SubjectPath::from_str("coverage.percent").unwrap()],
            vec![FactId::new("fact-1").unwrap()],
            vec![ObservationId::new("observation-1").unwrap()],
            vec![EvidenceId::new("evidence-1").unwrap()],
            vec![ProvenanceId::new("provenance-1").unwrap()],
            vec![AssessmentId::new("assessment-1").unwrap()],
        )
        .unwrap()
    }

    fn item(
        desired: &DesiredStateId,
        condition: &ConditionId,
        situation: &SituationId,
        state: &CurrentStateId,
    ) -> DeltaItem {
        DeltaItem::new(
            DeltaItemId::new("delta-item-1").unwrap(),
            desired.clone(),
            condition.clone(),
            DeltaKind::UnsatisfiedCondition,
            basis(situation, state),
            outcome(RequiredOutcomeKind::DomainChange),
            "the desired condition is not satisfied",
        )
        .unwrap()
    }

    fn desired_state(desired: &DesiredStateId, condition: &ConditionId) -> DesiredState {
        DesiredState::new(
            desired.clone(),
            vec![
                DesiredCondition::new(
                    condition.clone(),
                    SubjectPath::from_str("coverage.percent").unwrap(),
                    ComparisonOperator::Equals,
                    Some(TypedValue::Integer(95)),
                )
                .unwrap(),
            ],
            ConditionExpression::condition(condition.clone()),
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    #[test]
    fn versions_and_enum_names_are_strict_and_stable() {
        assert_eq!(PlanningIrVersion::V1.to_string(), "1.0");
        assert_eq!(
            PlanningIrVersion::from_str("1.0").unwrap(),
            PlanningIrVersion::V1
        );
        assert!(PlanningIrVersion::from_str("1").is_err());
        assert!(
            PlanningIrVersion::new(1, 1)
                .unwrap()
                .ensure_supported()
                .is_err()
        );

        for kind in [
            DeltaKind::Satisfied,
            DeltaKind::Violation,
            DeltaKind::MissingEvidence,
            DeltaKind::UnknownState,
            DeltaKind::Conflict,
            DeltaKind::UnresolvedInput,
            DeltaKind::UnsupportedComparison,
        ] {
            assert_eq!(DeltaKind::from_str(kind.as_str()).unwrap(), kind);
        }
        for kind in [
            RequiredOutcomeKind::DomainChange,
            RequiredOutcomeKind::EvidenceAcquisition,
            RequiredOutcomeKind::Observation,
            RequiredOutcomeKind::InputAcquisition,
            RequiredOutcomeKind::ConflictResolution,
            RequiredOutcomeKind::Assessment,
            RequiredOutcomeKind::NoOp,
        ] {
            assert_eq!(RequiredOutcomeKind::from_str(kind.as_str()).unwrap(), kind);
        }
        for reason in [
            DeltaReasonCode::ConditionSatisfied,
            DeltaReasonCode::ValueMismatch,
            DeltaReasonCode::ExplicitViolation,
            DeltaReasonCode::SubjectNotObserved,
            DeltaReasonCode::StateUnknown,
            DeltaReasonCode::StateConflict,
            DeltaReasonCode::MissingEvidence,
            DeltaReasonCode::StaleEvidence,
            DeltaReasonCode::FreshnessUnknown,
            DeltaReasonCode::IncompleteInformation,
            DeltaReasonCode::IncompatibleTypes,
            DeltaReasonCode::UnsupportedOperation,
            DeltaReasonCode::NegatedAssertionNotComparable,
            DeltaReasonCode::MissingState,
            DeltaReasonCode::UnresolvedInput,
        ] {
            assert_eq!(reason.to_string(), reason.as_str());
            assert_eq!(DeltaReasonCode::from_str(reason.as_str()).unwrap(), reason);
        }
        assert!(DeltaReasonCode::from_str("NOT_A_DELTA_REASON").is_err());
        assert!(matches!(
            PlanStepKind::from_str("provider-specific"),
            Err(ValidationError::UnknownDomainValue {
                field: "plan_step_kind",
                ..
            })
        ));
    }

    #[test]
    fn basis_and_outcomes_preserve_typed_references() {
        let (desired, condition, situation, state, ..) = ids();
        let value = outcome(RequiredOutcomeKind::DomainChange)
            .with_subject(SubjectPath::from_str("coverage.percent").unwrap())
            .with_expected(TypedValue::Integer(95))
            .unwrap();
        assert_eq!(value.subject().unwrap().to_string(), "coverage.percent");
        assert_eq!(value.expected(), Some(&TypedValue::Integer(95)));

        let basis = basis(&situation, &state);
        assert_eq!(basis.current_state().unwrap().as_str(), "state-1");
        assert_eq!(basis.facts()[0].as_str(), "fact-1");

        let delta_item = item(&desired, &condition, &situation, &state);
        assert_eq!(delta_item.condition(), &condition);
        assert_eq!(delta_item.reason(), DeltaReasonCode::ValueMismatch);
        assert!(delta_item.is_actionable());
        assert_eq!(delta_item.basis().evidence()[0].as_str(), "evidence-1");
    }

    #[test]
    fn delta_canonicalizes_items_and_supports_explicit_noop() {
        let (desired, condition, situation, state, delta_id, ..) = ids();
        let first = item(&desired, &condition, &situation, &state);
        let second = DeltaItem::new(
            DeltaItemId::new("delta-item-2").unwrap(),
            desired.clone(),
            condition.clone(),
            DeltaKind::Satisfied,
            basis(&situation, &state),
            outcome(RequiredOutcomeKind::NoOp),
            "the desired condition is already satisfied",
        )
        .unwrap();
        let delta = Delta::new(
            delta_id,
            desired.clone(),
            Some(situation.clone()),
            vec![second.clone(), first],
        )
        .unwrap();
        assert_eq!(delta.items()[0].id().as_str(), "delta-item-1");
        assert_eq!(delta.actionable_items().len(), 1);
        assert!(!delta.is_noop());

        let noop = Delta::new(
            DeltaId::new("delta-noop").unwrap(),
            desired,
            Some(situation),
            vec![second],
        )
        .unwrap();
        assert!(noop.is_noop());
        assert!(noop.actionable_items().is_empty());
    }

    #[test]
    fn delta_rejects_duplicate_and_mismatched_items() {
        let (desired, condition, situation, state, delta_id, ..) = ids();
        let first = item(&desired, &condition, &situation, &state);
        let duplicate = first.clone();
        assert!(matches!(
            Delta::new(
                delta_id.clone(),
                desired.clone(),
                Some(situation.clone()),
                vec![first, duplicate],
            ),
            Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "delta_item",
                ..
            })
        ));

        let other_item = DeltaItem::new(
            DeltaItemId::new("delta-item-2").unwrap(),
            DesiredStateId::new("desired-2").unwrap(),
            condition,
            DeltaKind::Violation,
            basis(&situation, &state),
            outcome(RequiredOutcomeKind::DomainChange),
            "mismatched desired state",
        )
        .unwrap();
        assert!(matches!(
            Delta::new(delta_id, desired, Some(situation), vec![other_item]),
            Err(ValidationError::InvalidStateCombination { .. })
        ));
    }

    #[test]
    fn capability_requirements_are_abstract_and_canonical() {
        let requirement = CapabilityRequirement::new_with_metadata(
            CapabilityRequirementId::new("requirement-1").unwrap(),
            CapabilityId::new("architecture.analyze").unwrap(),
            RequirementCardinality::Mandatory,
            DeltaItemId::new("delta-item-1").unwrap(),
            vec![CapabilityPrecondition::new("current-state").unwrap()],
            vec![CapabilityConstraint::new("read-only").unwrap()],
            "analyze the architecture gap",
        )
        .unwrap();
        assert_eq!(requirement.capability_id().as_str(), "architecture.analyze");
        assert_eq!(requirement.cardinality(), RequirementCardinality::Mandatory);
        assert_eq!(requirement.preconditions()[0].as_str(), "current-state");
        assert_eq!(requirement.constraints()[0].as_str(), "read-only");
    }

    #[test]
    fn plan_steps_and_plans_validate_internal_and_external_references() {
        let (
            desired,
            condition,
            situation,
            state,
            delta_id,
            delta_item_id,
            requirement_id,
            plan_id,
            step_id,
        ) = ids();
        let delta_item = item(&desired, &condition, &situation, &state);
        let delta = Delta::new(
            delta_id.clone(),
            desired.clone(),
            Some(situation),
            vec![delta_item],
        )
        .unwrap();
        let requirement = CapabilityRequirement::new(
            requirement_id.clone(),
            CapabilityId::new("architecture.analyze").unwrap(),
            RequirementCardinality::Mandatory,
            delta_item_id.clone(),
            "analyze the declared gap",
        )
        .unwrap();
        let lifecycle = LifecycleRequirement::new(
            LifecycleRequirementKind::VerificationAfterChange,
            "verification follows any change",
        )
        .unwrap();
        let step = PlanStep::new(
            step_id,
            PlanStepKind::Change,
            outcome(RequiredOutcomeKind::DomainChange),
            PlanCondition::desired_condition(condition),
            "close the declared gap",
        )
        .unwrap()
        .with_capability_requirements(vec![requirement_id])
        .unwrap()
        .with_delta_items(vec![delta_item_id])
        .unwrap()
        .with_verification(PlanCondition::outcome(outcome(
            RequiredOutcomeKind::Assessment,
        )))
        .with_lifecycle_requirement(lifecycle);
        let plan = Plan::new(plan_id, desired, delta_id, vec![requirement], vec![step]).unwrap();
        plan.validate_against_delta(&delta).unwrap();
        assert_eq!(plan.steps()[0].capability_requirements().len(), 1);
        assert!(plan.steps()[0].verification().is_some());
    }

    #[test]
    fn plan_rejects_self_and_dangling_references() {
        let (_, condition, ..) = ids();
        let self_id = PlanStepId::new("step-self").unwrap();
        let error = PlanStep::new(
            self_id.clone(),
            PlanStepKind::Change,
            outcome(RequiredOutcomeKind::DomainChange),
            PlanCondition::desired_condition(condition),
            "invalid step",
        )
        .unwrap()
        .with_dependencies(vec![self_id]);
        assert!(matches!(
            error,
            Err(ValidationError::SelfReference {
                field: "plan_step.dependencies"
            })
        ));

        let (_, condition, ..) = ids();
        let step = PlanStep::new(
            PlanStepId::new("step-1").unwrap(),
            PlanStepKind::Change,
            outcome(RequiredOutcomeKind::DomainChange),
            PlanCondition::desired_condition(condition.clone()),
            "dangling capability",
        )
        .unwrap()
        .with_capability_requirements(vec![CapabilityRequirementId::new("missing").unwrap()])
        .unwrap();
        assert!(matches!(
            Plan::new(
                PlanId::new("plan-1").unwrap(),
                DesiredStateId::new("desired-1").unwrap(),
                DeltaId::new("delta-1").unwrap(),
                Vec::new(),
                vec![step],
            ),
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "capability_requirement",
                ..
            })
        ));

        let dangling_dependency = PlanStep::new(
            PlanStepId::new("step-1").unwrap(),
            PlanStepKind::Change,
            outcome(RequiredOutcomeKind::DomainChange),
            PlanCondition::desired_condition(condition.clone()),
            "dangling dependency",
        )
        .unwrap()
        .with_dependencies(vec![PlanStepId::new("missing-step").unwrap()])
        .unwrap();
        assert!(matches!(
            Plan::new(
                PlanId::new("plan-1").unwrap(),
                DesiredStateId::new("desired-1").unwrap(),
                DeltaId::new("delta-1").unwrap(),
                Vec::new(),
                vec![dangling_dependency],
            ),
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "plan_step",
                ..
            })
        ));
    }

    #[test]
    fn duplicate_lineage_and_requirement_references_fail_closed() {
        let (desired, condition, situation, state, ..) = ids();
        assert!(matches!(
            DeltaBasis::new(
                Some(situation),
                Some(state),
                Vec::new(),
                vec![
                    FactId::new("fact-1").unwrap(),
                    FactId::new("fact-1").unwrap()
                ],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
            Err(ValidationError::DuplicateRelationship {
                field: "delta_basis.facts"
            })
        ));
        let requirement = CapabilityRequirement::new(
            CapabilityRequirementId::new("requirement-1").unwrap(),
            CapabilityId::new("architecture.analyze").unwrap(),
            RequirementCardinality::Mandatory,
            DeltaItemId::new("delta-item-1").unwrap(),
            "duplicate precondition",
        )
        .unwrap();
        let duplicate = CapabilityRequirement::new(
            CapabilityRequirementId::new("requirement-1").unwrap(),
            CapabilityId::new("architecture.verify").unwrap(),
            RequirementCardinality::Optional,
            DeltaItemId::new("delta-item-1").unwrap(),
            "duplicate identity",
        )
        .unwrap();
        let step = PlanStep::new(
            PlanStepId::new("step-1").unwrap(),
            PlanStepKind::Change,
            outcome(RequiredOutcomeKind::DomainChange),
            PlanCondition::desired_condition(condition),
            "step",
        )
        .unwrap();
        assert!(matches!(
            Plan::new(
                PlanId::new("plan-1").unwrap(),
                desired,
                DeltaId::new("delta-1").unwrap(),
                vec![requirement, duplicate],
                vec![step],
            ),
            Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "capability_requirement",
                ..
            })
        ));
    }

    #[test]
    fn public_contract_surface_is_strict_and_fully_accessible() {
        let (
            desired,
            condition,
            situation,
            state,
            delta_id,
            delta_item_id,
            requirement_id,
            plan_id,
            step_id,
        ) = ids();
        let subject = SubjectPath::from_str("architecture.dependencies").unwrap();

        let version = PlanningIrVersion::V1;
        assert_eq!(version.major(), 1);
        assert_eq!(version.minor(), 0);
        assert!(matches!(
            PlanningIrVersion::new(0, 1),
            Err(ValidationError::InvalidSchemaVersion)
        ));
        assert!(PlanningIrVersion::from_str("1.0.0").is_err());

        let delta_kinds = [
            DeltaKind::Satisfied,
            DeltaKind::UnsatisfiedCondition,
            DeltaKind::Violation,
            DeltaKind::MissingState,
            DeltaKind::MissingEvidence,
            DeltaKind::UnknownState,
            DeltaKind::Conflict,
            DeltaKind::UnresolvedInput,
            DeltaKind::UnsupportedComparison,
        ];
        for kind in delta_kinds {
            assert_eq!(kind.to_string(), kind.as_str());
            assert_eq!(DeltaKind::from_str(kind.as_str()).unwrap(), kind);
        }
        assert!(DeltaKind::from_str("NOT_A_DELTA_KIND").is_err());

        let outcome_kinds = [
            RequiredOutcomeKind::DomainChange,
            RequiredOutcomeKind::EvidenceAcquisition,
            RequiredOutcomeKind::Observation,
            RequiredOutcomeKind::InputAcquisition,
            RequiredOutcomeKind::ConflictResolution,
            RequiredOutcomeKind::Assessment,
            RequiredOutcomeKind::NoOp,
        ];
        for kind in outcome_kinds {
            assert_eq!(kind.to_string(), kind.as_str());
            assert_eq!(RequiredOutcomeKind::from_str(kind.as_str()).unwrap(), kind);
        }
        assert!(RequiredOutcomeKind::from_str("NOT_AN_OUTCOME").is_err());

        let expected =
            TypedValue::set(vec![TypedValue::Integer(1), TypedValue::Integer(2)]).unwrap();
        let required_outcome = RequiredOutcome::new(
            RequiredOutcomeKind::DomainChange,
            "remove the prohibited dependency",
        )
        .unwrap()
        .with_subject(subject.clone())
        .with_expected(expected.clone())
        .unwrap();
        assert_eq!(required_outcome.kind(), RequiredOutcomeKind::DomainChange);
        assert_eq!(
            required_outcome.description(),
            "remove the prohibited dependency"
        );
        assert_eq!(required_outcome.subject(), Some(&subject));
        assert_eq!(required_outcome.expected(), Some(&expected));
        assert!(RequiredOutcome::new(RequiredOutcomeKind::NoOp, " ").is_err());
        assert!(
            RequiredOutcome::new(RequiredOutcomeKind::NoOp, "valid")
                .unwrap()
                .with_expected(TypedValue::Set(Vec::new()))
                .is_err()
        );

        let empty_basis = DeltaBasis::empty();
        assert!(empty_basis.situation().is_none());
        assert!(empty_basis.current_state().is_none());
        assert!(empty_basis.state_subjects().is_empty());
        assert!(empty_basis.facts().is_empty());
        assert!(empty_basis.observations().is_empty());
        assert!(empty_basis.evidence().is_empty());
        assert!(empty_basis.provenances().is_empty());
        assert!(empty_basis.assessments().is_empty());

        let delta_item = DeltaItem::new(
            delta_item_id.clone(),
            desired.clone(),
            condition.clone(),
            DeltaKind::MissingState,
            DeltaBasis::new(
                Some(situation.clone()),
                Some(state.clone()),
                vec![subject.clone()],
                vec![FactId::new("fact-1").unwrap()],
                vec![ObservationId::new("observation-1").unwrap()],
                vec![EvidenceId::new("evidence-1").unwrap()],
                vec![ProvenanceId::new("provenance-1").unwrap()],
                vec![AssessmentId::new("assessment-1").unwrap()],
            )
            .unwrap(),
            required_outcome.clone(),
            "state is missing",
        )
        .unwrap();
        assert_eq!(delta_item.id(), &delta_item_id);
        assert_eq!(delta_item.desired_state(), &desired);
        assert_eq!(delta_item.condition(), &condition);
        assert_eq!(delta_item.kind(), DeltaKind::MissingState);
        assert_eq!(delta_item.required_outcome(), &required_outcome);
        assert_eq!(delta_item.rationale(), "state is missing");
        assert_eq!(delta_item.basis().situation(), Some(&situation));
        assert_eq!(delta_item.basis().current_state(), Some(&state));

        let delta = Delta::new(
            delta_id.clone(),
            desired.clone(),
            Some(situation.clone()),
            vec![delta_item.clone()],
        )
        .unwrap();
        assert_eq!(delta.version(), DECLARATIVE_PLANNING_IR_VERSION);
        assert_eq!(delta.id(), &delta_id);
        assert_eq!(delta.desired_state(), &desired);
        assert_eq!(delta.situation(), Some(&situation));
        assert_eq!(delta.items(), std::slice::from_ref(&delta_item));
        assert!(delta.contains_item(&delta_item_id));
        assert!(!delta.contains_item(&DeltaItemId::new("missing-item").unwrap()));
        assert!(!delta.is_noop());
        assert!(
            Delta::new_with_version(
                PlanningIrVersion::new(1, 1).unwrap(),
                DeltaId::new("unsupported-delta").unwrap(),
                desired.clone(),
                None,
                Vec::new(),
            )
            .is_err()
        );

        for cardinality in [
            RequirementCardinality::Mandatory,
            RequirementCardinality::Optional,
        ] {
            assert_eq!(
                RequirementCardinality::from_str(cardinality.as_str()).unwrap(),
                cardinality
            );
            assert_eq!(cardinality.to_string(), cardinality.as_str());
        }
        assert!(RequirementCardinality::from_str("REQUIRED").is_err());
        let requirement = CapabilityRequirement::new(
            requirement_id.clone(),
            CapabilityId::new("architecture.inspect").unwrap(),
            RequirementCardinality::Optional,
            delta_item_id.clone(),
            "inspect the current dependency graph",
        )
        .unwrap();
        assert_eq!(requirement.id(), &requirement_id);
        assert_eq!(requirement.capability(), requirement.capability_id());
        assert_eq!(requirement.cardinality(), RequirementCardinality::Optional);
        assert_eq!(requirement.originating_delta_item(), &delta_item_id);
        assert_eq!(requirement.delta_item_id(), &delta_item_id);
        assert!(requirement.preconditions().is_empty());
        assert!(requirement.constraints().is_empty());
        assert_eq!(
            requirement.rationale(),
            "inspect the current dependency graph"
        );

        let lifecycle_kinds = [
            LifecycleRequirementKind::VerificationAfterChange,
            LifecycleRequirementKind::EvidenceBeforeChange,
            LifecycleRequirementKind::IndependentVerification,
            LifecycleRequirementKind::HumanInput,
        ];
        for kind in lifecycle_kinds {
            assert_eq!(
                LifecycleRequirementKind::from_str(kind.as_str()).unwrap(),
                kind
            );
            assert_eq!(kind.to_string(), kind.as_str());
        }
        assert!(LifecycleRequirementKind::from_str("PROCESS_DEFINITION").is_err());
        let lifecycle = LifecycleRequirement::new(
            LifecycleRequirementKind::IndependentVerification,
            "verification must be independent",
        )
        .unwrap();
        assert_eq!(
            lifecycle.kind(),
            LifecycleRequirementKind::IndependentVerification
        );
        assert_eq!(lifecycle.description(), "verification must be independent");
        assert!(LifecycleRequirement::new(LifecycleRequirementKind::HumanInput, " ").is_err());

        let desired_condition = PlanCondition::desired_condition(condition.clone());
        assert!(matches!(
            desired_condition,
            PlanCondition::DesiredCondition(ref id) if id == &condition
        ));
        let outcome_condition = PlanCondition::outcome(required_outcome.clone());
        assert!(matches!(outcome_condition, PlanCondition::Outcome(_)));

        let step_kind_values = [
            PlanStepKind::NoOp,
            PlanStepKind::Change,
            PlanStepKind::Verification,
            PlanStepKind::EvidenceAcquisition,
            PlanStepKind::InputAcquisition,
            PlanStepKind::Observation,
            PlanStepKind::ConflictResolution,
        ];
        for kind in step_kind_values {
            assert_eq!(PlanStepKind::from_str(kind.as_str()).unwrap(), kind);
            assert_eq!(kind.to_string(), kind.as_str());
        }
        assert!(PlanStepKind::from_str("UNKNOWN_STEP").is_err());

        let step = PlanStep::new(
            step_id.clone(),
            PlanStepKind::Change,
            required_outcome.clone(),
            desired_condition,
            "apply the required change",
        )
        .unwrap()
        .with_capability_requirements(vec![requirement_id.clone()])
        .unwrap()
        .with_delta_items(vec![delta_item_id.clone()])
        .unwrap()
        .with_verification(outcome_condition)
        .with_lifecycle_requirement(lifecycle);
        assert_eq!(step.id(), &step_id);
        assert_eq!(step.kind(), PlanStepKind::Change);
        assert_eq!(step.outcome(), &required_outcome);
        assert!(step.dependencies().is_empty());
        assert_eq!(step.capability_requirements(), &[requirement_id]);
        assert_eq!(step.delta_items(), &[delta_item_id]);
        assert!(matches!(
            step.completion(),
            PlanCondition::DesiredCondition(_)
        ));
        assert!(matches!(
            step.verification(),
            Some(PlanCondition::Outcome(_))
        ));
        assert!(step.lifecycle_requirement().is_some());
        assert_eq!(step.rationale(), "apply the required change");
        assert!(
            PlanStep::new(
                PlanStepId::new("invalid-step").unwrap(),
                PlanStepKind::NoOp,
                outcome(RequiredOutcomeKind::NoOp),
                PlanCondition::outcome(outcome(RequiredOutcomeKind::NoOp)),
                " ",
            )
            .is_err()
        );

        let plan = Plan::new(
            plan_id.clone(),
            desired.clone(),
            delta_id.clone(),
            vec![requirement],
            vec![step.clone()],
        )
        .unwrap();
        assert_eq!(plan.version(), DECLARATIVE_PLANNING_IR_VERSION);
        assert_eq!(plan.id(), &plan_id);
        assert_eq!(plan.desired_state(), &desired);
        assert_eq!(plan.delta(), &delta_id);
        assert_eq!(plan.capability_requirements().len(), 1);
        assert_eq!(plan.steps(), std::slice::from_ref(&step));
        assert!(!plan.is_noop());
        assert!(
            Plan::new(
                PlanId::new("empty-plan").unwrap(),
                desired.clone(),
                delta_id.clone(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap()
            .is_noop()
        );
        assert!(
            Plan::new_with_version(
                PlanningIrVersion::new(1, 1).unwrap(),
                PlanId::new("unsupported-plan").unwrap(),
                desired.clone(),
                delta_id.clone(),
                Vec::new(),
                Vec::new(),
            )
            .is_err()
        );

        let wrong_delta = Delta::new(
            DeltaId::new("wrong-delta").unwrap(),
            desired.clone(),
            None,
            Vec::new(),
        )
        .unwrap();
        assert!(matches!(
            plan.validate_against_delta(&wrong_delta),
            Err(ValidationError::InvalidStateCombination { .. })
        ));
        let wrong_desired = Delta::new(
            delta_id,
            DesiredStateId::new("wrong-desired").unwrap(),
            None,
            Vec::new(),
        )
        .unwrap();
        assert!(matches!(
            plan.validate_against_delta(&wrong_desired),
            Err(ValidationError::InvalidStateCombination { .. })
        ));
    }

    #[test]
    fn validates_delta_and_plan_references_against_desired_state() {
        let (desired, condition, situation, state, delta_id, delta_item_id, ..) = ids();
        let desired_contract = desired_state(&desired, &condition);
        let delta = Delta::new(
            delta_id.clone(),
            desired.clone(),
            Some(situation.clone()),
            vec![item(&desired, &condition, &situation, &state)],
        )
        .unwrap();
        delta
            .validate_against_desired_state(&desired_contract)
            .unwrap();

        let wrong_desired_contract =
            desired_state(&DesiredStateId::new("wrong-desired").unwrap(), &condition);
        assert!(matches!(
            delta.validate_against_desired_state(&wrong_desired_contract),
            Err(ValidationError::InvalidStateCombination { .. })
        ));

        let missing_condition = ConditionId::new("missing-condition").unwrap();
        let missing_condition_delta = Delta::new(
            DeltaId::new("missing-condition-delta").unwrap(),
            desired.clone(),
            Some(situation.clone()),
            vec![
                DeltaItem::new(
                    DeltaItemId::new("missing-condition-item").unwrap(),
                    desired.clone(),
                    missing_condition,
                    DeltaKind::UnknownState,
                    basis(&situation, &state),
                    outcome(RequiredOutcomeKind::Observation),
                    "condition cannot be assessed",
                )
                .unwrap(),
            ],
        )
        .unwrap();
        assert!(matches!(
            missing_condition_delta.validate_against_desired_state(&desired_contract),
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "condition",
                ..
            })
        ));

        let valid_step = PlanStep::new(
            PlanStepId::new("step-1").unwrap(),
            PlanStepKind::Change,
            outcome(RequiredOutcomeKind::DomainChange),
            PlanCondition::desired_condition(condition.clone()),
            "close the declared gap",
        )
        .unwrap()
        .with_delta_items(vec![delta_item_id])
        .unwrap()
        .with_verification(PlanCondition::desired_condition(condition.clone()));
        let plan = Plan::new(
            PlanId::new("plan-1").unwrap(),
            desired.clone(),
            delta_id,
            Vec::new(),
            vec![valid_step],
        )
        .unwrap();
        plan.validate_against_desired_state(&desired_contract)
            .unwrap();

        let missing_condition_step = PlanStep::new(
            PlanStepId::new("step-missing-condition").unwrap(),
            PlanStepKind::Verification,
            outcome(RequiredOutcomeKind::Assessment),
            PlanCondition::desired_condition(ConditionId::new("missing-condition").unwrap()),
            "verify an undeclared condition",
        )
        .unwrap();
        let invalid_plan = Plan::new(
            PlanId::new("invalid-plan").unwrap(),
            desired.clone(),
            DeltaId::new("delta-1").unwrap(),
            Vec::new(),
            vec![missing_condition_step],
        )
        .unwrap();
        assert!(matches!(
            invalid_plan.validate_against_desired_state(&desired_contract),
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "condition",
                ..
            })
        ));
        assert!(matches!(
            plan.validate_against_desired_state(&wrong_desired_contract),
            Err(ValidationError::InvalidStateCombination { .. })
        ));
    }
}
