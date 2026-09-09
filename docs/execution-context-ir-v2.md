# ExecutionContextIR v2 (CG-02 extension)

CG-08 produces valid resolution results that CG-02 `ExecutionContextIR` v1
cannot represent: no workflow template, an empty effective Skill closure,
multiple Agent responsibilities or intrinsic constraints without a v1 field.
CG-02 v2 introduces `ExecutionContextIRV2`, a versioned handoff envelope for
those shapes.

The v2 envelope is lossless at the handoff boundary. It carries optional
workflow, primary Agent, policy and runtime identities, participating Agents,
the complete Skill/capability/constraint references, the immutable resolution
basis and a typed status/issue list. It does not grant execution.

`EXECUTABLE` is the only status eligible for adaptation to v1. It requires a
workflow, exactly one primary Agent, a non-empty Skill closure, policy identity
and no incompatibility issues. `NO_TEMPLATE`, `EMPTY_SKILLS`,
`MULTIPLE_AGENTS`, `UNMAPPED_CONSTRAINTS`, `NOT_CURRENTLY_ELIGIBLE` and
`POLICY_REQUIRED` remain valid v2 records but are fail-closed and cannot be
adapted to a runtime context.

The envelope uses schema `2.0`, rejects unknown JSON fields and validates on
both construction and parsing. Its sidecar-like fields are part of the
versioned CG-02 contract, not a second execution IR. CG-10 owns any adapter
from an executable v2 record to a concrete runtime representation; CG-08 does
not execute transitions, evaluate policy or mutate state.

QA evidence is in `crates/gateway-domain/tests/execution_context_v2.rs` and
the CG-08 application end-to-end suite. The tests prove round-trip stability,
non-executable multi-Agent retention, required executable fields, wrong-version
rejection and strict unknown-field handling.
