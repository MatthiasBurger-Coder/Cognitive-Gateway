# CG-08.12 implementation evidence and acceptance decision

Final parent acceptance: **BLOCKED — CG08-PROJECTION-01**. Passing resolver tests
do not close #7 or #158. The existing CG-02 v1 projection gap is specified in the
[application API and compatibility matrix](resolution-application-api.md).
No contract extension, synthetic production workflow or implicit authorization
is introduced by this evidence slice.

## Neutral worked example

`crates/gateway-application/tests/cg08_end_to_end.rs` consumes actual CG-07 output.
`tests/support/cg08_reference.rs` supplies the neutral CG-06/CG-07 records:

| Input | Value |
| --- | --- |
| Architecture fact / desired | Domain-to-infrastructure dependency present / absent |
| Coverage fact / desired | Decimal 92.00 / >=95.00 percent |
| Provenance | Explicit repository/tool/retrieval identities and evidence references |
| Mode / profile / scope | HARDENING / FULL_PATH / neutral-project |
| Catalog | Explicit synthetic Agent, core Skill, required leaf and unrelated Skill |

The test runs normalization, Situation assembly, Delta derivation, typed capability
requirement derivation and the CG-07 planner. The resulting Plan contains two
independent Change steps followed by their dependent Verification steps, in two
parallel layers. The planner contains no concrete Agent or Skill IDs.

CG-07 attaches `VERIFICATION_AFTER_CHANGE`. This example therefore requires a
synthetic lifecycle template and an explicit canonical lifecycle-constraint mapping;
removing the template without a mapping returns `Unsupported`. The test does not
remove the planner's lifecycle contract to manufacture a no-template success.
Optional no-template behavior is separately proven for Plans without a lifecycle
requirement by `resolution_process`, `resolution_composition`, `resolution_artifact`
and `resolution_application` tests.

The synthetic process declares separate remediation and verification activities,
each structurally exposed by a transition from START. Resolver inspection never
takes those transitions. An exact definition digest, instance revision and CG-06
process reference are captured in the immutable snapshot.

The resolution pipeline proves:

1. CG-03 typed discovery selects the core Skill's MUTATE or INSPECT contract.
2. Canonical Agent responsibility and the pinned process activity are preserved.
3. Mode/profile conditions activate core and leaf; required Skill closure is
   dependency-first `[fixture-leaf, fixture-core]`. The nested observation
   capability is explicitly bound; `fixture-related` is not activated.
4. One complete composition is selected. Change steps are eligible; Verification
   steps remain deferred until the caller supplies exact, fresh, basis-bound
   completion attestations for their predecessors. Those attestations are a
   trusted-caller fixture, not a resolver-issued or authenticated runtime receipt.
5. Explainability retains typed references and reports `NOT_EVALUATED` for policy.
   Malicious retrieval text asking for a rogue Agent and unrestricted mutation
   does not select an executor or grant permission. Raw source text is not exposed
   in the explanation.
6. Canonical artifacts round-trip with full revalidation. Repeated/permuted input
   produces identical snapshots and reports. Captured input and process remain
   unchanged. Inspection preserves the required MUTATE contract for CG-09.
7. Missing external CG-10 mapping is explicit. The separate `resolution_application`
   suite proves an actually representable fixture with real CG-02 constructors
   and validation, and rejects unsupported shapes. This is not a production
   mapping decision for all valid CG-08 results.

With the real Git-owned catalog, the same abstract requirements have genuine
`UnknownCapability` discovery outcomes and the composition is `Missing`. No
project-specific fixture capabilities are installed in the production catalog.

## Parent acceptance traceability

Test paths below are relative to `crates/gateway-application/tests/`; documentation
links describe the same implemented contracts, not just refinement scenarios.
The [Agent responsibility contract](resolution-agents.md) and
[artifact schema/validation contract](resolution-artifacts.md) complete the API
and domain documentation linked below.

| Parent #7 criterion | Automated evidence | Contract / status |
| --- | --- | --- |
| 1. Equivalent output | `cg08_end_to_end`, `resolution_candidates::reordered_catalog_and_rules_produce_identical_candidate_sets`, artifact canonical-set permutations | [Snapshots](resolution-snapshots.md), PASS |
| 2. Typed CG-03 resolution | `resolution_candidates`, actual CG-07 E2E, class/input/output negative cases | [Candidates](resolution-candidates.md), PASS |
| 3. Recursive required Skills | `resolution_skills`, nested provider composition, E2E core/leaf | [Skills](resolution-skills.md), PASS |
| 4. Optional/deterministic processes | `resolution_process`, whole-plan process composition, pinned E2E | [Processes](resolution-process.md), PASS |
| 5. Explicit failure/ambiguity | `resolution_composition`, artifact outcome round-trips | [Composition](resolution-composition.md), PASS |
| 6. Mode/profile/gates/blockers | `resolution_applicability`, pinned blocked E2E | [Applicability](resolution-applicability.md), PASS |
| 7. Planner and CG-04 authority | Exact original PlanStep equality, deferred E2E, stale pin tests, architecture guard | [Contract](resolution-contract.md), PASS |
| 8. Explainable selections/rejections | `resolution_explain`, typed diagnostic cases, golden JSON and E2E | [Explainability](resolution-explainability.md), PASS |
| 9. Existing CG-02 projection | `resolution_application` real-v1 positive fixture and explicit incompatibilities | [Compatibility matrix](resolution-application-api.md), **BLOCKED** for all required shapes |
| 10. Documentation and >=95% | Reproduction below and per-file CI gate | This index and linked contracts, PASS |

## Mandatory negative and graph cases

| Required scenario | Passing automated evidence |
| --- | --- |
| Missing/unknown capability; class/input/output mismatch | `cg08_end_to_end::real_catalog_no_match_and_negative_resolution_variants_remain_honest`; `resolution_candidates` |
| Unbound Skill provider | `resolution_agents::unowned_unlinked_skill_never_gets_an_arbitrary_agent` |
| Missing/cyclic dependency, cross Skill/Capability cycle | `resolution_skills::missing_nodes_cycles_and_limits_fail_closed`, `transitive_mutation_requirements_need_explicit_providers_and_detect_cross_cycles` |
| Uncertain conditions and mandatory closure | `resolution_skills::conditions_are_explicit_and_mandatory_dependencies_are_never_trimmed_to_success`; `support/skill_conditions.rs` |
| Related Skill nonactivation | E2E core/leaf assertion; `resolution_skills::diamond_closure_is_dependency_first_and_retains_every_path` |
| Incompatible process and global conflict | `resolution_process::explicit_activity_and_output_constraints_are_not_bypassed`; `resolution_composition::complete_plan_uses_one_process_and_enforces_canonical_roles_and_roots` |
| Ties and greedy-trap rejection | `resolution_composition::greedy_trap_filters_whole_closure_before_priority_and_keeps_ties` |
| Optional omission, equivalent alternatives and partial work | `resolution_composition::optional_groups_partial_failure_and_limits_are_explicit`; `resolution_contract::optional_is_not_an_inferred_alternative` |
| Mixed snapshot, wrong scope, stale revision, blocked gates | E2E pinned-process test; `resolution_snapshot`; `resolution_applicability::process_status_gates_and_activity_are_read_only_authority` |
| Malicious retrieval and no implicit permission | E2E trace/policy assertions; `resolution_application::external_policy_is_required_and_cannot_be_invented_from_readiness` |
| Forged/rehashed artifacts and unsupported versions | `resolution_artifact::tampered_contracts_closure_references_status_and_unknown_fields_fail_even_with_rehashed_content`; strict JSON/version test |
| Deterministic search limit | E2E negative variants; composition/serialization/explanation limit tests |
| No-op, no-template, independent/deferred steps | E2E; process optional-absence test; artifact round-trips; `resolution_applicability::diamond_dependencies_require_exact_fresh_scoped_attestations` |
| Multiple participants and separate responsibilities | `resolution_agents::primary_and_required_participants_keep_distinct_capability_responsibilities`; application incompatibility tests |

## Operating-mode / execution-profile matrix

`resolution_applicability::mode_profile_and_every_source_are_conjunctive` tests
all nine combinations against explicit HARDENING and FULL_PATH restrictions:

| Mode | FAST_PATH | NORMAL_PATH | FULL_PATH |
| --- | --- | --- | --- |
| DEVELOPMENT | Blocked | Blocked | Blocked |
| HARDENING | Blocked | Blocked | Eligible |
| RELEASE_QUALIFICATION | Blocked | Blocked | Blocked |

These are fixture restrictions, not a global prohibition on other modes. An
additional false process restriction blocks every combination. Mode and profile
remain independent dimensions; neither implies policy approval.

## Reproducible quality evidence

```powershell
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/check-architecture.sh
cargo llvm-cov -p gateway-application --all-targets --json --output-path target/cg08-coverage.json
pwsh -NoProfile -File scripts/check-resolution-coverage.ps1 -SelfTest
pwsh -NoProfile -File scripts/check-resolution-coverage.ps1 -ReportPath target/cg08-coverage.json
```

Local measurement on 2026-09-09 (covered/executable lines):

| Production module | Lines |
| --- | --- |
| resolution | 217/217 |
| resolution_snapshot | 196/197 |
| resolution_candidates | 94/94 |
| resolution_process | 160/161 |
| resolution_agents | 186/186 |
| resolution_skills | 200/201 |
| resolution_applicability | 214/215 |
| resolution_composition | 652/658 |
| resolution_encoding | 94/94 |
| resolution_explain | 515/529 |
| resolution_artifact | 318/325 |
| resolution_application | 338/343 |

Every module is >=95%. No exclusions are used. The CI gate discovers every
`src/resolution*.rs` file and requires exactly one nonempty coverage entry per
file; missing, duplicate, zero-line and below-threshold evidence fail. It compares
counts without percentage rounding and self-tests its negative paths. Existing
domain/registry/CLI gates remain unchanged. Production implementation is Rust-only;
PowerShell is a quality-check script, not a runtime dependency.

## Recorded acceptance review

This is a documented implementation self-review by Codex, **not an independent
approval or three separate human reviewers**. Local tests, Clippy, architecture
guard and per-file coverage passed. The PR records final-head CI evidence.

| Perspective | Implemented slice | Final parent acceptance |
| --- | --- | --- |
| Product / Domain | Capability-first outcomes, uncertainty, optional templates and no implicit ALLOW proven | **BLOCKED**: all required results cannot yet be projected |
| Architecture / Engineering | Immutable inputs, canonical authority, inward dependencies, no executor/compiler/transition ownership leakage | **BLOCKED**: CG-02/CG-10 owner decision CG08-PROJECTION-01 open |
| QA / Test | Neutral E2E, negative/graph matrix, round-trips and reproducible coverage pass | **BLOCKED**: parent criterion 9 and final all-PASS proof remain unmet |

Required next authority: CG-02/CG-10 must sanction a lossless mapping or an IR
extension/migration for no-template, empty-Skill, multi-Agent and unmapped-constraint
results. A scoped proof PR may merge with this blocker recorded; #158 and parent
#7 must remain open. Do not turn this record into an all-PASS acceptance by merely
changing the checkboxes.
