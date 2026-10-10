# Full EPIC-04 acceptance — #296

EPIC-04 #126 and the broad #245 definition remain **NOT_COMPLETE**. A green
component, shared-runtime or installed-client result does not authorize their
closure. #296 adds enforcement and reconciliation; it does not remove any
parent requirement or claim #279 whole-runtime acceptance.

## Review and delivered boundary

Four sequential role passes were performed by one agent; no independent
reviewer or approval is claimed. Repository mode is DEVELOPMENT / FULL_PATH.
Requirements: preserve all nineteen original criteria and five 2026-10-04
additions. Architecture: `cg-mcp` and `cg-local` use the existing canonical host,
facade and EPIC-08 shared coordinator, trusted interaction authority and
PostgreSQL journal. Automation: require the full declarative quality manifest,
canonical executable component suite and actual installed Codex shared-session
probe. Testing: missing, stale, contradictory or unexecuted evidence must deny
closure, including green narrower reports.

The exact registered context-artifact task is delivered; semantic interpretation,
model invocation and external connector effects remain explicitly unsupported.
These are capability boundaries, not proof of the separate #279 system scope.
#292/#294/#295 are implemented and have retained scoped evidence. Their closed
issue states are not inputs to acceptance. The current retained reports are
historical candidate evidence; full current-revision qualification is absent and
the installed/component producers explicitly retain `epic_04_status: NOT_COMPLETE`.

Decision: READY_FOR_WORKFLOW for the acceptance enforcement slice; parent
completion remains BLOCKED on fresh, consistent full-scope evidence. No extra
production plane or second coordinator is added.

## Reproduce and interpret

```sh
python3 scripts/qualify-epic04.py --run --output target/epic04-acceptance
```

Use a new output directory outside tracked source. This executes the complete
repository quality runner (including canonical executable tests, security/failure
checks, architecture and coverage, actual shared-service/PostgreSQL qualification)
and then the installed-client shared-session probe. It retains individual logs
and all 24 requirement decisions. Missing Docker, database, client or toolchain
access is unsuccessful; an absent report never becomes a successful fallback.

To reconcile already executed reports without rerunning them:

```sh
python3 scripts/qualify-epic04.py --output target/epic04-review \
  --quality target/epic04-acceptance/quality/summary.json \
  --component target/epic04-acceptance/quality/epic04-component/report.json \
  --installed target/epic04-acceptance/installed/report.json
```

Only `QUALIFIED_EPIC_04`, `epic_04_status: COMPLETE` and `closure_allowed: true`
return zero. Every mandatory input must bind revision, participating sources,
artifacts and required executable identities to the candidate. All current
quality commands, changed-file coverage and actual shared transitions are required;
inspection-only/canonical-only client runs are insufficient. Any mandatory report
retaining EPIC-04 NOT_COMPLETE blocks closure even when its scoped tests pass.
This is intentionally stricter than the component runner: current producers must
undergo a separate full-scope review before any completion declaration changes.
Do not edit a report's status to work around this gate. Source-bound proof must
be regenerated when qualifying sources change.

The PR workflow recognizes `Closes`, `Fixes` and `Resolves` claims for #126 or
#245 (including lists and repository URLs) and runs full acceptance for such
claims. Other PRs retain their ordinary component gates. Making this workflow a
required branch check is repository administration outside this local change;
manual GitHub closure is not intercepted. No issue status is changed by the
runner. #296's implementation closure is distinct from parent product closure.

## Stable requirement matrix

The executable manifest is `scripts/epic04-acceptance.json`. Its 24 stable IDs
preserve issue order; each contains the production owner/path, dependency state,
automated test path, qualification command and required evidence levels. The
runner fills VERIFIED/BLOCKED from actual input validation, never child states.

| ID | Parent requirement | Production path | Required evidence |
| --- | --- | --- | --- |
| E04-01 | Supported local connection without CG API key | cg-mcp -> LocalCodexHost -> CodexFacade | COMPONENT, EXECUTABLE, INSTALLED_CLIENT |
| E04-02 | Authentication outside canonical authority | local MCP admission/credential environment guard | COMPONENT, EXECUTABLE, INSTALLED_CLIENT |
| E04-03 | No provider/MCP types in authoritative contracts | architecture dependency and local MCP boundary guards | ARCHITECTURE, COVERAGE, REAL_POSTGRESQL, SHARED_SERVICE |
| E04-04 | Explicit workspace/project/session scope | WorkspaceResolver -> CodexFacade -> session admission | COMPONENT, EXECUTABLE, INSTALLED_CLIENT |
| E04-05 | Equivalent canonical results | canonical mapper -> existing resolver/explain/context services | COMPONENT, EXECUTABLE, INSTALLED_CLIENT |
| E04-06 | Provenance/revision/sensitivity/evidence lineage preserved | canonical references and session verified receipt resource | COMPONENT, EXECUTABLE, INSTALLED_CLIENT |
| E04-07 | Current CG policy/consent authorizes every operation | CodexFacade authorization -> shared trusted interaction authority | ARCHITECTURE, COMPONENT, COVERAGE, EXECUTABLE, INSTALLED_CLIENT, REAL_POSTGRESQL, SHARED_SERVICE |
| E04-08 | Discovery never grants authorization | catalog projection -> separate facade admission | COMPONENT, EXECUTABLE, INSTALLED_CLIENT |
| E04-09 | Mutation/admin denied without explicit authorization | facade policy -> registered typed session commands | COMPONENT, EXECUTABLE, INSTALLED_CLIENT |
| E04-10 | Client cannot grant capabilities/policy/process authority | CG-owned authority issuer and strict request mapping | COMPONENT, EXECUTABLE, INSTALLED_CLIENT |
| E04-11 | No secret/reference-only/provider leakage | launch guard -> sensitivity projection -> fixed diagnostics | COMPONENT, EXECUTABLE, INSTALLED_CLIENT |
| E04-12 | Unsupported protocol/schema versions fail closed | MCP initialize admission and frozen v1/v2 validators | COMPONENT, EXECUTABLE, INSTALLED_CLIENT |
| E04-13 | Cancellation/timeout/disconnect/malformed/overload bounded | bounded stdio runtime -> shared cancel/journal recovery | ARCHITECTURE, COMPONENT, COVERAGE, EXECUTABLE, INSTALLED_CLIENT, REAL_POSTGRESQL, SHARED_SERVICE |
| E04-14 | Existing application services reused | LocalCodexHost -> existing canonical services/shared coordinator | ARCHITECTURE, COMPONENT, COVERAGE, EXECUTABLE, INSTALLED_CLIENT, REAL_POSTGRESQL, SHARED_SERVICE |
| E04-15 | No duplicate EPIC-07 external connectors | inbound adapter; connector goals explicitly unsupported | ARCHITECTURE, COVERAGE, INSTALLED_CLIENT, REAL_POSTGRESQL, SHARED_SERVICE |
| E04-16 | CLI/MCP use same facade | cg-local and cg-mcp -> CodexFacade/LocalCodexHost | COMPONENT, EXECUTABLE, INSTALLED_CLIENT |
| E04-17 | Architecture dependency checks green | declarative quality manifest -> architecture guards | ARCHITECTURE, COVERAGE, REAL_POSTGRESQL, SHARED_SERVICE |
| E04-18 | Applicable changed production coverage >=95% | local MCP and shared runtime per-file coverage gates | ARCHITECTURE, COMPONENT, COVERAGE, EXECUTABLE, REAL_POSTGRESQL, SHARED_SERVICE |
| E04-19 | Every acceptance criterion has reproducible evidence | full acceptance reconciler -> source/artifact-bound reports | ARCHITECTURE, COMPONENT, COVERAGE, EXECUTABLE, INSTALLED_CLIENT, REAL_POSTGRESQL, SHARED_SERVICE |
| E04-20 | One-shot/session distinction, correlated pending/status/final references | v2 session projection -> shared coordinator -> verified receipt | ARCHITECTURE, COVERAGE, INSTALLED_CLIENT, REAL_POSTGRESQL, SHARED_SERVICE |
| E04-21 | Shared start/inspect/continue/cancel API; no second coordinator | cg-local/cg-mcp -> EPIC-08 shared application coordinator | ARCHITECTURE, COVERAGE, INSTALLED_CLIENT, REAL_POSTGRESQL, SHARED_SERVICE |
| E04-22 | Disconnect differs from cancel; resume revalidates scope/policy/consent | shared journal recovery and revision-bound interaction authority | ARCHITECTURE, COVERAGE, INSTALLED_CLIENT, REAL_POSTGRESQL, SHARED_SERVICE |
| E04-23 | Missing EPIC-05/06/07 features explicitly unsupported | registered context-artifact goal admission; unsupported capability response | ARCHITECTURE, COVERAGE, INSTALLED_CLIENT, REAL_POSTGRESQL, SHARED_SERVICE |
| E04-24 | Component evidence links separate #279 whole-session qualification | scoped reports -> full acceptance gate; #279 separate | ARCHITECTURE, COMPONENT, COVERAGE, EXECUTABLE, INSTALLED_CLIENT, REAL_POSTGRESQL, SHARED_SERVICE |

## #245 and #126 re-evaluation

#245 is currently closed, but its complete text includes later shared-session
acceptance and a full-parent evidence requirement. The retained component report
proves its named narrower scope; it does not prove that broad definition of done.
#126 is open and remains NOT_COMPLETE. Preserve both original and added criteria;
this local review does not silently narrow #245 or change external issue status.
The closure guard covers both broad acceptance scopes. #279 remains separate.

Rollback removes the acceptance runner/manifest and closure workflow; runtime
state and storage schemas are unaffected. Such removal also removes closure
enforcement and must not be represented as successful acceptance.
