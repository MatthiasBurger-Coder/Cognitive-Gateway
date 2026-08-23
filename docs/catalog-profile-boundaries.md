# Generic catalog and project profile boundaries (CG-03.06)

CG-03.06 establishes the physical and semantic boundary between reusable
Cognitive Gateway definitions and project-specific definitions. Tiny Swarm
World is the first reference profile, but the rule is intended to apply to
future projects as well.

## Repository layout

```text
catalog/
├── agents/
└── skills/

profiles/
└── tiny-swarm-world/
    ├── agents/
    ├── skills/
    ├── workflows/
    ├── policies/
    ├── retrieval/
    └── tools/
```

`catalog/agents/` and `catalog/skills/` contain reusable Agent and Skill
documents. The corresponding TSW directories contain only TSW-specific
documents. The remaining profile directories reserve later workflow, policy,
retrieval and adapter boundaries; CG-03.06 does not load or migrate them.

## Ownership and leakage rules

Generic definitions must be usable without TSW. They may describe
provider-independent responsibilities such as architecture, storage,
observability, resilience, security, Git, gRPC or Protobuf. They must not
assume TSW paths, Docker Swarm, LXD/Incus, Portainer, Linux/WSL, a particular
repository layout, a provider prompt, a model or a runtime package.

TSW profile definitions may retain product, repository and platform conventions
identified as `project-specific:tiny-swarm-world` by the CG-03.02 migration
matrix. Profile ownership does not authorize a capability: capability IDs are
abstract requirements and still require an independent policy decision.

Agent and Skill documents remain declarative. Workflow lifecycle, handoff,
branch/commit/PR process, prompts, model selection, runtime behavior and
concrete tool invocation remain outside these documents; workflow/process
semantics are deferred to CG-05.

## Loading contract

The `gateway-registry` crate exposes three explicit boundaries:

| API | Behavior |
| --- | --- |
| `Registry::load_catalog(path)` | Loads only `path/agents` and `path/skills`; no project profile is needed. |
| `Registry::load_profile(path)` | Loads only one project's `agents` and `skills`. |
| `Registry::load_catalog_with_profile(catalog, profile)` | Loads both boundaries, then combines them deterministically. |

The existing `Registry::load(path)` remains the directory loader used by both
named boundary methods. JSON files are discovered in normalized relative-path
order, every JSON file is parsed, and accepted documents are exposed in
canonical ID order. Non-JSON boundary documentation is ignored by the JSON
adapter.

Profiles may reference generic definitions by their canonical typed IDs. The
combined registry validates those references and the complete Skill dependency
graph after loading. A profile may not replace or shadow a generic definition:
an Agent or Skill ID present in both boundaries produces the deterministic,
fail-closed `CrossBoundaryDuplicateDefinition` error containing catalog and
profile provenance. There is no precedence, merge-by-path or last-writer-wins
rule. Duplicate IDs within either boundary continue to use the existing
duplicate-definition error.

This means the valid resolution shape is:

```text
catalog definition ──┐
                      ├── deterministic combined registry ── integrity validation
TSW profile reference ┘
```

The reverse dependency is prohibited: reusable catalog content must not need a
TSW profile to load or validate.

## Provenance

Both boundaries use the same strict versioned Agent and Skill contracts. Every
migrated document retains `origin.project`, `origin.source` and
`origin.migration_status`. Provenance is also included in cross-boundary
conflict and integrity diagnostics, so a normalized definition can be traced
back to its source without consulting retrieval or runtime state.

The CG-03.02 migration matrix remains authoritative for classification,
canonical IDs, merge targets and deprecated exclusions. CG-03.06 creates the
containers and resolution rules; the actual generic and TSW migrations are
performed by CG-03.07, CG-03.08 and CG-03.09.
