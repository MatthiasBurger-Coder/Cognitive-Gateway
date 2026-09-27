# ADR-014: Versioned evaluation and reference-only learning export

- Status: accepted for CG-20 implementation
- Date: 2026-09-27
- Scope: EPIC-02 #113, CG-20 #201, CG-20D #202

## Decision

Deterministic objective evaluators and explicit integer release thresholds live
in `gateway-domain`; versioned synthetic datasets and baseline values remain
outer fixtures. `gateway-application` assembles scoped, reference-only curated
snapshots from CG-18 memory and requires current eligibility revalidation at
consumption. Quality profiling is diagnostic and never changes a memory
record or its authority. The existing Python quality runner retains reports,
coverage and command logs. Optional model-assisted scoring is reported
separately and cannot authorize release.

## Consequences

Missing metric denominators, threshold regressions, stale memory references,
wrong scope and modified exports fail explicitly. Snapshot digests detect
accidental changes but are not signatures. Rollback must preserve memory
revocations and must not silently restore an older baseline or forgotten
payload. Provider adapters remain optional and outside the deterministic core.
