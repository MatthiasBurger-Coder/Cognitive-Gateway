//! A validated set of core definitions and their cross-object invariants.

use std::collections::HashSet;

use crate::{
    AgentDefinition, AgentId, PolicyDefinition, SkillDefinition, SkillId, ValidationError,
    WorkflowDefinition,
};

/// The validated definition graph used by deterministic resolution.
///
/// The catalog owns definitions by value and exposes only immutable slices.
/// Its constructor verifies that every typed reference points to a definition
/// in the same catalog. Individual descriptor constructors still validate
/// local invariants such as duplicate and self-referential relationships.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DefinitionCatalog {
    agents: Vec<AgentDefinition>,
    skills: Vec<SkillDefinition>,
    workflows: Vec<WorkflowDefinition>,
    policies: Vec<PolicyDefinition>,
}

impl DefinitionCatalog {
    /// Creates a catalog and validates all relationships between definitions.
    pub fn new(
        agents: Vec<AgentDefinition>,
        skills: Vec<SkillDefinition>,
        workflows: Vec<WorkflowDefinition>,
        policies: Vec<PolicyDefinition>,
    ) -> Result<Self, ValidationError> {
        let catalog = Self {
            agents,
            skills,
            workflows,
            policies,
        };
        catalog.validate()?;
        Ok(catalog)
    }

    /// Alias for [`Self::new`] for profile/parsing boundaries.
    pub fn try_new(
        agents: Vec<AgentDefinition>,
        skills: Vec<SkillDefinition>,
        workflows: Vec<WorkflowDefinition>,
        policies: Vec<PolicyDefinition>,
    ) -> Result<Self, ValidationError> {
        Self::new(agents, skills, workflows, policies)
    }

    /// Re-checks the catalog invariants without changing the catalog.
    pub fn validate(&self) -> Result<(), ValidationError> {
        let agent_ids =
            unique_definition_ids(self.agents.iter().map(AgentDefinition::id), "agent")?;
        let skill_ids =
            unique_definition_ids(self.skills.iter().map(SkillDefinition::id), "skill")?;
        let _workflow_ids = unique_definition_ids(
            self.workflows.iter().map(WorkflowDefinition::id),
            "workflow",
        )?;
        let policy_ids =
            unique_definition_ids(self.policies.iter().map(PolicyDefinition::id), "policy")?;

        for agent in &self.agents {
            for skill_id in agent.skill_ids() {
                require_definition(&skill_ids, "skill", skill_id)?;
            }
        }
        for skill in &self.skills {
            if let Some(owner_agent_id) = skill.owner_agent_id() {
                require_definition(&agent_ids, "agent", owner_agent_id)?;
            }
            for dependency_id in skill.dependency_ids() {
                require_definition(&skill_ids, "skill", dependency_id)?;
            }
        }
        detect_skill_cycles(&self.skills)?;
        for workflow in &self.workflows {
            require_definition(&agent_ids, "agent", workflow.primary_agent_id())?;
            require_definition(&policy_ids, "policy", workflow.policy_id())?;
            for skill_id in workflow.skill_ids() {
                require_definition(&skill_ids, "skill", skill_id)?;
            }
        }
        Ok(())
    }

    /// Returns all registered agent definitions.
    #[must_use]
    pub fn agents(&self) -> &[AgentDefinition] {
        &self.agents
    }

    /// Finds an agent by its typed identity.
    #[must_use]
    pub fn agent(&self, id: &AgentId) -> Option<&AgentDefinition> {
        self.agents.iter().find(|agent| agent.id() == id)
    }

    /// Returns all registered skill definitions.
    #[must_use]
    pub fn skills(&self) -> &[SkillDefinition] {
        &self.skills
    }

    /// Finds a skill by its typed identity.
    #[must_use]
    pub fn skill(&self, id: &SkillId) -> Option<&SkillDefinition> {
        self.skills.iter().find(|skill| skill.id() == id)
    }

    /// Returns all registered workflow definitions.
    #[must_use]
    pub fn workflows(&self) -> &[WorkflowDefinition] {
        &self.workflows
    }

    /// Returns all registered policy definitions.
    #[must_use]
    pub fn policies(&self) -> &[PolicyDefinition] {
        &self.policies
    }

    /// Finds a workflow by its typed identity.
    #[must_use]
    pub fn workflow(&self, id: &crate::WorkflowId) -> Option<&WorkflowDefinition> {
        self.workflows.iter().find(|workflow| workflow.id() == id)
    }

    /// Finds a policy by its typed identity.
    #[must_use]
    pub fn policy(&self, id: &crate::PolicyId) -> Option<&PolicyDefinition> {
        self.policies.iter().find(|policy| policy.id() == id)
    }
}

fn detect_skill_cycles(skills: &[SkillDefinition]) -> Result<(), ValidationError> {
    let mut visited = HashSet::new();
    for skill in skills {
        visit_skill(skill.id(), skills, &mut HashSet::new(), &mut visited)?;
    }
    Ok(())
}

fn visit_skill(
    id: &SkillId,
    skills: &[SkillDefinition],
    visiting: &mut HashSet<SkillId>,
    visited: &mut HashSet<SkillId>,
) -> Result<(), ValidationError> {
    if visited.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id.clone()) {
        return Err(ValidationError::CircularRelationship {
            field: "dependency_ids",
        });
    }

    if let Some(skill) = skills.iter().find(|skill| skill.id() == id) {
        for dependency_id in skill.dependency_ids() {
            visit_skill(dependency_id, skills, visiting, visited)?;
        }
    }

    visiting.remove(id);
    visited.insert(id.clone());
    Ok(())
}

fn unique_definition_ids<'a, T>(
    ids: impl IntoIterator<Item = &'a T>,
    kind: &'static str,
) -> Result<HashSet<&'a T>, ValidationError>
where
    T: Eq + std::hash::Hash + AsRef<str> + ?Sized,
{
    let mut unique = HashSet::new();
    for id in ids {
        if !unique.insert(id) {
            return Err(ValidationError::DuplicateDefinition {
                kind,
                id: id.as_ref().to_owned(),
            });
        }
    }
    Ok(unique)
}

fn require_definition<T>(
    ids: &HashSet<&T>,
    kind: &'static str,
    id: &T,
) -> Result<(), ValidationError>
where
    T: Eq + std::hash::Hash + AsRef<str> + ?Sized,
{
    if ids.contains(id) {
        Ok(())
    } else {
        Err(ValidationError::MissingDefinition {
            kind,
            id: id.as_ref().to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::DefinitionCatalog;
    use crate::{
        AgentDefinition, AgentId, CapabilityId, PolicyDefinition, PolicyId, SkillDefinition,
        SkillId, ValidationError, WorkflowDefinition, WorkflowId,
    };

    fn graph() -> DefinitionCatalog {
        let leaf = SkillDefinition::new(
            SkillId::new("inspect").unwrap(),
            "Inspect source",
            [],
            [CapabilityId::new("repository.read").unwrap()],
        )
        .unwrap();
        let review = SkillDefinition::new(
            SkillId::new("review").unwrap(),
            "Review findings",
            [SkillId::new("inspect").unwrap()],
            [],
        )
        .unwrap();
        let agent = AgentDefinition::new(
            AgentId::new("reviewer").unwrap(),
            "Reviews changes",
            [SkillId::new("review").unwrap()],
        )
        .unwrap();
        let policy = PolicyDefinition::new(
            PolicyId::new("read-only").unwrap(),
            "Read-only policy",
            [CapabilityId::new("repository.read").unwrap()],
        )
        .unwrap();
        let workflow = WorkflowDefinition::new(
            WorkflowId::new("review-workflow").unwrap(),
            "Review workflow",
            AgentId::new("reviewer").unwrap(),
            [SkillId::new("review").unwrap()],
            PolicyId::new("read-only").unwrap(),
        )
        .unwrap();

        DefinitionCatalog::new(
            vec![agent],
            vec![leaf, review],
            vec![workflow],
            vec![policy],
        )
        .unwrap()
    }

    #[test]
    fn validates_and_exposes_the_complete_definition_graph() {
        let catalog = graph();
        assert_eq!(catalog.agents().len(), 1);
        assert_eq!(catalog.skills().len(), 2);
        assert_eq!(catalog.workflows().len(), 1);
        assert_eq!(catalog.policies().len(), 1);
        assert!(catalog.agent(&AgentId::new("reviewer").unwrap()).is_some());
        assert!(catalog.skill(&SkillId::new("review").unwrap()).is_some());
        assert!(
            catalog
                .workflow(&WorkflowId::new("review-workflow").unwrap())
                .is_some()
        );
        assert!(
            catalog
                .policy(&PolicyId::new("read-only").unwrap())
                .is_some()
        );
        assert!(catalog.agent(&AgentId::new("missing").unwrap()).is_none());
        catalog.validate().unwrap();
    }

    #[test]
    fn rejects_missing_relationship_targets() {
        let agent = AgentDefinition::new(
            AgentId::new("reviewer").unwrap(),
            "Reviews changes",
            [SkillId::new("missing").unwrap()],
        )
        .unwrap();
        let result = DefinitionCatalog::new(vec![agent], vec![], vec![], vec![]);
        assert!(matches!(
            result,
            Err(ValidationError::MissingDefinition { kind: "skill", .. })
        ));
    }

    #[test]
    fn rejects_an_unknown_skill_owner() {
        let skill = SkillDefinition::new_with_owner(
            SkillId::new("inspect").unwrap(),
            "Inspect source",
            AgentId::new("missing-agent").unwrap(),
            [],
            [],
        )
        .unwrap();
        assert!(matches!(
            DefinitionCatalog::new(vec![], vec![skill], vec![], vec![]),
            Err(ValidationError::MissingDefinition { kind: "agent", .. })
        ));
    }

    #[test]
    fn rejects_circular_skill_dependencies() {
        let first = SkillDefinition::new(
            SkillId::new("first").unwrap(),
            "First",
            [SkillId::new("second").unwrap()],
            [],
        )
        .unwrap();
        let second = SkillDefinition::new(
            SkillId::new("second").unwrap(),
            "Second",
            [SkillId::new("first").unwrap()],
            [],
        )
        .unwrap();
        assert!(matches!(
            DefinitionCatalog::new(vec![], vec![first, second], vec![], vec![]),
            Err(ValidationError::CircularRelationship {
                field: "dependency_ids"
            })
        ));
    }

    #[test]
    fn rejects_duplicate_definition_ids() {
        let first =
            SkillDefinition::new(SkillId::new("inspect").unwrap(), "Inspect source", [], [])
                .unwrap();
        let second =
            SkillDefinition::new(SkillId::new("inspect").unwrap(), "Inspect again", [], [])
                .unwrap();
        assert!(matches!(
            DefinitionCatalog::new(vec![], vec![first, second], vec![], vec![]),
            Err(ValidationError::DuplicateDefinition { kind: "skill", .. })
        ));
    }

    #[test]
    fn validates_workflow_agent_policy_and_skill_targets() {
        let workflow = WorkflowDefinition::new(
            WorkflowId::new("review").unwrap(),
            "Review",
            AgentId::new("missing-agent").unwrap(),
            [SkillId::new("missing-skill").unwrap()],
            PolicyId::new("missing-policy").unwrap(),
        )
        .unwrap();
        assert!(matches!(
            DefinitionCatalog::new(vec![], vec![], vec![workflow], vec![]),
            Err(ValidationError::MissingDefinition { kind: "agent", .. })
        ));

        let workflow = WorkflowDefinition::new(
            WorkflowId::new("review").unwrap(),
            "Review",
            AgentId::new("agent").unwrap(),
            [SkillId::new("missing-skill").unwrap()],
            PolicyId::new("policy").unwrap(),
        )
        .unwrap();
        let agent = AgentDefinition::new(
            AgentId::new("agent").unwrap(),
            "Agent",
            [SkillId::new("missing-skill").unwrap()],
        )
        .unwrap();
        let policy = PolicyDefinition::new(PolicyId::new("policy").unwrap(), "Policy", []).unwrap();
        assert!(matches!(
            DefinitionCatalog::new(vec![agent], vec![], vec![workflow], vec![policy]),
            Err(ValidationError::MissingDefinition { kind: "skill", .. })
        ));

        let workflow = WorkflowDefinition::new(
            WorkflowId::new("review").unwrap(),
            "Review",
            AgentId::new("agent").unwrap(),
            [SkillId::new("skill").unwrap()],
            PolicyId::new("missing-policy").unwrap(),
        )
        .unwrap();
        let agent = AgentDefinition::new(
            AgentId::new("agent").unwrap(),
            "Agent",
            [SkillId::new("skill").unwrap()],
        )
        .unwrap();
        let skill = SkillDefinition::new(SkillId::new("skill").unwrap(), "Skill", [], []).unwrap();
        assert!(matches!(
            DefinitionCatalog::new(vec![agent], vec![skill], vec![workflow], vec![]),
            Err(ValidationError::MissingDefinition { kind: "policy", .. })
        ));
    }
}
