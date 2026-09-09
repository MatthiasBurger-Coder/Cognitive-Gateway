//! Bounded whole-binding search. Ordering is presentation, never a tie-break.
use crate::{
    resolution::{ResolutionBasis, ResolutionOutcome, StepBinding},
    resolution_agents::{AgentRules, ProcessRoleReference, bind_agent_candidates},
    resolution_applicability::{
        ApplicabilityReason, ApplicabilityRules, StepApplicability, evaluate_applicability,
    },
    resolution_candidates::{CandidateDiscovery, CandidateRules, discover_candidates},
    resolution_process::{
        ProcessCandidate, ProcessSelection, ProcessSelectionOutcome, ProcessSelectionRules,
        select_process,
    },
    resolution_skills::{
        ConditionStatus, EffectiveSkills, SkillCondition, SkillDiagnostic, SkillRules,
        resolve_skill_closure,
    },
    resolution_snapshot::ResolutionSnapshot,
};
use gateway_domain::{
    AgentId, CapabilityClass, CapabilityRequirementId, PlanStep, PlanStepId, PlanStepKind,
    RequirementCardinality, SchemaVersion, SkillId,
};
use gateway_process::{ActivityDefinition, ActivityId};
use gateway_registry::CapabilityProvider;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionRules {
    pub version: SchemaVersion,
    pub candidates: CandidateRules,
    pub processes: ProcessSelectionRules,
    pub agents: AgentRules,
    pub skills: BTreeMap<PlanStepId, SkillRules>,
    pub applicability: ApplicabilityRules,
    /// Integer scores apply only after complete hard-constraint validation.
    pub provider_priorities: BTreeMap<CapabilityProvider, i32>,
    /// Secondary explicit preference, never implicit omission of mandatory work.
    pub prefer_optional: bool,
    pub max_visits: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositionError {
    UnsupportedVersion,
    InvalidBudget,
    InvalidRules,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompositionDiagnostic {
    MissingRequirement(CapabilityRequirementId),
    RoleConflict,
    Skill(SkillDiagnostic),
    Applicability(ApplicabilityReason),
    OptionalOmitted(CapabilityRequirementId),
    SearchLimit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingAlternative {
    pub step: PlanStepId,
    pub binding: Option<StepBinding>,
    pub activity: Option<ActivityId>,
    pub chosen: BTreeMap<CapabilityRequirementId, CapabilityProvider>,
    pub skills: Option<EffectiveSkills>,
    pub applicability: StepApplicability,
    pub score: (i64, u32),
    pub diagnostics: BTreeSet<CompositionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepCompositions {
    pub step: PlanStepId,
    pub outcome: ResolutionOutcome,
    /// Unranked valid alternatives remain inspectable even on partial failure.
    pub alternatives: Vec<BindingAlternative>,
    pub rejections: Vec<BindingRejection>,
    pub diagnostics: BTreeSet<CompositionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingRejection {
    pub chosen: BTreeMap<CapabilityRequirementId, CapabilityProvider>,
    pub process: Option<crate::resolution::ProcessBinding>,
    pub activity: Option<ActivityId>,
    pub skills: Option<EffectiveSkills>,
    pub reasons: BTreeSet<CompositionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionReport {
    pub basis: ResolutionBasis,
    pub rules: CompositionRules,
    pub discovery: CandidateDiscovery,
    pub processes: ProcessSelection,
    pub steps: Vec<StepCompositions>,
    /// All highest-ranked complete plan alternatives. A tie remains ambiguous.
    pub alternatives: Vec<Vec<BindingAlternative>>,
    pub outcome: ResolutionOutcome,
    pub visits: u32,
    pub exhausted: bool,
}

struct Budget {
    used: u32,
    max: u32,
    exhausted: bool,
}
impl Budget {
    fn tick(&mut self) -> bool {
        if self.used == self.max {
            self.exhausted = true;
            false
        } else {
            self.used += 1;
            true
        }
    }
}

/// Every expansion consumes a deterministic unit, including rejected branches.
fn product<T: Clone>(lists: &[Vec<T>], budget: &mut Budget) -> Vec<Vec<T>> {
    let mut result = vec![vec![]];
    for list in lists {
        let mut next = vec![];
        for prefix in &result {
            for item in list {
                if !budget.tick() {
                    return vec![];
                }
                let mut row = prefix.clone();
                row.push(item.clone());
                next.push(row);
            }
        }
        result = next;
    }
    result
}

pub fn compose_resolution(
    snapshot: &ResolutionSnapshot,
    rules: &CompositionRules,
) -> Result<CompositionReport, CompositionError> {
    if rules.version != SchemaVersion::V1 {
        return Err(CompositionError::UnsupportedVersion);
    }
    if rules.max_visits == 0 || rules.max_visits > 100_000 {
        return Err(CompositionError::InvalidBudget);
    }
    if rules.applicability.version != SchemaVersion::V1
        || rules
            .skills
            .values()
            .any(|r| r.version != SchemaVersion::V1 || r.max_visits == 0 || r.max_visits > 100_000)
    {
        return Err(CompositionError::InvalidRules);
    }
    if rules
        .skills
        .keys()
        .chain(rules.applicability.restrictions.keys())
        .chain(rules.applicability.activities.keys())
        .chain(rules.applicability.completed.keys())
        .any(|id| {
            !snapshot
                .request()
                .plan()
                .steps()
                .iter()
                .any(|s| s.id() == id)
        })
    {
        return Err(CompositionError::InvalidRules);
    }
    let discovery = discover_candidates(snapshot, &rules.candidates)
        .map_err(|_| CompositionError::InvalidRules)?;
    let processes =
        select_process(snapshot, &rules.processes).map_err(|_| CompositionError::InvalidRules)?;
    // Validate explicit role rules even if no composition is possible.
    bind_agent_candidates(snapshot, &rules.candidates, &rules.agents)
        .map_err(|_| CompositionError::InvalidRules)?;
    let mut budget = Budget {
        used: 0,
        max: rules.max_visits,
        exhausted: false,
    };
    let mut steps: Vec<_> = snapshot
        .request()
        .plan()
        .steps()
        .iter()
        .map(|s| StepCompositions {
            step: s.id().clone(),
            outcome: ResolutionOutcome::Missing,
            alternatives: vec![],
            rejections: vec![],
            diagnostics: BTreeSet::new(),
        })
        .collect();
    let contexts: Vec<_> = if processes.outcome == ProcessSelectionOutcome::NoTemplate {
        vec![None]
    } else {
        processes.candidates.iter().map(Some).collect()
    };
    let mut alternatives = vec![];
    for process in contexts {
        let mut per_step = vec![];
        for (step, output) in snapshot.request().plan().steps().iter().zip(&mut steps) {
            let activities: Vec<_> = if step.kind() == PlanStepKind::NoOp {
                vec![None]
            } else {
                process.map_or_else(
                    || vec![None],
                    |p| {
                        p.activities
                            .get(step.id())
                            .into_iter()
                            .flatten()
                            .map(Some)
                            .collect()
                    },
                )
            };
            let mut choices = vec![];
            for activity in activities {
                if !budget.tick() {
                    break;
                }
                let context = SearchContext {
                    snapshot,
                    rules,
                    discovery: &discovery,
                    process,
                    activity,
                    step,
                };
                choices.extend(context.search(output, &mut budget)?);
            }
            for choice in &choices {
                if !output.alternatives.contains(choice) {
                    output.alternatives.push(choice.clone());
                }
            }
            per_step.push(choices);
        }
        for row in product(&per_step, &mut budget) {
            if !alternatives.contains(&row) {
                alternatives.push(row);
            }
        }
        if budget.exhausted {
            break;
        }
    }
    if let Some(best) = alternatives
        .iter()
        .map(|row| {
            row.iter()
                .fold((0_i64, 0_u32), |a, b| (a.0 + b.score.0, a.1 + b.score.1))
        })
        .max()
    {
        alternatives.retain(|row| {
            row.iter()
                .fold((0_i64, 0_u32), |a, b| (a.0 + b.score.0, a.1 + b.score.1))
                == best
        });
    }
    let outcome = if snapshot.request().plan().is_noop() {
        ResolutionOutcome::NoOp
    } else if budget.exhausted {
        ResolutionOutcome::SearchLimit
    } else if alternatives.len() > 1 {
        ResolutionOutcome::Ambiguous
    } else if alternatives.len() == 1 {
        ResolutionOutcome::Resolved
    } else if steps.iter().any(|s| !s.alternatives.is_empty()) {
        ResolutionOutcome::Partial
    } else if processes.outcome == ProcessSelectionOutcome::Unsupported {
        ResolutionOutcome::Unsupported
    } else if processes.outcome == ProcessSelectionOutcome::Missing
        || discovery.sets.iter().any(|s| s.candidates.is_empty())
    {
        ResolutionOutcome::Missing
    } else {
        ResolutionOutcome::Conflicting
    };
    if budget.exhausted {
        for step in &mut steps {
            step.diagnostics.insert(CompositionDiagnostic::SearchLimit);
        }
    }
    for (index, step) in steps.iter_mut().enumerate() {
        let relevant: Vec<_> = if alternatives.is_empty() {
            let best = step.alternatives.iter().map(|a| a.score).max();
            step.alternatives
                .iter()
                .filter(|a| Some(a.score) == best)
                .collect()
        } else {
            alternatives.iter().map(|row| &row[index]).collect()
        };
        step.outcome = if budget.exhausted {
            ResolutionOutcome::SearchLimit
        } else if relevant.is_empty() {
            ResolutionOutcome::Missing
        } else if relevant.iter().all(|a| *a == relevant[0]) {
            if relevant[0].binding.is_none() {
                ResolutionOutcome::NoOp
            } else {
                ResolutionOutcome::Resolved
            }
        } else {
            ResolutionOutcome::Ambiguous
        };
    }
    Ok(CompositionReport {
        basis: snapshot.request().basis().clone(),
        rules: rules.clone(),
        discovery,
        processes,
        steps,
        alternatives,
        outcome,
        visits: budget.used,
        exhausted: budget.exhausted,
    })
}

struct SearchContext<'a> {
    snapshot: &'a ResolutionSnapshot,
    rules: &'a CompositionRules,
    discovery: &'a CandidateDiscovery,
    process: Option<&'a ProcessCandidate>,
    activity: Option<&'a ActivityDefinition>,
    step: &'a PlanStep,
}

impl SearchContext<'_> {
    fn search(
        &self,
        output: &mut StepCompositions,
        budget: &mut Budget,
    ) -> Result<Vec<BindingAlternative>, CompositionError> {
        if self.step.kind() == PlanStepKind::NoOp {
            let applicability = evaluate_applicability(
                self.snapshot,
                self.step.id(),
                &BTreeMap::new(),
                &BTreeSet::new(),
                &self.rules.candidates,
                &self.rules.applicability,
            )
            .map_err(|_| CompositionError::InvalidRules)?;
            return Ok(vec![BindingAlternative {
                step: self.step.id().clone(),
                binding: None,
                activity: None,
                chosen: BTreeMap::new(),
                skills: None,
                applicability,
                score: (0, 0),
                diagnostics: BTreeSet::new(),
            }]);
        }
        let mut agent_rules = self.rules.agents.clone();
        let mut applicability = self.rules.applicability.clone();
        let mut skill_rules =
            self.rules
                .skills
                .get(self.step.id())
                .cloned()
                .unwrap_or(SkillRules {
                    version: SchemaVersion::V1,
                    roots: BTreeMap::new(),
                    conditions: BTreeMap::new(),
                    capability_providers: BTreeMap::new(),
                    max_visits: self.rules.max_visits,
                });
        if let (Some(process), Some(activity)) = (self.process, self.activity) {
            let reference = ProcessRoleReference {
                definition: process.binding.definition.clone(),
                activity: activity.id().clone(),
            };
            if agent_rules
                .process_roles
                .get(self.step.id())
                .is_some_and(|r| r != &reference)
            {
                return Ok(vec![]);
            }
            agent_rules
                .process_roles
                .insert(self.step.id().clone(), reference);
            if self.snapshot.process().is_some() {
                if applicability
                    .activities
                    .get(self.step.id())
                    .is_some_and(|id| id != activity.id())
                {
                    return Ok(vec![]);
                }
                applicability
                    .activities
                    .insert(self.step.id().clone(), activity.id().clone());
            }
            for c in activity.constraints() {
                let key = format!("{}={}", c.name(), c.value());
                if c.name() == "required-skill" {
                    let id = SkillId::new(c.value()).map_err(|_| CompositionError::InvalidRules)?;
                    skill_rules
                        .roots
                        .insert(id, RequirementCardinality::Mandatory);
                }
                // Role constraints are checked by the canonical role binder; required
                // Skill inclusion is checked by the closure. Other semantics stay explicit.
                let proven = matches!(
                    c.name(),
                    "primary-agent" | "participating-agent" | "required-skill"
                ) || self
                    .rules
                    .processes
                    .lifecycle_contracts
                    .values()
                    .any(|v| v == c);
                let condition = applicability
                    .semantics
                    .get(&key)
                    .cloned()
                    .unwrap_or(if proven {
                        SkillCondition::Always
                    } else {
                        SkillCondition::Unsupported(
                            gateway_domain::ReferenceId::new("process-constraint")
                                .expect("constant identity"),
                        )
                    });
                applicability
                    .semantics
                    .entry(key.clone())
                    .or_insert(condition.clone());
                applicability
                    .restrictions
                    .entry(self.step.id().clone())
                    .or_default()
                    .entry(key)
                    .or_default()
                    .push(condition);
            }
        } else if agent_rules.process_roles.contains_key(self.step.id()) {
            return Ok(vec![]);
        }
        let agents = bind_agent_candidates(self.snapshot, &self.rules.candidates, &agent_rules)
            .map_err(|_| CompositionError::InvalidRules)?;
        let agent = agents
            .steps
            .iter()
            .find(|s| &s.step == self.step.id())
            .ok_or(CompositionError::InvalidRules)?;
        let lists: Vec<_> = self
            .step
            .capability_requirements()
            .iter()
            .map(|id| {
                let set = self
                    .discovery
                    .sets
                    .iter()
                    .find(|s| s.step == *self.step.id() && &s.requirement == id)
                    .expect("validated discovery");
                let grouped = self
                    .snapshot
                    .request()
                    .alternatives()
                    .iter()
                    .any(|g| g.step == *self.step.id() && g.members.contains(id));
                let mut values: Vec<_> = set
                    .candidates
                    .iter()
                    .map(|c| Some((id.clone(), c.canonical.provider().clone())))
                    .collect();
                if grouped || set.cardinality == RequirementCardinality::Optional {
                    values.push(None);
                }
                if values.is_empty() {
                    output
                        .diagnostics
                        .insert(CompositionDiagnostic::MissingRequirement(id.clone()));
                }
                values
            })
            .collect();
        let mut result = vec![];
        for row in product(&lists, budget) {
            let chosen: BTreeMap<_, _> = row.into_iter().flatten().collect();
            if !self
                .snapshot
                .request()
                .alternatives()
                .iter()
                .filter(|g| &g.step == self.step.id())
                .all(|g| {
                    let n = g
                        .members
                        .iter()
                        .filter(|id| chosen.contains_key(*id))
                        .count();
                    n == 1 || (n == 0 && g.cardinality == RequirementCardinality::Optional)
                })
            {
                continue;
            }
            let closures = self.closures(&chosen, &skill_rules, output, budget)?;
            let had_closures = !closures.is_empty();
            let before = (result.len(), output.rejections.len());
            for closure in closures {
                let mut role_lists: Vec<Vec<AgentId>> = chosen
                    .iter()
                    .map(|(id, provider)| {
                        agent
                            .responsibilities
                            .get(id)
                            .into_iter()
                            .flatten()
                            .filter(|r| &r.provider == provider)
                            .map(|r| r.agent.clone())
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect()
                    })
                    .collect();
                // Explicit roots and nested capabilities also require canonical responsibility.
                for skill in &closure.skills {
                    role_lists.push(self.skill_agents(skill));
                }
                for provider in closure
                    .required_capabilities
                    .keys()
                    .filter_map(|id| closure.rules.capability_providers.get(id))
                {
                    if let CapabilityProvider::Agent { agent_id } = provider {
                        role_lists.push(vec![agent_id.clone()]);
                    }
                }
                for roles in product(&role_lists, budget) {
                    let used: BTreeSet<_> = roles.iter().cloned().collect();
                    if !agent.required_participants.is_subset(&used) {
                        continue;
                    }
                    for primary in agent.primary_candidates.intersection(&used) {
                        if !budget.tick() {
                            break;
                        }
                        let skill_roles = closure
                            .skills
                            .iter()
                            .cloned()
                            .zip(roles.iter().skip(chosen.len()).cloned())
                            .collect();
                        let binding = StepBinding {
                            process: self.process.map(|p| p.binding.clone()),
                            primary_agent: primary.clone(),
                            participating_agents: used
                                .iter()
                                .filter(|a| *a != primary)
                                .cloned()
                                .collect(),
                            skills: skill_roles,
                        };
                        let mut effective_applicability = applicability.clone();
                        for id in closure.required_capabilities.keys() {
                            let contract = self
                                .snapshot
                                .input()
                                .index
                                .get(id)
                                .expect("complete closure")
                                .capability();
                            for text in contract
                                .preconditions()
                                .iter()
                                .map(|c| c.as_str())
                                .chain(contract.constraints().iter().map(|c| c.as_str()))
                            {
                                let condition = if text == "read-only" {
                                    if contract.class() == CapabilityClass::Inspect {
                                        SkillCondition::Always
                                    } else {
                                        SkillCondition::Never
                                    }
                                } else {
                                    applicability.semantics.get(text).cloned().unwrap_or(
                                        SkillCondition::Unsupported(
                                            gateway_domain::ReferenceId::new("nested-contract")
                                                .expect("constant identity"),
                                        ),
                                    )
                                };
                                effective_applicability
                                    .restrictions
                                    .entry(self.step.id().clone())
                                    .or_default()
                                    .entry(text.to_owned())
                                    .or_default()
                                    .push(condition);
                            }
                        }
                        let readiness = evaluate_applicability(
                            self.snapshot,
                            self.step.id(),
                            &chosen,
                            &closure.required_capabilities.keys().cloned().collect(),
                            &self.rules.candidates,
                            &effective_applicability,
                        )
                        .map_err(|_| CompositionError::InvalidRules)?;
                        let invalid: Vec<_> = readiness
                            .reasons
                            .iter()
                            .filter(|r| {
                                matches!(
                                    r,
                                    ApplicabilityReason::Restriction(_, _)
                                        | ApplicabilityReason::CapabilityUnavailable(_)
                                        | ApplicabilityReason::Prerequisite(_, _)
                                )
                            })
                            .cloned()
                            .collect();
                        if !invalid.is_empty() {
                            self.reject(
                                output,
                                &chosen,
                                Some(closure.clone()),
                                invalid
                                    .into_iter()
                                    .map(CompositionDiagnostic::Applicability)
                                    .collect(),
                            );
                            continue;
                        }
                        let diagnostics = self
                            .step
                            .capability_requirements()
                            .iter()
                            .filter(|id| !chosen.contains_key(*id))
                            .cloned()
                            .map(CompositionDiagnostic::OptionalOmitted)
                            .collect();
                        let score = (
                            chosen
                                .values()
                                .chain(
                                    closure.required_capabilities.keys().filter_map(|id| {
                                        closure.rules.capability_providers.get(id)
                                    }),
                                )
                                .map(|p| {
                                    i64::from(*self.rules.provider_priorities.get(p).unwrap_or(&0))
                                })
                                .sum(),
                            if self.rules.prefer_optional {
                                chosen.len() as u32
                            } else {
                                0
                            },
                        );
                        let item = BindingAlternative {
                            step: self.step.id().clone(),
                            binding: Some(binding),
                            activity: self.activity.map(|a| a.id().clone()),
                            chosen: chosen.clone(),
                            skills: Some(closure.clone()),
                            applicability: readiness,
                            score,
                            diagnostics,
                        };
                        if !result.contains(&item) {
                            result.push(item);
                        }
                    }
                }
            }
            if had_closures
                && before == (result.len(), output.rejections.len())
                && !budget.exhausted
            {
                self.reject(
                    output,
                    &chosen,
                    None,
                    BTreeSet::from([CompositionDiagnostic::RoleConflict]),
                );
            }
        }
        if result.is_empty() && output.diagnostics.is_empty() {
            output
                .diagnostics
                .insert(CompositionDiagnostic::RoleConflict);
        }
        Ok(result)
    }

    fn skill_agents(&self, skill: &SkillId) -> Vec<AgentId> {
        self.snapshot
            .input()
            .registry
            .agents()
            .iter()
            .filter(|a| {
                self.snapshot
                    .input()
                    .registry
                    .skill(skill)
                    .is_some_and(|s| s.owner_agent_id() == Some(a.id()))
                    || a.skill_ids().iter().any(|root| {
                        self.snapshot
                            .input()
                            .registry
                            .resolve_skill(root)
                            .is_ok_and(|g| g.get(skill).is_some())
                    })
            })
            .map(|a| a.id().clone())
            .collect()
    }

    fn reject(
        &self,
        output: &mut StepCompositions,
        chosen: &BTreeMap<CapabilityRequirementId, CapabilityProvider>,
        skills: Option<EffectiveSkills>,
        reasons: BTreeSet<CompositionDiagnostic>,
    ) {
        output.diagnostics.extend(reasons.iter().cloned());
        let item = BindingRejection {
            chosen: chosen.clone(),
            process: self.process.map(|p| p.binding.clone()),
            activity: self.activity.map(|a| a.id().clone()),
            skills,
            reasons,
        };
        if !output.rejections.contains(&item) {
            output.rejections.push(item);
        }
    }

    fn closures(
        &self,
        chosen: &BTreeMap<CapabilityRequirementId, CapabilityProvider>,
        rules: &SkillRules,
        output: &mut StepCompositions,
        budget: &mut Budget,
    ) -> Result<Vec<EffectiveSkills>, CompositionError> {
        let mut pending = vec![rules.clone()];
        let mut result = vec![];
        while let Some(rules) = pending.pop() {
            if !budget.tick() {
                break;
            }
            let closure = resolve_skill_closure(
                self.snapshot,
                self.step.id(),
                chosen,
                &self.rules.candidates,
                &rules,
            )
            .map_err(|_| CompositionError::InvalidRules)?;
            if closure
                .diagnostics
                .contains(&SkillDiagnostic::LimitExceeded)
            {
                budget.exhausted = true;
                break;
            }
            if closure.complete {
                result.push(closure);
                continue;
            }
            if let Some(id) = closure.diagnostics.iter().find_map(|d| {
                if let SkillDiagnostic::UnboundCapability(id) = d {
                    Some(id)
                } else {
                    None
                }
            }) {
                if let Some(entry) = self.snapshot.input().index.get(id) {
                    for candidate in entry.candidates().iter().rev() {
                        if !budget.tick() {
                            break;
                        }
                        let mut next = rules.clone();
                        next.capability_providers
                            .insert(id.clone(), candidate.provider().clone());
                        pending.push(next);
                    }
                }
            } else {
                let reasons = closure
                    .diagnostics
                    .iter()
                    .filter(|d| {
                        !matches!(
                            d,
                            SkillDiagnostic::Condition(_, ConditionStatus::Satisfied, _)
                        )
                    })
                    .cloned()
                    .map(CompositionDiagnostic::Skill)
                    .collect();
                self.reject(output, chosen, Some(closure), reasons);
            }
        }
        Ok(result)
    }
}
