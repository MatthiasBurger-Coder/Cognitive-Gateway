//! Versioned, self-contained repository contracts for Agents and Skills.
//!
//! Version 2 keeps the semantic content needed by a Skill in the document
//! itself. Provenance and external `SKILL.md` references are deliberately
//! outside this runtime contract.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{
    AgentDefinition, AgentId, CapabilityId, KnowledgeQuery, NonEmptyText, SchemaVersion,
    SkillDefinition, SkillId, ValidationError, serialization::SerializationError,
};

/// The only schema version currently accepted by Agent and Skill documents.
pub const DEFINITION_SCHEMA_VERSION: SchemaVersion = SchemaVersion::V2;

/// Identifies the kind of a versioned repository definition document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DefinitionKind {
    Agent,
    Skill,
}

impl DefinitionKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Skill => "skill",
        }
    }
}

impl fmt::Display for DefinitionKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for DefinitionKind {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "agent" => Ok(Self::Agent),
            "skill" => Ok(Self::Skill),
            value => Err(ValidationError::UnknownDomainValue {
                field: "kind",
                value: value.to_owned(),
            }),
        }
    }
}

impl Serialize for DefinitionKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DefinitionKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_str(&String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// A versioned Agent repository document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDefinitionDocument {
    schema_version: SchemaVersion,
    id: AgentId,
    description: String,
    skill_ids: Vec<SkillId>,
}

impl AgentDefinitionDocument {
    /// Creates a v2 Agent document.
    pub fn new(
        id: AgentId,
        description: impl Into<String>,
        skill_ids: impl IntoIterator<Item = SkillId>,
    ) -> Result<Self, ValidationError> {
        Ok(Self::from_domain(AgentDefinition::new(
            id,
            description,
            skill_ids,
        )?))
    }

    #[must_use]
    pub fn from_domain(definition: AgentDefinition) -> Self {
        Self {
            schema_version: DEFINITION_SCHEMA_VERSION,
            id: definition.id().clone(),
            description: definition.description().to_owned(),
            skill_ids: definition.skill_ids().to_vec(),
        }
    }

    #[must_use]
    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    #[must_use]
    pub const fn kind(&self) -> DefinitionKind {
        DefinitionKind::Agent
    }

    #[must_use]
    pub fn id(&self) -> &AgentId {
        &self.id
    }

    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[must_use]
    pub fn skill_ids(&self) -> &[SkillId] {
        &self.skill_ids
    }

    #[must_use]
    pub fn to_domain(&self) -> AgentDefinition {
        AgentDefinition::new(
            self.id.clone(),
            self.description.clone(),
            self.skill_ids.clone(),
        )
        .expect("validated Agent document must convert to its domain definition")
    }

    pub fn to_json(&self) -> Result<String, SerializationError> {
        serde_json::to_string(self).map_err(SerializationError::Json)
    }

    pub fn from_json(value: &str) -> Result<Self, SerializationError> {
        let wire =
            serde_json::from_str::<WireAgentDefinition>(value).map_err(SerializationError::Json)?;
        Self::from_wire(wire).map_err(SerializationError::Validation)
    }

    fn from_wire(wire: WireAgentDefinition) -> Result<Self, ValidationError> {
        validate_version(wire.schema_version)?;
        if DefinitionKind::from_str(&wire.kind)? != DefinitionKind::Agent {
            return Err(ValidationError::UnknownDomainValue {
                field: "kind",
                value: wire.kind,
            });
        }
        Self::new(
            AgentId::new(wire.id)?,
            wire.description,
            wire.skill_ids
                .into_iter()
                .map(SkillId::new)
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}

/// A versioned, self-contained Skill repository document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefinitionDocument {
    schema_version: SchemaVersion,
    id: SkillId,
    name: String,
    description: String,
    owner_agent_id: Option<AgentId>,
    authoritative_sources: Vec<NonEmptyText>,
    rules: Vec<NonEmptyText>,
    verification: Vec<NonEmptyText>,
    dependency_ids: Vec<SkillId>,
    related_skill_ids: Vec<SkillId>,
    required_capability_ids: Vec<CapabilityId>,
    knowledge_queries: Vec<KnowledgeQuery>,
}

impl SkillDefinitionDocument {
    /// Creates a complete v2 Skill document. The optional owner and legacy
    /// capability/retrieval fields remain typed semantic fields; no external
    /// source or provenance is retained.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: SkillId,
        name: impl Into<String>,
        description: impl Into<String>,
        owner_agent_id: Option<AgentId>,
        authoritative_sources: impl IntoIterator<Item = impl Into<String>>,
        rules: impl IntoIterator<Item = impl Into<String>>,
        verification: impl IntoIterator<Item = impl Into<String>>,
        requires: impl IntoIterator<Item = SkillId>,
        related_skills: impl IntoIterator<Item = SkillId>,
        required_capability_ids: impl IntoIterator<Item = CapabilityId>,
        knowledge_queries: impl IntoIterator<Item = KnowledgeQuery>,
    ) -> Result<Self, ValidationError> {
        let definition = SkillDefinition::new(id, description, requires, required_capability_ids)?
            .with_name(name)?
            .with_owner_if_present(owner_agent_id)
            .with_authoritative_sources(authoritative_sources)?
            .with_rules(rules)?
            .with_verification(verification)?
            .with_related_skill_ids(related_skills)?
            .with_knowledge_queries(knowledge_queries);
        Ok(Self::from_domain(definition))
    }

    /// Creates a minimal complete Skill document with empty optional lists.
    pub fn new_minimal(
        id: SkillId,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        Self::new(
            id,
            name,
            description,
            None,
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
            std::iter::empty::<SkillId>(),
            std::iter::empty::<SkillId>(),
            std::iter::empty::<CapabilityId>(),
            std::iter::empty::<KnowledgeQuery>(),
        )
    }

    #[must_use]
    pub fn from_domain(definition: SkillDefinition) -> Self {
        Self {
            schema_version: DEFINITION_SCHEMA_VERSION,
            id: definition.id().clone(),
            name: definition.name().to_owned(),
            description: definition.description().to_owned(),
            owner_agent_id: definition.owner_agent_id().cloned(),
            authoritative_sources: definition.authoritative_sources().to_vec(),
            rules: definition.rules().to_vec(),
            verification: definition.verification().to_vec(),
            dependency_ids: definition.dependency_ids().to_vec(),
            related_skill_ids: definition.related_skill_ids().to_vec(),
            required_capability_ids: definition.required_capability_ids().to_vec(),
            knowledge_queries: definition.knowledge_queries().to_vec(),
        }
    }

    #[must_use]
    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    #[must_use]
    pub const fn kind(&self) -> DefinitionKind {
        DefinitionKind::Skill
    }

    #[must_use]
    pub fn id(&self) -> &SkillId {
        &self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[must_use]
    pub fn owner_agent_id(&self) -> Option<&AgentId> {
        self.owner_agent_id.as_ref()
    }

    #[must_use]
    pub fn authoritative_sources(&self) -> &[NonEmptyText] {
        &self.authoritative_sources
    }

    #[must_use]
    pub fn rules(&self) -> &[NonEmptyText] {
        &self.rules
    }

    #[must_use]
    pub fn verification(&self) -> &[NonEmptyText] {
        &self.verification
    }

    #[must_use]
    pub fn dependency_ids(&self) -> &[SkillId] {
        &self.dependency_ids
    }

    #[must_use]
    pub fn requires(&self) -> &[SkillId] {
        self.dependency_ids()
    }

    /// Alias using the explicit mandatory-reference terminology.
    #[must_use]
    pub fn required_skill_ids(&self) -> &[SkillId] {
        self.dependency_ids()
    }

    #[must_use]
    pub fn related_skill_ids(&self) -> &[SkillId] {
        &self.related_skill_ids
    }

    #[must_use]
    pub fn related_skills(&self) -> &[SkillId] {
        self.related_skill_ids()
    }

    #[must_use]
    pub fn required_capability_ids(&self) -> &[CapabilityId] {
        &self.required_capability_ids
    }

    #[must_use]
    pub fn knowledge_queries(&self) -> &[KnowledgeQuery] {
        &self.knowledge_queries
    }

    #[must_use]
    pub fn to_domain(&self) -> SkillDefinition {
        SkillDefinition::new(
            self.id.clone(),
            self.description.clone(),
            self.dependency_ids.clone(),
            self.required_capability_ids.clone(),
        )
        .expect("validated Skill document must convert to its domain definition")
        .with_name(self.name.clone())
        .expect("validated Skill name must convert to its domain definition")
        .with_owner_if_present(self.owner_agent_id.clone())
        .with_authoritative_sources(self.authoritative_sources.iter().map(NonEmptyText::as_str))
        .expect("validated Skill sources must convert to its domain definition")
        .with_rules(self.rules.iter().map(NonEmptyText::as_str))
        .expect("validated Skill rules must convert to its domain definition")
        .with_verification(self.verification.iter().map(NonEmptyText::as_str))
        .expect("validated Skill verification must convert to its domain definition")
        .with_related_skill_ids(self.related_skill_ids.clone())
        .expect("validated Skill references must convert to its domain definition")
        .with_knowledge_queries(self.knowledge_queries.clone())
    }

    pub fn to_json(&self) -> Result<String, SerializationError> {
        serde_json::to_string(self).map_err(SerializationError::Json)
    }

    pub fn from_json(value: &str) -> Result<Self, SerializationError> {
        let wire =
            serde_json::from_str::<WireSkillDefinition>(value).map_err(SerializationError::Json)?;
        Self::from_wire(wire).map_err(SerializationError::Validation)
    }

    fn from_wire(wire: WireSkillDefinition) -> Result<Self, ValidationError> {
        validate_version(wire.schema_version)?;
        if DefinitionKind::from_str(&wire.kind)? != DefinitionKind::Skill {
            return Err(ValidationError::UnknownDomainValue {
                field: "kind",
                value: wire.kind,
            });
        }
        Self::new(
            SkillId::new(wire.id)?,
            wire.name,
            wire.description,
            wire.owner_agent_id.map(AgentId::new).transpose()?,
            wire.authoritative_sources,
            wire.rules,
            wire.verification,
            wire.requires
                .into_iter()
                .map(SkillId::new)
                .collect::<Result<Vec<_>, _>>()?,
            wire.related_skills
                .into_iter()
                .map(SkillId::new)
                .collect::<Result<Vec<_>, _>>()?,
            wire.required_capability_ids
                .into_iter()
                .map(CapabilityId::new)
                .collect::<Result<Vec<_>, _>>()?,
            wire.knowledge_queries
                .into_iter()
                .map(KnowledgeQuery::new)
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}

fn validate_version(version: u16) -> Result<(), ValidationError> {
    if version == DEFINITION_SCHEMA_VERSION.major() {
        Ok(())
    } else {
        Err(ValidationError::UnsupportedSchemaVersion {
            expected: "2",
            actual: version.to_string(),
        })
    }
}

trait SkillDefinitionOwnerExt {
    fn with_owner_if_present(self, owner_agent_id: Option<AgentId>) -> Self;
}

impl SkillDefinitionOwnerExt for SkillDefinition {
    fn with_owner_if_present(self, owner_agent_id: Option<AgentId>) -> Self {
        match owner_agent_id {
            Some(owner) => self.with_owner(owner),
            None => self,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireAgentDefinition {
    schema_version: u16,
    kind: String,
    id: String,
    description: String,
    skill_ids: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireSkillDefinition {
    schema_version: u16,
    kind: String,
    id: String,
    name: String,
    description: String,
    #[serde(default)]
    owner_agent_id: Option<String>,
    authoritative_sources: Vec<String>,
    rules: Vec<String>,
    verification: Vec<String>,
    requires: Vec<String>,
    related_skills: Vec<String>,
    #[serde(default)]
    required_capability_ids: Vec<String>,
    #[serde(default)]
    knowledge_queries: Vec<String>,
}

impl Serialize for AgentDefinitionDocument {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireAgentDefinition {
            schema_version: self.schema_version.major(),
            kind: self.kind().to_string(),
            id: self.id.to_string(),
            description: self.description.clone(),
            skill_ids: self.skill_ids.iter().map(ToString::to_string).collect(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AgentDefinitionDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        AgentDefinitionDocument::from_wire(WireAgentDefinition::deserialize(deserializer)?)
            .map_err(D::Error::custom)
    }
}

impl Serialize for SkillDefinitionDocument {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireSkillDefinition {
            schema_version: self.schema_version.major(),
            kind: self.kind().to_string(),
            id: self.id.to_string(),
            name: self.name.clone(),
            description: self.description.clone(),
            owner_agent_id: self.owner_agent_id.as_ref().map(ToString::to_string),
            authoritative_sources: self
                .authoritative_sources
                .iter()
                .map(|value| value.as_str().to_owned())
                .collect(),
            rules: self
                .rules
                .iter()
                .map(|value| value.as_str().to_owned())
                .collect(),
            verification: self
                .verification
                .iter()
                .map(|value| value.as_str().to_owned())
                .collect(),
            requires: self
                .dependency_ids
                .iter()
                .map(ToString::to_string)
                .collect(),
            related_skills: self
                .related_skill_ids
                .iter()
                .map(ToString::to_string)
                .collect(),
            required_capability_ids: self
                .required_capability_ids
                .iter()
                .map(ToString::to_string)
                .collect(),
            knowledge_queries: self
                .knowledge_queries
                .iter()
                .map(|query| query.as_str().to_owned())
                .collect(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SkillDefinitionDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SkillDefinitionDocument::from_wire(WireSkillDefinition::deserialize(deserializer)?)
            .map_err(D::Error::custom)
    }
}

/// Compatibility names emphasizing that these are versioned contracts.
pub type VersionedAgentDefinition = AgentDefinitionDocument;
pub type VersionedSkillDefinition = SkillDefinitionDocument;

#[cfg(test)]
mod tests {
    use super::*;

    fn agent_json() -> String {
        serde_json::json!({
            "schema_version": 2,
            "kind": "agent",
            "id": "reviewer",
            "description": "Reviews architecture changes",
            "skill_ids": ["architecture"]
        })
        .to_string()
    }

    fn skill_json() -> String {
        serde_json::json!({
            "schema_version": 2,
            "kind": "skill",
            "id": "architecture",
            "name": "Architecture Expert",
            "description": "Reviews architecture boundaries",
            "owner_agent_id": "reviewer",
            "authoritative_sources": ["architecture guide"],
            "rules": ["Keep dependencies directed inward."],
            "verification": ["Run architecture tests."],
            "requires": ["foundation"],
            "related_skills": ["quality"],
            "required_capability_ids": ["repository.read"],
            "knowledge_queries": ["architecture boundaries"]
        })
        .to_string()
    }

    #[test]
    fn agent_documents_round_trip_and_convert_to_domain() {
        let document = AgentDefinitionDocument::from_json(&agent_json()).unwrap();
        assert_eq!(document.schema_version(), SchemaVersion::V2);
        assert_eq!(document.kind(), DefinitionKind::Agent);
        assert_eq!(document.to_domain().skill_ids().len(), 1);
        assert_eq!(
            AgentDefinitionDocument::from_json(&document.to_json().unwrap()).unwrap(),
            document
        );
        assert_eq!(
            serde_json::from_str::<AgentDefinitionDocument>(&document.to_json().unwrap()).unwrap(),
            document
        );
    }

    #[test]
    fn agent_documents_reject_wrong_version_kind_and_unknown_fields() {
        let old_version = agent_json().replace("\"schema_version\":2", "\"schema_version\":1");
        assert!(matches!(
            AgentDefinitionDocument::from_json(&old_version),
            Err(SerializationError::Validation(
                ValidationError::UnsupportedSchemaVersion { .. }
            ))
        ));

        let wrong_kind = agent_json().replace("\"kind\":\"agent\"", "\"kind\":\"skill\"");
        assert!(AgentDefinitionDocument::from_json(&wrong_kind).is_err());

        let unknown = agent_json().replace("\"skill_ids\"", "\"extra\":true,\"skill_ids\"");
        assert!(AgentDefinitionDocument::from_json(&unknown).is_err());
    }

    #[test]
    fn skill_documents_round_trip_all_structured_content() {
        let document = SkillDefinitionDocument::from_json(&skill_json()).unwrap();
        assert_eq!(document.schema_version(), SchemaVersion::V2);
        assert_eq!(document.name(), "Architecture Expert");
        assert_eq!(
            document.authoritative_sources()[0].as_str(),
            "architecture guide"
        );
        assert_eq!(
            document.rules()[0].as_str(),
            "Keep dependencies directed inward."
        );
        assert_eq!(
            document.verification()[0].as_str(),
            "Run architecture tests."
        );
        assert_eq!(document.requires()[0].as_str(), "foundation");
        assert_eq!(document.related_skills()[0].as_str(), "quality");
        assert_eq!(
            document.to_domain().related_skill_ids()[0].as_str(),
            "quality"
        );
        assert_eq!(
            SkillDefinitionDocument::from_json(&document.to_json().unwrap()).unwrap(),
            document
        );
        assert_eq!(
            serde_json::from_str::<SkillDefinitionDocument>(&document.to_json().unwrap()).unwrap(),
            document
        );
    }

    #[test]
    fn skill_documents_reject_invalid_content_and_references() {
        let invalid_name = skill_json().replace("Architecture Expert", " ");
        assert!(matches!(
            SkillDefinitionDocument::from_json(&invalid_name),
            Err(SerializationError::Validation(ValidationError::EmptyText {
                field: "name"
            }))
        ));

        for field in ["authoritative_sources", "rules", "verification"] {
            let invalid = skill_json().replace(
                &format!(
                    "\"{field}\":[\"{}\"]",
                    match field {
                        "authoritative_sources" => "architecture guide",
                        "rules" => "Keep dependencies directed inward.",
                        _ => "Run architecture tests.",
                    }
                ),
                &format!("\"{field}\": [\"bad\\u0000text\"]"),
            );
            assert!(matches!(
                SkillDefinitionDocument::from_json(&invalid),
                Err(SerializationError::Validation(
                    ValidationError::ControlCharacter { .. }
                ))
            ));
        }

        let duplicate = skill_json().replace(
            "\"related_skills\":[\"quality\"]",
            "\"related_skills\":[\"quality\",\"quality\"]",
        );
        assert!(matches!(
            SkillDefinitionDocument::from_json(&duplicate),
            Err(SerializationError::Validation(
                ValidationError::DuplicateRelationship {
                    field: "related_skill_ids"
                }
            ))
        ));

        let related_self = skill_json().replace(
            "\"related_skills\":[\"quality\"]",
            "\"related_skills\":[\"architecture\"]",
        );
        assert!(matches!(
            SkillDefinitionDocument::from_json(&related_self),
            Err(SerializationError::Validation(
                ValidationError::SelfReference {
                    field: "related_skill_ids"
                }
            ))
        ));

        let overlap = skill_json().replace(
            "\"related_skills\":[\"quality\"]",
            "\"related_skills\":[\"foundation\"]",
        );
        assert!(matches!(
            SkillDefinitionDocument::from_json(&overlap),
            Err(SerializationError::Validation(
                ValidationError::ConflictingRelationship { .. }
            ))
        ));
    }

    #[test]
    fn skill_documents_reject_obsolete_shape_and_malformed_typed_fields() {
        let old_version = skill_json().replace("\"schema_version\":2", "\"schema_version\":1");
        assert!(SkillDefinitionDocument::from_json(&old_version).is_err());
        let wrong_kind = skill_json().replace("\"kind\":\"skill\"", "\"kind\":\"agent\"");
        assert!(SkillDefinitionDocument::from_json(&wrong_kind).is_err());
        let unknown = skill_json().replace(
            "\"knowledge_queries\"",
            "\"origin\":{},\"knowledge_queries\"",
        );
        assert!(SkillDefinitionDocument::from_json(&unknown).is_err());
        let invalid_owner = skill_json().replace("\"reviewer\"", "\"bad owner\"");
        assert!(SkillDefinitionDocument::from_json(&invalid_owner).is_err());
        let invalid_id = skill_json().replace("\"architecture\"", "\"../architecture\"");
        assert!(SkillDefinitionDocument::from_json(&invalid_id).is_err());
        assert!(SkillDefinitionDocument::from_json("{}").is_err());
    }

    #[test]
    fn constructors_and_aliases_cover_minimal_documents() {
        let agent = AgentDefinitionDocument::new(
            AgentId::new("reviewer").unwrap(),
            "Reviews changes",
            [SkillId::new("skill").unwrap()],
        )
        .unwrap();
        assert_eq!(agent.skill_ids()[0].as_str(), "skill");

        let skill = SkillDefinitionDocument::new_minimal(
            SkillId::new("skill").unwrap(),
            "Skill",
            "Skill description",
        )
        .unwrap();
        assert_eq!(skill.required_skill_ids(), skill.requires());
        assert!(skill.related_skills().is_empty());
        assert!(skill.owner_agent_id().is_none());
        assert!(skill.required_capability_ids().is_empty());
        assert!(skill.knowledge_queries().is_empty());

        assert!(AgentDefinitionDocument::new(AgentId::new("bad").unwrap(), "\0", []).is_err());
        assert!(AgentDefinitionDocument::from_json("not json").is_err());
    }

    #[test]
    fn kind_values_are_strictly_typed() {
        assert_eq!(
            DefinitionKind::from_str("agent").unwrap(),
            DefinitionKind::Agent
        );
        assert_eq!(
            DefinitionKind::from_str("skill").unwrap(),
            DefinitionKind::Skill
        );
        assert!(DefinitionKind::from_str("workflow").is_err());
        let encoded = serde_json::to_string(&DefinitionKind::Skill).unwrap();
        assert_eq!(encoded, "\"skill\"");
        assert!(serde_json::from_str::<DefinitionKind>("\"workflow\"").is_err());
    }
}
