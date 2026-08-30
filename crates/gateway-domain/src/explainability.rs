//! Human-readable explainability for deterministic planning results.
//!
//! The explanation is projected from the same Delta, capability requirements,
//! Plan graph and planner decisions that produce execution input.  Evidence is
//! represented by typed lineage references from [`DeltaBasis`]; raw evidence
//! content is never copied into this projection.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::{
    CapabilityRequirementId, ConditionExpression, ConditionId, Delta, DeltaBasis, DeltaItemId,
    DeltaKind, DeltaReasonCode, DesiredCondition, DesiredState, DesiredStateId,
    LifecycleRequirement, NonEmptyText, PlanCondition, PlanStepId, PlannerResult, PlannerRuleCode,
    PlanningIrVersion, RequiredOutcome, ValidationError,
};

/// Version of the deterministic planning explanation contract.
pub const PLAN_EXPLAINABILITY_VERSION: PlanningIrVersion = PlanningIrVersion::V1;

/// A complete explainability projection for one planner result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanExplanation {
    version: PlanningIrVersion,
    desired_state: DesiredStateId,
    delta: crate::DeltaId,
    entries: Vec<PlanExplanationEntry>,
}

impl PlanExplanation {
    fn new(
        desired_state: DesiredStateId,
        delta: crate::DeltaId,
        mut entries: Vec<PlanExplanationEntry>,
    ) -> Self {
        entries.sort_by(|left, right| left.delta_item.cmp(&right.delta_item));
        Self {
            version: PLAN_EXPLAINABILITY_VERSION,
            desired_state,
            delta,
            entries,
        }
    }

    /// Returns the explanation contract version.
    #[must_use]
    pub const fn version(&self) -> PlanningIrVersion {
        self.version
    }

    /// Returns the explained DesiredState identity.
    #[must_use]
    pub fn desired_state(&self) -> &DesiredStateId {
        &self.desired_state
    }

    /// Returns the explained Delta identity.
    #[must_use]
    pub fn delta(&self) -> &crate::DeltaId {
        &self.delta
    }

    /// Returns entries in canonical Delta-item order.
    #[must_use]
    pub fn entries(&self) -> &[PlanExplanationEntry] {
        &self.entries
    }

    /// Renders the same semantic trace as concise human-readable text.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut text = format!(
            "Plan explanation {} for DesiredState {} and Delta {}\n",
            self.version, self.desired_state, self.delta
        );
        for entry in &self.entries {
            let _ = writeln!(
                text,
                "- Delta item {} [{} / {}] -> {}",
                entry.delta_item,
                entry.delta_kind,
                entry.comparison_reason,
                entry.required_outcome.description()
            );
            let _ = writeln!(
                text,
                "  DesiredCondition {} {:?} in expression {:?}",
                entry.condition, entry.desired_condition, entry.desired_expression
            );
            let _ = writeln!(text, "  Delta basis references: {:?}", entry.basis);
            let _ = writeln!(
                text,
                "  CapabilityRequirements: {}",
                display_ids(&entry.capability_requirements)
            );
            let _ = writeln!(
                text,
                "  PlanSteps: {}; dependencies: {}",
                display_ids(&entry.plan_steps),
                display_ids(&entry.dependencies)
            );
            let _ = writeln!(
                text,
                "  Completions: {:?}; verifications: {:?}; lifecycle: {:?}",
                entry.completions, entry.verifications, entry.lifecycle_requirements
            );
            let _ = writeln!(
                text,
                "  Step rationales: {}",
                display_texts(&entry.step_rationales)
            );
            let _ = writeln!(
                text,
                "  Planner rules: {} (version {})",
                entry
                    .planner_rules
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                entry.planner_version
            );
            let _ = writeln!(
                text,
                "  Planner rationales: {}",
                display_texts(&entry.planner_rationales)
            );
            let _ = writeln!(text, "  Rationale: {}", entry.rationale());
        }
        text
    }
}

/// One complete DesiredState-to-Plan trace entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanExplanationEntry {
    delta_item: DeltaItemId,
    desired_state: DesiredStateId,
    condition: ConditionId,
    desired_condition: DesiredCondition,
    desired_expression: ConditionExpression,
    delta_kind: DeltaKind,
    comparison_reason: DeltaReasonCode,
    basis: DeltaBasis,
    required_outcome: RequiredOutcome,
    capability_requirements: Vec<CapabilityRequirementId>,
    plan_steps: Vec<PlanStepId>,
    dependencies: Vec<PlanStepId>,
    completions: Vec<PlanCondition>,
    verifications: Vec<PlanCondition>,
    lifecycle_requirements: Vec<LifecycleRequirement>,
    step_rationales: Vec<NonEmptyText>,
    planner_rules: Vec<PlannerRuleCode>,
    planner_rationales: Vec<NonEmptyText>,
    planner_version: PlanningIrVersion,
    rationale: NonEmptyText,
}

impl PlanExplanationEntry {
    #[allow(clippy::too_many_arguments)]
    fn new(
        item: &crate::DeltaItem,
        desired_condition: DesiredCondition,
        desired_expression: ConditionExpression,
        capability_requirements: Vec<CapabilityRequirementId>,
        plan_steps: Vec<PlanStepId>,
        dependencies: Vec<PlanStepId>,
        completions: Vec<PlanCondition>,
        verifications: Vec<PlanCondition>,
        lifecycle_requirements: Vec<LifecycleRequirement>,
        step_rationales: Vec<NonEmptyText>,
        planner_rules: Vec<PlannerRuleCode>,
        planner_rationales: Vec<NonEmptyText>,
        planner_version: PlanningIrVersion,
    ) -> Result<Self, ValidationError> {
        let plan_step_count = plan_steps.len();
        Ok(Self {
            delta_item: item.id().clone(),
            desired_state: item.desired_state().clone(),
            condition: item.condition().clone(),
            desired_condition,
            desired_expression,
            delta_kind: item.kind(),
            comparison_reason: item.reason(),
            basis: item.basis().clone(),
            required_outcome: item.required_outcome().clone(),
            capability_requirements,
            plan_steps,
            dependencies,
            completions,
            verifications,
            lifecycle_requirements,
            step_rationales,
            planner_rules,
            planner_rationales,
            planner_version,
            rationale: NonEmptyText::new_for_field(
                format!(
                    "Delta item {} ({}) requires {} and is traced through {} PlanStep(s)",
                    item.id(),
                    item.reason(),
                    item.required_outcome().kind(),
                    plan_step_count
                ),
                "plan_explanation.rationale",
            )?,
        })
    }

    /// Returns the Delta item identity.
    #[must_use]
    pub fn delta_item(&self) -> &DeltaItemId {
        &self.delta_item
    }

    /// Returns the DesiredState identity.
    #[must_use]
    pub fn desired_state(&self) -> &DesiredStateId {
        &self.desired_state
    }

    /// Returns the originating DesiredCondition identity.
    #[must_use]
    pub fn condition(&self) -> &ConditionId {
        &self.condition
    }

    /// Returns the complete DesiredCondition record.
    #[must_use]
    pub fn desired_condition(&self) -> &DesiredCondition {
        &self.desired_condition
    }

    /// Returns the full DesiredState expression containing the condition.
    #[must_use]
    pub fn desired_expression(&self) -> &ConditionExpression {
        &self.desired_expression
    }

    /// Returns the Delta classification and comparison reason.
    #[must_use]
    pub const fn delta_kind(&self) -> DeltaKind {
        self.delta_kind
    }

    /// Returns the comparison reason retained by the Delta.
    #[must_use]
    pub const fn comparison_reason(&self) -> DeltaReasonCode {
        self.comparison_reason
    }

    /// Returns typed lineage references without raw evidence content.
    #[must_use]
    pub fn basis(&self) -> &DeltaBasis {
        &self.basis
    }

    /// Returns the required generic outcome.
    #[must_use]
    pub const fn required_outcome(&self) -> &RequiredOutcome {
        &self.required_outcome
    }

    /// Returns abstract capability requirement identities.
    #[must_use]
    pub fn capability_requirements(&self) -> &[CapabilityRequirementId] {
        &self.capability_requirements
    }

    /// Returns traced PlanStep identities.
    #[must_use]
    pub fn plan_steps(&self) -> &[PlanStepId] {
        &self.plan_steps
    }

    /// Returns explicit graph dependencies.
    #[must_use]
    pub fn dependencies(&self) -> &[PlanStepId] {
        &self.dependencies
    }

    /// Returns completion conditions from traced steps.
    #[must_use]
    pub fn completions(&self) -> &[PlanCondition] {
        &self.completions
    }

    /// Returns explicit verification conditions from traced steps.
    #[must_use]
    pub fn verifications(&self) -> &[PlanCondition] {
        &self.verifications
    }

    /// Returns generic lifecycle requirements from traced steps.
    #[must_use]
    pub fn lifecycle_requirements(&self) -> &[LifecycleRequirement] {
        &self.lifecycle_requirements
    }

    /// Returns rationales attached to the traced PlanSteps.
    #[must_use]
    pub fn step_rationales(&self) -> &[NonEmptyText] {
        &self.step_rationales
    }

    /// Returns stable planner rule identities.
    #[must_use]
    pub fn planner_rules(&self) -> &[PlannerRuleCode] {
        &self.planner_rules
    }

    /// Returns rationales attached to the planner rule decisions.
    #[must_use]
    pub fn planner_rationales(&self) -> &[NonEmptyText] {
        &self.planner_rationales
    }

    /// Returns the planner rule version.
    #[must_use]
    pub const fn planner_version(&self) -> PlanningIrVersion {
        self.planner_version
    }

    /// Returns the generated rationale.
    #[must_use]
    pub fn rationale(&self) -> &str {
        self.rationale.as_str()
    }
}

/// Builds an explainability trace from a validated PlannerResult.
pub fn explain_plan(
    desired_state: &DesiredState,
    delta: &Delta,
    result: &PlannerResult,
) -> Result<PlanExplanation, ValidationError> {
    let Some(plan) = result.plan() else {
        return Err(ValidationError::InvalidStateCombination {
            reason: "a PlannerResult without a Plan cannot be explained as executable work",
        });
    };
    plan.validate_for_resolution(desired_state, delta)?;

    let mut entries = Vec::new();
    for item in delta.items() {
        let desired_condition = desired_state
            .conditions()
            .iter()
            .find(|condition| condition.id() == item.condition())
            .cloned()
            .ok_or_else(|| ValidationError::MissingDeclarativeIdentity {
                kind: "condition",
                id: item.condition().to_string(),
            })?;
        let matching = plan
            .steps()
            .iter()
            .filter(|step| step.delta_items().contains(item.id()))
            .collect::<Vec<_>>();
        let mut capability_requirements = BTreeSet::new();
        let mut plan_steps = BTreeSet::new();
        let mut dependencies = BTreeSet::new();
        let mut completions = Vec::new();
        let mut verifications = Vec::new();
        let mut lifecycle_requirements = Vec::new();
        let mut step_rationales = Vec::new();
        for step in matching {
            plan_steps.insert(step.id().clone());
            dependencies.extend(step.dependencies().iter().cloned());
            capability_requirements.extend(step.capability_requirements().iter().cloned());
            completions.push(step.completion().clone());
            step_rationales.push(NonEmptyText::new(step.rationale())?);
            if let Some(verification) = step.verification() {
                verifications.push(verification.clone());
            }
            if let Some(lifecycle) = step.lifecycle_requirement() {
                lifecycle_requirements.push(lifecycle.clone());
            }
        }
        let mut planner_trace = result
            .decisions()
            .iter()
            .filter(|decision| decision.delta_item() == item.id())
            .map(|decision| (decision.rule(), decision.rationale().to_owned()))
            .collect::<Vec<_>>();
        planner_trace.sort();
        planner_trace.dedup();
        if planner_trace.is_empty() {
            return Err(ValidationError::InvalidStateCombination {
                reason: "every Delta item must have an inspectable planner rule decision",
            });
        }
        let planner_rules = planner_trace.iter().map(|(rule, _)| *rule).collect();
        let planner_rationales = planner_trace
            .into_iter()
            .map(|(_, rationale)| NonEmptyText::new(rationale))
            .collect::<Result<Vec<_>, _>>()?;
        entries.push(PlanExplanationEntry::new(
            item,
            desired_condition,
            desired_state.expression().clone(),
            capability_requirements.into_iter().collect(),
            plan_steps.into_iter().collect(),
            dependencies.into_iter().collect(),
            completions,
            verifications,
            lifecycle_requirements,
            step_rationales,
            planner_rules,
            planner_rationales,
            result.version(),
        )?);
    }
    Ok(PlanExplanation::new(
        desired_state.id().clone(),
        delta.id().clone(),
        entries,
    ))
}

fn display_ids<T: std::fmt::Display>(ids: &[T]) -> String {
    if ids.is_empty() {
        "none".to_owned()
    } else {
        ids.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn display_texts(texts: &[NonEmptyText]) -> String {
    if texts.is_empty() {
        "none".to_owned()
    } else {
        texts
            .iter()
            .map(NonEmptyText::as_str)
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        CapabilityId, CapabilityRequirement, ComparisonOperator, ConditionExpression,
        CurrentStateId, DeltaBasis, DeltaId, DeltaItem, DeltaKind, DesiredCondition, DesiredState,
        DesiredStateId, PlanCondition, PlanId, PlanStep, PlanStepId, PlanStepKind, PlannerRules,
        RequiredOutcome, RequiredOutcomeKind, SubjectPath, TypedValue,
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

    fn item() -> DeltaItem {
        DeltaItem::new(
            crate::DeltaItemId::new("item-1").unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            crate::ConditionId::new("condition-1").unwrap(),
            DeltaKind::MissingEvidence,
            DeltaBasis::new(
                Some(crate::SituationId::new("situation-1").unwrap()),
                Some(CurrentStateId::new("state-1").unwrap()),
                vec![SubjectPath::from_str("service.status").unwrap()],
                Vec::new(),
                Vec::new(),
                vec![crate::EvidenceId::new("evidence-1").unwrap()],
                Vec::new(),
                Vec::new(),
            )
            .unwrap(),
            RequiredOutcome::new(RequiredOutcomeKind::EvidenceAcquisition, "acquire evidence")
                .unwrap(),
            "evidence is missing",
        )
        .unwrap()
    }

    fn delta() -> Delta {
        Delta::new(
            DeltaId::new("delta-1").unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            Some(crate::SituationId::new("situation-1").unwrap()),
            vec![item()],
        )
        .unwrap()
    }

    fn result() -> PlannerResult {
        crate::plan(
            &desired(),
            &delta(),
            &[CapabilityRequirement::new(
                crate::CapabilityRequirementId::new("requirement-1").unwrap(),
                CapabilityId::new("evidence.acquire").unwrap(),
                crate::RequirementCardinality::Mandatory,
                crate::DeltaItemId::new("item-1").unwrap(),
                "acquire evidence",
            )
            .unwrap()],
            &PlannerRules::default(),
        )
        .unwrap()
    }

    #[test]
    fn explanation_preserves_the_complete_trace_and_sensitive_references() {
        let explanation = explain_plan(&desired(), &delta(), &result()).unwrap();
        assert_eq!(explanation.version(), PLAN_EXPLAINABILITY_VERSION);
        assert_eq!(explanation.desired_state().as_str(), "desired-1");
        assert_eq!(explanation.delta().as_str(), "delta-1");
        let entry = &explanation.entries()[0];
        assert_eq!(entry.delta_item().as_str(), "item-1");
        assert_eq!(entry.desired_state().as_str(), "desired-1");
        assert_eq!(entry.condition().as_str(), "condition-1");
        assert_eq!(
            entry.desired_condition().subject().to_string(),
            "service.status"
        );
        assert_eq!(entry.delta_kind(), DeltaKind::MissingEvidence);
        assert_eq!(entry.comparison_reason(), DeltaReasonCode::MissingEvidence);
        assert_eq!(entry.basis().evidence()[0].as_str(), "evidence-1");
        assert_eq!(
            entry.required_outcome().kind(),
            RequiredOutcomeKind::EvidenceAcquisition
        );
        assert_eq!(entry.capability_requirements().len(), 1);
        assert_eq!(entry.plan_steps().len(), 1);
        assert!(entry.dependencies().is_empty());
        assert_eq!(entry.completions().len(), 1);
        assert!(entry.verifications().is_empty());
        assert!(entry.lifecycle_requirements().is_empty());
        assert_eq!(entry.step_rationales().len(), 1);
        assert_eq!(
            entry.planner_version(),
            crate::DETERMINISTIC_PLANNER_VERSION
        );
        assert_eq!(
            entry.planner_rules(),
            &[PlannerRuleCode::EvidenceAcquisition]
        );
        assert_eq!(entry.planner_rationales().len(), 1);
        assert!(!entry.rationale().is_empty());
        let text = explanation.to_text();
        assert!(text.contains("DesiredCondition condition-1"));
        assert!(text.contains("evidence-1"));
        assert!(text.contains("MISSING_EVIDENCE"));
        assert!(text.contains("EVIDENCE_ACQUISITION"));
    }

    #[test]
    fn no_plan_cannot_be_explained_as_executable_work() {
        let result = crate::plan(&desired(), &delta(), &[], &PlannerRules::default()).unwrap();
        assert!(matches!(
            explain_plan(&desired(), &delta(), &result),
            Err(ValidationError::InvalidStateCombination { .. })
        ));
    }

    #[test]
    fn display_ids_and_public_plan_condition_paths_are_explicit() {
        assert_eq!(display_ids::<PlanStepId>(&[]), "none");
        assert_eq!(display_ids(&[PlanId::new("plan-1").unwrap()]), "plan-1");
        let _ = PlanStep::new(
            PlanStepId::new("step-1").unwrap(),
            PlanStepKind::Observation,
            RequiredOutcome::new(RequiredOutcomeKind::Observation, "observe").unwrap(),
            PlanCondition::outcome(
                RequiredOutcome::new(RequiredOutcomeKind::Observation, "observe").unwrap(),
            ),
            "observation",
        )
        .unwrap();
    }
}
