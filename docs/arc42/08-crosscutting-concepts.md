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

The compiler separates stable governance/policy/workflow/role/skill contracts from dynamic task, issue, Git, runtime, test and blocker state. This improves reproducibility and enables provider-side prompt caching where supported.

## 8.5 Capability classes

At minimum, capabilities are divided into `inspect` and `mutate`. Mutations can require additional policy checks and explicit user authorization.

## 8.6 Knowledge retrieval

Retrieval is advisory and evidence-producing. It must not directly create new authorities, permissions, agents, skills or workflows.

## 8.7 State model

Operating Mode, Execution Profile, workflow, gate, slice, blockers and authorization state are explicit machine-readable data rather than implicit prompt text.

The domain currently models `WorkflowState`, `GateState` and `BlockerState` as
strict, independently parseable state machines. `ExecutionState` validates
their coordinated invariants: blocked execution requires an active blocker and
a blocked gate; completion requires a passed or skipped gate; and active
blockers cannot coexist with running or terminal workflows. Unknown values and
illegal transitions fail closed. `OperatingMode` and `ExecutionProfile` stay
orthogonal, with project-specific restrictions represented by explicit
constraints rather than hard-coded coupling.

Capabilities are classified as `INSPECT` or `MUTATE`. Constraints are typed
domain declarations, including feature freeze, consent for live mutation and
an optional full-path requirement for release qualification. Neither type
contains provider handles or executes an adapter operation.

## 8.8 Dependency inversion and adapter replaceability

The deterministic Rust core follows Hexagonal Architecture. Domain and application contracts are stable inner abstractions; infrastructure and execution technologies are replaceable outer adapters.

The dependency rule is inward-only:

```text
Adapters -> Application Ports -> Domain/Core
```

RAG implementations depend on knowledge/retrieval ports, MCP/tool implementations depend on capability ports, and execution runtimes depend on runtime ports. None of these technologies may become a dependency of `gateway-domain`.

This enables isolated unit testing through port doubles, architecture verification from Cargo manifests, and replacement of infrastructure without rewriting core behavior.

Ports are owned by the inner application/domain model and describe stable contracts rather than vendor protocols. Driving adapters translate external requests into inbound ports; driven adapters implement outbound ports. This keeps transport, storage, model-provider, MCP and runtime concerns replaceable.

## 8.9 Auditability and separation of concerns

Each routing, policy and adapter interaction should be explainable through structured evidence: the selected or rejected definitions, the policy decision, the port used, adapter provenance and the resulting state. Evidence is observational; it cannot grant authority.

Authority, knowledge and capability remain separate planes:

- authority comes from canonical governance and policy sources;
- knowledge adapters retrieve information and provenance;
- capability adapters perform only operations authorized by the core.

RAG output therefore remains advisory, MCP/tool output remains capability-scoped, and execution runtimes remain consumers of the compiled context rather than policy authorities.

## 8.10 Architecture guard

The repository provides `scripts/check-architecture.sh` as an initial executable guard against obvious dependency inversions. CI runs this guard before the Rust quality gates. More exhaustive architecture tests may be added later without changing the dependency model.

## 8.11 Domain primitive validation

The domain crate uses distinct validated newtypes for task, agent, skill, workflow, policy, execution-context and capability identifiers. Identifiers are 1–128 characters, use only ASCII letters, digits, `-`, `_` and `.`, and must begin and end with an ASCII alphanumeric character. Identifiers are never silently trimmed or normalized; malformed values are rejected by their constructors and parsing implementations.

Required textual values must contain a non-whitespace character, be no longer than 16,384 characters, and contain no control characters other than tab, line feed and carriage return. `SchemaVersion` is a major/minor value object; major version zero is reserved and malformed version strings fail parsing. These rules are deterministic, side-effect free and independent of serialization frameworks or execution providers.
