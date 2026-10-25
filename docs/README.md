# Cognitive Gateway Documentation

This repository contains the canonical technical documentation for Cognitive Gateway.

## Current architecture and implementation status

See [current architecture state](current-architecture-state.md) for the dated implementation-versus-plan matrix. This distinction is normative: an Epic or target architecture does not by itself mean the runtime capability is implemented.

## Architecture

See [diagram conventions](diagram-conventions.md) for the normative Mermaid-first documentation rule.

See [`arc42/`](arc42/) for the living architecture documentation.

## Shared session contracts

[Shared session contracts and Codex binding](shared-session-contract.md) and
[ADR-021](adr/ADR-021-shared-structured-session-ownership.md) define the accepted
structured artifact goal, typed API, trusted interactions, version boundary and
prerequisite/evidence gates. Contract definition does not implement session services.

## Architecture Decisions

See [`adr/`](adr/) for accepted architecture decisions.

## CGSL implementation documentation

Start with the [CGSL implementer guide](cgsl-implementer-guide.md) for the complete
normative reading path, compiler stages and current delivery boundary. Use the
[reference walkthroughs](cgsl-reference-examples.md) to trace intent through task
semantics, planning, Process IR and prompt layouts, and the
[migration guide](cgsl-migration.md) for existing structured consumers and version
evolution.

See [Context requirement analysis and source selection](context-requirement-planning.md)
for EPIC-05.17 canonical contracts, CG-15 mapping, actual CG-10 handoff, examples
and measured qualification.

## Core domain contract

See [CGSL scope and canonical vocabulary](cgsl-scope-and-vocabulary.md) for
EPIC-05.01 task semantics, the finite formal-core vocabulary and boundaries
to existing domains. The [semantic interpretation architecture](semantic-language-and-interpretation.md)
distinguishes this specification from planned compiler/runtime capabilities.

See [CGSL conformance and compatibility](cgsl-conformance.md) for EPIC-05.15
frozen cross-implementation fixtures, roundtrip properties and adversarial component evidence.

See [Rust CGSL frontend](cgsl-frontend.md) for the offline parser/compiler API,
source locations, resource profile and verification commands.
See [Deterministic interpretation resolution](deterministic-interpretation-resolution.md)
for scoped entity/alias lookup, discourse and temporal resolution, source traces
and the EPIC-05.09 component verification boundary.
See [Completeness validation and clarification planning](semantic-completeness.md)
for the EPIC-05.10 application service, dependency ordering and reply revalidation.
See [CGSL 1.0 grammar, serialization and diagnostics](cgsl-language-v1.md) for
EPIC-05.07 normative syntax, source/IR roundtrip rules, diagnostic categories
and [complete examples and counterexamples](../examples/cgsl-v1/README.md).

See [Interpretation Context, precedence and lifecycle](interpretation-context.md)
for EPIC-05.03 request-scoped meaning, typed focus/history, isolation, conflict
rules and reference decision vectors. Host/runtime integration remains planned.

See [Ambiguity, conflict and knowledge-gap contracts](semantic-resolution-contracts.md)
for EPIC-05.04 domain types, stable diagnostics and mandatory compilation blockers.

See [Evidence, assumptions and epistemic semantics](semantic-epistemic-contract.md)
for EPIC-05.05 classification, source-preserving evidence references, admissibility
and assumption lifecycle contracts.

See [`retrieval-plane.md`](retrieval-plane.md) for CG-15 versioned retrieval,
embedding lineage, token estimation, reservations and bounded execution contracts.
See [`retrieval-pipeline.md`](retrieval-pipeline.md) for CG-16 federated source
dispatch, contextual metadata, deterministic hybrid fusion and reranking boundaries.
See [`recursive-retrieval.md`](recursive-retrieval.md) for CG-19 bounded rounds,
validated evidence sufficiency and CG-14 pause integration.
See [`reasoning-strategy.md`](reasoning-strategy.md) for CG-20C versioned
reasoning strategies, aggregate budgets and the typed adapter handoff.
See [`epic-02-evaluation.md`](epic-02-evaluation.md) for CG-20 objective
evaluation, baseline policy, profiling and curated learning export.
See [`epic-02-release-qualification.md`](epic-02-release-qualification.md)
for CG-20D requirement traceability and release readiness.
See [`knowledge-plane-hardening.md`](knowledge-plane-hardening.md) for CG-20A
trust boundaries, disclosure limited handoffs, and adversarial verification.

See [`resolution-contract.md`](resolution-contract.md) for CG-08 resolution
result semantics, immutable basis references and downstream ownership.
See [`resolution-snapshots.md`](resolution-snapshots.md) for coherent read-only
capture, scope isolation, canonical provenance and basis revalidation.
See [`resolution-candidates.md`](resolution-candidates.md) for exact typed
provider matching and the distinction between metadata and applicability.
See [`resolution-process.md`](resolution-process.md) for optional/pinned
Process templates, explicit lifecycle mappings and activity contracts.
See [`resolution-agents.md`](resolution-agents.md) for canonical Agent
responsibility alternatives and primary/participating role constraints.
See [`resolution-skills.md`](resolution-skills.md) for conditional activation,
required Skill closure, transitive capabilities and cycle/limit diagnostics.

See [`domain-model.md`](domain-model.md) for the consolidated CG-02 domain
contract: typed primitives, definitions and relationships, execution state,
capabilities, constraints and the provider-independent architecture boundary.

See [`execution-context-ir.md`](execution-context-ir.md) for the field-level
`ExecutionContextIR` v1 contract and invariants.

See [`ir-serialization.md`](ir-serialization.md) for the JSON wire schema,
validation behavior, public serialization API and version compatibility rules.

See [`agent-skill-definition-contracts.md`](agent-skill-definition-contracts.md)
for the CG-03.16 versioned Agent, Skill and machine-resolvable capability
document contracts, strict exclusions and representative normalized fixtures.

See [`reference-scenarios.md`](reference-scenarios.md) for the CG-02.06
executable acceptance scenarios covering the complete domain and IR contract.

See [`catalog-boundaries.md`](catalog-boundaries.md) for the Agent and Skill
catalog layout, ownership rules, loading APIs, deterministic capability index
and query behavior, and fail-closed semantics.

See [`project-context-boundary.md`](project-context-boundary.md) for the
request-scoped consuming-project configuration and retrieval provenance
contract.

See [`declarative-context-situation.md`](declarative-context-situation.md)
for the CG-06.01 declarative context and situation IR v1 foundations,
typed identities, aggregate ownership and versioning rules.

See [`declarative-planning.md`](declarative-planning.md) for the CG-07.01
Delta and declarative Plan IR v1 contracts, typed identities and ownership
boundaries.

See [`comparison-semantics.md`](comparison-semantics.md) for the CG-07.02
deterministic DesiredState-to-CurrentState comparison algebra and trace rules.

See [`delta-derivation.md`](delta-derivation.md) for the CG-07.03 deterministic
Delta classification, required outcomes, identity and lineage rules.

See [`planning-inputs.md`](planning-inputs.md) for the CG-07.04 information,
evidence, freshness, conflict and unresolved-input planning semantics.

See [`deterministic-planner.md`](deterministic-planner.md) for the CG-07.07
rule-based planner, stable decision identities, dependency semantics and
fail-closed diagnostics.

See [`plan-validation.md`](plan-validation.md) for the CG-07.08 deterministic
validation, canonical JSON serialization, semantic round-trip and
DesiredState-to-Plan explainability contract.

See [`planning-application.md`](planning-application.md) for the CG-07.09
stateless application facade, explicit CG-03/CG-06 snapshots and
provider-neutral output boundary.

See [`planning-end-to-end.md`](planning-end-to-end.md) for the CG-07.10
end-to-end acceptance proof, reference external-project scenario, negative
variants and ownership-boundary evidence.

See [`registry-inspection-cli.md`](registry-inspection-cli.md) for the CG-03
read-only registry and capability inspection commands, JSON output and exit
codes.

See [`tiny-swarm-world-process-inventory.md`](tiny-swarm-world-process-inventory.md)
and its [machine-readable inventory](tiny-swarm-world-process-inventory.json)
for the CG-04.14 TSW process-semantic classification and migration gaps.

See [`execution-graph-extension-boundary.md`](execution-graph-extension-boundary.md)
and its [machine-readable gap matrix](execution-graph-migration-gaps.json) for
the CG-04.16 lifecycle-versus-scheduling boundary and explicit S3D
dispositions.

See [`process-platform-integration-proof.md`](process-platform-integration-proof.md)
for the CG-04.17 vertical Rust-only integration proof and test matrix.

See the materialized reusable Agent definitions under [`../catalog/agents/`](../catalog/agents/)
and the reusable Skill definitions under [`../catalog/skills/`](../catalog/skills/).
Specialist Agents are normal catalog entries, and all catalog Skill references
use canonical IDs from the same built-in catalog.

## Documentation Policy

See [resolution application API](resolution-application-api.md) for CG-08.11 policy
handoff, every CG-02 v1 field mapping and open owner decision CG08-PROJECTION-01.

See [resolution artifacts](resolution-artifacts.md) for CG-08.10 canonical
serialization, bounded strict parsing and independent replay validation.

See [resolution explainability](resolution-explainability.md) for CG-08.09 typed
trace graphs, rule fingerprints, consistent projections and privacy boundaries.

See [whole-binding composition](resolution-composition.md) for CG-08.08 bounded
search, explicit ranking, ambiguity and partial-result semantics.

See [resolution applicability](resolution-applicability.md) for CG-08.07 readiness,
completion-evidence and process authority boundaries.

- Repository documentation is the technical source of truth.
- GitHub Wiki is intended for simplified end-user documentation, tutorials and operational guidance.
- Architectural or governance changes must update repository documentation in the same development flow as the corresponding code/configuration change.
- Wiki pages should link back to canonical repository documentation where appropriate.

See [CG-08 end-to-end proof and acceptance decision](resolution-end-to-end-proof.md)
for the neutral CG-07 integration, negative-case matrix, per-file coverage gate
and the unresolved CG-02/CG-10 projection blocker. Passing resolver tests do not
constitute final parent acceptance.

The approved CG-02 v2 handoff extension is documented in
[ExecutionContextIR v2](execution-context-ir-v2.md).

## Current documented decisions

1. Rust core + Python cognitive services.
2. Repository docs as technical authority; Wiki as end-user view.
3. Git authority, RAG knowledge retrieval, MCP/tool capabilities.
4. Deterministic workflow/agent/skill resolution before probabilistic retrieval.
5. Execution-runtime independence: Codex, PraisonAI and other runtimes are adapters.
6. Operating Mode and Execution Profile are independent dimensions.
7. Versioned Execution Context IR as the core runtime integration contract.
8. Hexagonal Architecture with inward dependencies and replaceable ports/adapters.
9. Git is authoritative for declarative Agent/Skill/Workflow/Policy definitions; runtime databases own mutable execution state, while SQL/graph/vector stores used for definition lookup are derived and rebuildable read models.
10. CGSL/SemanticTaskIR supplies the implemented component semantic boundary between natural language and deterministic planning; unresolved mandatory semantics must not be guessed.
11. MCP is an adapter protocol, never an authority source: inbound Codex integration (EPIC-04) and outbound connector integration (EPIC-07) are separate responsibilities.
12. Local models are optional replaceable cognitive services behind stable ports; model identity/version/digest/quantization and qualification are explicit, and model output is non-authoritative.
13. Learned procedures are versioned governed artifacts derived from eligible experience; history does not grant process or policy authority.

## Semantic interpretation and runtime boundaries

- [CGSL, SemanticTaskIR and natural-language interpretation](semantic-language-and-interpretation.md) — implemented formal compiler and bounded original-text inspection profile; general hosted interpretation remains a separate runtime path.
- [Versioned Codex-facing contracts](codex-facing-contracts.md) — EPIC-04.02 schemas, catalog, diagnostics, compatibility and frozen session projections.
- [`codex-application-facade.md`](codex-application-facade.md): implemented application facade, canonical delegation and trusted host ports (#239).
- [Codex scope isolation](codex-scope-isolation.md) — trusted workspace/session binding, immutable resources and provenance (#240).
- [Codex secret isolation](codex-secret-isolation.md) — credential admission, sensitive-data projection and sanitized diagnostics (#241).
- [EPIC-04.07 Codex operation policy and consent gates](codex-policy-gates.md)
- [Codex bounded runtime](codex-runtime.md) — invocation deadlines, cancellation, backpressure, diagnostics and health (#243).
- [Codex local integration](codex-local-integration.md) — EPIC-04 Codex -> CG local no-key MCP boundary.
- [Codex local setup and operator runbook](codex-local-setup.md) — bootstrap, secret-free configuration, CLI fallback and reproducible smoke tests (#244).
- [Codex inbound component qualification](codex-release-qualification.md) — executable goldens, security/failure evidence and requirement mapping; shared session lifecycle acceptance remains pending (#245).
- [Local MCP server](local-mcp-server.md) — #238 stdio lifecycle, discovery and transport; #239 facade integration.
- [Connector manifest v1](connector-manifest-v1.md) — EPIC-07.02 identity, canonical serialization and deterministic compatibility contract.
- [MCP connector/plugin runtime](mcp-connector-runtime.md) — EPIC-07 architecture, compilable ports and manifest contracts; links to the implemented protocol runtime and pending production integrations.
- [Outbound MCP client runtime](mcp-client-runtime.md) — EPIC-07.03 Rust protocol, bounded stdio, private admitted dispatch, lifecycle and qualification boundary.
- [Local model runtime / SLM-LxM boundary](local-model-runtime.md) — CG-27 signal adapters and standalone CPU/GPU benchmark framework; CG-27.01 optional service and qualification lifecycle; Qwen3-8B is a reference candidate only.
- [Learned procedures](learned-procedures.md) — CG-21 through CG-30 govern the complete reference learning/reflex lifecycle; see complete EPIC-03 acceptance.
- [Learned procedure evaluation](procedure-evaluation.md) — CG-23 validation, replay, simulation and evaluation evidence.
- [Learned procedure promotion](procedure-promotion.md) — CG-24 authoritative version registry, canary, supersession, rollback and audit inspection.

## Policy authorization

- [CG-09 Policy Engine and Inspect/Mutate separation](policy-engine.md)

## Context compilation

- [Prompt IR handoff boundary](prompt-ir-handoff-boundary.md) — #192 section ownership, admission, CG-26 attachment and two offline provider mappings; canonical planning/example integration remains pending.
- [CG-10 Context Compiler, semantic TAG and ExecutionContext projection](context-compiler.md)
- [CG-17 knowledge graph and graph retrieval](knowledge-graph.md)

- [CG-11 declarative CLI: commands, JSON inputs, policy boundary and exit codes](declarative-cli.md)
- [CG-12 external project proof: evidence, authorization and reproducible CLI replay](declarative-end-to-end.md)

## Declarative v0.1 release quality

Run `python3 scripts/quality-gate.py` for the complete CG-13 gate. See [the release checklist](declarative-quality-gates.md) for requirements, evidence and EPIC acceptance traceability.

## Closed-loop execution

- [CG-14 execution outcomes, evidence-backed goals, replanning, budgets and audit](closed-loop-execution.md)
- [CG-28A bounded parallel task execution and deterministic joins](parallel-execution.md)

- [Governed memory and experience retrieval](governed-memory.md) — CG-18 lifecycle, eligibility and context bridge.
- [Learned procedures and governed procedural learning](learned-procedures.md) — CG-21 domain contracts, lifecycle and immutable procedure versions.
- [Experience normalization and pattern inspection](experience-patterns.md) — CG-22 bounded correlation and read-only findings.
- [PostgreSQL Compose service](postgres-compose.md) — optional persistent database installation and operations.

- [Deterministic reflex engine](reflex-engine.md) — CG-25 ACTIVE matching, evidence gates, governed execution, verification and fallback.

- [Explainable cognitive model router](cognitive-model-router.md) — CG-26 typed capability snapshots, deterministic precedence, hard constraints, bounded fallback and execution provenance.

- [CG-28 governed learning signals and offline training](offline-learning.md) — evidence admission, reproducible scoped datasets, offline interfaces and model rollout/rollback.

- [v0.3 cognitive runtime release qualification](epic-03-release-qualification.md) — CG-30 integrated acceptance, classification/routing benchmarks, injected failures and clean-candidate evidence admission.

- [CG-29 distributed cognitive worker fabric](worker-fabric.md) — immutable snapshots, scoped scheduling, fencing, bounded retries, local adapter and orchestrator boundary.

- [Complete EPIC-03 acceptance](epic-03-complete-acceptance.md) — original criteria plus ML lifecycle, concrete CPU training/inference, PostgreSQL restart/rollback and isolated workers.

- [Full EPIC-04 acceptance](epic-04-acceptance.md) — all 24 parent criteria and fail-closed closure enforcement.

- [SemanticTaskIR v1 and versioning](semantic-task-ir-v1.md) — implemented Rust/wire contract, invariants, pinned references and compatibility; see the implementer guide for delivered compiler components and remaining runtime integration.
- [Optional interpretation adapters](interpretation-adapters.md) — deterministic-first classifier, embedding and SLM proposals, validation, provenance and fallback.

- [Dynamic example and few-shot context selection](dynamic-example-selection.md) — #286 contracts, eligibility, budgets and canonical CG-10 handoff.

- [Model invocation contracts](model-invocation-contracts.md) — #261 immutable capabilities, providers, generation profiles, invocation bindings and strict parameter negotiation.

- [Deterministic model capability routing](model-capability-routing.md) — EPIC-06.02 eligibility, CG-26 integration, diagnostics and evidence.

- [Provider-independent prompt and invocation IR](prompt-invocation-ir.md) — #263 admission, correlation and current-host integration.

- [Schema, reference and constraint verification](output-verification.md) — #266 typed, correlated verification and trusted-host integration.

- [Structured output parsing and tool proposals](structured-output-parsing.md) — #265.

- [Evidence and grounding verification](evidence-grounding-verification.md) — #267 source pins, CG-06 support and observed effects.

- [Bounded invocation retry, cancellation and degradation](invocation-resilience.md) — shared run budgets, fallback admission and usage reconciliation (#268).

- [Local and independent model provider adapters](model-provider-adapters.md) — #269 conformance, runtime-only credentials and opt-in live transport qualification.

- [Invocation trace, replay and role qualification](invocation-qualification.md) — EPIC-06.10 component evidence and live release boundary.

- [Deterministic connector mapping v1](connector-mapping-v1.md) — EPIC-07.04 explicit capability/source mapping, schema validation, atomic inventory and runtime qualification.

EPIC-07.05 #228 implements [canonical connector result normalization](connector-result-v1.md),
with CG-06 lineage, scope/quality propagation, acknowledgement/read-back separation
and reference-only context adapters. Application port revision 2 requires manifest
and inventory regeneration. Production credentials/policy and live effects remain
separate qualification work.
