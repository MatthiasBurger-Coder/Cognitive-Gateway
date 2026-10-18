# Current Architecture State


Current full-parent acceptance is governed by [EPIC-04 acceptance](epic-04-acceptance.md).
Shared structured services and installed-client lifecycle proof are delivered;
older evidence below retains its historical scope. Fresh full reconciliation
determines the current parent acceptance state.

**Reference date:** 2026-10-11; semantic-plane entries reconciled 2026-10-17

This document is the compact status map for the current Cognitive Gateway architecture. It distinguishes implemented contracts from planned architecture so that issue descriptions, arc42 and code are not accidentally treated as equivalent maturity.

## Status legend

- **IMPLEMENTED** — present in the default branch with executable code/tests or an established shipped contract.
- **PARTIAL** — foundations exist, but the complete vertical slice is not yet implemented.
- **PLANNED** — defined in an Epic/issue and part of the target architecture, but not yet a completed runtime capability.

## Current system map

```mermaid
flowchart LR
    U[Human / Client / Codex]
    NL[Natural Language]
    SEM[CGSL / SemanticTaskIR]
    SIT[Intent / Situation]
    PLAN[Delta / Planning]
    RES[Registry / Resolver]
    PROC[Process Engine]
    POL[Policy]
    RET[Retrieval / Memory / Graph]
    CTX[Context Compiler]
    RUN[Execution Runtime]
    MCP[MCP Connector Runtime]
    EXT[GitHub / Confluence / Files / CI]
    SLM[Local Model Runtime]
    LEARN[Learned Procedures]
    VERIFY[Verification / Evaluation]

    U --> NL
    NL -. planned semantic compilation .-> SEM
    SIT -->|typed compilation inputs| SEM
    SEM -->|validated application handoff| PLAN
    SIT --> PLAN
    PLAN --> RES
    RES --> PROC
    PROC --> POL
    RET --> CTX
    POL --> CTX
    CTX --> RUN
    RUN --> VERIFY
    VERIFY --> LEARN
    LEARN -. governed reuse .-> PLAN

    U -. EPIC-04 .-> RUN
    RET -. EPIC-07 .-> MCP
    MCP -. planned .-> EXT
    SEM -. optional proposal .-> SLM
```

## Capability status

| Area | Status | Notes |
| --- | --- | --- |
| Typed domain model / ExecutionContextIR | IMPLEMENTED | Versioned domain and serialization contracts exist. |
| Agent / Skill / Capability registry | IMPLEMENTED | Deterministic catalog and resolution foundations exist. |
| Process engine | IMPLEMENTED | Deterministic process semantics and integration proof exist. |
| Intent / Situation / declarative planning | IMPLEMENTED | CG-06 / CG-07 contracts and application flow are documented and tested. |
| Resolver / explainability | IMPLEMENTED / PARTIAL | Major CG-08 contracts exist; acceptance remains tied to documented integration constraints. |
| Policy and inspect/mutate separation | IMPLEMENTED | Policy remains the authority boundary for executable capability use. |
| Context compiler | IMPLEMENTED | Provider-independent context construction is available. |
| Retrieval / vector / graph / memory | IMPLEMENTED | CG-15 through CG-20 foundations are present. |
| Closed-loop execution | IMPLEMENTED | Bounded reassessment/replanning flow exists. |
| Parallel task execution | IMPLEMENTED | CG-28A bounded parallel scheduling is present. |
| Learned procedure domain contracts | IMPLEMENTED | CG-21 foundation added on 2026-10-03. |
| Experience normalization and pattern inspection | IMPLEMENTED | CG-22 correlates governed, verified outcomes and supports PostgreSQL backed inspection; it creates candidates without executable authority. |
| Learned procedure discovery/promotion/reflex runtime | IMPLEMENTED (governed reference runtime) | CG-21 through CG-25 and CG-30 demonstrate experience → detected pattern → evaluated procedure → governed promotion → reflex, fallback and rollback. |
| CGSL / SemanticTaskIR | PARTIAL (components implemented; full hosted runtime pending) | [Implementer guide](cgsl-implementer-guide.md): normative CGSL/task contracts, offline parser/compiler/formatter/serializer, deterministic resolution, completeness/clarification, optional proposals and CG-06/07/08 adapter are implemented. Process boundary and two offline prompt renderings have component proofs. [Conformance](cgsl-conformance.md) freezes portable source/IR/digest and adversarial expectations. #285 [canonical requirements/source planning and CG-10 handoff](context-requirement-planning.md) are implemented with offline conformance; #286 [canonical example selection and dedicated CG-10 handoff](dynamic-example-selection.md) are implemented with offline conformance; #263 invocation, #275 clarification host and #279 runtime integration remain separate. |
| Natural-language-to-IR compiler | IMPLEMENTED (bounded inspection profile); general runtime PARTIAL | The [inspection profile](cgsl-implementer-guide.md#supported-natural-language-inspection-profile) compiles original `Inspect artifact <id>` / `Inspect it` text through host-bound resolution/completeness to validated task IR. Formal CGSL compiles offline. General interpretation and the installed natural-language runtime path are not qualified. See [reference walkthroughs](cgsl-reference-examples.md). |
| Optional SLM semantic interpreter | PARTIAL (proposal adapters implemented) | [Optional interpretation adapters](interpretation-adapters.md) validate classifier/embedding/SLM proposals and preserve deterministic fallback; model output is never authoritative. Full semantic host/runtime qualification remains separate. |
| Local SLM/LxM runtime | IMPLEMENTED (reference service) | CG-27.01: separate CPU-first Ollama/Python containers, Rust port/adapter, immutable profiles, qualification, promotion and rollback. EPIC-05.11 proposal adapters exist; complete hosted interpretation remains separate integration work. |
| Local cognitive signal benchmarks | IMPLEMENTED | CG-27: replaceable four-task proposal adapter, versioned synthetic dataset, fixture and Ollama CPU/GPU harness, per-sample provenance and explicit failure/fallback evidence. |
| Governed offline learning pipeline | IMPLEMENTED (contracts and reference coordinator) | CG-28 validates signals, assembles scoped/versioned datasets, gates offline jobs/evaluation and journals canary/rollback. Concrete CPU feature/training/CV/calibration worker, PostgreSQL release journals and current-version inference/rollback are implemented; see complete EPIC-03 acceptance. |
| Distributed cognitive worker fabric | IMPLEMENTED (contracts and local reference) | CG-29: immutable scoped snapshots, bounded queue/retries, fenced leases, provenance and a local worker adapter. PostgreSQL coordination preserves leases/fencing/idempotency across restart; Linux process and isolated container resource enforcement are implemented. Swarm/Kubernetes transport remains optional. |
| Qwen3-8B reference profile | PLANNED | Reference candidate only; not a mandatory product dependency. |
| General provider-independent model invocation | PLANNED | EPIC-06 #178 owns the general model invocation boundary. |
| MCP connector/plugin runtime | PLANNED | EPIC-07 #223 owns external MCP server integration. |
| GitHub MCP connector | PLANNED | Reference connector under EPIC-07. |
| Codex -> CG local no-key MCP | PARTIAL | #238 implements [private stdio lifecycle/discovery](local-mcp-server.md); #236 / ADR-020 define trust. [Application facade](codex-application-facade.md) #239 is implemented and injectable. [Workspace/session isolation](codex-scope-isolation.md) #240 supplies admitted situation queries and scoped resource reads; discovery-only calls remain unsupported. [Credential and sensitive-data isolation](codex-secret-isolation.md) #241 checks launch environment, admission, request/result disclosure and trace/cache inputs. [Current policy/consent gates](codex-policy-gates.md) #242 and [bounded runtime](codex-runtime.md) #243 are implemented. #245 adds [inbound component qualification](codex-release-qualification.md); [canonical host wiring](codex-canonical-host.md) now provides resolve/explain/context through the shipped binaries, and [installed Codex qualification](codex-installed-client-qualification.md) exercises those paths with CLI 0.162.1. The accepted [shared session specification](shared-session-contract.md) / ADR-021 defines the verified artifact goal and explicit v2 boundary. Typed services, the shared coordinator, trusted interactions and PostgreSQL lifecycle/recovery are implemented and exercised by [runtime qualification](shared-session-implementation.md) and installed-client lifecycle checks. Fresh full-parent reconciliation after the 2026-10-11 evidence-contract correction remains required; #279 full model/connector/runtime acceptance is separate. |

## Normative boundaries

1. **Deterministic core before probabilistic assistance.** Model output may propose classifications, mappings or content, but deterministic contracts validate it.
2. **MCP is transport/capability plumbing, not authority.** Discovery never grants permission.
3. **Retrieval is information, not authority.** Retrieved content keeps provenance/trust/sensitivity metadata.
4. **CGSL describes task meaning; Process IR describes controlled execution.**
5. **Execution/runtime providers are replaceable.** Provider SDKs and protocol types stay outside authoritative domain contracts.
6. **Local model runtime is replaceable and independently deployable.** A concrete Qwen/Ollama combination is an implementation profile, not a core dependency.
7. **Learned procedures are governed artifacts.** Experience does not become executable authority merely because it succeeded previously.

## Issue anchors

- EPIC-04 #126 — Codex local no-key integration.
- EPIC-05 #177 — CGSL, SemanticTaskIR and contextual semantic resolution.
- EPIC-06 #178 — provider-independent model invocation.
- EPIC-07 #223 — MCP connector/plugin runtime.
- EPIC-03 #114 — adaptive cognitive runtime and learned procedures.
- CG-28 #219 — governed learning signals and offline training pipeline.
- CG-27 #218 — local SLM/LxM adapter and benchmark framework.
- CG-27.01 #249 — upgradable containerized local model service.
- CG-29 #220 — distributed cognitive worker fabric.

This file should be updated whenever a planned boundary becomes implemented or a new authoritative architectural boundary is introduced.

## EPIC-06.01 contract foundation (#261)

The candidate adds versioned immutable model capability/provider/invocation and
generation-profile contracts, strict numeric/control validation, effective-default
provenance and an application preparation/adapter port. Runtime availability is
separate from eligibility. [Component evidence](model-invocation-contracts.md) does
not establish a shipped invocation path. Routing integration is implemented in
#262 below; full CG-10 admission, real adapters and result verification remain
separate EPIC-06 slices.

## EPIC-06.04 rendering boundary (#264)

Versioned chat-message and block-message outer renderers now consume the actual
CG-10 handoff and prepared invocation through a pure application service.
Full-payload tokenizer accounting binds model/profile/tokenizer/template metadata,
preserves exact versus estimated provenance, reserves output and returns typed
overflow without truncation. [Component qualification](model-invocation-contracts.md#deterministic-semantic-renderers--epic-0604-264)
uses actual producer data with a synthetic tokenizer; production tokenizer/provider
adapters and the #263 authenticated admission/dispatch host remain prerequisites
for full issue integration.

## EPIC-06.02 deterministic invocation routing (#262)

Implemented as an application service over the existing CG-26 ranking and bounded
execution coordinator: strict capability/provider/health eligibility, exact
fallback task/context/output/tools/authority/generation invariants, context and
resource constraints, stable explanations and computed snapshot identity, and
zero model dispatch for deterministic tasks. Tests compile real CG-10 steps with
a fake host runtime. Installed inference transports and full invocation handoff
admission remain separate #269/#263 work. See
[operator guide and traceability](model-capability-routing.md).
