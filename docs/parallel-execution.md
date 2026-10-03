# CG-28A bounded parallel execution

`gateway_application::parallel_execution` schedules one previously compiled task
context per worker. The planner owns task proposals. `ContextApplication::compile_step`
revalidates resolution, Process mapping and the current CG-09 Policy decision before
it constructs the `CompiledStep` used by a `TaskCapsule`. The scheduler cannot
construct that context or change its approved capabilities. Replanning must compile
new contexts and new capsule digests.

## Contract and authority

A capsule binds one task ID, parent plan, objective, completion condition, resource
claims, capability subset, knowledge requirements, evidence requirements, input
snapshot, execution group, dependencies, retry/timeout/context budgets and the
already compiled context and output contract into a SHA-256 content digest. Delegation and scope
expansion are rejected. `dispatch_contract()` renders the provider-independent
mission, scope, permissions, stop conditions and out-of-scope reporting rule.
The runtime receives the compiled per-step context and output contract; the
full `CompiledStep` and its original caller input are used at admission but are
not exposed through the capsule. Retrieved fragments remain data inside the
compiled per-step context. The
scheduler requires declared knowledge queries to match that context and checks
its byte budget before dispatch.

The host exposes tools through `GuardedTools` only. It checks the exact capability,
forbidden resources, read/write/lock mode and relative resource path before a
call reaches `ToolPort`. File claims use path-segment containment; read/read is
compatible, writes and exclusive locks conflict. Claims are sorted before their
digest is calculated. The scheduler reserves each task's complete claim set
atomically in task-ID order, so workers never acquire partial lock sets or
wait on each other in a lock cycle. The host tool implementation must resolve file paths and
symlinks within its own sandbox; a logical claim alone is not an OS sandbox.
The worker has no Process mutation port. Any discovered adjacent work can be
returned as an out-of-scope observation; it is never scheduled automatically.

## Scheduling and results

`Scheduler::new` rejects duplicate IDs, unknown dependencies, cycles, missing
groups, malformed barriers, mixed plan/snapshot graphs and zero concurrency.
`next_batch` asks the trusted `DispatchAuthority` to recheck current Process
and Policy readiness, then chooses task IDs in lexical order from ready,
conflict-free work, respecting the declared sequential/parallel group and
concurrency limit. Denied tasks receive typed blocked results.
`run_wave` launches that batch on scoped threads, folds results in task-ID order,
and checks the current authenticated snapshot before and after each worker call.
The runtime is replaceable through `SubagentRuntime`; it receives exactly one
capsule and a guarded tool port.

`submit` checks task identity, capsule digest, snapshot, attempt number, evidence
requirements, reported resource changes and host `ResultVerifier` before any
result is accepted. A failed task retries only within its declared budget;
exhaustion is typed. Cancellation records pending branches as `Cancelled`; running results are
recorded as cancelled when they return. A terminal failure under `FAIL_FAST`
cancels the remaining graph. Malformed worker results stop the graph and leave
explicit failed or cancelled branch results instead of occupying slots forever.
Barriers retain all upstream result envelopes in sorted task-ID order, including
missing and failed branches. `ALL_REQUIRED`, `ANY_SUCCESS`, `QUORUM(n)`,
`FAIL_FAST` and `COLLECT_ALL` are typed policies with `PENDING`, `SATISFIED`
and `FAILED` outcomes. A retryable branch stays pending until it succeeds or
exhausts its budget. A task can depend on a barrier;
only its declared policy can release it. Joined results are evidence for the
calling Process/Closed Loop application to inspect, not a direct lifecycle
transition or goal-success assertion.

The in-process baseline measures wall time and rejects late results. It cannot
preempt a CPU-bound worker or an in-flight host tool call. A production host that
requires hard time or memory limits must enforce them in its runtime process or
container adapter. The `DispatchAuthority`, `SnapshotPort`, `ToolPort` and `ResultVerifier` are trusted
host boundaries; the worker cannot provide current revisions or verify its own
evidence.
