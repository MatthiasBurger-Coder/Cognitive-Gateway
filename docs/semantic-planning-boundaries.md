# Semantic task integration with CG-06/07/08

Issue [#190](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/190),
parent #177. The application module `semantic_planning` implements the typed
handoff in ADR-015. It imports existing contracts and application facades; no
Intent, Plan, Resolution, parser, session coordinator or execution model is added.

## Requirement review and ordering

Review basis: `6b038097d34134b48e5883f35e1cec33db16f6ce`, initially clean checkout,
DEVELOPMENT mode. The Three Amigos gate was performed as role passes by one agent,
not independent reviewers. Reviewed #190 and the full #177 body, including the
October 4 output/verification/evidence addition and October 6 context extension.

Requirement pass: deliver typed integration and deterministic component evidence;
do not infer predicates from goal prose. Architecture pass: CG-06 owns the source
aggregates, CG-07 owns Delta lineage, CG-08 owns provider and process selection.
Automation pass: implement the adapter in gateway-application with the parser
remaining a dev-only fixture dependency. Evidence pass: cover stale bindings,
missing mandatory semantics, conflicting source data and unchanged structured
operation, plus the positive chain and >=95% measured production line coverage.
Decision: **READY_FOR_WORKFLOW** for this component boundary slice.

| Prerequisite | Inspected implementation | State and ordering |
| --- | --- | --- |
| #180 task contract | domain `semantic_task.rs` | Present; immutable shape, exact reference validator port |
| #186 frontend | `gateway-cgsl` compiler | Present; only validated shapes can enter this adapter |
| #187 resolution | application `interpretation.rs` | Present; host authenticates exact pins and captured projections |
| #188 completeness | application `completeness.rs` | Present; host authenticates mandatory manifests; gaps must be resolved before handoff |
| CG-06/07/08 | situation/planning/resolution application facades | Present; adapter delegates to these owners |

Implementation order: source migration/admission, CG-07 delegation, CG-08
compatibility check, deterministic fixtures, measured coverage and documentation.
Planning estimate: 1–2 engineer-days implementation and 0.5–1 qualification,
moderate confidence; existing foundations included as dependencies, no new runtime
host or deployment included. Rollback removes the additive module/export and gate;
existing structured entrypoints remain compatible. Stop if an integration needs
new domain semantics or a second process/runtime owner.

## Mapping specification

| Source | Compilation / planning mapping | Authority and preservation |
| --- | --- | --- |
| CG-06 `Intent.desired_state` | Inline existing `DesiredState` | Copy exactly; reject differing explicit task predicates |
| `DeclarativeContext` | Caller supplies its pinned `context_refs` entry | Match ID; reject a conflicting pin; never invent digest/revision |
| `ObservedState` | Caller supplies pinned `current_state` | Match ID; preserve unknown/conflicted observations in the CG-06 document |
| `Situation` and observation/evidence records | Retain complete `DeclarativeContextSituationDocument` | Existing situation becomes CG-07 Delta basis; no conversion of assessments to facts |
| Task desired state | Existing comparison, Delta, capability derivation, planner | Missing predicates fail explicitly; no interpretation of goal prose |
| Abstract capability declarations | Match against actual Delta-derived requirements | Never fabricate lineage. An unmatched declaration fails, including on an empty/no-work derivation |
| Output, verification, evidence, constraints, policy, assumptions, history | Preserve complete immutable task in `SemanticPlanHandoff` | No flattening into Plan or conversion to policy/consent authority |
| Process pin | Preserve task pin; require identical CG-08 `required_definition` | Existing CG-08 performs selection/applicability; detailed Process IR proof remains #191 |
| Plan, Delta, desired state, situation | Exact-match against CG-08 snapshot inputs | No rewriting of caller snapshots, registry, Agent/Skill candidates or resolution rules |

`map_cg06_inputs` accepts construction data, a complete CG-06 document and explicit
context/state pins. It returns a structurally validated task. Optional CG-06
Intent can be absent when the producer already supplies desired-state predicates.
Original free text is not parsed, copied into execution context or made authority.

`plan_semantic_task` accepts that task, its source document, a trusted
`SemanticPlanningHost`, Delta ID, capability snapshot and explicit rules. Admission
checks source identities and desired-state equality, validates every task reference
through `SemanticReferenceValidator`, and invokes the host's required
`validate_source`. The host must authenticate the supplied capability snapshot
against the pinned declarations and the exact document contents against
the pins, target association and the complete mandatory semantic manifest. Merely
matching IDs or accepting JSON is not a conforming host. Output schemas and evidence
pins must exist at the captured revision; missing mandatory meaning must fail.
There is intentionally no permissive default host.

The resulting envelope contains existing `SemanticTaskIR`, CG-06 document, Delta,
Plan and capability snapshot. It is not execution authorization. `resolve`
rejects a changed capability index, revalidates admission and
compares the entire planning basis before calling the unchanged CG-08 facade.
Callers retain the envelope alongside `ResolvedPlan`; extracting Plan alone loses
semantic obligations and is not a supported migration to #192/#263. Runtime
invocation, actual output verification and policy evaluation remain downstream.

## Compatibility and migration

Existing structured consumers keep `DeclarativePlanningApplication` and
`DeclarativeResolutionApplication` unchanged. Semantic consumers add the source
mapper when starting with CG-06 data, then call `plan_semantic_task`. Formal CGSL
and natural-language completeness outputs use the same typed boundary. Neither
production planning nor resolution imports the frontend or a model provider.

Failures are typed: invalid reference/source admission, source mismatch, absent
desired state, unmapped capability, underlying planning error, blocked planner,
resolution-basis mismatch or underlying CG-08 error. These are not permissions
to retry with newer pins, drop requirements or choose a different process.

The fixture `crates/gateway-application/tests/semantic_planning.rs` compiles the
existing `examples/cgsl-v1/minimal.cgsl`, maps the existing structured resolution
fixture, runs real CG-07 planning and CG-08 resolution, and compares it with direct
structured resolution. It uses an exact captured fixture host, not an installed
client or external authoritative store. Production host wiring belongs to the
invocation owner; no new CLI command or runtime capability is claimed here.

## Acceptance and verification

| ID | Requirement/source | Production owner/path | Evidence level | Status |
| --- | --- | --- | --- | --- |
| B01 | #190 reuse CG-06/07/08 | source mapper -> existing planning/resolution facades | COMPONENT: deterministic integration fixture | VERIFIED |
| B02 | #190 no duplicate domain models | immutable envelope of existing contracts | CONTRACT: source/API review | VERIFIED |
| B03 | #190 typed validated planning entry | `plan_semantic_task`, required host admission | COMPONENT: positive, denied and missing-input tests | VERIFIED |
| B04 | #190 structured compatibility | unchanged structured facades | COMPONENT: direct resolution equality | VERIFIED |
| B05 | #190 hexagonal direction | application -> domain/CG-07/08; CGSL dev-only | CONTRACT: normalized Cargo dependency guard | VERIFIED |
| B06 | #190 deterministic behavior | immutable explicit inputs and rules | COMPONENT: repeated run equality | VERIFIED |
| B07 | #177 preserve output/verification/evidence | full task retained beside Plan | COMPONENT: exact task/contract equality and rejected admission | VERIFIED |
| B08 | #177 >=95% new production coverage | semantic_planning.rs | COMPONENT: llvm-cov and dedicated CI gate | VERIFIED |

Run `cargo test -p gateway-application --test semantic_planning --locked` and the
EPIC-05.12 entry in `scripts/quality-gates.json`. Retained, source-bound results and
broader repository limitations are in
[`EPIC-05.12-semantic-planning.json`](evidence/EPIC-05.12-semantic-planning.json).
These results qualify this component, not the full parent epic or installed runtime.
