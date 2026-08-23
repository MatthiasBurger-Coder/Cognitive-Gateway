//! Deterministic loading and registration of repository Agent and Skill
//! definition documents.
//!
//! The registry is deliberately a repository boundary, not an execution
//! runtime. It discovers JSON documents in lexical path order, parses every
//! definition file, rejects duplicate canonical IDs, and exposes the accepted
//! documents in canonical ID order. Cross-definition reference and dependency
//! graph validation is also performed deterministically before a combined
//! registry is consumed; loading and validation never infer relationships from
//! text or retrieval results.

#![forbid(unsafe_code)]

pub mod capability_index;

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use gateway_domain::{
    AgentDefinitionDocument, AgentId, CapabilityDefinition, CapabilityId, DefinitionKind,
    SerializationError, SkillDefinitionDocument, SkillId,
};

pub use capability_index::{
    CapabilityCandidate, CapabilityIndex, CapabilityIndexEntry, CapabilityProvider,
    CapabilityProviderKind, CapabilityQuery, CapabilityQueryOutcome, CapabilityQueryResult,
    CapabilityRejection, CapabilityRejectionReason, CapabilityResolutionError, CapabilitySelector,
    CapabilitySource,
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

/// A deterministic Agent/Skill relationship-validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryIntegrityError {
    /// A requested canonical Skill ID is not registered.
    SkillNotFound { skill_id: SkillId },
    /// A Skill cannot be resolved because its self-contained semantic content
    /// is incomplete for context compilation.
    IncompleteSkillDefinition {
        skill_id: SkillId,
        field: &'static str,
        source: String,
    },
    /// An Agent references a Skill that is not registered.
    MissingSkillReference {
        agent_id: AgentId,
        skill_id: SkillId,
        source: String,
    },
    /// A Skill names an owner Agent that is not registered.
    MissingAgentReference {
        skill_id: SkillId,
        agent_id: AgentId,
        source: String,
    },
    /// A Skill depends on a Skill that is not registered.
    MissingSkillDependency {
        skill_id: SkillId,
        dependency_id: SkillId,
        source: String,
    },
    /// A Skill points to an optional/related Skill that is not registered.
    MissingRelatedSkillReference {
        skill_id: SkillId,
        related_skill_id: SkillId,
        source: String,
    },
    /// Two providers use one canonical capability ID with incompatible
    /// reusable metadata.
    ConflictingCapabilityDeclaration {
        capability_id: CapabilityId,
        first_source: String,
        conflicting_source: String,
    },
    /// The dependency graph contains a cycle, including its closing edge.
    CircularSkillDependency { cycle: Vec<SkillId>, source: String },
}

impl fmt::Display for RegistryIntegrityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SkillNotFound { skill_id } => {
                write!(formatter, "skill {:?} is not registered", skill_id.as_str())
            }
            Self::IncompleteSkillDefinition {
                skill_id,
                field,
                source,
            } => write!(
                formatter,
                "skill {:?} from {source:?} has incomplete required field {field:?}",
                skill_id.as_str()
            ),
            Self::MissingSkillReference {
                agent_id,
                skill_id,
                source,
            } => write!(
                formatter,
                "agent {:?} from {source:?} references missing skill {:?}",
                agent_id.as_str(),
                skill_id.as_str()
            ),
            Self::MissingAgentReference {
                skill_id,
                agent_id,
                source,
            } => write!(
                formatter,
                "skill {:?} from {source:?} references missing owner agent {:?}",
                skill_id.as_str(),
                agent_id.as_str()
            ),
            Self::MissingSkillDependency {
                skill_id,
                dependency_id,
                source,
            } => write!(
                formatter,
                "skill {:?} from {source:?} depends on missing skill {:?}",
                skill_id.as_str(),
                dependency_id.as_str()
            ),
            Self::MissingRelatedSkillReference {
                skill_id,
                related_skill_id,
                source,
            } => write!(
                formatter,
                "skill {:?} from {source:?} references missing related skill {:?}",
                skill_id.as_str(),
                related_skill_id.as_str()
            ),
            Self::ConflictingCapabilityDeclaration {
                capability_id,
                first_source,
                conflicting_source,
            } => write!(
                formatter,
                "capability {:?} has conflicting declarations from {first_source:?} and {conflicting_source:?}",
                capability_id.as_str()
            ),
            Self::CircularSkillDependency { cycle, source } => {
                let path = cycle
                    .iter()
                    .map(SkillId::as_str)
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(formatter, "skill dependency cycle {path} from {source:?}")
            }
        }
    }
}

impl Error for RegistryIntegrityError {}

/// A stable topological view of the registered Skill dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDependencyGraph {
    dependencies: BTreeMap<SkillId, Vec<SkillId>>,
    topological_order: Vec<SkillId>,
}

/// A complete, deterministic dependency closure rooted at one Skill.
///
/// Skills are owned by the result so the resolved graph is independent of the
/// registry's filesystem paths and caller context. `skills` are in
/// mandatory dependency-first order. Related Skills remain visible on each
/// document, but are deliberately not added to this closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSkillGraph {
    root: SkillId,
    skills: Vec<SkillDefinitionDocument>,
    topological_order: Vec<SkillId>,
    dependencies: BTreeMap<SkillId, Vec<SkillId>>,
}

impl ResolvedSkillGraph {
    /// Returns the canonical Skill ID from which this closure was resolved.
    #[must_use]
    pub fn root(&self) -> &SkillId {
        &self.root
    }

    /// Alias emphasizing that the root is a Skill ID.
    #[must_use]
    pub fn root_skill_id(&self) -> &SkillId {
        self.root()
    }

    /// Returns complete Skill documents in deterministic dependency-first order.
    #[must_use]
    pub fn skills(&self) -> &[SkillDefinitionDocument] {
        &self.skills
    }

    /// Alias using the repository contract vocabulary.
    #[must_use]
    pub fn documents(&self) -> &[SkillDefinitionDocument] {
        self.skills()
    }

    /// Returns the resolved root Skill document.
    #[must_use]
    pub fn root_skill(&self) -> &SkillDefinitionDocument {
        self.get(&self.root)
            .expect("a resolved graph always contains its root")
    }

    /// Returns the resolved Skill with the supplied canonical ID, if it is in
    /// this closure.
    #[must_use]
    pub fn get(&self, skill_id: &SkillId) -> Option<&SkillDefinitionDocument> {
        self.skills.iter().find(|skill| skill.id() == skill_id)
    }

    /// Alias for [`Self::get`].
    #[must_use]
    pub fn skill(&self, skill_id: &SkillId) -> Option<&SkillDefinitionDocument> {
        self.get(skill_id)
    }

    /// Returns the stable IDs in dependency-first order.
    #[must_use]
    pub fn ids(&self) -> impl ExactSizeIterator<Item = &SkillId> {
        self.skills.iter().map(SkillDefinitionDocument::id)
    }

    /// Returns the deterministic topological order of this closure.
    #[must_use]
    pub fn topological_order(&self) -> &[SkillId] {
        &self.topological_order
    }

    /// Returns the declared mandatory dependencies of a resolved Skill.
    #[must_use]
    pub fn dependencies(&self, skill_id: &SkillId) -> Option<&[SkillId]> {
        self.dependencies.get(skill_id).map(Vec::as_slice)
    }

    /// Returns the number of Skills in this closure.
    #[must_use]
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Returns whether this closure is empty. A successful resolution is
    /// always non-empty because it has a root.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

impl SkillDependencyGraph {
    /// Returns every Skill ID with dependencies before their dependents.
    #[must_use]
    pub fn topological_order(&self) -> &[SkillId] {
        &self.topological_order
    }

    /// Returns the declared, ordered dependencies of a Skill.
    #[must_use]
    pub fn dependencies(&self, skill_id: &SkillId) -> Option<&[SkillId]> {
        self.dependencies.get(skill_id).map(Vec::as_slice)
    }

    /// Returns the number of Skills represented by the graph.
    #[must_use]
    pub fn len(&self) -> usize {
        self.dependencies.len()
    }

    /// Returns whether the graph contains no Skills.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.dependencies.is_empty()
    }
}

/// Compatibility alias for callers using the shorter error name.
pub type IntegrityError = RegistryIntegrityError;

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

    /// Builds and validates the Skill dependency graph.
    ///
    /// The returned order is deterministic and places every dependency before
    /// the Skill that depends on it. Missing dependencies and cycles fail
    /// closed with the originating document's canonical identity.
    pub fn dependency_graph(&self) -> Result<SkillDependencyGraph, RegistryIntegrityError> {
        let mut dependencies = BTreeMap::new();
        for document in &self.documents {
            let dependency_ids = document.dependency_ids().to_vec();
            for dependency_id in &dependency_ids {
                if self.get(dependency_id).is_none() {
                    return Err(RegistryIntegrityError::MissingSkillDependency {
                        skill_id: document.id().clone(),
                        dependency_id: dependency_id.clone(),
                        source: canonical_source(document),
                    });
                }
            }
            for related_skill_id in document.related_skill_ids() {
                if self.get(related_skill_id).is_none() {
                    return Err(RegistryIntegrityError::MissingRelatedSkillReference {
                        skill_id: document.id().clone(),
                        related_skill_id: related_skill_id.clone(),
                        source: canonical_source(document),
                    });
                }
            }
            dependencies.insert(document.id().clone(), dependency_ids);
        }

        let topological_order = topological_order(&dependencies).ok_or_else(|| {
            let cycle = find_dependency_cycle(&dependencies)
                .expect("a non-topological graph must contain a dependency cycle");
            let source = self
                .get(cycle.first().expect("a cycle must not be empty"))
                .map(canonical_source)
                .unwrap_or_else(|| "unknown source".to_owned());
            RegistryIntegrityError::CircularSkillDependency { cycle, source }
        })?;

        Ok(SkillDependencyGraph {
            dependencies,
            topological_order,
        })
    }

    /// Validates this Skill registry's dependency relationships.
    pub fn validate_integrity(&self) -> Result<(), RegistryIntegrityError> {
        self.dependency_graph().map(|_| ())
    }

    /// Resolves one canonical Skill ID and its complete mandatory dependency
    /// closure from this Skill registry alone.
    ///
    /// The result owns every resolved document. This makes equality and reuse
    /// independent of discovery order, filesystem paths and the lifetime of
    /// this registry. Related Skills are validated and remain exposed through
    /// each document, but never become part of the mandatory closure.
    pub fn resolve_skill(
        &self,
        skill_id: &SkillId,
    ) -> Result<ResolvedSkillGraph, RegistryIntegrityError> {
        self.validate_integrity()?;
        if self.get(skill_id).is_none() {
            return Err(RegistryIntegrityError::SkillNotFound {
                skill_id: skill_id.clone(),
            });
        }

        let mut closure = BTreeSet::new();
        collect_skill_dependencies(skill_id, self, &mut closure);
        let dependencies = closure
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    self.get(id)
                        .expect("validated Skill closure contains registered IDs")
                        .dependency_ids()
                        .to_vec(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let order = topological_order(&dependencies)
            .expect("validated Skill registry must have an acyclic mandatory dependency graph");
        let skills = order
            .iter()
            .map(|id| {
                let document = self
                    .get(id)
                    .expect("validated Skill closure contains registered IDs");
                validate_complete_skill(document)?;
                Ok(document.clone())
            })
            .collect::<Result<Vec<_>, RegistryIntegrityError>>()?;

        Ok(ResolvedSkillGraph {
            root: skill_id.clone(),
            skills,
            topological_order: order,
            dependencies,
        })
    }

    /// Alias for [`Self::resolve_skill`].
    pub fn resolve_skill_graph(
        &self,
        skill_id: &SkillId,
    ) -> Result<ResolvedSkillGraph, RegistryIntegrityError> {
        self.resolve_skill(skill_id)
    }

    /// Short alias for [`Self::resolve_skill`].
    pub fn resolve(
        &self,
        skill_id: &SkillId,
    ) -> Result<ResolvedSkillGraph, RegistryIntegrityError> {
        self.resolve_skill(skill_id)
    }
}

/// The Agent and Skill registries loaded from the Cognitive Gateway catalog.
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

    /// Loads `agents/` and `skills/` below a catalog directory.
    pub fn load(catalog_directory: impl AsRef<Path>) -> RegistryResult<Self> {
        let catalog_directory = catalog_directory.as_ref();
        Self::load_from_directories(
            catalog_directory.join("agents"),
            catalog_directory.join("skills"),
        )
    }

    /// Loads the generic catalog from its conventional `agents/` and
    /// `skills/` boundaries.
    ///
    /// The repository-level `catalog/` directory is passed by the caller so
    /// the registry remains usable by embedded applications and tests as well.
    pub fn load_catalog(catalog_directory: impl AsRef<Path>) -> RegistryResult<Self> {
        Self::load(catalog_directory)
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

    /// Validates Agent ownership/references and the complete Skill graph.
    ///
    /// Validation is deterministic: Agents, Skills and declared relationship
    /// lists are visited in their stable registry order. No execution or
    /// retrieval service is involved.
    pub fn validate_integrity(&self) -> Result<(), RegistryIntegrityError> {
        for agent in self.agents.iter() {
            for skill_id in agent.skill_ids() {
                if self.skills.get(skill_id).is_none() {
                    return Err(RegistryIntegrityError::MissingSkillReference {
                        agent_id: agent.id().clone(),
                        skill_id: skill_id.clone(),
                        source: canonical_source(agent),
                    });
                }
            }
        }

        for skill in self.skills.iter() {
            if let Some(agent_id) = skill.owner_agent_id()
                && self.agents.get(agent_id).is_none()
            {
                return Err(RegistryIntegrityError::MissingAgentReference {
                    skill_id: skill.id().clone(),
                    agent_id: agent_id.clone(),
                    source: canonical_source(skill),
                });
            }

            for related_skill_id in skill.related_skill_ids() {
                if self.skills.get(related_skill_id).is_none() {
                    return Err(RegistryIntegrityError::MissingRelatedSkillReference {
                        skill_id: skill.id().clone(),
                        related_skill_id: related_skill_id.clone(),
                        source: canonical_source(skill),
                    });
                }
            }
        }

        self.validate_capability_declarations()?;
        self.skills.validate_integrity()
    }

    /// Validates that one canonical capability ID has one reusable contract.
    ///
    /// Multiple Agents or Skills may provide the same capability and therefore
    /// become candidates. They must agree on the metadata so a later derived
    /// capability index cannot depend on discovery order.
    fn validate_capability_declarations(&self) -> Result<(), RegistryIntegrityError> {
        let mut declarations = BTreeMap::<CapabilityId, (CapabilityDefinition, String)>::new();

        for (source, capabilities) in self
            .agents
            .iter()
            .map(|agent| (canonical_source(agent), agent.provided_capabilities()))
            .chain(
                self.skills
                    .iter()
                    .map(|skill| (canonical_source(skill), skill.provided_capabilities())),
            )
        {
            for capability in capabilities {
                match declarations.get(capability.id()) {
                    Some((existing, first_source)) if existing != capability => {
                        return Err(RegistryIntegrityError::ConflictingCapabilityDeclaration {
                            capability_id: capability.id().clone(),
                            first_source: first_source.clone(),
                            conflicting_source: source.clone(),
                        });
                    }
                    Some(_) => {}
                    None => {
                        declarations.insert(
                            capability.id().clone(),
                            (capability.clone(), source.clone()),
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Alias for [`Self::validate_integrity`].
    pub fn validate(&self) -> Result<(), RegistryIntegrityError> {
        self.validate_integrity()
    }

    /// Returns the validated deterministic Skill dependency graph.
    pub fn dependency_graph(&self) -> Result<SkillDependencyGraph, RegistryIntegrityError> {
        self.validate_integrity()?;
        self.skills.dependency_graph()
    }

    /// Resolves one canonical Skill ID and its complete mandatory dependency
    /// closure from this catalog alone. External project content is never
    /// consulted.
    pub fn resolve_skill(
        &self,
        skill_id: &SkillId,
    ) -> Result<ResolvedSkillGraph, RegistryIntegrityError> {
        self.validate_integrity()?;
        self.skills.resolve_skill(skill_id)
    }

    /// Alias for [`Self::resolve_skill`].
    pub fn resolve_skill_graph(
        &self,
        skill_id: &SkillId,
    ) -> Result<ResolvedSkillGraph, RegistryIntegrityError> {
        self.resolve_skill(skill_id)
    }

    /// Short alias for [`Self::resolve_skill`].
    pub fn resolve(
        &self,
        skill_id: &SkillId,
    ) -> Result<ResolvedSkillGraph, RegistryIntegrityError> {
        self.resolve_skill(skill_id)
    }

    /// Builds a deterministic, rebuildable capability index from this catalog.
    ///
    /// The index is a derived read model over validated Agent and Skill
    /// declarations. It never changes registry membership and does not
    /// consult project context, retrieval results or runtime capabilities.
    pub fn capability_index(&self) -> Result<CapabilityIndex, RegistryIntegrityError> {
        CapabilityIndex::build(self)
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

trait HasCanonicalSource {
    fn canonical_source(&self) -> String;
}

impl HasCanonicalSource for AgentDefinitionDocument {
    fn canonical_source(&self) -> String {
        format!("agent:{}", self.id())
    }
}

impl HasCanonicalSource for SkillDefinitionDocument {
    fn canonical_source(&self) -> String {
        format!("skill:{}", self.id())
    }
}

fn canonical_source<T: HasCanonicalSource>(document: &T) -> String {
    document.canonical_source()
}

fn collect_skill_dependencies(
    skill_id: &SkillId,
    registry: &SkillRegistry,
    closure: &mut BTreeSet<SkillId>,
) {
    if !closure.insert(skill_id.clone()) {
        return;
    }
    let skill = registry
        .get(skill_id)
        .expect("Skill dependency references are validated before resolution");
    for dependency_id in skill.dependency_ids() {
        collect_skill_dependencies(dependency_id, registry, closure);
    }
}

fn validate_complete_skill(
    document: &SkillDefinitionDocument,
) -> Result<(), RegistryIntegrityError> {
    for (field, values) in [
        ("authoritative_sources", document.authoritative_sources()),
        ("rules", document.rules()),
        ("verification", document.verification()),
    ] {
        if values.is_empty() {
            return Err(RegistryIntegrityError::IncompleteSkillDefinition {
                skill_id: document.id().clone(),
                field,
                source: canonical_source(document),
            });
        }
    }
    Ok(())
}

fn topological_order(dependencies: &BTreeMap<SkillId, Vec<SkillId>>) -> Option<Vec<SkillId>> {
    let mut indegrees = dependencies
        .keys()
        .cloned()
        .map(|id| (id, 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut dependents = BTreeMap::<SkillId, Vec<SkillId>>::new();

    for (skill_id, dependency_ids) in dependencies {
        indegrees.insert(skill_id.clone(), dependency_ids.len());
        for dependency_id in dependency_ids {
            dependents
                .entry(dependency_id.clone())
                .or_default()
                .push(skill_id.clone());
        }
    }
    for skill_ids in dependents.values_mut() {
        skill_ids.sort();
    }

    let mut ready = indegrees
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(dependencies.len());

    while let Some(skill_id) = ready.pop_first() {
        order.push(skill_id.clone());
        if let Some(skill_ids) = dependents.get(&skill_id) {
            for dependent_id in skill_ids {
                let degree = indegrees
                    .get_mut(dependent_id)
                    .expect("every dependency graph node has an indegree");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(dependent_id.clone());
                }
            }
        }
    }

    (order.len() == dependencies.len()).then_some(order)
}

fn find_dependency_cycle(dependencies: &BTreeMap<SkillId, Vec<SkillId>>) -> Option<Vec<SkillId>> {
    let mut visited = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    let mut stack = Vec::new();

    for skill_id in dependencies.keys() {
        if let Some(cycle) = visit_dependency(
            skill_id,
            dependencies,
            &mut visited,
            &mut visiting,
            &mut stack,
        ) {
            return Some(cycle);
        }
    }
    None
}

fn visit_dependency(
    skill_id: &SkillId,
    dependencies: &BTreeMap<SkillId, Vec<SkillId>>,
    visited: &mut BTreeSet<SkillId>,
    visiting: &mut BTreeSet<SkillId>,
    stack: &mut Vec<SkillId>,
) -> Option<Vec<SkillId>> {
    if let Some(position) = stack.iter().position(|id| id == skill_id) {
        let mut cycle = stack[position..].to_vec();
        cycle.push(skill_id.clone());
        return Some(cycle);
    }
    if visited.contains(skill_id) {
        return None;
    }

    visiting.insert(skill_id.clone());
    stack.push(skill_id.clone());
    for dependency_id in dependencies
        .get(skill_id)
        .expect("every dependency graph node has a definition")
    {
        if let Some(cycle) = visit_dependency(dependency_id, dependencies, visited, visiting, stack)
        {
            return Some(cycle);
        }
    }
    stack.pop();
    visiting.remove(skill_id);
    visited.insert(skill_id.clone());
    None
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
        AgentDefinitionDocument, AgentId, CapabilityClass, CapabilityDefinition, CapabilityId,
        SkillDefinitionDocument, SkillId,
    };

    use super::{AgentRegistry, Registry, RegistryError, RegistryIntegrityError, SkillRegistry};

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
            r#"{{"schema_version":2,"kind":"agent","id":"{id}","description":"Agent {id}","skill_ids":["skill"]}}"#
        )
    }

    fn skill(id: &str) -> String {
        format!(
            r#"{{"schema_version":2,"kind":"skill","id":"{id}","name":"Skill {id}","description":"Skill {id}","owner_agent_id":null,"authoritative_sources":[],"rules":[],"verification":[],"requires":[],"related_skills":[],"required_capability_ids":[],"knowledge_queries":[]}}"#
        )
    }

    fn skill_document(
        id: &str,
        owner_agent_id: Option<&str>,
        dependencies: &[&str],
    ) -> SkillDefinitionDocument {
        SkillDefinitionDocument::new(
            SkillId::new(id).unwrap(),
            format!("Skill {id}"),
            format!("Skill {id}"),
            owner_agent_id.map(|value| AgentId::new(value).unwrap()),
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
            dependencies
                .iter()
                .map(|value| SkillId::new(*value).unwrap()),
            std::iter::empty::<SkillId>(),
            std::iter::empty::<gateway_domain::CapabilityId>(),
            std::iter::empty::<gateway_domain::KnowledgeQuery>(),
        )
        .unwrap()
    }

    fn complete_skill_document(
        id: &str,
        dependencies: &[&str],
        related_skills: &[&str],
    ) -> SkillDefinitionDocument {
        SkillDefinitionDocument::new(
            SkillId::new(id).unwrap(),
            format!("Skill {id}"),
            format!("Complete responsibility for {id}"),
            None,
            [format!("authoritative source for {id}")],
            [format!("rule for {id}")],
            [format!("verification for {id}")],
            dependencies
                .iter()
                .map(|value| SkillId::new(*value).unwrap()),
            related_skills
                .iter()
                .map(|value| SkillId::new(*value).unwrap()),
            std::iter::empty::<gateway_domain::CapabilityId>(),
            std::iter::empty::<gateway_domain::KnowledgeQuery>(),
        )
        .unwrap()
    }

    fn agent_document(id: &str, skills: &[&str]) -> AgentDefinitionDocument {
        AgentDefinitionDocument::new(
            AgentId::new(id).unwrap(),
            format!("Agent {id}"),
            skills.iter().map(|value| SkillId::new(*value).unwrap()),
        )
        .unwrap()
    }

    fn capability(domain: &str) -> CapabilityDefinition {
        CapabilityDefinition::new_with_contract(
            CapabilityId::new("shared.analysis").unwrap(),
            CapabilityClass::Inspect,
            domain,
            "A shared analysis capability",
            ["repository.snapshot"],
            ["analysis.report"],
            ["repository.available"],
            ["read-only"],
            ["analysis"],
        )
        .unwrap()
    }

    fn skill_document_with_related(id: &str, related_skills: &[&str]) -> SkillDefinitionDocument {
        SkillDefinitionDocument::new(
            SkillId::new(id).unwrap(),
            format!("Skill {id}"),
            format!("Skill {id}"),
            None,
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
            std::iter::empty::<SkillId>(),
            related_skills
                .iter()
                .map(|value| SkillId::new(*value).unwrap()),
            std::iter::empty::<gateway_domain::CapabilityId>(),
            std::iter::empty::<gateway_domain::KnowledgeQuery>(),
        )
        .unwrap()
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
                .id()
                .as_str(),
            "alpha"
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
            &agent("agent").replace("\"schema_version\":2", "\"schema_version\":3"),
        );
        let error = AgentRegistry::load(&root).unwrap_err();
        assert!(error.to_string().contains("not supported"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_self_and_duplicate_dependencies_at_the_document_boundary() {
        let root = temporary_directory("self-dependency");
        let self_reference = skill("same").replace("\"requires\":[]", "\"requires\":[\"same\"]");
        write_file(&root.join("self.json"), &self_reference);
        let error = SkillRegistry::load(&root).unwrap_err();
        assert!(error.to_string().contains("must not reference"));
        fs::remove_dir_all(&root).unwrap();

        let root = temporary_directory("duplicate-dependency");
        let duplicate =
            skill("same").replace("\"requires\":[]", "\"requires\":[\"other\",\"other\"]");
        write_file(&root.join("duplicate.json"), &duplicate);
        let error = SkillRegistry::load(&root).unwrap_err();
        assert!(error.to_string().contains("duplicate references"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn catalog_loads_agent_and_skill_boundaries() {
        let catalog_root = temporary_directory("catalog-only");
        write_file(&catalog_root.join("agents/agent.json"), &agent("agent"));
        write_file(&catalog_root.join("skills/skill.json"), &skill("skill"));

        let registry = Registry::load_catalog(&catalog_root).unwrap();
        registry.validate_integrity().unwrap();
        assert_eq!(
            registry
                .agents()
                .ids()
                .map(AgentId::as_str)
                .collect::<Vec<_>>(),
            ["agent"]
        );
        assert_eq!(
            registry
                .skills()
                .ids()
                .map(SkillId::as_str)
                .collect::<Vec<_>>(),
            ["skill"]
        );

        fs::remove_dir_all(catalog_root).unwrap();
    }

    #[test]
    fn in_memory_registration_is_sorted_and_rejects_duplicates() {
        let first = AgentDefinitionDocument::new(
            AgentId::new("zeta").unwrap(),
            "Zeta",
            [SkillId::new("skill").unwrap()],
        )
        .unwrap();
        let second = AgentDefinitionDocument::new(
            AgentId::new("alpha").unwrap(),
            "Alpha",
            [SkillId::new("skill").unwrap()],
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

    #[test]
    fn validates_references_and_returns_stable_dependency_order() {
        let registry = Registry::from_documents(
            [agent_document("reviewer", &["root"])],
            [
                skill_document("root", None, &["branch", "leaf"]),
                skill_document("leaf", None, &[]),
                skill_document("branch", None, &["leaf"]),
            ],
        )
        .unwrap();

        registry.validate_integrity().unwrap();
        let graph = registry.dependency_graph().unwrap();
        let order = graph
            .topological_order()
            .iter()
            .map(SkillId::as_str)
            .collect::<Vec<_>>();
        assert_eq!(order, ["leaf", "branch", "root"]);
        assert_eq!(
            graph
                .dependencies(&SkillId::new("root").unwrap())
                .unwrap()
                .iter()
                .map(SkillId::as_str)
                .collect::<Vec<_>>(),
            ["branch", "leaf"]
        );
    }

    #[test]
    fn resolves_a_complete_dependency_closure_without_activating_related_skills() {
        let registry = Registry::from_documents(
            [],
            [
                complete_skill_document("root", &["branch", "leaf"], &["optional"]),
                complete_skill_document("branch", &["leaf"], &[]),
                complete_skill_document("leaf", &[], &[]),
                skill_document("optional", None, &[]),
            ],
        )
        .unwrap();

        let graph = registry
            .resolve_skill(&SkillId::new("root").unwrap())
            .unwrap();
        assert_eq!(graph.root().as_str(), "root");
        assert_eq!(graph.root_skill_id(), graph.root());
        assert_eq!(graph.root_skill().id(), graph.root());
        assert_eq!(
            graph.ids().map(SkillId::as_str).collect::<Vec<_>>(),
            ["leaf", "branch", "root"]
        );
        assert_eq!(
            graph
                .topological_order()
                .iter()
                .map(SkillId::as_str)
                .collect::<Vec<_>>(),
            ["leaf", "branch", "root"]
        );
        assert_eq!(graph.documents(), graph.skills());
        assert!(graph.get(&SkillId::new("root").unwrap()).is_some());
        assert!(graph.skill(&SkillId::new("root").unwrap()).is_some());
        assert!(graph.get(&SkillId::new("optional").unwrap()).is_none());
        assert!(!graph.is_empty());
        assert_eq!(
            graph
                .dependencies(&SkillId::new("root").unwrap())
                .unwrap()
                .iter()
                .map(SkillId::as_str)
                .collect::<Vec<_>>(),
            ["branch", "leaf"]
        );
        assert_eq!(
            graph
                .get(&SkillId::new("root").unwrap())
                .unwrap()
                .related_skills()[0]
                .as_str(),
            "optional"
        );
    }

    #[test]
    fn resolution_is_equivalent_for_the_same_catalog_snapshot() {
        let first = Registry::from_documents(
            [],
            [
                complete_skill_document("root", &["branch"], &[]),
                complete_skill_document("leaf", &[], &[]),
                complete_skill_document("branch", &["leaf"], &[]),
            ],
        )
        .unwrap();
        let second = Registry::from_documents(
            [],
            [
                complete_skill_document("branch", &["leaf"], &[]),
                complete_skill_document("root", &["branch"], &[]),
                complete_skill_document("leaf", &[], &[]),
            ],
        )
        .unwrap();

        assert_eq!(
            first.resolve_skill(&SkillId::new("root").unwrap()),
            second.resolve_skill(&SkillId::new("root").unwrap())
        );
        assert_eq!(
            first.resolve_skill_graph(&SkillId::new("root").unwrap()),
            first.resolve(&SkillId::new("root").unwrap())
        );
        assert_eq!(
            first.skills().resolve_skill(&SkillId::new("root").unwrap()),
            first
                .skills()
                .resolve_skill_graph(&SkillId::new("root").unwrap())
        );
        assert_eq!(
            first.skills().resolve(&SkillId::new("root").unwrap()),
            first.skills().resolve_skill(&SkillId::new("root").unwrap())
        );
    }

    #[test]
    fn resolution_rejects_unknown_and_incomplete_skills() {
        let unknown = Registry::from_documents([], [])
            .unwrap()
            .resolve_skill(&SkillId::new("missing").unwrap());
        assert!(matches!(
            unknown,
            Err(RegistryIntegrityError::SkillNotFound { .. })
        ));
        let unknown = Registry::from_documents([], [])
            .unwrap()
            .resolve_skill(&SkillId::new("missing").unwrap())
            .unwrap_err();
        assert!(unknown.to_string().contains("missing"));

        let incomplete = Registry::from_documents([], [skill_document("incomplete", None, &[])])
            .unwrap()
            .resolve_skill(&SkillId::new("incomplete").unwrap());
        assert!(matches!(
            incomplete,
            Err(RegistryIntegrityError::IncompleteSkillDefinition {
                field: "authoritative_sources",
                ..
            })
        ));
        let incomplete = Registry::from_documents([], [skill_document("incomplete", None, &[])])
            .unwrap()
            .resolve_skill(&SkillId::new("incomplete").unwrap())
            .unwrap_err();
        assert!(incomplete.to_string().contains("authoritative_sources"));

        let missing_related = SkillRegistry::from_documents([skill_document_with_related(
            "related",
            &["missing-related"],
        )])
        .unwrap()
        .validate_integrity()
        .unwrap_err();
        assert!(matches!(
            missing_related,
            RegistryIntegrityError::MissingRelatedSkillReference { .. }
        ));
    }

    #[test]
    fn reports_missing_agent_and_skill_references_with_canonical_source() {
        let missing_skill =
            Registry::from_documents([agent_document("reviewer", &["missing"])], [])
                .unwrap()
                .validate_integrity()
                .unwrap_err();
        assert!(matches!(
            missing_skill,
            RegistryIntegrityError::MissingSkillReference { .. }
        ));
        assert!(missing_skill.to_string().contains("agent:reviewer"));

        let missing_owner =
            Registry::from_documents([], [skill_document("owned", Some("missing-agent"), &[])])
                .unwrap()
                .validate_integrity()
                .unwrap_err();
        assert!(matches!(
            missing_owner,
            RegistryIntegrityError::MissingAgentReference { .. }
        ));
        assert!(missing_owner.to_string().contains("skill:owned"));

        let missing_dependency =
            Registry::from_documents([], [skill_document("dependent", None, &["missing"])])
                .unwrap()
                .validate_integrity()
                .unwrap_err();
        assert!(matches!(
            missing_dependency,
            RegistryIntegrityError::MissingSkillDependency { .. }
        ));
        assert!(missing_dependency.to_string().contains("skill:dependent"));

        let missing_related = Registry::from_documents(
            [],
            [skill_document_with_related("related", &["missing-related"])],
        )
        .unwrap()
        .validate_integrity()
        .unwrap_err();
        assert!(matches!(
            missing_related,
            RegistryIntegrityError::MissingRelatedSkillReference { .. }
        ));
    }

    #[test]
    fn rejects_conflicting_capability_metadata_but_allows_multiple_providers() {
        let agent = AgentDefinitionDocument::new_with_provided_capabilities(
            AgentId::new("reviewer").unwrap(),
            "Reviews changes",
            [SkillId::new("analysis").unwrap()],
            [capability("analysis")],
        )
        .unwrap();
        let skill = complete_skill_document("analysis", &[], &[])
            .with_provided_capabilities([capability("analysis")])
            .unwrap();

        let valid = Registry::from_documents([agent.clone()], [skill.clone()]).unwrap();
        valid
            .validate_integrity()
            .expect("equivalent declarations can have multiple providers");

        let conflicting_skill = skill
            .with_provided_capabilities([capability("security")])
            .unwrap();
        let registry = Registry::from_documents([agent], [conflicting_skill]).unwrap();
        let error = registry.validate_integrity().unwrap_err();
        assert!(matches!(
            error,
            RegistryIntegrityError::ConflictingCapabilityDeclaration {
                capability_id,
                first_source,
                conflicting_source,
            } if capability_id.as_str() == "shared.analysis"
                && first_source == "agent:reviewer"
                && conflicting_source == "skill:analysis"
        ));
    }

    #[test]
    fn reports_a_reproducible_cycle_path() {
        let registry = Registry::from_documents(
            [],
            [
                skill_document("alpha", None, &["beta"]),
                skill_document("beta", None, &["alpha"]),
            ],
        )
        .unwrap();

        let error = registry.validate_integrity().unwrap_err();
        match error {
            RegistryIntegrityError::CircularSkillDependency { cycle, source } => {
                assert_eq!(
                    cycle.iter().map(SkillId::as_str).collect::<Vec<_>>(),
                    ["alpha", "beta", "alpha"]
                );
                assert_eq!(source, "skill:alpha");
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
