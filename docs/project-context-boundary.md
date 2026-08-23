# Project Context and Retrieval Boundary

Consuming-project configuration is external request context. It is not part of
the Gateway Agent/Skill catalog, is not loaded by `gateway-registry`, and is
not serialized into `ExecutionContextIR`.

## Application contract

`gateway-application::context::ProjectConfiguration` carries an opaque,
validated media type and content snapshot. `ProjectContext` owns that snapshot
at the application boundary. The application does not interpret the content as
an Agent, Skill, workflow, policy or capability definition.

`ExecutionRequest` carries a task together with its external
`ProjectContext`. The resulting domain `ExecutionContext` remains independent
of that project configuration. Project context may influence a request-scoped
plan through an application implementation, but it cannot add, replace or
override catalog membership.

## Retrieval contract

`KnowledgeRequest` carries a `KnowledgeQuery` and an optional
`ProjectContext`. A `KnowledgePort` returns `RetrievedKnowledge` values. Every
result has validated content and `KnowledgeProvenance` containing its source
and optional revision or snapshot identity.

Retrieval results are informational. They do not contain or create
capabilities, permissions, policy decisions, Agent/Skill membership or
workflow authority. A concrete RAG/vector/graph adapter remains outside the
deterministic core and must preserve the returned provenance.

The registry boundary remains independently usable:

```text
Gateway catalog ──> Registry validation/resolution ──> authority-bearing plan

ProjectContext ───> application request / retrieval scope ──> advisory knowledge
```
