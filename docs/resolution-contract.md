# CG-08 resolution contract v1

`gateway_application::resolution` owns concrete resolution contracts. These
compose existing CG-07 `Plan`, `PlanId`, `PlanStepId`, `CapabilityRequirementId`
and cardinality, CG-03 `CapabilityProvider`, CG-04 `DefinitionIdentity`, instance
identity/revision, and CG-06 Situation/scope identities. The application layer
already depends inward on all three core crates; no infrastructure dependency
or reverse dependency is introduced into `gateway-domain`.

## Request and content basis

`ResolutionRequest::new` owns a Plan and `ResolutionBasis`. Its private fields
and borrowed getters prevent mutation after validation. The constructor checks
the Plan identity and SHA-256 of CG-07's canonical `Plan::to_json()` output.
Schema and resolver rules support exactly `SchemaVersion::V1` (1.0); a
syntactically valid future version is rejected. Upstream typed IDs retain their
validating constructors. Fingerprints require exactly 64 hexadecimal digits,
normalized to lowercase.

The basis identifies the Situation and runtime scope, Situation content,
registry content, Process catalog content, Process state content, and optional
plan admission reference. Admission is provenance, never permission. Snapshot
content capture and cross-snapshot integrity belong to CG-08.02; this first
contract does not claim that caller-supplied catalog digests prove membership.

`RequirementAlternatives` declares a one-of group on a specific step. Its
members refer to existing requirements; groups cannot overlap on the same step
and contain at least two members. Mandatory groups select exactly one member;
optional groups select zero or one. The explicit group replaces individual
cardinality for its members. No grouping is inferred from the order of
requirements, their rationale, or the presence of an optional requirement.
When CG-07 does not carry equivalence metadata, the resolver must retain
individual requirements unless an explicit group is supplied.

## Result and validation boundary

Public step/candidate records are proposals. `ResolutionResult::new` returns
an immutable, structurally validated result tied to its request. It rejects
missing/duplicate steps or requirements, candidates with duplicate providers,
selections absent from candidate sets, unknown agent responsibilities, missing
mandatory coverage, invalid group cardinality, unexplained omissions, and
inconsistent complete/no-op outcomes. Steps are presented in canonical ID order.
Candidate order never settles semantic ambiguity.

A candidate records its CG-03 provider, exact definition fingerprint and typed
selection/rejection reason. Each candidate set refers to a requirement on the
originating PlanStep; the Plan retains the full abstract capability contract
reference, cardinality, preconditions, constraints and Delta lineage. A binding
contains an optional CG-04 definition identity (ID, version, digest) and optional
instance/revision, primary Agent, participating Agents and effective Skills
with explicit responsible Agents. Primary and participating roles are disjoint.

Structural validation is not canonical validation. CG-08.02/03/06/10 must prove
that referenced definitions exist in the captured catalog, contracts match and
the complete Skill closure is present. The v1 foundation exposes no operation
to execute, authorize, transition, retrieve, mutate catalogs or compile context.

Outcomes are `RESOLVED`, `NO_OP`, `MISSING`, `AMBIGUOUS`, `CONFLICTING`,
`UNSUPPORTED`, `INVALID_INPUT`, `PARTIAL` and `SEARCH_LIMIT`. Incomplete steps
retain candidate diagnostics but cannot carry a completed binding. Incomplete
mandatory work cannot be marked resolved overall. Empty plans and explicit
NoOp steps need no fabricated Agent, Skill or Process bindings. Lifecycle
readiness (`ELIGIBLE`, `BLOCKED`, `DEFERRED`, `UNKNOWN`, `NOT_APPLICABLE`) is
independent: a fully resolved step may remain blocked. Even `ELIGIBLE` is not
policy ALLOW and does not establish a legal transition.

All enums expose stable names and reject unknown strings. Full versioned artifact
serialization and canonical revalidation are owned by CG-08.10. A content hash
proves consistency only; historical results require current-basis revalidation.

## Ownership and CG-02 compatibility

| Owner | Responsibility |
| --- | --- |
| CG-02 | Existing ExecutionContextIR and mode/profile contracts |
| CG-03 | Canonical Agent/Skill/capability definitions and graph |
| CG-04 | Process definitions, runtime state and legal transitions |
| CG-06 | Situation, evidence and external scope |
| CG-07 | Desired/current Delta, Plan and abstract requirements |
| CG-08 | Concrete bindings and resolution diagnostics |
| CG-09 | Permission and approved capabilities |
| CG-10 | Context selection, compilation and IR projection |
| CG-11 | CLI driving adapter |

CG-02 v1 requires a workflow, primary Agent, non-empty Skills within workflow
closure, and separately approved capabilities. A process-free result is a
valid CG-08 outcome but not automatically representable in CG-02 v1. Multiple
Agents also require preserving responsibilities. CG-08.11 must prove supported
mappings and reject unsupported ones without fabricated workflows, flattened
responsibilities or self-approved capabilities. These integration gaps remain
open and block claiming complete parent acceptance.

## Reproducible verification

`cargo test -p gateway-application --test resolution_contract` exercises schema
and identity failures, every outcome/readiness/reason, fingerprints, no-template
and no-op behavior, reference failures, responsible Skill providers, optional
omissions and explicit alternative cardinality.

`cargo llvm-cov -p gateway-application --all-targets --json --output-path
target/cg08-coverage.json` produces per-file line evidence. Inspect the
`src/resolution.rs` entry; it must reach at least 95% covered lines.
Workspace tests, format, Clippy and the architecture guard remain required.
Runtime execution E2E is inapplicable to this contract-only work package;
CG-08.12 owns the complete Plan-to-resolution integration proof.

Measured on 2026-09-09 with cargo-llvm-cov 0.9.0: `src/resolution.rs`
has **213/213 covered lines (100%)**, with no coverage exclusions. The eight
contract tests pass alongside the existing application integration tests.
