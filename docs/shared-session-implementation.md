# Shared session implementation — #294 prerequisite record

Scope: the typed #272 prerequisite of [#294](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/294).
Runtime status: **NOT_IMPLEMENTED**. #294 is **NOT_COMPLETE**. The binding gate
remains blocked on shared service, verification, interaction and recovery evidence.

The accepted [contract](shared-session-contract.md) and
[ADR-021](adr/ADR-021-shared-structured-session-ownership.md) require shared
application ownership before consumer implementation. Issue #294 includes the
minimum foundations under #272/#273/#275/#276/#277; their absence is implementation
work within the issue's scope. This record preserves that scope and does not
substitute a contract gate for the requested production runtime.

## Requirement gate

The requirement, architecture, automation and evidence perspectives were applied
as sequential passes by one agent. They are not independent reviewer approvals.
The explicit verified context-artifact goal and provider-neutral ownership are
settled by ADR-021. The shipped host's default session hook remains unsupported.
The canonical compilation path exists; it provides neither independent stored
artifact verification nor durable lifecycle services.

| ID | Requirement/source | Production owner/path | Dependencies and actual state | Implementation evidence | Verification level | Status |
| --- | --- | --- | --- | --- | --- | --- |
| SI-01 | #272 distinct identity, ownership, typed API | `gateway-application::sessions` | No transport dependency; domain enums and Intent reused | `contracts.rs`, `SessionApplicationPort` | CONTRACT tests | PARTIAL: typed API exists; no application provider |
| SI-02 | #272 revision, replay, stale, terminal and pending admission | Shared coordinator consumes pure admission gate | Atomic ledger acceptance belongs to #276; not implemented | `admission.rs`, `PendingRef`, `JournalAppend` | CONTRACT tests; no concurrent durable command acceptance | PARTIAL |
| SI-03 | #273 explicit artifact goal and independent final verification | Shared application coordinator/verifier | Current resolve/context services exist; verifier/evidence capture absent | Supported-goal validator only | CONTRACT; verified compilation/completion NOT_RUN | OPEN |
| SI-04 | #275 structured answers and exact consent | Shared interaction authority | Issuer/store and live revocation absent | `SourceQuestion`, exact `ActionBinding`, distinct `VerifiedConsent` | CONTRACT; real pause/denial/withdrawal NOT_RUN | PARTIAL |
| SI-05 | #276 durable restart, fencing and command outcomes | Outer journal implements shared port | PostgreSQL adapter and recovery/reconciliation absent | `SessionCheckpoint`, `SessionJournalPort`, conditional append validator | CONTRACT; crash/restart/concurrency NOT_RUN | PARTIAL |
| SI-06 | #277 retained aggregate limits | Shared service owns journaled counters | Enforcement at dispatch/nested adapters absent | Validated baseline action/retry/deadline budget | CONTRACT; runtime budget reservation NOT_RUN | PARTIAL |
| SI-07 | #294 actual CLI/MCP v2 binding and frozen v1 | Daemon composition root / inbound projection | Waits for SI-02..06 service gates | No launcher or v1 schema changes | EXECUTABLE session evidence NOT_RUN | BLOCKED |

## Implemented contract details

Opaque identities enforce the frozen ASCII token alphabet and 128-byte bound.
Revisions reject values above the JSON safe-integer maximum and refuse overflow.
Immutable ownership includes principal, workspace, project, binding and stable
client owner; transport connection and authority mapping revision do not enter
the owner identity. Each mutation contains one command ID and expected revision.
Command-outcome inspection has a separate query target from session inspection.

The supported goal validator preserves the domain Intent and registers this
initial shape: one `EQUALS true` Boolean condition named `context-verified`, with
subject `cg.context.<projection-record-id>.verified`, and the corresponding single
condition expression. Other desired states, acceptance criteria and constraints
are unsupported by this initial validator. The trusted registration separately
pins canonical scope, plan, step, projection and up to 256 unique source records.
The verifier-owned subject must eventually denote all the verification checks
required by the accepted contract. A client-authored observation of that subject
cannot become a completion observation. The typed shape does not implement that
verifier or weaken those required checks.

Clarification answers can select only an exact pinned record from a stored
structured question. They cannot carry consent or replace the goal. Consent
requests and verified store records are different types. Verified consent binds
owner/task/run/pending/issued revision, reserved dispatch, step, canonical action
and arguments, artifact basis, current authority and expiry. Approval retains
the issuance revision; its own revision increment is allowed for dispatch.
Changed binding, stale revision, expiry or live revocation refuses the grant.
`from_trusted_record` belongs exclusively to trusted issuer/store adapters;
client DTO parsing must never call it. There is no issuer/store implementation.

`Prepared` dispatch knowledge denotes an identity reserved for exact consent
before invocation. `Reserved` denotes outstanding invocation intent and `Unknown`
denotes an unresolved outcome. Pending/runnable snapshots cannot conceal an
outstanding invocation. Terminal snapshots cannot conceal uncertainty or retain
an unconsumed prepared reservation; completed snapshots require an evidence
reference. These shape checks are not evidence verification.

Typed checkpoints retain pending payload, accepted consent and denial/withdrawal
history alongside goal/owner/run and cumulative baseline budgets. The conditional
append validator refuses owner/goal/run/mode/profile changes, stale fences or
revisions, history erasure, limit/deadline changes, usage rollback and inconsistent
command outcomes. The journal adapter must additionally enforce global owner
command uniqueness, atomicity, durable commit and ambiguity handling. A pure
validator cannot prove those storage properties. No checkpoint serializer,
PostgreSQL session adapter, migration or recovery engine is implemented.

## Required continuation order

1. Complete the #272 gate with actual owner-scoped command-ledger acceptance,
   concurrent-start evidence and the typed authority/input/verification contracts
   needed by the coordinator.
2. Implement #273's canonical coordinator and independent stored-artifact verifier,
   then produce admitted observations and immutable evidence through CG-14. Reject
   absent semantic/model/connector behavior before creating sessions.
3. Implement #275's real structured questions and trusted consent issuer/store,
   live denial/withdrawal, one-use consumption and changed-basis revalidation.
4. Implement the required #276/#277 persistence, fencing, lost-response recovery,
   cancellation and cumulative reservation gates. Keep uncertain effects blocked.
5. Publish exact v2 schemas, fixtures and routing, then bind both shipped launchers
   to this one shared service. Preserve frozen v1 unsupported session behavior.
6. Retain positive and denied SHARED_SERVICE/EXECUTABLE evidence and >=95% changed
   production-file coverage before claiming #294 completion.

Rollback removes the unused typed module and its contract tests. There is no
runtime/storage migration. The original 10–18 engineer-day issue estimate includes
the missing foundations; this contract prerequisite does not remove that effort.

## Verification

`session_contracts.rs` exercises shared application admission and typed checkpoint
validation directly. It never injects a ProjectionHost. These are CONTRACT checks,
not real lifecycle, interaction, durable recovery or shipped-host acceptance.

Coverage command:

```sh
cargo llvm-cov -p gateway-application --test session_contracts --locked --json --output-path /tmp/cg-session-contract-coverage.json
python3 scripts/check-session-contract-coverage.py /tmp/cg-session-contract-coverage.json
```

The separate per-file >=95% checker reuses the existing coverage gate's validated
count rules and self-tests. It does not lower or replace the local MCP/runtime
gate. The retained candidate report records executed checks and source digests;
source changes require a new measurement. Runtime evidence remains NOT_RUN.

Retained [candidate report](evidence/EPIC-04.13-shared-contracts.json) and
[coverage counts](evidence/EPIC-04.13-shared-contract-coverage.json): all ten shared
contract tests pass, with 100% measured line coverage in each of the four new
production files. The full application suite, frozen v1 contract regressions,
architecture guards, format check and workspace Clippy pass. These results
qualify only the implemented contract prerequisite; SI-02..07 runtime gates
remain open or blocked as shown above.
