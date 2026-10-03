# Codex Local Integration

## Status

**Planned — EPIC-04 #126.**

EPIC-04 defines the inbound integration from Codex to Cognitive Gateway through a local, no-CG-API-key boundary.

## Architectural direction

```mermaid
flowchart LR
    CODEX[Codex]
    MCP[Local MCP / stdio Adapter]
    FACADE[Codex-facing Application Facade]
    CORE[Cognitive Gateway Application Layer]
    POLICY[Policy / Consent]
    RES[Resolver / Process / Context]
    EXT[MCP Connector Runtime]

    CODEX --> MCP
    MCP --> FACADE
    FACADE --> POLICY
    POLICY --> CORE
    CORE --> RES
    CORE -. separate outbound concern .-> EXT
```

## Boundary rules

- Cognitive Gateway does not require or store an OpenAI API key for this integration.
- Codex owns its account/session authentication.
- MCP framing remains an adapter concern.
- Codex/OpenAI SDK types cannot enter authoritative domain contracts.
- Every request is bound to explicit workspace/project/session scope.
- Provenance and sensitivity survive the boundary.
- Tool availability never implies authorization.
- Mutation/admin operations remain subordinate to CG policy and consent.
- Codex cannot grant capabilities, alter policy or directly advance process state.

## Separation from EPIC-07

EPIC-04 is **Codex -> Cognitive Gateway**.

EPIC-07 is **Cognitive Gateway -> external MCP servers/systems**.

They may share protocol infrastructure at outer adapter boundaries, but must not duplicate responsibilities or introduce protocol-specific domain models.

## Planned vertical slice

```mermaid
sequenceDiagram
    participant C as Codex
    participant A as Local CG Adapter
    participant F as Application Facade
    participant P as Policy
    participant G as CG Services

    C->>A: versioned local request
    A->>F: validated scoped request
    F->>P: authorize operation
    P-->>F: allow / deny / consent
    F->>G: existing use case
    G-->>F: canonical result + evidence
    F-->>A: versioned response
    A-->>C: deterministic result
```

The implementation work is tracked by EPIC-04.01 through EPIC-04.10 (#236–#245).
