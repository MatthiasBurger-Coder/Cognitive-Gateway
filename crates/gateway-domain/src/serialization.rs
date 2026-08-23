//! The strict JSON wire contract for [`ExecutionContextIR`].
//!
//! The domain types remain independent of JSON implementation details. Their
//! serde implementations in this module expose only validated, canonical
//! values: identifiers and enums are strings, versions use `MAJOR.MINOR`, and
//! composite values retain their named fields. Deserialization always goes
//! through the domain constructors, so a syntactically valid JSON document is
//! not accepted unless it is also a valid domain value.

use std::{error::Error, fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{
    AgentId, BlockerState, CapabilityId, Constraint, ConstraintId, ConstraintKind,
    ExecutionContextIR, ExecutionContextId, ExecutionProfile, ExecutionRuntimeId, ExecutionState,
    GateState, KnowledgeQuery, OperatingMode, PolicyId, SchemaVersion, SkillId, TaskClassification,
    TaskConfidence, TaskDescriptor, TaskId, ValidationError, WorkflowId, WorkflowState,
};

/// Errors returned by the JSON convenience API.
#[derive(Debug)]
pub enum SerializationError {
    /// The payload was not valid JSON or could not be encoded by the JSON
    /// serializer.
    Json(serde_json::Error),
    /// The payload was JSON, but violated a domain invariant.
    Validation(ValidationError),
}

impl fmt::Display for SerializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "JSON serialization error: {error}"),
            Self::Validation(error) => {
                write!(formatter, "serialized value failed validation: {error}")
            }
        }
    }
}

impl Error for SerializationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Validation(error) => Some(error),
        }
    }
}

impl From<serde_json::Error> for SerializationError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<ValidationError> for SerializationError {
    fn from(error: ValidationError) -> Self {
        Self::Validation(error)
    }
}

macro_rules! string_value_serde {
    ($($type:ty),+ $(,)?) => {
        $(
            impl Serialize for $type {
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: Serializer,
                {
                    serializer.serialize_str(self.as_str())
                }
            }

            impl<'de> Deserialize<'de> for $type {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where
                    D: Deserializer<'de>,
                {
                    let value = String::deserialize(deserializer)?;
                    Self::new(value).map_err(D::Error::custom)
                }
            }
        )+
    };
}

string_value_serde!(
    AgentId,
    CapabilityId,
    ConstraintId,
    ExecutionContextId,
    ExecutionRuntimeId,
    PolicyId,
    SkillId,
    TaskId,
    WorkflowId,
    KnowledgeQuery,
);

impl Serialize for SchemaVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        SchemaVersion::from_str(&value).map_err(D::Error::custom)
    }
}

macro_rules! canonical_enum_serde {
    ($($type:ty),+ $(,)?) => {
        $(
            impl Serialize for $type {
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: Serializer,
                {
                    serializer.serialize_str(self.as_str())
                }
            }

            impl<'de> Deserialize<'de> for $type {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where
                    D: Deserializer<'de>,
                {
                    let value = String::deserialize(deserializer)?;
                    Self::from_str(&value).map_err(D::Error::custom)
                }
            }
        )+
    };
}

canonical_enum_serde!(
    BlockerState,
    ConstraintKind,
    ExecutionProfile,
    GateState,
    OperatingMode,
    WorkflowState,
);

impl Serialize for TaskConfidence {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f64(self.as_fraction())
    }
}

impl<'de> Deserialize<'de> for TaskConfidence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        TaskConfidence::new(f64::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl Serialize for TaskClassification {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireTaskClassification {
            task_type: self.task_type().to_owned(),
            confidence: self.confidence().as_fraction(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TaskClassification {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WireTaskClassification::deserialize(deserializer)?;
        wire.into_domain().map_err(D::Error::custom)
    }
}

impl Serialize for TaskDescriptor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireTaskDescriptor {
            id: self.id().to_string(),
            intent: self.intent().to_owned(),
            classification: self
                .classification()
                .map(|classification| WireTaskClassification {
                    task_type: classification.task_type().to_owned(),
                    confidence: classification.confidence().as_fraction(),
                }),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TaskDescriptor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WireTaskDescriptor::deserialize(deserializer)?;
        wire.into_domain().map_err(D::Error::custom)
    }
}

impl Serialize for Constraint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireConstraint {
            id: self.id().to_string(),
            kind: self.kind().to_string(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Constraint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WireConstraint::deserialize(deserializer)?;
        wire.into_domain().map_err(D::Error::custom)
    }
}

impl Serialize for ExecutionState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireExecutionState {
            workflow_state: self.workflow().to_string(),
            gate_state: self.gate().to_string(),
            blocker_state: self.blocker().to_string(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExecutionState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WireExecutionState::deserialize(deserializer)?;
        wire.into_domain().map_err(D::Error::custom)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireTaskClassification {
    task_type: String,
    confidence: f64,
}

impl WireTaskClassification {
    fn into_domain(self) -> Result<TaskClassification, ValidationError> {
        TaskClassification::new(self.task_type, self.confidence)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireTaskDescriptor {
    id: String,
    intent: String,
    classification: Option<WireTaskClassification>,
}

impl WireTaskDescriptor {
    fn into_domain(self) -> Result<TaskDescriptor, ValidationError> {
        let task = TaskDescriptor::new(TaskId::new(self.id)?, self.intent)?;
        self.classification
            .map_or(Ok(task.clone()), |classification| {
                let classification = classification.into_domain()?;
                task.with_classification(
                    classification.task_type().to_owned(),
                    classification.confidence().as_fraction(),
                )
            })
    }

    fn from_domain(value: &TaskDescriptor) -> Self {
        Self {
            id: value.id().to_string(),
            intent: value.intent().to_owned(),
            classification: value
                .classification()
                .map(|classification| WireTaskClassification {
                    task_type: classification.task_type().to_owned(),
                    confidence: classification.confidence().as_fraction(),
                }),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireConstraint {
    id: String,
    kind: String,
}

impl WireConstraint {
    fn into_domain(self) -> Result<Constraint, ValidationError> {
        Constraint::try_new(
            ConstraintId::new(self.id)?,
            ConstraintKind::from_str(&self.kind)?,
        )
    }

    fn from_domain(value: &Constraint) -> Self {
        Self {
            id: value.id().to_string(),
            kind: value.kind().to_string(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireExecutionState {
    workflow_state: String,
    gate_state: String,
    blocker_state: String,
}

impl WireExecutionState {
    fn into_domain(self) -> Result<ExecutionState, ValidationError> {
        ExecutionState::try_new(
            WorkflowState::from_str(&self.workflow_state)?,
            GateState::from_str(&self.gate_state)?,
            BlockerState::from_str(&self.blocker_state)?,
        )
    }

    fn from_domain(value: ExecutionState) -> Self {
        Self {
            workflow_state: value.workflow().to_string(),
            gate_state: value.gate().to_string(),
            blocker_state: value.blocker().to_string(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireExecutionContextIR {
    schema_version: String,
    id: String,
    task: WireTaskDescriptor,
    workflow_id: String,
    primary_agent_id: String,
    skill_ids: Vec<String>,
    operating_mode: String,
    execution_profile: String,
    state: WireExecutionState,
    policy_id: String,
    knowledge_queries: Vec<String>,
    approved_capability_ids: Vec<String>,
    constraints: Vec<WireConstraint>,
    target_runtime: String,
}

impl WireExecutionContextIR {
    fn from_domain(value: &ExecutionContextIR) -> Self {
        Self {
            schema_version: value.schema_version().to_string(),
            id: value.id().to_string(),
            task: WireTaskDescriptor::from_domain(value.task()),
            workflow_id: value.workflow_id().to_string(),
            primary_agent_id: value.primary_agent_id().to_string(),
            skill_ids: value.skill_ids().iter().map(ToString::to_string).collect(),
            operating_mode: value.operating_mode().to_string(),
            execution_profile: value.execution_profile().to_string(),
            state: WireExecutionState::from_domain(value.state()),
            policy_id: value.policy_id().to_string(),
            knowledge_queries: value
                .knowledge_queries()
                .iter()
                .map(|query| query.as_str().to_owned())
                .collect(),
            approved_capability_ids: value
                .approved_capability_ids()
                .iter()
                .map(ToString::to_string)
                .collect(),
            constraints: value
                .constraints()
                .iter()
                .map(WireConstraint::from_domain)
                .collect(),
            target_runtime: value.target_runtime().to_string(),
        }
    }

    fn into_domain(self) -> Result<ExecutionContextIR, ValidationError> {
        let skill_ids = self
            .skill_ids
            .into_iter()
            .map(SkillId::new)
            .collect::<Result<Vec<_>, _>>()?;
        let knowledge_queries = self
            .knowledge_queries
            .into_iter()
            .map(KnowledgeQuery::new)
            .collect::<Result<Vec<_>, _>>()?;
        let approved_capability_ids = self
            .approved_capability_ids
            .into_iter()
            .map(CapabilityId::new)
            .collect::<Result<Vec<_>, _>>()?;
        let constraints = self
            .constraints
            .into_iter()
            .map(WireConstraint::into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        ExecutionContextIR::new(
            SchemaVersion::from_str(&self.schema_version)?,
            ExecutionContextId::new(self.id)?,
            self.task.into_domain()?,
            WorkflowId::new(self.workflow_id)?,
            AgentId::new(self.primary_agent_id)?,
            skill_ids,
            OperatingMode::from_str(&self.operating_mode)?,
            ExecutionProfile::from_str(&self.execution_profile)?,
            self.state.into_domain()?,
            PolicyId::new(self.policy_id)?,
            knowledge_queries,
            approved_capability_ids,
            constraints,
            ExecutionRuntimeId::new(self.target_runtime)?,
        )
    }
}

impl Serialize for ExecutionContextIR {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireExecutionContextIR::from_domain(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExecutionContextIR {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        WireExecutionContextIR::deserialize(deserializer)?
            .into_domain()
            .map_err(D::Error::custom)
    }
}

impl ExecutionContextIR {
    /// Serializes this validated IR as compact, deterministic JSON.
    pub fn to_json(&self) -> Result<String, SerializationError> {
        serde_json::to_string(self).map_err(SerializationError::Json)
    }

    /// Serializes this validated IR as human-readable deterministic JSON.
    pub fn to_json_pretty(&self) -> Result<String, SerializationError> {
        serde_json::to_string_pretty(self).map_err(SerializationError::Json)
    }

    /// Deserializes and validates a v1 IR JSON document.
    ///
    /// Version `1.0` is currently the only supported compatibility path. All
    /// other versions, including a future minor version, are rejected rather
    /// than silently downgraded or coerced.
    pub fn from_json(value: &str) -> Result<Self, SerializationError> {
        let wire = serde_json::from_str::<WireExecutionContextIR>(value)
            .map_err(SerializationError::Json)?;
        wire.into_domain().map_err(SerializationError::Validation)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        ConstraintKind, ExecutionContextId, ExecutionProfile, ExecutionRuntimeId, GateState,
        OperatingMode, SchemaVersion, TaskId, WorkflowState,
    };

    fn context() -> ExecutionContextIR {
        ExecutionContextIR::new_v1(
            ExecutionContextId::new("context-1").unwrap(),
            TaskDescriptor::new(TaskId::new("task-1").unwrap(), "repair")
                .unwrap()
                .with_classification("runtime_bugfix", 0.94)
                .unwrap(),
            WorkflowId::new("issue-implementation").unwrap(),
            AgentId::new("senior-developer").unwrap(),
            [
                SkillId::new("quality-gate").unwrap(),
                SkillId::new("issue-workflow").unwrap(),
            ],
            OperatingMode::Hardening,
            ExecutionProfile::FullPath,
            ExecutionState::new(
                WorkflowState::Running,
                GateState::InProgress,
                BlockerState::Clear,
            )
            .unwrap(),
            PolicyId::new("safe-development").unwrap(),
            [KnowledgeQuery::new("quality gate history").unwrap()],
            [CapabilityId::new("quality.run").unwrap()],
            [Constraint::new(
                ConstraintId::new("feature-freeze").unwrap(),
                ConstraintKind::FeatureFreeze,
            )],
            ExecutionRuntimeId::new("runtime-default").unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn serializes_the_complete_ir_with_canonical_wire_values() {
        let json = context().to_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["schema_version"], "1.0");
        assert_eq!(value["operating_mode"], "HARDENING");
        assert_eq!(value["execution_profile"], "FULL_PATH");
        assert_eq!(value["state"]["gate_state"], "IN_PROGRESS");
        assert_eq!(value["constraints"][0]["kind"], "FEATURE_FREEZE");
        assert_eq!(value["task"]["classification"]["confidence"], 0.94);
        assert!(!json.contains("provider"));
    }

    #[test]
    fn round_trip_preserves_all_domain_semantics() {
        let original = context();
        let json = original.to_json().unwrap();
        let restored = ExecutionContextIR::from_json(&json).unwrap();
        let serde_restored = serde_json::from_str::<ExecutionContextIR>(&json).unwrap();

        assert_eq!(restored, original);
        assert_eq!(serde_restored, original);
        assert_eq!(serde_json::to_string(&restored).unwrap(), json);
    }

    #[test]
    fn pretty_json_is_also_accepted() {
        let original = context();
        let restored = ExecutionContextIR::from_json(&original.to_json_pretty().unwrap()).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn rejects_unknown_enum_values_without_coercion() {
        let mut value: serde_json::Value =
            serde_json::from_str(&context().to_json().unwrap()).unwrap();
        value["operating_mode"] = json!("production");

        let error = ExecutionContextIR::from_json(&value.to_string()).unwrap_err();
        assert!(error.to_string().contains("operating_mode"));
    }

    #[test]
    fn rejects_unsupported_and_malformed_schema_versions() {
        let mut value: serde_json::Value =
            serde_json::from_str(&context().to_json().unwrap()).unwrap();
        value["schema_version"] = json!("1.1");
        assert!(matches!(
            ExecutionContextIR::from_json(&value.to_string()),
            Err(SerializationError::Validation(
                ValidationError::UnsupportedSchemaVersion { .. }
            ))
        ));

        value["schema_version"] = json!("v1");
        assert!(ExecutionContextIR::from_json(&value.to_string()).is_err());
        value["schema_version"] = json!(1);
        assert!(ExecutionContextIR::from_json(&value.to_string()).is_err());
        assert_eq!(SchemaVersion::V1.to_string(), "1.0");
    }

    #[test]
    fn rejects_malformed_identifiers_and_invalid_state_combinations() {
        let mut value: serde_json::Value =
            serde_json::from_str(&context().to_json().unwrap()).unwrap();
        value["id"] = json!("../escape");
        assert!(ExecutionContextIR::from_json(&value.to_string()).is_err());

        value["id"] = json!("context-1");
        value["state"]["workflow_state"] = json!("COMPLETED");
        assert!(matches!(
            ExecutionContextIR::from_json(&value.to_string()),
            Err(SerializationError::Validation(
                ValidationError::InvalidStateCombination { .. }
            ))
        ));
    }

    #[test]
    fn rejects_duplicate_relationships_and_unknown_fields() {
        let mut value: serde_json::Value =
            serde_json::from_str(&context().to_json().unwrap()).unwrap();
        value["skill_ids"] = json!(["quality-gate", "quality-gate"]);
        assert!(matches!(
            ExecutionContextIR::from_json(&value.to_string()),
            Err(SerializationError::Validation(
                ValidationError::DuplicateRelationship { field: "skill_ids" }
            ))
        ));

        value["skill_ids"] = json!(["quality-gate", "issue-workflow"]);
        value["unexpected"] = json!(true);
        assert!(ExecutionContextIR::from_json(&value.to_string()).is_err());
    }

    #[test]
    fn direct_nested_values_use_the_same_strict_contract() {
        let task = TaskDescriptor::new(TaskId::new("task-1").unwrap(), "inspect").unwrap();
        let json = serde_json::to_string(&task).unwrap();
        assert_eq!(serde_json::from_str::<TaskDescriptor>(&json).unwrap(), task);
        assert!(serde_json::from_str::<OperatingMode>(r#""UNKNOWN""#).is_err());
        assert!(serde_json::from_str::<KnowledgeQuery>(r#""   """#).is_err());
    }

    #[test]
    fn all_typed_scalar_values_round_trip_through_serde() {
        macro_rules! round_trip {
            ($type:ty, $value:expr) => {
                let value: $type = $value;
                let json = serde_json::to_string(&value).unwrap();
                assert_eq!(serde_json::from_str::<$type>(&json).unwrap(), value);
            };
        }

        round_trip!(AgentId, AgentId::new("agent").unwrap());
        round_trip!(CapabilityId, CapabilityId::new("capability").unwrap());
        round_trip!(ConstraintId, ConstraintId::new("constraint").unwrap());
        round_trip!(
            ExecutionContextId,
            ExecutionContextId::new("context").unwrap()
        );
        round_trip!(
            ExecutionRuntimeId,
            ExecutionRuntimeId::new("runtime").unwrap()
        );
        round_trip!(PolicyId, PolicyId::new("policy").unwrap());
        round_trip!(SkillId, SkillId::new("skill").unwrap());
        round_trip!(TaskId, TaskId::new("task").unwrap());
        round_trip!(WorkflowId, WorkflowId::new("workflow").unwrap());
        round_trip!(KnowledgeQuery, KnowledgeQuery::new("knowledge").unwrap());
        round_trip!(SchemaVersion, SchemaVersion::V1);
        round_trip!(TaskConfidence, TaskConfidence::new(0.94).unwrap());

        assert!(serde_json::from_str::<TaskConfidence>("2.0").is_err());
        assert!(serde_json::from_str::<AgentId>(r#""../escape""#).is_err());
    }

    #[test]
    fn all_canonical_enum_values_round_trip_through_serde() {
        macro_rules! round_trip {
            ($type:ty, [$($value:expr),+ $(,)?]) => {
                $(
                    let value = $value;
                    let json = serde_json::to_string(&value).unwrap();
                    assert_eq!(serde_json::from_str::<$type>(&json).unwrap(), value);
                )+
            };
        }

        round_trip!(
            OperatingMode,
            [
                OperatingMode::Development,
                OperatingMode::Hardening,
                OperatingMode::ReleaseQualification,
            ]
        );
        round_trip!(
            ExecutionProfile,
            [
                ExecutionProfile::FastPath,
                ExecutionProfile::NormalPath,
                ExecutionProfile::FullPath,
            ]
        );
        round_trip!(
            WorkflowState,
            [
                WorkflowState::Pending,
                WorkflowState::Running,
                WorkflowState::Paused,
                WorkflowState::Blocked,
                WorkflowState::Completed,
                WorkflowState::Failed,
                WorkflowState::Cancelled,
            ]
        );
        round_trip!(
            GateState,
            [
                GateState::Pending,
                GateState::InProgress,
                GateState::Passed,
                GateState::Failed,
                GateState::Blocked,
                GateState::Skipped,
            ]
        );
        round_trip!(
            BlockerState,
            [
                BlockerState::Clear,
                BlockerState::Active,
                BlockerState::Resolved
            ]
        );
        round_trip!(
            ConstraintKind,
            [
                ConstraintKind::FeatureFreeze,
                ConstraintKind::LiveMutationRequiresConsent,
                ConstraintKind::RequireFullPathForReleaseQualification,
            ]
        );
    }

    #[test]
    fn composite_values_reject_invalid_nested_data() {
        let classification = TaskClassification::new("bugfix", 0.5).unwrap();
        let classification_json = serde_json::to_string(&classification).unwrap();
        assert_eq!(
            serde_json::from_str::<TaskClassification>(&classification_json).unwrap(),
            classification
        );

        let task = TaskDescriptor::new(TaskId::new("task").unwrap(), "repair")
            .unwrap()
            .with_classification("bugfix", 0.5)
            .unwrap();
        let task_json = serde_json::to_string(&task).unwrap();
        assert_eq!(
            serde_json::from_str::<TaskDescriptor>(&task_json).unwrap(),
            task
        );

        let constraint = Constraint::new(
            ConstraintId::new("freeze").unwrap(),
            ConstraintKind::FeatureFreeze,
        );
        let constraint_json = serde_json::to_string(&constraint).unwrap();
        assert_eq!(
            serde_json::from_str::<Constraint>(&constraint_json).unwrap(),
            constraint
        );

        let state = ExecutionState::new(
            WorkflowState::Running,
            GateState::InProgress,
            BlockerState::Clear,
        )
        .unwrap();
        let state_json = serde_json::to_string(&state).unwrap();
        assert_eq!(
            serde_json::from_str::<ExecutionState>(&state_json).unwrap(),
            state
        );
        assert!(
            serde_json::from_str::<ExecutionState>(
                r#"{"workflow_state":"COMPLETED","gate_state":"FAILED","blocker_state":"CLEAR"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn convenience_errors_keep_json_and_domain_failures_distinct() {
        let json_error = ExecutionContextIR::from_json("{").unwrap_err();
        assert!(matches!(json_error, SerializationError::Json(_)));
        assert!(json_error.source().is_some());
        assert!(json_error.to_string().contains("JSON"));

        let validation_error = SerializationError::from(ValidationError::InvalidSchemaVersion);
        assert!(matches!(
            validation_error,
            SerializationError::Validation(_)
        ));
        assert!(validation_error.source().is_some());
        assert!(validation_error.to_string().contains("validation"));

        let serde_error = serde_json::from_str::<serde_json::Value>("[").unwrap_err();
        let converted = SerializationError::from(serde_error);
        assert!(matches!(converted, SerializationError::Json(_)));
    }
}
