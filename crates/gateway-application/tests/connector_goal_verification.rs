use std::collections::{BTreeMap, BTreeSet};

use gateway_application::{
    PlanningCapabilitySnapshot, ScopedObservationBatch, SourceSnapshot,
    closed_loop::{
        ClosedLoop, ExecutionOutcome, ExecutionRuntimePort, LoopDecision, LoopRules, OutcomeStatus,
    },
    connector_results::{ConnectorResult, ConnectorResultMetadata},
    connectors::{
        ConnectorDispatch, ConnectorEffect, ConnectorObservation, ConnectorObservationBasis,
        ConnectorOperationClass, ConnectorRequest, ConnectorTarget,
    },
    context_application::CompileStepInput,
    resolution_snapshot::ResolutionSnapshotInput,
    sessions::{ConditionObservationMapping, RecordId, RecordRef, SupportedConnectorGoal},
};
use gateway_domain::*;

#[allow(dead_code)]
#[path = "support/composition.rs"]
mod composition;
#[allow(dead_code)]
#[path = "support/context_fixture.rs"]
mod fixture;
mod support;

fn scope() -> ContextScopeId {
    ContextScopeId::new("project-a").unwrap()
}

fn registered_fixture() -> (SupportedConnectorGoal, fixture::Fixture) {
    let fixture = fixture::Fixture::new();
    let condition = fixture.resolved.snapshot.input().desired.conditions()[0].clone();
    let step = fixture.projection.mapping.step.clone();
    let goal = SupportedConnectorGoal::from_host_registration(
        Intent::new(
            IntentId::new("connector-goal").unwrap(),
            fixture.resolved.snapshot.input().desired.clone(),
        ),
        scope(),
        RecordRef::new(
            RecordId::new("registered-plan").unwrap(),
            RecordId::new("1").unwrap(),
            format!("sha256:{}", "a".repeat(64)),
        )
        .unwrap(),
        vec![step],
        vec![
            ConditionObservationMapping::new(
                condition.id().clone(),
                SourceId::new("connector-read-source").unwrap(),
                CapabilityId::new("connector.read-state").unwrap(),
                ConnectorOperationClass::Read,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    (goal, fixture)
}

fn empty_initial(scope: ContextScopeId) -> ScopedObservationBatch {
    ScopedObservationBatch::new(
        scope,
        SourceSnapshot::new(
            SourceId::new("initial-state").unwrap(),
            SourceKind::Runtime,
            None,
            Some(ContentDigest::new("0".repeat(64)).unwrap()),
        )
        .unwrap(),
        ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap(),
    )
    .unwrap()
}

fn loop_rules(input: &ResolutionSnapshotInput) -> LoopRules {
    LoopRules {
        capabilities: PlanningCapabilitySnapshot::new(
            input.index.clone(),
            "connector-goal-test",
            PlanningIrVersion::V1,
        )
        .unwrap(),
        requirements: CapabilityRequirementRules::default()
            .with_observation(CapabilityId::new("architecture.dependency-analysis").unwrap())
            .with_evidence_acquisition(
                CapabilityId::new("architecture.dependency-analysis").unwrap(),
            ),
        planner: PlannerRules::default(),
        max_iterations: 2,
        max_retries: 1,
    }
}

fn start_loop(goal: &SupportedConnectorGoal, input: &ResolutionSnapshotInput) -> ClosedLoop {
    ClosedLoop::start(
        ReferenceId::new("connector-goal-run").unwrap(),
        goal.scope().clone(),
        goal.intent().clone(),
        empty_initial(goal.scope().clone()),
        loop_rules(input),
    )
    .unwrap()
}

fn compile_fixture(run: &ClosedLoop) -> fixture::Fixture {
    let mut input = fixture::input();
    let assessment = run.assessment();
    input.plan = assessment.plan.clone().unwrap();
    input.delta = assessment.delta.clone();
    input.desired = assessment
        .document
        .intent()
        .unwrap()
        .desired_state()
        .clone();
    input.situation = assessment.document.clone();
    fixture::Fixture::from_input(input)
}

fn mutate_request(step: &PlanStepId) -> ConnectorRequest {
    ConnectorRequest {
        target: ConnectorTarget {
            connector: NonEmptyText::new("registered-connector").unwrap(),
            scope: scope(),
        },
        capability: CapabilityId::new("connector.mutate-state").unwrap(),
        class: ConnectorOperationClass::Mutate,
        inventory_revision: ContentDigest::new("1".repeat(64)).unwrap(),
        arguments: BTreeMap::new(),
        task: TaskId::new("connector-goal-task").unwrap(),
        step: step.clone(),
        execution: NonEmptyText::new("mutation-execution").unwrap(),
        correlation: NonEmptyText::new("mutation-correlation").unwrap(),
    }
}

fn connector_result(
    goal: &SupportedConnectorGoal,
    step: &PlanStepId,
    execution: &ReferenceId,
    basis: ConnectorObservationBasis,
) -> ConnectorResult {
    let mutation = mutate_request(step);
    let request = ConnectorRequest {
        target: mutation.target.clone(),
        capability: CapabilityId::new("connector.read-state").unwrap(),
        class: ConnectorOperationClass::Read,
        inventory_revision: mutation.inventory_revision.clone(),
        arguments: BTreeMap::new(),
        task: mutation.task.clone(),
        step: step.clone(),
        execution: NonEmptyText::new(execution.as_str()).unwrap(),
        correlation: NonEmptyText::new("readback-correlation").unwrap(),
    };
    let dispatch = ConnectorDispatch {
        request: request.clone(),
        policy: PolicyId::new("connector-policy").unwrap(),
        policy_revision: ContentDigest::new("2".repeat(64)).unwrap(),
        consent_receipt: Some(NonEmptyText::new("trusted-consent-receipt").unwrap()),
        budget_reservation: NonEmptyText::new("budget-reservation").unwrap(),
        deadline: UnixTimestamp::new(100),
    };
    let source = goal.observations()[0].source().clone();
    let acquired = UnixTimestamp::new(20);
    let source_revision = SourceTimestamp::new("source-revision-2").unwrap();
    let producer = "authenticated-connector-host";
    let provenance = Provenance::new(
        ProvenanceId::new("readback-provenance").unwrap(),
        SourceKind::Tool,
        source.clone(),
        "connector://registered/state",
    )
    .unwrap()
    .with_producer(producer)
    .unwrap()
    .with_acquired_at(SourceTimestamp::new(acquired.seconds().to_string()).unwrap())
    .with_source_timestamp(source_revision.clone());
    let subject = goal.intent().desired_state().conditions()[0]
        .subject()
        .clone();
    let observation = Observation::new(
        ObservationId::new("readback-observation").unwrap(),
        subject.clone(),
        TypedValue::Boolean(true),
        provenance.id().clone(),
    )
    .unwrap();
    let fact = Fact::new(
        FactId::new("readback-fact").unwrap(),
        subject.clone(),
        TypedValue::Boolean(true),
        AssertionPolarity::Affirmed,
        vec![observation.id().clone()],
    )
    .unwrap();
    let evidence = Evidence::new(
        EvidenceId::new("readback-evidence").unwrap(),
        EvidenceKind::Report,
        "Independent connector state read",
        EvidenceContent::inline("The registered state condition was read as true.").unwrap(),
        provenance.id().clone(),
        vec![EvidenceLink::new(
            fact.id().clone(),
            EvidenceRelation::Supports,
        )],
    )
    .unwrap();
    let records = ObservationEvidenceSet::new(
        vec![provenance],
        vec![observation],
        vec![fact],
        vec![evidence],
    )
    .unwrap();
    let quality = QualityMetadata::new(
        TrustClass::ObservedEvidence,
        SensitivityClass::Public,
        Confidence::Unknown,
        FreshnessStatus::Fresh,
        Uncertainty::None,
    );
    let connector_observation = ConnectorObservation {
        metadata: ConnectorResultMetadata {
            task: request.task.clone(),
            step: request.step.clone(),
            execution: request.execution.clone(),
            object_reference: NonEmptyText::new("connector://registered/state").unwrap(),
            source_kind: SourceKind::Tool,
            source_digest: Some(ContentDigest::new("3".repeat(64)).unwrap()),
            observed_at: Some(acquired),
            authenticated_source: NonEmptyText::new(producer).unwrap(),
            quality,
            effect: ConnectorEffect::NoMutation,
            partial: false,
            verifies: if basis == ConnectorObservationBasis::IndependentPostEffectRead {
                Some(mutation)
            } else {
                None
            },
            non_authoritative_raw: None,
        },
        target: request.target.clone(),
        inventory_revision: request.inventory_revision.clone(),
        correlation: request.correlation.clone(),
        source,
        source_revision: Some(source_revision),
        content_digest: ContentDigest::new("4".repeat(64)).unwrap(),
        acquired_at: acquired,
        records,
        basis,
    };
    ConnectorResult::new(&dispatch, connector_observation).unwrap()
}

struct ResultRuntime {
    goal: SupportedConnectorGoal,
    step: PlanStepId,
    basis: ConnectorObservationBasis,
}

impl ExecutionRuntimePort for ResultRuntime {
    fn execute(
        &mut self,
        execution: &ReferenceId,
        _: &gateway_application::context_application::CompiledStep,
    ) -> ExecutionOutcome {
        let result = connector_result(&self.goal, &self.step, execution, self.basis);
        // This is the canonical conversion boundary: acknowledgement-only
        // ConnectorResult values cannot enter CG-14's observation assessment.
        let observations = result.batch().ok();
        ExecutionOutcome {
            execution: execution.clone(),
            status: OutcomeStatus::Completed,
            observations,
        }
    }
}

fn run_once(
    goal: &SupportedConnectorGoal,
    input: &ResolutionSnapshotInput,
    basis: ConnectorObservationBasis,
) -> ClosedLoop {
    let mut run = start_loop(goal, input);
    let execution_fixture = compile_fixture(&run);
    let mut runtime = ResultRuntime {
        goal: goal.clone(),
        step: execution_fixture.projection.mapping.step.clone(),
        basis,
    };
    let selected = BTreeSet::new();
    let decision = run
        .execute(
            CompileStepInput {
                resolved: &execution_fixture.resolved,
                authority: &execution_fixture.authority,
                policy_context: &execution_fixture.policy,
                catalog: &execution_fixture.catalog,
                projection: &execution_fixture.projection,
                candidates: &[],
                selected: &selected,
            },
            &mut runtime,
        )
        .unwrap();
    if basis == ConnectorObservationBasis::ProviderAcknowledgement {
        assert_eq!(decision, LoopDecision::Pause);
    } else {
        assert_eq!(decision, LoopDecision::Success);
    }
    run
}

#[test]
fn acknowledgement_cannot_verify_registered_connector_goal_but_independent_read_can() {
    let (goal, initial) = registered_fixture();

    let acknowledgement = run_once(
        &goal,
        &initial.resolved.snapshot.input().clone(),
        ConnectorObservationBasis::ProviderAcknowledgement,
    );
    assert!(acknowledgement.verified_outcome().is_none());

    let independently_observed = run_once(
        &goal,
        &initial.resolved.snapshot.input().clone(),
        ConnectorObservationBasis::IndependentPostEffectRead,
    );
    let receipt = independently_observed.verified_outcome().unwrap();
    assert_eq!(
        receipt.outcome(),
        gateway_application::experience_patterns::OutcomeClass::Success
    );
    assert_eq!(receipt.scope(), goal.scope());
    assert!(!receipt.facts().is_empty());
    assert!(!receipt.evidence().is_empty());
}
