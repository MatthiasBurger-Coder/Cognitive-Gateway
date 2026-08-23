# Core Domain Model (CG-02.02)

The `gateway-domain` crate contains provider-independent, immutable values used
by deterministic profile loading and resolution. Definitions contain validated
values and typed IDs only. They do not contain prompts, model names, runtime
handles, transport details or executable behavior.

## Descriptors

| Type | Required values | Relationships |
| --- | --- | --- |
| `TaskDescriptor` | `TaskId`, non-empty intent | Describes the requested work |
| `AgentDefinition` | `AgentId`, non-empty description | One or more unique `SkillId` values |
| `SkillDefinition` | `SkillId`, non-empty description | Optional owning `AgentId`; zero or more unique dependency `SkillId` values; zero or more `CapabilityId` values and `KnowledgeQuery` values |
| `WorkflowDefinition` | `WorkflowId`, non-empty description | Exactly one primary `AgentId`, one or more unique `SkillId` values and exactly one `PolicyId` |
| `PolicyDefinition` | `PolicyId`, non-empty description | Unique allow/deny `CapabilityId` values |

`PolicyDefinition` permits an empty allow-list for an explicit deny-by-default
policy. A capability may not be both allowed and denied. Skill dependencies
cannot contain the owning skill ID. Relationship order is retained so a later
resolver can remain deterministic.

## Cross-object validation

`DefinitionCatalog::new` owns a value-semantic set of definitions and validates
the complete graph before returning it:

- every agent skill exists;
- every skill dependency exists;
- every explicit skill owner exists;
- every workflow primary agent exists;
- every workflow skill exists; and
- every workflow policy exists.

Duplicate IDs, missing targets, duplicate relationships, self-references,
circular skill dependencies and conflicting policy relationships return
`ValidationError`. The catalog and all
descriptors expose immutable slices, so callers cannot bypass these invariants
after construction.

## Public API conventions

Each descriptor has a fallible `new` constructor and a `try_new` alias for
parsing boundaries, plus getters for its validated values. Relationships accept
any `IntoIterator` of the appropriate typed ID, allowing profile loaders to
pass vectors or arrays without converting through untyped strings. The
`SkillDefinition::with_knowledge_queries` value builder adds retrieval hints;
it does not grant authority or capabilities.

The model deliberately uses `AgentId`, `SkillId`, `WorkflowId`, `PolicyId` and
`CapabilityId` rather than strings. This prevents accidental cross-kind
references at compile time and keeps adapter/provider identity outside the
domain boundary.

## Operating mode and execution profile

`OperatingMode` describes the project lifecycle: `DEVELOPMENT`, `HARDENING` or
`RELEASE_QUALIFICATION`. `ExecutionProfile` describes execution depth:
`FAST_PATH`, `NORMAL_PATH` or `FULL_PATH`. They are independent dimensions;
all nine pairs are valid in the base domain model. A project policy or an
explicit `Constraint` may impose a stricter rule for a particular profile.

Both enums expose canonical uppercase `as_str`/`Display` values and strict
`FromStr` parsing. Unknown, lowercase or malformed values return
`ValidationError::UnknownDomainValue`; values are never silently coerced.

## Execution state

`WorkflowState`, `GateState` and `BlockerState` are separate state machines:

| Type | Values | Responsibility |
| --- | --- | --- |
| `WorkflowState` | `PENDING`, `RUNNING`, `PAUSED`, `BLOCKED`, `COMPLETED`, `FAILED`, `CANCELLED` | Lifecycle of the complete workflow run |
| `GateState` | `PENDING`, `IN_PROGRESS`, `PASSED`, `FAILED`, `BLOCKED`, `SKIPPED` | Current quality/governance gate |
| `BlockerState` | `CLEAR`, `ACTIVE`, `RESOLVED` | Whether progress is prevented by a blocker |

Each state machine provides `can_transition_to` and `transition_to`. Terminal
workflow states cannot be transitioned out of. `ExecutionState::new` validates
the coordinated triple: a blocked workflow requires a blocked gate and an
active blocker; a completed workflow requires a passed or skipped gate; a
pending workflow has a pending gate and no active blocker; and an active
blocker cannot coexist with a running or terminal workflow.

## Capabilities and constraints

`CapabilityDefinition` pairs a typed `CapabilityId` with a `CapabilityClass`.
`INSPECT` capabilities are read-only observations; `MUTATE` capabilities may
change state and therefore require policy evaluation. `CapabilityClass` also
uses strict canonical parsing. Provider handles and tool metadata do not
belong in the domain type.

`Constraint` pairs a typed `ConstraintId` with a `ConstraintKind`. The current
semantic kinds are `FEATURE_FREEZE`,
`LIVE_MUTATION_REQUIRES_CONSENT`, and
`REQUIRE_FULL_PATH_FOR_RELEASE_QUALIFICATION`. The last kind rejects
`FAST_PATH` and `NORMAL_PATH` only when the operating mode is release
qualification; it does not make the two dimensions intrinsically dependent.

These types are declarations and validation rules only. They do not execute
capabilities, persist state, select a runtime, or embed Codex/PraisonAI/OpenAI
behavior. Serialization and `ExecutionContextIR` composition are subsequent
CG-02 slices.
