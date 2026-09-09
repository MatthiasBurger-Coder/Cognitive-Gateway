//! CG-02 Execution Context IR v2 for lossless, fail-closed resolution handoff.
//!
//! v1 remains the executable runtime contract. v2 is a versioned handoff
//! envelope: it preserves incomplete or non-projectable CG-08 results without
//! pretending that they are executable contexts.

use serde::{Deserialize, Serialize};

use crate::{ExecutionContextIR, ValidationError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionProjectionStatus {
    Executable,
    NoTemplate,
    EmptySkills,
    MultipleAgents,
    UnmappedConstraints,
    NotCurrentlyEligible,
    PolicyRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionProjectionIssue {
    MissingWorkflow,
    MissingPrimaryAgent,
    EmptySkillClosure,
    MultipleAgents,
    UnmappedConstraint,
    NotCurrentlyEligible,
    MissingPolicyDecision,
    StaleBasis,
}

/// Lossless CG-08 handoff envelope introduced by CG-02 IR v2.
///
/// Identity fields are serialized as canonical strings at this boundary so
/// the envelope can carry absent or multiple identities. The envelope is not
/// an execution grant: only `Executable` may be adapted to v1 after the usual
/// catalog and policy validation, and all other statuses are fail-closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionContextIRV2 {
    pub schema_version: String,
    pub id: String,
    pub task: Option<String>,
    pub workflow_id: Option<String>,
    pub primary_agent_id: Option<String>,
    pub participating_agent_ids: Vec<String>,
    pub skill_ids: Vec<String>,
    pub operating_mode: String,
    pub execution_profile: String,
    pub state: Option<String>,
    pub policy_id: Option<String>,
    pub approved_capability_ids: Vec<String>,
    pub constraints: Vec<String>,
    pub target_runtime: Option<String>,
    pub resolution_basis: String,
    pub status: ExecutionProjectionStatus,
    pub issues: Vec<ExecutionProjectionIssue>,
    pub executable_v1: Option<ExecutionContextIR>,
}

impl ExecutionContextIRV2 {
    pub fn new(
        id: impl Into<String>,
        resolution_basis: impl Into<String>,
        status: ExecutionProjectionStatus,
    ) -> Result<Self, ValidationError> {
        let context = Self {
            schema_version: "2.0".to_owned(),
            id: id.into(),
            task: None,
            workflow_id: None,
            primary_agent_id: None,
            participating_agent_ids: Vec::new(),
            skill_ids: Vec::new(),
            operating_mode: String::new(),
            execution_profile: String::new(),
            state: None,
            policy_id: None,
            approved_capability_ids: Vec::new(),
            constraints: Vec::new(),
            target_runtime: None,
            resolution_basis: resolution_basis.into(),
            status,
            issues: Vec::new(),
            executable_v1: None,
        };
        context.validate()?;
        Ok(context)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != "2.0" {
            return Err(ValidationError::UnsupportedSchemaVersion {
                expected: "2.0",
                actual: self.schema_version.clone(),
            });
        }
        if self.id.trim().is_empty() || self.resolution_basis.trim().is_empty() {
            return Err(ValidationError::InvalidStateCombination {
                reason: "v2 handoff identity and resolution basis are required",
            });
        }
        if self.status == ExecutionProjectionStatus::Executable {
            if self.workflow_id.is_none() || self.primary_agent_id.is_none() {
                return Err(ValidationError::InvalidStateCombination {
                    reason: "executable v2 handoff requires workflow and primary agent",
                });
            }
            if self.skill_ids.is_empty() {
                return Err(ValidationError::EmptyRelationship { field: "skill_ids" });
            }
            if self.policy_id.is_none() {
                return Err(ValidationError::InvalidStateCombination {
                    reason: "executable v2 handoff requires external policy identity",
                });
            }
            if !self.issues.is_empty() {
                return Err(ValidationError::InvalidStateCombination {
                    reason: "executable v2 handoff cannot carry incompatibility issues",
                });
            }
            if self.executable_v1.is_none() {
                return Err(ValidationError::InvalidStateCombination {
                    reason: "executable v2 handoff requires a validated v1 context",
                });
            }
        } else if self.executable_v1.is_some() {
            return Err(ValidationError::InvalidStateCombination {
                reason: "non-executable v2 handoff cannot carry an executable v1 context",
            });
        }
        if matches!(self.status, ExecutionProjectionStatus::EmptySkills)
            && !self.skill_ids.is_empty()
        {
            return Err(ValidationError::InvalidStateCombination {
                reason: "empty-skills status must preserve an empty closure",
            });
        }
        Ok(())
    }

    pub fn from_executable_v1(
        id: impl Into<String>,
        resolution_basis: impl Into<String>,
        context: ExecutionContextIR,
    ) -> Result<Self, ValidationError> {
        let handoff = Self {
            schema_version: "2.0".to_owned(),
            id: id.into(),
            task: Some(context.task().id().to_string()),
            workflow_id: Some(context.workflow_id().to_string()),
            primary_agent_id: Some(context.primary_agent_id().to_string()),
            participating_agent_ids: vec![context.primary_agent_id().to_string()],
            skill_ids: context
                .skill_ids()
                .iter()
                .map(ToString::to_string)
                .collect(),
            operating_mode: context.operating_mode().to_string(),
            execution_profile: context.execution_profile().to_string(),
            state: Some(format!("{:?}", context.state())),
            policy_id: Some(context.policy_id().to_string()),
            approved_capability_ids: context
                .approved_capability_ids()
                .iter()
                .map(ToString::to_string)
                .collect(),
            constraints: context
                .constraints()
                .iter()
                .map(|c| c.id().to_string())
                .collect(),
            target_runtime: Some(context.target_runtime().to_string()),
            resolution_basis: resolution_basis.into(),
            status: ExecutionProjectionStatus::Executable,
            issues: Vec::new(),
            executable_v1: Some(context),
        };
        handoff.executable_v1.as_ref().unwrap().validate()?;
        handoff.validate()?;
        Ok(handoff)
    }

    pub fn into_executable_v1(self) -> Result<ExecutionContextIR, ValidationError> {
        self.validate()?;
        self.executable_v1
            .ok_or(ValidationError::InvalidStateCombination {
                reason: "handoff is not executable",
            })
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(value: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let context: Self = serde_json::from_str(value)?;
        context.validate()?;
        Ok(context)
    }
}
