//! CG-09 orchestration. Policy has no lifecycle mutation or provider API access.
use crate::{
    resolution::{LifecycleReadiness, ResolutionBasis, ResolutionOutcome},
    resolution_application::{
        DeclarativeResolutionApplication, ResolutionApplicationError, ResolvedPlan,
    },
};
use gateway_domain::{
    CapabilityDefinition, CapabilityId, ExecutionProfile, OperatingMode, PlanStepId,
};
use gateway_policy::{
    PolicyAuthority, PolicyDecision, PolicyEngine, ProcessReadiness, StepFacts, StepPolicyInput,
    StepPolicyReport,
};
use gateway_process::{EvaluationInputs, PolicyDecisionId, PolicyDecisionStatus};
use std::collections::{BTreeMap, BTreeSet};

/// Trusted facts pinned to the exact plan, scope, catalog and process revision.
/// The adapter must authenticate these independently of resolution/model output.
pub struct PolicyContext {
    pub basis: ResolutionBasis,
    pub operating_mode: OperatingMode,
    pub execution_profile: ExecutionProfile,
    pub steps: BTreeMap<PlanStepId, StepFacts>,
}

/// Native inputs assembled from one verified resolution and matching trusted
/// context. This is neither a permission grant nor a deserialization boundary.
#[derive(Debug, Clone)]
pub struct ResolvedStepPolicyInput {
    pub step: PlanStepId,
    pub capabilities: BTreeMap<CapabilityId, CapabilityDefinition>,
    pub operating_mode: OperatingMode,
    pub execution_profile: ExecutionProfile,
    pub process: ProcessReadiness,
    pub resolved: bool,
    pub has_prerequisites: bool,
    pub constraints: BTreeSet<String>,
    pub preconditions: BTreeSet<String>,
    pub facts: StepFacts,
}
impl ResolvedStepPolicyInput {
    pub fn as_input(&self) -> StepPolicyInput<'_> {
        StepPolicyInput {
            step: &self.step,
            capabilities: &self.capabilities,
            operating_mode: self.operating_mode,
            execution_profile: self.execution_profile,
            process: self.process,
            resolved: self.resolved,
            has_prerequisites: self.has_prerequisites,
            constraints: &self.constraints,
            preconditions: &self.preconditions,
            facts: &self.facts,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyApplicationError {
    Resolution(ResolutionApplicationError),
    StaleContext,
    UnknownStep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanPolicyReport {
    basis: ResolutionBasis,
    steps: BTreeMap<PlanStepId, StepPolicyReport>,
}
impl PlanPolicyReport {
    /// Canonical audit document. It is evidence, never a reusable permission token.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&serde_json::json!({
            "schema_version": 1,
            "basis": crate::resolution_encoding::basis_json(&self.basis),
            "decision": self.decision(),
            "steps": self.steps.values().collect::<Vec<_>>(),
        }))
    }
    pub fn basis(&self) -> &ResolutionBasis {
        &self.basis
    }
    pub fn steps(&self) -> &BTreeMap<PlanStepId, StepPolicyReport> {
        &self.steps
    }
    pub fn decision(&self) -> PolicyDecision {
        self.steps
            .values()
            .map(|s| s.decision)
            .max()
            .unwrap_or(PolicyDecision::Allow)
    }
    /// Adds the exact step decision to a declared Process Engine policy gate.
    /// Recheck the captured basis before use; process evaluation still owns
    /// lifecycle, revision checks, evidence gates and blockers.
    pub fn gate_inputs(
        &self,
        current: &ResolutionBasis,
        step: &PlanStepId,
        gate: PolicyDecisionId,
        inputs: EvaluationInputs,
    ) -> Result<EvaluationInputs, PolicyApplicationError> {
        if current != &self.basis {
            return Err(PolicyApplicationError::StaleContext);
        }
        let report = self
            .steps
            .get(step)
            .ok_or(PolicyApplicationError::UnknownStep)?;
        let status = match report.decision {
            PolicyDecision::Allow => PolicyDecisionStatus::Allow,
            PolicyDecision::Deny => PolicyDecisionStatus::Deny,
            PolicyDecision::RequireConsent | PolicyDecision::RequireEvidence => {
                PolicyDecisionStatus::Waiting
            }
        };
        // Preserve an already stricter decision: an allow cannot erase a deny.
        let status = match inputs.policy().decisions().get(&gate) {
            Some(PolicyDecisionStatus::Deny) => PolicyDecisionStatus::Deny,
            Some(PolicyDecisionStatus::Waiting) if status == PolicyDecisionStatus::Allow => {
                PolicyDecisionStatus::Waiting
            }
            _ => status,
        };
        Ok(inputs.with_policy_decision(gate, status))
    }
}

#[derive(Debug, Default)]
pub struct PolicyApplication;
impl PolicyApplication {
    pub fn evaluate(
        &self,
        resolved: &ResolvedPlan,
        authority: &PolicyAuthority,
        context: &PolicyContext,
    ) -> Result<PlanPolicyReport, PolicyApplicationError> {
        let (basis, inputs) = self.assemble(resolved, context)?;
        Ok(PlanPolicyReport {
            basis,
            steps: inputs
                .into_iter()
                .map(|(step, input)| (step, PolicyEngine::evaluate(authority, &input.as_input())))
                .collect(),
        })
    }

    /// Assemble the same exact step inputs consumed by whole-plan evaluation.
    /// The caller must still evaluate current policy and verify that its
    /// requested capability belongs to this canonical capability set.
    pub fn step_input(
        &self,
        resolved: &ResolvedPlan,
        context: &PolicyContext,
        step: &PlanStepId,
    ) -> Result<ResolvedStepPolicyInput, PolicyApplicationError> {
        self.step_inputs(resolved, context)?
            .remove(step)
            .ok_or(PolicyApplicationError::UnknownStep)
    }

    /// Assemble all canonical step inputs in one complete native validation.
    /// Returned inputs are owned data, not policy decisions or reusable current
    /// authority. Each new authority boundary must capture its inputs anew.
    pub fn step_inputs(
        &self,
        resolved: &ResolvedPlan,
        context: &PolicyContext,
    ) -> Result<BTreeMap<PlanStepId, ResolvedStepPolicyInput>, PolicyApplicationError> {
        Ok(self.assemble(resolved, context)?.1)
    }

    fn assemble(
        &self,
        resolved: &ResolvedPlan,
        context: &PolicyContext,
    ) -> Result<
        (
            ResolutionBasis,
            BTreeMap<PlanStepId, ResolvedStepPolicyInput>,
        ),
        PolicyApplicationError,
    > {
        let inspection = DeclarativeResolutionApplication
            .inspect_resolution(resolved)
            .map_err(PolicyApplicationError::Resolution)?;
        if context.basis != inspection.basis
            || context.operating_mode != resolved.snapshot.input().operating_mode
            || context.execution_profile != resolved.snapshot.input().execution_profile
        {
            return Err(PolicyApplicationError::StaleContext);
        }
        if context
            .steps
            .keys()
            .any(|id| !inspection.plan.steps().iter().any(|s| s.id() == id))
        {
            return Err(PolicyApplicationError::UnknownStep);
        }
        // Only a unique, complete whole-plan binding can be authorized.
        let selected = if inspection.report.outcome == ResolutionOutcome::Resolved
            && inspection.report.alternatives.len() == 1
        {
            inspection.report.alternatives.first()
        } else {
            None
        };
        let mut steps = BTreeMap::new();
        for step in inspection.plan.steps() {
            let binding = selected.and_then(|items| items.iter().find(|b| &b.step == step.id()));
            let policy_input = binding.and_then(|b| {
                inspection
                    .policy_inputs
                    .iter()
                    .find(|p| &p.alternative == b)
            });
            let noop = step.kind() == gateway_domain::PlanStepKind::NoOp;
            let ready = policy_input.map(|p| p.alternative.applicability.readiness);
            let process = match ready {
                Some(LifecycleReadiness::Eligible) => ProcessReadiness::Eligible,
                Some(LifecycleReadiness::Blocked) => ProcessReadiness::Blocked,
                Some(LifecycleReadiness::NotApplicable) => ProcessReadiness::NotApplicable,
                _ if noop => ProcessReadiness::NotApplicable,
                _ => ProcessReadiness::Unknown,
            };
            let empty = BTreeMap::new();
            let capabilities = policy_input.map_or(&empty, |p| &p.required_capabilities);
            let mut constraints: BTreeSet<_> = resolved
                .snapshot
                .input()
                .desired
                .constraints()
                .iter()
                .map(|c| format!("desired:{}", c.id()))
                .collect();
            let mut preconditions = BTreeSet::new();
            for requirement in inspection
                .plan
                .capability_requirements()
                .iter()
                .filter(|r| step.capability_requirements().contains(r.id()))
            {
                constraints.extend(requirement.constraints().iter().map(ToString::to_string));
                preconditions.extend(requirement.preconditions().iter().map(ToString::to_string));
            }
            if let Some(input) = policy_input {
                for constraint in &input.process_constraints {
                    // JSON tuple encoding prevents collisions between names and values.
                    constraints.insert(
                        serde_json::to_string(&(constraint.name(), constraint.value()))
                            .expect("string tuple serializes"),
                    );
                }
            }
            let facts = context.steps.get(step.id()).cloned().unwrap_or_default();
            let input = ResolvedStepPolicyInput {
                step: step.id().clone(),
                capabilities: capabilities.clone(),
                operating_mode: context.operating_mode,
                execution_profile: context.execution_profile,
                process,
                resolved: noop || policy_input.is_some(),
                has_prerequisites: !step.prerequisites().is_empty()
                    || !step.dependencies().is_empty(),
                constraints,
                preconditions,
                facts,
            };
            steps.insert(step.id().clone(), input);
        }
        Ok((inspection.basis, steps))
    }
}
