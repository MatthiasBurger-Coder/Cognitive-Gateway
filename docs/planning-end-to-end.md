# CG-07 End-to-End Acceptance Proof (CG-07.10)

The public integration fixture
[`crates/gateway-application/tests/cg07_end_to_end.rs`](../crates/gateway-application/tests/cg07_end_to_end.rs)
proves the complete deterministic planning handoff:

```text
CG-06 DesiredState + normalized CurrentState/Situation
        -> comparison result
        -> Delta with typed outcome and evidence lineage
        -> abstract CG-03 capability requirements
        -> declarative PlanStep DAG
        -> validation
        -> explainability and canonical serialization
        -> CG-08-ready Plan
```

## Reference external-project scenario

The fixture represents an external project with these goals and observations:

| Desired condition | Current observation | Delta result | Declarative work |
| --- | --- | --- | --- |
| `architecture.dependency = false` | dependency exists | `UNSATISFIED_CONDITION` | change/remediation |
| `coverage.percent >= 95` | coverage is `92` | `UNSATISFIED_CONDITION` | change/improvement |

Both Delta items retain the Situation, CurrentState, fact, observation,
evidence and provenance identities. Explicit abstract capability contracts
produce two change steps. An explicit verification requirement produces a
second verification step for each change, with graph dependencies proving the
change-before-verification ordering. The serialized Plan and explanation
contain no Agent, Skill, ProcessDefinition or raw evidence content.

`OperatingMode::Hardening` and `ExecutionProfile::FullPath` remain explicit
CG-06/CG-02 input dimensions. CG-07 does not compile them into a context,
select a process or mutate a CG-04 process instance.

## Acceptance variants

The same public API is exercised for:

- all goals satisfied: a valid explicit no-op Plan;
- missing evidence: `MISSING_EVIDENCE` and evidence-acquisition steps;
- conflicted assertions: `CONFLICT` and conflict-resolution steps;
- unobserved state: `UNKNOWN_STATE` and observation steps;
- incompatible values: `UNSUPPORTED_COMPARISON` and an assessment step;
- stale evidence with fresh-evidence rules: `MISSING_EVIDENCE` with
  `STALE_EVIDENCE`, never a guessed violation;
- missing canonical capability contracts: a blocking diagnostic and no Plan.

The fixture also reverses the input record order and verifies semantic Delta,
Plan and canonical JSON equality. Every result is produced from explicit
snapshots and rules; no project-global state, clock, random source, LLM, RAG
or provider runtime is required.

## Boundary acceptance

The test uses the CG-03 `CapabilityIndex` only as an explicit snapshot and
projects its abstract `CapabilityDefinition` declarations into CG-07. Agent
and Skill candidates remain outside the planning result. The application
facade returns only comparison, Delta, abstract requirements, Plan,
validation, explainability and serialization artifacts. Concrete resolution
belongs to CG-08; lifecycle/process authority remains with CG-04; policy and
authorization remain with CG-09; context compilation remains with CG-10.

Run the reproducible acceptance gate with:

```text
cargo test --workspace
cargo llvm-cov -p gateway-application --all-targets --summary-only --fail-under-lines 95
cargo clippy --workspace --all-targets -- -D warnings
./scripts/check-architecture.sh
cargo fmt --all -- --check
git diff --check
```
