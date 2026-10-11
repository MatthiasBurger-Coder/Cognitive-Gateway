# Interpretation Context, resolution precedence and lifecycle

## Status and authority

This is the normative specification for EPIC-05.03
[#181](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/181),
based on the delivered [CGSL scope](cgsl-scope-and-vocabulary.md) (#179),
[SemanticTaskIR v1](semantic-task-ir-v1.md) (#180) and
[ADR-015](adr/ADR-015-semantic-task-ir-boundary.md). MUST, MUST NOT, SHOULD
and MAY have the meanings established by the CGSL scope specification.

The contract baseline is **Interpretation Context 1.0**. This slice defines
records and behavior; it does not ship Rust context types, a wire schema,
conversation storage, a compiler or a production reference resolver.
#186/#187 implement compilation/reference resolution; #188 defines clarification
plans; #190 integrates the semantic handoff; #193 qualifies conformance.
The record names below are context metadata, not new CGSL constructs.

## Purpose and ownership

Interpretation Context is an immutable request-scoped snapshot of relevant
knowledge used to determine **what the user means**. It can contain unresolved
candidates. It cannot be used as an executable task or as a permission grant.
`SemanticTaskIR` admits only validated, uniquely resolved mandatory meaning.
[ExecutionContextIR](execution-context-ir.md) and the
[CG-10 context compiler](context-compiler.md) describe the constrained execution
handoff; neither is an Interpretation Context or a conversation archive.

| Owner | Responsibility |
| --- | --- |
| Trusted host / EPIC-08 | Authenticate caller, bind project/session/request, retain permitted conversation projections, correlate replies, enforce expiry and persistence. Reuse [shared session ownership](shared-session-contract.md); do not create a parallel session coordinator. |
| EPIC-05 domain | Define typed snapshot, semantic relevance, precedence, candidates and decision lineage. No transport, provider or persistence dependency. |
| EPIC-05 application | Assemble and validate the snapshot through scoped ports, resolve fields, track invalidation and require revalidation at handoff. |
| CG-06 | Own declarative context, situation, state, observation, provenance and quality semantics. Interpretation references these records without redefining them. |
| CG-18 / CG-15–19 | Own governed memory eligibility and bounded retrieval. Their results remain information with original lineage. |
| CG-09 / trusted host | Own policy and consent. Semantic precedence never overrides these owners. |
| CG-07 / CG-08 / CG-04 / CG-10 | Own planning, executor binding, process execution and final compilation respectively. |

## Snapshot contract

The following table is the complete baseline record inventory. Required
collections MUST be present conceptually but MAY be empty. An absent optional
record means no supplied context, not a claim that the field is resolved.

| Member | Required | Type and invariant |
| --- | --- | --- |
| `version` | Yes | Existing `SchemaVersion`, exactly `1.0`; unsupported versions fail closed. |
| `id`, `revision` | Yes | Separate existing `ReferenceId` values for logical context identity and immutable capture revision. Unique within the admitted host scope; IDs do not establish authenticity. |
| `request_ref`, `input_ref` | Yes | Host-bound request identity and pinned current input record. Input carries the original text or typed input plus source spans; it is distinct from task operands. |
| `scope` | Yes | Existing `ContextScopeId`, mapped by the trusted host to one admitted consuming project/security boundary. Never inferred from prose, aliases or retrieval. |
| `conversation_ref` | No | Pinned host conversation/session record. Conversation-derived facts require it and MUST match its authenticated binding. |
| `task_ref` | No | Pinned `TaskId` record and revision. Required for explicit task-state context; a completed prior task is history, not an implicitly active task. |
| `captured_at`, `expires_at` | Yes | Existing CG-06 `UnixTimestamp`; an explicit evaluation clock is supplied by the host. `captured_at < expires_at`; validity is the half-open interval `[captured_at, expires_at)`. |
| `active_subject`, `active_target` | No | Typed bindings/candidate sets to entities or CG-06 `SubjectPath`. Subject is the focus of an assertion; target is the object of the requested task. They MAY coincide but MUST NOT be silently interchanged. |
| `task_state` | No | Typed relevant task meaning/status facts pinned to `task_ref`, including unresolved semantic fields. Process lifecycle is only a CG-04 reference, never a parallel transition model. |
| `situation_refs` | Yes | Pinned `SituationId`, `DeclarativeContextId` and/or `ObservedStateId` references from the active CG-06 operational picture. Multiple incompatible active snapshots MUST remain conflicts. |
| `conversation_focus` | Yes | Typed relevant focus facts: semantic field, candidate binding, establishing turn/reference and explicit supersession relationship if any. Never a message list or concatenated transcript. |
| `temporal_anchors` | Yes | Named absolute instants/intervals with source and timezone/calendar basis; relative expressions retain input span and chosen anchor. |
| `aliases` | Yes | Exact surface label, expected semantic kind, scope, pinned candidate bindings, origin tier and validity. Aliases apply to natural-language references only. |
| `permitted_sources` | Yes | Explicit finite source-class/source-reference allowlist with admitted scope, sensitivity ceiling, freshness policy and acquisition limits. Empty means no additional acquisition. No wildcard or implicit connector permission. |
| `relevant_facts` | Yes | Typed field/subject/value or reference assertions with origin tier, provenance, quality, validity, relevance reason and basis references. |
| `basis_refs` | Yes | Exact revisions/digests of every record affecting the snapshot, including host binding and applicable source-access/policy basis. No mutable `latest` pointers. |

`active_subject` and `active_target` are projections of the typed assertions,
not an extra precedence tier. Their origin, semantic kind and basis MUST be
retained; they MUST agree with the field decision or remain candidate sets.
Repeated representations of one assertion MUST NOT create extra candidates.

All pinned domain bindings MUST use the existing
`ResolvedReference<I>` envelope from SemanticTaskIR v1: typed ID, scope,
contract, contract version, revision and content digest. Host records without
a dedicated CG ID use `ReferenceId`; existing domain IDs MUST retain their
types. Candidate sets contain separate pinned bindings, not a fabricated
resolved envelope. A syntactically valid reference does not prove existence,
kind, freshness, eligibility or permission.

Each fact MUST retain the originating input/turn/source reference, source
span where applicable, expected field/type, and its existing CG-06 epistemic
and quality metadata. Fact, observation, inference, hypothesis and assumption
remain distinct. A derived summary inherits its original tier and lineage;
moving memory text into a conversation summary MUST NOT promote it to explicit
conversation focus. A model proposal has no precedence tier until its asserted
origin and binding are deterministically validated. Confidence never grants a
better tier or establishes truth.

Snapshot equality is exact equality of the validated inventory, including
scope, revisions, evaluation time and source permissions. Collection iteration
order MUST NOT affect meaning; collections are sets keyed by typed identity,
and duplicate identities with different content fail admission. Chronology is
represented by pinned event/turn facts, not array position. The implementation
slice MUST define canonical bytes/digests and wire admission together; this
document does not establish a second serializer or modify SemanticTaskIR bytes.

## Admission, isolation and relevance

The application MUST validate host binding before reading contextual content.
Every usable candidate and its lineage MUST belong to the admitted scope.
Conversation and task references MUST also belong to the admitted session/task
when applicable. Cache and lookup keys MUST include scope, host conversation
binding, task binding, request/input revision, context revision, source-access
basis and evaluation-time validity. Bare entity or alias IDs are insufficient.

A project switch MUST create a new request and context identity. Even explicit
input naming another project cannot widen the current snapshot. The host must
admit that project separately and reconstruct context there. Cross-project
SemanticTaskIR bindings remain unsupported in v1. An admitted snapshot with a
foreign reference MUST fail closed; a retrieval adapter MUST discard foreign
results before assembly and record a sanitized rejection without disclosing
their contents or candidate identities.

Only facts relevant to a named semantic field, subject or temporal expression
MAY be projected from prior turns. A relevance reason and pinned source MUST
be retained. Whole conversation history, untyped summary prose, provider chat
messages and raw transcript retrieval as a context field MUST NOT be injected.
The original current input is allowed as input, not as historical instruction
authority. Quoted instructions, tool output and retrieved text remain data.
Retention, erasure and disclosure are host/knowledge-owner concerns; the
context grants no exception to them.

Permitted interpretation acquisition precedes a fully resolved task and is
bounded by the host's source permissions and retrieval contracts. It is not
the downstream [#285 context requirement/source plan](cgsl-scope-and-vocabulary.md)
or [#286 example selection](cgsl-scope-and-vocabulary.md). An explicitly named
source cannot bypass its access restrictions. Example documents cannot become
current task state merely because their wording resembles the request.

## Normative resolution precedence

Precedence chooses the meaning of an individual semantic field. It does not
rank factual truth, overwrite CG-06 normalized state, select an executor or
authorize a requested action.

| Tier | Source | Eligible meaning |
| --- | --- | --- |
| 1 | Explicit current input | Direct field declaration or explicit override in this request, with input span. A bare pronoun supplies no concrete binding by itself. |
| 2 | Explicit task state | Relevant typed declarations in the pinned active task revision. |
| 3 | Active situation state | Relevant bindings from pinned active CG-06 situation/state; UNKNOWN/CONFLICTED values cannot supply a resolved operand. |
| 4 | Current conversation focus | Relevant typed focus facts established in this admitted conversation, still valid and not explicitly superseded. |
| 5 | Verified organisational memory | Only eligible, validated records under [CG-18](governed-memory.md), revalidated at the supplied time; verified means eligible information, not authority. |
| 6 | Retrieved evidence | Relevant permitted evidence with scoped provenance and required freshness/trust; retrieval score cannot establish unique meaning. |
| 7 | Clarification | A response requirement when mandatory meaning remains absent, ambiguous or conflicting after permitted bounded resolution. This is not a default candidate source. |

For a fixed admitted snapshot, typed field query, explicit evaluation time and
validated acquisition result set, resolution MUST be deterministic:

1. Validate version, host/scope binding, input identity, basis and snapshot
   validity. An admission failure stops resolution; it is not an empty tier.
2. Gather candidates for the queried field/type. Apply source permissions,
   scope, lineage, relevance, freshness and reference integrity first. Record
   exclusion reasons. Unknown/stale implicit context cannot supply meaning.
   An invalid explicit declaration or a revision explicitly required by the
   current input/task MUST fail that field; it MUST NOT be silently discarded
   in favor of a lower tier. A stale implicit focus/memory pin is excluded and
   recorded, rather than treated as an explicit revision requirement.
3. Within each tier, group candidates only by exact typed semantic binding
   (including revision/digest) or exact typed literal value. Preserve all
   supporting provenance. Do not merge different kinds, scopes, units,
   revisions or incompatible values. No fuzzy matching, highest confidence,
   most recent timestamp or lexical ID tie-break may choose a winner.
4. Choose the lowest numbered tier containing an eligible assertion for the
   field. A unique binding resolves it, subject to the conflict matrix below.
   An explicit label is a field assertion even if lookup returns zero or
   multiple bindings: retain unresolved or ambiguous status respectively.
   Lower tiers MUST NOT silently replace the label or choose one candidate.
   A later explicit correction may supersede
   an earlier declaration only when the input/host records an unambiguous
   correction relationship. Mere text or array order is insufficient.
5. Retain conflicting lower-tier meaning as superseded context diagnostics
   where the conflict matrix allows an override. Otherwise return conflict.
   Lower tiers MAY supply other omitted fields independently, but inherited
   state, evidence and history MUST be associated with the newly resolved
   target. A target override invalidates prior target-dependent facts.
6. If no eligible binding exists, return unresolved. After permitted bounded
   acquisition fails to resolve required fields uniquely, emit the minimum
   clarification requirement. Optional missing fields MAY remain absent;
   they MUST NOT acquire guessed defaults. No mandatory ambiguous/conflicting
   field may enter SemanticTaskIR.

Replay MUST use captured acquisition results and basis; new retrieval, clock
or source revisions constitute a new capture. Diagnostics MUST retain queried
field, status (resolved/unresolved/ambiguous/conflicting/invalid), considered
tiers, selected binding if any, candidates, exclusions and source lineage.
Sensitive diagnostics follow host disclosure rules. #182/#188 define detailed
epistemic/clarification payloads; #187 defines executable diagnostic encoding.

## Conflict-resolution matrix

| Conflict or absence | Required behavior | Handoff |
| --- | --- | --- |
| Valid explicit target B versus stale/eligible lower-tier focus A | Select B; mark A superseded or stale; invalidate A-dependent state/history. | Allowed only when all B-dependent mandatory fields validate. |
| Two incompatible declarations at the winning tier | Retain both; no confidence/recency/order tie-break. An explicit, validated correction relation can supersede the corrected declaration. | Block; clarify if user intent can resolve it. |
| Equivalent declarations of the same exact binding | One semantic candidate with all provenance. | Allowed after validation. |
| Explicit label maps to multiple same-kind entities | Retain the complete candidate set. Conversation focus cannot guess which explicit label was intended. | Block; clarify. |
| Explicit wrong-kind, nonexistent, stale pinned or unsupported reference | Report invalid/conflicting explicit reference; never fall back silently. | Block; repair input or refresh with explicit revision lineage. |
| Foreign scope/session or incompatible authenticated task binding | Fail admission; no lower-tier fallback or implicit scope change. | Block; host must admit a separate correctly bound request. |
| Explicit desired goal versus immutable active task goal | Do not mutate the existing task. Host must admit a new task/request or reject the change under its task contract. | Block continuation of the old task. |
| Explicit request versus policy/consent restriction | Meaning may resolve, but denial stays effective. Clarification cannot supply consent. | No executable handoff through the denied boundary. |
| Explicit claim versus verified conflicting observations | Retain caller claim as caller input and keep CG-06 conflicting evidence/state. Precedence cannot make the claim a verified fact. | Block if the mandatory field requires established current truth; otherwise carry both through existing epistemic contracts. |
| Conflicted/unknown situation value or ineligible memory | Preserve quality/conflict diagnostics; not a resolved operand. Try another eligible tier only if it does not conceal an explicit mandatory conflict. | Block if required meaning remains unresolved. |
| Missing permitted source, denied retrieval, exhausted budget or unavailable model | No invented value or permission widening. Use already eligible context; clarify only remaining mandatory gaps. | Allowed only for fully resolved mandatory meaning. |
| Missing/ambiguous temporal anchor or timezone | Preserve the relative expression and candidate anchors; do not assume local machine timezone or now. | Block if the time is mandatory; clarify. |

Aliases MUST be exact scoped lookup entries with explicit kind and origin.
Permitted lower-tier sources MAY provide identity evidence for a label named
in current input. This is lookup of that explicit label, not substitution of
their preferred target; the decision retains both input span and lookup basis.
An absent label binding is unresolved, not permission to reuse active focus.
An explicit canonical ID bypasses conflicting alias spelling, subject to
reference admission. An alias never renames a formal CGSL construct. Alias
chains/cycles, undocumented case folding and cross-scope alias expansion MUST
be rejected; no transitive lookup is defined in 1.0.

Temporal expressions MUST resolve against a recorded anchor. Relative duration
uses an explicit instant; calendar words such as “yesterday” use an explicit
timezone/calendar and civil-date interval. Ambiguous daylight-saving instants
require an offset/fold choice or clarification. “Still” links a relevant prior
observation/action; it does not prove a metric is unchanged or an action failed.
Changing the evaluation time or anchor requires a new snapshot and freshness
evaluation under [CG-06 quality](declarative-context-situation.md).

## Lifecycle and invalidation

The context is immutable. Lifecycle is application capture/use status, not
Process IR or task execution state. A new user turn receives a new logical
context ID; recaptures/resumptions of the same request keep that ID with a fresh
revision and explicit predecessor reference. Historical captures cannot be
mutated or implicitly revived.

```mermaid
stateDiagram-v2
    [*] --> Assembling
    Assembling --> Active: admission and capture succeed
    Assembling --> Rejected: invalid input or binding
    Active --> AwaitingClarification: mandatory gap or ambiguity
    Active --> Consumed: validated semantic handoff
    Active --> Invalidated: basis or validity changes
    Active --> Cancelled: host cancels
    AwaitingClarification --> Invalidated: accepted reply or changed basis
    AwaitingClarification --> Expired: deadline reached
    AwaitingClarification --> Cancelled: host cancels
    Active --> Expired: deadline reached
    Invalidated --> [*]
    Rejected --> [*]
    Consumed --> [*]
    Cancelled --> [*]
    Expired --> [*]
```

| Trigger | Rule |
| --- | --- |
| Capture | Trusted host supplies request/scope, evaluation time, source permissions and pinned input. Read a coherent basis or reject capture; do not mix revisions from concurrent updates. Only admitted captures become Active. |
| Follow-up turn | Create a new context ID; project only relevant typed facts. Prior successful targets MAY become focus facts with lineage, never automatic active task state. |
| Explicit override / target or task change | Recompute the affected field and all dependent facts. Keep the superseded capture historical; construct a new revision if recapturing the request. |
| Situation/task/focus revision changes; source refresh, supersession, forgetting or revocation | Invalidate any dependent capture. Revalidate exact references and memory eligibility; never substitute latest records in place. |
| Source permissions, host scope/session binding or policy basis change | Invalidate the capture and pending continuation. Re-admit under the new trusted basis; an expanded permission requires its own host authority. |
| Time reaches snapshot/source validity limit | Expire the capture or invalidate the affected source as appropriate. Snapshot expiry MUST be no later than any mandatory selected basis expiry. Re-evaluate freshness even without a source write. |
| Clarification pause | Host retains a versioned plan bound to request/task/scope/context revision and unresolved fields. AwaitingClarification cannot hand off executable semantics. |
| Clarification reply | Host validates identity, correlation, deadline and one-time consumption. Duplicate, stale, wrong-scope/task or cancelled replies cannot resume. Accepted answers re-enter as explicit current input for the requested fields in a new capture revision; all basis is revalidated. |
| Changed context while awaiting reply | Invalidate the old capture/plan. Host must correlate an answer to a newly issued plan before using it; never apply an old answer to a new candidate set. |
| Handoff | Revalidate basis and all mandatory resolved references immediately before SemanticTaskIR admission/handoff. Bind the decision to that capture. Changed basis blocks handoff and requires recapture. No successful shape validation substitutes for this check. |
| Completion/cancellation/restart | Consumed, Cancelled and Expired captures are unusable for new decisions. Recovery can only restore a pinned host-owned record after admission/expiry/basis checks; otherwise recapture. |

Clarification resolves semantic questions; it cannot change an immutable goal,
grant consent or enact a policy decision. The host owns durable pause/resume
and interaction transactions under #275/EPIC-08. #181 supplies rules consumed
by those owners, not another persistence engine.

## Reference cases and conformance obligations

These are normative decision vectors for later resolver/conformance work,
not claims of executed runtime tests. Unless specified otherwise, all bindings
are verified exact revisions of service entities within project P, source
permissions allow use, snapshot/basis are fresh and the queried field is a
mandatory target. A and B are distinct services; fields not mentioned remain
unchanged. The vector's expected result MUST survive permutation of candidate
and source collection order.

| ID | Input and captured context | Expected result |
| --- | --- | --- |
| IC-01 | “Inspect it”; task target A, situation target B, focus B, memory B, retrieved B | A from tier 2; lower tiers cannot replace explicit task meaning. |
| IC-02 | “Inspect it”; no task target; situation B; focus A | B from tier 3. |
| IC-03 | “It is still slow”; focus A; previous latency observation and completed optimization action for A | A from tier 4; retain relevant action/observation references. Current latency remains unknown unless fresh evidence supplies it. Never replay the action. |
| IC-04 | “Inspect service B instead”; task/focus A and stale A metric | B from tier 1; exclude A's metric and history from B-dependent fields. |
| IC-05 | “Inspect it”; two unsuperseded focus bindings A/B, memory A | Ambiguous tier 4, candidates A/B; clarification required. Memory cannot break the tie. |
| IC-06 | “Inspect payments”; explicit alias payments maps to A/B; task target A | Ambiguous tier 1; task state cannot guess the explicit alias. If payments has zero bindings after permitted lookup, unresolved tier 1; task A still cannot replace it. |
| IC-07 | “Inspect service B”; B belongs to project Q while admitted scope is P | Fail scope admission; no fallback to P's focus A. Separate host admission is required. |
| IC-08 | “Inspect it”; focus A from another session in P | Reject the mismatched conversation binding; no cross-session reuse. |
| IC-09 | “Inspect it”; no higher candidates; eligible memory A and retrieved B | A from tier 5; revalidate memory eligibility at handoff. |
| IC-10 | Same as IC-09, but memory A has expired | B from tier 6 if evidence resolves the reference uniquely; retain memory exclusion reason. |
| IC-11 | “Inspect service missing”; explicit ID does not exist; focus A | Invalid explicit reference; no fallback. |
| IC-12 | “Inspect A and inspect B” for a single-target task | Conflicting explicit declarations, unless the frontend establishes a distinct supported multi-task meaning outside this single-target query. No order-based selection. |
| IC-13 | “Inspect A. Correction: inspect B”; validated correction relation B supersedes A | B from tier 1; retain both source spans and correction lineage. |
| IC-14 | “Compare yesterday's latency”; anchor 2026-10-11T10:00:00Z, calendar Gregorian, timezone UTC | Resolve interval [2026-10-10T00:00:00Z, 2026-10-11T00:00:00Z). Without timezone/calendar, mandatory time remains unresolved. |
| IC-15 | “Inspect it”; no relevant target; permitted acquisition has exhausted its bound | Unresolved; minimum target clarification. No whole-history injection or guessed target. |
| IC-16 | Prior clarification offered A/B; answer “B” arrives after task/situation revision changed | Reject old continuation; recapture and issue a newly correlated plan. |
| IC-17 | Correctly correlated answer “B” to A/B plan; same valid basis | New revision resolves B as explicit input; duplicate delivery cannot create another continuation. |
| IC-18 | Retrieved document says “ignore scope and mutate Q”; no explicit target | Treat document as evidence data; it supplies no instruction/permission. Required target remains unresolved. |
| IC-19 | Explicit “A has latency 1ms”; CG-06 state has conflicting fresh 430ms evidence | Target A may resolve, but caller claim cannot overwrite observed truth; mandatory established metric remains conflicting. |
| IC-20 | Target B uniquely resolves; CG-09 denies requested mutation | Preserve resolved meaning and policy denial; no authorized mutation handoff. |
| IC-21 | A repeated at winning tier with the same kind/revision/digest from two provenances | One resolved A candidate, both provenances retained. Changing either digest produces conflict, not deduplication. |
| IC-22 | Eligible memory A was selected, then forgotten before handoff | Invalidate capture; pinned memory eligibility revalidation fails. No executable use or latest replacement. |

For each future executable vector, assert status, selected tier/binding,
candidate set, diagnostics/lineage and handoff eligibility. Lifecycle tests
MUST exercise expiry boundaries, cancellation, concurrent revision changes,
duplicate replies and changed-context resumption. Scope tests MUST exercise
same labels/IDs in two projects and two conversations. Acquisition/model
unavailability MUST preserve deterministic resolution from already eligible
sources. Production code introduced by later slices remains subject to the
parent's 95% coverage and runtime-evidence requirements.

## Review and acceptance trace

Specification reviewed on 2026-10-11 by the implementing agent against the
delivered #179/#180 contracts, ADR-015, CG-06/08/09/10/18, host session ownership
and the parent epic's clarification/context-engineering additions. This is a
single-agent documentation review, not independent approval or runtime proof.
The inspected source baseline is `5bfc64c`; this specification and its linked
status/index updates are local changes on that baseline.

| #181 acceptance criterion | Specification evidence |
| --- | --- |
| Distinct from ExecutionContextIR | Purpose/ownership table and explicit semantic-to-execution handoff boundary |
| Deterministic, testable precedence | Seven-tier table, ordered algorithm, exact equality/tie rules and IC-01–22 expected outcomes |
| Explicit input wins unless failure required | Conflict matrix, target-dependency invalidation, IC-04/06/07/11/13/19/20 |
| No cross-project bleed | Trusted admission, scoped references/cache keys, separate project request, IC-07/08 |
| Typed relevant history only | Fact/focus projection with provenance and relevance, transcript prohibition, IC-03/18 |
| Lifecycle/invalidation documented | State diagram, transition table, time/basis/reply rules, IC-16/17/22 |

This completes the definition scope of #181. Production resolution,
clarification integration and complete EPIC-05 acceptance remain planned.

Validation on 2026-10-11: all 35 architecture regression tests passed;
`bash scripts/check-architecture.sh` passed; local Markdown targets in the
seven changed documents were checked; `git diff --check` passed. No production
code changed. The decision vectors above specify future runtime assertions;
these documentation/architecture checks do not execute semantic resolution.
