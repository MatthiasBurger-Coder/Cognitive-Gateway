# 3. Context and Scope

## 3.1 Business context

Cognitive Gateway mediates between task-producing clients and task-executing AI/agent runtimes.

```text
IDE / CLI / CI / API
        |
        v
Cognitive Gateway
        |
        +--> Knowledge sources
        +--> Capability/tool providers
        |
        v
Codex / PraisonAI / Local LLM / Cloud LLM
```

## 3.2 Inputs

Typical inputs include:

- natural-language user tasks;
- issue or work-item metadata;
- project profile and governance state;
- repository state;
- workflow/gate/blocker state;
- runtime and test evidence.

## 3.3 Outputs

Primary outputs are:

- validated workflow/agent/skill selections;
- policy decisions;
- retrieval plans;
- approved capability sets;
- versioned `ExecutionContextIR`;
- compiled stable and dynamic context;
- explainability/audit traces.

## 3.4 External systems

Potential external systems include:

- Git repositories;
- GitHub;
- IDE integrations;
- MCP servers;
- local model runtimes;
- PraisonAI;
- Codex/OpenAI runtimes;
- vector or graph stores in later releases.

## 3.5 Project profiles

Product-specific knowledge does not belong in the generic gateway core. Each
consumer contributes a project profile containing its own agents, skills,
workflows, policies, retrieval metadata and tool bindings.
