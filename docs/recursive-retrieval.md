# CG-19: bounded recursive retrieval and evidence sufficiency

Issue: [#197](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/197).

## Requirement trace

| Requirement sentence | Implementation boundary | Verification |
| --- | --- | --- |
| A claim needs validated evidence links, accepted trust and required freshness. | `gateway-domain::retrieval_plane::sufficiency`; `RetrievalEvidencePort` | `sufficiency_requires_validated_links_and_preserves_combined_failures`; `two_rounds_satisfy_without_resetting_usage_or_scope` |
| Missing evidence and provenance IDs remain explicit. | `SufficiencyAssessment::missing_evidence`, `missing_provenance` | `sufficiency_reports_missing_references_and_exhaustion` |
| Conflict, stale, untrusted and contaminated findings remain visible together; confidence does not establish sufficiency. | `assess_sufficiency` | `sufficiency_requires_validated_links_and_preserves_combined_failures` |
| Query refinement may change only queries, never scope, sources, requirements or budgets. | `RetrievalPlan::with_queries`; `QueryRefinementPort` | `two_rounds_satisfy_without_resetting_usage_or_scope` |
| Every round uses one cumulative result, round, elapsed, cost, token and context budget. | `retrieve_until_sufficient`; `FederatedRetrievalPort` | `two_rounds_satisfy_without_resetting_usage_or_scope`; `zero_and_overflowed_token_reservations_never_dispatch` |
| Repeated queries and evidence with no progress stop bounded recursion. | `retrieve_until_sufficient` | `duplicate_evidence_and_repeated_queries_cannot_claim_sufficiency` |
| Each round records its queries, proposed changes, accepted and rejected evidence, assessment, usage, error and stop reason. | `RoundTrace` | recursive retrieval tests |
| Insufficiency pauses CG-14 through its existing decision boundary and never creates an authorization grant. | `ClosedLoop::apply_retrieval_assessment` | CG-14 integration test |
| Graph results can participate when an explicitly selected graph adapter implements the CG-16 source port. | `FederatedRetrievalPort` and `KnowledgeRetrievalPort` | CG-17 graph federation tests |

## Contract

`RequiredInformation` is one claim. A verified evidence ID must be a member of
the fragment's declared links. The application asks `RetrievalEvidencePort` to
validate each result through the owning evidence boundary. Raw retrieved links,
relevance and model confidence cannot satisfy the claim. A claim with no named
evidence IDs still needs at least one validated link. An
`EvidenceSatisfied(n)` stop condition requires at least `n` distinct validated
links as well. Every named provenance ID
must be represented among accepted fragments. Unresolved conflict or
contamination blocks sufficiency even when another accepted fragment covers the
IDs. All findings remain in the set; `state` chooses the highest priority for
display: contaminated, conflicting, budget exhausted, stale, untrusted,
sufficient, partial, insufficient.

The host calls `retrieve_until_sufficient` with a validated plan, initial
usage, a CG-15 retrieval port, an evidence validator, an optional query refiner
and a maximum number of consecutive rounds without new validated evidence.
Only proposed queries change. Repeated query sets stop before dispatch.
Pre-dispatch checks reserve a round and query bytes; the retrieval port remains
responsible for conservative source/output reservations and actual measurements.
The concrete federated port reports cumulative counts and a nonterminal
V2 `Partial/MoreInformationNeeded` batch until a hard limit is reached. Its cost
counter remains zero because the local adapters have no priced service.

`RecursiveOutcome` retains accepted result identities, complete cumulative
usage, every round trace and a typed stop reason. A retrieval or validation
error stops further dispatch and retains measured failure usage. The
`retrieve_measured` port reports the attempted round; the concrete federated
adapter also records elapsed time and query bytes on failure. Priced or remote
adapters must override it with their own measured cost and token usage.
`TimedOut` and `Cancelled` remain distinct error codes in the trace.
Errors do not replay possibly completed mutations.

CG-14 may consume the assessment through `apply_retrieval_assessment`.
Insufficiency pauses; sufficiency leaves the current decision unchanged. A
paused run still needs a fresh CG-06 observation snapshot through `refresh` and
current CG-09 policy and CG-04 process inputs before execution.

## Reproduction

```sh
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --workspace --locked
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
bash scripts/check-architecture.sh
python3 scripts/quality-gate.py
```
