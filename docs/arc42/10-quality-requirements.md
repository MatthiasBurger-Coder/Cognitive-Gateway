# 10. Quality Requirements

## 10.1 Determinism

Given the same validated catalog, task descriptor and state, the deterministic
core must produce the same logical workflow, Agent, Skill, policy and
compiled-context result.

## 10.2 Safety

- Unknown references fail closed.
- Mutating capabilities are explicitly classified.
- Policy denial cannot be overridden by retrieval or model output.
- Authorization requirements are explicit and testable.

## 10.3 Performance

The always-on control plane should have low startup latency and memory footprint. Expensive cognitive services should be invoked only when required.

## 10.4 Testability

Core behavior must be testable without network access or an LLM.

Required v0.1 evidence includes:

- `cargo build --workspace`;
- `cargo fmt --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- unit tests;
- schema/contract tests;
- deterministic regression tests;
- end-to-end vertical-slice test.

## 10.5 Modularity

The core must remain independent from specific model providers, agent frameworks, vector databases and IDE integrations.

## 10.6 Explainability

Every non-trivial routing or policy decision must provide a reason trace suitable for diagnostics and audit.

## 10.7 Maintainability

Agent, skill, workflow and policy definitions should be declarative and
schema-validated rather than hard-coded into the daemon. Reusable Agents come
from the generic catalog; consuming-project context and configuration are
provided through explicit runtime or adapter inputs.
