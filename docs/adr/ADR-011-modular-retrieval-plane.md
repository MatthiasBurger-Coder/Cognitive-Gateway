# ADR-011: Bounded advisory retrieval contracts

- Status: Accepted
- Scope: CG-15 / EPIC-02, issue #174

## Context

Lexical, vector, graph and memory retrieval need a shared protocol before
concrete adapters are built. Provider availability, tokenization and vector
storage must not change the deterministic core's meaning or grant authority.
The repository already owns provenance, evidence, quality and request scope.

## Decision

Place versioned retrieval requests, validated immutable plans and result batches,
finite budgets, explicit stop conditions, explanation records, embedding lineage
and token estimate contracts in `gateway-domain::retrieval_plane`. Reuse existing
CG-06/CG-07 quality, evidence and provenance contracts. Put replaceable planning,
retrieval, embedding and estimation ports in the application outbound boundary.

Normalize explicit collections without querying providers. Validate supported
source/strategy identities before execution. Report unavailable optional services
as degradation; never silently replace or remove plan selections. Reserve context
by semantic class and account with checked unsigned arithmetic. Require scoped
source snapshot and complete model identity for all derived vectors. Distinguish
exact, bounded estimated, unbounded estimated and unknown token counts.

## Consequences

Adapters can evolve without SDK dependencies in the core. Consumers retain the
existing KnowledgePort while adopting the richer protocol. Executors must enforce
pre-dispatch reservations, cancellation, monotonic usage and service availability;
contract constructors validate values and do not perform I/O. CG-20B owns later
selection/compaction. Retrieval data remains incapable of representing grants,
policy decisions, process transitions or catalog registration.

See [the field-level contract](../retrieval-plane.md) for canonical ordering,
validation, compatibility, context mapping and coverage commands.
