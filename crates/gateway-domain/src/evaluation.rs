//! Deterministic CG-20 evaluation contracts. Scores use integer millionths.
use crate::{ContextScopeId, ReferenceId, SufficiencyFinding};
use std::collections::{BTreeMap, BTreeSet};

pub const EVALUATION_VERSION: u16 = 1;
pub const METRICS: [&str; 8] = [
    "precision",
    "recall",
    "sufficiency",
    "provenance",
    "freshness",
    "contamination",
    "budget",
    "token_efficiency",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationManifest {
    pub version: u16,
    pub dataset: ReferenceId,
    pub scope: ContextScopeId,
    pub source_digest: String,
    pub index_version: String,
    pub embedding_version: String,
    pub model_version: String,
    pub estimator_version: String,
    pub strategy_version: String,
    pub evaluator_version: String,
    pub baseline: ReferenceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenCase {
    pub id: ReferenceId,
    pub scope: ContextScopeId,
    pub relevant: BTreeSet<ReferenceId>,
    pub returned: Vec<ReferenceId>,
    pub expected_sufficiency: SufficiencyFinding,
    pub actual_sufficiency: SufficiencyFinding,
    pub expected_provenance: bool,
    pub actual_provenance: bool,
    pub expected_freshness: bool,
    pub actual_freshness: bool,
    pub expected_contamination_rejected: bool,
    pub actual_contamination_rejected: bool,
    pub token_budget: u64,
    pub tokens_used: u64,
    /// Tokens in selected fragments supported by the case's relevant set.
    pub justified_tokens: u64,
    pub latency_ms: u64,
    pub cost_units: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metric {
    pub numerator: u64,
    pub denominator: u64,
}
impl Metric {
    pub fn millionths(self) -> Option<u32> {
        (self.denominator != 0 && self.numerator <= self.denominator).then(|| {
            ((u128::from(self.numerator) * 1_000_000) / u128::from(self.denominator)) as u32
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationReport {
    pub manifest: EvaluationManifest,
    pub metrics: BTreeMap<&'static str, Metric>,
    pub cases: usize,
    pub latency_ms: u64,
    pub cost_units: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvaluationError {
    InvalidManifest,
    EmptyDataset,
    DuplicateCase,
    ScopeMismatch,
    DuplicateResult,
    ArithmeticOverflow,
    MissingMetric,
    BelowThreshold(&'static str),
    BaselineRegression(&'static str),
}

/// Empty relevance is a valid hard-negative case. Precision and recall use
/// only cases with a nonempty relevant set; their missing denominator fails release.
pub fn evaluate(
    manifest: EvaluationManifest,
    cases: &[GoldenCase],
) -> Result<EvaluationReport, EvaluationError> {
    if manifest.version != EVALUATION_VERSION
        || manifest.source_digest.len() != 64
        || !manifest
            .source_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || [
            &manifest.source_digest,
            &manifest.index_version,
            &manifest.embedding_version,
            &manifest.model_version,
            &manifest.estimator_version,
            &manifest.strategy_version,
            &manifest.evaluator_version,
        ]
        .iter()
        .any(|value| value.trim().is_empty())
    {
        return Err(EvaluationError::InvalidManifest);
    }
    if cases.is_empty() {
        return Err(EvaluationError::EmptyDataset);
    }
    let mut ids = BTreeSet::new();
    let mut metrics = BTreeMap::from(METRICS.map(|key| {
        (
            key,
            Metric {
                numerator: 0,
                denominator: 0,
            },
        )
    }));
    let mut latency_ms = 0u64;
    let mut cost_units = 0u64;
    for case in cases {
        if !ids.insert(&case.id) {
            return Err(EvaluationError::DuplicateCase);
        }
        if case.scope != manifest.scope {
            return Err(EvaluationError::ScopeMismatch);
        }
        let returned: BTreeSet<_> = case.returned.iter().collect();
        if returned.len() != case.returned.len() {
            return Err(EvaluationError::DuplicateResult);
        }
        let relevant_returned = returned
            .iter()
            .filter(|id| case.relevant.contains(*id))
            .count() as u64;
        if !case.relevant.is_empty() {
            add(
                &mut metrics,
                "precision",
                relevant_returned,
                (returned.len() as u64).max(1),
            )?;
            add(
                &mut metrics,
                "recall",
                relevant_returned,
                case.relevant.len() as u64,
            )?;
        }
        for (key, correct) in [
            (
                "sufficiency",
                case.expected_sufficiency == case.actual_sufficiency,
            ),
            (
                "provenance",
                case.expected_provenance == case.actual_provenance,
            ),
            (
                "freshness",
                case.expected_freshness == case.actual_freshness,
            ),
            (
                "contamination",
                case.expected_contamination_rejected == case.actual_contamination_rejected,
            ),
            ("budget", case.tokens_used <= case.token_budget),
        ] {
            add(&mut metrics, key, u64::from(correct), 1)?;
        }
        if case.justified_tokens > case.tokens_used {
            return Err(EvaluationError::InvalidManifest);
        }
        if case.tokens_used > 0 {
            add(
                &mut metrics,
                "token_efficiency",
                case.justified_tokens,
                case.tokens_used,
            )?;
        }
        latency_ms = latency_ms
            .checked_add(case.latency_ms)
            .ok_or(EvaluationError::ArithmeticOverflow)?;
        cost_units = cost_units
            .checked_add(case.cost_units)
            .ok_or(EvaluationError::ArithmeticOverflow)?;
    }
    Ok(EvaluationReport {
        manifest,
        metrics,
        cases: cases.len(),
        latency_ms,
        cost_units,
    })
}

fn add(
    metrics: &mut BTreeMap<&'static str, Metric>,
    key: &'static str,
    numerator: u64,
    denominator: u64,
) -> Result<(), EvaluationError> {
    let metric = metrics.get_mut(key).expect("known metric");
    metric.numerator = metric
        .numerator
        .checked_add(numerator)
        .ok_or(EvaluationError::ArithmeticOverflow)?;
    metric.denominator = metric
        .denominator
        .checked_add(denominator)
        .ok_or(EvaluationError::ArithmeticOverflow)?;
    Ok(())
}

/// An explicit policy pins the baseline identity and all objective floors.
/// Provider-assisted judgments are never accepted as release metrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePolicy {
    pub version: u16,
    pub baseline: ReferenceId,
    pub floors: BTreeMap<&'static str, u32>,
    pub baseline_scores: BTreeMap<&'static str, u32>,
    pub allowed_regression: u32,
}
impl ReleasePolicy {
    pub fn qualify(&self, report: &EvaluationReport) -> Result<(), EvaluationError> {
        if self.version != EVALUATION_VERSION
            || self.baseline != report.manifest.baseline
            || self.allowed_regression > 1_000_000
        {
            return Err(EvaluationError::InvalidManifest);
        }
        for key in METRICS {
            let actual = report
                .metrics
                .get(key)
                .and_then(|m| m.millionths())
                .ok_or(EvaluationError::MissingMetric)?;
            let floor = *self.floors.get(key).ok_or(EvaluationError::MissingMetric)?;
            let baseline = *self
                .baseline_scores
                .get(key)
                .ok_or(EvaluationError::MissingMetric)?;
            if floor > 1_000_000 || baseline > 1_000_000 {
                return Err(EvaluationError::InvalidManifest);
            }
            if actual < floor {
                return Err(EvaluationError::BelowThreshold(key));
            }
            if actual.saturating_add(self.allowed_regression) < baseline {
                return Err(EvaluationError::BaselineRegression(key));
            }
        }
        Ok(())
    }
}
