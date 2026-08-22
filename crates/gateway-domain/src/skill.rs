//! Reusable skill definitions and typed capability/knowledge relationships.

use crate::{
    AgentId, CapabilityId, KnowledgeQuery, NonEmptyText, SkillId, ValidationError,
    relationships::{reject_self_dependency, unique_relationships},
};

/// A reusable knowledge and capability contract.
///
/// Skills may depend on other skills and may declare abstract capabilities or
/// knowledge queries needed to perform their responsibility. These references
/// do not authorize execution; policy evaluation remains a separate concern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefinition {
    id: SkillId,
    description: NonEmptyText,
    owner_agent_id: Option<AgentId>,
    dependency_ids: Vec<SkillId>,
    required_capability_ids: Vec<CapabilityId>,
    knowledge_queries: Vec<KnowledgeQuery>,
}

impl SkillDefinition {
    /// Creates a skill with unique, non-self-referential dependencies.
    pub fn new(
        id: SkillId,
        description: impl Into<String>,
        dependency_ids: impl IntoIterator<Item = SkillId>,
        required_capability_ids: impl IntoIterator<Item = CapabilityId>,
    ) -> Result<Self, ValidationError> {
        let dependency_ids = unique_relationships(dependency_ids, "dependency_ids")?;
        reject_self_dependency(&id, &dependency_ids)?;

        Ok(Self {
            id,
            description: NonEmptyText::new_for_field(description, "description")?,
            owner_agent_id: None,
            dependency_ids,
            required_capability_ids: unique_relationships(
                required_capability_ids,
                "required_capability_ids",
            )?,
            knowledge_queries: Vec::new(),
        })
    }

    /// Alias for [`Self::new`] for callers at parsing boundaries.
    pub fn try_new(
        id: SkillId,
        description: impl Into<String>,
        dependency_ids: impl IntoIterator<Item = SkillId>,
        required_capability_ids: impl IntoIterator<Item = CapabilityId>,
    ) -> Result<Self, ValidationError> {
        Self::new(id, description, dependency_ids, required_capability_ids)
    }

    /// Creates a skill with an explicit owning agent relationship.
    pub fn new_with_owner(
        id: SkillId,
        description: impl Into<String>,
        owner_agent_id: AgentId,
        dependency_ids: impl IntoIterator<Item = SkillId>,
        required_capability_ids: impl IntoIterator<Item = CapabilityId>,
    ) -> Result<Self, ValidationError> {
        Ok(
            Self::new(id, description, dependency_ids, required_capability_ids)?
                .with_owner(owner_agent_id),
        )
    }

    /// Adds the skill's ordered knowledge queries while preserving value semantics.
    pub fn with_knowledge_queries(
        mut self,
        queries: impl IntoIterator<Item = KnowledgeQuery>,
    ) -> Self {
        self.knowledge_queries = queries.into_iter().collect();
        self
    }

    /// Associates the skill with an agent while preserving value semantics.
    #[must_use]
    pub fn with_owner(mut self, owner_agent_id: AgentId) -> Self {
        self.owner_agent_id = Some(owner_agent_id);
        self
    }

    /// Returns the skill identity.
    #[must_use]
    pub fn id(&self) -> &SkillId {
        &self.id
    }

    /// Returns the validated skill description.
    #[must_use]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }

    /// Returns the optional owning agent relationship.
    #[must_use]
    pub fn owner_agent_id(&self) -> Option<&AgentId> {
        self.owner_agent_id.as_ref()
    }

    /// Returns the ordered skill dependencies.
    #[must_use]
    pub fn dependency_ids(&self) -> &[SkillId] {
        &self.dependency_ids
    }

    /// Returns the abstract capabilities required by this skill.
    #[must_use]
    pub fn required_capability_ids(&self) -> &[CapabilityId] {
        &self.required_capability_ids
    }

    /// Returns the knowledge queries declared by this skill.
    #[must_use]
    pub fn knowledge_queries(&self) -> &[KnowledgeQuery] {
        &self.knowledge_queries
    }
}

#[cfg(test)]
mod tests {
    use super::SkillDefinition;
    use crate::{CapabilityId, KnowledgeQuery, SkillId, ValidationError};

    #[test]
    fn creates_a_skill_with_typed_relationships() {
        let skill = SkillDefinition::new(
            SkillId::new("repository-inspection").unwrap(),
            "Inspect repository state",
            [SkillId::new("filesystem-reading").unwrap()],
            [CapabilityId::new("repository.read").unwrap()],
        )
        .unwrap()
        .with_owner(crate::AgentId::new("reviewer").unwrap())
        .with_knowledge_queries([KnowledgeQuery::new("repository conventions").unwrap()]);

        assert_eq!(skill.id().as_str(), "repository-inspection");
        assert_eq!(skill.description(), "Inspect repository state");
        assert_eq!(skill.owner_agent_id().unwrap().as_str(), "reviewer");
        assert_eq!(skill.dependency_ids().len(), 1);
        assert_eq!(
            skill.required_capability_ids()[0].as_str(),
            "repository.read"
        );
        assert_eq!(
            skill.knowledge_queries()[0].as_str(),
            "repository conventions"
        );
    }

    #[test]
    fn allows_leaf_skills_but_rejects_invalid_dependencies() {
        assert!(
            SkillDefinition::new(
                SkillId::new("leaf").unwrap(),
                "A leaf skill",
                Vec::<SkillId>::new(),
                Vec::<CapabilityId>::new(),
            )
            .is_ok()
        );

        let id = SkillId::new("inspect").unwrap();
        assert!(matches!(
            SkillDefinition::new(id.clone(), "Inspect", [id], Vec::<CapabilityId>::new()),
            Err(ValidationError::SelfReference {
                field: "dependencies"
            })
        ));
    }

    #[test]
    fn rejects_duplicate_relationships_and_bad_text() {
        let capability = CapabilityId::new("repository.read").unwrap();
        assert!(matches!(
            SkillDefinition::new(
                SkillId::new("inspect").unwrap(),
                "Inspect",
                Vec::<SkillId>::new(),
                [capability.clone(), capability],
            ),
            Err(ValidationError::DuplicateRelationship {
                field: "required_capability_ids"
            })
        ));
        assert!(
            SkillDefinition::try_new(
                SkillId::new("inspect").unwrap(),
                "\0",
                Vec::<SkillId>::new(),
                Vec::<CapabilityId>::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn new_with_owner_keeps_the_owner_typed() {
        let skill = SkillDefinition::new_with_owner(
            SkillId::new("inspect").unwrap(),
            "Inspect",
            crate::AgentId::new("reviewer").unwrap(),
            [],
            [],
        )
        .unwrap();
        assert_eq!(skill.owner_agent_id().unwrap().as_str(), "reviewer");
    }
}
