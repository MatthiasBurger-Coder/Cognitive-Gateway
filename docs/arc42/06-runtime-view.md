# 6. Runtime View

## 6.1 Representative port-and-adapter request flow

The representative request flow makes the runtime boundary explicit:

```text
Driving Adapter (CLI / API / IDE / CI)
                |
                v
Inbound Application Port (submit task)
                |
                v
Application + Domain/Core
  validate -> resolve -> authorize -> build IR
                |
                v
Outbound Port (knowledge / runtime / evidence)
                |
                v
Driven Adapter (Git/RAG, runtime or evidence implementation)
                |
                v
Result + provenance/audit evidence
```

The driving adapter translates the transport request into the inbound port contract. The core performs deterministic validation, resolution and policy evaluation. The selected outbound port is implemented by a replaceable driven adapter, and the result returns through the application boundary with provenance or audit evidence where applicable.

Knowledge retrieval is not executable capability use: a knowledge adapter returns retrieved material and provenance. A capability request follows a separate policy-controlled capability port and may be denied before any MCP/tool adapter is called.

## 6.2 Deterministic request flow

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
Context Compiler / Execution Context IR
  |
  v
CLI / Runtime Adapter
```

This path must work without an LLM or external network access in v0.1.

## 6.3 Semantic classification flow

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

## 6.4 Retrieval flow

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

## 6.5 Tool execution flow

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

Mutation requests may require explicit authorization while inspection requests can remain available. The MCP/tool adapter is a driven capability adapter and cannot change the policy decision made by the core.
