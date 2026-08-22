# 5. Building Block View

## 5.1 Level 1

### Gateway Core

Responsibilities:

- domain model;
- registry;
- workflow and state handling;
- policy evaluation;
- context compilation;
- routing coordination;
- audit/explainability.

### Cognitive Services

Optional services for:

- semantic classification;
- embeddings;
- RAG;
- graph retrieval;
- ranking and summarization.

### Execution Runtime Adapters

Adapters for systems such as:

- Codex;
- PraisonAI;
- local LLMs;
- cloud LLM APIs.

### Capability Adapters

Controlled interfaces for repository, Git, quality gates, runtime inspection, GitHub and other tools.

## 5.2 Proposed Rust crates

```text
crates/
├── gateway-domain/
├── gateway-registry/
├── gateway-workflow/
├── gateway-policy/
├── gateway-context/
├── gateway-mcp/
└── gateway-daemon/
```

## 5.3 Proposed Python services

```text
cognitive-services/
├── classifier/
├── embeddings/
├── retrieval/
└── graph/
```

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
