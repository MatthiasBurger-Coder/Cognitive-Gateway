//! Canonical JSON serialization for Delta and declarative Plan artifacts.
//!
//! Wire values are deliberately separate from the domain structs.  Every
//! deserialization path reconstructs the domain value through its validating
//! constructors, canonicalizes collections and rejects unknown fields.

use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{
    CapabilityConstraint, CapabilityPrecondition, CapabilityRequirement, ConditionId, Delta,
    DeltaBasis, DeltaId, DeltaItem, DeltaItemId, DeltaKind, DeltaReasonCode, DesiredStateId,
    LifecycleRequirement, LifecycleRequirementKind, Plan, PlanCondition, PlanId, PlanStep,
    PlanStepId, PlanStepKind, PlanningIrVersion, RequiredOutcome, RequiredOutcomeKind,
    SerializationError, SubjectPath, ValidationError,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireRequiredOutcome {
    kind: String,
    description: String,
    subject: Option<String>,
    expected: Option<crate::TypedValue>,
}

impl WireRequiredOutcome {
    fn from_domain(value: &RequiredOutcome) -> Self {
        Self {
            kind: value.kind().to_string(),
            description: value.description().to_owned(),
            subject: value.subject().map(ToString::to_string),
            expected: value.expected().cloned(),
        }
    }

    fn into_domain(self) -> Result<RequiredOutcome, ValidationError> {
        let mut outcome =
            RequiredOutcome::new(RequiredOutcomeKind::from_str(&self.kind)?, self.description)?;
        if let Some(subject) = self.subject {
            outcome = outcome.with_subject(SubjectPath::from_str(&subject)?);
        }
        if let Some(expected) = self.expected {
            outcome = outcome.with_expected(expected)?;
        }
        Ok(outcome)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireDeltaBasis {
    situation: Option<String>,
    current_state: Option<String>,
    state_subjects: Vec<String>,
    facts: Vec<String>,
    observations: Vec<String>,
    evidence: Vec<String>,
    provenances: Vec<String>,
    assessments: Vec<String>,
}

impl WireDeltaBasis {
    fn from_domain(value: &DeltaBasis) -> Self {
        Self {
            situation: value.situation().map(ToString::to_string),
            current_state: value.current_state().map(ToString::to_string),
            state_subjects: value
                .state_subjects()
                .iter()
                .map(ToString::to_string)
                .collect(),
            facts: value.facts().iter().map(ToString::to_string).collect(),
            observations: value
                .observations()
                .iter()
                .map(ToString::to_string)
                .collect(),
            evidence: value.evidence().iter().map(ToString::to_string).collect(),
            provenances: value
                .provenances()
                .iter()
                .map(ToString::to_string)
                .collect(),
            assessments: value
                .assessments()
                .iter()
                .map(ToString::to_string)
                .collect(),
        }
    }

    fn into_domain(self) -> Result<DeltaBasis, ValidationError> {
        DeltaBasis::new(
            parse_optional(self.situation)?,
            parse_optional(self.current_state)?,
            self.state_subjects
                .into_iter()
                .map(|value| SubjectPath::from_str(&value))
                .collect::<Result<Vec<_>, _>>()?,
            parse_vec(self.facts)?,
            parse_vec(self.observations)?,
            parse_vec(self.evidence)?,
            parse_vec(self.provenances)?,
            parse_vec(self.assessments)?,
        )
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireDeltaItem {
    id: String,
    desired_state: String,
    condition: String,
    kind: String,
    reason: String,
    basis: WireDeltaBasis,
    required_outcome: WireRequiredOutcome,
    rationale: String,
}

impl WireDeltaItem {
    fn from_domain(value: &DeltaItem) -> Self {
        Self {
            id: value.id().to_string(),
            desired_state: value.desired_state().to_string(),
            condition: value.condition().to_string(),
            kind: value.kind().to_string(),
            reason: value.reason().to_string(),
            basis: WireDeltaBasis::from_domain(value.basis()),
            required_outcome: WireRequiredOutcome::from_domain(value.required_outcome()),
            rationale: value.rationale().to_owned(),
        }
    }

    fn into_domain(self) -> Result<DeltaItem, ValidationError> {
        DeltaItem::new_with_reason(
            DeltaItemId::new(self.id)?,
            DesiredStateId::new(self.desired_state)?,
            ConditionId::new(self.condition)?,
            DeltaKind::from_str(&self.kind)?,
            DeltaReasonCode::from_str(&self.reason)?,
            self.basis.into_domain()?,
            self.required_outcome.into_domain()?,
            self.rationale,
        )
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireDelta {
    version: String,
    id: String,
    desired_state: String,
    situation: Option<String>,
    items: Vec<WireDeltaItem>,
}

impl WireDelta {
    fn from_domain(value: &Delta) -> Self {
        Self {
            version: value.version().to_string(),
            id: value.id().to_string(),
            desired_state: value.desired_state().to_string(),
            situation: value.situation().map(ToString::to_string),
            items: value
                .items()
                .iter()
                .map(WireDeltaItem::from_domain)
                .collect(),
        }
    }

    fn into_domain(self) -> Result<Delta, ValidationError> {
        Delta::new_with_version(
            PlanningIrVersion::from_str(&self.version)?,
            DeltaId::new(self.id)?,
            DesiredStateId::new(self.desired_state)?,
            parse_optional(self.situation)?,
            self.items
                .into_iter()
                .map(WireDeltaItem::into_domain)
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireCapabilityRequirement {
    id: String,
    capability: String,
    cardinality: String,
    originating_delta_item: String,
    preconditions: Vec<String>,
    constraints: Vec<String>,
    rationale: String,
}

impl WireCapabilityRequirement {
    fn from_domain(value: &CapabilityRequirement) -> Self {
        Self {
            id: value.id().to_string(),
            capability: value.capability().to_string(),
            cardinality: value.cardinality().to_string(),
            originating_delta_item: value.originating_delta_item().to_string(),
            preconditions: value
                .preconditions()
                .iter()
                .map(ToString::to_string)
                .collect(),
            constraints: value
                .constraints()
                .iter()
                .map(ToString::to_string)
                .collect(),
            rationale: value.rationale().to_owned(),
        }
    }

    fn into_domain(self) -> Result<CapabilityRequirement, ValidationError> {
        CapabilityRequirement::new_with_metadata(
            crate::CapabilityRequirementId::new(self.id)?,
            crate::CapabilityId::new(self.capability)?,
            crate::RequirementCardinality::from_str(&self.cardinality)?,
            DeltaItemId::new(self.originating_delta_item)?,
            self.preconditions
                .into_iter()
                .map(CapabilityPrecondition::new)
                .collect::<Result<Vec<_>, _>>()?,
            self.constraints
                .into_iter()
                .map(CapabilityConstraint::new)
                .collect::<Result<Vec<_>, _>>()?,
            self.rationale,
        )
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum WirePlanCondition {
    #[serde(rename = "DESIRED_CONDITION")]
    DesiredCondition(String),
    #[serde(rename = "OUTCOME")]
    Outcome(WireRequiredOutcome),
}

impl WirePlanCondition {
    fn from_domain(value: &PlanCondition) -> Self {
        match value {
            PlanCondition::DesiredCondition(id) => Self::DesiredCondition(id.to_string()),
            PlanCondition::Outcome(outcome) => {
                Self::Outcome(WireRequiredOutcome::from_domain(outcome))
            }
        }
    }

    fn into_domain(self) -> Result<PlanCondition, ValidationError> {
        match self {
            Self::DesiredCondition(id) => {
                Ok(PlanCondition::desired_condition(ConditionId::new(id)?))
            }
            Self::Outcome(outcome) => Ok(PlanCondition::outcome(outcome.into_domain()?)),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireLifecycleRequirement {
    kind: String,
    description: String,
}

impl WireLifecycleRequirement {
    fn from_domain(value: &LifecycleRequirement) -> Self {
        Self {
            kind: value.kind().to_string(),
            description: value.description().to_owned(),
        }
    }

    fn into_domain(self) -> Result<LifecycleRequirement, ValidationError> {
        LifecycleRequirement::new(
            LifecycleRequirementKind::from_str(&self.kind)?,
            self.description,
        )
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WirePlanStep {
    id: String,
    kind: String,
    outcome: WireRequiredOutcome,
    dependencies: Vec<String>,
    capability_requirements: Vec<String>,
    delta_items: Vec<String>,
    prerequisites: Vec<WirePlanCondition>,
    completion: WirePlanCondition,
    verification: Option<WirePlanCondition>,
    lifecycle_requirement: Option<WireLifecycleRequirement>,
    rationale: String,
}

impl WirePlanStep {
    fn from_domain(value: &PlanStep) -> Self {
        Self {
            id: value.id().to_string(),
            kind: value.kind().to_string(),
            outcome: WireRequiredOutcome::from_domain(value.outcome()),
            dependencies: value
                .dependencies()
                .iter()
                .map(ToString::to_string)
                .collect(),
            capability_requirements: value
                .capability_requirements()
                .iter()
                .map(ToString::to_string)
                .collect(),
            delta_items: value
                .delta_items()
                .iter()
                .map(ToString::to_string)
                .collect(),
            prerequisites: value
                .prerequisites()
                .iter()
                .map(WirePlanCondition::from_domain)
                .collect(),
            completion: WirePlanCondition::from_domain(value.completion()),
            verification: value.verification().map(WirePlanCondition::from_domain),
            lifecycle_requirement: value
                .lifecycle_requirement()
                .map(WireLifecycleRequirement::from_domain),
            rationale: value.rationale().to_owned(),
        }
    }

    fn into_domain(self) -> Result<PlanStep, ValidationError> {
        let mut step = PlanStep::new(
            PlanStepId::new(self.id)?,
            PlanStepKind::from_str(&self.kind)?,
            self.outcome.into_domain()?,
            self.completion.into_domain()?,
            self.rationale,
        )?
        .with_dependencies(parse_vec(self.dependencies)?)?
        .with_capability_requirements(parse_vec(self.capability_requirements)?)?
        .with_delta_items(parse_vec(self.delta_items)?)?
        .with_prerequisites(
            self.prerequisites
                .into_iter()
                .map(WirePlanCondition::into_domain)
                .collect::<Result<Vec<_>, _>>()?,
        )?;
        if let Some(verification) = self.verification {
            step = step.with_verification(verification.into_domain()?);
        }
        if let Some(lifecycle) = self.lifecycle_requirement {
            step = step.with_lifecycle_requirement(lifecycle.into_domain()?);
        }
        Ok(step)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WirePlan {
    version: String,
    id: String,
    desired_state: String,
    delta: String,
    capability_requirements: Vec<WireCapabilityRequirement>,
    steps: Vec<WirePlanStep>,
}

impl WirePlan {
    fn from_domain(value: &Plan) -> Self {
        Self {
            version: value.version().to_string(),
            id: value.id().to_string(),
            desired_state: value.desired_state().to_string(),
            delta: value.delta().to_string(),
            capability_requirements: value
                .capability_requirements()
                .iter()
                .map(WireCapabilityRequirement::from_domain)
                .collect(),
            steps: value
                .steps()
                .iter()
                .map(WirePlanStep::from_domain)
                .collect(),
        }
    }

    fn into_domain(self) -> Result<Plan, ValidationError> {
        Plan::new_with_version(
            PlanningIrVersion::from_str(&self.version)?,
            PlanId::new(self.id)?,
            DesiredStateId::new(self.desired_state)?,
            DeltaId::new(self.delta)?,
            self.capability_requirements
                .into_iter()
                .map(WireCapabilityRequirement::into_domain)
                .collect::<Result<Vec<_>, _>>()?,
            self.steps
                .into_iter()
                .map(WirePlanStep::into_domain)
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}

impl Serialize for Delta {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireDelta::from_domain(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Delta {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        WireDelta::deserialize(deserializer)?
            .into_domain()
            .map_err(D::Error::custom)
    }
}

impl Delta {
    /// Serializes a Delta as compact canonical JSON.
    pub fn to_json(&self) -> Result<String, SerializationError> {
        serde_json::to_string(self).map_err(SerializationError::Json)
    }

    /// Serializes a Delta as human-readable canonical JSON.
    pub fn to_json_pretty(&self) -> Result<String, SerializationError> {
        serde_json::to_string_pretty(self).map_err(SerializationError::Json)
    }

    /// Deserializes and validates a canonical Delta JSON document.
    pub fn from_json(value: &str) -> Result<Self, SerializationError> {
        let wire = serde_json::from_str::<WireDelta>(value).map_err(SerializationError::Json)?;
        wire.into_domain().map_err(SerializationError::Validation)
    }
}

impl Serialize for Plan {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WirePlan::from_domain(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Plan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        WirePlan::deserialize(deserializer)?
            .into_domain()
            .map_err(D::Error::custom)
    }
}

impl Plan {
    /// Serializes a Plan as compact canonical JSON.
    pub fn to_json(&self) -> Result<String, SerializationError> {
        serde_json::to_string(self).map_err(SerializationError::Json)
    }

    /// Serializes a Plan as human-readable canonical JSON.
    pub fn to_json_pretty(&self) -> Result<String, SerializationError> {
        serde_json::to_string_pretty(self).map_err(SerializationError::Json)
    }

    /// Deserializes and validates a canonical Plan JSON document.
    pub fn from_json(value: &str) -> Result<Self, SerializationError> {
        let wire = serde_json::from_str::<WirePlan>(value).map_err(SerializationError::Json)?;
        wire.into_domain().map_err(SerializationError::Validation)
    }
}

fn parse_optional<T>(value: Option<String>) -> Result<Option<T>, ValidationError>
where
    T: TryFrom<String, Error = ValidationError>,
{
    value.map(T::try_from).transpose()
}

fn parse_vec<T>(values: Vec<String>) -> Result<Vec<T>, ValidationError>
where
    T: TryFrom<String, Error = ValidationError>,
{
    values.into_iter().map(T::try_from).collect()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use serde_json::json;

    use crate::{
        CapabilityConstraint, CapabilityPrecondition, CurrentStateId, DeltaBasis, DeltaId,
        DeltaItem, DeltaKind, DesiredStateId, LifecycleRequirement, LifecycleRequirementKind,
        PlanCondition, PlanId, PlanStep, PlanStepId, PlanStepKind, RequiredOutcome,
        RequiredOutcomeKind, SubjectPath, TypedValue,
    };

    use super::*;

    fn item() -> DeltaItem {
        DeltaItem::new_with_reason(
            crate::DeltaItemId::new("item-1").unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            crate::ConditionId::new("condition-1").unwrap(),
            DeltaKind::Violation,
            crate::DeltaReasonCode::ExplicitViolation,
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
            RequiredOutcome::new(RequiredOutcomeKind::DomainChange, "repair the service")
                .unwrap()
                .with_subject(SubjectPath::from_str("service.status").unwrap())
                .with_expected(TypedValue::symbol("healthy").unwrap())
                .unwrap(),
            "explicit violation requires repair",
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

    fn requirement() -> CapabilityRequirement {
        CapabilityRequirement::new_with_metadata(
            crate::CapabilityRequirementId::new("requirement-1").unwrap(),
            crate::CapabilityId::new("domain.change").unwrap(),
            crate::RequirementCardinality::Mandatory,
            crate::DeltaItemId::new("item-1").unwrap(),
            vec![CapabilityPrecondition::new("fresh-state").unwrap()],
            vec![CapabilityConstraint::new("bounded-change").unwrap()],
            "repair capability",
        )
        .unwrap()
    }

    fn plan() -> Plan {
        let prerequisite = PlanCondition::outcome(
            RequiredOutcome::new(RequiredOutcomeKind::Observation, "observe first").unwrap(),
        );
        let change = PlanStep::new(
            PlanStepId::new("step-change").unwrap(),
            PlanStepKind::Change,
            item().required_outcome().clone(),
            PlanCondition::desired_condition(crate::ConditionId::new("condition-1").unwrap()),
            "repair step",
        )
        .unwrap()
        .with_capability_requirements(vec![
            crate::CapabilityRequirementId::new("requirement-1").unwrap(),
        ])
        .unwrap()
        .with_delta_items(vec![crate::DeltaItemId::new("item-1").unwrap()])
        .unwrap()
        .with_prerequisites(vec![prerequisite])
        .unwrap();
        let verification = PlanStep::new(
            PlanStepId::new("step-verification").unwrap(),
            PlanStepKind::Verification,
            RequiredOutcome::new(RequiredOutcomeKind::Assessment, "verify repair").unwrap(),
            PlanCondition::desired_condition(crate::ConditionId::new("condition-1").unwrap()),
            "verification step",
        )
        .unwrap()
        .with_dependencies(vec![PlanStepId::new("step-change").unwrap()])
        .unwrap()
        .with_capability_requirements(vec![
            crate::CapabilityRequirementId::new("requirement-1").unwrap(),
        ])
        .unwrap()
        .with_delta_items(vec![crate::DeltaItemId::new("item-1").unwrap()])
        .unwrap()
        .with_verification(PlanCondition::outcome(
            RequiredOutcome::new(RequiredOutcomeKind::Assessment, "verification evidence").unwrap(),
        ))
        .with_lifecycle_requirement(
            LifecycleRequirement::new(
                LifecycleRequirementKind::VerificationAfterChange,
                "verification follows repair",
            )
            .unwrap(),
        );
        Plan::new(
            PlanId::new("plan-1").unwrap(),
            DesiredStateId::new("desired-1").unwrap(),
            DeltaId::new("delta-1").unwrap(),
            vec![requirement()],
            vec![verification, change],
        )
        .unwrap()
    }

    #[test]
    fn delta_and_plan_round_trip_with_all_planning_fields() {
        let delta = delta();
        let delta_json = delta.to_json().unwrap();
        assert_eq!(Delta::from_json(&delta_json).unwrap(), delta);
        assert_eq!(serde_json::from_str::<Delta>(&delta_json).unwrap(), delta);
        assert!(delta.to_json_pretty().unwrap().contains("\n"));

        let plan = plan();
        let plan_json = plan.to_json().unwrap();
        assert_eq!(Plan::from_json(&plan_json).unwrap(), plan);
        assert_eq!(serde_json::from_str::<Plan>(&plan_json).unwrap(), plan);
        assert!(plan.to_json_pretty().unwrap().contains("\n"));
        assert!(plan_json.contains("requirement-1"));
        assert!(plan_json.contains("VERIFICATION_AFTER_CHANGE"));
    }

    #[test]
    fn reordered_collections_canonicalize_to_the_same_semantics() {
        let plan = plan();
        let mut value: serde_json::Value = serde_json::from_str(&plan.to_json().unwrap()).unwrap();
        value["steps"].as_array_mut().unwrap().reverse();
        value["capability_requirements"]
            .as_array_mut()
            .unwrap()
            .reverse();
        let restored = Plan::from_json(&serde_json::to_string(&value).unwrap()).unwrap();
        assert_eq!(restored, plan);
        assert_eq!(restored.to_json().unwrap(), plan.to_json().unwrap());

        let delta = delta();
        let mut delta_value: serde_json::Value =
            serde_json::from_str(&delta.to_json().unwrap()).unwrap();
        delta_value["items"].as_array_mut().unwrap().reverse();
        assert_eq!(
            Delta::from_json(&serde_json::to_string(&delta_value).unwrap()).unwrap(),
            delta
        );
    }

    #[test]
    fn malformed_wire_values_fail_closed_and_concrete_fields_are_rejected() {
        assert!(matches!(
            Delta::from_json(
                "{\"version\":\"1.0\",\"id\":\"delta-1\",\"desired_state\":\"desired-1\",\"situation\":null,\"items\":[],\"agent\":\"concrete\"}"
            ),
            Err(SerializationError::Json(_))
        ));
        assert!(matches!(
            Plan::from_json(
                "{\"version\":\"2.0\",\"id\":\"plan-1\",\"desired_state\":\"desired-1\",\"delta\":\"delta-1\",\"capability_requirements\":[],\"steps\":[]}"
            ),
            Err(SerializationError::Validation(
                ValidationError::UnsupportedSchemaVersion { .. }
            ))
        ));
        let mut cyclic = serde_json::to_value(plan()).unwrap();
        cyclic["steps"][0]["dependencies"] = json!(["step-verification"]);
        assert!(matches!(
            Plan::from_json(&serde_json::to_string(&cyclic).unwrap()),
            Err(SerializationError::Validation(
                ValidationError::CircularRelationship { .. }
            ))
        ));
        assert!(matches!(
            Plan::from_json("not-json"),
            Err(SerializationError::Json(_))
        ));
    }
}
