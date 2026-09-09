use gateway_application::{
    resolution::*, resolution_application::*, resolution_artifact::*, resolution_composition::*,
    resolution_explain::TraceLimits, resolution_snapshot::*,
};
use gateway_domain::*;
use gateway_policy::PolicyDecision;
use gateway_process::{ProcessRegistry, ProcessSource};
use std::collections::BTreeSet;
#[path = "support/composition.rs"]
mod composition;
mod support;

fn input() -> ResolutionSnapshotInput {
    let mut input = composition::fixture();
    input.processes=ProcessRegistry::from_sources([ProcessSource::new("synthetic.feature","@process(synthetic)\n@process-version(1)\n@cg-language(1)\nFeature: Synthetic CG02 compatibility\nRule: Process\nGiven state START is initial\nGiven state END is terminal\nGiven event finish\nGiven activity inspect requires capability architecture.dependency-analysis\nGiven activity inspect constrained by primary-agent=alpha\nScenario: finish\nGiven process state START\nWhen event finish occurs\nThen transition to state END\nThen authorize activity inspect\nThen complete process\n")]).unwrap();
    support::with_process(input)
}
fn rules() -> CompositionRules {
    let mut r = composition::rules();
    r.provider_priorities.insert(composition::skill("good"), 10);
    r
}
fn catalog(input: &ResolutionSnapshotInput) -> DefinitionCatalog {
    DefinitionCatalog::new(
        input
            .registry
            .agents()
            .iter()
            .map(|a| a.to_domain())
            .collect(),
        input
            .registry
            .skills()
            .iter()
            .map(|s| s.to_domain())
            .collect(),
        vec![
            WorkflowDefinition::new(
                WorkflowId::new("synthetic-workflow").unwrap(),
                "explicit fixture mapping",
                AgentId::new("alpha").unwrap(),
                [SkillId::new("good").unwrap()],
                PolicyId::new("fixture-policy").unwrap(),
            )
            .unwrap(),
        ],
        vec![
            PolicyDefinition::new(
                PolicyId::new("fixture-policy").unwrap(),
                "explicit test policy",
                [
                    CapabilityId::new("architecture.dependency-analysis").unwrap(),
                    CapabilityId::new("nested").unwrap(),
                ],
            )
            .unwrap(),
        ],
    )
    .unwrap()
}
fn context(mode: OperatingMode, skills: &[&str], approved: &[&str]) -> ExecutionContextIR {
    ExecutionContextIR::new_v1(
        ExecutionContextId::new("fixture-context").unwrap(),
        TaskDescriptor::new(TaskId::new("fixture-task").unwrap(), "inspect architecture").unwrap(),
        WorkflowId::new("synthetic-workflow").unwrap(),
        AgentId::new("alpha").unwrap(),
        skills.iter().map(|s| SkillId::new(*s).unwrap()),
        mode,
        ExecutionProfile::FullPath,
        ExecutionState::new(
            WorkflowState::Running,
            GateState::Pending,
            BlockerState::Clear,
        )
        .unwrap(),
        PolicyId::new("fixture-policy").unwrap(),
        vec![],
        approved.iter().map(|c| CapabilityId::new(*c).unwrap()),
        vec![],
        ExecutionRuntimeId::new("fixture-runtime").unwrap(),
    )
    .unwrap()
}
fn mapping(resolved: &ResolvedPlan) -> WorkflowProjectionMapping {
    WorkflowProjectionMapping {
        basis: resolved.report.basis.clone(),
        step: resolved.report.steps[0].step.clone(),
        task: TaskId::new("fixture-task").unwrap(),
        process: resolved.report.alternatives[0][0]
            .binding
            .as_ref()
            .unwrap()
            .process
            .as_ref()
            .unwrap()
            .definition
            .clone(),
        workflow: WorkflowId::new("synthetic-workflow").unwrap(),
        decision_reference: ReferenceId::new("synthetic-cg02-cg10-mapping-decision").unwrap(),
    }
}
fn policy(resolved: &ResolvedPlan) -> ExternalPolicyResult {
    ExternalPolicyResult {
        basis: resolved.report.basis.clone(),
        step: resolved.report.steps[0].step.clone(),
        policy: PolicyId::new("fixture-policy").unwrap(),
        decision: PolicyDecision::Allow,
        approved_capabilities: BTreeSet::from([CapabilityId::new(
            "architecture.dependency-analysis",
        )
        .unwrap()]),
        decision_reference: ReferenceId::new("external-fixture-policy-result").unwrap(),
    }
}
fn project(
    resolved: &ResolvedPlan,
    catalog: &DefinitionCatalog,
    mapping: Option<&WorkflowProjectionMapping>,
    context: Option<&ExecutionContextIR>,
    policy: Option<&ExternalPolicyResult>,
) -> ProjectionCompatibility {
    DeclarativeResolutionApplication
        .inspect_v1_projection(
            resolved,
            ProjectionInputs {
                step: &resolved.report.steps[0].step,
                mapping,
                catalog,
                context,
                policy,
            },
        )
        .unwrap()
}

#[test]
fn real_cg02_constructors_and_validation_prove_one_process_bound_projection() {
    let input = input();
    let app = DeclarativeResolutionApplication;
    let resolved = app.resolve_plan(&input, &rules()).unwrap();
    assert_eq!(resolved.report.outcome, ResolutionOutcome::Resolved);
    assert_eq!(resolved.report, composition::run(&input, &rules()));
    let catalog = catalog(&input);
    let context = context(
        OperatingMode::Hardening,
        &["good"],
        &["architecture.dependency-analysis"],
    );
    context.validate_against(&catalog).unwrap();
    let mapping = mapping(&resolved);
    let policy = policy(&resolved);
    let result = project(
        &resolved,
        &catalog,
        Some(&mapping),
        Some(&context),
        Some(&policy),
    );
    assert_eq!(result.status, ProjectionStatus::CompatibleV1Shape);
    assert!(result.problems.is_empty());
    assert_eq!(result.basis, resolved.report.basis);
    let inspection = app.inspect_resolution(&resolved).unwrap();
    assert_eq!(inspection.plan, input.plan);
    assert_eq!(inspection.report, resolved.report);
    let chosen = inspection
        .policy_inputs
        .iter()
        .find(|p| p.alternative == resolved.report.alternatives[0][0])
        .unwrap();
    assert_eq!(
        chosen.required_capabilities
            [&CapabilityId::new("architecture.dependency-analysis").unwrap()]
            .class(),
        CapabilityClass::Inspect
    );
    assert!(!chosen.process_constraints.is_empty());
    assert_eq!(
        chosen.alternative.binding.as_ref().unwrap().primary_agent,
        AgentId::new("alpha").unwrap()
    );
    let text = app
        .serialize_resolution(&resolved, ArtifactLimits::default())
        .unwrap();
    assert_eq!(
        app.parse_resolution(&input, &rules(), &text, ArtifactLimits::default())
            .unwrap(),
        resolved
    );
    let trace = app
        .explain_resolution(
            &resolved,
            TraceLimits {
                max_nodes: 1000,
                max_optional_details: 0,
            },
        )
        .unwrap();
    assert_eq!(trace.policy_authorization, "NOT_EVALUATED");
    assert!(app.basis_is_current(&resolved, &input).unwrap());
    let mut changed = input.clone();
    changed.execution_profile = ExecutionProfile::NormalPath;
    assert!(!app.basis_is_current(&resolved, &changed).unwrap());
    assert_eq!(
        input.instance.as_ref().unwrap().revision(),
        resolved
            .snapshot
            .input()
            .instance
            .as_ref()
            .unwrap()
            .revision()
    );
}

#[test]
fn external_policy_is_required_and_cannot_be_invented_from_readiness() {
    let input = input();
    let app = DeclarativeResolutionApplication;
    let resolved = app.resolve_plan(&input, &rules()).unwrap();
    let catalog = catalog(&input);
    let context = context(
        OperatingMode::Hardening,
        &["good"],
        &["architecture.dependency-analysis"],
    );
    let mapping = mapping(&resolved);
    let approved = policy(&resolved);
    assert!(
        project(&resolved, &catalog, Some(&mapping), Some(&context), None)
            .problems
            .contains(&ProjectionProblem::MissingPolicy)
    );
    for decision in [PolicyDecision::Deny, PolicyDecision::RequireConsent] {
        let mut p = approved.clone();
        p.decision = decision;
        assert!(
            project(
                &resolved,
                &catalog,
                Some(&mapping),
                Some(&context),
                Some(&p)
            )
            .problems
            .contains(&ProjectionProblem::PolicyNotApproved)
        );
    }
    let mut p = approved.clone();
    p.approved_capabilities.clear();
    assert!(
        project(
            &resolved,
            &catalog,
            Some(&mapping),
            Some(&context),
            Some(&p)
        )
        .problems
        .contains(&ProjectionProblem::InsufficientApprovals)
    );
    p = approved.clone();
    p.basis.scope = ContextScopeId::new("other").unwrap();
    assert!(
        project(
            &resolved,
            &catalog,
            Some(&mapping),
            Some(&context),
            Some(&p)
        )
        .problems
        .contains(&ProjectionProblem::StalePolicy)
    );
    p = approved;
    p.approved_capabilities
        .insert(CapabilityId::new("nested").unwrap());
    assert!(
        project(
            &resolved,
            &catalog,
            Some(&mapping),
            Some(&context),
            Some(&p)
        )
        .problems
        .contains(&ProjectionProblem::UnmappedConstraints)
    );
}

#[test]
fn no_template_empty_skills_multiple_agents_and_catalog_mismatch_are_explicit() {
    let app = DeclarativeResolutionApplication;
    let initial = input();
    let catalog = catalog(&initial);
    let no_template = app.resolve_plan(&composition::fixture(), &rules()).unwrap();
    let result = project(&no_template, &catalog, None, None, None);
    assert_eq!(result.status, ProjectionStatus::Incompatible);
    assert!(result.problems.contains(&ProjectionProblem::NoTemplate));
    assert!(
        result
            .problems
            .contains(&ProjectionProblem::MissingOwnerMapping)
    );
    assert!(
        result
            .problems
            .contains(&ProjectionProblem::MissingExternalContext)
    );
    let mut r = composition::rules();
    r.provider_priorities
        .insert(composition::agent("alpha"), 10);
    let empty = app.resolve_plan(&initial, &r).unwrap();
    assert!(
        project(&empty, &catalog, None, None, None)
            .problems
            .contains(&ProjectionProblem::EmptySkills)
    );
    r = composition::rules();
    r.provider_priorities.insert(composition::skill("bad"), 10);
    r.agents.participants.insert(
        initial.plan.steps()[0].id().clone(),
        BTreeSet::from([AgentId::new("beta").unwrap()]),
    );
    let multi = app.resolve_plan(&initial, &r).unwrap();
    assert_eq!(multi.report.outcome, ResolutionOutcome::Resolved);
    assert!(
        project(&multi, &catalog, None, None, None)
            .problems
            .contains(&ProjectionProblem::MultipleAgents)
    );
    let resolved = app.resolve_plan(&initial, &rules()).unwrap();
    let mut mapping = mapping(&resolved);
    let policy = policy(&resolved);
    let ir = context(
        OperatingMode::Hardening,
        &["good"],
        &["architecture.dependency-analysis"],
    );
    mapping.basis.scope = ContextScopeId::new("other").unwrap();
    assert!(
        project(
            &resolved,
            &catalog,
            Some(&mapping),
            Some(&ir),
            Some(&policy)
        )
        .problems
        .contains(&ProjectionProblem::StaleMapping)
    );
    mapping.basis = resolved.report.basis.clone();
    mapping.workflow = WorkflowId::new("missing").unwrap();
    assert!(
        project(
            &resolved,
            &catalog,
            Some(&mapping),
            Some(&ir),
            Some(&policy)
        )
        .problems
        .contains(&ProjectionProblem::WorkflowMismatch)
    );
    mapping.workflow = WorkflowId::new("synthetic-workflow").unwrap();
    let wrong_mode = context(
        OperatingMode::Development,
        &["good"],
        &["architecture.dependency-analysis"],
    );
    assert!(
        project(
            &resolved,
            &catalog,
            Some(&mapping),
            Some(&wrong_mode),
            Some(&policy)
        )
        .problems
        .contains(&ProjectionProblem::ContextMismatch)
    );
    let wrong_skill = context(
        OperatingMode::Hardening,
        &["bad"],
        &["architecture.dependency-analysis"],
    );
    assert!(
        project(
            &resolved,
            &catalog,
            Some(&mapping),
            Some(&wrong_skill),
            Some(&policy)
        )
        .problems
        .contains(&ProjectionProblem::CatalogMismatch)
    );
    assert!(
        project(
            &resolved,
            &DefinitionCatalog::default(),
            Some(&mapping),
            Some(&ir),
            Some(&policy)
        )
        .problems
        .contains(&ProjectionProblem::CatalogMismatch)
    );
    let ambiguous = app.resolve_plan(&initial, &composition::rules()).unwrap();
    assert!(
        project(&ambiguous, &catalog, None, None, None)
            .problems
            .contains(&ProjectionProblem::Unresolved)
    );
}

struct Unavailable;
impl ResolutionSnapshotPort for Unavailable {
    fn capture(&self) -> Result<ResolutionSnapshotInput, SnapshotError> {
        Err(SnapshotError::InputUnavailable)
    }
}

#[test]
fn facade_failures_and_nested_policy_inputs_do_not_mutate_or_drop_constraints() {
    let app = DeclarativeResolutionApplication;
    assert!(matches!(
        app.resolve_plan(&Unavailable, &rules()),
        Err(ResolutionApplicationError::Snapshot(_))
    ));
    let mut r = rules();
    r.version = SchemaVersion::new(2, 0).unwrap();
    assert!(matches!(
        app.resolve_plan(&input(), &r),
        Err(ResolutionApplicationError::Composition(_))
    ));
    let mut fixture = composition::fixture();
    composition::edit_skill(
        &mut fixture,
        "good",
        "required_capability_ids",
        json!(["nested"]),
    );
    let mut r = rules();
    let mut sr = composition::skill_rules();
    sr.capability_providers.insert(
        CapabilityId::new("nested").unwrap(),
        composition::agent("alpha"),
    );
    r.skills.insert(fixture.plan.steps()[0].id().clone(), sr);
    let mut resolved = app.resolve_plan(&fixture, &r).unwrap();
    let inspected = app.inspect_resolution(&resolved).unwrap();
    assert!(inspected.policy_inputs.iter().any(|p| {
        p.required_capabilities
            .contains_key(&CapabilityId::new("nested").unwrap())
    }));
    assert!(matches!(
        app.inspect_v1_projection(
            &resolved,
            ProjectionInputs {
                step: &PlanStepId::new("absent").unwrap(),
                mapping: None,
                catalog: &DefinitionCatalog::default(),
                context: None,
                policy: None
            }
        ),
        Err(ResolutionApplicationError::UnknownStep)
    ));
    assert!(matches!(
        app.parse_resolution(&Unavailable, &r, "{}", ArtifactLimits::default()),
        Err(ResolutionApplicationError::Snapshot(_))
    ));
    assert!(matches!(
        app.parse_resolution(&fixture, &r, "{", ArtifactLimits::default()),
        Err(ResolutionApplicationError::Artifact(_))
    ));
    assert!(matches!(
        app.basis_is_current(&resolved, &Unavailable),
        Err(ResolutionApplicationError::Snapshot(_))
    ));
    assert!(matches!(
        app.explain_resolution(
            &resolved,
            TraceLimits {
                max_nodes: 0,
                max_optional_details: 0
            }
        ),
        Err(ResolutionApplicationError::Trace(_))
    ));
    assert!(matches!(
        app.serialize_resolution(
            &resolved,
            ArtifactLimits {
                max_bytes: 0,
                ..ArtifactLimits::default()
            }
        ),
        Err(ResolutionApplicationError::Artifact(_))
    ));
    resolved.report.outcome = ResolutionOutcome::Missing;
    assert!(app.inspect_resolution(&resolved).is_err());
    assert!(app.basis_is_current(&resolved, &fixture).is_err());
}

use serde_json::json;

#[test]
fn paused_and_noop_work_never_becomes_an_execution_context() {
    use gateway_application::{DeclarativeSituationApplication, ProcessSnapshotInput};
    use gateway_process::{LifecycleController, PauseReason};
    let mut fixture = input();
    let app = DeclarativeResolutionApplication;
    LifecycleController::pause(
        fixture.instance.as_mut().unwrap(),
        PauseReason::HumanReview,
        "synthetic review",
    )
    .unwrap();
    fixture.expected_revision = Some(fixture.instance.as_ref().unwrap().revision());
    fixture.situation_process = Some(
        DeclarativeSituationApplication::new()
            .process_reference(ProcessSnapshotInput::new(
                fixture.processes.definitions().next().unwrap(),
                fixture.instance.as_ref().unwrap(),
            ))
            .unwrap(),
    );
    let resolved = app.resolve_plan(&fixture, &rules()).unwrap();
    assert!(
        project(&resolved, &catalog(&fixture), None, None, None)
            .problems
            .contains(&ProjectionProblem::NotCurrentlyEligible)
    );
    let mut noop = composition::fixture();
    let old = &noop.delta.items()[0];
    noop.delta = Delta::new(
        noop.delta.id().clone(),
        noop.desired.id().clone(),
        Some(noop.situation.situation().id().clone()),
        vec![
            DeltaItem::new(
                old.id().clone(),
                noop.desired.id().clone(),
                old.condition().clone(),
                DeltaKind::Satisfied,
                old.basis().clone(),
                RequiredOutcome::new(RequiredOutcomeKind::NoOp, "satisfied").unwrap(),
                "satisfied",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let outcome = RequiredOutcome::new(RequiredOutcomeKind::NoOp, "satisfied").unwrap();
    noop.plan = Plan::new(
        noop.plan.id().clone(),
        noop.desired.id().clone(),
        noop.delta.id().clone(),
        vec![],
        vec![
            PlanStep::new(
                PlanStepId::new("noop").unwrap(),
                PlanStepKind::NoOp,
                outcome.clone(),
                PlanCondition::outcome(outcome),
                "satisfied",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let resolved = app.resolve_plan(&noop, &rules()).unwrap();
    assert_eq!(
        project(&resolved, &DefinitionCatalog::default(), None, None, None).status,
        ProjectionStatus::NoWork
    );
    let pinned = support::with_process(noop);
    let resolved = app.resolve_plan(&pinned, &rules()).unwrap();
    assert_eq!(resolved.report.steps[0].outcome, ResolutionOutcome::NoOp);
    assert!(resolved.report.alternatives[0][0].binding.is_none());
}

#[test]
fn intrinsic_constraint_without_v1_mapping_is_not_silently_discarded() {
    let mut fixture = input();
    let edit = |text: String| {
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        for capability in value["provided_capabilities"].as_array_mut().unwrap() {
            capability["constraints"]
                .as_array_mut()
                .unwrap()
                .push(json!("special-restriction"));
        }
        value.to_string()
    };
    let agents = fixture
        .registry
        .agents()
        .iter()
        .map(|a| AgentDefinitionDocument::from_json(&edit(a.to_json().unwrap())).unwrap())
        .collect::<Vec<_>>();
    let skills = fixture
        .registry
        .skills()
        .iter()
        .map(|s| SkillDefinitionDocument::from_json(&edit(s.to_json().unwrap())).unwrap())
        .collect::<Vec<_>>();
    fixture.registry = gateway_registry::Registry::from_documents(agents, skills).unwrap();
    fixture.index = fixture.registry.capability_index().unwrap();
    let mut r = rules();
    r.applicability.semantics.insert(
        "special-restriction".into(),
        gateway_application::resolution_skills::SkillCondition::Always,
    );
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(&fixture, &r)
        .unwrap();
    assert_eq!(resolved.report.outcome, ResolutionOutcome::Resolved);
    assert!(
        project(&resolved, &catalog(&fixture), None, None, None)
            .problems
            .contains(&ProjectionProblem::UnmappedConstraints)
    );
}
