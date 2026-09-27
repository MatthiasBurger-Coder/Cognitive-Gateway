# EPIC-02 v0.2 release qualification (CG-20D)

## Candidate qualification rule

CG-20 evaluation, profiling and curated export contracts and the CG-20D
fake-port replay are implemented in this candidate. Technical qualification
requires a `PASS` summary from `python3 scripts/quality-gate.py` on the exact
clean candidate commit, with every gate passing and the two CG-20 metric
artifacts and raw coverage present. The release decision is recorded by that
revision-bound evidence bundle. This document records implementation and
verification mapping, not release publication or independent human approval.
Revision `2684475` was blocked because these contracts and proofs were absent.

The source of requirements is [EPIC-02 #113](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/113),
its data-curation supplement, and [CG-20D #202](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/202).
`PASS locally` means a contract and its focused test passed during development.
`BUNDLE REQUIRED` means the exact candidate's clean-commit summary and raw
artifacts must be checked before qualification. All paths are relative to the repository root.

## Sentence-level requirement and evidence matrix

| ID | Required behavior | Implementation or port | Verification evidence | Status |
| --- | --- | --- | --- | --- |
| E02-A01 | Keep retrieval separate from execution authority. | `crates/gateway-application/src/ports/`, `retrieval_pipeline.rs`; `gateway-domain/src/retrieval_plane/` | `tests/architecture/test_dependencies.py`, `gateway-application/src/retrieval_pipeline/tests.rs` | PASS locally |
| E02-A02 | Keep retrieved, memory and context text from granting policy, process, capability or system authority. | `gateway-context/src/compiled.rs`, `gateway-application/src/context_application.rs` | `gateway-context/tests/compiled_context.rs`, `gateway-application/tests/context_application.rs` | PASS locally; integrated negative replay added |
| E02-A03 | Use a typed, versioned, bounded, inspectable retrieval plan. | `gateway-domain/src/retrieval_plane/plan.rs`, `budget.rs` | `gateway-domain/tests/retrieval_plane.rs` | PASS locally |
| E02-A04 | Keep lexical, semantic, graph and memory retrieval replaceable. | `gateway-application/src/ports/`, `retrieval_pipeline.rs`, `graph_retrieval.rs`, `memory.rs`; outer `gateway-daemon` adapters | `gateway-daemon/tests/retrieval.rs`, `graph_retrieval.rs`, `governed_memory.rs` | PASS locally; combined four-source replay added |
| E02-A05 | Keep embedding, token and model implementations outside core contracts. | `gateway-domain/src/retrieval_plane/embedding.rs`, `tokens.rs`; `gateway-application/src/ports/` | `scripts/check-dependencies.py`, `gateway-domain/tests/retrieval_plane.rs` | PASS locally |
| E02-A06 | Keep project data and derived indexes and memory scoped. | `gateway-domain/src/retrieval_plane/result.rs`, `knowledge_graph.rs`, `memory.rs` | `gateway-domain/tests/knowledge_graph.rs`, `gateway-daemon/tests/governed_memory.rs` | PASS locally; two-project component checks |
| E02-R01 | Preserve structural context, provenance and source identity in contextual and federated results. | `gateway-application/src/retrieval_pipeline.rs`; `gateway-domain/src/retrieval_plane/result.rs` | `gateway-application/src/retrieval_pipeline/tests.rs` | PASS locally |
| E02-R02 | Explain hybrid merge, deduplication and reranking. | `gateway-application/src/retrieval_pipeline.rs` | `gateway-application/src/retrieval_pipeline/tests.rs` | PASS locally |
| E02-R03 | Report no match, partial, conflict, stale, untrusted, contamination and exhausted budgets explicitly. | `gateway-domain/src/retrieval_plane/sufficiency.rs`; `gateway-application/src/recursive_retrieval.rs` | `gateway-domain/tests/retrieval_plane.rs`, recursive retrieval unit tests | PASS locally; combined negative matrix added |
| E02-R04 | Bound recursive queries, rounds, cost, latency, results and stop conditions. | `gateway-application/src/recursive_retrieval.rs`; `gateway-domain/src/retrieval_plane/budget.rs` | recursive retrieval unit tests; `scripts/check-cg19-coverage.py` | PASS locally; aggregate usage in integration report |
| E02-C01 | Enforce total and per-class context/token budgets and reserve authority, task and output space. | `gateway-context/src/budgeted.rs`; `gateway-application/src/context_budgeting.rs` | `gateway-context/tests/budgeted_selection.rs` | PASS locally; budgeted E2E handoff added |
| E02-C02 | Preserve compaction lineage, explain exclusions and make deterministic selections. | `gateway-context/src/budgeted.rs` | `gateway-context/tests/budgeted_selection.rs` | PASS locally; token-efficiency metric added |
| E02-C03 | Avoid wholesale conversation or retrieval injection. | `gateway-context/src/compiled.rs`, `budgeted.rs` | `gateway-context/tests/compiled_context.rs`, `budgeted_selection.rs` | PASS locally |
| E02-S01 | Preserve trust, sensitivity and source class across retrieval, reranking and context compilation. | `gateway-domain/src/quality.rs`; `gateway-application/src/retrieval_pipeline.rs`; `gateway-context/src/compiled.rs` | retrieval pipeline, compiled context and budgeted selection suites | PASS locally; combined replay added |
| E02-S02 | Reject indirect instructions and fail closed on unsatisfied trust requirements. | `gateway-application/src/retrieval_pipeline.rs`, `recursive_retrieval.rs`; `gateway-context/src/compiled.rs` | adversarial cases in retrieval and context suites | PASS locally; poisoned-source rejection added |
| E02-M01 | Preserve typed graph lineage and keep projections derived and non-authoritative. | `gateway-domain/src/knowledge_graph.rs`; `gateway-application/src/graph_retrieval.rs` | `gateway-domain/tests/knowledge_graph.rs`, `gateway-daemon/tests/graph_retrieval.rs` | PASS locally |
| E02-M02 | Track memory provenance, validation, expiry, supersession and forgetting; exclude ineligible records. | `gateway-domain/src/memory.rs`; `gateway-application/src/memory.rs` | `gateway-daemon/tests/governed_memory.rs` | PASS locally |
| E02-M03 | Revalidate exported learning references after invalidation and never restore forgotten payloads. | `gateway-application/src/memory.rs`, `evaluation.rs` | `gateway-daemon/tests/governed_memory.rs`; full-path export→forget→revalidate replay | PASS locally |
| E02-E01 | Pin versioned golden cases and source/index/model/estimator/evaluator manifests. | `gateway-domain/src/evaluation.rs`; `tests/fixtures/epic02-v0.2/golden.json` | `gateway-domain/tests/evaluation.rs`; retained `cg20-evaluation.json` | PASS locally; bundle required |
| E02-E02 | Reproduce deterministic objective metrics for relevance, sufficiency, token efficiency, provenance, freshness, contamination and failures. | `gateway-domain/src/evaluation.rs` | golden replay, reordered replay and measured fake-port integration report | PASS locally; bundle required |
| E02-E03 | Separate optional model-assisted scores from release authority. | `ReleasePolicy::qualify` accepts objective metrics only; no judge field | missing-metric and threshold failure tests | PASS locally |
| E02-E04 | Define numerical baselines, thresholds, missing/non-finite handling and regression policy. | `ReleasePolicy`; versioned `golden.json` | threshold, regression, invalid metric and missing denominator tests | PASS locally; bundle required |
| E02-D01 | Profile missingness, duplicates, conflicts, staleness, trust, type errors and useful distributions with denominators and lineage. | `gateway-application/src/evaluation.rs::profile` | `gateway-daemon/tests/governed_memory.rs` profile fixture | PASS locally |
| E02-D02 | Export deterministic curated snapshots with project/source identity, time, schema, label/outcome basis, validation and sensitivity. | `gateway-application/src/evaluation.rs::export_snapshot` | deterministic export, sensitive-reference and tamper tests | PASS locally |
| E02-D03 | Recheck eligibility and revocation before learning consumption; preserve sensitive data as controlled references. | `gateway-application/src/evaluation.rs::revalidate_snapshot`; CG-18 memory port | export→forget→revalidate and sensitive outcome/payload tests | PASS locally |
| E02-L01 | Reassess observed goal evidence after execution; completion alone does not establish success. | `gateway-application/src/closed_loop.rs` | `gateway-application/tests/support/closed_loop.rs`, context application suite | PASS locally; knowledge-aware replay added |
| E02-L02 | Replan under current process and policy authority without resetting iteration and retry budgets. | `gateway-application/src/closed_loop.rs`, `context_application.rs` | closed-loop integration suite and `scripts/check-closed-loop-coverage.py` | PASS locally; cross-plane replan added |
| E02-L03 | Treat reasoning strategy as a bounded provider-independent hint with explicit unsupported behavior and no hidden reasoning requirement. | `gateway-domain/src/reasoning_strategy.rs`; `gateway-application/src/reasoning_strategy.rs` | domain and application `reasoning_strategy.rs` suites | PASS locally; combined replay added |
| 20D-R01 | Replay an external-project goal through all four retrieval sources, trust/sufficiency, budgeted context, authorized fake runtime, observation, replan, memory and learning handoff. | `gateway-application/tests/support/closed_loop.rs` full-path fixture | `epic02_external_project_full_path_and_bounded_degradation`; `cg20-integration.json` | PASS locally; bundle required |
| 20D-R02 | Replay outage, no match, stale/conflict, contamination, insufficient context, policy denial and exhausted budgets with bounded calls. | same integration fixture and fake ports | negative matrix asserts stop/pause and zero unauthorized runtime calls | PASS locally; bundle required |
| 20D-R03 | Recheck process/policy/scope and aggregate budgets on every replan; strategy and knowledge never grant authority. | CG-14 `closed_loop.rs`, CG-19 `recursive_retrieval.rs`, CG-20B `context_budgeting.rs` | stale resolution and policy denial in full path; two-project, repeated-query and strategy suites | PASS locally; bundle required |
| 20D-R04 | Propagate memory revocation/forgetting to an existing learning export and keep sensitive payloads out of artifacts. | CG-20 export and revalidation | full-path export→forget→revalidate; sensitive outcome/reference test | PASS locally; bundle required |
| 20D-R05 | Map every criterion to measured, passing versioned release thresholds. | this matrix, `ReleasePolicy`, golden fixture | `cg20-evaluation.json`, `cg20-integration.json` and gate summary | BUNDLE REQUIRED |
| 20D-R06 | Pass the existing complete quality gate and applicable 95% per-file coverage on the candidate commit. | `scripts/quality-gate.py`, `quality-gates.json`, `check-cg20-coverage.py` | clean revision-bound `target/release-evidence/` summary required | BUNDLE REQUIRED |
| 20D-R07 | Align architecture, ADRs, examples and release checklist with the implemented v0.2 path. | `docs/arc42/05-building-block-view.md`, ADR-014, `epic-02-evaluation.md`, this checklist | architecture guard and documentation review | BUNDLE REQUIRED |

## Admission and release checklist

- [x] CG-14 through CG-20C contracts reviewed; CG-20 implementation is in this candidate worktree.
- [x] CG-20 evaluator, export and numerical threshold contracts are implemented with focused tests.
- [x] One typed-input, provider-free, fake-runtime full-path fixture passes.
- [x] Outage and adversarial matrix stops or pauses with bounded adapter calls.
- [x] Synthetic golden and measured fake-port results are reported separately.
- [ ] Verify a `PASS` summary on the exact clean candidate commit, with raw coverage, metrics and all logs retained.
- [ ] Requirement Lead, System Architect and Test/Evidence Reviewer findings are recorded against that commit.
- [ ] Release approval and publication are made separately.

No model invocation, training, provider renderer or production deployment is
claimed. Optional semantic, graph, embedding and model services must remain
replaceable; a failed service produces an explicit bounded outcome in the
integrated replay. Rollback must use a previously qualified code version
while preserving memory tombstones/revocations and checking version
compatibility. The release decision must cite the exact candidate commit and
the retained `target/release-evidence/` bundle.
