# Shared structured-session implementation — #294

Delivery state reviewed 2026-10-11. Retained reports below keep their original
candidate/source bindings; current full EPIC-04 acceptance requires a fresh
[full qualification](epic-04-acceptance.md), separate from #279.

The structured context-artifact runtime is implemented under its EPIC-08 owners
and bound to both shipped local executables. The registered structured baseline
is **QUALIFIED**: seven application runtime tests, five real PostgreSQL repository
tests and eight actual CLI/MCP scenarios pass. All 14 applicable changed production
files exceed 95% measured line coverage (minimum 95.17%).
See the retained [runtime report](evidence/EPIC-04.13-shared-runtime.json),
[coverage counts](evidence/EPIC-04.13-shared-runtime-coverage.json),
[actual transitions](evidence/EPIC-04.13-shared-runtime-transitions.jsonl),
[verified evidence](evidence/EPIC-04.13-shared-runtime-evidence.json) and
[qualification log](evidence/EPIC-04.13-shared-runtime.log).
The earlier [contract-only report](evidence/EPIC-04.13-shared-contracts.json)
records the previous delivery and remains historical evidence.

## Production ownership

`gateway-application::sessions::SessionCoordinator` owns start, inspect, clarify,
approve, continue, cancel and explicit recovery. It composes the existing
resolver, Process/Policy application, canonical context compiler and CG-14 goal
assessment. `boundary` translates the exact v2 envelope; `local_mcp` owns JSON-RPC
and transport lifetime. `cg-local` and `cg-mcp` instantiate the same coordinator
and PostgreSQL repository through `local_sessions::LocalApplication`.

`PostgresSessionStore` persists the full typed goal, immutable owner/run,
actual initial CG-14 assessment and pinned authority, selected input, pending
payload, consumed consent, authority decisions,
cumulative budget, lease, fence, command ledger and immutable artifact/evidence
bytes. It uses the existing scoped PostgreSQL journal with row locks. Conditional
append commits state, budget reservations, command acceptance, fencing and
released records together. Failed validation or storage operations roll back;
an ambiguous commit returns `CG_OUTCOME_UNKNOWN` and requires inspection.

Current session loads use a fresh PostgreSQL statement snapshot of the scoped
payload, digest and revision. They validate the checksum, complete journal,
owner and typed checkpoint without inserting a row or acquiring a writer row
lock. Conditional mutations still use transactions and fencing; a read result
cannot authorize a later write. An absent journal returns the same scope denial
as an absent owner-bound session. This change reduces repeated liveness-read
cost without caching claims between callbacks. The
[fresh-read component report](evidence/EPIC-08.03-postgres-fresh-read-validation.json.gz)
retains 21 repository and 15 local-admission passing tests, including separate
connections, a held writer lock, committed revision visibility, stale-CAS denial
and corruption rejection. It does not requalify the historical full runtime or
resolve the remaining connector-target latency by itself.

## Acceptance evidence

| Requirement | Production behavior | Meaningful verification |
| --- | --- | --- |
| Shared start/inspect and verified result | A registered domain Intent requests `cg.context.<projection>.verified`; the verifier independently reads the committed canonical artifact, checks digest/IR/current inputs and captures actual observations. CG-14 must report the exact goal satisfied before immutable evidence is committed. | Actual CLI/MCP baseline, restart and evidence resource; component corruption and current-input tests |
| Real clarification and consent | Source alternatives require one exact selection, which becomes an actual compiler input. Separate consent binds owner/task/run/nonce/revision/dispatch/step/action/arguments/basis/current authority/expiry. Only the trusted operator issuer writes grant records. | Actual source selection followed by a separate consent pause; replay, wrong nonce/reference, expiry and restart |
| Current authority and revocation | Fresh admission, resolution, Process/Policy and exact live grant checks run before reservation and verification. Clarification supplies no authorization. Denial/withdrawal are persisted authority events. | Actual changed-action/current-policy denial and withdrawal; component changes during dispatch and live revocation |
| Bounded explicit cancellation | Atomic fence replacement stops all later result commits by an outstanding pure compiler. Cancellation is terminal and inspectable; EOF and transport timeout never issue a session cancel. | Actual cancellation, EOF/reconnect; application/transport tests and PostgreSQL stale-fence rollback |
| Recovery, replay and budgets | Owner-wide command IDs, expected revisions and database row locks prevent duplicate dispatch. A live lease refuses takeover. Recovery reads committed artifacts, or fences an absent result and stops the pure task without retry. Absolute deadline and spent counters never reset. | Competing actual host processes, restart, real completion-commit failure/read-back, deferred commit ambiguity, corruption and rollback |
| Explicit capability boundary | Other desired states, semantic interpretation, model invocation and external connectors remain unsupported. Frozen v1 session tools retain their previous unsupported behavior; enabled v2 tools are separate names. | Actual unrelated-goal rejection and strict discovery/version tests; frozen v1 regressions |

Evidence levels are explicit: `session_contracts.rs` tests contracts;
`session_runtime.rs` tests the real application with controlled port failures and
a test clock; `session_store_tests.rs` tests real PostgreSQL transactions;
`test_sessions.py` tests actual shipped executables with a synthetic stdio client.
There is no prebuilt session projection supplying the runtime acceptance result.
These checks do not claim installed Codex-client qualification (#295), full
EPIC-04 reconciliation (#296), or the complete EPIC-08/model/connector runtime.

## Enable a registered task

Existing admissions remain valid. Sessions are opt-in: add a strict `sessions`
object to an existing canonical mapping, alongside its admitted plan/rules/
process/projection resources. Supply:

- `enabled: true`, an absolute `store_file` path, and `issuers` containing the
  authenticated launcher principal permitted to issue CG consent.
- The exact supported domain `intent`, and `basis` containing canonical `scope`,
  pinned `plan` and `projection` references, `step`, and `sources` alternatives.
  Session references contain exactly `id`, `revision`, `digest`. Source documents
  use the existing `cg.context-fragment` contract and strict fragment parser.
- `execution: {"mode":"DEVELOPMENT","profile":"FULL_PATH"}` matching the
  current canonical plan/policy, bounded `max_actions`/`max_retries`, absolute-run
  TTL configuration `ttl_ms` (1..86400000), and `consent_required`.

An optional trusted `resource_limits` object admits cumulative resource units
for new sessions. It contains a `cost_unit` reference and an `amounts` object
with explicit unsigned integer fields: `model_attempts`, `input_tokens`,
`output_tokens`, `output_bytes`, `cost_microunits`, `retrieval_rounds`,
`retrieval_results`, `retrieval_tokens`, `repair_attempts`, `worker_attempts`,
and `worker_compute_units`. Optional `connector_calls` defaults to zero and is
omitted when zero to preserve legacy reservation identities. One unit covers
one logical discovery, invocation or resource read, including bounded discovery
negotiation and pagination. At least one limit must be positive; zero refuses
that resource. Other missing fields and unknown fields are rejected. Admission is
authenticated against the owner and exact registered goal before Start; later
configuration changes cannot raise an existing session's limits.

The PostgreSQL checkpoint persists immutable resource limits, total conservative
charges and exact step reservations. Before connector preparation the
coordinator commits `preparing`, its reservation and command outcome together.
Approved consent resume obtains a fenced claim on that same reservation before
reading/reconstructing the proposal. Failed persistence releases no model or
resume call. Cancel or expired-lease recovery retains consumed resources and
never repeats preparation. Separate writer/reader-process tests qualify this
storage behavior; they do not qualify a PostgreSQL server restart or external
provider effects. The local host shares an `Arc<LocalSessionAccess>` with future
runtime adapters. This object owns current authenticated configuration and
resource-claim lookup against the same PostgreSQL journal; it owns neither a
runtime nor a coordinator. It
checks exact owner, scope, session/run, revision/fence, reservation/phase and
both wall-clock and original monotone deadlines. Joined invocation/connector
admission still requires producer and target configuration.

`host_mapping::policy` maps canonical policy against the exact current
resolution without requiring a context projection. `PolicyApplication::step_input`
and ordinary plan evaluation use the same native assembly, including resolved
capability closure, process readiness, constraints, preconditions and structural
prerequisites. A connector source must use those inputs and check that its
requested capability belongs to the actual resolved step.

`SessionConnectorRun` enforces connector-only child allocations with hard
operation quotes and current CG-09/claim checks; its controlled-peer tests do
not install it in the shipped runtime. `LocalConnectorRuntime` remains
unsupported until current producer/policy/connector/readback composition and
remaining retrieval/worker enforcement are implemented.

The registered Intent has exactly one Boolean `EQUALS true` condition named
`context-verified`, subject `cg.context.<projection-id>.verified`, a corresponding
single-condition expression, and empty acceptance criteria/constraints. Its
meaning is successful **verification of that context artifact**. It does not
execute or satisfy the unrelated desired state used to produce the pinned plan.
When source alternatives are registered, the action requires one chosen source;
unanswered selection produces the persisted structured input question.

The credential file contains a PostgreSQL connection string, for example
`host=127.0.0.1 port=55432 user=cognitive_gateway dbname=cognitive_gateway password=...`.
Keep that file outside the checkout with owner-only permissions; only the trusted
storage adapter reads it. The worker environment remains free of provider keys
and client authentication state. Start PostgreSQL as described in
[postgres-compose.md](postgres-compose.md).

`--session` identifies the stable admitted client owner across CLI processes and
MCP reconnects; it is distinct from the task's returned `session` and `run` IDs.
Both executables use the same launch options and admission. `cg-local --check`
reports `session_schema_version: "2.0"` and enabled mutations only for an enabled
session mapping. Unconfigured hosts advertise the original 13 tools; configured
hosts add the six `cg_session_*_v2` tools and v2 contract resources.

## Operator authority and recovery

The operator uses the same authenticated launch binding, with an allowed issuer
that equals its authenticated principal. This operation is intentionally absent
from the MCP tool catalog and accepts exactly these fields:

```json
{"session_id":"<returned-task-id>","issuer":"<admitted-principal>","decision":"approve"}
```

Pass that file through `cg-local --operation session.authority --request FILE`
and the existing required launch options. `approve` returns a persisted reference;
the v2 `session.approve` command consumes that reference for the current pending
nonce/revision. Raw client approval flags never create a grant. The operator can
also use `deny`, `withdraw`, or `recover`. Withdrawal works before or after grant
consumption, including after completion: terminal withdrawal updates the
authority audit while preserving the task revision, result and spent budget.
Rejected decisions do not advance state. `recover` is an explicit
trusted action; `session.inspect` never resumes, expires or retries a task.

On restart, inspect the session or the committed command ID first. For a reserved
compiler, wait for its persisted 30-second lease to expire before trusted recovery.
A committed artifact is independently verified against current authority; an
absent artifact is fenced and the pure task fails without releasing another
result. Expired questions are renewed only by explicit recovery when the run's
absolute deadline still permits it. Expired runs stop with retained usage.

Completed evidence is read through the existing owner-scoped reference URI:
`cg://workspaces/<w>/projects/<p>/bindings/<b>/references/<id>/<revision>/<digest>`.
The response uses the v2 evidence resource contract. Artifact bytes themselves
are not exposed through that resource path; the evidence receipt links their
immutable digest, exact goal/basis, current policy and verification checks.

## Qualification and rollback

```bash
python3 scripts/cognitive-test-host.py bash scripts/qualify-shared-sessions.sh /tmp/cg-session-evidence
```

This runs against a disposable loopback PostgreSQL host, requires the database
for the mandatory gate, tests contracts/shared services/actual executables,
uses distinct canonical project scopes per scenario so repeated full-gate runs
share the database without exhausting another scenario's bounded journal,
independently validates public responses, retains transition transcripts and
verified evidence, and enforces measured >=95% coverage for every applicable
changed executable production file. The same gate is registered in
`scripts/quality-gates.json`. Architecture, frozen contracts, format, Clippy and
workspace tests remain separate checks.

Storage uses journal kind `task-sessions-v2`, schema version 2, alongside existing
cognitive journal kinds. Rollback disables the mapping's session capability and
restores unsupported discovery; it does not delete PostgreSQL data. Terminal
sessions and owner-wide command ledgers are retained together. No owner transfer,
external-effect retry, semantic fallback or provider invocation is implemented.

## Connector retained-goal verification follow-up

The shipped local connector runtime implements pure final verification through
`local_sessions/goal_verifier.rs`. It authenticates the current configuration
and exact PostgreSQL checkpoint, reloads the actual owner-scoped immutable
goal-readback records, and builds the same complete readset as the application
coordinator. Supplied readbacks cannot replace stored records. Native
`ClosedLoop::verify_retained` evaluates the original registered intent, including
criteria and invariants, without a model or connector call. Each receipt binds
the full goal and internally hashed records, quality, source/version domains
and condition bindings. Legacy receipts without this readset binding cannot
qualify a new connector completion. New connector final evidence uses contract
version `2.0`; historical terminal evidence remains readable unchanged.

The read adapter uses the same store instance and checks the original checkpoint
and fence before each record lookup. Deadline checks surround the store reads
and native verification; current configuration and native resolution pins are
checked again before returning. That final check also loads the claim through
the currently configured repository: unchanged locator paths cannot hide a
changed store configuration behind the original open handle. Cancellation or a
concurrent checkpoint change invalidates the verification. These reads do not mint another resource allowance
or update the checkpoint. A PostgreSQL component scenario exercises normalized
source data through storage, decoding and the actual product verifier, including
weakened-goal rejection, deadline expiry and concurrent cancellation.

This delivered verifier consumes retained observations. Connector preparation,
execution and acquisition of new snapshots/readbacks still require their real
policy, producer and transport composition. The complete connector lifecycle,
installed-client/live qualification and current expanded coverage gates remain
open; the verifier component tests do not establish EPIC-08 acceptance.

### Private capability reference aliases

The private runtime's `cg.connector-capability` documents register full scoped
reference aliases for native `CapabilityDefinition` values. Their revisions
belong to the operator registration. The native capability index has no separate
capability revision, so the runtime does not derive one from an ID, registry
fingerprint or connector inventory version.

Producer-join references must match the loaded registration in ID, scope,
contract, contract version, revision and digest. The loaded JSON must parse as a
native declaration and canonically equal the actual currently resolved action
declaration. The descriptor ID and native CapabilityId must agree. Unsupported
contracts, drifted definitions, mismatched pins and incorrect content digests
are rejected. This check authenticates the referenced data; native policy,
Process/Context, consent and current inventory checks still govern dispatch.
