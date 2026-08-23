# ExecutionContextIR JSON Serialization (CG-02.05)

`gateway-domain` exposes a strict JSON representation for `ExecutionContextIR`.
The representation is provider-independent: it contains domain references,
state and authority decisions, but no prompts, model settings, transport data,
runtime handles or provider-specific payloads.

## Wire schema

The compact and pretty serializers emit the same fields. Object fields are
written in the following stable order:

```json
{
  "schema_version": "1.0",
  "id": "context-1",
  "task": {
    "id": "task-1",
    "intent": "repair",
    "classification": {
      "task_type": "runtime_bugfix",
      "confidence": 0.94
    }
  },
  "workflow_id": "issue-implementation",
  "primary_agent_id": "senior-developer",
  "skill_ids": ["quality-gate", "issue-workflow"],
  "operating_mode": "HARDENING",
  "execution_profile": "FULL_PATH",
  "state": {
    "workflow_state": "RUNNING",
    "gate_state": "IN_PROGRESS",
    "blocker_state": "CLEAR"
  },
  "policy_id": "safe-development",
  "knowledge_queries": ["quality gate history"],
  "approved_capability_ids": ["quality.run"],
  "constraints": [{"id": "feature-freeze", "kind": "FEATURE_FREEZE"}],
  "target_runtime": "runtime-default"
}
```

`classification` is either an object containing both `task_type` and
`confidence`, or `null`. The collection fields are always present. Empty
`knowledge_queries`, `approved_capability_ids` and `constraints` therefore
remain explicit rather than being confused with missing data. `skill_ids` must
remain non-empty.

IDs and enum/state values are encoded as their canonical strings. Confidence is
a finite JSON number in the inclusive range `0..=1` and is rounded to the
domain's four-decimal precision when it enters the model. No value is
case-folded, normalized or silently replaced with a default.

## Version and compatibility rules

`schema_version` is mandatory and uses the string form `MAJOR.MINOR`. The only
supported compatibility path is exactly `"1.0"`. `"1.1"`, `"2.0"`, malformed
strings, numeric versions and the reserved major version `0` are rejected.
There is currently no downgrade, upgrade or best-effort compatibility path;
adding one requires an explicit schema/version change and its own contract
tests.

Unknown enum/state strings, unknown object fields, duplicate relationship
entries, malformed identifiers, invalid confidence values and invalid state or
constraint combinations are rejected. JSON syntax/type errors are reported as
`SerializationError::Json`; valid JSON that violates a domain rule is reported
as `SerializationError::Validation` by `ExecutionContextIR::from_json`.

## Public API

```rust
use gateway_domain::ExecutionContextIR;

let compact_json = context.to_json()?;
let readable_json = context.to_json_pretty()?;
let restored = ExecutionContextIR::from_json(&compact_json)?;
assert_eq!(restored, context);
```

`ExecutionContextIR`, its task, constraints, execution state, identifiers,
schema version and enum values also implement `serde::Serialize` and
`serde::Deserialize`. Direct serde deserialization uses the same strict
constructors and rejects invalid values. The convenience methods are the
recommended boundary API when callers need the typed
`SerializationError` distinction.

Serialization is deterministic for a given value: collection order is
preserved and the serializer uses a fixed struct field order. A compact JSON
round trip therefore produces the same compact JSON and no semantic data is
lost.
