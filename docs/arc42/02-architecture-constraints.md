# 2. Architecture Constraints

## 2.1 Hexagonal Architecture is mandatory

The deterministic Rust core must use **Hexagonal Architecture (Ports & Adapters)** as a project-wide structural constraint. Domain and application abstractions are inside the hexagon; transports, infrastructure, knowledge stores, capability integrations and execution runtimes are outside it.

Dependencies must point inward toward stable domain and application abstractions:

```text
Driving Adapters -> Inbound/Application Ports -> Application + Domain/Core
Application + Domain/Core -> Outbound Ports -> Driven Adapters
```

The core must not depend on an adapter or on its technology. In particular, the core must not depend on OpenAI or another model provider, Codex, PraisonAI, concrete MCP implementations, vector or graph databases, GitHub APIs, filesystem/Git infrastructure details, or UI/transport frameworks. Such technologies may only appear in adapters that implement core-defined ports.

## 2.2 Deterministic core

The gateway core must remain usable without an LLM, RAG backend or external network dependency for deterministic operations such as catalog validation, registry lookup, workflow resolution, policy evaluation and context compilation.

## 2.3 Source of truth

Git-backed project files are the canonical source for governance, policies, workflow definitions, agent definitions, skill definitions and architecture documentation.

RAG may index these artifacts but never becomes their sole authority.

## 2.4 Knowledge/action separation

Knowledge retrieval and executable capabilities are separate concerns:

- Retrieval answers: **What must the runtime know?**
- MCP/tools answer: **What may the runtime do?**

Retrieval results must never grant permissions.

RAG is a knowledge adapter behind a retrieval/knowledge port. MCP and other tools are capability adapters behind capability ports. Neither adapter is an authority source, and neither may bypass deterministic policy evaluation.

## 2.5 Fail closed

Unknown agents, skills, workflows, policies, capability references, invalid state combinations and unavailable required ports must fail closed unless an explicit fallback is defined. A failed or untrusted adapter result must not be converted into authority, permission or an executable action.

## 2.6 Operating mode versus execution profile

Project phase and verification depth are independent dimensions.

Operating modes:

- `DEVELOPMENT`
- `HARDENING`
- `RELEASE_QUALIFICATION`

Execution profiles:

- `FAST_PATH`
- `NORMAL_PATH`
- `FULL_PATH`

## 2.7 Technology constraints

- Rust is the primary language for the deterministic core and daemon.
- Python is reserved for optional cognitive services such as SLM classification, embeddings and RAG.
- Kotlin may be used for a future dedicated IntelliJ integration only if needed.
- Execution runtimes such as Codex or PraisonAI must remain adapters, not core dependencies.

## 2.8 Documentation

The arc42 documentation and ADRs in this repository are authoritative technical documentation. GitHub Wiki content is an end-user view and must not become the only location for architecture decisions or governance rules.
