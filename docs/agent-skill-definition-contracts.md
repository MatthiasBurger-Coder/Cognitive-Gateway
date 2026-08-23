# Versioned Agent and Skill Definition Contracts (CG-03.03)

This document defines the repository-native v1 documents consumed by the
Agent and Skill Registry. The canonical machine-readable contracts are
[`schemas/agent.schema.json`](../schemas/agent.schema.json) and
[`schemas/skill.schema.json`](../schemas/skill.schema.json). JSON is the
interchange form; equivalent YAML may be used by a later repository adapter
provided it produces the same fields and validation result.

## Contract boundary

An Agent document declares a responsibility and its ordered skills. A Skill
document declares reusable knowledge and abstract capability requirements.
They are declarative data and contain no executable code. Workflow lifecycle,
prompt, model, runtime, transport, tool invocation and provider-specific
fields are outside these contracts and are rejected as unknown fields.

The document envelope has two layers:

| Layer | Fields | Mapping |
| --- | --- | --- |
| Versioned repository envelope | `schema_version`, `kind`, `origin` | Contract compatibility and provenance; not execution behavior. |
| CG-02 semantic definition | `id`, `description`, relationship fields | Converted to the typed CG-02 `AgentDefinition` or `SkillDefinition`. |

Both documents require `schema_version: "1.0"`. The numeric value `1`, future
minor versions, future major versions, malformed versions and reserved major
version `0` are rejected. There is no implicit upgrade or downgrade path.

## Agent document

```json
{
  "schema_version": "1.0",
  "kind": "agent",
  "id": "system-architect",
  "description": "Cross-module boundaries and architecture decisions",
  "skill_ids": ["architecture-hexagonal", "contract-governance-expert"],
  "origin": {
    "project": "Tiny-Swarm-World",
    "source": ".agents/roles/senior-system-architect.md",
    "migration_status": "MIGRATED"
  }
}
```

| Field | Rule | CG-02 mapping |
| --- | --- | --- |
| `schema_version` | Required exact string `1.0`. | `SchemaVersion::V1` at the document boundary. |
| `kind` | Required exact string `agent`. | Selects `AgentDefinition`. |
| `id` | Required valid `AgentId`. | `AgentDefinition::id`. |
| `description` | Required validated non-empty text. | `AgentDefinition::description`. |
| `skill_ids` | Required, non-empty, ordered and unique valid `SkillId` values. | `AgentDefinition::skill_ids`; existence is checked by `DefinitionCatalog`. |
| `origin` | Required complete provenance object. | Retained by the document and registry; intentionally not execution authority. |

## Skill document

```json
{
  "schema_version": "1.0",
  "kind": "skill",
  "id": "architecture-hexagonal",
  "description": "Hexagonal boundaries and dependency direction",
  "owner_agent_id": "system-architect",
  "dependency_ids": [],
  "required_capability_ids": ["repository.read"],
  "knowledge_queries": ["hexagonal architecture boundaries"],
  "origin": {
    "project": "Tiny-Swarm-World",
    "source": ".agents/skills/architecture-hexagonal/SKILL.md",
    "migration_status": "MERGED"
  }
}
```

| Field | Rule | CG-02 mapping |
| --- | --- | --- |
| `schema_version` | Required exact string `1.0`. | `SchemaVersion::V1` at the document boundary. |
| `kind` | Required exact string `skill`. | Selects `SkillDefinition`. |
| `id` | Required valid `SkillId`. | `SkillDefinition::id`. |
| `description` | Required validated non-empty text. | `SkillDefinition::description`. |
| `owner_agent_id` | Required field; a valid `AgentId` or JSON `null`. | `SkillDefinition::owner_agent_id`. |
| `dependency_ids` | Ordered and unique valid `SkillId` values; self-dependencies are rejected. | `SkillDefinition::dependency_ids`; missing targets and cycles are checked by `DefinitionCatalog`. |
| `required_capability_ids` | Ordered and unique valid `CapabilityId` values. | `SkillDefinition::required_capability_ids`; requirements do not grant permission. |
| `knowledge_queries` | Ordered validated `KnowledgeQuery` values. | `SkillDefinition::knowledge_queries`; retrieval remains separate from authority. |
| `origin` | Required complete provenance object. | Retained by the document and registry. |

## Provenance and migration status

`origin.project` identifies the source project or repository. `origin.source`
identifies the original path or another source reference and may contain path
characters that are not valid gateway IDs. Both values are required,
non-empty text. `origin.migration_status` is one of:

- `NATIVE` — authored in the Cognitive Gateway format;
- `MIGRATED` — imported from one source definition; or
- `MERGED` — normalized from multiple source definitions.

Consequently, a migrated or merged TSW document cannot omit its project or
source. Deprecated or excluded candidates are not valid registry documents and
must remain in migration evidence rather than being silently imported. The
CG-03.02 migration matrix remains the authority for whether a candidate is
catalog-owned, TSW-profile-owned, merged, deferred or deprecated.

## Validation and failure behavior

Validation is fail-closed and deterministic:

1. JSON syntax and required-field/type errors fail at document decoding.
2. Unknown document or `origin` fields fail because the contract denies
   unknown fields.
3. The document version and exact kind are checked.
4. IDs, descriptions, provenance, relationships and knowledge queries are
   converted through CG-02 constructors.
5. The deterministic registry loader discovers JSON files, parses every
   definition file in lexical relative-path order, rejects duplicate canonical
IDs and exposes accepted documents in canonical ID order. Cross-document
references and dependency cycles are validated by the registry integrity API;
document parsing does not infer references from text or retrieval.

The Rust API is exposed from `gateway_domain` as
`AgentDefinitionDocument` and `SkillDefinitionDocument` (also available as
`VersionedAgentDefinition` and `VersionedSkillDefinition`). Their
`to_domain()` methods produce the corresponding CG-02 value object while
`origin()` retains repository provenance. `from_json()` distinguishes JSON
decoding failures from domain validation failures through `SerializationError`.

## Deliberate exclusions

The CG-02 model does not provide semantic fields for `owns`, `applies_to`,
agent-level capability allow-lists, profile selectors or definition-level
execution constraints. Those concepts must not be smuggled into these
documents under ambiguous names. Applicability and ownership policy belong in
the later registry/profile or workflow layers; capability authority belongs to
`PolicyDefinition` and the compiled `ExecutionContextIR`. This keeps the v1
document mapping lossless for every supported CG-02 field and prevents
provider/runtime leakage.

The representative normalized TSW fixtures are:

- [`schemas/examples/agent-system-architect.json`](../schemas/examples/agent-system-architect.json)
- [`schemas/examples/skill-architecture-hexagonal.json`](../schemas/examples/skill-architecture-hexagonal.json)

They preserve TSW source provenance while using canonical IDs and provider-
neutral descriptions.

## Deterministic registry loading

`gateway-registry` exposes `AgentRegistry` and `SkillRegistry` loaders. Each
loader accepts a directory, recursively discovers files whose extension is
`json`, and treats every discovered file as a definition document. Discovery
is sorted by normalized relative path before parsing, so the first diagnostic
and duplicate-ID path are reproducible. Valid documents are then sorted by
their canonical typed ID for stable iteration and binary-search lookup.

`Registry::load(profile_directory)` loads the conventional `agents/` and
`skills/` directories together. `Registry::validate_integrity()` then checks
Agent-to-Skill references, Skill owner Agents, missing Skill dependencies and
dependency cycles. Its `dependency_graph()` result provides a deterministic
dependency-first topological order. Missing roots, unreadable files, malformed
JSON, wrong kinds, unsupported schema versions and domain-invalid values all
fail the load; invalid JSON files are never silently skipped. Files with other
extensions are ignored because JSON is the only repository adapter currently
implemented. Loading performs no agent or skill execution and has no RAG,
LLM or provider dependency.
