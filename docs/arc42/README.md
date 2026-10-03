# Cognitive Gateway — arc42 Architecture

This directory is the canonical technical architecture documentation for Cognitive Gateway.

0. [Vision](00-vision.md)
1. [Introduction and Goals](01-introduction-and-goals.md)
2. [Architecture Constraints](02-architecture-constraints.md)
3. [Context and Scope](03-context-and-scope.md)
4. [Solution Strategy](04-solution-strategy.md)
5. [Building Block View](05-building-block-view.md)
6. [Runtime View](06-runtime-view.md)
7. [Deployment View](07-deployment-view.md)
8. [Cross-Cutting Concepts](08-crosscutting-concepts.md)
9. [Architecture Decisions](09-architecture-decisions.md)
10. [Quality Requirements](10-quality-requirements.md)
11. [Risks and Technical Debt](11-risks-and-technical-debt.md)
12. [Glossary](12-glossary.md)

## Documentation rule

The repository documentation specifies the architecture. The GitHub Wiki may explain the architecture for end users, but it is not the technical source of truth.

## Current status companion

Arc42 describes both implemented architecture and explicitly identified target
boundaries. Use [the current architecture state](../current-architecture-state.md)
to distinguish IMPLEMENTED, PARTIAL and PLANNED capabilities as of the documented
reference date.

Companion boundary documents:

- [CGSL / SemanticTaskIR](../semantic-language-and-interpretation.md)
- [Codex local integration](../codex-local-integration.md)
- [MCP connector/plugin runtime](../mcp-connector-runtime.md)
- [Local model runtime](../local-model-runtime.md)
- [Learned procedures](../learned-procedures.md)
- [Learned procedure evaluation](../procedure-evaluation.md)
