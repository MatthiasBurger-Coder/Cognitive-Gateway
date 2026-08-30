//! CG-07.05 deterministic derivation of abstract capability requirements.
//!
//! The derivation binds a Delta's typed required outcomes to explicitly
//! configured canonical [`CapabilityDefinition`] contracts.  The input is a
//! provider-independent catalog snapshot; no Agent, Skill, ProcessDefinition
//! or runtime selection is represented here.  Missing or incompatible
//! contracts are retained as blocking diagnostics so a caller cannot mistake
//! an incomplete derivation for an executable plan.

use std::{collections::BTreeMap, fmt, str::FromStr};

use crate::{
    CapabilityClass, CapabilityDefinition, CapabilityId, CapabilityRequirement,
    CapabilityRequirementId, Delta, DeltaItem, DeltaItemId, DeltaKind, DesiredState,
    DesiredStateId, NonEmptyText, PlanningIrVersion, RequiredOutcomeKind, RequirementCardinality,
    ValidationError,
};

/// The currently supported abstract capability-requirement derivation version.
pub const CAPABILITY_REQUIREMENTS_VERSION: PlanningIrVersion = PlanningIrVersion::V1;

/// An explicit canonical capability binding for one required outcome kind.
///
/// Equivalent alternatives are opt-in.  They are emitted as optional
/// requirements and never discovered by fuzzy matching or catalog order.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct CapabilityBinding {
    capability: CapabilityId,
    cardinality: RequirementCardinality,
    equivalent_alternatives: Vec<CapabilityId>,
}

impl CapabilityBinding {
    /// Creates a binding with explicit mandatory/optional cardinality.
    #[must_use]
    pub fn new(capability: CapabilityId, cardinality: RequirementCardinality) -> Self {
        Self {
            capability,
            cardinality,
            equivalent_alternatives: Vec::new(),
        }
    }

    /// Creates a mandatory binding.
    #[must_use]
    pub fn mandatory(capability: CapabilityId) -> Self {
        Self::new(capability, RequirementCardinality::Mandatory)
    }

    /// Creates an optional binding.
    #[must_use]
    pub fn optional(capability: CapabilityId) -> Self {
        Self::new(capability, RequirementCardinality::Optional)
    }

    /// Adds explicitly equivalent optional alternatives in canonical order.
    pub fn with_equivalent_alternatives(
        mut self,
        mut alternatives: Vec<CapabilityId>,
    ) -> Result<Self, ValidationError> {
        alternatives.sort();
        if alternatives
            .iter()
            .any(|alternative| alternative == &self.capability)
            || alternatives.windows(2).any(|pair| pair[0] == pair[1])
        {
            return Err(ValidationError::DuplicateRelationship {
                field: "capability_binding.equivalent_alternatives",
            });
        }
        self.equivalent_alternatives = alternatives;
        Ok(self)
    }

    /// Returns the explicitly bound canonical capability.
    #[must_use]
    pub fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    /// Returns the primary requirement cardinality.
    #[must_use]
    pub const fn cardinality(&self) -> RequirementCardinality {
        self.cardinality
    }

    /// Returns explicitly equivalent alternatives in canonical order.
    #[must_use]
    pub fn equivalent_alternatives(&self) -> &[CapabilityId] {
        &self.equivalent_alternatives
    }
}

/// Explicit outcome-to-capability bindings used by deterministic derivation.
///
/// No default binding is inferred from descriptions, tags, providers or
/// catalog position.  Callers must bind the canonical IDs supplied by CG-03.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct CapabilityRequirementRules {
    version: PlanningIrVersion,
    domain_change: Option<CapabilityBinding>,
    evidence_acquisition: Option<CapabilityBinding>,
    observation: Option<CapabilityBinding>,
    input_acquisition: Option<CapabilityBinding>,
    conflict_resolution: Option<CapabilityBinding>,
    assessment: Option<CapabilityBinding>,
}

impl CapabilityRequirementRules {
    /// Creates supported rules without inventing capability IDs.
    pub fn new(version: PlanningIrVersion) -> Result<Self, ValidationError> {
        version.ensure_supported()?;
        Ok(Self {
            version,
            domain_change: None,
            evidence_acquisition: None,
            observation: None,
            input_acquisition: None,
            conflict_resolution: None,
            assessment: None,
        })
    }

    /// Returns empty supported v1 rules.
    #[must_use]
    pub const fn v1() -> Self {
        Self {
            version: CAPABILITY_REQUIREMENTS_VERSION,
            domain_change: None,
            evidence_acquisition: None,
            observation: None,
            input_acquisition: None,
            conflict_resolution: None,
            assessment: None,
        }
    }

    /// Binds domain-changing outcomes to a canonical capability.
    #[must_use]
    pub fn with_domain_change(mut self, capability: CapabilityId) -> Self {
        self.domain_change = Some(CapabilityBinding::mandatory(capability));
        self
    }

    /// Binds domain-changing outcomes with explicit cardinality and alternatives.
    #[must_use]
    pub fn with_domain_change_binding(mut self, binding: CapabilityBinding) -> Self {
        self.domain_change = Some(binding);
        self
    }

    /// Binds evidence-acquisition outcomes to a canonical capability.
    #[must_use]
    pub fn with_evidence_acquisition(mut self, capability: CapabilityId) -> Self {
        self.evidence_acquisition = Some(CapabilityBinding::mandatory(capability));
        self
    }

    /// Binds evidence-acquisition outcomes with explicit metadata.
    #[must_use]
    pub fn with_evidence_acquisition_binding(mut self, binding: CapabilityBinding) -> Self {
        self.evidence_acquisition = Some(binding);
        self
    }

    /// Binds observation/re-observation outcomes to a canonical capability.
    #[must_use]
    pub fn with_observation(mut self, capability: CapabilityId) -> Self {
        self.observation = Some(CapabilityBinding::mandatory(capability));
        self
    }

    /// Binds observation/re-observation outcomes with explicit metadata.
    #[must_use]
    pub fn with_observation_binding(mut self, binding: CapabilityBinding) -> Self {
        self.observation = Some(binding);
        self
    }

    /// Binds explicit input-acquisition outcomes to a canonical capability.
    #[must_use]
    pub fn with_input_acquisition(mut self, capability: CapabilityId) -> Self {
        self.input_acquisition = Some(CapabilityBinding::mandatory(capability));
        self
    }

    /// Binds input-acquisition outcomes with explicit metadata.
    #[must_use]
    pub fn with_input_acquisition_binding(mut self, binding: CapabilityBinding) -> Self {
        self.input_acquisition = Some(binding);
        self
    }

    /// Binds conflict-resolution outcomes to a canonical capability.
    #[must_use]
    pub fn with_conflict_resolution(mut self, capability: CapabilityId) -> Self {
        self.conflict_resolution = Some(CapabilityBinding::mandatory(capability));
        self
    }

    /// Binds conflict-resolution outcomes with explicit metadata.
    #[must_use]
    pub fn with_conflict_resolution_binding(mut self, binding: CapabilityBinding) -> Self {
        self.conflict_resolution = Some(binding);
        self
    }

    /// Binds assessment/verification outcomes to a canonical capability.
    #[must_use]
    pub fn with_assessment(mut self, capability: CapabilityId) -> Self {
        self.assessment = Some(CapabilityBinding::mandatory(capability));
        self
    }

    /// Alias for [`Self::with_assessment`] emphasizing verification use.
    #[must_use]
    pub fn with_verification(self, capability: CapabilityId) -> Self {
        self.with_assessment(capability)
    }

    /// Binds assessment/verification outcomes with explicit metadata.
    #[must_use]
    pub fn with_assessment_binding(mut self, binding: CapabilityBinding) -> Self {
        self.assessment = Some(binding);
        self
    }

    /// Returns the derivation rules version.
    #[must_use]
    pub const fn version(&self) -> PlanningIrVersion {
        self.version
    }

    fn binding(&self, outcome: RequiredOutcomeKind) -> Option<&CapabilityBinding> {
        match outcome {
            RequiredOutcomeKind::DomainChange => self.domain_change.as_ref(),
            RequiredOutcomeKind::EvidenceAcquisition => self.evidence_acquisition.as_ref(),
            RequiredOutcomeKind::Observation => self.observation.as_ref(),
            RequiredOutcomeKind::InputAcquisition => self.input_acquisition.as_ref(),
            RequiredOutcomeKind::ConflictResolution => self.conflict_resolution.as_ref(),
            RequiredOutcomeKind::Assessment => self.assessment.as_ref(),
            RequiredOutcomeKind::NoOp => None,
        }
    }
}

impl Default for CapabilityRequirementRules {
    fn default() -> Self {
        Self::v1()
    }
}

/// Stable diagnostic classification for capability-requirement derivation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum CapabilityRequirementDiagnosticCode {
    MissingCapabilityContract,
    IncompatibleCapabilityClass,
    UnsupportedRequiredOutcome,
    InvalidOutcomeForDelta,
}

impl CapabilityRequirementDiagnosticCode {
    /// Returns the stable machine-readable diagnostic name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingCapabilityContract => "MISSING_CAPABILITY_CONTRACT",
            Self::IncompatibleCapabilityClass => "INCOMPATIBLE_CAPABILITY_CLASS",
            Self::UnsupportedRequiredOutcome => "UNSUPPORTED_REQUIRED_OUTCOME",
            Self::InvalidOutcomeForDelta => "INVALID_OUTCOME_FOR_DELTA",
        }
    }
}

impl fmt::Display for CapabilityRequirementDiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CapabilityRequirementDiagnosticCode {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "MISSING_CAPABILITY_CONTRACT" => Ok(Self::MissingCapabilityContract),
            "INCOMPATIBLE_CAPABILITY_CLASS" => Ok(Self::IncompatibleCapabilityClass),
            "UNSUPPORTED_REQUIRED_OUTCOME" => Ok(Self::UnsupportedRequiredOutcome),
            "INVALID_OUTCOME_FOR_DELTA" => Ok(Self::InvalidOutcomeForDelta),
            value => Err(ValidationError::UnknownDomainValue {
                field: "capability_requirement_diagnostic_code",
                value: value.to_owned(),
            }),
        }
    }
}

/// One explicit gap retained when capability derivation cannot close safely.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct CapabilityRequirementDiagnostic {
    code: CapabilityRequirementDiagnosticCode,
    delta_item: DeltaItemId,
    outcome: RequiredOutcomeKind,
    capability: Option<CapabilityId>,
    blocking: bool,
    rationale: NonEmptyText,
}

impl CapabilityRequirementDiagnostic {
    fn new(
        code: CapabilityRequirementDiagnosticCode,
        item: &DeltaItem,
        capability: Option<CapabilityId>,
        blocking: bool,
        rationale: String,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            code,
            delta_item: item.id().clone(),
            outcome: item.required_outcome().kind(),
            capability,
            blocking,
            rationale: NonEmptyText::new_for_field(
                rationale,
                "capability_requirement_diagnostic.rationale",
            )?,
        })
    }

    /// Returns the stable diagnostic code.
    #[must_use]
    pub const fn code(&self) -> CapabilityRequirementDiagnosticCode {
        self.code
    }

    /// Returns the originating Delta item.
    #[must_use]
    pub fn delta_item(&self) -> &DeltaItemId {
        &self.delta_item
    }

    /// Returns the required outcome that could not be bound safely.
    #[must_use]
    pub const fn outcome(&self) -> RequiredOutcomeKind {
        self.outcome
    }

    /// Returns the referenced capability, when one was configured.
    #[must_use]
    pub fn capability(&self) -> Option<&CapabilityId> {
        self.capability.as_ref()
    }

    /// Returns whether this diagnostic prevents execution readiness.
    #[must_use]
    pub const fn is_blocking(&self) -> bool {
        self.blocking
    }

    /// Returns the stable diagnostic rationale.
    #[must_use]
    pub fn rationale(&self) -> &str {
        self.rationale.as_str()
    }
}

/// The deterministic capability-requirement derivation result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDerivation {
    version: PlanningIrVersion,
    delta: crate::DeltaId,
    desired_state: DesiredStateId,
    requirements: Vec<CapabilityRequirement>,
    diagnostics: Vec<CapabilityRequirementDiagnostic>,
}

impl CapabilityDerivation {
    /// Returns the derivation version.
    #[must_use]
    pub const fn version(&self) -> PlanningIrVersion {
        self.version
    }

    /// Returns the source Delta identity.
    #[must_use]
    pub fn delta(&self) -> &crate::DeltaId {
        &self.delta
    }

    /// Returns the source DesiredState identity.
    #[must_use]
    pub fn desired_state(&self) -> &DesiredStateId {
        &self.desired_state
    }

    /// Returns requirements in stable identity order.
    #[must_use]
    pub fn requirements(&self) -> &[CapabilityRequirement] {
        &self.requirements
    }

    /// Returns explicit derivation diagnostics in stable source order.
    #[must_use]
    pub fn diagnostics(&self) -> &[CapabilityRequirementDiagnostic] {
        &self.diagnostics
    }

    /// Returns whether no blocking contract gap remains.
    #[must_use]
    pub fn is_execution_ready(&self) -> bool {
        self.diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_blocking())
    }

    /// Returns whether the result contains no requirements or diagnostics.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.requirements.is_empty() && self.diagnostics.is_empty()
    }
}

/// Compatibility alias emphasizing that the result is a derivation.
pub type CapabilityRequirementDerivation = CapabilityDerivation;

/// Derives abstract requirements using explicit canonical capability bindings.
pub fn derive_capability_requirements(
    delta: &Delta,
    desired_state: &DesiredState,
    capabilities: &[CapabilityDefinition],
    rules: &CapabilityRequirementRules,
) -> Result<CapabilityDerivation, ValidationError> {
    rules.version.ensure_supported()?;
    delta.validate_against_desired_state(desired_state)?;
    let catalog = canonical_catalog(capabilities)?;
    let mut requirements = Vec::new();
    let mut diagnostics = Vec::new();

    for item in delta.items().iter().filter(|item| item.is_actionable()) {
        if !outcome_matches_delta(item.kind(), item.required_outcome().kind()) {
            diagnostics.push(CapabilityRequirementDiagnostic::new(
                CapabilityRequirementDiagnosticCode::InvalidOutcomeForDelta,
                item,
                None,
                true,
                format!(
                    "Delta item {} classifies as {} but declares required outcome {}; no capability requirement was emitted",
                    item.id(),
                    item.kind(),
                    item.required_outcome().kind()
                ),
            )?);
            continue;
        }

        let outcome = item.required_outcome().kind();
        let Some(binding) = rules.binding(outcome) else {
            diagnostics.push(CapabilityRequirementDiagnostic::new(
                CapabilityRequirementDiagnosticCode::UnsupportedRequiredOutcome,
                item,
                None,
                true,
                format!(
                    "required outcome {} for Delta item {} has no explicit canonical capability binding",
                    outcome,
                    item.id()
                ),
            )?);
            continue;
        };

        derive_binding(item, binding, &catalog, &mut requirements, &mut diagnostics)?;
    }

    requirements.sort_by(|left, right| left.id().cmp(right.id()));
    diagnostics.sort_by(|left, right| {
        left.delta_item
            .cmp(&right.delta_item)
            .then(left.code.cmp(&right.code))
            .then(left.capability.cmp(&right.capability))
    });
    Ok(CapabilityDerivation {
        version: CAPABILITY_REQUIREMENTS_VERSION,
        delta: delta.id().clone(),
        desired_state: desired_state.id().clone(),
        requirements,
        diagnostics,
    })
}

fn canonical_catalog(
    capabilities: &[CapabilityDefinition],
) -> Result<BTreeMap<CapabilityId, &CapabilityDefinition>, ValidationError> {
    let mut catalog = BTreeMap::new();
    for capability in capabilities {
        if catalog
            .insert(capability.id().clone(), capability)
            .is_some()
        {
            return Err(ValidationError::DuplicateDefinition {
                kind: "capability",
                id: capability.id().to_string(),
            });
        }
    }
    Ok(catalog)
}

fn derive_binding(
    item: &DeltaItem,
    binding: &CapabilityBinding,
    catalog: &BTreeMap<CapabilityId, &CapabilityDefinition>,
    requirements: &mut Vec<CapabilityRequirement>,
    diagnostics: &mut Vec<CapabilityRequirementDiagnostic>,
) -> Result<(), ValidationError> {
    append_requirement(
        item,
        binding.capability(),
        binding.cardinality(),
        catalog,
        requirements,
        diagnostics,
        false,
    )?;
    for alternative in binding.equivalent_alternatives() {
        append_requirement(
            item,
            alternative,
            RequirementCardinality::Optional,
            catalog,
            requirements,
            diagnostics,
            true,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_requirement(
    item: &DeltaItem,
    capability_id: &CapabilityId,
    cardinality: RequirementCardinality,
    catalog: &BTreeMap<CapabilityId, &CapabilityDefinition>,
    requirements: &mut Vec<CapabilityRequirement>,
    diagnostics: &mut Vec<CapabilityRequirementDiagnostic>,
    alternative: bool,
) -> Result<(), ValidationError> {
    let Some(capability) = catalog.get(capability_id) else {
        diagnostics.push(CapabilityRequirementDiagnostic::new(
            CapabilityRequirementDiagnosticCode::MissingCapabilityContract,
            item,
            Some(capability_id.clone()),
            cardinality == RequirementCardinality::Mandatory,
            format!(
                "canonical capability contract {} is missing for Delta item {}; execution readiness remains closed",
                capability_id,
                item.id()
            ),
        )?);
        return Ok(());
    };

    let expected_class = expected_capability_class(item.required_outcome().kind());
    if capability.class() != expected_class {
        diagnostics.push(CapabilityRequirementDiagnostic::new(
            CapabilityRequirementDiagnosticCode::IncompatibleCapabilityClass,
            item,
            Some(capability_id.clone()),
            cardinality == RequirementCardinality::Mandatory,
            format!(
                "canonical capability contract {} is {} but required outcome {} needs {}; no incompatible requirement was emitted",
                capability_id,
                capability.class(),
                item.required_outcome().kind(),
                expected_class
            ),
        )?);
        return Ok(());
    }

    let requirement_id = requirement_id(item.id(), capability_id, alternative);
    if requirements.iter().any(|requirement| {
        requirement.originating_delta_item() == item.id()
            && requirement.capability() == capability_id
            && requirement.cardinality() == cardinality
            && requirement.preconditions() == capability.preconditions()
            && requirement.constraints() == capability.constraints()
    }) {
        return Ok(());
    }

    requirements.push(CapabilityRequirement::new_with_metadata(
        requirement_id,
        capability_id.clone(),
        cardinality,
        item.id().clone(),
        capability.preconditions().to_vec(),
        capability.constraints().to_vec(),
        requirement_rationale(item, capability_id, alternative),
    )?);
    Ok(())
}

fn requirement_id(
    delta_item: &DeltaItemId,
    capability: &CapabilityId,
    alternative: bool,
) -> CapabilityRequirementId {
    let readable = if alternative {
        format!("requirement-{}-alternative-{}", delta_item, capability)
    } else {
        format!("requirement-{}", delta_item)
    };
    CapabilityRequirementId::new(if readable.len() <= 128 {
        readable
    } else {
        format!("requirement-{:016x}", stable_hash(&readable))
    })
    .expect("derived requirement identity must satisfy identifier validation")
}

fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn requirement_rationale(item: &DeltaItem, capability: &CapabilityId, alternative: bool) -> String {
    let basis = item.basis();
    let situation = basis
        .situation()
        .map_or_else(|| "none".to_owned(), ToString::to_string);
    let current_state = basis
        .current_state()
        .map_or_else(|| "none".to_owned(), ToString::to_string);
    let role = if alternative {
        "explicitly equivalent optional alternative"
    } else {
        "explicit canonical capability binding"
    };
    format!(
        "{} {} derives from Delta item {} (condition {}, desired state {}, situation {}, current state {}); source rationale: {}",
        role,
        capability,
        item.id(),
        item.condition(),
        item.desired_state(),
        situation,
        current_state,
        item.rationale()
    )
}

fn expected_capability_class(outcome: RequiredOutcomeKind) -> CapabilityClass {
    match outcome {
        RequiredOutcomeKind::DomainChange => CapabilityClass::Mutate,
        RequiredOutcomeKind::EvidenceAcquisition
        | RequiredOutcomeKind::Observation
        | RequiredOutcomeKind::InputAcquisition
        | RequiredOutcomeKind::ConflictResolution
        | RequiredOutcomeKind::Assessment
        | RequiredOutcomeKind::NoOp => CapabilityClass::Inspect,
    }
}

fn outcome_matches_delta(delta_kind: DeltaKind, outcome: RequiredOutcomeKind) -> bool {
    match delta_kind {
        DeltaKind::Satisfied => outcome == RequiredOutcomeKind::NoOp,
        DeltaKind::UnsatisfiedCondition | DeltaKind::Violation | DeltaKind::MissingState => {
            outcome == RequiredOutcomeKind::DomainChange
        }
        DeltaKind::MissingEvidence => matches!(
            outcome,
            RequiredOutcomeKind::EvidenceAcquisition | RequiredOutcomeKind::Assessment
        ),
        DeltaKind::UnknownState => matches!(
            outcome,
            RequiredOutcomeKind::Observation | RequiredOutcomeKind::Assessment
        ),
        DeltaKind::Conflict => matches!(
            outcome,
            RequiredOutcomeKind::ConflictResolution
                | RequiredOutcomeKind::Observation
                | RequiredOutcomeKind::Assessment
        ),
        DeltaKind::UnresolvedInput => outcome == RequiredOutcomeKind::InputAcquisition,
        DeltaKind::UnsupportedComparison => matches!(
            outcome,
            RequiredOutcomeKind::Assessment | RequiredOutcomeKind::Observation
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        CapabilityClass, CapabilityDefinition, ComparisonOperator, ConditionExpression,
        CurrentStateId, DeltaBasis, DeltaId, DeltaItem, DeltaKind, DesiredCondition, DesiredState,
        DesiredStateId, RequiredOutcome, SubjectPath, TypedValue,
    };

    use super::*;

    fn desired() -> DesiredState {
        DesiredState::new(
            DesiredStateId::new("desired-1").unwrap(),
            vec![
                DesiredCondition::new(
                    crate::ConditionId::new("condition-1").unwrap(),
                    SubjectPath::from_str("service.status").unwrap(),
                    ComparisonOperator::Equals,
                    Some(TypedValue::Symbol(
                        crate::SymbolValue::new("healthy").unwrap(),
                    )),
                )
                .unwrap(),
            ],
            ConditionExpression::condition(crate::ConditionId::new("condition-1").unwrap()),
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    fn item(kind: DeltaKind, outcome: RequiredOutcomeKind, id: &str) -> DeltaItem {
        DeltaItem::new(
            DeltaItemId::new(id).unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            crate::ConditionId::new("condition-1").unwrap(),
            kind,
            DeltaBasis::new(
                Some(crate::SituationId::new("situation-1").unwrap()),
                Some(CurrentStateId::new("state-1").unwrap()),
                vec![SubjectPath::from_str("service.status").unwrap()],
                vec![crate::FactId::new("fact-1").unwrap()],
                vec![crate::ObservationId::new("observation-1").unwrap()],
                vec![crate::EvidenceId::new("evidence-1").unwrap()],
                vec![crate::ProvenanceId::new("provenance-1").unwrap()],
                vec![crate::AssessmentId::new("assessment-1").unwrap()],
            )
            .unwrap(),
            RequiredOutcome::new(outcome, "achieve the explicit required outcome").unwrap(),
            "traceable Delta rationale",
        )
        .unwrap()
    }

    fn delta(items: Vec<DeltaItem>) -> Delta {
        Delta::new(
            DeltaId::new("delta-1").unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            Some(crate::SituationId::new("situation-1").unwrap()),
            items,
        )
        .unwrap()
    }

    fn capability(id: &str, class: CapabilityClass) -> CapabilityDefinition {
        CapabilityDefinition::new_with_contract(
            CapabilityId::new(id).unwrap(),
            class,
            "planning",
            "fulfil a canonical abstract planning outcome",
            ["planning.input"],
            ["planning.output"],
            ["state.available"],
            ["bounded"],
            ["planning"],
        )
        .unwrap()
    }

    fn all_rules() -> CapabilityRequirementRules {
        CapabilityRequirementRules::v1()
            .with_domain_change(CapabilityId::new("domain.change").unwrap())
            .with_evidence_acquisition(CapabilityId::new("evidence.acquire").unwrap())
            .with_observation(CapabilityId::new("state.observe").unwrap())
            .with_input_acquisition(CapabilityId::new("input.acquire").unwrap())
            .with_conflict_resolution(CapabilityId::new("conflict.resolve").unwrap())
            .with_assessment(CapabilityId::new("state.assess").unwrap())
    }

    #[test]
    fn binding_rules_are_explicit_and_alternatives_are_canonical() {
        let binding = CapabilityBinding::mandatory(CapabilityId::new("state.observe").unwrap())
            .with_equivalent_alternatives(vec![
                CapabilityId::new("state.inspect-b").unwrap(),
                CapabilityId::new("state.inspect-a").unwrap(),
            ])
            .unwrap();
        assert_eq!(binding.cardinality(), RequirementCardinality::Mandatory);
        assert_eq!(
            binding
                .equivalent_alternatives()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["state.inspect-a", "state.inspect-b"]
        );
        assert!(matches!(
            binding.clone().with_equivalent_alternatives(vec![
                CapabilityId::new("state.inspect-a").unwrap(),
                CapabilityId::new("state.inspect-a").unwrap(),
            ]),
            Err(ValidationError::DuplicateRelationship { .. })
        ));
        assert!(matches!(
            binding.with_equivalent_alternatives(vec![CapabilityId::new("state.observe").unwrap()]),
            Err(ValidationError::DuplicateRelationship { .. })
        ));
        assert_eq!(
            CapabilityRequirementRules::default().version(),
            PlanningIrVersion::V1
        );
    }

    #[test]
    fn public_rule_diagnostic_and_result_surfaces_are_strict() {
        let optional = CapabilityBinding::optional(CapabilityId::new("state.observe").unwrap());
        assert_eq!(optional.cardinality(), RequirementCardinality::Optional);
        assert_eq!(optional.capability().as_str(), "state.observe");
        assert!(optional.equivalent_alternatives().is_empty());

        let rules = CapabilityRequirementRules::new(PlanningIrVersion::V1)
            .unwrap()
            .with_domain_change_binding(optional.clone())
            .with_evidence_acquisition_binding(optional.clone())
            .with_observation_binding(optional.clone())
            .with_input_acquisition_binding(optional.clone())
            .with_conflict_resolution_binding(optional.clone())
            .with_assessment_binding(optional)
            .with_verification(CapabilityId::new("state.verify").unwrap());
        assert_eq!(rules.version(), CAPABILITY_REQUIREMENTS_VERSION);

        for code in [
            CapabilityRequirementDiagnosticCode::MissingCapabilityContract,
            CapabilityRequirementDiagnosticCode::IncompatibleCapabilityClass,
            CapabilityRequirementDiagnosticCode::UnsupportedRequiredOutcome,
            CapabilityRequirementDiagnosticCode::InvalidOutcomeForDelta,
        ] {
            assert_eq!(code.to_string(), code.as_str());
            assert_eq!(
                CapabilityRequirementDiagnosticCode::from_str(code.as_str()).unwrap(),
                code
            );
        }
        assert!(matches!(
            CapabilityRequirementDiagnosticCode::from_str("not-a-code"),
            Err(ValidationError::UnknownDomainValue { .. })
        ));

        assert_eq!(
            expected_capability_class(RequiredOutcomeKind::NoOp),
            CapabilityClass::Inspect
        );
        assert!(outcome_matches_delta(
            DeltaKind::Satisfied,
            RequiredOutcomeKind::NoOp
        ));
        assert!(!outcome_matches_delta(
            DeltaKind::Satisfied,
            RequiredOutcomeKind::DomainChange
        ));
        assert!(outcome_matches_delta(
            DeltaKind::Violation,
            RequiredOutcomeKind::DomainChange
        ));
        assert!(outcome_matches_delta(
            DeltaKind::MissingState,
            RequiredOutcomeKind::DomainChange
        ));
        assert!(outcome_matches_delta(
            DeltaKind::MissingEvidence,
            RequiredOutcomeKind::Assessment
        ));
        assert!(outcome_matches_delta(
            DeltaKind::UnknownState,
            RequiredOutcomeKind::Assessment
        ));
        assert!(outcome_matches_delta(
            DeltaKind::Conflict,
            RequiredOutcomeKind::Observation
        ));
        assert!(outcome_matches_delta(
            DeltaKind::UnresolvedInput,
            RequiredOutcomeKind::InputAcquisition
        ));
        assert!(outcome_matches_delta(
            DeltaKind::UnsupportedComparison,
            RequiredOutcomeKind::Observation
        ));
        assert!(!outcome_matches_delta(
            DeltaKind::UnresolvedInput,
            RequiredOutcomeKind::Assessment
        ));
    }

    #[test]
    fn maps_all_supported_outcomes_to_compatible_contracts() {
        let items = vec![
            item(
                DeltaKind::UnsatisfiedCondition,
                RequiredOutcomeKind::DomainChange,
                "item-change",
            ),
            item(
                DeltaKind::MissingEvidence,
                RequiredOutcomeKind::EvidenceAcquisition,
                "item-evidence",
            ),
            item(
                DeltaKind::UnknownState,
                RequiredOutcomeKind::Observation,
                "item-observe",
            ),
            item(
                DeltaKind::UnresolvedInput,
                RequiredOutcomeKind::InputAcquisition,
                "item-input",
            ),
            item(
                DeltaKind::Conflict,
                RequiredOutcomeKind::ConflictResolution,
                "item-conflict",
            ),
            item(
                DeltaKind::UnsupportedComparison,
                RequiredOutcomeKind::Assessment,
                "item-assess",
            ),
        ];
        let catalog = vec![
            capability("state.assess", CapabilityClass::Inspect),
            capability("conflict.resolve", CapabilityClass::Inspect),
            capability("input.acquire", CapabilityClass::Inspect),
            capability("state.observe", CapabilityClass::Inspect),
            capability("evidence.acquire", CapabilityClass::Inspect),
            capability("domain.change", CapabilityClass::Mutate),
        ];
        let result =
            derive_capability_requirements(&delta(items), &desired(), &catalog, &all_rules())
                .unwrap();
        assert_eq!(result.requirements().len(), 6);
        assert!(result.diagnostics().is_empty());
        assert!(result.is_execution_ready());
        assert!(!result.is_noop());
        assert_eq!(result.version(), CAPABILITY_REQUIREMENTS_VERSION);
        assert_eq!(result.delta().as_str(), "delta-1");
        assert_eq!(result.desired_state().as_str(), "desired-1");
        assert!(
            result
                .requirements()
                .windows(2)
                .all(|pair| pair[0].id() < pair[1].id())
        );
        assert!(result.requirements().iter().all(|requirement| {
            requirement.rationale().contains("desired state desired-1")
                && requirement.rationale().contains("situation-1")
                && requirement.rationale().contains("state-1")
                && !requirement.rationale().contains("Agent")
                && !requirement.rationale().contains("Skill")
        }));
    }

    #[test]
    fn preserves_contract_metadata_and_explicit_equivalent_optional_alternatives() {
        let primary = capability("state.observe", CapabilityClass::Inspect);
        let alternative = capability("state.inspect", CapabilityClass::Inspect);
        let binding = CapabilityBinding::mandatory(primary.id().clone())
            .with_equivalent_alternatives(vec![alternative.id().clone()])
            .unwrap();
        let rules = CapabilityRequirementRules::v1().with_observation_binding(binding);
        let result = derive_capability_requirements(
            &delta(vec![item(
                DeltaKind::UnknownState,
                RequiredOutcomeKind::Observation,
                "item-1",
            )]),
            &desired(),
            &[primary, alternative],
            &rules,
        )
        .unwrap();
        assert_eq!(result.requirements().len(), 2);
        assert_eq!(
            result.requirements()[0].cardinality(),
            RequirementCardinality::Mandatory
        );
        assert_eq!(
            result.requirements()[1].cardinality(),
            RequirementCardinality::Optional
        );
        assert_eq!(
            result.requirements()[0].preconditions()[0].as_str(),
            "state.available"
        );
        assert_eq!(
            result.requirements()[0].constraints()[0].as_str(),
            "bounded"
        );
        assert!(result.is_execution_ready());
    }

    #[test]
    fn reports_missing_and_incompatible_contracts_fail_closed() {
        let missing = derive_capability_requirements(
            &delta(vec![item(
                DeltaKind::UnknownState,
                RequiredOutcomeKind::Observation,
                "item-missing",
            )]),
            &desired(),
            &[],
            &CapabilityRequirementRules::v1()
                .with_observation(CapabilityId::new("state.observe").unwrap()),
        )
        .unwrap();
        assert_eq!(missing.requirements().len(), 0);
        assert_eq!(
            missing.diagnostics()[0].code(),
            CapabilityRequirementDiagnosticCode::MissingCapabilityContract
        );
        assert!(missing.diagnostics()[0].is_blocking());
        assert!(!missing.is_execution_ready());
        assert_eq!(
            missing.diagnostics()[0].delta_item().as_str(),
            "item-missing"
        );
        assert_eq!(
            missing.diagnostics()[0].outcome(),
            RequiredOutcomeKind::Observation
        );
        assert_eq!(
            missing.diagnostics()[0].capability().unwrap().as_str(),
            "state.observe"
        );
        assert!(!missing.is_noop());

        let incompatible = derive_capability_requirements(
            &delta(vec![item(
                DeltaKind::UnsatisfiedCondition,
                RequiredOutcomeKind::DomainChange,
                "item-bad-class",
            )]),
            &desired(),
            &[capability("domain.change", CapabilityClass::Inspect)],
            &CapabilityRequirementRules::v1()
                .with_domain_change(CapabilityId::new("domain.change").unwrap()),
        )
        .unwrap();
        assert_eq!(
            incompatible.diagnostics()[0].code(),
            CapabilityRequirementDiagnosticCode::IncompatibleCapabilityClass
        );
        assert!(!incompatible.is_execution_ready());
    }

    #[test]
    fn reports_unbound_and_malformed_outcomes_explicitly() {
        let unbound = derive_capability_requirements(
            &delta(vec![item(
                DeltaKind::Conflict,
                RequiredOutcomeKind::ConflictResolution,
                "item-unbound",
            )]),
            &desired(),
            &[],
            &CapabilityRequirementRules::v1(),
        )
        .unwrap();
        assert_eq!(
            unbound.diagnostics()[0].code(),
            CapabilityRequirementDiagnosticCode::UnsupportedRequiredOutcome
        );
        assert!(
            unbound.diagnostics()[0]
                .rationale()
                .contains("no explicit canonical capability binding")
        );
        assert!(unbound.diagnostics()[0].capability().is_none());
        assert!(!unbound.is_noop());

        let malformed = derive_capability_requirements(
            &delta(vec![item(
                DeltaKind::UnknownState,
                RequiredOutcomeKind::DomainChange,
                "item-malformed",
            )]),
            &desired(),
            &[],
            &CapabilityRequirementRules::v1(),
        )
        .unwrap();
        assert_eq!(
            malformed.diagnostics()[0].code(),
            CapabilityRequirementDiagnosticCode::InvalidOutcomeForDelta
        );
        assert!(!malformed.is_execution_ready());
    }

    #[test]
    fn ignores_satisfied_items_and_is_invariant_to_catalog_order() {
        let satisfied = item(
            DeltaKind::Satisfied,
            RequiredOutcomeKind::NoOp,
            "item-satisfied",
        );
        let changed = item(
            DeltaKind::UnsatisfiedCondition,
            RequiredOutcomeKind::DomainChange,
            "item-change",
        );
        let first = vec![capability("domain.change", CapabilityClass::Mutate)];
        let second = vec![capability("domain.change", CapabilityClass::Mutate)];
        let result_a = derive_capability_requirements(
            &delta(vec![satisfied.clone(), changed.clone()]),
            &desired(),
            &first,
            &all_rules(),
        )
        .unwrap();
        let result_b = derive_capability_requirements(
            &delta(vec![changed, satisfied]),
            &desired(),
            &second,
            &all_rules(),
        )
        .unwrap();
        assert_eq!(result_a.requirements(), result_b.requirements());
        assert_eq!(result_a.diagnostics(), result_b.diagnostics());
        assert!(!result_a.is_noop());

        let empty =
            derive_capability_requirements(&delta(Vec::new()), &desired(), &[], &all_rules())
                .unwrap();
        assert!(empty.is_noop());
        assert!(empty.is_execution_ready());
    }

    #[test]
    fn rejects_unsupported_versions_and_duplicate_catalog_entries() {
        let version = PlanningIrVersion::new(2, 0).unwrap();
        assert!(matches!(
            CapabilityRequirementRules::new(version),
            Err(ValidationError::UnsupportedSchemaVersion { .. })
        ));
        let duplicate = capability("domain.change", CapabilityClass::Mutate);
        assert!(matches!(
            derive_capability_requirements(
                &delta(vec![item(
                    DeltaKind::UnsatisfiedCondition,
                    RequiredOutcomeKind::DomainChange,
                    "item-duplicate"
                )]),
                &desired(),
                &[duplicate.clone(), duplicate],
                &all_rules(),
            ),
            Err(ValidationError::DuplicateDefinition {
                kind: "capability",
                ..
            })
        ));
        assert_eq!(
            CapabilityRequirementDiagnosticCode::from_str("MISSING_CAPABILITY_CONTRACT")
                .unwrap()
                .to_string(),
            "MISSING_CAPABILITY_CONTRACT"
        );
        assert!(CapabilityRequirementDiagnosticCode::from_str("UNKNOWN").is_err());
    }

    #[test]
    fn long_delta_item_ids_keep_requirement_identity_valid() {
        let long_id = "a".repeat(128);
        let result = derive_capability_requirements(
            &delta(vec![item(
                DeltaKind::UnsatisfiedCondition,
                RequiredOutcomeKind::DomainChange,
                &long_id,
            )]),
            &desired(),
            &[capability("domain.change", CapabilityClass::Mutate)],
            &all_rules(),
        )
        .unwrap();
        assert_eq!(result.requirements().len(), 1);
        assert!(
            result.requirements()[0]
                .id()
                .as_str()
                .starts_with("requirement-")
        );
        assert!(result.requirements()[0].id().as_str().len() <= 128);
    }
}
