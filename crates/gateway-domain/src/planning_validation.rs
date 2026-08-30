//! Deterministic validation reports for Delta and declarative Plan artifacts.
//!
//! Construction already rejects malformed local values.  This module adds the
//! cross-artifact validation boundary used before CG-08 resolution and keeps
//! every rejected condition inspectable through a stable machine code.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Delta, DeltaItemId, DesiredState, NonEmptyText, Plan, PlanCondition, PlanStep, PlanStepId,
    PlanStepKind, PlannerDiagnostic, PlannerResult, RequiredOutcomeKind, ValidationError,
};

/// Version of the validation and explainability diagnostics contract.
pub const PLANNING_VALIDATION_VERSION: crate::PlanningIrVersion = crate::PlanningIrVersion::V1;

/// Stable machine-readable validation reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PlanningValidationDiagnosticCode {
    DuplicateIdentity,
    DanglingReference,
    MissingMandatoryCapability,
    MissingCompletionCondition,
    MissingVerificationCondition,
    DependencyCycle,
    SelfDependency,
    InvalidOrderingPrerequisite,
    ContradictoryOutcome,
    MutuallyExclusiveOutcome,
    UnsupportedVersion,
    ConcreteSelection,
    UnsupportedPlanningGap,
    UntraceableStep,
}

impl PlanningValidationDiagnosticCode {
    /// Returns the stable machine-readable code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateIdentity => "DUPLICATE_IDENTITY",
            Self::DanglingReference => "DANGLING_REFERENCE",
            Self::MissingMandatoryCapability => "MISSING_MANDATORY_CAPABILITY",
            Self::MissingCompletionCondition => "MISSING_COMPLETION_CONDITION",
            Self::MissingVerificationCondition => "MISSING_VERIFICATION_CONDITION",
            Self::DependencyCycle => "DEPENDENCY_CYCLE",
            Self::SelfDependency => "SELF_DEPENDENCY",
            Self::InvalidOrderingPrerequisite => "INVALID_ORDERING_PREREQUISITE",
            Self::ContradictoryOutcome => "CONTRADICTORY_OUTCOME",
            Self::MutuallyExclusiveOutcome => "MUTUALLY_EXCLUSIVE_OUTCOME",
            Self::UnsupportedVersion => "UNSUPPORTED_VERSION",
            Self::ConcreteSelection => "CONCRETE_SELECTION",
            Self::UnsupportedPlanningGap => "UNSUPPORTED_PLANNING_GAP",
            Self::UntraceableStep => "UNTRACEABLE_STEP",
        }
    }
}

impl std::fmt::Display for PlanningValidationDiagnosticCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for PlanningValidationDiagnosticCode {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "DUPLICATE_IDENTITY" => Ok(Self::DuplicateIdentity),
            "DANGLING_REFERENCE" => Ok(Self::DanglingReference),
            "MISSING_MANDATORY_CAPABILITY" => Ok(Self::MissingMandatoryCapability),
            "MISSING_COMPLETION_CONDITION" => Ok(Self::MissingCompletionCondition),
            "MISSING_VERIFICATION_CONDITION" => Ok(Self::MissingVerificationCondition),
            "DEPENDENCY_CYCLE" => Ok(Self::DependencyCycle),
            "SELF_DEPENDENCY" => Ok(Self::SelfDependency),
            "INVALID_ORDERING_PREREQUISITE" => Ok(Self::InvalidOrderingPrerequisite),
            "CONTRADICTORY_OUTCOME" => Ok(Self::ContradictoryOutcome),
            "MUTUALLY_EXCLUSIVE_OUTCOME" => Ok(Self::MutuallyExclusiveOutcome),
            "UNSUPPORTED_VERSION" => Ok(Self::UnsupportedVersion),
            "CONCRETE_SELECTION" => Ok(Self::ConcreteSelection),
            "UNSUPPORTED_PLANNING_GAP" => Ok(Self::UnsupportedPlanningGap),
            "UNTRACEABLE_STEP" => Ok(Self::UntraceableStep),
            value => Err(ValidationError::UnknownDomainValue {
                field: "planning_validation_diagnostic_code",
                value: value.to_owned(),
            }),
        }
    }
}

/// One deterministic, human-readable validation diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PlanningValidationDiagnostic {
    code: PlanningValidationDiagnosticCode,
    subject: Option<String>,
    rationale: NonEmptyText,
}

impl PlanningValidationDiagnostic {
    fn new(
        code: PlanningValidationDiagnosticCode,
        subject: Option<String>,
        rationale: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            code,
            subject,
            rationale: NonEmptyText::new_for_field(rationale, "planning_validation.rationale")?,
        })
    }

    /// Returns the stable machine-readable reason.
    #[must_use]
    pub const fn code(&self) -> PlanningValidationDiagnosticCode {
        self.code
    }

    /// Returns the affected Plan, step or Delta identity when applicable.
    #[must_use]
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }

    /// Returns the deterministic explanation.
    #[must_use]
    pub fn rationale(&self) -> &str {
        self.rationale.as_str()
    }
}

/// Complete deterministic validation output for a planning artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningValidationReport {
    version: crate::PlanningIrVersion,
    diagnostics: Vec<PlanningValidationDiagnostic>,
}

impl PlanningValidationReport {
    fn new(mut diagnostics: Vec<PlanningValidationDiagnostic>) -> Self {
        diagnostics.sort_by(|left, right| {
            left.subject
                .cmp(&right.subject)
                .then(left.code.cmp(&right.code))
                .then(left.rationale.cmp(&right.rationale))
        });
        Self {
            version: PLANNING_VALIDATION_VERSION,
            diagnostics,
        }
    }

    /// Returns the validation report version.
    #[must_use]
    pub const fn version(&self) -> crate::PlanningIrVersion {
        self.version
    }

    /// Returns diagnostics in stable subject/code order.
    #[must_use]
    pub fn diagnostics(&self) -> &[PlanningValidationDiagnostic] {
        &self.diagnostics
    }

    /// Returns whether the artifact is safe to pass to CG-08 resolution.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Validates a Plan against its originating DesiredState and Delta.
pub fn validate_plan(
    plan: &Plan,
    desired_state: &DesiredState,
    delta: &Delta,
) -> PlanningValidationReport {
    let mut diagnostics = Vec::new();
    if let Err(error) = plan.validate_against_delta(delta) {
        push_validation_error(&mut diagnostics, error);
    }
    if let Err(error) = plan.validate_against_desired_state(desired_state) {
        push_validation_error(&mut diagnostics, error);
    }

    let known_requirement_ids = plan
        .capability_requirements()
        .iter()
        .map(|requirement| requirement.id().clone())
        .collect::<BTreeSet<_>>();
    let known_delta_item_ids = delta
        .items()
        .iter()
        .map(|item| item.id().clone())
        .collect::<BTreeSet<_>>();
    let known_step_ids = plan
        .steps()
        .iter()
        .map(|step| step.id().clone())
        .collect::<BTreeSet<_>>();
    for step in plan.steps() {
        validate_step_references(
            step,
            &known_requirement_ids,
            &known_delta_item_ids,
            &known_step_ids,
            desired_state,
            &mut diagnostics,
        );
    }
    validate_delta_coverage(plan, delta, &mut diagnostics);
    validate_outcome_consistency(plan, delta, &mut diagnostics);
    validate_verification_edges(plan, &mut diagnostics);
    PlanningValidationReport::new(diagnostics)
}

/// Validates a complete planner result, retaining unsupported planning gaps.
pub fn validate_planner_result(
    desired_state: &DesiredState,
    delta: &Delta,
    result: &PlannerResult,
) -> PlanningValidationReport {
    let mut diagnostics = result
        .diagnostics()
        .iter()
        .map(planner_diagnostic)
        .collect::<Vec<_>>();
    if let Some(plan) = result.plan() {
        diagnostics.extend(validate_plan(plan, desired_state, delta).diagnostics);
    }
    PlanningValidationReport::new(diagnostics)
}

impl Plan {
    /// Returns a complete cross-artifact validation report.
    #[must_use]
    pub fn validation_report(
        &self,
        desired_state: &DesiredState,
        delta: &Delta,
    ) -> PlanningValidationReport {
        validate_plan(self, desired_state, delta)
    }

    /// Fails closed when a Plan is not safe for downstream resolution.
    pub fn validate_for_resolution(
        &self,
        desired_state: &DesiredState,
        delta: &Delta,
    ) -> Result<(), ValidationError> {
        if self.validation_report(desired_state, delta).is_valid() {
            Ok(())
        } else {
            Err(ValidationError::InvalidStateCombination {
                reason: "Plan failed deterministic resolution validation",
            })
        }
    }
}

fn validate_step_references(
    step: &PlanStep,
    requirement_ids: &BTreeSet<crate::CapabilityRequirementId>,
    delta_item_ids: &BTreeSet<DeltaItemId>,
    step_ids: &BTreeSet<PlanStepId>,
    desired_state: &DesiredState,
    diagnostics: &mut Vec<PlanningValidationDiagnostic>,
) {
    for dependency in step.dependencies() {
        if dependency == step.id() {
            push(
                diagnostics,
                PlanningValidationDiagnosticCode::SelfDependency,
                Some(step.id().to_string()),
                format!("PlanStep {} depends on itself", step.id()),
            );
        } else if !step_ids.contains(dependency) {
            push(
                diagnostics,
                PlanningValidationDiagnosticCode::DanglingReference,
                Some(step.id().to_string()),
                format!(
                    "PlanStep {} references missing dependency {}",
                    step.id(),
                    dependency
                ),
            );
        }
    }
    for requirement in step.capability_requirements() {
        if !requirement_ids.contains(requirement) {
            push(
                diagnostics,
                PlanningValidationDiagnosticCode::DanglingReference,
                Some(step.id().to_string()),
                format!(
                    "PlanStep {} references missing capability requirement {}",
                    step.id(),
                    requirement
                ),
            );
        }
    }
    for delta_item in step.delta_items() {
        if !delta_item_ids.contains(delta_item) {
            push(
                diagnostics,
                PlanningValidationDiagnosticCode::DanglingReference,
                Some(step.id().to_string()),
                format!(
                    "PlanStep {} traces missing Delta item {}",
                    step.id(),
                    delta_item
                ),
            );
        }
    }
    for condition in step.prerequisites() {
        validate_condition_reference(condition, desired_state, step.id(), diagnostics);
        if condition == step.completion() {
            push(
                diagnostics,
                PlanningValidationDiagnosticCode::InvalidOrderingPrerequisite,
                Some(step.id().to_string()),
                format!(
                    "PlanStep {} contains a self-satisfying ordering condition",
                    step.id()
                ),
            );
        }
    }
    validate_condition_reference(step.completion(), desired_state, step.id(), diagnostics);
    if let Some(verification) = step.verification() {
        validate_condition_reference(verification, desired_state, step.id(), diagnostics);
    }
}

fn validate_condition_reference(
    condition: &PlanCondition,
    desired_state: &DesiredState,
    step_id: &PlanStepId,
    diagnostics: &mut Vec<PlanningValidationDiagnostic>,
) {
    if let PlanCondition::DesiredCondition(condition_id) = condition
        && !desired_state
            .conditions()
            .iter()
            .any(|condition| condition.id() == condition_id)
    {
        push(
            diagnostics,
            PlanningValidationDiagnosticCode::DanglingReference,
            Some(step_id.to_string()),
            format!(
                "PlanStep {} references missing DesiredCondition {}",
                step_id, condition_id
            ),
        );
    }
}

fn validate_delta_coverage(
    plan: &Plan,
    delta: &Delta,
    diagnostics: &mut Vec<PlanningValidationDiagnostic>,
) {
    for item in delta.actionable_items() {
        let matching = plan
            .steps()
            .iter()
            .filter(|step| step.delta_items().contains(item.id()))
            .collect::<Vec<_>>();
        if matching.is_empty() {
            push(
                diagnostics,
                PlanningValidationDiagnosticCode::MissingCompletionCondition,
                Some(item.id().to_string()),
                format!(
                    "actionable Delta item {} has no traceable PlanStep",
                    item.id()
                ),
            );
            push(
                diagnostics,
                PlanningValidationDiagnosticCode::MissingMandatoryCapability,
                Some(item.id().to_string()),
                format!(
                    "Delta item {} has no PlanStep carrying its mandatory capability requirement",
                    item.id()
                ),
            );
            continue;
        }
        let mandatory_requirement_ids = plan
            .capability_requirements()
            .iter()
            .filter(|requirement| {
                requirement.originating_delta_item() == item.id()
                    && requirement.cardinality() == crate::RequirementCardinality::Mandatory
            })
            .map(|requirement| requirement.id())
            .collect::<BTreeSet<_>>();
        if !matching.iter().any(|step| {
            step.capability_requirements()
                .iter()
                .any(|requirement| mandatory_requirement_ids.contains(requirement))
        }) {
            push(
                diagnostics,
                PlanningValidationDiagnosticCode::MissingMandatoryCapability,
                Some(item.id().to_string()),
                format!("Delta item {} has no capability-backed PlanStep", item.id()),
            );
        } else if !matching
            .iter()
            .any(|step| step.kind() == expected_step_kind(item.required_outcome().kind()))
        {
            push(
                diagnostics,
                PlanningValidationDiagnosticCode::UntraceableStep,
                Some(item.id().to_string()),
                format!(
                    "Delta item {} has no PlanStep with its required outcome",
                    item.id()
                ),
            );
        }
    }
}

fn validate_outcome_consistency(
    plan: &Plan,
    delta: &Delta,
    diagnostics: &mut Vec<PlanningValidationDiagnostic>,
) {
    let mut outcomes = BTreeMap::<DeltaItemId, BTreeSet<RequiredOutcomeKind>>::new();
    for step in plan.steps() {
        if step.kind() == PlanStepKind::Verification {
            continue;
        }
        for item in step.delta_items() {
            outcomes
                .entry(item.clone())
                .or_default()
                .insert(step.outcome().kind());
        }
    }
    for (item_id, kinds) in &outcomes {
        if kinds.len() > 1 {
            push(
                diagnostics,
                PlanningValidationDiagnosticCode::ContradictoryOutcome,
                Some(item_id.to_string()),
                format!(
                    "Delta item {} is assigned mutually contradictory outcomes",
                    item_id
                ),
            );
        }
    }
    for item in delta.items() {
        if let Some(kinds) = outcomes.get(item.id())
            && kinds
                .iter()
                .any(|kind| *kind != item.required_outcome().kind())
        {
            push(
                diagnostics,
                PlanningValidationDiagnosticCode::MutuallyExclusiveOutcome,
                Some(item.id().to_string()),
                format!(
                    "Plan outcome for Delta item {} does not match the Delta contract",
                    item.id()
                ),
            );
        }
    }
}

fn validate_verification_edges(plan: &Plan, diagnostics: &mut Vec<PlanningValidationDiagnostic>) {
    for step in plan.steps() {
        let Some(lifecycle) = step.lifecycle_requirement() else {
            continue;
        };
        if lifecycle.kind() != crate::LifecycleRequirementKind::VerificationAfterChange
            || step.kind() == PlanStepKind::Verification
        {
            continue;
        }
        if !plan.steps().iter().any(|candidate| {
            candidate.kind() == PlanStepKind::Verification
                && candidate.dependencies().contains(step.id())
        }) {
            push(
                diagnostics,
                PlanningValidationDiagnosticCode::MissingVerificationCondition,
                Some(step.id().to_string()),
                format!(
                    "PlanStep {} requires verification but has no dependent verification step",
                    step.id()
                ),
            );
        }
    }
}

fn expected_step_kind(outcome: RequiredOutcomeKind) -> PlanStepKind {
    match outcome {
        RequiredOutcomeKind::DomainChange => PlanStepKind::Change,
        RequiredOutcomeKind::EvidenceAcquisition => PlanStepKind::EvidenceAcquisition,
        RequiredOutcomeKind::Observation => PlanStepKind::Observation,
        RequiredOutcomeKind::InputAcquisition => PlanStepKind::InputAcquisition,
        RequiredOutcomeKind::ConflictResolution => PlanStepKind::ConflictResolution,
        RequiredOutcomeKind::Assessment | RequiredOutcomeKind::NoOp => PlanStepKind::Verification,
    }
}

fn planner_diagnostic(diagnostic: &PlannerDiagnostic) -> PlanningValidationDiagnostic {
    let code = if diagnostic.is_blocking() {
        PlanningValidationDiagnosticCode::UnsupportedPlanningGap
    } else {
        PlanningValidationDiagnosticCode::UntraceableStep
    };
    PlanningValidationDiagnostic::new(
        code,
        diagnostic.delta_item().map(ToString::to_string),
        format!("{}: {}", diagnostic.code(), diagnostic.rationale()),
    )
    .expect("planner diagnostics always carry validated rationale text")
}

fn push(
    diagnostics: &mut Vec<PlanningValidationDiagnostic>,
    code: PlanningValidationDiagnosticCode,
    subject: Option<String>,
    rationale: impl Into<String>,
) {
    diagnostics.push(
        PlanningValidationDiagnostic::new(code, subject, rationale)
            .expect("validation diagnostics always carry non-empty rationale"),
    );
}

fn push_validation_error(
    diagnostics: &mut Vec<PlanningValidationDiagnostic>,
    error: ValidationError,
) {
    let code = match error {
        ValidationError::CircularRelationship { .. } => {
            PlanningValidationDiagnosticCode::DependencyCycle
        }
        ValidationError::SelfReference { .. } => PlanningValidationDiagnosticCode::SelfDependency,
        ValidationError::UnsupportedSchemaVersion { .. } => {
            PlanningValidationDiagnosticCode::UnsupportedVersion
        }
        ValidationError::DuplicateDeclarativeIdentity { .. }
        | ValidationError::DuplicateRelationship { .. } => {
            PlanningValidationDiagnosticCode::DuplicateIdentity
        }
        ValidationError::MissingDeclarativeIdentity { .. }
        | ValidationError::InvalidStateCombination { .. } => {
            PlanningValidationDiagnosticCode::DanglingReference
        }
        _ => PlanningValidationDiagnosticCode::UntraceableStep,
    };
    push(diagnostics, code, None, error.to_string());
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::str::FromStr;

    use crate::{
        CapabilityRequirement, ComparisonOperator, ConditionExpression, CurrentStateId, DeltaBasis,
        DeltaId, DeltaItem, DeltaItemId, DeltaKind, DesiredCondition, DesiredState, DesiredStateId,
        LifecycleRequirement, LifecycleRequirementKind, PlanCondition, PlanId, PlanStep,
        PlanStepId, PlanStepKind, RequiredOutcome, RequiredOutcomeKind, SubjectPath, TypedValue,
        ValidationError,
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
                    Some(TypedValue::symbol("healthy").unwrap()),
                )
                .unwrap(),
            ],
            ConditionExpression::condition(crate::ConditionId::new("condition-1").unwrap()),
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    fn desired_with_id(id: &str) -> DesiredState {
        DesiredState::new(
            DesiredStateId::new(id).unwrap(),
            vec![
                DesiredCondition::new(
                    crate::ConditionId::new("condition-1").unwrap(),
                    SubjectPath::from_str("service.status").unwrap(),
                    ComparisonOperator::Equals,
                    Some(TypedValue::symbol("healthy").unwrap()),
                )
                .unwrap(),
            ],
            ConditionExpression::condition(crate::ConditionId::new("condition-1").unwrap()),
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    fn item() -> DeltaItem {
        DeltaItem::new(
            crate::DeltaItemId::new("item-1").unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            crate::ConditionId::new("condition-1").unwrap(),
            DeltaKind::UnsatisfiedCondition,
            DeltaBasis::new(
                None,
                Some(CurrentStateId::new("state-1").unwrap()),
                vec![SubjectPath::from_str("service.status").unwrap()],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap(),
            RequiredOutcome::new(RequiredOutcomeKind::DomainChange, "change state").unwrap(),
            "validation item",
        )
        .unwrap()
    }

    fn requirement() -> CapabilityRequirement {
        CapabilityRequirement::new(
            crate::CapabilityRequirementId::new("requirement-1").unwrap(),
            crate::CapabilityId::new("domain.change").unwrap(),
            crate::RequirementCardinality::Mandatory,
            crate::DeltaItemId::new("item-1").unwrap(),
            "validation requirement",
        )
        .unwrap()
    }

    fn delta() -> Delta {
        Delta::new(
            DeltaId::new("delta-1").unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            None,
            vec![item()],
        )
        .unwrap()
    }

    fn delta_with_id(id: &str) -> Delta {
        Delta::new(
            DeltaId::new(id).unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            None,
            vec![item()],
        )
        .unwrap()
    }

    fn plan(step: PlanStep) -> Plan {
        Plan::new(
            PlanId::new("plan-1").unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            DeltaId::new("delta-1").unwrap(),
            vec![requirement()],
            vec![step],
        )
        .unwrap()
    }

    fn plan_with_steps(steps: Vec<PlanStep>) -> Plan {
        Plan::new(
            PlanId::new("plan-1").unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            DeltaId::new("delta-1").unwrap(),
            vec![requirement()],
            steps,
        )
        .unwrap()
    }

    fn step() -> PlanStep {
        PlanStep::new(
            crate::PlanStepId::new("step-1").unwrap(),
            PlanStepKind::Change,
            RequiredOutcome::new(RequiredOutcomeKind::DomainChange, "change state").unwrap(),
            PlanCondition::desired_condition(crate::ConditionId::new("condition-1").unwrap()),
            "validation step",
        )
        .unwrap()
        .with_capability_requirements(vec![
            crate::CapabilityRequirementId::new("requirement-1").unwrap(),
        ])
        .unwrap()
        .with_delta_items(vec![crate::DeltaItemId::new("item-1").unwrap()])
        .unwrap()
    }

    fn step_with_kind_and_outcome(
        id: &str,
        kind: PlanStepKind,
        outcome: RequiredOutcome,
        completion: PlanCondition,
    ) -> PlanStep {
        PlanStep::new(
            PlanStepId::new(id).unwrap(),
            kind,
            outcome,
            completion,
            "additional validation step",
        )
        .unwrap()
        .with_capability_requirements(vec![
            crate::CapabilityRequirementId::new("requirement-1").unwrap(),
        ])
        .unwrap()
        .with_delta_items(vec![DeltaItemId::new("item-1").unwrap()])
        .unwrap()
    }

    #[test]
    fn valid_plan_has_an_empty_report_and_resolution_boundary() {
        let report = validate_plan(&plan(step()), &desired(), &delta());
        assert!(report.is_valid());
        assert_eq!(report.version(), PLANNING_VALIDATION_VERSION);
        assert!(
            plan(step())
                .validate_for_resolution(&desired(), &delta())
                .is_ok()
        );
    }

    #[test]
    fn reports_missing_trace_capability_and_verification_contracts() {
        let empty = Plan::new(
            PlanId::new("plan-1").unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            DeltaId::new("delta-1").unwrap(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        let report = validate_plan(&empty, &desired(), &delta());
        assert!(report.diagnostics().iter().any(|diagnostic| {
            diagnostic.code() == PlanningValidationDiagnosticCode::MissingCompletionCondition
        }));
        assert!(
            report
                .diagnostics()
                .iter()
                .all(|diagnostic| { !diagnostic.rationale().is_empty() })
        );

        let verification_required = step().with_lifecycle_requirement(
            crate::LifecycleRequirement::new(
                crate::LifecycleRequirementKind::VerificationAfterChange,
                "verify change",
            )
            .unwrap(),
        );
        let report = validate_plan(&plan(verification_required), &desired(), &delta());
        assert!(report.diagnostics().iter().any(|diagnostic| {
            diagnostic.code() == PlanningValidationDiagnosticCode::MissingVerificationCondition
        }));
    }

    #[test]
    fn stable_codes_and_planner_gaps_are_machine_readable() {
        for code in [
            PlanningValidationDiagnosticCode::DuplicateIdentity,
            PlanningValidationDiagnosticCode::DanglingReference,
            PlanningValidationDiagnosticCode::MissingMandatoryCapability,
            PlanningValidationDiagnosticCode::MissingCompletionCondition,
            PlanningValidationDiagnosticCode::MissingVerificationCondition,
            PlanningValidationDiagnosticCode::DependencyCycle,
            PlanningValidationDiagnosticCode::SelfDependency,
            PlanningValidationDiagnosticCode::InvalidOrderingPrerequisite,
            PlanningValidationDiagnosticCode::ContradictoryOutcome,
            PlanningValidationDiagnosticCode::MutuallyExclusiveOutcome,
            PlanningValidationDiagnosticCode::UnsupportedVersion,
            PlanningValidationDiagnosticCode::ConcreteSelection,
            PlanningValidationDiagnosticCode::UnsupportedPlanningGap,
            PlanningValidationDiagnosticCode::UntraceableStep,
        ] {
            assert_eq!(
                code.to_string()
                    .parse::<PlanningValidationDiagnosticCode>()
                    .unwrap(),
                code
            );
        }
        assert!(
            "UNKNOWN"
                .parse::<PlanningValidationDiagnosticCode>()
                .is_err()
        );
        let planner =
            crate::plan(&desired(), &delta(), &[], &crate::PlannerRules::default()).unwrap();
        let report = validate_planner_result(&desired(), &delta(), &planner);
        assert!(!report.is_valid());
        assert_eq!(
            report.diagnostics()[0].code(),
            PlanningValidationDiagnosticCode::UnsupportedPlanningGap
        );
    }

    #[test]
    fn validates_reference_ordering_and_condition_diagnostics() {
        let raw = PlanStep::new(
            PlanStepId::new("raw-step").unwrap(),
            PlanStepKind::Change,
            RequiredOutcome::new(RequiredOutcomeKind::DomainChange, "change").unwrap(),
            PlanCondition::desired_condition(crate::ConditionId::new("condition-1").unwrap()),
            "raw validation step",
        )
        .unwrap()
        .with_dependencies(vec![PlanStepId::new("missing-step").unwrap()])
        .unwrap()
        .with_capability_requirements(vec![
            crate::CapabilityRequirementId::new("missing-requirement").unwrap(),
        ])
        .unwrap()
        .with_delta_items(vec![DeltaItemId::new("missing-item").unwrap()])
        .unwrap();
        let mut diagnostics = Vec::new();
        validate_step_references(
            &raw,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            &desired(),
            &mut diagnostics,
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code() == PlanningValidationDiagnosticCode::DanglingReference
                && diagnostic.subject() == Some("raw-step")
        }));

        let missing_condition = PlanStep::new(
            PlanStepId::new("missing-condition-step").unwrap(),
            PlanStepKind::Observation,
            RequiredOutcome::new(RequiredOutcomeKind::Observation, "observe").unwrap(),
            PlanCondition::desired_condition(crate::ConditionId::new("missing-condition").unwrap()),
            "missing condition step",
        )
        .unwrap()
        .with_verification(PlanCondition::desired_condition(
            crate::ConditionId::new("missing-verification").unwrap(),
        ));
        let mut missing_condition_diagnostics = Vec::new();
        validate_step_references(
            &missing_condition,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::from([PlanStepId::new("missing-condition-step").unwrap()]),
            &desired(),
            &mut missing_condition_diagnostics,
        );
        assert_eq!(missing_condition_diagnostics.len(), 2);

        let completion =
            PlanCondition::desired_condition(crate::ConditionId::new("condition-1").unwrap());
        let ordered = PlanStep::new(
            PlanStepId::new("ordered-step").unwrap(),
            PlanStepKind::Change,
            RequiredOutcome::new(RequiredOutcomeKind::DomainChange, "change").unwrap(),
            completion.clone(),
            "invalid ordering step",
        )
        .unwrap()
        .with_prerequisites(vec![completion])
        .unwrap();
        let mut ordering_diagnostics = Vec::new();
        validate_step_references(
            &ordered,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            &desired(),
            &mut ordering_diagnostics,
        );
        assert!(ordering_diagnostics.iter().any(|diagnostic| {
            diagnostic.code() == PlanningValidationDiagnosticCode::InvalidOrderingPrerequisite
        }));
    }

    #[test]
    fn reports_cross_artifact_outcome_and_capability_gaps() {
        let wrong_delta_report =
            validate_plan(&plan(step()), &desired(), &delta_with_id("delta-2"));
        assert!(wrong_delta_report.diagnostics().iter().any(|diagnostic| {
            diagnostic.code() == PlanningValidationDiagnosticCode::DanglingReference
        }));

        let wrong_desired_report =
            validate_plan(&plan(step()), &desired_with_id("desired-2"), &delta());
        assert!(wrong_desired_report.diagnostics().iter().any(|diagnostic| {
            diagnostic.code() == PlanningValidationDiagnosticCode::DanglingReference
        }));

        let missing_trace = step()
            .with_delta_items(vec![DeltaItemId::new("missing-item").unwrap()])
            .unwrap();
        let missing_trace_report = validate_plan(&plan(missing_trace), &desired(), &delta());
        assert!(missing_trace_report.diagnostics().iter().any(|diagnostic| {
            diagnostic.code() == PlanningValidationDiagnosticCode::MissingCompletionCondition
        }));

        let missing_mandatory = step().with_capability_requirements(Vec::new()).unwrap();
        let missing_mandatory_report =
            validate_plan(&plan(missing_mandatory), &desired(), &delta());
        assert!(
            missing_mandatory_report
                .diagnostics()
                .iter()
                .any(|diagnostic| {
                    diagnostic.code()
                        == PlanningValidationDiagnosticCode::MissingMandatoryCapability
                })
        );

        let wrong_kind = step_with_kind_and_outcome(
            "wrong-kind",
            PlanStepKind::Observation,
            RequiredOutcome::new(RequiredOutcomeKind::DomainChange, "change").unwrap(),
            PlanCondition::desired_condition(crate::ConditionId::new("condition-1").unwrap()),
        );
        let wrong_kind_report = validate_plan(&plan(wrong_kind), &desired(), &delta());
        assert!(wrong_kind_report.diagnostics().iter().any(|diagnostic| {
            diagnostic.code() == PlanningValidationDiagnosticCode::UntraceableStep
        }));

        let contradictory = step_with_kind_and_outcome(
            "observation",
            PlanStepKind::Observation,
            RequiredOutcome::new(RequiredOutcomeKind::Observation, "observe").unwrap(),
            PlanCondition::outcome(
                RequiredOutcome::new(RequiredOutcomeKind::Observation, "observation recorded")
                    .unwrap(),
            ),
        );
        let contradictory_report = validate_plan(
            &plan_with_steps(vec![step(), contradictory]),
            &desired(),
            &delta(),
        );
        assert!(contradictory_report.diagnostics().iter().any(|diagnostic| {
            diagnostic.code() == PlanningValidationDiagnosticCode::ContradictoryOutcome
        }));
        assert!(contradictory_report.diagnostics().iter().any(|diagnostic| {
            diagnostic.code() == PlanningValidationDiagnosticCode::MutuallyExclusiveOutcome
        }));
    }

    #[test]
    fn accepts_valid_verification_edges_and_retains_planner_validation() {
        let lifecycle = LifecycleRequirement::new(
            LifecycleRequirementKind::VerificationAfterChange,
            "verify change",
        )
        .unwrap();
        let change = step().with_lifecycle_requirement(lifecycle.clone());
        let verification = PlanStep::new(
            PlanStepId::new("verification").unwrap(),
            PlanStepKind::Verification,
            RequiredOutcome::new(RequiredOutcomeKind::Assessment, "verify").unwrap(),
            PlanCondition::outcome(
                RequiredOutcome::new(RequiredOutcomeKind::Assessment, "verified").unwrap(),
            ),
            "dependent verification",
        )
        .unwrap()
        .with_dependencies(vec![PlanStepId::new("step-1").unwrap()])
        .unwrap()
        .with_lifecycle_requirement(lifecycle);
        assert!(
            validate_plan(
                &plan_with_steps(vec![verification, change]),
                &desired(),
                &delta()
            )
            .is_valid()
        );

        let planner = crate::plan(
            &desired(),
            &delta(),
            &[requirement()],
            &crate::PlannerRules::default(),
        )
        .unwrap();
        let mismatched = validate_planner_result(&desired_with_id("desired-2"), &delta(), &planner);
        assert!(!mismatched.is_valid());
    }

    #[test]
    fn covers_versions_expected_kinds_and_error_mapping() {
        for (outcome, expected) in [
            (RequiredOutcomeKind::DomainChange, PlanStepKind::Change),
            (
                RequiredOutcomeKind::EvidenceAcquisition,
                PlanStepKind::EvidenceAcquisition,
            ),
            (RequiredOutcomeKind::Observation, PlanStepKind::Observation),
            (
                RequiredOutcomeKind::InputAcquisition,
                PlanStepKind::InputAcquisition,
            ),
            (
                RequiredOutcomeKind::ConflictResolution,
                PlanStepKind::ConflictResolution,
            ),
            (RequiredOutcomeKind::Assessment, PlanStepKind::Verification),
            (RequiredOutcomeKind::NoOp, PlanStepKind::Verification),
        ] {
            assert_eq!(expected_step_kind(outcome), expected);
        }

        let errors = [
            (
                ValidationError::CircularRelationship { field: "steps" },
                PlanningValidationDiagnosticCode::DependencyCycle,
            ),
            (
                ValidationError::SelfReference { field: "steps" },
                PlanningValidationDiagnosticCode::SelfDependency,
            ),
            (
                ValidationError::UnsupportedSchemaVersion {
                    expected: "1.0",
                    actual: "2.0".to_owned(),
                },
                PlanningValidationDiagnosticCode::UnsupportedVersion,
            ),
            (
                ValidationError::DuplicateDeclarativeIdentity {
                    kind: "step",
                    id: "step-1".to_owned(),
                },
                PlanningValidationDiagnosticCode::DuplicateIdentity,
            ),
            (
                ValidationError::DuplicateRelationship { field: "steps" },
                PlanningValidationDiagnosticCode::DuplicateIdentity,
            ),
            (
                ValidationError::MissingDeclarativeIdentity {
                    kind: "condition",
                    id: "missing".to_owned(),
                },
                PlanningValidationDiagnosticCode::DanglingReference,
            ),
            (
                ValidationError::InvalidStateCombination { reason: "mismatch" },
                PlanningValidationDiagnosticCode::DanglingReference,
            ),
            (
                ValidationError::EmptyText { field: "rationale" },
                PlanningValidationDiagnosticCode::UntraceableStep,
            ),
        ];
        for (error, expected) in errors {
            let mut diagnostics = Vec::new();
            push_validation_error(&mut diagnostics, error);
            assert_eq!(diagnostics[0].code(), expected);
            assert!(diagnostics[0].subject().is_none());
            assert!(!diagnostics[0].rationale().is_empty());
        }

        assert_eq!(
            PlanningValidationDiagnostic::new(
                PlanningValidationDiagnosticCode::DanglingReference,
                Some("subject-1".to_owned()),
                "missing reference",
            )
            .unwrap()
            .subject(),
            Some("subject-1")
        );
    }
}
