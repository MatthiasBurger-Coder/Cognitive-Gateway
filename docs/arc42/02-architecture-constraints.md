# 2. Architecture Constraints

## 2.1 Deterministic core

The gateway core must remain usable without an LLM, RAG backend or external network dependency for deterministic operations such as profile validation, registry lookup, workflow resolution, policy evaluation and context compilation.

## 2.2 Source of truth

Git-backed project files are the canonical source for governance, policies, workflow definitions, agent definitions, skill definitions and architecture documentation.

RAG may index these artifacts but never becomes their sole authority.

## 2.3 Knowledge/action separation

Knowledge retrieval and executable capabilities are separate concerns:

- Retrieval answers: **What must the runtime know?**
- MCP/tools answer: **What may the runtime do?**

Retrieval results must never grant permissions.

## 2.4 Fail closed

Unknown agents, skills, workflows, policies, capability references or invalid state combinations must fail closed unless an explicit fallback is defined.

## 2.5 Operating mode versus execution profile

Project phase and verification depth are independent dimensions.

Operating modes:

- `DEVELOPMENT`
- `HARDENING`
- `RELEASE_QUALIFICATION`

Execution profiles:

- `FAST_PATH`
- `NORMAL_PATH`
- `FULL_PATH`

## 2.6 Technology constraints

- Rust is the primary language for the deterministic core and daemon.
- Python is reserved for optional cognitive services such as SLM classification, embeddings and RAG.
- Kotlin may be used for a future dedicated IntelliJ integration only if needed.
- Execution runtimes such as Codex or PraisonAI must remain adapters, not core dependencies.

## 2.7 Documentation

The arc42 documentation and ADRs in this repository are authoritative technical documentation. GitHub Wiki content is an end-user view and must not become the only location for architecture decisions or governance rules.
