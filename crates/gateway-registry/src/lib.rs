//! Deterministic loading and registration of repository Agent and Skill
//! definition documents.
//!
//! The registry is deliberately a repository boundary, not an execution
//! runtime. It discovers JSON documents in lexical path order, parses every
//! definition file, rejects duplicate canonical IDs, and exposes the accepted
//! documents in canonical ID order. Cross-definition reference and dependency
//! graph validation belongs to the next registry-integrity layer; loading
//! itself never infers relationships from text or retrieval results.

#![forbid(unsafe_code)]

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use gateway_domain::{
    AgentDefinitionDocument, AgentId, DefinitionKind, SerializationError, SkillDefinitionDocument,
    SkillId,
};

/// The result of loading one repository-backed registry.
pub type RegistryResult<T> = Result<T, RegistryError>;

/// A deterministic registry-loading failure.
#[derive(Debug)]
pub enum RegistryError {
    /// The registry root or one of its descendants could not be inspected.
    Io { path: PathBuf, source: io::Error },
    /// A JSON definition could not be decoded or failed domain validation.
    InvalidDocument {
        kind: DefinitionKind,
        path: PathBuf,
        source: SerializationError,
    },
    /// Two files attempted to register the same canonical identifier.
    DuplicateDefinition {
        kind: DefinitionKind,
        id: String,
        first_path: PathBuf,
        duplicate_path: PathBuf,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "could not inspect registry path {:?}: {source}",
                    path
                )
            }
            Self::InvalidDocument { kind, path, source } => write!(
                formatter,
                "invalid {kind} definition at {:?}: {source}",
                path
            ),
            Self::DuplicateDefinition {
                kind,
                id,
                first_path,
                duplicate_path,
            } => write!(
                formatter,
                "duplicate {kind} definition {id:?}: first registered at {:?}, again at {:?}",
                first_path, duplicate_path
            ),
        }
    }
}

impl Error for RegistryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidDocument { source, .. } => Some(source),
            Self::DuplicateDefinition { .. } => None,
        }
    }
}

/// A deterministic collection of validated Agent documents.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentRegistry {
    documents: Vec<AgentDefinitionDocument>,
}

impl AgentRegistry {
    /// Loads all `*.json` Agent documents below `directory`.
    ///
    /// Directory traversal and parsing happen in lexical relative-path order.
    /// Non-JSON files are outside the current JSON adapter and are ignored;
    /// every JSON file is treated as a definition and must therefore be valid.
    pub fn load(directory: impl AsRef<Path>) -> RegistryResult<Self> {
        let directory = directory.as_ref();
        let files = discover_json_files(directory)?;
        let mut documents = Vec::with_capacity(files.len());
        let mut registered = Vec::with_capacity(files.len());

        for path in files {
            let value = read_document(&path)?;
            let document = AgentDefinitionDocument::from_json(&value).map_err(|source| {
                RegistryError::InvalidDocument {
                    kind: DefinitionKind::Agent,
                    path: path.clone(),
                    source,
                }
            })?;
            register_id(
                DefinitionKind::Agent,
                document.id().as_str(),
                &path,
                &mut registered,
            )?;
            documents.push(document);
        }

        sort_agents(&mut documents);
        Ok(Self { documents })
    }

    /// Alias for [`Self::load`] for callers that prefer an explicit boundary
    /// name.
    pub fn load_directory(directory: impl AsRef<Path>) -> RegistryResult<Self> {
        Self::load(directory)
    }

    /// Builds an Agent registry from already parsed documents.
    pub fn from_documents(
        documents: impl IntoIterator<Item = AgentDefinitionDocument>,
    ) -> RegistryResult<Self> {
        let mut documents = documents.into_iter().collect::<Vec<_>>();
        ensure_unique_agent_ids(&documents)?;
        sort_agents(&mut documents);
        Ok(Self { documents })
    }

    /// Returns all registered documents in canonical ID order.
    #[must_use]
    pub fn documents(&self) -> &[AgentDefinitionDocument] {
        &self.documents
    }

    /// Returns the number of registered Agents.
    #[must_use]
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Returns whether no Agents are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// Iterates over Agents in canonical ID order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &AgentDefinitionDocument> {
        self.documents.iter()
    }

    /// Alias for [`Self::documents`] using the registry's domain vocabulary.
    #[must_use]
    pub fn agents(&self) -> &[AgentDefinitionDocument] {
        self.documents()
    }

    /// Finds an Agent document by typed canonical ID.
    #[must_use]
    pub fn get(&self, id: &AgentId) -> Option<&AgentDefinitionDocument> {
        self.documents
            .binary_search_by(|document| document.id().cmp(id))
            .ok()
            .map(|index| &self.documents[index])
    }

    /// Alias for [`Self::get`].
    #[must_use]
    pub fn agent(&self, id: &AgentId) -> Option<&AgentDefinitionDocument> {
        self.get(id)
    }

    /// Returns whether the canonical Agent ID is registered.
    #[must_use]
    pub fn contains(&self, id: &AgentId) -> bool {
        self.get(id).is_some()
    }

    /// Returns the stable canonical IDs in registry order.
    #[must_use]
    pub fn ids(&self) -> impl ExactSizeIterator<Item = &AgentId> {
        self.documents.iter().map(AgentDefinitionDocument::id)
    }
}

/// A deterministic collection of validated Skill documents.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillRegistry {
    documents: Vec<SkillDefinitionDocument>,
}

impl SkillRegistry {
    /// Loads all `*.json` Skill documents below `directory`.
    ///
    /// Directory traversal and parsing happen in lexical relative-path order.
    /// Non-JSON files are outside the current JSON adapter and are ignored;
    /// every JSON file is treated as a definition and must therefore be valid.
    pub fn load(directory: impl AsRef<Path>) -> RegistryResult<Self> {
        let directory = directory.as_ref();
        let files = discover_json_files(directory)?;
        let mut documents = Vec::with_capacity(files.len());
        let mut registered = Vec::with_capacity(files.len());

        for path in files {
            let value = read_document(&path)?;
            let document = SkillDefinitionDocument::from_json(&value).map_err(|source| {
                RegistryError::InvalidDocument {
                    kind: DefinitionKind::Skill,
                    path: path.clone(),
                    source,
                }
            })?;
            register_id(
                DefinitionKind::Skill,
                document.id().as_str(),
                &path,
                &mut registered,
            )?;
            documents.push(document);
        }

        sort_skills(&mut documents);
        Ok(Self { documents })
    }

    /// Alias for [`Self::load`] for callers that prefer an explicit boundary
    /// name.
    pub fn load_directory(directory: impl AsRef<Path>) -> RegistryResult<Self> {
        Self::load(directory)
    }

    /// Builds a Skill registry from already parsed documents.
    pub fn from_documents(
        documents: impl IntoIterator<Item = SkillDefinitionDocument>,
    ) -> RegistryResult<Self> {
        let mut documents = documents.into_iter().collect::<Vec<_>>();
        ensure_unique_skill_ids(&documents)?;
        sort_skills(&mut documents);
        Ok(Self { documents })
    }

    /// Returns all registered documents in canonical ID order.
    #[must_use]
    pub fn documents(&self) -> &[SkillDefinitionDocument] {
        &self.documents
    }

    /// Returns the number of registered Skills.
    #[must_use]
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Returns whether no Skills are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// Iterates over Skills in canonical ID order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &SkillDefinitionDocument> {
        self.documents.iter()
    }

    /// Alias for [`Self::documents`] using the registry's domain vocabulary.
    #[must_use]
    pub fn skills(&self) -> &[SkillDefinitionDocument] {
        self.documents()
    }

    /// Finds a Skill document by typed canonical ID.
    #[must_use]
    pub fn get(&self, id: &SkillId) -> Option<&SkillDefinitionDocument> {
        self.documents
            .binary_search_by(|document| document.id().cmp(id))
            .ok()
            .map(|index| &self.documents[index])
    }

    /// Alias for [`Self::get`].
    #[must_use]
    pub fn skill(&self, id: &SkillId) -> Option<&SkillDefinitionDocument> {
        self.get(id)
    }

    /// Returns whether the canonical Skill ID is registered.
    #[must_use]
    pub fn contains(&self, id: &SkillId) -> bool {
        self.get(id).is_some()
    }

    /// Returns the stable canonical IDs in registry order.
    #[must_use]
    pub fn ids(&self) -> impl ExactSizeIterator<Item = &SkillId> {
        self.documents.iter().map(SkillDefinitionDocument::id)
    }
}

/// The Agent and Skill registries loaded from one profile directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Registry {
    agents: AgentRegistry,
    skills: SkillRegistry,
}

impl Registry {
    /// Creates a combined registry from its independently loaded boundaries.
    #[must_use]
    pub const fn new(agents: AgentRegistry, skills: SkillRegistry) -> Self {
        Self { agents, skills }
    }

    /// Loads `agents/` and `skills/` below a profile directory.
    pub fn load(profile_directory: impl AsRef<Path>) -> RegistryResult<Self> {
        let profile_directory = profile_directory.as_ref();
        Self::load_from_directories(
            profile_directory.join("agents"),
            profile_directory.join("skills"),
        )
    }

    /// Alias emphasizing that the input is a project profile.
    pub fn load_profile(profile_directory: impl AsRef<Path>) -> RegistryResult<Self> {
        Self::load(profile_directory)
    }

    /// Loads the two registry boundaries from explicit directories.
    pub fn load_from_directories(
        agents_directory: impl AsRef<Path>,
        skills_directory: impl AsRef<Path>,
    ) -> RegistryResult<Self> {
        Ok(Self::new(
            AgentRegistry::load(agents_directory)?,
            SkillRegistry::load(skills_directory)?,
        ))
    }

    /// Builds a combined registry from already parsed documents.
    pub fn from_documents(
        agents: impl IntoIterator<Item = AgentDefinitionDocument>,
        skills: impl IntoIterator<Item = SkillDefinitionDocument>,
    ) -> RegistryResult<Self> {
        Ok(Self::new(
            AgentRegistry::from_documents(agents)?,
            SkillRegistry::from_documents(skills)?,
        ))
    }

    /// Returns the Agent registry.
    #[must_use]
    pub fn agents(&self) -> &AgentRegistry {
        &self.agents
    }

    /// Returns the Skill registry.
    #[must_use]
    pub fn skills(&self) -> &SkillRegistry {
        &self.skills
    }

    /// Finds an Agent document by typed canonical ID.
    #[must_use]
    pub fn agent(&self, id: &AgentId) -> Option<&AgentDefinitionDocument> {
        self.agents.get(id)
    }

    /// Finds a Skill document by typed canonical ID.
    #[must_use]
    pub fn skill(&self, id: &SkillId) -> Option<&SkillDefinitionDocument> {
        self.skills.get(id)
    }
}

fn discover_json_files(root: &Path) -> RegistryResult<Vec<PathBuf>> {
    let metadata = fs::metadata(root).map_err(|source| RegistryError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    if !metadata.is_dir() {
        return Err(RegistryError::Io {
            path: root.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::NotADirectory,
                "registry root is not a directory",
            ),
        });
    }

    let mut files = Vec::new();
    collect_json_files(root, &mut files)?;
    files.sort_by_key(|left| relative_sort_key(root, left));
    Ok(files)
}

fn collect_json_files(directory: &Path, files: &mut Vec<PathBuf>) -> RegistryResult<()> {
    let entries = fs::read_dir(directory).map_err(|source| RegistryError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| RegistryError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| RegistryError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            collect_json_files(&path, files)?;
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "json")
        {
            files.push(path);
        }
    }
    Ok(())
}

fn relative_sort_key(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn read_document(path: &Path) -> RegistryResult<String> {
    fs::read_to_string(path).map_err(|source| RegistryError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn register_id(
    kind: DefinitionKind,
    id: &str,
    path: &Path,
    registered: &mut Vec<(String, PathBuf)>,
) -> RegistryResult<()> {
    if let Some((_, first_path)) = registered
        .iter()
        .find(|(registered_id, _)| registered_id == id)
    {
        return Err(RegistryError::DuplicateDefinition {
            kind,
            id: id.to_owned(),
            first_path: first_path.clone(),
            duplicate_path: path.to_path_buf(),
        });
    }
    registered.push((id.to_owned(), path.to_path_buf()));
    Ok(())
}

fn ensure_unique_agent_ids(documents: &[AgentDefinitionDocument]) -> RegistryResult<()> {
    let mut registered = Vec::with_capacity(documents.len());
    for document in documents {
        register_id(
            DefinitionKind::Agent,
            document.id().as_str(),
            Path::new("<memory>"),
            &mut registered,
        )?;
    }
    Ok(())
}

fn ensure_unique_skill_ids(documents: &[SkillDefinitionDocument]) -> RegistryResult<()> {
    let mut registered = Vec::with_capacity(documents.len());
    for document in documents {
        register_id(
            DefinitionKind::Skill,
            document.id().as_str(),
            Path::new("<memory>"),
            &mut registered,
        )?;
    }
    Ok(())
}

fn sort_agents(documents: &mut [AgentDefinitionDocument]) {
    documents.sort_by(|left, right| left.id().cmp(right.id()));
}

fn sort_skills(documents: &mut [SkillDefinitionDocument]) {
    documents.sort_by(|left, right| left.id().cmp(right.id()));
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use gateway_domain::{
        AgentDefinitionDocument, AgentId, DefinitionOrigin, MigrationStatus, SkillId,
    };

    use super::{AgentRegistry, Registry, RegistryError, SkillRegistry};

    fn temporary_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("cognitive-gateway-registry-{name}-{nonce}"))
    }

    fn write_file(path: &Path, value: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, value).unwrap();
    }

    fn agent(id: &str) -> String {
        format!(
            r#"{{"schema_version":"1.0","kind":"agent","id":"{id}","description":"Agent {id}","skill_ids":["skill"],"origin":{{"project":"project","source":"agents/{id}.json","migration_status":"MIGRATED"}}}}"#
        )
    }

    fn skill(id: &str) -> String {
        format!(
            r#"{{"schema_version":"1.0","kind":"skill","id":"{id}","description":"Skill {id}","owner_agent_id":null,"dependency_ids":[],"required_capability_ids":[],"knowledge_queries":[],"origin":{{"project":"project","source":"skills/{id}.json","migration_status":"NATIVE"}}}}"#
        )
    }

    fn origin() -> DefinitionOrigin {
        DefinitionOrigin::new("project", "memory.json", MigrationStatus::Native).unwrap()
    }

    #[test]
    fn loads_json_documents_and_orders_by_canonical_id() {
        let root = temporary_directory("ordered");
        write_file(&root.join("z.json"), &agent("zeta"));
        write_file(&root.join("nested/a.json"), &agent("alpha"));
        write_file(&root.join("README.md"), "documentation is not a definition");

        let registry = AgentRegistry::load(&root).unwrap();
        let repeated = AgentRegistry::load(&root).unwrap();
        assert_eq!(registry, repeated);
        let ids = registry.ids().map(AgentId::as_str).collect::<Vec<_>>();
        assert_eq!(ids, ["alpha", "zeta"]);
        assert_eq!(
            registry
                .get(&AgentId::new("alpha").unwrap())
                .unwrap()
                .origin()
                .source(),
            "agents/alpha.json"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_duplicate_ids_with_both_paths() {
        let root = temporary_directory("duplicate");
        write_file(&root.join("first.json"), &skill("same"));
        write_file(&root.join("second.json"), &skill("same"));

        let error = SkillRegistry::load(&root).unwrap_err();
        match error {
            RegistryError::DuplicateDefinition {
                id,
                first_path,
                duplicate_path,
                ..
            } => {
                assert_eq!(id, "same");
                assert!(first_path.ends_with("first.json"));
                assert!(duplicate_path.ends_with("second.json"));
            }
            other => panic!("unexpected error: {other}"),
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_malformed_and_unsupported_documents_without_skipping_them() {
        let root = temporary_directory("invalid");
        write_file(&root.join("malformed.json"), "not json");
        let error = AgentRegistry::load(&root).unwrap_err();
        assert!(matches!(error, RegistryError::InvalidDocument { .. }));
        assert!(error.to_string().contains("malformed.json"));
        fs::remove_dir_all(&root).unwrap();

        let root = temporary_directory("version");
        write_file(
            &root.join("agent.json"),
            &agent("agent").replace("\"1.0\"", "\"1.1\""),
        );
        let error = AgentRegistry::load(&root).unwrap_err();
        assert!(error.to_string().contains("not supported"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn combined_profile_loads_agent_and_skill_boundaries() {
        let root = temporary_directory("profile");
        write_file(&root.join("agents/agent.json"), &agent("agent"));
        write_file(&root.join("skills/skill.json"), &skill("skill"));

        let registry = Registry::load(&root).unwrap();
        assert!(registry.agent(&AgentId::new("agent").unwrap()).is_some());
        assert!(registry.skill(&SkillId::new("skill").unwrap()).is_some());
        assert_eq!(registry.agents().documents().len(), 1);
        assert_eq!(registry.skills().documents().len(), 1);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn in_memory_registration_is_sorted_and_rejects_duplicates() {
        let origin = origin();
        let first = AgentDefinitionDocument::new(
            AgentId::new("zeta").unwrap(),
            "Zeta",
            [SkillId::new("skill").unwrap()],
            origin.clone(),
        )
        .unwrap();
        let second = AgentDefinitionDocument::new(
            AgentId::new("alpha").unwrap(),
            "Alpha",
            [SkillId::new("skill").unwrap()],
            origin,
        )
        .unwrap();
        let registry = AgentRegistry::from_documents([first.clone(), second.clone()]).unwrap();
        assert_eq!(registry.documents()[0], second);
        assert_eq!(registry.documents()[1], first);

        let duplicate = AgentRegistry::from_documents([first.clone(), first]).unwrap_err();
        assert!(matches!(
            duplicate,
            RegistryError::DuplicateDefinition { .. }
        ));
    }
}
