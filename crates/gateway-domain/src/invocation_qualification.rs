//! Qualification observations are trusted evaluator input, never model authority.
use crate::{ContentDigest, NonEmptyText, ReferenceId, ResolvedReference, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationProvenance {
    pub schema_version: SchemaVersion,
    pub model: ResolvedReference<ReferenceId>,
    pub family: NonEmptyText,
    pub tokenizer: ResolvedReference<ReferenceId>,
    pub runtime: ResolvedReference<ReferenceId>,
    pub quantization: NonEmptyText,
    pub generation_profile: ContentDigest,
    pub renderer: ResolvedReference<ReferenceId>,
    pub dataset: ResolvedReference<ReferenceId>,
    pub hardware: NonEmptyText,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Fixture,
    Live,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationRole {
    SemanticExtraction,
    ToolProposal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationMetric {
    SchemaConformance,
    FieldCorrectness,
    RequiredCompleteness,
    ReferenceAccuracy,
    ClarificationCorrectness,
    UnsupportedClaimRate,
    CitationValidity,
    ToolConformance,
    ToolIdentifierValidity,
    ArgumentValidity,
    UnauthorizedToolRefusal,
    BoundedRecovery,
    ContextLimitBehavior,
    FailureFallbackBehavior,
    ColdLatencyMs,
    WarmLatencyMs,
    ThroughputMilliPerSecond,
    CpuMillis,
    RamBytes,
    GpuMillis,
    VramBytes,
    InputTokens,
    OutputTokens,
    CalibrationErrorBasisPoints,
}
/// Rates use (successful cases, total cases); scalar measurements use denominator
/// one and the unit named by the metric. No floating point or aggregate score.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricObservation {
    pub numerator: u64,
    pub denominator: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricThreshold {
    pub minimum: u64,
    pub maximum: u64,
    pub minimum_samples: u64,
}
/// Rate thresholds are basis points; scalar thresholds use their named units.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationProfile {
    pub schema_version: SchemaVersion,
    pub id: ResolvedReference<ReferenceId>,
    pub provenance: QualificationProvenance,
    pub role: QualificationRole,
    pub confidence_emitted: bool,
    pub gpu_used: bool,
    pub thresholds: BTreeMap<QualificationMetric, MetricThreshold>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationMeasurements {
    pub schema_version: SchemaVersion,
    pub provenance: QualificationProvenance,
    pub evidence: EvidenceKind,
    /// Digest of policy-permitted case records and deterministic grader results.
    pub cases_digest: ContentDigest,
    pub metrics: BTreeMap<QualificationMetric, MetricObservation>,
    /// Optional LM diagnostics never participate in promotion gates. Dataset and
    /// tokenizer are pinned above; cross-model comparisons require justification.
    pub perplexity_diagnostic: Option<NonEmptyText>,
    pub cross_entropy_diagnostic: Option<NonEmptyText>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QualificationDiagnostic {
    InvalidProfile,
    ProvenanceMismatch,
    MissingMetric,
    InvalidMeasurement,
    ThresholdFailed,
    FixtureOnly,
}
#[derive(Debug, Clone, Serialize)]
pub struct QualificationDecision {
    pub schema_version: SchemaVersion,
    pub profile: ResolvedReference<ReferenceId>,
    pub provenance: QualificationProvenance,
    pub cases_digest: ContentDigest,
    pub profile_digest: ContentDigest,
    pub measurements_digest: ContentDigest,
    pub failed_metrics: BTreeMap<QualificationMetric, QualificationDiagnostic>,
    pub evidence: EvidenceKind,
    pub gates_passed: bool,
    pub eligible_for_promotion: bool,
    /// Confidence never substitutes for a deterministic verification decision.
    pub confidence_calibrated: bool,
    pub diagnostics: Vec<QualificationDiagnostic>,
}
impl QualificationMetric {
    pub fn is_rate(self) -> bool {
        use QualificationMetric::*;
        matches!(
            self,
            SchemaConformance
                | FieldCorrectness
                | RequiredCompleteness
                | ReferenceAccuracy
                | ClarificationCorrectness
                | UnsupportedClaimRate
                | CitationValidity
                | ToolConformance
                | ToolIdentifierValidity
                | ArgumentValidity
                | UnauthorizedToolRefusal
                | BoundedRecovery
                | ContextLimitBehavior
                | FailureFallbackBehavior
        )
    }
}
