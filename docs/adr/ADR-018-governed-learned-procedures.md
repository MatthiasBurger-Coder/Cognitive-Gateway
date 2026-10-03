# ADR-018 — Governed, Immutable Learned Procedures

- **Status:** Accepted
- **Date:** 2026-10-03

## Context

Cognitive Gateway should reuse validated successful experience without turning memory, model output or historical success into uncontrolled executable authority.

## Decision

Represent procedural learning as explicit versioned domain artifacts:

- eligible governed experience;
- situation fingerprints and pattern candidates;
- immutable learned procedure content with canonical digest;
- steps that reference existing process, capability and policy identities;
- explicit observation/evidence/verification requirements;
- explicit fallback behavior;
- append-only lifecycle transitions.

A learned procedure never creates new process or policy authority.

## Consequences

- Procedure versions are reproducible and tamper-evident through canonical serialization/digest binding.
- Promotion/suspension/retirement decisions remain explicit and auditable.
- Memory is evidence for learning, not permission.
- CG-21 contracts, CG-23 evaluation and [CG-24 promotion](../procedure-promotion.md) are implemented. CG-24 uses authenticated application authority and an append-only registry with bounded canary, explicit supersession and rollback; automatic discovery/reflex execution remains incremental.
