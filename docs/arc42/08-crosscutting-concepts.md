# 8. Cross-Cutting Concepts

## 8.1 Authority and provenance

Every context fragment should retain provenance. Governance and policy content must be traceable to canonical repository sources.

## 8.2 Explainability

Resolver and policy decisions must produce machine-readable reasons describing:

- matched rules;
- selected dependencies;
- rejected alternatives;
- policy allow/deny decisions;
- missing or invalid references.

## 8.3 Fail-closed behavior

Unknown definitions, malformed profiles and unauthorized capabilities are rejected by default.

## 8.4 Stable versus dynamic context

The compiler separates:

**Stable context**
- governance;
- policy;
- workflow definitions;
- role contracts;
- skill contracts.

**Dynamic context**
- current task;
- issue details;
- Git diff;
- runtime state;
- test results;
- current blockers.

This separation improves reproducibility and allows provider-side prompt caching where supported.

## 8.5 Capability classes

At minimum, capabilities are divided into:

- `inspect`: read-only or observational operations;
- `mutate`: operations that change repositories, infrastructure, remote systems or state.

Mutations can require additional policy checks and explicit user authorization.

## 8.6 Knowledge retrieval

Retrieval is advisory and evidence-producing. It must not directly create new authorities, permissions, agents, skills or workflows.

## 8.7 State model

Operating Mode, Execution Profile, workflow, gate, slice, blockers and authorization state are explicit machine-readable data rather than implicit prompt text.
