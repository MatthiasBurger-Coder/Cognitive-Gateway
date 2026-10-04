# EPIC-03 complete acceptance: adaptive reference runtime

Scope: [EPIC-03 #114](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/114),
CG-21 through CG-30 and the
[ML lifecycle supplement](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/114#issuecomment-5493295170).
This acceptance covers the model-independent core, a concrete CPU binary-classification
adapter, durable PostgreSQL coordinator journals and an isolated container worker.
The final decision is produced by the complete gate and revision-bound qualification
report, rather than by the closed status of the ten implementation issues.

## Requirements and executable evidence

| Requirement | Implementation / retained proof |
| --- | --- |
| Experience, knowledge and execution authority stay distinct | CG-21 typed contracts; CG-22 governed memory and trace admission; original CG-30 lifecycle |
| Repeated experience nominates candidates without auto-promotion | CG-22 pattern detection and CG-30 candidate lineage; CG-24 authenticated promotion commands |
| Immutable, auditable learned procedures and explicit evaluation | CG-23 replay/counterfactual bundles; CG-24 registry journals and version/digest checks |
| Rejection, approval, canary, activation, deprecation, supersession, rollback | CG-24 lifecycle tests; original CG-30 canary/successor/restored-reflex proof |
| Explicit applicability; missing/stale/conflicting evidence fails closed | CG-25/CG-30 negative classification cases and zero-dispatch assertions |
| Process/Policy authority, verification and cognitive fallback | CG-25 coordination, CG-26 recompilation before fallback, CG-30 denial/failure suite |
| Deterministic operation without models; replaceable adapters | Existing core tests, CG-26 router and local inference port; CPU adapter is outbound infrastructure |
| Actual model identity, upgrade safety and explainable decisions | Immutable artifacts, CG-26 routing traces, CG-28 upgrade-impact/re-certification and code-digest refusal |
| Validated learning signals and explicit labels | CG-28 source revalidation; real CPU test ingests 96 verified positive/negative memory records |
| Reproducible splits with duplicate/group/time leakage prevention | CG-28 manifest; inner CV groups also bind source/example families; chronological validation/test separation |
| Versioned feature schema, extraction, selection and transformation | Trusted `LearningFeaturePort`; train-only variance selection and normalization; fold feature digests and raw source lineage |
| Baseline and candidate training; bounded Grid/Random search | Majority baseline and nearest-centroid CPU candidate; seeded search space, trial budget, F1 objective and explicit stop rule |
| Appropriate cross-validation; final test excluded from tuning | Group and forward chronological CV; train/validation-only subprocess; separate Test-only evaluation and overlap refusal |
| Task metrics, calibration and safety profiles | Binary confusion matrix, precision/recall/F1/specificity/MCC/Brier/ECE; regression and ranking metric profiles; validation-only temperature calibration; held-out floors/ceilings |
| Comparison against baseline and prior release | Versioned evaluation profile; held-out baseline gate; second concrete CPU release evaluates the exact pinned predecessor |
| Digest/version experiment lineage and reproduction | Dataset/recipe/split/source/code/environment/model digests; CV fit/score IDs, seeds and selected parameters; reversed-input replay |
| Drift, OOD and governed recertification | Missing/nonfinite features refuse; OOD abstains; validated outcome/feature monitoring proposes re-evaluate/retrain/rollback without changing authority |
| Offline training and production inference separated | Existing CG-28 authorization; cleared subprocess environment; production local inference port has no training operation |
| Durable model release, restart and actual inference rollback | `DurableModelReleases`, qualification/authority replay under PostgreSQL row lock; actual PostgreSQL restart; inference versions 2 → 3 → 2; revocation refusal |
| Frozen worker inputs, bounded retries, fencing and idempotency | CG-29 contracts and `DurableScheduler`; restart recovery, stale lease rejection, duplicate result rejection and competing-coordinator single claim |
| Trace/scope/provenance and replaceable orchestration | Dedicated project journals, typed snapshots, correlated results; transport/orchestrator SDKs stay outside the core |
| Worker time, CPU, memory and output enforcement | Linux bounded subprocess; real timeout/CPU/memory/output failures; container with no network, read-only filesystem, unprivileged UID, no credentials/mounts, 256 MiB/one CPU/32 process bounds |
| End-to-end rollback/fallback and performance evidence | Original CG-30 regression plus `epic03-live-release.json`, `epic03-durable-workers.json`, `epic03-worker-container.json` and measured `epic03-ml-experiment.json` |
| Existing architecture/coverage standards | Every established gate remains required; ≥95% measured coverage per new Rust module and Python pipeline; failed gates block release admission |

## Reproduction and final decision

On the clean candidate commit, run:

```sh
CARGO_BUILD_JOBS=1 python3 scripts/quality-gate.py --output target/epic03-complete-evidence
python3 scripts/qualify-cg30.py --bundle target/epic03-complete-evidence \
  --output target/epic03-complete-release-report.json
```

The runner provisions and removes a dedicated loopback-only PostgreSQL test container.
It records its immutable image identity without writing credentials into evidence.
Ignored live database tests are explicitly admitted into the workspace coverage run.
The additional acceptance gate builds and removes its own container worker image.
These operations require Linux `prlimit`/`timeout`, Python, Rust/LLVM coverage and Docker.
They do not touch existing application databases, model registries or running deployments.

`QUALIFIED_REFERENCE_RUNTIME_SCOPE` means every current gate passed on one clean
Git revision; source/artifact hashes match; original CG-30 metrics pass; and the
supplementary ML, actual inference rollback, PostgreSQL restart, worker consistency,
container isolation and coverage proofs pass. Any missing/altered/failed bundle is
rejected. Publication and deployment are separate operations.

## Runtime composition and retained boundaries

Trusted hosts compose `CpuOfflineAdapter` with governed memory, evidence verification,
a snapshot-backed `LearningFeaturePort` and `OfflineAuthorizationPort`. Feature
adapters must authenticate pre-decision values, source revision, sensitivity and
eligibility; outcome labels cannot be supplied as predictors. The CPU fixture is
synthetic health classification, not a trained universal reasoning model.

`ModelRecoveryAuthority` revalidates retained training/evaluation evidence and
original approvals. JSON/digests do not authenticate these decisions. Its implementation
must handle revocation and authenticate canary observations. `DurableModelReleases`
replays those checks before every mutation/inference selection; a PostgreSQL transaction
commits the journal and active routing projection together. `CpuReleaseInference`
resolves the current active version per call and refuses changed artifacts or code.
Rollback thus affects the next inference without restarting or retaining a stale cache.

Database credentials belong exclusively to trusted coordinators. Store scope comes
from authenticated host context. `DurableScheduler` transacts complete command journals,
including fencing tokens, budgets, results and idempotency history; failed mutations
roll back. Each journal is bounded to 4,096 commands and 32 MiB; saturation returns
backpressure rather than discarding deduplication history. The reference requires an
explicit retention/lifecycle decision before that bound is reached. Model journals
retain at most 256 immutable versions and 4,096 lifecycle events.

`ProcessCognitiveWorker` receives a copied snapshot, a host-pinned script revision and
a host clock. It enforces the remaining lease interval, address-space/output limits
and CPU seconds (`compute_units` in this Linux adapter). The isolated container uses
the same proposal-only CPU implementation. Worker results still require the coordinator's
lease, scope, provenance and budget checks; no worker receives Process/Policy or
promotion ports. Swarm/Kubernetes transport remains an optional adapter choice.

## Measurement interpretation

The original reflex qualification retains its fake action/model dispatch scope.
The supplementary CPU runs train and invoke a real nearest-centroid model. CPU time,
wall time and peak process RSS are measured on the executing host; RSS includes the
Python runtime and imports. Container wall time includes startup. The measurements
use named synthetic data and establish reproducibility and regression behavior, not
population accuracy or a production SLA. GPU and external-model measurements are
outside this CPU reference profile. Deployment-specific model families, hardware and
real project datasets must pass their own versioned profiles before rollout.
