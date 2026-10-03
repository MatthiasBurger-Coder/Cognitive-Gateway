# CG-22: experience normalization and pattern inspection

The application pipeline is `gateway_application::experience_patterns::inspect_patterns`.
It reads a bounded, scope-specific snapshot from an `ExperienceIngestionPort` and
rechecks every referenced memory entry through `MemoryApplication`. The daemon
provides `InMemoryVerifiedExecutionSource` as a process-local proof adapter and
`PostgresExperienceStore` as a durable reference adapter. PostgreSQL admission
calls the same normalization/eligibility checks as inspection, stores the
canonical execution and pinned eligibility reference, and rejects conflicting
replays. Its table and index are defined in
[`001_verified_executions.sql`](../crates/gateway-daemon/migrations/001_verified_executions.sql).
Reopening the database recovers the stored references. `PostgresMemoryStore`
persists governed memory entries and curation decisions in the same database,
allowing restart-safe revalidation. Its migration is
[`002_governed_memory.sql`](../crates/gateway-daemon/migrations/002_governed_memory.sql).
It accepts reference-only payloads; the owning store must manage the referenced
content and its erasure.
Production adapters must supply `VerifiedExecution` only after checking the
execution trace and evidence against their authoritative execution store. The
port is a trust boundary; a caller-supplied struct alone cannot prove that a
run was verified.

`ClosedLoop::verified_outcome` issues a typed receipt after `ingest` accepts
correlated observations, finds fact and evidence lineage, and reaches either
an evidence-backed success or a hard failure. A `Completed` runtime status by
itself does not issue a receipt. The receipt has a unique execution snapshot
identity, source digest, verdict, evidence and validation identity. A curator
can use these fields to create and validate an `ExperienceRecord`; its
`label_basis` must name one of the receipt's evidence IDs. Then
`PostgresExperienceStore::record_closed_loop` checks that the current memory
record pins the same snapshot, digest, validation, label and outcome before
persisting the reference. A refreshed or revoked record fails later
revalidation during pattern inspection.

An entry enters correlation only when the governed memory state is validated,
current, learning eligible, and has matching source trace, validation and label references.
The accepted outcome labels are exactly `SUCCESS` and `FAILURE`; an unfamiliar
label stops inspection for that snapshot. The pipeline obtains a current
`MemoryEligibilityReference` and places it with source provenance and the
evaluation reference in each normalized `ExperienceBasis`. Consumers must
revalidate those references again when they use a report.

Fingerprint extraction sorts typed fact, capability and operating-mode
signals. A fact or capability is required. Raw trace text and optional semantic
hints do not enter the fingerprint. The hint is lowercased and whitespace
normalized solely to nominate another group for inspection. Structural near
matches share signals and differ by at most one signal. Neither nomination
establishes applicability. Scope and runtime are separate grouping keys, so
executions from different projects or runtimes cannot jointly support a
candidate. Identical fingerprints in different runtimes are linked for
inspection through `cross_runtime_matches`, without pooling their evidence.

The report pins the inspected scope, time and limits and has one finding per exact group. It retains successful and failed
normalized experience, trace/evidence references, observed outcome counts,
structural near matches and semantic nominations. A `PatternCandidate` is
created only from at least two successful, eligible experiences in the same
group. Its deterministic ID hashes the runtime, canonical fingerprint and
sorted source bases. A candidate has no procedure steps or execution grant.
Failed and contradictory outcomes remain in the finding as negative evidence.

`PatternLimits` bounds input count, group count, signals and evidence per input,
records retained per group and the minimum support threshold. Excess inputs or
groups fail explicitly. A port must enforce these limits before loading or
cloning entries. A group over its limit retains its newest records and at least
its newest failure;
the report exposes pre-sampling success/failure counts and the total sampled
out count. Sampling can prevent a candidate by reducing its retained support.
All ordering and tie breaks are deterministic. Metrics report input, eligible,
rejected, retained, sampled out, group and candidate counts without payloads.

The PostgreSQL adapter has a separate `RetentionPolicy`: default maximum of
10,000 references per scope and 90 days of age. Admission prunes older rows
within its transaction, rejects a full scope, and returns the prune count. It
stores references and optional bounded semantic hints, not execution payloads.
The adapter connects without TLS and is intended for the local Compose service.

`cg patterns --scope <project-scope> [--at <unix-seconds>] [--json]` reads both
PostgreSQL stores and runs current eligibility checks before displaying a
report. It uses the credential file configured for the Compose service.
`cg patterns --report <file-or-json> [--json]` displays a previously generated
report. Both modes are read-only; a report is inspection data, not evidence of
verification or authorization.

The regression corpus in `crates/gateway-daemon/tests/experience_patterns.rs`
covers repeated successes with a contradictory failure, failed-only and sparse
groups, reordered/noisy inputs, semantic-only nominations, project and runtime
separation, eligibility rejection, mismatched validation, duplicate signals,
and resource limits.
The ignored PostgreSQL integration test can be run against the local Compose
service with `./scripts/test-postgres.sh`. It covers database reconnect for
both stores, idempotency, conflicting replays, capacity, negative evidence,
revocation, forget decisions and payload-reference removal.
