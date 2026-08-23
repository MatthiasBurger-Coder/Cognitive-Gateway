# Tests

This directory contains cross-crate architecture and contract-test specifications. Unit tests remain colocated with the owning crate; integration tests that exercise more than one crate belong here when the workspace has behavior to exercise.

The bootstrap quality baseline is the executable architecture guard at [`../scripts/check-architecture.sh`](../scripts/check-architecture.sh), together with the workspace commands documented in the root README:

```text
cargo fmt --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The [`architecture/`](architecture/) area records cross-crate architecture-test contracts. Schema and Execution Context IR compatibility tests are added with the corresponding domain and catalog-loading slices; CG-01 intentionally introduces no provider, RAG or MCP behavior to test.

## Current test coverage

The current domain-primitive slice is covered by unit tests colocated with its implementation in `crates/gateway-domain/src/`. These tests cover:

- typed task, agent, skill, workflow, policy, execution-context and capability identifiers;
- identifier alphabet, boundaries, length limits and safe parsing;
- required text validation, including whitespace-only, control-character and length cases;
- `SchemaVersion` construction, formatting and malformed-version rejection;
- validated `TaskDescriptor` and `KnowledgeQuery` construction.
- validated `AgentDefinition`, `SkillDefinition`, `WorkflowDefinition` and
  `PolicyDefinition` construction;
- local relationship invariants and complete cross-definition validation via
  `DefinitionCatalog`;
- strict parsing and canonical representations for operating modes, execution
  profiles, workflow/gate/blocker states and constraint kinds;
- explicit lifecycle transitions and coordinated execution-state invariants;
- inspect/mutate capability classification and machine-resolvable capability
  contract metadata;
- mode/profile constraint checks and strict Agent/Skill document round trips.
- strict `ExecutionContextIR` JSON round trips, malformed payload handling,
  unknown enum rejection and unsupported-version rejection.

The context component has an integration test at [`../crates/gateway-context/tests/context_compiler.rs`](../crates/gateway-context/tests/context_compiler.rs). It verifies context compilation, preservation of domain values and all nine combinations of the three operating modes with the three independent execution profiles.

The domain acceptance fixture at
[`../crates/gateway-domain/tests/reference_scenarios.rs`](../crates/gateway-domain/tests/reference_scenarios.rs)
proves the EPIC reference context, all operating-mode/profile pairs,
workflow/gate/blocker states, authority constraints, negative cases and full
IR serialization round trips. Its scenario inventory and acceptance evidence
are documented in [`../docs/reference-scenarios.md`](../docs/reference-scenarios.md).

The catalog contract fixture at
[`../crates/gateway-registry/tests/catalog_contract.rs`](../crates/gateway-registry/tests/catalog_contract.rs)
loads the repository catalog, validates all Agent/Skill relationships and
asserts the complete generic Agent set, including the promoted specialist
Agents, and the catalog Skill set with generic-boundary checks.

The project-agnostic integration fixture at
[`../crates/gateway-registry/tests/project_agnostic.rs`](../crates/gateway-registry/tests/project_agnostic.rs)
proves that the `analysis-storage-architect` Agent and Skill resolve from the
catalog alone and that the conventional and explicit catalog loading APIs
produce the same snapshot. External project context is not an input to either
registry API; it remains an explicit runtime or retrieval concern.

The registry unit tests cover deterministic catalog loading, duplicate
detection, complete Skill reference integrity and deterministic dependency
resolution. The architecture guard also prevents external registry roots from
being reintroduced.
