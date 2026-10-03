# CG-25 deterministic reflex engine

Issue [#216](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/216).
`gateway-application::reflex` implements a model-independent fast path over the
CG-24 promotion service, CG-10 context compiler, existing execution runtime and
CG-06 deterministic normalization/comparison contracts.

`ReflexEngine::run` returns a serializable `ReflexResult`. Its disposition is
`SUCCESS`, `FULL_COGNITIVE_PATH` or `STOPPED`; failures have stable enum names.
The driving host routes `FULL_COGNITIVE_PATH` to its normal cognitive coordinator,
with the trace and consumed dispatch count. This service never invokes a model.

## Matching and applicability

The engine loads the registry through its private trusted promotion service.
Only the exact ACTIVE `(id, version, digest)` pointer can match. Approved, canary,
deprecated, rolled-back and superseded versions cannot run here. There is no
legacy lifecycle or caller-supplied registry shortcut. A fingerprint must match
scope and every canonical typed signal exactly. A subset, superset, different
scope, absent match or multiple active matches returns to the cognitive path.

Every step, including a retry, reacquires the live situation and checks:

- Exact fingerprint, explicit blockers and the current ACTIVE registry pointer.
- Required observations and evidence, supporting lineage and a consistent known
  normalized state. Missing, conflicting, unsupported and unknown state fail closed.
- Required fact signals present in the live records and operating mode/capability
  signals consistent with the current authority inputs.
- Exact live records and normalized entries bound to the resolution snapshot.
- Exact process definition ID/version/digest, workflow policy and one declared
  capability bound to the current resolved plan step.
- Explicit plan prerequisites evaluated through deterministic conditions.
- Current Process eligibility and Policy authorization through
  `ContextApplication::compile_step`, with a mandatory process snapshot.

`ReflexInputs` is a trusted host boundary. It collects complete scoped observations,
assigns stable fact identities to their immutable meaning, supplies current
process/resolution/policy inputs and reports monotonic Unix time. Models and workers
must not receive this port, the promotion service or its authenticated authority.
A fingerprint or evidence ID supplied by a model is not source authentication.

The fast path deliberately requires observation/evidence occurrence timestamps to
be Unix-second strings. Unknown formats, absent timestamps, future times and ages
above the configured limit fail closed. Hosts normalize authenticated source times
at ingestion; the engine never guesses an opaque timestamp's meaning.

## Dispatch and verification

The first dispatch atomically reserves the exact ACTIVE version through CG-24.
A duplicate execution ID, revision conflict, revocation or unavailable registry
prevents dispatch. Retries and later steps recheck eligibility and compile fresh
inputs. Every attempt receives a distinct correlated execution ID and consumes one
iteration and one resource unit before calling the runtime.

`ReflexRuntime` extends the existing `ExecutionRuntimePort` with
`execute_bounded`, accepting an absolute exclusive deadline and one resource unit
for that attempt. The adapter must enforce cancellation and the resource bound
inside dispatch. The engine also rejects expired/regressing time before dispatch
and after return. Retry, iteration, elapsed-time and resource budgets are cumulative;
failed attempts never refund them. Only an explicit `RETRYABLE_FAILURE` can retry.
An uncertain transport result must be blocked rather than marked retryable.

A completed runtime status alone cannot establish success. Each step requires a
correlated, new scoped ingestion snapshot; its observations and required verification
evidence must have been produced during the dispatch. Evidence must support the
normalized completion/verification conditions. Desired-condition references use
CG-06 comparison; typed outcome conditions require a known supported subject and
an exact expected value where declared. Observation, evidence-acquisition and
conflict-resolution outcomes can establish a known supported subject without an
expected value. Unbounded/uninterpretable outcomes fail verification.

The final step additionally checks the complete desired expression, acceptance
criteria and constraints. All declared verification evidence must participate in
that step's completion/verification support. Explicit procedure fallback controls
post-dispatch failures (`STOP` or `RETURN_TO_PLANNER`). Unproven pre-dispatch
applicability always returns to the cognitive path.

The CG-24 journal records success only after verification. Execution, verification
and refusal outcomes are retained by exact procedure version. A failed outcome
append never returns success; the reservation remains available for trusted host
reconciliation. The trace retains fingerprint and version identity, evidence IDs,
source ingestion identities, compiled process/capability/policy decisions, execution
IDs, consumed budgets, verification lineage and fallback reason. It omits raw
observation values and external content.

## Reproducible checks

```sh
cargo test -p gateway-application --test context_application reflex_cases
CG25_REFLEX_OUTPUT=reflex.json cargo test -p gateway-application \
  --test context_application \
  reflex_cases::reflex_executes_verified_active_procedure_without_model -- --exact
```

Regression cases cover exact/near/ambiguous matching, inactive versions, missing
observations/evidence, stale/future/conflicting inputs, blockers, Policy denial,
wrong bindings, duplicate execution IDs, verification failure/reused snapshots,
retry/resource/iteration exhaustion, deadline expiry and clock regression.
