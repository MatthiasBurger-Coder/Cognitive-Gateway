//! CG-08.04 optional template selection over compiled canonical definitions.

use crate::{
    resolution::{ProcessBinding, ResolutionBasis},
    resolution_snapshot::ResolutionSnapshot,
};
use gateway_domain::{LifecycleRequirementKind, PlanStepId, RequirementCardinality, SchemaVersion};
use gateway_process::{
    ActivityConstraint, ActivityDefinition, ActivityId, DefinitionIdentity, EvidenceTypeId,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplatePreference {
    None,
    Optional,
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSelectionRules {
    pub version: SchemaVersion,
    pub preference: TemplatePreference,
    pub required_definition: Option<DefinitionIdentity>,
    pub activities: BTreeMap<PlanStepId, ActivityId>,
    pub output_evidence: BTreeMap<PlanStepId, BTreeSet<EvidenceTypeId>>,
    /// Explicit rule maps a Plan lifecycle kind to a required canonical activity
    /// contract. A missing rule is unsupported, never inferred from prose.
    pub lifecycle_contracts: BTreeMap<LifecycleRequirementKind, ActivityConstraint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessSelectionError {
    UnsupportedVersion,
    UnknownStep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessSelectionOutcome {
    NoTemplate,
    Unique,
    Ambiguous,
    Missing,
    Incompatible,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessRejectionReason {
    DefinitionConstraint,
    PinnedDefinition,
    ActivityContract,
    UnsupportedLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCandidate {
    pub binding: ProcessBinding,
    /// All compatible declared activities are retained; no first-activity tie break.
    pub activities: BTreeMap<PlanStepId, Vec<ActivityDefinition>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRejection {
    pub definition: DefinitionIdentity,
    pub step: Option<PlanStepId>,
    pub reason: ProcessRejectionReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSelection {
    pub basis: ResolutionBasis,
    pub rules: ProcessSelectionRules,
    pub outcome: ProcessSelectionOutcome,
    pub candidates: Vec<ProcessCandidate>,
    pub rejections: Vec<ProcessRejection>,
}

pub fn select_process(
    snapshot: &ResolutionSnapshot,
    rules: &ProcessSelectionRules,
) -> Result<ProcessSelection, ProcessSelectionError> {
    if rules.version != SchemaVersion::V1 {
        return Err(ProcessSelectionError::UnsupportedVersion);
    }
    let plan = snapshot.request().plan();
    for id in rules.activities.keys().chain(rules.output_evidence.keys()) {
        if !plan.steps().iter().any(|s| s.id() == id) {
            return Err(ProcessSelectionError::UnknownStep);
        }
    }
    let mut result = ProcessSelection {
        basis: snapshot.request().basis().clone(),
        rules: rules.clone(),
        outcome: ProcessSelectionOutcome::NoTemplate,
        candidates: vec![],
        rejections: vec![],
    };
    let lifecycle_required = plan
        .steps()
        .iter()
        .any(|s| s.lifecycle_requirement().is_some());
    let mandatory = lifecycle_required
        || rules.preference == TemplatePreference::Required
        || snapshot.process().is_some()
        || rules.required_definition.is_some()
        || !rules.activities.is_empty()
        || !rules.output_evidence.is_empty();
    if !mandatory && rules.preference == TemplatePreference::None {
        return Ok(result);
    }
    let unsupported = plan.steps().iter().any(|s| {
        s.lifecycle_requirement()
            .is_some_and(|r| !rules.lifecycle_contracts.contains_key(&r.kind()))
    });
    for definition in snapshot.input().processes.definitions() {
        let identity = definition.identity();
        let pinned = snapshot.process();
        let reason = if pinned.is_some_and(|p| {
            p.definition_id() != identity.id()
                || p.definition_version() != identity.version()
                || p.definition_digest() != identity.digest()
        }) {
            Some(ProcessRejectionReason::PinnedDefinition)
        } else if rules
            .required_definition
            .as_ref()
            .is_some_and(|id| id != identity)
        {
            Some(ProcessRejectionReason::DefinitionConstraint)
        } else if unsupported {
            Some(ProcessRejectionReason::UnsupportedLifecycle)
        } else {
            None
        };
        if let Some(reason) = reason {
            result.rejections.push(ProcessRejection {
                definition: identity.clone(),
                step: None,
                reason,
            });
            continue;
        }
        let mut activities = BTreeMap::new();
        let mut compatible = true;
        for step in plan
            .steps()
            .iter()
            .filter(|s| s.kind() != gateway_domain::PlanStepKind::NoOp)
        {
            let matches = definition
                .activities()
                .iter()
                .filter(|activity| {
                    if rules
                        .activities
                        .get(step.id())
                        .is_some_and(|id| id != activity.id())
                    {
                        return false;
                    }
                    if rules.output_evidence.get(step.id()).is_some_and(|outputs| {
                        outputs
                            .iter()
                            .any(|id| !activity.output_evidence().contains(id))
                    }) {
                        return false;
                    }
                    if step.lifecycle_requirement().is_some_and(|requirement| {
                        !rules
                            .lifecycle_contracts
                            .get(&requirement.kind())
                            .is_some_and(|c| activity.constraints().contains(c))
                    }) {
                        return false;
                    }
                    let groups = snapshot
                        .request()
                        .alternatives()
                        .iter()
                        .filter(|g| &g.step == step.id())
                        .collect::<Vec<_>>();
                    for requirement in plan
                        .capability_requirements()
                        .iter()
                        .filter(|r| step.capability_requirements().contains(r.id()))
                    {
                        if requirement.cardinality() == RequirementCardinality::Mandatory
                            && !groups.iter().any(|g| g.members.contains(requirement.id()))
                            && !activity.capabilities().contains(requirement.capability())
                        {
                            return false;
                        }
                    }
                    for group in groups {
                        if group.cardinality == RequirementCardinality::Mandatory
                            && !plan.capability_requirements().iter().any(|r| {
                                group.members.contains(r.id())
                                    && activity.capabilities().contains(r.capability())
                            })
                        {
                            return false;
                        }
                    }
                    true
                })
                .cloned()
                .collect::<Vec<_>>();
            if matches.is_empty() {
                compatible = false;
                result.rejections.push(ProcessRejection {
                    definition: identity.clone(),
                    step: Some(step.id().clone()),
                    reason: ProcessRejectionReason::ActivityContract,
                });
            }
            activities.insert(step.id().clone(), matches);
        }
        if compatible {
            result.candidates.push(ProcessCandidate {
                binding: ProcessBinding {
                    definition: identity.clone(),
                    instance: pinned.map(|p| (p.instance_id().clone(), p.instance_revision())),
                },
                activities,
            });
        }
    }
    result.outcome = match result.candidates.len() {
        1 => ProcessSelectionOutcome::Unique,
        2.. => ProcessSelectionOutcome::Ambiguous,
        _ if unsupported => ProcessSelectionOutcome::Unsupported,
        _ if !mandatory => ProcessSelectionOutcome::NoTemplate,
        _ if snapshot.input().processes.is_empty() => ProcessSelectionOutcome::Missing,
        _ => ProcessSelectionOutcome::Incompatible,
    };
    Ok(result)
}
