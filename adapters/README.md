# Adapters

Concrete integrations live outside the deterministic core and implement ports defined by `gateway-application`.

Driving adapters may include CLI, API, IDE and CI entry points. Driven
adapters may include repository/Git retrieval, RAG/vector/graph retrieval,
MCP/tool capabilities, evidence stores and execution runtimes. These are outer
components and remain replaceable; their provider-specific details are not
part of the domain contract.
