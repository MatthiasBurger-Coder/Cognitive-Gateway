use std::collections::BTreeMap;

use gateway_application::{
    connector_proposals::{DispatchedProposal, ProposalBatch, ProposalError},
    connector_results::{ConnectorResult, ConnectorResultMetadata},
    connectors::{
        ConnectorDispatch, ConnectorEffect, ConnectorObservation, ConnectorObservationBasis,
        ConnectorOperationClass, ConnectorRequest, ConnectorTarget,
    },
    output_parsing::parse_invocation_output,
    output_verification::{ConstraintReport, OutputVerificationPort, ParsedOutput, verify_output},
    prompt_ir::{InvocationAdmissionPort, InvocationIr, admit_invocation_ir},
    prompt_rendering::ToolDefinition,
    sessions::{
        ConditionObservationMapping, ConnectorSessionTurn, CorrelatedConnectorReadback,
        GoalVerificationReadback, RecordId, RecordRef, ScopedConnectorSnapshotPayload,
        SessionError, SupportedConnectorGoal,
    },
};
use gateway_domain::{output_verification::*, prompt_ir::*, semantic_output::*, *};
use serde_json::json;

type TurnMutation = Box<dyn Fn(&mut ConnectorSessionTurn)>;
type GoalReadbackMutation = Box<dyn Fn(&mut GoalVerificationReadback)>;

#[path = "../../../tests/fixtures/prompt-rendering-v1/support.rs"]
mod model_support;

struct Admission;
impl SemanticReferenceValidator for Admission {
    fn validate(
        &self,
        _: &SemanticTaskData,
        _: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl InvocationAdmissionPort for Admission {
    fn validate_current(&self, _: &InvocationIr) -> Result<(), InvocationIrError> {
        Ok(())
    }
}

struct OutputHost;
impl SemanticReferenceValidator for OutputHost {
    fn validate(
        &self,
        _: &SemanticTaskData,
        _: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl InvocationAdmissionPort for OutputHost {
    fn validate_current(&self, _: &InvocationIr) -> Result<(), InvocationIrError> {
        Ok(())
    }
}
impl OutputVerificationPort for OutputHost {
    fn resolve_reference(
        &self,
        _: &InvocationIr,
        _: &OutputReference,
    ) -> Result<(), VerificationError> {
        Ok(())
    }

    fn evaluate(
        &self,
        ir: &InvocationIr,
        output: &ParsedOutput,
    ) -> Result<ConstraintReport, VerificationError> {
        let checks = ir
            .prompt()
            .handoff()
            .task()
            .data()
            .verification_contract
            .checks
            .iter()
            .filter(|check| **check != SemanticVerificationCheck::OutputSchemaValid)
            .map(|check| (*check, CheckOutcome::Passed))
            .collect();
        Ok(ConstraintReport {
            binding: output.binding(),
            policy: CheckOutcome::Passed,
            process: CheckOutcome::Passed,
            task_completeness: CheckOutcome::Passed,
            semantic_output: CheckOutcome::Passed,
            checks,
        })
    }
}

fn goal_and_prepared() -> (
    SupportedConnectorGoal,
    gateway_application::sessions::PreparedConnectorTurn,
) {
    goal_and_prepared_for_steps(vec![PlanStepId::new("connector-step").unwrap()])
}

pub(crate) fn goal_and_prepared_for_steps(
    steps: Vec<PlanStepId>,
) -> (
    SupportedConnectorGoal,
    gateway_application::sessions::PreparedConnectorTurn,
) {
    goal_and_prepared_for_steps_with_proposal_count(steps, 2)
}

#[allow(dead_code)] // Shared with the separate session_runtime integration-test crate.
pub(crate) fn one_proposal_goal_and_prepared() -> (
    SupportedConnectorGoal,
    gateway_application::sessions::PreparedConnectorTurn,
) {
    goal_and_prepared_for_steps_with_proposal_count(
        vec![PlanStepId::new("connector-step").unwrap()],
        1,
    )
}

fn goal_and_prepared_for_steps_with_proposal_count(
    steps: Vec<PlanStepId>,
    proposal_count: usize,
) -> (
    SupportedConnectorGoal,
    gateway_application::sessions::PreparedConnectorTurn,
) {
    let h = model_support::handoff();
    let p = model_support::prepared(&h, 100_000, true);
    let tools = p
        .invocation()
        .data()
        .permitted_capabilities
        .iter()
        .map(|(id, binding)| {
            (
                id.clone(),
                ToolDefinition {
                    binding: binding.clone(),
                    description: NonEmptyText::new("Modify registered state").unwrap(),
                    input: OutputType::Object {
                        fields: BTreeMap::from([(
                            "value".to_owned(),
                            OutputField {
                                required: true,
                                value_type: OutputType::String {},
                            },
                        )]),
                    },
                    connector_inventory: None,
                },
            )
        })
        .collect();
    let ir = admit_invocation_ir(SchemaVersion::V1, h, p, tools, &Admission).unwrap();
    let capability = ir.prompt().tools().values().next().unwrap().binding.clone();
    let proposals = (0..proposal_count)
        .map(|ordinal| {
            json!({
                "capability":capability,
                "arguments":{"value":format!("proposal-{ordinal}")}
            })
        })
        .collect::<Vec<_>>();
    let wire = json!({
        "schema_version":"1.0",
        "invocation":ir.identity(),
        "result":"entity",
        "references":[],
        "tools":proposals
    });
    let parsed = parse_invocation_output(&ir, &serde_json::to_vec(&wire).unwrap()).unwrap();
    let verified = verify_output(&ir, parsed, &OutputHost, vec![]).unwrap();
    let scope = ir.prepared().invocation().data().task.scope.clone();
    let goal = SupportedConnectorGoal::from_host_registration(
        Intent::new(
            IntentId::new("connector-goal").unwrap(),
            DesiredState::new(
                DesiredStateId::new("desired").unwrap(),
                vec![
                    DesiredCondition::new(
                        ConditionId::new("ready").unwrap(),
                        SubjectPath::new(["connector", "ready"]).unwrap(),
                        ComparisonOperator::Equals,
                        Some(TypedValue::Boolean(true)),
                    )
                    .unwrap(),
                ],
                ConditionExpression::condition(ConditionId::new("ready").unwrap()),
                vec![],
                vec![],
            )
            .unwrap(),
        ),
        scope.clone(),
        RecordRef::new(
            RecordId::new("connector-plan").unwrap(),
            RecordId::new("1").unwrap(),
            format!("sha256:{}", "a".repeat(64)),
        )
        .unwrap(),
        steps,
        vec![
            ConditionObservationMapping::new(
                ConditionId::new("ready").unwrap(),
                SourceId::new("connector-state").unwrap(),
                CapabilityId::new("connector.read-state").unwrap(),
                ConnectorOperationClass::Read,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let before =
        ScopedConnectorSnapshotPayload::new(scope, br#"{"state":"before"}"#.to_vec()).unwrap();
    let prepared =
        gateway_application::sessions::PreparedConnectorTurn::new(ir, verified, before).unwrap();
    (goal, prepared)
}

fn request(
    scope: &ContextScopeId,
    step: &PlanStepId,
    task: &TaskId,
    ordinal: usize,
    class: ConnectorOperationClass,
    execution: &str,
    correlation: &str,
) -> ConnectorRequest {
    ConnectorRequest {
        target: ConnectorTarget {
            connector: NonEmptyText::new("registered-connector").unwrap(),
            scope: scope.clone(),
        },
        capability: CapabilityId::new(if class == ConnectorOperationClass::Mutate {
            format!("connector.mutate-{ordinal}")
        } else {
            "connector.read-state".to_owned()
        })
        .unwrap(),
        class,
        inventory_revision: ContentDigest::new("b".repeat(64)).unwrap(),
        arguments: BTreeMap::new(),
        task: task.clone(),
        step: step.clone(),
        execution: NonEmptyText::new(execution).unwrap(),
        correlation: NonEmptyText::new(correlation).unwrap(),
    }
}

fn dispatch_observation(request: &ConnectorRequest) -> ConnectorObservation {
    ConnectorObservation {
        metadata: ConnectorResultMetadata {
            task: request.task.clone(),
            step: request.step.clone(),
            execution: request.execution.clone(),
            object_reference: NonEmptyText::new("connector://state").unwrap(),
            source_kind: SourceKind::Tool,
            source_digest: None,
            observed_at: None,
            authenticated_source: NonEmptyText::new("connector-host").unwrap(),
            quality: QualityMetadata::new(
                TrustClass::ObservedEvidence,
                SensitivityClass::Public,
                Confidence::Unknown,
                FreshnessStatus::Fresh,
                Uncertainty::None,
            ),
            effect: ConnectorEffect::Acknowledged,
            partial: false,
            verifies: None,
            non_authoritative_raw: None,
        },
        target: request.target.clone(),
        inventory_revision: request.inventory_revision.clone(),
        correlation: request.correlation.clone(),
        source: SourceId::new("connector-dispatch-source").unwrap(),
        source_revision: None,
        content_digest: ContentDigest::new("c".repeat(64)).unwrap(),
        acquired_at: UnixTimestamp::new(20),
        records: ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap(),
        basis: ConnectorObservationBasis::ProviderAcknowledgement,
    }
}

fn readback_observation(
    original: &ConnectorRequest,
    read_request: &ConnectorRequest,
) -> ConnectorObservation {
    let source = SourceId::new("connector-state").unwrap();
    let acquired_at = UnixTimestamp::new(30);
    let source_revision = SourceTimestamp::new("state-revision-2").unwrap();
    let provenance = Provenance::new(
        ProvenanceId::new("readback-provenance").unwrap(),
        SourceKind::Tool,
        source.clone(),
        "connector://state",
    )
    .unwrap()
    .with_producer("authenticated-connector-host")
    .unwrap()
    .with_acquired_at(SourceTimestamp::new("30").unwrap())
    .with_source_timestamp(source_revision.clone());
    let subject = SubjectPath::new(["connector", "ready"]).unwrap();
    let observed = Observation::new(
        ObservationId::new("readback-observation").unwrap(),
        subject.clone(),
        TypedValue::Boolean(true),
        provenance.id().clone(),
    )
    .unwrap();
    let fact = Fact::new(
        FactId::new("readback-fact").unwrap(),
        subject,
        TypedValue::Boolean(true),
        AssertionPolarity::Affirmed,
        vec![observed.id().clone()],
    )
    .unwrap();
    let evidence = Evidence::new(
        EvidenceId::new("readback-evidence").unwrap(),
        EvidenceKind::Report,
        "Independent state read",
        EvidenceContent::inline("The registered state was read after dispatch.").unwrap(),
        provenance.id().clone(),
        vec![EvidenceLink::new(
            fact.id().clone(),
            EvidenceRelation::Supports,
        )],
    )
    .unwrap();
    ConnectorObservation {
        metadata: ConnectorResultMetadata {
            task: read_request.task.clone(),
            step: read_request.step.clone(),
            execution: read_request.execution.clone(),
            object_reference: NonEmptyText::new("connector://state").unwrap(),
            source_kind: SourceKind::Tool,
            source_digest: Some(ContentDigest::new("d".repeat(64)).unwrap()),
            observed_at: Some(acquired_at),
            authenticated_source: NonEmptyText::new("authenticated-connector-host").unwrap(),
            quality: QualityMetadata::new(
                TrustClass::ObservedEvidence,
                SensitivityClass::Public,
                Confidence::Unknown,
                FreshnessStatus::Fresh,
                Uncertainty::None,
            ),
            effect: ConnectorEffect::NoMutation,
            partial: false,
            verifies: Some(original.clone()),
            non_authoritative_raw: None,
        },
        target: read_request.target.clone(),
        inventory_revision: read_request.inventory_revision.clone(),
        correlation: read_request.correlation.clone(),
        source,
        source_revision: Some(source_revision),
        content_digest: ContentDigest::new("e".repeat(64)).unwrap(),
        acquired_at,
        records: ObservationEvidenceSet::new(
            vec![provenance],
            vec![observed],
            vec![fact],
            vec![evidence],
        )
        .unwrap(),
        basis: ConnectorObservationBasis::IndependentPostEffectRead,
    }
}

fn connector_turn(
    goal: &SupportedConnectorGoal,
    prepared: &gateway_application::sessions::PreparedConnectorTurn,
) -> ConnectorSessionTurn {
    connector_turn_for_step(goal, prepared, &goal.steps()[0])
}

pub(crate) fn connector_turn_for_step(
    goal: &SupportedConnectorGoal,
    prepared: &gateway_application::sessions::PreparedConnectorTurn,
    step: &PlanStepId,
) -> ConnectorSessionTurn {
    let proposals = &prepared.verified_output().output().data().tools;
    let task = &prepared.invocation().prepared().invocation().data().task.id;
    let requests = (0..proposals.len())
        .map(|ordinal| {
            request(
                goal.scope(),
                step,
                task,
                ordinal,
                ConnectorOperationClass::Mutate,
                &format!("mutation-execution-{ordinal}"),
                &format!("mutation-correlation-{ordinal}"),
            )
        })
        .collect::<Vec<_>>();
    let results = requests
        .iter()
        .enumerate()
        .map(|(ordinal, request)| DispatchedProposal {
            ordinal,
            request: request.clone(),
            observation: dispatch_observation(request),
        })
        .collect::<Vec<_>>();
    let original = &requests[0];
    let read_request = request(
        goal.scope(),
        step,
        task,
        0,
        ConnectorOperationClass::Inspect,
        "readback-execution",
        "readback-correlation",
    );
    let observation = readback_observation(original, &read_request);
    let read_dispatch = ConnectorDispatch {
        request: read_request.clone(),
        policy: PolicyId::new("readback-policy").unwrap(),
        policy_revision: ContentDigest::new("f".repeat(64)).unwrap(),
        consent_receipt: None,
        budget_reservation: NonEmptyText::new("readback-budget").unwrap(),
        deadline: UnixTimestamp::new(100),
    };
    let result = ConnectorResult::new(&read_dispatch, observation.clone()).unwrap();
    let batch = result.batch().unwrap();
    let readback = CorrelatedConnectorReadback {
        proposal_ordinal: 0,
        read_request,
        observation,
        batch,
    };
    ConnectorSessionTurn {
        proposal_batch: ProposalBatch {
            invocation: prepared.invocation().identity().invocation_digest.clone(),
            all_calls_returned: true,
            results,
            failure: None,
        },
        after_snapshot: Some(
            ScopedConnectorSnapshotPayload::new(
                goal.scope().clone(),
                br#"{"state":"after"}"#.to_vec(),
            )
            .unwrap(),
        ),
        independent_readbacks: vec![readback],
        goal_verification_readbacks: vec![],
    }
}

#[allow(dead_code)]
pub(crate) fn add_goal_verification_readback(
    goal: &SupportedConnectorGoal,
    step: &PlanStepId,
    mut turn: ConnectorSessionTurn,
) -> Result<ConnectorSessionTurn, SessionError> {
    let mutation = turn
        .proposal_batch
        .results
        .first()
        .ok_or(SessionError::InvalidInteraction)?
        .request
        .clone();
    let read_request = request(
        goal.scope(),
        step,
        &mutation.task,
        0,
        ConnectorOperationClass::Read,
        "goal-read-execution",
        "goal-read-correlation",
    );
    let mut observation = readback_observation(&mutation, &read_request);
    observation.basis = ConnectorObservationBasis::SourceRead;
    observation.metadata.verifies = None;
    let dispatch = ConnectorDispatch {
        request: read_request.clone(),
        policy: PolicyId::new("goal-read-policy").unwrap(),
        policy_revision: ContentDigest::new("f".repeat(64)).unwrap(),
        consent_receipt: None,
        budget_reservation: NonEmptyText::new("goal-read-budget").unwrap(),
        deadline: UnixTimestamp::new(100),
    };
    let batch = ConnectorResult::new(&dispatch, observation.clone())
        .map_err(|_| SessionError::InvalidInteraction)?
        .batch()
        .map_err(|_| SessionError::InvalidInteraction)?;
    let condition = goal.intent().desired_state().conditions()[0].id().clone();
    turn.goal_verification_readbacks = vec![GoalVerificationReadback {
        condition,
        read_request,
        observation,
        batch,
    }];
    Ok(turn)
}

fn fixture() -> (
    SupportedConnectorGoal,
    gateway_application::sessions::PreparedConnectorTurn,
    ConnectorSessionTurn,
) {
    let (goal, prepared) = goal_and_prepared();
    let turn = connector_turn(&goal, &prepared);
    (goal, prepared, turn)
}

#[test]
fn exact_independent_readback_is_accepted() {
    let (goal, prepared, turn) = fixture();
    assert_eq!(turn.validate(&goal, &goal.steps()[0], &prepared), Ok(()));
}

#[test]
fn independent_readback_requires_exact_mutation_association_and_distinct_identity() {
    for mutation in 0..3 {
        let (goal, prepared, mut turn) = fixture();
        let original = turn.proposal_batch.results[0].request.clone();
        let readback = &mut turn.independent_readbacks[0];
        match mutation {
            0 => {
                readback.read_request.correlation = original.correlation.clone();
                readback.observation.correlation = original.correlation;
            }
            1 => {
                readback.read_request.execution = original.execution.clone();
                readback.observation.metadata.execution = original.execution;
            }
            _ => {
                readback.observation.metadata.verifies = Some(request(
                    goal.scope(),
                    &goal.steps()[0],
                    &original.task,
                    9,
                    ConnectorOperationClass::Mutate,
                    "other-execution",
                    "other-correlation",
                ));
            }
        }
        assert_eq!(
            turn.validate(&goal, &goal.steps()[0], &prepared),
            Err(SessionError::InvalidInteraction),
            "mutation {mutation} must be rejected"
        );
    }
}

#[test]
fn partial_proposal_results_must_be_a_complete_prefix() {
    let (goal, prepared, mut turn) = fixture();
    turn.proposal_batch.all_calls_returned = false;
    turn.proposal_batch.results.remove(0);
    turn.proposal_batch.failure = Some((1, ProposalError::PolicyDenied));
    turn.independent_readbacks.clear();
    assert_eq!(
        turn.validate(&goal, &goal.steps()[0], &prepared),
        Err(SessionError::InvalidInteraction)
    );
}

#[test]
fn before_and_after_snapshots_must_match_registered_scope() {
    let (goal, prepared, mut turn) = fixture();
    turn.after_snapshot = Some(
        ScopedConnectorSnapshotPayload::new(
            ContextScopeId::new("foreign-project").unwrap(),
            br#"{"state":"after"}"#.to_vec(),
        )
        .unwrap(),
    );
    assert_eq!(
        turn.validate(&goal, &goal.steps()[0], &prepared),
        Err(SessionError::InvalidInteraction)
    );

    let (goal, mut prepared, turn) = fixture();
    prepared = gateway_application::sessions::PreparedConnectorTurn::new(
        prepared.invocation().clone(),
        prepared.verified_output().clone(),
        ScopedConnectorSnapshotPayload::new(
            ContextScopeId::new("foreign-project").unwrap(),
            br#"{"state":"before"}"#.to_vec(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        turn.validate(&goal, &goal.steps()[0], &prepared),
        Err(SessionError::InvalidInteraction),
        "before snapshot from a different scope must be rejected"
    );
}

#[test]
fn uncertain_proposal_effect_is_not_downgraded_by_successful_calls() {
    let (goal, prepared, mut turn) = fixture();
    turn.proposal_batch.results[0].observation.metadata.effect = ConnectorEffect::Unknown;
    assert_eq!(
        gateway_application::sessions::aggregate_proposal_effect(&turn.proposal_batch),
        ConnectorEffect::Unknown
    );
    assert!(turn.proposal_batch.all_calls_returned);
    assert_eq!(
        turn.validate(&goal, &goal.steps()[0], &prepared),
        Ok(()),
        "the returned call stays typed as Unknown even when an after snapshot is acquired"
    );
}

#[test]
fn turn_validator_rejects_malformed_batch_shapes_and_effect_claims() {
    let invalid_cases: Vec<(&str, TurnMutation)> = vec![
        (
            "wrong invocation digest",
            Box::new(|turn| {
                turn.proposal_batch.invocation = ContentDigest::new("f".repeat(64)).unwrap();
            }),
        ),
        (
            "non-prefix ordinal",
            Box::new(|turn| turn.proposal_batch.results[0].ordinal = 1),
        ),
        (
            "failure ordinal does not follow returned prefix",
            Box::new(|turn| {
                turn.proposal_batch.all_calls_returned = false;
                turn.proposal_batch.failure = Some((0, ProposalError::PolicyDenied));
            }),
        ),
        (
            "partial batch without a failure",
            Box::new(|turn| turn.proposal_batch.all_calls_returned = false),
        ),
        (
            "complete batch with missing proposal result",
            Box::new(|turn| {
                turn.proposal_batch.results.pop();
                turn.independent_readbacks.clear();
            }),
        ),
        (
            "acknowledgement from a read request",
            Box::new(|turn| {
                turn.proposal_batch.results[0].request.class = ConnectorOperationClass::Read
            }),
        ),
        (
            "after snapshot attached to a pre-dispatch denial",
            Box::new(|turn| {
                turn.proposal_batch.all_calls_returned = false;
                turn.proposal_batch.results.clear();
                turn.proposal_batch.failure = Some((0, ProposalError::PolicyDenied));
                turn.independent_readbacks.clear();
            }),
        ),
        (
            "mutating acknowledgement without after snapshot",
            Box::new(|turn| turn.after_snapshot = None),
        ),
        (
            "no mutation without after snapshot",
            Box::new(|turn| {
                turn.proposal_batch.results[0].observation.metadata.effect =
                    ConnectorEffect::NoMutation;
                turn.after_snapshot = None;
            }),
        ),
        (
            "duplicate independent readback ordinal",
            Box::new(|turn| {
                turn.independent_readbacks
                    .push(turn.independent_readbacks[0].clone())
            }),
        ),
        (
            "readback ordinal has no dispatched proposal",
            Box::new(|turn| turn.independent_readbacks[0].proposal_ordinal = 9),
        ),
    ];

    for (name, mutate) in invalid_cases {
        let (goal, prepared, mut turn) = fixture();
        mutate(&mut turn);
        assert_eq!(
            turn.validate(&goal, &goal.steps()[0], &prepared),
            Err(SessionError::InvalidInteraction),
            "case: {name}"
        );
    }
}

#[test]
fn final_goal_reads_are_rejected_on_nonfinal_step() {
    let (goal, prepared) = goal_and_prepared_for_steps(vec![
        PlanStepId::new("connector-step-one").unwrap(),
        PlanStepId::new("connector-step-two").unwrap(),
    ]);
    let first = goal.steps()[0].clone();
    let mut turn = connector_turn_for_step(&goal, &prepared, &first);
    turn = add_goal_verification_readback(&goal, &first, turn).unwrap();
    assert_eq!(
        turn.validate(&goal, &first, &prepared),
        Err(SessionError::InvalidInteraction)
    );
}

#[test]
fn final_goal_reads_are_required_to_cover_conditions_only_after_all_steps() {
    let (goal, prepared) = goal_and_prepared_for_steps(vec![
        PlanStepId::new("connector-step-one").unwrap(),
        PlanStepId::new("connector-step-two").unwrap(),
    ]);
    let first = goal.steps()[0].clone();
    let mut first_turn = connector_turn_for_step(&goal, &prepared, &first);
    first_turn = add_goal_verification_readback(&goal, &first, first_turn).unwrap();
    assert_eq!(
        first_turn.validate(&goal, &first, &prepared),
        Err(SessionError::InvalidInteraction)
    );

    let last = goal.steps()[1].clone();
    let mut final_turn = connector_turn_for_step(&goal, &prepared, &last);
    final_turn = add_goal_verification_readback(&goal, &last, final_turn).unwrap();
    assert_eq!(final_turn.validate(&goal, &last, &prepared), Ok(()));

    final_turn.goal_verification_readbacks[0]
        .read_request
        .capability = CapabilityId::new("connector.wrong-read").unwrap();
    assert_eq!(
        final_turn.validate(&goal, &last, &prepared),
        Err(SessionError::InvalidInteraction)
    );
}

#[test]
fn goal_readback_validation_fails_closed_across_mapping_quality_and_record_drift() {
    let (goal, prepared) = goal_and_prepared_for_steps(vec![
        PlanStepId::new("connector-step-one").unwrap(),
        PlanStepId::new("connector-step-two").unwrap(),
    ]);
    let last = goal.steps()[1].clone();
    let invalid_cases: Vec<(&str, GoalReadbackMutation)> = vec![
        (
            "unknown condition",
            Box::new(|readback| readback.condition = ConditionId::new("other").unwrap()),
        ),
        (
            "unmapped source",
            Box::new(|readback| {
                readback.observation.source = SourceId::new("other-source").unwrap()
            }),
        ),
        (
            "unmapped capability",
            Box::new(|readback| {
                readback.read_request.capability = CapabilityId::new("other-capability").unwrap()
            }),
        ),
        (
            "mutating read capability",
            Box::new(|readback| readback.read_request.class = ConnectorOperationClass::Mutate),
        ),
        (
            "read bound to wrong step",
            Box::new(|readback| {
                readback.read_request.step = PlanStepId::new("wrong-step").unwrap()
            }),
        ),
        (
            "read bound to wrong task",
            Box::new(|readback| readback.read_request.task = TaskId::new("wrong-task").unwrap()),
        ),
        (
            "wrong observation basis",
            Box::new(|readback| {
                readback.observation.basis = ConnectorObservationBasis::IndependentPostEffectRead
            }),
        ),
        (
            "mutation link on goal read",
            Box::new(|readback| {
                readback.observation.metadata.verifies = Some(turn_request_for_test(readback));
            }),
        ),
        (
            "effect is not no-mutation",
            Box::new(|readback| readback.observation.metadata.effect = ConnectorEffect::Unknown),
        ),
        (
            "partial read",
            Box::new(|readback| readback.observation.metadata.partial = true),
        ),
        (
            "caller input trust",
            Box::new(|readback| {
                readback.observation.metadata.quality = QualityMetadata::new(
                    TrustClass::CallerInput,
                    SensitivityClass::Public,
                    Confidence::Unknown,
                    FreshnessStatus::Fresh,
                    Uncertainty::None,
                );
            }),
        ),
        (
            "stale read",
            Box::new(|readback| {
                readback.observation.metadata.quality = QualityMetadata::new(
                    TrustClass::ObservedEvidence,
                    SensitivityClass::Public,
                    Confidence::Unknown,
                    FreshnessStatus::Stale,
                    Uncertainty::None,
                );
            }),
        ),
        (
            "unresolved conflict",
            Box::new(|readback| {
                readback.observation.metadata.quality = readback
                    .observation
                    .metadata
                    .quality
                    .with_conflict(ConflictStatus::Unresolved);
            }),
        ),
        (
            "unknown uncertainty",
            Box::new(|readback| {
                readback.observation.metadata.quality = QualityMetadata::new(
                    TrustClass::ObservedEvidence,
                    SensitivityClass::Public,
                    Confidence::Unknown,
                    FreshnessStatus::Fresh,
                    Uncertainty::Unknown,
                );
            }),
        ),
        (
            "observation target scope mismatch",
            Box::new(|readback| {
                readback.observation.target.scope = ContextScopeId::new("foreign-project").unwrap();
            }),
        ),
        (
            "observation records differ from batch",
            Box::new(|readback| {
                readback.observation.records =
                    ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap();
            }),
        ),
        (
            "observation content digest differs from snapshot",
            Box::new(|readback| {
                readback.observation.content_digest = ContentDigest::new("9".repeat(64)).unwrap()
            }),
        ),
        (
            "missing observed subject and supporting fact",
            Box::new(|readback| {
                let records = ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap();
                readback.observation.records = records.clone();
                readback.batch = gateway_application::ScopedObservationBatch::new(
                    readback.batch.scope().clone(),
                    readback.batch.snapshot().clone(),
                    records,
                )
                .unwrap();
            }),
        ),
    ];

    for (name, mutate) in invalid_cases {
        let mut turn = connector_turn_for_step(&goal, &prepared, &last);
        turn = add_goal_verification_readback(&goal, &last, turn).unwrap();
        mutate(&mut turn.goal_verification_readbacks[0]);
        assert_eq!(
            turn.validate(&goal, &last, &prepared),
            Err(SessionError::InvalidInteraction),
            "case: {name}"
        );
    }
}

fn turn_request_for_test(readback: &GoalVerificationReadback) -> ConnectorRequest {
    readback.read_request.clone()
}
