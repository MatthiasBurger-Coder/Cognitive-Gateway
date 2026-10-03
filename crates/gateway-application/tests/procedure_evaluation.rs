use gateway_application::procedure_evaluation::{StepSimulationInput, simulate_step};
use gateway_domain::{
    CapabilityClass, CapabilityDefinition, CapabilityId, ContentDigest, ExecutionProfile,
    OperatingMode, PolicyDefinition, PolicyId, ReferenceId,
    learning::{ProcedureStep, ProcessReference},
    procedure_evaluation::InputStatus,
};
use gateway_policy::{Approval, PolicyAuthority, StepFacts};
use gateway_process::*;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn simulation_uses_real_engines_without_mutating_process_and_preserves_denials() {
    let source = "@process(inspect)\n@process-version(1)\n@cg-language(1)\nFeature: Inspect\nRule: Process\nGiven state START is initial\nGiven state END is terminal\nGiven event finish\nGiven gate review\nGiven activity inspect requires capability architecture.dependency-analysis\nGiven activity inspect constrained by mode=DEVELOPMENT\nScenario: finish\nGiven process state START\nWhen event finish occurs\nThen transition to state END\nThen authorize activity inspect\nThen require gate review\nThen complete process\n";
    let registry =
        ProcessRegistry::from_sources([ProcessSource::new("inspect.feature", source)]).unwrap();
    let definition = registry
        .get(
            &ProcessDefinitionId::new("inspect").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .unwrap();
    let instance =
        ProcessInstance::start(definition, ProcessInstanceId::new("instance-1").unwrap()).unwrap();
    let event = EventOccurrence::new(
        EventOccurrenceId::new("event-1").unwrap(),
        EventTypeId::new("finish").unwrap(),
        instance.id().clone(),
        instance.revision(),
    );
    let capability = CapabilityId::new("architecture.dependency-analysis").unwrap();
    let policy = PolicyId::new("policy-1").unwrap();
    let declared = ProcedureStep::new(
        ProcessReference::new(
            ReferenceId::new("inspect").unwrap(),
            1,
            ContentDigest::new(definition.identity().digest().as_str()).unwrap(),
        )
        .unwrap(),
        capability.clone(),
        policy.clone(),
    );
    let authority = PolicyAuthority {
        capabilities: BTreeMap::from([(
            capability.clone(),
            CapabilityDefinition::new(capability.clone(), CapabilityClass::Inspect),
        )]),
        policies: vec![
            PolicyDefinition::new(policy.clone(), "allow", [capability.clone()]).unwrap(),
        ],
        ..Default::default()
    };
    let inputs = EvaluationInputs::default()
        .with_capabilities([capability.clone()])
        .with_gate(GateId::new("review").unwrap(), GateStatus::Passed);
    let facts = StepFacts {
        authorizations: BTreeMap::from([(capability.clone(), Approval::Granted)]),
        satisfied_constraints: BTreeSet::from([
            serde_json::to_string(&("mode", "DEVELOPMENT")).unwrap()
        ]),
        ..Default::default()
    };
    let empty_facts = StepFacts::default();
    let empty_inputs = EvaluationInputs::default();
    let capture = |authority, inputs, facts, event| {
        simulate_step(StepSimulationInput {
            declared: &declared,
            definition,
            instance: &instance,
            event,
            inputs,
            authority,
            facts,
            operating_mode: OperatingMode::Development,
            execution_profile: ExecutionProfile::FullPath,
            execution: InputStatus::Present,
        })
        .unwrap()
    };
    let success = capture(&authority, &inputs, &facts, &event);
    assert!(success.process_allowed && success.policy_allowed && success.capability_available);
    assert_eq!(success, capture(&authority, &inputs, &facts, &event));
    assert_eq!(instance.current_state().as_str(), "START");
    let denied = PolicyAuthority {
        policies: vec![PolicyDefinition::new(policy, "deny", []).unwrap()],
        ..authority.clone()
    };
    let result = capture(&denied, &inputs, &facts, &event);
    assert!(result.process_allowed);
    assert!(!result.policy_allowed);
    assert!(result.policy_trace.contains("NOT_ALLOWLISTED"));
    assert!(!capture(&authority, &inputs, &empty_facts, &event).policy_allowed);
    let result = capture(&authority, &empty_inputs, &facts, &event);
    assert!(!result.capability_available);
    let failed_gate = inputs
        .clone()
        .with_gate(GateId::new("review").unwrap(), GateStatus::Failed);
    let result = capture(&authority, &failed_gate, &facts, &event);
    assert!(!result.process_allowed && !result.policy_allowed);
    assert!(
        result
            .process_trace
            .contains("required gate failed or is blocked")
    );
    let wrong_event = EventOccurrence::new(
        EventOccurrenceId::new("event-2").unwrap(),
        EventTypeId::new("unknown").unwrap(),
        instance.id().clone(),
        instance.revision(),
    );
    let result = capture(&authority, &inputs, &facts, &wrong_event);
    assert!(!result.process_allowed && !result.policy_allowed);
    let missing = PolicyAuthority::default();
    assert!(!capture(&missing, &inputs, &facts, &event).policy_allowed);
}
