# CG-29 distributed cognitive worker fabric

[Issue #220](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/220)
provides versioned advisory work contracts, scheduler/worker ports, a bounded
reference coordinator and a local worker adapter. Transport and orchestration
stay outside the domain and application boundaries.

## Contract and authority

`gateway-domain::worker_fabric::WorkItem` owns a private immutable `WorkSpec`.
The SHA-256 identity covers the canonical serialized specification: version,
project scope, operation identity, trace, work kind, exact snapshot bytes,
ordered source revisions, runtime/model revision and every budget. Retransmitting
the same specification has the same identity. An intentional second operation
uses a new operation reference. A separate snapshot digest survives retries.
Deserialization grants no trust: every scheduler/worker admission validates both
digests and the bounded contract. Sources/runtime/model references must identify
immutable revisions; the host resolves and authenticates those revisions before
submission. Workers consume copied bytes rather than fetching changing inputs.

Supported kinds are retrieval, pattern inspection, evaluation and model work.
Handlers return opaque proposals with scope, trace, fencing token, attempt and
source/model/runtime/node provenance. There are no approve, promote, Process or
Policy mutation commands in this protocol. A completed result is advisory;
the existing retrieval, experience, evaluation and model services must validate
its contents and recheck current policy/source eligibility before using it.
The fabric does not automatically install learned procedures or model releases.

Authenticated coordinator composition owns submission and inspection. Host
adapters bind each worker advertisement to a verified identity and project;
caller-supplied scope strings are not authentication. The local adapter is
dedicated to one scope. Its handler receives only the immutable item. Provision
scoped read-only dependencies and do not mount authoritative stores or lifecycle
credentials into worker containers. Transport provenance assertions require an
authenticated worker connection; matching fields alone do not establish trust.

## Scheduling, leases and bounded resources

`CognitiveSchedulerPort` and `CognitiveWorkerPort` are application ports.
`ReferenceScheduler` serializes operations with mutable ownership and picks the
first eligible work identity in deterministic order. Selection matches project,
work kind, exact runtime and optional exact model. Global concurrency, worker
slots and aggregate worker memory/compute reservations bound dispatch. Submission
also caps attempts, lease duration, lifetime, per-work memory and cumulative
compute reservations. Each attempt reserves its entire compute allowance even
if its node disappears without telemetry. At most 64 attempts, 64 source
references, 1 MiB input and 1 MiB result are allowed by the wire contract; host
configuration may impose smaller limits.

A claim creates a unique monotonically increasing fencing token and a lease
bounded by the absolute work deadline. Explicit failure or lease expiration
requeues work after a bounded delay, or records terminal budget/deadline failure.
Recovery runs before claim/completion/failure and can be invoked on a host timer.
Expired attempts cannot commit or fail a newer assignment. Identical accepted
results return `false` on duplicate completion; conflicting results fail.
Completion binds every provenance field and rejects oversized or over-budget
proposals. The coordinator supplies monotonic milliseconds and measures
completion time; workers cannot extend their own lease with telemetry.

The in-memory reference limits both global and per-project retained records,
including completed/failed deduplication tombstones. Capacity exhaustion returns
explicit backpressure and increments metrics. Records are never silently evicted:
this preserves idempotency throughout this scheduler instance's lifetime and
bounds retained memory. A full instance needs a deliberate host lifecycle change;
it is not a continuously draining production queue. Inspection retains scope,
trace, snapshot, attempts, reserved compute, last failure and accepted result.
Metrics expose submissions, duplicate delivery, capacity refusal, dispatch,
recovery, failure, invalid result and completion counts.

## Local adapter and orchestrator extension

`gateway-daemon::worker_fabric::LocalCognitiveWorker` runs an injected scoped
handler through the same contracts. `snapshot_probe` is a deterministic diagnostic
handler for all four work kinds. `dispatch_one` claims, executes and records the
proposal/failure using a host clock. Production handlers can wrap the existing
read-only retrieval/pattern/evaluation/model adapters without changing the core.

The reference is synchronous and memory-only: it does not preempt a hung Rust
handler, enforce operating-system memory/CPU quotas or survive coordinator
restart. Lease expiry fences late results, but cannot stop remote computation.
For production local containers, Swarm or Kubernetes, implement these ports with:

1. Authenticated transport, bounded message decoding, dedicated project queues,
   scoped storage/network access and runtime/model artifact verification.
2. A durable transaction for admission, claim, retry, fencing and completion;
   retain idempotency records across restart and ensure unique fencing tokens.
3. Node health recovery, periodic deadline/lease scans and cooperative cancellation
   plus process/container termination for expired attempts.
4. Container CPU/memory limits consistent with reservations, bounded transport
   buffers and backpressure propagated to the coordinator.
5. Persisted correlated status/metrics and an outbox or equivalent transaction
   when an authoritative coordinator consumes a proposal. An external side effect
   needs its own idempotency key and authority check.

Worker pods/services may scale independently. Swarm service IDs or Kubernetes
Pod/Job types stay inside their adapters. Changing orchestrators does not change
work identity or move lifecycle/policy authority into workers.

## Acceptance evidence

`crates/gateway-domain/tests/worker_fabric.rs` proves stable immutable identity,
transport round-trip/tamper rejection and all invalid contract bounds.
`crates/gateway-daemon/tests/worker_fabric.rs` exercises actual ports and the local
adapter. It injects duplicate/conflicting delivery, lost nodes, timeout, stale
results/failure tokens, explicit failure, retry exhaustion, delayed retry,
slow completion, forged provenance, wrong scope, unsupported runtime/model,
resource exhaustion, terminal deduplication capacity and clock regression.
All four kinds round-trip as proposals without an authority dependency.

```sh
cargo test -p gateway-domain -p gateway-daemon --test worker_fabric --locked
CG29_FABRIC_OUTPUT="$PWD/target/cg29-fabric.json" \
  cargo test -p gateway-daemon --test worker_fabric --locked
cargo llvm-cov -p gateway-domain -p gateway-application -p gateway-daemon \
  --test worker_fabric --json --output-path target/cg29-coverage.json
python3 scripts/check-cg29-coverage.py target/cg29-coverage.json
```

The existing full quality gate includes CG-29 correlated recovery/deduplication
evidence and a 95% line coverage gate for all three production modules using the
workspace coverage artifact. The new durable and isolated adapters below additionally prove restart recovery
and resource enforcement; orchestrator-specific transport remains replaceable.

## Durable and isolated adapters

`DurableScheduler` now supplies scoped PostgreSQL transactions and journal replay,
including fencing and idempotency history across coordinator restart. Separate
coordinators contend under row locks. `ProcessCognitiveWorker` terminates attempts
under Linux wall/CPU/address-space/output limits. The container reference adds
unprivileged execution, no network/credentials/mounts and hard resource bounds.
See [complete EPIC-03 acceptance](epic-03-complete-acceptance.md) for APIs, tests,
bounded retention and the qualified deployment scope.
