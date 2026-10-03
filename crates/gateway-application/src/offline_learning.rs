//! CG-28 admission, deterministic assembly and explicitly authorized offline jobs.
use crate::memory::{MemoryApplication, MemoryError, MemoryStore};
use gateway_domain::{
    ContentDigest, ContextScopeId, ReferenceId, UnixTimestamp,
    evaluation::{self, GoldenCase, ReleasePolicy},
    offline_learning::*,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LearningError {
    Memory(MemoryError),
    InvalidSignal,
    Unverified,
    ScopeMismatch,
    InvalidManifest,
    DuplicateIdentity,
    Leakage,
    EmptySplit,
    Revoked,
    Unauthorized,
    InvalidRun,
    Evaluation(evaluation::EvaluationError),
    InvalidRelease,
    InvalidTransition,
    Storage,
}
impl From<MemoryError> for LearningError {
    fn from(value: MemoryError) -> Self {
        Self::Memory(value)
    }
}

/// Trusted adapter: verify exact measurements, example identity, leakage family,
/// model identity and all evidence against a validated trace. Raw events, JSON,
/// model judgments and mere existence of an evidence ID are insufficient.
pub trait LearningEvidencePort {
    fn verify(&self, signal: &LearningSignal) -> Result<bool, LearningError>;
}

pub fn admit_signal<S: MemoryStore, E: LearningEvidencePort>(
    memory: &MemoryApplication<S>,
    evidence: &E,
    scope: &ContextScopeId,
    signal: &LearningSignal,
    at: UnixTimestamp,
) -> Result<(), LearningError> {
    if signal.memory.scope != *scope {
        return Err(LearningError::ScopeMismatch);
    }
    let success = signal
        .measurements
        .get(&SignalKind::Success)
        .copied()
        .unwrap_or(0);
    let failure = signal
        .measurements
        .get(&SignalKind::Failure)
        .copied()
        .unwrap_or(0);
    if signal.schema_version != OFFLINE_LEARNING_VERSION
        || signal.producer_model.version == 0
        || signal.evidence.is_empty()
        || signal.evidence.len() > 64
        || success > 1
        || failure > 1
        || success + failure != 1
        || [
            SignalKind::QualityGate,
            SignalKind::HumanCorrection,
            SignalKind::PolicyDenial,
            SignalKind::Rollback,
        ]
        .iter()
        .any(|kind| signal.measurements.get(kind).is_some_and(|v| *v > 1))
    {
        return Err(LearningError::InvalidSignal);
    }
    if !memory.revalidate_reference(&signal.memory, at)? {
        return Err(LearningError::Revoked);
    }
    let entry = memory
        .store()
        .get(scope, &signal.memory.id)?
        .ok_or(LearningError::Revoked)?;
    if entry.record.scope != *scope
        || entry.record.validate().is_err()
        || entry.record.provenance != signal.provenance
        || entry.record.observed_at != signal.observed_at
        || entry.record.validation.as_ref() != Some(&signal.validation)
        || entry.record.label_basis.as_ref() != Some(&signal.label_basis)
        || entry.record.outcome.as_ref().map(|x| x.as_str())
            != Some(if success == 1 { "SUCCESS" } else { "FAILURE" })
    {
        return Err(LearningError::InvalidSignal);
    }
    if !evidence.verify(signal)? {
        return Err(LearningError::Unverified);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DatasetConfiguration {
    pub id: ReferenceId,
    pub version: u32,
    pub scope: ContextScopeId,
    pub source_revision: ReferenceId,
    pub builder_version: ReferenceId,
    pub excluded_examples: BTreeSet<ContentDigest>,
    pub max_signals: usize,
}

/// Fails the whole request on an ineligible source; no silent loss of negatives.
/// Partition assignments are explicit and included in the version digest.
pub fn assemble_dataset<S: MemoryStore, E: LearningEvidencePort>(
    memory: &MemoryApplication<S>,
    evidence: &E,
    configuration: DatasetConfiguration,
    mut selections: Vec<DatasetRow>,
    at: UnixTimestamp,
) -> Result<DatasetManifest, LearningError> {
    if configuration.version == 0
        || configuration.max_signals == 0
        || selections.len() > configuration.max_signals
    {
        return Err(LearningError::InvalidManifest);
    }
    selections.sort_by(|a, b| a.signal.id.cmp(&b.signal.id));
    let mut identities = BTreeSet::new();
    let mut groups = BTreeMap::new();
    let mut traces = BTreeMap::new();
    let mut source_digests = BTreeMap::new();
    let mut examples = BTreeMap::new();
    let mut splits = BTreeSet::new();
    let mut rows = Vec::new();
    let mut duplicates = Vec::new();
    for row in selections {
        let signal = &row.signal;
        admit_signal(memory, evidence, &configuration.scope, signal, at)?;
        if !identities.insert(signal.id.clone()) {
            return Err(LearningError::DuplicateIdentity);
        }
        if configuration.excluded_examples.iter().any(|d| {
            d.as_str()
                .eq_ignore_ascii_case(signal.example_digest.as_str())
        }) || !same_split(&mut groups, signal.leakage_group.clone(), row.split)
            || !same_split(
                &mut traces,
                signal.memory.source_snapshot.clone(),
                row.split,
            )
            || !same_split(
                &mut source_digests,
                signal.memory.source_digest.as_str().to_lowercase(),
                row.split,
            )
        {
            return Err(LearningError::Leakage);
        }
        // Hex case must not defeat normalized-content deduplication.
        let key = signal.example_digest.as_str().to_lowercase();
        if let Some((split, success)) = examples.get(&key) {
            if *split != row.split {
                return Err(LearningError::Leakage);
            }
            if *success
                != signal
                    .measurements
                    .get(&SignalKind::Success)
                    .copied()
                    .unwrap_or(0)
            {
                return Err(LearningError::InvalidSignal);
            }
            duplicates.push(row);
        } else {
            examples.insert(
                key,
                (
                    row.split,
                    signal
                        .measurements
                        .get(&SignalKind::Success)
                        .copied()
                        .unwrap_or(0),
                ),
            );
            splits.insert(row.split);
            rows.push(row);
        }
    }
    if splits.len() != 3 {
        return Err(LearningError::EmptySplit);
    }
    let mut manifest = DatasetManifest {
        schema_version: OFFLINE_LEARNING_VERSION,
        id: configuration.id,
        version: configuration.version,
        scope: configuration.scope,
        source_revision: configuration.source_revision,
        assembled_at: at,
        builder_version: configuration.builder_version,
        excluded_examples: configuration.excluded_examples,
        rows,
        duplicates,
        digest: zero_digest(),
    };
    manifest.digest = manifest_digest(&manifest);
    Ok(manifest)
}
fn same_split<K: Ord>(map: &mut BTreeMap<K, DatasetSplit>, key: K, split: DatasetSplit) -> bool {
    map.insert(key, split)
        .is_none_or(|previous| previous == split)
}

/// Rebuilds from pinned rows, including duplicate sources; forgotten data cannot
/// be recovered from a historical manifest or by rolling back a model release.
pub fn revalidate_dataset<S: MemoryStore, E: LearningEvidencePort>(
    memory: &MemoryApplication<S>,
    evidence: &E,
    dataset: &DatasetManifest,
    at: UnixTimestamp,
) -> Result<(), LearningError> {
    if dataset.schema_version != OFFLINE_LEARNING_VERSION
        || dataset.assembled_at > at
        || dataset.digest != manifest_digest(dataset)
    {
        return Err(LearningError::InvalidManifest);
    }
    let mut selections = dataset.rows.clone();
    selections.extend(dataset.duplicates.clone());
    let rebuilt = assemble_dataset(
        memory,
        evidence,
        DatasetConfiguration {
            id: dataset.id.clone(),
            version: dataset.version,
            scope: dataset.scope.clone(),
            source_revision: dataset.source_revision.clone(),
            builder_version: dataset.builder_version.clone(),
            excluded_examples: dataset.excluded_examples.clone(),
            max_signals: selections.len(),
        },
        selections,
        at,
    )?;
    if rebuilt.rows != dataset.rows || rebuilt.duplicates != dataset.duplicates {
        return Err(LearningError::InvalidManifest);
    }
    Ok(())
}

pub(crate) fn digest<T: Serialize>(value: &T) -> ContentDigest {
    let bytes = serde_json::to_vec(value).expect("typed metadata serializes");
    ContentDigest::new(format!("{:x}", Sha256::digest(bytes))).expect("SHA-256")
}
pub(crate) fn zero_digest() -> ContentDigest {
    ContentDigest::new("0".repeat(64)).expect("SHA-256")
}
pub fn manifest_digest(dataset: &DatasetManifest) -> ContentDigest {
    let mut copy = dataset.clone();
    copy.digest = zero_digest();
    digest(&copy)
}
pub fn recipe_digest(recipe: &TrainingRecipe) -> ContentDigest {
    digest(recipe)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OfflineJob {
    pub id: ReferenceId,
    pub scope: ContextScopeId,
    pub dataset_digest: ContentDigest,
    pub recipe_digest: ContentDigest,
    pub base_model: ModelVersion,
}
/// Host-controlled authorization of an isolated worker and exact job binding.
/// Implementations must deny production runtime and deny expired/revoked grants.
pub trait OfflineAuthorizationPort {
    fn authorize(&self, job: &OfflineJob, at: UnixTimestamp) -> Result<bool, LearningError>;
}
/// Only Train/Validation rows are exposed to the training worker.
pub trait OfflineTrainingPort {
    fn train(
        &self,
        job: &OfflineJob,
        recipe: &TrainingRecipe,
        rows: &[DatasetRow],
    ) -> Result<TrainingRun, LearningError>;
}
pub struct OfflineEvaluationResult {
    pub cases: Vec<GoldenCase>,
    pub evidence: ReferenceId,
    pub evaluated_at: UnixTimestamp,
}
/// The adapter verifies the training-run evidence and expected labels against
/// validated sources. Isolated evaluation executes the exact candidate against only held-out Test
/// rows. It must resolve references through controlled access and record evidence.
pub trait OfflineEvaluationPort {
    fn evaluate(
        &self,
        run: &TrainingRun,
        recipe: &TrainingRecipe,
        rows: &[DatasetRow],
    ) -> Result<OfflineEvaluationResult, LearningError>;
}

#[allow(clippy::too_many_arguments)]
pub fn train_offline<
    S: MemoryStore,
    E: LearningEvidencePort,
    A: OfflineAuthorizationPort,
    T: OfflineTrainingPort,
>(
    memory: &MemoryApplication<S>,
    evidence: &E,
    authorization: &A,
    trainer: &T,
    dataset: &DatasetManifest,
    recipe: &TrainingRecipe,
    job_id: ReferenceId,
    at: UnixTimestamp,
) -> Result<TrainingRun, LearningError> {
    validate_recipe(recipe)?;
    revalidate_dataset(memory, evidence, dataset, at)?;
    let job = OfflineJob {
        id: job_id,
        scope: dataset.scope.clone(),
        dataset_digest: dataset.digest.clone(),
        recipe_digest: recipe_digest(recipe),
        base_model: recipe.base_model.clone(),
    };
    if !authorization.authorize(&job, at)? {
        return Err(LearningError::Unauthorized);
    }
    let rows: Vec<_> = dataset
        .rows
        .iter()
        .filter(|r| r.split != DatasetSplit::Test)
        .cloned()
        .collect();
    let run = trainer.train(&job, recipe, &rows)?;
    if run.job != job.id
        || run.scope != job.scope
        || run.dataset_digest != job.dataset_digest
        || run.recipe_digest != job.recipe_digest
        || run.schema_version != OFFLINE_LEARNING_VERSION
        || run.candidate.version == 0
        || run.candidate == recipe.base_model
        || run.completed_at < at
    {
        return Err(LearningError::InvalidRun);
    }
    Ok(run)
}
fn validate_recipe(recipe: &TrainingRecipe) -> Result<(), LearningError> {
    if recipe.schema_version != OFFLINE_LEARNING_VERSION
        || recipe.version == 0
        || recipe.base_model.version == 0
        || recipe
            .parameters
            .iter()
            .any(|(k, v)| k.trim().is_empty() || v.trim().is_empty())
    {
        return Err(LearningError::InvalidManifest);
    }
    Ok(())
}

/// Opaque qualification cannot be constructed by deserializing a passing report.
pub struct QualifiedModel {
    pub(crate) run: TrainingRun,
    pub(crate) recipe: TrainingRecipe,
    pub(crate) evaluation: ModelEvaluation,
}
impl QualifiedModel {
    pub fn run(&self) -> &TrainingRun {
        &self.run
    }
    pub fn evaluation(&self) -> &ModelEvaluation {
        &self.evaluation
    }
}

#[allow(clippy::too_many_arguments)]
pub fn evaluate_offline<
    S: MemoryStore,
    E: LearningEvidencePort,
    A: OfflineAuthorizationPort,
    V: OfflineEvaluationPort,
>(
    memory: &MemoryApplication<S>,
    evidence: &E,
    authorization: &A,
    evaluator: &V,
    dataset: &DatasetManifest,
    recipe: &TrainingRecipe,
    run: TrainingRun,
    policy: &ReleasePolicy,
    at: UnixTimestamp,
) -> Result<QualifiedModel, LearningError> {
    validate_recipe(recipe)?;
    revalidate_dataset(memory, evidence, dataset, at)?;
    if run.schema_version != OFFLINE_LEARNING_VERSION
        || run.scope != dataset.scope
        || run.dataset_digest != dataset.digest
        || run.recipe_digest != recipe_digest(recipe)
        || run.candidate.version == 0
        || run.completed_at > at
        || run.completed_at < dataset.assembled_at
    {
        return Err(LearningError::InvalidRun);
    }
    let job = OfflineJob {
        id: run.job.clone(),
        scope: run.scope.clone(),
        dataset_digest: run.dataset_digest.clone(),
        recipe_digest: run.recipe_digest.clone(),
        base_model: recipe.base_model.clone(),
    };
    if !authorization.authorize(&job, at)? {
        return Err(LearningError::Unauthorized);
    }
    let rows: Vec<_> = dataset
        .rows
        .iter()
        .filter(|r| r.split == DatasetSplit::Test)
        .cloned()
        .collect();
    let result = evaluator.evaluate(&run, recipe, &rows)?;
    let expected: BTreeSet<_> = rows.iter().map(|r| r.signal.id.clone()).collect();
    let actual: BTreeSet<_> = result.cases.iter().map(|c| c.id.clone()).collect();
    if expected != actual || actual.len() != result.cases.len() || result.evaluated_at < at {
        return Err(LearningError::InvalidRun);
    }
    let report = evaluation::evaluate(
        evaluation::EvaluationManifest {
            version: evaluation::EVALUATION_VERSION,
            dataset: dataset.id.clone(),
            scope: dataset.scope.clone(),
            source_digest: dataset.digest.as_str().to_owned(),
            index_version: dataset.builder_version.as_str().to_owned(),
            embedding_version: recipe.base_model.artifact_digest.as_str().to_owned(),
            model_version: run.candidate.artifact_digest.as_str().to_owned(),
            estimator_version: recipe.trainer_version.as_str().to_owned(),
            strategy_version: recipe.id.as_str().to_owned(),
            evaluator_version: recipe.evaluator_version.as_str().to_owned(),
            baseline: policy.baseline.clone(),
        },
        &result.cases,
    )
    .map_err(LearningError::Evaluation)?;
    policy.qualify(&report).map_err(LearningError::Evaluation)?;
    let evaluation = ModelEvaluation {
        dataset_digest: dataset.digest.clone(),
        evaluator_version: recipe.evaluator_version.clone(),
        evidence: result.evidence,
        evaluated_at: result.evaluated_at,
        baseline: policy.baseline.clone(),
        policy_digest: digest(&(
            policy.version,
            &policy.baseline,
            &policy.floors,
            &policy.baseline_scores,
            policy.allowed_regression,
        )),
        scores_millionths: report
            .metrics
            .iter()
            .map(|(k, v)| (k.to_string(), v.millionths().expect("qualified metric")))
            .collect(),
        cases: report.cases,
        latency_ms: report.latency_ms,
        cost_units: report.cost_units,
    };
    Ok(QualifiedModel {
        run,
        recipe: recipe.clone(),
        evaluation,
    })
}

pub fn upgrade_impact(
    scope: &ContextScopeId,
    previous: &ModelVersion,
    next: &ModelVersion,
    artifacts: &[ModelDependentArtifact],
) -> Result<Vec<UpgradeImpact>, LearningError> {
    if previous.version == 0 || next.version == 0 || previous == next {
        return Err(LearningError::InvalidManifest);
    }
    let mut seen = BTreeSet::new();
    let mut impacts = Vec::new();
    for artifact in artifacts {
        if artifact.scope != *scope {
            return Err(LearningError::ScopeMismatch);
        }
        if !seen.insert(&artifact.id) {
            return Err(LearningError::DuplicateIdentity);
        }
        if artifact.models.contains(previous) {
            let action = match artifact.kind {
                ModelArtifactKind::Embedding | ModelArtifactKind::VectorIndex => {
                    UpgradeAction::Reembed
                }
                ModelArtifactKind::LearnedProcedure => UpgradeAction::Recertify,
                ModelArtifactKind::SemanticMapping
                | ModelArtifactKind::PromptCache
                | ModelArtifactKind::EvaluationBaseline => UpgradeAction::Reevaluate,
            };
            impacts.push(UpgradeImpact {
                artifact: artifact.clone(),
                action,
            });
        }
    }
    impacts.sort_by(|a, b| a.artifact.id.cmp(&b.artifact.id));
    Ok(impacts)
}
