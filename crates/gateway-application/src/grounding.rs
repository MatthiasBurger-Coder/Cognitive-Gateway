//! Deterministic grounding over current EPIC-02 and CG-06 evidence; no effects.
use crate::{output_verification::ParsedOutput, prompt_ir::InvocationIr};
use gateway_domain::{grounding::*, *};

/// Trusted evidence-owner integration. No permissive implementation is supplied.
pub trait GroundingEvidencePort {
    /// Inspect the complete result and tool arguments using pinned task rules.
    /// Project every assertion and citation; omission is a verifier defect.
    fn manifest(
        &self,
        ir: &InvocationIr,
        output: &ParsedOutput,
    ) -> Result<GroundingManifest, GroundingError>;
    /// Authenticate the exact pin against current storage and return the full
    /// relevant challenge/support basis, including uncited contradictory facts.
    /// Validate parent provenance and prevent model-derived source laundering.
    fn resolve(
        &self,
        ir: &InvocationIr,
        citation: &ResolvedReference<EvidenceId>,
    ) -> Result<GroundingEvidence, GroundingError>;
}

pub fn verify_grounding(
    ir: &InvocationIr,
    output: &ParsedOutput,
    host: &dyn GroundingEvidencePort,
) -> Result<(), GroundingError> {
    use GroundingError as E;
    if output.binding().invocation != ir.identity() {
        return Err(E::CorrelationMismatch);
    }
    let manifest = host.manifest(ir, output)?;
    validate_manifest(&manifest, &output.binding(), &output.data().result)?;
    let invocation = ir.prepared().invocation().data();
    for claim in &manifest.claims {
        for citation in &claim.citations {
            if citation.scope != invocation.task.scope {
                return Err(E::CrossScopeEvidence);
            }
            let snapshot = host.resolve(ir, citation)?;
            if snapshot.source.scope != invocation.task.scope {
                return Err(E::CrossScopeEvidence);
            }
            if snapshot.binding != *citation
                || gateway_domain::model_invocation::validate_model_binding(&snapshot.source)
                    .is_err()
                || snapshot.knowledge.revision() != Some(snapshot.source.revision.as_str())
                || snapshot.lineage.validate().is_err()
            {
                return Err(E::InvalidCitation);
            }
            let evidence = snapshot
                .lineage
                .evidence()
                .iter()
                .find(|item| item.id() == &citation.id)
                .ok_or(E::InvalidCitation)?;
            let provenance = snapshot
                .lineage
                .provenances()
                .iter()
                .find(|item| item.id() == evidence.provenance())
                .ok_or(E::InvalidCitation)?;
            if snapshot.knowledge.source() != provenance.source_reference()
                || &snapshot.source.id != provenance.source_id()
            {
                return Err(E::InvalidCitation);
            }
            if !manifest.allowed_trust.contains(&snapshot.quality.trust())
                || matches!(
                    provenance.source_kind(),
                    SourceKind::Model | SourceKind::Synthetic
                )
                || matches!(
                    evidence.kind(),
                    EvidenceKind::ModelOutput | EvidenceKind::SyntheticFixture
                )
            {
                return Err(E::UntrustedEvidence);
            }
            if snapshot.quality.sensitivity() > invocation.sensitivity {
                return Err(E::SensitivityViolation);
            }
            if snapshot.quality.freshness() != FreshnessStatus::Fresh
                || evaluate_freshness(
                    snapshot.observed_at,
                    Some(manifest.evaluated_at),
                    manifest.freshness,
                ) != Ok(FreshnessStatus::Fresh)
            {
                return Err(E::StaleEvidence);
            }
            if snapshot.quality.conflict() != ConflictStatus::None
                || snapshot.quality.uncertainty() != Uncertainty::None
                || snapshot.lineage.facts().iter().any(|fact| {
                    fact.subject() == &claim.subject
                        && (fact.value() != &claim.value
                            || fact.polarity() != AssertionPolarity::Affirmed)
                })
                || snapshot.lineage.evidence().iter().any(|item| {
                    item.links().iter().any(|link| {
                        link.fact() == &claim.fact
                            && link.relation() == EvidenceRelation::Challenges
                    })
                })
            {
                return Err(E::ContradictoryEvidence);
            }
            let fact = snapshot
                .lineage
                .facts()
                .iter()
                .find(|fact| fact.id() == &claim.fact)
                .ok_or(E::UnsupportedClaim)?;
            if fact.subject() != &claim.subject
                || fact.value() != &claim.value
                || !evidence.links().iter().any(|link| {
                    link.fact() == &claim.fact && link.relation() == EvidenceRelation::Supports
                })
            {
                return Err(E::UnsupportedClaim);
            }
            if let Some(action) = &claim.external_effect {
                if action.scope != invocation.task.scope
                    || gateway_domain::model_invocation::validate_model_binding(action).is_err()
                    || snapshot.observed_effect.as_ref() != Some(action)
                    || snapshot.quality.trust() != TrustClass::ObservedEvidence
                    || !matches!(
                        provenance.source_kind(),
                        SourceKind::Tool | SourceKind::Runtime | SourceKind::Ci
                    )
                    || !matches!(
                        evidence.kind(),
                        EvidenceKind::ToolOutput
                            | EvidenceKind::TestResult
                            | EvidenceKind::Measurement
                            | EvidenceKind::Artifact
                    )
                    || !snapshot.lineage.observations().iter().any(|observation| {
                        fact.observations().contains(observation.id())
                            && observation.provenance() == evidence.provenance()
                            && observation.subject() == &claim.subject
                            && observation.value() == &claim.value
                    })
                {
                    return Err(E::ExternalEffectUnproven);
                }
            }
        }
    }
    Ok(())
}
