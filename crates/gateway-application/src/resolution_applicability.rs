//! Snapshot-only applicability; eligibility is never execution authorization.
use crate::{
    resolution::{LifecycleReadiness, ResolutionBasis},
    resolution_candidates::{CandidateRules, discover_candidates},
    resolution_skills::{ConditionStatus, SkillCondition, evaluate_skill_condition},
    resolution_snapshot::ResolutionSnapshot,
};
use gateway_domain::{
    CapabilityClass, CapabilityId, CapabilityRequirementId, EvidenceId, FreshnessStatus,
    PlanCondition, PlanStep, PlanStepId, PlanStepKind, SchemaVersion,
};
use gateway_process::{ActivityId, GateStatus, ProcessInstanceStatus};
use gateway_registry::CapabilityProvider;
use std::collections::{BTreeMap, BTreeSet};

/// An explicit runtime attestation, not a conclusion inferred from a binding.
/// The caller must obtain it from its completion-evidence authority. The resolver
/// checks scope/basis, exact contracts and quality, but does not authenticate it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionEvidence {
    pub basis: ResolutionBasis,
    pub contracts: BTreeSet<PlanCondition>,
    pub references: BTreeSet<EvidenceId>,
    pub status: ConditionStatus,
    /// Already evaluated by the source using explicit time/rules, never our clock.
    pub freshness: FreshnessStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicabilityRules {
    pub version: SchemaVersion,
    /// Every condition is conjunctive. Keys identify the imposing source.
    pub restrictions: BTreeMap<PlanStepId, BTreeMap<String, Vec<SkillCondition>>>,
    /// Exact canonical precondition/constraint text -> explicit supported semantics.
    pub semantics: BTreeMap<String, SkillCondition>,
    pub completed: BTreeMap<PlanStepId, CompletionEvidence>,
    pub activities: BTreeMap<PlanStepId, ActivityId>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ApplicabilityReason {
    PredecessorPending(PlanStepId),
    InvalidCompletion(PlanStepId),
    Prerequisite(usize, ConditionStatus),
    Restriction(String, ConditionStatus),
    ProcessUnavailable,
    ProcessStatus(ProcessInstanceStatus),
    Gate(String, GateStatus),
    Blocker(String),
    Waiting,
    ActivityUnavailable,
    CapabilityUnavailable(CapabilityId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicabilityError {
    UnsupportedVersion,
    UnknownStep,
    InvalidProvider,
    InvalidRules,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepApplicability {
    pub basis: ResolutionBasis,
    /// Preserves all DAG edges, completion, verification and lifecycle contracts.
    pub step: PlanStep,
    pub readiness: LifecycleReadiness,
    pub reasons: BTreeSet<ApplicabilityReason>,
}

pub fn evaluate_applicability(
    snapshot: &ResolutionSnapshot,
    step_id: &PlanStepId,
    chosen: &BTreeMap<CapabilityRequirementId, CapabilityProvider>,
    extra_capabilities: &BTreeSet<CapabilityId>,
    candidate_rules: &CandidateRules,
    rules: &ApplicabilityRules,
) -> Result<StepApplicability, ApplicabilityError> {
    if rules.version != SchemaVersion::V1 {
        return Err(ApplicabilityError::UnsupportedVersion);
    }
    let plan = snapshot.request().plan();
    let step = plan
        .steps()
        .iter()
        .find(|s| s.id() == step_id)
        .ok_or(ApplicabilityError::UnknownStep)?;
    if rules
        .restrictions
        .keys()
        .chain(rules.completed.keys())
        .chain(rules.activities.keys())
        .any(|id| !plan.steps().iter().any(|s| s.id() == id))
    {
        return Err(ApplicabilityError::UnknownStep);
    }
    let discovery = discover_candidates(snapshot, candidate_rules)
        .map_err(|_| ApplicabilityError::InvalidRules)?;
    let mut reasons = BTreeSet::new();
    let mut capabilities = extra_capabilities.clone();
    for (id, provider) in chosen {
        let candidate = discovery
            .sets
            .iter()
            .filter(|s| &s.step == step_id && &s.requirement == id)
            .flat_map(|s| &s.candidates)
            .find(|c| c.canonical.provider() == provider)
            .ok_or(ApplicabilityError::InvalidProvider)?;
        let contract = candidate.canonical.capability();
        capabilities.insert(contract.id().clone());
        for text in contract
            .preconditions()
            .iter()
            .map(|c| c.as_str())
            .chain(contract.constraints().iter().map(|c| c.as_str()))
        {
            let status = if text == "read-only" {
                if contract.class() == CapabilityClass::Inspect {
                    ConditionStatus::Satisfied
                } else {
                    ConditionStatus::Unsatisfied
                }
            } else {
                rules
                    .semantics
                    .get(text)
                    .map_or(ConditionStatus::Unsupported, |c| {
                        evaluate_skill_condition(snapshot, c)
                    })
            };
            if status != ConditionStatus::Satisfied {
                reasons.insert(ApplicabilityReason::Restriction(text.to_owned(), status));
            }
        }
    }
    for predecessor in step.dependencies() {
        let original = plan
            .steps()
            .iter()
            .find(|s| s.id() == predecessor)
            .ok_or(ApplicabilityError::InvalidRules)?;
        match rules.completed.get(predecessor) {
            None => {
                reasons.insert(ApplicabilityReason::PredecessorPending(predecessor.clone()));
            }
            Some(proof) if completion_valid(snapshot, original, proof) => {}
            Some(_) => {
                reasons.insert(ApplicabilityReason::InvalidCompletion(predecessor.clone()));
            }
        }
    }
    let pending = !reasons.iter().all(|r| {
        !matches!(
            r,
            ApplicabilityReason::PredecessorPending(_) | ApplicabilityReason::InvalidCompletion(_)
        )
    });
    // Future conditions may be established by predecessors. Do not evaluate them
    // against today's state and erase a valid future binding.
    if !pending {
        for (index, condition) in step.prerequisites().iter().enumerate() {
            let attested = step
                .dependencies()
                .iter()
                .filter_map(|id| rules.completed.get(id))
                .any(|p| p.contracts.contains(condition));
            let status = if attested {
                ConditionStatus::Satisfied
            } else {
                match condition {
                    PlanCondition::DesiredCondition(id) => evaluate_skill_condition(
                        snapshot,
                        &SkillCondition::DesiredCondition(id.clone()),
                    ),
                    PlanCondition::Outcome(_) => ConditionStatus::Unknown,
                }
            };
            if status != ConditionStatus::Satisfied {
                reasons.insert(ApplicabilityReason::Prerequisite(index, status));
            }
        }
    }
    for (source, conditions) in rules.restrictions.get(step_id).into_iter().flatten() {
        for condition in conditions {
            let status = evaluate_skill_condition(snapshot, condition);
            if status != ConditionStatus::Satisfied {
                reasons.insert(ApplicabilityReason::Restriction(source.clone(), status));
            }
        }
    }
    if step.kind() != PlanStepKind::NoOp {
        check_process(snapshot, step, &capabilities, rules, &mut reasons);
    }
    let readiness = if step.kind() == PlanStepKind::NoOp {
        LifecycleReadiness::NotApplicable
    } else if reasons.is_empty() {
        LifecycleReadiness::Eligible
    } else if reasons.iter().any(|r| {
        matches!(
            r,
            ApplicabilityReason::Restriction(
                _,
                ConditionStatus::Unsatisfied | ConditionStatus::Conflicted
            ) | ApplicabilityReason::ProcessStatus(_)
                | ApplicabilityReason::Gate(_, _)
                | ApplicabilityReason::Blocker(_)
                | ApplicabilityReason::Waiting
                | ApplicabilityReason::ActivityUnavailable
                | ApplicabilityReason::CapabilityUnavailable(_)
        )
    }) {
        LifecycleReadiness::Blocked
    } else if pending {
        LifecycleReadiness::Deferred
    } else {
        LifecycleReadiness::Unknown
    };
    Ok(StepApplicability {
        basis: snapshot.request().basis().clone(),
        step: step.clone(),
        readiness,
        reasons,
    })
}

fn completion_valid(
    snapshot: &ResolutionSnapshot,
    step: &PlanStep,
    proof: &CompletionEvidence,
) -> bool {
    proof.basis == *snapshot.request().basis()
        && !proof.references.is_empty()
        && proof.status == ConditionStatus::Satisfied
        && proof.freshness == FreshnessStatus::Fresh
        && proof.contracts.contains(step.completion())
        && step
            .verification()
            .is_none_or(|v| proof.contracts.contains(v))
        && proof
            .contracts
            .iter()
            .all(|c| c == step.completion() || Some(c) == step.verification())
}

fn check_process(
    snapshot: &ResolutionSnapshot,
    step: &PlanStep,
    capabilities: &BTreeSet<CapabilityId>,
    rules: &ApplicabilityRules,
    reasons: &mut BTreeSet<ApplicabilityReason>,
) {
    let Some(process) = snapshot.process() else {
        if step.lifecycle_requirement().is_some() || rules.activities.contains_key(step.id()) {
            reasons.insert(ApplicabilityReason::ProcessUnavailable);
        }
        return;
    };
    if process.status() != ProcessInstanceStatus::Running {
        reasons.insert(ApplicabilityReason::ProcessStatus(process.status()));
    }
    for (gate, status) in process.active_gates() {
        if *status != GateStatus::Passed {
            reasons.insert(ApplicabilityReason::Gate(gate.as_str().to_owned(), *status));
        }
    }
    for (id, blocker) in process.blockers() {
        if blocker.active() {
            reasons.insert(ApplicabilityReason::Blocker(id.as_str().to_owned()));
        }
    }
    if process.waiting_condition().is_some() {
        reasons.insert(ApplicabilityReason::Waiting);
    }
    let activity = rules.activities.get(step.id()).and_then(|id| {
        process
            .authorized_activities()
            .iter()
            .find(|a| a.id() == id)
    });
    let Some(activity) = activity else {
        reasons.insert(ApplicabilityReason::ActivityUnavailable);
        return;
    };
    for constraint in activity.constraints() {
        let key = format!("{}={}", constraint.name(), constraint.value());
        let status = rules
            .semantics
            .get(&key)
            .map_or(ConditionStatus::Unsupported, |c| {
                evaluate_skill_condition(snapshot, c)
            });
        if status != ConditionStatus::Satisfied {
            reasons.insert(ApplicabilityReason::Restriction(key, status));
        }
    }
    for capability in capabilities {
        if !activity
            .capabilities()
            .iter()
            .any(|c| c.as_str() == capability.as_str())
        {
            reasons.insert(ApplicabilityReason::CapabilityUnavailable(
                capability.clone(),
            ));
        }
    }
}
