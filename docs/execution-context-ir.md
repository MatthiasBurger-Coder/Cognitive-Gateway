# ExecutionContextIR v1 (CG-02.04)

`ExecutionContextIR` is the versioned, provider-independent contract between
deterministic gateway resolution and an execution runtime. It is a domain value
object: it owns validated values and typed references, but does not execute
anything and does not contain provider handles, prompts or model-specific
configuration.

## Schema

The v1 representation contains these mandatory fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `schema_version` | `SchemaVersion` | Must be `1.0` for `ExecutionContextIR` v1. |
| `id` | `ExecutionContextId` | Stable identity of this execution context. |
| `task` | `TaskDescriptor` | Task identity and intent; optional semantic classification is carried atomically as task type plus confidence. |
| `workflow_id` | `WorkflowId` | Selected workflow definition. |
| `primary_agent_id` | `AgentId` | Selected primary agent. |
| `skill_ids` | non-empty ordered `SkillId` list | Effective skills, including dependencies. |
| `operating_mode` | `OperatingMode` | `DEVELOPMENT`, `HARDENING` or `RELEASE_QUALIFICATION`. |
| `execution_profile` | `ExecutionProfile` | `FAST_PATH`, `NORMAL_PATH` or `FULL_PATH`. |
| `state` | `ExecutionState` | Validated workflow, gate and blocker state triple. |
| `policy_id` | `PolicyId` | Policy used to authorize the context. |
| `knowledge_queries` | ordered `KnowledgeQuery` list | Information requested from retrieval adapters. |
| `approved_capability_ids` | ordered `CapabilityId` list | Capabilities approved for this execution. |
| `constraints` | ordered `Constraint` list | Constraints applied to the mode/profile pair. |
| `target_runtime` | `ExecutionRuntimeId` | Opaque runtime identity; mapping to a concrete adapter is outside the domain. |

`TaskDescriptor::with_classification` can represent semantic signals such as
`runtime_bugfix` with confidence `0.94`. Classification is optional for callers
that only have deterministic task intent, but type and confidence are always
present together when it is supplied.

## Optionality and invariants

There are no nullable required execution fields. Collections are always present
and owned:

- `skill_ids` must be non-empty and contain unique IDs.
- `knowledge_queries` may be empty and then explicitly means that no retrieval
  is requested.
- `approved_capability_ids` may be empty and then explicitly means that no
  capability is approved (deny-by-default).
- `constraints` may be empty and then means that no additional constraint is
  applied. Constraint IDs must be unique.

Construction rejects every schema version other than `1.0`, duplicate
relationships, empty effective skills, and constraints that are incompatible
with the selected operating mode/profile. `ExecutionState` validates its own
workflow/gate/blocker invariants before it can enter the IR.

`validate_against(&DefinitionCatalog)` adds graph and authority checks:

- workflow, primary agent, policy and all effective skills must exist;
- the context agent must equal the workflow's primary agent;
- every workflow skill and its complete dependency closure must be present;
- no skill outside that closure may be selected;
- every capability required by a selected skill must be approved; and
- every approved capability must be allowed by the selected policy and not
  denied by it.

The catalog check deliberately does not infer capabilities from retrieval
results. Retrieval supplies knowledge; policy and the resolved context supply
authority.

## Public API

Use `ExecutionContextIR::new` when the version should be supplied explicitly,
or `ExecutionContextIR::new_v1` at a v1-only application boundary.
`try_new` is an equivalent parsing-boundary alias. Accessors expose immutable
slices and typed values. `validate` checks context-local invariants, while
`validate_against` performs the additional catalog/reference checks.

`ExecutionRuntimeId` (also available as `RuntimeId`) is intentionally only a
validated identity. Runtime adapters own all provider-specific configuration.
Serialization and compatibility handling are the responsibility of the next
schema/serialization slice; this issue defines the in-memory v1 contract.
