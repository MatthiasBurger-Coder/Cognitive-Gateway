# Tests

This directory contains cross-crate architecture and contract-test specifications. Unit tests remain colocated with the owning crate; integration tests that exercise more than one crate belong here when the workspace has behavior to exercise.

The bootstrap quality baseline is the executable architecture guard at [`../scripts/check-architecture.sh`](../scripts/check-architecture.sh), together with the workspace commands documented in the root README:

```text
cargo fmt --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The [`architecture/`](architecture/) area records cross-crate architecture-test contracts. Schema and Execution Context IR compatibility tests are added with the corresponding domain and profile-loading slices; CG-01 intentionally introduces no provider, RAG or MCP behavior to test.

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
- inspect/mutate capability classification and mode/profile constraint checks.
- strict `ExecutionContextIR` JSON round trips, malformed payload handling,
  unknown enum rejection and unsupported-version rejection.

The context component has an integration test at [`../crates/gateway-context/tests/context_compiler.rs`](../crates/gateway-context/tests/context_compiler.rs). It verifies context compilation, preservation of domain values and all nine combinations of the three operating modes with the three independent execution profiles.

The domain acceptance fixture at
[`../crates/gateway-domain/tests/reference_scenarios.rs`](../crates/gateway-domain/tests/reference_scenarios.rs)
proves the EPIC reference context, all operating-mode/profile pairs,
workflow/gate/blocker states, authority constraints, negative cases and full
IR serialization round trips. Its scenario inventory and acceptance evidence
are documented in [`../docs/reference-scenarios.md`](../docs/reference-scenarios.md).

The catalog migration fixture at
[`../crates/gateway-registry/tests/catalog_migration.rs`](../crates/gateway-registry/tests/catalog_migration.rs)
loads the repository catalog, validates all Agent/Skill relationships and
asserts the complete CG-03.07 Agent set and CG-03.08 Skill set, including
provenance, merge status and generic-boundary checks.

The profile migration fixture at
../crates/gateway-registry/tests/profile_migration.rs loads the Tiny Swarm
World profile, asserts the complete CG-03.09 Agent/Skill set, verifies
provenance and confirms that scope-gated candidates remain excluded. It also
validates the combined catalog/profile boundary.
