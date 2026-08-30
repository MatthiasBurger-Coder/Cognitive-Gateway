//! Stable application APIs for the complete CG-07 declarative planning chain.
//!
//! The facade is deliberately stateless. All semantic snapshots and rule
//! versions are explicit inputs, while the output boundary contains only
//! provider-independent comparison, Delta, capability-requirement and Plan
//! artifacts. The CG-03 index is read only for abstract capability
//! declarations; its Agent and Skill candidates never enter a planning
//! result.

use gateway_domain::{
    CapabilityRequirementDerivation, CapabilityRequirementRules, ComparisonResult, ComparisonRules,
    CurrentState, Delta, DeltaDerivation, DeltaDerivationRules, DeltaId, DesiredState, Plan,
    PlannerResult, PlannerRules, PlanningIrVersion, PlanningValidationReport, SerializationError,
    Situation, ValidationError, compare_desired_state, derive_capability_requirements,
    derive_delta_with_rules, explain_plan, plan_from_capability_derivation,
};
use gateway_registry::CapabilityIndex;

/// Stable failure returned by the CG-07 application boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningApplicationError {
    code: &'static str,
    message: String,
}

impl PlanningApplicationError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Returns the stable machine-readable error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Returns the non-sensitive diagnostic message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for PlanningApplicationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PlanningApplicationError {}

impl From<ValidationError> for PlanningApplicationError {
    fn from(error: ValidationError) -> Self {
        Self::new("DOMAIN_VALIDATION_ERROR", error.to_string())
    }
}

impl From<SerializationError> for PlanningApplicationError {
    fn from(error: SerializationError) -> Self {
        Self::new("PLANNING_SERIALIZATION_ERROR", error.to_string())
    }
}

impl From<gateway_registry::RegistryIntegrityError> for PlanningApplicationError {
    fn from(error: gateway_registry::RegistryIntegrityError) -> Self {
        Self::new("CAPABILITY_SNAPSHOT_ERROR", error.to_string())
    }
}

/// An explicit, immutable CG-03 capability-index snapshot for planning.
///
/// The snapshot carries an application-supplied identity and supported
/// version so explainability can identify the exact abstract contract basis.
/// Only capability declarations are projected into CG-07; provider candidates
/// remain an internal CG-03 concern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningCapabilitySnapshot {
    index: CapabilityIndex,
    identity: gateway_domain::NonEmptyText,
    version: PlanningIrVersion,
}

impl PlanningCapabilitySnapshot {
    /// Creates a planning snapshot from a validated CG-03 capability index.
    pub fn new(
        index: CapabilityIndex,
        identity: impl Into<String>,
        version: PlanningIrVersion,
    ) -> Result<Self, PlanningApplicationError> {
        version.ensure_supported()?;
        Ok(Self {
            index,
            identity: gateway_domain::NonEmptyText::new(identity)?,
            version,
        })
    }

    /// Returns the caller-supplied snapshot identity.
    #[must_use]
    pub fn identity(&self) -> &str {
        self.identity.as_str()
    }

    /// Returns the supported snapshot contract version.
    #[must_use]
    pub const fn version(&self) -> PlanningIrVersion {
        self.version
    }

    /// Returns abstract capability IDs available in the snapshot.
    #[must_use]
    pub fn capability_ids(&self) -> impl ExactSizeIterator<Item = &gateway_domain::CapabilityId> {
        self.index.ids()
    }

    /// Returns the number of abstract capability declarations in the snapshot.
    #[must_use]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Returns whether the snapshot contains no abstract declarations.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    fn declarations(&self) -> Vec<gateway_domain::CapabilityDefinition> {
        self.index
            .entries()
            .map(|entry| entry.capability().clone())
            .collect()
    }
}

/// Explicit versions of all rule contracts participating in one planning run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PlanningRuleSnapshot {
    comparison: gateway_domain::ComparisonSemanticsVersion,
    delta: PlanningIrVersion,
    capability_requirements: PlanningIrVersion,
    planner: PlanningIrVersion,
}

impl PlanningRuleSnapshot {
    /// Captures versions from the explicit rules used by a planning run.
    #[must_use]
    pub const fn from_rules(
        comparison: &ComparisonRules,
        delta: &DeltaDerivationRules,
        capability_requirements: &CapabilityRequirementRules,
        planner: &PlannerRules,
    ) -> Self {
        Self {
            comparison: comparison.version(),
            delta: delta.version(),
            capability_requirements: capability_requirements.version(),
            planner: planner.version(),
        }
    }

    /// Returns the comparison semantics version.
    #[must_use]
    pub const fn comparison(&self) -> gateway_domain::ComparisonSemanticsVersion {
        self.comparison
    }

    /// Returns the Delta derivation version.
    #[must_use]
    pub const fn delta(&self) -> PlanningIrVersion {
        self.delta
    }

    /// Returns the capability-requirement derivation version.
    #[must_use]
    pub const fn capability_requirements(&self) -> PlanningIrVersion {
        self.capability_requirements
    }

    /// Returns the deterministic planner version.
    #[must_use]
    pub const fn planner(&self) -> PlanningIrVersion {
        self.planner
    }
}

/// Application-level explainability enriched with snapshot and rule identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningExplainability {
    trace: gateway_domain::PlanExplanation,
    capability_snapshot: gateway_domain::NonEmptyText,
    capability_snapshot_version: PlanningIrVersion,
    rules: PlanningRuleSnapshot,
}

impl PlanningExplainability {
    fn new(
        trace: gateway_domain::PlanExplanation,
        snapshot: &PlanningCapabilitySnapshot,
        rules: PlanningRuleSnapshot,
    ) -> Self {
        Self {
            trace,
            capability_snapshot: snapshot.identity.clone(),
            capability_snapshot_version: snapshot.version,
            rules,
        }
    }

    /// Returns the domain-level DesiredState-to-Plan trace.
    #[must_use]
    pub const fn trace(&self) -> &gateway_domain::PlanExplanation {
        &self.trace
    }

    /// Returns the abstract capability snapshot identity.
    #[must_use]
    pub fn capability_snapshot(&self) -> &str {
        self.capability_snapshot.as_str()
    }

    /// Returns the abstract capability snapshot version.
    #[must_use]
    pub const fn capability_snapshot_version(&self) -> PlanningIrVersion {
        self.capability_snapshot_version
    }

    /// Returns all rule versions captured for the planning run.
    #[must_use]
    pub const fn rules(&self) -> PlanningRuleSnapshot {
        self.rules
    }

    /// Renders snapshot metadata followed by the canonical domain trace.
    #[must_use]
    pub fn to_text(&self) -> String {
        format!(
            "Capability snapshot {} (version {})\nRules: comparison {}, delta {}, capability requirements {}, planner {}\n{}",
            self.capability_snapshot.as_str(),
            self.capability_snapshot_version,
            self.rules.comparison,
            self.rules.delta,
            self.rules.capability_requirements,
            self.rules.planner,
            self.trace.to_text(),
        )
    }
}

/// Stateless facade exposing the complete CG-07 application contract.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeclarativePlanningApplication;

impl DeclarativePlanningApplication {
    /// Creates the stateless planning facade.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Compares a DesiredState with an explicit CurrentState snapshot.
    ///
    /// An optional Situation is checked only for snapshot identity; its
    /// assessments and references remain owned by CG-06 and are not mutated.
    pub fn compare_desired_to_situation(
        &self,
        desired_state: &DesiredState,
        current_state: &CurrentState,
        situation: Option<&Situation>,
        rules: &ComparisonRules,
    ) -> Result<ComparisonResult, PlanningApplicationError> {
        validate_situation_snapshot(current_state, situation)?;
        compare_desired_state(desired_state, current_state, rules)
            .map_err(PlanningApplicationError::from)
    }

    /// Derives a Delta while retaining its complete comparison trace.
    #[allow(clippy::too_many_arguments)]
    pub fn derive_delta(
        &self,
        delta_id: DeltaId,
        desired_state: &DesiredState,
        current_state: &CurrentState,
        situation: Option<&Situation>,
        comparison_rules: &ComparisonRules,
        delta_rules: &DeltaDerivationRules,
    ) -> Result<DeltaDerivation, PlanningApplicationError> {
        derive_delta_with_rules(
            delta_id,
            desired_state,
            current_state,
            situation,
            comparison_rules,
            delta_rules,
        )
        .map_err(PlanningApplicationError::from)
    }

    /// Derives abstract capability requirements from a CG-03 index snapshot.
    pub fn derive_capability_requirements(
        &self,
        desired_state: &DesiredState,
        delta: &Delta,
        snapshot: &PlanningCapabilitySnapshot,
        rules: &CapabilityRequirementRules,
    ) -> Result<CapabilityRequirementDerivation, PlanningApplicationError> {
        derive_capability_requirements(delta, desired_state, &snapshot.declarations(), rules)
            .map_err(PlanningApplicationError::from)
    }

    /// Builds a provider-independent Plan from abstract requirements.
    pub fn build_plan(
        &self,
        desired_state: &DesiredState,
        delta: &Delta,
        derivation: &CapabilityRequirementDerivation,
        rules: &PlannerRules,
    ) -> Result<PlannerResult, PlanningApplicationError> {
        plan_from_capability_derivation(desired_state, delta, derivation, rules)
            .map_err(PlanningApplicationError::from)
    }

    /// Validates a Plan before handing it to CG-08 resolution.
    #[must_use]
    pub fn validate_plan(
        &self,
        desired_state: &DesiredState,
        delta: &Delta,
        plan: &Plan,
    ) -> PlanningValidationReport {
        plan.validation_report(desired_state, delta)
    }

    /// Explains a validated planner result and records snapshot/rule metadata.
    pub fn explain_plan(
        &self,
        desired_state: &DesiredState,
        delta: &Delta,
        result: &PlannerResult,
        snapshot: &PlanningCapabilitySnapshot,
        rules: PlanningRuleSnapshot,
    ) -> Result<PlanningExplainability, PlanningApplicationError> {
        explain_plan(desired_state, delta, result)
            .map(|trace| PlanningExplainability::new(trace, snapshot, rules))
            .map_err(PlanningApplicationError::from)
    }

    /// Serializes a Plan through the canonical CG-07 JSON contract.
    pub fn serialize_plan(&self, plan: &Plan) -> Result<String, PlanningApplicationError> {
        plan.to_json().map_err(PlanningApplicationError::from)
    }
}

fn validate_situation_snapshot(
    current_state: &CurrentState,
    situation: Option<&Situation>,
) -> Result<(), PlanningApplicationError> {
    let Some(situation) = situation else {
        return Ok(());
    };
    situation.version().ensure_supported()?;
    if situation
        .observed_state_id()
        .is_some_and(|observed_state| observed_state != current_state.id())
    {
        return Err(PlanningApplicationError::from(
            ValidationError::InvalidStateCombination {
                reason: "Situation and CurrentState must reference the same observed snapshot",
            },
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use gateway_domain::{
        CapabilityId, ComparisonOperator, ConditionExpression, DeltaId, DesiredCondition,
        DesiredState, DesiredStateId, ObservedState, PlanningIrVersion, SubjectPath,
    };
    use gateway_registry::Registry;

    use super::*;

    fn desired() -> DesiredState {
        DesiredState::new(
            DesiredStateId::new("desired-application").unwrap(),
            vec![
                DesiredCondition::new(
                    gateway_domain::ConditionId::new("condition-application").unwrap(),
                    SubjectPath::from_str("architecture.boundary").unwrap(),
                    ComparisonOperator::Present,
                    None,
                )
                .unwrap(),
            ],
            ConditionExpression::condition(
                gateway_domain::ConditionId::new("condition-application").unwrap(),
            ),
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    fn current() -> CurrentState {
        ObservedState::new_v1(gateway_domain::ObservedStateId::new("state-application").unwrap())
    }

    fn snapshot() -> PlanningCapabilitySnapshot {
        let catalog = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
        let index = Registry::load_catalog(catalog)
            .unwrap()
            .capability_index()
            .unwrap();
        PlanningCapabilitySnapshot::new(index, "catalog-fixture-1", PlanningIrVersion::V1).unwrap()
    }

    fn rules() -> (
        ComparisonRules,
        DeltaDerivationRules,
        CapabilityRequirementRules,
        PlannerRules,
    ) {
        (
            ComparisonRules::default(),
            DeltaDerivationRules::default(),
            CapabilityRequirementRules::default()
                .with_observation(CapabilityId::new("architecture.boundary-validation").unwrap()),
            PlannerRules::default(),
        )
    }

    #[test]
    fn runs_the_complete_provider_neutral_planning_sequence() {
        let app = DeclarativePlanningApplication::new();
        let desired = desired();
        let current = current();
        let snapshot = snapshot();
        assert!(!snapshot.is_empty());
        assert_eq!(snapshot.identity(), "catalog-fixture-1");
        assert_eq!(snapshot.version(), PlanningIrVersion::V1);
        assert_eq!(snapshot.len(), snapshot.capability_ids().len());
        assert!(
            snapshot
                .capability_ids()
                .any(|id| id.as_str() == "architecture.boundary-validation")
        );
        let (comparison_rules, delta_rules, capability_rules, planner_rules) = rules();

        let comparison = app
            .compare_desired_to_situation(&desired, &current, None, &comparison_rules)
            .unwrap();
        assert_eq!(
            comparison.outcome(),
            gateway_domain::ComparisonOutcome::Unknown
        );

        let delta = app
            .derive_delta(
                DeltaId::new("delta-application").unwrap(),
                &desired,
                &current,
                None,
                &comparison_rules,
                &delta_rules,
            )
            .unwrap();
        assert_eq!(delta.comparison(), &comparison);
        let requirements = app
            .derive_capability_requirements(&desired, delta.delta(), &snapshot, &capability_rules)
            .unwrap();
        assert!(requirements.is_execution_ready());
        assert_eq!(requirements.requirements().len(), 1);

        let result = app
            .build_plan(&desired, delta.delta(), &requirements, &planner_rules)
            .unwrap();
        let plan = result.plan().unwrap();
        assert!(result.is_execution_ready());
        assert_eq!(plan.steps().len(), 1);
        assert!(app.validate_plan(&desired, delta.delta(), plan).is_valid());

        let rule_snapshot = PlanningRuleSnapshot::from_rules(
            &comparison_rules,
            &delta_rules,
            &capability_rules,
            &planner_rules,
        );
        assert_eq!(rule_snapshot.planner(), PlanningIrVersion::V1);
        assert_eq!(rule_snapshot.comparison(), comparison_rules.version());
        assert_eq!(rule_snapshot.delta(), delta_rules.version());
        assert_eq!(
            rule_snapshot.capability_requirements(),
            capability_rules.version()
        );
        let explanation = app
            .explain_plan(&desired, delta.delta(), &result, &snapshot, rule_snapshot)
            .unwrap();
        assert_eq!(explanation.capability_snapshot(), "catalog-fixture-1");
        assert_eq!(
            explanation.capability_snapshot_version(),
            PlanningIrVersion::V1
        );
        assert_eq!(explanation.trace().entries().len(), 1);
        assert!(explanation.to_text().contains("catalog-fixture-1"));
        assert!(explanation.to_text().contains("CapabilityRequirements"));

        let serialized = app.serialize_plan(plan).unwrap();
        assert_eq!(Plan::from_json(&serialized).unwrap(), *plan);
    }

    #[test]
    fn rejects_invalid_snapshot_and_keeps_missing_capabilities_explicit() {
        let empty_index = CapabilityIndex::default();
        assert!(
            PlanningCapabilitySnapshot::new(empty_index.clone(), "", PlanningIrVersion::V1)
                .unwrap_err()
                .code()
                == "DOMAIN_VALIDATION_ERROR"
        );
        assert!(
            PlanningCapabilitySnapshot::new(
                empty_index,
                "empty",
                PlanningIrVersion::new(2, 0).unwrap()
            )
            .unwrap_err()
            .message()
            .contains("not supported")
        );

        let app = DeclarativePlanningApplication::new();
        let desired = desired();
        let current = current();
        let (comparison_rules, delta_rules, _, _) = rules();
        let delta = app
            .derive_delta(
                DeltaId::new("delta-missing-capability").unwrap(),
                &desired,
                &current,
                None,
                &comparison_rules,
                &delta_rules,
            )
            .unwrap();
        let snapshot = PlanningCapabilitySnapshot::new(
            CapabilityIndex::default(),
            "empty-index",
            PlanningIrVersion::V1,
        )
        .unwrap();
        let capability_rules = CapabilityRequirementRules::default()
            .with_observation(CapabilityId::new("not-indexed").unwrap());
        let derivation = app
            .derive_capability_requirements(&desired, delta.delta(), &snapshot, &capability_rules)
            .unwrap();
        assert!(!derivation.is_execution_ready());
        assert!(derivation.diagnostics().iter().any(|diagnostic| {
            diagnostic.code()
                == gateway_domain::CapabilityRequirementDiagnosticCode::MissingCapabilityContract
        }));
        let planner = app
            .build_plan(
                &desired,
                delta.delta(),
                &derivation,
                &PlannerRules::default(),
            )
            .unwrap();
        assert!(planner.plan().is_none());
        assert!(
            app.explain_plan(
                &desired,
                delta.delta(),
                &planner,
                &snapshot,
                PlanningRuleSnapshot::from_rules(
                    &comparison_rules,
                    &delta_rules,
                    &capability_rules,
                    &PlannerRules::default(),
                ),
            )
            .unwrap_err()
            .message()
            .contains("without a Plan")
        );
    }

    #[test]
    fn application_errors_are_stable_and_serialization_errors_are_projected() {
        let error = PlanningApplicationError::from(ValidationError::EmptyText { field: "test" });
        assert_eq!(error.code(), "DOMAIN_VALIDATION_ERROR");
        assert!(error.to_string().contains("DOMAIN_VALIDATION_ERROR"));
        let serialization = PlanningApplicationError::from(SerializationError::Json(
            serde_json::from_str::<serde_json::Value>("not-json").unwrap_err(),
        ));
        assert_eq!(serialization.code(), "PLANNING_SERIALIZATION_ERROR");
        assert!(!serialization.message().is_empty());
        let snapshot_error = PlanningApplicationError::from(
            gateway_registry::RegistryIntegrityError::SkillNotFound {
                skill_id: gateway_domain::SkillId::new("missing-skill").unwrap(),
            },
        );
        assert_eq!(snapshot_error.code(), "CAPABILITY_SNAPSHOT_ERROR");
        assert!(snapshot_error.message().contains("missing-skill"));
    }

    #[test]
    fn situation_scope_is_checked_without_becoming_planning_state() {
        let app = DeclarativePlanningApplication::new();
        let desired = desired();
        let current = current();
        let rules = ComparisonRules::default();
        let matching = gateway_domain::SituationAssemblyInput::new(current.clone())
            .assemble(gateway_domain::SituationId::new("situation-matching").unwrap())
            .unwrap();
        assert!(
            app.compare_desired_to_situation(&desired, &current, Some(&matching), &rules)
                .is_ok()
        );

        let other_state =
            ObservedState::new_v1(gateway_domain::ObservedStateId::new("state-other").unwrap());
        let mismatched = gateway_domain::SituationAssemblyInput::new(other_state)
            .assemble(gateway_domain::SituationId::new("situation-mismatched").unwrap())
            .unwrap();
        let error = app
            .compare_desired_to_situation(&desired, &current, Some(&mismatched), &rules)
            .unwrap_err();
        assert_eq!(error.code(), "DOMAIN_VALIDATION_ERROR");
        assert!(error.message().contains("same observed snapshot"));
    }
}
