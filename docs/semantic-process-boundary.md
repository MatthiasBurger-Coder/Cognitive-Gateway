# Semantic task / process boundary

Issue [#191](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/191),
parent #177. This specification implements the boundary in ADR-015 and composes
the existing [semantic planning handoff](semantic-planning-boundaries.md) with
[Strict Cognitive Gherkin](strict-cognitive-gherkin-v1.md) and CG-04 Process IR.
MUST and MUST NOT below are normative.

## Ownership

| Owner | Meaning and authority | Excluded authority |
| --- | --- | --- |
| CGSL / `SemanticTaskIR` | Task kind, target, outcome, typed inputs, observations, desired predicates, constraints, output/verification obligations and reference pins | Process states, transitions, events, guards, activities, gates, retries, blockers and instance mutation |
| CG-07 | Desired/current comparison, Delta lineage, capability requirements and Plan | Interpreting goal prose as a process or granting execution permission |
| CG-08 | Explicit template selection, activity compatibility, provider binding and applicability over captured snapshots | Changing a pinned definition or bypassing lifecycle/policy requirements |
| CG-04 | Gherkin compilation, canonical definition identity, transition legality, lifecycle and instance revision | Parsing CGSL or interpreting task prose |
| CG-09 / trusted runtime host | Policy, consent, authorization and effect admission | Treating a task reference or successful selection as consent |

CGSL `state` is a reference to a CG-06 observed-state snapshot. `desired_state`
expresses acceptance predicates. Neither is a process state or transition target.
`history` references past records; it MUST NOT replay events. Task verification
requirements MUST NOT mark a process gate passed. A goal description remains
explanatory even if it contains words such as "transition" or "complete".

The two frontends MUST remain independent: `gateway-cgsl` depends on the shared
domain, and `gateway-process` depends on the shared domain. Neither depends on
the other, including dev/build/conditional dependencies. The shared domain MUST
NOT depend on either frontend. Application integration may use CGSL only as a
test fixture dependency; production consumers receive typed task contracts.

## Exact reference mapping

`ProcessRef` is conceptual terminology, not a new Rust model. Its existing Rust
representation is `gateway_domain::learning::ProcessReference`, carried by
`SemanticTaskIR.data().process: Option<ProcessReference>`.

| Semantic task pin | CG-04 `DefinitionIdentity` | Required comparison |
| --- | --- | --- |
| `id: ReferenceId` | `id: ProcessDefinitionId` | Exact identifier string |
| `version: u32` (positive) | `version: ProcessDefinitionVersion` | Exact numeric value |
| `digest: ContentDigest` | `digest: ProcessDefinitionDigest` | Exact lowercase SHA-256 digest |

The domain envelope avoids a dependency cycle. It MUST contain only a reference,
never embedded Gherkin, Process IR, instance IDs or lifecycle instructions.
Construction validates shape; it does not establish existence or applicability.
Reference admission projects this pin as `cg.process-definition`, contract `1.0`,
numeric definition version as revision, and the task scope. The trusted
`SemanticPlanningHost` MUST authenticate the pin and target association before
handoff. This contract version is independent of CGSL, Gherkin and Process IR
versions.

The application MUST retain the complete immutable task alongside the Plan.
When a task pin exists, `SemanticPlanHandoff::resolve` requires all three identity
components to match `CompositionRules.processes.required_definition`; absent or
different rules return `ResolutionMismatch`. A caller obtains that identity from
the compiled catalog and explicitly sets the existing CG-08 rule. There is no
latest-version fallback, prose-based lookup, or implicit rewrite of the task.

An absent task pin leaves selection to explicit CG-08 rules and the captured
process snapshot. It does not imply "no process permitted". A required pin makes
selection mandatory even with `TemplatePreference::None`. CG-08 still checks
activity capability/output/lifecycle contracts and any pinned instance identity.
A valid pin can therefore produce `Missing` or `Incompatible`. `resolve` returning
`Ok` means a report was produced; callers MUST inspect its resolution and process
selection outcomes before proceeding. Ambiguity remains explicit in CG-08.

## Reference sequence

```mermaid
sequenceDiagram
    participant S as CGSL frontend
    participant A as Semantic planning application
    participant R as CG-08 resolver
    participant P as CG-04
    P->>P: Compile review.feature and validate definition/digest
    S->>A: SemanticTaskIR with exact ProcessReference
    A->>A: Host admission; CG-06 basis; CG-07 Delta and Plan
    A->>R: Full handoff + captured catalog + explicit required_definition
    R->>R: Exact identity and activity compatibility
    R-->>A: Resolution report with process candidate
    P->>P: Evaluate explicit instance/event and gate inputs
    P-->>P: Reject without gate; accept with passed gate
```

The executable reference scenario is
`process_boundary_reference_scenario_and_denied_handoffs` in
`crates/gateway-application/tests/semantic_planning.rs`. It compiles
`tests/fixtures/semantic-process-boundary/review.feature` through the real CG-04
registry, inserts the resulting pin into the existing minimal CGSL source, and
uses the real CGSL compiler, CG-06 mapper, CG-07 planner and CG-08 resolver.
The task requests an analysis capability; the selected process controls the
review of that analysis. Its `review.accepted` event cannot pass without the
`analysis-review` gate, regardless of task validity or process selection.

The scenario proves deterministic unique process selection, complete task
preservation, mismatched ID/version/digest rejection, mandatory-rule rejection,
missing catalog rejection, changed-content rejection and incompatible activity
rejection. CG-04 evaluation then rejects absent gate input and accepts an explicit
passed gate, without mutating the task or instance. This is component integration
with an exact captured fixture host, not installed-client, persistent runtime,
policy issuance or effect execution evidence.

## Requirement review and acceptance

Review basis: `492040bae2b039ef045d62ebd0160cb9e4683b6d`, initially clean checkout,
DEVELOPMENT mode. The Three Amigos review was performed as role passes by one
agent. Reviewed #191, #177 including its October 4 and October 6 additions,
ADR-015, the task contract and actual planning/resolution/process implementations.

Requirement pass: prove separate composable layers, retaining output, verification
and evidence obligations. Architecture pass: existing owners and reference types
suffice. Automation pass: add tests and docs to existing workspace/architecture
gates; no launcher or storage change. Evidence pass: require actual dual-frontend
compilation, exact-pin and applicability failures, and CG-04 gate evaluation.
Decision: **READY_FOR_WORKFLOW**. #179 scope and #190 handoff prerequisites are
implemented in this revision. No new host or process model is required.

Ordered scope: dependency mutation tests; source/IR rejection tests; reference
integration scenario; normative documentation; regression and quality checks.
Estimated effort at intake: 0.5–1 engineer-day implementation and 0.5 qualification,
moderate confidence, existing foundations included. Rollback removes additive
tests/fixture/docs. Stop if proof requires changing lifecycle ownership or adding
runtime invocation. The boundary itself adds no production semantics. The later
user-authorized gate repairs are reviewed separately in the
[Three Amigos completion review](epic-05.13-three-amigos-review.md), including
measured coverage for the equivalent Rust changes and native memory helper.

| ID | #191 criterion | Production path / dependency | Verification level | Status |
| --- | --- | --- | --- | --- |
| P01 | No CGSL transition/state-machine semantics | CGSL compiler and strict task/reference deserialization | CONTRACT: lifecycle fields rejected at root and in process pin; raw Gherkin rejected | VERIFIED |
| P02 | Process IR independent of CGSL parser | Normalized Cargo graph, shared domain | ARCHITECTURE: both directions and domain cycles rejected across aliases/kinds/targets | VERIFIED |
| P03 | Explicit typed process selection | Semantic handoff -> CG-08 exact identity and activity matching | COMPONENT: positive selection and denied reference/catalog/activity paths | VERIFIED |
| P04 | CG-04 remains authoritative | Compiled Gherkin -> transition evaluator | COMPONENT: task selection cannot satisfy a gate | VERIFIED |
| P05 | Boundary in arc42 and tested | arc42 section 8.12 and this specification | DOCUMENTATION + executed tests | VERIFIED |

Reproduce with `cargo test -p gateway-application --test semantic_planning -p
gateway-cgsl --test frontend --locked`, `python -m unittest discover -s
tests/architecture -p "test_*.py"`, and `python scripts/check-dependencies.py`.
All are included by existing workspace test and architecture quality gates.
Execution results and limits are retained in
[EPIC-05.13 evidence](evidence/EPIC-05.13-process-boundary.json).
The bounded criteria above pass. The initially blocking lint, portability and
test-observation failures have been repaired and revalidated: workspace Rust
tests, workspace Clippy, architecture guards and contract tests pass. The final
review finds #191 ready for completion. Three existing PostgreSQL-only tests are
explicitly ignored by the normal workspace command; full installed-client,
database and container release qualification is not claimed by this boundary
issue. This report records qualification before commit and issue closure.
