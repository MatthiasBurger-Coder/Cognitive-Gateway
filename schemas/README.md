# Schemas

This directory is the canonical home for versioned, machine-readable project and runtime contracts.

The bootstrap slice reserves the following schema boundaries:

- `agent.schema.json`
- `skill.schema.json`
- `workflow.schema.json`
- `policy.schema.json`
- `project-state.schema.json`
- `execution-context.schema.json`

The machine-readable schema documents, profile loading and deterministic
profile validation are implemented in CG-03. The CG-02 JSON wire contract for
`ExecutionContextIR` is documented and implemented by the domain crate in
[`../docs/ir-serialization.md`](../docs/ir-serialization.md); the JSON Schema
artifact remains a later schema-loading deliverable.

Schema documents must be versioned, fail closed on invalid input and remain independent of concrete RAG, MCP and execution-runtime technologies.
