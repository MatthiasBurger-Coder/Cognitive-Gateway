# Generic catalog and project profile boundaries

CG-03.06 established the physical and semantic boundary between reusable
Cognitive Gateway definitions and project-specific definitions. CG-03.13
applies that boundary to Agents: reusable specialist responsibility belongs in
the generic catalog, while project profiles do not provide an Agent partition.
The rule applies consistently to every project profile.

## Repository layout

```text
catalog/
├── agents/
└── skills/

profiles/
└── example-project/
    ├── skills/
    ├── workflows/
    ├── policies/
    ├── retrieval/
    └── tools/
```

`catalog/agents/` and `catalog/skills/` contain reusable Agent and Skill
documents. A project profile has no Agent boundary: all reusable Agents,
including narrow technology specialists, are catalog entries. Project-specific
Skills and the remaining profile directories reserve later workflow, policy,
retrieval and adapter boundaries.

## Ownership and leakage rules

Generic definitions must be usable without a project profile. They may describe
provider-independent responsibilities such as architecture, storage,
observability, resilience, security, Git, gRPC or Protobuf. They must not
assume project paths, Docker Swarm, LXD/Incus, Portainer, Linux/WSL, a particular
repository layout, a provider prompt, a model or a runtime package.

Project profile definitions may retain product, repository and platform
conventions in their supported boundaries. Profile ownership does not
authorize a capability: capability IDs are abstract requirements and still
require an independent policy decision. Project knowledge and activation
evidence are runtime or retrieval inputs, not Agent catalog data.

Agent and Skill documents remain declarative. Workflow lifecycle, handoff,
branch/commit/PR process, prompts, model selection, runtime behavior and
concrete tool invocation remain outside these documents; workflow/process
semantics are deferred to CG-05.

## Loading contract

The `gateway-registry` crate exposes three explicit boundaries:

| API | Behavior |
| --- | --- |
| `Registry::load_catalog(path)` | Loads only `path/agents` and `path/skills`; no project profile is needed. |
| `Registry::load_profile(path)` | Loads only one project's `skills`; profile Agent directories are not discovered. |
| `Registry::load_catalog_with_profile(catalog, profile)` | Loads both boundaries, then combines them deterministically. |

The existing `Registry::load(path)` remains the directory loader used by both
named boundary methods. JSON files are discovered in normalized relative-path
order, every JSON file is parsed, and accepted documents are exposed in
canonical ID order. Non-JSON boundary documentation is ignored by the JSON
adapter.

Profiles may reference generic definitions by their canonical typed IDs. The
combined registry validates those references and the complete Skill dependency
graph and related Skill references after loading. A profile may not replace or
shadow a generic definition: a Skill ID present in both boundaries produces
the deterministic, fail-closed `CrossBoundaryDuplicateDefinition` error
containing both canonical definitions. Agents are never loaded from a profile,
so there is no profile Agent override path. There is no precedence, merge-by-
path or last-writer-wins rule. Duplicate IDs within either boundary continue
to use the existing duplicate-definition error.

This means the valid resolution shape is:

```text
catalog definition ──┐
                      ├── deterministic combined registry ── integrity validation
project profile reference ┘
```

The reverse dependency is prohibited: reusable catalog content must not need a
project profile to load or validate.

For a context-ready Skill closure, `Registry::resolve_skill(skill_id)` starts
at one canonical Skill ID and recursively follows only mandatory `requires`
edges. It returns owned complete Skill documents in deterministic
dependency-first order. `related_skills` are validated and exposed as
informational references, but do not activate additional graph members.
Resolution succeeds against `Registry::load_catalog` alone; an external
project profile is neither required nor consulted. An independently supplied
project context can therefore coexist without changing the generic graph
unless a separate, explicit selection contract chooses another root.

## Contract independence

Both boundaries use the same strict v2 Agent and Skill contracts. Skill
semantics are self-contained in structured fields and all cross-Skill edges use
canonical Skill IDs. Project paths, migration status, provenance metadata and
external `SKILL.md` content references are outside the runtime contract.

Incomplete context-ready Skill documents (missing authoritative sources,
rules or verification guidance) fail during graph resolution. Obsolete or
project-bound fields such as `origin`, `content_ref` or arbitrary project
metadata fail strict document decoding; project knowledge belongs at runtime
through retrieval/input ports rather than in the reusable Skill graph.

The canonical Agent catalog is the sole Agent discovery boundary. Project
profiles may reference catalog Agents by canonical ID but cannot define,
replace or shadow them. Historical migration records, source-project identity
and provenance remain outside runtime Agent definitions.
