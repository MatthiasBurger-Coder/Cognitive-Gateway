# CG-28 governed learning signals and offline training

Issue: [#219](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/219).
Contracts: `gateway-domain::offline_learning`. Application boundaries:
`gateway-application::offline_learning` and `model_releases`.

## Signal admission

`LearningSignal` v1 contains reference-only provenance: the exact CG-18 memory
revision and revocation sequence, validated trace digest, validation, label basis,
evaluation and evidence references, observation time, producing model version,
normalized example digest and episode/near-duplicate leakage group. Measurements
cover success/failure, quality gate, retries, repairs, latency in milliseconds,
cost in adapter-defined integer units, human correction, policy denial, rollback
and recurrence. Boolean measures use 0/1; counts are unsigned. Exactly one of
success/failure must equal one. A validated failure is useful negative evidence.

`admit_signal` independently revalidates current learning eligibility and matches
outcome, provenance, time, validation and label basis to governed memory. The
`LearningEvidencePort` must verify the **entire signal** against a trusted
validated execution/evaluation trace, including measurements, normalized example,
leakage family and model identity. Merely finding an evidence ID, accepting a
model judgment, or deserializing a purportedly verified event is insufficient.
The port can revoke previously verified evidence. Admission fails closed.

The schemas are [learning-signal.schema.json](../schemas/learning-signal.schema.json)
and [offline-learning.schema.json](../schemas/offline-learning.schema.json).
Schema validation checks shape; application admission checks truth and eligibility.
Neither signal values nor their JSON representation provide capabilities, process
transitions, procedure promotion or model release authority.

## Reproducible datasets and recipes

`assemble_dataset` takes an explicit project scope, dataset ID/version, source
revision, builder version, reserved example digests, bounded input count and
Train/Validation/Test assignments. It sorts signal IDs deterministically, requires
all three nonempty splits, rejects duplicate signal IDs, and deduplicates by the
validated normalized example digest. Duplicate sources remain in the manifest
for complete provenance and subsequent revocation checks. Conflicting outcome
labels for the same example fail admission.

Trace IDs, trace content digests, normalized example digests and leakage families
must each remain within one split. Reserved examples are excluded from the entire
dataset. Digest comparisons for deduplication and reserved examples ignore hex
case. The source adapter must assign a common leakage family to related tasks,
episodes and near duplicates; the builder cannot infer semantic equivalence from
opaque reference-only content. Project scope is checked on every source. No
cross-project merge or consent override is supplied.

The manifest digest is SHA-256 over compact Rust serde JSON of all manifest
fields, in declaration order, with `digest` replaced by 64 zeroes. Maps and sets
are ordered, rows/duplicate rows use canonical signal-ID order, and timestamps
are explicit inputs. This encoding is the v1 canonical format; arbitrary JSON
key reordering is not an alternative digest format. Rebuilding with identical
inputs, configuration and assembly time produces the same version/digest.
Recipes pin ID/version, base model artifact, trainer/evaluator versions,
environment digest, seed and ordered parameters. The recipe digest is SHA-256
over its compact typed JSON. Seeds and environment pins support reproducibility;
a concrete trainer remains responsible for deterministic algorithms and reporting
any hardware/backend nondeterminism.

`revalidate_dataset` verifies digest, ordering, splits, deduplication and every
source, including dropped duplicates, immediately before training and evaluation.
Forgotten, expired, refreshed or invalidated memory revokes existing exports.
Historical release metadata is retained; it cannot recover forgotten payloads.

## Offline boundary and evaluation

The explicit `train_offline` and `evaluate_offline` APIs require an
`OfflineAuthorizationPort` approval bound to job, project, dataset digest, recipe
digest and base model. A production caller has no default grant. The host adapter
must deny production runtime, verify grant expiry/revocation, and provision an
isolated worker without production credentials or routing access. Controlled
reference resolution must enforce source sensitivity and project scope. Training
receives only Train/Validation rows; evaluation receives only Test rows.

`OfflineTrainingPort` returns an evidence-bound run with exact job/dataset/recipe
binding and an immutable candidate artifact. `OfflineEvaluationPort` must verify
training evidence, load that exact candidate, and check expected labels against
validated sources. Evaluation must return each held-out signal ID exactly once.
The application computes CG-20 precision, recall, sufficiency, provenance,
freshness, contamination, budget and token efficiency, and applies every floor
and baseline regression bound. Missing denominators, absent cases and regressions
fail qualification. Metadata retains evaluator, baseline/policy digest, evidence,
case counts, latency, cost and explicit times. A passing evaluation yields an
opaque `QualifiedModel`; deserialized reports cannot construct this value.

There is no training route in the daemon's production inference adapter or the
CG-27 model service. The concrete `CpuOfflineAdapter` adds train-only feature fitting, bounded Grid/Random search,
group/chronological CV, validation calibration and separate held-out evaluation. Its
CPU classifier and isolated container are qualified through the [complete EPIC-03
acceptance](epic-03-complete-acceptance.md). Training remains explicitly authorized.

## Immutable release, canary and rollback

`ModelReleaseRegistry` is a process-local reference coordinator for one project.
Registration consumes a qualified model and constructs an immutable release
manifest, including recipe, training run, evaluation, bounded canary and exact
currently active predecessor. ID/version reuse is rejected even if the artifact
digest changes. Its digest uses the same zero-digest canonical format as datasets.

Every registration, canary start, verified observation, activation and rollback
requires independent `ModelReleaseAuthority` approval of the exact event. Actor
and policy fields describe decisions; they do not authenticate them. Canary
observations additionally require verification of exact candidate, cohort, time,
counts and unique underlying requests. Observation/evidence IDs cannot be replayed.
Counts use checked arithmetic and a request ceiling. Activation requires enough
successes, failures within budget and an unchanged predecessor. Failed operations
leave routing and the journal unchanged.

Operator rollback procedure:

1. Inspect the immutable release and its recorded predecessor and observations.
2. Obtain an independent policy decision for the exact rollback event/digest.
3. Invoke `rollback` on the active or canary candidate. An active rollback restores
   only its exact previously active predecessor. A first-release rollback disables
   routing. A canary rollback preserves the incumbent. A rolled-back version
   cannot be activated again.
4. Persist the append-only event and routing change atomically in the host's
   deployment store before changing production routing. Retain both manifests
   and all evaluation and canary evidence.

`DurableModelReleases` now persists this registry in PostgreSQL and reconstructs it
through the same governed commands and `ModelRecoveryAuthority`. `CpuReleaseInference`
selects the exact active durable release on every call; actual database restart and
version 2 → 3 → 2 inference rollback are tested. Hosts authenticate recovery evidence
and project scope. These APIs do not modify CG-24 or Process/Policy state.

## Upgrade impact

`upgrade_impact` compares exact immutable model dependencies in one project and
returns a deterministic artifact-ID ordered report. Embeddings and vector indexes
require re-embedding; learned procedures require re-certification; semantic
mappings, prompt caches and evaluation baselines require re-evaluation. Unrelated
model dependencies remain unaffected. Hosts must keep this inventory complete;
the report identifies work and does not perform or waive certification.

## Acceptance evidence

`cargo test -p gateway-daemon --test offline_learning --locked` exercises validated
negative/success admission, exact evidence matching, reference-only reproducible
exports, duplicate provenance, split leakage, reserved data, project isolation,
revocation/forgetting, tamper rejection before worker invocation, explicit offline
authorization, candidate binding, objective evaluation, immutable releases,
canary failure, supersession, rollback and derived-artifact impact.

The `CG28_LEARNING_OUTPUT` environment variable exports the synthetic release and
rollback proof from
`immutable_releases_canary_supersession_and_exact_rollback_keep_audit_history`.
The quality gate retains it as `cg28-learning.json` and requires 95% measured
line coverage for each new application module. This is synthetic boundary
and lifecycle evidence, not a measured claim about a trained production model.
