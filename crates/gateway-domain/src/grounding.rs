//! Grounding contracts v1. Trusted projections are never model acceptance claims.
use crate::{output_verification::VerificationBinding, *};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GroundingError {
    Unavailable,
    IncompatibleVersion,
    CorrelationMismatch,
    UnsupportedClaim,
    InvalidCitation,
    CrossScopeEvidence,
    StaleEvidence,
    ContradictoryEvidence,
    UntrustedEvidence,
    SensitivityViolation,
    ExternalEffectUnproven,
}

/// The task-specific verifier projects *all* factual assertions and citations
/// from the complete payload, including tool arguments. Never deserialize this
/// manifest from a model's self-declared list of claims.
#[derive(Debug, Clone)]
pub struct GroundingClaim {
    pub pointer: String,
    pub wire_value: Value,
    pub subject: SubjectPath,
    pub value: TypedValue,
    pub fact: FactId,
    pub citations: Vec<ResolvedReference<EvidenceId>>,
    /// Exact host-issued action identity, including revision and scope.
    pub external_effect: Option<ResolvedReference<ReferenceId>>,
}

#[derive(Debug, Clone)]
pub struct GroundingManifest {
    pub schema_version: SchemaVersion,
    pub binding: VerificationBinding,
    pub claims: Vec<GroundingClaim>,
    pub evaluated_at: UnixTimestamp,
    pub freshness: FreshnessPolicy,
    pub allowed_trust: Vec<TrustClass>,
}

/// Current evidence-owner snapshot. Pin authentication and completeness of the
/// CG-06 support/challenge set are obligations of the trusted store adapter.
#[derive(Debug, Clone)]
pub struct GroundingEvidence {
    pub binding: ResolvedReference<EvidenceId>,
    /// Source and evidence record have independent version histories.
    pub source: ResolvedReference<SourceId>,
    pub knowledge: KnowledgeProvenance,
    pub lineage: ObservationEvidenceSet,
    pub quality: QualityMetadata,
    pub observed_at: Option<UnixTimestamp>,
    /// Supplied only by the independent effect observer, never by model output.
    pub observed_effect: Option<ResolvedReference<ReferenceId>>,
}

/// Bind the trusted projection to exact bytes and actual payload locations.
pub fn validate_manifest(
    manifest: &GroundingManifest,
    binding: &VerificationBinding,
    result: &Value,
) -> Result<(), GroundingError> {
    use GroundingError as E;
    if manifest.schema_version != SchemaVersion::V1 {
        return Err(E::IncompatibleVersion);
    }
    if &manifest.binding != binding {
        return Err(E::CorrelationMismatch);
    }
    if manifest.claims.is_empty() {
        return Err(E::UnsupportedClaim);
    }
    let mut pointers = std::collections::BTreeSet::new();
    for claim in &manifest.claims {
        if !pointers.insert(&claim.pointer)
            || result.pointer(&claim.pointer) != Some(&claim.wire_value)
            || claim.value.validate().is_err()
            || claim.citations.is_empty()
        {
            return Err(E::UnsupportedClaim);
        }
        let mut citations = std::collections::BTreeSet::new();
        for pin in &claim.citations {
            if !citations.insert(&pin.id)
                || crate::model_invocation::validate_model_binding(pin).is_err()
            {
                return Err(E::InvalidCitation);
            }
        }
    }
    Ok(())
}
