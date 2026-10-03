//! CG-28 reference-only contracts. Serialized evidence never grants authority.
use crate::{
    ContentDigest, ContextScopeId, EvidenceId, ProvenanceId, ReferenceId, UnixTimestamp,
    memory::MemoryEligibilityReference,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const OFFLINE_LEARNING_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SignalKind {
    Success,
    Failure,
    QualityGate,
    Retries,
    Repairs,
    LatencyMs,
    CostUnits,
    HumanCorrection,
    PolicyDenial,
    Rollback,
    Recurrence,
}

/// A request to admit measurements, with references to the validating evidence.
/// The evidence port must verify the entire value against its trusted source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningSignal {
    pub schema_version: u16,
    pub id: ReferenceId,
    pub memory: MemoryEligibilityReference,
    pub provenance: ProvenanceId,
    pub validation: ReferenceId,
    pub label_basis: ReferenceId,
    pub evaluation: ReferenceId,
    pub evidence: BTreeSet<EvidenceId>,
    pub observed_at: UnixTimestamp,
    pub producer_model: ModelVersion,
    /// Validated identity of normalized input/target, excluding provenance IDs.
    pub example_digest: ContentDigest,
    /// Verified task/episode/near-duplicate family; must stay in one split.
    pub leakage_group: ReferenceId,
    /// Boolean measurements use 0/1; latency, cost and counts are unsigned integers.
    pub measurements: BTreeMap<SignalKind, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelVersion {
    pub id: ReferenceId,
    pub version: u32,
    pub artifact_digest: ContentDigest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DatasetSplit {
    Train,
    Validation,
    Test,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetRow {
    pub signal: LearningSignal,
    pub split: DatasetSplit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetManifest {
    pub schema_version: u16,
    pub id: ReferenceId,
    pub version: u32,
    pub scope: ContextScopeId,
    pub source_revision: ReferenceId,
    pub assembled_at: UnixTimestamp,
    pub builder_version: ReferenceId,
    pub excluded_examples: BTreeSet<ContentDigest>,
    /// Sorted by signal ID, reference-only, including all duplicate provenance.
    pub rows: Vec<DatasetRow>,
    pub duplicates: Vec<DatasetRow>,
    pub digest: ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrainingRecipe {
    pub schema_version: u16,
    pub id: ReferenceId,
    pub version: u32,
    pub base_model: ModelVersion,
    pub trainer_version: ReferenceId,
    pub evaluator_version: ReferenceId,
    pub environment_digest: ContentDigest,
    pub seed: u64,
    pub parameters: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrainingRun {
    pub schema_version: u16,
    pub job: ReferenceId,
    pub scope: ContextScopeId,
    pub dataset_digest: ContentDigest,
    pub recipe_digest: ContentDigest,
    pub candidate: ModelVersion,
    pub evidence: ReferenceId,
    pub completed_at: UnixTimestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEvaluation {
    pub dataset_digest: ContentDigest,
    pub evaluator_version: ReferenceId,
    pub evidence: ReferenceId,
    pub evaluated_at: UnixTimestamp,
    pub baseline: ReferenceId,
    pub policy_digest: ContentDigest,
    pub scores_millionths: BTreeMap<String, u32>,
    pub cases: usize,
    pub latency_ms: u64,
    pub cost_units: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCanary {
    pub cohorts: BTreeSet<ReferenceId>,
    pub starts_at: UnixTimestamp,
    pub ends_at: UnixTimestamp,
    pub max_requests: u64,
    pub max_failures: u64,
    pub required_successes: u64,
}

/// Immutable candidate metadata; deployment state lives in an independent journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelReleaseManifest {
    pub schema_version: u16,
    pub scope: ContextScopeId,
    pub training: TrainingRun,
    pub recipe: TrainingRecipe,
    pub evaluation: ModelEvaluation,
    pub canary: ModelCanary,
    pub predecessor: Option<ModelVersion>,
    pub digest: ContentDigest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ModelReleaseAction {
    Register,
    StartCanary,
    ObserveCanary,
    Activate,
    Rollback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelReleaseDecision {
    pub id: ReferenceId,
    pub actor: ProvenanceId,
    pub policy_decision: ReferenceId,
    pub at: UnixTimestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelReleaseEvent {
    pub decision: ModelReleaseDecision,
    pub action: ModelReleaseAction,
    pub release_digest: ContentDigest,
    pub restored: Option<ModelVersion>,
    pub observation: Option<CanaryObservation>,
}

/// The observation port verifies the entire batch and its cohort/time provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanaryObservation {
    pub id: ReferenceId,
    pub cohort: ReferenceId,
    pub observed_at: UnixTimestamp,
    pub successes: u64,
    pub failures: u64,
    pub evidence: ReferenceId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ModelArtifactKind {
    Embedding,
    VectorIndex,
    LearnedProcedure,
    SemanticMapping,
    PromptCache,
    EvaluationBaseline,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UpgradeAction {
    Reembed,
    Recertify,
    Reevaluate,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDependentArtifact {
    pub id: ReferenceId,
    pub scope: ContextScopeId,
    pub kind: ModelArtifactKind,
    pub models: BTreeSet<ModelVersion>,
    pub digest: ContentDigest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeImpact {
    pub artifact: ModelDependentArtifact,
    pub action: UpgradeAction,
}
