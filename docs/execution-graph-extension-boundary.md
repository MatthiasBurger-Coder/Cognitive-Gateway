# Execution-Graph Extension Boundary

This document records the CG-04.16 boundary between the deterministic
Process IR v1 lifecycle engine and future S3D-style execution scheduling. The
machine-readable classifications and dispositions are maintained in
[`execution-graph-migration-gaps.json`](execution-graph-migration-gaps.json).

## Authority boundary

The Process IR v1 is authoritative for lifecycle legality:

| Process IR v1 | Future execution-graph extension |
| --- | --- |
| states and terminal states | slice/task dependency DAG |
| events and event occurrences | unknown-dependency and cycle validation |
| transitions and typed guards | topological ordering |
| gates, evidence and invariants | execution groups |
| blockers, waiting and stop conditions | sequential/parallel scheduling decisions |
| bounded retry and recovery | typed locks and lock conflicts |
| authorized capability-first activities | joins, barriers and stream metadata |

An execution graph may enrich an already authorized activity plan, but it may
not make an illegal lifecycle transition legal. The safe order is:

```text
event + lifecycle snapshot
  -> Process IR v1 evaluation
  -> reject or accept lifecycle transition
  -> optional execution-graph evaluation for accepted work
  -> adapter executes the approved plan
  -> evidence/failure event returns through the lifecycle boundary
```

The current compiler emits the explicit
`ExecutionGraphExtension(kind = "execution-graph", supported = false)` seam.
This is a declaration that a future typed extension may attach; it is not a
claim that the catalog or runtime currently schedules a graph. The migrated
process definitions therefore contain no DAG, lock, parallelism, join or
stream fields.

## Neutral evidence fixtures

The fixtures under
[`crates/gateway-process/fixtures/execution-graph/`](../crates/gateway-process/fixtures/execution-graph/)
are provider- and project-neutral input examples. They cover a valid graph
with sequential and parallel groups, topological ordering, an explicit join,
locks, parallelization input, stream metadata and failure routing. Separate
fixtures cover cycles, unknown dependencies and exclusive-lock conflicts.

They are not executable scheduling semantics. Their expected dispositions
make the required future rejection behavior visible without using free text
or an LLM fallback.

## Migration gap decisions

The four concrete CG-04.14/04.15 gaps remain explicit:

- `CG04-GAP-EXECUTION-DAG` is deferred to a typed execution-graph extension.
- `CG04-GAP-LOCKS-PARALLELISM` is unsupported until lock, parallel and stream
  contracts exist.
- `CG04-GAP-JOIN-BARRIER` is deferred to the same typed extension.
- `CG04-GAP-RUNTIME-ORCHESTRATION` is excluded from CG-04 and remains an
  external agent/runtime adapter concern.

Failure routing, gates/evidence and stop conditions are already represented
by Process IR v1 and remain lifecycle inputs. TSW source files, project IDs,
agent identities, runtime handles and concrete platform operations are not
copied into the canonical process catalog or process-core runtime.

Adding scheduling later requires a new typed extension contract and its own
validation/evaluation boundary. It does not require changing the source
language's lifecycle vocabulary, the definition identity/digest, instance
revision rules or atomic lifecycle mutation port.
