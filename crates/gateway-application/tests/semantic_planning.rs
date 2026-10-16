use gateway_application::{
    PlanningCapabilitySnapshot, resolution_snapshot::*, semantic_planning::*,
};
use gateway_domain::*;
#[path = "support/composition.rs"]
#[allow(dead_code)]
mod composition;
#[allow(dead_code)]
mod support;

fn pin<I>(id: I, contract: &str) -> ResolvedReference<I> {
    ResolvedReference {
        id,
        scope: ContextScopeId::new("project-a").unwrap(),
        contract: ReferenceId::new(contract).unwrap(),
        contract_version: SchemaVersion::V1,
        revision: ReferenceId::new("r1").unwrap(),
        digest: ContentDigest::new("a".repeat(64)).unwrap(),
    }
}
fn data() -> SemanticTaskData {
    let compiled = gateway_cgsl::compile(
        "minimal.cgsl",
        include_bytes!("../../../examples/cgsl-v1/minimal.cgsl"),
    )
    .unwrap();
    let mut data = compiled.task_shape().unwrap().data().clone();
    data.target.reference.scope = ContextScopeId::new("project-a").unwrap();
    data.output_contract.schema.scope = data.target.reference.scope.clone();
    data.evidence_refs
        .push(pin(EvidenceId::new("evidence").unwrap(), "cg.evidence"));
    data.verification_contract
        .checks
        .push(SemanticVerificationCheck::EvidenceRequired);
    data.capability_requirements.push(pin(
        CapabilityId::new("architecture.dependency-analysis").unwrap(),
        "cg.capability",
    ));
    data
}
fn mapped(input: &ResolutionSnapshotInput) -> SemanticTaskIR {
    map_cg06_inputs(
        data(),
        &input.situation,
        pin(input.situation.context().id().clone(), "cg.context"),
        pin(input.situation.observed_state().id().clone(), "cg.state"),
    )
    .unwrap()
}
struct Host {
    task: SemanticTaskIR,
    source: DeclarativeContextSituationDocument,
    capabilities: PlanningCapabilitySnapshot,
    deny_pin: bool,
    deny_source: bool,
}
fn invalid() -> ValidationError {
    ValidationError::InvalidStateCombination {
        reason: "fixture admission denied",
    }
}
impl SemanticReferenceValidator for Host {
    fn validate(
        &self,
        task: &SemanticTaskData,
        _: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        if self.deny_pin || task != self.task.data() {
            Err(invalid())
        } else {
            Ok(())
        }
    }
}
impl SemanticPlanningHost for Host {
    fn validate_source(
        &self,
        task: &SemanticTaskIR,
        source: &DeclarativeContextSituationDocument,
        capabilities: &PlanningCapabilitySnapshot,
    ) -> Result<(), ValidationError> {
        if self.deny_source
            || task != &self.task
            || source != &self.source
            || capabilities != &self.capabilities
        {
            Err(invalid())
        } else {
            Ok(())
        }
    }
}
fn host(task: &SemanticTaskIR, input: &ResolutionSnapshotInput) -> Host {
    Host {
        task: task.clone(),
        source: input.situation.clone(),
        capabilities: PlanningCapabilitySnapshot::new(
            input.index.clone(),
            "fixture",
            PlanningIrVersion::V1,
        )
        .unwrap(),
        deny_pin: false,
        deny_source: false,
    }
}
fn rules() -> SemanticPlanningRules {
    SemanticPlanningRules {
        capabilities: CapabilityRequirementRules::default()
            .with_observation(CapabilityId::new("architecture.dependency-analysis").unwrap()),
        ..Default::default()
    }
}
fn run(
    task: &SemanticTaskIR,
    input: &ResolutionSnapshotInput,
    host: &Host,
    rules: &SemanticPlanningRules,
) -> Result<SemanticPlanHandoff, SemanticPlanningError> {
    let snapshot =
        PlanningCapabilitySnapshot::new(input.index.clone(), "fixture", PlanningIrVersion::V1)
            .unwrap();
    plan_semantic_task(
        task,
        &input.situation,
        host,
        DeltaId::new("delta").unwrap(),
        &snapshot,
        rules,
    )
}

#[test]
fn semantic_task_to_plan_and_resolution_reuses_existing_contracts() {
    let mut input = support::fixture();
    let task = mapped(&input);
    // `mapped` begins with the real offline frontend, then migrates CG-06 inputs.
    let host = host(&task, &input);
    let result = run(&task, &input, &host, &rules()).unwrap();
    assert_eq!(result, run(&task, &input, &host, &rules()).unwrap());
    assert_eq!(result.task(), &task);
    assert_eq!(result.source(), &input.situation);
    assert_eq!(result.delta(), &input.delta);
    assert_eq!(
        result.plan().capability_requirements()[0].capability(),
        &task.data().capability_requirements[0].id
    );
    input.plan = result.plan().clone();
    let resolved = result
        .resolve(&host, &input, &composition::rules())
        .unwrap();
    let direct = gateway_application::resolution_application::DeclarativeResolutionApplication
        .resolve_plan(&input, &composition::rules())
        .unwrap();
    assert_eq!(resolved, direct);
    assert_eq!(
        result.task().data().output_contract,
        task.data().output_contract
    );
    assert_eq!(
        result.task().data().verification_contract,
        task.data().verification_contract
    );
    assert_eq!(
        result.task().data().evidence_refs,
        task.data().evidence_refs
    );
}

#[test]
fn migration_rejects_conflicts_and_preserves_explicit_inputs() {
    let input = support::fixture();
    let task = mapped(&input);
    let context = task.data().context_refs[0].clone();
    let state = task.data().current_state.clone().unwrap();
    assert_eq!(
        map_cg06_inputs(
            task.data().clone(),
            &input.situation,
            context.clone(),
            state.clone()
        )
        .unwrap(),
        task
    );
    for field in [
        "context-id",
        "state-id",
        "context-revision",
        "state-revision",
        "desired",
    ] {
        let mut data = task.data().clone();
        let mut c = context.clone();
        let mut s = state.clone();
        match field {
            "context-id" => c.id = DeclarativeContextId::new("wrong").unwrap(),
            "state-id" => s.id = ObservedStateId::new("wrong").unwrap(),
            "context-revision" => c.revision = ReferenceId::new("wrong").unwrap(),
            "state-revision" => s.revision = ReferenceId::new("wrong").unwrap(),
            _ => {
                data.desired_state = Some(
                    DesiredState::new(
                        DesiredStateId::new("other").unwrap(),
                        input.desired.conditions().to_vec(),
                        input.desired.expression().clone(),
                        vec![],
                        vec![],
                    )
                    .unwrap(),
                )
            }
        }
        assert_eq!(
            map_cg06_inputs(data, &input.situation, c, s),
            Err(SemanticPlanningError::SourceMismatch)
        );
    }
    let mut bad = task.data().clone();
    bad.goal.description.clear();
    assert!(matches!(
        map_cg06_inputs(bad, &input.situation, context, state),
        Err(SemanticPlanningError::InvalidSemantics(_))
    ));
}

#[test]
fn stale_incomplete_unmapped_and_mismatched_tasks_fail_closed() {
    let input = support::fixture();
    let task = mapped(&input);
    let mut host = host(&task, &input);
    host.deny_pin = true;
    assert!(matches!(
        run(&task, &input, &host, &rules()),
        Err(SemanticPlanningError::InvalidSemantics(_))
    ));
    host.deny_pin = false;
    host.deny_source = true;
    assert!(matches!(
        run(&task, &input, &host, &rules()),
        Err(SemanticPlanningError::InvalidSemantics(_))
    ));
    host.deny_source = false;
    let mut changed = input.clone();
    changed.index = gateway_registry::CapabilityIndex::default();
    assert!(matches!(
        run(&task, &changed, &host, &rules()),
        Err(SemanticPlanningError::InvalidSemantics(_))
    ));
    for field in ["state", "context", "desired"] {
        let mut data = task.data().clone();
        match field {
            "state" => data.current_state = None,
            "context" => data.context_refs.clear(),
            _ => data.desired_state = None,
        }
        let bad = SemanticTaskIR::new(data).unwrap();
        assert_eq!(
            run(&bad, &input, &host, &rules()),
            Err(SemanticPlanningError::SourceMismatch)
        );
    }
    let mut bare = input.clone();
    bare.situation = DeclarativeContextSituationDocument::new(
        input.situation.context().clone(),
        None,
        None,
        input.situation.observed_state().clone(),
        input.situation.situation().clone(),
    )
    .unwrap();
    let task = mapped(&bare);
    let h = self::host(&task, &bare);
    assert_eq!(
        run(&task, &bare, &h, &rules()),
        Err(SemanticPlanningError::MissingDesiredState)
    );
    let task = mapped(&input);
    assert!(matches!(
        run(&task, &input, &host, &SemanticPlanningRules::default()),
        Err(SemanticPlanningError::UnmappedCapability(_))
    ));
    let mut data = task.data().clone();
    data.capability_requirements.clear();
    let task = SemanticTaskIR::new(data).unwrap();
    assert_eq!(
        run(
            &task,
            &input,
            &self::host(&task, &input),
            &SemanticPlanningRules::default()
        ),
        Err(SemanticPlanningError::PlanningBlocked)
    );
}

#[test]
fn resolution_revalidates_and_rejects_changed_snapshots() {
    let mut input = support::fixture();
    let task = mapped(&input);
    let mut host = host(&task, &input);
    let handoff = run(&task, &input, &host, &rules()).unwrap();
    input.plan = handoff.plan().clone();
    host.deny_source = true;
    assert!(matches!(
        handoff.resolve(&host, &input, &composition::rules()),
        Err(SemanticPlanningError::InvalidSemantics(_))
    ));
    host.deny_source = false;
    let mut changed = input.clone();
    changed.index = gateway_registry::CapabilityIndex::default();
    assert_eq!(
        handoff.resolve(&host, &changed, &composition::rules()),
        Err(SemanticPlanningError::ResolutionMismatch)
    );
    let mut bad = input.clone();
    bad.scope = ContextScopeId::new("other").unwrap();
    assert_eq!(
        handoff.resolve(&host, &bad, &composition::rules()),
        Err(SemanticPlanningError::ResolutionMismatch)
    );
    let mut bad = input.clone();
    bad.rule_version = SchemaVersion::V2;
    assert!(matches!(
        handoff.resolve(&host, &bad, &composition::rules()),
        Err(SemanticPlanningError::Resolution(_))
    ));
    let input = support::with_process(input);
    let definition = input.processes.definitions().next().unwrap().identity();
    let mut data = task.data().clone();
    data.process = Some(
        gateway_domain::learning::ProcessReference::new(
            ReferenceId::new(definition.id().as_str()).unwrap(),
            definition.version().value(),
            ContentDigest::new(definition.digest().as_str()).unwrap(),
        )
        .unwrap(),
    );
    let task = SemanticTaskIR::new(data).unwrap();
    let host = self::host(&task, &input);
    let handoff = run(&task, &input, &host, &rules()).unwrap();
    let mut r = composition::rules();
    assert_eq!(
        handoff.resolve(&host, &input, &r),
        Err(SemanticPlanningError::ResolutionMismatch)
    );
    r.processes.required_definition = Some(definition.clone());
    assert!(handoff.resolve(&host, &input, &r).is_ok());
}
