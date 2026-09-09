//! CG-08.03 exact capability discovery. Matching is not selection or permission.

use crate::{
    resolution::{ContentFingerprint, ResolutionBasis},
    resolution_snapshot::ResolutionSnapshot,
};
use gateway_domain::{
    CapabilityClass, CapabilityPrecondition, CapabilityRequirementId, PlanStepId,
    RequiredOutcomeKind, RequirementCardinality, SchemaVersion,
};
use gateway_registry::{
    CapabilityCandidate, CapabilityQuery, CapabilityRejection, CapabilitySelector,
};
use std::collections::{BTreeMap, BTreeSet};

/// Additional conjunctive constraints, never a replacement capability identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateRules {
    pub version: SchemaVersion,
    pub selectors: BTreeMap<CapabilityRequirementId, BTreeSet<CapabilitySelector>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateError {
    UnsupportedVersion,
    UnknownRequirement,
    CapabilitySubstitution,
    InvalidCatalog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateOutcome {
    Compatible,
    UnknownCapability,
    MissingProvider,
    Incompatible,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredCandidate {
    pub canonical: CapabilityCandidate,
    pub definition_fingerprint: ContentFingerprint,
    /// Metadata presence is not evidence that the precondition is satisfied.
    pub unresolved_preconditions: BTreeSet<CapabilityPrecondition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateSet {
    pub step: PlanStepId,
    pub requirement: CapabilityRequirementId,
    pub cardinality: RequirementCardinality,
    pub query: CapabilityQuery,
    pub outcome: CandidateOutcome,
    pub candidates: Vec<DiscoveredCandidate>,
    pub rejections: Vec<CapabilityRejection>,
}

/// Retains rules and source basis so later composition can reproduce discovery.
/// Optional failures remain diagnostic; only composition can justify omission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateDiscovery {
    pub basis: ResolutionBasis,
    pub rules: CandidateRules,
    pub sets: Vec<CandidateSet>,
}

pub fn discover_candidates(
    snapshot: &ResolutionSnapshot,
    rules: &CandidateRules,
) -> Result<CandidateDiscovery, CandidateError> {
    if rules.version != SchemaVersion::V1 {
        return Err(CandidateError::UnsupportedVersion);
    }
    let plan = snapshot.request().plan();
    for (id, selectors) in &rules.selectors {
        let requirement = plan
            .capability_requirements()
            .iter()
            .find(|r| r.id() == id)
            .ok_or(CandidateError::UnknownRequirement)?;
        if selectors.iter().any(
            |s| matches!(s, CapabilitySelector::CapabilityId(id) if id != requirement.capability()),
        ) {
            return Err(CandidateError::CapabilitySubstitution);
        }
    }
    let index = &snapshot.input().index;
    let registry = &snapshot.input().registry;
    let mut sets = Vec::new();
    for step in plan.steps() {
        for id in step.capability_requirements() {
            let requirement = plan
                .capability_requirements()
                .iter()
                .find(|r| r.id() == id)
                .ok_or(CandidateError::UnknownRequirement)?;
            let class = if step.outcome().kind() == RequiredOutcomeKind::DomainChange {
                CapabilityClass::Mutate
            } else {
                CapabilityClass::Inspect
            };
            let mut query =
                CapabilityQuery::for_capability(requirement.capability().clone()).with_class(class);
            for condition in requirement.preconditions() {
                query = query.with_selector(CapabilitySelector::Precondition(condition.clone()));
            }
            for constraint in requirement.constraints() {
                query = query.with_selector(CapabilitySelector::Constraint(constraint.clone()));
            }
            for selector in rules.selectors.get(id).into_iter().flatten() {
                query = query.with_selector(selector.clone());
            }
            let matches = index.query(&query);
            let outcome = if !matches.matches().is_empty() {
                CandidateOutcome::Compatible
            } else if index.contains(requirement.capability()) {
                CandidateOutcome::Incompatible
            } else if registry.skills().iter().any(|s| {
                s.required_capability_ids()
                    .contains(requirement.capability())
            }) {
                // A canonical required-capability reference declares a need,
                // but supplies neither a provider nor a provided contract.
                CandidateOutcome::MissingProvider
            } else {
                CandidateOutcome::UnknownCapability
            };
            let mut candidates = Vec::new();
            for canonical in matches.matches() {
                let document = match canonical.provider() {
                    gateway_registry::CapabilityProvider::Agent { agent_id } => registry
                        .agent(agent_id)
                        .ok_or(CandidateError::InvalidCatalog)?
                        .to_json(),
                    gateway_registry::CapabilityProvider::Skill { skill_id } => registry
                        .skill(skill_id)
                        .ok_or(CandidateError::InvalidCatalog)?
                        .to_json(),
                }
                .map_err(|_| CandidateError::InvalidCatalog)?;
                candidates.push(DiscoveredCandidate {
                    canonical: canonical.clone(),
                    definition_fingerprint: ContentFingerprint::of_bytes(document.as_bytes()),
                    unresolved_preconditions: canonical
                        .capability()
                        .preconditions()
                        .iter()
                        .chain(requirement.preconditions())
                        .cloned()
                        .collect(),
                });
            }
            sets.push(CandidateSet {
                step: step.id().clone(),
                requirement: id.clone(),
                cardinality: requirement.cardinality(),
                query,
                outcome,
                candidates,
                rejections: matches.rejections().to_vec(),
            });
        }
    }
    Ok(CandidateDiscovery {
        basis: snapshot.request().basis().clone(),
        rules: rules.clone(),
        sets,
    })
}
