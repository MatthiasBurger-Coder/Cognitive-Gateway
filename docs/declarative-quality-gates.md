# Declarative v0.1 quality gates and release evidence (CG-13)

## Run the complete gate

```sh
rustup component add rustfmt clippy llvm-tools-preview
cargo install cargo-llvm-cov --locked
python3 scripts/quality-gate.py
```

Run from a checkout with Bash, Git, Python 3.11+ and the Rust toolchain installed.
Start the local PostgreSQL service with `./scripts/start-postgres.sh` before a
local full gate run. The daemon coverage step executes the CG-22 database
integration test; CI provides an isolated PostgreSQL service automatically.
No provider, LLM, database or runtime service is needed. Cargo may need network
access to fetch dependencies; after provisioning, the core tests run locally.
`--output <new-directory>` chooses the evidence location; existing directories
are refused. There is no skip or reduced-threshold option. The runner invokes
the ordered commands in [quality-gates.json](../scripts/quality-gates.json).
The [Rust Quality workflow](../.github/workflows/rust.yml) executes this same
entry point on pull requests, main pushes and manual dispatch.

The gate runs architecture and gate-failure regression tests, formatting,
workspace build/tests, both installed CLI smoke checks, the CG-12 external
project export and independent replay, clippy and every established coverage
gate. Workspace tests include unit, component, contract and integration tests.
The domain, registry and daemon each retain their 95% aggregate line floor.
CG-08 resolver files and the existing CG-09 policy, CG-10 context and CG-11 CLI
file sets each retain their **per-file 95%** floor. The CG-14 closed-loop
application module has the same per-file floor, checked from the complete
application coverage report. Missing, duplicate, invalid,
zero-line and below-threshold coverage entries fail. Counts are compared before
rounding. The portable resolver checker discovers every `resolution*.rs` file
and shares the existing strict Python checker; the PowerShell entry point
remains available. No production source exclusions are added.

The CG-20 evaluation gate replays the versioned golden dataset, retains raw
objective metrics in `cg20-evaluation.json`, and enforces 95% line coverage
for both new evaluation production modules from the workspace report. See
[EPIC-02 evaluation](epic-02-evaluation.md). The synthetic dataset report and
the external-project fake-port replay are separate evidence types.

For focused diagnosis, use `./scripts/check-architecture.sh`,
`python3 -m unittest discover -s tests/architecture`, or the exact command
recorded for a failed step. Focused checks do not substitute for a complete run.

## Evidence and release decision

Each new evidence directory contains:

- `summary.json`: scope, Git revision, worktree status, source SHA-256 hashes,
  tool versions, start/end timestamps, exact commands, statuses and exit codes;
- numbered logs for executed gates, plus `worktree.patch` for tracked changes;
- raw JSON coverage reports and the threshold-check output in gate logs;
- `external-project-proof/`: exported synthetic caller inputs, canonical
  outputs and independently replayed CLI outputs;
- SHA-256 hashes for retained artifacts in the summary.

A source or revision change during execution invalidates the run. A failed
command stops the run with a nonzero exit code; later gates remain
`NOT_RUN`. A crash may leave `RUNNING`; this is never completion evidence.
CI retains the bundle even on failure for 30 days. Download and retain the
reviewed bundle with release records before CI retention expires. Hashes detect
accidental changes; the bundle is not a cryptographic attestation.

Release reviewer checklist:

- [ ] The tested source matches the proposed release commit; the complete
  summary says `PASS` and every gate says `PASS` with exit code zero.
- [ ] The worktree was clean for release qualification. Local dirty runs are
  development evidence; the patch and hashes identify their inputs but do not
  replace a clean committed source tree.
- [ ] Raw coverage reports and all required 95% threshold results are present.
- [ ] External proof and replay match, and frozen fixture changes, if any,
  received explicit contract review.
- [ ] The EPIC acceptance mapping below has been reviewed, including deferred
  adapter scope; documentation and dependency allowlist agree with the code.
- [ ] The bundle is retained with the release decision and identifies its Git
  revision and toolchain. Passing checks support review; they do not publish a
  release or automatically close the EPIC.

## EPIC #1 acceptance mapping

These rows map all acceptance groups in [EPIC #1](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/1)
to executable evidence. Paths below are relative to `crates/`; all named Rust
suites run under the mandatory workspace test step.

| EPIC acceptance criteria | Required evidence |
| --- | --- |
| Framework-independent core; mechanical dependency boundaries | `scripts/check-dependencies.py`, `scripts/check-architecture.sh`, `tests/architecture/test_dependencies.py`; exact graph in arc42 §5 |
| Project-agnostic canonical data; no profiles; explicit external input | `gateway-registry/tests/catalog_contract.rs`, `project_agnostic.rs`; `gateway-process/tests/process_platform_integration.rs`; `gateway-daemon/tests/declarative_cli/cg12.rs`; catalog/profile guard |
| Knowledge and capability ports are separate; future retrieval/model adapters cannot bypass authority | `gateway-application` port contracts and unit tests; `gateway-context/tests/compiled_context.rs` authority-injection cases; `gateway-application/tests/policy_application.rs`; CG-12 blocked/denied cases |
| Operating Mode and Execution Profile independent | `gateway-domain/tests/reference_scenarios.rs`, `gateway-context/tests/context_compiler.rs` (all nine combinations) |
| Versioned ExecutionContextIR; no silent declarative redefinition | domain execution-context unit contracts, `gateway-domain/tests/execution_context_v2.rs`, `gateway-context/tests/v2_handoff.rs`; CG-12 typed projection and frozen compiled outputs |
| Structured planning/resolution without LLM; deterministic Skill dependencies | `gateway-application/tests/cg07_end_to_end.rs`, `cg08_end_to_end.rs`; registry unit tests; CG-12 frozen Plan and resolution |
| Deterministic process compilation/runtime and policy | `gateway-process/tests/coverage_contracts.rs`, `process_platform_integration.rs`; `gateway-policy/tests/policy_engine.rs`; frozen explanation with process and policy decisions |
| Workflow/Process registry and lifecycle contracts (CG-05 merged into CG-04) | `gateway-process` registry unit tests; `gateway-process/tests/process_platform_integration.rs` registry identity/pinning and migrated catalog proof; `execution_graph_boundary.rs`; application resolution/process tests |
| Malformed/conflicting canonical definitions rejected | `gateway-domain/tests/definition_contracts.rs`, registry unit tests, process unit/contract tests, CLI malformed-input cases |
| Desired State and retrieval grant neither permissions nor process transitions | CG-12 `desired_state_and_evidence_cannot_authorize_or_unblock_execution`; `gateway-application/tests/resolution_process.rs`, `policy_application.rs`; context authority-injection tests |
| Original input distinct; no wholesale conversation injection; dynamic evidence retains provenance | `gateway-application/tests/cg06_end_to_end.rs`, `context_application.rs`; `gateway-context/tests/compiled_context.rs`; CG-12 minimal context, exact input and selected evidence checks |
| External/retrieved data cannot become authority; stable catalog distinct from runtime context | `gateway-registry/tests/project_agnostic.rs`; `gateway-context/tests/compiled_context.rs`, `v2_handoff.rs`; CG-12 external working directory and negative authorization cases |
| Unit/component/contract coverage; valid registry/process definitions; established coverage thresholds | Complete workspace tests and every crate/file coverage command in the gate manifest |
| Deterministic regression fixtures for planning/resolution/process/policy/context | `tests/fixtures/declarative-v0.1/*.json`, asserted by CG-12; `gateway-application/tests/fixtures/resolution-trace-golden.json`, asserted by `resolution_explain.rs` |
| Automated external-project vertical slice | CG-12 tests plus installed `cg` replay; six JSON outputs compared to tested exports |
| Inspectable assessment and DesiredState → Delta → Plan lineage | `gateway-application/tests/cg06_end_to_end.rs`, `cg07_end_to_end.rs`; frozen assessment/plan with exact evidence references |
| Explainable Capability/Agent/Skill/Process selection and rejection | `gateway-application/tests/resolution_explain.rs`, `resolution_candidates.rs`, `resolution_composition.rs`; CG-12 missing/unknown requirements |
| Stable process blocker and policy reason codes | process runtime/contract tests, `gateway-policy/tests/policy_engine.rs`, `gateway-application/tests/policy_application.rs`; frozen explanation |
| Inspectable context selection and provenance | `gateway-context/tests/compiled_context.rs`, `gateway-application/tests/context_application.rs`; both frozen per-step compiled contexts |

The former CG-05 Workflow Registry scope was [merged into CG-04](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/6).
`gateway-process` owns the single registry and lifecycle authority;
`gateway-workflow` remains a compile-only placeholder with no behavioral tests.
The complete gate builds it without claiming a second registry implementation.

Future retrieval, learned-model and execution adapters are attachment contracts
in this scope. The gate proves current boundary and denial behavior; it does
not claim those future implementations exist. The declarative slice compiles
an authorized context and does not execute or verify a real project mutation.

## Deterministic fixture review

The six [frozen outputs](../tests/fixtures/declarative-v0.1/) use the synthetic
CG-12 architecture/coverage scenario. They cover Situation assessment, Delta,
Plan, capability resolution, process/policy explanation and two minimal
ExecutionContextIR projections. JSON object key order is ignored; all values
and array order are compared exactly. Reordered captured evidence must still
produce the same Plan and resolution. Existing resolution trace fixtures
also cover rejection/ambiguity paths.

To investigate an intentional change, export CG-12 outputs as documented in
[the external-project proof](declarative-end-to-end.md), compare every changed
field against its contract, and update fixtures in a reviewed change. A failed
fixture assertion deliberately prevents export; inspect the assertion and
review the contract before updating expected data. Tests never regenerate
fixtures automatically. Synthetic expected outputs under `tests/fixtures/`
are test evidence, not project configuration or canonical catalog membership.
