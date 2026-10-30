# Connector release qualification — EPIC-07.12 #235

Historical evidence and removed review documents are linked to their archived Git
revision. They are not current acceptance evidence. Generate fresh reports in an
output directory or retain them as CI artifacts.

The qualification suite retains separate synthetic and authenticated live evidence
for the complete connector acceptance surface. Rust owns runtime, mapping,
normalization, CG-09 enforcement, retry decisions and aggregate accounting.
Python executes gates and supplies an opt-in external MCP test server.

Run synthetic security, failure, compatibility and golden scenarios:

```sh
python3 tests/scripts/qualify-connectors.py --run --output target/connector-component
```

Run complete #235 qualification, including controlled GitHub effects, independent
observations and every existing quality/coverage gate:

```sh
python3 tests/scripts/qualify-connectors.py --release \
  --live-repository MatthiasBurger-Coder/Cognitive-Gateway \
  --output target/connector-release
```

The live profile requires the operator's authenticated `gh` session, network
access, repository branch permissions, Docker and the documented Rust/Python
quality prerequisites. It creates two unique `cg-qualification-235-*` branches
at the existing main commit, never updates an existing branch, and removes only
branches whose successful creation it recorded. It sends no issue/PR comments.
Secrets are resolved through the outer credential adapter, never written to the
report. Live scenarios use the established `live_adapter_profile` naming so
ordinary CI and coverage remain offline. The explicit release command runs them
with `--ignored` and records their actual success records.

Output directories must be new and under `target/`. Omitting `--run`/`--release`
produces NOT_RUN and exit 1. Missing/ignored required tests, zero-test successes,
ambiguous matches, source drift, incomplete live observations or a failed full
quality gate reject qualification. Reports bind revision, tracked/untracked
source hashes, exact commands, successful test identities and recursive artifact
hashes. The Git index (including execute bits) is bound separately and must remain
unchanged during qualification. Previously retained reports never substitute for
fresh execution. The conformance GitHub HTTP peer is Git-executable: its missing
execute bit caused the existing Linux CI failure, masked by Windows mount modes.
A regression checks the canonical Git mode rather than trusting local permissions.
The complete serial quality subprocess has an eight-hour orchestration timeout
to accommodate fresh instrumented builds. Individual connector gates retain
their thirty-minute orchestration limit; application request, session and
resource deadlines remain enforced by their existing tests.

## Evidence and compatibility

`tests/scripts/connector-acceptance.json` covers all 30 original parent criteria,
three qualification additions and six parent integration-review additions.
Each report embeds their observed status and exact executed test. The executable
compatibility matrix is
`tests/fixtures/conformance-connector-v1/variations.json`: protocol/version and
schema incompatibility, schema drift, unknown/duplicate tools, malformed results,
substituted objects, RPC/framing failures and wrong response IDs. Standard and
legacy conformance profiles use the same provider-neutral contracts.

The GitHub golden resolves an issue reference, invokes the actual MCP executable,
ingests scoped evidence, normalizes state, derives a delta/plan, resolves a
configured inspection capability, evaluates CG-09 and compiles selected evidence
references through CG-10. The frozen component projection lives in
`tests/fixtures/connector-qualification-v1/golden.json`. The same test also runs
the shipped `cg` commands, checks exact evidence/provenance propagation and
current-policy denial, and repeats compilation without another connector call.
The live read profile retrieves actual issue #235 through `cg-github-mcp` and
runs that native CLI chain with its acquired records. Operator-owned desired
state, catalog, process and policy mappings remain explicit; no model or
unrestricted natural-language/session execution is claimed.

The separate live peer performs actual GitHub ref creation and readback through
admitted MCP tools. One creation returns an acknowledgement; a second deliberately
loses its response after the external effect. Rust verifies consent denial sends
nothing, acknowledgement cannot become completion evidence, uncertainty never
replays the mutation, and a new authenticated MCP connection produces an
`IndependentPostEffectRead` bound to the exact original request. The observations
cross the same `ConnectorApplicationPort`, `ConnectorResult` and recovery
contracts consumed by #274/#276. No external exactly-once guarantee is claimed.

Connector retry/reconnect tests charge the same native session allocation without
resetting usage or deadline. Full quality includes PostgreSQL retention/restart,
installed observation, architecture and all existing >=95% production gates.
There is no new production runtime code; the Python qualification runner itself
also has a >=95% executable regression gate.

Full qualification also exposed a pre-existing shared-coordinator coverage gap
(775/818 lines). Additional native session regressions exercise scope drift
before artifact commit and before independent verification, plus recovery of a
journaled cancellation. They require no foreign-scope goal completion, no
redispatch and no budget refund; the existing 95% threshold remains unchanged.
The shared-session, Before-journal and atomic-Before launchers also pin the LLVM
coverage directory before cleanup. Their manual `show-env` flow measures directly
in that directory; cleaning Cargo's derived nested directory would retain old
profiles. An executable regression checks the environment at the first cleanup
command, before any compilation or test process is allowed to start.
The shared-session gate now also executes the existing native `codex_sessions`
scenario, covering shipped session bridge paths missing from its previous test
selection. Its local MCP adapter threshold is unchanged.
Runtime regressions also cover oversized malformed error metadata and failed
reply writes after completion, cancellation, panic and timeout. They check bounded
sanitized diagnostics and that a lost reply never restarts application work.
Resource-authority regressions reject stale claims and expired phase creation,
then expire a partially consumed phase and verify that allocation fails without
refilling its remaining allowance or changing the durable reservation.
Snapshot library and integration regressions revalidate canonical reload for
atomic, versioned and bracketed profiles and require the selected consistency
data to survive unchanged. Both isolated and broad coverage gates exercise this
contract using the existing native fixtures.
Domain library qualification reuses the public semantic resolution, epistemic
and procedure promotion contract cases alongside their integration targets.
Additional cases reject unsupported retrieval versions and freeze every
sufficiency audit code. Existing coverage reports guide these additions; static
review never substitutes for fresh coverage in the final full release run.

## Acceptance ownership and report meaning

#235 and ADR-022 require qualifying connector contracts consumed by #274/#276;
complete session orchestration and the general natural-language host have their
own EPIC-08 acceptance.
EPIC-07 retains transport/provider ownership; EPIC-08 owns session persistence,
aggregate lifecycle and goal orchestration. The matrix keeps every original and
added criterion while correcting these rows to their actual contract boundary.
Live effects and the native CLI chain are now executed, rather than inferred
from component success. This correction does not close #274/#276/#277/#279 or
assert their full system implementation.

`QUALIFIED_COMPONENT_SCOPE` permits no closure and retains the missing live/full
quality rows. Only `QUALIFIED_CONNECTOR_RELEASE`, all 39 rows verified, successful
live observations and the exact complete quality manifest allow #235 closure.
The report's `closure_issues` contains only 235; parent #223 and system #279
acceptance remain separately assessed. #279 consumes the normalized component
and live contract evidence from this report.

Requirement, architecture, automation and testing reviews are role passes by one
agent; no independent review or approval is claimed. Intake revision: `659c52d`,
DEVELOPMENT/FULL_PATH. ADR-022 and the existing result/policy/resilience contracts
remain authoritative. Rollback removes this additive qualification suite, golden,
live test profiles and quality-manifest entry.

The retained execution record is
[`docs/evidence/EPIC-07.12-connector-qualification.json`](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-07.12-connector-qualification.json); its compressed validation
bundle preserves reports and logs. Source bindings describe the candidate before
adding the evidence record itself. Historical component evidence remains labelled
with its original scope and cannot authorize full release acceptance.
