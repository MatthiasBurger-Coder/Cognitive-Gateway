//! CG-07.04 deterministic information-resolution planning inputs.
//!
//! Planning inputs describe what must be learned, supplied or resolved before
//! a desired state can be decided safely.  They are declarative outcomes only:
//! this module does not retrieve evidence, ask a user, select a source or
//! mutate a Situation.

use std::{fmt, str::FromStr};

use crate::{
    identifiers::{ConditionId, DeltaId, DeltaItemId, DesiredStateId, PlanningInputId},
    intent::DesiredState,
    planning::{
        Delta, DeltaItem, DeltaKind, DeltaReasonCode, PlanningIrVersion, RequiredOutcome,
        RequiredOutcomeKind,
    },
    quality::SensitivityClass,
    validation::{NonEmptyText, ValidationError},
};

/// The currently supported information-resolution planning-input version.
pub const PLANNING_INPUT_VERSION: PlanningIrVersion = PlanningIrVersion::V1;

/// The semantic kind of information needed to make a Delta item decidable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PlanningInputKind {
    EvidenceAcquisition,
    Observation,
    ConflictResolution,
    InputAcquisition,
    Normalization,
}

impl PlanningInputKind {
    /// Returns the stable machine-readable kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EvidenceAcquisition => "EVIDENCE_ACQUISITION",
            Self::Observation => "OBSERVATION",
            Self::ConflictResolution => "CONFLICT_RESOLUTION",
            Self::InputAcquisition => "INPUT_ACQUISITION",
            Self::Normalization => "NORMALIZATION",
        }
    }
}

impl fmt::Display for PlanningInputKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for PlanningInputKind {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "EVIDENCE_ACQUISITION" => Ok(Self::EvidenceAcquisition),
            "OBSERVATION" => Ok(Self::Observation),
            "CONFLICT_RESOLUTION" => Ok(Self::ConflictResolution),
            "INPUT_ACQUISITION" => Ok(Self::InputAcquisition),
            "NORMALIZATION" => Ok(Self::Normalization),
            value => Err(ValidationError::UnknownDomainValue {
                field: "planning_input_kind",
                value: value.to_owned(),
            }),
        }
    }
}

/// The freshness contract a future evidence result must meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum FreshnessRequirement {
    NotSpecified,
    Fresh,
}

impl FreshnessRequirement {
    /// Returns the stable machine-readable freshness requirement.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotSpecified => "NOT_SPECIFIED",
            Self::Fresh => "FRESH",
        }
    }
}

impl fmt::Display for FreshnessRequirement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for FreshnessRequirement {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "NOT_SPECIFIED" => Ok(Self::NotSpecified),
            "FRESH" => Ok(Self::Fresh),
            value => Err(ValidationError::UnknownDomainValue {
                field: "freshness_requirement",
                value: value.to_owned(),
            }),
        }
    }
}

/// Explicit constraints carried by an information-resolution input.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct InformationRequirements {
    freshness: FreshnessRequirement,
    minimum_sensitivity: Option<SensitivityClass>,
    evidence: Vec<crate::EvidenceId>,
    provenances: Vec<crate::ProvenanceId>,
}

impl InformationRequirements {
    /// Creates canonical information requirements from opaque references.
    pub fn new(
        freshness: FreshnessRequirement,
        minimum_sensitivity: Option<SensitivityClass>,
        mut evidence: Vec<crate::EvidenceId>,
        mut provenances: Vec<crate::ProvenanceId>,
    ) -> Result<Self, ValidationError> {
        sort_unique(&mut evidence, "information_requirements.evidence")?;
        sort_unique(&mut provenances, "information_requirements.provenances")?;
        Ok(Self {
            freshness,
            minimum_sensitivity,
            evidence,
            provenances,
        })
    }

    /// Returns the freshness requirement.
    #[must_use]
    pub const fn freshness(&self) -> FreshnessRequirement {
        self.freshness
    }

    /// Returns the optional minimum handling classification.
    #[must_use]
    pub const fn minimum_sensitivity(&self) -> Option<SensitivityClass> {
        self.minimum_sensitivity
    }

    /// Returns referenced evidence identities in canonical order.
    #[must_use]
    pub fn evidence(&self) -> &[crate::EvidenceId] {
        &self.evidence
    }

    /// Returns referenced provenance identities in canonical order.
    #[must_use]
    pub fn provenances(&self) -> &[crate::ProvenanceId] {
        &self.provenances
    }
}

/// Machine-readable completion condition for an information input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PlanningInputCompletion {
    EvidenceAvailable,
    FreshEvidenceAvailable,
    StateObserved,
    ConflictResolved,
    ExplicitInputProvided,
    ComparableStateAvailable,
}

impl PlanningInputCompletion {
    /// Returns the stable machine-readable completion condition.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EvidenceAvailable => "EVIDENCE_AVAILABLE",
            Self::FreshEvidenceAvailable => "FRESH_EVIDENCE_AVAILABLE",
            Self::StateObserved => "STATE_OBSERVED",
            Self::ConflictResolved => "CONFLICT_RESOLVED",
            Self::ExplicitInputProvided => "EXPLICIT_INPUT_PROVIDED",
            Self::ComparableStateAvailable => "COMPARABLE_STATE_AVAILABLE",
        }
    }
}

impl fmt::Display for PlanningInputCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Machine-readable verification condition for an information input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PlanningInputVerification {
    EvidenceValidated,
    ObservationNormalized,
    ExplicitResolutionRecorded,
    InputRecorded,
    ComparisonSupported,
}

impl PlanningInputVerification {
    /// Returns the stable machine-readable verification condition.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EvidenceValidated => "EVIDENCE_VALIDATED",
            Self::ObservationNormalized => "OBSERVATION_NORMALIZED",
            Self::ExplicitResolutionRecorded => "EXPLICIT_RESOLUTION_RECORDED",
            Self::InputRecorded => "INPUT_RECORDED",
            Self::ComparisonSupported => "COMPARISON_SUPPORTED",
        }
    }
}

impl fmt::Display for PlanningInputVerification {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Explicit options for deriving information inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PlanningInputRules {
    version: PlanningIrVersion,
    minimum_sensitivity: Option<SensitivityClass>,
}

impl PlanningInputRules {
    /// Creates supported rules without inventing a sensitivity requirement.
    pub fn new(version: PlanningIrVersion) -> Result<Self, ValidationError> {
        version.ensure_supported()?;
        Ok(Self {
            version,
            minimum_sensitivity: None,
        })
    }

    /// Returns the default v1 rules.
    #[must_use]
    pub const fn v1() -> Self {
        Self {
            version: PLANNING_INPUT_VERSION,
            minimum_sensitivity: None,
        }
    }

    /// Adds an explicit minimum handling classification for acquired data.
    #[must_use]
    pub const fn requiring_minimum_sensitivity(mut self, sensitivity: SensitivityClass) -> Self {
        self.minimum_sensitivity = Some(sensitivity);
        self
    }

    /// Returns the planning-input version.
    #[must_use]
    pub const fn version(self) -> PlanningIrVersion {
        self.version
    }

    /// Returns the optional explicit sensitivity requirement.
    #[must_use]
    pub const fn minimum_sensitivity(self) -> Option<SensitivityClass> {
        self.minimum_sensitivity
    }
}

impl Default for PlanningInputRules {
    fn default() -> Self {
        Self::v1()
    }
}

/// One declarative information-resolution requirement derived from a Delta.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PlanningInput {
    version: PlanningIrVersion,
    id: PlanningInputId,
    desired_state: DesiredStateId,
    delta: DeltaId,
    delta_item: DeltaItemId,
    condition: ConditionId,
    kind: PlanningInputKind,
    reason: DeltaReasonCode,
    required_outcome: RequiredOutcome,
    completion: PlanningInputCompletion,
    verification: PlanningInputVerification,
    requirements: InformationRequirements,
    rationale: NonEmptyText,
}

impl PlanningInput {
    /// Creates one validated planning input.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: PlanningInputId,
        desired_state: DesiredStateId,
        delta: DeltaId,
        delta_item: DeltaItemId,
        condition: ConditionId,
        kind: PlanningInputKind,
        reason: DeltaReasonCode,
        required_outcome: RequiredOutcome,
        completion: PlanningInputCompletion,
        verification: PlanningInputVerification,
        requirements: InformationRequirements,
        rationale: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        validate_outcome_kind(kind, required_outcome.kind())?;
        Ok(Self {
            version: PLANNING_INPUT_VERSION,
            id,
            desired_state,
            delta,
            delta_item,
            condition,
            kind,
            reason,
            required_outcome,
            completion,
            verification,
            requirements,
            rationale: NonEmptyText::new_for_field(rationale, "planning_input.rationale")?,
        })
    }

    /// Returns the stable input identity.
    #[must_use]
    pub fn id(&self) -> &PlanningInputId {
        &self.id
    }

    /// Returns the owning desired-state identity.
    #[must_use]
    pub fn desired_state(&self) -> &DesiredStateId {
        &self.desired_state
    }

    /// Returns the source Delta identity.
    #[must_use]
    pub fn delta(&self) -> &DeltaId {
        &self.delta
    }

    /// Returns the source Delta item identity.
    #[must_use]
    pub fn delta_item(&self) -> &DeltaItemId {
        &self.delta_item
    }

    /// Returns the originating DesiredState condition.
    #[must_use]
    pub fn condition(&self) -> &ConditionId {
        &self.condition
    }

    /// Returns the information-resolution kind.
    #[must_use]
    pub const fn kind(&self) -> PlanningInputKind {
        self.kind
    }

    /// Returns the semantic Delta reason retained by this input.
    #[must_use]
    pub const fn reason(&self) -> DeltaReasonCode {
        self.reason
    }

    /// Returns the declarative outcome to achieve.
    #[must_use]
    pub const fn required_outcome(&self) -> &RequiredOutcome {
        &self.required_outcome
    }

    /// Returns the machine-readable completion condition.
    #[must_use]
    pub const fn completion(&self) -> PlanningInputCompletion {
        self.completion
    }

    /// Returns the machine-readable verification condition.
    #[must_use]
    pub const fn verification(&self) -> PlanningInputVerification {
        self.verification
    }

    /// Returns explicit freshness, sensitivity and lineage constraints.
    #[must_use]
    pub const fn requirements(&self) -> &InformationRequirements {
        &self.requirements
    }

    /// Returns the stable human-readable rationale.
    #[must_use]
    pub fn rationale(&self) -> &str {
        self.rationale.as_str()
    }
}

/// Derives all information-resolution inputs from a validated Delta.
pub fn derive_planning_inputs(
    delta: &Delta,
    desired_state: &DesiredState,
    rules: &PlanningInputRules,
) -> Result<Vec<PlanningInput>, ValidationError> {
    rules.version.ensure_supported()?;
    delta.validate_against_desired_state(desired_state)?;
    let mut inputs = Vec::new();
    for item in delta.items() {
        if let Some(input) = derive_one(item, delta, rules)? {
            inputs.push(input);
        }
    }
    inputs.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(inputs)
}

/// Derives one information-resolution input when the Delta item requires it.
pub fn derive_planning_input(
    delta: &Delta,
    desired_state: &DesiredState,
    delta_item: &DeltaItemId,
    rules: &PlanningInputRules,
) -> Result<Option<PlanningInput>, ValidationError> {
    rules.version.ensure_supported()?;
    delta.validate_against_desired_state(desired_state)?;
    let item = delta
        .items()
        .iter()
        .find(|item| item.id() == delta_item)
        .ok_or_else(|| ValidationError::MissingDeclarativeIdentity {
            kind: "delta_item",
            id: delta_item.to_string(),
        })?;
    derive_one(item, delta, rules)
}

fn derive_one(
    item: &DeltaItem,
    delta: &Delta,
    rules: &PlanningInputRules,
) -> Result<Option<PlanningInput>, ValidationError> {
    let (kind, completion, verification, freshness) = match item.kind() {
        DeltaKind::MissingEvidence => (
            PlanningInputKind::EvidenceAcquisition,
            if matches!(
                item.reason(),
                DeltaReasonCode::StaleEvidence | DeltaReasonCode::FreshnessUnknown
            ) {
                PlanningInputCompletion::FreshEvidenceAvailable
            } else {
                PlanningInputCompletion::EvidenceAvailable
            },
            PlanningInputVerification::EvidenceValidated,
            if matches!(
                item.reason(),
                DeltaReasonCode::StaleEvidence | DeltaReasonCode::FreshnessUnknown
            ) {
                FreshnessRequirement::Fresh
            } else {
                FreshnessRequirement::NotSpecified
            },
        ),
        DeltaKind::UnknownState => (
            PlanningInputKind::Observation,
            PlanningInputCompletion::StateObserved,
            PlanningInputVerification::ObservationNormalized,
            FreshnessRequirement::NotSpecified,
        ),
        DeltaKind::Conflict => (
            PlanningInputKind::ConflictResolution,
            PlanningInputCompletion::ConflictResolved,
            PlanningInputVerification::ExplicitResolutionRecorded,
            FreshnessRequirement::NotSpecified,
        ),
        DeltaKind::UnresolvedInput => (
            PlanningInputKind::InputAcquisition,
            PlanningInputCompletion::ExplicitInputProvided,
            PlanningInputVerification::InputRecorded,
            FreshnessRequirement::NotSpecified,
        ),
        DeltaKind::UnsupportedComparison => (
            PlanningInputKind::Normalization,
            PlanningInputCompletion::ComparableStateAvailable,
            PlanningInputVerification::ComparisonSupported,
            FreshnessRequirement::NotSpecified,
        ),
        DeltaKind::Satisfied
        | DeltaKind::UnsatisfiedCondition
        | DeltaKind::Violation
        | DeltaKind::MissingState => return Ok(None),
    };

    let requirements = InformationRequirements::new(
        freshness,
        rules.minimum_sensitivity,
        item.basis().evidence().to_vec(),
        item.basis().provenances().to_vec(),
    )?;
    let input = PlanningInput::new(
        PlanningInputId::new(item.id().as_str())?,
        desired_state_id(delta),
        delta.id().clone(),
        item.id().clone(),
        item.condition().clone(),
        kind,
        item.reason(),
        item.required_outcome().clone(),
        completion,
        verification,
        requirements,
        format!(
            "information input for Delta item {}: {}",
            item.id(),
            item.rationale()
        ),
    )?;
    Ok(Some(input))
}

fn desired_state_id(delta: &Delta) -> DesiredStateId {
    delta.desired_state().clone()
}

fn validate_outcome_kind(
    kind: PlanningInputKind,
    outcome: RequiredOutcomeKind,
) -> Result<(), ValidationError> {
    let valid = match kind {
        PlanningInputKind::EvidenceAcquisition => {
            outcome == RequiredOutcomeKind::EvidenceAcquisition
        }
        PlanningInputKind::Observation => outcome == RequiredOutcomeKind::Observation,
        PlanningInputKind::ConflictResolution => outcome == RequiredOutcomeKind::ConflictResolution,
        PlanningInputKind::InputAcquisition => outcome == RequiredOutcomeKind::InputAcquisition,
        PlanningInputKind::Normalization => outcome == RequiredOutcomeKind::Assessment,
    };
    if valid {
        Ok(())
    } else {
        Err(ValidationError::InvalidStateCombination {
            reason: "planning input kind and required outcome kind must agree",
        })
    }
}

fn sort_unique<T: Ord>(values: &mut [T], field: &'static str) -> Result<(), ValidationError> {
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ValidationError::DuplicateRelationship { field });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        identifiers::{ConditionId, CurrentStateId, DeltaId, DeltaItemId, DesiredStateId},
        intent::{
            ComparisonOperator, ConditionExpression, DesiredCondition, DesiredState, SubjectPath,
            TypedValue,
        },
        planning::{
            Delta, DeltaBasis, DeltaItem, DeltaKind, DeltaReasonCode, PlanningIrVersion,
            RequiredOutcome,
        },
        quality::SensitivityClass,
        validation::ValidationError,
    };

    use super::*;

    fn desired() -> DesiredState {
        DesiredState::new(
            DesiredStateId::new("desired-1").unwrap(),
            vec![
                DesiredCondition::new(
                    ConditionId::new("coverage").unwrap(),
                    SubjectPath::from_str("coverage.percent").unwrap(),
                    ComparisonOperator::Equals,
                    Some(TypedValue::Integer(95)),
                )
                .unwrap(),
            ],
            ConditionExpression::condition(ConditionId::new("coverage").unwrap()),
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    fn item(kind: DeltaKind, reason: DeltaReasonCode, outcome: RequiredOutcomeKind) -> DeltaItem {
        DeltaItem::new_with_reason(
            DeltaItemId::new(format!("item-{}", kind)).unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            ConditionId::new("coverage").unwrap(),
            kind,
            reason,
            DeltaBasis::new(
                None,
                Some(CurrentStateId::new("state-1").unwrap()),
                vec![SubjectPath::from_str("coverage.percent").unwrap()],
                Vec::new(),
                Vec::new(),
                vec![crate::EvidenceId::new("evidence-1").unwrap()],
                vec![crate::ProvenanceId::new("provenance-1").unwrap()],
                Vec::new(),
            )
            .unwrap(),
            RequiredOutcome::new(outcome, "achieve the explicit information outcome").unwrap(),
            "deterministic information gap",
        )
        .unwrap()
    }

    fn delta(items: Vec<DeltaItem>) -> Delta {
        Delta::new(
            DeltaId::new("delta-1").unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            None,
            items,
        )
        .unwrap()
    }

    #[test]
    fn derives_each_information_kind_and_preserves_references() {
        let cases = [
            (
                DeltaKind::MissingEvidence,
                DeltaReasonCode::MissingEvidence,
                PlanningInputKind::EvidenceAcquisition,
                RequiredOutcomeKind::EvidenceAcquisition,
            ),
            (
                DeltaKind::UnknownState,
                DeltaReasonCode::SubjectNotObserved,
                PlanningInputKind::Observation,
                RequiredOutcomeKind::Observation,
            ),
            (
                DeltaKind::Conflict,
                DeltaReasonCode::StateConflict,
                PlanningInputKind::ConflictResolution,
                RequiredOutcomeKind::ConflictResolution,
            ),
            (
                DeltaKind::UnresolvedInput,
                DeltaReasonCode::UnresolvedInput,
                PlanningInputKind::InputAcquisition,
                RequiredOutcomeKind::InputAcquisition,
            ),
            (
                DeltaKind::UnsupportedComparison,
                DeltaReasonCode::IncompatibleTypes,
                PlanningInputKind::Normalization,
                RequiredOutcomeKind::Assessment,
            ),
        ];
        let items = cases
            .iter()
            .map(|(kind, reason, _, outcome)| item(*kind, *reason, *outcome))
            .collect();
        let delta = delta(items);
        let rules = PlanningInputRules::default()
            .requiring_minimum_sensitivity(SensitivityClass::Confidential);
        let inputs = derive_planning_inputs(&delta, &desired(), &rules).unwrap();
        assert_eq!(inputs.len(), cases.len());
        for (_, _, expected_kind, expected_outcome) in cases {
            let input = inputs
                .iter()
                .find(|input| input.kind() == expected_kind)
                .unwrap();
            assert_eq!(input.kind(), expected_kind);
            assert_eq!(input.required_outcome().kind(), expected_outcome);
            assert_eq!(input.delta(), delta.id());
            assert_eq!(input.desired_state(), delta.desired_state());
            assert_eq!(input.requirements().evidence().len(), 1);
            assert_eq!(input.requirements().provenances().len(), 1);
            assert_eq!(
                input.requirements().minimum_sensitivity(),
                Some(SensitivityClass::Confidential)
            );
        }
    }

    #[test]
    fn stale_and_unknown_freshness_require_fresh_evidence() {
        for reason in [
            DeltaReasonCode::StaleEvidence,
            DeltaReasonCode::FreshnessUnknown,
        ] {
            let delta = delta(vec![item(
                DeltaKind::MissingEvidence,
                reason,
                RequiredOutcomeKind::EvidenceAcquisition,
            )]);
            let input = derive_planning_inputs(&delta, &desired(), &PlanningInputRules::default())
                .unwrap()
                .pop()
                .unwrap();
            assert_eq!(
                input.requirements().freshness(),
                FreshnessRequirement::Fresh
            );
            assert_eq!(
                input.completion(),
                PlanningInputCompletion::FreshEvidenceAvailable
            );
        }
    }

    #[test]
    fn domain_gaps_and_satisfied_items_do_not_create_information_inputs() {
        let delta = delta(vec![
            item(
                DeltaKind::Satisfied,
                DeltaReasonCode::ConditionSatisfied,
                RequiredOutcomeKind::NoOp,
            ),
            item(
                DeltaKind::UnsatisfiedCondition,
                DeltaReasonCode::ValueMismatch,
                RequiredOutcomeKind::DomainChange,
            ),
            item(
                DeltaKind::Violation,
                DeltaReasonCode::ExplicitViolation,
                RequiredOutcomeKind::DomainChange,
            ),
            item(
                DeltaKind::MissingState,
                DeltaReasonCode::MissingState,
                RequiredOutcomeKind::DomainChange,
            ),
        ]);
        assert!(
            derive_planning_inputs(&delta, &desired(), &PlanningInputRules::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rules_and_public_codes_are_strict() {
        assert_eq!(
            PlanningInputRules::default().version(),
            PlanningIrVersion::V1
        );
        assert!(PlanningInputRules::new(PlanningIrVersion::V1).is_ok());
        assert!(matches!(
            PlanningInputRules::new(PlanningIrVersion::new(1, 1).unwrap()),
            Err(ValidationError::UnsupportedSchemaVersion { .. })
        ));
        for kind in [
            PlanningInputKind::EvidenceAcquisition,
            PlanningInputKind::Observation,
            PlanningInputKind::ConflictResolution,
            PlanningInputKind::InputAcquisition,
            PlanningInputKind::Normalization,
        ] {
            assert_eq!(PlanningInputKind::from_str(kind.as_str()).unwrap(), kind);
            assert_eq!(kind.to_string(), kind.as_str());
        }
        assert!(PlanningInputKind::from_str("UNKNOWN").is_err());
        for freshness in [
            FreshnessRequirement::NotSpecified,
            FreshnessRequirement::Fresh,
        ] {
            assert_eq!(
                FreshnessRequirement::from_str(freshness.as_str()).unwrap(),
                freshness
            );
            assert_eq!(freshness.to_string(), freshness.as_str());
        }
        assert!(FreshnessRequirement::from_str("UNKNOWN").is_err());
        for completion in [
            PlanningInputCompletion::EvidenceAvailable,
            PlanningInputCompletion::FreshEvidenceAvailable,
            PlanningInputCompletion::StateObserved,
            PlanningInputCompletion::ConflictResolved,
            PlanningInputCompletion::ExplicitInputProvided,
            PlanningInputCompletion::ComparableStateAvailable,
        ] {
            assert_eq!(completion.to_string(), completion.as_str());
        }
        for verification in [
            PlanningInputVerification::EvidenceValidated,
            PlanningInputVerification::ObservationNormalized,
            PlanningInputVerification::ExplicitResolutionRecorded,
            PlanningInputVerification::InputRecorded,
            PlanningInputVerification::ComparisonSupported,
        ] {
            assert_eq!(verification.to_string(), verification.as_str());
        }
        let requirements = InformationRequirements::new(
            FreshnessRequirement::Fresh,
            None,
            vec![crate::EvidenceId::new("evidence-1").unwrap()],
            vec![crate::ProvenanceId::new("provenance-1").unwrap()],
        )
        .unwrap();
        assert!(
            InformationRequirements::new(
                FreshnessRequirement::Fresh,
                None,
                vec![crate::EvidenceId::new("evidence-1").unwrap(); 2],
                Vec::new(),
            )
            .is_err()
        );
        assert!(
            PlanningInput::new(
                PlanningInputId::new("input-1").unwrap(),
                DesiredStateId::new("desired-1").unwrap(),
                DeltaId::new("delta-1").unwrap(),
                DeltaItemId::new("item-1").unwrap(),
                ConditionId::new("coverage").unwrap(),
                PlanningInputKind::Observation,
                DeltaReasonCode::SubjectNotObserved,
                RequiredOutcome::new(RequiredOutcomeKind::EvidenceAcquisition, "wrong").unwrap(),
                PlanningInputCompletion::StateObserved,
                PlanningInputVerification::ObservationNormalized,
                requirements,
                "invalid outcome pairing",
            )
            .is_err()
        );
    }

    #[test]
    fn single_input_lookup_is_traceable_and_fail_closed() {
        let delta = delta(vec![item(
            DeltaKind::UnknownState,
            DeltaReasonCode::StateUnknown,
            RequiredOutcomeKind::Observation,
        )]);
        let item_id = delta.items()[0].id().clone();
        let input =
            derive_planning_input(&delta, &desired(), &item_id, &PlanningInputRules::default())
                .unwrap()
                .unwrap();
        assert_eq!(input.id().as_str(), item_id.as_str());
        assert_eq!(input.condition().as_str(), "coverage");
        assert!(
            derive_planning_input(
                &delta,
                &desired(),
                &DeltaItemId::new("missing").unwrap(),
                &PlanningInputRules::default(),
            )
            .is_err()
        );
    }

    #[test]
    fn outcome_kind_validation_covers_all_input_kinds() {
        let outcome = RequiredOutcome::new(RequiredOutcomeKind::NoOp, "wrong").unwrap();
        for kind in [
            PlanningInputKind::EvidenceAcquisition,
            PlanningInputKind::Observation,
            PlanningInputKind::ConflictResolution,
            PlanningInputKind::InputAcquisition,
            PlanningInputKind::Normalization,
        ] {
            assert!(validate_outcome_kind(kind, outcome.kind()).is_err());
        }
        assert!(
            validate_outcome_kind(
                PlanningInputKind::Normalization,
                RequiredOutcomeKind::Assessment
            )
            .is_ok()
        );
    }
}
