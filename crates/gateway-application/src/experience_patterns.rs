//! Bounded, read-only correlation of governed execution experience.
use crate::memory::{MemoryApplication, MemoryError, MemoryStore};
use gateway_domain::{
    ContextScopeId, EvidenceId, ReferenceId, UnixTimestamp,
    learning::{ExperienceBasis, FingerprintSignal, PatternCandidate, SituationFingerprint},
    memory::MemoryReason,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// An execution adapter supplies these references after verifying its trace.
/// The memory application independently checks the governed record and revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedExecution {
    pub memory_id: ReferenceId,
    pub runtime: ReferenceId,
    pub trace: ReferenceId,
    pub evaluation: ReferenceId,
    pub validation: ReferenceId,
    pub label_basis: ReferenceId,
    pub evidence: Vec<EvidenceId>,
    pub signals: Vec<FingerprintSignal>,
    /// Optional search hint. It never contributes to the fingerprint or candidate.
    pub semantic_hint: Option<String>,
}

pub trait ExperienceIngestionPort {
    fn list_verified(
        &self,
        scope: &ContextScopeId,
        limits: PatternLimits,
    ) -> Result<Vec<VerifiedExecution>, PatternError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OutcomeClass {
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedExperience {
    pub basis: ExperienceBasis,
    pub runtime: ReferenceId,
    pub fingerprint: SituationFingerprint,
    pub outcome: OutcomeClass,
    pub trace: ReferenceId,
    pub validation: ReferenceId,
    pub label_basis: ReferenceId,
    pub evidence: Vec<EvidenceId>,
    pub observed_at: UnixTimestamp,
    pub semantic_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternLimits {
    pub max_inputs: usize,
    pub max_groups: usize,
    pub max_per_group: usize,
    pub max_signals: usize,
    pub max_evidence: usize,
    pub min_successes: usize,
}

impl Default for PatternLimits {
    fn default() -> Self {
        Self {
            max_inputs: 10_000,
            max_groups: 1_000,
            max_per_group: 100,
            max_signals: 64,
            max_evidence: 64,
            min_successes: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternMetrics {
    pub input_count: usize,
    pub eligible_count: usize,
    pub rejected_count: usize,
    pub retained_count: usize,
    pub sampled_out_count: usize,
    pub group_count: usize,
    pub candidate_count: usize,
}

/// Confidence is explicit evidence counts, never a model score or permission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternFinding {
    pub runtime: ReferenceId,
    pub fingerprint: SituationFingerprint,
    pub observed_success_count: usize,
    pub observed_failure_count: usize,
    pub successes: Vec<NormalizedExperience>,
    pub failures: Vec<NormalizedExperience>,
    pub cross_runtime_matches: Vec<ReferenceId>,
    pub near_matches: Vec<ReferenceId>,
    pub semantic_nominations: Vec<ReferenceId>,
    pub candidate: Option<PatternCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternReport {
    pub scope: ContextScopeId,
    pub inspected_at: UnixTimestamp,
    pub limits: PatternLimits,
    pub findings: Vec<PatternFinding>,
    pub metrics: PatternMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    Memory(MemoryError),
    InvalidLimits,
    TooManyInputs,
    TooManyGroups,
    DuplicateInput,
    InvalidExecution,
    InvalidFingerprint,
    Storage,
}

impl From<MemoryError> for PatternError {
    fn from(value: MemoryError) -> Self {
        Self::Memory(value)
    }
}

/// Revalidates each source before normalization. Ineligible records are counted
/// and skipped; a malformed or mismatched verified trace fails closed.
pub fn inspect_patterns<S: MemoryStore, P: ExperienceIngestionPort>(
    memory: &MemoryApplication<S>,
    port: &P,
    scope: &ContextScopeId,
    at: UnixTimestamp,
    limits: PatternLimits,
) -> Result<PatternReport, PatternError> {
    if limits.max_inputs == 0
        || limits.max_groups == 0
        || limits.max_per_group == 0
        || limits.max_signals == 0
        || limits.max_evidence == 0
        || limits.min_successes < 2
        || limits.min_successes > limits.max_per_group
    {
        return Err(PatternError::InvalidLimits);
    }
    let inputs = port.list_verified(scope, limits)?;
    if inputs.len() > limits.max_inputs {
        return Err(PatternError::TooManyInputs);
    }
    let mut metrics = PatternMetrics {
        input_count: inputs.len(),
        ..PatternMetrics::default()
    };
    let mut seen = BTreeSet::new();
    let mut groups: BTreeMap<(ReferenceId, Vec<FingerprintSignal>), Vec<NormalizedExperience>> =
        BTreeMap::new();
    for input in inputs {
        if !seen.insert(input.memory_id.clone()) {
            return Err(PatternError::DuplicateInput);
        }
        if input.evidence.is_empty()
            || input.evidence.len() > limits.max_evidence
            || input.signals.len() > limits.max_signals
            || input.trace == input.memory_id
            || input
                .semantic_hint
                .as_ref()
                .is_some_and(|hint| hint.len() > 256)
        {
            return Err(PatternError::InvalidExecution);
        }
        let mut evidence = input.evidence;
        evidence.sort();
        if evidence.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(PatternError::InvalidExecution);
        }
        let fingerprint = SituationFingerprint::new(scope.clone(), input.signals)
            .map_err(|_| PatternError::InvalidFingerprint)?;
        let Some(entry) = memory.store().get(scope, &input.memory_id)? else {
            metrics.rejected_count += 1;
            continue;
        };
        if entry.record.scope != *scope {
            return Err(PatternError::InvalidExecution);
        }
        if entry.learning_reasons(at) != [MemoryReason::Eligible] {
            metrics.rejected_count += 1;
            continue;
        }
        if entry.record.source_snapshot != input.trace
            || entry.record.validation.as_ref() != Some(&input.validation)
            || entry.record.label_basis.as_ref() != Some(&input.label_basis)
        {
            return Err(PatternError::InvalidExecution);
        }
        let outcome = match entry.record.outcome.as_ref().map(|x| x.as_str()) {
            Some("SUCCESS") => OutcomeClass::Success,
            Some("FAILURE") => OutcomeClass::Failure,
            _ => return Err(PatternError::InvalidExecution),
        };
        let reference = memory.eligibility_reference(scope, &input.memory_id, at)?;
        let basis =
            ExperienceBasis::new(reference, entry.record.provenance.clone(), input.evaluation)
                .map_err(|_| PatternError::InvalidExecution)?;
        let normalized = NormalizedExperience {
            basis,
            runtime: input.runtime.clone(),
            fingerprint: fingerprint.clone(),
            outcome,
            trace: input.trace,
            validation: input.validation,
            label_basis: input.label_basis,
            evidence,
            observed_at: entry.record.observed_at,
            semantic_hint: input
                .semantic_hint
                .map(|hint| {
                    hint.split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_lowercase()
                })
                .filter(|hint| !hint.is_empty()),
        };
        metrics.eligible_count += 1;
        groups
            .entry((input.runtime, fingerprint.signals().to_vec()))
            .or_default()
            .push(normalized);
        if groups.len() > limits.max_groups {
            return Err(PatternError::TooManyGroups);
        }
    }
    let mut findings = Vec::new();
    for ((runtime, _), mut records) in groups {
        // Keep the most recent records, then restore ID order for stable output.
        records.sort_by(|a, b| {
            b.observed_at
                .cmp(&a.observed_at)
                .then_with(|| a.basis.memory().id.cmp(&b.basis.memory().id))
        });
        let observed_success_count = records
            .iter()
            .filter(|x| x.outcome == OutcomeClass::Success)
            .count();
        let observed_failure_count = records.len() - observed_success_count;
        if records.len() > limits.max_per_group {
            metrics.sampled_out_count += records.len() - limits.max_per_group;
            // Retain the newest failure as negative evidence even when recent
            // successes would otherwise fill the entire bounded sample.
            if let Some(index) = records
                .iter()
                .position(|x| x.outcome == OutcomeClass::Failure)
            {
                let failure = records.remove(index);
                records.truncate(limits.max_per_group - 1);
                records.push(failure);
            } else {
                records.truncate(limits.max_per_group);
            }
        }
        records.sort_by(|a, b| a.basis.memory().id.cmp(&b.basis.memory().id));
        metrics.retained_count += records.len();
        let fingerprint = records[0].fingerprint.clone();
        let mut successes = Vec::new();
        let mut failures = Vec::new();
        for record in records {
            match record.outcome {
                OutcomeClass::Success => successes.push(record),
                OutcomeClass::Failure => failures.push(record),
            }
        }
        let candidate = if successes.len() >= limits.min_successes {
            let bases = successes
                .iter()
                .map(|x| x.basis.clone())
                .collect::<Vec<_>>();
            let id = candidate_id(&runtime, &fingerprint, &bases);
            Some(
                PatternCandidate::new(id, fingerprint.clone(), bases)
                    .map_err(|_| PatternError::InvalidExecution)?,
            )
        } else {
            None
        };
        findings.push(PatternFinding {
            runtime,
            fingerprint,
            observed_success_count,
            observed_failure_count,
            successes,
            failures,
            cross_runtime_matches: Vec::new(),
            near_matches: Vec::new(),
            semantic_nominations: Vec::new(),
            candidate,
        });
    }
    // Comparing groups is bounded by max_groups. Hints nominate inspection only.
    for i in 0..findings.len() {
        for j in (i + 1)..findings.len() {
            let left = &findings[i].fingerprint;
            let right = &findings[j].fingerprint;
            if findings[i].runtime != findings[j].runtime {
                if left.signals() == right.signals() {
                    let (head, tail) = findings.split_at_mut(j);
                    let (a, b) = (&mut head[i], &mut tail[0]);
                    let a_id = a
                        .successes
                        .first()
                        .or_else(|| a.failures.first())
                        .unwrap()
                        .basis
                        .memory()
                        .id
                        .clone();
                    let b_id = b
                        .successes
                        .first()
                        .or_else(|| b.failures.first())
                        .unwrap()
                        .basis
                        .memory()
                        .id
                        .clone();
                    a.cross_runtime_matches.push(b_id);
                    b.cross_runtime_matches.push(a_id);
                }
                continue;
            }
            let overlap = left
                .signals()
                .iter()
                .filter(|s| right.signals().contains(s))
                .count();
            let near =
                overlap > 0 && left.signals().len().max(right.signals().len()) - overlap <= 1;
            let semantic = hints(&findings[i])
                .intersection(&hints(&findings[j]))
                .next()
                .is_some();
            if !near && !semantic {
                continue;
            }
            let (head, tail) = findings.split_at_mut(j);
            let (a, b) = (&mut head[i], &mut tail[0]);
            let a_id = a
                .successes
                .first()
                .or_else(|| a.failures.first())
                .unwrap()
                .basis
                .memory()
                .id
                .clone();
            let b_id = b
                .successes
                .first()
                .or_else(|| b.failures.first())
                .unwrap()
                .basis
                .memory()
                .id
                .clone();
            if near {
                a.near_matches.push(b_id.clone());
                b.near_matches.push(a_id.clone());
            }
            if semantic {
                a.semantic_nominations.push(b_id);
                b.semantic_nominations.push(a_id);
            }
        }
    }
    metrics.group_count = findings.len();
    metrics.candidate_count = findings.iter().filter(|x| x.candidate.is_some()).count();
    Ok(PatternReport {
        scope: scope.clone(),
        inspected_at: at,
        limits,
        findings,
        metrics,
    })
}

fn hints(finding: &PatternFinding) -> BTreeSet<&str> {
    finding
        .successes
        .iter()
        .chain(&finding.failures)
        .filter_map(|x| x.semantic_hint.as_deref())
        .collect()
}

fn candidate_id(
    runtime: &ReferenceId,
    fingerprint: &SituationFingerprint,
    bases: &[ExperienceBasis],
) -> ReferenceId {
    let mut hash = Sha256::new();
    hash.update(
        serde_json::to_vec(&(runtime, fingerprint, bases)).expect("typed candidate key serializes"),
    );
    ReferenceId::new(format!("pattern-{:x}", hash.finalize()))
        .expect("hash is a valid reference ID")
}
