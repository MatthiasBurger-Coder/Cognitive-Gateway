# Provider-independent prompt and invocation IR — #263

`gateway-application::prompt_ir` admits the actual immutable CG-10
`PlannedContextHandoff` with a negotiated `PreparedModelInvocation` and the exact
authorized tool definitions. `gateway-domain::prompt_ir` defines version 1.0
producer bindings, correlation identity and stable admission diagnostics. No
provider SDK or new context/planning owner is introduced.

`PromptIr` preserves the full typed producer rather than accepting a JSON payload:
original task input and constraints, stable catalog instructions, scoped context
fragments (including knowledge, memory and reference-only representations), evidence,
canonical requirements/plan/satisfaction links, optional examples, output and
verification remain distinct. Tools remain a separate exact inventory. There is
no history-loading, retrieval, reference materialization or inference interface.
Explicit history references in the semantic task remain references. A redacted
CG-10 audit export cannot construct this envelope.

`InvocationIr` includes the prepared invocation, so authority, sensitivity,
locality, finite budgets, strategy, generation profile and effective controls
remain bound alongside semantic content. Fields are private and output-only;
changing a cloned producer or definition requires admission again. A task pin,
context pin, output definition or tool inventory mismatch fails before consulting
the host. Producer pins reject `latest`, unsupported versions and cross-scope
bindings. CG-10 already validates task/context content digests, completeness of
mandatory context, source classes, budgets and output/verification compatibility.

## Trusted host integration

Implement `InvocationAdmissionPort` and its existing
`SemanticReferenceValidator` supertrait in the application host. There is no
permissive production default. The application invokes the existing task
`validate_references` path on every check, then calls `validate_current` with the
complete immutable candidate. This host must authenticate, against trusted stores:

- The semantic completeness result/manifest and every task reference, including
  schema, rule and verifier contents, revisions, existence and target association.
- The exact task/context/projection, requirements/plan/example pins and selected
  source revisions; freshness, quarantine, provenance, sensitivity and scope.
- Stable catalog definitions and current Process/Policy admission; exact tool
  definitions and permitted disclosure/localities for the complete payload.

A well-formed digest does not authenticate its source. Missing stores or current
admission produce `AUTHENTICATION_UNAVAILABLE` or a specific denial. Hosts must
not resolve aliases, acquire missing context or silently update supplied pins.
Semantic incompleteness produces `INCOMPLETE_TASK`; stale content produces
`STALE_INPUT`; revoked decisions produce `AUTHORITY_DENIED`; disallowed disclosure
produces `DISCLOSURE_DENIED`. Invalid pins produce `INVALID_BINDING`; an unsupported
envelope version produces `INCOMPATIBLE_VERSION`.

Call `admit_invocation_ir` after semantic/CG-10 compilation and strict model
negotiation. Call `render_invocation_ir` with the admitted envelope, current host,
explicit tokenizer profile and #264 renderer/tokenizer ports. It rechecks current
admission before rendering. `dispatch_invocation_ir` rechecks immediately before
transport and refuses a payload bound to a different IR. `PromptInvocationPort`
receives both immutable IR and correlated rendering; it returns only an unverified
`ModelProposal`. A response carrying a different invocation digest is rejected.
No external effect or verification success follows from transport success.

For CG-26 routing use #262 `route_invocations_and_execute`. Its host `prepare`
continues current CG-10/Process/Policy revalidation before every route, including
fallback. In each model `attempt`, build/admit the handoff for that exact prepared
alternative, render, then dispatch through these APIs. Deterministic/reflex paths
continue without a model invocation. Admission failure must return a failed attempt;
fallback cannot reuse a rendering for a different prepared invocation or widen
context/authority. The older #261 preparation and #264 pure rendering APIs remain
component APIs and are insufficient as dispatch admission on their own.

No installed host or live provider transport is added by this slice; #269/#279
own those compositions. Current-store and provider tests use explicit synthetic
host/transport implementations. Operators must supply authenticated stores and
current authority checks when composing the service; deployment configuration
alone cannot establish admission. No new storage, credentials or launcher flags
are required by these library contracts.

## Encoding, audit and recovery

Canonical IR bytes are compact `serde_json::to_vec(&ir)`: fixed struct order,
ordered tool maps and existing canonical producer serialization. Version 1.0
pins this encoding. `InvocationIrIdentity` records envelope version, invocation ID,
invocation contract SHA-256, full handoff SHA-256, tool-definition SHA-256 and full
IR SHA-256. Both schema shape and verification definitions are retained and pinned
in `PromptBindings`. Provider text is not semantic identity. The renderer adds its
payload digest, model/tokenizer/template versions and accounting snapshot. Retain
#262 routing telemetry alongside these records, joined by the prepared invocation
digest, to capture the exact route snapshot and alternatives. Retain these together for #266/#267 verification correlation; no chain of thought is needed.
Do not persist full sensitive IRs where audit disclosure policy requires redaction.

On stale input, recompile/reacquire through the owning upstream services and create
a newly admitted invocation; never patch pins on an old rendering. Restore current
authority or correct disclosure before retrying denied requests. Token overflow
still uses #264's typed error and CG-10's replanning/compaction, with no truncation.
Rollback removes these additive modules and their host calls together; existing
pure preparation/rendering does not become permission to dispatch.

## Review and acceptance evidence

Review basis: base `201d54a9c7eccb66e16860898f681dbb1859e654`, clean checkout,
DEVELOPMENT mode; #263/#178, #261/#262/#264, #192 boundary, canonical #285/#286
producers, ADR-007/008/015. Requirement, architecture, automation and evidence
review used role passes by the implementing agent, without independent approval.
Readiness: READY_FOR_WORKFLOW for the additive contracts/application boundary.
Ordered work: bind existing producers; enforce current-host admission at each use;
connect rendering/transport ports; qualify negative paths and measured coverage.
Estimate: 1–2 engineer-days implementation, 0.5–1 qualification, moderate
confidence, using existing foundations. Live hosts/adapters are excluded.

| ID | Requirement/source | Production path/owner | Dependencies/state | Automated evidence | Status |
| --- | --- | --- | --- | --- | --- |
| I01 | #263 no implicit history | Typed `PromptIr` over CG-10 producer | Existing #285/#286 handoff | Full producer equality, strict-binding history rejection, explicit-only render payload | VERIFIED (component) |
| I02 | #263 context/schema pins | `PromptBindings`, `InvocationIr::identity`, admission | Existing task/context content validation and #261 preparation | Exact bindings, repeat identity/digests, cross-invocation rendering refusal | VERIFIED (component) |
| I03 | #263 stale input rejected | Task reference validation + current admission on admit/render/dispatch | Trusted current-store port; no default authority | Changed store task/context/schema/verification/plan/examples/tool/authority; revocation after rendering; zero dispatch | VERIFIED (synthetic host integration) |
| I04 | #263 untrusted data stays data | Lossless producer, separate tool definitions, #264 rendering | CG-10 external fragment trust and dedicated example sections | Hostile examples never enter instructions, original input preserved; existing two-renderer adversarial suite | VERIFIED (structural) |
| I05 | #263 adapter interfaces/diagnostics | `PromptInvocationPort`, admission/render/dispatch services | #264 ports implemented, #269 live transports separate | Transport failures, wrong response identity, version/tool failures, frozen denial fixture | VERIFIED (component) |
| I06 | #178 >=95% coverage, architecture/docs | Versioned additive Rust modules, CI coverage gate | Existing Rust/Python gates | Per-file measured coverage, architecture regressions, lint/format; retained report | VERIFIED; [evidence](evidence/EPIC-06.03-prompt-ir.json) |

Run `cargo test -p gateway-application --test prompt_ir --locked` and the
EPIC-06.03 entry in `scripts/quality-gates.json`. Fixtures reuse actual canonical
CG-10 producer construction in `tests/fixtures/prompt-rendering-v1/support.rs`;
`tests/fixtures/prompt-ir-v1/denials.json` freezes diagnostic spellings. This proves
structural isolation and deterministic Gateway behavior, not probabilistic model
resistance, live provider compatibility or completion of the parent invocation plane.
