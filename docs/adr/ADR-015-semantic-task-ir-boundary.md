# ADR-015 — SemanticTaskIR as the Natural-Language Semantic Boundary

- **Status:** Accepted
- **Date:** 2026-10-03

## Context

Cognitive Gateway needs to accept human language without allowing ambiguity or model-specific prompt behavior to leak into deterministic planning and execution contracts.

## Decision

Introduce CGSL and a versioned `SemanticTaskIR` as the planned canonical semantic boundary between natural-language interpretation and existing deterministic planning/resolution.

Natural-language interpretation may use probabilistic helpers, but mandatory unresolved, ambiguous or conflicting semantics must remain explicit and cannot be silently guessed.

CGSL answers **what the task means**. Process IR answers **how controlled execution proceeds**. Provider-specific prompt/runtime rendering remains outside both.

## Consequences

- Existing CG-06/07/08 contracts are reused rather than duplicated.
- Models may propose semantic candidates but cannot define authoritative semantics.
- The semantic compiler must remain provider/network independent at its deterministic core.
- Structured callers can continue to bypass natural-language interpretation and submit existing typed contracts directly.
- EPIC-05 #177 owns the implementation and conformance work.
