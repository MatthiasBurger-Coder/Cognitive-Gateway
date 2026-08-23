# Schemas

This directory is the canonical home for versioned, machine-readable project and runtime contracts.

The bootstrap slice reserves the following schema boundaries:

- `agent.schema.json`
- `skill.schema.json`
- `workflow.schema.json`
- `policy.schema.json`
- `project-state.schema.json`
- `execution-context.schema.json`

The Agent and Skill contracts are implemented by
[`agent.schema.json`](agent.schema.json) and [`skill.schema.json`](skill.schema.json),
with representative fixtures under [`examples/`](examples/). Their field
mapping and validation boundary are documented in
[`../docs/agent-skill-definition-contracts.md`](../docs/agent-skill-definition-contracts.md).
Definitions are stored under [`../catalog/`](../catalog/), the sole built-in
Agent/Skill catalog. The catalog boundary and loading rules are documented in
[`../docs/catalog-boundaries.md`](../docs/catalog-boundaries.md).
The `gateway-registry` crate provides deterministic JSON catalog loading. It
recursively discovers `*.json` files in lexical relative-path order, rejects
malformed or unsupported documents and duplicate canonical IDs, and exposes
the resulting documents in canonical ID order. Non-JSON files are outside the
current JSON adapter. `Registry::validate_integrity()` provides the separate
cross-definition reference and Skill dependency-graph validation step.
The CG-02 JSON wire contract for `ExecutionContextIR` is documented and
implemented by the domain crate in [`../docs/ir-serialization.md`](../docs/ir-serialization.md);
its JSON Schema artifact remains a later schema-loading deliverable.

The v2 Agent and Skill documents are self-contained: structured Skill content
and canonical required/related Skill references live in the definition itself.
Provenance, external content references and consuming-project `SKILL.md` paths
are not runtime fields. Schema documents must be versioned, fail closed on
invalid input and remain independent of concrete RAG, MCP and execution-runtime
technologies.
