//! CG-08.06 effective Skill closure over the CG-03 dependency graph.

use crate::{
    resolution::ResolutionBasis,
    resolution_candidates::{CandidateRules, discover_candidates},
    resolution_snapshot::ResolutionSnapshot,
};
use gateway_domain::{
    CapabilityClass, CapabilityId, CapabilityRequirementId, ComparisonOutcome, ComparisonRules,
    ConditionId, ExecutionProfile, OperatingMode, PlanStepId, ReferenceId, RequirementCardinality,
    SchemaVersion, SkillId,
};
use gateway_process::StateId;
use gateway_registry::{CapabilityProvider, SkillDependencyGraph};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillCondition {
    Always,
    Never,
    Mode(OperatingMode),
    Profile(ExecutionProfile),
    ProcessState(StateId),
    DesiredCondition(ConditionId),
    Unsupported(ReferenceId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConditionStatus {
    Satisfied,
    Unsatisfied,
    Unknown,
    Conflicted,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRules {
    pub version: SchemaVersion,
    pub roots: BTreeMap<SkillId, RequirementCardinality>,
    pub conditions: BTreeMap<SkillId, SkillCondition>,
    /// Nested requirements are bound explicitly, never to the first indexed provider.
    pub capability_providers: BTreeMap<CapabilityId, CapabilityProvider>,
    pub max_visits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillNode {
    Skill(SkillId),
    Capability(CapabilityId),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillDiagnostic {
    MissingSkill(SkillId),
    Condition(SkillId, ConditionStatus, bool),
    MissingCapability(CapabilityId),
    UnboundCapability(CapabilityId),
    InvalidCapabilityProvider(CapabilityId),
    Cycle(Vec<SkillNode>),
    LimitExceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillClosureError {
    UnsupportedVersion,
    InvalidBudget,
    UnknownStep,
    InvalidChosenProvider,
    InvalidCatalog,
    InvalidConditionReference,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveSkills {
    pub basis: ResolutionBasis,
    pub step: PlanStepId,
    pub chosen: BTreeMap<CapabilityRequirementId, CapabilityProvider>,
    pub candidate_rules: CandidateRules,
    pub rules: SkillRules,
    pub skills: Vec<SkillId>,
    pub inclusion_paths: BTreeMap<SkillId, BTreeSet<Vec<SkillNode>>>,
    /// These remain requirements, never provided/approved capabilities.
    pub required_capabilities: BTreeMap<CapabilityId, Option<CapabilityClass>>,
    pub diagnostics: BTreeSet<SkillDiagnostic>,
    pub complete: bool,
}

pub fn evaluate_skill_condition(
    snapshot: &ResolutionSnapshot,
    condition: &SkillCondition,
) -> ConditionStatus {
    let boolean = |value| {
        if value {
            ConditionStatus::Satisfied
        } else {
            ConditionStatus::Unsatisfied
        }
    };
    match condition {
        SkillCondition::Always => ConditionStatus::Satisfied,
        SkillCondition::Never => ConditionStatus::Unsatisfied,
        SkillCondition::Mode(mode) => boolean(snapshot.input().operating_mode == *mode),
        SkillCondition::Profile(profile) => boolean(snapshot.input().execution_profile == *profile),
        SkillCondition::ProcessState(state) => {
            snapshot.process().map_or(ConditionStatus::Unknown, |p| {
                boolean(p.current_state() == state)
            })
        }
        SkillCondition::Unsupported(_) => ConditionStatus::Unsupported,
        SkillCondition::DesiredCondition(id) => gateway_domain::compare_condition(
            &snapshot.input().desired,
            id,
            snapshot.input().situation.observed_state(),
            &ComparisonRules::default(),
        )
        .map_or(ConditionStatus::Unsupported, |result| {
            condition_status(result.outcome())
        }),
    }
}

fn condition_status(outcome: ComparisonOutcome) -> ConditionStatus {
    match outcome {
        ComparisonOutcome::Satisfied => ConditionStatus::Satisfied,
        ComparisonOutcome::Unsatisfied => ConditionStatus::Unsatisfied,
        ComparisonOutcome::Conflicted => ConditionStatus::Conflicted,
        ComparisonOutcome::Incomparable => ConditionStatus::Unsupported,
        _ => ConditionStatus::Unknown,
    }
}

pub fn resolve_skill_closure(
    snapshot: &ResolutionSnapshot,
    step: &PlanStepId,
    chosen: &BTreeMap<CapabilityRequirementId, CapabilityProvider>,
    candidate_rules: &CandidateRules,
    rules: &SkillRules,
) -> Result<EffectiveSkills, SkillClosureError> {
    if rules.version != SchemaVersion::V1 {
        return Err(SkillClosureError::UnsupportedVersion);
    }
    if rules.max_visits == 0 || rules.max_visits > 100_000 {
        return Err(SkillClosureError::InvalidBudget);
    }
    if !snapshot
        .request()
        .plan()
        .steps()
        .iter()
        .any(|s| s.id() == step)
    {
        return Err(SkillClosureError::UnknownStep);
    }
    let discovery = discover_candidates(snapshot, candidate_rules)
        .map_err(|_| SkillClosureError::InvalidChosenProvider)?;
    let mut roots = rules.roots.clone();
    if rules
        .conditions
        .keys()
        .any(|id| snapshot.input().registry.skill(id).is_none())
    {
        return Err(SkillClosureError::InvalidConditionReference);
    }
    for (requirement, provider) in chosen {
        if !discovery.sets.iter().any(|s| {
            &s.step == step
                && &s.requirement == requirement
                && s.candidates
                    .iter()
                    .any(|c| c.canonical.provider() == provider)
        }) {
            return Err(SkillClosureError::InvalidChosenProvider);
        }
        if let CapabilityProvider::Skill { skill_id } = provider {
            roots.insert(skill_id.clone(), RequirementCardinality::Mandatory);
        }
    }
    let graph = snapshot
        .input()
        .registry
        .dependency_graph()
        .map_err(|_| SkillClosureError::InvalidCatalog)?;
    let result = EffectiveSkills {
        basis: snapshot.request().basis().clone(),
        step: step.clone(),
        chosen: chosen.clone(),
        candidate_rules: candidate_rules.clone(),
        rules: rules.clone(),
        skills: vec![],
        inclusion_paths: BTreeMap::new(),
        required_capabilities: BTreeMap::new(),
        diagnostics: BTreeSet::new(),
        complete: true,
    };
    let mut walker = Walker {
        snapshot,
        rules,
        graph,
        result,
        path: vec![],
        visits: 0,
    };
    for (root, cardinality) in roots {
        walker.skill(&root, cardinality == RequirementCardinality::Mandatory);
    }
    Ok(walker.result)
}

struct Walker<'a> {
    snapshot: &'a ResolutionSnapshot,
    rules: &'a SkillRules,
    graph: SkillDependencyGraph,
    result: EffectiveSkills,
    path: Vec<SkillNode>,
    visits: u32,
}

impl Walker<'_> {
    fn fail(&mut self, diagnostic: SkillDiagnostic) {
        self.result.complete = false;
        self.result.diagnostics.insert(diagnostic);
    }

    fn enter(&mut self, node: SkillNode) -> bool {
        if self.visits >= self.rules.max_visits || self.path.len() >= 64 {
            self.fail(SkillDiagnostic::LimitExceeded);
            return false;
        }
        self.visits += 1;
        if self.path.contains(&node) {
            let mut cycle = self.path.clone();
            cycle.push(node);
            self.fail(SkillDiagnostic::Cycle(cycle));
            return false;
        }
        self.path.push(node);
        true
    }

    fn skill(&mut self, id: &SkillId, mandatory: bool) {
        if !self.enter(SkillNode::Skill(id.clone())) {
            return;
        }
        let Some(skill) = self.snapshot.input().registry.skill(id) else {
            self.fail(SkillDiagnostic::MissingSkill(id.clone()));
            self.path.pop();
            return;
        };
        let status = self
            .rules
            .conditions
            .get(id)
            .map_or(ConditionStatus::Satisfied, |condition| {
                evaluate_skill_condition(self.snapshot, condition)
            });
        self.result
            .diagnostics
            .insert(SkillDiagnostic::Condition(id.clone(), status, mandatory));
        if status != ConditionStatus::Satisfied {
            if mandatory {
                self.result.complete = false;
            }
            self.path.pop();
            return;
        }
        self.result
            .inclusion_paths
            .entry(id.clone())
            .or_default()
            .insert(self.path.clone());
        // Read the CG-03 graph; do not reinterpret related_skills or narrative rules.
        let dependencies = self.graph.dependencies(id).unwrap_or_default().to_vec();
        let capabilities = skill.required_capability_ids().to_vec();
        for dependency in dependencies {
            self.skill(&dependency, true);
        }
        for capability in capabilities {
            self.capability(&capability);
        }
        if !self.result.skills.contains(id) {
            self.result.skills.push(id.clone());
        }
        self.path.pop();
    }

    fn capability(&mut self, id: &CapabilityId) {
        if !self.enter(SkillNode::Capability(id.clone())) {
            return;
        }
        let index = &self.snapshot.input().index;
        self.result
            .required_capabilities
            .insert(id.clone(), index.get(id).map(|e| e.capability().class()));
        let Some(entry) = index.get(id) else {
            self.fail(SkillDiagnostic::MissingCapability(id.clone()));
            self.path.pop();
            return;
        };
        match self.rules.capability_providers.get(id) {
            None => self.fail(SkillDiagnostic::UnboundCapability(id.clone())),
            Some(provider) if !entry.candidates().iter().any(|c| c.provider() == provider) => {
                self.fail(SkillDiagnostic::InvalidCapabilityProvider(id.clone()))
            }
            Some(CapabilityProvider::Skill { skill_id }) => self.skill(skill_id, true),
            Some(CapabilityProvider::Agent { .. }) => {}
        }
        self.path.pop();
    }
}

#[cfg(test)]
#[path = "../tests/support/skill_conditions.rs"]
mod tests;
