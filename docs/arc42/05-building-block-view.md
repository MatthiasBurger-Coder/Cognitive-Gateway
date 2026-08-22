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
+-------------------------+
| gateway-application     |
| inbound/outbound ports  |
+------------+------------+
             |
             v
+-------------------------+
| gateway-domain          |
| stable domain model     |
+-------------------------+
             ^
             |
+------------+---------------------------------------------+
| gateway-registry | gateway-workflow | gateway-policy    |
| gateway-context  | deterministic application components |
+------------+---------------------------------------------+
             ^
             |
      gateway-daemon / future adapters
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

### Legal dependency direction

- `gateway-domain` has no workspace or infrastructure dependency.
- `gateway-application` depends on `gateway-domain` and defines ports.
- `gateway-registry`, `gateway-workflow`, `gateway-policy` and `gateway-context` depend only on inner abstractions required by their responsibility.
- `gateway-daemon` is an outer composition root and may depend on inner crates.
- future adapters may depend on application/domain contracts; the core must never depend on adapters.
- circular crate dependencies are forbidden.

### Adapter attachment points

```text
KnowledgePort       <- Git / filesystem / vector / graph RAG adapters
CapabilityPort      <- MCP / Git / quality / GitHub / runtime-tool adapters
ExecutionRuntimePort<- Codex / PraisonAI / local/cloud LLM adapters
EvidencePort        <- audit/evidence persistence adapters
```

Concrete adapter technologies are intentionally not selected by CG-01.02.

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
