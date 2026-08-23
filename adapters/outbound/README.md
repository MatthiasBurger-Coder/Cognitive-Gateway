# Outbound Adapters

Outbound adapters are driven adapters. They implement application-defined ports for repository/Git access, knowledge retrieval, capabilities, execution runtimes and evidence.

Concrete RAG stores, MCP integrations, model providers, GitHub clients and runtime SDKs must remain in this outer layer and must not become dependencies of the deterministic core.

Knowledge adapters receive a `KnowledgeRequest` with an optional explicit
`ProjectContext` scope and return validated `RetrievedKnowledge` values with
provenance. They must not return capabilities, policy decisions or catalog
membership.
