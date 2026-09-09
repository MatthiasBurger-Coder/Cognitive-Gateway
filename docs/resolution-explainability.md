# CG-08.09: one semantic trace, two projections

`explain_resolution(snapshot, report, TraceLimits)` recomputes the report against
the immutable basis and exact rules before explaining it. Stale basis, altered
reports, invalid rules and invalid limits return distinct typed errors. A public
report or deserialized trace is not itself authority.

The v1 graph has typed source categories, stable reason codes, content-addressed
nodes, canonical node/edge ordering and shared references. `to_text` renders the
same nodes, codes and edges as `to_json`; it has no independent narrative branch.
Both expose the resolution outcome, search completeness and explicit
`policy_authorization=NOT_EVALUATED`. SELECTED means selected binding, not permission.
Readiness, process status/gates/blockers and pending dependencies remain separate.

The chain covers DesiredState/Situation -> Delta/DeltaItem -> Plan/PlanStep ->
CapabilityRequirement -> canonical contract/provider -> binding, responsible
Agents, effective Skills and recursive Skill/capability dependencies. Rejected
contracts and bindings keep their requirement/provider references and decisive
reasons. Closure condition status distinguishes required failure and optional
exclusion. Equal-rank alternatives, non-selected alternatives and incomplete-search
alternatives are separately coded; no-template is an explicit process decision.
Process references retain definition/version/digest and instance/revision when
present. The complete source basis and canonical rule fingerprint identify the
decision inputs; rule nodes also expose explicit integer priorities and independent
mode/profile values.

## Privacy, identity and limits

Rule encoding is explicit v1 JSON, not Rust Debug output. Every stage's rules,
completion attestations, selected rule version and ranking/search limits contribute
to its SHA-256 fingerprint. Unordered conjunctions are normalized and deduplicated.
The encoded rule body is used internally for hashing; the trace publishes its
fingerprint rather than copying arbitrary condition text. Intrinsic condition
references and failed selectors are likewise fingerprinted. Evidence/provenance,
Situation and completion receipts stay references: no raw evidence, waiting
detail, source instructions or inferred policy approval are copied.

`max_nodes` must be 1..100000 and optional detail allowance 0..100000. Required
nodes are built first. Matched-selector detail references are added only with
remaining capacity; omitted detail count is explicit. If the mandatory core cannot
fit, `RequiredTraceLimit` returns no misleading partial explanation. Shared node
and edge sets deduplicate repeats; search itself remains bounded by CG-08.08.
Fingerprint equality proves content consistency, not source authenticity or an
authorization decision. Completion-attestation trust remains the CG-08.07 caller
boundary. Historical graphs require current-basis revalidation before any handoff.

## Evidence

Five integration tests and two mapping/encoding unit tests exercise graph links,
provider responsibilities, recursive dependencies, attestation references, paused
processes, contract/closure rejections, stale/tampered input, incomplete search,
ordering, mandatory versus optional trace limits and sensitivity preservation.
`tests/fixtures/resolution-trace-golden.json` locks the semantic reason-code sets
for selected/no-template/dependency, ambiguous, rejected and no-op cases. Text/JSON
identity and every edge endpoint are checked independently of those fixtures.

```powershell
cargo test -p gateway-application --test resolution_explain
$env:CARGO_TARGET_DIR='D:\Projects\Cognitive-Gateway\target\cg08'
cargo llvm-cov -p gateway-application --all-targets --json --output-path target/cg08-coverage.json
```

Production coverage: trace **507/527 lines (96.20%)**, semantic encoding
**93/94 (98.94%)**. Workspace tests, fmt, Clippy and architecture guard pass.
