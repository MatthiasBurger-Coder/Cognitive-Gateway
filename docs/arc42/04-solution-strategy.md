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

The initial system resolves workflow, role, skills, dependencies and policies without an LLM. Probabilistic services are optional enhancements.

### Ports and adapters

The core depends on abstractions rather than Codex, PraisonAI, Ollama, GitHub or a specific vector database.

### Execution Context IR

Natural-language requests are transformed into a validated intermediate representation before execution. This provides a stable integration boundary between routing and runtime execution.

### Local cognitive services

A local SLM may provide semantic classification and relevance signals. These signals are validated by deterministic registries and policies before they influence an execution plan.

### Progressive retrieval

v0.1 starts with filesystem, Git and registry retrieval. Vector and graph retrieval are introduced only after the deterministic core is proven.

### Minimal context

The compiler emits only required authority, workflow, skill and retrieved knowledge. Stable and dynamic content are separated to improve caching and reproducibility.
