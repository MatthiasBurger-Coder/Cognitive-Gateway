# Cognitive Gateway

Cognitive Gateway is a local, model-independent **AI Context & Agent Control Plane** between clients such as IDEs, CLI tools and CI/CD systems and one or more execution runtimes such as Codex, PraisonAI, local LLMs or cloud models.

The gateway is not another agent framework and not merely a RAG system. Its responsibility is to determine which workflows, roles, skills, policies, knowledge and capabilities are relevant and allowed for a task, then compile a minimal execution context for the selected runtime.

> **Authority defines the boundaries. State describes the situation. Cognitive routing determines what is needed. Retrieval supplies knowledge. MCP/tools supply capabilities. The execution runtime acts inside those boundaries.**

## Documentation

The technical architecture is maintained in the repository as the canonical source of truth:

- [`docs/arc42/`](docs/arc42/) — arc42 architecture documentation
- [`docs/adr/`](docs/adr/) — Architecture Decision Records

The GitHub Wiki is intended for simplified end-user documentation, tutorials and usage guidance. If Wiki content and repository architecture documentation ever conflict, the repository documentation is authoritative.

## Current v0.1 Direction

- Rust for the deterministic gateway core and long-running daemon
- Python for optional cognitive services such as local SLMs, embeddings and RAG
- Kotlin only if a dedicated IntelliJ integration becomes necessary
- deterministic workflow/agent/skill resolution before probabilistic retrieval
- Git as source of truth
- RAG as knowledge retrieval, not authority
- MCP/tool adapters as controlled capabilities
- execution runtimes remain replaceable

See EPIC #1 and the CG-01…CG-10 issues for the v0.1 implementation plan.
