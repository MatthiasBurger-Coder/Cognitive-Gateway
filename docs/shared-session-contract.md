# Shared session contract refinement — EPIC-04.12 / #272

Date: 2026-10-10. Status: **contract refinement; no session runtime implemented**.
Owners: #272 contracts, #273 coordinator, #275 interactions, #276 persistence,
#277 budgets; inbound binding #293/#294. The user accepted structured tasks with
existing Rust services; Semantic/model/connector paths remain unsupported.

## Product completion boundary

`cg.intent / 1.0` carries the existing validated domain Intent. Its DesiredState,
acceptance criteria and constraints remain authoritative. It is not an arbitrary
task-kind DTO and cannot be silently reinterpreted as "context was compiled".
The shared host must name a supported structured task, its precise success
condition and the trusted source of its observations before exposing start.

One decision remains open: whether the first task explicitly requests a
verified context artifact, or requires an executed fachlicher Sollzustand.
The latter additionally needs a concrete execution/observation adapter; existing
Rust assessment/planning/resolution/context services alone cannot produce its
verified outcome. Neither a model claim, fixture fact nor serialized result
projection can substitute for this boundary. This open choice affects #273
implementation and dispatch qualification; it does not block canonical wiring.

## Ownership and trusted inputs

The application layer owns SessionId, immutable client owner, lifecycle,
revision, pending interactions, command deduplication, budgets and terminal
result. `ClosedLoop` remains the planning/execution decision owner. A shared
coordinator composes it with current-authority and journal ports; no MCP framing,
Codex identity DTO or provider SDK enters that service. Both `cg-local` and MCP
call the same service.

The outer host authenticates snapshots, loads current catalog/process/policy,
acquires external outcomes if supported, and supplies the journal adapter.
It does not implement another planner or a second lifecycle. Snapshot admission
must include source identity, digest/revision, freshness, classification, scope
and evidence links. Client answers may identify a supported typed input or select
an admitted source; they cannot manufacture observed facts or permissions.

Use distinct IDs for transport connection, client-owner session, task SessionId,
run, command, dispatch and pending interaction. Task ownership includes principal,
workspace/project/binding and client-owner session. It never comes from a mutable
request field. Mapping revision is an authority snapshot, not task identity.

## Commands and transitions

Keep the frozen inbound session envelope. Map it into provider-neutral typed
application commands only after admission and existing CG policy checks:

| Command | Required application semantics |
| --- | --- |
| start | Strict supported Intent, authenticated initial snapshot, unique command ID; allocate task/run IDs and persist initial assessment before returning |
| inspect | Query exact owned task and journal revision; no side effect or automatic resume |
| clarify | Exact task/revision/pending ID; validate supported typed answer against stored question; consume once; never grant consent or fabricate observations |
| approve | Resolve exact trusted CG consent record; validate owner, task, pending ID, revision, expiry, action/arguments and current authority; client approval flags are insufficient |
| continue | Revision checked; fresh authority/snapshots and current consent revalidated; schedule at most one eligible dispatch; preserve cumulative limits |
| cancel | Explicit authorized terminal request; distinguish cooperative cancellation, uncertain in-flight outcome and confirmed terminal cancellation |

Every accepted mutation increments a monotonic bounded revision. Rejected
commands do not mutate state. Duplicate command IDs are refused, including after
restart. A previously committed command whose response was lost must be inspected;
it is never silently re-executed. Terminal tasks cannot be resumed or restarted.
Wrong owner/scope, stale revision or altered pending ID fails before dispatch.

`ClosedLoop` Continue/Replan maps to runnable shared work; Pause requires an
actual actionable stored input/consent request or an explicit supported failure
reason. Unsupported capability is refused, not converted to an invented
clarification. Stopped maps to a recorded terminal failure or explicit confirmed
cancellation. Success maps to completed only when the exact supported goal has
an authenticated verified evidence reference. Inspect must not claim completed
with a null or merely projected final result.

## Interaction bindings

A pending question owns its typed schema, task, revision, input basis, expiry and
single-consumption marker. Semantic interpretation remains unsupported in the
accepted first path. Changing relevant basis invalidates the prior question.
An answer must enter the normal structured validation/capture boundary; it does
not bypass normalization or CG-14 assessment.

A pending consent owns the exact action identity, canonical argument digest,
step, authority/process/catalog fingerprints, owner, task revision and expiry.
The verified consent record is supplied by the trusted consent authority through
a pinned reference. `session.approve` transports that reference; it does not mint
consent. Denial and withdrawal are recorded. Any changed action/arguments or
relevant authority invalidates previous consent, including between approval and
continue. Re-evaluate CG policy immediately before any dispatch.

## Journal, recovery and budgets

The shared application defines journal events and validates recovery. The outer
adapter must provide atomic conditional append, durable acknowledgement and
single-owner/concurrency control. Persist initial scope/goal, command ledger,
revisions, input/authority fingerprints, pending payloads and expiry, consumed
iteration/retry/interaction budgets, dispatch intent/correlation, outcome and
verified final evidence reference. Stored secret payloads remain reference-only.

Persist dispatch intent and consumed budget before invocation. Record correlated
outcome afterwards. Recovery of an intent without a confirmed outcome enters an
explicit uncertain state; never replay a possible effect automatically. Reconnect
only reattaches an admitted owner. EOF or MCP invocation cancellation does not
cancel the task, replenish budgets, acknowledge consent or restart dispatch.

`ClosedLoop::to_json()` is a redacted diagnostic trace, not a restorable
checkpoint. Do not deserialize it into trusted executable state. Recovery needs
validated checkpoint/event records carrying all required state and authenticated
inputs, with explicit schema/version and corruption checks. A journal adapter
alone does not implement these application semantics.

## Required delivery evidence

The minimum shared foundation must prove actual typed start/inspect, true input
and consent pauses, valid replies, single dispatch, fresh-authority continuation,
verified result, cancellation, disconnect/reconnect and process restart. Include
wrong-scope/owner, duplicate/stale/expired reply, changed action/arguments/policy,
withdrawal, exhausted budgets, journal conflict/corruption and uncertain dispatch.
Run through the shipped local composition root and the installed client after
shared service tests pass. An injected `ProjectionHost` is not this evidence.

Contract refinement becomes READY only after the concrete supported task and
result source are fixed and the shared journal/coordinator interfaces are
executable and reviewed against existing application contracts. #293 and #294
remain open while this document describes future implementation.
