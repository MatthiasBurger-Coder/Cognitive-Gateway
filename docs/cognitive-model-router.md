# Explainable cognitive model router (CG-26)

CG-26 #217 supplies provider independent advisory selection and bounded dispatch
coordination. It does not install or require a model runtime. CG-27 owns local
serving and model qualification; EPIC-06 owns inference implementations.

## Contracts and ownership

`gateway-domain::cognitive_routing` defines version 1.0 requests, route contracts,
model identities, capability snapshots, hard rejection codes and replay artifacts.
All wire inputs reject unknown fields. Deserialization is structural; request and
snapshot admission validate versions, nonempty metadata, unique candidate IDs,
model/procedure identity consistency and local model privacy before selection.

`gateway-registry::model_capabilities::ModelCapabilityRegistry` admits an immutable,
inspectable snapshot. `ModelCapabilityPort` decouples application selection from
storage. Configuration digest, cost unit, model version/artifact digest,
runtime/version, quantization and qualified/available flags are explicit host
configuration. Qualification is supplied by trusted configuration, never inferred
from model output. Refresh/requalification produces a new snapshot; in-flight
operations retain the original one.

## Selection precedence

The host supplies task class (resolution, classification, extraction, ranking,
reasoning or verification), ordinal novelty/reasoning depth/uncertainty/evidence
completeness, exact input and output contracts, proven deterministic sufficiency,
reflex applicability, privacy boundary, available hardware and cumulative bounds.
Low, medium and high are ordered levels, not probabilistic confidence scores.
Evidence completeness must meet a candidate's minimum; the other three levels
must not exceed its declared maxima. Missing evidence is never manufactured by
selection.

All hard filters run first: qualification, availability, task, cognitive levels,
input/output compatibility, privacy, hardware, cost and latency. Cost units must
match exactly; the router performs no implicit currency conversion. Privacy is
ordered on-device, private network, external. Local SLM/LxM contracts require
on-device processing. A candidate must declare a supported hardware requirement
that the host explicitly supplies.

Eligible routes are ranked lexicographically:

1. Deterministic resolver (only when deterministic sufficiency is proven).
2. Approved reflex (only when applicability is proven).
3. Local SLM.
4. Specialized local model/LxM.
5. Strong LLM.

Within a kind, lower declared cost, lower latency and lexical candidate ID break
ties. This tuple is the route score; there are no mutable learned weights or
floating point calculations. Registry input order cannot change the result.
Every candidate appears in the explanation with all failed filters; eligible
losers carry `LOWER_PRECEDENCE`. No compatible candidate is an explicit outcome,
including an empty registry. A deterministic-only registry needs no model identity
or external inference service.

## Execution and fallback

`route_and_execute` obtains one snapshot and dispatches at most `max_attempts`.
Each attempt reserves the selected candidate's conservative cost/latency bounds.
Failed calls consume the reservation too. Measured usage can increase accounting
but cannot reduce the reservation. A report exceeding its bound, overflowing
arithmetic, mismatching cost unit/model identity, or omitting a successful output
reference ends the operation; these faults cannot obtain a fresh fallback budget.
Runtime adapters must enforce their call deadline and cancel overlong operations;
the coordinator cannot preempt a synchronous port. Preparation overhead is outside
these per-dispatch latency bounds.

Unavailable runtimes, model failures and invalid outputs fall back to the next
compatible route, with all original constraints and the remaining cumulative
budget. Each candidate is attempted once. There is no mandatory escalation to an
external model, no unlimited retries, and no fallback that weakens the output
contract. Exhausted bounds and attempt limits are explicit telemetry outcomes.

Before **every** dispatch, including fallback, the trusted runtime's `prepare`
method must call CG-10 compilation with current Process and Policy inputs. Its
return type is the read-only `CompiledStep`, which cannot be constructed by a
model. The application compares the compiled output contract with the requested
contract before dispatch. Denial or stale authority ends routing. The host must
bind the requested task/input contract to that compiled step. Reflex adapters
must run through CG-25's ACTIVE procedure, evidence, applicability, authorization,
budget and verification checks; a router flag does not replace those checks.
Model outputs remain proposals. Existing execution/verification paths alone own
capabilities, transitions and evidence acceptance. A compiled step remains a
snapshot; adapters recheck live authority at any later mutation boundary.

## Explanation and telemetry

`RoutingTelemetry::to_json` exports each decision, complete capability profiles,
configuration digest, request decision reference, rejected alternatives, actual
participating model identities (including failed calls), per-attempt outcome and
usage, aggregate conservative usage, and terminal disposition. Execution
provenance ties each attempted candidate to the compiled context ID and resolution
basis, including plan, situation, scope and Process state/catalog fingerprints.
Fallback decisions record previously attempted candidates and remaining budgets.
These are public decision records, not prompts or internal reasoning text. Hosts
should use reference-only output schemas in public configuration and apply their
normal disclosure policy when storing request/configuration artifacts.

## Verification

Offline tests exercise every hard filter, all five route kinds, deterministic
registry ordering and tie breaks, invalid contracts, inspectable versions, no-model
execution, failure fallback and conservative accounting, exhausted bounds,
privacy-preserving fallback, fresh Process/Policy denial, output mismatch and
incorrect runtime identity/usage reports. No external model is needed. The shared
fixture is `tests/fixtures/cognitive-routing.rs`; domain and application tests
validate their respective boundaries independently.
