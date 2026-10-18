# Deterministic invocation routing — EPIC-06.02 (#262)

`gateway_application::model_routing` connects the CG-26 router to the validated
EPIC-06.01 model contracts. This is an application service and host adapter
boundary. Concrete inference transports and full CG-10 invocation admission remain
owned by #269 and #263. No inference server is required for the routing tests.

## Host integration

1. Capture a qualified CG-26 `ModelCapabilitySnapshot`. Keep its explicit cost
   unit, conservative cost/latency reservations, hardware requirements and
   qualification provenance. Do not populate it from model output.
2. For every model candidate, supply an `InvocationRouteBinding` keyed by candidate
   ID: host-admitted `ModelInvocation`, immutable `ModelCapability` and
   `ModelProvider`, and independent `ModelRuntimeStatus`. Deterministic and reflex
   candidates have no model binding. Missing, duplicate or orphan model routes fail
   registry validation; there is no provider discovery or inferred fallback.
3. Build an `InvocationRoutingSnapshot` with `schema_version: SchemaVersion::V1`.
   `resolve_invocation_route(&request, &snapshot)` produces an inspectable decision
   and optional `PreparedModelInvocation`, without dispatch.
4. For bounded execution call
   `route_invocations_and_execute(&request, &snapshot, &runtime)` with an
   `InvocationRouteRuntime`. Its `prepare` must compile a current CG-10 step under
   Process/Policy authority on every attempt. Its `attempt` receives the compiled
   step and negotiated invocation for a model, or `None` for deterministic/reflex
   execution. Deterministic/reflex handlers must never invoke an inference port;
   reflex execution continues to use CG-25 admission and verification.

The older `select_route` and `route_and_execute` APIs remain the CG-26 advisory
boundary for existing consumers. Model invocation hosts use the new strict APIs;
CG-26 candidate metadata alone does not establish invocation compatibility.

CG-26's step completion/verification contract and EPIC-06's typed output schema
are different contracts. The former remains matched against every candidate and
freshly compiled step; the latter is matched by its exact pinned schema against
model capabilities. They are not substituted for one another. The trusted host
owns their task mapping and the compiled-context binding (#263). This service
neither grants policy permission nor verifies generated output.

## Eligibility and fallback invariants

Hard invocation checks run before CG-26 ranking: exact capability/provider pins,
model ID/revision/digest and adapter ID/revision, declared locality, task and input
support, exact output-schema pin, reasoning strategy, structured output/tool
support, sensitivity, permitted localities, context plus reserved output tokens,
and strict generation-control negotiation. Health observations validate their
provider and observation pins independently. Unknown health is unavailable.
CG-26 still enforces qualification, availability, cognitive requirements,
input/output support, privacy, hardware, cost and latency.

All fallback invocations must be identical except for their explicitly
host-admitted capability pins. That includes task/scope/revision, compiled-context
pin, output shape/schema, exposed tools, authority, sensitivity/localities,
generation profile, input reservation, provenance and timeout/attempt limits.
There is no implicit re-admission or widening of a failed invocation. The route
request cannot exceed the invocation's attempt or timeout bound. Cost units must
match; there is no currency conversion.

When `deterministic_sufficient` is true, every model candidate receives
`MODEL_NOT_REQUIRED`, even if deterministic/reflex candidates are absent,
unavailable or fail. Such an operation terminates explicitly rather than calling
a model. Reflex applicability remains a separate CG-26 hard filter.

Eligible routes use the existing lexicographic order: route kind, cost, latency,
candidate ID. Equivalent snapshots, including candidate-order permutations,
produce the same decision and computed snapshot digest. Runtime availability
changes cannot alter semantic eligibility or introduce an undeclared provider.

Execution reuses CG-26's coordinator: one frozen snapshot, no repeated candidate,
fresh Process/Policy preparation, original hard filters on each fallback,
cumulative conservative reservations on failed calls as well as success, and
terminal denial on invalid reports, output mismatch or exceeded bounds. A host
must enforce the declared synchronous call deadline. Tokenizer accounting,
session-wide token accounting and retries remain #264/#268 responsibilities.

## Diagnostics and operation

`RouteExplanation.alternatives` retains all CG-26 rejection reasons plus
`MODEL_NOT_REQUIRED` and typed `Invocation(ModelContractError)` reasons. For
example, `{"INVOCATION":"CONTEXT_LIMIT_EXCEEDED"}` records input plus reserved
output overflow; `MODEL_CAPABILITY_MISMATCH` covers unsupported semantics,
sensitivity/locality or generation controls; `MODEL_UNAVAILABLE` covers unavailable
or unknown health; `INVALID_BINDING` identifies identity drift. A malformed
registry or inconsistent fallback request returns `RoutingError` before dispatch.
No eligible route yields `selected: None`; execution records `NO_COMPATIBLE_ROUTE`.

Operators should correct the declaration, health observation, qualification,
resource allocation or authorized request and capture a new snapshot. Never
increase permissions, switch locality or drop schema/tools/evidence implicitly
as an availability workaround. Do not compact or truncate context in the router;
request CG-10/EPIC-02 replanning when the reservation exceeds a model's limit.

`InvocationRoutingTelemetry` includes the version, computed canonical snapshot
digest, full captured declarations and health observation provenance, effective
negotiated controls, and existing CG-26 decision/attempt/usage/compiled-step
provenance. Apply project disclosure policy when retaining these records. The
legacy configuration digest is retained but does not replace the computed digest.
Model generation remains probabilistic; reproducible selection is not a claim
that a model produces identical output.

## Requirement and evidence traceability

Review method: requirement, architecture, automation and tester role passes by
one implementing agent; no independent review or approval is claimed.

| ID | Requirement/source | Production path/owner | Dependencies | Automated evidence | Status |
| --- | --- | --- | --- | --- | --- |
| R01 | #262 stable eligible selections | `model_routing::admit`, `resolve_invocation_route`, CG-26 `evaluate` | Shipped #261 contracts and CG-26 ranking | `equivalent_snapshots_resolve_stably_with_pinned_effective_controls` | VERIFIED |
| R02 | #262 deterministic tasks make zero model calls; preserve reflex | `MODEL_NOT_REQUIRED` hard filter; optional prepared handoff | Host deterministic/reflex handlers; existing CG-25 admission | `deterministic_execution_and_failure_never_call_a_model_and_reflex_is_preserved`, `no_model_installation_is_needed_for_successful_deterministic_or_reflex_execution` | VERIFIED |
| R03 | #262 unavailable/incompatible providers cannot widen scope/locality | Strict preparation, exact common fallback data, identity matching | #261 immutable/admitted types and health pins | `contract_and_runtime_failures_have_stable_diagnostics`, `individually_admitted_alternatives_cannot_change_meaning_or_authority`, `unavailable_local_provider_cannot_escalate_to_disallowed_external_provider` | VERIFIED |
| R04 | #262 task/output/context/privacy/resources and explicit reasons | Invocation eligibility plus CG-26 hard filters | Declared hardware, cost and latency bounds | `all_cg26_filters_still_apply_after_invocation_negotiation`, contract-negative cases, existing CG-26 tests | VERIFIED |
| R05 | Parent #178 bounded fallback and authority ownership | Shared CG-26 execution coordinator; `InvocationRouteRuntime` | Fresh real CG-10 `CompiledStep` per attempt | `fallback_uses_same_contracts_and_remaining_cumulative_budget`, existing context/router execution tests | VERIFIED |
| R06 | #262 versioning, architecture, >=95% materially changed file coverage | Rust application/contracts; no SDK dependencies | Standard architecture and measured coverage gates | `test_model_invocation.py`, `check-model-routing-coverage.py`, qualification evidence | VERIFIED |

The shared positive/negative fixture builders are in
`tests/fixtures/model-routing-v1/support.rs`. Run the EPIC-06.02 entry in
`scripts/quality-gates.json` for measured coverage. The retained
[evidence report](evidence/EPIC-06.02-model-routing.json) records executed checks
and binds their source files. This evidence qualifies the routing service and
trusted-host integration with a test runtime, not an installed inference adapter
or the complete parent epic.
