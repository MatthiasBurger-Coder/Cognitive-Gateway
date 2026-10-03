# Schemas

This directory is the canonical home for versioned, machine-readable project and runtime contracts.

The governed experience v1 wire shape is in
[`experience.schema.json`](experience.schema.json). The EPIC-03 learning candidate and procedure v1 wire shapes are in
[`learning.schema.json`](learning.schema.json); their domain invariants and
digest rules are documented in [`../docs/learning-contracts.md`](../docs/learning-contracts.md).

The bootstrap slice reserves the following schema boundaries:

- `agent.schema.json`
- `skill.schema.json`
- `workflow.schema.json`
- `policy.schema.json`
- `project-state.schema.json`
- `execution-context.schema.json`

`project-state.schema.json` is reserved for a later project-state contract. The
current consuming-project configuration boundary is intentionally opaque and
request-scoped in `gateway-application`; it is not a catalog or domain
definition schema.

The Agent and Skill contracts are implemented by
[`agent.schema.json`](agent.schema.json) and [`skill.schema.json`](skill.schema.json),
with representative fixtures under [`examples/`](examples/). Their field
mapping and validation boundary are documented in
[`../docs/agent-skill-definition-contracts.md`](../docs/agent-skill-definition-contracts.md).
Both contracts optionally carry `provided_capabilities`, whose nested entries
are strict, typed and project-independent capability declarations. Capability
metadata is descriptive provider knowledge; it is not policy authority.
Definitions are stored under [`../catalog/`](../catalog/), the sole built-in
Agent/Skill catalog. The catalog boundary and loading rules are documented in
[`../docs/catalog-boundaries.md`](../docs/catalog-boundaries.md).
The `gateway-registry` crate provides deterministic JSON catalog loading. It
recursively discovers `*.json` files in lexical relative-path order, rejects
malformed or unsupported documents and duplicate canonical IDs, and exposes
the resulting documents in canonical ID order. Non-JSON files are outside the
current JSON adapter. `Registry::validate_integrity()` provides the separate
cross-definition reference and Skill dependency-graph validation step.
The CG-02 JSON wire contract for `ExecutionContextIR` is documented and
implemented by the domain crate in [`../docs/ir-serialization.md`](../docs/ir-serialization.md);
its JSON Schema artifact remains a later schema-loading deliverable.

The v2 Agent and Skill documents are self-contained: structured Skill content
and canonical required/related Skill references live in the definition itself.
Provenance, external content references and consuming-project `SKILL.md` paths
are not runtime fields. Schema documents must be versioned, fail closed on
invalid input and remain independent of concrete RAG, MCP and execution-runtime
technologies.

[`procedure-evaluation.schema.json`](procedure-evaluation.schema.json) describes CG-23
datasets, replay snapshots, results and reproducible evidence bundles. Rust validation
additionally verifies canonical procedure content, snapshot/bundle digests, exact
expected outcomes, coverage and evidence-bound lifecycle admission.

[`procedure-promotion.schema.json`](procedure-promotion.schema.json) describes the
CG-24 promotion journal and commands. Rust replay additionally enforces immutable
versions, evidence binding, lifecycle edges, canary bounds and exact safe rollback.
The schema does not authenticate actor/policy claims or confer runtime permission.

`model-profile.schema.json` defines the optional local model service profile v1.0.
See [the operator guide](../docs/local-model-runtime.md) for runtime discovery,
qualification binding and lifecycle transitions.

`model-benchmark-dataset.schema.json` defines CG-27 named/versioned cognitive
signal datasets, task input/output schemas, prompts and expected proposal labels.
The harness additionally validates uniqueness, capabilities and pinned provenance.

[`learning-signal.schema.json`](learning-signal.schema.json) and
[`offline-learning.schema.json`](offline-learning.schema.json) define CG-28
reference-only signals, datasets, recipes, run/release metadata and upgrade impact.
Admission, exact evidence validation and independent authority are enforced by
the application; JSON never grants training or rollout permission.
