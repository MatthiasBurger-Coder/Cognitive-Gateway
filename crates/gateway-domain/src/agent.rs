//! Agent definitions and their typed skill relationships.

use crate::{AgentId, NonEmptyText, SkillId, ValidationError, relationships::unique_relationships};

/// A named responsibility contract for work performed by the gateway.
///
/// An agent definition describes responsibility only. It does not contain a
/// prompt, model, runtime handle or executable behavior. At least one skill is
/// required; whether each referenced skill exists is checked by
/// [`crate::DefinitionCatalog`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDefinition {
    id: AgentId,
    description: NonEmptyText,
    skill_ids: Vec<SkillId>,
}

impl AgentDefinition {
    /// Creates an agent with at least one unique skill reference.
    pub fn new(
        id: AgentId,
        description: impl Into<String>,
        skill_ids: impl IntoIterator<Item = SkillId>,
    ) -> Result<Self, ValidationError> {
        let skill_ids = unique_relationships(skill_ids, "skill_ids")?;
        if skill_ids.is_empty() {
            return Err(ValidationError::EmptyRelationship { field: "skill_ids" });
        }

        Ok(Self {
            id,
            description: NonEmptyText::new_for_field(description, "description")?,
            skill_ids,
        })
    }

    /// Alias for [`Self::new`] for callers at parsing boundaries.
    pub fn try_new(
        id: AgentId,
        description: impl Into<String>,
        skill_ids: impl IntoIterator<Item = SkillId>,
    ) -> Result<Self, ValidationError> {
        Self::new(id, description, skill_ids)
    }

    /// Returns the agent identity.
    #[must_use]
    pub fn id(&self) -> &AgentId {
        &self.id
    }

    /// Returns the validated responsibility description.
    #[must_use]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }

    /// Returns the ordered, unique skills required by this agent.
    #[must_use]
    pub fn skill_ids(&self) -> &[SkillId] {
        &self.skill_ids
    }

    /// Alias expressing the relationship in domain language.
    #[must_use]
    pub fn skills(&self) -> &[SkillId] {
        self.skill_ids()
    }
}

#[cfg(test)]
mod tests {
    use super::AgentDefinition;
    use crate::{AgentId, SkillId, ValidationError};

    fn ids() -> [SkillId; 2] {
        [
            SkillId::new("inspect").unwrap(),
            SkillId::new("verify").unwrap(),
        ]
    }

    #[test]
    fn creates_an_immutable_agent_with_typed_skills() {
        let agent =
            AgentDefinition::new(AgentId::new("reviewer").unwrap(), "Reviews changes", ids())
                .unwrap();

        assert_eq!(agent.id().as_str(), "reviewer");
        assert_eq!(agent.description(), "Reviews changes");
        assert_eq!(agent.skills(), ids());
    }

    #[test]
    fn rejects_missing_or_duplicate_skills() {
        let id = AgentId::new("reviewer").unwrap();
        assert!(matches!(
            AgentDefinition::new(id.clone(), "Reviews changes", Vec::<SkillId>::new()),
            Err(ValidationError::EmptyRelationship { field: "skill_ids" })
        ));

        let skill = SkillId::new("inspect").unwrap();
        assert!(matches!(
            AgentDefinition::new(id, "Reviews changes", [skill.clone(), skill]),
            Err(ValidationError::DuplicateRelationship { field: "skill_ids" })
        ));
    }

    #[test]
    fn rejects_an_invalid_description() {
        assert!(AgentDefinition::try_new(AgentId::new("reviewer").unwrap(), "\0", ids(),).is_err());
    }
}
