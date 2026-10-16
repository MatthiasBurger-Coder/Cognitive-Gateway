//! EPIC-05.05: epistemic meaning is independent of confidence and authority.
//! These interpretation attachments do not extend the SemanticTaskIR 1.0 wire.

use crate::{
    Confidence, ConflictStatus, Evidence, EvidenceId, EvidenceKind, EvidenceLink, FactId,
    FreshnessStatus, ObservationId, Provenance, QualityMetadata, ReferenceId, ResolvedReference,
    SemanticAssumption, TypedValue, ValidationError,
};

/// A derived conclusion keeps its evidence bindings; it is never itself evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticInference {
    conclusion: TypedValue,
    basis: Vec<ResolvedReference<EvidenceId>>,
}

impl SemanticInference {
    pub fn new(
        conclusion: TypedValue,
        mut basis: Vec<ResolvedReference<EvidenceId>>,
    ) -> Result<Self, ValidationError> {
        conclusion.validate()?;
        basis.sort_by(|a, b| a.id.cmp(&b.id));
        if basis.is_empty() || basis.windows(2).any(|pair| pair[0].id == pair[1].id) {
            return Err(ValidationError::InvalidStateCombination {
                reason: "inference requires distinct evidence bindings",
            });
        }
        Ok(Self { conclusion, basis })
    }

    pub fn conclusion(&self) -> &TypedValue {
        &self.conclusion
    }

    pub fn basis(&self) -> &[ResolvedReference<EvidenceId>] {
        &self.basis
    }
}

/// Different variants cannot be extracted as facts through confidence or casts.
/// All references still require authoritative reference validation at handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EpistemicClaim {
    Fact(ResolvedReference<FactId>),
    Observation(ResolvedReference<ObservationId>),
    Inference(SemanticInference),
    Hypothesis(ResolvedReference<ReferenceId>),
    Assumption(SemanticAssumption),
}

/// Acceptance and retention are orthogonal to CG-06 freshness and conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeVerification {
    Verified,
    Unverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeRetention {
    Active,
    Deprecated,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpistemicUse {
    CurrentKnowledge,
    Inspection,
}

/// Ordered, machine-readable denial reasons; all applicable reasons are retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpistemicDenial {
    NonFactual,
    Unverified,
    Stale,
    FreshnessUnknown,
    Conflicting,
    Deprecated,
    Archived,
}

/// Semantic eligibility only. This never establishes authenticity or permission.
/// Inspection preserves every category, including archived and conflicting data.
pub fn epistemic_denials(
    claim: &EpistemicClaim,
    verification: KnowledgeVerification,
    retention: KnowledgeRetention,
    quality: QualityMetadata,
    usage: EpistemicUse,
) -> Vec<EpistemicDenial> {
    if usage == EpistemicUse::Inspection {
        return Vec::new();
    }
    let mut reasons = Vec::new();
    if !matches!(
        claim,
        EpistemicClaim::Fact(_) | EpistemicClaim::Observation(_)
    ) {
        reasons.push(EpistemicDenial::NonFactual);
    }
    if verification != KnowledgeVerification::Verified {
        reasons.push(EpistemicDenial::Unverified);
    }
    match quality.freshness() {
        FreshnessStatus::Fresh => {}
        FreshnessStatus::Stale => reasons.push(EpistemicDenial::Stale),
        FreshnessStatus::Unknown => reasons.push(EpistemicDenial::FreshnessUnknown),
    }
    if quality.conflict() == ConflictStatus::Unresolved {
        reasons.push(EpistemicDenial::Conflicting);
    }
    match retention {
        KnowledgeRetention::Active => {}
        KnowledgeRetention::Deprecated => reasons.push(EpistemicDenial::Deprecated),
        KnowledgeRetention::Archived => reasons.push(EpistemicDenial::Archived),
    }
    reasons
}

/// Payload-free evidence projection from a trusted, resolved CG-06 source.
/// The host must check the pin's scope, contract, revision and digest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticEvidence {
    reference: ResolvedReference<EvidenceId>,
    provenance: Provenance,
    kind: EvidenceKind,
    links: Vec<EvidenceLink>,
}

impl SemanticEvidence {
    pub fn from_source(
        reference: ResolvedReference<EvidenceId>,
        evidence: &Evidence,
        provenance: &Provenance,
    ) -> Result<Self, ValidationError> {
        if &reference.id != evidence.id() || evidence.provenance() != provenance.id() {
            return Err(ValidationError::InvalidStateCombination {
                reason: "semantic evidence must preserve source identity and provenance",
            });
        }
        Ok(Self {
            reference,
            provenance: provenance.clone(),
            kind: evidence.kind(),
            links: evidence.links().to_vec(),
        })
    }

    pub fn reference(&self) -> &ResolvedReference<EvidenceId> {
        &self.reference
    }
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    pub fn kind(&self) -> EvidenceKind {
        self.kind
    }
    pub fn links(&self) -> &[EvidenceLink] {
        &self.links
    }
}

/// Advisory metadata wraps a result without changing its validation behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterpretationConfidence<T> {
    pub result: T,
    pub confidence: Confidence,
    pub basis: ResolvedReference<ReferenceId>,
}

/// No `Verified` state: validation creates separately admitted knowledge and
/// retires the premise. Historical assumption records remain assumptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssumptionState {
    Proposed,
    Active,
    Discharged,
    Refuted,
    Expired,
    Withdrawn,
}

impl AssumptionState {
    pub fn transition_to(self, next: Self) -> Result<Self, ValidationError> {
        if matches!(
            (self, next),
            (Self::Proposed, Self::Active | Self::Withdrawn)
                | (
                    Self::Active,
                    Self::Discharged | Self::Refuted | Self::Expired | Self::Withdrawn
                )
        ) {
            Ok(next)
        } else {
            Err(ValidationError::InvalidStateCombination {
                reason: "invalid semantic assumption lifecycle transition",
            })
        }
    }
}
