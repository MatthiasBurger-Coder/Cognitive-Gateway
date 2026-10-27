# Connector policy admission v1 — EPIC-07.07 #230 / prerequisite #369

`gateway_application::connector_policy::Cg09ConnectorAdmission` implements
the provider-independent admission adapter for outbound connector requests. It
calls the existing `gateway_policy::PolicyEngine`; it does not duplicate policy
rules or communicate with an MCP provider.

## Trusted input boundary

Composition supplies a `ConnectorPolicySource`. This is an authority boundary,
not a convenience data provider: it must authenticate the host actor, load the
current `PolicyAuthority`, and return request-scoped `StepFacts`, process/mode
facts, current policy identity/revision, consent state/receipt, and any budget
reservation/deadline. It must bind those facts to the complete request and
optional source: target and project scope, canonical capability and operation
class, typed arguments, task/step/execution/correlation, source/resource and
inventory revision. No planner, model, connector, mapping document, or public
dispatch record may populate authoritative approvals or consent.

`ConnectorPolicySnapshot` is intentionally supplied only through this trusted
port. The adapter rejects unknown capabilities, canonical class mismatches,
unallowlisted/denied capabilities, missing required evidence, non-Allow
CG-09 reports and missing consent. READ/SEARCH/INSPECT map to canonical
`CapabilityClass::Inspect`; MUTATE/ADMIN map to `Mutate`. ADMIN additionally
requires the canonical capability to be in the host's explicit
`administrative_capabilities` set. It never inherits approval from a different
or ordinary mutation capability. Mapping consent metadata may set
`consent_required`; it cannot grant consent. CG-09 continues to require consent
for Mutate.

The dispatch records the current policy ID/revision, consent receipt, budget
reservation and deadline. The host calls `revalidate` immediately before
transport effects. Revalidation reloads current facts, reruns CG-09 and rejects
any changed binding or non-Allow result. Failures are content-free and report
`NotDispatched` at this boundary.

Current target checks and `validate_current_dispatch` also support bounded
transport liveness polling. Their source lookups are read-only and acquire no
grants or reservations. Polling reruns pure CG-09/binding validation without
calling `record_decision`; explicit authorization and immediate pre-effect
revalidation retain their audit boundary.

### Required production step and audit refinement

`ConnectorPolicySnapshot` now requires the exact `step`, `resolved`,
`has_prerequisites`, `constraints` and `preconditions`, alongside authenticated
`StepFacts`. There are no permissive defaults. A mismatched step fails before
policy evaluation; all other fields flow into the existing CG-09 engine.
`has_prerequisites` comes from the current plan; whether they are satisfied is
separate authenticated evidence. The production source must obtain these facts
from the same actual step assembly used by `PolicyApplication` and confirm the
requested capability belongs to that resolved step.

The existing `record_decision` hook now receives a read-only
`ConnectorPolicyEvaluation`. Its stage is either `Authorize` or
`PreEffect` carrying the original dispatch. The envelope retains the exact
captured snapshot, request/source, optional policy report and effective outcome.
Early snapshot-contract failures have no fabricated report. A policy report
that allows combined with a stale dispatch binding records a denied outcome.
The result is computed before audit; an audit failure prevents dispatch.
Successful pre-effect admission additionally checks current facts after the
audit without writing another audit, so a grant withdrawn by the audit callback
cannot survive into the standalone transport path. Polling remains audit-free.
These records describe evaluation, not proof that an effect occurred.

`PostgresSessionStore::record_connector_policy_decision` now implements that
bounded append-only journal in the owner-scoped `CognitiveStore`, under
`connector-policy-decisions-v1`. It leaves the session checkpoint revision and
fence untouched. Its immutable audit context captures the actual allocation,
native resolution basis and complete policy/runtime/manifest/mapping/producer
pins. Events retain the evaluated envelope, safe request identity, original
dispatch identity, outcome and normalized findings; they contain no raw
arguments, credentials or provider payloads. The hash chain and bounded journal
are checked before append, and an unknown commit does not grant admission.
The distinct operator consent decision ledger retains its existing ownership
and purpose. PostgreSQL component tests exercise this journal; a production
connector policy source must still install it using its captured immutable
basis. That source and the complete installed runtime composition remain open.

The private `local_sessions/runtime_admission/connector_source.rs` now provides
the concrete Source control-plane component: current authenticated store/claim
checks, native Producer and Action contexts, exact selector membership,
request-specific consent validation and the real decision journal. Its
PostgreSQL tests do not establish the installed preparation/execution path.
The required native model-producer, retained consent-resume and qualified
target-factory consumers remain under implementation.

`preview_connector_policy` exposes the same pure evaluation used by actual
CG-09 admission for an outstanding consent question. It returns a native policy
report without constructing a dispatch, acquiring a grant or writing an audit.
An Allow report does not establish an action context or permit execution.

### Read capabilities belong to the actual action step

Snapshot, post-effect and final goal reads keep the exact session action-step
binding. Their readonly capabilities must belong to that same step's actual
resolved capability closure. Mapping metadata cannot add them. Existing native
planning can carry multiple mandatory requirements for one DeltaItem in its
action step; an actual selected Skill can also declare an Inspect capability
dependency with an explicit real provider. `PolicyApplication::step_input`
assembles those definitions and their constraints through the existing resolver.

A separate verification step does not automatically authorize a read under the
action step. Missing or unresolved readonly dependencies must deny before
connector I/O. Even a correctly resolved read still requires its actual current
CG-09 policy and authenticated facts; resolving the Mutate capability provides
no implicit read grant. The production factory must preserve these joins rather
than weakening the request-step check or fabricating a capability definition.

## Exact connector consent identity

`sessions::ConnectorConsentBinding` is a distinct application contract from
artifact `ActionBinding`. It binds authenticated owner/project, canonical
scope, session/run/issued revision, step/dispatch, the full content-free
`ConnectorRequestIdentity`, policy ID/revision, expiry and authenticated
issuer. The request identity includes connector target, capability, operation
class, inventory revision, typed-request digest (including typed arguments),
task/step/execution and correlation. Its strict decoder validates all typed
identities and the request scope/step relationship. The trusted adapter must
compare it with a freshly built identity from the exact request and current
policy before dispatch; any drift, expiry or revocation denies.

`PostgresSessionStore` now has a distinct `PendingInteraction::ConnectorConsent`
variant and a content-addressed connector decision ledger. The owner-scoped
loader returns current grant, denial or withdrawal state, and the operator
decision method checks the exact pending binding, issuer and expiry before it
records a decision. A separate `ApproveConnector` application command reloads
the grant, checks the exact pending identity, requires current runtime
revalidation and advances the checkpoint to Runnable without dispatching.
Connector continuation requires a runtime to stage a consent challenge with
durable prepared-proposal records and to resume that exact proposal after
approval; unsupported implementations fail closed. Existing artifact consent
retains its own `ActionBinding` and `Grant` semantics.

The local host accepts a strict trusted `connector_goal` registration block,
bound to the existing canonical plan and containing only typed read-only goal
observation mappings. It can persist an initial UNKNOWN assessment. The local
runtime does not implement proposal preparation, CG-09 policy admission or
connector dispatch, so a registered goal cannot continue into an effect. A
production runtime must implement `prepare_authorized_step`, persist the
verified proposal identity and bytes returned with a consent challenge,
implement `resume_approved_step` without rerunning the model, and re-evaluate
CG-09 immediately before transport effects. The retained connector fixture
uses a policy-cleared Ready result and does not exercise consent issuance or
approval end to end.

## Explanation and evidence

CG-09 findings are deterministic and machine-readable at the policy engine
boundary. `ConnectorPolicySource::record_decision` is called for policy
evaluations and receives the immutable evaluation envelope; the connector port
projects non-Allow outcomes to the stable `PermissionDenied` failure code. No
raw policy/provider prose crosses the connector boundary. The report is never a
dispatch token.

The retained [component test source](../crates/gateway-application/tests/connector_policy.rs)
covers Inspect allow, missing Mutate consent, additional consent requirements,
ADMIN escalation, required evidence and consent receipt changes at revalidation.
The [retained evidence record](evidence/EPIC-07.07-policy.json) binds those
results to source hashes. This is COMPONENT evidence for the adapter. It does not establish an installed
coordinator, durable reservation/consent store, live provider, independent
post-effect verification or EPIC-07 completion. EPIC-08 #274 owns coordinator
composition and system-level zero-effect verification.
