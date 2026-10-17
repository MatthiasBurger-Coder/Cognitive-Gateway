# CGSL reference examples

These walkthroughs connect natural-language intent to formal semantics and the
planning, process and prompt boundaries. Natural-language sentences are explanatory
inputs. The bounded inspection profile below is executable; other walkthroughs
do not claim that a shipped general interpreter produces these documents.
All fixture pins and hosts are synthetic. Follow the complete linked artifacts;
abbreviations in this prose are not valid reference operands.

## Inspect an artifact

Executable supported request: `Inspect artifact artifact-1.` The public
`CompletenessValidator::compile_inspection_request` service parses the original
sentence and resolves its target against permitted captured registry/project
records. With the minimal fixture's authenticated output contract and task
identity it produces exactly the same canonical task bytes and digest as the
formal source below. `Inspect it.` produces that same task when the unique
eligible focus is `artifact-1`; missing or ambiguous focus returns diagnostics
and a clarification plan without executable IR. See the
[profile and host contract](cgsl-implementer-guide.md#supported-natural-language-inspection-profile).

```sh
cargo test -p gateway-application --test inspection_language --locked
```

Request: “Inspect artifact-1 in project-x and report its properties.” The complete
formal source is [minimal.cgsl](../examples/cgsl-v1/minimal.cgsl). The captured
basis must authenticate `artifact-1`, kind `ARTIFACT`, scope `project-x`, contract
`cg.artifact` 1.0, revision `7`, digest consisting of 64 `a` characters. The output
schema is separately pinned as `ArtifactInspection` with contract
`cg.output-schema` 1.0 at that same scope/revision/digest. Those fixed digest
values describe fixture premises, not hashes of real artifact contents.

The request resolves to task `task-inspect`, type `INSPECT`, outcome
`inspect-artifact`. Output checks are `OUTPUT_SCHEMA_VALID` and
`NO_UNRESOLVED_REFERENCE`. No state, process or extra evidence is inferred.
`compile` returns a shape with empty interpretation sidecars. Its canonical task
bytes must equal [minimal.canonical.json](../tests/fixtures/semantic-task-v1/minimal.canonical.json)
and its digest must equal [minimal.sha256](../tests/fixtures/semantic-task-v1/minimal.sha256).
Successful shape compilation does not authenticate either external pin.

For the executable planning continuation, the
[semantic planning fixture](../crates/gateway-application/tests/semantic_planning.rs)
explicitly transforms this source's task data: it rebinds target/output scope to
`project-a`, adds pinned evidence `evidence`, adds `EVIDENCE_REQUIRED`, and declares
`architecture.dependency-analysis`. `map_cg06_inputs` attaches the complete
[CG-06/CG-08 source fixture](../crates/gateway-application/tests/support/cg08_reference.rs),
its actual desired-state predicates and explicit context/state pins. This is a
new task revision; it does not retain the minimal fixture digest.

`plan_semantic_task` authenticates the exact task/source/capability snapshot with
the fixture host. Existing CG-07 comparison and Delta derivation produce the Plan.
The test checks exact source and Delta equality and verifies that the Plan's
derived capability matches the declaration. `SemanticPlanHandoff::resolve`
revalidates the basis and delegates to CG-08. The result equals direct structured
resolution. The task's output contract, verification checks and evidence survive
unchanged beside the Plan. No capability lineage is inferred from goal prose.

```sh
cargo test -p gateway-application --test semantic_planning --locked semantic_task_to_plan_and_resolution_reuses_existing_contracts -- --exact
```

## Analyze remaining latency after a previous optimization

Request: “ServiceX is still slow after query optimization. Identify remaining
causes, using PERF-142, and assess the 300 ms p95 goal. Return an evidence-backed
analysis; live changes require consent.” The full formal document is
[performance-analysis.cgsl](../examples/cgsl-v1/performance-analysis.cgsl).

| Resolved meaning | Exact source operand and interpretation |
| --- | --- |
| Task and subject | `ANALYZE_PERFORMANCE`, target `SERVICE` / `ServiceX`, scope `project-x` |
| Goal and inputs | Outcome `identify-latency-causes`; incident reference `PERF-142`; typed integer `threshold-ms = 300` |
| Context and current state | Pinned `service-x-context` and `service-x-7`; current numeric latency remains in the referenced state, not invented in CGSL |
| Desired state | Existing CG-06 condition `p95-target`: `ServiceX.p95_ms LESS_OR_EQUAL INTEGER(300)` |
| Observations, evidence and history | Pins `p95-run-7`, `latency-report-7`, `query-optimization`; history is not an instruction to repeat an action |
| Constraint and requirement | `LIVE_MUTATION_REQUIRES_CONSENT`; abstract capability `performance.analysis`, without naming an executor |
| Assumption | `workload-comparable`, boolean premise true, unknown confidence, pinned `workload-note` basis; not verified workload equality |
| Policy and process | Pinned `performance-diagnostics`; definition reference `diagnostics`, version 1, digest of 64 `b` characters; neither reference authorizes execution |
| Output and verification | Pinned `PerformanceAnalysis`; all four checks: output schema, required evidence, supported claims and no unresolved reference |

The document populates all 17 task-mapped constructs. Exact references carry
their contract/version, revision, scope and digest. The frontend compares its
canonical task to [performance-analysis.canonical.json](../tests/fixtures/semantic-task-v1/performance-analysis.canonical.json)
and [performance-analysis.sha256](../tests/fixtures/semantic-task-v1/performance-analysis.sha256).
To admit planning, a real host must supply and authenticate the referenced state,
evidence, schema, process and capability content. The fixture is not proof that
these sources exist in an installed environment. A claim about current latency
requires admissible fresh observations; previous optimization history and the
assumption cannot satisfy it. Missing required evidence blocks completeness.

## Resolve “inspect it” or ask the minimum clarification

Request: “Inspect it.” In a host-bound capture with two equally eligible focus
targets, `artifact-1` and `artifact-2`, neither confidence nor array order selects
one. [incomplete.cgsl](../examples/cgsl-v1/incomplete.cgsl) is a valid inspection
document: `target` is omitted; `ambiguity` preserves both complete typed candidate
values and provenance; `knowledge_gap` records `AMBIGUOUS_ENTITY`, mandatory
target meaning and route `HUMAN_CLARIFICATION`; `confidence` attaches unknown
confidence to the present goal using a pinned basis. It cannot yield a task
projection. The three constructs are valid source operands even though the
document intentionally remains non-executable.

The [portable resolution profile](../tests/fixtures/cgsl-conformance-v1/resolution.json)
provides full captured contexts and frozen candidate expectations. Deterministic
resolution first checks explicit current input, explicit task state, situation,
relevant focus/history, eligible verified memory, permitted retrieval and finally
clarification. A winning tier's ambiguity/conflict is retained; lower tiers cannot
guess its winner. A unique eligible situation target can resolve “it” without
asking. An expired implicit memory candidate is excluded with a reason.

The [completeness fixture](../crates/gateway-application/tests/completeness.rs)
demonstrates the actual reply path. With no target fact, compilation is not called.
The host captures a reply as current-input target `service`, updates the capture
revision to `reply-capture`, and authenticates it at fixture time 101. The same
completeness compiler resolves the target, compiles a task `task` in `project-p`
using the minimal source as its template, and revalidates the emitted bindings.
Changed basis or expired capture rejects before compilation. This separate
fixture demonstrates the mechanism; it does not pretend to persist the two-artifact
conversation. #275 owns correlation/expiry/duplicate reply hosting; clarification
cannot create consent.

```sh
cargo test -p gateway-application --test completeness --locked clarified_request_reenters_full_validation_and_normal_compiler -- --exact
cargo test -p gateway-application --test completeness --locked resumed_compilation_rejects_stale_duplicate_and_changed_scope_projections -- --exact
```

## Continue across Process IR and Prompt IR boundaries

The process continuation uses the first planning fixture with an explicit pin to
[review.feature](../tests/fixtures/semantic-process-boundary/review.feature),
parsed by the real Strict Cognitive Gherkin frontend. The test derives the actual
definition identity/digest and carries it through CGSL `process`. CG-08 requires
that exact definition and selects its `analyze` activity with capability
`architecture.dependency-analysis`. Process state starts at `REVIEW`; the
`analysis-review` gate and `review.accepted` event remain Process IR/engine-owned.
A task's output/verification checks cannot pass the gate or transition to
`COMPLETE`. The same test rejects changed definition pins, missing definitions and incompatible activities;
the engine denies transition without its required gate. See the [full boundary proof](semantic-process-boundary.md).

```sh
cargo test -p gateway-application --test semantic_planning --locked process_boundary_reference_scenario_and_denied_handoffs -- --exact
```

The prompt continuation is a separate
[executable reference](../crates/gateway-context/tests/prompt_handoff_reference.rs),
using the original minimal task and real CG-10 `CompiledContext`. It adds an
evidence fragment with provenance, freshness, trust and scope and the complete
[supplement](../tests/fixtures/prompt-handoff-reference/supplement.json) containing
illustrative context-plan and example metadata. One renderer emits system/user
messages, another emits named blocks. Decoding both must recover identical task,
context, output, verification, planning and example sections. Hostile example text
remains non-authoritative data and never enters system instructions. Neither
renderer fetches content or calls a model.

```sh
cargo test -p gateway-context --test prompt_handoff_reference --locked
```

This reference proves preservation across two layouts. It is not a production
wire schema or a single integrated natural-language-to-model run. #285/#286 own
canonical producer conformance; #263 owns actual invocation admission and routing;
#266/#267 own independent runtime result verification. The [Prompt IR contract](prompt-ir-handoff-boundary.md)
specifies every required section and failure boundary.

## Construct coverage and counterexamples

| Construct(s) | Valid nonempty example |
| --- | --- |
| `task`, `goal`, `target`, `output`, `verification` | Minimal inspection source |
| `input`, `context`, `state`, `desired_state`, `observation`, `history` | Performance source |
| `constraint`, `requires`, `evidence`, `assumption`, `policy`, `process` | Performance source |
| `knowledge_gap`, `ambiguity`, `confidence` | Incomplete inspection source; valid sidecars intentionally block executable projection |

All 20 constructs have complete source operands. For major negative cases, run
the [example manifest](../examples/cgsl-v1/expectations.json) and
[portable conformance corpus](../tests/fixtures/cgsl-conformance-v1/README.md):

| Case | Expected boundary |
| --- | --- |
| Duplicate decoded key, trailing comma, comment, lone surrogate/invalid UTF-8 | Syntax diagnostic; no semantic admission |
| `objective` alias, shorthand target string, provider output member, missing goal | Closed vocabulary/typed operand/mandatory-field rejection |
| Unsupported 1.1 or 2.0 source, malformed version | Version rejection before operands; no downgrade |
| Two eligible targets or incompatible same-tier facts | Ambiguous/conflicting state with all candidates; no executable task |
| Stale explicit pin, foreign scope, wrong digest/kind | Reference/admission failure; no latest fallback or scope widening |
| Unavailable host/source/model | Explicit failure or retained deterministic result as appropriate; no fabricated semantics |
| Prompt-like retrieved/example text | Retained as data with provenance; no policy, instruction or consent authority |

Diagnostic names in the specification include reserved integration codes. The
[conformance guide](cgsl-conformance.md) identifies current emitter limits;
host `Scope`/`Admission` failures must not be relabeled as frontend diagnostics.
