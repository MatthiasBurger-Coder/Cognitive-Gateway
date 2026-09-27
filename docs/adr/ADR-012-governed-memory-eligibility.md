# ADR-012: Governed memory eligibility and revocation

Status: Accepted for the CG-18 development contract.

## Context

Validated experience can become stale or be withdrawn after a context or dataset reference has been created. A stored result cannot be treated as perpetual evidence or authority.

## Decision

Memory is derived, scoped data. Domain records preserve source snapshot identity, digest, explicit times, quality, validation, outcome and label basis. Application operations append a reasoned curation decision and atomically update a revisioned projection through `MemoryStore`. Current eligibility is recalculated at the consumer's explicit time. Learning references pin schema, scope, revision, eligibility version and source snapshot; consumers revalidate against the current store before use. Forgetting purges payload and retains a tombstone with revocation history. CG-10 memory fragments remain derived assessment, including reference-only representation for sensitive payloads.

## Consequences

An export can become ineligible without altering its historical manifest. A deployment store must atomically persist both the projection and decision and must never restore forgotten payloads during rollback. The process-local proof adapter is not suitable for durable operation. Query ranking, vector indexing, export construction and model training remain outside this decision.
