# Cognitive Gateway Documentation

This repository contains the canonical technical documentation for Cognitive Gateway.

## Architecture

See [`arc42/`](arc42/) for the living architecture documentation.

## Architecture Decisions

See [`adr/`](adr/) for accepted architecture decisions.

## Domain model

See [`domain-model.md`](domain-model.md) for the descriptor and relationship
contract introduced by CG-02.02, including the CG-02.03 execution state,
capability and constraint model.

See [`execution-context-ir.md`](execution-context-ir.md) for the complete
CG-02.04 `ExecutionContextIR` v1 contract, field semantics and invariants.

## Documentation Policy

- Repository documentation is the technical source of truth.
- GitHub Wiki is intended for simplified end-user documentation, tutorials and operational guidance.
- Architectural or governance changes must update repository documentation in the same development flow as the corresponding code/configuration change.
- Wiki pages should link back to canonical repository documentation where appropriate.

## Current documented decisions

1. Rust core + Python cognitive services.
2. Repository docs as technical authority; Wiki as end-user view.
3. Git authority, RAG knowledge retrieval, MCP/tool capabilities.
4. Deterministic workflow/agent/skill resolution before probabilistic retrieval.
5. Execution-runtime independence: Codex, PraisonAI and other runtimes are adapters.
6. Operating Mode and Execution Profile are independent dimensions.
7. Versioned Execution Context IR as the core runtime integration contract.
8. Hexagonal Architecture with inward dependencies and replaceable ports/adapters.
