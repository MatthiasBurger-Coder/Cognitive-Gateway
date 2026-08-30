//! CG-07.10 end-to-end acceptance proof.
//!
//! The scenarios exercise the public application facade from the CG-06
//! DesiredState/CurrentState boundary through the CG-08-ready declarative
//! Plan.  The fixture deliberately contains no process definition, executor,
//! policy decision or context compilation step.

use std::str::FromStr;

use gateway_application::{
    DeclarativePlanningApplication, PlanningCapabilitySnapshot, PlanningRuleSnapshot,
};
use gateway_domain::{
    AgentDefinitionDocument, AssertionPolarity, CapabilityClass, CapabilityDefinition,
    CapabilityId, CapabilityRequirement, CapabilityRequirementRules, ComparisonOperator,
    ComparisonOutcome, ComparisonRules, ConditionExpression, CurrentState, DecimalValue,
    DeltaDerivationRules, DeltaId, DeltaKind, DesiredCondition, DesiredState, DesiredStateId,
    Evidence, EvidenceContent, EvidenceId, EvidenceKind, EvidenceLink, EvidenceRelation,
    ExecutionContext, ExecutionProfile, Fact, FactId, NormalizationInput, Observation,
    ObservationEvidenceSet, ObservationId, ObservedStateId, OperatingMode, PlanStepKind,
    PlannerRules, PlanningIrVersion, Provenance, ProvenanceId, ReferenceId, RequiredOutcomeKind,
    RequirementCardinality, Situation, SituationAssemblyInput, SituationId, SituationReference,
    SkillDefinitionDocument, SourceId, SourceKind, SubjectPath, TaskDescriptor, TypedValue,
    normalize_current_state,
};
use gateway_registry::{CapabilityIndex, Registry};

const ARCHITECTURE_SUBJECT: &str = "architecture.dependency";
const COVERAGE_SUBJECT: &str = "coverage.percent";
const ARCHITECTURE_CONDITION: &str = "no-infrastructure-dependency";
const COVERAGE_CONDITION: &str = "coverage-target";
const CHANGE_CAPABILITY: &str = "project.declarative-change";
const EVIDENCE_CAPABILITY: &str = "project.evidence-acquisition";
const OBSERVATION_CAPABILITY: &str = "project.state-observation";
const CONFLICT_CAPABILITY: &str = "project.conflict-resolution";
const VERIFICATION_CAPABILITY: &str = "project.quality-verification";

fn condition(
    id: &str,
    subject: &str,
    operator: ComparisonOperator,
    expected: Option<TypedValue>,
) -> DesiredCondition {
    DesiredCondition::new(
        gateway_domain::ConditionId::new(id).unwrap(),
        SubjectPath::from_str(subject).unwrap(),
        operator,
        expected,
    )
    .unwrap()
}

fn desired_reference() -> DesiredState {
    let architecture = condition(
        ARCHITECTURE_CONDITION,
        ARCHITECTURE_SUBJECT,
        ComparisonOperator::Equals,
        Some(TypedValue::Boolean(false)),
    );
    let coverage = condition(
        COVERAGE_CONDITION,
        COVERAGE_SUBJECT,
        ComparisonOperator::GreaterOrEqual,
        Some(TypedValue::Decimal(DecimalValue::new(9500, 2).unwrap())),
    );
    DesiredState::new(
        DesiredStateId::new("desired-external-quality").unwrap(),
        vec![architecture, coverage],
        ConditionExpression::all(vec![
            ConditionExpression::condition(
                gateway_domain::ConditionId::new(ARCHITECTURE_CONDITION).unwrap(),
            ),
            ConditionExpression::condition(
                gateway_domain::ConditionId::new(COVERAGE_CONDITION).unwrap(),
            ),
        ])
        .unwrap(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap()
}

fn desired_single(
    id: &str,
    condition_id: &str,
    subject: &str,
    operator: ComparisonOperator,
    expected: Option<TypedValue>,
) -> DesiredState {
    DesiredState::new(
        DesiredStateId::new(id).unwrap(),
        vec![condition(condition_id, subject, operator, expected)],
        ConditionExpression::condition(gateway_domain::ConditionId::new(condition_id).unwrap()),
        Vec::new(),
        Vec::new(),
    )
    .unwrap()
}

fn provenance(id: &str, source: SourceKind) -> Provenance {
    Provenance::new(
        ProvenanceId::new(id).unwrap(),
        source,
        SourceId::new(format!("source-{id}")).unwrap(),
        format!("fixture://{id}"),
    )
    .unwrap()
}

fn records(
    architecture_values: &[bool],
    coverage: TypedValue,
    include_evidence: bool,
    reverse: bool,
) -> ObservationEvidenceSet {
    let repository = provenance("repository", SourceKind::Repository);
    let coverage_tool = provenance("coverage-tool", SourceKind::Tool);
    let mut observations = Vec::new();
    let mut facts = Vec::new();
    let mut evidence = Vec::new();

    for (index, value) in architecture_values.iter().copied().enumerate() {
        let observation_id =
            ObservationId::new(format!("observation-architecture-{index}")).unwrap();
        let fact_id = FactId::new(format!("fact-architecture-{index}")).unwrap();
        observations.push(
            Observation::new(
                observation_id.clone(),
                SubjectPath::from_str(ARCHITECTURE_SUBJECT).unwrap(),
                TypedValue::Boolean(value),
                repository.id().clone(),
            )
            .unwrap(),
        );
        facts.push(
            Fact::new(
                fact_id.clone(),
                SubjectPath::from_str(ARCHITECTURE_SUBJECT).unwrap(),
                TypedValue::Boolean(value),
                AssertionPolarity::Affirmed,
                vec![observation_id],
            )
            .unwrap(),
        );
        if include_evidence {
            evidence.push(
                Evidence::new(
                    EvidenceId::new(format!("evidence-architecture-{index}")).unwrap(),
                    EvidenceKind::Report,
                    "architecture dependency report",
                    EvidenceContent::inline("sensitive architecture report content").unwrap(),
                    repository.id().clone(),
                    vec![EvidenceLink::new(fact_id, EvidenceRelation::Supports)],
                )
                .unwrap(),
            );
        }
    }

    let coverage_observation_id = ObservationId::new("observation-coverage").unwrap();
    let coverage_fact_id = FactId::new("fact-coverage").unwrap();
    observations.push(
        Observation::new(
            coverage_observation_id.clone(),
            SubjectPath::from_str(COVERAGE_SUBJECT).unwrap(),
            coverage.clone(),
            coverage_tool.id().clone(),
        )
        .unwrap(),
    );
    facts.push(
        Fact::new(
            coverage_fact_id.clone(),
            SubjectPath::from_str(COVERAGE_SUBJECT).unwrap(),
            coverage,
            AssertionPolarity::Affirmed,
            vec![coverage_observation_id],
        )
        .unwrap(),
    );
    if include_evidence {
        evidence.push(
            Evidence::new(
                EvidenceId::new("evidence-coverage").unwrap(),
                EvidenceKind::Measurement,
                "coverage measurement report",
                EvidenceContent::inline("sensitive coverage report content").unwrap(),
                coverage_tool.id().clone(),
                vec![EvidenceLink::new(
                    coverage_fact_id,
                    EvidenceRelation::Supports,
                )],
            )
            .unwrap(),
        );
    }

    let mut provenances = vec![repository, coverage_tool];
    if reverse {
        provenances.reverse();
        observations.reverse();
        facts.reverse();
        evidence.reverse();
    }
    ObservationEvidenceSet::new(provenances, observations, facts, evidence).unwrap()
}

fn current(
    id: &str,
    records: ObservationEvidenceSet,
    unknown_subjects: &[&str],
    require_evidence: bool,
) -> CurrentState {
    let mut input = NormalizationInput::new(records).with_required_evidence(require_evidence);
    if !unknown_subjects.is_empty() {
        input = input
            .with_unknown_subjects(
                unknown_subjects
                    .iter()
                    .map(|subject| SubjectPath::from_str(subject).unwrap()),
            )
            .unwrap();
    }
    normalize_current_state(ObservedStateId::new(id).unwrap(), input).unwrap()
}

fn situation(current: &CurrentState, records: ObservationEvidenceSet) -> Situation {
    SituationAssemblyInput::new(current.clone())
        .with_records(records)
        .with_references(vec![SituationReference::External {
            source: SourceId::new("external-project").unwrap(),
            reference: ReferenceId::new("quality-report").unwrap(),
        }])
        .unwrap()
        .assemble(SituationId::new("situation-external-quality").unwrap())
        .unwrap()
}

fn capability_snapshot() -> PlanningCapabilitySnapshot {
    let agent_id = gateway_domain::AgentId::new("reference-agent").unwrap();
    let skill_id = gateway_domain::SkillId::new("reference-skill").unwrap();
    let capabilities = [
        (CHANGE_CAPABILITY, CapabilityClass::Mutate),
        (EVIDENCE_CAPABILITY, CapabilityClass::Inspect),
        (OBSERVATION_CAPABILITY, CapabilityClass::Inspect),
        (CONFLICT_CAPABILITY, CapabilityClass::Inspect),
        (VERIFICATION_CAPABILITY, CapabilityClass::Inspect),
    ]
    .into_iter()
    .map(|(id, class)| CapabilityDefinition::new(CapabilityId::new(id).unwrap(), class))
    .collect::<Vec<_>>();
    let agent =
        AgentDefinitionDocument::new(agent_id.clone(), "reference provider", [skill_id.clone()])
            .unwrap();
    let skill = SkillDefinitionDocument::new_with_provided_capabilities(
        skill_id,
        "reference skill",
        "reference skill for canonical contracts",
        Some(agent_id),
        std::iter::empty::<String>(),
        std::iter::empty::<String>(),
        std::iter::empty::<String>(),
        std::iter::empty::<gateway_domain::SkillId>(),
        std::iter::empty::<gateway_domain::SkillId>(),
        std::iter::empty::<CapabilityId>(),
        std::iter::empty::<gateway_domain::KnowledgeQuery>(),
        capabilities,
    )
    .unwrap();
    let registry = Registry::from_documents([agent], [skill]).unwrap();
    let index = registry.capability_index().unwrap();
    PlanningCapabilitySnapshot::new(index, "cg03-reference-snapshot", PlanningIrVersion::V1)
        .unwrap()
}

fn empty_snapshot() -> PlanningCapabilitySnapshot {
    PlanningCapabilitySnapshot::new(
        CapabilityIndex::default(),
        "cg03-empty-snapshot",
        PlanningIrVersion::V1,
    )
    .unwrap()
}

fn rules() -> (ComparisonRules, DeltaDerivationRules) {
    (ComparisonRules::default(), DeltaDerivationRules::default())
}

fn derive_delta(
    app: &DeclarativePlanningApplication,
    desired: &DesiredState,
    current: &CurrentState,
    situation: Option<&Situation>,
    delta_id: &str,
    comparison_rules: &ComparisonRules,
    delta_rules: &DeltaDerivationRules,
) -> gateway_domain::DeltaDerivation {
    app.derive_delta(
        DeltaId::new(delta_id).unwrap(),
        desired,
        current,
        situation,
        comparison_rules,
        delta_rules,
    )
    .unwrap()
}

#[test]
fn reference_project_reaches_a_traceable_cg08_ready_plan() {
    let app = DeclarativePlanningApplication::new();
    let desired = desired_reference();
    let source_records = records(
        &[true],
        TypedValue::Decimal(DecimalValue::new(9200, 2).unwrap()),
        true,
        false,
    );
    let current_state = current(
        "current-external-quality",
        source_records.clone(),
        &[],
        false,
    );
    let current_situation = situation(&current_state, source_records.clone());

    // OperatingMode and ExecutionProfile remain CG-06/CG-02 input dimensions;
    // CG-07 consumes the normalized state and does not compile an execution context.
    let execution_context = ExecutionContext {
        task: TaskDescriptor::new(
            gateway_domain::TaskId::new("task-quality").unwrap(),
            "remediate quality deficits",
        )
        .unwrap(),
        operating_mode: OperatingMode::Hardening,
        execution_profile: ExecutionProfile::FullPath,
    };
    assert_eq!(execution_context.operating_mode, OperatingMode::Hardening);
    assert_eq!(
        execution_context.execution_profile,
        ExecutionProfile::FullPath
    );

    let (comparison_rules, delta_rules) = rules();
    let comparison = app
        .compare_desired_to_situation(
            &desired,
            &current_state,
            Some(&current_situation),
            &comparison_rules,
        )
        .unwrap();
    assert_eq!(comparison.outcome(), ComparisonOutcome::Unsatisfied);
    assert!(
        comparison
            .children()
            .iter()
            .all(|child| child.outcome() == ComparisonOutcome::Unsatisfied)
    );

    let delta = derive_delta(
        &app,
        &desired,
        &current_state,
        Some(&current_situation),
        "delta-external-quality",
        &comparison_rules,
        &delta_rules,
    );
    assert_eq!(delta.comparison(), &comparison);
    assert_eq!(delta.delta().items().len(), 2);
    assert!(delta.delta().items().iter().all(|item| {
        item.kind() == DeltaKind::UnsatisfiedCondition
            && item.required_outcome().kind() == RequiredOutcomeKind::DomainChange
            && item.basis().situation().is_some()
            && !item.basis().facts().is_empty()
            && !item.basis().evidence().is_empty()
            && !item.basis().provenances().is_empty()
    }));

    let snapshot = capability_snapshot();
    let capability_rules = CapabilityRequirementRules::default()
        .with_domain_change(CapabilityId::new(CHANGE_CAPABILITY).unwrap());
    let requirements = app
        .derive_capability_requirements(&desired, delta.delta(), &snapshot, &capability_rules)
        .unwrap();
    assert!(requirements.is_execution_ready());
    assert_eq!(requirements.requirements().len(), 2);
    assert!(
        requirements
            .requirements()
            .iter()
            .all(|requirement| requirement.capability().as_str() == CHANGE_CAPABILITY)
    );

    let verification = CapabilityRequirement::new(
        gateway_domain::CapabilityRequirementId::new("requirement.quality-verification").unwrap(),
        CapabilityId::new(VERIFICATION_CAPABILITY).unwrap(),
        RequirementCardinality::Mandatory,
        delta.delta().items()[0].id().clone(),
        "verify both desired quality outcomes after remediation",
    )
    .unwrap();
    let planner_rules = PlannerRules::default().with_verification_requirement(verification);
    let planner_result = app
        .build_plan(&desired, delta.delta(), &requirements, &planner_rules)
        .unwrap();
    assert!(planner_result.is_execution_ready());
    let plan = planner_result.plan().unwrap();
    assert_eq!(plan.steps().len(), 4);
    assert_eq!(
        plan.steps()
            .iter()
            .filter(|step| step.kind() == PlanStepKind::Change)
            .count(),
        2
    );
    assert_eq!(
        plan.steps()
            .iter()
            .filter(|step| step.kind() == PlanStepKind::Verification)
            .count(),
        2
    );
    assert!(
        plan.steps()
            .iter()
            .filter(|step| step.kind() == PlanStepKind::Verification)
            .all(|step| !step.dependencies().is_empty())
    );
    assert_eq!(plan.parallel_layers().unwrap().len(), 2);
    assert!(app.validate_plan(&desired, delta.delta(), plan).is_valid());

    let rule_snapshot = PlanningRuleSnapshot::from_rules(
        &comparison_rules,
        &delta_rules,
        &capability_rules,
        &planner_rules,
    );
    let explanation = app
        .explain_plan(
            &desired,
            delta.delta(),
            &planner_result,
            &snapshot,
            rule_snapshot,
        )
        .unwrap();
    assert_eq!(explanation.trace().entries().len(), 2);
    assert!(
        explanation
            .trace()
            .entries()
            .iter()
            .all(|entry| !entry.basis().evidence().is_empty())
    );
    assert!(explanation.to_text().contains("cg03-reference-snapshot"));
    assert!(explanation.to_text().contains("DOMAIN_CHANGE"));
    assert!(
        !explanation
            .to_text()
            .contains("sensitive architecture report content")
    );
    assert!(!explanation.to_text().contains("reference-agent"));
    assert!(!explanation.to_text().contains("reference-skill"));

    let serialized = app.serialize_plan(plan).unwrap();
    assert_eq!(gateway_domain::Plan::from_json(&serialized).unwrap(), *plan);
    assert!(!serialized.contains("reference-agent"));
    assert!(!serialized.contains("reference-skill"));

    let reordered_records = records(
        &[true],
        TypedValue::Decimal(DecimalValue::new(9200, 2).unwrap()),
        true,
        true,
    );
    let reordered_current = current(
        "current-external-quality",
        reordered_records.clone(),
        &[],
        false,
    );
    let reordered_situation = situation(&reordered_current, reordered_records);
    let reordered_delta = derive_delta(
        &app,
        &desired,
        &reordered_current,
        Some(&reordered_situation),
        "delta-external-quality",
        &comparison_rules,
        &delta_rules,
    );
    let reordered_requirements = app
        .derive_capability_requirements(
            &desired,
            reordered_delta.delta(),
            &snapshot,
            &capability_rules,
        )
        .unwrap();
    let reordered_plan = app
        .build_plan(
            &desired,
            reordered_delta.delta(),
            &reordered_requirements,
            &planner_rules,
        )
        .unwrap();
    assert_eq!(delta.delta(), reordered_delta.delta());
    assert_eq!(plan, reordered_plan.plan().unwrap());
    assert_eq!(
        serialized,
        app.serialize_plan(reordered_plan.plan().unwrap()).unwrap()
    );
}

#[test]
fn final_acceptance_variants_keep_uncertainty_and_gaps_explicit() {
    let app = DeclarativePlanningApplication::new();
    let snapshot = capability_snapshot();
    let (comparison_rules, delta_rules) = rules();

    let satisfied_desired = desired_reference();
    let satisfied_records = records(
        &[false],
        TypedValue::Decimal(DecimalValue::new(9500, 2).unwrap()),
        true,
        false,
    );
    let satisfied_current = current("current-satisfied", satisfied_records.clone(), &[], false);
    let satisfied_situation = situation(&satisfied_current, satisfied_records);
    let satisfied_delta = derive_delta(
        &app,
        &satisfied_desired,
        &satisfied_current,
        Some(&satisfied_situation),
        "delta-satisfied",
        &comparison_rules,
        &delta_rules,
    );
    assert!(satisfied_delta.delta().is_noop());
    let satisfied_requirements = app
        .derive_capability_requirements(
            &satisfied_desired,
            satisfied_delta.delta(),
            &snapshot,
            &CapabilityRequirementRules::default()
                .with_domain_change(CapabilityId::new(CHANGE_CAPABILITY).unwrap()),
        )
        .unwrap();
    let satisfied_plan = app
        .build_plan(
            &satisfied_desired,
            satisfied_delta.delta(),
            &satisfied_requirements,
            &PlannerRules::default(),
        )
        .unwrap();
    assert!(satisfied_plan.is_noop());
    assert!(
        app.validate_plan(
            &satisfied_desired,
            satisfied_delta.delta(),
            satisfied_plan.plan().unwrap(),
        )
        .is_valid()
    );

    let missing_records = records(
        &[true],
        TypedValue::Decimal(DecimalValue::new(9200, 2).unwrap()),
        false,
        false,
    );
    let missing_current = current(
        "current-missing-evidence",
        missing_records.clone(),
        &[],
        true,
    );
    let missing_situation = situation(&missing_current, missing_records);
    let missing_comparison = app
        .compare_desired_to_situation(
            &satisfied_desired,
            &missing_current,
            Some(&missing_situation),
            &comparison_rules,
        )
        .unwrap();
    assert_eq!(
        missing_comparison.outcome(),
        ComparisonOutcome::InsufficientEvidence
    );
    let missing_delta = derive_delta(
        &app,
        &satisfied_desired,
        &missing_current,
        Some(&missing_situation),
        "delta-missing-evidence",
        &comparison_rules,
        &delta_rules,
    );
    assert!(
        missing_delta
            .delta()
            .items()
            .iter()
            .filter(|item| item.is_actionable())
            .all(|item| item.kind() == DeltaKind::MissingEvidence)
    );
    let missing_requirements = app
        .derive_capability_requirements(
            &satisfied_desired,
            missing_delta.delta(),
            &snapshot,
            &CapabilityRequirementRules::default()
                .with_evidence_acquisition(CapabilityId::new(EVIDENCE_CAPABILITY).unwrap()),
        )
        .unwrap();
    let missing_plan = app
        .build_plan(
            &satisfied_desired,
            missing_delta.delta(),
            &missing_requirements,
            &PlannerRules::default(),
        )
        .unwrap();
    assert!(
        missing_plan
            .plan()
            .unwrap()
            .steps()
            .iter()
            .all(|step| step.kind() == PlanStepKind::EvidenceAcquisition)
    );

    let conflict_records = records(
        &[true, false],
        TypedValue::Decimal(DecimalValue::new(9500, 2).unwrap()),
        true,
        false,
    );
    let conflict_current = current("current-conflicted", conflict_records.clone(), &[], false);
    let conflict_situation = situation(&conflict_current, conflict_records);
    let conflict_delta = derive_delta(
        &app,
        &satisfied_desired,
        &conflict_current,
        Some(&conflict_situation),
        "delta-conflicted",
        &comparison_rules,
        &delta_rules,
    );
    let conflict_item = conflict_delta
        .delta()
        .items()
        .iter()
        .find(|item| item.condition().as_str() == ARCHITECTURE_CONDITION)
        .unwrap();
    assert_eq!(conflict_item.kind(), DeltaKind::Conflict);
    assert_eq!(
        conflict_item.required_outcome().kind(),
        RequiredOutcomeKind::ConflictResolution
    );
    let conflict_requirements = app
        .derive_capability_requirements(
            &satisfied_desired,
            conflict_delta.delta(),
            &snapshot,
            &CapabilityRequirementRules::default()
                .with_conflict_resolution(CapabilityId::new(CONFLICT_CAPABILITY).unwrap()),
        )
        .unwrap();
    let conflict_plan = app
        .build_plan(
            &satisfied_desired,
            conflict_delta.delta(),
            &conflict_requirements,
            &PlannerRules::default(),
        )
        .unwrap();
    assert!(
        conflict_plan
            .plan()
            .unwrap()
            .steps()
            .iter()
            .any(|step| step.kind() == PlanStepKind::ConflictResolution)
    );

    let unknown_desired = desired_single(
        "desired-unknown",
        "unknown-architecture",
        ARCHITECTURE_SUBJECT,
        ComparisonOperator::Equals,
        Some(TypedValue::Boolean(false)),
    );
    let unknown_records = records(
        &[],
        TypedValue::Decimal(DecimalValue::new(9500, 2).unwrap()),
        true,
        false,
    );
    let unknown_current = current(
        "current-unknown",
        unknown_records,
        &[ARCHITECTURE_SUBJECT],
        false,
    );
    let unknown_delta = derive_delta(
        &app,
        &unknown_desired,
        &unknown_current,
        None,
        "delta-unknown",
        &comparison_rules,
        &delta_rules,
    );
    assert_eq!(
        unknown_delta.delta().items()[0].kind(),
        DeltaKind::UnknownState
    );
    let unknown_requirements = app
        .derive_capability_requirements(
            &unknown_desired,
            unknown_delta.delta(),
            &snapshot,
            &CapabilityRequirementRules::default()
                .with_observation(CapabilityId::new(OBSERVATION_CAPABILITY).unwrap()),
        )
        .unwrap();
    let unknown_plan = app
        .build_plan(
            &unknown_desired,
            unknown_delta.delta(),
            &unknown_requirements,
            &PlannerRules::default(),
        )
        .unwrap();
    assert!(
        unknown_plan
            .plan()
            .unwrap()
            .steps()
            .iter()
            .any(|step| step.kind() == PlanStepKind::Observation)
    );

    let incomparable_desired = desired_single(
        "desired-incomparable",
        "incomparable-coverage",
        COVERAGE_SUBJECT,
        ComparisonOperator::GreaterOrEqual,
        Some(TypedValue::Decimal(DecimalValue::new(9500, 2).unwrap())),
    );
    let incomparable_records = records(&[], TypedValue::string("ninety-two").unwrap(), true, false);
    let incomparable_current = current("current-incomparable", incomparable_records, &[], false);
    let incomparable_delta = derive_delta(
        &app,
        &incomparable_desired,
        &incomparable_current,
        None,
        "delta-incomparable",
        &comparison_rules,
        &delta_rules,
    );
    assert_eq!(
        incomparable_delta.delta().items()[0].kind(),
        DeltaKind::UnsupportedComparison
    );
    let incomparable_requirements = app
        .derive_capability_requirements(
            &incomparable_desired,
            incomparable_delta.delta(),
            &snapshot,
            &CapabilityRequirementRules::default()
                .with_assessment(CapabilityId::new(VERIFICATION_CAPABILITY).unwrap()),
        )
        .unwrap();
    let incomparable_plan = app
        .build_plan(
            &incomparable_desired,
            incomparable_delta.delta(),
            &incomparable_requirements,
            &PlannerRules::default(),
        )
        .unwrap();
    assert!(
        incomparable_plan
            .plan()
            .unwrap()
            .steps()
            .iter()
            .any(|step| step.kind() == PlanStepKind::Verification)
    );

    let stale_records = records(
        &[],
        TypedValue::Decimal(DecimalValue::new(9200, 2).unwrap()),
        true,
        false,
    );
    let stale_input = NormalizationInput::new(stale_records).with_quality_metadata(
        SubjectPath::from_str(COVERAGE_SUBJECT).unwrap(),
        vec![gateway_domain::QualityMetadata::new(
            gateway_domain::TrustClass::ObservedEvidence,
            gateway_domain::SensitivityClass::Public,
            gateway_domain::Confidence::score(0.9).unwrap(),
            gateway_domain::FreshnessStatus::Stale,
            gateway_domain::Uncertainty::None,
        )],
    );
    let stale_current =
        normalize_current_state(ObservedStateId::new("current-stale").unwrap(), stale_input)
            .unwrap();
    let stale_rules = ComparisonRules::default().requiring_fresh_evidence(true);
    let stale_comparison = app
        .compare_desired_to_situation(&incomparable_desired, &stale_current, None, &stale_rules)
        .unwrap();
    assert_eq!(
        stale_comparison.outcome(),
        ComparisonOutcome::InsufficientEvidence
    );
    let stale_delta = derive_delta(
        &app,
        &incomparable_desired,
        &stale_current,
        None,
        "delta-stale",
        &stale_rules,
        &delta_rules,
    );
    assert_eq!(
        stale_delta.delta().items()[0].kind(),
        DeltaKind::MissingEvidence
    );
    assert_eq!(
        stale_delta.delta().items()[0].reason(),
        gateway_domain::DeltaReasonCode::StaleEvidence
    );

    let absent_capability_delta = derive_delta(
        &app,
        &satisfied_desired,
        &current(
            "current-absent-capability",
            records(
                &[true],
                TypedValue::Decimal(DecimalValue::new(9200, 2).unwrap()),
                true,
                false,
            ),
            &[],
            false,
        ),
        None,
        "delta-absent-capability",
        &comparison_rules,
        &delta_rules,
    );
    let absent_requirements = app
        .derive_capability_requirements(
            &satisfied_desired,
            absent_capability_delta.delta(),
            &empty_snapshot(),
            &CapabilityRequirementRules::default()
                .with_domain_change(CapabilityId::new(CHANGE_CAPABILITY).unwrap()),
        )
        .unwrap();
    assert!(!absent_requirements.is_execution_ready());
    let absent_plan = app
        .build_plan(
            &satisfied_desired,
            absent_capability_delta.delta(),
            &absent_requirements,
            &PlannerRules::default(),
        )
        .unwrap();
    assert!(absent_plan.plan().is_none());
    let explanation_error = app
        .explain_plan(
            &satisfied_desired,
            absent_capability_delta.delta(),
            &absent_plan,
            &empty_snapshot(),
            PlanningRuleSnapshot::from_rules(
                &comparison_rules,
                &delta_rules,
                &CapabilityRequirementRules::default()
                    .with_domain_change(CapabilityId::new(CHANGE_CAPABILITY).unwrap()),
                &PlannerRules::default(),
            ),
        )
        .unwrap_err();
    assert_eq!(explanation_error.code(), "DOMAIN_VALIDATION_ERROR");
}
