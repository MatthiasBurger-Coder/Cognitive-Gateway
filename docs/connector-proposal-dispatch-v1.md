# Verified connector proposal dispatch v1 — EPIC-08.03 #274

`gateway_application::connector_proposals::ConnectorProposalDispatcher` bridges
an `output_verification::VerifiedOutput` to the existing
`ConnectorApplicationPort`. It rejects a verification record for a different
`InvocationIr`, validates each proposal against the admitted prompt tool
binding, and asks a trusted `ConnectorProposalBindingPort` to bind it to
current task/session facts. The returned typed request must preserve the exact
capability, project scope, task identity and JSON arguments. Each admitted
`ToolDefinition` also carries the mapped connector target, exact discovered
inventory revision and executable argument-schema digest. The binder compares
those pins with fresh discovery and the `OutputType` schema before it builds a
request; same-name capability/schema drift fails before transport. The MCP host
exposes complete argument/result schemas only for its current discovered
revision, and the application converts only its closed schema subset into an
invocation definition. Decimal/collection
argument shapes fail closed until their canonical connector mapping supports
them.

`CanonicalConnectorProposalBinder` is the reusable implementation: it reruns
`InvocationAdmissionPort`, obtains step/execution/target from the trusted
`ConnectorSessionBindingPort`, discovers the current canonical connector
inventory, and builds a request only for an exact discovered capability. It
projects only boolean, integer and string object arguments into the existing
typed value contract. Inventory discovery supplies mapping metadata only; each
subsequent `invoke` still obtains current CG-09 authorization and a fresh
pre-effect revalidation.

Proposal correlations use the versioned `cg.connector-proposal-correlation/2.0`
basis: invocation identity, zero-based proposal ordinal and complete proposal.
Two identical proposals at different positions remain distinct ordered actions;
their request digests and consent cannot be interchanged. Rebuilding an older
persisted proposal with a different identity must fail its existing pins rather
than silently renew consent.

Each proposal is revalidated against current invocation state immediately before
binding and is dispatched in order through the connector application boundary.
That boundary owns current CG-09 authorization, consent, scope, inventory,
credential, budget and pre-effect revalidation. A failed call stops later calls.
The batch retains earlier observations and the exact `ConnectorFailure`,
including `effect: Unknown`; it never rewrites provider acknowledgement as
verified task success. A later independent verifier must decide whether the
desired result was achieved.

## Provider model names in session tool invocations

The version numbers in this paragraph describe the legacy `CanonicalV1` wire
profile. `PortableV1` below uses distinct templates.

When a provider uses a different wire name for an admitted canonical model,
the trusted host supplies the same `RuntimeModelBinding` to
`SessionToolHttpRenderer::for_runtime` and `SessionToolHttpAdapter::new`.
The renderer checks the full canonical model reference, renders the provider
name into the payload, and returns template version `2`. The host must use
that renderer identity in its `TokenizerProfile`; the profile's model remains
the canonical reference. Rendering and token accounting precede dispatch.

The adapter reconstructs the exact mapped body and rejects a changed mapping
before HTTP. It accepts a response only when the provider model name matches
the configured mapping, then returns the canonical model ID to the existing
output-verification boundary. A provider model name never changes invocation
identity, connector inventory pins or dispatch authority. Without a runtime
mapping, the renderer and adapter continue to use template version `1` and the
canonical model name on the wire.

## Composition boundary and limitation

Session adapters receive a coordinator-minted `SessionPhaseResources` only
after the corresponding fenced claim commits. The persisted reservation's v2
identity binds disjoint preparation, approved-resume, execution and fallback
snapshot allocations. Model/repair resources belong to preparation; later
phases cannot obtain them or recreate the original whole-step allowance.
Legacy total-only v1 reservations remain readable and cannot mint capabilities.
Goal verification is pure computation over persisted records; the coordinator
refuses nonzero acquisition budgets in that phase until a one-shot claim exists.

For model calls, allocate a model-only child slice and construct one
`SessionToolRun`. Every retry/fallback calls `SessionToolInvocation::invoke_in`
with that same run. Its immutable limits, cost unit, cancellation token and
deadline cannot be replaced between attempts. Smaller provider usage reports
do not replenish conservative output-token/cost reservations. The existing
invocation engine retains its unknown-output byte reservation and terminal
uncertainty behavior.

`SessionResourceAuthorityPort` must join the active owner/run/step/dispatch,
claim revision/fence and reservation to the exact registered invocation
producer pins. Separate valid scope checks cannot authorize a different task
in the same scope. Estimated token counts additionally require explicit current
qualification of the complete model/tokenizer/render profile, method provenance
and payload upper bound. Arbitrary estimates fail closed. Current claim,
producer and qualified-bound checks run after host callbacks immediately before
provider dispatch; claim/producer checks and the run's original cancellation/
deadline check run again after output-verification callbacks.

These APIs are qualified components. They do not supply the missing production
authority source or install the connector runtime described below.

### Connector phase enforcement

`SessionConnectorRun` consumes one coordinator-minted connector-only child
allocation and implements the existing `ConnectorApplicationPort`. A logical
discovery, invocation or resource read spends one `connector_calls` unit before
transport; discovery includes bounded connection negotiation and pagination.
The optional field is omitted when zero, preserving legacy reservation bytes.
Model-only allocations reject connector units. Mutation and administrative
operations require the execution phase; recovery uses a distinct read-only
allocation. Goal verification cannot acquire resources through this adapter.

The trusted quote source supplies hard upper bounds for the exact operation,
cost unit and allocation. Unknown bounds fail before transport. Charges are
retained after denial, cancellation and failed or uncertain results. Result
metric validation failure terminates the run, including read-only failures;
unknown mutations also prohibit direct retry. Independent recovery requires a
separate admitted read-only run.

Transport context checks join current allocation, exact request/source/target
and quote with CG-09 admission. The dispatch reservation must equal the child
allocation's persisted reservation. CG-09 polling uses validation-only current
facts without recording an audit per poll; normal authorization and explicit
pre-effect revalidation remain audited. A final allocation/request check runs
after regular guards. These control-plane callbacks must be bounded and must
not create connector effects or change authority as a side effect of reading.

Disable and shutdown retain exact target checks and can drain a revoked run
without acquiring another operation allowance. They grant no application work;
connect and health cannot bypass the reserved discovery path. Tests use actual
coordinator allocations, mapped MCP transport and CG-09 with controlled peers.
They establish component integration, not installed-provider qualification.

The binder is a trusted port, not a client/model adapter. Its implementation
must revalidate the current invocation and resolve the current session's
step/execution binding, authenticated actor, source mapping and full scoped
snapshot. The dispatcher does not let a proposal choose those fields. It also
does not itself persist a session checkpoint or post-effect evidence.

The existing `SessionCoordinator` admits both its structured context-artifact
goal and a separately registered connector goal. `LocalSessionHost` can load
that trusted registration and persist an initial `UNKNOWN` assessment, but its
`LocalConnectorRuntime` still returns `UnsupportedCapability` for preparation
and execution. Its installed pure goal verifier checks independently persisted
readsets, the complete original goal and current scoped repository claim.
Consequently the proposal path remains a provider-
independent COMPONENT bridge, not a claim that the shipped CLI currently
executes tool proposals. Durable session consent is implemented at the shared
store boundary. A native current-state policy source and a sealed target factory
are implemented as components; their complete composition with a native model
producer, snapshot/readback acquisition and shipped-path qualification remain required for
full #274 and EPIC-08 acceptance.

The [component tests](../crates/gateway-application/tests/output_verification.rs)
exercise verified sequential proposals, exact argument binding, stale
invocation rejection, zero transport calls on policy/consent/inventory/schema
drift, retained partial results and uncertain effect typing. MCP schema tests
qualify the current-revision schema accessor and the closed-schema conversion.
The source-bound [qualification record](evidence/EPIC-08.03-connector-proposals.json)
records exact commands, coverage and limitations.

## Portable provider function names and proposal text

The explicit `SessionToolWireMode::PortableV1` profile uses provider function
aliases with 64 ASCII letters, digits or underscores. Each alias hashes the
complete admitted tool definition, including its canonical capability and current
inventory/schema pins. The adapter reconstructs the exact request and checks the
invocation-local inverse before returning canonical capability IDs. Unknown,
stale and native-ID fallback names are rejected. Names and schemas remain inside
the rendered payload measured by the admitted tokenizer profile.

Renderer and adapter must both receive `with_wire_mode(PortableV1)`. The
renderer returns template `3` without a runtime model mapping and `4` with one;
the adapter rejects a mismatched template before HTTP. Supported native schemas
remain unchanged. Optional scalar arguments use provider `strict:false` and
still undergo gateway schema/argument validation. Provider strict decoding cannot
replace gateway admission. Legacy templates `1`/`2` preserve their old projection;
their dotted capability names are unsuitable for an OpenAI Chat Completions endpoint.

`SessionToolResponseMode::ToolProposalTextV1` is separately explicit. It requires
an admitted String output and at least one actual function proposal. Missing or
null provider content becomes empty text, and JSON-looking text stays text.
Neither empty text nor a proposal certifies successful execution. The current
producer definition must bind both modes and the actual renderer/tokenizer
profile; transport setters alone do not supply that authority. A native producer
consumer is still being integrated.

Provider output is checked against the actual credential after every consumed
JSON decoding boundary, including function arguments and result JSON. Escaping
a credential with JSON Unicode escapes cannot bypass this check. No raw provider
response or secret enters the retained verification record. The
[transport qualification archive](evidence/EPIC-08.03-portable-model-transport-validation.json.gz)
retains the 38 session-tool and 11 model-adapter passing tests; one existing live
provider test is ignored. This evidence uses loopback HTTP and injected trusted
verification hosts, not an installed or complete native session.
