# Closed-loop execution and replanning (CG-14)

`gateway_application::closed_loop::ClosedLoop` closes the declarative control
loop through the existing CG-06 assessment, CG-07 planning, CG-08 resolution,
CG-09 policy and CG-10 compilation boundaries. Execution adapters implement
`ExecutionRuntimePort`. The application owns deterministic decisions and audit
history; adapters own runtime invocation and external report acquisition.

## Application sequence

1. Call `ClosedLoop::start(run_id, scope, intent, observations, rules)`.
   Supply a unique run ID, a scoped complete observation snapshot, immutable
   capability/planning rules, an iteration limit and a retry limit. A run ID
   namespaces assessment and execution identities; adapters must not reuse it
   for another run. Zero iterations permits assessment but no execution.
2. Read `decision()` and `assessment()`. Use the returned document, effective
   DesiredState, Delta and Plan as the next `ResolutionSnapshotInput`. Capture
   current catalog, process revision and authenticated policy inputs through
   their existing boundaries. Supply an explicit workflow/state projection.
3. Call `execute(CompileStepInput, runtime)` for the first remaining step in
   deterministic topological order. The application checks the exact scope,
   DesiredState, document, Delta, Plan and selected step. CG-10 revalidates the
   resolution and evaluates CG-09 policy before the runtime is invoked.
4. The runtime returns an `ExecutionOutcome` with the supplied execution ID,
   a typed status and an optional `ScopedObservationBatch`. The application
   reassesses that snapshot, recomputes the Delta, and selects its next decision.
5. Repeat step 2 for Continue or Replan. On Pause, obtain the missing input and
   call `refresh` with a complete current snapshot. Success and Stopped are
   terminal. `stop()` records an explicit terminal blocker.

When CG-19 retrieval supplies a sufficiency assessment, the host may call
`apply_retrieval_assessment`. Missing evidence pauses the run; sufficient
retrieval leaves its current decision unchanged. A paused run still requires
fresh CG-06 observations through `refresh` and fresh policy/process inputs.

This is an event-driven Rust API. Calls execute one step each; the host decides
when to capture inputs and schedule another call. The existing CLI remains a
read-only assessment/planning/compilation interface. No concrete runtime,
provider SDK or project-specific configuration is required by the core.

## Evidence and goals

Outcome status is a runtime claim. `Completed` alone cannot satisfy a goal.
Every supplied batch is validated and normalized through CG-06 with supporting
evidence required. Comparison requires explicit fresh quality metadata;
missing evidence, unknown freshness, uncertainty and conflicts remain gaps.
Adapters must acquire reports and authenticate their provenance independently
of model output, and assess freshness at capture time.

A batch is a **complete replacement snapshot**, not a patch. Adapters assemble
all still-current observations for the scope; omitted goal subjects become
unknown. Prior snapshots remain in the audit, so old contradictory reports
cannot silently contaminate the current assessment or disappear from history.
Catalog definitions are only read.

The effective goal is the conjunction of the original DesiredState expression,
all acceptance-criterion expressions and all declarative constraint expressions.
The original Intent and original user input remain in the audit. The effective
Intent is provided in each assessment document so planning, resolution and
policy evaluate the same requirements. CG-07 comparison semantics handle the
supported Boolean, numeric and other typed values. Success requires a satisfied
comparison, including acceptance criteria; model confidence is insufficient.

## Decisions and budgets

| Decision | Cause and next action |
| --- | --- |
| Continue | Remaining step contracts, dependencies, prerequisites, verification and their observed subject entries are unchanged. Resolve and authorize the remaining plan against the new snapshot. |
| Replan | Initial work, changed Delta, changed remaining evidence or changed step contracts. Consume the newly derived Plan and capture fresh resolution/policy inputs. |
| Pause | No observation snapshot, missing planning capability/input, unavailable authorization/evidence, or invalid compilation basis. Supply current evidence and authority, then refresh. |
| Success | The full explicit goal is satisfied by observed evidence. No runtime call is made for a NoOp. |
| Stopped | Hard failure, explicit blocker, policy denial, exhausted iteration limit or exhausted retry limit. A new run requires an explicit caller decision. |

Even Continue produces a Plan rebased onto the new Delta and Situation. This
preserves unchanged remaining work while invalidating old approvals and
projections. The application never infers Process Engine transitions or clears
process gates. A Process Engine blocker expressed by CG-09 as Deny stops the
run; recovery requires the owning process application and a new run.

Each dispatched attempt consumes one iteration before runtime invocation.
Retries count attempts whose supplied observations do not reduce the number
of actionable Delta items; this is a conservative progress test, not a claim
that every numeric improvement completes a condition. A retry limit of N
allows N such attempts to be followed by another attempt, then stops on the
next non-progress result. Counts never reset during replanning or refresh.
Missing observations pause immediately, and refresh still enforces the consumed
iteration limit. A hard failure or explicit blocker takes precedence over
satisfied evidence; otherwise evidence-backed success may finish on the final
allowed attempt.

Only one execution can be pending. Wrong execution IDs, wrong scopes and
invalid observation inputs cannot mutate the assessment or trigger a second
dispatch. `pending_execution()` exposes correlation for a corrected outcome
submitted through `ingest`. Duplicate completed outcomes are rejected. Adapters
must report uncertain transport failures as blocked and must not automatically
retry operations that may already have taken effect. Timeouts, cancellation and
cross-process persistence belong to the hosting adapter; this API keeps an
in-memory run and does not provide crash recovery or exactly-once delivery.

## Audit contract

`audit()` and `to_json()` expose a version-1 deterministic, redacted trace containing:

- original and effective Intent IDs, scope, unique run ID, limits and counters;
- every decision, stable reason, revision, source ingestion key, Delta item count,
  Plan ID, goal outcome and blocking planner diagnostic codes;
- each dispatch's correlation ID and a disclosure-limited context with resolution
  fingerprints, policy findings and process revision; external content, caller
  input, normalized task, output contract and constraints are redacted;
- correlated runtime status, source identity and the resulting revision;
- rejected compilation attempts and their diagnostics.

Serialization exports diagnostic references, not an authorization token or an
executable checkpoint. The full evidence and execution context remain available
through the authenticated application boundaries during the run. Audit records
never copy evidence payloads or free-form adapter explanations. Hosts should
still apply the scope's storage and retention policy to diagnostic references.

## Acceptance evidence

The `closed_loop` tests in the CG-10 application integration suite use the real
planner, resolver, policy engine and context compiler with synthetic runtime
adapters. They cover success, multi-step continuation, changed/missing evidence,
acceptance criteria and constraints, stale snapshots, wrong correlations/scopes,
process pause, missing authorization, denial, hard failure, blockers and budgets.

```sh
cargo test -p gateway-application --test context_application closed_loop
cargo llvm-cov -p gateway-application --test context_application \
  --json --output-path /tmp/cg14-coverage.json
python3 scripts/check-closed-loop-coverage.py --self-test
python3 scripts/check-closed-loop-coverage.py /tmp/cg14-coverage.json
python3 scripts/quality-gate.py
```

The complete quality gate enforces **95% line coverage** for the closed-loop
production module using the application coverage report. All established gates
remain in force.
