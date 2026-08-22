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
