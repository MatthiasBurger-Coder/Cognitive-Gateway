//! Git-backed deterministic Process Definition Registry.

use std::{
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

use crate::{
    CompilationError, ProcessDefinition, ProcessDefinitionId, ProcessDefinitionVersion,
    ProcessValidator, SemanticCompiler,
};

/// A Git-owned process source supplied to registry construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSource {
    path: String,
    content: String,
}

impl ProcessSource {
    #[must_use]
    pub fn new(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: content.into(),
        }
    }
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// A definition and its canonical source path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredProcess {
    source_path: String,
    definition: ProcessDefinition,
}

impl RegisteredProcess {
    #[must_use]
    pub fn source_path(&self) -> &str {
        &self.source_path
    }
    #[must_use]
    pub fn definition(&self) -> &ProcessDefinition {
        &self.definition
    }
}

/// Stable failure returned by discovery, compilation, validation or admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRegistryError {
    code: &'static str,
    source_path: Option<String>,
    message: String,
}

impl ProcessRegistryError {
    fn new(code: &'static str, source_path: Option<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            source_path,
            message: message.into(),
        }
    }
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
    #[must_use]
    pub fn source_path(&self) -> Option<&str> {
        self.source_path.as_deref()
    }
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProcessRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.source_path {
            write!(formatter, "{} [{}]: {}", self.code, path, self.message)
        } else {
            write!(formatter, "{}: {}", self.code, self.message)
        }
    }
}
impl Error for ProcessRegistryError {}

/// Derived registry state. Git source files remain the authority.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessRegistry {
    entries: Vec<RegisteredProcess>,
}

impl ProcessRegistry {
    /// Builds a registry from source files in deterministic path order.
    pub fn from_sources<I>(sources: I) -> Result<Self, ProcessRegistryError>
    where
        I: IntoIterator<Item = ProcessSource>,
    {
        let mut sources = sources.into_iter().collect::<Vec<_>>();
        sources.sort_by(|left, right| left.path.cmp(&right.path));
        let mut entries = Vec::new();
        for source in sources {
            let result = SemanticCompiler::compile(&source.content)
                .map_err(|error| compilation_error(source.path.clone(), error))?;
            let report = ProcessValidator::validate(result.definition());
            if !report.is_valid() {
                let diagnostics = report
                    .diagnostics()
                    .iter()
                    .map(|item| format!("{}: {}", item.code(), item.message()))
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(ProcessRegistryError::new(
                    "INVALID_DEFINITION",
                    Some(source.path),
                    diagnostics,
                ));
            }
            let identity = result.definition().identity();
            if entries.iter().any(|entry: &RegisteredProcess| {
                entry.definition.identity().id() == identity.id()
                    && entry.definition.identity().version() == identity.version()
            }) {
                return Err(ProcessRegistryError::new(
                    "DUPLICATE_DEFINITION_ID_VERSION",
                    Some(source.path),
                    format!(
                        "{} version {} is already registered",
                        identity.id(),
                        identity.version()
                    ),
                ));
            }
            entries.push(RegisteredProcess {
                source_path: source.path,
                definition: result.definition().clone(),
            });
        }
        entries.sort_by(|left, right| {
            left.definition
                .identity()
                .id()
                .cmp(right.definition.identity().id())
                .then(
                    left.definition
                        .identity()
                        .version()
                        .cmp(&right.definition.identity().version()),
                )
        });
        Ok(Self { entries })
    }

    /// Discovers feature files recursively below root.
    pub fn load(root: impl AsRef<Path>) -> Result<Self, ProcessRegistryError> {
        let mut paths = Vec::new();
        discover(root.as_ref(), &mut paths)?;
        let sources = paths
            .into_iter()
            .map(|path| {
                let relative = path
                    .strip_prefix(root.as_ref())
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let content = fs::read_to_string(&path).map_err(|error| {
                    ProcessRegistryError::new(
                        "SOURCE_READ_ERROR",
                        Some(relative.clone()),
                        error.to_string(),
                    )
                })?;
                Ok(ProcessSource::new(relative, content))
            })
            .collect::<Result<Vec<_>, ProcessRegistryError>>()?;
        Self::from_sources(sources)
    }

    #[must_use]
    pub fn entries(&self) -> &[RegisteredProcess] {
        &self.entries
    }
    pub fn definitions(&self) -> impl Iterator<Item = &ProcessDefinition> {
        self.entries.iter().map(RegisteredProcess::definition)
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    #[must_use]
    pub fn get(
        &self,
        id: &ProcessDefinitionId,
        version: ProcessDefinitionVersion,
    ) -> Option<&ProcessDefinition> {
        self.entries.iter().find_map(|entry| {
            let identity = entry.definition.identity();
            (identity.id() == id && identity.version() == version).then_some(&entry.definition)
        })
    }
    #[must_use]
    pub fn resolve(
        &self,
        id: &ProcessDefinitionId,
        version: Option<ProcessDefinitionVersion>,
    ) -> Option<&ProcessDefinition> {
        match version {
            Some(version) => self.get(id, version),
            None => self
                .entries
                .iter()
                .filter(|entry| entry.definition.identity().id() == id)
                .max_by_key(|entry| entry.definition.identity().version())
                .map(RegisteredProcess::definition),
        }
    }
}

fn compilation_error(path: String, error: CompilationError) -> ProcessRegistryError {
    let message = error
        .diagnostics()
        .iter()
        .map(|item| format!("{}: {}", item.code(), item.message()))
        .collect::<Vec<_>>()
        .join("; ");
    ProcessRegistryError::new("COMPILATION_ERROR", Some(path), message)
}

fn discover(path: &Path, output: &mut Vec<PathBuf>) -> Result<(), ProcessRegistryError> {
    let metadata = fs::metadata(path).map_err(|error| {
        ProcessRegistryError::new(
            "DISCOVERY_ERROR",
            Some(path.to_string_lossy().into_owned()),
            error.to_string(),
        )
    })?;
    if metadata.is_file() {
        if path
            .extension()
            .is_some_and(|extension| extension == "feature")
        {
            output.push(path.to_owned());
        }
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(ProcessRegistryError::new(
            "DISCOVERY_ERROR",
            Some(path.to_string_lossy().into_owned()),
            "catalog root is neither file nor directory",
        ));
    }
    let mut children = fs::read_dir(path)
        .map_err(|error| {
            ProcessRegistryError::new(
                "DISCOVERY_ERROR",
                Some(path.to_string_lossy().into_owned()),
                error.to_string(),
            )
        })?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            ProcessRegistryError::new(
                "DISCOVERY_ERROR",
                Some(path.to_string_lossy().into_owned()),
                error.to_string(),
            )
        })?;
    children.sort();
    for child in children {
        discover(&child, output)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const SOURCE: &str = include_str!("../fixtures/strict-cognitive-gherkin/valid.feature");

    #[test]
    fn registers_valid_sources_in_stable_identity_order() {
        let registry = ProcessRegistry::from_sources([
            ProcessSource::new("z.feature", SOURCE),
            ProcessSource::new(
                "a.feature",
                SOURCE.replace("canonical-issue-lifecycle", "another"),
            ),
        ])
        .unwrap();
        assert_eq!(registry.len(), 2);
        assert_eq!(
            registry.entries()[0].definition().identity().id().as_str(),
            "another"
        );
        assert!(
            registry
                .resolve(
                    &ProcessDefinitionId::new("canonical-issue-lifecycle").unwrap(),
                    None
                )
                .is_some()
        );
        assert_eq!(registry.definitions().count(), 2);
    }

    #[test]
    fn rejects_invalid_duplicate_and_missing_sources_fail_closed() {
        let invalid =
            include_str!("../fixtures/strict-cognitive-gherkin/invalid-unknown-step.feature");
        assert_eq!(
            ProcessRegistry::from_sources([ProcessSource::new("invalid.feature", invalid)])
                .unwrap_err()
                .code(),
            "COMPILATION_ERROR"
        );
        assert_eq!(
            ProcessRegistry::from_sources([
                ProcessSource::new("a.feature", SOURCE),
                ProcessSource::new("b.feature", SOURCE),
            ])
            .unwrap_err()
            .code(),
            "DUPLICATE_DEFINITION_ID_VERSION"
        );
        assert_eq!(
            ProcessRegistry::load("path-that-does-not-exist")
                .unwrap_err()
                .code(),
            "DISCOVERY_ERROR"
        );
    }

    #[test]
    fn discovers_nested_feature_files_and_ignores_other_files() {
        let root = std::env::temp_dir().join(format!("cg-process-registry-{}", std::process::id()));
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("ignored.txt"), "ignored").unwrap();
        fs::write(nested.join("process.feature"), SOURCE).unwrap();
        let registry = ProcessRegistry::load(&root).unwrap();
        assert_eq!(registry.len(), 1);
        assert_eq!(
            registry.entries()[0].source_path(),
            "nested/process.feature"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
