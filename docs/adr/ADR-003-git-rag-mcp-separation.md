# ADR-003 — Git Authority, RAG Knowledge, MCP Capabilities

- Status: Accepted
- Date: 2026-08-22

## Context

The architecture combines canonical project rules, dynamic knowledge retrieval and executable tools. Mixing these concerns would make permissions probabilistic and hard to audit.

## Decision

Adopt the following separation:

- **Git/repository** = canonical source of truth for governance, policies, workflows, skills and architecture.
- **RAG/retrieval** = knowledge accelerator that answers what the runtime needs to know.
- **MCP/tools** = controlled capability boundary that answers what the runtime may do.
- **Workflow/state** = explicit current operational state.

## Critical rule

Retrieval results never grant permissions and never become the sole authority for governance.

## Consequences

- knowledge and action can evolve independently;
- tool authorization stays deterministic;
- retrieval quality failures cannot silently bypass policy;
- provenance and evidence remain traceable to canonical sources.
