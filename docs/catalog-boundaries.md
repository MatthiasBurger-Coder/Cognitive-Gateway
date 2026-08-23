# Agent and Skill catalog boundary

The Cognitive Gateway repository owns one built-in, project-independent Agent
and Skill catalog. Reusable responsibility and skill semantics live under
`catalog/`; consuming-project knowledge and configuration enter only through
explicit runtime, retrieval or adapter boundaries.

## Repository layout

```text
catalog/
├── agents/
└── skills/
```

`catalog/agents/` is the only Agent discovery boundary. `catalog/skills/` is
the only built-in Skill discovery boundary. Both use the strict v2
self-contained JSON contracts under `schemas/`.

## Loading contract

The `gateway-registry` crate discovers JSON definitions in lexical relative
path order and exposes them in canonical ID order:

| API | Behavior |
| --- | --- |
| `Registry::load_catalog(path)` | Loads `path/agents` and `path/skills` as the built-in catalog. |
| `Registry::load(path)` | Alias for loading one catalog directory. |
| `Registry::load_from_directories(agents, skills)` | Loads explicit catalog boundaries for composition and tests. |

Malformed documents, unsupported schema versions and duplicate canonical IDs
fail closed. `Registry::validate_integrity()` then validates Agent-to-Skill,
Skill-owner, related-Skill and mandatory dependency references, as well as
equivalence of shared capability declarations. There is no secondary built-in
registry, merge operation or override rule.

## Resolution and runtime context

`Registry::resolve_skill(skill_id)` resolves only the requested canonical Skill
ID and its transitive `requires` closure. It returns complete Skill documents
in deterministic dependency-first order. `related_skills` remain informational
references and do not activate additional graph members.

Resolution requires only the built-in catalog. Repository content, project
configuration, retrieved knowledge, external `SKILL.md` files and runtime
state are contextual inputs owned by explicit ports or adapters; they cannot
create or change Agent/Skill catalog membership.

## Definition boundary

Agent and Skill documents contain reusable declarative semantics, typed
canonical IDs, abstract capability requirements and optional typed
`provided_capabilities` contracts. They do not contain consuming-project
identities, repository paths, provenance fields, external content references or
runtime authorization. Technology-specific expertise is valid when it remains
reusable across consumers.
