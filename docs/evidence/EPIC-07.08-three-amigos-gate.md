# EPIC-07.08 / #231 — requirement and delivery review

Reviewed on 2026-10-27 against base commit
`742c318b47b1e06ced615618fe7ac0c1d05f71ce` plus the existing dirty workspace.
The workspace already contained EPIC-07.07/08.03 policy, connector dispatch,
resources and session changes. This slice extends those actual interfaces and
does not claim those pre-existing changes as its own. Final evidence binds the
candidate files by SHA-256; the Git base alone does not identify this candidate.

Review provenance: one agent performed requirement engineer, architect,
automation developer and tester role passes. No independent reviewers,
approvals, delegation or live-provider qualification are implied.

## Scope and readiness

The repository's `.agents/AGENTS.md` selects DEVELOPMENT / FULL_PATH with no
feature freeze. The named issue, parent #223 and its 2026-10-04 integration
additions, #276 and #277, ADR-008/021/022, current architecture and the actual
CG-09/session/connector paths define this slice. No hardening admission or
external approval is required. Pre-implementation readiness: READY_FOR_WORKFLOW.

The ordered slice is: pure application retry/recovery contract; outer host
availability/circuit/rate-limit enforcement; session-owned attempt/deadline and
aggregate accounting; deterministic and real subprocess failure qualification;
coverage and quality/evidence audit. Existing policy/session foundations are
present in the working candidate. The implementation does not introduce a second
policy service, session journal, resource ledger or provider SDK. Rollback removes
the new retry/circuit wiring and recovery port; no schema or database migration
is required. The original single-attempt host interface is retained.

## Requirement trace

| ID | Requirement/source | Production owner/path | Dependencies and actual state | Implementation evidence | Required verification | Status |
| --- | --- | --- | --- | --- | --- | --- |
| R01 | #231 typed failures, timeouts and cancellation | application failure vocabulary; MCP protocol/context | Existing typed port and context retained | distinct rate-limit projection and shared context | pure decisions, real timeout/cancel/rate simulations | VERIFIED |
| R02 | #231 retries bounded | application RetryPolicy; SessionConnectorRun | Existing non-cloneable allocation reused | 1–32 attempts; exponential capped delay; no retry without mapping support | decision table; persistent failure and rate-limit tests | VERIFIED |
| R03 | #231 unsafe/uncertain mutations never blindly replay | recovery decisions and session terminal state | Existing effect-status and journal ownership retained | Unknown/Acknowledged blocked even with provider keys/idempotency | real peer ambiguous mutation and repeated-call denial | VERIFIED |
| R04 | #231 health/degraded/unavailable observable | outer Resilience and host Observer | Existing nonblocking observer contract | snapshots, failure streak, circuit/cooldown, bounded counters | deterministic circuit tests; real peer failure/events | VERIFIED |
| R05 | #231 connector failures cannot corrupt process/policy | CG-09 admission and narrow outer connector state | Existing current dispatch and scope guards retained | each attempt reauthorizes/revalidates; no process/policy write handle | existing denied/late-revoked policy, scope, quote and post-effect tests | VERIFIED |
| R06 | #231 concurrency/backpressure explicit | per-host permit and session spending mutex | Existing private session mutex retained | single operation and half-open probe; no queue; rejection metrics | 16 concurrent real-peer rejections and deterministic half-open rejection | VERIFIED |
| R07 | #231 rate limits | MCP adapter profile and host-wide cooldown | Provider profile -32029 explicitly documented | typed delay lower bound; malformed hints rejected; all lifecycle paths covered | protocol profile and actual subprocess simulations | VERIFIED |
| R08 | #276 integration addition: explicit provider idempotency/read-back | application ConnectorRecoveryPort; strict canonical mapper | Durable effect reconciliation remains #276 owner | keyed argument validation; current read-only read-back or Unsupported | inventory/key/read-back mismatch tests; Unknown remains terminal | VERIFIED |
| R09 | #277 integration addition: remaining aggregate budgets never reset | SessionConnectorRun, existing allocation/authority | Existing persisted phase ceiling and costs reused | each attempt and recovery discovery charged; one deadline/token; monotonic retries | exhausted slice, reconnect accounting, cancellation and revocation during wait | VERIFIED |

The exact commands, executed statuses, source hashes and coverage are retained in
[the qualification report](EPIC-07.08-resilience.json). A passing component test
is not a passing workspace quality gate. The report distinguishes the implemented
slice from unrelated baseline failures and external/live qualification.

## Review findings resolved in this slice

- Recovery metadata lookup must not repeat policy authorization/audit or execute
  mapping effect hooks. It is an inventory-bound metadata lookup; each actual
  attempt retains the normal authoritative path.
- Rate-limit cooldown must cover connection bootstrap, discovery and health as
  well as tool execution. Unknown mutation outcomes still block after a typed
  rate-limit response.
- Retrying after transport loss requires a separately charged reconnect/discovery
  through the same cursor. The request revision cannot silently migrate.
- The first/narrowest admitted CG-09 deadline is retained across the entire
  retry sequence, including when a fresh authorization could otherwise renew it.
- Permission revocation and cancellation during backoff must stop before another
  reservation or wire send. Delay waits check current allocation authority.
- Breaker recovery never establishes goal completion. An unsupported read-back
  contract remains explicitly blocked; provider idempotency does not resolve an
  unknown outcome.

## Qualification boundary

Synthetic independent stdio peers exercise real subprocess framing, transport,
policy/session adapters and accounting. These are executable component paths,
not a claim of GitHub service availability, installed-client qualification,
durable external exactly-once semantics or completion of parents #223/#271.
#235 owns live connector qualification; #276 owns journal reconciliation; #277
owns wider aggregate session and no-progress behavior.
