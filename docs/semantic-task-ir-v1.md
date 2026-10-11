# SemanticTaskIR v1 and versioning contract

## Status and ownership

EPIC-05.02 [#180](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/180)
implements the provider-independent Rust task contract in
[`semantic_task.rs`](../crates/gateway-domain/src/semantic_task.rs), its
[JSON Schema](../schemas/semantic-task.schema.json) and
[reference fixtures](../tests/fixtures/semantic-task-v1/). The supported task
schema is **1.0**. This specification refines the
[CGSL scope and vocabulary](cgsl-scope-and-vocabulary.md) and
[ADR-015](adr/ADR-015-semantic-task-ir-boundary.md).

This slice supplies typed task meaning and structural admission. It does not
implement the CGSL grammar/compiler, entity resolution, planning adapters,
output validation, verification execution or runtime invocation. Those remain
#181–#193. MUST and MUST NOT below are normative.

## Domain and wire fields

`SemanticTaskData` is construction input. `SemanticTaskIR::new` validates and
canonicalizes it into immutable `SemanticTaskIR`. `from_json` and serde
`Deserialize` use the same admission. `data()` exposes a read-only view;
changing a task requires fresh construction and validation.

All fields in this table MUST appear on the wire, except the three explicitly
optional fields. Collections MAY be empty unless a task-specific downstream
contract requires operands. Empty collections mean no declarations; they do
not certify that acquisition, policy evaluation or verification is complete.

| Field | Rust representation / meaning |
| --- | --- |
| `schema_version` | Existing `SchemaVersion`, supported value `1.0` |
| `id` | Existing `TaskId`, caller-assigned task identity |
| `task_type` | Finite `SemanticTaskType`: `ANALYZE`, `ANALYZE_PERFORMANCE`, `INSPECT`, `CREATE`, `MODIFY`, `VERIFY` |
| `target` | `SemanticTarget`: `SERVICE`, `ARTIFACT`, `PROJECT` or `ENTITY`, with a pinned `ReferenceId` binding |
| `goal` | `SemanticGoal`: canonical outcome `ReferenceId` and nonempty explanatory description |
| `inputs` | Named roles (`ReferenceId`), each a CG-06 `TypedValue` literal or pinned `ReferenceId` binding |
| `context_refs` | Pinned existing `DeclarativeContextId` bindings; interpretation knowledge, not execution context |
| `current_state` | Optional pinned CG-06 `ObservedStateId`; unknown/conflicted entries remain in the referenced snapshot |
| `desired_state` | Optional inline CG-06 `DesiredState`, with its existing version, typed predicates, constraints and acceptance criteria |
| `observations` | Pinned CG-06 `ObservationId` bindings; original provenance remains in the referenced records |
| `history` | Pinned `ReferenceId` bindings to historical data, never replay instructions |
| `capability_requirements` | Pinned abstract `CapabilityId` contracts; declarations, not concrete Agent/Skill bindings |
| `constraints` | Existing CG-02 `Constraint` values and `ConstraintKind`; formal predicate constraints remain inside `DesiredState` |
| `policy_refs` | Pinned existing `PolicyId` bindings, never consent or allow/deny decisions |
| `evidence_refs` | Pinned CG-06 `EvidenceId` bindings, retaining support/challenge and provenance semantics at source |
| `assumptions` | Explicit unverified `SemanticAssumption`: ID, typed premise, pinned basis and existing CG-06 `Confidence` |
| `output_contract` | `SemanticOutputContract`: pinned provider-neutral output schema |
| `verification_contract` | Explicit finite `SemanticVerificationCheck` collection |
| `process` | Optional existing domain `learning::ProcessReference` (definition ID, positive version, digest); no inline Process IR |

The inventory classifies outcomes: analysis explains, inspection reports,
creation requests a new artifact, modification requests a target change, and
verification evaluates acceptance. `ANALYZE_PERFORMANCE` specializes analysis
for performance. Task kinds do not choose an executor or grant permissions.
Goal descriptions explain the outcome identifier; they MUST NOT substitute
for required typed predicates or supply executable instructions. If formal
state acceptance matters, `desired_state` MUST be supplied by the producer;
the IR does not infer it from prose.

`CapabilityRequirement` from CG-07 includes Delta-derived lineage and therefore
is not semantically equivalent to an upstream declaration. Reusing that type
here would manufacture planning lineage. This contract instead reuses
`CapabilityId`; #190 owns validated conversion to the existing planner.

The process reference is reused from the domain because `gateway-process`
depends on `gateway-domain`. Importing `DefinitionIdentity` into this crate
would introduce a dependency cycle. #191 owns the mapping and applicability
proof against the actual CG-04 definition, not a second process model.

These wire field names do not extend the 20-name CGSL source vocabulary.
`current_state`, for example, is a wire field corresponding to CGSL `state`;
it is not an accepted source-language alias.

## Invariants and reference admission

1. Schema version MUST be supported. Unknown task kinds, target kinds,
   verification checks and object members MUST fail closed. Duplicate JSON
   members MUST be rejected by direct typed parsing.
2. Mandatory meaning MUST use concrete typed operands. This executable-shaped
   contract has no unresolved, ambiguous, conflicting or knowledge-gap variants.
   Such candidates remain outside `SemanticTaskIR`; confidence or an assumption
   MUST NOT select a target or fill a missing mandatory operand.
3. Goal descriptions MUST satisfy existing `NonEmptyText` validation. IDs and
   values MUST satisfy their existing CG-02/06 constructors. Inline desired
   state MUST satisfy CG-06 expression, operator and reference validation.
4. A `ResolvedReference<I>` MUST carry exactly one typed `id`, `scope`,
   `contract`, `contract_version`, `revision` and `digest`. Digests MUST be
   64 lowercase hexadecimal SHA-256 characters. The descriptor records a
   captured binding; its syntax alone does not prove existence or uniqueness.
5. Every reference MUST share the explicit target scope. v1 supports one scope
   per task; cross-project bindings require a separately specified future
   contract. No implicit scope widening or latest-revision fallback is allowed.
6. Collection identities MUST be unique: input role, reference ID within each
   reference collection, constraint ID, assumption ID and verification check.
   Two revisions of the same ID in one collection are conflicting input and
   MUST be rejected. Scalar typed sets MUST be homogeneous, nonempty and have
   no duplicate values. Sets are sorted using the existing typed value order.
7. Verification MUST include `OUTPUT_SCHEMA_VALID` and
   `NO_UNRESOLVED_REFERENCE`. `ALL_CLAIMS_SUPPORTED` additionally requires
   `EVIDENCE_REQUIRED`. Declaring these checks does not say they passed.
8. Assumptions remain unverified premises even with confidence 1.0. Source
   observations/evidence retain original lineage, quality and epistemic status;
   reference transport MUST NOT promote them to facts or policy authority.

A consumer MUST call `validate_references` with an authoritative
`SemanticReferenceValidator` before executable handoff, and revalidate on a
changed basis. The validator receives the full task and each exact binding.
It MUST check existence, unique resolution, target kind, semantic role,
contract/version support, source digest, captured revision, scope and
association with the target, including inline desired-state subjects. The
output schema must be available and appropriate to the requested result;
capability references must identify abstract catalog capabilities, and policy
references must load trusted policy records. Missing, stale, wrong-kind,
unsupported or conflicting bindings MUST return an error. The IR provides the
validation port; #187/#190 supply production resolution and handoff adapters.

The process binding is projected for this validation as contract
`cg.process-definition`, contract version `1.0`, numeric definition version in
`revision`, definition digest and the task scope. This identifies the domain
reference envelope, not the CGSL or Gherkin language version. CG-04 validates
the actual definition version/digest and CG-08 validates applicability.

JSON Schema checks structural shape. Rust additionally checks collection IDs,
cross-field scope, typed-set homogeneity, CG-06 predicates, process identifiers
and verification implications. Neither a JSON Schema result nor successful
Rust shape construction establishes external reference truth or permission.

## Canonical serialization and identity

`to_canonical_json()` is the normative canonical encoder. Its result is compact
UTF-8 JSON without BOM, insignificant whitespace or a trailing newline. Object
keys are recursively ordered lexically by the `serde_json` value map; the
workspace uses its default sorted map representation. This is a project wire
contract, not a claim of RFC 8785 conformance. Generic serde serialization
roundtrips the same data but its struct-member order is not canonical bytes.

Reference collections are sorted by typed ID, inputs by role, constraints and
assumptions by ID, and verification checks in the declared order
`OUTPUT_SCHEMA_VALID`, `EVIDENCE_REQUIRED`, `ALL_CLAIMS_SUPPORTED`,
`NO_UNRESOLVED_REFERENCE`. CG-06 `DesiredState` retains canonical condition,
constraint, acceptance and expression ordering. Typed set operands are sorted
by CG-06 `TypedValue::Ord`, including expected operands in desired state.
Collections have set semantics; history is a set of pinned records, and event
chronology remains inside those records rather than array position.

Optional `current_state`, `desired_state` and `process` may be absent or null
on input. Canonical output always emits them, using null for absence. Other
collections are always emitted, including empty arrays. Numeric operands use
existing CG-06 integer/exact-decimal shapes; confidence uses existing bounded
four-decimal quantization. Decimal scale remains meaningful: this layer does
not convert units or remove declared precision. Strings preserve case,
whitespace and Unicode scalar contents; no Unicode normalization or prose
rewriting is performed. Quotes and backslashes are escaped; backspace, form
feed, newline, carriage return and tab use their short JSON escapes. Other
U+0000–U+001F characters use lowercase `\u00xx` escapes. Other characters
are emitted as UTF-8, and `/` is unescaped. Integers use minimal base-10
notation. Confidence scores use decimal fractions with unnecessary trailing
zeros removed, except the endpoints encode as `0.0` and `1.0`.

`TaskId` is a caller identity, stable across revisions if the caller chooses.
`content_digest()` is the lowercase hexadecimal SHA-256 of **all** canonical
JSON bytes, including schema version, task ID, references and their revisions.
It identifies an exact task revision, not an equivalence class independent of
ID or precision. No self-digest field is included. Permuting collection input
order does not change the digest; changing meaning, scope, ID, output,
verification or a pinned source changes it. The frozen `.canonical.json` and
`.sha256` fixtures give independent implementations byte-level expectations.

## Versioning, compatibility and deprecation

CGSL language version, SemanticTaskIR schema version and every referenced
CG-02/06/07/04 contract version are independent. Matching major numbers do not
establish compatibility between these contracts. No negotiation is inferred
from a provider or model.

The initial reader and writer support exactly SemanticTaskIR **1.0**.
Unsupported major versions, future minor versions, unknown enum values and
unknown members MUST be rejected. Readers MUST NOT discard unknown semantics,
retry as 1.0, accept a known subset or silently downgrade. `from_json` exposes
`ValidationError::UnsupportedSchemaVersion` for structurally valid unsupported
versions; serde surfaces admission errors through its deserialization error.
Task versions must use canonical `MAJOR.MINOR` spelling without leading zeros.
Malformed versions or malformed payloads may fail earlier as wire errors.

A **minor** revision MAY add optional fields whose absence preserves the exact
previous meaning, optional metadata, or explicitly opt-in features. It MUST
specify defaults, supported reader versions, canonical representation and
conformance fixtures. A newer reader MUST continue to read supported earlier
minor schemas using each schema's original meaning and canonical bytes. An
older reader rejects a newer minor until explicitly upgraded; backward
compatibility does not imply forward compatibility. New enum variants must be
version-gated; they cannot be accepted by an earlier schema. A writer may emit
an earlier minor only through an explicit lossless projection proving that
no new mandatory meaning or verification requirement is discarded.

A **major** revision is required for changing field or construct meaning,
removing/renaming fields or values, adding mandatory semantics without an
old-document equivalent, changing scope/authority rules, weakening invariants,
or changing canonical bytes/digest rules for an existing schema. Migration
must be explicit, validated and auditable. Unsupported major versions fail
closed even when their JSON resembles v1.

Deprecation must be documented with the first affected version, rationale,
replacement and migration fixtures. Deprecated members retain their original
meaning and encoding throughout the supported major version; warnings may be
added, but silent rewriting or removal is prohibited. Removal requires a new
major version. A compatibility change must update Rust admission, JSON Schema,
fixtures and documentation together. There are no deprecated v1.0 members.

## Acceptance evidence and limits

| #180 criterion | Evidence |
| --- | --- |
| Provider/model independence | Strict finite types and nested unknown-field rejection tests; no provider dependencies |
| No unresolved mandatory references | Concrete mandatory shapes, negative candidate/missing-field fixtures and mandatory external-validation port |
| Reuse existing primitives | Typed CG IDs, `SchemaVersion`, `TypedValue`, `DesiredState`, `Constraint`, `Confidence`, `ContentDigest`, `ProcessReference` |
| Canonical deterministic serialization | Frozen canonical bytes/digests; permutation and typed-set tests |
| Major incompatibility fails closed | Rust/serde and JSON Schema unsupported-version tests |
| Additions and deprecations documented | Compatibility and migration rules above |
| Roundtrip reference fixtures | Minimal inspection and populated performance-analysis fixtures, including all optional fields |

`crates/gateway-domain/tests/semantic_task_v1.rs` checks the Rust contract;
`tests/contracts/test_semantic_task_contract.py` checks JSON Schema and frozen
canonical bytes independently. The standard domain coverage gate requires
95% line coverage, with `scripts/check-semantic-task-coverage.py` enforcing
the same minimum for the new module separately in the standard quality gate.
This slice does not claim production reference resolution,
natural-language compilation or end-to-end executable handoff.

Validation executed on 2026-10-11: workspace tests and workspace/all-target
Clippy (`-D warnings`) passed; formatting and the architecture dependency guard
passed; all 15 contract tests and 35 architecture regression tests passed.
`cargo llvm-cov -p gateway-domain --all-targets --locked --fail-under-lines 95`
measured 97.48% domain line coverage. The separate semantic-task module gate
passed at 169/175 lines (96.57%). The reference-validation tests use a synthetic
captured basis, not a production resolver or installed-client runtime proof.
