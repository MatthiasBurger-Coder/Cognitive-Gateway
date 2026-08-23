# Versioned Agent and Skill Definition Contracts (CG-03.10)

The canonical Agent and Skill contracts are strict, self-contained JSON
documents. Their semantic content is the source of truth; loading or runtime
resolution never requires a project repository, an external `SKILL.md` or a
`content_ref`.

## Contract boundary

Both documents use numeric `schema_version: 2`. JSON decoding rejects unknown
fields, and domain constructors validate identifiers, text and relationships.
Agents retain ordered Skill references. Skills contain their complete
declarative content and distinguish mandatory Skill dependencies from optional
or related references.

### Agent

```json
{
  "schema_version": 2,
  "kind": "agent",
  "id": "system-architect",
  "description": "Cross-module boundaries and architecture decisions",
  "skill_ids": ["architecture-hexagonal"]
}
```

### Skill

```json
{
  "schema_version": 2,
  "kind": "skill",
  "id": "archunit-expert",
  "name": "ArchUnit Expert",
  "description": "Architecture tests and package-boundary validation.",
  "authoritative_sources": ["architecture documentation", "architecture tests"],
  "rules": ["Do not weaken architecture rules to make a change pass."],
  "verification": ["Run the affected architecture tests and quality gate."],
  "requires": ["hexagonal-architecture-expert"],
  "related_skills": ["quality-gate-expert"]
}
```

The existing typed `owner_agent_id`, `required_capability_ids` and
`knowledge_queries` fields remain available as optional semantic extensions to
the Skill contract. They do not grant authority. Capability authority remains
the responsibility of policy evaluation.

| Field | Rule | Meaning |
| --- | --- | --- |
| `name` | Required validated text. | Human-readable Skill name. |
| `description` | Required validated text. | Responsibility summary. |
| `authoritative_sources` | Ordered validated text list. | Declarative selectors or patterns for applicable source material; not an external content dependency. |
| `rules` | Ordered validated text list. | Declarative instructions and constraints. |
| `verification` | Ordered validated text list. | Declarative checks and evidence guidance. |
| `requires` | Ordered unique typed `SkillId` list. | Mandatory dependencies included in graph validation and cycle detection. |
| `related_skills` | Ordered unique typed `SkillId` list. | Optional/contextual references; never activation or capability authority. |

## Reference behavior

Skill IDs are canonical typed identifiers, never repository paths or aliases.
Self-references and duplicate references are rejected at document parsing.
The same ID may not appear in both `requires` and `related_skills`. The
registry rejects missing targets in either list, but only `requires` contributes
to the deterministic dependency-first topological order and mandatory cycle
detection.

Agent-to-Skill and Skill owner-Agent references are validated by the catalog
registry. A diagnostic identifies the canonical definition (`agent:<id>` or
`skill:<id>`).

## Serialization and failure behavior

`AgentDefinitionDocument` and `SkillDefinitionDocument` expose typed accessors,
`to_json()` and `from_json()`. `to_domain()` returns the corresponding domain
definition without losing structured Skill content. Direct serde
serialization uses the same strict wire contract.

The parser fails closed when JSON is malformed, `schema_version` is not the
numeric value `2`, `kind` is wrong, a required field is absent, an obsolete
field such as `origin` or `content_ref` is present, or any typed value fails
validation. Schema files under `schemas/` mirror these rules. Runtime Agent and
Skill documents contain no consuming-project identity, repository path or
provenance metadata.
