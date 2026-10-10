# Shared structured-session implementation — #294

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
