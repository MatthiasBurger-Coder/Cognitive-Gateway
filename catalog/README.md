# Agent and Skill catalog

This directory is the repository boundary for reusable Cognitive Gateway Agent
and Skill definitions. It is the only built-in definition source and is
independent of any consuming project.

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
`Registry::load_catalog("catalog")`; no external project context is required
for catalog loading, validation or Skill resolution.

## Ownership rules

- Only reusable responsibility and skill semantics belong here.
- Project paths, repository layout and other consuming-project conventions do
  not belong in the catalog. They arrive through explicit runtime, retrieval
  or adapter boundaries.
- Workflow/process behavior, prompts, model/provider details and executable
  tool instructions do not belong in Agent or Skill documents.
- Runtime project knowledge is contextual input and cannot change catalog
  membership. There is no secondary built-in registry, merge operation or
  override/precedence rule.

## Catalog contents

The catalog contains every reusable Agent and Skill definition, including
narrow technology specialists. Consuming-project knowledge and configuration
remain outside this catalog.

Skill applicability is represented through the v2 self-contained contract:
structured content, optional agent ownership, abstract capability
requirements, retrieval hints, mandatory `requires` dependencies and optional
`related_skills`. A Skill does not gain a project selector or an execution
permission; applicability and capability authorization remain separate
registry/policy concerns.

`Registry::resolve_skill(skill_id)` returns an owned `ResolvedSkillGraph` for
the requested canonical ID and its transitive `requires` closure. The result
is dependency-first and deterministic, and contains the complete structured
Skill documents needed by a later Context Compiler. Related Skills remain
available on each document but are not implicitly included. Resolution needs
only this catalog: consuming-project paths, identities, provenance, external
`SKILL.md` files and retrieved knowledge are outside the graph.
