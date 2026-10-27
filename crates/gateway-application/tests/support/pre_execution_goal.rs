//! Trusted-read adapter for application/storage component tests. Uses native
//! CG06 and real ClosedLoop verification; creates no InvocationIR or After.
use gateway_application::{
    closed_loop::{ClosedLoop, LoopRules},
    connector_results::ConnectorResultMetadata,
    connectors::*,
    external_context::{ScopedObservationBatch, SourceSnapshot},
    sessions::*,
};
use gateway_domain::*;
use serde_json::json;
use std::collections::BTreeMap;

pub fn definitions(scope: &ContextScopeId) -> Vec<PreExecutionGoalPin> {
    ["cg.connector-runtime","cg.connector-manifest","cg.connector-mapping","cg.connector-state-universe","cg.connector-normalization"].into_iter().enumerate().map(|(i,contract)|{
        let body=json!({"fixture":"native-observed-goal","role":contract});
        serde_json::from_value(json!({"id":format!("goal-definition-{i}"),"scope":scope,"contract":contract,"contract_version":"1.0","revision":"1","digest":context_content_digest(&serde_json::to_vec(&body).unwrap())})).unwrap()
    }).collect()
}
pub fn goal(scope: ContextScopeId) -> SupportedConnectorGoal {
    let conditions = ["ready", "consistent"]
        .into_iter()
        .map(|name| {
            DesiredCondition::new(
                ConditionId::new(name).unwrap(),
                SubjectPath::new(["state", name]).unwrap(),
                ComparisonOperator::Equals,
                Some(TypedValue::Boolean(true)),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let desired = DesiredState::new(
        DesiredStateId::new("observed-complete-goal").unwrap(),
        conditions.clone(),
        ConditionExpression::all(
            conditions
                .iter()
                .map(|c| ConditionExpression::condition(c.id().clone()))
                .collect(),
        )
        .unwrap(),
        vec![],
        vec![],
    )
    .unwrap();
    SupportedConnectorGoal::from_host_registration(
        Intent::new(IntentId::new("already-observed-goal").unwrap(), desired),
        scope,
        content_reference("observed-goal-plan", b"observed-goal-plan").unwrap(),
        vec![
            PlanStepId::new("inspect-first").unwrap(),
            PlanStepId::new("unnecessary-mutation").unwrap(),
        ],
        conditions
            .iter()
            .map(|c| {
                ConditionObservationMapping::new(
                    c.id().clone(),
                    SourceId::new("registered-state").unwrap(),
                    CapabilityId::new("connector.read-state").unwrap(),
                    ConnectorOperationClass::Read,
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap()
}
pub fn data(
    goal: &SupportedConnectorGoal,
    binding: PreExecutionGoalBinding,
    satisfied: bool,
    now_ms: u64,
) -> PreExecutionGoalData {
    let mapping = &goal.observations()[0];
    assert!(
        goal.observations()
            .iter()
            .all(|m| m.source() == mapping.source()
                && m.capability() == mapping.capability()
                && m.operation_class() == mapping.operation_class()),
        "fixture qualifies one atomic complete source export"
    );
    let provenance = Provenance::new(
        ProvenanceId::new("goal-provenance").unwrap(),
        SourceKind::Tool,
        mapping.source().clone(),
        "qualified-state-object",
    )
    .unwrap()
    .with_producer("trusted-reader")
    .unwrap()
    .with_acquired_at(SourceTimestamp::new((now_ms / 1000).to_string()).unwrap())
    .with_source_timestamp(SourceTimestamp::new("state-revision-1").unwrap());
    let mut observations = Vec::new();
    let mut facts = Vec::new();
    let mut evidences = Vec::new();
    for (i, c) in goal
        .intent()
        .desired_state()
        .conditions()
        .iter()
        .enumerate()
    {
        let value = if satisfied {
            c.expected().cloned().unwrap_or(TypedValue::Boolean(true))
        } else {
            TypedValue::Boolean(false)
        };
        let o = Observation::new(
            ObservationId::new(format!("goal-observation-{i}")).unwrap(),
            c.subject().clone(),
            value.clone(),
            provenance.id().clone(),
        )
        .unwrap();
        let f = Fact::new(
            FactId::new(format!("goal-fact-{i}")).unwrap(),
            c.subject().clone(),
            value,
            AssertionPolarity::Affirmed,
            vec![o.id().clone()],
        )
        .unwrap();
        evidences.push(
            Evidence::new(
                EvidenceId::new(format!("goal-evidence-{i}")).unwrap(),
                EvidenceKind::Report,
                "Qualified source export",
                EvidenceContent::inline("Trusted adapter atomic source export").unwrap(),
                provenance.id().clone(),
                vec![EvidenceLink::new(
                    f.id().clone(),
                    EvidenceRelation::Supports,
                )],
            )
            .unwrap(),
        );
        observations.push(o);
        facts.push(f);
    }
    let records =
        ObservationEvidenceSet::new(vec![provenance], observations, facts, evidences).unwrap();
    let digest = context_content_digest(records.to_json().unwrap().as_bytes());
    let quality = QualityMetadata::new(
        TrustClass::ObservedEvidence,
        SensitivityClass::Public,
        Confidence::NotApplicable,
        FreshnessStatus::Fresh,
        Uncertainty::None,
    );
    let request = ConnectorRequest {
        target: ConnectorTarget {
            connector: NonEmptyText::new("qualified-state").unwrap(),
            scope: goal.scope().clone(),
        },
        capability: mapping.capability().clone(),
        class: mapping.operation_class(),
        inventory_revision: context_content_digest(b"actual-inventory"),
        arguments: BTreeMap::new(),
        task: binding.task.clone(),
        step: binding.acquisition.step.clone(),
        execution: binding.execution.clone(),
        correlation: NonEmptyText::new(format!(
            "{}-atomic-read",
            binding.acquisition.dispatch.as_str()
        ))
        .unwrap(),
    };
    let observation = ConnectorObservation {
        metadata: ConnectorResultMetadata {
            task: request.task.clone(),
            step: request.step.clone(),
            execution: request.execution.clone(),
            object_reference: NonEmptyText::new("qualified-state-object").unwrap(),
            source_kind: SourceKind::Tool,
            source_digest: Some(digest.clone()),
            observed_at: Some(UnixTimestamp::new((now_ms / 1000) as i64)),
            authenticated_source: NonEmptyText::new("trusted-reader").unwrap(),
            quality,
            effect: ConnectorEffect::NoMutation,
            partial: false,
            verifies: None,
            non_authoritative_raw: None,
        },
        target: request.target.clone(),
        inventory_revision: request.inventory_revision.clone(),
        correlation: request.correlation.clone(),
        source: mapping.source().clone(),
        source_revision: Some(SourceTimestamp::new("state-revision-1").unwrap()),
        content_digest: digest.clone(),
        acquired_at: UnixTimestamp::new((now_ms / 1000) as i64),
        records: records.clone(),
        basis: ConnectorObservationBasis::SourceRead,
    };
    let mut batch = ScopedObservationBatch::new(
        goal.scope().clone(),
        SourceSnapshot::new(
            mapping.source().clone(),
            SourceKind::Tool,
            observation.source_revision.clone(),
            Some(digest),
        )
        .unwrap(),
        records,
    )
    .unwrap();
    for c in goal.intent().desired_state().conditions() {
        batch = batch.with_quality_metadata(c.subject().clone(), vec![quality]);
    }
    let inputs = goal
        .intent()
        .desired_state()
        .conditions()
        .iter()
        .map(|c| PreExecutionGoalReadInput {
            condition: c.id(),
            read_request: &request,
            observation: &observation,
            batch: &batch,
            max_age_seconds: 60,
        })
        .collect::<Vec<_>>();
    PreExecutionGoalData::new(goal, binding, definitions(goal.scope()), &inputs, now_ms).unwrap()
}
pub fn candidate(
    data: PreExecutionGoalData,
    goal: &SupportedConnectorGoal,
    rules: LoopRules,
) -> Option<PreExecutionGoalCandidate> {
    let receipt = ClosedLoop::verify_retained(
        data.execution().unwrap(),
        goal.scope().clone(),
        goal.intent().clone(),
        data.readset(),
        rules,
    )
    .unwrap()?;
    Some(PreExecutionGoalCandidate::new(data, &receipt).unwrap())
}
