# ADR-008 — Hexagonal Architecture and Ports & Adapters

- Status: Accepted
- Date: 2026-08-22

## Context and problem

Cognitive Gateway coordinates authority, state, deterministic routing, knowledge retrieval, capabilities, context compilation and execution runtimes. These concerns have different stability and trust boundaries. If transports, model providers, databases, MCP implementations or runtime SDKs are imported into the deterministic core, the core becomes difficult to test, replace and audit. It could also accidentally treat retrieved knowledge or an integration response as authority.

The project therefore needs an explicit structural rule before the Rust implementation grows beyond its bootstrap crates.

## Decision

Adopt **Hexagonal Architecture**, also called **Ports & Adapters**, for the deterministic Rust core.

The domain and application core owns stable concepts, use cases, policies and port contracts. External systems connect through adapters:

- driving adapters (CLI, API, IDE and CI) call inbound application ports;
- driven adapters implement outbound ports for knowledge, capabilities, execution runtimes and evidence;
- the core performs deterministic validation, routing, authorization and context compilation;
- adapter technologies remain replaceable and are selected at the composition boundary.

The dependency rule is:

```text
Driving Adapters -> Inbound Ports -> Application + Domain/Core
Application + Domain/Core -> Outbound Ports -> Driven Adapters
```

Pure serialization libraries (`serde` and `serde_json`) are the explicitly
allowed exception for versioned domain wire contracts. They do not connect the
core to an external system or provider; all provider, transport, persistence
and integration dependencies remain outside the domain.

Dependencies point inward toward stable abstractions. The core must not depend on OpenAI or another model provider, Codex, PraisonAI, concrete MCP implementations, vector or graph databases, GitHub APIs, filesystem/Git infrastructure details, or UI/transport frameworks.

## Knowledge, capability and runtime adapters

RAG is treated as one or more **knowledge adapters** behind knowledge/retrieval ports. RAG returns knowledge and provenance; it does not define governance, grant permissions or override policy.

MCP and other tool integrations are **capability adapters** behind capability ports. The core policy decision determines whether a capability may be used; an MCP/tool adapter only performs an operation authorized through its port. Knowledge retrieval and executable capability use are separate flows.

Codex, PraisonAI, local LLMs and cloud model APIs are **execution-runtime adapters** behind an execution-runtime port. The runtime consumes the validated Execution Context IR and is not a source of authority or policy.

## Consequences

Positive consequences:

- domain and application behavior can be tested with port doubles;
- infrastructure, providers, MCP implementations and runtimes can be replaced without rewriting core behavior;
- dependency direction can be reviewed from Cargo manifests and architecture documentation;
- adapter provenance and policy outcomes can be recorded for auditability;
- the deterministic core remains usable without an LLM, RAG backend or external network.

Trade-offs:

- ports require deliberate contracts and may introduce mapping code at boundaries;
- simple integrations can require an adapter and composition-root wiring;
- cross-cutting concerns must be designed so they do not leak infrastructure details into the core.

## Rejected alternatives

- **Framework-centered architecture:** rejected because a specific agent, model or integration framework would become an architectural dependency.
- **Infrastructure-first layered design:** rejected because inward dependency direction and replaceability would be less explicit.
- **Direct runtime/tool calls from domain code:** rejected because this would mix policy, knowledge and execution concerns and prevent isolated deterministic testing.

This ADR establishes structure only. It does not select a RAG store, MCP implementation, execution runtime or concrete adapter technology.
