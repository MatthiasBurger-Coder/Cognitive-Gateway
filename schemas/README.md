# Schemas

This directory is the canonical home for versioned, machine-readable project and runtime contracts.

The bootstrap slice reserves the following schema boundaries:

- `agent.schema.json`
- `skill.schema.json`
- `workflow.schema.json`
- `policy.schema.json`
- `project-state.schema.json`
- `execution-context.schema.json`

The CG-03.03 Agent and Skill contracts are implemented by
[`agent.schema.json`](agent.schema.json) and [`skill.schema.json`](skill.schema.json),
with representative fixtures under [`examples/`](examples/). Their field
mapping and validation boundary are documented in
[`../docs/agent-skill-definition-contracts.md`](../docs/agent-skill-definition-contracts.md).
Profile loading and deterministic registry validation are later CG-03 slices.
The CG-02 JSON wire contract for `ExecutionContextIR` is documented and
implemented by the domain crate in [`../docs/ir-serialization.md`](../docs/ir-serialization.md);
its JSON Schema artifact remains a later schema-loading deliverable.

Schema documents must be versioned, fail closed on invalid input and remain independent of concrete RAG, MCP and execution-runtime technologies.
