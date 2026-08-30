//! CG-07.07 deterministic rule-based declarative planning.
//!
//! This module turns a validated Delta and abstract capability requirements
//! into a declarative Plan.  It decides required outcomes and their explicit
//! dependency shape only.  Concrete Agent, Skill and ProcessDefinition
//! resolution, authorization and process lifecycle transitions remain outside
//! the domain planner.

use std::collections::BTreeMap;

use crate::{
    CapabilityRequirement, CapabilityRequirementDerivation, Delta, DeltaItem, DeltaItemId,
    DeltaKind, DesiredState, LifecycleRequirement, LifecycleRequirementKind, NonEmptyText, Plan,
    PlanCondition, PlanId, PlanStep, PlanStepId, PlanStepKind, PlanningIrVersion, RequiredOutcome,
    RequiredOutcomeKind, ValidationError,
};

/// The currently supported deterministic planner version.
pub const DETERMINISTIC_PLANNER_VERSION: PlanningIrVersion = PlanningIrVersion::V1;

/// Stable identity for a generic planner rule decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PlannerRuleCode {
    NoOp,
    DomainChange,
    ViolationRemediation,
    EvidenceAcquisition,
    Observation,
    InputAcquisition,
    ConflictResolution,
    Assessment,
    InformationBeforeChange,
    VerificationAfterChange,
}

impl PlannerRuleCode {
    /// Returns the stable machine-readable rule code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoOp => "NO_OP",
            Self::DomainChange => "DOMAIN_CHANGE",
            Self::ViolationRemediation => "VIOLATION_REMEDIATION",
            Self::EvidenceAcquisition => "EVIDENCE_ACQUISITION",
            Self::Observation => "OBSERVATION",
            Self::InputAcquisition => "INPUT_ACQUISITION",
            Self::ConflictResolution => "CONFLICT_RESOLUTION",
            Self::Assessment => "ASSESSMENT",
            Self::InformationBeforeChange => "INFORMATION_BEFORE_CHANGE",
            Self::VerificationAfterChange => "VERIFICATION_AFTER_CHANGE",
        }
    }
}

impl std::fmt::Display for PlannerRuleCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for PlannerRuleCode {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "NO_OP" => Ok(Self::NoOp),
            "DOMAIN_CHANGE" => Ok(Self::DomainChange),
            "VIOLATION_REMEDIATION" => Ok(Self::ViolationRemediation),
            "EVIDENCE_ACQUISITION" => Ok(Self::EvidenceAcquisition),
            "OBSERVATION" => Ok(Self::Observation),
            "INPUT_ACQUISITION" => Ok(Self::InputAcquisition),
            "CONFLICT_RESOLUTION" => Ok(Self::ConflictResolution),
            "ASSESSMENT" => Ok(Self::Assessment),
            "INFORMATION_BEFORE_CHANGE" => Ok(Self::InformationBeforeChange),
            "VERIFICATION_AFTER_CHANGE" => Ok(Self::VerificationAfterChange),
            value => Err(ValidationError::UnknownDomainValue {
                field: "planner_rule_code",
                value: value.to_owned(),
            }),
        }
    }
}

/// Stable diagnostic code emitted when generic planning cannot proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PlannerDiagnosticCode {
    MissingCapabilityRequirement,
    MissingVerificationCapabilityRequirement,
    UnsupportedDeltaOutcome,
    CapabilityContractGap,
}

impl PlannerDiagnosticCode {
    /// Returns the stable machine-readable diagnostic code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingCapabilityRequirement => "MISSING_CAPABILITY_REQUIREMENT",
            Self::MissingVerificationCapabilityRequirement => {
                "MISSING_VERIFICATION_CAPABILITY_REQUIREMENT"
            }
            Self::UnsupportedDeltaOutcome => "UNSUPPORTED_DELTA_OUTCOME",
            Self::CapabilityContractGap => "CAPABILITY_CONTRACT_GAP",
        }
    }
}

impl std::fmt::Display for PlannerDiagnosticCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for PlannerDiagnosticCode {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "MISSING_CAPABILITY_REQUIREMENT" => Ok(Self::MissingCapabilityRequirement),
            "MISSING_VERIFICATION_CAPABILITY_REQUIREMENT" => {
                Ok(Self::MissingVerificationCapabilityRequirement)
            }
            "UNSUPPORTED_DELTA_OUTCOME" => Ok(Self::UnsupportedDeltaOutcome),
            "CAPABILITY_CONTRACT_GAP" => Ok(Self::CapabilityContractGap),
            value => Err(ValidationError::UnknownDomainValue {
                field: "planner_diagnostic_code",
                value: value.to_owned(),
            }),
        }
    }
}

/// Explicit generic planner options.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PlannerRules {
    version: PlanningIrVersion,
    information_before_change: bool,
    merge_equivalent_requirements: bool,
    verification_requirement: Option<CapabilityRequirement>,
    verification_lifecycle: Option<LifecycleRequirement>,
    prerequisites: Vec<PlanCondition>,
}

impl PlannerRules {
    /// Creates supported rules with generic information-before-change enabled.
    pub fn new(version: PlanningIrVersion) -> Result<Self, ValidationError> {
        version.ensure_supported()?;
        Ok(Self {
            version,
            information_before_change: true,
            merge_equivalent_requirements: true,
            verification_requirement: None,
            verification_lifecycle: None,
            prerequisites: Vec::new(),
        })
    }

    /// Returns the supported v1 defaults.
    #[must_use]
    pub const fn v1() -> Self {
        Self {
            version: DETERMINISTIC_PLANNER_VERSION,
            information_before_change: true,
            merge_equivalent_requirements: true,
            verification_requirement: None,
            verification_lifecycle: None,
            prerequisites: Vec::new(),
        }
    }

    /// Explicitly enables or disables information prerequisites before changes.
    #[must_use]
    pub const fn with_information_before_change(mut self, required: bool) -> Self {
        self.information_before_change = required;
        self
    }

    /// Requires overlapping information work to precede a domain change.
    #[must_use]
    pub const fn requiring_information_before_change(self) -> Self {
        self.with_information_before_change(true)
    }

    /// Keeps information and domain-change branches independent unless other
    /// explicit graph edges require a dependency.
    #[must_use]
    pub const fn allowing_independent_information(self) -> Self {
        self.with_information_before_change(false)
    }

    /// Explicitly enables or disables semantic requirement deduplication.
    #[must_use]
    pub const fn with_merge_equivalent_requirements(mut self, merge: bool) -> Self {
        self.merge_equivalent_requirements = merge;
        self
    }

    /// Returns whether semantically equivalent abstract requirements merge.
    #[must_use]
    pub const fn merging_equivalent_requirements(&self) -> bool {
        self.merge_equivalent_requirements
    }

    /// Adds a verification capability requirement used after domain changes.
    ///
    /// The requirement must already reference a Delta item.  The planner only
    /// reuses this abstract requirement; it does not create a concrete
    /// executor or capability contract.
    #[must_use]
    pub fn with_verification_requirement(mut self, requirement: CapabilityRequirement) -> Self {
        self.verification_requirement = Some(requirement);
        self.verification_lifecycle = Some(
            LifecycleRequirement::new(
                LifecycleRequirementKind::VerificationAfterChange,
                "verification follows the domain-changing outcome",
            )
            .expect("the built-in verification lifecycle description is valid"),
        );
        self
    }

    /// Adds or replaces the generic lifecycle hint attached to generated
    /// verification steps.
    #[must_use]
    pub fn with_verification_lifecycle(mut self, lifecycle: LifecycleRequirement) -> Self {
        self.verification_lifecycle = Some(lifecycle);
        self
    }

    /// Adds canonical prerequisite conditions to generated steps.
    pub fn with_prerequisites(
        mut self,
        mut prerequisites: Vec<PlanCondition>,
    ) -> Result<Self, ValidationError> {
        sort_unique(&mut prerequisites, "planner_rules.prerequisites")?;
        self.prerequisites = prerequisites;
        Ok(self)
    }

    /// Adds one prerequisite condition to generated steps.
    pub fn with_prerequisite(self, prerequisite: PlanCondition) -> Result<Self, ValidationError> {
        self.with_prerequisites(vec![prerequisite])
    }

    /// Returns the planner rule version.
    #[must_use]
    pub const fn version(&self) -> PlanningIrVersion {
        self.version
    }

    /// Returns whether overlapping information must precede a change.
    #[must_use]
    pub const fn information_before_change(&self) -> bool {
        self.information_before_change
    }

    /// Returns the optional verification requirement.
    #[must_use]
    pub const fn verification_requirement(&self) -> Option<&CapabilityRequirement> {
        self.verification_requirement.as_ref()
    }

    /// Returns the optional verification lifecycle hint.
    #[must_use]
    pub const fn verification_lifecycle(&self) -> Option<&LifecycleRequirement> {
        self.verification_lifecycle.as_ref()
    }

    /// Returns generated-step prerequisite conditions.
    #[must_use]
    pub fn prerequisites(&self) -> &[PlanCondition] {
        &self.prerequisites
    }
}

impl Default for PlannerRules {
    fn default() -> Self {
        Self::v1()
    }
}

/// One inspectable rule decision made by the deterministic planner.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PlannerDecision {
    version: PlanningIrVersion,
    rule: PlannerRuleCode,
    delta_item: DeltaItemId,
    rationale: NonEmptyText,
}

impl PlannerDecision {
    fn new(
        rule: PlannerRuleCode,
        item: &DeltaItem,
        rationale: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            version: DETERMINISTIC_PLANNER_VERSION,
            rule,
            delta_item: item.id().clone(),
            rationale: NonEmptyText::new_for_field(rationale, "planner_decision.rationale")?,
        })
    }

    /// Returns the planner rule version.
    #[must_use]
    pub const fn version(&self) -> PlanningIrVersion {
        self.version
    }

    /// Returns the stable rule identity.
    #[must_use]
    pub const fn rule(&self) -> PlannerRuleCode {
        self.rule
    }

    /// Returns the originating Delta item.
    #[must_use]
    pub fn delta_item(&self) -> &DeltaItemId {
        &self.delta_item
    }

    /// Returns the decision rationale.
    #[must_use]
    pub fn rationale(&self) -> &str {
        self.rationale.as_str()
    }
}

/// One explicit planning gap or unsupported generic case.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PlannerDiagnostic {
    version: PlanningIrVersion,
    code: PlannerDiagnosticCode,
    delta_item: Option<DeltaItemId>,
    blocking: bool,
    rationale: NonEmptyText,
}

impl PlannerDiagnostic {
    fn new(
        code: PlannerDiagnosticCode,
        delta_item: Option<DeltaItemId>,
        blocking: bool,
        rationale: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            version: DETERMINISTIC_PLANNER_VERSION,
            code,
            delta_item,
            blocking,
            rationale: NonEmptyText::new_for_field(rationale, "planner_diagnostic.rationale")?,
        })
    }

    /// Returns the diagnostic version.
    #[must_use]
    pub const fn version(&self) -> PlanningIrVersion {
        self.version
    }

    /// Returns the stable diagnostic code.
    #[must_use]
    pub const fn code(&self) -> PlannerDiagnosticCode {
        self.code
    }

    /// Returns the originating Delta item, when applicable.
    #[must_use]
    pub fn delta_item(&self) -> Option<&DeltaItemId> {
        self.delta_item.as_ref()
    }

    /// Returns whether this diagnostic prevents plan construction.
    #[must_use]
    pub const fn is_blocking(&self) -> bool {
        self.blocking
    }

    /// Returns the diagnostic rationale.
    #[must_use]
    pub fn rationale(&self) -> &str {
        self.rationale.as_str()
    }
}

/// Result of deterministic planning, including inspectable rule decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerResult {
    version: PlanningIrVersion,
    plan: Option<Plan>,
    decisions: Vec<PlannerDecision>,
    diagnostics: Vec<PlannerDiagnostic>,
}

impl PlannerResult {
    /// Returns the planner version.
    #[must_use]
    pub const fn version(&self) -> PlanningIrVersion {
        self.version
    }

    /// Returns the Plan when no blocking diagnostic prevented construction.
    #[must_use]
    pub const fn plan(&self) -> Option<&Plan> {
        self.plan.as_ref()
    }

    /// Returns decisions in canonical Delta-item/rule order.
    #[must_use]
    pub fn decisions(&self) -> &[PlannerDecision] {
        &self.decisions
    }

    /// Returns diagnostics in canonical Delta-item/code order.
    #[must_use]
    pub fn diagnostics(&self) -> &[PlannerDiagnostic] {
        &self.diagnostics
    }

    /// Returns whether a valid plan exists and no blocking gap remains.
    #[must_use]
    pub fn is_execution_ready(&self) -> bool {
        self.plan.is_some()
            && self
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.blocking)
    }

    /// Returns whether the result contains a valid empty/no-op Plan.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.plan.as_ref().is_some_and(Plan::is_noop)
    }
}

/// Plans from a validated Delta and abstract capability requirements.
pub fn plan(
    desired_state: &DesiredState,
    delta: &Delta,
    capability_requirements: &[CapabilityRequirement],
    rules: &PlannerRules,
) -> Result<PlannerResult, ValidationError> {
    rules.version.ensure_supported()?;
    delta.validate_against_desired_state(desired_state)?;
    let mut requirements =
        index_requirements(capability_requirements, rules.merge_equivalent_requirements)?;
    if let Some(verification) = rules.verification_requirement() {
        if !delta.contains_item(verification.originating_delta_item()) {
            return Err(ValidationError::MissingDeclarativeIdentity {
                kind: "delta_item",
                id: verification.originating_delta_item().to_string(),
            });
        }
    }

    let mut decisions = Vec::new();
    let mut diagnostics = Vec::new();
    let mut steps = Vec::new();
    let mut step_contexts = Vec::new();

    for item in delta.items() {
        if !item.is_actionable() {
            decisions.push(PlannerDecision::new(
                PlannerRuleCode::NoOp,
                item,
                format!(
                    "Delta item {} is already satisfied; no PlanStep is required",
                    item.id()
                ),
            )?);
            continue;
        }

        let Some(kind) = step_kind(item.required_outcome().kind()) else {
            diagnostics.push(PlannerDiagnostic::new(
                PlannerDiagnosticCode::UnsupportedDeltaOutcome,
                Some(item.id().clone()),
                true,
                format!(
                    "Delta item {} declares unsupported required outcome {}; no PlanStep was fabricated",
                    item.id(),
                    item.required_outcome().kind()
                ),
            )?);
            continue;
        };

        let requirement_ids = requirements
            .values()
            .filter(|requirement| requirement.originating_delta_item() == item.id())
            .map(|requirement| requirement.id().clone())
            .collect::<Vec<_>>();
        if requirement_ids.is_empty() {
            diagnostics.push(PlannerDiagnostic::new(
                PlannerDiagnosticCode::MissingCapabilityRequirement,
                Some(item.id().clone()),
                true,
                format!(
                    "actionable Delta item {} has no abstract capability requirement",
                    item.id()
                ),
            )?);
            continue;
        }

        let step = make_step(item, kind, requirement_ids, rules)?;
        decisions.push(PlannerDecision::new(
            planner_rule(item.kind(), item.required_outcome().kind()),
            item,
            format!(
                "Delta item {} produces declarative {} work under planner version {}",
                item.id(),
                kind,
                DETERMINISTIC_PLANNER_VERSION
            ),
        )?);
        step_contexts.push(StepContext { item, kind, step });
    }

    for index in 0..step_contexts.len() {
        if step_contexts[index].kind != PlanStepKind::Change || !rules.information_before_change {
            continue;
        }
        let dependencies = step_contexts
            .iter()
            .filter(|information_step| information_step.kind != PlanStepKind::Change)
            .filter(|information_step| {
                information_overlaps(step_contexts[index].item, information_step.item)
            })
            .map(|information_step| information_step.step.id().clone())
            .collect::<Vec<_>>();
        let mut dependencies = dependencies;
        dependencies.sort();
        dependencies.dedup();
        if !dependencies.is_empty() {
            let context = &mut step_contexts[index];
            let item = context.item;
            context.step = context.step.clone().with_dependencies(dependencies)?;
            decisions.push(PlannerDecision::new(
                PlannerRuleCode::InformationBeforeChange,
                item,
                format!(
                    "overlapping information outcomes precede change Delta item {}",
                    item.id()
                ),
            )?);
        }
    }
    steps.extend(step_contexts.into_iter().map(|context| context.step));

    let change_steps = steps
        .iter()
        .filter(|step| step.kind() == PlanStepKind::Change)
        .cloned()
        .collect::<Vec<_>>();
    if let Some(verification_requirement) = rules.verification_requirement() {
        for change in change_steps {
            let Some(item) = delta
                .items()
                .iter()
                .find(|item| step_id(item.id()) == *change.id())
            else {
                continue;
            };
            // Reuse an equivalent canonical requirement when configured;
            // otherwise add the separately validated verification contract.
            let verification_requirement_id = if rules.merge_equivalent_requirements {
                requirements
                    .values()
                    .find(|candidate| semantically_equivalent(candidate, verification_requirement))
                    .map(|candidate| candidate.id().clone())
            } else {
                None
            };
            let verification_requirement_id = verification_requirement_id.unwrap_or_else(|| {
                requirements.insert(
                    verification_requirement.id().clone(),
                    verification_requirement.clone(),
                );
                verification_requirement.id().clone()
            });
            let verification = make_verification_step(item, &verification_requirement_id, rules)?;
            decisions.push(PlannerDecision::new(
                PlannerRuleCode::VerificationAfterChange,
                item,
                format!(
                    "verification for change Delta item {} depends explicitly on {}",
                    item.id(),
                    change.id()
                ),
            )?);
            steps.push(verification);
        }
    }

    decisions.sort_by(|left, right| {
        left.delta_item
            .cmp(&right.delta_item)
            .then(left.rule.cmp(&right.rule))
    });
    diagnostics.sort_by(|left, right| {
        left.delta_item
            .cmp(&right.delta_item)
            .then(left.code.cmp(&right.code))
    });
    if diagnostics.iter().any(PlannerDiagnostic::is_blocking) {
        return Ok(PlannerResult {
            version: DETERMINISTIC_PLANNER_VERSION,
            plan: None,
            decisions,
            diagnostics,
        });
    }

    let plan = Plan::new(
        plan_id(delta.id()),
        desired_state.id().clone(),
        delta.id().clone(),
        requirements.into_values().collect(),
        steps,
    )?;
    plan.validate_against_delta(delta)?;
    plan.validate_against_desired_state(desired_state)?;
    Ok(PlannerResult {
        version: DETERMINISTIC_PLANNER_VERSION,
        plan: Some(plan),
        decisions,
        diagnostics,
    })
}

/// Plans from the output of CG-07.05 capability derivation.
pub fn plan_from_capability_derivation(
    desired_state: &DesiredState,
    delta: &Delta,
    derivation: &CapabilityRequirementDerivation,
    rules: &PlannerRules,
) -> Result<PlannerResult, ValidationError> {
    if derivation.delta() != delta.id() {
        return Err(ValidationError::InvalidStateCombination {
            reason: "capability derivation and Delta must have matching identities",
        });
    }
    if derivation.desired_state() != desired_state.id() {
        return Err(ValidationError::InvalidStateCombination {
            reason: "capability derivation and DesiredState must have matching identities",
        });
    }
    let mut result = plan(desired_state, delta, derivation.requirements(), rules)?;
    for diagnostic in derivation.diagnostics() {
        result.diagnostics.push(PlannerDiagnostic::new(
            PlannerDiagnosticCode::CapabilityContractGap,
            Some(diagnostic.delta_item().clone()),
            diagnostic.is_blocking(),
            format!(
                "capability derivation {} for Delta item {}: {}",
                diagnostic.code(),
                diagnostic.delta_item(),
                diagnostic.rationale()
            ),
        )?);
    }
    result.diagnostics.sort_by(|left, right| {
        left.delta_item
            .cmp(&right.delta_item)
            .then(left.code.cmp(&right.code))
    });
    if result
        .diagnostics
        .iter()
        .any(PlannerDiagnostic::is_blocking)
    {
        result.plan = None;
    }
    Ok(result)
}

/// Derives capability requirements and plans them in one deterministic call.
pub fn plan_from_capabilities(
    desired_state: &DesiredState,
    delta: &Delta,
    capabilities: &[crate::CapabilityDefinition],
    capability_rules: &crate::CapabilityRequirementRules,
    planner_rules: &PlannerRules,
) -> Result<PlannerResult, ValidationError> {
    let derivation = crate::derive_capability_requirements(
        delta,
        desired_state,
        capabilities,
        capability_rules,
    )?;
    plan_from_capability_derivation(desired_state, delta, &derivation, planner_rules)
}

/// Alias emphasizing the planner's derivation result.
pub fn derive_plan(
    delta: &Delta,
    desired_state: &DesiredState,
    capability_requirements: &[CapabilityRequirement],
    rules: &PlannerRules,
) -> Result<PlannerResult, ValidationError> {
    plan(desired_state, delta, capability_requirements, rules)
}

struct StepContext<'a> {
    item: &'a DeltaItem,
    kind: PlanStepKind,
    step: PlanStep,
}

fn index_requirements(
    capability_requirements: &[CapabilityRequirement],
    merge_equivalent: bool,
) -> Result<BTreeMap<crate::CapabilityRequirementId, CapabilityRequirement>, ValidationError> {
    let mut indexed = BTreeMap::new();
    for requirement in capability_requirements {
        if indexed
            .insert(requirement.id().clone(), requirement.clone())
            .is_some()
        {
            return Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "capability_requirement",
                id: requirement.id().to_string(),
            });
        }
    }
    if !merge_equivalent {
        return Ok(indexed);
    }

    let mut canonical = BTreeMap::new();
    for (id, requirement) in indexed {
        if canonical
            .values()
            .any(|existing| semantically_equivalent(existing, &requirement))
        {
            continue;
        }
        canonical.insert(id, requirement);
    }
    Ok(canonical)
}

fn semantically_equivalent(left: &CapabilityRequirement, right: &CapabilityRequirement) -> bool {
    left.capability() == right.capability()
        && left.cardinality() == right.cardinality()
        && left.originating_delta_item() == right.originating_delta_item()
        && left.preconditions() == right.preconditions()
        && left.constraints() == right.constraints()
}

fn make_step(
    item: &DeltaItem,
    kind: PlanStepKind,
    requirements: Vec<crate::CapabilityRequirementId>,
    rules: &PlannerRules,
) -> Result<PlanStep, ValidationError> {
    let completion = if kind == PlanStepKind::Change {
        PlanCondition::desired_condition(item.condition().clone())
    } else {
        PlanCondition::outcome(item.required_outcome().clone())
    };
    PlanStep::new(
        step_id(item.id()),
        kind,
        item.required_outcome().clone(),
        completion,
        format!("plan Delta item {}: {}", item.id(), item.rationale()),
    )?
    .with_capability_requirements(requirements)?
    .with_delta_items(vec![item.id().clone()])?
    .with_prerequisites(rules.prerequisites().to_vec())
}

fn make_verification_step(
    item: &DeltaItem,
    verification_requirement_id: &crate::CapabilityRequirementId,
    rules: &PlannerRules,
) -> Result<PlanStep, ValidationError> {
    let mut step = PlanStep::new(
        verification_step_id(item.id()),
        PlanStepKind::Verification,
        RequiredOutcome::new(
            RequiredOutcomeKind::Assessment,
            format!("verify completion of Delta item {}", item.id()),
        )?,
        PlanCondition::desired_condition(item.condition().clone()),
        format!(
            "verify the domain-changing outcome for Delta item {}",
            item.id()
        ),
    )?
    .with_dependencies(vec![step_id(item.id())])?
    .with_capability_requirements(vec![verification_requirement_id.clone()])?
    .with_delta_items(vec![item.id().clone()])?
    .with_prerequisites(rules.prerequisites().to_vec())?;
    if let Some(lifecycle) = rules.verification_lifecycle() {
        step = step.with_lifecycle_requirement(lifecycle.clone());
    }
    Ok(step)
}

fn step_kind(outcome: RequiredOutcomeKind) -> Option<PlanStepKind> {
    match outcome {
        RequiredOutcomeKind::DomainChange => Some(PlanStepKind::Change),
        RequiredOutcomeKind::EvidenceAcquisition => Some(PlanStepKind::EvidenceAcquisition),
        RequiredOutcomeKind::Observation => Some(PlanStepKind::Observation),
        RequiredOutcomeKind::InputAcquisition => Some(PlanStepKind::InputAcquisition),
        RequiredOutcomeKind::ConflictResolution => Some(PlanStepKind::ConflictResolution),
        RequiredOutcomeKind::Assessment => Some(PlanStepKind::Verification),
        RequiredOutcomeKind::NoOp => None,
    }
}

fn planner_rule(delta_kind: DeltaKind, outcome: RequiredOutcomeKind) -> PlannerRuleCode {
    match delta_kind {
        DeltaKind::Satisfied => PlannerRuleCode::NoOp,
        DeltaKind::Violation => PlannerRuleCode::ViolationRemediation,
        _ => match outcome {
            RequiredOutcomeKind::DomainChange => PlannerRuleCode::DomainChange,
            RequiredOutcomeKind::EvidenceAcquisition => PlannerRuleCode::EvidenceAcquisition,
            RequiredOutcomeKind::Observation => PlannerRuleCode::Observation,
            RequiredOutcomeKind::InputAcquisition => PlannerRuleCode::InputAcquisition,
            RequiredOutcomeKind::ConflictResolution => PlannerRuleCode::ConflictResolution,
            RequiredOutcomeKind::Assessment => PlannerRuleCode::Assessment,
            RequiredOutcomeKind::NoOp => PlannerRuleCode::NoOp,
        },
    }
}

fn information_overlaps(change: &DeltaItem, information: &DeltaItem) -> bool {
    change.condition() == information.condition()
        || change
            .basis()
            .state_subjects()
            .iter()
            .any(|subject| information.basis().state_subjects().contains(subject))
}

fn plan_id(delta: &crate::DeltaId) -> PlanId {
    derived_id("plan", delta.as_str())
}

fn step_id(delta_item: &DeltaItemId) -> PlanStepId {
    derived_id("step", delta_item.as_str())
}

fn verification_step_id(delta_item: &DeltaItemId) -> PlanStepId {
    derived_id("verify", delta_item.as_str())
}

fn derived_id<T>(prefix: &str, source: &str) -> T
where
    T: TryFrom<String, Error = ValidationError>,
{
    let readable = format!("{}-{}", prefix, source);
    let value = if readable.len() <= 128 {
        readable
    } else {
        format!("{}-{:016x}", prefix, stable_hash(&readable))
    };
    T::try_from(value).expect("derived planning identity must satisfy identifier validation")
}

fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
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
        CapabilityClass, CapabilityDefinition, CapabilityId, CapabilityRequirement,
        CapabilityRequirementRules, ComparisonOperator, ConditionExpression, CurrentStateId,
        DeltaBasis, DeltaId, DeltaItem, DeltaKind, DesiredCondition, DesiredState, DesiredStateId,
        PlanCondition, PlanStepKind, RequiredOutcome, RequiredOutcomeKind, SubjectPath, TypedValue,
    };

    use super::*;

    fn desired() -> DesiredState {
        desired_with_id("desired-1")
    }

    fn desired_with_id(id: &str) -> DesiredState {
        DesiredState::new(
            DesiredStateId::new(id).unwrap(),
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
            crate::DeltaItemId::new(id).unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            crate::ConditionId::new("condition-1").unwrap(),
            kind,
            DeltaBasis::new(
                Some(crate::SituationId::new("situation-1").unwrap()),
                Some(CurrentStateId::new("state-1").unwrap()),
                vec![SubjectPath::from_str("service.status").unwrap()],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap(),
            RequiredOutcome::new(outcome, "achieve the planning outcome").unwrap(),
            "planner test Delta item",
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

    fn requirement(item_id: &str, requirement_id: &str, capability: &str) -> CapabilityRequirement {
        CapabilityRequirement::new(
            crate::CapabilityRequirementId::new(requirement_id).unwrap(),
            CapabilityId::new(capability).unwrap(),
            crate::RequirementCardinality::Mandatory,
            crate::DeltaItemId::new(item_id).unwrap(),
            "planner capability requirement",
        )
        .unwrap()
    }

    fn capability(id: &str, class: CapabilityClass) -> CapabilityDefinition {
        CapabilityDefinition::new(CapabilityId::new(id).unwrap(), class)
    }

    #[test]
    fn plans_supported_outcomes_and_preserves_independent_branches() {
        let first = item(
            DeltaKind::UnsatisfiedCondition,
            RequiredOutcomeKind::DomainChange,
            "item-change",
        );
        let second = item(
            DeltaKind::MissingEvidence,
            RequiredOutcomeKind::EvidenceAcquisition,
            "item-evidence",
        );
        let result = plan(
            &desired(),
            &delta(vec![first, second]),
            &[
                requirement("item-change", "req-change", "domain.change"),
                requirement("item-evidence", "req-evidence", "evidence.acquire"),
            ],
            &PlannerRules::v1().allowing_independent_information(),
        )
        .unwrap();
        let plan = result.plan().unwrap();
        assert!(result.is_execution_ready());
        assert!(!result.is_noop());
        assert_eq!(result.version(), DETERMINISTIC_PLANNER_VERSION);
        assert_eq!(plan.id().as_str(), "plan-delta-1");
        assert_eq!(plan.steps().len(), 2);
        assert!(
            plan.steps()
                .iter()
                .all(|step| step.dependencies().is_empty())
        );
        assert!(
            plan.steps()
                .iter()
                .any(|step| step.kind() == PlanStepKind::Change)
        );
        assert!(
            plan.steps()
                .iter()
                .any(|step| step.kind() == PlanStepKind::EvidenceAcquisition)
        );
        assert!(result.decisions().iter().all(|decision| {
            decision.version() == DETERMINISTIC_PLANNER_VERSION
                && decision.rationale().contains("Delta item")
        }));
    }

    #[test]
    fn adds_information_and_verification_dependencies_when_explicitly_required() {
        let evidence = item(
            DeltaKind::MissingEvidence,
            RequiredOutcomeKind::EvidenceAcquisition,
            "item-evidence",
        );
        let change = item(
            DeltaKind::Violation,
            RequiredOutcomeKind::DomainChange,
            "item-change",
        );
        let verification_requirement = requirement("item-change", "req-verify", "state.verify");
        let result = plan(
            &desired(),
            &delta(vec![change, evidence]),
            &[
                requirement("item-change", "req-change", "domain.change"),
                requirement("item-evidence", "req-evidence", "evidence.acquire"),
                verification_requirement.clone(),
            ],
            &PlannerRules::v1().with_verification_requirement(verification_requirement),
        )
        .unwrap();
        let plan = result.plan().unwrap();
        let change_step = plan
            .steps()
            .iter()
            .find(|step| step.kind() == PlanStepKind::Change)
            .unwrap();
        let verification = plan
            .steps()
            .iter()
            .find(|step| step.kind() == PlanStepKind::Verification)
            .unwrap();
        assert!(
            change_step
                .dependencies()
                .contains(&PlanStepId::new("step-item-evidence").unwrap())
        );
        assert_eq!(
            verification.dependencies(),
            &[PlanStepId::new("step-item-change").unwrap()]
        );
        assert_eq!(
            verification.capability_requirements(),
            &[crate::CapabilityRequirementId::new("req-verify").unwrap()]
        );
        assert!(verification.lifecycle_requirement().is_some());
        assert!(
            result
                .decisions()
                .iter()
                .any(|decision| decision.rule() == PlannerRuleCode::VerificationAfterChange)
        );
    }

    #[test]
    fn no_op_and_information_completion_semantics_are_explicit() {
        let satisfied = item(
            DeltaKind::Satisfied,
            RequiredOutcomeKind::NoOp,
            "item-satisfied",
        );
        let result = plan(
            &desired(),
            &delta(vec![satisfied]),
            &[],
            &PlannerRules::default(),
        )
        .unwrap();
        assert!(result.is_noop());
        assert!(result.plan().unwrap().steps().is_empty());
        assert_eq!(result.decisions()[0].rule(), PlannerRuleCode::NoOp);

        let observed = item(
            DeltaKind::UnknownState,
            RequiredOutcomeKind::Observation,
            "item-observed",
        );
        let result = plan(
            &desired(),
            &delta(vec![observed]),
            &[requirement("item-observed", "req-observe", "state.observe")],
            &PlannerRules::v1()
                .with_prerequisite(PlanCondition::outcome(
                    RequiredOutcome::new(
                        RequiredOutcomeKind::Assessment,
                        "prerequisite assessment",
                    )
                    .unwrap(),
                ))
                .unwrap(),
        )
        .unwrap();
        let step = &result.plan().unwrap().steps()[0];
        assert_eq!(step.kind(), PlanStepKind::Observation);
        assert_eq!(
            step.completion(),
            &PlanCondition::outcome(
                item(
                    DeltaKind::UnknownState,
                    RequiredOutcomeKind::Observation,
                    "item-observed",
                )
                .required_outcome()
                .clone()
            )
        );
        assert_eq!(step.prerequisites().len(), 1);
    }

    #[test]
    fn reports_missing_requirements_unsupported_cases_and_duplicate_inputs() {
        let missing = plan(
            &desired(),
            &delta(vec![item(
                DeltaKind::UnsatisfiedCondition,
                RequiredOutcomeKind::DomainChange,
                "item-missing",
            )]),
            &[],
            &PlannerRules::default(),
        )
        .unwrap();
        assert!(missing.plan().is_none());
        assert_eq!(
            missing.diagnostics()[0].code(),
            PlannerDiagnosticCode::MissingCapabilityRequirement
        );
        assert!(missing.diagnostics()[0].is_blocking());
        assert!(!missing.is_execution_ready());
        assert_eq!(
            missing.diagnostics()[0].delta_item().unwrap().as_str(),
            "item-missing"
        );

        let unsupported = plan(
            &desired(),
            &delta(vec![item(
                DeltaKind::UnknownState,
                RequiredOutcomeKind::NoOp,
                "item-unsupported",
            )]),
            &[],
            &PlannerRules::default(),
        )
        .unwrap();
        assert_eq!(
            unsupported.diagnostics()[0].code(),
            PlannerDiagnosticCode::UnsupportedDeltaOutcome
        );
        assert!(unsupported.plan().is_none());

        let duplicate = requirement("item-missing", "req-duplicate", "domain.change");
        assert!(matches!(
            plan(
                &desired(),
                &delta(vec![item(
                    DeltaKind::UnsatisfiedCondition,
                    RequiredOutcomeKind::DomainChange,
                    "item-missing",
                )]),
                &[duplicate.clone(), duplicate],
                &PlannerRules::default(),
            ),
            Err(ValidationError::DuplicateDeclarativeIdentity { .. })
        ));
    }

    #[test]
    fn capability_derivation_and_capability_snapshot_apis_fail_closed() {
        let change_item = item(
            DeltaKind::UnsatisfiedCondition,
            RequiredOutcomeKind::DomainChange,
            "item-change",
        );
        let derivation = crate::derive_capability_requirements(
            &delta(vec![change_item]),
            &desired(),
            &[capability("domain.change", CapabilityClass::Mutate)],
            &CapabilityRequirementRules::v1()
                .with_domain_change(CapabilityId::new("domain.change").unwrap()),
        )
        .unwrap();
        let result = plan_from_capability_derivation(
            &desired(),
            &delta(vec![item(
                DeltaKind::UnsatisfiedCondition,
                RequiredOutcomeKind::DomainChange,
                "item-change",
            )]),
            &derivation,
            &PlannerRules::default(),
        )
        .unwrap();
        assert!(result.is_execution_ready());
        assert_eq!(result.plan().unwrap().steps().len(), 1);

        let snapshot_result = plan_from_capabilities(
            &desired(),
            &delta(vec![item(
                DeltaKind::UnsatisfiedCondition,
                RequiredOutcomeKind::DomainChange,
                "item-change",
            )]),
            &[capability("domain.change", CapabilityClass::Mutate)],
            &CapabilityRequirementRules::v1()
                .with_domain_change(CapabilityId::new("domain.change").unwrap()),
            &PlannerRules::default(),
        )
        .unwrap();
        assert!(snapshot_result.is_execution_ready());
    }

    #[test]
    fn public_rules_and_diagnostic_codes_are_stable() {
        let rules = PlannerRules::new(PlanningIrVersion::V1).unwrap();
        assert!(rules.information_before_change());
        assert_eq!(rules.version(), DETERMINISTIC_PLANNER_VERSION);
        assert!(rules.merging_equivalent_requirements());
        assert!(
            !rules
                .clone()
                .allowing_independent_information()
                .with_merge_equivalent_requirements(false)
                .merging_equivalent_requirements()
        );
        assert!(rules.verification_requirement().is_none());
        assert!(rules.verification_lifecycle().is_none());
        assert!(rules.prerequisites().is_empty());
        assert!(PlannerRules::new(PlanningIrVersion::new(2, 0).unwrap()).is_err());
        for code in [
            PlannerRuleCode::NoOp,
            PlannerRuleCode::DomainChange,
            PlannerRuleCode::ViolationRemediation,
            PlannerRuleCode::EvidenceAcquisition,
            PlannerRuleCode::Observation,
            PlannerRuleCode::InputAcquisition,
            PlannerRuleCode::ConflictResolution,
            PlannerRuleCode::Assessment,
            PlannerRuleCode::InformationBeforeChange,
            PlannerRuleCode::VerificationAfterChange,
        ] {
            assert_eq!(PlannerRuleCode::from_str(code.as_str()).unwrap(), code);
            assert_eq!(code.to_string(), code.as_str());
        }
        for code in [
            PlannerDiagnosticCode::MissingCapabilityRequirement,
            PlannerDiagnosticCode::MissingVerificationCapabilityRequirement,
            PlannerDiagnosticCode::UnsupportedDeltaOutcome,
            PlannerDiagnosticCode::CapabilityContractGap,
        ] {
            assert_eq!(
                PlannerDiagnosticCode::from_str(code.as_str()).unwrap(),
                code
            );
            assert_eq!(code.to_string(), code.as_str());
        }
        assert!(PlannerRuleCode::from_str("UNKNOWN").is_err());
        assert!(PlannerDiagnosticCode::from_str("UNKNOWN").is_err());

        let decision = plan(
            &desired(),
            &delta(vec![item(
                DeltaKind::Satisfied,
                RequiredOutcomeKind::NoOp,
                "item-decision",
            )]),
            &[],
            &PlannerRules::default(),
        )
        .unwrap()
        .decisions()[0]
            .clone();
        assert_eq!(decision.delta_item().as_str(), "item-decision");
        assert!(!decision.rationale().is_empty());
        assert_eq!(decision.version(), DETERMINISTIC_PLANNER_VERSION);

        let diagnostic = PlannerDiagnostic::new(
            PlannerDiagnosticCode::CapabilityContractGap,
            None,
            false,
            "optional capability contract gap",
        )
        .unwrap();
        assert_eq!(diagnostic.version(), DETERMINISTIC_PLANNER_VERSION);
        assert!(diagnostic.delta_item().is_none());
        assert!(!diagnostic.is_blocking());
        assert!(!diagnostic.rationale().is_empty());
    }

    #[test]
    fn merges_equivalent_requirements_only_when_the_rule_allows_it() {
        let requirements = [
            requirement("item-change", "req-a", "domain.change"),
            requirement("item-change", "req-b", "domain.change"),
        ];
        let merged = plan(
            &desired(),
            &delta(vec![item(
                DeltaKind::UnsatisfiedCondition,
                RequiredOutcomeKind::DomainChange,
                "item-change",
            )]),
            &requirements,
            &PlannerRules::default(),
        )
        .unwrap();
        assert_eq!(merged.plan().unwrap().capability_requirements().len(), 1);
        assert_eq!(
            merged.plan().unwrap().steps()[0].capability_requirements(),
            &[crate::CapabilityRequirementId::new("req-a").unwrap()]
        );

        let preserved = plan(
            &desired(),
            &delta(vec![item(
                DeltaKind::UnsatisfiedCondition,
                RequiredOutcomeKind::DomainChange,
                "item-change",
            )]),
            &requirements,
            &PlannerRules::default().with_merge_equivalent_requirements(false),
        )
        .unwrap();
        assert_eq!(preserved.plan().unwrap().capability_requirements().len(), 2);
        assert_eq!(
            preserved.plan().unwrap().steps()[0].capability_requirements(),
            &[
                crate::CapabilityRequirementId::new("req-a").unwrap(),
                crate::CapabilityRequirementId::new("req-b").unwrap()
            ]
        );
    }

    #[test]
    fn covers_remaining_rule_surfaces_and_fail_closed_boundaries() {
        let prerequisite = PlanCondition::outcome(
            RequiredOutcome::new(RequiredOutcomeKind::Assessment, "precondition").unwrap(),
        );
        assert!(
            PlannerRules::v1()
                .with_prerequisites(vec![prerequisite.clone(), prerequisite.clone()])
                .is_err()
        );
        let rules = PlannerRules::v1()
            .requiring_information_before_change()
            .with_verification_lifecycle(
                LifecycleRequirement::new(
                    LifecycleRequirementKind::HumanInput,
                    "human review is required",
                )
                .unwrap(),
            )
            .with_prerequisite(prerequisite.clone())
            .unwrap();
        assert!(rules.information_before_change());
        assert_eq!(
            rules.verification_lifecycle().unwrap().kind(),
            LifecycleRequirementKind::HumanInput
        );
        assert_eq!(rules.prerequisites(), &[prerequisite]);

        let items = vec![
            item(
                DeltaKind::UnsatisfiedCondition,
                RequiredOutcomeKind::DomainChange,
                "item-change",
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
                DeltaKind::UnknownState,
                RequiredOutcomeKind::Assessment,
                "item-assessment",
            ),
        ];
        let planning_delta = delta(items);
        let verification = requirement("item-change", "req-verification", "state.verify");
        let result = plan(
            &desired(),
            &planning_delta,
            &[
                requirement("item-change", "req-change", "domain.change"),
                requirement("item-input", "req-input", "input.acquire"),
                requirement("item-conflict", "req-conflict", "conflict.resolve"),
                requirement("item-assessment", "req-assessment", "state.assess"),
            ],
            &rules.with_verification_requirement(verification),
        )
        .unwrap();
        let plan = result.plan().unwrap();
        assert_eq!(plan.steps().len(), 5);
        assert!(
            plan.steps()
                .iter()
                .any(|step| step.kind() == PlanStepKind::InputAcquisition)
        );
        assert!(
            plan.steps()
                .iter()
                .any(|step| step.kind() == PlanStepKind::ConflictResolution)
        );
        assert!(
            plan.steps()
                .iter()
                .any(|step| step.kind() == PlanStepKind::Verification)
        );
        assert!(
            plan.steps()
                .iter()
                .all(|step| step.prerequisites().len() == 1)
        );

        assert_eq!(
            step_kind(RequiredOutcomeKind::InputAcquisition),
            Some(PlanStepKind::InputAcquisition)
        );
        assert_eq!(
            step_kind(RequiredOutcomeKind::ConflictResolution),
            Some(PlanStepKind::ConflictResolution)
        );
        assert_eq!(
            step_kind(RequiredOutcomeKind::Assessment),
            Some(PlanStepKind::Verification)
        );
        assert_eq!(
            planner_rule(DeltaKind::Satisfied, RequiredOutcomeKind::NoOp),
            PlannerRuleCode::NoOp
        );
        assert_eq!(
            planner_rule(DeltaKind::Violation, RequiredOutcomeKind::DomainChange),
            PlannerRuleCode::ViolationRemediation
        );
        assert_eq!(
            planner_rule(
                DeltaKind::UnresolvedInput,
                RequiredOutcomeKind::InputAcquisition
            ),
            PlannerRuleCode::InputAcquisition
        );
        assert_eq!(
            planner_rule(DeltaKind::Conflict, RequiredOutcomeKind::ConflictResolution),
            PlannerRuleCode::ConflictResolution
        );
        assert_eq!(
            planner_rule(DeltaKind::UnknownState, RequiredOutcomeKind::Assessment),
            PlannerRuleCode::Assessment
        );

        let base_delta = delta(vec![item(
            DeltaKind::UnsatisfiedCondition,
            RequiredOutcomeKind::DomainChange,
            "item-change",
        )]);
        let derivation = crate::derive_capability_requirements(
            &base_delta,
            &desired(),
            &[capability("domain.change", CapabilityClass::Mutate)],
            &CapabilityRequirementRules::v1()
                .with_domain_change(CapabilityId::new("domain.change").unwrap()),
        )
        .unwrap();
        let wrong_delta = Delta::new(
            DeltaId::new("delta-2").unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            Some(crate::SituationId::new("situation-1").unwrap()),
            vec![item(
                DeltaKind::UnsatisfiedCondition,
                RequiredOutcomeKind::DomainChange,
                "item-change",
            )],
        )
        .unwrap();
        assert!(matches!(
            plan_from_capability_derivation(
                &desired(),
                &wrong_delta,
                &derivation,
                &PlannerRules::default(),
            ),
            Err(ValidationError::InvalidStateCombination { .. })
        ));
        assert!(matches!(
            plan_from_capability_derivation(
                &desired_with_id("desired-2"),
                &base_delta,
                &derivation,
                &PlannerRules::default(),
            ),
            Err(ValidationError::InvalidStateCombination { .. })
        ));
        let alias = derive_plan(
            &base_delta,
            &desired(),
            derivation.requirements(),
            &PlannerRules::default(),
        )
        .unwrap();
        assert!(alias.is_execution_ready());
    }

    #[test]
    fn rejects_invalid_verification_requirement_and_long_ids_remain_valid() {
        let verification = requirement("not-in-delta", "req-verify", "state.verify");
        assert!(matches!(
            plan(
                &desired(),
                &delta(Vec::new()),
                &[],
                &PlannerRules::default().with_verification_requirement(verification),
            ),
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "delta_item",
                ..
            })
        ));

        let long_id = "a".repeat(128);
        let result = plan(
            &desired(),
            &delta(vec![item(
                DeltaKind::UnsatisfiedCondition,
                RequiredOutcomeKind::DomainChange,
                &long_id,
            )]),
            &[requirement(&long_id, "req-long", "domain.change")],
            &PlannerRules::default(),
        )
        .unwrap();
        assert!(result.plan().unwrap().steps()[0].id().as_str().len() <= 128);
    }

    #[test]
    fn propagates_capability_gaps_without_fabricating_a_plan() {
        let derivation = crate::derive_capability_requirements(
            &delta(vec![item(
                DeltaKind::UnknownState,
                RequiredOutcomeKind::Observation,
                "item-gap",
            )]),
            &desired(),
            &[],
            &CapabilityRequirementRules::v1()
                .with_observation(CapabilityId::new("state.observe").unwrap()),
        )
        .unwrap();
        let result = plan_from_capability_derivation(
            &desired(),
            &delta(vec![item(
                DeltaKind::UnknownState,
                RequiredOutcomeKind::Observation,
                "item-gap",
            )]),
            &derivation,
            &PlannerRules::default(),
        )
        .unwrap();
        assert!(result.plan().is_none());
        assert!(result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == PlannerDiagnosticCode::CapabilityContractGap));
        assert!(!result.is_execution_ready());
    }
}
