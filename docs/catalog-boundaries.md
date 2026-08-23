# Agent and Skill catalog boundary

The Cognitive Gateway repository owns one built-in, project-independent Agent
and Skill catalog. Reusable responsibility and skill semantics live under
`catalog/`; consuming-project knowledge and configuration enter only through
explicit runtime, retrieval or adapter boundaries.

## Repository layout

```text
catalog/
├── agents/
└── skills/
```

`catalog/agents/` is the only Agent discovery boundary. `catalog/skills/` is
the only built-in Skill discovery boundary. Both use the strict v2
self-contained JSON contracts under `schemas/`.

## Loading contract

The `gateway-registry` crate discovers JSON definitions in lexical relative
path order and exposes them in canonical ID order:

| API | Behavior |
| --- | --- |
| `Registry::load_catalog(path)` | Loads `path/agents` and `path/skills` as the built-in catalog. |
| `Registry::load(path)` | Alias for loading one catalog directory. |
| `Registry::load_from_directories(agents, skills)` | Loads explicit catalog boundaries for composition and tests. |

Malformed documents, unsupported schema versions and duplicate canonical IDs
fail closed. `Registry::validate_integrity()` then validates Agent-to-Skill,
Skill-owner, related-Skill and mandatory dependency references, as well as
equivalence of shared capability declarations. There is no secondary built-in
registry, merge operation or override rule.

The project-agnostic integration proof is
[`crates/gateway-registry/tests/project_agnostic.rs`](../crates/gateway-registry/tests/project_agnostic.rs).
It loads the repository catalog without a project directory, validates the
`analysis-storage-architect` Agent and Skill together, resolves the complete
`resilience-engineering -> analysis-storage-architect` dependency closure, and
verifies that conventional and explicitly supplied catalog boundaries produce
the same registry snapshot.

## Resolution and runtime context

`Registry::resolve_skill(skill_id)` resolves only the requested canonical Skill
ID and its transitive `requires` closure. It returns complete Skill documents
in deterministic dependency-first order. `related_skills` remain informational
references and do not activate additional graph members.

## Capability index and query

`Registry::capability_index()` builds a rebuildable, owned read model from the
validated `provided_capabilities` declarations on Agents and Skills. The index
is not an authority boundary: Git-owned catalog documents remain the source of
truth, and rebuilding the index does not add or remove catalog membership.

`CapabilityQuery::new(capability_id)` performs an exact canonical ID lookup.
Queries can additionally use explicit typed selectors for class, domain, input
and output kinds, intrinsic preconditions and constraints, or applicability
tags. Selectors are conjunctive and list selectors require the selected value
to be declared by the capability. `CapabilityQuery::all()` supports a
structured-selector query across all capability IDs. Matching uses no text
similarity, embeddings, retrieval or inference.

Results preserve every matching Agent and Skill provider in stable canonical
order. Each `CapabilityCandidate` includes the matched declaration, its
canonical source, owner-Agent relationship, direct Skill relationships and the
mandatory Skill dependency closure in dependency-first order. Rejected
candidates retain the failed selectors as explainability data. Results
explicitly report `NoMatch`, `Unique` or `Ambiguous`;
`CapabilityIndex::resolve_unique` fails closed for the first and last outcomes.

The index builder validates shared capability metadata before materialization.
If two providers declare one capability ID with different reusable metadata,
the existing `ConflictingCapabilityDeclaration` integrity error is returned.
Multiple providers with equivalent metadata remain visible as candidates and
are never silently collapsed to one provider.

Resolution requires only the built-in catalog. Repository content, project
configuration, retrieved knowledge, external `SKILL.md` files and runtime
state are contextual inputs owned by explicit ports or adapters; they cannot
create or change Agent/Skill catalog membership.

## Definition boundary

Agent and Skill documents contain reusable declarative semantics, typed
canonical IDs, abstract capability requirements and optional typed
`provided_capabilities` contracts. They do not contain consuming-project
identities, repository paths, provenance fields, external content references or
runtime authorization. Technology-specific expertise is valid when it remains
reusable across consumers.
