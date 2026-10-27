# Trusted connector admission — #369

`gateway_application::connector_policy::Cg09ConnectorAdmission` adapts outbound
connector admission to the existing CG-09 `PolicyEngine`. It introduces no
provider SDK, parallel policy engine or permissive default.

## Trusted source contract

A host supplies `ConnectorPolicySource`. Each lookup must authenticate the actor
and current project scope, load current policy/process facts and consent, and
bind them to the exact request and optional source/resource. The binding covers
connector, capability, operation class, typed arguments, inventory revision,
task, step, execution and correlation, policy identity/revision, consent receipt,
aggregate reservation and absolute deadline. The source owns current clock,
expiry, revocation and durable reservation checks. Missing or ambiguous authority
must fail closed. A model, discovered tool or mapping hint cannot supply consent.

`ConnectorPolicySnapshot` requires native step resolution, prerequisites,
constraints, preconditions, mode/profile and authenticated `StepFacts`. Production
composition must obtain them from the existing policy/resolution owners; labels
or a desired goal cannot substitute for canonical capability membership.

| Connector class | CG-09 class | Additional requirement |
| --- | --- | --- |
| READ / SEARCH / INSPECT | Inspect | Current canonical authorization; mapping/policy may additionally require consent. |
| MUTATE | Mutate | Existing CG-09 consent rules and authenticated current consent. |
| ADMIN | Mutate | Explicit membership in the host's administrative capability set, plus mutation authorization/consent. |

The adapter rejects unknown capabilities, class/step mismatches, explicit denials,
missing evidence or prerequisites, and every non-Allow policy outcome. Consent
metadata only increases requirements. It cannot grant consent or turn an ordinary
mutation approval into administrative permission.

## Canonical resolved-step inputs — #399

`PolicyApplication::step_input(resolved, context, step)` and
`step_inputs(resolved, context)` expose the existing native input assembly.
They validate the complete resolution and matching context before returning
owned `ResolvedStepPolicyInput` data. Whole-plan `evaluate` uses the same private
assembly, then passes each input to the existing `PolicyEngine`.

A connector source can reuse the canonical capability closure, process readiness,
structural prerequisites, constraints, preconditions and exact step facts rather
than rebuilding them or inventing default approval facts. Ambiguous resolutions
have no selected canonical capabilities. Foreign steps/facts, changed basis,
mode/profile and forged resolutions retain the existing validation failures.

The inputs are data, not decisions, current-authority witnesses or grants.
They do not authenticate caller-supplied `StepFacts` and perform no provider I/O.
A production source must obtain fresh authenticated facts at each authority
boundary and check that its requested capability belongs to this exact native
step. It must still enforce request/scope/inventory, consent, reservation and
deadline bindings through the admission contract above.

## Dispatch and audit boundaries

Authorization returns a request-bound `ConnectorDispatch`. That public data is
not an independently reusable grant. Immediately before transport, the host
calls `revalidate`, which obtains current facts, reruns CG-09 and checks exact
policy/consent/reservation/deadline bindings. The trusted source must also reject
request/source substitution, expired or revoked consent and changed inventory.

`ConnectorPolicyEvaluation` exposes immutable captured facts, the original
request/source, optional native policy report, effective outcome and stage to an
audit sink. Stages distinguish initial authorization and immediate pre-effect
revalidation, retaining the original dispatch. Contract rejection does not invent
a policy report. A policy Allow with a changed dispatch binding is still recorded
as an effective denial. Audit failure denies; pre-effect admission rechecks current
facts after the audit callback to catch revocation during auditing.

`validate_current_dispatch` performs the same current checks without writing
another audit record or acquiring authorization. Legacy admission implementations
inherit a denial for this hook. `preview_connector_policy` is pure explanatory
data: it can identify a consent requirement but cannot create a dispatch/grant.
Failure projections remain deterministic, content-free and `NotDispatched`.

## Qualification and scope

The adapter matrix exercises all operation classes, consent and administrative
requirements, native prerequisites/evidence, changed bindings, audit callback
revocation/failure and repeated audit-free liveness checks. The existing MCP host suite checks credential boundaries;
a dedicated contract regression proves that a legacy
admission source cannot obtain a current grant through a fallback callback.

The retained [evidence](evidence/EPIC-07.13-connector-admission.json) records the
candidate source hashes, commands, measured coverage and independent review.
The CI connector-admission gate requires at least 95% measured line coverage
for both `connector_policy.rs` and its `connectors.rs` contract.

The bounded #369 contract is now composed by the installed read-only
`ObservedGoalRuntime` through its current native `GoalSource` and durable
PostgreSQL claim/audit boundary. See [#230 integration and acceptance](connector-policy-integration.md)
for the complete enforcement matrix. General installed model proposals,
mutation consent continuation, retries and live provider effects remain with
their existing owners and #274. Neither parent epic is closed by this boundary.
