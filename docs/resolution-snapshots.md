# CG-08.02 immutable resolution inputs

`ResolutionSnapshotPort::capture` returns one owned `ResolutionSnapshotInput`.
`ResolutionSnapshot::capture` calls it exactly once, validates it, and stores it
behind shared-reference getters. The in-memory implementation clones explicitly
supplied snapshots. A future external adapter must return a coherent read;
this interface has no mutation, transition, execution, network or clock method.

Inputs reuse CG-03 `Registry` and `CapabilityIndex`, CG-04 `ProcessRegistry` and
optional `ProcessInstance`, CG-06 `DeclarativeContextSituationDocument` and
optional `ProcessSituationReference`, and CG-07 `DesiredState`, `Delta`, `Plan`.
CG-02 OperatingMode and ExecutionProfile remain explicit required inputs.
All core interpretation stays with its existing owner.

The snapshot validates the Plan through CG-07 `validate_for_resolution`, checks
its Delta's Situation/current-state basis and the document's optional Intent,
and requires plan and Situation scope labels to equal the request scope. These
labels are adapter-supplied provenance, not authenticated permission. No global
project state is used. A rebuilt index is obtained through CG-03's own
`Registry::capability_index`; an unequal supplied index is rejected as mixed.

For a Process instance, the exact ID/version must exist in the ProcessRegistry.
The optional expected revision is checked before CG-06's existing read-only
process-reference operation verifies the CG-04 digest, runtime references and
AuthorizedActivity inspection. A supplied Situation process reference must
equal this complete inspection, including state, gates, blockers and activities.
Supplying a reference/revision without an instance is inconsistent.

Availability is explicit: `Present`, `AbsentOptional`, or `UnavailableRequired`
when a PlanStep declares lifecycle requirements but no instance was supplied.
The last status is valid incomplete evidence; later applicability resolution
must not turn it into permission or readiness.

## Content basis

The basis contains SHA-256 digests of canonical Plan JSON, sorted canonical
Agent/Skill documents, sorted Process definition digests, and the complete
CG-04 process inspection. The Situation-basis digest also covers the complete
CG-06 document, DesiredState, Delta, OperatingMode, ExecutionProfile and explicit
alternative groups. Group order is normalized; group cardinality and members
remain semantic. Every string is prefixed by its byte length before hashing,
avoiding delimiter ambiguity. Source filesystem paths are nonsemantic and
excluded from catalog digests. The process-definition digest is CG-04's own
canonical digest, never a hash of Debug output.

`same_basis` compares the captured basis, including scope, plan admission and
rule version. Equal offline snapshots prove reproducibility only. Downstream
execution must acquire a new authoritative snapshot and compare its basis;
this API cannot assert that external state has not changed. Private ownership
ensures changes to a source after capture do not affect the prior snapshot.

## Errors and evidence

Errors are typed, stable variants: unavailable input, unsupported version,
scope mismatch, invalid Plan, Situation mismatch, invalid catalog, mixed index,
missing Process definition, invalid Process, stale revision, inconsistent
Process projection, and invalid resolution request. They contain no raw
project/evidence data. Only v1 input/rule versions are accepted.

`cargo test -p gateway-application --test resolution_snapshot` proves capture
count, isolation, index equivalence under reordered documents, mixed indices,
wrong scopes, invalid upstream Plan/Situation, stale revisions, digest mismatch,
missing optional versus required Process state, and alternative-basis behavior.
Fixtures consume real CG-07 planner output and the real canonical CG-03/CG-04
catalogs. They make no claim that observation requirements select a provider;
candidate discovery belongs to CG-08.03.

Run `cargo llvm-cov -p gateway-application --all-targets --json --output-path
target/cg08-coverage.json` and inspect `src/resolution_snapshot.rs` for the
required >=95% production line coverage, without exclusions. Workspace CI,
format, Clippy and the architecture guard apply unchanged.

Measured on 2026-09-09 with cargo-llvm-cov 0.9.0: **185/185 production
lines covered (100%)**, no exclusions; six snapshot integration tests pass.
