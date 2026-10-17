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

## 11.8 Semantic shape mistaken for executable admission

Risk: a successful CGSL parse, JSON Schema check or `task_shape()` is treated as
proof of reference truth, mandatory completeness or execution permission.

Mitigation: trusted captured-basis reference/source validation and host-derived
requirement manifests precede handoff; Process/Policy gates remain authoritative.
[Reference walkthroughs](../cgsl-reference-examples.md) distinguish each evidence
level and show denied paths. Persisted clarification hosting (#275) and full
runtime proof (#279) remain integration obligations.

## 11.9 Semantic loss and conformance drift

Risk: exporting Plan alone drops task output/evidence/verification obligations;
source formatting is confused with IR normalization; fixtures drift with code.

Mitigation: preserve the immutable semantic envelope, retain interpretation
sidecars when saving source, and compare against frozen cross-implementation
bytes/digests. [Migration](../cgsl-migration.md) requires explicit versioned,
validated transformations. The #285 context and #286 bounded example producers
have checked CG-10 handoffs. Production invocation admission (#263) must qualify
current host authentication, disclosure and runtime integration against the
[Prompt IR boundary](../prompt-ir-handoff-boundary.md); illustrative renderings
are not sufficient runtime evidence.
