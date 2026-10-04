# CG-30 cognitive runtime evaluation and v0.3 release qualification

[Issue #221](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/221)
qualifies the adaptive runtime supplied by CG-21 through CG-29. Qualification
requires the complete existing quality gate and the CG-30 acceptance evidence on
one clean candidate commit. A development replay is useful evidence, but cannot
qualify a release. Publication and deployment remain separate operations.

## Acceptance suite and release thresholds

`gateway-application/tests/support/cg30.rs` integrates the existing application
services with scoped in-memory adapters and bounded fake execution. The test
`reflex_cases::cg30::cognitive_runtime_release_qualification` performs:

1. Admit and validate two successful experiences and one failure through governed
   memory, then inspect verified traces to detect a pattern. Retain the failure;
   derive the learned procedure from the detected candidate's actual lineage.
2. Evaluate positive, historical failure and generated counterfactual cases.
   Recompute bundles, replay reversed datasets and reject altered evaluation.
3. Submit promotion commands through authenticated fixture authority. Reserve
   a canary, prove activation with a pending reservation fails, compile the real
   Process/Policy context, execute a bounded fake runtime and verify correlated,
   fresh evidence against the desired state before recording the canary outcome.
4. Activate and execute the reflex through the production engine. Qualify a second
   immutable procedure version, supersede the first, execute the successor,
   roll it back and execute the restored exact version. Preserve both journals
   and evaluation histories.
5. Measure exact, novel, scope-mismatched, ambiguous, inactive, blocked,
   missing/stale/future/conflicting evidence and Process/Policy bypass cases.
   Activation means **any runtime dispatch**, including one that later fails.
6. Evaluate ten routing cases, then exercise actual model failure/fallback,
   policy revocation before fallback, complete model outage and model identity
   mismatch through `route_and_execute` and the real context compiler.
7. Inject worker hard failure, bounded retry exhaustion, reused verification,
   outcome-store failure and reservation conflict. Replay retained journals;
   retry duplicate execution IDs and prove they never redispatch. An outcome
   storage failure retains a pending reservation, requiring trusted reconciliation.

The versioned fixture policy is
[`tests/fixtures/epic03-v0.3/policy.json`](../tests/fixtures/epic03-v0.3/policy.json).
It requires zero false positives among 11 negative cases, zero false negatives
among one positive case, all ten expected routing choices and zero constraint
violations. Every negative classification returns `FULL_COGNITIVE_PATH` with
zero runtime calls. Worker retries consume at most two dispatches. Every successful
fast-path proof includes procedure/fingerprint, source ingestion, evidence,
Process/Policy/capability, execution identity, budget and verification provenance.
These are fixture regression thresholds with explicit denominators, not estimates
of population error rates.

## Requirement-to-evidence report

| Acceptance criterion | Retained evidence and automated proof |
| --- | --- |
| Experience → pattern → candidate → evaluation → promotion → reflex | `cg30-qualification.json`: patterns, complete lifecycle journal, three reflex proofs |
| Novel/ambiguous fallback and measured false activation | CG-30 classification cases and confusion matrix; exact refusal reasons asserted by Rust |
| Process/Policy bypass fails | CG-30 policy denial and incorrect workflow binding; existing context and policy gates; routed fallback recompiles current authority |
| Missing/stale/conflicting evidence fails closed | CG-30 classification traces; CG-23 counterfactual evaluation bundles |
| Model unavailable/failing fallback | CG-30 routing matrix and execution telemetry; existing `cognitive_router_execution` tests |
| Worker failure/retry consistency | CG-30 failure cases, journal replay, duplicate rejection and pending outcome proof; CG-28A scheduler coverage gate |
| Canary, supersession and rollback | CG-30 verified fake-runtime canaries and post-rollback reflex; `cg24-promotion.json` and inspection |
| Replay/counterfactual regression | Every CG-30 retained bundle recomputes identically after case reordering and rejects tampering; `cg23-evaluation.json` |
| Model upgrade safety | Unqualified replacement routing refusal and actual identity mismatch in CG-30; CG-27 qualification lifecycle tests; `cg28-learning.json` and CG-28 upgrade-impact/requalification tests |
| Explainability/provenance | CG-30 stage/field assertions for every successful fast path; full routing requests, frozen snapshots, alternatives and execution provenance |
| Reproduction versions/digests/configuration | Candidate revision, source and artifact SHA-256 maps, toolchain, commands, worktree patch in `summary.json`; CG-30 policy, budgets, procedure versions/digests and evaluation manifests |
| Architecture/coverage gates | Full `quality-gates.json` manifest, all logs, raw `cg16-coverage.json`, existing domain/resolver/policy/context/retrieval/evaluation/promotion/learning/scheduler gates |
| Latency/resource/cost evidence | Per-case host wall time and min/median/p95/max; dispatch counts and cumulative routed cost/latency reservations, zero external model calls; CG-27 fixture benchmark and separate recorded CPU reference evidence |

## Reproduction and release admission

Development replay (output must use an absolute path because Cargo sets the test
working directory to the crate):

```sh
mkdir -p target/cg30-development
CG30_QUALIFICATION_OUTPUT="$PWD/target/cg30-development/cg30-qualification.json" \
  cargo test -p gateway-application --test context_application --locked \
  reflex_cases::cg30::cognitive_runtime_release_qualification -- --exact
python3 scripts/qualify-cg30.py --metrics target/cg30-development/cg30-qualification.json
```

Release qualification on the clean candidate, using a fresh evidence directory:

```sh
python3 scripts/quality-gate.py --output target/v0.3-release-evidence
python3 scripts/qualify-cg30.py --bundle target/v0.3-release-evidence \
  --output target/v0.3-release-report.json
```

The complete gate retains `cg30-qualification.json`; metrics admission runs as
its final gate. The second command checks the candidate is still clean, its exact
revision and source digests match, every current gate command passed, required
artifacts exist, every retained artifact hash matches and CG-30 measurements meet
policy. Missing, malformed, altered, partial, dirty or failed bundles exit nonzero.
The report is written with exclusive creation and never overwrites prior evidence.
A successful report now says `QUALIFIED_REFERENCE_RUNTIME_SCOPE` and additionally
admits the [complete ML/durable-runtime acceptance](epic-03-complete-acceptance.md). The generic gate summary retains
its historical `declarative-v0.1` scope label; the v0.3 report explicitly binds the
expanded full manifest and its CG-30 artifact. Hashes establish integrity and
reproduction bindings, not authentication of an untrusted evidence producer.

## Measurement limits and release decision

The suite makes no network/model calls. Its injected model reports and candidate
cost/latency fields measure routing accounting, not inference performance or
monetary expenditure. Host wall time includes governance replay and test dispatch;
no hardware SLA is inferred or silently gated. Memory utilization is explicitly
unmeasured (`null`). The CG-27 fixture benchmark and previously recorded CPU model
reference must remain distinct from measurements on the v0.3 deployment candidate.
The supplementary CPU acceptance now measures real CPU training/inference and
container execution on the candidate host. Other model artifacts, GPUs and production
workloads require their own live qualification.

The implementation and local fixture checks do not constitute a clean candidate
release decision. The reviewable release evidence is the complete gate directory
and the revision-bound v0.3 report produced above. A failed established gate blocks
qualification; it must be repaired without lowering its threshold.
