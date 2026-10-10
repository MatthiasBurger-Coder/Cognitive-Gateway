# CGSL scope, semantic boundaries and canonical vocabulary

## Status and authority

This is the normative scope and vocabulary specification for EPIC-05.01
[#179](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/179),
under [EPIC-05 #177](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/177).
It refines [ADR-015](adr/ADR-015-semantic-task-ir-boundary.md) and the
[semantic interpretation architecture](semantic-language-and-interpretation.md).
The vocabulary baseline is **CGSL 1.0**. This document specifies semantic
contracts; it does not establish an implemented parser, compiler or runtime
handoff. Those capabilities remain [planned](current-architecture-state.md).

MUST, MUST NOT, SHALL and SHALL NOT express mandatory conformance rules.
SHOULD expresses a recommendation whose exceptions need a documented reason.
MAY expresses an optional capability. Examples and proposed type names are
informative; the meanings, ownership and validation obligations are normative.

## Scope

CGSL describes the resolved meaning of a task: what outcome is sought, which
subject it concerns, which inputs and knowledge support it, what restrictions
apply, and what output and verification are required. `SemanticTaskIR` is the
canonical typed result of semantic compilation, not another language of intent.

CGSL owns the finite semantic vocabulary, composition of task meaning, typed
references to existing domain contracts, explicit interpretation incompleteness,
and preservation of provenance and epistemic distinctions at the handoff.
Only uniquely resolved, valid mandatory semantics MAY enter an executable
`SemanticTaskIR`. An inspection or clarification candidate MAY retain gaps,
ambiguity and conflict, but MUST NOT be mislabeled executable.

CGSL MUST NOT define process activities, events, transitions, gates, blockers,
retry or scheduling behavior. It MUST NOT derive a Delta or Plan, select an
Agent/Skill/provider, grant permission, or perform retrieval or execution.
Provider message roles, prompt templates, model identifiers, sampling,
temperature, tuning, weights, training and provider SDK options are outside
the semantic core. Output schema requirements express result meaning; a
provider's structured-output transport is a later rendering concern.

The deterministic formal compiler MUST be able to operate without a model or
network. Optional interpretation helpers MAY propose candidates; their output
MUST pass the same deterministic validation and cannot create authority.
Structured callers MAY continue using existing CG-06/07/08 APIs directly.

## Canonical vocabulary

The formal core contains exactly the following **20 construct names**. Names
are case-sensitive lowercase identifiers; singular names remain canonical even
when their operands are collections. Each name has one meaning. Operand type
categories below constrain later schema/grammar work; they are not declarations
of new Rust types or a complete wire schema.

“CGSL validation” means structural, semantic, completeness and reference
validation in the planned compiler. Validation delegated to another domain MUST
use its existing contracts; it cannot be replaced by a permissive CGSL copy.

| Construct | Normative meaning | Operand type category | Semantic ownership | Validation responsibility |
| --- | --- | --- | --- | --- |
| `task` | The canonical classification of the requested work. | Typed task-kind identifier | CGSL task classification | CGSL checks a supported task kind; type inventory belongs to #180. |
| `goal` | The outcome sought by this task, independent of execution procedure. | Typed outcome descriptor | CGSL task outcome; CG-06 owns state predicates | CGSL checks explicit outcome and consistency with any `desired_state`; it MUST NOT invent state predicates from prose. |
| `target` | The uniquely identified entity or subject to which the task applies. | Scoped typed entity/subject reference | CGSL reference binding; source domain owns entity | CGSL checks kind, scope, existence on the captured basis and unique resolution. |
| `input` | Explicit typed data supplied as operands of the task. | Collection of named typed values or references | CGSL operand roles; CG-06 owns original input/value semantics | CGSL checks role/type/reference compatibility; original input remains separate from normalized meaning. |
| `context` | References to relevant knowledge used to interpret the task. | Collection of scoped context references | CGSL relevance to interpretation; CG-06 owns source/scope data | CGSL checks scope, basis and reference integrity; source quality remains CG-06-owned. |
| `state` | The normalized current state relevant to the target. | CG-06 `ObservedState` reference or validated projection | CG-06 | CG-06 normalization/validation; CGSL checks target and basis association, retaining unknown/conflicted entries. |
| `desired_state` | The conditions that define an acceptable target state. | Existing CG-06 `DesiredState` | CG-06 | CG-06 validates conditions, operators, expressions and constraints; CGSL checks target/goal association. |
| `observation` | A measured or directly reported value with provenance. | Collection of CG-06 `Observation` records/references | CG-06 | CG-06 `ObservationEvidenceSet` validates observation/provenance lineage; CGSL preserves it. |
| `history` | References to relevant prior events or actions used as interpretation data. | Collection of scoped historical references | CGSL relevance; originating domain owns events/actions | CGSL checks lineage, scope and relevance; CG-04 validates any referenced process history. History is not an instruction to replay. |
| `constraint` | A mandatory restriction on acceptable task interpretation or realization. | Collection of typed restrictions or existing constraint references | CGSL attachment; CG-02/06/07/09 own applicable restriction semantics | CGSL checks kind and consistency; domain validators check predicates, and CG-09 checks enforcement. Unknown restrictions cannot be treated as satisfied. |
| `requires` | Abstract capabilities needed to realize the task outcome. | Collection of canonical CG-03 capability-contract references | CGSL task declaration; CG-07 owns derived `CapabilityRequirement` | CGSL validates canonical identities/contracts; CG-07 owns Delta lineage and derivation, CG-08 binding, CG-09 permission. |
| `evidence` | References to material supporting or challenging task claims. | Collection of CG-06 `Evidence` references | CG-06 | CG-06 validates provenance and support/challenge links; CGSL preserves them. CG-04/09 separately validate gate/authorization use. |
| `knowledge_gap` | An explicit absence of information needed to resolve a semantic field. | Typed missing-information descriptor bound to subject/field | CGSL interpretation incompleteness | CGSL checks the affected field and missing requirement; #182 defines the detailed contract. It cannot substitute for a value. |
| `ambiguity` | An explicit set of competing interpretations for one semantic field. | Typed field reference plus candidate set | CGSL interpretation incompleteness | CGSL checks distinct compatible candidates and retains them; #182 defines detailed states. Ordering or confidence cannot silently select one. |
| `assumption` | An explicit temporary premise not established as verified knowledge. | Typed premise with basis and affected-field references | CGSL premise declaration; CG-06 owns underlying quality metadata | CGSL preserves premise status and trace; #183 defines its lifecycle. An assumption cannot satisfy mandatory unique resolution or manufacture authority. |
| `confidence` | Metadata expressing confidence in a particular assertion or interpretation. | Existing CG-06 `Confidence`, attached to a typed subject | CG-06 quality semantics | CG-06 validates Score in 0..=1, Unknown or NotApplicable; CGSL checks attachment. Confidence is neither trust nor verification. |
| `policy` | References to authoritative policies applicable to the task. | Collection of canonical policy references | CG-09 | CGSL checks reference integrity; CG-09 loads trusted authority and evaluates independently. A caller reference is not consent or an allow decision. |
| `output` | The structured shape and semantic requirements of the requested result. | Provider-neutral output contract or schema reference | CGSL task result contract | CGSL checks contract completeness/references; #184 defines result validation. Provider adapters only project the validated contract. |
| `verification` | The checks required to establish that a result satisfies the task. | Provider-neutral verification contract | CGSL task acceptance contract | CGSL checks typed checks and output/evidence references; #184 defines evaluation. Declaring a check does not establish that it passed. |
| `process` | An optional reference to a Process Definition relevant to realizing the task. | CG-04 `DefinitionIdentity` reference (ID/version/digest) | CG-04 definition; CG-08 selection/binding | CGSL checks reference kind/integrity; CG-08 checks catalog applicability, CG-04 legality. No inline definition or instance transition is permitted. |

`goal` identifies the sought outcome; `desired_state` supplies any formal
state predicates for that outcome. Neither is a second copy of the other.
`input` supplies task operands; `context` supplies interpretation knowledge.
`state` is normalized state; `observation` is a source report. `evidence`
supports or challenges claims; it is not a synonym for an observation or a
process evidence requirement. `output` states what to produce; `verification`
states how acceptance will be checked. CGSL MUST preserve these distinctions.

## Boundary matrix

| Domain / authoritative contract | CGSL may carry | Responsibility retained outside CGSL |
| --- | --- | --- |
| CG-02 [domain model](domain-model.md) and [ExecutionContextIR](execution-context-ir.md) | Existing typed values, constraints and references | Execution context, mode/profile and execution-state semantics; no new meaning for these enums |
| CG-03 [catalog boundary](catalog-boundaries.md) | Canonical abstract capability references | Agent/Skill/capability membership and definitions; no task-local catalog mutation |
| CG-04 [Strict Cognitive Gherkin](strict-cognitive-gherkin-v1.md) and [Process IR](process-catalog.md) | Optional definition reference; read-only history references | States, transitions, activities, gates, evidence requirements, blockers, retries and instance storage |
| CG-06 [declarative context and situation](declarative-context-situation.md) | Existing intent/desired-state, observation, evidence, normalized-state, quality and scoped-context contracts | State normalization, Situation assembly, provenance, trust, freshness and information lifecycle |
| CG-07 [declarative planning](declarative-planning.md), [comparison](comparison-semantics.md), [capability requirements](capability-requirements.md) | Validated task outcome, state/desired-state and abstract capability needs | Comparison, Delta, requirement derivation, Plan/PlanStep, dependencies and completion/verification conditions |
| CG-08 [resolution contract](resolution-contract.md) and [process resolution](resolution-process.md) | Resolved entity references and task declarations as upstream input | Concrete Agent/Skill/Process bindings, alternative cardinality, readiness and current-basis revalidation |
| CG-09 [policy engine](policy-engine.md) | Restriction declarations and policy references | Trusted authority, authenticated consent, capability authorization and fresh allow/deny decisions |
| EPIC-05.17/18 [#285](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/285) / [#286](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/286) | Task meaning from which context needs can be derived | Information-class requirements, permitted source-class selection and dynamic example selection; selected examples remain non-authoritative data |
| CG-10 [context compiler](context-compiler.md) | Validated task, output/verification contracts and selected references | Final minimization/assembly, Semantic TAG and ExecutionContextIR projection; interpretation context is separate |
| EPIC-02 [retrieval](retrieval-plane.md) and [budgeting](context-budgeting.md) | Knowledge needs and provenance-preserving references | Acquisition, ranking, trust handling, budgets and compaction; CGSL does not fetch or authorize sources |
| Prompt IR / EPIC-05.14 [#192](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/192) | Validated task contracts for a later handoff | Prompt/execution instruction assembly with context and evidence structurally separated |
| Provider adapters / EPIC-06 [#178](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/178) | Provider-neutral semantics and result requirements | Message formatting, templates, runtime/model selection, tuning and invocation |
| Trusted host / EPIC-08 [#271](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/271) | Task/scope/revision-bound clarification requirements | Interaction, reply correlation, expiry and persistence. Clarification answers cannot become consent. |

Semantic entity/reference resolution answers which subject the user means.
CG-08 resolution answers which concrete catalog providers can realize a Plan.
They MUST NOT be merged into a second executor-selection engine in CGSL.

## Formal-core closure and version discipline

The construct table is the exhaustive vocabulary registry for this baseline.
Task-kind identifiers, schema identifiers, entity references and typed operand
members are not additional constructs. Fact, inference and hypothesis remain
distinct epistemic categories inside referenced records; they are not aliases
for `observation`, `evidence` or `assumption` and do not introduce new top-level
keywords. Conflict MUST remain explicit in interpretation diagnostics and
existing CG-06 data, rather than being hidden as absence or ordinary ambiguity.

The formal frontend MUST reject unknown names, incorrect case and semantic
aliases. For example, `objective` for `goal`, `current_state` for `state`,
`inputs` for `input`, `constraints` for `constraint`, `capabilities` for
`requires`, `workflow` for `process` and `prompt` for `output` are invalid
construct names. Historical epic sketches and Rust/wire field names do not
extend this source vocabulary. A separate natural-language interpretation
frontend MAY recognize synonyms and propose canonical constructs, with source
traceability; a formal parser MUST NOT silently perform that conversion.

A new construct MUST have distinct semantics, an explicit versioned
specification change, ownership and validation rules, and conformance cases.
An implementation MUST NOT accept it in a version that does not specify it.
Changing an existing construct's meaning is incompatible. CGSL version and
SemanticTaskIR schema version are separate contracts; neither implies automatic
compatibility with CG-06/07/08 schema versions. Unsupported versions MUST fail
closed. #180 owns the detailed compatibility contract; #185 owns grammar,
serialization and diagnostics. This slice does not freeze cardinalities,
required-field profiles, lexical syntax or wire-field spelling ahead of them.

## Reference examples

These are semantic sketches, **not parseable fixtures or approved wire syntax**.
All references below stand for validated, scoped, revision-bound upstream
contracts. URI spellings are illustrative. #185/#193 own executable fixtures.

### Resolved performance analysis

| Construct | Example semantic operand |
| --- | --- |
| `task` | AnalyzePerformance |
| `goal` | Identify remaining latency causes for ServiceX |
| `target` | service://ServiceX in scope project-x |
| `input` | Named operand: incident → issue://PERF-142 |
| `context` | context://project-x/service-x, captured revision 7 |
| `state` | observed-state://service-x-7, with CG-06 exact typed p95 value 430 ms |
| `desired_state` | desired-state://service-x-latency, with CG-06 condition p95 <= 300 ms |
| `observation` | observation://p95-run-7, retaining its provenance |
| `history` | action://database-query-optimization, recorded as completed |
| `constraint` | Typed restriction prohibiting repetition of that completed action |
| `requires` | Canonical capability contract performance.analysis |
| `evidence` | evidence://latency-report-7, supporting the current latency claim |
| `assumption` | Explicit unverified premise that workload remains comparable; cannot stand in for a mandatory measured workload input |
| `confidence` | CG-06 Unknown attached to that premise |
| `policy` | Reference to canonical policy performance-diagnostics |
| `output` | schema://PerformanceAnalysis with causes, evidence links and recommendations |
| `verification` | Schema validity and support for each reported causal claim |
| `process` | Optional pinned CG-04 diagnostic definition ID/version/digest |

The state and desired-state contracts use the same explicitly declared unit;
CGSL does not introduce unit conversion. Omitting `process` remains legitimate
task meaning. A later CG-08/CG-10 mapping may reject an unsupported executable
shape; it MUST NOT fabricate a workflow to make it executable.

### Incomplete interpretation

For “The service is still too slow” when focus includes ServiceX and ServiceY,
`ambiguity` identifies `target` and retains both scoped candidates. If no p95
threshold is available, `knowledge_gap` identifies the missing desired-state
threshold. This candidate is inspectable, but cannot be executable while those
mandatory semantics remain unresolved. A higher confidence score cannot remove
either diagnostic. A clarification response re-enters semantic validation and
does not authorize a performance-changing mutation.

### Rejected formal-core meanings

| Input attempt | Required disposition |
| --- | --- |
| `objective` or `constraints` used as construct names | Reject aliases; report the canonical construct. |
| `state` used to set a Process Instance to IMPLEMENT | Reject ownership violation; lifecycle state remains CG-04-owned. |
| `process` containing Given/When/Then transitions | Reject inline process semantics; use a CG-04 definition reference. |
| `requires` containing a concrete Agent or tool provider | Reject concrete binding; CG-08 owns selection. |
| `policy` containing a caller-supplied ALLOW or consent | Reject authority escalation; CG-09/host owns authority. |
| `output` containing temperature or provider message roles | Reject provider configuration in semantic task meaning. |
| Model-generated hypothesis supplied as verified `evidence` | Reject epistemic promotion; retain source/provenance and unverified status. |

## Architectural review and acceptance trace

Reviewed on 2026-10-11 by the implementing agent against ADR-015, the existing
CG-06/07/08/09/10 contracts and EPIC-05, including its 2026-10-04 integration
review and 2026-10-06 context-engineering extension. This is a documentation
review, not independent reviewer approval or runtime conformance evidence.

| #179 acceptance criterion | Specification evidence |
| --- | --- |
| Exactly one normative meaning per construct | Exhaustive 20-row vocabulary and explicit distinction rules |
| No Process IR / Strict Cognitive Gherkin duplication | Scope exclusions, CG-04 boundary row and rejected lifecycle examples |
| Provider prompting and tuning excluded | Scope, Prompt IR/provider boundary rows and rejected configuration example |
| Existing CG-06/07/08 contracts referenced rather than redefined | Linked domain contracts and delegated validation in both tables |
| Finite, versionable vocabulary suitable for compiler work | Exhaustive names, operand categories and version discipline |
| Ambiguous terms and aliases rejected | Case-sensitive canonical naming, explicit alias rejection and separate interpretation frontend |
| Reviewed against EPIC-05 boundaries | Review above, including context/source/example planning and host-owned interaction/consent |

Follow-on slices #180–#185 define detailed IR, interpretation, epistemic,
output and grammar contracts; #186 implements the compiler; #190–#193 prove
integration and conformance. This specification does not claim their delivery.
