# 6. Runtime View

## 6.1 Representative port-and-adapter request flow

The representative request flow makes the runtime boundary explicit:

```mermaid
flowchart TB
    DA[Driving Adapter<br/>CLI / API / IDE / CI] --> IN[Inbound Application Port<br/>submit task]
    IN --> V[Validate]
    V --> R[Resolve]
    R --> A[Authorize]
    A --> IR[Build IR]
    IR --> OUT[Outbound Port<br/>knowledge / runtime / evidence]
    OUT --> AD[Driven Adapter<br/>Git/RAG / runtime / evidence]
    AD --> RESULT[Result + provenance / audit evidence]
```

The driving adapter translates the transport request into the inbound port contract. The core performs deterministic validation, resolution and policy evaluation. The selected outbound port is implemented by a replaceable driven adapter, and the result returns through the application boundary with provenance or audit evidence where applicable.

Knowledge retrieval is not executable capability use: a knowledge adapter returns retrieved material and provenance. A capability request follows a separate policy-controlled capability port and may be denied before any MCP/tool adapter is called.

## 6.2 Deterministic request flow

```mermaid
flowchart TB
    T[Task] --> CL[Catalog Loader]
    CL --> RV[Registry Validation]
    RV --> RES[Workflow / Agent / Skill Resolver]
    RES --> P[Policy Engine]
    P --> C[Context Compiler / Execution Context IR]
    C --> RA[CLI / Runtime Adapter]
```

This path must work without an LLM or external network access in v0.1.
The Catalog Loader receives only the Gateway-owned catalog boundary. Consuming
project context enters through explicit request, input, retrieval or adapter
ports; it may influence a request-scoped plan, but it cannot add, replace or
override an Agent or Skill definition. In the application contract this input
is an opaque `ProjectContext` carried by `ExecutionRequest`; it is not part of
the registry or `ExecutionContextIR`.

## 6.3 Semantic interpretation flow (components implemented; host composition partial)

The semantic composition boundary is:

```mermaid
flowchart LR
    NL[Natural Language] --> IC[Interpretation Context]
    IC --> RES[Entity / Reference Resolution]
    RES --> SEM[CGSL / Semantic Compiler]
    SLM[Optional SLM proposal] -. candidate signals .-> SEM
    SEM --> STI[Validated SemanticTaskIR]
    STI --> PLAN[CG-07 Planning]
```

The SLM is optional. It proposes interpretation candidates; it does not define
canonical semantics. Mandatory ambiguity or knowledge gaps remain explicit and
may require retrieval or clarification. The offline CGSL frontend returns typed
shapes/sidecars. Application resolution and completeness authenticate the captured
basis and mandatory manifest before compilation and revalidate the emitted task.
Clarification replies enter as fresh input through that same path; #275 owns
persisted reply correlation and expiry. The semantic planning envelope delegates
to CG-07/08 and retains output/verification/evidence alongside Plan. See the
[executable reference walkthroughs](../cgsl-reference-examples.md). These component
proofs do not establish the full #279 hosted runtime or #263 invocation path.

## 6.4 Retrieval flow

```mermaid
flowchart TB
    T[Validated Task + Selected Skills] --> RP[Retrieval Planner]
    RP --> RS[Repository Search]
    RP --> VR[Vector Retrieval]
    RP --> GR[Graph Retrieval]
    RP --> EH[Evidence History]
    RS --> K[Retrieved Knowledge<br/>with typed provenance]
    VR --> K
    GR --> K
    EH --> K
    K --> CC[Context Compiler]
```

Retrieval never grants capabilities or overrides policy.

## 6.5 Tool execution flow

```mermaid
flowchart TB
    ER[Execution Runtime] --> CR[Capability Request]
    CR --> P[Policy Engine]
    P -->|allow| MCP[MCP / Tool Adapter]
    P -->|deny| D[Denied]
```

Mutation requests may require explicit authorization while inspection requests can remain available. The MCP/tool adapter is a driven capability adapter and cannot change the policy decision made by the core.

## 6.6 Closed-loop goal execution

CG-14 adds an event-driven application coordinator: Intent and scoped evidence
produce a Situation, Delta and Plan; fresh resolution, policy and process
inputs authorize one compiled step; a replaceable execution port returns a
correlated Outcome and complete observation snapshot. CG-06 reassessment and
CG-07 comparison determine success, continuation, replanning, pause or stop.
Iteration and retry limits bound execution across replans. Every revision and
execution remains linked in the audit. Process lifecycle mutation remains with
CG-04. See [the application contract](../closed-loop-execution.md).

## 6.7 Codex local invocation (planned EPIC-04)

```mermaid
sequenceDiagram
    participant C as Codex
    participant A as Local MCP Adapter
    participant F as Application Facade
    participant P as Policy
    participant G as Gateway Services
    C->>A: scoped versioned request
    A->>F: validated request
    F->>P: authorize
    P-->>F: allow / deny / consent
    F->>G: existing use case
    G-->>F: canonical result + evidence
    F-->>C: deterministic response
```

CG stores no OpenAI API key for this path and Codex remains a client rather
than an authority source.

## 6.8 External connector invocation (planned EPIC-07)

```mermaid
flowchart LR
    PLAN[Authorized request] --> MAP[Canonical capability/source mapping]
    MAP --> MCP[MCP Runtime]
    MCP --> EXT[External MCP Server]
    EXT --> NORM[Normalized result + provenance]
    NORM --> EVID[Evidence / Context]
```

Discovery is not authorization. Mutation retry safety, credentials, trust,
sensitivity and scope are explicit Gateway-managed concerns.

## 6.9 Local model invocation and upgrade (CG-27.01 reference service)

A logical model role resolves to a qualified model profile. The local inference
port calls a separately deployable runtime. Candidate model upgrades are
benchmarked and qualified before explicit promotion; rollback restores the
previous qualified profile without rebuilding the Gateway.

## 6.10 Learned procedure lifecycle (CG-21, CG-23, CG-24 implemented)

Eligible governed experience becomes a `PatternCandidate` and a digest-bound
`LearnedProcedure`. [CG-23](../procedure-evaluation.md) evaluates historical
snapshots and counterfactuals, retaining reproducible passing evidence.
[CG-24](../procedure-promotion.md) implements discovered/candidate/validated/
evaluated/approved/canary/active admission through authenticated application
commands and an append-only version registry. Canary execution is bounded by
scope, cohort, time and outcome budgets. Supersession and rollback atomically
change active versions while preserving evidence and historical outcomes.
The read-only `cg procedures` command inspects the complete journal without a model.
Reuse still requires current process, capability and policy authorization;
[CG-25](../reflex-engine.md) adds exact ACTIVE matching, evidence gates, compiled
Process/Policy dispatch, budgets and post-execution verification. The complete EPIC-03 reference lifecycle and durable CPU inference integration
are covered by [complete acceptance](../epic-03-complete-acceptance.md).

The CG-27.01 reference deployment, provider-neutral port and qualification lifecycle
are implemented; see [the operator guide](../local-model-runtime.md).
EPIC-05.11 supplies optional interpretation proposal adapters at component level;
complete hosted interpretation and model invocation remain separate integration work.

## Inbound canonical and session delivery

The admitted local `cg-mcp`/`cg-local` composition root optionally maps pinned
plan/rules/process records through the shared strict CLI input mapper into
canonical resolution, explanation and context compilation. Existing application
services own computation and Process/Policy checks; trusted admission owns the
source classification and operation grant. Generated resolution references are
checked against the current canonical artifact before explain, compile or read.
See [canonical host](../codex-canonical-host.md).

The actual installed Codex client has exercised these paths with neutral admitted
fixture snapshots and full CLI envelope parity, separately from synthetic
protocol/component tests. See [installed-client qualification](../codex-installed-client-qualification.md).
Session service delivery remains pending. [Accepted contract ADR-021](../adr/ADR-021-shared-structured-session-ownership.md)
specifies one application-owned coordinator, a verified context artifact as the
explicit goal, exact trusted interactions and a separate v2 session boundary.
See the [typed specification and prerequisite matrix](../shared-session-contract.md).
The artifact verifier and #272/#273/#275/#276/#277 services remain delivery work;
no session lifecycle is claimed by the standalone host or this runtime diagram.

## Context planning and CG-10 producer handoff (#285)

```mermaid
flowchart LR
    ST[Validated SemanticTaskIR and captured CG-07 needs] --> A[Requirement analysis]
    A --> R[ContextRequirementsIR 1.0]
    R --> S[Deterministic source selection]
    SM[Trusted source metadata] --> S
    S --> P[ContextPlanIR 1.0]
    P --> AC[Existing governed acquisition owners]
    AC --> CC[CG-10 compiled context]
    P --> H[CG-10 planned-context handoff]
    R --> H
    CC --> H
    H --> N[263: future production invocation admission]
```

Present sources skip acquisition. Unavailable requirements remain explicit;
missing mandatory satisfaction, stale or foreign-scope inputs and exact-binding
mismatches reject the handoff. The handoff itself has no acquisition or invocation
port. [The producer contract](../context-requirement-planning.md) describes exact
version/digest correlation and the bounded offline proof.

## Dynamic examples in the planned handoff (#286)

An active #285 `Examples` need derives an `ExampleRequirement` from the exact task
and a host-admitted source/reuse policy. Existing acquisition supplies captured
pairs. Eligibility rejects invalid, unvalidated, stale, incompatible, cross-scope
or quarantined data before optional advisory ranking. Deterministic relevance,
tag diversity, duplicate suppression and hard count/token bounds yield a pinned
`ExampleSet`. The source plan records that set as a dedicated satisfaction link.
CG-10 accepts the same immutable set and expected binding; zero optional examples
succeed and missing mandatory examples block. The #192 projection and both
reference layouts retain examples as non-authoritative data. See the
[selection flow and exact producer contract](../dynamic-example-selection.md).
