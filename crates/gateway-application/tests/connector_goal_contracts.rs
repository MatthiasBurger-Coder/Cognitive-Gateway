use gateway_application::{connectors::ConnectorOperationClass, sessions::*};
use gateway_domain::{
    CapabilityId, ComparisonOperator, ConditionExpression, ConditionId, ContextScopeId,
    DesiredCondition, DesiredState, DesiredStateId, Intent, IntentId, PlanStepId, SourceId,
    SubjectPath, TypedValue,
};

fn reference(id: &str) -> RecordRef {
    RecordRef::new(
        RecordId::new(id).unwrap(),
        RecordId::new("1").unwrap(),
        format!("sha256:{}", "a".repeat(64)),
    )
    .unwrap()
}

fn intent(condition_ids: &[&str]) -> Intent {
    let conditions = condition_ids
        .iter()
        .map(|id| {
            DesiredCondition::new(
                ConditionId::new(*id).unwrap(),
                SubjectPath::new(["external", id.to_owned()]).unwrap(),
                ComparisonOperator::Equals,
                Some(TypedValue::Boolean(true)),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let expression = if conditions.len() == 1 {
        ConditionExpression::condition(conditions[0].id().clone())
    } else {
        ConditionExpression::all(
            conditions
                .iter()
                .map(|condition| ConditionExpression::condition(condition.id().clone()))
                .collect(),
        )
        .unwrap()
    };
    Intent::new(
        IntentId::new("goal").unwrap(),
        DesiredState::new(
            DesiredStateId::new("desired").unwrap(),
            conditions,
            expression,
            vec![],
            vec![],
        )
        .unwrap(),
    )
}

fn mapping(condition: &str, source: &str, capability: &str) -> ConditionObservationMapping {
    ConditionObservationMapping::new(
        ConditionId::new(condition).unwrap(),
        SourceId::new(source).unwrap(),
        CapabilityId::new(capability).unwrap(),
        ConnectorOperationClass::Read,
    )
    .unwrap()
}

fn goal(
    intent: Intent,
    steps: Vec<PlanStepId>,
    observations: Vec<ConditionObservationMapping>,
) -> Result<SupportedConnectorGoal, SessionError> {
    SupportedConnectorGoal::from_host_registration(
        intent,
        ContextScopeId::new("project-a").unwrap(),
        reference("plan"),
        steps,
        observations,
    )
}

#[test]
fn registered_goal_preserves_scope_plan_step_order_and_exact_observation_bindings() {
    let registered_intent = intent(&["first", "second"]);
    let result = goal(
        registered_intent.clone(),
        vec![
            PlanStepId::new("step-2").unwrap(),
            PlanStepId::new("step-1").unwrap(),
        ],
        vec![
            mapping("second", "source-b", "resource.read"),
            mapping("first", "source-a", "repository.inspect"),
        ],
    )
    .unwrap();

    assert_eq!(result.intent(), &registered_intent);
    assert_eq!(result.scope().as_str(), "project-a");
    assert_eq!(result.plan(), &reference("plan"));
    assert_eq!(
        result
            .steps()
            .iter()
            .map(PlanStepId::as_str)
            .collect::<Vec<_>>(),
        vec!["step-2", "step-1"]
    );
    assert_eq!(
        result
            .observations()
            .iter()
            .map(|mapping| mapping.condition().as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );
}

#[test]
fn steps_must_be_nonempty_unique_and_bounded() {
    let registered_intent = intent(&["ready"]);
    assert_eq!(
        goal(
            registered_intent.clone(),
            vec![],
            vec![mapping("ready", "s", "c")]
        )
        .unwrap_err(),
        SessionError::InvalidInput
    );
    assert_eq!(
        goal(
            registered_intent.clone(),
            vec![
                PlanStepId::new("same").unwrap(),
                PlanStepId::new("same").unwrap()
            ],
            vec![mapping("ready", "s", "c")],
        )
        .unwrap_err(),
        SessionError::InvalidInput
    );
    let too_many = (0..=MAX_CONNECTOR_GOAL_STEPS)
        .map(|index| PlanStepId::new(format!("step-{index}")).unwrap())
        .collect();
    assert_eq!(
        goal(
            registered_intent,
            too_many,
            vec![mapping("ready", "s", "c")]
        )
        .unwrap_err(),
        SessionError::LimitExceeded
    );
}

#[test]
fn each_intent_condition_requires_exactly_one_observation_mapping() {
    let registered_intent = intent(&["first", "second"]);
    assert_eq!(
        goal(
            registered_intent.clone(),
            vec![PlanStepId::new("step").unwrap()],
            vec![mapping("first", "s", "c")],
        )
        .unwrap_err(),
        SessionError::InvalidInput
    );
    assert_eq!(
        goal(
            registered_intent.clone(),
            vec![PlanStepId::new("step").unwrap()],
            vec![mapping("first", "s", "c"), mapping("extra", "s", "c")],
        )
        .unwrap_err(),
        SessionError::InvalidInput
    );
    assert_eq!(
        goal(
            registered_intent,
            vec![PlanStepId::new("step").unwrap()],
            vec![mapping("first", "s1", "c1"), mapping("first", "s2", "c2")],
        )
        .unwrap_err(),
        SessionError::InvalidInput
    );
}

#[test]
fn observation_mappings_reject_mutating_and_admin_capabilities() {
    for class in [
        ConnectorOperationClass::Mutate,
        ConnectorOperationClass::Admin,
    ] {
        assert_eq!(
            ConditionObservationMapping::new(
                ConditionId::new("ready").unwrap(),
                SourceId::new("source").unwrap(),
                CapabilityId::new("external.call").unwrap(),
                class,
            )
            .unwrap_err(),
            SessionError::UnsupportedCapability
        );
    }
    for class in [
        ConnectorOperationClass::Read,
        ConnectorOperationClass::Search,
        ConnectorOperationClass::Inspect,
    ] {
        let mapped = ConditionObservationMapping::new(
            ConditionId::new("ready").unwrap(),
            SourceId::new("source").unwrap(),
            CapabilityId::new("external.read").unwrap(),
            class,
        )
        .unwrap();
        assert_eq!(mapped.operation_class(), class);
    }
}

#[test]
fn session_goal_variant_accessors_preserve_artifact_and_connector_meanings() {
    let connector = SupportedSessionGoal::connector(
        goal(
            intent(&["ready"]),
            vec![PlanStepId::new("step").unwrap()],
            vec![mapping("ready", "source", "read")],
        )
        .unwrap(),
    );
    assert!(connector.as_artifact().is_none());
    assert_eq!(
        connector.as_connector().unwrap().scope().as_str(),
        "project-a"
    );
    assert_eq!(
        connector.intent(),
        connector.as_connector().unwrap().intent()
    );
    assert_eq!(connector.scope(), connector.as_connector().unwrap().scope());

    let projection = reference("projection");
    let artifact_intent = Intent::new(
        IntentId::new("artifact-goal").unwrap(),
        DesiredState::new(
            DesiredStateId::new("artifact-state").unwrap(),
            vec![
                DesiredCondition::new(
                    ConditionId::new("context-verified").unwrap(),
                    SubjectPath::new(["cg", "context", "projection", "verified"]).unwrap(),
                    ComparisonOperator::Equals,
                    Some(TypedValue::Boolean(true)),
                )
                .unwrap(),
            ],
            ConditionExpression::condition(ConditionId::new("context-verified").unwrap()),
            vec![],
            vec![],
        )
        .unwrap(),
    );
    let artifact = SupportedArtifactGoal::validate(
        artifact_intent,
        ArtifactGoalBasis {
            scope: ContextScopeId::new("project-a").unwrap(),
            plan: reference("artifact-plan"),
            step: PlanStepId::new("step").unwrap(),
            projection,
            sources: vec![],
        },
    )
    .unwrap();
    let wrapped_artifact = SupportedSessionGoal::artifact(artifact.clone());
    assert_eq!(wrapped_artifact.as_artifact(), Some(&artifact));
    assert!(wrapped_artifact.as_connector().is_none());
    assert_eq!(wrapped_artifact.scope().as_str(), "project-a");
}
