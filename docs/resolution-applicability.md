# CG-08.07: applicability without authorization

`resolution_applicability::evaluate_applicability` inspects one immutable
`ResolutionSnapshot`. Its `StepApplicability` retains the exact PlanStep (DAG,
completion, verification, requirements and lifecycle) and complete basis. Provider
completeness and readiness are separate: an empty selected-provider set can still
describe readiness, but is **not** a complete or executable binding.

All supplied source restrictions are conjunctive. Mode and profile are separate
typed conditions; all nine combinations are tested. Canonical selected capability
preconditions and constraints are re-read from CG-03, not trusted from proposals.
`read-only` intrinsically requires INSPECT and cannot be overridden. Other exact
contract strings require explicit versioned semantics; missing semantics remain
UNSUPPORTED and cannot yield eligibility. Extra capabilities from effective Skill
closure must be included by composition, along with its Agent/Skill restrictions.
This function does not independently construct a complete closure or select roles.

Dependent steps are DEFERRED until every immediate predecessor has explicit
completion evidence. `CompletionEvidence` is a caller-provided runtime attestation
from the caller's completion-evidence authority, **not an authenticated artifact**
or a claim manufactured by resolution. It must reference evidence, match the full
current basis, be satisfied and fresh, and attest exactly the predecessor's
completion and verification contracts. Unknown/stale/conflicted/incomplete or
cross-scope attestations do not count. The ingestion authority evaluates freshness
with explicit time/rules; this resolver reads no clock. After any basis change the
caller must revalidate and supply attestations for that basis. References alone
are not proof of authenticity; adapters must establish trust before calling.

Pending predecessors defer evaluation of future prerequisites. Otherwise an exact
attested predecessor contract or CG-06 DesiredCondition comparison must support
the prerequisite; an unproven typed outcome stays UNKNOWN. Independent branches
are checked independently and candidate discovery is not suppressed.

With an active process, only RUNNING plus passed active gates, no active blocker,
no waiting condition and the explicitly referenced currently available Activity
can be eligible. Its capabilities must cover the effective requested capabilities;
its constraints require explicit conjunctive semantics. Missing required process
evidence is UNKNOWN. No activity is inferred from a state name. CG-04 inspection
lists structurally available activities: eligibility here does **not** establish
guard truth, legal transition, policy approval, or permission to run. Final CG-04
and policy evaluation and current revision revalidation remain mandatory.

No state is mutated, gates cleared, process resumed, or execution initiated.

## Evidence

Five integration tests cover diamond/chain order, independent branches, exact and
invalid completion contracts, prerequisites, nine mode/profile pairs, missing
canonical conditions, invalid requests, all six lifecycle statuses, all gate
statuses, active/inactive blockers, waiting, activity/capability restrictions and
unchanged process/plan state. Existing snapshot tests cover stale revisions.

```powershell
cargo test -p gateway-application --test resolution_applicability
$env:CARGO_TARGET_DIR='D:\Projects\Cognitive-Gateway\target\cg08'
cargo llvm-cov -p gateway-application --all-targets --json --output-path target/cg08-coverage.json
```

Measured new production file: **213/215 lines, 99.07%**. Workspace tests, formatting,
Clippy with warnings denied and the architecture dependency guard also pass.
