# ADR-001 — Rust Core and Python Cognitive Services

- Status: Accepted
- Date: 2026-08-22

## Context

Cognitive Gateway combines two different classes of workload: a deterministic, always-on control plane and rapidly evolving AI/retrieval functionality.

## Decision

Use **Rust** for the gateway core and daemon, including registry, resolver, workflow state, policy engine, context compiler, cache, IPC/MCP, tool routing and telemetry.

Use **Python** for optional cognitive services such as local SLMs, embeddings, semantic classification, RAG, Graph RAG and evaluation.

Use **Kotlin** only if a dedicated IntelliJ integration becomes necessary.

## Consequences

Positive:

- low-footprint native daemon;
- strong deterministic/safety boundary;
- good concurrency and packaging characteristics;
- access to Python's AI ecosystem without coupling it into the core.

Trade-offs:

- polyglot build and release process;
- explicit inter-process/service contracts are required.

## Guardrail

The Rust core must remain functional for deterministic requests when Python cognitive services are unavailable.
