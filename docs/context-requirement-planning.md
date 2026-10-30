# Context requirement analysis and source selection

Historical evidence and removed review documents are linked to their archived Git
revision. They are not current acceptance evidence. Generate fresh reports in an
output directory or retain them as CI artifacts.

Issue #285 implements EPIC-05.17 between validated task meaning and acquisition.
It follows ADR-007/008/015 and the [#192 boundary](prompt-ir-handoff-boundary.md).
CGSL remains a task language. CG-15 owns retrieval, budgets and sufficiency;
CG-10 owns compilation. No persistence, launcher, policy engine or retrieval
implementation is added. Production invocation admission remains #263 owned.

## Contracts and ownership

`gateway-domain::context_planning` owns output-only `ContextRequirementsIR` 1.0
and `ContextPlanIR` 1.0. Constructors validate supported versions, duplicate
identities, incompatible trust/source constraints and finite bounds. Aggregate
fields are private and immutable. New consumers must retain these shared types;
the JSON value projections are canonical exports, not permissive input schemas.
A future input reader must reconstruct through validating constructors.

`gateway-application::context_planning::analyze_context_requirements` accepts a
`SemanticTaskIR` and `ContextAnalysisInput`. Its host first validates all task
reference pins and semantic completeness, then authenticates the complete input
against the captured task/Situation/Plan basis. A shape-valid task alone is
insufficient. The host must reject missing, ambiguous or conflicting mandatory
meaning through the existing #188 admission path. Host validation must not
resolve replacements, acquire information or grant permissions.

The input includes the exact task/Situation/Plan pins, host-validated CG-07
`PlanningInput` projections and additional explicit typed needs. The host must
verify that projections were derived from the captured Situation/Plan, including
condition activation. Existing `derive_planning_inputs` remains the CG-07 owner;
this analyzer does not reproduce Delta or plan derivation. Nonempty planning
inputs require both Situation and Plan bindings. Their identities receive the
`planning-` prefix, their purposes retain CG-07 required outcomes, and their
`InformationRequirements` survive unchanged. Additional needs cannot replace or
weaken generated needs: duplicate identities fail closed.

The deterministic baseline is:

| Task semantics | Requirement / necessity | Permitted source preference |
| --- | --- | --- |
| Every task | Authority / mandatory | Catalog authority |
| Every task | Task input / mandatory | Current input, task state |
| Every task | Output and verification / mandatory | Task state |
| Existing target, or explicit current state | System state / mandatory | Repository, Situation state, runtime state |
| Entity target | Structured data in place of system state | Structured data, Situation state |
| Create or modify artifact/project | Domain knowledge / optional | Repository, RAG |
| Evidence pins or evidence verification | Evidence / mandatory | Situation state, repository, RAG, tools |
| Explicit history | Governed history / mandatory | Task state, memory |
| Observations or current state | Runtime state / mandatory | Situation state, runtime state, tools |
| CG-07 information-resolution inputs | Mandatory typed needs | Mapped by input kind: evidence, observation, explicit input or normalization |
| Explicit host-admitted additional semantics | Mandatory, optional, or conditional | Exact declared ordered source classes |

Create tasks without current state need no existing-target requirement.
Examples, architecture/domain knowledge, tools, memory and structured data can
also be requested explicitly through additional typed needs. Goal prose and
retrieval availability never infer new requirements. Conditional requirements
retain both condition identity and evaluated activation in the bound IR.
Optional means eligible but nonblocking when absent; inactive conditions select
nothing. No blanket history or implicit example selection occurs.

Every requirement retains identity, class, purpose, necessity, ordered permitted
sources, explicit accepted trust set, freshness/evidence/provenance requirements,
minimum/maximum sensitivity, CG-15 `RetrievalBudget`, stop condition and priority.
`RequiredInformation`, `InformationRequirements`, `ContextBudget`, token/cost/
round/result/latency bounds and `StopCondition` are existing shared contracts.
There is no parallel retrieval or policy schema.

## Planning, absence and bounds

`select_context_sources` receives only immutable requirements and finite trusted
source metadata. Metadata is supplied by the admitted host, never accepted as
an authenticity claim from retrieved content. `covers` binds source relevance
to requirement identities; the host verifies target association. Scope is exact:
foreign sources are rejected, even when their IDs match. A cross-project task
needs explicit upstream semantic admission into the intended scope; the planner
cannot authorize a scope exception.

Selection serves mandatory and active conditional requirements first, then optional
needs. Each group uses requirement priority/ID order. Within each requirement it
prefers already-present eligible context, then declared source preference, then
source ID. Duplicate source identities fail closed. Trust uses explicit
membership, never numeric ranking. Unknown freshness fails a fresh requirement;
unresolved conflicts, uncertainty, excessive sensitivity and infeasible token
reservations reject a candidate. Present context must already carry its payload
digest and required evidence/provenance. Pending acquisition retains those
obligations without inventing result IDs or requiring unknown result bytes. The
evidence stop threshold is a distinct-evidence count checked on actual retained
context, reusing CG-15 semantics. Quality
labels express suitability and do not authenticate a source or authorize use.

Each chosen source must fit its per-requirement retrieval/context token limits
and the appropriate disjoint CG-15 context reservation. Distinct source IDs are
charged once against the total retained-context token cap. Other acquisition
limits remain attached unchanged for the acquisition owner to enforce. Selection
never invokes retrieval, a model, memory or tools and never loops until confident.
Unavailable mandatory needs remain in `missing`; selected acquisition sources
remain pending until CG-10 proves satisfaction. Missing context is never invented.
A class with no active permitted need receives `NONE_REQUIRED`. Candidate decisions
carry stable machine-readable selected/rejected reasons, with static human
explanations available through `ContextSelectionReason::explanation()`.

`context_retrieval_plan` adapts an explicitly selected non-present repository,
RAG or memory source into CG-15 `RetrievalRequest` / `RetrievalPlan`. It preserves
information constraints, scope, Situation, step, budget and stop conditions.
Baseline retrieval descriptions retain the explicit target and outcome identities;
CG-07 descriptions retain the required outcome and target.
The host supplies the exact source/strategy implementation binding and declared
support. Unsupported source kinds fail explicitly. Structured data, tools and
clarification remain acquisition obligations of their existing owners; the
adapter cannot turn them into a second retrieval engine. Present context cannot
be adapted into a retrieval request. No availability-dependent rewrite of task
meaning or requirements occurs.

## Canonical identity and CG-10 handoff

Requirements bind the exact task schema version, scope, revision and task
`content_digest()`, plus optional Situation/Plan version/revision/digest pins.
Requirements and plans include schema version 1.0 and explicit revision IDs.
Canonical JSON uses compact UTF-8 `serde_json::Value` output: lexically ordered
object keys, canonical necessity/priority/ID order, sorted decisions, BTree
sets/maps, and ordered source preferences. SHA-256 hashes the complete projection
without whitespace or a trailing newline. Requirements and plan `binding()`
return existing `ResolvedReference<ReferenceId>` values. Source pins bind the captured source snapshot and revision. A separate optional
`payload_digest` binds retained bytes: it may be unknown before acquisition and
must be supplied for actual satisfaction. Any known planned payload digest must
remain exact. A reference-only fragment hashes its reference bytes and is never
materialized implicitly.

`ContextCompiler::handoff_planned_context` is the actual CG-10 integration.
It accepts the canonical producer records, a real `CompiledContext`, an expected
exact context snapshot pin, acquired-source satisfaction metadata, and the existing
resolved `semantic_output::OutputContract` / `VerificationContract`. It reuses
`validate_contracts` and retains the full contracts, including rule/verifier pins.
The trusted host authenticates snapshot revisions and source pins upstream;
this pure boundary checks shape, exact content correlation and satisfaction.
It does not claim live authenticity, consent or invocation admission.

The handoff rejects changed task/requirements/context bindings, cross-scope
context, missing mandatory or active conditional satisfaction, stale/untrusted
or over-budget actual metadata, missing/wrong fragment links, unexpected
acquisitions and unplanned dynamic fragments. Actual source class and pin must
match the canonical selection; actual tokens cannot exceed the planned bound.
Fragment quality, payload digest, source/revision and evidence/provenance links
must match supplied metadata and satisfy the original requirement. CG-10's
existing scope/step validation remains in force.

Present task/output/catalog satisfaction is structural. Task digest binds the
whole task; output satisfaction binds the resolved output/verification definitions;
catalog satisfaction binds CG-10's stable catalog reference section. Only catalog
context may satisfy authority, and it never grants permission. External fragments
cannot satisfy instruction/authority requirements. Memory and evidence retain
their existing CG-10 kinds and metadata. Requirements of class Examples require the dedicated validated example section.
[#286 implements bounded example production and the dedicated section](dynamic-example-selection.md);
missing mandatory examples block this handoff.

`PlannedContextHandoff` retains the exact task, requirements, plan and compiled
context. Its canonical projection separates instructions, dynamic context,
evidence, optional canonical examples, output and verification. It
retains complete planning records, satisfaction links and exact task/requirements/
plan/context/execution-projection/output/verification bindings. Two offline test
layouts preserve the entire payload unchanged. Neither handoff nor rendering has
an acquisition port. #263 still owns the full production Prompt/Invocation IR,
current authority authentication, routing admission and invocation enforcement.

## Delivery review and evidence

Review basis: `adab916eed911015f23adeab9c68ce270e6f76e3`, initially clean checkout,
DEVELOPMENT mode. Sources: #285 including October 17 handoff criteria, #177,
#192, ADR-007/008/015, semantic completeness/planning, CG-10, CG-15 and release
gates. Three Amigos review used requirement, architecture, automation and tester
role passes by one agent; no independent review or approval is claimed.

Readiness: **READY_FOR_WORKFLOW**. Typed semantic admission, planning inputs,
CG-15 budgets and the #192 specification are available. Ordered implementation:
shared contracts; semantic analysis/source selection and CG-15 mapping; real
CG-10 conformance; adversarial/reference proof; docs and per-file coverage gate.
Estimated effort including qualification: 3–5 engineer-days implementation and
1–2 qualification, moderate confidence; foundations already exist. No storage or
deployment migration. Rollback removes the additive modules, exports, tests,
examples, documentation and gate. Stop on contract ownership conflict, regression
or failure to meet the 95% requirement. Production invocation and bounded example
production are separate owning issues, not hidden prerequisites for #285.

| ID | Requirement/source | Production owner/path | Verification | Status |
| --- | --- | --- | --- | --- |
| C01 | Validated task -> typed requirements | Domain contracts; application analyzer; completeness/reference host | Admission denial, exact basis, baseline and CG-07 input tests | VERIFIED |
| C02 | Source classes may be unnecessary | Source selector | Explicit RAG/memory/examples/data/tools `NONE_REQUIRED` | VERIFIED |
| C03 | Necessity, constraints, budgets and stop | Shared CG-15 types in requirements | Mandatory/optional/conditional, conflicts, evidence, reservation and total-bound tests | VERIFIED |
| C04 | Already present vs acquisition | Source selector and retrieval adapter | Existing content preference; no present-source retrieval | VERIFIED |
| C05 | CG-15 mapping without duplication | `context_retrieval_plan` | Exact constraints/budget/stop/scope preservation, memory mapping, unsupported tools | VERIFIED |
| C06 | Explainable source decisions | Domain reason enum; canonical plan | All static explanations and machine reason fixtures | VERIFIED |
| C07 | Availability-independent meaning and determinism | Requirements/plan projections | Reversed candidates, unavailable sources, immutable task/digest | VERIFIED |
| C08 | No authority or cross-project promotion | Requirements validation and CG-10 | Canonical trust rejection, foreign scope, catalog-link and fragment tests | VERIFIED |
| C09 | Negative fixtures/reference tasks | Rust conformance tests and JSON examples | Coding/investigation/structured data; unavailable/conflicting/stale/untrusted/budget cases | VERIFIED |
| C10 | Actual #192 producer conformance | `ContextCompiler::handoff_planned_context` | Canonical records and exact pins through real CG-10, full output/verification, offline layout equality | VERIFIED |
| C11 | Missing/stale/cross-scope/unsatisfied handoff | CG-10 link validation | Negative pin, quality, payload, missing-link and unplanned acquisition tests | VERIFIED |
| C12 | No hidden retrieval or prompt/policy engine | Pure application/CG-10 modules | Offline Rust proof and architecture guards | VERIFIED |
| C13 | >=95% changed production coverage | Three new production modules | Dedicated per-file measured gate, included in release checks | VERIFIED |

Executed commands, per-file measurements and completion status are recorded in
[evidence/EPIC-05.17-context-planning.json](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-05.17-context-planning.json).

Completion review: **VERIFIED** at the canonical producer/CG-10 handoff level.
The 20 new conformance tests and 660 Rust regressions across the four affected
crates passed. Architecture (43 tests, one existing platform skip), contracts
(17 tests), dependency guard, formatting and scoped all-target Clippy passed.
Measured line coverage: domain 339/340 (99.71%), application 358/359 (99.72%),
CG-10 handoff 212/214 (99.07%). The dedicated per-file 95% gate passed and is
included in release qualification. Injected fixture-host proof does not establish
live reference authenticity, installed-runtime invocation or production invocation admission.
