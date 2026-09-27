//! CG-10: revalidate resolution, evaluate current policy, project one active step.
use crate::{
    policy_application::{PolicyApplication, PolicyApplicationError, PolicyContext},
    resolution::ResolutionBasis,
    resolution_application::{
        DeclarativeResolutionApplication, ExternalPolicyResult, ProjectionInputs,
        ProjectionProblem, ProjectionStatus, ResolutionApplicationError, ResolvedPlan,
        WorkflowProjectionMapping,
    },
};
use gateway_context::{CompileError, CompiledContext, ContextDisclosurePolicy, ContextFragment};
use gateway_domain::{
    DefinitionCatalog, ExecutionContextIR, ExecutionContextId, ExecutionRuntimeId, ExecutionState,
    KnowledgeQuery, OriginalInput, ReferenceId, TaskDescriptor, ValidationError,
};
use gateway_policy::{PolicyAuthority, PolicyDecision, StepPolicyReport};
use std::collections::BTreeSet;

/// Explicit CG-02 mapping decision pinned to the same process/situation snapshot.
/// The caller owns task normalization and state mapping; the compiler never
/// translates arbitrary Process state names or authorizes a transition.
#[derive(Debug, Clone)]
pub struct ContextProjection {
    pub mapping: WorkflowProjectionMapping,
    pub id: ExecutionContextId,
    pub task: TaskDescriptor,
    pub state: ExecutionState,
    pub state_basis: ResolutionBasis,
    pub state_decision: ReferenceId,
    pub target_runtime: ExecutionRuntimeId,
    pub knowledge_queries: Vec<KnowledgeQuery>,
}

pub struct CompileStepInput<'a> {
    pub resolved: &'a ResolvedPlan,
    pub authority: &'a PolicyAuthority,
    pub policy_context: &'a PolicyContext,
    pub catalog: &'a DefinitionCatalog,
    pub projection: &'a ContextProjection,
    pub candidates: &'a [ContextFragment],
    pub selected: &'a BTreeSet<ReferenceId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextApplicationError {
    Resolution(ResolutionApplicationError),
    Policy(PolicyApplicationError),
    NotAuthorized(PolicyDecision),
    UnknownStep,
    NoExecutableBinding,
    StaleMapping,
    PolicyMismatch,
    Projection(ValidationError),
    Incompatible(BTreeSet<ProjectionProblem>),
    Assembly(CompileError),
}

/// Read-only compilation result, not a transferable authorization token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledStep {
    context: CompiledContext,
    basis: ResolutionBasis,
    output_contract: serde_json::Value,
    policy: StepPolicyReport,
    mapping: WorkflowProjectionMapping,
    state_decision: ReferenceId,
    restrictions: serde_json::Value,
    original_input: Option<OriginalInput>,
    process_state: serde_json::Value,
}
impl CompiledStep {
    pub fn context(&self) -> &CompiledContext {
        &self.context
    }
    pub fn basis(&self) -> &ResolutionBasis {
        &self.basis
    }
    pub fn original_input(&self) -> Option<&OriginalInput> {
        self.original_input.as_ref()
    }
    pub fn policy(&self) -> &StepPolicyReport {
        &self.policy
    }
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        self.serialize(None)
    }
    /// Export with a host-authenticated disclosure policy for external data.
    pub fn to_json_with_policy(
        &self,
        policy: ContextDisclosurePolicy,
    ) -> Result<String, serde_json::Error> {
        self.serialize(Some(policy))
    }
    fn serialize(
        &self,
        policy: Option<ContextDisclosurePolicy>,
    ) -> Result<String, serde_json::Error> {
        let context_json = match policy {
            Some(policy) => self.context.to_json_with_policy(policy)?,
            None => self.context.to_json()?,
        };
        let mut value: serde_json::Value = serde_json::from_str(&context_json)?;
        value["basis"] = crate::resolution_encoding::basis_json(&self.basis);
        value["stable"]["authority"] = serde_json::json!(self.policy.policies);
        value["user_input"] = self.original_input.as_ref().map(|original| {
            let (representation, content) = match original {
                OriginalInput::Inline(text) => ("inline", text.as_str()),
                OriginalInput::Reference(id) => ("reference", id.as_str()),
            };
            if policy.is_some_and(|policy| !policy.include_caller_input) {
                return serde_json::json!({"kind": "user_input", "trust": "CALLER_INPUT",
                    "representation": "redacted", "content": "[REDACTED]"});
            }
            serde_json::json!({"kind": "user_input", "trust": "CALLER_INPUT", "representation": representation,
                "content": content, "provenance": {"situation": self.basis.situation.as_str(), "snapshot": self.basis.situation_fingerprint.as_str()}})
        }).unwrap_or(serde_json::Value::Null);
        value["gateway"] = serde_json::json!({
            "task": self.context.execution_context().task(),
            "output_contract": self.output_contract,
            "constraints": self.restrictions,
            "policy": self.policy,
            "runtime_state": {"execution": self.context.execution_context().state(), "process": self.process_state},
            "provenance": {
                "plan": self.basis.plan.as_str(), "situation": self.basis.situation.as_str(),
                "workflow_mapping": self.mapping.decision_reference.as_str(),
                "state_mapping": self.state_decision.as_str(),
                "process_definition": self.mapping.process.id().as_str(),
                "process_version": self.mapping.process.version().to_string(),
                "process_digest": self.mapping.process.digest().as_str(),
            },
        });
        if policy.is_some_and(|policy| !policy.include_caller_input) {
            for field in ["task", "output_contract", "constraints"] {
                value["gateway"][field] = serde_json::json!({"representation":"redacted"});
            }
        }
        serde_json::to_string(&value)
    }
    pub fn explain(&self) -> String {
        self.context.explain()
    }
}

#[derive(Debug, Default)]
pub struct ContextApplication;
impl ContextApplication {
    pub fn compile_step(
        &self,
        input: CompileStepInput<'_>,
    ) -> Result<CompiledStep, ContextApplicationError> {
        let projection = input.projection;
        let mapping = &projection.mapping;
        let inspection = DeclarativeResolutionApplication
            .inspect_resolution(input.resolved)
            .map_err(ContextApplicationError::Resolution)?;
        if mapping.basis != inspection.basis
            || projection.state_basis != inspection.basis
            || projection.task.id() != &mapping.task
        {
            return Err(ContextApplicationError::StaleMapping);
        }
        let step = inspection
            .plan
            .steps()
            .iter()
            .find(|s| s.id() == &mapping.step)
            .ok_or(ContextApplicationError::UnknownStep)?;
        // Reevaluate using current authenticated facts; never accept an external Allow token.
        let report = PolicyApplication
            .evaluate(input.resolved, input.authority, input.policy_context)
            .map_err(ContextApplicationError::Policy)?;
        let policy = &report.steps()[&mapping.step];
        if policy.decision != PolicyDecision::Allow {
            return Err(ContextApplicationError::NotAuthorized(policy.decision));
        }
        let alternative = inspection
            .report
            .alternatives
            .first()
            .and_then(|items| items.iter().find(|a| a.step == mapping.step))
            .ok_or(ContextApplicationError::NoExecutableBinding)?;
        let binding = alternative
            .binding
            .as_ref()
            .ok_or(ContextApplicationError::NoExecutableBinding)?;
        let policy_input = inspection
            .policy_inputs
            .iter()
            .find(|p| &p.alternative == alternative)
            .ok_or(ContextApplicationError::NoExecutableBinding)?;
        let workflow = input
            .catalog
            .workflow(&mapping.workflow)
            .ok_or(ContextApplicationError::StaleMapping)?;
        // The workflow policy must be one of the authoritative policies actually evaluated.
        if !input
            .authority
            .policies
            .iter()
            .any(|p| p.id() == workflow.policy_id() && input.catalog.policy(p.id()) == Some(p))
        {
            return Err(ContextApplicationError::PolicyMismatch);
        }
        let mut constraints = input.authority.constraints.clone();
        constraints.sort_by(|a, b| a.id().cmp(b.id()));
        constraints.dedup();
        let mut queries = projection.knowledge_queries.clone();
        queries.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        queries.dedup();
        let skills = alternative
            .skills
            .as_ref()
            .map(|s| s.skills.clone())
            .unwrap_or_default();
        let context = ExecutionContextIR::new_v1(
            projection.id.clone(),
            projection.task.clone(),
            mapping.workflow.clone(),
            binding.primary_agent.clone(),
            skills,
            input.policy_context.operating_mode,
            input.policy_context.execution_profile,
            projection.state,
            workflow.policy_id().clone(),
            queries,
            policy_input.required_capabilities.keys().cloned(),
            constraints,
            projection.target_runtime.clone(),
        )
        .map_err(ContextApplicationError::Projection)?;
        let external = ExternalPolicyResult {
            basis: inspection.basis.clone(),
            step: mapping.step.clone(),
            policy: workflow.policy_id().clone(),
            decision: policy.decision,
            approved_capabilities: policy_input.required_capabilities.keys().cloned().collect(),
            decision_reference: mapping.decision_reference.clone(),
        };
        let compatibility = DeclarativeResolutionApplication
            .inspect_v1_projection(
                input.resolved,
                ProjectionInputs {
                    step: &mapping.step,
                    mapping: Some(mapping),
                    catalog: input.catalog,
                    context: Some(&context),
                    policy: Some(&external),
                },
            )
            .map_err(ContextApplicationError::Resolution)?;
        if compatibility.status != ProjectionStatus::CompatibleV1Shape {
            return Err(ContextApplicationError::Incompatible(
                compatibility.problems,
            ));
        }
        let context = CompiledContext::assemble(
            context,
            inspection.basis.scope.clone(),
            mapping.step.clone(),
            input.candidates,
            input.selected,
        )
        .map_err(ContextApplicationError::Assembly)?;
        let plan = serde_json::to_value(&inspection.plan).expect("validated Plan serializes");
        let step_wire = plan["steps"]
            .as_array()
            .expect("Plan steps")
            .iter()
            .find(|s| s["id"] == mapping.step.as_str())
            .expect("validated step");
        let output_contract = serde_json::json!({"completion": step_wire["completion"], "verification": step_wire["verification"]});
        let requirements: Vec<_> = plan["capability_requirements"]
            .as_array()
            .expect("Plan requirements")
            .iter()
            .filter(|r| {
                step.capability_requirements()
                    .iter()
                    .any(|id| r["id"] == id.as_str())
            })
            .collect();
        let desired = serde_json::to_value(&input.resolved.snapshot.input().desired)
            .expect("validated DesiredState serializes");
        let restrictions = serde_json::json!({
            "typed": context.execution_context().constraints(),
            "requirements": requirements,
            "capabilities": policy_input.required_capabilities.values().collect::<Vec<_>>(),
            "desired": desired["constraints"],
            "process": policy_input.process_constraints.iter().map(|c| (c.name(), c.value())).collect::<Vec<_>>(),
        });
        let original_input = input
            .resolved
            .snapshot
            .input()
            .situation
            .intent()
            .and_then(|i| i.original_input())
            .cloned();
        let process_state = input
            .resolved
            .snapshot
            .process()
            .map(|p| {
                serde_json::json!({
                    "instance": p.instance_id().as_str(), "revision": p.instance_revision().value(),
                    "current_state": p.current_state().as_str(), "status": p.status(),
                    "activity": alternative.activity,
                })
            })
            .unwrap_or(serde_json::Value::Null);
        Ok(CompiledStep {
            original_input,
            process_state,
            context,
            basis: inspection.basis,
            output_contract,
            policy: policy.clone(),
            mapping: mapping.clone(),
            state_decision: projection.state_decision.clone(),
            restrictions,
        })
    }
}
