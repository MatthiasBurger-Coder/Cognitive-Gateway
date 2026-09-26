# CG-09 Policy Engine

The Policy Engine evaluates authorization before a resolved plan step executes.
It is deterministic, provider independent and fail closed. The Process Engine
continues to own lifecycle transitions, process revisions, evidence gates and
blockers.

## Boundaries and authority

`gateway-policy::PolicyEngine` is a pure function over `PolicyAuthority` and
`StepPolicyInput`. It performs no I/O, retrieval, model calls, clock reads or
process mutations. `gateway-application::policy_application::PolicyApplication`
connects it to the CG-08 resolver and the CG-04 process authorization boundary.

The caller loads `PolicyAuthority` from authoritative governance. It contains:

- Applicable `PolicyDefinition` allow/deny lists. Every applicable policy must
  allow a capability; any explicit deny wins. An empty policy set denies every
  requested capability.
- Canonical capability contracts, including the Inspect/Mutate classification.
- Named execution constraints and required evidence per capability.

The caller separately authenticates `PolicyContext` and its `StepFacts` for the
current principal and request. Their basis pins the plan, consuming scope,
registry, process catalog, situation and process state. These records are
adapter inputs, not authentication mechanisms. Constructing a Rust struct or
knowing a fingerprint does not establish authority.

**Never populate authority, authorizations, consent, evidence attestations or
work classification from planner, resolver, retrieval or model output.** Such
output can suggest work. It cannot grant permission. The same applies to a
DesiredState: its conditions express goals, not permission to realize them.
There is no textual goal override in the policy API.

Reports are audit evidence, not transferable execution tokens. Do not cache an
Allow across policy changes, revoked authorization/consent, changed evidence or
process revisions. Reevaluate immediately before use with current trusted inputs.

## Evaluation

The application replays validation of the resolution report. It rejects altered
artifacts, mismatched context bases, mismatched operating modes/profiles and facts
for unknown steps. It authorizes only a unique, complete whole-plan binding.
Missing, ambiguous, partial and search-limited resolution cannot authorize work.
The application includes capabilities required by the complete Skill closure.
It never merges mutually exclusive alternatives into an approved capability set.

For each step, the core evaluates all restrictions and emits ordered findings:

| Check | Missing or conflicting input |
| --- | --- |
| Canonical capability | Unknown contract or changed class/metadata: Deny |
| Applicable policy lists | Explicit deny or absent allow: Deny |
| Capability authorization | Missing: RequireConsent; denied: Deny |
| Mutate consent | Missing: RequireConsent; denied: Deny |
| Required evidence and preconditions | Missing exact evidence key: RequireEvidence |
| Capability, requirement and process constraints | Missing trusted attestation: RequireEvidence |
| Process readiness | Blocked: Deny; unknown/deferred: RequireEvidence |
| Step prerequisites/dependencies | Missing trusted completion attestation: RequireEvidence |
| Feature freeze | Feature work: Deny; unknown work class: RequireEvidence |
| Release depth constraint | ReleaseQualification without FullPath: Deny |

Inspect does not imply permission: it still needs policy allowlisting and
explicit authorization. Mutate additionally requires explicit consent. This is
a conservative default even without `LiveMutationRequiresConsent`; adding that
constraint cannot weaken it. FullPath never grants authorization or consent.
`read-only` on a Mutate contract is a hard contradiction and produces Deny,
even if a caller supplies a constraint attestation.

The decision precedence is:

`Deny > RequireConsent > RequireEvidence > Allow`

All findings remain in the report, including evidence still needed when consent
is also missing. Findings are sorted and deduplicated. Policy input order does
not affect the report. A genuine NoOp produces no capability approvals.

### Evidence and constraint keys

Capability/requirement preconditions and configured evidence use their exact
string keys. Arbitrary text is not interpreted as a rule. Unknown restriction
keys remain blocked until a trusted adapter evaluates them and supplies the
corresponding attestation in `satisfied_constraints`.

Process ActivityConstraint keys use JSON string tuples, for example
`["primary-agent","alpha"]`. This preserves both name and value without delimiter
collisions. Declarative DesiredState constraints use `desired:<constraint-id>`;
the attestation must validate that constraint's expression on the pinned basis.
These attestations only satisfy restrictions. They cannot override deny rules,
canonical classification or missing authorization.

## Application API

1. Resolve the plan through `DeclarativeResolutionApplication::resolve_plan`.
2. Load current `PolicyAuthority` independently of that result.
3. Capture an authenticated `PolicyContext` with the same basis, operating mode
   and execution profile. Supply step-scoped authorization, consent, evidence,
   constraint and prerequisite attestations.
4. Call `PolicyApplication.evaluate(&resolved, &authority, &context)`.
5. Inspect each entry in `PlanPolicyReport::steps()`. Only `Allow` may execute.
   `decision()` returns the strictest result for the whole plan.
6. Record `PlanPolicyReport::to_json()` as audit evidence.

`StepPolicyReport` includes schema version 1, step identity, decision, applicable
policy IDs, authoritative capability classes and machine-readable findings.
Each finding contains a decision, reason code and subject (policy, capability,
evidence, constraint or step reference). `PlanPolicyReport` also serializes the
resolution basis. Neither report is deserializable as a permission token.

The legacy capability-only `PolicyEvaluator` trait remains available for existing
adapters. It cannot represent full CG-09 authorization; use the new engine for
execution decisions. `PolicyDecision` now also includes `RequireEvidence`.

## Process gates

Declare a Process Engine guard `PolicyDecisionIs { policy, status: Allow }` for
the transition that requires a policy decision. Use
`report.gate_inputs(&current_basis, &step, gate_id, inputs)` to add that step's
result to `EvaluationInputs`, then pass the inputs to `TransitionEvaluator` or
the existing Process Application. The adapter owns the step-to-gate mapping.

| Policy result | Process policy status |
| --- | --- |
| Allow | Allow |
| Deny | Deny |
| RequireConsent / RequireEvidence | Waiting |

Missing gates remain fail closed under the existing process contract. The bridge
rejects stale bases and unknown steps. It preserves a Deny or Waiting already
present when merging a less restrictive decision. It does not satisfy evidence
gates, clear blockers, resume paused processes or apply a transition. Existing
process revision checks still apply. Evaluate again after any relevant change.

## Verification

The regression suites are:

- `crates/gateway-policy/tests/policy_engine.rs`: decision precedence, inspect vs
  mutate, forged contracts, exact evidence, governance and mode/profile matrix.
- `crates/gateway-application/tests/policy_application.rs`: deterministic plan
  evaluation, artifact validation, scope/revision checks, Skill dependencies,
  process constraints, paused processes, NoOp and actual process transitions.

Run:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
bash scripts/check-architecture.sh
cargo llvm-cov -p gateway-policy -p gateway-application \
  --test policy_engine --test policy_application \
  --json --output-path target/cg09-coverage.json
python3 scripts/check-policy-coverage.py --self-test
python3 scripts/check-policy-coverage.py target/cg09-coverage.json
```

On a workspace shared between Windows and Linux, set
`CARGO_LLVM_COV_TARGET_DIR` to a fresh platform-specific directory to avoid mixing
object formats. CI enforces at least 95% line coverage separately for the policy
core and application module. Missing, duplicated or invalid measurements fail
the gate, as do percentages below 95% before rounding.

### Implementation evidence (2026-09-26)

The CG-09 suites pass all 17 tests. Measured production line coverage:

| File | Covered lines | Coverage |
| --- | --- | --- |
| `gateway-policy/src/lib.rs` | 191 / 191 | 100.00% |
| `gateway-application/src/policy_application.rs` | 144 / 145 | 99.31% |

Workspace tests, workspace Clippy with warnings denied, formatting and the
architecture guard passed. The commands above reproduce the checks; CI measures
coverage again rather than trusting these recorded percentages.
