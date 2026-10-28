# Evidence, assumptions and epistemic semantics

EPIC-05.05 [#183](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/183)
defines the normative interpretation contract implemented in
[`semantic_epistemic.rs`](../crates/gateway-domain/src/semantic_epistemic.rs).
It builds on [SemanticTaskIR 1.0](semantic-task-ir-v1.md),
[interpretation context](interpretation-context.md) and
[resolution contracts](semantic-resolution-contracts.md). MUST and MUST NOT
are normative. These domain attachments do not change the 1.0 task wire format
or introduce CGSL keywords. Compiler, persistence and production reference
adapters remain #186–#190 work; this is not runtime integration evidence.

## Classification and independent metadata

| Category | Structural representation | Meaning |
| --- | --- | --- |
| Fact | `EpistemicClaim::Fact(ResolvedReference<FactId>)` | Accepted structured CG-06 claim, with its observation lineage; a fact ID alone is not verification. |
| Observation | `Observation(ResolvedReference<ObservationId>)` | Directly observed value with source provenance and occurrence time. An observation does not establish a causal explanation. |
| Inference | `Inference(SemanticInference)` | Typed conclusion with one or more distinct evidence bindings. Evidence supports reasoning; the conclusion is not an evidence record. |
| Hypothesis | `Hypothesis(ResolvedReference<ReferenceId>)` | Pinned candidate explanation requiring validation, which may have no supporting evidence yet. |
| Assumption | `Assumption(SemanticAssumption)` | Explicit temporary premise, with ID, typed value, pinned basis and CG-06 confidence. Never verified knowledge. |

There is no conversion from an inference, hypothesis or assumption to a fact
or evidence. New independently validated CG-06 records MUST be created to
establish new knowledge, preserving links to the original proposal. Relabeling
an ID or copying a conclusion into a literal does not satisfy admission.
The trusted reference adapter MUST check the actual source contract and role.

Reuse CG-06 `Provenance`, `QualityMetadata`, `TrustClass`, `Confidence`,
`FreshnessStatus`, `ConflictStatus`, `EvidenceKind` and `EvidenceRelation`.
No trust class grants authority. Verification (`Verified`/`Unverified`) and
retention (`Active`/`Deprecated`/`Archived`) are additional orthogonal semantic
dimensions because CG-06 quality has no verification or retention field.
They MUST come from the trusted source's validation/curation record, never
from a model's assertion. Missing verification maps to `Unverified`; absent
or unsupported retention metadata blocks source admission, with no default
to active. A verified fact may also be stale, conflicting or archived.

“Suspected stale” means freshness is not established: retain the source reason
and use CG-06 `FreshnessStatus::Unknown`, rather than inventing a second
freshness scale. Evaluate freshness with CG-06 `evaluate_freshness`, explicit
evaluation time and the applicable freshness policy. Never use an implicit
clock, reset the source timestamp on retrieval, or erase uncertainty/conflict.

## Evidence and admissibility contract

`SemanticTaskIR.evidence_refs` and inference bases contain pinned
`ResolvedReference<EvidenceId>` values: ID, scope, source contract/version,
revision and SHA-256 digest. Large source payloads MUST remain in the source
store. CGSL and task serialization carry references by default; no inline
evidence-body field is introduced. CG-06 may retain inline summaries or
external `EvidenceContent` references at source.

`SemanticEvidence::from_source` projects a resolved CG-06 evidence record
without its content or summary, preserving exact provenance (including source,
producer, timestamps and parent IDs), evidence kind and support/challenge
links. It rejects mismatched evidence and provenance identities. The host
MUST validate the complete `ObservationEvidenceSet`, including parent
provenance and fact/observation links, and authenticate the pin before this
projection. Identity equality does not authenticate a digest or revision.

Admission proceeds in this order:

1. Authenticate scope/access and resolve every exact contract/version/revision/
   digest binding; missing, substituted, wrong-role or changed bindings fail.
2. Validate the CG-06 lineage set and target association. Retain support AND
   challenge edges. `conflicting_fact_ids` and normalized quality conflict
   cannot be cleared by ranking or choosing the highest confidence source.
3. Load source verification/retention and evaluate freshness. Preserve original
   trust, sensitivity, confidence, uncertainty and provenance.
4. Apply `epistemic_denials` for the intended use. It returns all applicable
   denials in deterministic order, independent of confidence and trust rank.
5. Revalidate references and mandatory semantics immediately before handoff.
   Successful shape or epistemic eligibility checks alone grant no permission.

| Source condition | Current knowledge for a mandatory operand | Inspection/history/advisory analysis |
| --- | --- | --- |
| Verified, active, fresh fact or observation, no conflict | Eligible after all reference, subject and policy checks | Eligible as labeled source data |
| Unverified | `Unverified` denial | Retain as unverified |
| Stale | `Stale` denial | Retain timestamp and stale status |
| Suspected stale / unknown freshness | `FreshnessUnknown` denial | Retain uncertainty and reason |
| Conflicting | `Conflicting` denial | Retain both assertions and all evidence links |
| Deprecated | `Deprecated` denial | Retain replacement reference if available; no silent substitution |
| Archived | `Archived` denial | Historical inspection only under source access rules |
| Inference, hypothesis or assumption, regardless of verification metadata | `NonFactual` denial | Retain original classification |

Inspection eligibility does not permit selecting a mandatory target, filling a
required fact, counting a challenge as support, or satisfying
`ALL_CLAIMS_SUPPORTED`. Conflicting/stale data may itself be the explicit subject
of an inspection. Its captured reference still must be valid: a historical
record is different from a stale or substituted reference binding.

An `EvidenceKind::ModelOutput` can establish that a model emitted text. It does
not establish that text's claims. Retrieved documents and caller statements
likewise retain their source kind. Their existence cannot verify a hypothesis.
Evidence sufficiency and verification execution remain downstream owners;
`EVIDENCE_REQUIRED` declares an obligation, not a successful result.

## Assumption lifecycle

An assumption MUST record its premise, reason/basis, source lineage, task and
scope, captured revision, permitted advisory use, validation obligation and
expiry/invalidation condition in its referenced interpretation record. The
existing task assumption fields remain unchanged; lifecycle records are
external attachments keyed by task revision and assumption ID.

| Transition | Required condition |
| --- | --- |
| Proposed → Active | Explicitly retained for advisory reasoning; basis validated, scope fixed, validation and expiry conditions recorded. This is not consent. |
| Proposed → Withdrawn | Premise not adopted; preserve audit reason. |
| Active → Discharged | Independent evidence satisfies the recorded validation obligation. Link separately admitted knowledge and its validation record; original premise remains an assumption. |
| Active → Refuted | Evidence contradicts the premise; retain challenge and validation record. |
| Active → Expired | Explicit expiry reached, or task/scope/source revision changes invalidate the premise. |
| Active → Withdrawn | Premise abandoned with recorded reason. |

`AssumptionState::transition_to` rejects every other transition, including
reactivation of terminal records and a transition to verified knowledge.
The application owns checking transition conditions and persisting immutable
events with explicit time, actor/source and reason. This domain state machine
checks transition topology only. Reconsideration creates a new assumption ID
and basis. Discharge/refutation/expiry MUST invalidate dependent conclusions
and trigger revalidation; they do not silently rewrite an admitted task.
Neither active nor discharged assumptions can fill mandatory unresolved facts.

## Interpretation confidence

`InterpretationConfidence<T>` attaches the existing bounded CG-06 `Confidence`
to a result and a pinned interpretation basis (method/model/rule revision and
source context). Unknown and not-applicable remain distinct from score zero.
It does not replace source evidence confidence or modify `T`'s validation.
Even confidence 1.0 cannot resolve ambiguity, erase a gap, establish source
authenticity, promote a category or bypass deterministic/policy validation.

## Examples and acceptance evidence

Measured p95=430ms is an observation. “Database contention causes latency” is
an inference only if it has evidence references; without support it remains a
hypothesis. Neither can become a fact by renaming its variant or scoring it
1.0. “Assume the target is 300ms” remains an assumption: a missing mandatory
target still produces `SEM_MISSING_REQUIRED_FIELD` through #182 contracts.
A verified but stale measurement with unresolved challenges produces both
`Stale` and `Conflicting` denials, while remaining inspectable as historical
evidence. A model report retains `ModelOutput`, including when referenced as
evidence of what the model said.

| #183 criterion | Implemented contract / automated evidence |
| --- | --- |
| Categories cannot collapse | Typed `EpistemicClaim`, distinct inference basis, no promotion API; `classification_and_admissibility_never_promote_nonfacts` |
| Provenance retained | CG-06 source projection rejects ID/provenance mismatch; `evidence_projection_preserves_lineage_without_copying_payload` |
| Stale/conflicting distinguishable | Independent CG-06 flags and accumulated denials; classification/admissibility test |
| Confidence cannot bypass checks | `maximum_confidence_cannot_resolve_a_missing_required_fact` |
| Large bodies referenced | Projection has no payload field; source content absent from projection test; unchanged v1 reference wire |
| CG-06 reused | Existing typed metadata, lineage, support/challenge records; no replacement trust or freshness system |
| Assumption lifecycle | Exhaustive transition-pair test `assumption_lifecycle_has_no_promotion_or_reactivation` |

Run `cargo test -p gateway-domain --locked` and the domain/semantic coverage
gate in `tests/scripts/quality-gates.json`. These tests establish domain contracts,
not the future compiler, resolver, lifecycle persistence or runtime path.

Validation on 2026-10-16: domain all-target tests and the 95% coverage gate
passed (97.50% overall; new epistemic module 90/90 lines). The semantic
per-file coverage gate and its self-test passed. Domain all-target Clippy with
warnings denied, formatting, architecture/dependency guards and all 15 Python
wire-contract tests passed. Python contract dependencies were supplied through
`uv run --with jsonschema`; the architecture script ran under Git Bash with
`python3` mapped to the installed `py -3`. The complete workspace/runtime
qualification suite was not run for this domain-contract slice.
