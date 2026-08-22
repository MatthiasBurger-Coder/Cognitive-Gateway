# ADR-005 — Execution Runtime Independence

- Status: Accepted
- Date: 2026-08-22

## Context

Cognitive Gateway may execute work through Codex, PraisonAI, local models or future runtimes. Making one of these systems the architectural core would create framework lock-in.

## Decision

Execution runtimes are adapters behind a stable gateway contract. The gateway determines what is needed and allowed; the selected execution runtime determines how the approved task is executed.

PraisonAI may be used as an agent-orchestration runtime. Codex may be used directly as a coding kernel. Neither is a mandatory core dependency.

## Consequences

- runtimes can be compared or replaced;
- project governance remains independent of runtime behavior;
- IDE integration can retain the existing Codex plugin while consuming Cognitive Gateway through MCP/tool integration;
- the gateway can later support multiple execution backends without redesigning the domain model.
