# Example Agents

Reserved for project-specific agent responsibility declarations. Agent definitions belong to the project profile and must not be embedded in the generic gateway core.

Agent definitions in this directory use the strict versioned JSON contract in
[`../../../schemas/agent.schema.json`](../../../schemas/agent.schema.json).
`gateway-registry::AgentRegistry` loads them deterministically; executable
agent behavior is outside the registry.
