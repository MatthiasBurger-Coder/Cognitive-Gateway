# ADR-009 — Git Definition Authority and Runtime State Persistence

- Status: Accepted
- Date: 2026-08-23

## Context

Cognitive Gateway manages two fundamentally different categories of data:

1. **declarative system definitions** that describe what the system is allowed and configured to do, such as agents, skills, workflows, policies and project profiles;
2. **dynamic runtime state** that describes what the system is currently doing or has done, such as workflow instances, execution history, evidence, blockers, checkpoints and audit events.

A conventional database is attractive for fast lookup, but making it the primary authority for declarative definitions would weaken Git-based review, versioning, reproducibility, provenance and rollback. At the same time, storing mutable runtime state only in Git would be operationally inappropriate.

The architecture therefore requires an explicit authority boundary between canonical definitions, runtime persistence and derived indexes.

## Decision

Adopt the following storage authority model.

### 1. Git repository is the Source of Truth for declarative definitions

The canonical form of the following artifacts is stored as versioned repository files:

- Agent definitions;
- Skill definitions;
- Workflow definitions;
- Policy definitions;
- project/profile definitions;
- registry metadata and schema versions;
- architecture and governance configuration where applicable.

These definitions are loaded through deterministic registry loaders and validated before use.

A database, vector store, graph database, cache or RAG index must not become the sole authority for whether such a definition exists or what its canonical content is.

### 2. Runtime databases are authoritative for dynamic execution state

Mutable operational state is persisted through runtime persistence ports and suitable adapters/databases. This includes, where implemented:

- workflow/process instances;
- current process state;
- transition and execution history;
- blockers and resolutions;
- evidence records and references;
- checkpoints and resume state;
- audit events;
- runtime observations and operational metadata.

This information is not treated as static repository configuration because it represents evolving system activity.

### 3. Databases and indexes may provide rebuildable read models for definitions

Relational databases, key-value stores, graph databases, vector databases and in-memory caches may hold derived representations of repository definitions for efficient lookup, traversal, retrieval or ranking.

Such stores are considered **derived read models or caches** when they represent declarative definitions.

They must satisfy the following rule:

> If a derived definition store is deleted, its authoritative contents can be reconstructed from the canonical Git repository without semantic data loss.

Examples include:

- SQL registry read models;
- in-memory Agent/Skill registry indexes;
- Skill dependency graph indexes;
- vector embeddings for semantic retrieval;
- graph projections for relationship traversal;
- search indexes and caches.

### 4. Runtime state and definition authority must not be conflated

The same database technology may physically host multiple categories of data, but the architectural authority remains different:

```text
Git / repository
  │
  │ canonical, versioned definitions
  ▼
Schema Validation + Registry Loading
  │
  ├──► In-memory Registry
  ├──► SQL Read Model
  ├──► Graph Index
  └──► Vector / Search Index

Process / Execution Runtime
  │
  ▼
Runtime Persistence Port
  │
  ▼
Operational Database
  ├── workflow instances
  ├── execution history
  ├── evidence
  ├── blockers
  ├── checkpoints
  └── audit events
```

## Critical Rules

- Repository files remain canonical for declarative Agent, Skill, Workflow and Policy definitions.
- Runtime databases may not silently override canonical repository definitions.
- Derived definition stores must be rebuildable from Git.
- RAG/vector retrieval cannot grant registry membership, capabilities, permissions or workflow authority.
- Runtime state is persisted independently from the repository definition lifecycle.
- Persistence technologies remain adapters behind explicit ports and must not leak into the core domain model.
- A definition must be traceable to its repository version/provenance when used in an execution context.

## Consequences

### Positive

- declarative configuration remains reviewable through normal Git/PR workflows;
- every definition has version history, diff, provenance and rollback;
- registry state is reproducible from a known repository revision;
- database technologies can be replaced without changing definition authority;
- fast SQL/graph/vector lookup remains possible without sacrificing determinism;
- runtime persistence can be optimized independently from configuration management;
- audit trails can associate runtime activity with the exact definition revision that authorized it.

### Trade-offs

- synchronization/rebuild mechanisms are required for derived indexes;
- runtime records should retain definition IDs and revision/version metadata;
- startup or deployment flows must detect stale derived indexes;
- the system must clearly distinguish canonical data from materialized/read-model data in APIs and documentation.

## Relationship to Other ADRs

- **ADR-002** establishes repository documentation as the technical source of truth.
- **ADR-003** separates Git authority, RAG knowledge and MCP capabilities.
- **ADR-004** requires deterministic resolution before probabilistic retrieval.
- **ADR-008** requires persistence and repository technologies to remain replaceable adapters behind hexagonal boundaries.

This ADR specializes those decisions for registry definitions, runtime persistence and database/index usage.
