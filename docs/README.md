# Cognitive Gateway Documentation

This repository contains the canonical technical documentation for Cognitive Gateway.

## Architecture

See [`arc42/`](arc42/) for the living architecture documentation.

## Architecture Decisions

See [`adr/`](adr/) for accepted architecture decisions.

## Core domain contract

See [`domain-model.md`](domain-model.md) for the consolidated CG-02 domain
contract: typed primitives, definitions and relationships, execution state,
capabilities, constraints and the provider-independent architecture boundary.

See [`execution-context-ir.md`](execution-context-ir.md) for the field-level
`ExecutionContextIR` v1 contract and invariants.

See [`ir-serialization.md`](ir-serialization.md) for the JSON wire schema,
validation behavior, public serialization API and version compatibility rules.

See [`agent-skill-definition-contracts.md`](agent-skill-definition-contracts.md)
for the CG-03.03 versioned Agent and Skill document contracts, provenance
model, strict exclusions and representative normalized fixtures.

See [`reference-scenarios.md`](reference-scenarios.md) for the CG-02.06
executable acceptance scenarios covering the complete domain and IR contract.

See [`catalog-profile-boundaries.md`](catalog-profile-boundaries.md) for the
generic catalog and project profile layout, ownership rules, loading APIs and
fail-closed conflict semantics.

See the materialized reusable Agent definitions under [`../catalog/agents/`](../catalog/agents/)
for CG-03.07 and the reusable Skill definitions under [`../catalog/skills/`](../catalog/skills/)
for CG-03.08. Example project-specific Agent and Skill definitions are under
[`../profiles/example-project/`](../profiles/example-project/).

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
9. Git is authoritative for declarative Agent/Skill/Workflow/Policy definitions; runtime databases own mutable execution state, while SQL/graph/vector stores used for definition lookup are derived and rebuildable read models.
