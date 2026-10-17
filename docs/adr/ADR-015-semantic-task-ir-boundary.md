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

## Implementation status — 2026-10-17

The [implementer guide](../cgsl-implementer-guide.md) records the delivered
CGSL 1.0 parser/compiler/serializer, typed contracts, resolution/completeness,
CG-06/07/08 adapter and component conformance. The Process boundary and two offline
prompt renderings preserve the decision's separation. [Canonical context planning and its CG-10 handoff](../context-requirement-planning.md)
and [bounded examples](../dynamic-example-selection.md) now have producer proof.
Production invocation/clarification hosting remain downstream;
this update does not change the original authority or ownership decision.

Parent acceptance follow-up: the bounded `artifact-inspection/1.0` natural-language
profile now compiles original text through existing resolver/completeness ports.
The independent valid-source witness compares Rust/Python canonical IR and digest
outputs. Both are documented in the implementer/conformance guides; no new
authority, task wire version or provider/runtime ownership is introduced.
