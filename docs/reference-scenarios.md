# Core Domain Reference Scenarios (CG-02.06)

The executable acceptance fixtures in
[`crates/gateway-domain/tests/reference_scenarios.rs`](../crates/gateway-domain/tests/reference_scenarios.rs)
prove the domain contract established by CG-02.01 through CG-02.05. They are
deliberately provider-neutral: the fixture uses `codex` only as an opaque
`ExecutionRuntimeId`; it does not depend on, configure or invoke Codex.

## EPIC #1 example

The first fixture is the required vertical-slice shape, equivalent to the
EPIC task “Implement issue #252”. It maps the example into the versioned IR as
follows:

| EPIC concept | Domain representation | Reference value |
| --- | --- | --- |
| runtime bug fix | `TaskDescriptor` classification | `runtime_bugfix`, confidence `0.94` |
| repair request | `TaskDescriptor` intent | `repair` |
| release workflow | `workflow_id` | `classic-rc1` |
| primary agent | `primary_agent_id` | `senior-devops` |
| selected skills | ordered `skill_ids` | engine, swarm, evidence |
| current execution | `ExecutionState` | `RUNNING / IN_PROGRESS / CLEAR` |
| hardening depth | mode/profile pair | `HARDENING / FULL_PATH` |
| retrieved knowledge | ordered `knowledge_queries` | three incident/runtime queries |
| allowed actions | `approved_capability_ids` | Docker inspection, Swarm inspection, runtime checks |
| governance | ordered `constraints` | feature freeze, mutation consent |
| execution kernel | `target_runtime` | opaque identity `codex` |

The policy is explicit because capability authority is a domain invariant even
though the abbreviated EPIC JSON did not show a policy field. The selected
skills include the full dependency closure: Swarm initialization depends on
engine installation. `validate_against` proves both facts and also proves that
retrieval queries do not grant capabilities.

## Scenario inventory

- `epic_reference_context_preserves_every_domain_decision` checks that all
  semantic fields survive construction and that the complete definition graph
  and authority decision validate.
- `all_operating_mode_and_execution_profile_pairs_are_valid` exercises all
  nine combinations. With no stricter constraint, the dimensions remain
  independent.
- `representative_state_scenarios_remain_valid_in_the_complete_ir` covers
  pending, running, paused, blocked, completed, failed and cancelled workflow
  states with their valid gate/blocker coordination.
- `capability_classes_and_constraints_keep_authority_separate_from_knowledge`
  demonstrates the inspect/mutate distinction and keeps capability approvals,
  constraints and retrieval requests in separate IR collections.
- `invalid_authority_and_dimension_combinations_fail_closed` covers missing
  approvals, policy denial and release qualification with an insufficient
  execution profile.
- `complete_context_round_trip_is_deterministic_and_provider_neutral` proves
  compact and pretty JSON round trips, stable compact output and the absence
  of provider prompt/model payloads.
- `malformed_reference_payloads_are_rejected_without_coercion` covers unknown
  mode values, unsupported versions, duplicate relationships and malformed
  identifiers.

## Acceptance evidence

Run the reference scenarios and the complete workspace quality baseline with:

```text
cargo test --workspace
cargo llvm-cov -p gateway-domain --all-targets --fail-under-lines 95
cargo clippy --workspace --all-targets -- -D warnings
```

The coverage command is the same measurable domain threshold used by CI. The
reference scenarios add acceptance coverage without introducing an adapter,
runtime handle or external service dependency.
