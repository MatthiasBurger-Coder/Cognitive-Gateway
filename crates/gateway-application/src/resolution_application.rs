//! Provider-neutral resolver API and inspection-only CG-09/CG-10 boundaries.
use crate::{
    resolution::{LifecycleReadiness, ResolutionBasis, ResolutionOutcome},
    resolution_artifact::{self, ArtifactError, ArtifactLimits},
    resolution_composition::{
        BindingAlternative, CompositionError, CompositionReport, CompositionRules,
        compose_resolution,
    },
    resolution_encoding::{fingerprint, rules_json},
    resolution_explain::{self, ResolutionTrace, TraceError, TraceLimits},
    resolution_skills::SkillCondition,
    resolution_snapshot::{ResolutionSnapshot, ResolutionSnapshotPort, SnapshotError},
};
use gateway_domain::{
    CapabilityClass, CapabilityDefinition, CapabilityId, DefinitionCatalog, ExecutionContextIR,
    Plan, PlanStepId, PolicyId, ReferenceId, WorkflowId,
};
use gateway_policy::PolicyDecision;
use gateway_process::{ActivityConstraint, DefinitionIdentity};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionApplicationError {
    Snapshot(SnapshotError),
    Composition(CompositionError),
    Artifact(ArtifactError),
    Trace(TraceError),
    UnknownStep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPlan {
    pub snapshot: ResolutionSnapshot,
    pub report: CompositionReport,
}

/// Required contracts for exactly one alternative, never a union of exclusive choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyBindingInput {
    pub alternative: BindingAlternative,
    pub required_capabilities: BTreeMap<CapabilityId, CapabilityDefinition>,
    pub process_constraints: Vec<ActivityConstraint>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionInspection {
    pub basis: ResolutionBasis,
    pub rule_fingerprint: String,
    pub plan: Plan,
    /// Includes every source restriction, candidate distinction and unresolved result.
    pub report: CompositionReport,
    pub policy_inputs: Vec<PolicyBindingInput>,
}

/// Supplied by the CG-02/CG-10 mapping owner, never invented by this resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowProjectionMapping {
    pub basis: ResolutionBasis,
    pub step: PlanStepId,
    pub task: gateway_domain::TaskId,
    pub process: DefinitionIdentity,
    pub workflow: WorkflowId,
    pub decision_reference: ReferenceId,
}

/// An externally authenticated CG-09 result. This type does not evaluate policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalPolicyResult {
    pub basis: ResolutionBasis,
    pub step: PlanStepId,
    pub policy: PolicyId,
    pub decision: PolicyDecision,
    pub approved_capabilities: BTreeSet<CapabilityId>,
    pub decision_reference: ReferenceId,
}
pub struct ProjectionInputs<'a> {
    pub step: &'a PlanStepId,
    pub mapping: Option<&'a WorkflowProjectionMapping>,
    pub catalog: &'a DefinitionCatalog,
    /// A proposed CG-10 output supplied for contract checking, not constructed here.
    pub context: Option<&'a ExecutionContextIR>,
    pub policy: Option<&'a ExternalPolicyResult>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectionProblem {
    Unresolved,
    NoTemplate,
    EmptySkills,
    MultipleAgents,
    NotCurrentlyEligible,
    MissingOwnerMapping,
    StaleMapping,
    WorkflowMismatch,
    MissingExternalContext,
    ContextMismatch,
    CatalogMismatch,
    UnmappedConstraints,
    MissingPolicy,
    PolicyNotApproved,
    StalePolicy,
    InsufficientApprovals,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionStatus {
    CompatibleV1Shape,
    Incompatible,
    NoWork,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionCompatibility {
    pub status: ProjectionStatus,
    pub basis: ResolutionBasis,
    pub problems: BTreeSet<ProjectionProblem>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DeclarativeResolutionApplication;
impl DeclarativeResolutionApplication {
    pub fn resolve_plan(
        &self,
        port: &impl ResolutionSnapshotPort,
        rules: &CompositionRules,
    ) -> Result<ResolvedPlan, ResolutionApplicationError> {
        let snapshot =
            ResolutionSnapshot::capture(port).map_err(ResolutionApplicationError::Snapshot)?;
        let report = compose_resolution(&snapshot, rules)
            .map_err(ResolutionApplicationError::Composition)?;
        Ok(ResolvedPlan { snapshot, report })
    }
    pub fn validate_resolution(
        &self,
        resolution: &ResolvedPlan,
    ) -> Result<(), ResolutionApplicationError> {
        resolution_artifact::validate_resolution(
            &resolution.snapshot,
            &resolution.report,
            ArtifactLimits::default(),
        )
        .map_err(ResolutionApplicationError::Artifact)
    }
    pub fn inspect_resolution(
        &self,
        resolution: &ResolvedPlan,
    ) -> Result<ResolutionInspection, ResolutionApplicationError> {
        self.validate_resolution(resolution)?;
        let snapshot = &resolution.snapshot;
        let mut policy_inputs = vec![];
        for alternative in resolution.report.steps.iter().flat_map(|s| &s.alternatives) {
            let mut ids: BTreeSet<_> = alternative
                .chosen
                .keys()
                .map(|id| {
                    snapshot
                        .request()
                        .plan()
                        .capability_requirements()
                        .iter()
                        .find(|r| r.id() == id)
                        .expect("validated requirement")
                        .capability()
                        .clone()
                })
                .collect();
            if let Some(closure) = &alternative.skills {
                ids.extend(closure.required_capabilities.keys().cloned());
            }
            let required_capabilities = ids
                .into_iter()
                .map(|id| {
                    let contract = snapshot
                        .input()
                        .index
                        .get(&id)
                        .expect("validated required contract")
                        .capability()
                        .clone();
                    (id, contract)
                })
                .collect();
            let process_constraints = alternative
                .binding
                .as_ref()
                .and_then(|b| b.process.as_ref())
                .and_then(|p| {
                    snapshot
                        .input()
                        .processes
                        .get(p.definition.id(), p.definition.version())
                })
                .and_then(|p| {
                    p.activities()
                        .iter()
                        .find(|a| Some(a.id()) == alternative.activity.as_ref())
                })
                .map_or_else(Vec::new, |a| a.constraints().to_vec());
            policy_inputs.push(PolicyBindingInput {
                alternative: alternative.clone(),
                required_capabilities,
                process_constraints,
            });
        }
        Ok(ResolutionInspection {
            basis: resolution.report.basis.clone(),
            rule_fingerprint: fingerprint(&rules_json(&resolution.report.rules)),
            plan: snapshot.request().plan().clone(),
            report: resolution.report.clone(),
            policy_inputs,
        })
    }
    pub fn explain_resolution(
        &self,
        resolution: &ResolvedPlan,
        limits: TraceLimits,
    ) -> Result<ResolutionTrace, ResolutionApplicationError> {
        resolution_explain::explain_resolution(&resolution.snapshot, &resolution.report, limits)
            .map_err(ResolutionApplicationError::Trace)
    }
    pub fn serialize_resolution(
        &self,
        resolution: &ResolvedPlan,
        limits: ArtifactLimits,
    ) -> Result<String, ResolutionApplicationError> {
        resolution_artifact::serialize_resolution(&resolution.snapshot, &resolution.report, limits)
            .map_err(ResolutionApplicationError::Artifact)
    }
    pub fn parse_resolution(
        &self,
        port: &impl ResolutionSnapshotPort,
        rules: &CompositionRules,
        text: &str,
        limits: ArtifactLimits,
    ) -> Result<ResolvedPlan, ResolutionApplicationError> {
        let snapshot =
            ResolutionSnapshot::capture(port).map_err(ResolutionApplicationError::Snapshot)?;
        let report = resolution_artifact::parse_resolution(&snapshot, rules, text, limits)
            .map_err(ResolutionApplicationError::Artifact)?;
        Ok(ResolvedPlan { snapshot, report })
    }
    pub fn basis_is_current(
        &self,
        resolution: &ResolvedPlan,
        port: &impl ResolutionSnapshotPort,
    ) -> Result<bool, ResolutionApplicationError> {
        self.validate_resolution(resolution)?;
        let current =
            ResolutionSnapshot::capture(port).map_err(ResolutionApplicationError::Snapshot)?;
        Ok(resolution.snapshot.same_basis(&current))
    }
    /// Checks an external compiler proposal. Does not compile or authorize anything.
    pub fn inspect_v1_projection(
        &self,
        resolution: &ResolvedPlan,
        input: ProjectionInputs<'_>,
    ) -> Result<ProjectionCompatibility, ResolutionApplicationError> {
        let inspection = self.inspect_resolution(resolution)?;
        let mut problems = BTreeSet::new();
        let basis = inspection.basis.clone();
        if !inspection.plan.steps().iter().any(|s| s.id() == input.step) {
            return Err(ResolutionApplicationError::UnknownStep);
        }
        if inspection.plan.is_noop() {
            return Ok(ProjectionCompatibility {
                status: ProjectionStatus::NoWork,
                basis,
                problems,
            });
        }
        if inspection.report.outcome != ResolutionOutcome::Resolved
            || inspection.report.alternatives.len() != 1
        {
            problems.insert(ProjectionProblem::Unresolved);
            return Ok(ProjectionCompatibility {
                status: ProjectionStatus::Incompatible,
                basis,
                problems,
            });
        }
        let alternative = inspection.report.alternatives[0]
            .iter()
            .find(|a| &a.step == input.step)
            .expect("complete plan");
        let Some(binding) = &alternative.binding else {
            return Ok(ProjectionCompatibility {
                status: ProjectionStatus::NoWork,
                basis,
                problems,
            });
        };
        if alternative.applicability.readiness != LifecycleReadiness::Eligible {
            problems.insert(ProjectionProblem::NotCurrentlyEligible);
        }
        if binding.process.is_none() {
            problems.insert(ProjectionProblem::NoTemplate);
        }
        if binding.skills.is_empty() {
            problems.insert(ProjectionProblem::EmptySkills);
        }
        if !binding.participating_agents.is_empty()
            || binding.skills.values().any(|a| a != &binding.primary_agent)
        {
            problems.insert(ProjectionProblem::MultipleAgents);
        }
        let contracts = &inspection
            .policy_inputs
            .iter()
            .find(|p| p.alternative == *alternative)
            .expect("inspected binding")
            .required_capabilities;
        if contracts.values().flat_map(|c| c.constraints()).any(|c| {
            c.as_str() != "read-only"
                && !matches!(
                    inspection
                        .report
                        .rules
                        .applicability
                        .semantics
                        .get(c.as_str()),
                    Some(SkillCondition::Mode(_) | SkillCondition::Profile(_))
                )
        }) {
            problems.insert(ProjectionProblem::UnmappedConstraints);
        }
        match input.mapping {
            None => {
                problems.insert(ProjectionProblem::MissingOwnerMapping);
            }
            Some(mapping) => {
                if mapping.basis != basis
                    || &mapping.step != input.step
                    || binding
                        .process
                        .as_ref()
                        .is_none_or(|p| p.definition != mapping.process)
                {
                    problems.insert(ProjectionProblem::StaleMapping);
                }
                if input.catalog.workflow(&mapping.workflow).is_none() {
                    problems.insert(ProjectionProblem::WorkflowMismatch);
                }
            }
        }
        match input.policy {
            None => {
                problems.insert(ProjectionProblem::MissingPolicy);
            }
            Some(policy) => {
                if policy.basis != basis || &policy.step != input.step {
                    problems.insert(ProjectionProblem::StalePolicy);
                }
                if policy.decision != PolicyDecision::Allow {
                    problems.insert(ProjectionProblem::PolicyNotApproved);
                }
                if contracts
                    .keys()
                    .any(|id| !policy.approved_capabilities.contains(id))
                {
                    problems.insert(ProjectionProblem::InsufficientApprovals);
                }
                // Read-only is represented by approving only required INSPECT contracts;
                // arbitrary extra approvals cannot defeat the intrinsic restriction.
                if contracts
                    .values()
                    .any(|c| c.constraints().iter().any(|x| x.as_str() == "read-only"))
                    && policy.approved_capabilities.iter().any(|id| {
                        contracts
                            .get(id)
                            .is_none_or(|c| c.class() != CapabilityClass::Inspect)
                    })
                {
                    problems.insert(ProjectionProblem::UnmappedConstraints);
                }
            }
        }
        match input.context {
            None => {
                problems.insert(ProjectionProblem::MissingExternalContext);
            }
            Some(context) => {
                let skills = alternative
                    .skills
                    .as_ref()
                    .map(|s| s.skills.as_slice())
                    .unwrap_or_default();
                if context.primary_agent_id() != &binding.primary_agent
                    || input
                        .mapping
                        .is_some_and(|m| context.task().id() != &m.task)
                    || context.skill_ids() != skills
                    || context.operating_mode() != resolution.snapshot.input().operating_mode
                    || context.execution_profile() != resolution.snapshot.input().execution_profile
                {
                    problems.insert(ProjectionProblem::ContextMismatch);
                }
                if input
                    .mapping
                    .is_none_or(|m| context.workflow_id() != &m.workflow)
                    || input
                        .catalog
                        .workflow(context.workflow_id())
                        .is_none_or(|w| w.policy_id() != context.policy_id())
                {
                    problems.insert(ProjectionProblem::WorkflowMismatch);
                }
                if input.policy.is_none_or(|p| {
                    context.policy_id() != &p.policy
                        || context
                            .approved_capability_ids()
                            .iter()
                            .cloned()
                            .collect::<BTreeSet<_>>()
                            != p.approved_capabilities
                }) {
                    problems.insert(ProjectionProblem::PolicyNotApproved);
                }
                if context.validate_against(input.catalog).is_err()
                    || input.catalog.agent(&binding.primary_agent)
                        != resolution
                            .snapshot
                            .input()
                            .registry
                            .agent(&binding.primary_agent)
                            .map(|a| a.to_domain())
                            .as_ref()
                    || skills.iter().any(|id| {
                        input.catalog.skill(id)
                            != resolution
                                .snapshot
                                .input()
                                .registry
                                .skill(id)
                                .map(|s| s.to_domain())
                                .as_ref()
                    })
                {
                    problems.insert(ProjectionProblem::CatalogMismatch);
                }
            }
        }
        let status = if problems.is_empty() {
            ProjectionStatus::CompatibleV1Shape
        } else {
            ProjectionStatus::Incompatible
        };
        Ok(ProjectionCompatibility {
            status,
            basis,
            problems,
        })
    }
}
