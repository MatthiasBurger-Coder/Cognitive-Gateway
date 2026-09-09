# CG-08.03 typed provider discovery

`discover_candidates(&ResolutionSnapshot, &CandidateRules)` queries the existing
CG-03 `CapabilityIndex` once per PlanStep requirement. The result retains the
step/requirement identity, cardinality, exact query, matched canonical candidates,
definition fingerprints, and CG-03 candidate-level rejection reasons. The
source basis and complete selector rules are retained; later composition must
use both. Candidate discovery does not produce a ResolutionResult or select a
first provider. Stable ordering is presentation, not semantic precedence.

Each query fixes the original capability ID. CG-07 DomainChange outcomes require
MUTATE; other supported outcomes require INSPECT, consistent with CG-07's
capability derivation. Requirement preconditions and intrinsic constraints are
conjunctive metadata selectors. Additional typed input/output, domain, class,
precondition, constraint and applicability selectors can further restrict the
query. They cannot replace its ID or weaken its required class. Unknown rule
versions, selectors attached to unknown requirements and capability substitution
attempts fail closed. Typed selector enums cannot express unknown operator names.

The index's canonical declaration supplies class/input/output and other contract
metadata. The Plan itself does not duplicate all that metadata; callers requiring
specific input/output kinds supply explicit selectors. All matched selectors and
failed selectors are returned by CG-03. Metadata matching does not establish
that a precondition is currently true: every declared precondition remains in
`unresolved_preconditions`. Evidence evaluation and constraint applicability
are subsequent CG-08.07 work; no missing fact is interpreted as true.

Outcomes distinguish compatible metadata, unknown capability, missing provider,
and incompatible providers. CG-03 indexes provided declarations only, so a
known reference without a provider is detected through canonical Skill
`required_capability_ids`. Such a reference establishes a need, not a provided
contract or executable provider. An ID absent from both index and required
references is unknown. A present indexed ID whose providers fail the query is
incompatible, with detailed rejections. No retrieval input or fuzzy matching
surface exists.

Mandatory failures remain unsatisfied requirements. Optional failures retain
their optional cardinality and explicit no-match outcome; this phase does not
silently erase them. CG-08.08 must justify omissions and prove mandatory coverage.
Explicit groups in the snapshot are the only source of equivalent alternatives.
Discovery queries every member without substituting one ID for another. Legacy
CG-07 optional requirements without group metadata remain independent optional
requirements; no equivalence is reconstructed from ordering or prose.

## Verification

`cargo test -p gateway-application --test resolution_candidates` runs seven
integration tests with real CG-07 Plans and canonical catalogs. They cover
multiple providers, class/input/output and other selector mismatches, MUTATE
non-downgrade, optional no-match, provided-versus-required confusion, unknown
IDs/rules and reordered registry equivalence. The synthetic unprovided reference
test changes only its in-memory fixture, not the production catalog.

`cargo llvm-cov -p gateway-application --all-targets --json --output-path
target/cg08-coverage.json` measured **94/94 production lines (100%)** in
`src/resolution_candidates.rs` on 2026-09-09, with cargo-llvm-cov 0.9.0 and no
exclusions. Workspace tests, Clippy, format and architecture guard pass.
