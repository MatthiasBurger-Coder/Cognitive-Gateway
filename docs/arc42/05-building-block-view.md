# 5. Building Block View

## 5.1 Level 1

### Gateway Core

The deterministic core follows Hexagonal Architecture. It owns stable domain concepts, application ports, deterministic registries, workflow resolution, policy evaluation and context compilation.

### Cognitive Services

Optional services provide semantic classification, embeddings, RAG, graph retrieval, ranking and summarization. They are outside the deterministic core and connect through knowledge/retrieval ports.

### Execution Runtime Adapters

Codex, PraisonAI, local LLMs and cloud LLM APIs are replaceable driven adapters behind execution-runtime ports.

### Capability Adapters

Repository, Git, quality gates, runtime inspection, GitHub and MCP/tool integrations are controlled adapters behind capability ports. Retrieval never grants capability permissions.

## 5.2 Rust workspace and hexagonal mapping

```text
        Driving Adapters
       CLI / API / IDE / CI
                |
                v
        Inbound Application Ports
                |
                v
  +------------------------------------+
  | Application + Domain/Core          |
  |                                    |
  | gateway-application (use cases,   |
  | ports)                             |
  | gateway-domain (stable model)     |
  | registry/workflow/policy/context   |
  +------------------------------------+
                |
                v
        Outbound Application Ports
          /          |           \
         v           v            v
   Knowledge      Capability   Runtime/Evidence
   Driven        Driven       Driven Adapters
   Adapters      Adapters
```

Initial crates:

```text
crates/
├── gateway-domain/
├── gateway-application/
├── gateway-registry/
├── gateway-workflow/
├── gateway-policy/
├── gateway-context/
└── gateway-daemon/
```

The intended responsibility of each current crate is:

- `gateway-domain`: typed identifiers, immutable definitions, execution state,
  capabilities, constraints, `ExecutionContextIR` and domain validation;
- `gateway-application`: inbound use-case ports, outbound ports and application orchestration;
- `gateway-registry`: deterministic loading and validation of registered definitions;
- `gateway-workflow`: workflow resolution and execution-state rules;
- `gateway-policy`: authorization and fail-closed policy evaluation;
- `gateway-context`: validated context compilation and Execution Context IR handling;
- `gateway-daemon`: composition root and future process/transport wiring, outside the core.

Concrete CLI/API/IDE/CI driving adapters and Git/RAG, MCP/tool, runtime and
evidence driven adapters are outer components. They must implement the
contracts exposed by `gateway-application` rather than introduce dependencies
into `gateway-domain` or the deterministic core.

### Legal dependency direction

- `gateway-domain` contains stable domain concepts and has no provider,
  transport or infrastructure dependency; `serde` and `serde_json` are the
  explicitly allowed serialization dependencies for the versioned wire
  contract.
- `gateway-application` contains application use cases and defines inbound and outbound ports; it depends inward on `gateway-domain`.
- `gateway-registry`, `gateway-workflow`, `gateway-policy` and `gateway-context` are deterministic inner components and depend only on inner abstractions required by their responsibility.
- `gateway-daemon` is the initial outer composition root and may depend on inner crates and, later, on concrete adapters.
- driving adapters may call inbound ports; driven adapters may implement outbound ports. Both depend on core-defined contracts.
- the core must never depend on adapters, transport frameworks, providers, databases, Git/GitHub details or concrete MCP implementations.
- circular crate dependencies are forbidden.

### Adapter attachment points

```text
KnowledgePort       <- Git / filesystem / vector / graph RAG adapters
CapabilityPort      <- MCP / Git / quality / GitHub / runtime-tool adapters
ExecutionRuntimePort<- Codex / PraisonAI / local/cloud LLM adapters
EvidencePort        <- audit/evidence persistence adapters
```

These are attachment points, not permissions. The policy engine decides whether a capability request is allowed; an adapter only performs the operation authorized through its port. Knowledge adapters return knowledge and provenance, never authority or permissions.

The adapter technologies are replaceable implementation choices. Their
provider-specific configuration and behavior do not belong in the domain
contract.

## 5.3 Proposed Python services

```text
cognitive-services/
├── classifier/
├── embeddings/
├── retrieval/
└── graph/
```

Python cognitive services remain optional and sit outside the deterministic Rust core.

## 5.4 Project profiles

```text
profiles/<project>/
├── agents/
├── skills/
├── workflows/
├── policies/
├── retrieval/
└── tools/
```

Profiles configure the generic gateway without embedding product-specific logic in the core.
