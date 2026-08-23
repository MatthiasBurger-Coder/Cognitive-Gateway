//! Reusable skill definitions and typed capability/knowledge relationships.

use crate::{
    AgentId, CapabilityDefinition, CapabilityId, KnowledgeQuery, NonEmptyText, SkillId,
    ValidationError,
    capability::unique_capabilities,
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
    name: NonEmptyText,
    description: NonEmptyText,
    owner_agent_id: Option<AgentId>,
    dependency_ids: Vec<SkillId>,
    required_capability_ids: Vec<CapabilityId>,
    provided_capabilities: Vec<CapabilityDefinition>,
    knowledge_queries: Vec<KnowledgeQuery>,
    authoritative_sources: Vec<NonEmptyText>,
    rules: Vec<NonEmptyText>,
    verification: Vec<NonEmptyText>,
    related_skill_ids: Vec<SkillId>,
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

        let name = NonEmptyText::new_for_field(id.as_str(), "name")?;

        Ok(Self {
            id,
            name,
            description: NonEmptyText::new_for_field(description, "description")?,
            owner_agent_id: None,
            dependency_ids,
            required_capability_ids: unique_relationships(
                required_capability_ids,
                "required_capability_ids",
            )?,
            provided_capabilities: Vec::new(),
            knowledge_queries: Vec::new(),
            authoritative_sources: Vec::new(),
            rules: Vec::new(),
            verification: Vec::new(),
            related_skill_ids: Vec::new(),
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

    /// Adds the reusable capabilities directly provided by this Skill.
    pub fn with_provided_capabilities(
        mut self,
        capabilities: impl IntoIterator<Item = CapabilityDefinition>,
    ) -> Result<Self, ValidationError> {
        self.provided_capabilities = unique_capabilities(capabilities)?;
        Ok(self)
    }

    /// Alias for [`Self::with_provided_capabilities`].
    pub fn with_capabilities(
        self,
        capabilities: impl IntoIterator<Item = CapabilityDefinition>,
    ) -> Result<Self, ValidationError> {
        self.with_provided_capabilities(capabilities)
    }

    /// Sets the human-readable skill name.
    pub fn with_name(mut self, name: impl Into<String>) -> Result<Self, ValidationError> {
        self.name = NonEmptyText::new_for_field(name, "name")?;
        Ok(self)
    }

    /// Sets the authoritative source selectors retained by the skill.
    pub fn with_authoritative_sources(
        mut self,
        sources: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ValidationError> {
        self.authoritative_sources = sources
            .into_iter()
            .map(|source| NonEmptyText::new_for_field(source, "authoritative_sources"))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self)
    }

    /// Sets the declarative rules retained by the skill.
    pub fn with_rules(
        mut self,
        rules: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ValidationError> {
        self.rules = rules
            .into_iter()
            .map(|rule| NonEmptyText::new_for_field(rule, "rules"))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self)
    }

    /// Sets the verification guidance retained by the skill.
    pub fn with_verification(
        mut self,
        verification: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, ValidationError> {
        self.verification = verification
            .into_iter()
            .map(|item| NonEmptyText::new_for_field(item, "verification"))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self)
    }

    /// Sets optional or related Skill references.
    pub fn with_related_skill_ids(
        mut self,
        related_skill_ids: impl IntoIterator<Item = SkillId>,
    ) -> Result<Self, ValidationError> {
        let related_skill_ids = unique_relationships(related_skill_ids, "related_skill_ids")?;
        if related_skill_ids.iter().any(|related| related == &self.id) {
            return Err(ValidationError::SelfReference {
                field: "related_skill_ids",
            });
        }
        if related_skill_ids
            .iter()
            .any(|related| self.dependency_ids.contains(related))
        {
            return Err(ValidationError::ConflictingRelationship {
                field: "skill_references",
            });
        }
        self.related_skill_ids = related_skill_ids;
        Ok(self)
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

    /// Returns the human-readable skill name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
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

    /// Returns the reusable capabilities directly provided by this Skill.
    #[must_use]
    pub fn provided_capabilities(&self) -> &[CapabilityDefinition] {
        &self.provided_capabilities
    }

    /// Alias for callers that use the shorter capability vocabulary.
    #[must_use]
    pub fn capabilities(&self) -> &[CapabilityDefinition] {
        self.provided_capabilities()
    }

    /// Returns the knowledge queries declared by this skill.
    #[must_use]
    pub fn knowledge_queries(&self) -> &[KnowledgeQuery] {
        &self.knowledge_queries
    }

    /// Returns authoritative source selectors or patterns.
    #[must_use]
    pub fn authoritative_sources(&self) -> &[NonEmptyText] {
        &self.authoritative_sources
    }

    /// Returns declarative rules.
    #[must_use]
    pub fn rules(&self) -> &[NonEmptyText] {
        &self.rules
    }

    /// Returns verification guidance.
    #[must_use]
    pub fn verification(&self) -> &[NonEmptyText] {
        &self.verification
    }

    /// Returns optional or related Skill references.
    #[must_use]
    pub fn related_skill_ids(&self) -> &[SkillId] {
        &self.related_skill_ids
    }

    /// Alias for the required Skill references.
    #[must_use]
    pub fn requires(&self) -> &[SkillId] {
        self.dependency_ids()
    }

    /// Alias using the contract's canonical `requires` terminology.
    #[must_use]
    pub fn required_skill_ids(&self) -> &[SkillId] {
        self.dependency_ids()
    }
}

#[cfg(test)]
mod tests {
    use super::SkillDefinition;
    use crate::{CapabilityDefinition, CapabilityId, KnowledgeQuery, SkillId, ValidationError};

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

    #[test]
    fn preserves_complete_content_and_separates_related_references() {
        let required = SkillId::new("foundation").unwrap();
        let related = SkillId::new("quality").unwrap();
        let skill = SkillDefinition::new(
            SkillId::new("architecture").unwrap(),
            "Architecture boundaries",
            [required.clone()],
            [],
        )
        .unwrap()
        .with_name("Architecture Expert")
        .unwrap()
        .with_authoritative_sources(["architecture guide"])
        .unwrap()
        .with_rules(["Keep dependencies directed inward."])
        .unwrap()
        .with_verification(["Run architecture tests."])
        .unwrap()
        .with_related_skill_ids([related.clone()])
        .unwrap();

        assert_eq!(skill.name(), "Architecture Expert");
        assert_eq!(
            skill.authoritative_sources()[0].as_str(),
            "architecture guide"
        );
        assert_eq!(
            skill.rules()[0].as_str(),
            "Keep dependencies directed inward."
        );
        assert_eq!(skill.verification()[0].as_str(), "Run architecture tests.");
        assert_eq!(skill.requires(), std::slice::from_ref(&required));
        assert_eq!(skill.required_skill_ids(), std::slice::from_ref(&required));
        assert_eq!(skill.related_skill_ids(), &[related]);
    }

    #[test]
    fn rejects_related_self_references_and_overlap_with_required_references() {
        let id = SkillId::new("architecture").unwrap();
        let result = SkillDefinition::new(id.clone(), "Architecture", [], [])
            .unwrap()
            .with_related_skill_ids([id]);
        assert!(matches!(result, Err(ValidationError::SelfReference { .. })));

        let required = SkillId::new("foundation").unwrap();
        let result = SkillDefinition::new(
            SkillId::new("architecture").unwrap(),
            "Architecture",
            [required.clone()],
            [],
        )
        .unwrap()
        .with_related_skill_ids([required]);
        assert!(matches!(
            result,
            Err(ValidationError::ConflictingRelationship {
                field: "skill_references"
            })
        ));
    }

    #[test]
    fn validates_each_structured_content_collection() {
        let skill = SkillDefinition::new(
            SkillId::new("architecture").unwrap(),
            "Architecture",
            [],
            [],
        )
        .unwrap();
        assert!(matches!(
            skill.clone().with_name("\0"),
            Err(ValidationError::ControlCharacter { field: "name" })
        ));
        assert!(matches!(
            skill.clone().with_authoritative_sources(["\0"]),
            Err(ValidationError::ControlCharacter {
                field: "authoritative_sources"
            })
        ));
        assert!(matches!(
            skill.clone().with_rules(["\0"]),
            Err(ValidationError::ControlCharacter { field: "rules" })
        ));
        assert!(matches!(
            skill.clone().with_verification(["\0"]),
            Err(ValidationError::ControlCharacter {
                field: "verification"
            })
        ));
        assert!(matches!(
            skill.with_related_skill_ids([
                SkillId::new("quality").unwrap(),
                SkillId::new("quality").unwrap(),
            ]),
            Err(ValidationError::DuplicateRelationship {
                field: "related_skill_ids"
            })
        ));
    }

    #[test]
    fn exposes_provided_capabilities_separately_from_requirements() {
        let capability = CapabilityDefinition::new(
            CapabilityId::new("repository.read").unwrap(),
            crate::CapabilityClass::Inspect,
        );
        let skill = SkillDefinition::new(
            SkillId::new("architecture").unwrap(),
            "Architecture",
            [],
            [CapabilityId::new("repository.read").unwrap()],
        )
        .unwrap()
        .with_provided_capabilities([capability.clone()])
        .unwrap();

        assert_eq!(
            skill.required_capability_ids()[0].as_str(),
            "repository.read"
        );
        assert_eq!(skill.provided_capabilities(), &[capability]);
    }

    #[test]
    fn supports_the_capability_alias_and_all_content_accessors() {
        let capability = CapabilityDefinition::new(
            CapabilityId::new("repository.read").unwrap(),
            crate::CapabilityClass::Inspect,
        );
        let dependency = SkillId::new("foundation").unwrap();
        let related = SkillId::new("quality").unwrap();
        let skill = SkillDefinition::new(
            SkillId::new("architecture").unwrap(),
            "Architecture boundaries",
            [dependency.clone()],
            [CapabilityId::new("repository.read").unwrap()],
        )
        .unwrap()
        .with_name("Architecture Expert")
        .unwrap()
        .with_owner(crate::AgentId::new("reviewer").unwrap())
        .with_authoritative_sources(["architecture guide"])
        .unwrap()
        .with_rules(["Keep dependencies directed inward."])
        .unwrap()
        .with_verification(["Run architecture tests."])
        .unwrap()
        .with_related_skill_ids([related.clone()])
        .unwrap()
        .with_knowledge_queries([KnowledgeQuery::new("architecture boundaries").unwrap()])
        .with_capabilities([capability.clone()])
        .unwrap();

        assert_eq!(skill.name(), "Architecture Expert");
        assert_eq!(skill.description(), "Architecture boundaries");
        assert_eq!(skill.owner_agent_id().unwrap().as_str(), "reviewer");
        assert_eq!(skill.dependency_ids(), &[dependency]);
        assert_eq!(skill.requires(), skill.required_skill_ids());
        assert_eq!(skill.related_skill_ids(), &[related]);
        assert_eq!(skill.required_capability_ids().len(), 1);
        assert_eq!(skill.provided_capabilities(), &[capability]);
        assert_eq!(skill.capabilities(), skill.provided_capabilities());
        assert_eq!(skill.knowledge_queries().len(), 1);
        assert_eq!(skill.authoritative_sources().len(), 1);
        assert_eq!(skill.rules().len(), 1);
        assert_eq!(skill.verification().len(), 1);
    }
}
