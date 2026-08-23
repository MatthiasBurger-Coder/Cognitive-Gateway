//! Versioned repository-document contracts for agents and skills.
//!
//! The document types are deliberately a small envelope around the CG-02
//! domain definitions. `origin` is document provenance and is not part of the
//! executable domain definition. All semantic fields are converted through
//! the existing domain constructors before a document is accepted.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{
    AgentDefinition, AgentId, CapabilityId, KnowledgeQuery, SchemaVersion, SkillDefinition,
    SkillId, ValidationError, serialization::SerializationError,
};

/// The only schema version currently accepted by agent and skill documents.
pub const DEFINITION_SCHEMA_VERSION: SchemaVersion = SchemaVersion::V1;

/// Identifies the kind of a versioned repository definition document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DefinitionKind {
    Agent,
    Skill,
}

impl DefinitionKind {
    /// Returns the canonical document value.
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
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(D::Error::custom)
    }
}

/// Records where a repository definition came from and how it was migrated.
///
/// The source is intentionally free-form text rather than a typed gateway ID:
/// it may be a repository path, URL, commit-qualified reference or another
/// source identifier. It is still required and validated as non-empty domain
/// text so migrated definitions cannot lose their provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionOrigin {
    project: String,
    source: String,
    migration_status: MigrationStatus,
}

impl DefinitionOrigin {
    /// Creates complete provenance for a repository definition.
    pub fn new(
        project: impl Into<String>,
        source: impl Into<String>,
        migration_status: MigrationStatus,
    ) -> Result<Self, ValidationError> {
        let project = crate::NonEmptyText::new_for_field(project, "origin.project")?.into_inner();
        let source = crate::NonEmptyText::new_for_field(source, "origin.source")?.into_inner();
        Ok(Self {
            project,
            source,
            migration_status,
        })
    }

    /// Returns the source project name.
    #[must_use]
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Returns the source path or source identifier.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the migration state recorded for this document.
    #[must_use]
    pub const fn migration_status(&self) -> MigrationStatus {
        self.migration_status
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireDefinitionOrigin {
    project: String,
    source: String,
    migration_status: String,
}

impl WireDefinitionOrigin {
    fn into_domain(self) -> Result<DefinitionOrigin, ValidationError> {
        DefinitionOrigin::new(
            self.project,
            self.source,
            MigrationStatus::from_str(&self.migration_status)?,
        )
    }

    fn from_domain(value: &DefinitionOrigin) -> Self {
        Self {
            project: value.project.clone(),
            source: value.source.clone(),
            migration_status: value.migration_status.to_string(),
        }
    }
}

/// The migration state of a repository definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MigrationStatus {
    /// The definition was authored in the Cognitive Gateway format.
    Native,
    /// The definition was imported from a source repository.
    Migrated,
    /// Multiple source definitions were normalized into this definition.
    Merged,
}

impl MigrationStatus {
    /// Returns the canonical document value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "NATIVE",
            Self::Migrated => "MIGRATED",
            Self::Merged => "MERGED",
        }
    }
}

impl fmt::Display for MigrationStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for MigrationStatus {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "NATIVE" => Ok(Self::Native),
            "MIGRATED" => Ok(Self::Migrated),
            "MERGED" => Ok(Self::Merged),
            value => Err(ValidationError::UnknownDomainValue {
                field: "migration_status",
                value: value.to_owned(),
            }),
        }
    }
}

impl Serialize for MigrationStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MigrationStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(D::Error::custom)
    }
}

/// A versioned agent repository document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDefinitionDocument {
    schema_version: SchemaVersion,
    id: AgentId,
    description: String,
    skill_ids: Vec<SkillId>,
    origin: DefinitionOrigin,
}

impl AgentDefinitionDocument {
    /// Creates a v1 agent document from typed domain values and provenance.
    pub fn new(
        id: AgentId,
        description: impl Into<String>,
        skill_ids: impl IntoIterator<Item = SkillId>,
        origin: DefinitionOrigin,
    ) -> Result<Self, ValidationError> {
        let definition = AgentDefinition::new(id, description, skill_ids)?;
        Ok(Self::from_domain(definition, origin))
    }

    /// Creates a document from an already validated domain definition.
    #[must_use]
    pub fn from_domain(definition: AgentDefinition, origin: DefinitionOrigin) -> Self {
        Self {
            schema_version: DEFINITION_SCHEMA_VERSION,
            id: definition.id().clone(),
            description: definition.description().to_owned(),
            skill_ids: definition.skill_ids().to_vec(),
            origin,
        }
    }

    /// Returns the document schema version.
    #[must_use]
    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    /// Returns the fixed document kind.
    #[must_use]
    pub const fn kind(&self) -> DefinitionKind {
        DefinitionKind::Agent
    }

    /// Returns the typed agent identity.
    #[must_use]
    pub fn id(&self) -> &AgentId {
        &self.id
    }

    /// Returns the validated responsibility description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns the ordered, unique skill references.
    #[must_use]
    pub fn skill_ids(&self) -> &[SkillId] {
        &self.skill_ids
    }

    /// Returns document provenance.
    #[must_use]
    pub const fn origin(&self) -> &DefinitionOrigin {
        &self.origin
    }

    /// Converts this document to the corresponding CG-02 domain definition.
    #[must_use]
    pub fn to_domain(&self) -> AgentDefinition {
        AgentDefinition::new(
            self.id.clone(),
            self.description.clone(),
            self.skill_ids.clone(),
        )
        .expect("validated agent document must convert to its domain definition")
    }

    /// Serializes this document as compact deterministic JSON.
    pub fn to_json(&self) -> Result<String, SerializationError> {
        serde_json::to_string(self).map_err(SerializationError::Json)
    }

    /// Parses and validates one JSON agent document.
    pub fn from_json(value: &str) -> Result<Self, SerializationError> {
        let wire =
            serde_json::from_str::<WireAgentDefinition>(value).map_err(SerializationError::Json)?;
        Self::from_wire(wire).map_err(SerializationError::Validation)
    }

    fn from_wire(wire: WireAgentDefinition) -> Result<Self, ValidationError> {
        validate_version(SchemaVersion::from_str(&wire.schema_version)?)?;
        if DefinitionKind::from_str(&wire.kind)? != DefinitionKind::Agent {
            return Err(ValidationError::UnknownDomainValue {
                field: "kind",
                value: wire.kind,
            });
        }
        let id = AgentId::new(wire.id)?;
        let skill_ids = wire
            .skill_ids
            .into_iter()
            .map(SkillId::new)
            .collect::<Result<Vec<_>, _>>()?;
        let origin = wire.origin.into_domain()?;
        let definition = AgentDefinition::new(id, wire.description, skill_ids)?;
        Ok(Self::from_domain(definition, origin))
    }
}

/// A versioned skill repository document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefinitionDocument {
    schema_version: SchemaVersion,
    id: SkillId,
    description: String,
    owner_agent_id: Option<AgentId>,
    dependency_ids: Vec<SkillId>,
    required_capability_ids: Vec<CapabilityId>,
    knowledge_queries: Vec<KnowledgeQuery>,
    origin: DefinitionOrigin,
}

impl SkillDefinitionDocument {
    /// Creates a v1 skill document from typed domain values and provenance.
    pub fn new(
        id: SkillId,
        description: impl Into<String>,
        owner_agent_id: Option<AgentId>,
        dependency_ids: impl IntoIterator<Item = SkillId>,
        required_capability_ids: impl IntoIterator<Item = CapabilityId>,
        knowledge_queries: impl IntoIterator<Item = KnowledgeQuery>,
        origin: DefinitionOrigin,
    ) -> Result<Self, ValidationError> {
        let definition =
            SkillDefinition::new(id, description, dependency_ids, required_capability_ids)?
                .with_owner_if_present(owner_agent_id)
                .with_knowledge_queries(knowledge_queries);
        Ok(Self::from_domain(definition, origin))
    }

    /// Creates a document from an already validated domain definition.
    #[must_use]
    pub fn from_domain(definition: SkillDefinition, origin: DefinitionOrigin) -> Self {
        Self {
            schema_version: DEFINITION_SCHEMA_VERSION,
            id: definition.id().clone(),
            description: definition.description().to_owned(),
            owner_agent_id: definition.owner_agent_id().cloned(),
            dependency_ids: definition.dependency_ids().to_vec(),
            required_capability_ids: definition.required_capability_ids().to_vec(),
            knowledge_queries: definition.knowledge_queries().to_vec(),
            origin,
        }
    }

    /// Returns the document schema version.
    #[must_use]
    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    /// Returns the fixed document kind.
    #[must_use]
    pub const fn kind(&self) -> DefinitionKind {
        DefinitionKind::Skill
    }

    /// Returns the typed skill identity.
    #[must_use]
    pub fn id(&self) -> &SkillId {
        &self.id
    }

    /// Returns the validated skill description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns the optional owning agent relationship.
    #[must_use]
    pub fn owner_agent_id(&self) -> Option<&AgentId> {
        self.owner_agent_id.as_ref()
    }

    /// Returns ordered, unique skill dependencies.
    #[must_use]
    pub fn dependency_ids(&self) -> &[SkillId] {
        &self.dependency_ids
    }

    /// Returns ordered, unique abstract capability requirements.
    #[must_use]
    pub fn required_capability_ids(&self) -> &[CapabilityId] {
        &self.required_capability_ids
    }

    /// Returns ordered knowledge queries.
    #[must_use]
    pub fn knowledge_queries(&self) -> &[KnowledgeQuery] {
        &self.knowledge_queries
    }

    /// Returns document provenance.
    #[must_use]
    pub const fn origin(&self) -> &DefinitionOrigin {
        &self.origin
    }

    /// Converts this document to the corresponding CG-02 domain definition.
    #[must_use]
    pub fn to_domain(&self) -> SkillDefinition {
        SkillDefinition::new(
            self.id.clone(),
            self.description.clone(),
            self.dependency_ids.clone(),
            self.required_capability_ids.clone(),
        )
        .expect("validated skill document must convert to its domain definition")
        .with_owner_if_present(self.owner_agent_id.clone())
        .with_knowledge_queries(self.knowledge_queries.clone())
    }

    /// Serializes this document as compact deterministic JSON.
    pub fn to_json(&self) -> Result<String, SerializationError> {
        serde_json::to_string(self).map_err(SerializationError::Json)
    }

    /// Parses and validates one JSON skill document.
    pub fn from_json(value: &str) -> Result<Self, SerializationError> {
        let wire =
            serde_json::from_str::<WireSkillDefinition>(value).map_err(SerializationError::Json)?;
        Self::from_wire(wire).map_err(SerializationError::Validation)
    }

    fn from_wire(wire: WireSkillDefinition) -> Result<Self, ValidationError> {
        validate_version(SchemaVersion::from_str(&wire.schema_version)?)?;
        if DefinitionKind::from_str(&wire.kind)? != DefinitionKind::Skill {
            return Err(ValidationError::UnknownDomainValue {
                field: "kind",
                value: wire.kind,
            });
        }
        let id = SkillId::new(wire.id)?;
        let owner_agent_id = wire.owner_agent_id.map(AgentId::new).transpose()?;
        let dependency_ids = wire
            .dependency_ids
            .into_iter()
            .map(SkillId::new)
            .collect::<Result<Vec<_>, _>>()?;
        let required_capability_ids = wire
            .required_capability_ids
            .into_iter()
            .map(CapabilityId::new)
            .collect::<Result<Vec<_>, _>>()?;
        let knowledge_queries = wire
            .knowledge_queries
            .into_iter()
            .map(KnowledgeQuery::new)
            .collect::<Result<Vec<_>, _>>()?;
        let origin = wire.origin.into_domain()?;
        let definition = SkillDefinition::new(
            id,
            wire.description,
            dependency_ids,
            required_capability_ids,
        )?
        .with_owner_if_present(owner_agent_id)
        .with_knowledge_queries(knowledge_queries);
        Ok(Self::from_domain(definition, origin))
    }
}

fn validate_version(version: SchemaVersion) -> Result<(), ValidationError> {
    if version == DEFINITION_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(ValidationError::UnsupportedSchemaVersion {
            expected: "1.0",
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
    schema_version: String,
    kind: String,
    id: String,
    description: String,
    skill_ids: Vec<String>,
    origin: WireDefinitionOrigin,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireSkillDefinition {
    schema_version: String,
    kind: String,
    id: String,
    description: String,
    owner_agent_id: Option<String>,
    dependency_ids: Vec<String>,
    required_capability_ids: Vec<String>,
    knowledge_queries: Vec<String>,
    origin: WireDefinitionOrigin,
}

impl Serialize for AgentDefinitionDocument {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireAgentDefinition {
            schema_version: self.schema_version.to_string(),
            kind: self.kind().to_string(),
            id: self.id.to_string(),
            description: self.description.clone(),
            skill_ids: self.skill_ids.iter().map(ToString::to_string).collect(),
            origin: WireDefinitionOrigin::from_domain(&self.origin),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AgentDefinitionDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WireAgentDefinition::deserialize(deserializer)?;
        AgentDefinitionDocument::from_wire(wire).map_err(D::Error::custom)
    }
}

impl Serialize for SkillDefinitionDocument {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireSkillDefinition {
            schema_version: self.schema_version.to_string(),
            kind: self.kind().to_string(),
            id: self.id.to_string(),
            description: self.description.clone(),
            owner_agent_id: self.owner_agent_id.as_ref().map(ToString::to_string),
            dependency_ids: self
                .dependency_ids
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
            origin: WireDefinitionOrigin::from_domain(&self.origin),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SkillDefinitionDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WireSkillDefinition::deserialize(deserializer)?;
        SkillDefinitionDocument::from_wire(wire).map_err(D::Error::custom)
    }
}

/// Compatibility name emphasizing that these are versioned contracts.
pub type VersionedAgentDefinition = AgentDefinitionDocument;

/// Compatibility name emphasizing that these are versioned contracts.
pub type VersionedSkillDefinition = SkillDefinitionDocument;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentId, CapabilityId, SkillId};

    fn origin(status: MigrationStatus) -> DefinitionOrigin {
        DefinitionOrigin::new(
            "Tiny-Swarm-World",
            ".agents/roles/senior-system-architect.md",
            status,
        )
        .unwrap()
    }

    #[test]
    fn agent_document_round_trips_and_maps_to_cg02() {
        let document = AgentDefinitionDocument::new(
            AgentId::new("system-architect").unwrap(),
            "Cross-module boundaries and architecture decisions",
            [SkillId::new("architecture-hexagonal").unwrap()],
            origin(MigrationStatus::Migrated),
        )
        .unwrap();

        let json = document.to_json().unwrap();
        let restored = AgentDefinitionDocument::from_json(&json).unwrap();
        assert_eq!(restored, document);
        assert_eq!(restored.schema_version(), SchemaVersion::V1);
        assert_eq!(restored.kind(), DefinitionKind::Agent);
        assert_eq!(restored.description(), document.description());
        assert_eq!(restored.to_domain().skill_ids(), document.skill_ids());
        assert_eq!(
            restored.origin().migration_status(),
            MigrationStatus::Migrated
        );
    }

    #[test]
    fn skill_document_round_trips_all_cg02_fields() {
        let document = SkillDefinitionDocument::new(
            SkillId::new("architecture-hexagonal").unwrap(),
            "Hexagonal boundaries and dependency direction",
            Some(AgentId::new("system-architect").unwrap()),
            [],
            [CapabilityId::new("repository.read").unwrap()],
            [KnowledgeQuery::new("hexagonal architecture boundaries").unwrap()],
            origin(MigrationStatus::Merged),
        )
        .unwrap();

        let restored = SkillDefinitionDocument::from_json(&document.to_json().unwrap()).unwrap();
        assert_eq!(restored, document);
        assert_eq!(restored.schema_version(), SchemaVersion::V1);
        assert_eq!(restored.kind(), DefinitionKind::Skill);
        assert_eq!(restored.description(), document.description());
        assert_eq!(restored.dependency_ids(), document.dependency_ids());
        assert_eq!(
            restored.required_capability_ids(),
            document.required_capability_ids()
        );
        assert_eq!(
            restored.to_domain().owner_agent_id(),
            document.owner_agent_id()
        );
        assert_eq!(
            restored.to_domain().knowledge_queries(),
            document.knowledge_queries()
        );
    }

    #[test]
    fn rejects_unsupported_versions_invalid_kind_and_unknown_fields() {
        let agent = r#"{
            "schema_version":"1.1","kind":"agent","id":"agent",
            "description":"Agent","skill_ids":["skill"],
            "origin":{"project":"source","source":"roles/agent.md","migration_status":"MIGRATED"}
        }"#;
        assert!(matches!(
            AgentDefinitionDocument::from_json(agent),
            Err(SerializationError::Validation(
                ValidationError::UnsupportedSchemaVersion { .. }
            ))
        ));

        let wrong_kind = agent.replace("1.1", "1.0").replace("agent", "skill");
        assert!(AgentDefinitionDocument::from_json(&wrong_kind).is_err());

        let skill = r#"{
            "schema_version":"1.0","kind":"agent","id":"skill",
            "description":"Skill","owner_agent_id":null,"dependency_ids":[],
            "required_capability_ids":[],"knowledge_queries":[],
            "origin":{"project":"source","source":"skills/skill.json","migration_status":"NATIVE"}
        }"#;
        assert!(SkillDefinitionDocument::from_json(skill).is_err());

        let unknown = agent.replace("\"origin\"", "\"prompt\":\"runtime text\",\"origin\"");
        assert!(AgentDefinitionDocument::from_json(&unknown).is_err());
    }

    #[test]
    fn rejects_incomplete_provenance_and_bad_relationships() {
        let missing_source = r#"{
            "schema_version":"1.0","kind":"skill","id":"skill",
            "description":"Skill","owner_agent_id":null,"dependency_ids":[],
            "required_capability_ids":[],"knowledge_queries":[],
            "origin":{"project":"source","source":" ","migration_status":"MIGRATED"}
        }"#;
        assert!(SkillDefinitionDocument::from_json(missing_source).is_err());

        let duplicate = missing_source.replace(
            "\"dependency_ids\":[]",
            "\"dependency_ids\":[\"dependency\",\"dependency\"]",
        );
        assert!(SkillDefinitionDocument::from_json(&duplicate).is_err());

        let invalid_status = missing_source.replace("MIGRATED", "IMPORTED");
        assert!(SkillDefinitionDocument::from_json(&invalid_status).is_err());

        let unowned = missing_source
            .replace("\" \"", "\"skills/skill.md\"")
            .replace("\"MIGRATED\"", "\"NATIVE\"");
        let unowned = SkillDefinitionDocument::from_json(&unowned).unwrap();
        assert!(unowned.owner_agent_id().is_none());
        assert!(unowned.to_domain().owner_agent_id().is_none());
    }

    #[test]
    fn provenance_and_enums_use_canonical_values() {
        assert_eq!(DefinitionKind::Agent.to_string(), "agent");
        assert_eq!(MigrationStatus::Native.to_string(), "NATIVE");
        assert_eq!(
            MigrationStatus::from_str("MERGED").unwrap(),
            MigrationStatus::Merged
        );
        assert!(DefinitionKind::from_str("workflow").is_err());
        assert_eq!(
            serde_json::to_string(&DefinitionKind::Agent).unwrap(),
            "\"agent\""
        );
        assert_eq!(
            serde_json::from_str::<DefinitionKind>("\"skill\"").unwrap(),
            DefinitionKind::Skill
        );
        assert!(serde_json::from_str::<DefinitionKind>("\"workflow\"").is_err());
        for status in [
            MigrationStatus::Native,
            MigrationStatus::Migrated,
            MigrationStatus::Merged,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(
                serde_json::from_str::<MigrationStatus>(&json).unwrap(),
                status
            );
        }
        assert!(DefinitionOrigin::new(" ", "source", MigrationStatus::Native).is_err());
    }

    #[test]
    fn serde_deserialization_uses_the_same_validating_boundary() {
        let agent = AgentDefinitionDocument::new(
            AgentId::new("agent").unwrap(),
            "Agent",
            [SkillId::new("skill").unwrap()],
            origin(MigrationStatus::Native),
        )
        .unwrap();
        let skill = SkillDefinitionDocument::new(
            SkillId::new("skill").unwrap(),
            "Skill",
            None,
            [],
            [],
            [],
            origin(MigrationStatus::Native),
        )
        .unwrap();

        assert_eq!(
            serde_json::from_str::<AgentDefinitionDocument>(&agent.to_json().unwrap()).unwrap(),
            agent
        );
        assert_eq!(
            serde_json::from_str::<SkillDefinitionDocument>(&skill.to_json().unwrap()).unwrap(),
            skill
        );
    }
}
