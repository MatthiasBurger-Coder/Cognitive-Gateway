# Ambiguity, conflict and knowledge-gap contracts

This is the normative EPIC-05.04 [#182](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/182)
contract, based on [ADR-015](adr/ADR-015-semantic-task-ir-boundary.md),
[SemanticTaskIR v1](semantic-task-ir-v1.md) and
[Interpretation Context 1.0](interpretation-context.md). Their MUST/MUST NOT
conventions and source precedence remain authoritative.

The Rust domain contracts in `gateway-domain::semantic_resolution` are
implemented. They do not implement acquisition, reference lookup, context
storage, clarification interaction or compilation. #186/#187 own production
compilation/reference resolution, #188 owns clarification plans and #190 owns
handoff. These are immutable interpretation records outside SemanticTaskIR;
no new executable IR fields or canonical task bytes are introduced. The finite
state/reason/requirement/route enums have strict Serde names; aggregate wire
admission, snapshot canonicalization and diagnostic transport remain #187 work.

## Resolution contract

`SemanticResolution<T>` uses a concrete domain type for each queried field.
References MUST retain their existing typed `ResolvedReference<I>` envelopes;
targets use `SemanticTarget`, literals use the existing validated CG-06 types.
Generic construction does not validate T's contract or prove reference existence.

| State | Payload | Meaning and mandatory-field behavior |
| --- | --- | --- |
| `RESOLVED` | One `ResolutionCandidate<T>` | Unique eligible meaning with original provenance; may supply the field after source/reference validation. |
| `UNRESOLVED` | `ResolutionFailure` | Required meaning absent, unknown, stale implicit context or insufficient confidence; blocks. |
| `AMBIGUOUS` | `ResolutionCandidates<T>` | At least two possible meanings, with no unique referent; blocks. |
| `CONFLICTING` | `ResolutionCandidates<T>` | At least two incompatible assertions about the same field; blocks. |
| `INVALID` | `ResolutionFailure` | Invalid explicit reference or unsupported intent; blocks and requires repair/unsupported handling. |

A candidate MUST have nonempty original CG-06 `Provenance`. Lineage is sorted
by `ProvenanceId`; duplicate identities are rejected, including different
content under one identity. Candidate sets reject fewer than two meanings and
exact duplicate values. Equivalent assertions MUST be grouped into one value
with combined original lineage before construction. Different reference
revisions/digests remain different meanings. Candidate iteration order is
inspectable but MUST NOT imply preference; confidence, timestamp or lexical ID
MUST NOT select a winner. This slice defines no canonical candidate wire bytes.

Ambiguity means “A or B might be intended”; conflict means “A and B are both
asserted and incompatible.” Domain structure cannot establish incompatibility:
the application classifies assertions using the Interpretation Context conflict
matrix. Two explicit targets cannot become mere ambiguity to evade that matrix.
A single low-confidence model proposal remains unresolved. Confidence neither
proves a fact nor supplies authorization.

`ResolutionFailure` rejects ambiguity/conflict reasons because they need
inspectable candidates. `SemanticResolution::validate` checks failure/state
agreement; `require_resolved` validates it again and exposes a value only for
RESOLVED. Directly constructed malformed enum combinations fail closed through
this accessor. Consumers MUST validate before interpreting a resolution record.
There is no conversion of these records to executable IR and no default-value
operation. Every mandatory field MUST pass `require_resolved` and its own type,
source, scope and reference checks before construction of SemanticTaskIR.
The existing IR independently rejects resolution payloads in task operands.

## Knowledge gaps and compilation

| `KnowledgeGap` member | Contract |
| --- | --- |
| `subject` | `KnowledgeGapSubject`: resolved `SemanticTarget`, CG-06 `SubjectPath`, or host-bound request `ReferenceId` when no subject is resolved. A request attachment MUST NOT fabricate a target. |
| `missing_field` | Validated CG-06 `SubjectPath` naming the missing semantic field, such as `target` or `performance.p95_target`. |
| `reason` | Finite `SemanticGapReason`; never unconstrained prose as a machine category. |
| `provenance` | Nonempty original CG-06 provenance of the input/assertion or inspected source exposing the gap. Absence is not evidence of an invented fact. |
| `required_resolution_class` | One requested route: `CURRENT_STATE`, `ORGANISATIONAL_MEMORY`, `PERMITTED_RETRIEVAL`, `HUMAN_CLARIFICATION`, `INPUT_REPAIR` or `UNSUPPORTED`. |
| `requirement` | `MANDATORY` or `ADVISORY`, derived from declared task/output/verification dependencies. |

A mandatory gap always blocks compilation (`blocks_compilation` is true).
Structural optionality in SemanticTaskIR does not establish advisory status:
current state, observations or an input may be mandatory for a particular task.
An advisory gap is allowed only for an optional, omitted enrichment on which
no required meaning, output claim or verification predicate depends. It remains
in the interpretation diagnostic sidecar; it MUST NOT be represented as an
invented value or an assumption satisfying the missing fact. Optional ambiguity
and conflict may remain advisory only when the entire affected field is omitted
and independent mandatory semantics are complete. Scope/admission, policy and
invalid explicit mandatory declarations cannot be downgraded to advisory.

Resolution routes describe what could resolve a gap; they are not source-access
permission, consent, retry instructions or a second knowledge coordinator.
The application MUST use the existing finite permitted-source list and bounded
retrieval/memory contracts. A clarification answer cannot grant policy authority.
If a route fails, preserve the gap; never widen source permissions or insert a
fallback value. A later reply/source update creates a fresh capture and must be
revalidated; the old resolution and provenance remain immutable.

CG-06 provenance is reused without changing its trust/lifecycle semantics.
The application MUST bind provenance IDs and source references to exact records
in the admitted Interpretation Context basis, preserve input spans where
applicable, validate scope/session, freshness, kind, revision and digest, and
apply host disclosure rules before exposing candidates. Source-aware structure
alone does not authenticate lineage. Foreign results must be sanitized and
rejected before these candidate records are exposed. #183 supplies detailed
epistemic attachments; this slice does not promote assumptions into facts.

## Stable diagnostics

Codes below are case-sensitive machine identifiers. Existing codes MUST NOT
be renamed or reused for different meanings; additions require a reviewed
contract extension. Human messages may vary and MUST NOT be parsed for status.

| Reason | Code | State |
| --- | --- | --- |
| `MISSING_REQUIRED_FIELD` | `SEM_MISSING_REQUIRED_FIELD` | UNRESOLVED |
| `UNKNOWN_ENTITY` | `SEM_UNKNOWN_ENTITY` | UNRESOLVED |
| `AMBIGUOUS_ENTITY` | `SEM_AMBIGUOUS_ENTITY` | AMBIGUOUS |
| `AMBIGUOUS_REFERENCE` | `SEM_AMBIGUOUS_REFERENCE` | AMBIGUOUS |
| `CONFLICTING_CONTEXT` | `SEM_CONFLICTING_CONTEXT` | CONFLICTING |
| `STALE_REFERENCE` | `SEM_STALE_REFERENCE` | UNRESOLVED |
| `UNSUPPORTED_INTENT` | `SEM_UNSUPPORTED_INTENT` | INVALID |
| `INSUFFICIENT_CONFIDENCE` | `SEM_INSUFFICIENT_CONFIDENCE` | UNRESOLVED |
| `INVALID_REFERENCE` | `SEM_INVALID_REFERENCE` | INVALID |
| Malformed failure/state combination | `SEM_INVALID_RESOLUTION_CONTRACT` | Reject contract |

`require_resolved` reports generic ambiguity as `SEM_AMBIGUOUS_REFERENCE`;
entity-specific diagnostics use `SemanticGapReason::AmbiguousEntity` on the
associated gap. Detailed diagnostics MUST retain request/context revision,
queried field, status, selected candidate if any, candidates, provenance,
considered tiers and exclusions as required by Interpretation Context. The
application owns this envelope and its wire encoding (#187).

A stale implicit focus pin is excluded with `STALE_REFERENCE`; if no other
eligible meaning exists, the field remains UNRESOLVED. An explicitly required
stale pin is INVALID with `INVALID_REFERENCE`, with the stale exclusion reason
retained in the detailed diagnostic. It MUST NOT fall back to a different pin.

## Worked examples

These tables describe interpretation records, not executable task JSON. Source
names stand for the full pinned input/context and CG-06 provenance records.

| Input/basis | Typed result and lineage | Gap/route | Compilation |
| --- | --- | --- | --- |
| “Analyze the service”; no target/focus supplied in request-182/input-r1 | `UNRESOLVED(MISSING_REQUIRED_FIELD)` attached to request-182, field `target`; provenance input-r1 | Mandatory / HUMAN_CLARIFICATION | Block; do not create a service ID. |
| “Analyze Payments”; exact scoped alias lookup yields service-A@r3 and service-B@r7 | `AMBIGUOUS`: two `SemanticTarget(Service)` values with separate full bindings and input/alias-lookup lineage | `AMBIGUOUS_ENTITY`, mandatory / HUMAN_CLARIFICATION | Block; focus service-A cannot guess which explicit label was meant. |
| “Analyze it”; implicit focus service-A@r2 has expired, with no eligible alternative | `UNRESOLVED(STALE_REFERENCE)`; preserve focus-turn and expiry evidence | Mandatory / CURRENT_STATE, if already permitted; otherwise HUMAN_CLARIFICATION | Block until a fresh eligible binding exists. |
| “Analyze service-A at revision r2”; explicitly required pin is stale | `INVALID(INVALID_REFERENCE)` with stale exclusion and input-r1 lineage | Mandatory / INPUT_REPAIR | Block; latest r3 cannot silently replace r2. |
| Same winning-tier task field explicitly asserts service-A and service-B with no correction relation | `CONFLICTING`: both typed targets and both declaration sources | `CONFLICTING_CONTEXT`, mandatory / HUMAN_CLARIFICATION | Block; confidence and list order cannot choose. |
| Resolved service-A, but its required p95 goal value is absent | Gap subject `Target(service-A)`, field `performance.p95_target`, `MISSING_REQUIRED_FIELD`, original goal input lineage | Mandatory / HUMAN_CLARIFICATION | Block; no numerical default or assumption. |
| Resolved inspect task; optional historical comparison is stale and no required output depends on it | Gap subject `Target(service-A)`, field `history.comparison`, `STALE_REFERENCE` and inspected-source lineage | Advisory / PERMITTED_RETRIEVAL | May compile without comparison; keep diagnostic outside IR. |

## Verification and completion boundary

`crates/gateway-domain/tests/semantic_resolution.rs` covers the complete state
and reason inventory, frozen codes, lineage/candidate invariants, malformed
state rejection, mandatory/advisory gaps and rejection of interpretation
payloads in executable IR fields. Existing `semantic_task_v1` fixtures preserve
canonical task bytes and reference-validation checks. The semantic coverage
gate now requires at least 95% independently for both production modules.

```bash
cargo test -p gateway-domain --test semantic_resolution --test semantic_task_v1 --locked
cargo llvm-cov -p gateway-domain --all-targets --fail-under-lines 95 --json --output-path target/semantic-domain-coverage.json
python3 tests/scripts/check-semantic-task-coverage.py --self-test
python3 tests/scripts/check-semantic-task-coverage.py target/semantic-domain-coverage.json
```

This establishes domain contract behavior and existing IR admission, not an
installed natural-language compiler or production resolution/handoff proof.
