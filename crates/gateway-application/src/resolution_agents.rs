//! CG-08.05 canonical responsibility alternatives, without runtime launch.

use crate::{
    resolution::ResolutionBasis,
    resolution_candidates::{CandidateRules, discover_candidates},
    resolution_snapshot::ResolutionSnapshot,
};
use gateway_domain::{AgentId, CapabilityRequirementId, PlanStepId, SchemaVersion, SkillId};
use gateway_process::{ActivityDefinition, ActivityId, DefinitionIdentity};
use gateway_registry::CapabilityProvider;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRoleReference {
    pub definition: DefinitionIdentity,
    pub activity: ActivityId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRules {
    pub version: SchemaVersion,
    pub primary: BTreeMap<PlanStepId, AgentId>,
    pub participants: BTreeMap<PlanStepId, BTreeSet<AgentId>>,
    pub process_roles: BTreeMap<PlanStepId, ProcessRoleReference>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentBindingError {
    UnsupportedVersion,
    UnknownStep,
    InvalidProcessReference,
    InvalidRole,
    InvalidCatalog,
    InvalidCandidateRules,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResponsibilitySource {
    DirectProvider,
    SkillOwner,
    AgentSkill(SkillId),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AgentResponsibility {
    pub provider: CapabilityProvider,
    pub agent: AgentId,
    pub source: ResponsibilitySource,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentDiagnostic {
    UnboundSkill(SkillId),
    MissingProvider(CapabilityRequirementId),
    MissingPrimary,
    IncompatiblePrimary(AgentId),
    MissingParticipant(AgentId),
    ConflictingPrimary,
    ProcessCapabilityMismatch(CapabilityRequirementId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepAgentCandidates {
    pub step: PlanStepId,
    pub primary_candidates: BTreeSet<AgentId>,
    pub required_participants: BTreeSet<AgentId>,
    pub responsibilities: BTreeMap<CapabilityRequirementId, BTreeSet<AgentResponsibility>>,
    pub process_activity: Option<ActivityDefinition>,
    pub diagnostics: BTreeSet<AgentDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentBindingCandidates {
    pub basis: ResolutionBasis,
    pub candidate_rules: CandidateRules,
    pub rules: AgentRules,
    pub steps: Vec<StepAgentCandidates>,
}

pub fn bind_agent_candidates(
    snapshot: &ResolutionSnapshot,
    candidate_rules: &CandidateRules,
    rules: &AgentRules,
) -> Result<AgentBindingCandidates, AgentBindingError> {
    if rules.version != SchemaVersion::V1 {
        return Err(AgentBindingError::UnsupportedVersion);
    }
    let plan = snapshot.request().plan();
    for id in rules
        .primary
        .keys()
        .chain(rules.participants.keys())
        .chain(rules.process_roles.keys())
    {
        if !plan.steps().iter().any(|s| s.id() == id) {
            return Err(AgentBindingError::UnknownStep);
        }
    }
    let discovery = discover_candidates(snapshot, candidate_rules)
        .map_err(|_| AgentBindingError::InvalidCandidateRules)?;
    let mut result = AgentBindingCandidates {
        basis: snapshot.request().basis().clone(),
        candidate_rules: candidate_rules.clone(),
        rules: rules.clone(),
        steps: vec![],
    };
    for step in plan.steps() {
        let activity = rules
            .process_roles
            .get(step.id())
            .map(|reference| {
                let definition = snapshot
                    .input()
                    .processes
                    .get(reference.definition.id(), reference.definition.version())
                    .ok_or(AgentBindingError::InvalidProcessReference)?;
                if definition.identity() != &reference.definition
                    || snapshot.process().is_some_and(|p| {
                        p.definition_id() != reference.definition.id()
                            || p.definition_version() != reference.definition.version()
                            || p.definition_digest() != reference.definition.digest()
                    })
                {
                    return Err(AgentBindingError::InvalidProcessReference);
                }
                definition
                    .activities()
                    .iter()
                    .find(|a| a.id() == &reference.activity)
                    .cloned()
                    .ok_or(AgentBindingError::InvalidProcessReference)
            })
            .transpose()?;
        let mut primaries = rules
            .primary
            .get(step.id())
            .cloned()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut participants = rules
            .participants
            .get(step.id())
            .cloned()
            .unwrap_or_default();
        if let Some(activity) = &activity {
            for constraint in activity.constraints() {
                match constraint.name() {
                    "primary-agent" => {
                        primaries.insert(
                            AgentId::new(constraint.value())
                                .map_err(|_| AgentBindingError::InvalidRole)?,
                        );
                    }
                    "participating-agent" => {
                        participants.insert(
                            AgentId::new(constraint.value())
                                .map_err(|_| AgentBindingError::InvalidRole)?,
                        );
                    }
                    _ => {} // Retained in process_activity for other constraint owners.
                }
            }
        }
        let mut item = StepAgentCandidates {
            step: step.id().clone(),
            primary_candidates: BTreeSet::new(),
            required_participants: participants,
            responsibilities: BTreeMap::new(),
            process_activity: activity,
            diagnostics: BTreeSet::new(),
        };
        for set in discovery.sets.iter().filter(|s| &s.step == step.id()) {
            let mut responsibilities = BTreeSet::new();
            for candidate in &set.candidates {
                if item.process_activity.as_ref().is_some_and(|a| {
                    !a.capabilities()
                        .contains(candidate.canonical.capability_id())
                }) {
                    item.diagnostics
                        .insert(AgentDiagnostic::ProcessCapabilityMismatch(
                            set.requirement.clone(),
                        ));
                    continue;
                }
                responsibilities.extend(provider_agents(snapshot, candidate.canonical.provider())?);
                if let CapabilityProvider::Skill { skill_id } = candidate.canonical.provider() {
                    if !responsibilities
                        .iter()
                        .any(|r| &r.provider == candidate.canonical.provider())
                    {
                        item.diagnostics
                            .insert(AgentDiagnostic::UnboundSkill(skill_id.clone()));
                    }
                }
            }
            if responsibilities.is_empty() {
                item.diagnostics
                    .insert(AgentDiagnostic::MissingProvider(set.requirement.clone()));
            }
            item.primary_candidates
                .extend(responsibilities.iter().map(|r| r.agent.clone()));
            item.responsibilities
                .insert(set.requirement.clone(), responsibilities);
        }
        for participant in &item.required_participants {
            if !item.primary_candidates.contains(participant) {
                item.diagnostics
                    .insert(AgentDiagnostic::MissingParticipant(participant.clone()));
            }
        }
        item.primary_candidates
            .retain(|id| !item.required_participants.contains(id));
        if primaries.len() > 1 {
            item.diagnostics.insert(AgentDiagnostic::ConflictingPrimary);
            item.primary_candidates.clear();
        } else if let Some(primary) = primaries.first() {
            if !item.primary_candidates.contains(primary) {
                item.diagnostics
                    .insert(AgentDiagnostic::IncompatiblePrimary(primary.clone()));
            }
            item.primary_candidates.retain(|id| id == primary);
        }
        if step.kind() != gateway_domain::PlanStepKind::NoOp && item.primary_candidates.is_empty() {
            item.diagnostics.insert(AgentDiagnostic::MissingPrimary);
        }
        result.steps.push(item);
    }
    Ok(result)
}

fn provider_agents(
    snapshot: &ResolutionSnapshot,
    provider: &CapabilityProvider,
) -> Result<BTreeSet<AgentResponsibility>, AgentBindingError> {
    let registry = &snapshot.input().registry;
    let mut agents = BTreeSet::new();
    match provider {
        CapabilityProvider::Agent { agent_id } => {
            agents.insert(AgentResponsibility {
                provider: provider.clone(),
                agent: agent_id.clone(),
                source: ResponsibilitySource::DirectProvider,
            });
        }
        CapabilityProvider::Skill { skill_id } => {
            let skill = registry
                .skill(skill_id)
                .ok_or(AgentBindingError::InvalidCatalog)?;
            if let Some(owner) = skill.owner_agent_id() {
                agents.insert(AgentResponsibility {
                    provider: provider.clone(),
                    agent: owner.clone(),
                    source: ResponsibilitySource::SkillOwner,
                });
            }
            for agent in registry.agents().iter() {
                for root in agent.skill_ids() {
                    let closure = registry
                        .resolve_skill(root)
                        .map_err(|_| AgentBindingError::InvalidCatalog)?;
                    if closure.get(skill_id).is_some() {
                        agents.insert(AgentResponsibility {
                            provider: provider.clone(),
                            agent: agent.id().clone(),
                            source: ResponsibilitySource::AgentSkill(root.clone()),
                        });
                    }
                }
            }
        }
    }
    Ok(agents)
}
