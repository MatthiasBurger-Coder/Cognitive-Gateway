# Connector sessions with an already satisfied goal — #403

This delivery adds an invocation-free preparation path to the shared session
coordinator. It covers an original connector goal that current authenticated
reads already satisfy. Execution of an unfulfilled goal remains part of #274;
this path does not establish full EPIC-08 acceptance.

## Configure the installed entrypoint

Use the existing authenticated `cg-mcp` / `cg-local` launcher and canonical
admission described in [shared sessions](shared-session-implementation.md).
The `sessions` mapping selects either an artifact `basis` or a `connector_goal`.
For connector goals, omit `basis` and supply:

- The complete original `intent`, exact plan reference, admitted step IDs and
  one registered observation per goal condition. Each observation names its
  actual source, canonical capability and read operation class.
- `resource_limits`, containing the cumulative resource amounts and cost unit.
  Preparation requires zero model, repair and worker allowances. Conservative
  discovery/read reservations are retained after failure or interruption.
- An `observed_goal_runtime` with exact plan, goal, native policy, manifest,
  mapping, semantic task, context projection, read association, quote and verifier
  bindings. Every document has an absolute path and a full scoped content pin.
- One `goal_reads` entry per original condition, with exact mapping reference,
  capability, source, typed arguments and normalization pin. Identical full read
  routes share one acquisition. Separate objects use separate argument routes.
- An absolute stdio executable and working directory, bounded protocol limits,
  a compatibility declaration, and an exact credential profile association.

The executable setup in
[`observed_goal_session.py`](../crates/gateway-daemon/tests/support/observed_goal_session.py)
builds these definitions from real `cg assess`, `plan`, `resolve` and `compile`
outputs. It is a controlled test peer, not a production operator configuration.
Use the operator's actual catalog, process, policy, server and credentials.

The read association uses `cg.connector-read-association/1.0` and
`EXACT_TASK_OPERANDS_V1`. It binds actual task targets or input-reference
operands to mapped scalar arguments and response identity fields. The response
object field must equal the normalization object field. No free-form argument
template, model proposal or synthetic capability alias supplies authority.

The supported normalization acquisition is `DIRECT_CURRENT_READ_PROFILE` with
`REQUEST_START_LOWER_BOUND` and an explicit positive maximum age. Its source,
sensitivity and authenticated profile must match the admitted route. An exact
qualified scalar read has nonprobabilistic confidence `NotApplicable`; legacy
unknown quality remains unknown and cannot prove the goal.

This profile requires an entirely `Inspect` native capability closure; a mixed
or mutating closure remains unsupported even when the selected wire operation
is a read.

## Credentials and storage

`store_file` remains an absolute, private file containing the PostgreSQL
connection string. One command pins that selector and shares its connection;
every authority and checkpoint read remains fresh. Changing the database
selector during a command produces a stale-revision failure.

`credential_store` is a separate bounded, strict JSON file:

```json
{"schema_version":1,"profiles":{"records.local":{"connector":"example.records","server":"example.records.local","tools":["records.read"],"generation":1,"environment":{"TOKEN":"operator-secret"}}}}
```

Keep both files outside the checkout with owner-only permissions. The exact
profile, server and allowed wire tool must match. Secrets enter only the
credentialed child transport environment; goal evidence and policy audit retain
no secret values. Credential resolution happens on connection; reconnect
resolves the file again. The runtime admission's stdio environment is empty.

## Lifecycle and recovery

`Start` validates the full native task and current definitions and returns
`Runnable`. `Continue` reserves preparation resources durably, acquires a fenced
claim, discovers the admitted inventory and executes the exact readonly routes.
The actual in-flight acquisition uses the existing `Dispatching` state with
`Reserved` dispatch knowledge. `Preparation` is its resource phase. The frozen
v2 schemas and state vocabulary remain unchanged. Once the reads have a known
`NoMutation` outcome, marker persistence returns to `Runnable` with `Prepared`
knowledge while independent goal verification is pending.
Current native CG-09 decisions and sanitized policy audit precede the actual
tool call. Preparation grants no mutation permission or action consent.

Strong observations must cover every original condition with consistent scoped
object versions and fresh authenticated metadata. The native verifier evaluates
the full original goal. A successful tool response alone cannot complete it.
An unfulfilled goal returns the unsupported-capability outcome; its consumed
reservation is retained.

The coordinator commits a typed immutable pre-execution goal marker, reloads it
from PostgreSQL, checks its owner, kind, digest, current definitions and age,
then independently runs the native verifier before committing `Completed` and
final evidence. This creates neither an `InvocationIR` nor an `After` snapshot.

After an interrupted command, inspect the persisted session first. Trusted
`session.authority` recovery uses the existing authenticated operator command.
A committed runnable goal marker can be verified without another provider call.
A preparation interrupted before marker commit fails after its lease expires;
recovery does not reset the budget or repeat acquisition. Definition drift,
expired observations, corruption and a competing fence prevent completion.

## Qualification and rollback

Run the installed integration test against a disposable PostgreSQL host:

```bash
python3 scripts/cognitive-test-host.py cargo test -p gateway-daemon --test observed_goal_sessions
```

Run the complete registered qualification, including the application and daemon
regressions and the strict per-file production coverage gate:

```bash
python3 scripts/cognitive-test-host.py bash scripts/qualify-observed-goal-sessions.sh /tmp/cg-observed-goal-evidence
```

The test must run with PostgreSQL enabled to constitute installed runtime
evidence. Application-only tests and a database-free test invocation are
insufficient. Workspace, architecture, contract and measured production coverage
gates remain required before declaring this delivery complete.

Rollback disables the admitted session mapping and preserves its journal and
immutable evidence. Existing artifact session admissions and omitted serialized
connector fields retain their previous meaning.
