# Declarative Context and Situation IR v1 (CG-06.01)

CG-06 owns the provider-independent declarative layer above the narrow
CG-02 `ExecutionContextIR`. The v1 foundation is implemented in
`gateway-domain::declarative_context` and the typed identities are exported
from `gateway-domain::identifiers`.

## Physical ownership

The selected ownership layout is:

```text
gateway-domain::identifiers
  typed CG-06 identities and validation
gateway-domain::declarative_context
  version value object and aggregate boundaries
gateway-domain::situation
  assessment, risk, situation assembly and explainability semantics
gateway-application
  later CG-06 external-context and runtime integration use cases and ports
gateway-context
  CG-10 Context Compiler / Semantic TAG / ExecutionContextIR projection
```

CG-06 does not add a dependency to `gateway-context`, and it does not define
`CompiledContext`, provider prompts, planning, policy decisions or retrieval
implementations. The domain remains inward-facing and provider-independent.

## Version contract

`DeclarativeContextVersion` uses the strict `MAJOR.MINOR` form. `1.0` is the
only version accepted by the v1 aggregate constructors. Syntactically valid
future versions can be parsed for diagnostics, but are rejected at the
aggregate boundary with `UnsupportedSchemaVersion`; they are never silently
downgraded.

## Aggregate boundaries

- `DeclarativeContext` owns the versioned declarative input identity.
- `ObservedState` owns a versioned normalized observation snapshot. Later
  APIs may expose the same aggregate as `CurrentState`.
- `Situation` owns the versioned operational-picture identity.

The fields inside those aggregates are intentionally added by the dependent
CG-06 slices. This prevents CG-06.01 from guessing later intent, evidence,
assessment or process-state semantics.

## Intent and desired-state semantics (CG-06.02)

`Intent` retains an `IntentId`, one explicit `DesiredState` and optional
`OriginalInput`. Original input is either validated inline text or a typed
external `ReferenceId`; it is never confused with normalized desired data.

`DesiredState` contains:

- `DesiredCondition` records with a typed `SubjectPath`, strict comparison
  operator and optional expected value;
- finite `ConditionExpression` trees using `Condition`, `ALL`, `ANY` and
  `NOT`; and
- `DeclarativeConstraint` and `AcceptanceCriterion` records that remain
  goal-space data and never grant capabilities, permissions or process
  transitions.

Supported values are boolean, signed integer, exact bounded decimal, validated
string, identifier-like symbol and non-empty homogeneous sets of scalar
values. Decimal values use an integer plus an explicit scale (maximum 18),
never locale parsing or binary floating-point equality. No implicit
string/number or unit conversion exists in v1.

Conditions and expressions are validated at construction. Unknown condition
references, empty logical branches, duplicate identities, incompatible
operator/value pairs and unsupported nesting fail closed. Collections are
canonicalized by typed identity or expression key; list input order cannot
change logical meaning.

## Observation, fact, evidence and provenance semantics (CG-06.03)

Observation, Fact, Evidence and Provenance are separate explicit records in
gateway-domain::observation:

- Observation is one typed report or measurement and must reference a
  Provenance record.
- Fact is a normalized assertion or explicit negation. It must reference at
  least one observation; the collection boundary then verifies the complete
  observation-to-provenance chain.
- Evidence is a typed artifact, report, measurement, retrieval result, tool
  output or model output that supports or challenges one or more facts. It
  carries provenance and cannot grant capability, permission or a process
  transition.
- Provenance records a provider-independent source class and source
  reference, optional producer/timestamps and parent lineage. Source class
  does not imply trust, authority or freshness.

EvidenceContent keeps the core IR bounded: small inline material is possible,
while large artifacts use a typed ReferenceId and optional 64-character
hexadecimal content digest. Source timestamps remain opaque source-supplied
text; the domain does not infer local time or current truth. Model and
retrieval output therefore remains explicitly labeled input/evidence and
cannot silently become an authoritative fact.

ObservationEvidenceSet is the joint validation boundary. It sorts each record
type by typed identity, rejects duplicate typed identities, deduplicates
semantically equivalent records by their canonical keys (retaining the lowest
typed identity), and fails closed for dangling parent-provenance,
observation-to-provenance, fact-to-observation or evidence-to-fact references.
A fact with both SUPPORTS and CHALLENGES edges is retained and reported by
conflicting_fact_ids(); conflicting evidence is not silently discarded or
converted into authorization.

Deduplication keys are deterministic canonical strings: observations use
subject/value/provenance/occurrence time, facts use subject/value/polarity and
observation lineage,
and evidence uses kind/summary/content/provenance, links and occurrence time.
The explicit CG-04 process evidence requirement and gate semantics remain in
gateway-process; this domain set is only a typed situation-side evidence
boundary and does not duplicate process authority.

## Current-state normalization (CG-06.04)

normalize_current_state is a pure function from a validated
ObservationEvidenceSet and explicit NormalizationInput options to an
ObservedState snapshot. It never reads wall-clock time, mutates external
project data, selects a preferred source or produces a Delta, Plan,
CapabilityRequirement or process transition.

Each NormalizedStateEntry has one of four statuses: KNOWN exposes exactly one
typed value and its assertion polarity; UNKNOWN represents an explicitly
unobserved subject; CONFLICTED retains all incompatible claims and stable
diagnostics; UNSUPPORTED retains claims without exposing a successful value
when the caller explicitly requires supporting evidence. Every entry carries
fact, observation, evidence and provenance lineage.

Subject collections are ordered by SubjectPath. Claims from equivalent
explicit inputs are grouped without source precedence. Different typed
values, different assertion polarities and support/challenge evidence for one
fact remain visible as conflicts. V1 defines no unit conversion, so a string,
integer and decimal representation are never silently made comparable.
Missing evidence, incompatible value types, conflicts and unknown state have
stable NormalizationReasonCode values; dangling input references remain
ValidationError failures at the CG-06.03 collection boundary.

## Information quality semantics (CG-06.05)

The quality module keeps trust, handling sensitivity, confidence, freshness,
uncertainty and conflict as independent metadata. TrustClass distinguishes
canonical references, observed evidence, retrieved content, caller input,
derived assessments and synthetic data; none of these values grants policy,
capability or process authority. SensitivityClass ranges from PUBLIC through
NORMAL, INTERNAL and CONFIDENTIAL to SECRET and is metadata for later
handling decisions, not a permission decision.

Confidence has Score, Unknown and NotApplicable values. Scores are bounded to
0..=1 and high confidence cannot resolve contradictory assertions. Freshness
is evaluated only by evaluate_freshness with explicit UnixTimestamp inputs
and a FreshnessPolicy; missing times are UNKNOWN, expired values are STALE,
and future source times fail closed. ValidityInterval rejects impossible
endpoints without consulting an ambient clock.

QualityMetadata::merge is the deterministic propagation rule used by
normalization: differing trust classes become MIXED, the strongest
sensitivity is retained, differing confidence becomes UNKNOWN, stale input
is not hidden by fresh input, and uncertainty/conflict are conservatively
preserved. A quality conflict marks a normalized entry CONFLICTED. The
metadata boundary is explicit and provider-independent; neither model
confidence nor serialization location can escalate information into
authority.

## External context ports, scope isolation and lifecycle (CG-06.07)

`gateway-application` owns the external-context boundary. Its
`DeclarativeIntentInputPort` accepts a structured `Intent`, while
`ObservationEvidenceInputPort` accepts a `ScopedObservationBatch` containing
the already validated observation/evidence/provenance graph and explicit
quality metadata. `ScopedContextSource` is provider-neutral: repository,
filesystem, Git/GitHub, CI, runtime and retrieval adapters identify
themselves only through `SourceKind` and return `SourceSnapshot` metadata plus
domain records.

`ContextScopeId` is an opaque request/session isolation key, never a
canonical project profile. `ScopeLifecycle` is explicit: OPEN accepts input,
SEALED is read-only, and CLOSED removes the transient session. The neutral
`InMemoryContextStore` has no global current-project singleton and stores
intent/batches under the scope key only. Replaying the same explicit batch
returns an idempotent result; the same retry identity with different data is
rejected. Identical source object IDs in different scopes remain distinct.

`CachePort` entries are keyed by `(ContextScopeId, ContextCacheEntryId)` and
carry the declarative version, source change token/digest, full validated
lineage and quality metadata. Cache presence cannot modify catalog membership,
permissions or process semantics. Entries are explicitly invalidatable and
scope cleanup removes transient derived state. Cache/retry keys contain only
source metadata and typed identities, never raw evidence or secret-like
values. `CacheCapabilities` exposes sensitivity retention, invalidation,
raw-content and retention characteristics for later operational/policy
constraints; it is not itself an authorization decision.

## Situation assembly, assessments and risks (CG-06.06)

`SituationAssemblyInput` deterministically combines one normalized
`ObservedState` snapshot with explicit assessments, risks, unresolved
diagnostics and optional external/runtime references. The resulting
`Situation` stores the observed-state identity, keeps assessments and risks
as separate derived collections, and orders every collection by its typed
identity. Assembly does not read a clock, call a provider or create a Delta,
Plan, permission, capability grant or process transition.

An `Assessment` contains a category, conclusion, lifecycle-neutral status,
stable reason code, human summary, explicit basis references and
`QualityMetadata`. Its `AssessmentOrigin` is either a versioned deterministic
`AssessmentRuleContract` (with optional semantic digest) or an explicit
external source/provenance pair. Model proposals therefore remain
`PROPOSED` model-sourced derived data and cannot replace the underlying
facts. Unsupported rule versions fail closed.

`Risk` separately contains category, severity, status, explicit basis and
quality metadata. `RiskLikelihood::Unknown` and qualitative likelihoods are
valid first-class results; a numeric likelihood is accepted only as an
explicit bounded probability and is never inferred from severity or
confidence. Risk assembly checks that sensitivity, stale freshness,
uncertainty and unresolved conflict are not silently downgraded from the
state/assessment basis.

State conflicts, unknown state, missing required evidence and other
data-quality conditions remain visible as `SituationDiagnostic` records.
`Situation::explainability()` is a deterministic human-readable projection
of the stored assessment/risk conclusion, reason code and exact basis; it
does not perform a second evaluation. Dangling state, fact, evidence,
provenance or assessment references are rejected at the assembly boundary.

## Serialization and compatibility (CG-06.08)

The complete CG-06 v1 model has a provider-independent JSON wire contract in
`gateway-domain::cg06_serialization`. `DeclarativeContextSituationDocument`
serializes the context, optional intent and observation/evidence records,
normalized observed state, and assembled situation as one validated document.
The individual validated aggregates also expose compact `to_json` and
`from_json` helpers for focused fixtures and integration boundaries.

The wire contract uses explicit discriminators for typed values, logical
expressions, quality metadata, evidence content, assessment/risk origins and
risk likelihoods. Exact decimals retain their unscaled integer and scale;
integer, decimal, string, symbol, boolean and set values are never coerced.
Secret-like or large evidence is represented by a typed reference and optional
digest, so serialization does not require embedding raw content or imply
disclosure authority.

Serialization is canonical: identity-keyed collections and semantically
unordered diagnostics/references are sorted before domain reconstruction and
emission. Object field order is fixed by the Rust wire structs. A compact
round trip therefore produces the same JSON representation, while pretty JSON
is accepted as presentation-only input. The representative regression fixture
covers the full intent → records → normalized state → assessment/risk →
situation chain, including provenance and sensitivity metadata.

Deserialization first parses the strict wire shape and then rebuilds every
aggregate through its domain validation constructors. Unknown fields,
discriminators, malformed identifiers, unsupported `1.x` versions, duplicate
identities, dangling lineage/basis references, invalid confidence/time
intervals and unsupported rule versions fail closed. v1 has no implicit legacy
or future-version migration; a compatibility path must be introduced as an
explicit later contract change. The wire layer contains no model-provider
roles, prompt tags, vector-store types or runtime cache syntax.

## Application operations and runtime references (CG-06.09)

`gateway-application::DeclarativeSituationApplication` exposes the stable
Rust use-case boundary for `validate_declarative_context`,
`normalize_current_state`, `assess_situation`, `inspect_situation`,
`explain_situation` and `serialize_situation`. These operations delegate to
the validated CG-06 domain aggregates; they do not create a second
normalization, assessment or authorization implementation. Scoped external
input remains owned by the CG-06.07 `ContextScope` and its ports;
`validate_scoped_declarative_context` consumes the resulting read-only
`ScopedContextSnapshot` and merges source batches only through the existing
`ObservationEvidenceSet` validation boundary.

`OperatingMode` and `ExecutionProfile` are reused directly from CG-02 through
the read-only `ExecutionContext` reference. CG-04 process state is attached
only through `ProcessSnapshotInput` and the resulting
`ProcessSituationReference`, which wraps CG-04's `ProcessInspection`. The
projection retains Process Definition ID/version/digest, Process Instance ID,
revision, current state, lifecycle status, active gate statuses, blocker
records, process evidence, retry/waiting data and authorized abstract
activities without copying process transition semantics into CG-06.

Process snapshots are accepted only after the CG-04 definition-pinning check;
callers may additionally provide an expected instance revision. A mismatched
definition or stale revision fails explicitly, and the Application API offers
no arbitrary process-state assignment or transition operation. Process
Definition and Process Instance remain authoritative in CG-04, including legal
transitions, gates, blockers, history and mutation storage.

`SituationInspection` and `SituationExplainability` identify the exact
declarative context, observed-state snapshot, CG-02 mode/profile and optional
CG-04 definition/instance revision used. They are read-only projections. A
later CG-09 policy/authorization extension and the CG-10 projection remain
outside this slice:

```text
Situation + authorized Plan Step + Resolution + Policy
        ↓ CG-10
minimal CompiledContext / ExecutionContextIR projection
```

## End-to-end reference slice (CG-06.10)

The neutral integration fixture in
`crates/gateway-application/tests/cg06_end_to_end.rs` demonstrates the full
CG-06 path for an external project without introducing a provider or a real
secret:

```text
scoped source snapshot
  → intent + repository/tool/synthetic provenance
  → observations, facts and evidence
  → normalized state (coverage = 92.00, sensitivity = SECRET)
  → deterministic assessment + qualitative risk
  → Situation
  → read-only CG-02 execution context and CG-04 process reference
  → explainability + canonical JSON
```

The fixture records the architecture dependency observation
`domain -> infrastructure exists`, a coverage report below the requested
95.00 threshold, and a reference-only sensitive artifact with digest. The
raw value is never present. Input collections are deliberately reordered to
prove canonical reconstruction, the same scope retry is idempotent, and a
second scope remains empty. CG-04 is represented by a pinned, blocked review
snapshot; the test verifies that inspection does not mutate the process.
The slice stops at `Situation`: it does not plan, authorize, compile context,
or execute an activity.

## Identity and collection rules

The core identities are separate Rust types for context, intent, desired
state, acceptance criteria, observations, facts, evidence, provenance,
observed state, assessments, risks, situations, scopes, sources and external
references. They share strict identifier validation but cannot be passed to
one another accidentally.

Input collection order is not semantic. Later normalizers and serializers
must expose collections in canonical typed-identity order. Optional fields
must distinguish absent from present values; no default value may be inferred
from input order, locale, wall-clock time or provider behavior.

## Boundaries for later slices

CG-06.02–CG-06.06 add the detailed intent, state, evidence, quality and
situation members. CG-06.03 owns the situation-side evidence/provenance
records; CG-06.07 adds explicitly scoped external-context ports. CG-06.08
defines the wire contract. CG-06.09 integrates CG-02 and CG-04 by reference
and read-only boundaries. None of those changes may move ownership into
gateway-context or introduce CG-07, CG-09 or CG-10 semantics.
