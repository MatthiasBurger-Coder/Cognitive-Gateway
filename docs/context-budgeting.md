# CG-20B token bounded context

Issue: [#198](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/198).

`gateway-context::budgeted::select_context` makes a deterministic selection of external data under the CG-15 `ContextBudget`. `gateway_application::context_budgeting::compile_budgeted_step` first runs the current CG-10 application checks with no external data, estimates fixed semantic sections, selects fragments, then runs the same CG-10 policy and compilation path with the selected fragments. It checks the compiled result for required IDs and measures emitted fragments again before returning the bounded handoff. No conversation history is read implicitly; the caller passes the candidate list and explicit required IDs.

## Requirement and verification matrix

| Requirement sentence | Implementation and contract | Verification |
| --- | --- | --- |
| Fixed authority, task and output contract sections must each fit their reserved class. | `budgeted::select_context`; `ContextBudget::validate_usage` | `mandatory_evidence_and_fixed_sections_fail_explicitly` |
| Retrieval content cannot borrow authority, task, output or safety capacity. | `budgeted::class`, disjoint class checks; full safety margin held in `usage` | `deterministic_ties_budget_and_source_provenance`; `budgeted_selection_flows_through_current_policy_and_compiler` |
| Exact counts must name the requested target; approximate counts use a stated upper bound; unknown counts fail. | `budgeted::bound`; `TokenEstimatorPort::estimate_context` | `unknown_and_unbounded_estimates_never_become_exact`; application integration |
| Estimator failure cannot silently turn into zero tokens. | `BudgetedCompileError::Estimator`; default port method returns `InvalidEstimate` | application integration and existing retrieval token tests |
| Fixed inputs and scores produce the same selection and reasons, with ID ties. | `select_context` canonical order and `SelectionDecision` | `deterministic_ties_budget_and_source_provenance` |
| Redundant content from distinct sources retains distinct provenance. | redundancy check includes complete `FragmentMetadata` | `deterministic_ties_budget_and_source_provenance` |
| Optional compaction preserves source IDs, trust, sensitivity, uncertainty and conflict state. | `ContextCompactionPort`, `CompactedCandidate`, metadata equality and `BudgetedSelection::lineage` | `compaction_preserves_lineage_and_rejects_metadata_relabeling` |
| Mandatory evidence cannot be omitted by selection or final compilation. | explicit `required` IDs; mandatory first; post-compile ID check | `mandatory_evidence_and_fixed_sections_fail_explicitly`; application integration |
| The final selected content must still fit after adapter compaction. | final `estimate_context` and budget revalidation | application integration |
| Selection must retain current process and policy checks and exclude unrelated history. | `compile_budgeted_step` calls `ContextApplication::compile_step` twice with explicit candidates | `budgeted_selection_flows_through_current_policy_and_compiler` |

## Contract and failure behavior

The application estimator receives a scoped semantic section or selected fragment, plus target identity. Its response is the existing `TokenEstimate`, including estimator ID and version. Exact counts are accepted only for that target. Estimated counts require an upper bound at least as large as the estimate. Unknown, inconsistent and failed measurements reject the bounded compilation. Candidate counts can come from the retrieval estimator; the application measures the final selected text again and uses the larger bound. The serialized handoff records per-fragment, per-section and total estimator provenance, the count kind, usage, selection reasons and source IDs for compacted fragments. The application measures the final serialized fragments and entire semantic envelope; the full envelope must fit the total less the safety margin.

Authority, task and output contract estimates are mandatory. Runtime state is also measured and requires its own reservation. The safety margin is fully held. Fixed-section accounting includes the basis, provenance and execution IR, and the final whole-envelope check catches framing and tokenization effects. A missing class reservation has zero capacity. Oversized mandatory sections or required evidence return an explicit `MandatoryOverBudget` error. Optional candidates that do not fit get a `BudgetExceeded` decision. Auditable selection decisions serialize stable `CONTEXT_*` reason codes. An authenticated exclusion set marks optional IDs `CONTEXT_QUARANTINED`; required IDs fail with `QuarantinedMandatory`, and a compaction artifact containing an excluded source is rejected. No tokens move between classes.

The selector sorts mandatory candidates first, then descending integer score and ascending reference ID. Identical content is suppressed only when kind, representation and all source, quality and selection metadata agree. A compaction artifact may replace only nonmandatory knowledge or memory fragments from one matching source and metadata set. It cannot become an authority or task fragment, alter trust or handling labels, merge conflicting evidence, or replace required evidence. Source IDs are carried in the `lineage` map and the serialized `context_selection` trace. The source adapter remains responsible for the factual correctness of summary text; the core validates its structural safety and budget.

This budget applies to the provider independent semantic context, measured for the caller's named target. A provider renderer owns its own final message framing and hard provider window check. The bounded handoff is not an authorization token or a provider prompt.

## Verification

Run the issue's full validation commands from the repository root:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
bash scripts/check-architecture.sh
python3 scripts/quality-gate.py
```

The focused suites are `gateway-context/tests/budgeted_selection.rs` and `gateway-application/tests/context_application.rs`. The established quality runner writes revision-bound logs and coverage in `target/release-evidence/`.
