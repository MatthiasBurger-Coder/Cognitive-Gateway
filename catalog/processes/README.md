# Canonical Process Catalog

This directory is the Git-owned source of truth for reusable
Strict Cognitive Gherkin process definitions. The Rust gateway-process
registry discovers feature files recursively, sorts paths deterministically,
compiles and validates every definition, then derives runtime registry state.

Catalog membership is never inferred from a consuming project, retrieval
result, agent, tool or external workflow runtime.
