# EPIC-07.08 / #231 — connector resilience review

Delivery base: `506ce69b9cf472fc8bec850d4e4b7295137438ad` (current main at
isolation). Review performed by one agent through requirement, architecture,
automation and testing role passes; no independent approval is implied.
Repository governance selects DEVELOPMENT / FULL_PATH without feature freeze.
Pre-implementation readiness: READY_FOR_WORKFLOW.

The original implementation was developed against a dirty integration workspace.
This delivery transfers only #231 changes onto committed main, retains the
current native session fixtures, and requalifies the resulting source. Unrelated
uncommitted EPIC-08 work is excluded. The native legacy SessionConnectorRun
facade remains read-only; mutation uncertainty is preserved at the admitted host
boundary. No schema, persistence migration or credential change is introduced.

| ID | Requirement/source | Production owner | Verification | Status |
| --- | --- | --- | --- | --- |
| R01 | #231 typed timeout, cancellation and rate-limit failures | application failure types, MCP context/protocol | pure decisions and real stdio fault profiles | VERIFIED |
| R02 | #231 bounded safe retries | application RetryPolicy, native SessionConnectorRun | attempt ceiling, exponential delay, exhausted native allocation | VERIFIED |
| R03 | #231 unsafe and unknown mutations never replay | effect status and recovery decisions | real uncertain mutation timeout plus idempotency/reconciliation decision tests | VERIFIED |
| R04 | #231 degraded/unavailable observability | host health and circuit state, Observer | snapshots, counters, real peer failures and circuit events | VERIFIED |
| R05 | #231 process/policy authority survives failure | existing native CG-09 and resource authority checks | current native policy, scope, late revocation and quote regressions | VERIFIED |
| R06 | #231 bounded concurrency/backpressure | one per-host permit and one native spending cursor | 16 concurrent rejected calls, one half-open probe | VERIFIED |
| R07 | #231 rate limits and cooldown | explicit MCP adapter profile, shared host cooldown | bootstrap/discovery/health/tool rate-limit tests and malformed hints | VERIFIED |
| R08 | #276 integration: explicit idempotency/read-back support | ConnectorRecoveryPort and strict canonical mapping | inventory-bound metadata, unsupported read-back, unknown effects remain blocked | VERIFIED |
| R09 | #277 integration: aggregate budgets never reset | existing coordinator allocation, SessionConnectorRun | real native retries/reconnects charge each call/cost; cancellation/revocation during backoff | VERIFIED |

The pure application engine owns decisions. The outer host owns time, protocol,
rate-limit cooldown, circuit state and concurrency. The existing coordinator
retains the non-cloneable spending cursor and durable phase ceiling. Every retry
uses the original request/key, cancellation token and narrowest admitted
monotonic/CG-09 deadline; reconnect/discovery has a separate debit. No second
ledger or process/policy coordinator is introduced.

Read-back metadata grants no dispatch and proves no completion. The canonical
mapper explicitly reports Unsupported until a provider read-back contract is
qualified. Independent reconciliation and goal verification remain #276 owners;
external exactly-once semantics and parent epic completion are not claimed.
Live provider qualification remains #235.

[Source-bound evidence and executed gate results](EPIC-07.08-resilience.json)
record the candidate qualification. Issue closure follows verified PR merge.
