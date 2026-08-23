# Tiny Swarm World profile

This is the project-specific profile boundary for Tiny Swarm World. It keeps
TSW product, repository and platform assumptions out of the reusable
Cognitive Gateway catalog.

```text
profiles/tiny-swarm-world/
├── agents/      # TSW-specific AgentDefinition documents
├── skills/      # TSW-specific SkillDefinition documents
├── workflows/   # reserved for CG-05; no workflow migration in CG-03.06
├── policies/    # reserved for profile policy slices
├── retrieval/   # reserved for profile retrieval configuration
└── tools/       # reserved for abstract capability adapters
```

Agent and Skill documents use the same strict versioned contracts as the
generic catalog. They remain declarative and provider/runtime independent at
the domain-contract level. TSW-specific knowledge such as Docker Swarm,
LXD/Incus, Portainer, Linux/WSL and TSW repository conventions is allowed here
when supported by the migration matrix.

## Boundary rules

- The profile is optional; `Registry::load_catalog("catalog")` must work
  without it.
- Profile definitions may reference generic catalog Agent/Skill IDs when the
  combined loader is used.
- Profile definitions may not redefine a catalog ID. The combined loader
  rejects such collisions with both provenance values; no override or
  last-writer-wins behavior exists.
- Profile loading is deterministic: JSON files are discovered in normalized
  relative-path order and exposed in canonical ID order.
- Every migrated definition retains `origin.project`, `origin.source` and
  `origin.migration_status`.
- Profile ownership does not grant capability authority. Capability
  requirements still require an independent policy decision.

Use `Registry::load_profile("profiles/tiny-swarm-world")` for the profile
boundary alone, or
`Registry::load_catalog_with_profile("catalog", "profiles/tiny-swarm-world")`
for the combined registry. Call `validate_integrity()` after loading when
cross-document Agent/Skill references and dependency graphs must be checked.

## Current migration

CG-03.09 materializes four TSW-specific Agents and 52 TSW-specific Skills
selected by the CG-03.02 migration matrix. Profile Agents may reference generic
catalog Skills; the combined loader rejects any duplicate canonical ID.
Scope-gated candidates remain excluded pending explicit approval, and profile
ownership does not authorize execution.
