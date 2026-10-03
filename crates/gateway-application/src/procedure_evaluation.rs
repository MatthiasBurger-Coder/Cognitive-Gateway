//! Capture real deterministic Process/Policy decisions for non-mutating replay.
//! The driving adapter authenticates the authority and request facts. Captured
//! decisions are simulation evidence, never reusable runtime permission tokens.
use std::collections::{BTreeMap, BTreeSet};

use gateway_domain::learning::{ProcedureStep, ProcessReference};
use gateway_domain::procedure_evaluation::{InputStatus, StepSnapshot};
use gateway_domain::{
    ContentDigest, ExecutionProfile, OperatingMode, PlanStepId, ReferenceId, ValidationError,
};
use gateway_policy::{
    PolicyAuthority, PolicyDecision, PolicyEngine, ProcessReadiness, StepFacts, StepPolicyInput,
};
use gateway_process::{
    EvaluationInputs, EventOccurrence, ProcessDefinition, ProcessInstance, TransitionEvaluator,
};

pub struct StepSimulationInput<'a> {
    pub declared: &'a ProcedureStep,
    pub definition: &'a ProcessDefinition,
    pub instance: &'a ProcessInstance,
    pub event: &'a EventOccurrence,
    pub inputs: &'a EvaluationInputs,
    pub authority: &'a PolicyAuthority,
    pub facts: &'a StepFacts,
    pub operating_mode: OperatingMode,
    pub execution_profile: ExecutionProfile,
    pub execution: InputStatus,
}

pub fn simulate_step(input: StepSimulationInput<'_>) -> Result<StepSnapshot, ValidationError> {
    let process =
        TransitionEvaluator::evaluate(input.definition, input.instance, input.event, input.inputs);
    let process_allowed = process.accepted()
        && process
            .authorized_activity_definition()
            .is_some_and(|activity| {
                activity
                    .capabilities()
                    .contains(input.declared.capability())
            });
    let identity = input.definition.identity();
    let capabilities = input
        .authority
        .capabilities
        .get(input.declared.capability())
        .map(|capability| BTreeMap::from([(capability.id().clone(), capability.clone())]))
        .unwrap_or_default();
    let constraints = process
        .authorized_activity_definition()
        .into_iter()
        .flat_map(|activity| activity.constraints())
        .map(|constraint| {
            serde_json::to_string(&(constraint.name(), constraint.value()))
                .expect("constraint strings serialize")
        })
        .collect::<BTreeSet<_>>();
    let step = PlanStepId::new("procedure-simulation")?;
    let preconditions = BTreeSet::new();
    let policy = PolicyEngine::evaluate(
        input.authority,
        &StepPolicyInput {
            step: &step,
            capabilities: &capabilities,
            operating_mode: input.operating_mode,
            execution_profile: input.execution_profile,
            process: if process_allowed {
                ProcessReadiness::Eligible
            } else {
                ProcessReadiness::Blocked
            },
            resolved: process_allowed,
            has_prerequisites: false,
            constraints: &constraints,
            preconditions: &preconditions,
            facts: input.facts,
        },
    );
    Ok(StepSnapshot {
        process: ProcessReference::new(
            ReferenceId::new(identity.id().as_str())?,
            identity.version().value(),
            ContentDigest::new(identity.digest().as_str())?,
        )?,
        capability: input.declared.capability().clone(),
        policy: input.declared.policy().clone(),
        capability_available: capabilities.contains_key(input.declared.capability())
            && input
                .inputs
                .capabilities()
                .contains(input.declared.capability()),
        process_allowed,
        policy_allowed: policy.decision == PolicyDecision::Allow
            && input
                .authority
                .policies
                .iter()
                .any(|policy| policy.id() == input.declared.policy()),
        process_trace:
            serde_json::json!({"accepted": process.accepted(), "reason": process.reason(),
            "constraints": process.constraint_evaluations(), "guards": process.guard_evaluations()})
            .to_string(),
        policy_trace: serde_json::to_string(&policy).expect("policy report serializes"),
        execution: input.execution,
    })
}
