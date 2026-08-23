//! Read-only inspection adapter for the built-in Agent, Skill and Capability
//! registries.
//
// This module deliberately contains command parsing and presentation only.
// Loading, integrity validation, dependency resolution and capability matching
// remain owned by gateway-registry.

#![forbid(unsafe_code)]

use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use gateway_domain::{
    AgentId, CapabilityConstraint, CapabilityDefinition, CapabilityId, CapabilityInputKind,
    CapabilityOutputKind, CapabilityPrecondition, CapabilityTag, SkillId,
};
use gateway_registry::{
    CapabilityCandidate, CapabilityIndexEntry, CapabilityProvider, CapabilityQueryOutcome,
    CapabilityRejection, CapabilitySelector, Registry, RegistryError, RegistryIntegrityError,
    ResolvedSkillGraph,
};
use serde_json::{Value, json};

const DEFAULT_CATALOG: &str = "catalog";
const EXIT_USAGE: i32 = 2;
const EXIT_CATALOG: i32 = 3;
const EXIT_UNKNOWN_ID: i32 = 4;

/// Runs the cg-registry command and returns its process exit code.
pub fn run<I, S>(arguments: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let arguments = arguments.into_iter().map(Into::into).collect::<Vec<_>>();
    let json_output = arguments.iter().any(|argument| argument == "--json");

    match execute(arguments) {
        Ok(Execution::Output {
            value,
            json: format,
        }) => {
            if format {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value)
                        .expect("inspection report values must be serializable")
                );
            } else {
                print!("{}", render_human(&value));
            }
            0
        }
        Ok(Execution::Help(text)) => {
            println!("{text}");
            0
        }
        Ok(Execution::Version(version)) => {
            println!("{version}");
            0
        }
        Err(error) => {
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&error.to_json())
                        .expect("inspection error values must be serializable")
                );
            } else {
                eprintln!("error: {error}");
            }
            error.exit_code()
        }
    }
}

enum Execution {
    Output { value: Value, json: bool },
    Help(String),
    Version(String),
}

fn execute(arguments: Vec<String>) -> Result<Execution, CliError> {
    let parsed = parse(arguments)?;
    match parsed {
        Parsed::Help => Ok(Execution::Help(usage().to_owned())),
        Parsed::Version => Ok(Execution::Version(env!("CARGO_PKG_VERSION").to_owned())),
        Parsed::Command(command) => {
            let registry = load_registry(&command.catalog)?;
            registry.validate_integrity().map_err(CliError::Integrity)?;
            let value = inspect(&registry, &command.command)?;
            Ok(Execution::Output {
                value,
                json: command.json,
            })
        }
    }
}

enum Parsed {
    Help,
    Version,
    Command(ParsedCommand),
}

struct ParsedCommand {
    catalog: PathBuf,
    json: bool,
    command: Command,
}

enum Command {
    AgentList,
    AgentShow(String),
    SkillList,
    SkillShow(String),
    SkillGraph(String),
    CapabilityList,
    CapabilityShow(String),
    CapabilityResolve(String),
}

fn parse(arguments: Vec<String>) -> Result<Parsed, CliError> {
    let mut catalog = PathBuf::from(DEFAULT_CATALOG);
    let mut json = false;
    let mut positional = Vec::new();
    let mut index = 1;

    if arguments.len() <= 1 {
        return Ok(Parsed::Help);
    }

    while index < arguments.len() {
        let argument = &arguments[index];
        match argument.as_str() {
            "--help" | "-h" => return Ok(Parsed::Help),
            "--version" | "-V" => return Ok(Parsed::Version),
            "--json" => json = true,
            "--catalog" | "-c" => {
                index += 1;
                let value = arguments.get(index).ok_or_else(|| {
                    CliError::Usage("--catalog requires a directory argument".to_owned())
                })?;
                catalog = PathBuf::from(value);
            }
            value if value.starts_with("--catalog=") => {
                let value = value.trim_start_matches("--catalog=");
                if value.is_empty() {
                    return Err(CliError::Usage(
                        "--catalog requires a directory argument".to_owned(),
                    ));
                }
                catalog = PathBuf::from(value);
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option {value:?}")));
            }
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }

    let command = match positional.as_slice() {
        [resource, action] => parse_command(resource, action, None)?,
        [resource, action, id] => parse_command(resource, action, Some(id))?,
        [] => return Ok(Parsed::Help),
        _ => {
            return Err(CliError::Usage(
                "expected <resource> <action> [canonical-id]".to_owned(),
            ));
        }
    };

    Ok(Parsed::Command(ParsedCommand {
        catalog,
        json,
        command,
    }))
}

fn parse_command(resource: &str, action: &str, id: Option<&String>) -> Result<Command, CliError> {
    match (resource, action, id) {
        ("agent", "list", None) => Ok(Command::AgentList),
        ("agent", "show", Some(id)) => Ok(Command::AgentShow(id.clone())),
        ("skill", "list", None) => Ok(Command::SkillList),
        ("skill", "show", Some(id)) => Ok(Command::SkillShow(id.clone())),
        ("skill", "graph", Some(id)) => Ok(Command::SkillGraph(id.clone())),
        ("capability", "list", None) => Ok(Command::CapabilityList),
        ("capability", "show", Some(id)) => Ok(Command::CapabilityShow(id.clone())),
        ("capability", "resolve", Some(id)) => Ok(Command::CapabilityResolve(id.clone())),
        ("agent" | "skill" | "capability", action, None) => Err(CliError::Usage(format!(
            "{resource} {action} requires a canonical ID"
        ))),
        (_, action, None) => Err(CliError::Usage(format!(
            "unsupported command {resource} {action}"
        ))),
        (resource, action, Some(_)) => Err(CliError::Usage(format!(
            "unsupported command {resource} {action}"
        ))),
    }
}

fn load_registry(catalog: &Path) -> Result<Registry, CliError> {
    Registry::load_catalog(catalog).map_err(CliError::Load)
}

fn inspect(registry: &Registry, command: &Command) -> Result<Value, CliError> {
    match command {
        Command::AgentList => Ok(agent_list(registry)),
        Command::AgentShow(id) => agent_show(registry, id),
        Command::SkillList => Ok(skill_list(registry)),
        Command::SkillShow(id) => skill_show(registry, id),
        Command::SkillGraph(id) => skill_graph(registry, id),
        Command::CapabilityList => capability_list(registry),
        Command::CapabilityShow(id) => capability_show(registry, id),
        Command::CapabilityResolve(id) => capability_resolve(registry, id),
    }
}

fn agent_list(registry: &Registry) -> Value {
    let agents = registry
        .agents()
        .iter()
        .map(|agent| agent_value(registry, agent.id().as_str()))
        .collect::<Vec<_>>();
    json!({
        "command": "agent list",
        "kind": "agent_list",
        "count": agents.len(),
        "agents": agents,
    })
}

fn agent_show(registry: &Registry, id: &str) -> Result<Value, CliError> {
    let id = AgentId::new(id.to_owned()).map_err(|source| CliError::InvalidId {
        kind: "agent",
        id: id.to_owned(),
        source: source.to_string(),
    })?;
    if registry.agent(&id).is_none() {
        return Err(CliError::UnknownId {
            kind: "agent",
            id: id.to_string(),
        });
    }
    Ok(json!({
        "command": "agent show",
        "kind": "agent",
        "agent": agent_value(registry, id.as_str()),
    }))
}

fn agent_value(registry: &Registry, id: &str) -> Value {
    let agent_id = AgentId::new(id.to_owned()).expect("validated registry IDs must be valid");
    let agent = registry
        .agent(&agent_id)
        .expect("agent list/show must use a registered ID");
    let owned_skill_ids = registry
        .skills()
        .iter()
        .filter(|skill| skill.owner_agent_id() == Some(agent.id()))
        .map(|skill| skill.id().as_str())
        .collect::<Vec<_>>();
    json!({
        "schema_version": agent.schema_version().major(),
        "id": agent.id().as_str(),
        "description": agent.description(),
        "skill_ids": ids(agent.skill_ids()),
        "owned_skill_ids": owned_skill_ids,
        "provided_capabilities": agent
            .provided_capabilities()
            .iter()
            .map(capability_value)
            .collect::<Vec<_>>(),
    })
}

fn skill_list(registry: &Registry) -> Value {
    let skills = registry
        .skills()
        .iter()
        .map(|skill| {
            json!({
                "id": skill.id().as_str(),
                "name": skill.name(),
                "description": skill.description(),
                "requires": ids(skill.requires()),
                "related_skills": ids(skill.related_skills()),
                "provided_capability_ids": skill
                    .provided_capabilities()
                    .iter()
                    .map(|capability| capability.id().as_str())
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "command": "skill list",
        "kind": "skill_list",
        "count": skills.len(),
        "skills": skills,
    })
}

fn skill_show(registry: &Registry, id: &str) -> Result<Value, CliError> {
    let id = parse_skill_id(id)?;
    let skill = registry.skill(&id).ok_or_else(|| CliError::UnknownId {
        kind: "skill",
        id: id.to_string(),
    })?;
    Ok(json!({
        "command": "skill show",
        "kind": "skill",
        "skill": skill_value(skill),
    }))
}

fn skill_graph(registry: &Registry, id: &str) -> Result<Value, CliError> {
    let id = parse_skill_id(id)?;
    let graph = registry.resolve_skill(&id).map_err(|error| match error {
        RegistryIntegrityError::SkillNotFound { .. } => CliError::UnknownId {
            kind: "skill",
            id: id.to_string(),
        },
        error => CliError::Integrity(error),
    })?;
    Ok(graph_value(&graph))
}

fn parse_skill_id(id: &str) -> Result<SkillId, CliError> {
    SkillId::new(id.to_owned()).map_err(|source| CliError::InvalidId {
        kind: "skill",
        id: id.to_owned(),
        source: source.to_string(),
    })
}

fn graph_value(graph: &ResolvedSkillGraph) -> Value {
    let dependencies = graph
        .topological_order()
        .iter()
        .map(|id| {
            (
                id.as_str().to_owned(),
                json!(ids(graph.dependencies(id).unwrap_or_default())),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({
        "command": "skill graph",
        "kind": "skill_graph",
        "root": graph.root().as_str(),
        "topological_order": ids(graph.topological_order()),
        "dependencies": dependencies,
        "closure": graph.skills().iter().map(skill_value).collect::<Vec<_>>(),
    })
}

fn capability_list(registry: &Registry) -> Result<Value, CliError> {
    let index = registry.capability_index().map_err(CliError::Integrity)?;
    let capabilities = index.entries().map(index_entry_value).collect::<Vec<_>>();
    Ok(json!({
        "command": "capability list",
        "kind": "capability_list",
        "count": capabilities.len(),
        "capabilities": capabilities,
    }))
}

fn capability_show(registry: &Registry, id: &str) -> Result<Value, CliError> {
    let id = parse_capability_id(id)?;
    let index = registry.capability_index().map_err(CliError::Integrity)?;
    let entry = index.get(&id).ok_or_else(|| CliError::UnknownId {
        kind: "capability",
        id: id.to_string(),
    })?;
    Ok(json!({
        "command": "capability show",
        "kind": "capability",
        "capability": index_entry_value(entry),
    }))
}

fn capability_resolve(registry: &Registry, id: &str) -> Result<Value, CliError> {
    let id = parse_capability_id(id)?;
    let index = registry.capability_index().map_err(CliError::Integrity)?;
    if !index.contains(&id) {
        return Err(CliError::UnknownId {
            kind: "capability",
            id: id.to_string(),
        });
    }
    let result = index.query_capability(&id);
    let outcome = result.outcome();
    Ok(json!({
        "command": "capability resolve",
        "kind": "capability_resolution",
        "query": { "capability_id": id.as_str(), "selectors": [] },
        "capability": result.matches().first().map(|candidate| capability_value(candidate.capability())),
        "outcome": outcome_name(outcome),
        "status": if result.matches().is_empty() { "unresolvable" } else { "resolvable" },
        "matches": result.matches().iter().map(candidate_value).collect::<Vec<_>>(),
        "rejections": result.rejections().iter().map(rejection_value).collect::<Vec<_>>(),
    }))
}

fn parse_capability_id(id: &str) -> Result<CapabilityId, CliError> {
    CapabilityId::new(id.to_owned()).map_err(|source| CliError::InvalidId {
        kind: "capability",
        id: id.to_owned(),
        source: source.to_string(),
    })
}

fn index_entry_value(entry: &CapabilityIndexEntry) -> Value {
    let mut value = capability_value(entry.capability());
    let object = value
        .as_object_mut()
        .expect("capability contract reports must be JSON objects");
    object.insert("ambiguous".to_owned(), json!(entry.is_ambiguous()));
    object.insert(
        "providers".to_owned(),
        json!(
            entry
                .candidates()
                .iter()
                .map(candidate_value)
                .collect::<Vec<_>>()
        ),
    );
    value
}

fn candidate_value(candidate: &CapabilityCandidate) -> Value {
    json!({
        "provider": provider_value(candidate.provider()),
        "capability": capability_value(candidate.capability()),
        "owner_agent_id": candidate.owner_agent_id().map(AgentId::as_str),
        "skill_ids": ids(candidate.skill_ids()),
        "dependency_closure": ids(candidate.dependency_closure()),
        "matched_selectors": candidate
            .matched_selectors()
            .iter()
            .map(selector_value)
            .collect::<Vec<_>>(),
    })
}

fn rejection_value(rejection: &CapabilityRejection) -> Value {
    json!({
        "provider": provider_value(rejection.provider()),
        "capability": capability_value(rejection.capability()),
        "reasons": rejection
            .reasons()
            .iter()
            .map(|reason| selector_value(reason.selector()))
            .collect::<Vec<_>>(),
    })
}

fn provider_value(provider: &CapabilityProvider) -> Value {
    json!({
        "kind": provider.kind().as_str(),
        "id": provider.id(),
        "source": provider.canonical_source(),
    })
}

fn capability_value(capability: &CapabilityDefinition) -> Value {
    json!({
        "id": capability.id().as_str(),
        "class": capability.class().as_str(),
        "domain": capability.domain().as_str(),
        "description": capability.description(),
        "input_kinds": strings(capability.input_kinds(), CapabilityInputKind::as_str),
        "output_kinds": strings(capability.output_kinds(), CapabilityOutputKind::as_str),
        "preconditions": strings(capability.preconditions(), CapabilityPrecondition::as_str),
        "constraints": strings(capability.constraints(), CapabilityConstraint::as_str),
        "applicability_tags": strings(capability.applicability_tags(), CapabilityTag::as_str),
    })
}

fn selector_value(selector: &CapabilitySelector) -> Value {
    match selector {
        CapabilitySelector::CapabilityId(value) => {
            json!({ "type": "capability_id", "value": value.as_str() })
        }
        CapabilitySelector::Class(value) => json!({ "type": "class", "value": value.as_str() }),
        CapabilitySelector::Domain(value) => {
            json!({ "type": "domain", "value": value.as_str() })
        }
        CapabilitySelector::InputKind(value) => {
            json!({ "type": "input_kind", "value": value.as_str() })
        }
        CapabilitySelector::OutputKind(value) => {
            json!({ "type": "output_kind", "value": value.as_str() })
        }
        CapabilitySelector::Precondition(value) => {
            json!({ "type": "precondition", "value": value.as_str() })
        }
        CapabilitySelector::Constraint(value) => {
            json!({ "type": "constraint", "value": value.as_str() })
        }
        CapabilitySelector::ApplicabilityTag(value) => {
            json!({ "type": "applicability_tag", "value": value.as_str() })
        }
    }
}

fn skill_value(skill: &gateway_domain::SkillDefinitionDocument) -> Value {
    json!({
        "schema_version": skill.schema_version().major(),
        "id": skill.id().as_str(),
        "name": skill.name(),
        "description": skill.description(),
        "owner_agent_id": skill.owner_agent_id().map(AgentId::as_str),
        "authoritative_sources": strings(skill.authoritative_sources(), |value| value.as_str()),
        "rules": strings(skill.rules(), |value| value.as_str()),
        "verification": strings(skill.verification(), |value| value.as_str()),
        "requires": ids(skill.requires()),
        "related_skills": ids(skill.related_skills()),
        "required_capability_ids": ids(skill.required_capability_ids()),
        "knowledge_queries": strings(skill.knowledge_queries(), |value| value.as_str()),
        "provided_capabilities": skill
            .provided_capabilities()
            .iter()
            .map(capability_value)
            .collect::<Vec<_>>(),
    })
}

fn ids<T>(values: &[T]) -> Vec<&str>
where
    T: AsRef<str>,
{
    values.iter().map(AsRef::as_ref).collect()
}

fn strings<T, F>(values: &[T], as_str: F) -> Vec<&str>
where
    F: Fn(&T) -> &str,
{
    values.iter().map(as_str).collect()
}

fn outcome_name(outcome: CapabilityQueryOutcome) -> &'static str {
    match outcome {
        CapabilityQueryOutcome::NoMatch => "no_match",
        CapabilityQueryOutcome::Unique => "unique",
        CapabilityQueryOutcome::Ambiguous => "ambiguous",
    }
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    Load(RegistryError),
    Integrity(RegistryIntegrityError),
    InvalidId {
        kind: &'static str,
        id: String,
        source: String,
    },
    UnknownId {
        kind: &'static str,
        id: String,
    },
}

impl CliError {
    fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) | Self::InvalidId { .. } => EXIT_USAGE,
            Self::Load(_) | Self::Integrity(_) => EXIT_CATALOG,
            Self::UnknownId { .. } => EXIT_UNKNOWN_ID,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Usage(_) => "usage_error",
            Self::Load(_) => "catalog_load_error",
            Self::Integrity(_) => "catalog_integrity_error",
            Self::InvalidId { .. } => "invalid_id",
            Self::UnknownId { .. } => "unknown_id",
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "kind": "error",
            "code": self.code(),
            "message": self.to_string(),
            "exit_code": self.exit_code(),
        })
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(formatter, "{message}; see --help"),
            Self::Load(error) => write!(formatter, "could not load catalog: {error}"),
            Self::Integrity(error) => write!(formatter, "catalog is invalid: {error}"),
            Self::InvalidId { kind, id, source } => {
                write!(formatter, "invalid {kind} ID {id:?}: {source}")
            }
            Self::UnknownId { kind, id } => write!(formatter, "unknown {kind} ID {id:?}"),
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Load(error) => Some(error),
            Self::Integrity(error) => Some(error),
            _ => None,
        }
    }
}

fn render_human(value: &Value) -> String {
    let mut output = String::from("Cognitive Gateway Registry Inspection\n\n");
    render_value(value, 0, &mut output);
    output
}

fn render_value(value: &Value, indent: usize, output: &mut String) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let label = human_label(key);
                match value {
                    Value::Object(_) => {
                        line(output, indent, &format!("{label}:"));
                        render_value(value, indent + 2, output);
                    }
                    Value::Array(values) if values.iter().any(Value::is_object) => {
                        line(output, indent, &format!("{label}:"));
                        render_value(value, indent + 2, output);
                    }
                    Value::Array(values) => {
                        line(output, indent, &format!("{label}:"));
                        for value in values {
                            line(output, indent + 2, &format!("- {}", scalar(value)));
                        }
                    }
                    _ => line(output, indent, &format!("{label}: {}", scalar(value))),
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                if value.is_object() {
                    line(output, indent, "-");
                    render_value(value, indent + 2, output);
                } else {
                    line(output, indent, &format!("- {}", scalar(value)));
                }
            }
        }
        _ => line(output, indent, &scalar(value)),
    }
}

fn line(output: &mut String, indent: usize, text: &str) {
    output.push_str(&" ".repeat(indent));
    output.push_str(text);
    output.push('\n');
}

fn scalar(value: &Value) -> String {
    match value {
        Value::Null => "none".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => "(details)".to_owned(),
    }
}

fn human_label(value: &str) -> String {
    if value == "matches" {
        return "Candidates".to_owned();
    }
    value
        .split('_')
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn usage() -> &'static str {
    "Cognitive Gateway registry inspection (read-only)

Usage:
  cg-registry [--catalog <dir>] [--json] <resource> <action> [canonical-id]

Commands:
  agent list                         List Agents in canonical order
  agent show <agent-id>              Inspect one Agent
  skill list                         List Skills in canonical order
  skill show <skill-id>              Inspect one complete Skill
  skill graph <skill-id>             Show the mandatory dependency closure
  capability list                    List indexed Capabilities
  capability show <capability-id>    Inspect a Capability and its providers
  capability resolve <capability-id> Resolve providers and match explanation

Options:
  -c, --catalog <dir>                Catalog directory (default: catalog)
      --json                         Emit deterministic machine-readable JSON
  -h, --help                         Show this help
      --version                      Show the CLI version

Exit codes:
  0  success
  2  invalid command, option or identifier
  3  catalog loading or integrity failure
  4  unknown canonical identifier"
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_domain::{
        AgentDefinitionDocument, CapabilityClass, CapabilityDomain, SkillDefinitionDocument,
    };
    use gateway_registry::CapabilityQuery;

    fn args(values: &[&str]) -> Vec<String> {
        std::iter::once("cg-registry")
            .chain(values.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn parses_all_supported_commands_and_options() {
        assert!(matches!(
            parse(args(&["agent", "list"])),
            Ok(Parsed::Command(_))
        ));
        assert!(matches!(
            parse(args(&[
                "agent",
                "show",
                "system-architect",
                "--json",
                "-c",
                "catalog"
            ])),
            Ok(Parsed::Command(_))
        ));
        assert!(matches!(
            parse(args(&["skill", "list"])),
            Ok(Parsed::Command(_))
        ));
        assert!(matches!(
            parse(args(&["skill", "show", "architecture-hexagonal"])),
            Ok(Parsed::Command(_))
        ));
        assert!(matches!(
            parse(args(&["skill", "graph", "architecture-hexagonal"])),
            Ok(Parsed::Command(_))
        ));
        assert!(matches!(
            parse(args(&["capability", "list"])),
            Ok(Parsed::Command(_))
        ));
        assert!(matches!(
            parse(args(&[
                "capability",
                "show",
                "architecture.dependency-analysis"
            ])),
            Ok(Parsed::Command(_))
        ));
        assert!(matches!(
            parse(args(&[
                "capability",
                "resolve",
                "architecture.dependency-analysis"
            ])),
            Ok(Parsed::Command(_))
        ));
        assert!(matches!(parse(args(&["--help"])), Ok(Parsed::Help)));
        assert!(matches!(parse(args(&["--version"])), Ok(Parsed::Version)));
    }

    #[test]
    fn rejects_malformed_commands_and_options() {
        assert!(matches!(parse(args(&[])), Ok(Parsed::Help)));
        assert!(matches!(parse(args(&["--json"])), Ok(Parsed::Help)));
        assert!(matches!(
            parse(args(&["--catalog=somewhere", "agent", "list"])),
            Ok(Parsed::Command(_))
        ));
        assert!(matches!(
            parse(args(&["--catalog="])),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            parse(args(&["--unknown"])),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            parse(args(&["--catalog"])),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            parse(args(&["agent", "show"])),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            parse(args(&["unknown", "list"])),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            parse(args(&["agent", "list", "extra"])),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            parse(args(&["agent", "list", "one", "two"])),
            Err(CliError::Usage(_))
        ));
    }

    #[test]
    fn emits_human_labels_for_nested_machine_reports() {
        let report = json!({
            "kind": "example",
            "some_value": ["one", "two"],
            "nested": {"inner_value": true},
            "objects": [{"id": "a"}],
        });
        let output = render_human(&report);
        assert!(output.contains("Some Value:"));
        assert!(output.contains("Inner Value: true"));
        assert!(output.contains("Id: a"));
    }

    #[test]
    fn serializes_selector_and_capability_contract_values() {
        let capability = CapabilityDefinition::new(
            CapabilityId::new("repository.read").unwrap(),
            CapabilityClass::Inspect,
        )
        .with_domain("repository")
        .unwrap()
        .with_description("Read repository state")
        .unwrap()
        .with_input_kinds(["repository.snapshot"])
        .unwrap()
        .with_output_kinds(["repository.state"])
        .unwrap()
        .with_preconditions(["repository.available"])
        .unwrap()
        .with_constraints(["read-only"])
        .unwrap()
        .with_applicability_tags(["repository"])
        .unwrap();
        let value = capability_value(&capability);
        assert_eq!(value["class"], "INSPECT");
        assert_eq!(value["input_kinds"][0], "repository.snapshot");
        assert_eq!(
            selector_value(&CapabilitySelector::Class(CapabilityClass::Inspect))["type"],
            "class"
        );
        assert_eq!(
            provider_value(&CapabilityProvider::Agent {
                agent_id: AgentId::new("reviewer").unwrap()
            })["source"],
            "agent:reviewer"
        );
    }

    #[test]
    fn reports_errors_with_stable_codes_and_exit_statuses() {
        let unknown = CliError::UnknownId {
            kind: "skill",
            id: "missing".to_owned(),
        };
        assert_eq!(unknown.exit_code(), EXIT_UNKNOWN_ID);
        assert_eq!(unknown.to_json()["code"], "unknown_id");
        assert_eq!(CliError::Usage("bad".to_owned()).exit_code(), EXIT_USAGE);
        assert_eq!(
            CliError::Load(RegistryError::Io {
                path: PathBuf::from("catalog"),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
            })
            .exit_code(),
            EXIT_CATALOG
        );
        let integrity = CliError::Integrity(RegistryIntegrityError::SkillNotFound {
            skill_id: SkillId::new("missing").unwrap(),
        });
        let invalid = CliError::InvalidId {
            kind: "skill",
            id: "bad/id".to_owned(),
            source: "invalid".to_owned(),
        };
        for error in [
            CliError::Usage("bad".to_owned()),
            CliError::Load(RegistryError::Io {
                path: PathBuf::from("catalog"),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
            }),
            integrity,
            invalid,
            unknown,
        ] {
            assert!(!error.to_string().is_empty());
            assert!(!error.code().is_empty());
            assert!(error.to_json().is_object());
            let _ = Error::source(&error);
        }
    }

    fn catalog_registry() -> Registry {
        Registry::load_catalog(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog"))
            .unwrap()
    }

    #[test]
    fn builds_every_supported_report_from_one_catalog_snapshot() {
        let registry = catalog_registry();
        let commands = [
            Command::AgentList,
            Command::AgentShow("system-architect".to_owned()),
            Command::SkillList,
            Command::SkillShow("quality-gate-governance".to_owned()),
            Command::SkillGraph("quality-gate-governance".to_owned()),
            Command::CapabilityList,
            Command::CapabilityShow("architecture.dependency-analysis".to_owned()),
            Command::CapabilityResolve("architecture.dependency-analysis".to_owned()),
        ];
        for command in &commands {
            let report = inspect(&registry, command).unwrap();
            assert!(report.is_object());
        }

        let query =
            CapabilityQuery::all().with_domain(CapabilityDomain::new("architecture").unwrap());
        let query_result = registry.capability_index().unwrap().query(&query);
        assert!(!query_result.rejections().is_empty());
        for rejection in query_result.rejections() {
            assert!(rejection_value(rejection).is_object());
        }
        assert_eq!(outcome_name(CapabilityQueryOutcome::NoMatch), "no_match");
        assert_eq!(outcome_name(CapabilityQueryOutcome::Unique), "unique");
        assert_eq!(outcome_name(CapabilityQueryOutcome::Ambiguous), "ambiguous");
    }

    #[test]
    fn covers_identifier_failures_catalog_failures_and_run_format_branches() {
        let registry = catalog_registry();
        assert!(matches!(
            agent_show(&registry, "bad/id"),
            Err(CliError::InvalidId { kind: "agent", .. })
        ));
        assert!(matches!(
            agent_show(&registry, "missing"),
            Err(CliError::UnknownId { kind: "agent", .. })
        ));
        assert!(matches!(
            skill_show(&registry, "bad/id"),
            Err(CliError::InvalidId { kind: "skill", .. })
        ));
        assert!(matches!(
            skill_show(&registry, "missing"),
            Err(CliError::UnknownId { kind: "skill", .. })
        ));
        assert!(matches!(
            skill_graph(&registry, "bad/id"),
            Err(CliError::InvalidId { kind: "skill", .. })
        ));
        assert!(matches!(
            skill_graph(&registry, "missing"),
            Err(CliError::UnknownId { kind: "skill", .. })
        ));
        assert!(matches!(
            capability_show(&registry, "bad/id"),
            Err(CliError::InvalidId {
                kind: "capability",
                ..
            })
        ));
        assert!(matches!(
            capability_show(&registry, "missing.capability"),
            Err(CliError::UnknownId {
                kind: "capability",
                ..
            })
        ));
        assert!(matches!(
            capability_resolve(&registry, "bad/id"),
            Err(CliError::InvalidId {
                kind: "capability",
                ..
            })
        ));
        assert!(matches!(
            capability_resolve(&registry, "missing.capability"),
            Err(CliError::UnknownId {
                kind: "capability",
                ..
            })
        ));

        let missing = PathBuf::from("this-catalog-does-not-exist");
        assert!(matches!(
            load_registry(&missing),
            Err(CliError::Load(RegistryError::Io { .. }))
        ));
        assert_eq!(run(args(&["--help"])), 0);
        assert_eq!(run(args(&["--version"])), 0);
        assert_eq!(
            run(args(&[
                "--catalog",
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../catalog")
                    .to_str()
                    .unwrap(),
                "agent",
                "list",
                "--json",
            ])),
            0
        );
        assert_eq!(
            run(args(&[
                "--catalog",
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../catalog")
                    .to_str()
                    .unwrap(),
                "capability",
                "list",
            ])),
            0
        );
        assert_eq!(
            run(args(&[
                "--catalog",
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../catalog")
                    .to_str()
                    .unwrap(),
                "capability",
                "show",
                "bad/id",
                "--json",
            ])),
            EXIT_USAGE
        );
        assert_eq!(
            run(args(&[
                "--catalog",
                "this-catalog-does-not-exist",
                "agent",
                "list",
            ])),
            EXIT_CATALOG
        );
    }

    #[test]
    fn covers_owned_skill_and_invalid_combined_registry_paths() {
        let owner = AgentId::new("owner").unwrap();
        let skill = SkillDefinitionDocument::new(
            SkillId::new("owned").unwrap(),
            "Owned skill",
            "Owned skill description",
            Some(owner.clone()),
            ["source"],
            ["rule"],
            ["verify"],
            [],
            [],
            [],
            [],
        )
        .unwrap();
        let valid = Registry::from_documents(
            [
                AgentDefinitionDocument::new(owner, "Owner", [SkillId::new("owned").unwrap()])
                    .unwrap(),
            ],
            [skill],
        )
        .unwrap();
        let value = agent_value(&valid, "owner");
        assert_eq!(value["owned_skill_ids"][0], "owned");

        let orphan = SkillDefinitionDocument::new(
            SkillId::new("orphan").unwrap(),
            "Orphan skill",
            "Orphan skill description",
            Some(AgentId::new("missing-owner").unwrap()),
            ["source"],
            ["rule"],
            ["verify"],
            [],
            [],
            [],
            [],
        )
        .unwrap();
        let invalid =
            Registry::from_documents(std::iter::empty::<AgentDefinitionDocument>(), [orphan])
                .unwrap();
        assert!(matches!(
            skill_graph(&invalid, "orphan"),
            Err(CliError::Integrity(
                RegistryIntegrityError::MissingAgentReference { .. }
            ))
        ));
        assert!(matches!(
            capability_list(&invalid),
            Err(CliError::Integrity(_))
        ));
    }

    #[test]
    fn covers_all_selector_serializations_and_human_scalar_shapes() {
        let capability_id = CapabilityId::new("capability").unwrap();
        let selectors = [
            CapabilitySelector::CapabilityId(capability_id),
            CapabilitySelector::Class(CapabilityClass::Inspect),
            CapabilitySelector::Domain(CapabilityDomain::new("domain").unwrap()),
            CapabilitySelector::InputKind(CapabilityInputKind::new("input").unwrap()),
            CapabilitySelector::OutputKind(CapabilityOutputKind::new("output").unwrap()),
            CapabilitySelector::Precondition(CapabilityPrecondition::new("precondition").unwrap()),
            CapabilitySelector::Constraint(CapabilityConstraint::new("constraint").unwrap()),
            CapabilitySelector::ApplicabilityTag(CapabilityTag::new("tag").unwrap()),
        ];
        for selector in &selectors {
            assert!(selector_value(selector).is_object());
        }

        let scalar_report = json!({
            "null_value": null,
            "boolean_value": false,
            "number_value": 7,
            "empty_values": [],
        });
        let mut output = String::new();
        render_value(&scalar_report, 0, &mut output);
        render_value(&json!("scalar"), 0, &mut output);
        render_value(&json!(["array"]), 0, &mut output);
        assert!(output.contains("Null Value: none"));
        assert!(output.contains("Boolean Value: false"));
        assert!(output.contains("Number Value: 7"));
        assert!(output.contains("scalar"));
        assert!(output.contains("- array"));
        assert_eq!(scalar(&Value::Null), "none");
        assert_eq!(scalar(&json!(true)), "true");
        assert_eq!(scalar(&json!(7)), "7");
        assert_eq!(scalar(&json!("text")), "text");
        assert_eq!(scalar(&json!([])), "(details)");
        assert_eq!(human_label(""), "");
    }
}
