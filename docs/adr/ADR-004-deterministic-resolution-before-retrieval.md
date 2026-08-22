# ADR-004 — Deterministic Resolution Before Probabilistic Retrieval

- Status: Accepted
- Date: 2026-08-22

## Context

Vector similarity and LLM classification are useful for semantic relevance but are unsuitable as sole authorities for workflow, skill or permission selection.

## Decision

Use a deterministic registry/resolver as the authoritative mechanism for selecting valid workflows, agents, skills, dependencies and capabilities.

A local SLM or retrieval system may provide semantic signals and candidate references, but only registered, policy-valid definitions may enter a validated execution plan.

## Principle

> **SLM understands. Resolver decides. Compiler builds. Kernel acts.**

And:

> **Resolver determines capabilities. Retrieval determines information.**

## Consequences

- repeatable behavior for identical input/state;
- improved auditability;
- fail-closed handling of unknown identifiers;
- retrieval and models can be replaced without changing governance semantics.
