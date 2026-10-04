//! Concrete offline CPU adapter behind CG-28 ports. Feature resolution belongs
//! to trusted snapshot adapters; production inference has no training endpoint.
use crate::{bounded_process::BoundedProcess, durable_models::DurableModelReleases};
use gateway_application::{
    local_inference::*, model_releases::ModelRecoveryAuthority, offline_learning::*,
};
use gateway_domain::{
    ContentDigest, ReferenceId, SufficiencyFinding, UnixTimestamp, evaluation::GoldenCase,
    offline_learning::*,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

/// Must resolve pre-decision features from the exact validated source snapshot,
/// enforce source sensitivity, and reject revoked/unknown feature provenance.
pub trait LearningFeaturePort {
    fn features(&self, row: &DatasetRow) -> Result<BTreeMap<String, f64>, LearningError>;
}
pub struct CpuOfflineAdapter<F> {
    pub process: BoundedProcess,
    pub artifacts: PathBuf,
    pub features: F,
    pub clock: fn() -> UnixTimestamp,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn parameter(recipe: &TrainingRecipe, key: &str) -> Result<Value, LearningError> {
    serde_json::from_str(
        recipe
            .parameters
            .get(key)
            .ok_or(LearningError::InvalidManifest)?,
    )
    .map_err(|_| LearningError::InvalidManifest)
}
impl<F: LearningFeaturePort> CpuOfflineAdapter<F> {
    fn rows(&self, rows: &[DatasetRow]) -> Result<Vec<Value>, LearningError> {
        rows.iter().map(|r| {
            let features = self.features.features(r)?;
            if features.is_empty() || features.values().any(|v| !v.is_finite()) {
                return Err(LearningError::InvalidSignal);
            }
            Ok(json!({"id":r.signal.id, "scope":r.signal.memory.scope, "split":r.split,
                "time":r.signal.observed_at.seconds(), "label":r.signal.measurements.get(&SignalKind::Success).copied().unwrap_or(0),
                "label_basis":r.signal.label_basis, "evidence":r.signal.evidence,
                "source":r.signal.memory.source_digest, "example":r.signal.example_digest,
                "group":r.signal.leakage_group, "features":features}))
        }).collect()
    }
    fn invoke(&self, operation: &str, request: &Value) -> Result<Vec<u8>, LearningError> {
        self.process
            .run(
                operation,
                &serde_json::to_vec(request).map_err(|_| LearningError::InvalidRun)?,
                30_000,
                268_435_456,
                30,
                4_194_304,
            )
            .map_err(|_| LearningError::InvalidRun)
    }
    pub fn artifact(&self, model: &ModelVersion) -> Result<(Value, Vec<u8>), LearningError> {
        let bytes = fs::read(self.artifacts.join(model.artifact_digest.as_str()))
            .map_err(|_| LearningError::Storage)?;
        if hash(&bytes) != model.artifact_digest.as_str() {
            return Err(LearningError::Unverified);
        }
        let value = serde_json::from_slice(&bytes).map_err(|_| LearningError::InvalidRun)?;
        Ok((value, bytes))
    }
}
impl<F: LearningFeaturePort> OfflineTrainingPort for CpuOfflineAdapter<F> {
    fn train(
        &self,
        job: &OfflineJob,
        recipe: &TrainingRecipe,
        rows: &[DatasetRow],
    ) -> Result<TrainingRun, LearningError> {
        if rows.iter().any(|r| r.split == DatasetSplit::Test) {
            return Err(LearningError::Leakage);
        }
        let mut request = json!({"scope":job.scope, "job":job.id, "dataset_digest":job.dataset_digest,
            "recipe_digest":job.recipe_digest, "rows":self.rows(rows)?, "plan":parameter(recipe,"ml_plan")?});
        if let Some(prior) = recipe.parameters.get("prior_model") {
            let model: ModelVersion =
                serde_json::from_str(prior).map_err(|_| LearningError::InvalidManifest)?;
            let (_, original) = self.artifact(&model)?;
            request["prior_json"] =
                json!(String::from_utf8(original).map_err(|_| LearningError::InvalidRun)?);
            request["prior_artifact_digest"] = json!(model.artifact_digest);
        }
        let bytes = self.invoke("train", &request)?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| LearningError::InvalidRun)?;
        if value["artifact"]["job"] != request["job"]
            || value["artifact"]["scope"] != request["scope"]
        {
            return Err(LearningError::InvalidRun);
        }
        let digest = ContentDigest::new(hash(&bytes)).map_err(|_| LearningError::InvalidRun)?;
        let path = self.artifacts.join(digest.as_str());
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut file) => {
                file.write_all(&bytes)
                    .and_then(|_| file.sync_all())
                    .map_err(|_| LearningError::Storage)?;
                fs::File::open(&self.artifacts)
                    .and_then(|f| f.sync_all())
                    .map_err(|_| LearningError::Storage)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if fs::read(self.artifacts.join(digest.as_str()))
                    .map_err(|_| LearningError::Storage)?
                    != bytes
                {
                    return Err(LearningError::Unverified);
                }
            }
            Err(_) => return Err(LearningError::Storage),
        }
        Ok(TrainingRun {
            schema_version: 1,
            job: job.id.clone(),
            scope: job.scope.clone(),
            dataset_digest: job.dataset_digest.clone(),
            recipe_digest: job.recipe_digest.clone(),
            candidate: ModelVersion {
                id: recipe.base_model.id.clone(),
                version: recipe
                    .base_model
                    .version
                    .checked_add(1)
                    .ok_or(LearningError::InvalidRun)?,
                artifact_digest: digest.clone(),
            },
            evidence: ReferenceId::new(format!("sha256-{}", digest.as_str()))
                .map_err(|_| LearningError::InvalidRun)?,
            completed_at: (self.clock)(),
        })
    }
}
impl<F: LearningFeaturePort> OfflineEvaluationPort for CpuOfflineAdapter<F> {
    fn evaluate(
        &self,
        run: &TrainingRun,
        recipe: &TrainingRecipe,
        rows: &[DatasetRow],
    ) -> Result<OfflineEvaluationResult, LearningError> {
        if rows.iter().any(|r| r.split != DatasetSplit::Test) {
            return Err(LearningError::Leakage);
        }
        let (mut request, original) = self.artifact(&run.candidate)?;
        if request["artifact"]["job"] != json!(run.job)
            || request["artifact"]["scope"] != json!(run.scope)
            || request["artifact"]["dataset_digest"] != json!(run.dataset_digest)
            || request["artifact"]["recipe_digest"] != json!(recipe_digest(recipe))
            || run.evidence.as_str() != format!("sha256-{}", run.candidate.artifact_digest.as_str())
        {
            return Err(LearningError::Unverified);
        }
        request["artifact_json"] =
            json!(String::from_utf8(original).map_err(|_| LearningError::InvalidRun)?);
        request["rows"] = json!(self.rows(rows)?);
        request["profile"] = parameter(recipe, "ml_profile")?;
        let bytes = self.invoke("evaluate", &request)?;
        let result: Value =
            serde_json::from_slice(&bytes).map_err(|_| LearningError::InvalidRun)?;
        if result["status"] != "PASS" {
            return Err(LearningError::InvalidRun);
        }
        let predictions = result["predictions"]
            .as_array()
            .ok_or(LearningError::InvalidRun)?;
        let mut cases = Vec::new();
        for row in rows {
            let prediction = predictions
                .iter()
                .find(|p| p["id"] == json!(row.signal.id))
                .ok_or(LearningError::InvalidRun)?;
            let expected = row
                .signal
                .measurements
                .get(&SignalKind::Success)
                .copied()
                .unwrap_or(0);
            let label = prediction["label"]
                .as_u64()
                .filter(|x| *x <= 1)
                .ok_or(LearningError::InvalidRun)?;
            let expected_id = ReferenceId::new(format!("label-{expected}"))
                .map_err(|_| LearningError::InvalidRun)?;
            let returned_id = ReferenceId::new(format!("label-{label}"))
                .map_err(|_| LearningError::InvalidRun)?;
            cases.push(GoldenCase {
                id: row.signal.id.clone(),
                scope: run.scope.clone(),
                relevant: BTreeSet::from([expected_id]),
                returned: vec![returned_id],
                expected_sufficiency: SufficiencyFinding::Sufficient,
                actual_sufficiency: SufficiencyFinding::Sufficient,
                expected_provenance: true,
                actual_provenance: true,
                expected_freshness: true,
                actual_freshness: true,
                expected_contamination_rejected: true,
                actual_contamination_rejected: true,
                token_budget: 1,
                tokens_used: 1,
                justified_tokens: u64::from(label == expected),
                latency_ms: 0,
                cost_units: 0,
            });
        }
        let evidence = ReferenceId::new(format!("sha256-{}", hash(&bytes)))
            .map_err(|_| LearningError::InvalidRun)?;
        let path = self
            .artifacts
            .join(evidence.as_str().trim_start_matches("sha256-"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => file
                .write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| LearningError::Storage)?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if fs::read(path).map_err(|_| LearningError::Storage)? != bytes {
                    return Err(LearningError::Unverified);
                }
            }
            Err(_) => return Err(LearningError::Storage),
        }
        Ok(OfflineEvaluationResult {
            cases,
            evidence,
            evaluated_at: (self.clock)(),
        })
    }
}

/// Reads the active durable release for every call; rollback changes subsequent
/// inference without restarting. Returned classifications remain proposals.
pub struct CpuReleaseInference<'a, A> {
    pub releases: &'a DurableModelReleases,
    pub authority: &'a A,
    pub artifacts: PathBuf,
    pub process: BoundedProcess,
}
impl<A: ModelRecoveryAuthority> LocalInferencePort for CpuReleaseInference<'_, A> {
    fn infer(
        &self,
        request: &LocalInferenceRequest,
    ) -> Result<LocalInferenceProposal, LocalInferenceError> {
        if request.schema_version != "1.0"
            || request.role != "outcome-classifier"
            || request.input_contract != "health-features-v1"
            || request.output_contract != "binary-outcome-proposal-v1"
            || !request.output_schema.is_object()
        {
            return Err(LocalInferenceError::InvalidRequest);
        }
        let model = self
            .releases
            .active(self.authority)
            .map_err(|_| LocalInferenceError::Unavailable)?
            .ok_or(LocalInferenceError::Unavailable)?;
        let bytes = fs::read(self.artifacts.join(model.artifact_digest.as_str()))
            .map_err(|_| LocalInferenceError::Unavailable)?;
        if hash(&bytes) != model.artifact_digest.as_str() {
            return Err(LocalInferenceError::InvalidProposal);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| LocalInferenceError::InvalidProposal)?;
        if value["artifact"]["scope"] != json!(self.releases.scope())
            || value["artifact"]["feature_schema_version"] != request.input_contract
        {
            return Err(LocalInferenceError::InvalidProposal);
        }
        let features: Value = serde_json::from_str(&request.prompt)
            .map_err(|_| LocalInferenceError::InvalidRequest)?;
        let script =
            fs::read(&self.process.script).map_err(|_| LocalInferenceError::Unavailable)?;
        if value["artifact"]["runtime"]["code_digest"] != hash(&script) {
            return Err(LocalInferenceError::InvalidProposal);
        }
        let input = json!({"artifact_json":String::from_utf8(bytes).map_err(|_| LocalInferenceError::InvalidProposal)?,"row":{"features":features}});
        let output = self
            .process
            .run(
                "predict",
                &serde_json::to_vec(&input).map_err(|_| LocalInferenceError::InvalidRequest)?,
                5000,
                268_435_456,
                5,
                16384,
            )
            .map_err(|_| LocalInferenceError::Unavailable)?;
        let proposal: Value =
            serde_json::from_slice(&output).map_err(|_| LocalInferenceError::InvalidProposal)?;
        Ok(LocalInferenceProposal {
            schema_version: "1.0".into(),
            kind: "proposal".into(),
            model_id: model.id.as_str().into(),
            artifact_digest: format!("sha256:{}", model.artifact_digest.as_str()),
            proposal: proposal.clone(),
            metrics: json!({"cost_unit":"external-provider-calls","cost":0}),
            provenance: LocalInferenceProvenance {
                model_version: model.version.to_string(),
                runtime: "cg-offline-cpu-v1".into(),
                runtime_version: proposal["runtime"]["python"]
                    .as_str()
                    .ok_or(LocalInferenceError::InvalidProposal)?
                    .into(),
                runtime_configuration: value["artifact"]["model"]["features"].clone(),
                prompt_version: "health-features-v1".into(),
                template_digest: format!("sha256:{}", hash(b"binary-outcome-proposal-v1")),
                system_digest: format!("sha256:{}", hash(b"proposal-only")),
                input_contract: request.input_contract.clone(),
                output_contract: request.output_contract.clone(),
            },
        })
    }
}
