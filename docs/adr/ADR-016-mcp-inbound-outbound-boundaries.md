# ADR-016 — Separate Inbound Client MCP from Outbound Connector MCP

- **Status:** Accepted
- **Date:** 2026-10-03

## Context

Cognitive Gateway uses MCP in two different directions: clients such as Codex may invoke Gateway capabilities, while Gateway itself must connect to external systems such as GitHub, Confluence or filesystems.

Conflating these directions would duplicate responsibilities and risk making protocol discovery look like domain authority.

## Decision

Treat MCP strictly as adapter infrastructure and keep two explicit responsibilities:

- **EPIC-04:** inbound Codex -> Cognitive Gateway local MCP/client integration.
- **EPIC-07:** outbound Cognitive Gateway -> external MCP connector/plugin runtime.

Shared protocol utilities may exist only behind outer adapter boundaries. MCP protocol/provider types do not enter authoritative domain contracts.

Tool/resource discovery never grants permission, trust or process authority.

## Consequences

- Codex remains a client, not an authority source.
- External connectors remain providers of capabilities/evidence, not authority.
- Policy, scope, provenance, trust, sensitivity, consent and retry decisions remain Gateway responsibilities.
- GitHub is a reference connector rather than special core logic.
