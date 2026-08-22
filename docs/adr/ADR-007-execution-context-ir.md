# ADR-007 — Execution Context IR as the Integration Contract

- Status: Accepted
- Date: 2026-08-22

## Context

Natural-language prompts are unstable integration contracts. Cognitive Gateway needs a model-independent representation between classification/resolution and runtime execution.

## Decision

Introduce a versioned `ExecutionContextIR` as the central integration contract.

It carries at least:

- task description/type and semantic signals;
- operating mode and execution profile;
- workflow/gate/blocker state;
- selected primary agent;
- effective skills;
- knowledge queries/references;
- approved capabilities;
- constraints and policy outcomes;
- target execution runtime;
- provenance/explainability metadata where applicable.

The IR is serialized using versioned schemas and protected by contract tests.

## Consequences

- execution runtimes receive a stable, validated contract;
- runtime adapters are decoupled from natural-language parsing;
- deterministic regression testing becomes practical;
- IR evolution requires explicit versioning and compatibility decisions.
