# Model capability, provider and invocation contracts — EPIC-06.01 (#261)

Historical evidence and removed review documents are linked to their archived Git
revision. They are not current acceptance evidence. Generate fresh reports in an
output directory or retain them as CI artifacts.

The Rust contracts in `gateway-domain::model_invocation` and preparation service
in `gateway-application::model_invocation` define the provider-neutral boundary.
They add no model endpoint, credentials, transport, renderer or runtime launcher.
CG-09 remains policy authority, CG-04 remains lifecycle authority, and CG-10
remains context-selection authority. Models return unverified proposals.

## Ownership and integration

| Contract | Meaning and construction |
| --- | --- |
| `ModelCapability` | Immutable exact capability/model/provider pins, CG-26 task classes, supported input contract IDs and exact output-schema pins, context/output limits, structured modes, tool support, reasoning strategies and provenance |
| `ModelProvider` | Immutable provider and adapter declarations, locality, sensitivity ceiling, supported generation controls/ranges and known or unexposed defaults |
| `GenerationProfile` | Immutable profile identity/revision, version, typed controls, explicit output reserve and structured mode, reproducibility class |
| `ModelInvocation` | Output-only validated envelope binding task content digest, compiled-context pin, capability pin, output definition, strategy, tool inventory, profile, finite budgets, disclosure limits, authority and provenance |
| `ModelRuntimeStatus` | Separate mutable health observation; unavailable/unknown health cannot change semantic eligibility |
| `EffectiveGenerationRecord` | Requested profile/provider declaration digests, each effective control's value/origin/provenance, unexposed defaults, output reserve, mode and reproducibility class |
| `PreparedModelInvocation` | Output-only application preparation result carrying invocation, capability, provider and effective generation record to `ModelInvocationPort` |

Reuse `ResolvedReference`, `SchemaVersion`, `ContentDigest`, `SemanticTaskIR`,
`semantic_output::OutputContract`, `CapabilityId`, `SensitivityClass`, and CG-26
`TaskClass`/`PrivacyBoundary`. No new dependency edge or semantic-task field is
introduced. Existing `cognitive_routing::ModelIdentity`, `RouteCandidate` and
`LocalInferencePort` retain their existing consumers. This slice introduces no
second router. [EPIC-06.02 routing](model-capability-routing.md) now composes
these contracts with the existing CG-26 selection and bounded coordinator.

Host integration sequence:

1. Resolve and authenticate a current task and output definition through the
   existing semantic reference/completeness path. Capture current Process/Policy
   admission, permitted tools/localities and sensitivity in `ModelInvocationBasis`.
2. Capture the exact full CG-10 context digest and revision. Construct
   `ModelInvocationData`; deserialize raw input only into this untrusted data type.
3. Call `ModelInvocation::new(data, basis)`. It verifies exact task digest/ID/scope,
   context pin, output definition and schema pin; prevents capability or disclosure
   expansion; and rejects unsupported versions and invalid bounds.
4. Call `prepare_model_invocation` with immutable capability/provider declarations
   and an authenticated current health observation. Semantic eligibility is checked
   before availability. No routing or fallback occurs.
5. The [#263 IR admission service](prompt-invocation-ir.md) and #264 rendering
   service bind the full semantic handoff and recheck current authority
   and tokenizer-aware budgets, then call an outer adapter through the port. The
   adapter must apply every effective control or return a typed mismatch. Transport
   success produces a `ModelProposal`, never a verified result or tool dispatch.

`ModelInvocationBasis` is trusted captured host input, not proof that a caller's
claims are authentic. The constructor cannot authenticate external content from a
pin alone. Hosts must verify reference existence/content, freshness, policy/process
admission, qualification, task revision, provenance and disclosure against trusted
stores. Contract tests use synthetic pins, not live authorization or provider proof.
A local-only basis cannot be widened to an external provider through this service.

## Generation semantics and deterministic encoding

All wire contracts use exact schema version `1.0` and deny unknown fields. Profile,
provider and capability deserialization runs the same validating constructors used
by Rust callers. Validated objects expose immutable data references; modifying a
cloned data object requires validation again. Invocation and preparation results
cannot be deserialized directly.

Optional controls form a closed typed map: `TEMPERATURE`, `TOP_P`, `TOP_K`,
`STOP_CONDITIONS`, `SEED`. Scalars are finite; temperature is nonnegative, top-p is
in (0,1], top-k is positive, seed is an unsigned hint, and stops are nonempty UTF-8
strings. A declaration supplies finite inclusive scalar/integer domains or explicit
stop count/byte limits. Stops are a semantic set; encoding sorts and deduplicates
them. V1 permits at most 128 stops of at most 4096 bytes each. No concrete sampling
value or vendor default is supplied by the core.

`max_output_tokens`, input reservation, context limits and timeout are bounded
unsigned 32-bit integers; attempts are bounded unsigned 16-bit integers. Required
limits must be positive; context addition uses widened arithmetic to avoid overflow.
These are per-invocation contract bounds. #268 owns cumulative session enforcement,
retries and reconciliation; preparation does not execute attempts or reset budgets.

Equivalent profiles serialize to identical bytes: field order is fixed, control
maps use the closed enum order, stop sets sort lexically, and negative zero is
normalized. The frozen profile fixture includes canonical JSON and SHA-256. Identity
is `(profile_id, revision, content_digest)`; model/provider/adapter identities reuse
exact scoped revision/digest pins. `latest` revision aliases and noncanonical
uppercase digests fail. Trusted registries must reject rebinding an existing
identity/revision to different content; local validation is not a registry.

Strict negotiation has no implicit degradation. Missing controls or values outside
declared support return `MODEL_CAPABILITY_MISMATCH`. Every supported control has
one known default or an explicit unexposed-default entry. Requested values override
known defaults and carry profile-digest provenance. Known defaults carry declaration-
digest provenance. Unexposed defaults remain visible as unknown, never fabricated.
The explicit output reserve and structured mode are always in the effective record.
A seed requires `SEED_HINT`; it never establishes bit-for-bit replay. Both profile
and effective record remain audit inputs even when generation is stochastic.

## Diagnostics and operator guidance

Stable error spellings are `INCOMPATIBLE_VERSION`, `INVALID_BINDING`,
`INVALID_LIMITS`, `INVALID_GENERATION_PARAMETER`, `MODEL_CAPABILITY_MISMATCH`,
`CONTEXT_LIMIT_EXCEEDED`, `AUTHORITY_MISMATCH`, and `MODEL_UNAVAILABLE`.
Version, binding, numeric and authority failures occur before any adapter call.
Capability mismatch is a configuration/admission failure; changing health cannot
repair it. Unknown health fails closed as unavailable. Fix declarations/profiles
explicitly, create a new revision for changed material configuration, and retain
requested/effective parameters with the trace. Never silently remove controls,
expand locality/tools, truncate evidence or reinterpret a model response as success.

No secrets, SDK objects or arbitrary provider option bags enter these contracts.
Pin provenance and sensitivity protect correlation and handling; storage retention,
redaction and secret management remain host responsibilities. Keep effective records
with the immutable invocation/task/context/model bindings; downstream result
verification must correlate those bindings before accepting output.

## Requirement and evidence traceability

Single-agent requirement, architecture, automation and tester role passes informed
this bounded implementation. No independent reviewer or approval is claimed.
The repository is in DEVELOPMENT mode. The ready implementation order is shared
contracts → validating constructors → preparation/port → tests → gates/docs.
No EPIC-05 implementation dependency cycle is introduced.

| ID | Requirement/source | Production owner | Automated evidence |
| --- | --- | --- | --- |
| M01 | #261 incompatible versions and invalid/unbounded limits | Domain constructors, immutable deserialization | `incompatible_versions_unknown_provider_fields_and_unbounded_wire_values_fail`, `rejects_nonfinite_and_invalid_generation_controls_before_dispatch`, declaration/capability validation tests |
| M02 | #261 provider types stay outside canonical contracts | Domain typed algebra and application port; existing Cargo architecture guard | Unknown provider-field rejection; `test_model_invocation.py`; `check-architecture.sh` |
| M03 | #261 task/context/output/tool binding, scope and authority | `ModelInvocation::new` | `invocation_binds_captured_task_context_schema_scope_and_authority` and locality rejection test |
| M04 | #261 availability separate from eligibility | `validate_model_eligibility`, preparation | `semantic_eligibility_is_exact_and_never_depends_on_health`, `health_is_a_separate_check_after_semantic_eligibility` |
| M05 | #261 deterministic generation serialization | `GenerationProfile`, ordered maps/sets, zero normalization | Frozen profile/digest, permutation and negative fixture tests |
| M06 | #261 unsupported controls never silently dropped | `effective_generation`, sealed preparation boundary, adapter port obligation | Every requested control rejected when support is removed; domain/range rejection; application preparation tests |
| M07 | #261 effective parameter/provenance auditability | `EffectiveGenerationRecord`, `PreparedModelInvocation` | Requested/default/unknown provenance test; prepared adapter boundary test |
| M08 | #178 canonical generation controls only; >=95% coverage | Domain/application, CI quality gate | `check-model-invocation-coverage.py`, retained measured coverage and validation logs |

Run component tests with
`cargo test -p gateway-domain -p gateway-application --test model_invocation --locked`.
Run the measured per-file gate using the EPIC-06.01 entry in
`tests/scripts/quality-gates.json`. See
[evidence](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-06.01-model-contracts.json) for executed checks and source
bindings. [Production routing (#262)](model-capability-routing.md) is implemented
as a separate application service. [Prompt/Invocation IR (#263)](prompt-invocation-ir.md)
now supplies the application admission boundary.
[Cumulative resilience (#268)](invocation-resilience.md) supplies a shared
application run ledger. [Local and independent HTTP adapters (#269)](model-provider-adapters.md)
are implemented with loopback conformance; live/full-plane qualification (#270)
remains separate.
The #264 rendering/accounting boundary is implemented below.

## Deterministic semantic renderers — EPIC-06.04 (#264)

`gateway-application::prompt_rendering::render_invocation` consumes the actual
immutable CG-10 `PlannedContextHandoff`, a `PreparedModelInvocation`, the complete
host-authenticated tool definitions and an explicit tokenizer profile. It checks
exact task/context/output bindings, model/profile identity, renderer/template
identity and the complete permitted tool inventory before rendering. This is a
pure preparation boundary: it does not acquire sources, select context, compact,
load history, execute tools or dispatch a model.

The outer `gateway-daemon::prompt_renderers::SemanticRenderer` implements two
version-1 runtime message projections:

| Renderer identity / template | Runtime structure | Semantic payload |
| --- | --- | --- |
| `semantic-chat` / `1` | `messages` with one system and one user string | JSON envelope in user content |
| `semantic-blocks` / `1` | Separate `system`; one user message with a text block | Same JSON envelope in block text |

These structures can be consumed by chat-message and block-message transports.
They are offline projections, not complete provider HTTP requests. Endpoint,
model parameters, native tool/output configuration, credentials and exact provider
SDK compatibility belong to #269. Neither layout interpolates template text or
infers roles from content. JSON serialization escapes quotes/control characters;
XML, role and chat-template delimiters remain string data. Only the fixed Gateway
boundary statement and CG-10 stable catalog references enter the system channel.
Catalog references do not themselves authenticate instruction prose. Every other
handoff section remains separate inside `semantic`: bindings, task, context,
evidence, examples, planning, output and verification. Scope, revisions, trust,
sensitivity and provenance survive losslessly, including reference-only content.
Tools retain exact capability pins, descriptions and typed input schemas; the
output schema uses the existing `OutputContract::json_schema()` projection.
Structural role separation does not prove probabilistic injection resistance.

`gateway-domain::prompt_rendering` defines `TokenizerProfile`, `CountMode`,
`TokenUsage`, `TokenCountResult`, `TokenBudgetSnapshot` and stable diagnostics.
Tokenizer profiles carry exact schema version `1.0`; unsupported versions fail.
`TokenizerPort` receives the **complete serialized runtime payload**, its SHA-256,
semantic sections and effective model pin, generation-profile digest, tokenizer
ID/version, renderer ID and template version. An adapter must count the actual
framing, instructions, task, context, evidence, tools and schemas, allocating all
usage across the named component fields and `framing_tokens`. Planning, bindings,
examples and verification also contribute to total usage. The components sum to
`total_input_tokens`; they are allocations of a whole-payload count, not a claim
that independently tokenizing substrings is additive for every tokenizer.

Exact counting is preferred whenever the effective tokenizer is available.
Conservative estimation must return `estimated` and a pinned method/provenance
reference. Never substitute a character heuristic labeled `exact`, invent a
universal byte bound or count only user prose. The tests use an explicitly named
synthetic byte tokenizer; it is not a production LLM tokenizer. This slice defines
the replaceable tokenizer port; model-specific tokenizer adapters remain outer
implementations. Unavailable counting fails as `TOKENIZER_UNAVAILABLE`.

The service rejects mismatched profile/payload digests, floating tokenizer
versions, invalid provenance, zero totals and arithmetic overflow. It reserves
the effective profile's output capacity and compares the full sum against the
selected capability's context limit. Exactly fitting input is accepted.
`CONTEXT_LIMIT_EXCEEDED` returns the full accounting snapshot to EPIC-02/CG-10
for replan/compaction. No evidence or constraint is dropped. Other stable errors
are `INVALID_BINDING`, `UNSUPPORTED_RENDERER`, `INVALID_ACCOUNTING` and
`RENDERING_FAILED`. Errors never return a dispatchable `RenderedInvocation`.
Successful results retain invocation, handoff and payload digests plus complete
accounting metadata. The host controls trace retention and disclosure because the
serialized payload contains full admitted content.

Host integration order is: authenticate current Process/Policy and full handoff
through #263; prepare the model invocation; authenticate tool definitions and
choose the tokenizer/template profile for that exact route; render and account;
then pass both prepared controls and the successful render to the corresponding
outer transport. Re-run admission/rendering/accounting after any route or context
change. A renderer does not authenticate source stores, define a new Invocation
IR, implement cumulative session budgets (#268), or accept model output (#266/267).
The repository still lacks #263's complete invocation admission/dispatch host;
these APIs do not make the older `ModelInvocationPort` a qualified dispatch path.
Issue-level production integration remains dependent on that prerequisite.

| ID | #264 requirement | Production owner | Automated proof |
| --- | --- | --- | --- |
| R01 | Same canonical input/version yields identical structure | Outer renderer; immutable `RenderInput` | Both layouts decode to the complete actual CG-10 producer output; repeated payload/accounting equality |
| R02 | Delimiters cannot upgrade trust | JSON envelope, fixed runtime roles | Hostile canonical examples remain in user data in both layouts; no additional roles/system content |
| R03 | Tokenizer/model/template metadata and exact/estimated provenance | Domain accounting and `TokenizerPort` | Both count modes, profile/version/payload/provenance mismatch denials |
| R04 | Schemas/tools/framing and output reserve count | Application full-payload accounting | Synthetic exact whole-payload count; nonzero schema/framing usage; boundary at limit ±1 |
| R05 | Overflow explicit; mandatory data intact | Typed error and immutable handoff | Overflow snapshot and unchanged source; no successful rendered result on errors |
| R06 | Existing scope/authority/provenance types and independent adapters | CG-10 handoff, application ports, daemon adapters | Exact section/tool equality, stale bindings/tool inventory denials, architecture guard |
| R07 | >=95% introduced production coverage | `check-prompt-rendering-coverage.py`, release gate | Per-file measured coverage for domain/application/outer adapter; missing and undercovered evidence rejected |

Run `cargo test -p gateway-daemon --test prompt_rendering --locked` and the
EPIC-06.04 measured gate in `tests/scripts/quality-gates.json`. Qualification scope and
executed evidence are retained in
[`EPIC-06.04-prompt-rendering.json`](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-06.04-prompt-rendering.json).
Live-provider and installed-host E2E are not claimed by these offline tests.

The [#266 verification boundary](output-verification.md) now binds parsed output
and trusted constraint/rule reports to the full admitted invocation identity and
exact response digest. Schema/reference/tool checks and current admission are
mandatory; critics are advisory. Verification grants no tool-dispatch authority.
