use std::str::FromStr;

use gateway_application::ports::inbound::{
    DeclarativeIntentInputPort, ObservationEvidenceInputPort, ScopeLifecyclePort,
};
use gateway_application::{
    DeclarativeSituationApplication, InMemoryContextStore, ProcessSnapshotInput,
    ScopedObservationBatch, SourceSnapshot,
};
use gateway_domain::{
    AcceptanceCriterion, AcceptanceCriterionId, Assessment, AssessmentConclusion, AssessmentId,
    AssessmentKind, AssessmentOrigin, AssessmentRuleContract, AssessmentRuleId,
    AssessmentRuleVersion, AssessmentStatus, BasisReferences, ComparisonOperator,
    ConditionExpression, ConditionId, Confidence, ContentDigest, ContextScopeId, DecimalValue,
    DeclarativeConstraint, DeclarativeContext, DeclarativeContextId, DesiredCondition,
    DesiredState, Evidence, EvidenceContent, EvidenceId, EvidenceKind, EvidenceLink,
    EvidenceRelation, Fact, FactId, FreshnessStatus, Intent, IntentId, NormalizationInput,
    Observation, ObservationEvidenceSet, ObservationId, ObservedStateId, OriginalInput, Provenance,
    ProvenanceId, QualitativeLikelihood, QualityMetadata, ReasonCode, Risk, RiskCategory, RiskId,
    RiskLikelihood, RiskOrigin, RiskSeverity, RiskStatus, SituationAssemblyInput, SituationId,
    SituationReference, SourceId, SourceKind, SourceTimestamp, SubjectPath, TrustClass, TypedValue,
    Uncertainty,
};
use gateway_process::{
    BlockerDefinition, BlockerId, EventTypeDefinition, EventTypeId, EvidenceRequirement,
    GateDefinition, GateId, GateStatus, GuardExpression, ProcessDefinitionBuilder, ProcessInstance,
    ProcessInstanceId, ProcessInstanceStatus, StateDefinition, StateId, TransitionDefinition,
    TransitionId, TransitionProjection,
};

fn intent() -> Intent {
    let architecture = DesiredCondition::new(
        ConditionId::new("architecture-observed").unwrap(),
        SubjectPath::from_str("architecture.dependency").unwrap(),
        ComparisonOperator::Present,
        None,
    )
    .unwrap();
    let coverage = DesiredCondition::new(
        ConditionId::new("coverage-target").unwrap(),
        SubjectPath::from_str("coverage.percent").unwrap(),
        ComparisonOperator::GreaterOrEqual,
        Some(TypedValue::Decimal(DecimalValue::new(9500, 2).unwrap())),
    )
    .unwrap();
    let expression = ConditionExpression::all(vec![
        ConditionExpression::condition(architecture.id().clone()),
        ConditionExpression::condition(coverage.id().clone()),
    ])
    .unwrap();
    let desired = DesiredState::new(
        gateway_domain::DesiredStateId::new("desired-quality").unwrap(),
        vec![architecture, coverage],
        expression,
        vec![DeclarativeConstraint::new(
            gateway_domain::ConstraintId::new("coverage-is-explicit").unwrap(),
            ConditionExpression::condition(ConditionId::new("coverage-target").unwrap()),
        )],
        vec![
            AcceptanceCriterion::new(
                AcceptanceCriterionId::new("criterion-coverage").unwrap(),
                "coverage target is explicit",
                ConditionExpression::condition(ConditionId::new("coverage-target").unwrap()),
            )
            .unwrap(),
        ],
    )
    .unwrap();
    Intent::new(IntentId::new("intent-quality").unwrap(), desired)
        .with_original_input(OriginalInput::inline(
            "Ensure the domain layer has no infrastructure dependency and coverage is at least 95 percent.",
        )
        .unwrap())
}

fn records() -> ObservationEvidenceSet {
    let repository = Provenance::new(
        ProvenanceId::new("provenance-repository").unwrap(),
        SourceKind::Repository,
        SourceId::new("external-fixture-repository").unwrap(),
        "git://external-fixture/repository",
    )
    .unwrap()
    .with_producer("fixture-repository")
    .unwrap()
    .with_acquired_at(SourceTimestamp::new("2026-08-29T10:00:00Z").unwrap());
    let tool = Provenance::new(
        ProvenanceId::new("provenance-tool").unwrap(),
        SourceKind::Tool,
        SourceId::new("coverage-tool").unwrap(),
        "tool://coverage-report",
    )
    .unwrap()
    .with_producer("coverage-fixture")
    .unwrap()
    .with_source_timestamp(SourceTimestamp::new("2026-08-29T10:01:00Z").unwrap());
    let synthetic = Provenance::new(
        ProvenanceId::new("provenance-synthetic").unwrap(),
        SourceKind::Synthetic,
        SourceId::new("synthetic-fixture").unwrap(),
        "synthetic://sensitivity-reference",
    )
    .unwrap();

    let architecture_observation = Observation::new(
        ObservationId::new("observation-architecture").unwrap(),
        SubjectPath::from_str("architecture.dependency").unwrap(),
        TypedValue::Boolean(true),
        repository.id().clone(),
    )
    .unwrap();
    let coverage_observation = Observation::new(
        ObservationId::new("observation-coverage").unwrap(),
        SubjectPath::from_str("coverage.percent").unwrap(),
        TypedValue::Decimal(DecimalValue::new(9200, 2).unwrap()),
        tool.id().clone(),
    )
    .unwrap();
    let fixture_observation = Observation::new(
        ObservationId::new("observation-fixture").unwrap(),
        SubjectPath::from_str("fixture.synthetic").unwrap(),
        TypedValue::symbol("FIXTURE").unwrap(),
        synthetic.id().clone(),
    )
    .unwrap();

    let architecture_fact = Fact::new(
        FactId::new("fact-architecture").unwrap(),
        SubjectPath::from_str("architecture.dependency").unwrap(),
        TypedValue::Boolean(true),
        gateway_domain::AssertionPolarity::Affirmed,
        vec![architecture_observation.id().clone()],
    )
    .unwrap();
    let coverage_fact = Fact::new(
        FactId::new("fact-coverage").unwrap(),
        SubjectPath::from_str("coverage.percent").unwrap(),
        TypedValue::Decimal(DecimalValue::new(9200, 2).unwrap()),
        gateway_domain::AssertionPolarity::Affirmed,
        vec![coverage_observation.id().clone()],
    )
    .unwrap();
    let fixture_fact = Fact::new(
        FactId::new("fact-fixture").unwrap(),
        SubjectPath::from_str("fixture.synthetic").unwrap(),
        TypedValue::symbol("FIXTURE").unwrap(),
        gateway_domain::AssertionPolarity::Affirmed,
        vec![fixture_observation.id().clone()],
    )
    .unwrap();

    let architecture_evidence = Evidence::new(
        EvidenceId::new("evidence-architecture").unwrap(),
        EvidenceKind::Report,
        "architecture analysis report",
        EvidenceContent::inline("domain -> infrastructure exists").unwrap(),
        repository.id().clone(),
        vec![EvidenceLink::new(
            architecture_fact.id().clone(),
            EvidenceRelation::Supports,
        )],
    )
    .unwrap();
    let coverage_evidence = Evidence::new(
        EvidenceId::new("evidence-coverage").unwrap(),
        EvidenceKind::Measurement,
        "test coverage report",
        EvidenceContent::inline("coverage measurement").unwrap(),
        tool.id().clone(),
        vec![EvidenceLink::new(
            coverage_fact.id().clone(),
            EvidenceRelation::Supports,
        )],
    )
    .unwrap();
    let sensitive_reference = Evidence::new(
        EvidenceId::new("evidence-sensitive-reference").unwrap(),
        EvidenceKind::Artifact,
        "sensitive coverage artifact reference",
        EvidenceContent::reference(
            gateway_domain::ReferenceId::new("coverage-artifact-reference").unwrap(),
            Some(ContentDigest::new("d".repeat(64)).unwrap()),
        ),
        synthetic.id().clone(),
        vec![EvidenceLink::new(
            coverage_fact.id().clone(),
            EvidenceRelation::Supports,
        )],
    )
    .unwrap();

    ObservationEvidenceSet::new(
        vec![repository, tool, synthetic],
        vec![
            architecture_observation,
            coverage_observation,
            fixture_observation,
        ],
        vec![architecture_fact, coverage_fact, fixture_fact],
        vec![
            architecture_evidence,
            coverage_evidence,
            sensitive_reference,
        ],
    )
    .unwrap()
}

fn process_definition() -> gateway_process::ProcessDefinition {
    let start = StateId::new("start").unwrap();
    let review = StateId::new("review").unwrap();
    let event = EventTypeId::new("review-required").unwrap();
    let evidence = gateway_process::EvidenceTypeId::new("review-evidence").unwrap();
    ProcessDefinitionBuilder::new(
        gateway_process::ProcessDefinitionId::new("quality-process").unwrap(),
        gateway_process::ProcessDefinitionVersion::new(1).unwrap(),
    )
    .with_states([
        StateDefinition::new(start.clone(), true, false).unwrap(),
        StateDefinition::new(review.clone(), false, false).unwrap(),
    ])
    .with_events([EventTypeDefinition::new(event.clone())])
    .with_gates([GateDefinition::new(
        GateId::new("review-gate").unwrap(),
        vec![EvidenceRequirement::new(evidence.clone(), true)],
    )])
    .with_evidence([EvidenceRequirement::new(evidence, true)])
    .with_blockers([BlockerDefinition::new(
        BlockerId::new("review-blocker").unwrap(),
        "human review is required",
        true,
    )
    .unwrap()])
    .with_transitions([TransitionDefinition::new(
        TransitionId::new("enter-review").unwrap(),
        start,
        event,
        review,
        GuardExpression::Always,
    )])
    .build()
    .unwrap()
}

fn blocked_process_instance(definition: &gateway_process::ProcessDefinition) -> ProcessInstance {
    let mut instance = ProcessInstance::start(
        definition,
        ProcessInstanceId::new("quality-process-instance").unwrap(),
    )
    .unwrap();
    instance
        .apply_projection(
            definition,
            TransitionProjection::new(
                instance.revision(),
                TransitionId::new("enter-review").unwrap(),
                StateId::new("review").unwrap(),
                ProcessInstanceStatus::Blocked,
                "review gate is active",
            )
            .unwrap(),
        )
        .unwrap();
    instance.set_gate_status(GateId::new("review-gate").unwrap(), GateStatus::Blocked);
    instance.record_evidence(gateway_process::EvidenceTypeId::new("review-evidence").unwrap());
    instance.record_blocker(
        gateway_process::BlockerRuntimeState::new(
            BlockerId::new("review-blocker").unwrap(),
            "human review is required",
            true,
        )
        .unwrap(),
    );
    instance
}

#[test]
fn neutral_external_project_reaches_a_deterministic_explainable_situation() {
    let app = DeclarativeSituationApplication::new();
    let store = InMemoryContextStore::new();
    let scope = store
        .open_scope(ContextScopeId::new("external-project-a").unwrap())
        .unwrap();
    let submitted_intent = intent();
    assert_eq!(
        store
            .submit_intent(&scope, submitted_intent.clone())
            .unwrap(),
        gateway_application::IngestionResult::Accepted
    );
    assert_eq!(
        store.submit_intent(&scope, submitted_intent).unwrap(),
        gateway_application::IngestionResult::IdempotentReplay
    );

    let source_records = records();
    let reordered_records = ObservationEvidenceSet::new(
        source_records.provenances().iter().rev().cloned().collect(),
        source_records
            .observations()
            .iter()
            .rev()
            .cloned()
            .collect(),
        source_records.facts().iter().rev().cloned().collect(),
        source_records.evidence().iter().rev().cloned().collect(),
    )
    .unwrap();
    assert_eq!(reordered_records, source_records);
    let quality = QualityMetadata::new(
        TrustClass::ObservedEvidence,
        gateway_domain::SensitivityClass::Secret,
        Confidence::score(0.92).unwrap(),
        FreshnessStatus::Fresh,
        Uncertainty::None,
    );
    let batch = ScopedObservationBatch::new(
        scope.id().clone(),
        SourceSnapshot::new(
            SourceId::new("external-fixture").unwrap(),
            SourceKind::Synthetic,
            Some(SourceTimestamp::new("2026-08-29T10:02:00Z").unwrap()),
            Some(ContentDigest::new("e".repeat(64)).unwrap()),
        )
        .unwrap(),
        reordered_records,
    )
    .unwrap()
    .with_quality_metadata(
        SubjectPath::from_str("coverage.percent").unwrap(),
        vec![quality],
    );
    assert_eq!(
        store
            .ingest_observations(&scope, batch.clone())
            .unwrap()
            .result(),
        gateway_application::IngestionResult::Accepted
    );
    assert_eq!(
        store.ingest_observations(&scope, batch).unwrap().result(),
        gateway_application::IngestionResult::IdempotentReplay
    );

    let scoped = store.snapshot(&scope).unwrap();
    assert_eq!(scoped.intent().unwrap().id().as_str(), "intent-quality");
    assert_eq!(scoped.batches().len(), 1);
    let batch = &scoped.batches()[0];
    let mut normalization_input = NormalizationInput::new(batch.records().clone());
    for (subject, metadata) in batch.quality_metadata() {
        normalization_input =
            normalization_input.with_quality_metadata(subject.clone(), metadata.clone());
    }
    let observed_state = app
        .normalize_current_state(
            ObservedStateId::new("state-quality").unwrap(),
            normalization_input,
        )
        .unwrap();
    let coverage = observed_state
        .entries()
        .iter()
        .find(|entry| entry.subject().to_string() == "coverage.percent")
        .unwrap();
    assert_eq!(coverage.status(), gateway_domain::StateStatus::Known);
    assert_eq!(
        coverage.metadata().unwrap().sensitivity(),
        gateway_domain::SensitivityClass::Secret
    );
    assert_eq!(
        coverage.value(),
        Some(&TypedValue::Decimal(DecimalValue::new(9200, 2).unwrap()))
    );

    let basis = BasisReferences::from_state_entry(coverage).unwrap();
    let assessment_quality = coverage.metadata().unwrap();
    let assessment = Assessment::new(
        AssessmentId::new("assessment-coverage").unwrap(),
        AssessmentKind::Coverage,
        AssessmentConclusion::AtRisk,
        AssessmentStatus::Determined,
        ReasonCode::new("COVERAGE_BELOW_TARGET").unwrap(),
        "coverage is below the requested target",
        basis.clone(),
        AssessmentOrigin::Deterministic {
            rule: AssessmentRuleContract::new(
                AssessmentRuleId::new("coverage-rule").unwrap(),
                AssessmentRuleVersion::V1,
            )
            .unwrap()
            .with_semantic_digest(ContentDigest::new("f".repeat(64)).unwrap()),
        },
        assessment_quality,
    )
    .unwrap();
    let risk = Risk::new(
        RiskId::new("risk-coverage").unwrap(),
        RiskCategory::Quality,
        RiskSeverity::High,
        RiskLikelihood::Qualitative(QualitativeLikelihood::Possible),
        RiskStatus::Open,
        ReasonCode::new("COVERAGE_RISK").unwrap(),
        "coverage target may remain unmet",
        BasisReferences::new(
            basis.state_subjects().to_vec(),
            basis.facts().to_vec(),
            basis.evidence().to_vec(),
            basis.provenances().to_vec(),
            vec![assessment.id().clone()],
        )
        .unwrap(),
        RiskOrigin::AssessmentDerived,
        assessment_quality,
    )
    .unwrap();
    let situation = app
        .assess_situation(
            SituationAssemblyInput::new(observed_state.clone())
                .with_records(batch.records().clone())
                .with_assessments(vec![assessment])
                .unwrap()
                .with_risks(vec![risk])
                .unwrap()
                .with_references(vec![
                    SituationReference::External {
                        source: SourceId::new("external-fixture").unwrap(),
                        reference: gateway_domain::ReferenceId::new("analysis-report").unwrap(),
                    },
                    SituationReference::Runtime {
                        runtime: gateway_domain::ExecutionRuntimeId::new("runtime-fixture")
                            .unwrap(),
                        reference: gateway_domain::ReferenceId::new("runtime-snapshot").unwrap(),
                    },
                ])
                .unwrap(),
            SituationId::new("situation-quality").unwrap(),
        )
        .unwrap();
    let process_definition = process_definition();
    let process_instance = blocked_process_instance(&process_definition);
    let process_before = process_instance.clone();
    let execution_context = gateway_domain::ExecutionContext {
        task: gateway_domain::TaskDescriptor::new(
            gateway_domain::TaskId::new("task-quality").unwrap(),
            "inspect quality",
        )
        .unwrap(),
        operating_mode: gateway_domain::OperatingMode::Hardening,
        execution_profile: gateway_domain::ExecutionProfile::FullPath,
    };
    let document = app
        .validate_scoped_declarative_context(
            DeclarativeContext::new_v1(DeclarativeContextId::new("context-quality").unwrap()),
            &scoped,
            observed_state,
            situation,
        )
        .unwrap();
    let process_input = ProcessSnapshotInput::new(&process_definition, &process_instance)
        .requiring_revision(process_instance.revision());
    let inspection = app
        .inspect_situation(&document, Some(&execution_context), Some(process_input))
        .unwrap();
    let process = inspection.process().unwrap();
    assert_eq!(process.status(), ProcessInstanceStatus::Blocked);
    assert_eq!(process.current_state().as_str(), "review");
    assert_eq!(process.active_gates().len(), 1);
    assert_eq!(process.blockers().len(), 1);
    assert_eq!(process.evidence().len(), 1);
    assert_eq!(process.instance_revision().value(), 1);
    assert_eq!(process_before, *process.inspection().instance());
    assert_eq!(process_before, process_instance);

    let explanation = app.explain_situation(&inspection);
    assert_eq!(
        explanation.operating_mode(),
        Some(gateway_domain::OperatingMode::Hardening)
    );
    assert_eq!(
        explanation.execution_profile(),
        Some(gateway_domain::ExecutionProfile::FullPath)
    );
    assert_eq!(
        explanation.process_definition_id().unwrap().as_str(),
        "quality-process"
    );
    assert_eq!(
        explanation.process_instance_id().unwrap().as_str(),
        "quality-process-instance"
    );
    assert_eq!(explanation.process_instance_revision().unwrap().value(), 1);
    assert_eq!(explanation.traces().len(), 2);

    let serialized = app.serialize_situation(&document).unwrap();
    assert!(serialized.contains("coverage-artifact-reference"));
    assert!(serialized.contains("SECRET"));
    assert!(!serialized.contains("raw-secret-value"));
    let restored =
        gateway_domain::DeclarativeContextSituationDocument::from_json(&serialized).unwrap();
    assert_eq!(restored, document);

    let isolated_scope = store
        .open_scope(ContextScopeId::new("external-project-b").unwrap())
        .unwrap();
    let isolated = store.snapshot(&isolated_scope).unwrap();
    assert!(isolated.intent().is_none());
    assert!(isolated.batches().is_empty());
}
