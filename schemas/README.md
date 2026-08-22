# Schemas

This directory is the canonical home for versioned, machine-readable project and runtime contracts.

The bootstrap slice reserves the following schema boundaries:

- `agent.schema.json`
- `skill.schema.json`
- `workflow.schema.json`
- `policy.schema.json`
- `project-state.schema.json`
- `execution-context.schema.json`

The schemas themselves, profile loading and deterministic validation are implemented in CG-03. Keeping the boundary here makes the repository layout explicit without coupling the Rust core to a serialization or validation framework.

Schema documents must be versioned, fail closed on invalid input and remain independent of concrete RAG, MCP and execution-runtime technologies.
