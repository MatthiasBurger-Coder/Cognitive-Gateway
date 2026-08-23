# ADR-010 — Request-Scoped Project Context and Retrieval Provenance

- Status: Accepted
- Date: 2026-08-23

## Context

The Gateway catalog is reusable across consuming projects, while repository
configuration and project knowledge vary per request. Passing those concerns
through a registry or returning unprovenanced retrieval text would make the
authority boundary ambiguous.

## Decision

- consuming-project configuration is carried as opaque `ProjectContext` at the
  `gateway-application` request boundary;
- the registry does not accept project context and remains responsible only for
  the Gateway-owned catalog;
- retrieval requests may carry an explicit project scope through
  `KnowledgeRequest`;
- retrieval adapters return typed `RetrievedKnowledge` values with validated
  content and `KnowledgeProvenance`;
- retrieval results and project configuration cannot grant capabilities,
  policy decisions, Agent/Skill membership or workflow authority;
- project configuration and retrieved material are not fields of
  `ExecutionContextIR`.

Concrete configuration and RAG providers remain outer adapters. This decision
defines their contracts without selecting a provider, storage technology or
project-specific schema.

## Consequences

- catalog loading and resolution remain project-independent and deterministic;
- project-specific inputs are visible at the application boundary and cannot
  be mistaken for canonical definitions;
- retrieval provenance is available for audit and diagnostics;
- provider adapters can evolve without changing domain authority semantics;
- an implementation still needs to decide how an individual project stores
  and supplies its opaque configuration.
