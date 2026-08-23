# Versioned Agent, Skill and Capability Definition Contracts (CG-03.16)

The canonical Agent and Skill contracts are strict, self-contained JSON
documents. Their semantic content is the source of truth; loading or runtime
resolution never requires a project repository, an external `SKILL.md` or a
`content_ref`.

## Contract boundary

Both documents use numeric `schema_version: 2`. JSON decoding rejects unknown
fields, and domain constructors validate identifiers, text and relationships.
Agents retain ordered Skill references and may declare reusable capabilities
they provide directly. Skills contain their complete declarative content,
distinguish mandatory Skill dependencies from optional or related references,
and may declare reusable capabilities they provide directly.

### Agent

```json
{
  "schema_version": 2,
  "kind": "agent",
  "id": "system-architect",
  "description": "Cross-module boundaries and architecture decisions",
  "skill_ids": ["architecture-hexagonal"],
  "provided_capabilities": []
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
  "related_skills": ["quality-gate-expert"],
  "provided_capabilities": [
    {
      "id": "architecture.dependency-analysis",
      "class": "INSPECT",
      "domain": "architecture",
      "description": "Analyze dependency direction and boundary relationships",
      "input_kinds": ["repository.snapshot"],
      "output_kinds": ["architecture.dependency-graph"],
      "preconditions": ["repository.available"],
      "constraints": ["read-only"],
      "applicability_tags": ["architecture", "dependency-analysis"]
    }
  ]
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
| `provided_capabilities` | Ordered unique capability contracts. | Reusable capabilities directly provided by the Agent or Skill; the containing definition establishes the provider relationship. |

## Capability contract

`provided_capabilities` is an optional field on both Agent and Skill documents.
Each entry is a complete, project-independent capability contract:

| Field | Rule | Meaning |
| --- | --- | --- |
| `id` | Required canonical `CapabilityId`. | Stable identity used for exact deterministic matching. |
| `class` | `INSPECT` or `MUTATE`. | Safety classification; `MUTATE` remains subject to policy. |
| `domain` | Required canonical `CapabilityDomain`. | Reusable capability domain such as `architecture` or `quality`. |
| `description` | Required validated text. | Responsibility or purpose for explainability. |
| `input_kinds` | Ordered unique canonical `CapabilityInputKind` list. | Required input or context classes. |
| `output_kinds` | Ordered unique canonical `CapabilityOutputKind` list. | Result kinds produced by the capability. |
| `preconditions` | Ordered unique canonical list. | Conditions intrinsic to using the capability. |
| `constraints` | Ordered unique canonical list. | Intrinsic limitations; these are not policy decisions. |
| `applicability_tags` | Ordered unique canonical list. | Deterministic selectors for matching and filtering. |

Capability contracts describe what a reusable Agent or Skill can provide. They
do not describe what should be done now, contain project state or authorize
execution. `required_capability_ids` remains a Skill requirement relationship;
`provided_capabilities` is the provider declaration. Policy and
`approved_capability_ids` remain the authority boundary.

The same capability ID may be provided by multiple Agents or Skills, but all
providers must declare equivalent semantic metadata when the catalog is
validated. A duplicate ID within one Agent or Skill is rejected. This gives a
derived capability index a stable contract while preserving all provider
relationships.

## Reference behavior

Skill IDs are canonical typed identifiers, never repository paths or aliases.
Self-references and duplicate references are rejected at document parsing.
The same ID may not appear in both `requires` and `related_skills`. The
registry rejects missing targets in either list, but only `requires` contributes
to the deterministic dependency-first topological order and mandatory cycle
detection.

Agent-to-Skill, Skill owner-Agent and capability declaration relationships are
validated by the catalog registry. A diagnostic identifies the canonical
definition (`agent:<id>` or `skill:<id>`).

## Serialization and failure behavior

`AgentDefinitionDocument` and `SkillDefinitionDocument` expose typed accessors
for provided capabilities, `to_json()` and `from_json()`. `to_domain()` returns
the corresponding domain definition without losing structured Skill content or
capability metadata. Capability entries also support direct strict serde
serialization.

The parser fails closed when JSON is malformed, `schema_version` is not the
numeric value `2`, `kind` is wrong, a required field is absent, an obsolete
field such as `origin` or `content_ref` is present, or any typed value fails
validation. Schema files under `schemas/` mirror these rules. Runtime Agent and
Skill documents contain no consuming-project identity, repository path or
provenance metadata.
