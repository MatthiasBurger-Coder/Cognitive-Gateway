# 7. Deployment View

## 7.1 v0.1

v0.1 is a local developer-side deployment.

```text
Developer Workstation
├── Cognitive Gateway CLI/Core (Rust)
├── Project Repository
└── Local request configuration and runtime context
```

No external AI service is required for deterministic validation and resolution.

## 7.2 Planned daemon deployment

```text
Developer Workstation
├── IntelliJ / CLI / CI Client
├── Cognitive Gateway Daemon (Rust)
│   ├── registry/cache
│   ├── workflow/state
│   ├── policy
│   ├── context compiler
│   └── MCP/tool gateway
└── Optional Cognitive Services (Python)
    ├── local SLM
    ├── embeddings
    └── retrieval
```

## 7.3 Model deployment

Local cognitive models may run through an external model runtime or Python service. The Rust core must communicate through a stable port/interface and must not require a specific model server.

## 7.4 Portability

The target is a local cross-platform developer tool. Packaging should favor a self-contained Rust binary for the control plane, with optional separately installable cognitive services.
