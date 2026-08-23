# Generic catalog

This directory is the repository boundary for reusable Cognitive Gateway Agent
and Skill definitions. It is intentionally independent of every project
profile, including Tiny Swarm World.

```text
catalog/
├── agents/   # reusable AgentDefinition documents
└── skills/   # reusable SkillDefinition documents
```

Definitions use the strict versioned JSON contracts in
[`../schemas/agent.schema.json`](../schemas/agent.schema.json) and
[`../schemas/skill.schema.json`](../schemas/skill.schema.json). Their
descriptions must remain provider-, runtime- and project-independent. They may
declare abstract capability requirements and retrieval hints, but neither is
permission to execute.

The catalog can be loaded on its own with
`Registry::load_catalog("catalog")`. A project profile is never required for
catalog loading or validation.

## Ownership rules

- Only reusable responsibility and skill semantics belong here.
- TSW paths, Docker Swarm, LXD/Incus, Portainer, Linux/WSL, repository layout
  and other product conventions belong in the TSW profile or an adapter.
- Workflow/process behavior, prompts, model/provider details and executable
  tool instructions do not belong in Agent or Skill documents.
- Migrated or merged definitions retain complete source provenance.
- A profile may reference a catalog definition by its canonical ID, but may not
  redefine it.

The combined loader reads the catalog before the selected profile, sorts the
result by canonical ID, and rejects any cross-boundary Agent or Skill ID
collision. There is no implicit precedence or override behavior.

## Current migration

CG-03.07 materializes the reusable Tiny Swarm World candidates selected by the
CG-03.02 migration matrix: eight catalog Agents and 37 canonical catalog
Skills. Merged skills retain all source paths in `origin.source`; deferred,
project-specific and deprecated candidates remain outside this catalog.
