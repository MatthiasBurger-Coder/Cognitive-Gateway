//! Neutral CG-07 output through CG-08. Fixtures are explicitly synthetic.
use gateway_application::{
    DeclarativePlanningApplication, DeclarativeSituationApplication, PlanningCapabilitySnapshot,
    ProcessSnapshotInput, resolution::*, resolution_applicability::*, resolution_application::*,
    resolution_artifact::ArtifactLimits, resolution_candidates::CandidateOutcome,
    resolution_composition::*, resolution_explain::TraceLimits, resolution_skills::*,
    resolution_snapshot::*,
};
use gateway_domain::*;
use gateway_process::{
    ActivityConstraint, ProcessInstance, ProcessInstanceRevision, ProcessRegistry, ProcessSource,
};
use gateway_registry::{CapabilitySelector, Registry};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
#[path = "support/composition.rs"]
mod composition;
#[path = "support/cg08_reference.rs"]
mod reference;
mod support;

fn synthetic_registry() -> Registry {
    let agent = AgentId::new("fixture-agent").unwrap();
    let core = SkillId::new("fixture-core").unwrap();
    let leaf = SkillId::new("fixture-leaf").unwrap();
    let related = SkillId::new("fixture-related").unwrap();
    let owner =
        AgentDefinitionDocument::new(agent.clone(), "synthetic capability owner", [core.clone()])
            .unwrap();
    let core = SkillDefinition::new_with_owner(
        core,
        "synthetic reusable resolution contract",
        agent.clone(),
        [leaf.clone()],
        [CapabilityId::new(reference::OBSERVATION_CAPABILITY).unwrap()],
    )
    .unwrap()
    .with_related_skill_ids([related.clone()])
    .unwrap()
    .with_provided_capabilities([
        CapabilityDefinition::new(
            CapabilityId::new(reference::CHANGE_CAPABILITY).unwrap(),
            CapabilityClass::Mutate,
        ),
        CapabilityDefinition::new(
            CapabilityId::new(reference::VERIFICATION_CAPABILITY).unwrap(),
            CapabilityClass::Inspect,
        ),
        CapabilityDefinition::new(
            CapabilityId::new(reference::EVIDENCE_CAPABILITY).unwrap(),
            CapabilityClass::Inspect,
        ),
        CapabilityDefinition::new(
            CapabilityId::new(reference::CONFLICT_CAPABILITY).unwrap(),
            CapabilityClass::Inspect,
        ),
    ])
    .unwrap();
    let leaf =
        SkillDefinition::new_with_owner(leaf, "synthetic shared dependency", agent.clone(), [], [])
            .unwrap()
            .with_provided_capabilities([CapabilityDefinition::new(
                CapabilityId::new(reference::OBSERVATION_CAPABILITY).unwrap(),
                CapabilityClass::Inspect,
            )])
            .unwrap();
    let related =
        SkillDefinition::new_with_owner(related, "related is not required", agent, [], [])
            .unwrap()
            .with_provided_capabilities([CapabilityDefinition::new(
                CapabilityId::new("project.unrelated").unwrap(),
                CapabilityClass::Mutate,
            )])
            .unwrap();
    Registry::from_documents(
        [owner],
        [core, leaf, related].into_iter().map(|skill| {
            let mut wire =
                serde_json::to_value(SkillDefinitionDocument::from_domain(skill)).unwrap();
            wire["authoritative_sources"] = serde_json::json!(["synthetic test contract"]);
            wire["rules"] = serde_json::json!(["only declared capabilities"]);
            wire["verification"] = serde_json::json!(["assert canonical closure"]);
            SkillDefinitionDocument::from_json(&wire.to_string()).unwrap()
        }),
    )
    .unwrap()
}
fn fixture(reverse: bool, satisfied: bool) -> ResolutionSnapshotInput {
    let registry = synthetic_registry();
    let index = registry.capability_index().unwrap();
    let desired = reference::desired_reference();
    let records = reference::records(
        &[!satisfied],
        TypedValue::Decimal(DecimalValue::new(if satisfied { 9500 } else { 9200 }, 2).unwrap()),
        true,
        reverse,
    );
    let current = reference::current("current-external-quality", records.clone(), &[], false);
    let situation = reference::situation(&current, records.clone());
    let planner = DeclarativePlanningApplication::new();
    let delta = planner
        .derive_delta(
            DeltaId::new("delta-external-quality").unwrap(),
            &desired,
            &current,
            Some(&situation),
            &ComparisonRules::default(),
            &DeltaDerivationRules::default(),
        )
        .unwrap();
    let capability_snapshot = PlanningCapabilitySnapshot::new(
        index.clone(),
        "synthetic-cg08-catalog",
        PlanningIrVersion::V1,
    )
    .unwrap();
    let capability_rules = CapabilityRequirementRules::default()
        .with_domain_change(CapabilityId::new(reference::CHANGE_CAPABILITY).unwrap());
    let requirements = planner
        .derive_capability_requirements(
            &desired,
            delta.delta(),
            &capability_snapshot,
            &capability_rules,
        )
        .unwrap();
    let mut planner_rules = PlannerRules::default();
    if !satisfied {
        planner_rules = planner_rules.with_verification_requirement(
            CapabilityRequirement::new(
                CapabilityRequirementId::new("requirement.quality-verification").unwrap(),
                CapabilityId::new(reference::VERIFICATION_CAPABILITY).unwrap(),
                RequirementCardinality::Mandatory,
                delta.delta().items()[0].id().clone(),
                "verify both outcomes after remediation",
            )
            .unwrap(),
        );
    }
    let plan = planner
        .build_plan(&desired, delta.delta(), &requirements, &planner_rules)
        .unwrap()
        .plan()
        .unwrap()
        .clone();
    assert!(
        planner
            .validate_plan(&desired, delta.delta(), &plan)
            .is_valid()
    );
    let document = DeclarativeSituationApplication::new()
        .validate_declarative_context(
            DeclarativeContext::new_v1(
                DeclarativeContextId::new("context-external-quality").unwrap(),
            ),
            Some(Intent::new(
                IntentId::new("intent-quality").unwrap(),
                desired.clone(),
            )),
            Some(records),
            current,
            situation,
        )
        .unwrap();
    ResolutionSnapshotInput {
        version: SchemaVersion::V1,
        scope: ContextScopeId::new("neutral-project").unwrap(),
        plan_scope: ContextScopeId::new("neutral-project").unwrap(),
        situation_scope: ContextScopeId::new("neutral-project").unwrap(),
        plan,
        desired,
        delta: delta.delta().clone(),
        situation: document,
        operating_mode: OperatingMode::Hardening,
        execution_profile: ExecutionProfile::FullPath,
        registry,
        index,
        processes: ProcessRegistry::from_sources(Vec::<ProcessSource>::new()).unwrap(),
        instance: None,
        expected_revision: None,
        situation_process: None,
        admission: Some(ReferenceId::new("external-plan-admission").unwrap()),
        rule_version: SchemaVersion::V1,
        alternatives: vec![],
    }
}
fn resolver_rules(input: &ResolutionSnapshotInput) -> CompositionRules {
    let mut r = composition::rules();
    r.processes.lifecycle_contracts.insert(
        LifecycleRequirementKind::VerificationAfterChange,
        ActivityConstraint::new("lifecycle", "VERIFICATION_AFTER_CHANGE").unwrap(),
    );
    for step in input.plan.steps() {
        let mut sr = composition::skill_rules();
        sr.conditions = BTreeMap::from([
            (
                SkillId::new("fixture-core").unwrap(),
                SkillCondition::Mode(OperatingMode::Hardening),
            ),
            (
                SkillId::new("fixture-leaf").unwrap(),
                SkillCondition::Profile(ExecutionProfile::FullPath),
            ),
            (
                SkillId::new("fixture-related").unwrap(),
                SkillCondition::Never,
            ),
        ]);
        r.skills.insert(step.id().clone(), sr);
    }
    r
}
fn pin(mut input: ResolutionSnapshotInput, blocked: bool) -> ResolutionSnapshotInput {
    input.processes = ProcessRegistry::from_sources([ProcessSource::new(
        "synthetic.feature",
        r#"@process(synthetic-quality)
@process-version(1)
@cg-language(1)
Feature: Synthetic quality lifecycle
Rule: Process
Given state START is initial
Given state END is terminal
Given event finish
Given event check
Given gate review
Given activity remediate requires capability project.declarative-change
Given activity remediate requires capability project.state-observation
Given activity verify requires capability project.quality-verification
Given activity verify requires capability project.state-observation
Given activity verify constrained by lifecycle=VERIFICATION_AFTER_CHANGE
Scenario: finish
Given process state START
When event finish occurs
Then transition to state END
Then authorize activity remediate
Then complete process
Scenario: check
Given process state START
When event check occurs
Then transition to state END
Then authorize activity verify
Then complete process
"#,
    )])
    .unwrap();
    input = support::with_process(input);
    let definition = input.processes.definitions().next().unwrap();
    let instance = input.instance.as_ref().unwrap();
    let mut wire: serde_json::Value = serde_json::from_str(&instance.to_json().unwrap()).unwrap();
    wire["active_gates"] = serde_json::json!({"review":if blocked {"BLOCKED"}else{"PASSED"}});
    let instance = ProcessInstance::from_json(&wire.to_string()).unwrap();
    input.situation_process = Some(
        DeclarativeSituationApplication::new()
            .process_reference(ProcessSnapshotInput::new(definition, &instance))
            .unwrap(),
    );
    input.expected_revision = Some(instance.revision());
    input.instance = Some(instance);
    input
}

#[test]
fn cg07_neutral_plan_resolves_end_to_end_with_deferred_verification_and_no_implicit_permission() {
    let input = pin(fixture(false, false), false);
    let rules = resolver_rules(&input);
    let app = DeclarativeResolutionApplication;
    assert_eq!(input.plan.steps().len(), 4);
    assert_eq!(input.plan.parallel_layers().unwrap().len(), 2);
    assert!(
        input
            .delta
            .items()
            .iter()
            .all(|d| d.kind() == DeltaKind::UnsatisfiedCondition
                && !d.basis().evidence().is_empty()
                && !d.basis().provenances().is_empty())
    );
    let planner_json = input.plan.to_json().unwrap();
    assert!(!planner_json.contains("fixture-agent"));
    assert!(!planner_json.contains("fixture-core"));
    let result = app.resolve_plan(&input, &rules).unwrap();
    assert_eq!(result.report.outcome, ResolutionOutcome::Resolved);
    assert_eq!(result.report.alternatives.len(), 1);
    for binding in &result.report.alternatives[0] {
        assert!(binding.binding.as_ref().unwrap().process.is_some());
        assert_eq!(
            binding
                .skills
                .as_ref()
                .unwrap()
                .skills
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
            ["fixture-leaf", "fixture-core"]
        );
        assert!(
            !binding
                .skills
                .as_ref()
                .unwrap()
                .skills
                .contains(&SkillId::new("fixture-related").unwrap())
        );
        let original = input
            .plan
            .steps()
            .iter()
            .find(|s| s.id() == &binding.step)
            .unwrap();
        assert_eq!(&binding.applicability.step, original);
        assert_eq!(
            binding.applicability.readiness,
            if original.kind() == PlanStepKind::Verification {
                LifecycleReadiness::Deferred
            } else {
                LifecycleReadiness::Eligible
            },
            "{:?}",
            binding.applicability.reasons
        );
    }
    let inspection = app.inspect_resolution(&result).unwrap();
    assert!(inspection.policy_inputs.iter().any(|p| {
        p.required_capabilities
            .values()
            .any(|c| c.class() == CapabilityClass::Mutate)
    }));
    assert!(inspection.policy_inputs.iter().all(|p| {
        !p.required_capabilities
            .contains_key(&CapabilityId::new("project.unrelated").unwrap())
    }));
    let trace = app
        .explain_resolution(
            &result,
            TraceLimits {
                max_nodes: 10000,
                max_optional_details: 100,
            },
        )
        .unwrap();
    assert_eq!(trace.policy_authorization, "NOT_EVALUATED");
    assert!(!trace.to_json().contains("rogue-agent"));
    assert!(
        !trace
            .to_text()
            .contains("sensitive coverage report content")
    );
    let artifact = app
        .serialize_resolution(&result, ArtifactLimits::default())
        .unwrap();
    assert_eq!(
        app.parse_resolution(&input, &rules, &artifact, ArtifactLimits::default())
            .unwrap(),
        result
    );
    assert_eq!(
        app.resolve_plan(&pin(fixture(true, false), false), &rules)
            .unwrap(),
        result
    );
    assert_eq!(app.resolve_plan(&input, &rules).unwrap(), result);
    assert_eq!(result.snapshot.input(), &input);
    let projection = app
        .inspect_v1_projection(
            &result,
            ProjectionInputs {
                step: &input.plan.steps()[0].id().clone(),
                mapping: None,
                catalog: &DefinitionCatalog::default(),
                context: None,
                policy: None,
            },
        )
        .unwrap();
    assert_eq!(projection.status, ProjectionStatus::Incompatible);
    assert!(
        projection
            .problems
            .contains(&ProjectionProblem::MissingOwnerMapping)
    );
    // CG08-PROJECTION-01 is deliberately not hidden by a fake workflow.
    let mut completed = rules.clone();
    for step in input
        .plan
        .steps()
        .iter()
        .filter(|s| s.kind() == PlanStepKind::Change)
    {
        completed.applicability.completed.insert(
            step.id().clone(),
            CompletionEvidence {
                basis: result.report.basis.clone(),
                contracts: std::iter::once(step.completion().clone())
                    .chain(step.verification().cloned())
                    .collect(),
                references: BTreeSet::from([
                    EvidenceId::new("explicit-completion-receipt").unwrap()
                ]),
                status: ConditionStatus::Satisfied,
                freshness: FreshnessStatus::Fresh,
            },
        );
    }
    let after = app.resolve_plan(&input, &completed).unwrap();
    assert!(
        after.report.alternatives[0]
            .iter()
            .all(|a| a.applicability.readiness == LifecycleReadiness::Eligible)
    );
}

#[test]
fn pinned_process_gates_stale_revision_scope_and_mixed_catalog_fail_closed() {
    let input = pin(fixture(false, false), true);
    let rules = resolver_rules(&input);
    let app = DeclarativeResolutionApplication;
    let before = input.instance.as_ref().unwrap().to_json().unwrap();
    let result = app.resolve_plan(&input, &rules).unwrap();
    assert_eq!(result.report.outcome, ResolutionOutcome::Resolved);
    assert!(
        result.report.alternatives[0]
            .iter()
            .all(|a| a.binding.as_ref().unwrap().process.is_some()
                && a.applicability.readiness == LifecycleReadiness::Blocked)
    );
    assert_eq!(before, input.instance.as_ref().unwrap().to_json().unwrap());
    let mut stale = input.clone();
    stale.expected_revision = Some(ProcessInstanceRevision::new(
        input.instance.as_ref().unwrap().revision().value() + 1,
    ));
    assert!(matches!(
        app.resolve_plan(&stale, &rules),
        Err(ResolutionApplicationError::Snapshot(
            SnapshotError::StaleRevision
        ))
    ));
    let mut wrong = input.clone();
    wrong.plan_scope = ContextScopeId::new("other").unwrap();
    assert!(matches!(
        app.resolve_plan(&wrong, &rules),
        Err(ResolutionApplicationError::Snapshot(
            SnapshotError::ScopeMismatch
        ))
    ));
    let mut mixed = input.clone();
    mixed.index = Registry::load(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog"))
        .unwrap()
        .capability_index()
        .unwrap();
    assert!(matches!(
        app.resolve_plan(&mixed, &rules),
        Err(ResolutionApplicationError::Snapshot(
            SnapshotError::MixedIndex
        ))
    ));
    let unblocked = pin(fixture(false, false), false);
    assert!(!app.basis_is_current(&result, &unblocked).unwrap());
    let noop = fixture(false, true);
    let result = app.resolve_plan(&noop, &resolver_rules(&noop)).unwrap();
    assert_eq!(result.report.outcome, ResolutionOutcome::NoOp);
    assert!(result.report.steps.is_empty());
}

#[test]
fn real_catalog_no_match_and_negative_resolution_variants_remain_honest() {
    let app = DeclarativeResolutionApplication;
    let without_lifecycle = fixture(false, false);
    // Actual CG-07 verification requires a lifecycle. Do not strip it to obtain no-template.
    assert_eq!(
        app.resolve_plan(&without_lifecycle, &composition::rules())
            .unwrap()
            .report
            .outcome,
        ResolutionOutcome::Unsupported
    );
    let input = pin(fixture(false, false), false);
    let mut rules = resolver_rules(&input);
    let mut real = input.clone();
    real.registry =
        Registry::load(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog")).unwrap();
    real.index = real.registry.capability_index().unwrap();
    let mut real_rules = resolver_rules(&real);
    real_rules.skills.clear();
    let missing = app.resolve_plan(&real, &real_rules).unwrap();
    assert_eq!(missing.report.outcome, ResolutionOutcome::Missing);
    assert!(
        missing
            .report
            .discovery
            .sets
            .iter()
            .all(|s| s.outcome == CandidateOutcome::UnknownCapability)
    );
    for selector in [
        CapabilitySelector::Class(CapabilityClass::Inspect),
        CapabilitySelector::InputKind(CapabilityInputKind::new("missing-input").unwrap()),
        CapabilitySelector::OutputKind(CapabilityOutputKind::new("missing-output").unwrap()),
    ] {
        let id = input
            .plan
            .steps()
            .iter()
            .find(|s| s.kind() == PlanStepKind::Change)
            .unwrap()
            .capability_requirements()[0]
            .clone();
        let mut r = rules.clone();
        r.candidates
            .selectors
            .insert(id, BTreeSet::from([selector]));
        let failure = app.resolve_plan(&input, &r).unwrap();
        assert_ne!(failure.report.outcome, ResolutionOutcome::Resolved);
        assert!(
            failure
                .report
                .discovery
                .sets
                .iter()
                .any(|s| s.outcome == CandidateOutcome::Incompatible)
        );
    }
    for sr in rules.skills.values_mut() {
        sr.conditions.insert(
            SkillId::new("fixture-core").unwrap(),
            SkillCondition::DesiredCondition(
                ConditionId::new(reference::COVERAGE_CONDITION).unwrap(),
            ),
        );
    }
    assert_ne!(
        app.resolve_plan(&input, &rules).unwrap().report.outcome,
        ResolutionOutcome::Resolved
    );
    let mut r = resolver_rules(&input);
    r.max_visits = 1;
    assert_eq!(
        app.resolve_plan(&input, &r).unwrap().report.outcome,
        ResolutionOutcome::SearchLimit
    );
    r.version = SchemaVersion::new(2, 0).unwrap();
    assert!(app.resolve_plan(&input, &r).is_err());
    // Reuse a separate synthetic fixture to prove dishonest source suggestions never add providers.
    let mut helper = composition::fixture();
    composition::edit_skill(
        &mut helper,
        "good",
        "required_capability_ids",
        serde_json::json!(["missing"]),
    );
    let mut helper_rules = composition::rules();
    helper_rules
        .provider_priorities
        .insert(composition::skill("good"), 100);
    helper_rules
        .provider_priorities
        .insert(composition::agent("alpha"), 10);
    let fallback = composition::run(&helper, &helper_rules);
    assert!(fallback.steps.iter().any(|s| !s.rejections.is_empty()));
    assert_eq!(fallback.outcome, ResolutionOutcome::Resolved);
}
