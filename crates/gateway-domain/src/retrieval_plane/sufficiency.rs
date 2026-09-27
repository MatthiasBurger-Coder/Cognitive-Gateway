use super::{RequiredInformation, RetrievedFragment};
use crate::{
    ConflictStatus, EvidenceId, FreshnessRequirement, FreshnessStatus, ProvenanceId, ReferenceId,
    Uncertainty,
};
use std::collections::BTreeSet;

/// A finding is retained even when another finding has higher display priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SufficiencyFinding {
    Sufficient,
    Partial,
    Insufficient,
    Conflicting,
    Stale,
    Untrusted,
    Contaminated,
    BudgetExhausted,
}

/// Only the owning evidence boundary may fill `validated_evidence`. A retrieval
/// fragment's unvalidated links and relevance score never establish a claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssessedFragment {
    pub fragment: RetrievedFragment,
    pub validated_evidence: BTreeSet<EvidenceId>,
    pub contaminated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SufficiencyAssessment {
    pub state: SufficiencyFinding,
    pub findings: BTreeSet<SufficiencyFinding>,
    pub accepted: BTreeSet<ReferenceId>,
    pub rejected: BTreeSet<ReferenceId>,
    pub validated_evidence: BTreeSet<EvidenceId>,
    pub missing_evidence: BTreeSet<EvidenceId>,
    pub missing_provenance: BTreeSet<ProvenanceId>,
    pub missing_evidence_count: u64,
}

/// Deterministic assessment of one explicit information requirement. Unknown
/// freshness fails a Fresh requirement. Conflict and contamination remain
/// blockers even if another fragment appears to cover the same evidence.
pub fn assess_sufficiency(
    required: &RequiredInformation,
    fragments: &[AssessedFragment],
    budget_exhausted: bool,
) -> SufficiencyAssessment {
    assess_sufficiency_with_threshold(required, fragments, 1, budget_exhausted)
}

/// The threshold also represents an explicit distinct-evidence requirement
/// when a retrieval plan uses `StopCondition::EvidenceSatisfied`.
pub fn assess_sufficiency_with_threshold(
    required: &RequiredInformation,
    fragments: &[AssessedFragment],
    minimum_distinct_evidence: u64,
    budget_exhausted: bool,
) -> SufficiencyAssessment {
    let mut findings = BTreeSet::new();
    let mut accepted = BTreeSet::new();
    let mut rejected = BTreeSet::new();
    let mut validated_evidence = BTreeSet::new();
    let mut provenances = BTreeSet::new();
    for candidate in fragments {
        let fragment = &candidate.fragment;
        let quality = fragment.quality;
        let mut valid = true;
        if candidate.contaminated {
            findings.insert(SufficiencyFinding::Contaminated);
            valid = false;
        }
        if quality.uncertainty() != Uncertainty::None {
            findings.insert(SufficiencyFinding::Untrusted);
            valid = false;
        }
        if quality.conflict() != ConflictStatus::None {
            findings.insert(SufficiencyFinding::Conflicting);
            valid = false;
        }
        if required.requirements.freshness() == FreshnessRequirement::Fresh
            && quality.freshness() != FreshnessStatus::Fresh
        {
            findings.insert(SufficiencyFinding::Stale);
            valid = false;
        }
        if !required.accepted_trust.contains(&quality.trust())
            || quality.sensitivity() > required.maximum_sensitivity
            || required
                .requirements
                .minimum_sensitivity()
                .is_some_and(|minimum| quality.sensitivity() < minimum)
        {
            findings.insert(SufficiencyFinding::Untrusted);
            valid = false;
        }
        if !candidate.validated_evidence.is_subset(&fragment.evidence) {
            findings.insert(SufficiencyFinding::Contaminated);
            valid = false;
        }
        if valid && !candidate.validated_evidence.is_empty() {
            accepted.insert(fragment.id.clone());
            validated_evidence.extend(candidate.validated_evidence.iter().cloned());
            provenances.insert(fragment.provenance.id().clone());
        } else {
            rejected.insert(fragment.id.clone());
        }
    }
    let missing_evidence: BTreeSet<_> = required
        .requirements
        .evidence()
        .iter()
        .filter(|id| !validated_evidence.contains(*id))
        .cloned()
        .collect();
    let missing_provenance: BTreeSet<_> = required
        .requirements
        .provenances()
        .iter()
        .filter(|id| !provenances.contains(*id))
        .cloned()
        .collect();
    let missing_evidence_count = minimum_distinct_evidence
        .max(1)
        .saturating_sub(validated_evidence.len() as u64);
    let covered =
        missing_evidence_count == 0 && missing_evidence.is_empty() && missing_provenance.is_empty();
    let blocked = [
        SufficiencyFinding::Contaminated,
        SufficiencyFinding::Conflicting,
        SufficiencyFinding::Stale,
        SufficiencyFinding::Untrusted,
    ]
    .into_iter()
    .any(|finding| findings.contains(&finding));
    let coverage = if covered && !blocked {
        SufficiencyFinding::Sufficient
    } else if !validated_evidence.is_empty() {
        SufficiencyFinding::Partial
    } else {
        SufficiencyFinding::Insufficient
    };
    findings.insert(coverage);
    if budget_exhausted && coverage != SufficiencyFinding::Sufficient {
        findings.insert(SufficiencyFinding::BudgetExhausted);
    }
    let state = [
        SufficiencyFinding::Contaminated,
        SufficiencyFinding::Conflicting,
        SufficiencyFinding::BudgetExhausted,
        SufficiencyFinding::Stale,
        SufficiencyFinding::Untrusted,
        SufficiencyFinding::Sufficient,
        SufficiencyFinding::Partial,
        SufficiencyFinding::Insufficient,
    ]
    .into_iter()
    .find(|finding| findings.contains(finding))
    .expect("coverage finding exists");
    SufficiencyAssessment {
        state,
        findings,
        accepted,
        rejected,
        validated_evidence,
        missing_evidence,
        missing_provenance,
        missing_evidence_count,
    }
}
