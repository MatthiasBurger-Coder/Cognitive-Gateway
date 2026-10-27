# Verified connector proposal dispatch

The application bridge accepts an admitted `InvocationIr` and the
`VerifiedOutput` produced for that exact invocation. A model tool call remains
a proposal. The bridge binds its capability and scalar arguments to the
current connector inventory before invoking `ConnectorApplicationPort`.

`ToolDefinition::from_connector_schema` constructs the model-visible closed
object schema from supported connector metadata. Supported fields are strings,
integers and booleans, with explicit required fields and no additional
properties. Unsupported validation keywords and nested schemas are rejected.
Values must also satisfy existing native `TypedValue` limits: integers fit
`i64`, and strings satisfy `NonEmptyText` (including its length and control
character rules). Empty or whitespace-only strings and other unrepresentable
values are rejected before invocation without trimming or coercion.
The connector, scope, inventory revision, complete raw argument schema and its
content digest are included in the actual tool definition and therefore the
invocation identity. The shared validator checks the raw content pin and its
strictly parsed argument type separately; descriptive annotations and ordering
of required fields never substitute a normalized digest for the mapped pin. Ordinary
tools use `connector_inventory: None`, which is omitted from serialization.

The `CanonicalConnectorProposalBinder` uses the existing invocation admission
owner and a trusted `ConnectorSessionBindingPort`. That owner supplies the
current target, step and execution coordinates. The task comes from the
invocation. The model cannot select these coordinates. Discovery supplies the
current capability mapping and operation class; it supplies no permission.

`ConnectorProposalDispatcher` runs proposals in order. Each request includes
the complete invocation identity and proposal ordinal in its correlation
digest. Repeated proposals remain distinct effect instances. Each invocation
crosses the existing connector policy and immediate pre-effect checks. Native
policy inputs come from `PolicyApplication.step_input`; the caller must
authenticate current facts and prove the requested capability belongs to that
native step's capability closure.

The dispatcher retains validated earlier observations if a later proposal
fails, identifies the failure ordinal and stops subsequent calls. It validates
returned observations using the shared `ConnectorResult` owner. A returned
observation, including a provider acknowledgement, does not prove the task goal.
Independent scoped readback and the existing goal verifier remain necessary.

This module does not persist a session, create consent, reconstruct a historical
invocation or install a production operator policy source. Its controlled MCP
integration test qualifies the application bridge. Installed session execution,
durable consent continuation, snapshots, recovery and full #230/#274 acceptance
require their own complete runtime evidence.
