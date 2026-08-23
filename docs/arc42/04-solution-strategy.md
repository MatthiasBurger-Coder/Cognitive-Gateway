# 4. Solution Strategy

Cognitive Gateway uses a layered control-plane architecture.

```text
Authority / Policy
        |
        v
State
        |
        v
Cognitive Routing
        |
   +----+----+
   |         |
Knowledge  Capabilities
   |         |
   +----+----+
        v
Context Compiler
        |
        v
Execution Context IR
        |
        v
Execution Runtime
```

## Key strategies

### Deterministic first

The initial system resolves workflows, agents, skills, dependencies and
policies without an LLM. Probabilistic services are optional enhancements and
cannot create authority or bypass validation.

### Hexagonal structure and ports

Ports & Adapters is the structural architecture strategy. Driving adapters such as CLI, API, IDE and CI enter through inbound application ports. The application and domain core owns validation, deterministic routing, policy evaluation and context compilation. It reaches external concerns only through outbound ports implemented by driven adapters.

The control-plane concepts map onto the hexagon as follows:

- **Authority** and **State** are validated domain inputs and constraints.
- **Cognitive Routing** is an application/core responsibility that determines the relevant workflow, primary agent, skills and knowledge queries.
- The **Knowledge Plane** is accessed through knowledge/retrieval ports; Git, filesystem, vector and graph RAG implementations are driven adapters.
- The **Capability Plane** is accessed through capability ports; MCP, repository, Git and quality-tool integrations are driven adapters subject to policy.
- The **Context Compiler** is an application/core service that produces the validated Execution Context IR.
- The **Execution Runtime** is reached through an execution-runtime port; Codex, PraisonAI, local models and cloud models are replaceable driven adapters.

The dependency rule is inward-only: adapters depend on application ports and domain abstractions, while the core never imports adapter technologies.

### Execution Context IR

Natural-language requests are transformed into the validated, provider-
independent `ExecutionContextIR v1` before execution. It carries typed task,
workflow, agent, skill, state, policy, knowledge, capability, constraint and
runtime identity values. Its field contract, invariants and JSON compatibility
rules are defined in [`../execution-context-ir.md`](../execution-context-ir.md)
and [`../ir-serialization.md`](../ir-serialization.md).

### Local cognitive services

A local SLM may provide semantic classification and relevance signals. These signals are validated by deterministic registries and policies before they influence an execution plan.

### Progressive retrieval

v0.1 starts with filesystem, Git and registry retrieval. Vector and graph retrieval are introduced only after the deterministic core is proven.

### Minimal context

The context compiler emits only required authority, workflow, skill and
retrieved-knowledge inputs. The IR remains structured and provider-independent;
rendering it into runtime-specific prompts or requests belongs outside the
domain contract.
