# Connector resilience — EPIC-07.08 (#231)

The application owns recovery decisions in `connector_resilience`; the daemon
owns time, cancellation, concurrency and circuit state in `mcp_client`.
The existing CG-09 admission, normalization and non-cloneable session allocation
remain the effect and accounting boundaries. No provider failure edits policy,
process or catalogs.

## Failure and recovery contract

`ConnectorFailure` retains the content-free code, correlation and effect status.
Timeout, cancellation, rate limit, unavailable provider, backpressure, permission,
authentication, schema and protocol failures remain distinguishable. Partial
mutation writes, lost responses and post-effect failures retain `Unknown`.

`ConnectorRecoveryPort` exposes current inventory-bound `RecoverySupport` to
session/CG-14 goal consumers: retry safety, unsupported/idempotent/provider-key
idempotency, and unsupported or explicitly mapped read-back capability. Provider
keys identify a configured argument, and the host requires a nonempty string
value. Every retry uses the same immutable request, including that key. Metadata
lookups perform no transport or dispatch authorization. They cannot grant access.
The canonical mapper derives metadata from strict operator mappings and the exact
inventory revision. Its read-back support is explicitly `Unsupported` until a
provider read-back contract is qualified. Another trusted mapping may expose a
read-back capability only if the current inventory classifies it as read-only.

`RetryPolicy` is a pure decision engine. Defaults allow three attempts including
the original operation, with 50ms exponential backoff capped at two seconds.
Configuration permits 1–32 attempts and backoff up to 60 seconds. Only unavailable,
timeout, rate-limit and backpressure failures are candidates. Explicit mapping
retry safety is required. Acknowledged effects never replay. Unknown effects
always require reconciliation, including operations declared idempotent or keyed.
Unsupported reconciliation leaves them blocked; the retry engine never claims
that a read-back capability or provider acknowledgement proves completion.
A separate admitted read and the existing goal verifier must establish the
original effect. Reconciliation neither refunds the original spend nor authorizes
replaying the mutation.

## Runtime and aggregate budgets

A raw `McpConnectorHost` call performs one attempt. `SessionConnectorRun` invokes
the retry engine on the delivered connector port, defaulting to no retries for
mappings without explicit support. `invoke_with_retry` selects a bounded policy;
it cannot enlarge the coordinator allocation or operation deadline.

One run holds its spending lock throughout attempts and backoff. Every attempt
reauthorizes and revalidates current CG-09 facts and reserves a full quote against
the original allocation, including calls, retrieval resources and cost. No refund
or counter reset occurs after failure, cancellation or reconnect. Retry counts
are monotonic within the run; aggregate limits remain in the existing retained
session reservation, so creating a policy cannot reset them. Reconnecting and
rediscovering after transport unavailability is separately charged and must keep
the request's inventory revision valid. A discovery failure terminates the retry
sequence. Existing session persistence conservatively retains the phase ceiling;
this change introduces no second ledger or migration.

All attempts share the cancellation token and the minimum of the allocation,
caller and host operation deadlines; the narrowest admitted CG-09 dispatch deadline is retained across the entire
sequence. Fresh authorization cannot extend it.
Provider delays are lower bounds, including when longer than exponential backoff.
If the delay cannot fit, the original failure is returned immediately. Waits
check cancellation/current authority at intervals of at most five milliseconds.
Unknown mutation outcomes make the entire run terminal, including subsequent
ordinary invocations. A new independent recovery phase still uses its own
coordinator-reserved slice and the existing goal verification contracts.

## Rate-limit adapter profile

MCP does not provide a universal rate-limit code for every provider. This host
recognizes the explicit Gateway adapter profile JSON-RPC error code `-32029`,
with optional `error.data.retryAfterMs` (integer 0–3600000; default 1000ms).
Provider adapters must translate their typed rate limits to this profile. Other
codes stay protocol failures; provider prose and secret-bearing error data never
become application diagnostics. Invalid delay values fail as protocol errors.
A rate-limited call retains its ready transport and inventory, but a shared host
cooldown prevents further wire dispatch until the delay expires. Mutations still
retain `Unknown` and cannot retry even when the provider reports a rate limit.

## Availability, concurrency and observability

Every host permits one in-flight connector operation, with no waiting queue.
Concurrent calls fail immediately with `Backpressure/NotDispatched`. The bound
also covers connect, discovery and health probes; operator disable/shutdown
remain available to cancel and drain a busy or open-circuit host. This is a
per-host limit; hosts remain scoped to their configured target. The existing
session allowance separately rejects concurrent/reentrant spending.

`ResilienceConfig` defaults to five consecutive dependency failures and a
30-second open interval. Failed provider negotiation, transport, timeout,
protocol, schema and rate-limit operations degrade health and count toward the
breaker. Permission denials, cancellation and resource exhaustion do not establish
provider failure. Open circuits reject without provider I/O. After the interval,
one operation receives a half-open permit. Success closes and resets the failure
streak; a failed or denied probe reopens. Dropping a permit during unwinding
releases the concurrency slot and counts an availability failure. Host-local
breaker state is ephemeral; restarting does not reset retained session budgets.

`health_snapshot()` performs no I/O. It reports health, circuit, occupancy,
remaining cooldown, attempts, successes, failures, rejections and failure streak.
A healthy initial circuit is not proof of connection readiness: lifecycle state
retains that meaning. The existing bounded nonblocking `Observer` receives
content-free health snapshots and retry decisions, alongside protocol lifecycle
and request events. Counters saturate instead of wrapping. Observer implementations
must obey the existing nonblocking/non-panicking contract and bound retention.

## Qualification

Tests exercise pure retry decisions, provider metadata, circuit transitions and
single half-open admission. Real subprocess conformance tests cover rate limits,
malformed rate metadata, uncertain mutation outcomes, bounded retries, retained
aggregate spending, cancellation during backoff, persistent failures and concurrent
load rejection. These are independent synthetic-peer/component proofs; live
provider and installed-client qualification remain EPIC-07.12 (#235).

Run:

```sh
cargo test -p gateway-application --test connector_resilience --locked
cargo test -p gateway-daemon --lib mcp_client:: --locked
cargo test -p gateway-daemon --test mcp_client --locked
```

The [source-bound qualification report](evidence/EPIC-07.08-resilience.json)
retains the measured coverage, subprocess evidence and outstanding workspace
quality failures. It does not authorize issue closure while those gates fail.
