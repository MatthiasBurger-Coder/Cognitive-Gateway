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
for the CG-03.16 versioned Agent, Skill and machine-resolvable capability
document contracts, strict exclusions and representative normalized fixtures.

See [`reference-scenarios.md`](reference-scenarios.md) for the CG-02.06
executable acceptance scenarios covering the complete domain and IR contract.

See [`catalog-boundaries.md`](catalog-boundaries.md) for the Agent and Skill
catalog layout, ownership rules, loading APIs, deterministic capability index
and query behavior, and fail-closed semantics.

See [`project-context-boundary.md`](project-context-boundary.md) for the
request-scoped consuming-project configuration and retrieval provenance
contract.

See [`registry-inspection-cli.md`](registry-inspection-cli.md) for the CG-03
read-only registry and capability inspection commands, JSON output and exit
codes.

See [`tiny-swarm-world-process-inventory.md`](tiny-swarm-world-process-inventory.md)
and its [machine-readable inventory](tiny-swarm-world-process-inventory.json)
for the CG-04.14 TSW process-semantic classification and migration gaps.

See the materialized reusable Agent definitions under [`../catalog/agents/`](../catalog/agents/)
and the reusable Skill definitions under [`../catalog/skills/`](../catalog/skills/).
Specialist Agents are normal catalog entries, and all catalog Skill references
use canonical IDs from the same built-in catalog.

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
