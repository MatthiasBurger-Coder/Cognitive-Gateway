//! Workflow definitions and their typed agent, skill and policy relationships.

use crate::{
    AgentId, NonEmptyText, PolicyId, SkillId, ValidationError, relationships::unique_relationships,
};

/// A deterministic composition of one primary agent, required skills and a policy.
///
/// Workflows contain references, not embedded definitions. This keeps profile
/// loading and resolution separate from the domain value and makes missing
/// references fail closed in [`crate::DefinitionCatalog`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDefinition {
    id: crate::WorkflowId,
    description: NonEmptyText,
    primary_agent_id: AgentId,
    skill_ids: Vec<SkillId>,
    policy_id: PolicyId,
}

impl WorkflowDefinition {
    /// Creates a workflow with a mandatory primary agent, policy and skill set.
    pub fn new(
        id: crate::WorkflowId,
        description: impl Into<String>,
        primary_agent_id: AgentId,
        skill_ids: impl IntoIterator<Item = SkillId>,
        policy_id: PolicyId,
    ) -> Result<Self, ValidationError> {
        let skill_ids = unique_relationships(skill_ids, "skill_ids")?;
        if skill_ids.is_empty() {
            return Err(ValidationError::EmptyRelationship { field: "skill_ids" });
        }

        Ok(Self {
            id,
            description: NonEmptyText::new_for_field(description, "description")?,
            primary_agent_id,
            skill_ids,
            policy_id,
        })
    }

    /// Alias for [`Self::new`] for callers at parsing boundaries.
    pub fn try_new(
        id: crate::WorkflowId,
        description: impl Into<String>,
        primary_agent_id: AgentId,
        skill_ids: impl IntoIterator<Item = SkillId>,
        policy_id: PolicyId,
    ) -> Result<Self, ValidationError> {
        Self::new(id, description, primary_agent_id, skill_ids, policy_id)
    }

    /// Returns the workflow identity.
    #[must_use]
    pub fn id(&self) -> &crate::WorkflowId {
        &self.id
    }

    /// Returns the validated workflow description.
    #[must_use]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }

    /// Returns the required primary agent.
    #[must_use]
    pub fn primary_agent_id(&self) -> &AgentId {
        &self.primary_agent_id
    }

    /// Returns the ordered skills selected by this workflow.
    #[must_use]
    pub fn skill_ids(&self) -> &[SkillId] {
        &self.skill_ids
    }

    /// Returns the policy governing this workflow.
    #[must_use]
    pub fn policy_id(&self) -> &PolicyId {
        &self.policy_id
    }
}

#[cfg(test)]
mod tests {
    use super::WorkflowDefinition;
    use crate::{AgentId, PolicyId, SkillId, ValidationError, WorkflowId};

    fn workflow() -> WorkflowDefinition {
        WorkflowDefinition::new(
            WorkflowId::new("review").unwrap(),
            "Review a change",
            AgentId::new("reviewer").unwrap(),
            [
                SkillId::new("inspect").unwrap(),
                SkillId::new("verify").unwrap(),
            ],
            PolicyId::new("safe-review").unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn exposes_all_typed_workflow_relationships() {
        let workflow = workflow();
        assert_eq!(workflow.id().as_str(), "review");
        assert_eq!(workflow.description(), "Review a change");
        assert_eq!(workflow.primary_agent_id().as_str(), "reviewer");
        assert_eq!(workflow.skill_ids().len(), 2);
        assert_eq!(workflow.policy_id().as_str(), "safe-review");
    }

    #[test]
    fn requires_at_least_one_skill_and_rejects_duplicates() {
        assert!(matches!(
            WorkflowDefinition::new(
                WorkflowId::new("review").unwrap(),
                "Review",
                AgentId::new("reviewer").unwrap(),
                Vec::<SkillId>::new(),
                PolicyId::new("safe-review").unwrap(),
            ),
            Err(ValidationError::EmptyRelationship { field: "skill_ids" })
        ));

        let skill = SkillId::new("inspect").unwrap();
        assert!(matches!(
            WorkflowDefinition::new(
                WorkflowId::new("review").unwrap(),
                "Review",
                AgentId::new("reviewer").unwrap(),
                [skill.clone(), skill],
                PolicyId::new("safe-review").unwrap(),
            ),
            Err(ValidationError::DuplicateRelationship { field: "skill_ids" })
        ));
    }

    #[test]
    fn try_new_rejects_an_invalid_description() {
        assert!(
            WorkflowDefinition::try_new(
                crate::WorkflowId::new("review").unwrap(),
                "\0",
                crate::AgentId::new("reviewer").unwrap(),
                [crate::SkillId::new("inspect").unwrap()],
                crate::PolicyId::new("safe-review").unwrap(),
            )
            .is_err()
        );
    }
}
