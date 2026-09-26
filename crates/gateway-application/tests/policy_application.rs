use gateway_application::{policy_application::*, resolution::*, resolution_application::*};
use gateway_domain::*;
use gateway_policy::*;
use gateway_process::*;
use std::collections::BTreeSet;
#[allow(dead_code)]
#[path = "support/composition.rs"]
mod composition;
mod support;

fn fixture() -> (ResolvedPlan, PolicyAuthority, PolicyContext) {
    let input = composition::fixture();
    let mut rules = composition::rules();
    rules
        .provider_priorities
        .insert(composition::skill("good"), 10);
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(&input, &rules)
        .unwrap();
    assert_eq!(resolved.report.outcome, ResolutionOutcome::Resolved);
    let authority = PolicyAuthority {
        capabilities: input
            .index
            .entries()
            .map(|e| (e.id().clone(), e.capability().clone()))
            .collect(),
        policies: vec![
            PolicyDefinition::new(
                PolicyId::new("safe").unwrap(),
                "safe",
                input.index.ids().cloned(),
            )
            .unwrap(),
        ],
        ..Default::default()
    };
    let facts = StepFacts {
        authorizations: authority
            .capabilities
            .keys()
            .map(|id| (id.clone(), Approval::Granted))
            .collect(),
        evidence: authority
            .capabilities
            .values()
            .flat_map(|c| c.preconditions().iter().map(ToString::to_string))
            .collect(),
        satisfied_constraints: authority
            .capabilities
            .values()
            .flat_map(|c| c.constraints().iter().map(ToString::to_string))
            .collect(),
        prerequisites_satisfied: true,
        ..Default::default()
    };
    let context = PolicyContext {
        basis: resolved.report.basis.clone(),
        operating_mode: input.operating_mode,
        execution_profile: input.execution_profile,
        steps: input
            .plan
            .steps()
            .iter()
            .map(|s| (s.id().clone(), facts.clone()))
            .collect(),
    };
    (resolved, authority, context)
}
#[test]
fn deterministic_plan_policy_and_serializable_step_evidence() {
    let (resolved, authority, context) = fixture();
    let report = PolicyApplication
        .evaluate(&resolved, &authority, &context)
        .unwrap();
    assert_eq!(report.decision(), PolicyDecision::Allow);
    assert_eq!(
        report,
        PolicyApplication
            .evaluate(&resolved, &authority, &context)
            .unwrap()
    );
    assert_eq!(report.basis(), &context.basis);
    let json: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
    assert_eq!(json["decision"], "ALLOW");
    assert_eq!(json["basis"]["scope"], "project-a");
    assert_eq!(
        report.steps().len(),
        resolved.snapshot.request().plan().steps().len()
    );
    for step in report.steps().values() {
        assert!(serde_json::to_string(step).unwrap().contains("INSPECT"));
    }
}
#[test]
fn desired_goals_never_override_deny_and_missing_facts_block() {
    let (resolved, mut authority, mut context) = fixture();
    context.steps.clear();
    assert_eq!(
        PolicyApplication
            .evaluate(&resolved, &authority, &context)
            .unwrap()
            .decision(),
        PolicyDecision::RequireConsent
    );
    let (_, _, context) = fixture();
    authority.policies.push(
        PolicyDefinition::with_denied_capabilities(
            PolicyId::new("forbidden").unwrap(),
            "forbidden",
            [],
            authority.capabilities.keys().cloned(),
        )
        .unwrap(),
    );
    let report = PolicyApplication
        .evaluate(&resolved, &authority, &context)
        .unwrap();
    assert_eq!(report.decision(), PolicyDecision::Deny);
    assert!(report.steps().values().all(|s| {
        s.findings
            .iter()
            .any(|f| f.reason == PolicyReason::ExplicitDeny)
    }));
}
#[test]
fn stale_scope_mode_profile_and_unknown_step_are_rejected() {
    let (resolved, authority, mut context) = fixture();
    context.basis.scope = ContextScopeId::new("other").unwrap();
    assert_eq!(
        PolicyApplication.evaluate(&resolved, &authority, &context),
        Err(PolicyApplicationError::StaleContext)
    );
    context.basis = resolved.report.basis.clone();
    context.operating_mode = OperatingMode::Development;
    assert_eq!(
        PolicyApplication.evaluate(&resolved, &authority, &context),
        Err(PolicyApplicationError::StaleContext)
    );
    context.operating_mode = resolved.snapshot.input().operating_mode;
    context.execution_profile = ExecutionProfile::FastPath;
    assert_eq!(
        PolicyApplication.evaluate(&resolved, &authority, &context),
        Err(PolicyApplicationError::StaleContext)
    );
    context.execution_profile = resolved.snapshot.input().execution_profile;
    context
        .steps
        .insert(PlanStepId::new("foreign").unwrap(), StepFacts::default());
    assert_eq!(
        PolicyApplication.evaluate(&resolved, &authority, &context),
        Err(PolicyApplicationError::UnknownStep)
    );
}
#[test]
fn forged_resolution_and_ambiguous_bindings_cannot_authorize() {
    let (mut resolved, authority, mut context) = fixture();
    resolved.report.visits += 1;
    assert!(matches!(
        PolicyApplication.evaluate(&resolved, &authority, &context),
        Err(PolicyApplicationError::Resolution(_))
    ));
    let input = composition::fixture();
    let ambiguous = DeclarativeResolutionApplication
        .resolve_plan(&input, &composition::rules())
        .unwrap();
    context.basis = ambiguous.report.basis.clone();
    let report = PolicyApplication
        .evaluate(&ambiguous, &authority, &context)
        .unwrap();
    assert_eq!(report.decision(), PolicyDecision::Deny);
    assert!(report.steps().values().all(|s| {
        s.findings
            .iter()
            .any(|f| f.reason == PolicyReason::Unresolved)
    }));
}
fn process() -> (
    ProcessDefinition,
    ProcessInstance,
    EventOccurrence,
    PolicyDecisionId,
) {
    let gate = PolicyDecisionId::new("cg09").unwrap();
    let definition = ProcessDefinitionBuilder::new(
        ProcessDefinitionId::new("test").unwrap(),
        ProcessDefinitionVersion::new(1).unwrap(),
    )
    .with_states([
        StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
        StateDefinition::new(StateId::new("end").unwrap(), false, true).unwrap(),
    ])
    .with_events([EventTypeDefinition::new(
        EventTypeId::new("finish").unwrap(),
    )])
    .with_transitions([TransitionDefinition::new(
        TransitionId::new("finish").unwrap(),
        StateId::new("start").unwrap(),
        EventTypeId::new("finish").unwrap(),
        StateId::new("end").unwrap(),
        GuardExpression::PolicyDecisionIs {
            policy: gate.clone(),
            status: PolicyDecisionStatus::Allow,
        },
    )])
    .build()
    .unwrap();
    let instance =
        ProcessInstance::start(&definition, ProcessInstanceId::new("instance").unwrap()).unwrap();
    let event = EventOccurrence::new(
        EventOccurrenceId::new("finish-1").unwrap(),
        EventTypeId::new("finish").unwrap(),
        instance.id().clone(),
        instance.revision(),
    );
    (definition, instance, event, gate)
}
#[test]
fn policy_gates_transitions_without_overriding_process_blockers() {
    let (resolved, mut authority, mut context) = fixture();
    let (definition, instance, event, gate) = process();
    let step = resolved.snapshot.request().plan().steps()[0].id();
    for (decision, expected) in [
        (PolicyDecision::Allow, TransitionDecisionCode::Accepted),
        (
            PolicyDecision::RequireEvidence,
            TransitionDecisionCode::WaitingForAuthorization,
        ),
        (
            PolicyDecision::RequireConsent,
            TransitionDecisionCode::WaitingForAuthorization,
        ),
        (
            PolicyDecision::Deny,
            TransitionDecisionCode::AuthorizationDenied,
        ),
    ] {
        if decision == PolicyDecision::RequireEvidence {
            authority.required_evidence = authority
                .capabilities
                .keys()
                .map(|id| (id.clone(), BTreeSet::from(["review".into()])))
                .collect();
        }
        if decision == PolicyDecision::RequireConsent {
            context.steps.clear();
        }
        if decision == PolicyDecision::Deny {
            authority.policies.clear();
        }
        let report = PolicyApplication
            .evaluate(&resolved, &authority, &context)
            .unwrap();
        assert_eq!(report.decision(), decision);
        let inputs = report
            .gate_inputs(
                &context.basis,
                step,
                gate.clone(),
                EvaluationInputs::default(),
            )
            .unwrap();
        assert_eq!(
            TransitionEvaluator::evaluate(&definition, &instance, &event, &inputs).code(),
            expected
        );
        if decision == PolicyDecision::Allow {
            let mut blocked = instance.clone();
            blocked.record_blocker(
                BlockerRuntimeState::new(BlockerId::new("incident").unwrap(), "incident", true)
                    .unwrap(),
            );
            assert_eq!(
                TransitionEvaluator::evaluate(&definition, &blocked, &event, &inputs).code(),
                TransitionDecisionCode::ActiveBlocker
            );
        }
    }
    assert_eq!(instance.revision(), ProcessInstanceRevision::initial());
}
#[test]
fn process_mapping_rejects_stale_reports_unknown_steps_and_preserves_deny() {
    let (resolved, authority, context) = fixture();
    let report = PolicyApplication
        .evaluate(&resolved, &authority, &context)
        .unwrap();
    let step = resolved.snapshot.request().plan().steps()[0].id();
    let gate = PolicyDecisionId::new("gate").unwrap();
    for status in [
        PolicyDecisionStatus::Deny,
        PolicyDecisionStatus::Waiting,
        PolicyDecisionStatus::Allow,
    ] {
        let inputs = report
            .gate_inputs(
                &context.basis,
                step,
                gate.clone(),
                EvaluationInputs::default().with_policy_decision(gate.clone(), status),
            )
            .unwrap();
        assert_eq!(inputs.policy().decisions()[&gate], status);
    }
    let mut stale = context.basis.clone();
    stale.process_state_fingerprint = ContentFingerprint::of_bytes(b"new revision");
    assert_eq!(
        report.gate_inputs(&stale, step, gate.clone(), EvaluationInputs::default()),
        Err(PolicyApplicationError::StaleContext)
    );
    assert_eq!(
        report.gate_inputs(
            &context.basis,
            &PlanStepId::new("foreign").unwrap(),
            gate,
            EvaluationInputs::default()
        ),
        Err(PolicyApplicationError::UnknownStep)
    );
}

#[test]
fn process_constraints_and_paused_state_remain_policy_blockers() {
    use gateway_application::{DeclarativeSituationApplication, ProcessSnapshotInput};
    let (_, authority, mut context) = fixture();
    let mut input = composition::fixture();
    input.processes = ProcessRegistry::from_sources([ProcessSource::new("synthetic.feature", "@process(synthetic)\n@process-version(1)\n@cg-language(1)\nFeature: Policy integration\nRule: Process\nGiven state START is initial\nGiven state END is terminal\nGiven event finish\nGiven activity inspect requires capability architecture.dependency-analysis\nGiven activity inspect constrained by primary-agent=alpha\nScenario: finish\nGiven process state START\nWhen event finish occurs\nThen transition to state END\nThen authorize activity inspect\nThen complete process\n")]).unwrap();
    let mut input = support::with_process(input);
    let mut rules = composition::rules();
    rules
        .provider_priorities
        .insert(composition::skill("good"), 10);
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(&input, &rules)
        .unwrap();
    context.basis = resolved.report.basis.clone();
    let report = PolicyApplication
        .evaluate(&resolved, &authority, &context)
        .unwrap();
    assert_eq!(report.decision(), PolicyDecision::RequireEvidence);
    assert!(report.steps().values().any(|s| {
        s.findings
            .iter()
            .any(|f| f.subject == "[\"primary-agent\",\"alpha\"]")
    }));
    for facts in context.steps.values_mut() {
        facts
            .satisfied_constraints
            .insert("[\"primary-agent\",\"alpha\"]".into());
    }
    assert_eq!(
        PolicyApplication
            .evaluate(&resolved, &authority, &context)
            .unwrap()
            .decision(),
        PolicyDecision::Allow
    );
    LifecycleController::pause(
        input.instance.as_mut().unwrap(),
        PauseReason::HumanReview,
        "review",
    )
    .unwrap();
    input.expected_revision = Some(input.instance.as_ref().unwrap().revision());
    input.situation_process = Some(
        DeclarativeSituationApplication::new()
            .process_reference(ProcessSnapshotInput::new(
                input.processes.definitions().next().unwrap(),
                input.instance.as_ref().unwrap(),
            ))
            .unwrap(),
    );
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(&input, &rules)
        .unwrap();
    context.basis = resolved.report.basis.clone();
    let report = PolicyApplication
        .evaluate(&resolved, &authority, &context)
        .unwrap();
    assert_eq!(report.decision(), PolicyDecision::Deny);
    assert!(report.steps().values().any(|s| {
        s.findings
            .iter()
            .any(|f| f.reason == PolicyReason::ProcessBlocked)
    }));
}

#[test]
fn noop_has_no_capability_authorization_side_effect() {
    let mut input = composition::fixture();
    let old = &input.delta.items()[0];
    input.delta = Delta::new(
        input.delta.id().clone(),
        input.desired.id().clone(),
        Some(input.situation.situation().id().clone()),
        vec![
            DeltaItem::new(
                old.id().clone(),
                input.desired.id().clone(),
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
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.desired.id().clone(),
        input.delta.id().clone(),
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
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(&input, &composition::rules())
        .unwrap();
    let context = PolicyContext {
        basis: resolved.report.basis.clone(),
        operating_mode: input.operating_mode,
        execution_profile: input.execution_profile,
        steps: Default::default(),
    };
    let report = PolicyApplication
        .evaluate(&resolved, &PolicyAuthority::default(), &context)
        .unwrap();
    assert_eq!(report.decision(), PolicyDecision::Allow);
    assert!(
        report
            .steps()
            .values()
            .all(|s| s.capability_classes.is_empty())
    );
}

#[test]
fn capabilities_added_by_skill_closure_cannot_escape_policy() {
    let (_, mut authority, mut context) = fixture();
    let mut input = composition::fixture();
    composition::edit_skill(
        &mut input,
        "good",
        "required_capability_ids",
        serde_json::json!(["nested"]),
    );
    let mut rules = composition::rules();
    rules
        .provider_priorities
        .insert(composition::skill("good"), 100);
    let mut skills = composition::skill_rules();
    skills.capability_providers.insert(
        CapabilityId::new("nested").unwrap(),
        composition::agent("alpha"),
    );
    rules
        .skills
        .insert(input.plan.steps()[0].id().clone(), skills);
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(&input, &rules)
        .unwrap();
    assert_eq!(resolved.report.outcome, ResolutionOutcome::Resolved);
    context.basis = resolved.report.basis.clone();
    authority.policies = vec![
        PolicyDefinition::with_denied_capabilities(
            PolicyId::new("safe").unwrap(),
            "deny extra capability",
            [CapabilityId::new("architecture.dependency-analysis").unwrap()],
            [CapabilityId::new("nested").unwrap()],
        )
        .unwrap(),
    ];
    let report = PolicyApplication
        .evaluate(&resolved, &authority, &context)
        .unwrap();
    assert_eq!(report.decision(), PolicyDecision::Deny);
    assert!(report.steps().values().all(|s| {
        s.capability_classes
            .contains_key(&CapabilityId::new("nested").unwrap())
    }));
}

#[test]
fn desired_state_restrictions_only_narrow_authorization() {
    let (_, mut authority, mut context) = fixture();
    let mut input = composition::fixture();
    let desired = &input.desired;
    input.desired = DesiredState::new(
        desired.id().clone(),
        desired.conditions().to_vec(),
        desired.expression().clone(),
        vec![DeclarativeConstraint::new(
            ConstraintId::new("stay-safe").unwrap(),
            desired.expression().clone(),
        )],
        vec![],
    )
    .unwrap();
    input.situation = DeclarativeContextSituationDocument::new(
        DeclarativeContext::new_v1(DeclarativeContextId::new("context").unwrap()),
        Some(Intent::new(
            IntentId::new("intent").unwrap(),
            input.desired.clone(),
        )),
        None,
        input.situation.observed_state().clone(),
        input.situation.situation().clone(),
    )
    .unwrap();
    let mut rules = composition::rules();
    rules
        .provider_priorities
        .insert(composition::skill("good"), 10);
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(&input, &rules)
        .unwrap();
    context.basis = resolved.report.basis.clone();
    let report = PolicyApplication
        .evaluate(&resolved, &authority, &context)
        .unwrap();
    assert_eq!(report.decision(), PolicyDecision::RequireEvidence);
    assert!(
        report
            .steps()
            .values()
            .any(|s| s.findings.iter().any(|f| f.subject == "desired:stay-safe"))
    );
    for facts in context.steps.values_mut() {
        facts
            .satisfied_constraints
            .insert("desired:stay-safe".into());
    }
    assert_eq!(
        PolicyApplication
            .evaluate(&resolved, &authority, &context)
            .unwrap()
            .decision(),
        PolicyDecision::Allow
    );
    authority.policies.clear();
    assert_eq!(
        PolicyApplication
            .evaluate(&resolved, &authority, &context)
            .unwrap()
            .decision(),
        PolicyDecision::Deny
    );
}
