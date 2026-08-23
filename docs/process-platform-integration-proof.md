# CG-04.17 Process Platform Integration Proof

The CG-04.17 integration proof exercises the Rust-only process platform from
the Git-owned catalog through an atomic runtime transition. The executable
proof is in
[`crates/gateway-process/tests/process_platform_integration.rs`](../crates/gateway-process/tests/process_platform_integration.rs).

## Vertical path

```text
catalog/processes/*.feature
  -> SourceDocument frontend
  -> SemanticCompiler
  -> canonical Process IR v1 + digest
  -> ProcessValidator + ProcessRegistry
  -> pinned ProcessInstance
  -> EventOccurrence + typed inputs
  -> TransitionEvaluator
  -> AtomicProcessMutation
  -> idempotent state/history/evidence outcome
```

The representative path uses `implementation-lifecycle` and reaches
`COMPLETE` through `ANALYZE`, `THREE_AMIGOS`, `IMPLEMENT`, `VERIFY`,
`ARCHITECTURE_REVIEW`, `E2E` and `EVIDENCE`. Repair, blocker, retry, pause,
authorization, evidence, gate and terminal paths are exercised with the same
typed contracts.

## Required behavior covered

| Proof area | Integration evidence |
| --- | --- |
| deterministic catalog and compiler | repeated registry snapshots, compiler results and canonical JSON are equal |
| identity and pinning | duplicate ID/version, changed digest and wrong definition fail closed |
| lifecycle legality | illegal transitions, unknown events and terminal events are rejected |
| constraints | missing evidence, failed gates, active blockers and authorization waiting/denial are typed decisions |
| mutation safety | duplicate occurrence delivery is a no-op, stale revisions reject, failed commits leave no state or occurrence behind |
| recovery | pause/resume, bounded retry, exhaustion and declared repair transitions are explicit |
| explainability | simulation equals direct evaluation; compilation and runtime explanations are machine-readable and human-readable |
| migration boundary | migrated definitions register deterministically and retain an unsupported `execution-graph` extension seam |

## Trusted-core boundary

`gateway-process` production dependencies are limited to the local
`gateway-domain` contract and Rust ecosystem serialization/digest crates. The
process compiler, validator, registry, instance model, evaluator and mutation
port do not invoke Python, JVM/Java/Kotlin, Node.js, Cucumber, BPMN, an
external workflow engine, an LLM, RAG or a provider runtime. The architecture
guard and CI quality workflow verify the dependency direction.

Agents and tools receive authorized activity projections; they do not receive
an arbitrary state mutation API. Authoritative state changes require a
validated transition projection and the atomic mutation port.

S3D dependency, scheduling, lock, parallelization and join semantics remain
explicitly classified at the boundary documented in
[`execution-graph-extension-boundary.md`](execution-graph-extension-boundary.md).
