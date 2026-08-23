# Core Domain Contract (CG-02.01–CG-02.07)

This page is the canonical summary of the provider-independent domain contract
implemented by CG-02.01 through CG-02.06. The Rust implementation is in
[`crates/gateway-domain/src/`](../crates/gateway-domain/src/); the focused
field-level contracts are linked below.

## Boundary and responsibility

`gateway-domain` contains validated values, typed references and deterministic
invariants. It does not contain prompts, model configuration, runtime handles,
transport or persistence details, provider SDKs, executable capability calls,
or adapter behavior. It may use `serde` and `serde_json` for the versioned
wire contract; those libraries do not introduce an external provider or
integration boundary.

The domain model describes what may be selected and under which conditions.
Application services resolve and compose these values; ports and adapters load
profiles, retrieve knowledge, perform capabilities and execute runtimes. A
skill requirement or retrieval result is not permission by itself.

## Typed primitives

### Identifiers

The model uses distinct newtypes for each reference kind:

| Type | Identifies |
| --- | --- |
| `TaskId` | A task aggregate |
| `AgentId` | An agent definition |
| `SkillId` | A skill definition |
| `WorkflowId` | A workflow definition |
| `PolicyId` | A policy definition |
| `ExecutionContextId` | One execution context |
| `CapabilityId` | An abstract capability |
| `ConstraintId` | A named execution constraint |
| `ExecutionRuntimeId` (`RuntimeId`) | An opaque runtime identity |

Identifiers are validated without trimming or normalization. They are 1–128
characters, contain only ASCII letters, digits, `-`, `_` and `.`, and begin and
end with an ASCII alphanumeric character. The distinct Rust types prevent an
agent ID from being passed where a skill or policy ID is required, even when
their textual values happen to match.

`NonEmptyText` is used for required text. It must contain a non-whitespace
character, may be at most 16,384 characters, and may contain only tab, line
feed and carriage return as control characters. `KnowledgeQuery` is a
validated required-text value used for retrieval hints.

`SchemaVersion` is a major/minor value object. Major version `0` is reserved;
the only version accepted by `ExecutionContextIR` v1 is `1.0`. All primitive
parsers are strict: unknown, lowercase, malformed or otherwise non-canonical
values fail with `ValidationError`.

## Definitions and their boundaries

Repository-native versioned Agent and Skill documents are defined in
[`agent-skill-definition-contracts.md`](agent-skill-definition-contracts.md).
They are strict document envelopes around the `AgentDefinition` and
`SkillDefinition` values below: schema version and provenance are document
metadata, while supported semantic fields map through the existing typed
constructors. They do not add prompts, runtime/provider fields or workflow
lifecycle semantics to the core model.

These definitions are immutable value objects. Relationship order is retained
so deterministic resolution can preserve the declared order. Constructors
validate local rules; `DefinitionCatalog` validates the complete reference
graph.

| Object | Responsibility | Relationships and limits |
| --- | --- | --- |
| `TaskDescriptor` | Identifies the requested work and carries its intent. | Optional `TaskClassification` containing `task_type` and finite confidence `0..=1`; the two classification values are present together. |
| `AgentDefinition` | Declares a named responsibility contract. | Requires a non-empty, unique ordered list of `SkillId` values. It contains no prompt, model, runtime or behavior. |
| `SkillDefinition` | Declares a reusable knowledge and capability requirement. | Optional owning `AgentId`; unique `SkillId` dependencies; unique required `CapabilityId` values; ordered `KnowledgeQuery` values. A skill cannot depend on itself, and these requirements do not authorize execution. |
| `WorkflowDefinition` | Selects a deterministic unit of work. | Exactly one primary `AgentId`, one or more unique ordered `SkillId` values, and exactly one `PolicyId`. It references definitions rather than embedding them. |
| `PolicyDefinition` | Defines the authoritative capability decision input for a workflow. | Unique allowed and denied `CapabilityId` lists. The allow-list may be empty for deny-by-default; a capability cannot occur in both lists. |
| `CapabilityDefinition` | Classifies an abstract capability by safety impact. | A typed `CapabilityId` and `CapabilityClass`: `INSPECT` or `MUTATE`. It has no provider handle or tool implementation. |
| `Constraint` | Declares an execution-planning rule. | A typed `ConstraintId` and `ConstraintKind`; it is checked against the operating-mode/profile pair. |

`TaskClassification` is semantic metadata, not authority. `TaskConfidence`
stores the normalized finite confidence value. Neither may select a workflow
or grant a capability without deterministic validation.

### Definition catalog

`DefinitionCatalog` owns agents, skills, workflows and policies by value and
exposes immutable slices and typed lookups. `DefinitionCatalog::new` and
`validate` reject:

- duplicate definition IDs;
- agent skill, skill dependency or explicit skill-owner references that are missing;
- workflow primary-agent, skill or policy references that are missing; and
- circular skill dependencies.

Local constructors additionally reject duplicate relationships, skill
self-references, conflicting policy allow/deny entries, empty required
relationships and invalid text. A catalog does not register
`CapabilityDefinition` values: capabilities are typed references classified by
the capability boundary, while policy and context carry the authority
decisions for those references.

## Operating mode and execution profile

`OperatingMode` describes project lifecycle. `ExecutionProfile` describes
verification/execution depth. They are independent dimensions:

| `OperatingMode` | `ExecutionProfile` |
| --- | --- |
| `DEVELOPMENT` | `FAST_PATH` |
| `HARDENING` | `NORMAL_PATH` |
| `RELEASE_QUALIFICATION` | `FULL_PATH` |

The table is illustrative, not a one-to-one mapping: all nine combinations
are valid in the base domain model. A policy or explicit `Constraint` may
restrict a combination. Currently
`REQUIRE_FULL_PATH_FOR_RELEASE_QUALIFICATION` rejects `FAST_PATH` and
`NORMAL_PATH` only when the mode is `RELEASE_QUALIFICATION`; it does not make
the dimensions intrinsically coupled. Both enums use strict canonical
uppercase parsing and display.

## Execution state

The IR carries an `ExecutionState`, which is a validated triple of three
separate state machines:

| State machine | Values | Meaning |
| --- | --- | --- |
| `WorkflowState` | `PENDING`, `RUNNING`, `PAUSED`, `BLOCKED`, `COMPLETED`, `FAILED`, `CANCELLED` | Lifecycle of the complete workflow execution |
| `GateState` | `PENDING`, `IN_PROGRESS`, `PASSED`, `FAILED`, `BLOCKED`, `SKIPPED` | Current quality or governance gate |
| `BlockerState` | `CLEAR`, `ACTIVE`, `RESOLVED` | Whether a blocker currently prevents progress |

Each machine exposes `can_transition_to` and `transition_to`. Workflow
terminal states cannot transition further. `ExecutionState::new` then checks
the cross-field rules:

- `PENDING` requires a `PENDING` gate and `CLEAR` blocker;
- `RUNNING` and `PAUSED` require `PENDING` or `IN_PROGRESS` gate and no active blocker;
- `BLOCKED` requires a `BLOCKED` gate and an `ACTIVE` blocker;
- `COMPLETED` requires a `PASSED` or `SKIPPED` gate and no active blocker;
- `FAILED` requires a `FAILED` gate and no active blocker; and
- `CANCELLED` may use any gate state but cannot have an active blocker.

Unknown state values, illegal transitions and invalid combinations fail closed.

## Capabilities, policy and constraints

`CapabilityClass::INSPECT` describes read-only observation. `MUTATE` describes
an operation that may change state and therefore requires policy evaluation.
The class is descriptive; the domain does not execute either kind.

Policies are explicit authority inputs. An empty allow-list means no
capability is approved. A capability is approved only when it is allowed by
the selected policy and not denied by it. Retrieval never changes that result.

The supported constraint kinds are:

- `FEATURE_FREEZE`;
- `LIVE_MUTATION_REQUIRES_CONSENT`; and
- `REQUIRE_FULL_PATH_FOR_RELEASE_QUALIFICATION`.

Constraints are declarations, not enforcement calls. Their mode/profile
compatibility is validated when the context is constructed.

## ExecutionContextIR v1

`ExecutionContextIR` is the versioned, provider-independent contract between
deterministic resolution and an execution runtime. It is a value object: it
contains typed references and validated decisions, but no prompt, model name,
runtime handle, transport data or executable behavior.

The complete v1 field contract is documented in
[`execution-context-ir.md`](execution-context-ir.md). Its fields are:

```text
schema_version, id, task, workflow_id, primary_agent_id, skill_ids,
operating_mode, execution_profile, state, policy_id, knowledge_queries,
approved_capability_ids, constraints, target_runtime
```

Construction requires schema version `1.0`, a non-empty unique effective skill
list, unique approved capability IDs and unique constraint IDs. Knowledge
queries and approved capabilities may be empty, explicitly meaning “none”.
`validate_against(&DefinitionCatalog)` additionally proves that the workflow,
agent, policy and skills exist; the agent matches the workflow primary agent;
the effective skills are exactly within the workflow dependency closure; all
required skill capabilities are approved; and every approved capability is
allowed by policy.

The JSON wire and compatibility contract is documented in
[`ir-serialization.md`](ir-serialization.md). It is strict and deterministic:
fields and collections are explicit, canonical IDs/enums are strings, unknown
fields and malformed values are rejected, and only `"1.0"` is currently
supported. JSON syntax/type failures are distinct from domain validation
failures. There is no implicit upgrade, downgrade or best-effort compatibility
path.

## Documentation map

- [arc42 architecture](arc42/README.md) — system boundaries, building blocks and runtime flows;
- [ExecutionContextIR v1](execution-context-ir.md) — field semantics and catalog invariants;
- [JSON serialization](ir-serialization.md) — wire format, errors and compatibility;
- [reference scenarios](reference-scenarios.md) — executable acceptance evidence; and
- [ADR-008](adr/ADR-008-hexagonal-architecture.md) — ports, adapters and dependency direction.
