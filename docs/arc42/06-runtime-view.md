# 6. Runtime View

## 6.1 Deterministic request flow

```text
Task
  |
  v
Profile Loader
  |
  v
Registry Validation
  |
  v
Workflow / Agent / Skill Resolver
  |
  v
Policy Engine
  |
  v
Execution Context IR
  |
  v
Context Compiler
  |
  v
CLI / Runtime Adapter
```

This path must work without an LLM or external network access in v0.1.

## 6.2 Semantic classification flow

Later releases may add:

```text
Task
  |
  v
Local SLM
  |
  v
Semantic signals + confidence
  |
  v
Deterministic Resolver
```

The SLM proposes semantic interpretation. The resolver validates registered workflows, agents, skills and capabilities.

## 6.3 Retrieval flow

```text
Validated task + selected skills
        |
        v
Retrieval Planner
        |
        +--> repository search
        +--> vector retrieval
        +--> graph retrieval
        +--> evidence history
        |
        v
Retrieved knowledge with provenance
        |
        v
Context Compiler
```

Retrieval never grants capabilities or overrides policy.

## 6.4 Tool execution flow

```text
Execution Runtime
       |
       v
Capability request
       |
       v
Policy Engine
       |
   allow / deny
       |
       v
MCP / Tool Adapter
```

Mutation requests may require explicit authorization while inspection requests can remain available.
