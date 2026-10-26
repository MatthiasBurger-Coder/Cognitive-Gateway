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
    for step in ambiguous.snapshot.request().plan().steps() {
        let native = PolicyApplication
            .step_input(&ambiguous, &context, step.id())
            .unwrap();
        assert!(!native.resolved);
        assert!(
            native.capabilities.is_empty(),
            "no alternative can masquerade as a canonical selection"
        );
    }
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
    let native = PolicyApplication
        .step_input(&resolved, &context, input.plan.steps()[0].id())
        .unwrap();
    assert!(native.constraints.contains("[\"primary-agent\",\"alpha\"]"));
    assert_eq!(
        PolicyEngine::evaluate(&authority, &native.as_input()),
        report.steps()[input.plan.steps()[0].id()]
    );
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
    let native = PolicyApplication
        .step_input(&resolved, &context, input.plan.steps()[0].id())
        .unwrap();
    assert_eq!(native.process, ProcessReadiness::Blocked);
    assert_eq!(
        PolicyEngine::evaluate(&authority, &native.as_input()),
        report.steps()[input.plan.steps()[0].id()]
    );
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
    let native = PolicyApplication
        .step_input(&resolved, &context, input.plan.steps()[0].id())
        .unwrap();
    assert!(native.resolved);
    assert_eq!(native.process, ProcessReadiness::NotApplicable);
    assert!(native.capabilities.is_empty());
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
    let native = PolicyApplication
        .step_input(&resolved, &context, input.plan.steps()[0].id())
        .unwrap();
    assert!(
        native
            .capabilities
            .contains_key(&CapabilityId::new("nested").unwrap()),
        "connector sources must see capabilities introduced by the selected skill closure"
    );
    assert_eq!(
        PolicyEngine::evaluate(&authority, &native.as_input()),
        report.steps()[input.plan.steps()[0].id()]
    );
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
    let native = PolicyApplication
        .step_input(&resolved, &context, input.plan.steps()[0].id())
        .unwrap();
    assert!(native.constraints.contains("desired:stay-safe"));
    assert_eq!(
        PolicyEngine::evaluate(&authority, &native.as_input()),
        report.steps()[input.plan.steps()[0].id()]
    );
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

#[test]
fn direct_step_inputs_reject_foreign_steps_and_every_untrusted_context_basis() {
    let (mut resolved, _, mut context) = fixture();
    let step = resolved.snapshot.request().plan().steps()[0].id().clone();
    assert!(matches!(
        PolicyApplication.step_input(&resolved, &context, &PlanStepId::new("foreign").unwrap()),
        Err(PolicyApplicationError::UnknownStep)
    ));
    for field in [
        "scope",
        "catalog",
        "process",
        "mode",
        "profile",
        "foreign-facts",
    ] {
        let (_, _, fresh) = fixture();
        context = fresh;
        match field {
            "scope" => context.basis.scope = ContextScopeId::new("foreign").unwrap(),
            "catalog" => {
                context.basis.registry_fingerprint =
                    ContentFingerprint::of_bytes(b"replacement catalog")
            }
            "process" => {
                context.basis.process_state_fingerprint =
                    ContentFingerprint::of_bytes(b"replacement process")
            }
            "mode" => context.operating_mode = OperatingMode::Development,
            "profile" => context.execution_profile = ExecutionProfile::FastPath,
            _ => {
                context
                    .steps
                    .insert(PlanStepId::new("foreign").unwrap(), StepFacts::default());
            }
        }
        assert!(
            matches!(
                PolicyApplication.step_input(&resolved, &context, &step),
                Err(PolicyApplicationError::StaleContext | PolicyApplicationError::UnknownStep)
            ),
            "{field}"
        );
    }
    let (_, _, context) = fixture();
    resolved.report.visits += 1;
    assert!(matches!(
        PolicyApplication.step_input(&resolved, &context, &step),
        Err(PolicyApplicationError::Resolution(_))
    ));
}

#[test]
fn requirement_metadata_and_prerequisite_structure_survive_independent_trusted_facts() {
    let (_, authority, mut context) = fixture();
    let mut input = composition::fixture();
    let original = &input.plan.capability_requirements()[0];
    let requirement = CapabilityRequirement::new_with_metadata(
        original.id().clone(),
        original.capability().clone(),
        original.cardinality(),
        original.originating_delta_item().clone(),
        vec![CapabilityPrecondition::new("before.committed").unwrap()],
        vec![CapabilityConstraint::new("exact.target").unwrap()],
        "step contract",
    )
    .unwrap();
    let steps = input
        .plan
        .steps()
        .iter()
        .map(|step| {
            step.clone()
                .with_prerequisite(PlanCondition::desired_condition(
                    input.desired.conditions()[0].id().clone(),
                ))
                .unwrap()
        })
        .collect();
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.plan.desired_state().clone(),
        input.plan.delta().clone(),
        vec![requirement],
        steps,
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
    let step = input.plan.steps()[0].id();
    context.steps.get_mut(step).unwrap().prerequisites_satisfied = false;
    let native = PolicyApplication
        .step_input(&resolved, &context, step)
        .unwrap();
    assert!(native.has_prerequisites);
    let denied = PolicyEngine::evaluate(&authority, &native.as_input());
    for (reason, subject) in [
        (PolicyReason::PrerequisiteMissing, step.as_str()),
        (PolicyReason::EvidenceMissing, "before.committed"),
        (PolicyReason::ConstraintUnsatisfied, "exact.target"),
    ] {
        assert!(
            denied
                .findings
                .iter()
                .any(|finding| finding.reason == reason && finding.subject == subject)
        );
    }
    let facts = context.steps.get_mut(step).unwrap();
    facts.prerequisites_satisfied = true;
    facts.evidence.insert("before.committed".into());
    facts.satisfied_constraints.insert("exact.target".into());
    let current = PolicyApplication
        .step_input(&resolved, &context, step)
        .unwrap();
    assert!(
        current.has_prerequisites,
        "evidence does not erase the structural requirement"
    );
    let report = PolicyEngine::evaluate(&authority, &current.as_input());
    assert!(!report.findings.iter().any(|finding| {
        matches!(
            finding.reason,
            PolicyReason::PrerequisiteMissing
                | PolicyReason::EvidenceMissing
                | PolicyReason::ConstraintUnsatisfied
        ) && [step.as_str(), "before.committed", "exact.target"].contains(&finding.subject.as_str())
    }));
    assert_eq!(
        &report,
        &PolicyApplication
            .evaluate(&resolved, &authority, &context)
            .unwrap()
            .steps()[step]
    );
}

#[test]
fn one_whole_plan_input_capture_preserves_native_decisions_and_per_step_facts() {
    let (resolved, authority, mut context) = fixture();
    let first = resolved.snapshot.request().plan().steps()[0].id().clone();
    context
        .steps
        .get_mut(&first)
        .unwrap()
        .authorizations
        .clear();
    let inputs = PolicyApplication.step_inputs(&resolved, &context).unwrap();
    let report = PolicyApplication
        .evaluate(&resolved, &authority, &context)
        .unwrap();
    assert_eq!(
        inputs.len(),
        resolved.snapshot.request().plan().steps().len()
    );
    for (step, input) in &inputs {
        let expected = PolicyEngine::evaluate(&authority, &input.as_input());
        assert_eq!(report.steps().get(step), Some(&expected));
        assert_eq!(input.step, *step);
    }
    assert_ne!(report.steps()[&first].decision, PolicyDecision::Allow);
    // A fresh capture uses changed facts; the previous data stays immutable.
    context.steps.get_mut(&first).unwrap().authorizations = authority
        .capabilities
        .keys()
        .map(|id| (id.clone(), Approval::Granted))
        .collect();
    let fresh = PolicyApplication.step_inputs(&resolved, &context).unwrap();
    assert!(inputs[&first].facts.authorizations.is_empty());
    assert!(!fresh[&first].facts.authorizations.is_empty());
    assert_eq!(
        PolicyEngine::evaluate(&authority, &fresh[&first].as_input()).decision,
        PolicyDecision::Allow
    );
}

#[test]
fn whole_plan_inputs_validate_all_resolution_and_context_before_returning_any_step() {
    let (mut resolved, _, mut context) = fixture();
    context
        .steps
        .insert(PlanStepId::new("foreign").unwrap(), StepFacts::default());
    assert!(matches!(
        PolicyApplication.step_inputs(&resolved, &context),
        Err(PolicyApplicationError::UnknownStep)
    ));
    let (_, _, mut context) = fixture();
    context.basis.registry_fingerprint = ContentFingerprint::of_bytes(b"changed canonical catalog");
    assert!(matches!(
        PolicyApplication.step_inputs(&resolved, &context),
        Err(PolicyApplicationError::StaleContext)
    ));
    let (_, _, context) = fixture();
    resolved.report.visits += 1;
    assert!(matches!(
        PolicyApplication.step_inputs(&resolved, &context),
        Err(PolicyApplicationError::Resolution(_))
    ));
}

#[test]
fn native_multistep_inputs_keep_each_requirement_and_facts_separate() {
    let mut input = composition::fixture();
    // Declare each operand's metadata on its actual catalog capability. Native
    // resolution verifies requirement metadata against these declarations.
    let declare = |text: String| {
        let mut doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        for capability in doc["provided_capabilities"].as_array_mut().unwrap() {
            let ordinal = match capability["id"].as_str().unwrap() {
                "architecture.dependency-analysis" => 0,
                "nested" => 1,
                other => panic!("unexpected fixture capability: {other}"),
            };
            capability["preconditions"] = serde_json::json!([format!("evidence.source-{ordinal}")]);
            capability["constraints"] = serde_json::json!([format!("target.source-{ordinal}")]);
        }
        doc.to_string()
    };
    let agents = input
        .registry
        .agents()
        .documents()
        .iter()
        .map(|doc| AgentDefinitionDocument::from_json(&declare(doc.to_json().unwrap())).unwrap())
        .collect::<Vec<_>>();
    let skills = input
        .registry
        .skills()
        .iter()
        .map(|doc| SkillDefinitionDocument::from_json(&declare(doc.to_json().unwrap())).unwrap())
        .collect::<Vec<_>>();
    input.registry = gateway_registry::Registry::from_documents(agents, skills).unwrap();
    input.index = input.registry.capability_index().unwrap();
    let authority = PolicyAuthority {
        capabilities: input
            .index
            .entries()
            .map(|e| (e.id().clone(), e.capability().clone()))
            .collect(),
        policies: vec![
            PolicyDefinition::new(
                PolicyId::new("two-sources").unwrap(),
                "two sources",
                input.index.ids().cloned(),
            )
            .unwrap(),
        ],
        ..Default::default()
    };
    let conditions = ["source-a", "source-b"]
        .into_iter()
        .map(|id| {
            DesiredCondition::new(
                ConditionId::new(id).unwrap(),
                SubjectPath::new(["state", id]).unwrap(),
                ComparisonOperator::Equals,
                Some(TypedValue::Boolean(true)),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let desired = DesiredState::new(
        DesiredStateId::new("two-independent-observations").unwrap(),
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
    let current = CurrentState::new_v1(ObservedStateId::new("before-observation").unwrap());
    let situation = SituationAssemblyInput::new(current.clone())
        .assemble(SituationId::new("two-source-situation").unwrap())
        .unwrap();
    let delta = gateway_application::DeclarativePlanningApplication::new()
        .derive_delta(
            DeltaId::new("two-source-delta").unwrap(),
            &desired,
            &current,
            Some(&situation),
            &ComparisonRules::default(),
            &DeltaDerivationRules::default(),
        )
        .unwrap()
        .delta()
        .clone();
    assert_eq!(delta.items().len(), 2);
    let requirements = delta
        .items()
        .iter()
        .enumerate()
        .map(|(i, item)| {
            CapabilityRequirement::new_with_metadata(
                CapabilityRequirementId::new(format!("observe-source-{i}")).unwrap(),
                CapabilityId::new(if i == 0 {
                    "architecture.dependency-analysis"
                } else {
                    "nested"
                })
                .unwrap(),
                RequirementCardinality::Mandatory,
                item.id().clone(),
                vec![CapabilityPrecondition::new(format!("evidence.source-{i}")).unwrap()],
                vec![CapabilityConstraint::new(format!("target.source-{i}")).unwrap()],
                "Observe this exact source",
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let plan = gateway_domain::plan(&desired, &delta, &requirements, &PlannerRules::default())
        .unwrap()
        .plan()
        .unwrap()
        .clone();
    assert_eq!(
        plan.steps().len(),
        2,
        "both steps come from native planning"
    );
    let steps = requirements
        .iter()
        .map(|requirement| {
            plan.steps()
                .iter()
                .find(|step| step.capability_requirements().contains(requirement.id()))
                .unwrap()
                .id()
                .clone()
        })
        .collect::<Vec<_>>();
    assert_ne!(steps[0], steps[1]);
    input.situation = DeclarativeContextSituationDocument::new(
        DeclarativeContext::new_v1(DeclarativeContextId::new("two-source-context").unwrap()),
        Some(Intent::new(
            IntentId::new("two-source-task").unwrap(),
            desired.clone(),
        )),
        None,
        current,
        situation,
    )
    .unwrap();
    input.desired = desired;
    input.delta = delta;
    input.plan = plan;
    let mut rules = composition::rules();
    rules
        .provider_priorities
        .insert(composition::skill("good"), 10);
    rules
        .provider_priorities
        .insert(composition::skill("nested-provider"), 10);
    for i in 0..2 {
        rules.applicability.semantics.insert(
            format!("evidence.source-{i}"),
            gateway_application::resolution_skills::SkillCondition::Always,
        );
        rules.applicability.semantics.insert(
            format!("target.source-{i}"),
            gateway_application::resolution_skills::SkillCondition::Always,
        );
    }
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(&input, &rules)
        .unwrap();
    assert_eq!(
        resolved.report.outcome,
        ResolutionOutcome::Resolved,
        "{:#?}",
        resolved.report
    );

    let base_facts = StepFacts {
        authorizations: authority
            .capabilities
            .keys()
            .map(|id| (id.clone(), Approval::Granted))
            .collect(),
        prerequisites_satisfied: true,
        ..Default::default()
    };
    let mut allowed = base_facts.clone();
    allowed.evidence.insert("evidence.source-0".into());
    allowed
        .satisfied_constraints
        .insert("target.source-0".into());
    let mut missing_evidence = base_facts;
    missing_evidence
        .satisfied_constraints
        .insert("target.source-1".into());
    let mut context = PolicyContext {
        basis: resolved.report.basis.clone(),
        operating_mode: input.operating_mode,
        execution_profile: input.execution_profile,
        steps: [
            (steps[0].clone(), allowed),
            (steps[1].clone(), missing_evidence),
        ]
        .into(),
    };
    let inputs = PolicyApplication.step_inputs(&resolved, &context).unwrap();
    let report = PolicyApplication
        .evaluate(&resolved, &authority, &context)
        .unwrap();
    assert_eq!(inputs.len(), 2);
    for (i, step) in steps.iter().enumerate() {
        let other = 1 - i;
        assert!(
            inputs[step]
                .preconditions
                .contains(&format!("evidence.source-{i}"))
        );
        assert!(
            !inputs[step]
                .preconditions
                .contains(&format!("evidence.source-{other}"))
        );
        assert!(
            inputs[step]
                .constraints
                .contains(&format!("target.source-{i}"))
        );
        assert!(
            !inputs[step]
                .constraints
                .contains(&format!("target.source-{other}"))
        );
        assert_eq!(inputs[step].facts, context.steps[step]);
        assert_eq!(
            report.steps()[step],
            PolicyEngine::evaluate(&authority, &inputs[step].as_input())
        );
        let single = PolicyApplication
            .step_input(&resolved, &context, step)
            .unwrap();
        assert_eq!(
            report.steps()[step],
            PolicyEngine::evaluate(&authority, &single.as_input())
        );
    }
    assert_eq!(report.steps()[&steps[0]].decision, PolicyDecision::Allow);
    assert_eq!(
        report.steps()[&steps[1]].decision,
        PolicyDecision::RequireEvidence
    );
    assert!(
        report.steps()[&steps[1]]
            .findings
            .iter()
            .any(|f| f.reason == PolicyReason::EvidenceMissing && f.subject == "evidence.source-1")
    );
    context
        .steps
        .get_mut(&steps[1])
        .unwrap()
        .evidence
        .insert("evidence.source-1".into());
    let fresh = PolicyApplication.step_inputs(&resolved, &context).unwrap();
    assert!(
        !inputs[&steps[1]]
            .facts
            .evidence
            .contains("evidence.source-1")
    );
    assert_eq!(
        PolicyEngine::evaluate(&authority, &fresh[&steps[1]].as_input()).decision,
        PolicyDecision::Allow
    );
}
