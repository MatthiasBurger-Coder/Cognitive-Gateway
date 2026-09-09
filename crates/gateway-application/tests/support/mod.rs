use gateway_application::{
    DeclarativePlanningApplication, DeclarativeSituationApplication, ProcessSnapshotInput,
    resolution_snapshot::*,
};
use gateway_domain::*;
use gateway_process::{ProcessInstance, ProcessInstanceId, ProcessRegistry};
use gateway_registry::Registry;
use std::path::PathBuf;

pub fn fixture() -> ResolutionSnapshotInput {
    let condition = DesiredCondition::new(
        ConditionId::new("condition").unwrap(),
        SubjectPath::new(["quality", "passed"]).unwrap(),
        ComparisonOperator::Equals,
        Some(TypedValue::Boolean(true)),
    )
    .unwrap();
    let desired = DesiredState::new(
        DesiredStateId::new("desired").unwrap(),
        vec![condition],
        ConditionExpression::condition(ConditionId::new("condition").unwrap()),
        vec![],
        vec![],
    )
    .unwrap();
    let current = CurrentState::new_v1(ObservedStateId::new("current").unwrap());
    let situation = SituationAssemblyInput::new(current.clone())
        .assemble(SituationId::new("situation").unwrap())
        .unwrap();
    let delta = DeclarativePlanningApplication::new()
        .derive_delta(
            DeltaId::new("delta").unwrap(),
            &desired,
            &current,
            Some(&situation),
            &ComparisonRules::default(),
            &DeltaDerivationRules::default(),
        )
        .unwrap()
        .delta()
        .clone();
    let requirements = delta
        .items()
        .iter()
        .map(|item| {
            CapabilityRequirement::new(
                CapabilityRequirementId::new("requirement").unwrap(),
                CapabilityId::new("architecture.dependency-analysis").unwrap(),
                RequirementCardinality::Mandatory,
                item.id().clone(),
                "observe unknown state",
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let plan = gateway_domain::plan(&desired, &delta, &requirements, &PlannerRules::default())
        .unwrap()
        .plan()
        .unwrap()
        .clone();
    let document = DeclarativeContextSituationDocument::new(
        DeclarativeContext::new_v1(DeclarativeContextId::new("context").unwrap()),
        Some(Intent::new(
            IntentId::new("intent").unwrap(),
            desired.clone(),
        )),
        None,
        current,
        situation,
    )
    .unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
    let registry = Registry::load(&root).unwrap();
    let index = registry.capability_index().unwrap();
    ResolutionSnapshotInput {
        version: SchemaVersion::V1,
        scope: ContextScopeId::new("project-a").unwrap(),
        plan_scope: ContextScopeId::new("project-a").unwrap(),
        situation_scope: ContextScopeId::new("project-a").unwrap(),
        plan,
        desired,
        delta,
        situation: document,
        operating_mode: OperatingMode::Hardening,
        execution_profile: ExecutionProfile::FullPath,
        registry,
        index,
        processes: ProcessRegistry::load(root.join("processes")).unwrap(),
        instance: None,
        expected_revision: None,
        situation_process: None,
        admission: None,
        rule_version: SchemaVersion::V1,
        alternatives: vec![],
    }
}

pub fn with_process(mut input: ResolutionSnapshotInput) -> ResolutionSnapshotInput {
    let definition = input.processes.definitions().next().unwrap();
    let instance =
        ProcessInstance::start(definition, ProcessInstanceId::new("instance").unwrap()).unwrap();
    input.situation_process = Some(
        DeclarativeSituationApplication::new()
            .process_reference(ProcessSnapshotInput::new(definition, &instance))
            .unwrap(),
    );
    input.expected_revision = Some(instance.revision());
    input.instance = Some(instance);
    input
}
