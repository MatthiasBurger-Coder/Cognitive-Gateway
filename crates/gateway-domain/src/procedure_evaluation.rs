//! Deterministic, non-executing replay over explicitly captured simulation inputs.
//! A digest proves identity, not source truth or runtime authorization.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::learning::{FallbackBehavior, FingerprintSignal, LearnedProcedure, ProcessReference};
use crate::{
    ContentDigest, ContextScopeId, EvidenceId, ObservationId, ReferenceId, ValidationError,
};

pub const PROCEDURE_EVALUATOR_VERSION: u16 = 1;

fn invalid(reason: &'static str) -> ValidationError {
    ValidationError::InvalidDeclarativeValue { reason }
}
fn digest<T: Serialize>(value: &T) -> ContentDigest {
    ContentDigest::new(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("replay contracts serialize"))
    ))
    .expect("SHA-256 digest")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InputStatus {
    Present,
    Missing,
    Stale,
    Conflicting,
    Failed,
}

/// A captured process/policy decision for a declared step. Absence fails closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepSnapshot {
    pub process: ProcessReference,
    pub capability: crate::CapabilityId,
    pub policy: crate::PolicyId,
    pub capability_available: bool,
    pub process_allowed: bool,
    pub policy_allowed: bool,
    pub process_trace: String,
    pub policy_trace: String,
    pub execution: InputStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaySnapshot {
    pub id: ReferenceId,
    pub at: i64,
    pub scope: ContextScopeId,
    pub signals: BTreeSet<FingerprintSignal>,
    pub observations: BTreeMap<ObservationId, InputStatus>,
    pub evidence: BTreeMap<EvidenceId, InputStatus>,
    pub verification: BTreeMap<EvidenceId, InputStatus>,
    /// Revalidated eligibility for every declared source experience.
    pub experience: BTreeMap<ReferenceId, InputStatus>,
    pub steps: Vec<StepSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CaseKind {
    HistoricalSuccess,
    HistoricalFailure,
    NearMatch,
    MissingEvidence,
    StaleEvidence,
    ConflictingEvidence,
    MissingObservation,
    IneligibleExperience,
    ProcessDenial,
    PolicyDenial,
    CapabilityMissing,
    VersionChange,
    ExecutionFailure,
    VerificationFailure,
}

/// Expected outcomes are exact: a negative refused for the wrong reason fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReplayOutcome {
    Success,
    NotApplicable,
    ObservationUnavailable,
    EvidenceUnavailable,
    ExperienceUnavailable,
    ProcessDenied,
    PolicyDenied,
    CapabilityMissing,
    VersionMismatch,
    ExecutionFailed,
    VerificationFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayCase {
    pub id: ReferenceId,
    pub kind: CaseKind,
    pub expected: ReplayOutcome,
    pub snapshot: ReplaySnapshot,
    pub snapshot_digest: ContentDigest,
}
impl ReplayCase {
    pub fn new(
        id: ReferenceId,
        kind: CaseKind,
        expected: ReplayOutcome,
        snapshot: ReplaySnapshot,
    ) -> Self {
        let snapshot_digest = digest(&snapshot);
        Self {
            id,
            kind,
            expected,
            snapshot,
            snapshot_digest,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationDataset {
    pub schema_version: u16,
    pub id: ReferenceId,
    pub version: u32,
    pub cases: Vec<ReplayCase>,
}
impl EvaluationDataset {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != 1 || self.version == 0 || self.cases.is_empty() {
            return Err(invalid("invalid evaluation dataset version or empty cases"));
        }
        let mut ids = BTreeSet::new();
        for case in &self.cases {
            if !ids.insert(&case.id) || case.snapshot_digest != digest(&case.snapshot) {
                return Err(invalid("duplicate case or snapshot digest mismatch"));
            }
            let required = match case.kind {
                CaseKind::HistoricalSuccess => Some(ReplayOutcome::Success),
                CaseKind::HistoricalFailure => None,
                CaseKind::NearMatch => Some(ReplayOutcome::NotApplicable),
                CaseKind::MissingEvidence
                | CaseKind::StaleEvidence
                | CaseKind::ConflictingEvidence => Some(ReplayOutcome::EvidenceUnavailable),
                CaseKind::MissingObservation => Some(ReplayOutcome::ObservationUnavailable),
                CaseKind::IneligibleExperience => Some(ReplayOutcome::ExperienceUnavailable),
                CaseKind::ProcessDenial => Some(ReplayOutcome::ProcessDenied),
                CaseKind::PolicyDenial => Some(ReplayOutcome::PolicyDenied),
                CaseKind::CapabilityMissing => Some(ReplayOutcome::CapabilityMissing),
                CaseKind::VersionChange => Some(ReplayOutcome::VersionMismatch),
                CaseKind::ExecutionFailure => Some(ReplayOutcome::ExecutionFailed),
                CaseKind::VerificationFailure => Some(ReplayOutcome::VerificationFailed),
            };
            if required.is_some_and(|expected| expected != case.expected)
                || (case.kind == CaseKind::HistoricalFailure
                    && case.expected == ReplayOutcome::Success)
            {
                return Err(invalid("case kind contradicts expected outcome"));
            }
        }
        Ok(())
    }
    pub fn content_digest(&self) -> ContentDigest {
        let mut canonical = self.clone();
        canonical.cases.sort_by(|a, b| a.id.cmp(&b.id));
        digest(&canonical)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationManifest {
    pub schema_version: u16,
    pub evaluator_version: u16,
    pub runtime_version: ReferenceId,
    pub candidate: ReferenceId,
    pub procedure: ReferenceId,
    pub procedure_version: u32,
    pub procedure_digest: ContentDigest,
    pub dataset: ReferenceId,
    pub dataset_version: u32,
    pub dataset_digest: ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseResult {
    pub id: ReferenceId,
    pub snapshot_digest: ContentDigest,
    pub outcome: ReplayOutcome,
    pub activated: bool,
    pub passed: bool,
    pub critical_false_positive: bool,
    pub completed_steps: usize,
    pub fallback: Option<FallbackBehavior>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationReport {
    pub manifest: EvaluationManifest,
    pub results: Vec<CaseResult>,
    pub missing_coverage: BTreeSet<CaseKind>,
    pub passed: bool,
}

/// Self-contained reproducible evidence; consumers recompute the report on use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationBundle {
    pub procedure: LearnedProcedure,
    pub dataset: EvaluationDataset,
    pub report: EvaluationReport,
    pub digest: ContentDigest,
}
impl EvaluationBundle {
    pub fn evaluate(
        procedure: &LearnedProcedure,
        mut dataset: EvaluationDataset,
        runtime_version: ReferenceId,
    ) -> Result<Self, ValidationError> {
        // Revalidate even values constructed through raw serde deserialization.
        LearnedProcedure::from_json(
            &procedure
                .to_json()
                .map_err(|_| invalid("invalid procedure serialization"))?,
        )
        .map_err(|_| invalid("invalid procedure contract"))?;
        dataset.validate()?;
        dataset.cases.sort_by(|a, b| a.id.cmp(&b.id));
        let report = evaluate(procedure, &dataset, runtime_version);
        let digest = digest(&(procedure, &dataset, &report));
        Ok(Self {
            procedure: procedure.clone(),
            dataset,
            report,
            digest,
        })
    }
    pub fn validate(&self) -> Result<(), ValidationError> {
        let expected = Self::evaluate(
            &self.procedure,
            self.dataset.clone(),
            self.report.manifest.runtime_version.clone(),
        )?;
        if self != &expected {
            return Err(invalid("evaluation evidence mismatch"));
        }
        Ok(())
    }
    pub fn proves(&self, procedure: &LearnedProcedure) -> Result<(), ValidationError> {
        self.validate()?;
        if &self.procedure != procedure || !self.report.passed {
            return Err(invalid(
                "successful evaluation for exact procedure version required",
            ));
        }
        Ok(())
    }
}

fn evaluate(
    procedure: &LearnedProcedure,
    dataset: &EvaluationDataset,
    runtime_version: ReferenceId,
) -> EvaluationReport {
    use CaseKind::*;
    let required = BTreeSet::from([
        HistoricalSuccess,
        HistoricalFailure,
        NearMatch,
        MissingEvidence,
        StaleEvidence,
        ConflictingEvidence,
        MissingObservation,
        IneligibleExperience,
        ProcessDenial,
        PolicyDenial,
        CapabilityMissing,
        VersionChange,
        ExecutionFailure,
        VerificationFailure,
    ]);
    let covered = dataset
        .cases
        .iter()
        .map(|case| case.kind)
        .collect::<BTreeSet<_>>();
    let missing_coverage = required
        .difference(&covered)
        .copied()
        .collect::<BTreeSet<_>>();
    let mut results = dataset
        .cases
        .iter()
        .map(|case| replay(procedure, case))
        .collect::<Vec<_>>();
    results.sort_by(|a, b| a.id.cmp(&b.id));
    EvaluationReport {
        manifest: EvaluationManifest {
            schema_version: 1,
            evaluator_version: PROCEDURE_EVALUATOR_VERSION,
            runtime_version,
            candidate: procedure.source_candidate().clone(),
            procedure: procedure.id().clone(),
            procedure_version: procedure.version(),
            procedure_digest: procedure.digest().clone(),
            dataset: dataset.id.clone(),
            dataset_version: dataset.version,
            dataset_digest: dataset.content_digest(),
        },
        passed: missing_coverage.is_empty()
            && results
                .iter()
                .all(|r| r.passed && !r.critical_false_positive),
        results,
        missing_coverage,
    }
}

fn replay(procedure: &LearnedProcedure, case: &ReplayCase) -> CaseResult {
    let s = &case.snapshot;
    let mut activated = false;
    let mut completed_steps = 0;
    let outcome = 'result: {
        if s.signals
            .iter()
            .filter(|signal| matches!(signal, FingerprintSignal::OperatingMode(_)))
            .count()
            > 1
            || s.scope != *procedure.fingerprint().scope()
            || procedure
                .fingerprint()
                .signals()
                .iter()
                .any(|signal| !s.signals.contains(signal))
        {
            break 'result ReplayOutcome::NotApplicable;
        }
        if procedure
            .required_observations()
            .iter()
            .any(|id| s.observations.get(id) != Some(&InputStatus::Present))
        {
            break 'result ReplayOutcome::ObservationUnavailable;
        }
        if procedure
            .required_evidence()
            .iter()
            .any(|id| s.evidence.get(id) != Some(&InputStatus::Present))
        {
            break 'result ReplayOutcome::EvidenceUnavailable;
        }
        if procedure
            .experience()
            .iter()
            .any(|basis| s.experience.get(&basis.memory().id) != Some(&InputStatus::Present))
        {
            break 'result ReplayOutcome::ExperienceUnavailable;
        }
        if s.steps.len() != procedure.steps().len() {
            break 'result ReplayOutcome::VersionMismatch;
        }
        // Check all gates before activation, including gates on later steps.
        for (declared, captured) in procedure.steps().iter().zip(&s.steps) {
            if declared.process() != &captured.process
                || declared.capability() != &captured.capability
                || declared.policy() != &captured.policy
            {
                break 'result ReplayOutcome::VersionMismatch;
            }
            if !captured.capability_available {
                break 'result ReplayOutcome::CapabilityMissing;
            }
            if !captured.process_allowed || captured.process_trace.trim().is_empty() {
                break 'result ReplayOutcome::ProcessDenied;
            }
            if !captured.policy_allowed || captured.policy_trace.trim().is_empty() {
                break 'result ReplayOutcome::PolicyDenied;
            }
        }
        activated = true;
        for step in &s.steps {
            if step.execution != InputStatus::Present {
                break 'result ReplayOutcome::ExecutionFailed;
            }
            completed_steps += 1;
        }
        if procedure
            .verification_evidence()
            .iter()
            .any(|id| s.verification.get(id) != Some(&InputStatus::Present))
        {
            break 'result ReplayOutcome::VerificationFailed;
        }
        ReplayOutcome::Success
    };
    let negative_activation = !matches!(
        case.expected,
        ReplayOutcome::Success | ReplayOutcome::ExecutionFailed | ReplayOutcome::VerificationFailed
    );
    let critical_false_positive = negative_activation && activated;
    CaseResult {
        id: case.id.clone(),
        snapshot_digest: case.snapshot_digest.clone(),
        outcome,
        activated,
        passed: outcome == case.expected && !critical_false_positive,
        critical_false_positive,
        completed_steps,
        fallback: (outcome != ReplayOutcome::Success).then_some(procedure.fallback()),
    }
}

/// Single-variable counterfactuals preserve the original immutable historical case.
/// The baseline must succeed before it can be used to generate meaningful negatives.
pub fn counterfactuals(
    procedure: &LearnedProcedure,
    positive: &ReplayCase,
) -> Result<Vec<ReplayCase>, ValidationError> {
    if positive.snapshot_digest != digest(&positive.snapshot)
        || replay(procedure, positive).outcome != ReplayOutcome::Success
    {
        return Err(invalid(
            "counterfactual baseline must verify intended success",
        ));
    }
    let mut cases = Vec::new();
    let mut add = |suffix: &str, kind, expected, mut snapshot: ReplaySnapshot| {
        snapshot.id = ReferenceId::new(format!("{}.{}", positive.snapshot.id, suffix))
            .expect("derived snapshot ID");
        cases.push(ReplayCase::new(
            ReferenceId::new(format!("{}.{}", positive.id, suffix)).expect("derived case ID"),
            kind,
            expected,
            snapshot,
        ));
    };
    for (index, signal) in procedure.fingerprint().signals().iter().enumerate() {
        let mut s = positive.snapshot.clone();
        s.signals.remove(signal);
        add(
            &format!("near-{index}"),
            CaseKind::NearMatch,
            ReplayOutcome::NotApplicable,
            s,
        );
    }
    let mut s = positive.snapshot.clone();
    s.scope = ContextScopeId::new(format!("{}.other", s.scope)).expect("derived scope");
    add(
        "scope",
        CaseKind::NearMatch,
        ReplayOutcome::NotApplicable,
        s,
    );
    for id in procedure.required_observations() {
        let mut s = positive.snapshot.clone();
        s.observations.remove(id);
        add(
            &format!("observation-{id}"),
            CaseKind::MissingObservation,
            ReplayOutcome::ObservationUnavailable,
            s,
        );
    }
    for id in procedure.required_evidence() {
        for (status, kind, suffix) in [
            (InputStatus::Missing, CaseKind::MissingEvidence, "missing"),
            (InputStatus::Stale, CaseKind::StaleEvidence, "stale"),
            (
                InputStatus::Conflicting,
                CaseKind::ConflictingEvidence,
                "conflict",
            ),
        ] {
            let mut s = positive.snapshot.clone();
            s.evidence.insert(id.clone(), status);
            add(
                &format!("{suffix}-{id}"),
                kind,
                ReplayOutcome::EvidenceUnavailable,
                s,
            );
        }
    }
    for basis in procedure.experience() {
        let mut s = positive.snapshot.clone();
        s.experience.remove(&basis.memory().id);
        add(
            &format!("experience-{}", basis.memory().id),
            CaseKind::IneligibleExperience,
            ReplayOutcome::ExperienceUnavailable,
            s,
        );
    }
    for index in 0..procedure.steps().len() {
        for (kind, expected, suffix) in [
            (
                CaseKind::ProcessDenial,
                ReplayOutcome::ProcessDenied,
                "process",
            ),
            (
                CaseKind::PolicyDenial,
                ReplayOutcome::PolicyDenied,
                "policy",
            ),
            (
                CaseKind::CapabilityMissing,
                ReplayOutcome::CapabilityMissing,
                "capability",
            ),
            (
                CaseKind::VersionChange,
                ReplayOutcome::VersionMismatch,
                "version",
            ),
            (
                CaseKind::ExecutionFailure,
                ReplayOutcome::ExecutionFailed,
                "execution",
            ),
        ] {
            let mut s = positive.snapshot.clone();
            let step = &mut s.steps[index];
            match kind {
                CaseKind::ProcessDenial => step.process_allowed = false,
                CaseKind::PolicyDenial => step.policy_allowed = false,
                CaseKind::CapabilityMissing => step.capability_available = false,
                CaseKind::VersionChange => {
                    step.process = ProcessReference::new(
                        step.process.id().clone(),
                        step.process.version().checked_add(1).unwrap_or(1),
                        step.process.digest().clone(),
                    )?
                }
                CaseKind::ExecutionFailure => step.execution = InputStatus::Failed,
                _ => unreachable!(),
            }
            add(&format!("{suffix}-{index}"), kind, expected, s);
        }
    }
    for id in procedure.verification_evidence() {
        let mut s = positive.snapshot.clone();
        s.verification.insert(id.clone(), InputStatus::Failed);
        add(
            &format!("verification-{id}"),
            CaseKind::VerificationFailure,
            ReplayOutcome::VerificationFailed,
            s,
        );
    }
    Ok(cases)
}
