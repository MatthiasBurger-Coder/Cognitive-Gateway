# 11. Risks and Technical Debt

## 11.1 Premature complexity

Risk: introducing Rust, Python, MCP, local SLMs, RAG, graph storage and multiple agent runtimes at the same time would create too many failure surfaces.

Mitigation: v0.1 is deliberately Rust-only and deterministic. Cognitive services and advanced retrieval are staged later.

## 11.2 Policy leakage into prompts

Risk: governance becomes descriptive prompt text instead of enforceable behavior.

Mitigation: policies and capability checks are machine-readable and evaluated by the deterministic core.

## 11.3 Retrieval as accidental authority

Risk: vector similarity selects the wrong skill or omits a required governance rule.

Mitigation: registries/resolvers determine valid agents, skills and capabilities; retrieval only supplies information.

## 11.4 Framework lock-in

Risk: coupling the architecture directly to Codex, PraisonAI, LangChain or a specific model runtime.

Mitigation: execution runtimes and cognitive services are adapters behind stable ports.

## 11.5 Documentation drift

Risk: GitHub Wiki, code and architecture documentation diverge.

Mitigation: repository arc42/ADRs remain authoritative; Wiki is a simplified end-user view.

## 11.6 IR evolution

Risk: changes to `ExecutionContextIR` break adapters and stored evidence.

Mitigation: explicit schema versioning and contract tests.

## 11.7 Local model uncertainty

Risk: a local SLM provides inconsistent semantic classification.

Mitigation: confidence reporting, deterministic validation, escalation/fallback and no authority delegation to the SLM.
