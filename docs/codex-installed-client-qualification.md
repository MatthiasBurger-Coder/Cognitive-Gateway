# Installed Codex interoperability — EPIC-04.14

The installed `codex-cli 0.162.1` qualifies the delivered no-key path for
inspection, canonical resolve/explain/context and the registered shared
structured context-artifact lifecycle. The real client discovers all nineteen
tools, invokes all six v2 session operations, and reads the independently
verified final evidence through its local app-server API. Observed MCP identity
is `codex-mcp-client / 0.162.1`; requested and negotiated protocol versions are
`2025-06-18`.

## Reproduce against the candidate

```sh
cargo build -p gateway-daemon --bin cg --bin cg-mcp --bin cg-local --locked
python3 scripts/cognitive-test-host.py python3 scripts/qualify-installed-codex.py \
  --shared-session-fixture --output target/installed-codex-shared
```

The output directory must be new. This command requires an installed Codex,
Linux procfs, Docker and the Python dependencies used by the executable session
fixtures (`jsonschema`). The wrapper supplies a disposable loopback PostgreSQL
host and removes it afterwards. Provider credentials are unnecessary. The
PostgreSQL connection file is trusted CG storage configuration; its contents
are excluded from retained evidence.

Use `--canonical-fixture` for the narrower canonical check or omit both fixture
options for inspection only. `--codex` and `--bin-dir` pin executables.
The runner uses isolated HOME/CODEX_HOME settings, private stdio, ephemeral
read-only threads, and empty CG environments. It starts no inference turn,
reads no user Codex configuration and transfers no provider authentication.
A forwarding observer records initialize identity, negotiation and discovery
field shapes, forwarding protocol frames unchanged to the real host. The trusted
launch pins the expected identity before observing it. Wrong identity/version
checks connect the installed client directly to CG so rejection is exercised
by the shipped host, rather than by the observer.

The installed binary's generated app-server JSON schemas are inspected for
`mcpServer/tool/call`, `mcpServer/resource/read` and `mcpServerStatus/list` before
use, with schema hashes retained. No model or paid inference API is needed.
Codex's app-server tool projection omits MCP execution metadata; the runner
checks tool names, annotations and operation schemas, and reads both complete
frozen catalog resources for exact equality.

## Dependencies and review

Four sequential role passes were performed by one agent, without independent
reviewer or approval claims. Requirements: qualify actual invocation, including
sessions, while preserving the structured-task scope. Architecture: ADR-020 and
ADR-021 place credentials outside the inbound boundary and lifecycle authority
in shared application/storage services. Automation: extend the existing probe
and reuse only admitted input preparation from the executable fixtures. Testing:
require actual installed-client transitions, CLI parity, refusal paths, restart,
EOF cleanup and revision-bound artifacts before claiming this slice qualified.

Decision: READY_FOR_WORKFLOW at baseline `8a33f48`. Canonical host #292 and shared
services #294 are implemented in the candidate and have executable/runtime
qualification. The probe calls `cg-mcp` through the installed client; it does not
substitute an application host or invent session projections. Neutral pinned
plan/rules/process/policy/projection/catalog records and two source alternatives
replace operator inputs. Disposable PostgreSQL replaces operator storage. Trusted
`cg-local session.authority` issues consent; client approval never grants it.

## Acceptance evidence

Retained [report](evidence/EPIC-04.14-installed-codex/report.json),
[initialize observation](evidence/EPIC-04.14-installed-codex/mcp-initialize.json),
[compressed RPC transcript](evidence/EPIC-04.14-installed-codex/rpc-evidence.json.gz)
and [verified receipt](evidence/EPIC-04.14-installed-codex/verified-evidence.json)
bind the result to the Git baseline, dirty-candidate flag, source/manifest hashes,
launcher and running native binary hashes, CG executable hashes, commands,
protocol, requests/results and artifact hashes. Source, revision and executable
hashes are checked again after cleanup. Inspect the transcript with
`gzip -dc docs/evidence/EPIC-04.14-installed-codex/rpc-evidence.json.gz`.

| ID | Requirement | Installed-client evidence | Status |
| --- | --- | --- | --- |
| IC-01 | Exact binary, initialize identity and negotiation | npm launcher and running native executable hashes; wire identity/version; requested and negotiated protocol | VERIFIED |
| IC-02 | Frozen discovery and supported application invocation | Exact v1/v2 catalog resources, nineteen tool names and schemas; inspection and canonical full-envelope CLI parity | VERIFIED |
| IC-03 | Resolve/explain/context and supported shared lifecycle | All canonical operations; start/inspect/clarify/approve/continue/cancel through the actual client; CLI inspection envelope parity and independently verified evidence receipt | VERIFIED |
| IC-04 | Private stdio and no provider auth in CG | Isolated HOME/CODEX_HOME, `env -i`, empty host environments; no inference turn or user configuration write | VERIFIED |
| IC-05 | Wrong admission, denied/error paths, EOF and bounded cleanup | Host rejects wrong name/version; forged consent, wrong pending identity, replay, stale revision, foreign scope, current-policy block, terminal continuation and unrelated goal refused; restart preserves pending/terminal state and budgets; explicit cancellation distinct from EOF | VERIFIED |
| IC-06 | Revision-bound artifacts and precise scope | Source/binary/manifest/API/transcript hashes; substitutions and commands; structured-task receipt only | VERIFIED |
| IC-07 | Automated reproducibility and unavailable prerequisites | Runner guardrails require NOT_RUN for an absent client and BLOCKED for absent shared storage; nonzero exits; no successful fallback | VERIFIED |

Successful full-slice status is `QUALIFIED_INSTALLED_SHARED_SESSIONS`. Narrower
runs report `QUALIFIED_INSTALLED_CANONICAL` or `QUALIFIED_INSTALLED_INSPECTION`.
Missing client or prerequisite access yields NOT_RUN/BLOCKED; failed behavior
checks yield FAIL. Forced or unsuccessful EOF cleanup prevents qualification.

The receipt establishes the registered verified context-artifact goal. It does
not establish the unrelated desired state used to construct the admitted plan.
Crash/commit ambiguity, competing ownership, expiry and storage corruption have
separate shared-runtime evidence in #294; this probe qualifies installed-client
interoperability, normal EOF/restart and the exercised refusal paths.
Successful modes now retain `epic_04_status: NOT_ASSESSED` and
`closure_allowed: false`; unsuccessful modes retain `NOT_COMPLETE`. This
2026-10-11 producer-contract correction leaves historical reports unchanged.
Parent reconciliation #296 alone may assess all 24 EPIC-04 criteria; a fresh
full run is required after the correction. Complete EPIC-08 model/connector/system
acceptance #279 remains separate. Discovery
alone is never model/tool-proposal or full-runtime evidence.

Validation for this candidate: 23 architecture tests (including four runner
guardrails), 12 frozen contract tests and all 22 local CLI/MCP integration tests
passed; the latter include actual PostgreSQL lifecycle/recovery scenarios.
Candidate build, architecture dependency guard, `cargo fmt --check`, independent
local MCP protocol/schema conformance and `git diff --check` passed. Retained
artifact and participating source hashes were independently recomputed, and the
RPC transcript was checked for all six session tool invocations and exclusion
of provider/database credential values. No Rust production file changed in this
slice; the shared-runtime production coverage gate remains the separate #294
evidence linked above.
