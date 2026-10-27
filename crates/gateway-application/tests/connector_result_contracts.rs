use gateway_application::{
    connector_results::{
        ConnectorResult, ConnectorResultMetadata, ResultDiagnostic, ResultError,
        validate_independent_post_effect_read,
    },
    connectors::{
        ConnectorDispatch, ConnectorEffect, ConnectorObservation, ConnectorObservationBasis,
        ConnectorOperationClass, ConnectorRequest, ConnectorTarget,
    },
};
use gateway_domain::*;
use std::collections::BTreeMap;

fn request(class: ConnectorOperationClass, execution: &str, correlation: &str) -> ConnectorRequest {
    ConnectorRequest {
        target: ConnectorTarget {
            connector: NonEmptyText::new("connector-test").unwrap(),
            scope: ContextScopeId::new("project-test").unwrap(),
        },
        capability: CapabilityId::new(match class {
            ConnectorOperationClass::Read => "connector.read",
            ConnectorOperationClass::Search => "connector.search",
            ConnectorOperationClass::Inspect => "connector.inspect",
            ConnectorOperationClass::Mutate => "connector.mutate",
            ConnectorOperationClass::Admin => "connector.admin",
        })
        .unwrap(),
        class,
        inventory_revision: ContentDigest::new("a".repeat(64)).unwrap(),
        arguments: BTreeMap::new(),
        task: TaskId::new("task-test").unwrap(),
        step: PlanStepId::new("step-test").unwrap(),
        execution: NonEmptyText::new(execution).unwrap(),
        correlation: NonEmptyText::new(correlation).unwrap(),
    }
}

fn dispatch(request: ConnectorRequest) -> ConnectorDispatch {
    ConnectorDispatch {
        request,
        policy: PolicyId::new("policy-test").unwrap(),
        policy_revision: ContentDigest::new("b".repeat(64)).unwrap(),
        consent_receipt: None,
        budget_reservation: NonEmptyText::new("budget-test").unwrap(),
        deadline: UnixTimestamp::new(100),
    }
}

fn observation(
    request: &ConnectorRequest,
    basis: ConnectorObservationBasis,
    effect: ConnectorEffect,
    verifies: Option<ConnectorRequest>,
) -> ConnectorObservation {
    let source = SourceId::new("connector-source").unwrap();
    let acquired = UnixTimestamp::new(20);
    let revision = SourceTimestamp::new("source-revision-1").unwrap();
    let source_reference = "connector://objects/item-1";
    let producer = "authenticated-connector-host";
    let provenance = Provenance::new(
        ProvenanceId::new("provenance-test").unwrap(),
        SourceKind::Tool,
        source.clone(),
        source_reference,
    )
    .unwrap()
    .with_producer(producer)
    .unwrap()
    .with_acquired_at(SourceTimestamp::new("20").unwrap())
    .with_source_timestamp(revision.clone());
    let subject = SubjectPath::new(["connector", "ready"]).unwrap();
    let observed = Observation::new(
        ObservationId::new("observation-test").unwrap(),
        subject.clone(),
        TypedValue::Boolean(true),
        provenance.id().clone(),
    )
    .unwrap();
    let fact = Fact::new(
        FactId::new("fact-test").unwrap(),
        subject,
        TypedValue::Boolean(true),
        AssertionPolarity::Affirmed,
        vec![observed.id().clone()],
    )
    .unwrap();
    let evidence = Evidence::new(
        EvidenceId::new("evidence-test").unwrap(),
        EvidenceKind::Report,
        "Observed connector state",
        EvidenceContent::inline("The connector reports ready.").unwrap(),
        provenance.id().clone(),
        vec![EvidenceLink::new(
            fact.id().clone(),
            EvidenceRelation::Supports,
        )],
    )
    .unwrap();
    ConnectorObservation {
        metadata: ConnectorResultMetadata {
            task: request.task.clone(),
            step: request.step.clone(),
            execution: request.execution.clone(),
            object_reference: NonEmptyText::new(source_reference).unwrap(),
            source_kind: SourceKind::Tool,
            source_digest: Some(ContentDigest::new("c".repeat(64)).unwrap()),
            observed_at: Some(acquired),
            authenticated_source: NonEmptyText::new(producer).unwrap(),
            quality: QualityMetadata::new(
                TrustClass::ObservedEvidence,
                SensitivityClass::Public,
                Confidence::Unknown,
                FreshnessStatus::Fresh,
                Uncertainty::None,
            ),
            effect,
            partial: false,
            verifies,
            non_authoritative_raw: None,
        },
        target: request.target.clone(),
        inventory_revision: request.inventory_revision.clone(),
        correlation: request.correlation.clone(),
        source,
        source_revision: Some(revision),
        content_digest: ContentDigest::new("d".repeat(64)).unwrap(),
        acquired_at: acquired,
        records: ObservationEvidenceSet::new(
            vec![provenance],
            vec![observed],
            vec![fact],
            vec![evidence],
        )
        .unwrap(),
        basis,
    }
}

fn read_result() -> (ConnectorDispatch, ConnectorObservation) {
    let request = request(
        ConnectorOperationClass::Read,
        "read-execution",
        "read-correlation",
    );
    let dispatch = dispatch(request.clone());
    let observation = observation(
        &request,
        ConnectorObservationBasis::SourceRead,
        ConnectorEffect::NoMutation,
        None,
    );
    (dispatch, observation)
}

#[test]
fn dispatch_binding_and_quality_rejections_cover_each_identity_axis() {
    let (dispatch, valid) = read_result();
    assert_eq!(ConnectorResult::validate(&dispatch, &valid), Ok(()));

    let mut mismatches = vec![];
    let mut value = valid.clone();
    value.target.connector = NonEmptyText::new("other-connector").unwrap();
    mismatches.push(value);
    let mut value = valid.clone();
    value.inventory_revision = ContentDigest::new("e".repeat(64)).unwrap();
    mismatches.push(value);
    let mut value = valid.clone();
    value.correlation = NonEmptyText::new("other-correlation").unwrap();
    mismatches.push(value);
    let mut value = valid.clone();
    value.metadata.task = TaskId::new("other-task").unwrap();
    mismatches.push(value);
    let mut value = valid.clone();
    value.metadata.step = PlanStepId::new("other-step").unwrap();
    mismatches.push(value);
    let mut value = valid.clone();
    value.metadata.execution = NonEmptyText::new("other-execution").unwrap();
    mismatches.push(value);
    let mut value = valid.clone();
    value.acquired_at = UnixTimestamp::new(101);
    mismatches.push(value);
    let mut value = valid.clone();
    value.metadata.observed_at = Some(UnixTimestamp::new(21));
    mismatches.push(value);
    for value in mismatches {
        assert_eq!(
            ConnectorResult::validate(&dispatch, &value),
            Err(ResultError::Binding)
        );
    }

    let mut value = valid.clone();
    value.metadata.source_kind = SourceKind::Model;
    assert_eq!(
        ConnectorResult::validate(&dispatch, &value),
        Err(ResultError::Quality)
    );
    let mut value = valid.clone();
    value.metadata.quality = QualityMetadata::new(
        TrustClass::CallerInput,
        SensitivityClass::Public,
        Confidence::Unknown,
        FreshnessStatus::Fresh,
        Uncertainty::None,
    );
    assert_eq!(
        ConnectorResult::validate(&dispatch, &value),
        Err(ResultError::Quality)
    );
    let mut value = valid;
    value.metadata.partial = true;
    assert_eq!(
        ConnectorResult::validate(&dispatch, &value),
        Err(ResultError::Quality)
    );
}

#[test]
fn all_observation_basis_rules_and_independent_read_linkage_are_enforced() {
    let mutation = request(
        ConnectorOperationClass::Mutate,
        "mutation-run",
        "mutation-call",
    );
    let read = request(ConnectorOperationClass::Inspect, "read-run", "read-call");
    let valid_read = observation(
        &read,
        ConnectorObservationBasis::IndependentPostEffectRead,
        ConnectorEffect::NoMutation,
        Some(mutation.clone()),
    );
    assert!(ConnectorResult::validate(&dispatch(read.clone()), &valid_read).is_ok());
    assert!(validate_independent_post_effect_read(&mutation, &read, &valid_read).is_ok());

    let mut invalid = valid_read.clone();
    invalid.metadata.verifies = None;
    assert_eq!(
        ConnectorResult::validate(&dispatch(read.clone()), &invalid),
        Err(ResultError::Basis)
    );
    let mut invalid = valid_read.clone();
    invalid.metadata.verifies.as_mut().unwrap().target.connector =
        NonEmptyText::new("foreign").unwrap();
    assert_eq!(
        ConnectorResult::validate(&dispatch(read.clone()), &invalid),
        Err(ResultError::Basis)
    );
    let mut invalid = valid_read.clone();
    invalid.metadata.verifies.as_mut().unwrap().task = TaskId::new("foreign-task").unwrap();
    assert_eq!(
        ConnectorResult::validate(&dispatch(read.clone()), &invalid),
        Err(ResultError::Basis)
    );
    let mut invalid = valid_read.clone();
    invalid.metadata.verifies.as_mut().unwrap().correlation = read.correlation.clone();
    assert_eq!(
        ConnectorResult::validate(&dispatch(read.clone()), &invalid),
        Err(ResultError::Basis)
    );
    let mut invalid = valid_read.clone();
    invalid.metadata.verifies.as_mut().unwrap().execution = read.execution.clone();
    assert_eq!(
        ConnectorResult::validate(&dispatch(read.clone()), &invalid),
        Err(ResultError::Basis)
    );
    let mut invalid = valid_read.clone();
    invalid.metadata.verifies.as_mut().unwrap().class = ConnectorOperationClass::Read;
    assert_eq!(
        ConnectorResult::validate(&dispatch(read.clone()), &invalid),
        Err(ResultError::Basis)
    );

    let mut invalid = valid_read.clone();
    invalid.metadata.effect = ConnectorEffect::Acknowledged;
    assert_eq!(
        ConnectorResult::validate(&dispatch(read.clone()), &invalid),
        Err(ResultError::Basis)
    );
    let invalid_read = request(ConnectorOperationClass::Mutate, "read-run", "read-call");
    let invalid = observation(
        &invalid_read,
        ConnectorObservationBasis::IndependentPostEffectRead,
        ConnectorEffect::NoMutation,
        Some(mutation.clone()),
    );
    assert_eq!(
        ConnectorResult::validate(&dispatch(invalid_read), &invalid),
        Err(ResultError::Basis)
    );

    let mut invalid_mutation = mutation.clone();
    invalid_mutation.step = PlanStepId::new("foreign-step").unwrap();
    assert_eq!(
        validate_independent_post_effect_read(&invalid_mutation, &read, &valid_read),
        Err(ResultError::Basis)
    );
    let same_execution = request(ConnectorOperationClass::Mutate, "read-run", "mutation-call");
    let mut same_execution_observation = valid_read.clone();
    same_execution_observation.metadata.verifies = Some(same_execution.clone());
    assert_eq!(
        validate_independent_post_effect_read(&same_execution, &read, &same_execution_observation,),
        Err(ResultError::Basis)
    );
    let same_correlation = request(ConnectorOperationClass::Mutate, "read-run", "read-call");
    let mut same_correlation_observation = valid_read.clone();
    same_correlation_observation.metadata.verifies = Some(same_correlation.clone());
    assert_eq!(
        validate_independent_post_effect_read(
            &same_correlation,
            &read,
            &same_correlation_observation,
        ),
        Err(ResultError::Basis)
    );
    let mut foreign_original = mutation.clone();
    foreign_original.target.connector = NonEmptyText::new("foreign-target").unwrap();
    let mut foreign_original_observation = valid_read.clone();
    foreign_original_observation.metadata.verifies = Some(foreign_original.clone());
    assert_eq!(
        validate_independent_post_effect_read(
            &foreign_original,
            &read,
            &foreign_original_observation,
        ),
        Err(ResultError::Basis)
    );

    let mutate_ack = request(ConnectorOperationClass::Mutate, "ack-run", "ack-call");
    let ack = observation(
        &mutate_ack,
        ConnectorObservationBasis::ProviderAcknowledgement,
        ConnectorEffect::Acknowledged,
        None,
    );
    let result = ConnectorResult::new(&dispatch(mutate_ack), ack).unwrap();
    assert_eq!(
        result.diagnostics(),
        vec![ResultDiagnostic::AcknowledgementOnly]
    );
    assert_eq!(result.batch(), Err(ResultError::Basis));
}

#[test]
fn diagnostics_batch_fragments_and_lineage_failures_are_typed() {
    let (base_dispatch, _) = read_result();
    let stale_conflicted_partial_unknown = QualityMetadata::new(
        TrustClass::ObservedEvidence,
        SensitivityClass::Public,
        Confidence::Unknown,
        FreshnessStatus::Stale,
        Uncertainty::Unknown,
    )
    .with_conflict(ConflictStatus::Unresolved);
    let mut ack_request = base_dispatch.request.clone();
    ack_request.class = ConnectorOperationClass::Mutate;
    ack_request.capability = CapabilityId::new("connector.mutate").unwrap();
    ack_request.execution = NonEmptyText::new("ack-execution").unwrap();
    ack_request.correlation = NonEmptyText::new("ack-correlation").unwrap();
    let mut ack = observation(
        &ack_request,
        ConnectorObservationBasis::ProviderAcknowledgement,
        ConnectorEffect::Unknown,
        None,
    );
    ack.metadata.quality = stale_conflicted_partial_unknown;
    ack.metadata.partial = true;
    let ack_dispatch = dispatch(ack_request);
    let result = ConnectorResult::new(&ack_dispatch, ack).unwrap();
    assert_eq!(
        result.diagnostics(),
        vec![
            ResultDiagnostic::AcknowledgementOnly,
            ResultDiagnostic::Stale,
            ResultDiagnostic::Conflict,
            ResultDiagnostic::Partial,
            ResultDiagnostic::UncertainEffect,
        ]
    );
    assert_eq!(result.batch(), Err(ResultError::Basis));

    let (valid_dispatch, valid) = read_result();
    let valid_result = ConnectorResult::new(&valid_dispatch, valid.clone()).unwrap();
    let batch = valid_result.batch().unwrap();
    assert_eq!(batch.scope(), &valid.target.scope);
    assert_eq!(valid_result.fragments().unwrap().len(), 1);
    assert_eq!(valid_result.observation(), &valid);
    assert_eq!(valid_result.into_observation(), valid);

    let mut empty_observations = valid.clone();
    empty_observations.records =
        ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap();
    assert_eq!(
        ConnectorResult::validate(&valid_dispatch, &empty_observations),
        Err(ResultError::Lineage)
    );
    let mut empty_evidence = valid.clone();
    empty_evidence.records = ObservationEvidenceSet::new(
        valid.records.provenances().to_vec(),
        valid.records.observations().to_vec(),
        valid.records.facts().to_vec(),
        vec![],
    )
    .unwrap();
    assert_eq!(
        ConnectorResult::validate(&valid_dispatch, &empty_evidence),
        Err(ResultError::Lineage)
    );

    let source = SourceId::new("connector-source").unwrap();
    let p1 = Provenance::new(
        ProvenanceId::new("cycle-a").unwrap(),
        SourceKind::Tool,
        source.clone(),
        "connector://objects/item-1",
    )
    .unwrap()
    .with_parent_provenance(vec![ProvenanceId::new("cycle-b").unwrap()])
    .unwrap();
    let p2 = Provenance::new(
        ProvenanceId::new("cycle-b").unwrap(),
        SourceKind::Tool,
        source,
        "connector://objects/item-1",
    )
    .unwrap()
    .with_parent_provenance(vec![ProvenanceId::new("cycle-a").unwrap()])
    .unwrap();
    let mut cyclic = valid;
    cyclic.records = ObservationEvidenceSet::new(
        vec![cyclic.records.provenances()[0].clone(), p1, p2],
        cyclic.records.observations().to_vec(),
        cyclic.records.facts().to_vec(),
        cyclic.records.evidence().to_vec(),
    )
    .unwrap();
    assert_eq!(
        ConnectorResult::validate(&valid_dispatch, &cyclic),
        Err(ResultError::Lineage)
    );
}
