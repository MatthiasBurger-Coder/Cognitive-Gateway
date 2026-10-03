# CG-23 learned procedure validation, replay and simulation

Issue [#214](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/214).
The domain evaluation contract is in `gateway-domain::procedure_evaluation`;
`gateway-application::procedure_evaluation::simulate_step` captures decisions
from the existing Process and Policy engines without committing transitions or
invoking capabilities.

## Inputs and authority

An evaluation pins the candidate, complete immutable procedure version and digest,
dataset identity/version/digest, evaluator version and explicit runtime version.
Each historical case includes a content-hashed snapshot with explicit evaluation
time, project scope, typed fingerprint signals, observation/evidence status,
revalidated source-experience eligibility, ordered process references, capability
availability, policy/process decisions and traces, execution outcome and
verification status. Historical successes and failures are supplied independently;
failed history is never relabeled as success by the simulator.

The capture application calls `TransitionEvaluator` and `PolicyEngine`. It requires
an authorized activity containing the declared capability and the declared policy
in the authority set. All other applicable policies still participate. Missing
capabilities, unresolved process events, missing authorization and policy denial
remain refusals. Inputs and process instances are borrowed and never changed.
Adapters authenticate policy authority, source truth, eligibility and freshness at
capture time. The explicit status values describe that captured state, rather than
reading a clock or querying live sources during replay.

The CLI accepts operator-supplied snapshots for offline simulation. Fixture or model
claims of `PRESENT`/allowed do not authenticate live evidence or grant permission.
Hashes detect altered content; they are not signatures. Successful simulation is
an evaluation prerequisite, never approval or runtime authorization. Promotion
approval is implemented separately in [CG-24](procedure-promotion.md); active reflex
execution is implemented in the [CG-25 reflex coordinator](reflex-engine.md).

## Evaluation behavior

Static validation checks the procedure's canonical contract and digest, dataset
schema/version, unique case IDs, expected outcome consistency and every snapshot
digest. Replay rejects conflicting operating modes and checks scope and every required fingerprint signal, observations,
evidence, source eligibility, exact process identity/version/digest and step binding,
capability availability and both process and policy decisions. Missing, stale,
conflicting and failed inputs fail closed. Every step is checked before activation.
An execution failure stops simulated execution and records completed steps;
verification failure records activation and completed steps but never success.
Every refusal/failure records the procedure's declared fallback.

The counterfactual suite starts from a verified positive case and changes one input
at a time: remove each fingerprint signal, change scope, remove each observation,
mark each evidence item missing/stale/conflicting, remove each experience basis,
then deny process/policy, remove capability, change process version and inject
execution failure at each step, and fail each verification requirement. It retains
the original historical inputs and assigns distinct snapshot IDs and new digests.

Evaluation requires coverage of historical success and failure plus all negative
scenario categories. Incomplete coverage fails evaluation. Each case must match its
exact expected outcome: refusal for an unrelated earlier problem does not pass.
Activation on a case expected to refuse before execution is a **critical false
positive**, even if later execution or verification fails.

Cases and results are sorted by ID, map/set inputs are canonical and no clock,
randomness, network or runtime executes during replay. Reordered equivalent datasets
produce byte-identical bundles. A bundle contains the procedure, dataset, report and
SHA-256 of their serialized tuple. Validation reproduces the entire report and
digest, detecting report, manifest, input or procedure edits.

`ProcedureLifecycle::apply` rejects advancement to `EVALUATED` without evidence.
`apply_evaluated` recomputes a passing bundle, checks the lifecycle's exact procedure
digest and requires the transition's decision reference to equal the bundle digest.
Existing audit history, actor, time and legal transition checks still apply.

## CLI and reproducible evidence

```sh
cg simulate --procedure tests/fixtures/procedure-evaluation-v1/procedure.json \
  --dataset tests/fixtures/procedure-evaluation-v1/historical.json \
  --runtime-version runtime-1 --json > bundle.json
cg replay --bundle bundle.json --json > replay.json
cmp bundle.json replay.json
```

`simulate` adds counterfactuals to each historical positive. `evaluate` evaluates
exactly the supplied dataset. `replay` validates and reproduces a complete bundle.
JSON output is a self-contained evidence bundle; human output uses the existing CLI
report renderer. Exit 11 retains the failed report, exit 3 reports invalid contracts
or altered artifacts, and exit 2 reports usage errors.

The CG-23 quality gate retains `cg23-evaluation.json` using the checked-in historical
fixture and Rust golden replay test. Domain tests cover deterministic replay,
critical false positives, wrong-reason refusals, incomplete/tampered/version-mismatched
bundles, multiple steps, verification failures and lifecycle admission. Application
tests exercise real Process/Policy decisions; CLI tests exercise simulation,
evaluation, replay, deterministic output, failure exit codes and tamper rejection.
