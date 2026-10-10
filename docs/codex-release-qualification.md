# Inbound no-key component qualification — EPIC-04.10 #245

The qualification harness starts the real Rust `cg-mcp` and `cg-local` executables
with empty environments, binds explicit temporary workspace roots, and exchanges
newline-delimited JSON-RPC with a synthetic `codex / 1.0` client using MCP
`2025-11-25` and frozen application schema `1.0`. It requires no provider key,
Codex account, network listener, connector, model service or database.

The component result is distinct from EPIC-04 completion and the full runtime
qualification in #279. The standalone immutable host supports situation
inspect/assess and scoped resources. Canonical inspect/resolve/explain/context also
cross MCP and the facade using injected Rust hosts and existing canonical service
fixtures. Shared session projections are
qualified through `Server -> CodexFacade -> CodexHost`, with current CG policy and
consent checks. The projection fixture supplies running, clarification pause,
consent pause and cancelled results; it implements no task coordinator. Shared
durable services #272/#273/#275 are absent. Supported session lifecycle operations
against those services remain an acceptance gap; this report does not close #245.

## Reproduction and retained evidence

Run from the checkout root with Rust/Cargo, cargo-llvm-cov, Python 3.11+ and the
packages in `tests/contracts/requirements.txt` installed:

```sh
python3 scripts/qualify-codex-local.py --output target/epic04-qualification
```

The output directory must be new. The harness builds binaries, runs canonical,
security, bridge, failure, executable golden, protocol/schema and architecture
checks, formatting and workspace Clippy. It measures the established thirteen-file
MCP coverage gate at >=95% per file. Any gate failure, missing/low coverage, or
source change during execution returns nonzero and records `FAIL`. A successful
run records `QUALIFIED_COMPONENT_SCOPE` and `epic_04_status: NOT_COMPLETE`.
`report.json` retains commands, exit codes, Git revision, worktree status, source
SHA-256 hashes, artifact hashes, timestamps, coverage and explicit limitations.
Logs retain individual test names. A dirty candidate is reviewable through its
source hashes and Git changes, but is not a clean release qualification.

The release quality manifest runs the same component harness after its existing
MCP coverage gate, supplying that run's coverage with `--coverage-report`. This
avoids repeating instrumentation; standalone runs measure coverage themselves.
Neither mode substitutes for the repository's complete release quality gate.

## Requirement-to-evidence matrix

All paths below are relative to the checkout root. No installed Codex client
version is qualified by the fixture identity.

| #245 criterion | Objective evidence | Qualification boundary |
| --- | --- | --- |
| Local no-key invocation succeeds | `tests/codex-local/test_qualification.py::test_golden_no_key_inspection_determinism_and_cli_parity`; `inspect.response.json` | Real executables/private pipes, empty CG environment |
| Equivalent canonical inputs/outputs | Same golden across reordered JSON, repeated calls, fresh connections and CLI; repeated canonical resolve/explain/evidence assertions in `codex_facade.rs` | Complete application envelope compared; transport correlation/timing is separate |
| Cross-workspace isolation | Executable cross-workspace scenario; `codex_isolation.rs::two_admitted_projects_with_overlapping_source_ids_cannot_share_context`, symlink/session/cache isolation tests | Explicit roots and bindings; no ambient scope |
| Secret/provider credentials do not leak | Executable credential/sensitivity scenario; `local_mcp.rs` credential environment/auth-store/request tests; `codex_facade.rs` security cases | Fake credentials only; fixed diagnostics, no echo |
| Policy/mutation denial and consent | Executable denied profile/unsupported mutations; `codex_facade.rs::policy_gates`; `codex_qualification.rs::client_approval_cannot_replace_trusted_consent_or_dispatch_session_mutations` | Current trusted policy/consent; discovery supplies no grant |
| Unsupported protocol/schema versions | Executable wrong-version/malformed/duplicate scenario; `codex_facade.rs::validation_precedence_scope_versions_and_pins_fail_closed` | Fail closed; no downgrade |
| Timeout/cancel/disconnect | Executable idle deadline, EOF and reconnect; `local_mcp::runtime::tests::active_cancel_disconnect_errors_and_deadline_are_bounded`, overload/panic/write/frame-budget faults | Active faults use injected ports/transports; no durable rollback claim |
| Resolve/explain/context/provenance | `codex_qualification.rs::canonical_inspect_resolve_explain_and_context_are_deterministic_through_mcp`; facade stale-step/policy/lineage regressions | MCP bridge to canonical Rust services through fixture hosts; standalone operations remain unsupported |
| No provider dependency in inner authority | `scripts/check-architecture.sh`, `test_dependencies.py`, `test_local_mcp_boundary.py` | Cargo dependency mutations plus framing/module guards |
| Architecture remains green | `architecture.log`, `architecture-regressions.log`, workspace Clippy | Existing gates unchanged |
| >=95% materially changed production coverage | `local-mcp-coverage.json`, per-file coverage in report | No production Rust changed; existing gate still required |
| Session start/status, pauses and cancellation projections | `codex_qualification.rs::shared_host_start_status_pause_and_cancellation_projections_cross_the_bridge`; `session-projections.json` | Shared host contract projection proof; durable service lifecycle remains pending |
| All EPIC-04 criteria have objective evidence | This matrix and slice mapping below | Component evidence complete; full EPIC-04 acceptance remains pending shared service integration |
| Distinguish inbound from #279 full runtime | Machine report scope/status/limitations | No connector/model completion or full runtime release claim |

## EPIC-04 slice traceability

The parent #126 acceptance criteria map individually as follows. These are
component proofs; the installed-client and integrated-session gaps above remain
explicit even when the listed regression tests pass.

| Parent criterion (in issue order) | Reproducible automated evidence |
| --- | --- |
| Local supported connection without a CG key | Executable golden/parity scenario in `test_qualification.py` |
| Codex authentication outside canonical authority | `local_mcp.rs::inherited_credential_environment_is_denied_without_echoing_values`, including the untouched auth-store fixture |
| No MCP/provider types in authoritative domain | `test_dependencies.py`, `test_local_mcp_boundary.py` and dependency guard |
| Explicit workspace/project/session scope | `codex_isolation.rs` root/session/principal/connection regressions |
| Equivalent canonical results | Executable golden ordering/reconnect/parity; canonical MCP bridge determinism test |
| Provenance, revision, sensitivity and lineage preserved | `codex_facade.rs::canonical_situation_validation_and_lineage_projection`; `codex_isolation.rs::immutable_reference_provenance_survives_inline_and_reference_mapping` |
| CG policy/consent authorizes exposed operations | `codex_facade.rs::policy_gates::mutation_requires_current_policy_enablement_consent_and_evidence` |
| Discovery never authorizes | `policy_gates::discovery_and_permissive_host_do_not_supply_policy` |
| Mutation/admin denied without explicit authority | Executable unsupported session mutations; policy disabled/no-policy/explicit-deny regressions; closed frozen catalog |
| Client cannot grant capabilities/policy/process state | `policy_gates::read_and_inspect_never_accept_forged_authority_or_commands`; wrong-class/process-blocked/evidence tests |
| SECRET/reference-only/provider data absent from output/log/cache/trace | Facade `security_cases` projection/reference credential tests; `codex_isolation.rs` sensitivity/cache tests; real stdio credential injection |
| Unsupported versions fail closed | Executable wrong protocol/schema, admission version and frozen contract tests |
| Cancel/timeout/disconnect/malformed/overload bounded | Executable deadline/EOF/malformed tests; adapter runtime active fault and overload tests |
| Existing application services reused | Canonical inspect/resolve/explain/context MCP bridge using existing shared fixtures |
| No duplicate EPIC-07 connectors | Dependency/module guards; bridge host has no external dispatch; frozen thirteen-tool catalog test |
| CLI and MCP share facade | Full canonical response equality against the executable golden |
| Architecture dependency checks green | Required architecture guard and nineteen architecture regression tests |
| Applicable >=95% production coverage | Required thirteen-file measured local MCP coverage gate |
| Every criterion has reproducible evidence | This matrix, required runner gates and source/artifact hashes in `report.json` |

| Slice | Contract and evidence |
| --- | --- |
| #236 trust/no-key architecture | ADR-020, `codex-local-integration.md`; dependency and boundary mutation tests |
| #237 frozen versioned contracts | `codex-facing-contracts.md`; schema/fixture tests and independent live 13-tool protocol check |
| #238 local MCP server | `local-mcp-server.md`; lifecycle/discovery/malformed subprocess and adapter tests |
| #239 canonical facade | `codex-application-facade.md`; canonical inspect/assess/resolve/explain/context/registry/evidence tests |
| #240 explicit scope isolation | `codex-scope-isolation.md`; two-root, symlink, stale pin, session and cache regressions |
| #241 credential/sensitivity isolation | `codex-secret-isolation.md`; launch/admission/request/result/trace/cache secret tests |
| #242 current policy/consent | `codex-policy-gates.md`; no-policy/authorization/consent/evidence/deny/class/replay tests |
| #243 bounded runtime | `codex-runtime.md`; deadline/cancellation/EOF/backpressure/panic/write/limit faults and counters |
| #244 operator setup/fallback | `codex-local-setup.md`; bootstrap tests and full CLI/MCP canonical response parity |
| #245 end-to-end qualification | New executable goldens, session bridge projection tests, component report and this matrix |

## Release decision

The harness can qualify the inbound component scope. EPIC-04's full release
decision remains **NOT COMPLETE** until shared session start/status,
clarification/consent pause and cancellation are exercised against the supported
shared services rather than supplied projections. Installed-client interoperability
also requires a controlled run with its exact initialize name/version pair. #279
must independently qualify connector/model and durable runtime behavior.

Verification on 2026-10-10: the standalone component harness returned
`QUALIFIED_COMPONENT_SCOPE`; all ten required gates passed, including nineteen
architecture tests, twelve independent contract tests, seven operator/executable
tests and three canonical/session bridge tests. All thirteen coverage files
exceeded 95% (minimum 96.12%). The workspace regression run passed 782 tests with
three existing optional tests ignored; the added canonical bridge scenario also
passed its dedicated test target. Local evidence is retained under
`target/epic04-qualification-245-final/`, with the workspace log at
`target/epic04-245-workspace-tests.log`. This is component evidence, not execution
of the separate complete release gate or qualification of an installed client.
