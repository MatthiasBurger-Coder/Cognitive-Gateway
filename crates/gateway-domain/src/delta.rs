//! CG-07.03 deterministic DesiredState-to-Delta derivation.
//!
//! This module projects comparison results into explicit semantic gaps.  A
//! Delta item describes a gap and the outcome needed to close it; it never
//! grants permission to act and never resolves capabilities, agents, skills,
//! processes or policy.

use std::collections::BTreeSet;

use crate::{
    comparison::{
        ComparisonOutcome, ComparisonReasonCode, ComparisonResult, ComparisonRules,
        ComparisonTarget, compare_desired_state,
    },
    declarative_context::{CurrentState, Situation},
    identifiers::{DeltaId, DeltaItemId},
    intent::{ComparisonOperator, ConditionExpression, DesiredCondition, DesiredState},
    planning::{
        Delta, DeltaBasis, DeltaItem, DeltaKind, DeltaReasonCode, PlanningIrVersion,
        RequiredOutcome, RequiredOutcomeKind,
    },
    situation::SituationDiagnosticCode,
    validation::ValidationError,
};

/// The currently supported deterministic Delta derivation version.
pub const DELTA_DERIVATION_VERSION: PlanningIrVersion = PlanningIrVersion::V1;

/// Options that control only the Delta projection, not comparison semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct DeltaDerivationRules {
    version: PlanningIrVersion,
    include_satisfied_explanations: bool,
}

impl DeltaDerivationRules {
    /// Creates supported derivation rules.
    pub fn new(version: PlanningIrVersion) -> Result<Self, ValidationError> {
        version.ensure_supported()?;
        Ok(Self {
            version,
            include_satisfied_explanations: true,
        })
    }

    /// Returns the default v1 derivation rules.
    #[must_use]
    pub const fn v1() -> Self {
        Self {
            version: DELTA_DERIVATION_VERSION,
            include_satisfied_explanations: true,
        }
    }

    /// Selects whether satisfied leaf conditions remain as non-actionable
    /// explanation items in the Delta.
    #[must_use]
    pub const fn with_satisfied_explanations(mut self, include: bool) -> Self {
        self.include_satisfied_explanations = include;
        self
    }

    /// Returns the derivation contract version.
    #[must_use]
    pub const fn version(self) -> PlanningIrVersion {
        self.version
    }

    /// Returns whether satisfied explanation items are retained.
    #[must_use]
    pub const fn includes_satisfied_explanations(self) -> bool {
        self.include_satisfied_explanations
    }
}

impl Default for DeltaDerivationRules {
    fn default() -> Self {
        Self::v1()
    }
}

/// The Delta and its complete comparison tree, allowing non-actionable
/// expression branches to remain explainable without entering the Delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeltaDerivation {
    delta: Delta,
    comparison: ComparisonResult,
}

impl DeltaDerivation {
    fn new(delta: Delta, comparison: ComparisonResult) -> Self {
        Self { delta, comparison }
    }

    /// Returns the derived Delta.
    #[must_use]
    pub const fn delta(&self) -> &Delta {
        &self.delta
    }

    /// Returns the full comparison tree used to derive the Delta.
    #[must_use]
    pub const fn comparison(&self) -> &ComparisonResult {
        &self.comparison
    }

    /// Consumes the wrapper and returns the Delta.
    #[must_use]
    pub fn into_delta(self) -> Delta {
        self.delta
    }
}

/// Derives a deterministic Delta from one DesiredState and CurrentState.
///
/// The optional Situation contributes only its identity and matching basis
/// references.  All decision semantics come from the supplied comparison
/// rules and the explicit snapshots.
pub fn derive_delta(
    delta_id: DeltaId,
    desired_state: &DesiredState,
    current_state: &CurrentState,
    situation: Option<&Situation>,
    comparison_rules: &ComparisonRules,
) -> Result<Delta, ValidationError> {
    Ok(derive_delta_with_rules(
        delta_id,
        desired_state,
        current_state,
        situation,
        comparison_rules,
        &DeltaDerivationRules::default(),
    )?
    .into_delta())
}

/// Derives a Delta and retains the comparison tree for explainability.
pub fn derive_delta_with_rules(
    delta_id: DeltaId,
    desired_state: &DesiredState,
    current_state: &CurrentState,
    situation: Option<&Situation>,
    comparison_rules: &ComparisonRules,
    derivation_rules: &DeltaDerivationRules,
) -> Result<DeltaDerivation, ValidationError> {
    derivation_rules.version.ensure_supported()?;
    desired_state.version().ensure_supported()?;
    current_state.version().ensure_supported()?;
    if let Some(situation) = situation {
        situation.version().ensure_supported()?;
        if situation
            .observed_state_id()
            .is_some_and(|observed_state| observed_state != current_state.id())
        {
            return Err(ValidationError::InvalidStateCombination {
                reason: "Situation and CurrentState must reference the same observed snapshot",
            });
        }
    }

    let comparison = compare_desired_state(desired_state, current_state, comparison_rules)?;
    let mut leaves = Vec::new();
    collect_leaf_results(&comparison, &mut vec![0], false, true, &mut leaves);

    let mut seen = BTreeSet::new();
    let mut items = Vec::new();
    for leaf in leaves {
        let (outcome, negated) = effective_outcome(leaf.result, leaf.negated);
        let Some(condition) = find_condition(desired_state, leaf.condition_id) else {
            return Err(ValidationError::MissingDeclarativeIdentity {
                kind: "condition",
                id: leaf.condition_id.to_string(),
            });
        };
        let (kind, reason) = classify(condition, outcome, leaf.result.reason(), negated);
        if !leaf.required
            || (!derivation_rules.include_satisfied_explanations && kind == DeltaKind::Satisfied)
        {
            continue;
        }

        let deduplication_key = (condition.id().clone(), kind, reason, leaf.required);
        if !seen.insert(deduplication_key) {
            continue;
        }

        let item_id = DeltaItemId::new(format!("condition.{}", path_string(&leaf.path)))?;
        let basis = build_basis(leaf.result, situation, kind);
        let required_outcome = required_outcome(condition, kind, negated)?;
        let rationale = rationale(condition, kind, reason, negated);
        items.push(DeltaItem::new_with_reason(
            item_id,
            desired_state.id().clone(),
            condition.id().clone(),
            kind,
            reason,
            basis,
            required_outcome,
            rationale,
        )?);
    }

    let delta = Delta::new(
        delta_id,
        desired_state.id().clone(),
        situation.map(|value| value.id().clone()),
        items,
    )?;
    delta.validate_against_desired_state(desired_state)?;
    Ok(DeltaDerivation::new(delta, comparison))
}

/// Derives a Delta while retaining the comparison tree and using default
/// projection rules.
pub fn derive_delta_with_comparison(
    delta_id: DeltaId,
    desired_state: &DesiredState,
    current_state: &CurrentState,
    situation: Option<&Situation>,
    comparison_rules: &ComparisonRules,
) -> Result<DeltaDerivation, ValidationError> {
    derive_delta_with_rules(
        delta_id,
        desired_state,
        current_state,
        situation,
        comparison_rules,
        &DeltaDerivationRules::default(),
    )
}

#[derive(Debug)]
struct LeafResult<'a> {
    result: &'a ComparisonResult,
    condition_id: &'a crate::identifiers::ConditionId,
    path: Vec<usize>,
    negated: bool,
    required: bool,
}

fn collect_leaf_results<'a>(
    result: &'a ComparisonResult,
    path: &mut Vec<usize>,
    negated: bool,
    required: bool,
    leaves: &mut Vec<LeafResult<'a>>,
) {
    match result.target() {
        ComparisonTarget::Condition(condition_id) => leaves.push(LeafResult {
            result,
            condition_id,
            path: path.clone(),
            negated,
            required,
        }),
        ComparisonTarget::Expression(expression) => match expression {
            ConditionExpression::Condition(_) => {
                unreachable!("comparison condition target is used for leaf conditions")
            }
            ConditionExpression::All(_) => {
                for (index, child) in result.children().iter().enumerate() {
                    path.push(index);
                    collect_leaf_results(child, path, negated, required, leaves);
                    path.pop();
                }
            }
            ConditionExpression::Any(_) => {
                let has_satisfied_alternative = result
                    .children()
                    .iter()
                    .any(|child| child.outcome() == ComparisonOutcome::Satisfied);
                for (index, child) in result.children().iter().enumerate() {
                    path.push(index);
                    let child_required = required
                        && (!has_satisfied_alternative
                            || child.outcome() == ComparisonOutcome::Satisfied);
                    collect_leaf_results(child, path, negated, child_required, leaves);
                    path.pop();
                }
            }
            ConditionExpression::Not(_) => {
                let child = result
                    .children()
                    .first()
                    .expect("NOT comparison result must have one child");
                path.push(0);
                collect_leaf_results(child, path, !negated, required, leaves);
                path.pop();
            }
        },
    }
}

fn effective_outcome(result: &ComparisonResult, negated: bool) -> (ComparisonOutcome, bool) {
    if !negated {
        return (result.outcome(), false);
    }
    let outcome = match result.outcome() {
        ComparisonOutcome::Satisfied => ComparisonOutcome::Unsatisfied,
        ComparisonOutcome::Unsatisfied => ComparisonOutcome::Satisfied,
        outcome => outcome,
    };
    (outcome, true)
}

fn find_condition<'a>(
    desired_state: &'a DesiredState,
    condition_id: &crate::identifiers::ConditionId,
) -> Option<&'a DesiredCondition> {
    desired_state
        .conditions()
        .iter()
        .find(|condition| condition.id() == condition_id)
}

fn classify(
    condition: &DesiredCondition,
    outcome: ComparisonOutcome,
    comparison_reason: ComparisonReasonCode,
    negated: bool,
) -> (DeltaKind, DeltaReasonCode) {
    match outcome {
        ComparisonOutcome::Satisfied => (DeltaKind::Satisfied, DeltaReasonCode::ConditionSatisfied),
        ComparisonOutcome::Unsatisfied => {
            if negated || is_explicit_restriction_violation(condition) {
                (DeltaKind::Violation, DeltaReasonCode::ExplicitViolation)
            } else {
                (
                    DeltaKind::UnsatisfiedCondition,
                    DeltaReasonCode::ValueMismatch,
                )
            }
        }
        ComparisonOutcome::Unknown => match comparison_reason {
            ComparisonReasonCode::MissingEvidence => {
                (DeltaKind::MissingEvidence, DeltaReasonCode::MissingEvidence)
            }
            ComparisonReasonCode::SubjectNotObserved | ComparisonReasonCode::StateUnknown => {
                (DeltaKind::UnknownState, map_reason(comparison_reason))
            }
            _ => (DeltaKind::UnknownState, DeltaReasonCode::StateUnknown),
        },
        ComparisonOutcome::Conflicted => (DeltaKind::Conflict, DeltaReasonCode::StateConflict),
        ComparisonOutcome::InsufficientEvidence => {
            (DeltaKind::MissingEvidence, map_reason(comparison_reason))
        }
        ComparisonOutcome::UnresolvedInput => {
            (DeltaKind::UnresolvedInput, DeltaReasonCode::UnresolvedInput)
        }
        ComparisonOutcome::Incomparable => (
            DeltaKind::UnsupportedComparison,
            map_reason(comparison_reason),
        ),
    }
}

fn is_explicit_restriction_violation(condition: &DesiredCondition) -> bool {
    matches!(
        condition.operator(),
        ComparisonOperator::NotEquals | ComparisonOperator::Absent
    )
}

fn map_reason(reason: ComparisonReasonCode) -> DeltaReasonCode {
    match reason {
        ComparisonReasonCode::ValueMatches => DeltaReasonCode::ConditionSatisfied,
        ComparisonReasonCode::ValueDoesNotMatch => DeltaReasonCode::ValueMismatch,
        ComparisonReasonCode::SubjectNotObserved => DeltaReasonCode::SubjectNotObserved,
        ComparisonReasonCode::StateUnknown => DeltaReasonCode::StateUnknown,
        ComparisonReasonCode::StateConflict => DeltaReasonCode::StateConflict,
        ComparisonReasonCode::MissingEvidence => DeltaReasonCode::MissingEvidence,
        ComparisonReasonCode::StaleEvidence => DeltaReasonCode::StaleEvidence,
        ComparisonReasonCode::FreshnessUnknown => DeltaReasonCode::FreshnessUnknown,
        ComparisonReasonCode::IncompleteInformation => DeltaReasonCode::IncompleteInformation,
        ComparisonReasonCode::IncompatibleTypes => DeltaReasonCode::IncompatibleTypes,
        ComparisonReasonCode::UnsupportedOperation => DeltaReasonCode::UnsupportedOperation,
        ComparisonReasonCode::NegatedAssertionNotComparable => {
            DeltaReasonCode::NegatedAssertionNotComparable
        }
        ComparisonReasonCode::ExpressionSatisfied
        | ComparisonReasonCode::ExpressionUnsatisfied
        | ComparisonReasonCode::ExpressionUnknown
        | ComparisonReasonCode::ExpressionConflict
        | ComparisonReasonCode::ExpressionInsufficientEvidence
        | ComparisonReasonCode::ExpressionUnresolvedInput
        | ComparisonReasonCode::ExpressionIncomparable => DeltaReasonCode::StateUnknown,
    }
}

fn required_outcome(
    condition: &DesiredCondition,
    kind: DeltaKind,
    negated: bool,
) -> Result<RequiredOutcome, ValidationError> {
    let (outcome_kind, description) = match kind {
        DeltaKind::Satisfied => (
            RequiredOutcomeKind::NoOp,
            format!("condition {} is already satisfied", condition.id()),
        ),
        DeltaKind::UnsatisfiedCondition => (
            RequiredOutcomeKind::DomainChange,
            format!(
                "make {} {} the declared value",
                condition.subject(),
                condition.operator()
            ),
        ),
        DeltaKind::Violation => (
            RequiredOutcomeKind::DomainChange,
            if negated {
                format!("make condition {} false", condition.id())
            } else {
                format!(
                    "remove the explicit violation of condition {}",
                    condition.id()
                )
            },
        ),
        DeltaKind::MissingState => (
            RequiredOutcomeKind::DomainChange,
            format!("establish the required state for {}", condition.subject()),
        ),
        DeltaKind::MissingEvidence => (
            RequiredOutcomeKind::EvidenceAcquisition,
            format!("obtain sufficient evidence for {}", condition.subject()),
        ),
        DeltaKind::UnknownState => (
            RequiredOutcomeKind::Observation,
            format!("observe or reassess {}", condition.subject()),
        ),
        DeltaKind::Conflict => (
            RequiredOutcomeKind::ConflictResolution,
            format!("resolve conflicting assertions for {}", condition.subject()),
        ),
        DeltaKind::UnresolvedInput => (
            RequiredOutcomeKind::InputAcquisition,
            format!("obtain the unresolved input for {}", condition.subject()),
        ),
        DeltaKind::UnsupportedComparison => (
            RequiredOutcomeKind::Assessment,
            format!("resolve how {} can be compared", condition.subject()),
        ),
    };
    let mut outcome =
        RequiredOutcome::new(outcome_kind, description)?.with_subject(condition.subject().clone());
    if !negated && matches!(kind, DeltaKind::UnsatisfiedCondition | DeltaKind::Violation) {
        if let Some(expected) = condition.expected() {
            outcome = outcome.with_expected(expected.clone())?;
        }
    }
    Ok(outcome)
}

fn rationale(
    condition: &DesiredCondition,
    kind: DeltaKind,
    reason: DeltaReasonCode,
    negated: bool,
) -> String {
    let context = if negated {
        " through a negated expression"
    } else {
        ""
    };
    format!(
        "condition {} on {} is classified as {} ({}){}",
        condition.id(),
        condition.subject(),
        kind,
        reason,
        context
    )
}

fn path_string(path: &[usize]) -> String {
    path.iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn build_basis(
    result: &ComparisonResult,
    situation: Option<&Situation>,
    kind: DeltaKind,
) -> DeltaBasis {
    let trace = result.trace();
    let mut facts = trace.facts().to_vec();
    let mut observations = trace.observations().to_vec();
    let mut evidence = trace.evidence().to_vec();
    let mut provenances = trace.provenances().to_vec();
    let mut assessments = Vec::new();

    if let Some(situation) = situation {
        for assessment in situation.assessments() {
            if basis_overlaps_trace(assessment.basis(), trace) {
                assessments.push(assessment.id().clone());
                facts.extend(assessment.basis().facts().iter().cloned());
                evidence.extend(assessment.basis().evidence().iter().cloned());
                provenances.extend(assessment.basis().provenances().iter().cloned());
            }
        }
        for diagnostic in situation.diagnostics() {
            if diagnostic_matches(diagnostic.code(), kind)
                && diagnostic
                    .basis()
                    .state_subjects()
                    .iter()
                    .any(|subject| trace.subjects().contains(subject))
            {
                facts.extend(diagnostic.basis().facts().iter().cloned());
                evidence.extend(diagnostic.basis().evidence().iter().cloned());
                provenances.extend(diagnostic.basis().provenances().iter().cloned());
                assessments.extend(diagnostic.basis().assessments().iter().cloned());
            }
        }
    }

    deduplicate(&mut facts);
    deduplicate(&mut observations);
    deduplicate(&mut evidence);
    deduplicate(&mut provenances);
    deduplicate(&mut assessments);
    DeltaBasis::new(
        situation.map(|value| value.id().clone()),
        Some(trace.observed_state().clone()),
        trace.subjects().to_vec(),
        facts,
        observations,
        evidence,
        provenances,
        assessments,
    )
    .expect("comparison traces and canonical situation bases are valid")
}

fn basis_overlaps_trace(
    basis: &crate::situation::BasisReferences,
    trace: &crate::comparison::ComparisonTrace,
) -> bool {
    basis
        .state_subjects()
        .iter()
        .any(|subject| trace.subjects().contains(subject))
        || basis
            .facts()
            .iter()
            .any(|fact| trace.facts().contains(fact))
        || basis
            .evidence()
            .iter()
            .any(|evidence| trace.evidence().contains(evidence))
        || basis
            .provenances()
            .iter()
            .any(|provenance| trace.provenances().contains(provenance))
}

fn diagnostic_matches(code: SituationDiagnosticCode, kind: DeltaKind) -> bool {
    match kind {
        DeltaKind::UnknownState => code == SituationDiagnosticCode::UnknownState,
        DeltaKind::MissingEvidence => matches!(
            code,
            SituationDiagnosticCode::DataQuality | SituationDiagnosticCode::UnsupportedState
        ),
        DeltaKind::Conflict => code == SituationDiagnosticCode::StateConflict,
        DeltaKind::UnsupportedComparison => code == SituationDiagnosticCode::UnsupportedState,
        _ => false,
    }
}

fn deduplicate<T: Ord>(values: &mut Vec<T>) {
    values.sort();
    values.dedup();
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        DeclarativeContextVersion,
        comparison::{ComparisonOutcome, ComparisonReasonCode, ComparisonRules},
        declarative_context::{ObservedState, Situation},
        identifiers::{ConditionId, CurrentStateId, DeltaId, DesiredStateId, SituationId},
        intent::{
            ComparisonOperator, ConditionExpression, DesiredCondition, DesiredState, SubjectPath,
            TypedValue,
        },
        normalization::{NormalizedStateEntry, StateLineage},
        observation::AssertionPolarity,
        planning::{DeltaKind, DeltaReasonCode, PlanningIrVersion, RequiredOutcomeKind},
        quality::{
            Confidence, FreshnessStatus, QualityMetadata, SensitivityClass, TrustClass, Uncertainty,
        },
        situation::{
            Assessment, AssessmentConclusion, AssessmentKind, AssessmentOrigin,
            AssessmentRuleContract, AssessmentRuleVersion, AssessmentStatus, BasisReferences,
            ReasonCode, SituationDiagnostic, SituationDiagnosticCode,
        },
        validation::ValidationError,
    };

    use super::*;

    fn desired(expression: ConditionExpression) -> DesiredState {
        let first = DesiredCondition::new(
            ConditionId::new("coverage").unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            ComparisonOperator::GreaterOrEqual,
            Some(TypedValue::Integer(95)),
        )
        .unwrap();
        let second = DesiredCondition::new(
            ConditionId::new("dependency").unwrap(),
            SubjectPath::from_str("domain.infrastructure").unwrap(),
            ComparisonOperator::NotEquals,
            Some(TypedValue::Symbol(
                crate::SymbolValue::new("infrastructure").unwrap(),
            )),
        )
        .unwrap();
        DesiredState::new(
            DesiredStateId::new("desired-1").unwrap(),
            vec![first, second],
            expression,
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    fn known_state(subject: &str, value: TypedValue) -> NormalizedStateEntry {
        let subject = SubjectPath::from_str(subject).unwrap();
        NormalizedStateEntry::from_parts(
            subject,
            crate::normalization::StateStatus::Known,
            Some(value),
            Some(AssertionPolarity::Affirmed),
            Vec::new(),
            StateLineage::from_parts(Vec::new(), Vec::new(), Vec::new(), Vec::new()).unwrap(),
            Vec::new(),
            None,
        )
        .unwrap()
    }

    fn current(entries: Vec<NormalizedStateEntry>) -> crate::CurrentState {
        ObservedState::new_v1_with_entries(
            CurrentStateId::new("state-1").unwrap(),
            entries,
            Vec::new(),
        )
    }

    #[test]
    fn derives_classified_items_and_typed_outcomes() {
        let desired = desired(
            ConditionExpression::all(vec![
                ConditionExpression::condition(ConditionId::new("coverage").unwrap()),
                ConditionExpression::condition(ConditionId::new("dependency").unwrap()),
            ])
            .unwrap(),
        );
        let state = current(vec![
            known_state("coverage.percent", TypedValue::Integer(92)),
            known_state(
                "domain.infrastructure",
                TypedValue::Symbol(crate::SymbolValue::new("infrastructure").unwrap()),
            ),
        ]);
        let derivation = derive_delta_with_comparison(
            DeltaId::new("delta-1").unwrap(),
            &desired,
            &state,
            None,
            &ComparisonRules::default(),
        )
        .unwrap();
        assert_eq!(
            derivation.comparison().outcome(),
            ComparisonOutcome::Unsatisfied
        );
        assert_eq!(derivation.delta().items().len(), 2);
        let violation = derivation
            .delta()
            .items()
            .iter()
            .find(|item| item.condition().as_str() == "dependency")
            .unwrap();
        assert_eq!(violation.kind(), DeltaKind::Violation);
        assert_eq!(violation.reason(), DeltaReasonCode::ExplicitViolation);
        assert_eq!(
            violation.required_outcome().kind(),
            RequiredOutcomeKind::DomainChange
        );
        assert!(
            derivation
                .delta()
                .items()
                .iter()
                .all(DeltaItem::is_actionable)
        );
    }

    #[test]
    fn missing_evidence_and_unknown_remain_distinct() {
        let desired = desired(
            ConditionExpression::all(vec![
                ConditionExpression::condition(ConditionId::new("coverage").unwrap()),
                ConditionExpression::condition(ConditionId::new("dependency").unwrap()),
            ])
            .unwrap(),
        );
        let state = current(Vec::new());
        let delta = derive_delta(
            DeltaId::new("delta-unknown").unwrap(),
            &desired,
            &state,
            None,
            &ComparisonRules::default(),
        )
        .unwrap();
        assert_eq!(delta.items().len(), 2);
        assert!(
            delta
                .items()
                .iter()
                .all(|item| item.kind() == DeltaKind::UnknownState)
        );
        assert!(
            delta
                .items()
                .iter()
                .all(|item| item.required_outcome().kind() == RequiredOutcomeKind::Observation)
        );
    }

    #[test]
    fn satisfied_explanations_are_non_actionable_and_can_be_omitted() {
        let desired = desired(ConditionExpression::condition(
            ConditionId::new("coverage").unwrap(),
        ));
        let state = current(vec![known_state(
            "coverage.percent",
            TypedValue::Integer(95),
        )]);
        let with_explanation = derive_delta(
            DeltaId::new("delta-satisfied").unwrap(),
            &desired,
            &state,
            None,
            &ComparisonRules::default(),
        )
        .unwrap();
        assert!(with_explanation.is_noop());
        assert_eq!(with_explanation.items().len(), 1);
        assert!(!with_explanation.items()[0].is_actionable());

        let omitted = derive_delta_with_rules(
            DeltaId::new("delta-satisfied-omitted").unwrap(),
            &desired,
            &state,
            None,
            &ComparisonRules::default(),
            &DeltaDerivationRules::default().with_satisfied_explanations(false),
        )
        .unwrap();
        assert!(omitted.delta().items().is_empty());
        assert_eq!(omitted.comparison().outcome(), ComparisonOutcome::Satisfied);
    }

    #[test]
    fn any_expression_does_not_emit_unneeded_alternatives() {
        let desired = desired(
            ConditionExpression::any(vec![
                ConditionExpression::condition(ConditionId::new("coverage").unwrap()),
                ConditionExpression::condition(ConditionId::new("dependency").unwrap()),
            ])
            .unwrap(),
        );
        let state = current(vec![known_state(
            "coverage.percent",
            TypedValue::Integer(95),
        )]);
        let delta = derive_delta(
            DeltaId::new("delta-any").unwrap(),
            &desired,
            &state,
            None,
            &ComparisonRules::default(),
        )
        .unwrap();
        assert!(delta.is_noop());
        assert_eq!(delta.items().len(), 1);
        assert_eq!(delta.items()[0].condition().as_str(), "coverage");
    }

    #[test]
    fn negated_satisfied_condition_becomes_explicit_violation() {
        let desired = desired(ConditionExpression::negate(ConditionExpression::condition(
            ConditionId::new("coverage").unwrap(),
        )));
        let state = current(vec![known_state(
            "coverage.percent",
            TypedValue::Integer(95),
        )]);
        let delta = derive_delta(
            DeltaId::new("delta-not").unwrap(),
            &desired,
            &state,
            None,
            &ComparisonRules::default(),
        )
        .unwrap();
        assert_eq!(delta.items()[0].kind(), DeltaKind::Violation);
        assert_eq!(
            delta.items()[0].reason(),
            DeltaReasonCode::ExplicitViolation
        );
        assert!(delta.items()[0].required_outcome().expected().is_none());
    }

    #[test]
    fn situation_lineage_and_identity_are_preserved() {
        let desired = desired(ConditionExpression::condition(
            ConditionId::new("coverage").unwrap(),
        ));
        let state = current(vec![known_state(
            "coverage.percent",
            TypedValue::Integer(92),
        )]);
        let situation = Situation::new_v1(SituationId::new("situation-1").unwrap());
        let derivation = derive_delta_with_comparison(
            DeltaId::new("delta-situation").unwrap(),
            &desired,
            &state,
            Some(&situation),
            &ComparisonRules::default(),
        )
        .unwrap();
        assert_eq!(
            derivation.delta().situation().unwrap().as_str(),
            "situation-1"
        );
        assert_eq!(
            derivation.delta().items()[0]
                .basis()
                .current_state()
                .unwrap()
                .as_str(),
            "state-1"
        );
        assert_eq!(
            derivation.delta().items()[0]
                .basis()
                .situation()
                .unwrap()
                .as_str(),
            "situation-1"
        );
    }

    #[test]
    fn mismatched_situation_is_rejected_and_rules_fail_closed() {
        let desired = desired(ConditionExpression::condition(
            ConditionId::new("coverage").unwrap(),
        ));
        let state = current(Vec::new());
        let situation = Situation::new_v1(SituationId::new("situation-1").unwrap());
        let other = ObservedState::new_v1(CurrentStateId::new("state-2").unwrap());
        let situation_with_other_state = Situation::from_parts(
            DeclarativeContextVersion::V1,
            situation.id().clone(),
            other.id().clone(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert!(matches!(
            derive_delta(
                DeltaId::new("delta-mismatch").unwrap(),
                &desired,
                &state,
                Some(&situation_with_other_state),
                &ComparisonRules::default(),
            ),
            Err(ValidationError::InvalidStateCombination { .. })
        ));
        assert!(matches!(
            DeltaDerivationRules::new(PlanningIrVersion::new(1, 1).unwrap()),
            Err(ValidationError::UnsupportedSchemaVersion { .. })
        ));
    }

    #[test]
    fn covers_rules_reasons_classifications_and_outcome_kinds() {
        let rules = DeltaDerivationRules::default();
        assert_eq!(rules.version(), PlanningIrVersion::V1);
        assert!(rules.includes_satisfied_explanations());
        assert!(
            !rules
                .with_satisfied_explanations(false)
                .includes_satisfied_explanations()
        );
        assert!(DeltaDerivationRules::new(PlanningIrVersion::V1).is_ok());

        let condition = DesiredCondition::new(
            ConditionId::new("coverage").unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            ComparisonOperator::Equals,
            Some(TypedValue::Integer(95)),
        )
        .unwrap();
        for (kind, expected_outcome) in [
            (DeltaKind::Satisfied, RequiredOutcomeKind::NoOp),
            (
                DeltaKind::UnsatisfiedCondition,
                RequiredOutcomeKind::DomainChange,
            ),
            (DeltaKind::Violation, RequiredOutcomeKind::DomainChange),
            (DeltaKind::MissingState, RequiredOutcomeKind::DomainChange),
            (
                DeltaKind::MissingEvidence,
                RequiredOutcomeKind::EvidenceAcquisition,
            ),
            (DeltaKind::UnknownState, RequiredOutcomeKind::Observation),
            (DeltaKind::Conflict, RequiredOutcomeKind::ConflictResolution),
            (
                DeltaKind::UnresolvedInput,
                RequiredOutcomeKind::InputAcquisition,
            ),
            (
                DeltaKind::UnsupportedComparison,
                RequiredOutcomeKind::Assessment,
            ),
        ] {
            assert_eq!(
                required_outcome(&condition, kind, false).unwrap().kind(),
                expected_outcome
            );
        }

        let all_reasons = [
            ComparisonReasonCode::ValueMatches,
            ComparisonReasonCode::ValueDoesNotMatch,
            ComparisonReasonCode::SubjectNotObserved,
            ComparisonReasonCode::StateUnknown,
            ComparisonReasonCode::StateConflict,
            ComparisonReasonCode::MissingEvidence,
            ComparisonReasonCode::StaleEvidence,
            ComparisonReasonCode::FreshnessUnknown,
            ComparisonReasonCode::IncompleteInformation,
            ComparisonReasonCode::IncompatibleTypes,
            ComparisonReasonCode::UnsupportedOperation,
            ComparisonReasonCode::NegatedAssertionNotComparable,
            ComparisonReasonCode::ExpressionSatisfied,
            ComparisonReasonCode::ExpressionUnsatisfied,
            ComparisonReasonCode::ExpressionUnknown,
            ComparisonReasonCode::ExpressionConflict,
            ComparisonReasonCode::ExpressionInsufficientEvidence,
            ComparisonReasonCode::ExpressionUnresolvedInput,
            ComparisonReasonCode::ExpressionIncomparable,
        ];
        for reason in all_reasons {
            let _ = map_reason(reason);
        }
        assert_eq!(
            classify(
                &condition,
                ComparisonOutcome::Unknown,
                ComparisonReasonCode::MissingEvidence,
                false
            ),
            (DeltaKind::MissingEvidence, DeltaReasonCode::MissingEvidence)
        );
        assert_eq!(
            classify(
                &condition,
                ComparisonOutcome::Unknown,
                ComparisonReasonCode::ExpressionUnknown,
                false
            ),
            (DeltaKind::UnknownState, DeltaReasonCode::StateUnknown)
        );
        assert_eq!(
            classify(
                &condition,
                ComparisonOutcome::UnresolvedInput,
                ComparisonReasonCode::ExpressionUnresolvedInput,
                false
            ),
            (DeltaKind::UnresolvedInput, DeltaReasonCode::UnresolvedInput)
        );
        assert_eq!(
            classify(
                &condition,
                ComparisonOutcome::Incomparable,
                ComparisonReasonCode::IncompatibleTypes,
                false
            ),
            (
                DeltaKind::UnsupportedComparison,
                DeltaReasonCode::IncompatibleTypes
            )
        );
        assert_eq!(
            effective_outcome(
                derive_delta_with_comparison(
                    DeltaId::new("effective").unwrap(),
                    &desired(ConditionExpression::condition(
                        ConditionId::new("coverage").unwrap()
                    )),
                    &current(vec![known_state(
                        "coverage.percent",
                        TypedValue::Integer(95)
                    )]),
                    None,
                    &ComparisonRules::default(),
                )
                .unwrap()
                .comparison(),
                true,
            ),
            (ComparisonOutcome::Unsatisfied, true)
        );
        assert_eq!(path_string(&[]), "");
        let mut values = vec![2, 1, 2, 1];
        deduplicate(&mut values);
        assert_eq!(values, vec![1, 2]);
        for code in [
            SituationDiagnosticCode::UnknownState,
            SituationDiagnosticCode::StateConflict,
            SituationDiagnosticCode::UnsupportedState,
            SituationDiagnosticCode::UnresolvedAssessment,
            SituationDiagnosticCode::UnknownRisk,
            SituationDiagnosticCode::DataQuality,
        ] {
            for kind in [
                DeltaKind::Satisfied,
                DeltaKind::UnknownState,
                DeltaKind::MissingEvidence,
                DeltaKind::Conflict,
                DeltaKind::UnsupportedComparison,
            ] {
                let _ = diagnostic_matches(code, kind);
            }
        }
    }

    #[test]
    fn retains_relevant_situation_assessments_and_diagnostic_basis() {
        let desired = desired(ConditionExpression::condition(
            ConditionId::new("coverage").unwrap(),
        ));
        let state = current(vec![
            NormalizedStateEntry::from_parts(
                SubjectPath::from_str("coverage.percent").unwrap(),
                crate::normalization::StateStatus::Conflicted,
                None,
                None,
                Vec::new(),
                StateLineage::from_parts(Vec::new(), Vec::new(), Vec::new(), Vec::new()).unwrap(),
                vec![crate::normalization::NormalizationReasonCode::ConflictingAssertions],
                None,
            )
            .unwrap(),
        ]);
        let subject = SubjectPath::from_str("coverage.percent").unwrap();
        let basis = BasisReferences::new(
            vec![subject.clone()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        let assessment = Assessment::new(
            crate::AssessmentId::new("coverage-assessment").unwrap(),
            AssessmentKind::Coverage,
            AssessmentConclusion::Negative,
            AssessmentStatus::Determined,
            ReasonCode::new("coverage_conflict").unwrap(),
            "coverage is conflicted",
            basis.clone(),
            AssessmentOrigin::Deterministic {
                rule: AssessmentRuleContract::new(
                    crate::AssessmentRuleId::new("coverage-rule").unwrap(),
                    AssessmentRuleVersion::V1,
                )
                .unwrap(),
            },
            QualityMetadata::new(
                TrustClass::DerivedAssessment,
                SensitivityClass::Internal,
                Confidence::Unknown,
                FreshnessStatus::Unknown,
                Uncertainty::None,
            ),
        )
        .unwrap();
        let diagnostic = SituationDiagnostic::new(
            SituationDiagnosticCode::StateConflict,
            "conflicting coverage assertions",
            basis,
        )
        .unwrap();
        let situation = Situation::from_parts(
            DeclarativeContextVersion::V1,
            SituationId::new("situation-conflict").unwrap(),
            state.id().clone(),
            vec![assessment],
            Vec::new(),
            vec![diagnostic],
            Vec::new(),
        );
        let derivation = derive_delta_with_comparison(
            DeltaId::new("delta-conflict").unwrap(),
            &desired,
            &state,
            Some(&situation),
            &ComparisonRules::default(),
        )
        .unwrap();
        let item = &derivation.delta().items()[0];
        assert_eq!(item.kind(), DeltaKind::Conflict);
        assert_eq!(item.basis().assessments().len(), 1);
        assert!(item.basis().state_subjects().contains(&subject));
    }

    #[test]
    fn any_without_a_satisfied_alternative_keeps_all_required_gaps() {
        let desired = desired(
            ConditionExpression::any(vec![
                ConditionExpression::condition(ConditionId::new("coverage").unwrap()),
                ConditionExpression::condition(ConditionId::new("dependency").unwrap()),
            ])
            .unwrap(),
        );
        let delta = derive_delta(
            DeltaId::new("delta-any-unknown").unwrap(),
            &desired,
            &current(Vec::new()),
            None,
            &ComparisonRules::default(),
        )
        .unwrap();
        assert_eq!(delta.items().len(), 2);
        assert!(delta.items().iter().all(|item| item.is_actionable()));
    }
}
