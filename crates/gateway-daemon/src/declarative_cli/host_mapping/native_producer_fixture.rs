//! Test-only native planner/resolver/policy fixture. No model/effect dispatch.
//! This module is a child of host_mapping so it reuses the actual CLI pipeline.
use super::super::{Options, pipeline};
use super::{capture, compile, policy};
use gateway_application::{
    context_application::{CompileStepInput, CompiledStep, ContextApplication},
    policy_application::PolicyApplication,
    resolution_application::{DeclarativeResolutionApplication, ResolvedPlan},
    sessions::SessionError,
};
use gateway_domain::*;
use gateway_policy::PolicyDecision;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) struct NativeProducerFixture {
    pub catalog: PathBuf,
    pub plan: Value,
    pub rules: Value,
    pub process: Value,
    pub policy: Value,
    pub basis: Value,
    pub resolved: ResolvedPlan,
    pub task: TaskId,
    pub producer_step: PlanStepId,
    pub action_step: PlanStepId,
    pub capabilities: BTreeMap<CapabilityId, CapabilityDefinition>,
    pub producer_projection: Value,
    pub action_projection: Value,
}
fn invalid(_: impl std::fmt::Debug) -> SessionError {
    SessionError::InvalidInput
}
fn copy_tree(source: &Path, target: &Path) -> Result<(), SessionError> {
    std::fs::create_dir_all(target).map_err(invalid)?;
    for entry in std::fs::read_dir(source).map_err(invalid)? {
        let entry = entry.map_err(invalid)?;
        let target = target.join(entry.file_name());
        if entry.file_type().map_err(invalid)?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target).map_err(invalid)?;
        }
    }
    Ok(())
}
/// Generate both roles from actual unknown-condition planning. The caller's
/// complete intent is preserved. Fewer than two actual planned steps is refused;
/// no dummy action, replacement goal condition or fabricated policy result is added.
pub(crate) fn real_plan(
    root: &Path,
    scope: &ContextScopeId,
    intent: &Intent,
) -> Result<NativeProducerFixture, SessionError> {
    build_native_plan(root, scope, intent, false)
}

/// Actual CLI planning from an unknown first condition and an explicitly
/// observed false second boolean condition. The latter derives real change
/// work; its separate skill requires inspection without modifying that skill.
/// Both conditions remain exactly the caller's original goal.
pub(crate) fn real_mixed_plan(
    root: &Path,
    scope: &ContextScopeId,
    intent: &Intent,
) -> Result<NativeProducerFixture, SessionError> {
    build_native_plan(root, scope, intent, true)
}

fn build_native_plan(
    root: &Path,
    scope: &ContextScopeId,
    intent: &Intent,
    mixed: bool,
) -> Result<NativeProducerFixture, SessionError> {
    if !root.is_absolute() {
        return Err(SessionError::InvalidInput);
    }
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/declarative-cli");
    let catalog = root.join("catalog");
    copy_tree(&fixtures.join("catalog"), &catalog)?;
    let document = |name: &str| -> Result<Value, SessionError> {
        serde_json::from_slice(
            &std::fs::read(fixtures.join(format!("{name}.json"))).map_err(invalid)?,
        )
        .map_err(invalid)
    };
    let mut context = document("context")?;
    context["scope"] = json!(scope);
    context["unknown_subjects"] = json!(
        intent
            .desired_state()
            .conditions()
            .iter()
            .map(|c| c.subject().to_string())
            .collect::<Vec<_>>()
    );
    let mut rules = document("rules")?;
    let mut process = document("process")?;
    if mixed {
        configure_mixed(&catalog, &mut context, &mut rules, &mut process, intent)?;
    }
    let options = Options {
        command: "plan".into(),
        values: [
            (
                "catalog".into(),
                catalog.to_str().ok_or(SessionError::InvalidInput)?.into(),
            ),
            ("context".into(), context.to_string()),
            (
                "intent".into(),
                serde_json::to_string(intent).map_err(invalid)?,
            ),
            ("rules".into(), rules.to_string()),
        ]
        .into(),
        json: true,
    };
    let (plan, exit) = pipeline::execute(&options).map_err(invalid)?;
    if exit != 0 {
        return Err(SessionError::InvalidInput);
    }
    if mixed {
        let native: Plan = serde_json::from_value(plan["plan"].clone()).map_err(invalid)?;
        let action = native
            .steps()
            .iter()
            .find(|step| step.kind() == PlanStepKind::Change)
            .ok_or(SessionError::InvalidInput)?;
        rules["resolution"]["activities"] = Value::Object(
            native
                .steps()
                .iter()
                .map(|step| {
                    (
                        step.id().as_str().to_owned(),
                        json!(if step.kind() == PlanStepKind::Change {
                            "update"
                        } else {
                            "inspect"
                        }),
                    )
                })
                .collect(),
        );
        rules["resolution"]["skill_capability_providers"] = json!({
            action.id().as_str(): { "architecture.dependency-analysis": { "skill": "inspect" } }
        });
    }
    let (snapshot, rules_native) =
        capture(&catalog, &[plan.clone(), rules.clone(), process.clone()]).map_err(invalid)?;
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(&snapshot, &rules_native)
        .map_err(invalid)?;
    let steps = snapshot.plan.steps();
    if steps.len() < 2 {
        return Err(SessionError::UnsupportedCapability);
    }
    let producer_step = steps[0].id().clone();
    let action_step = steps[1].id().clone();
    let serialized: Value = serde_json::from_str(
        &DeclarativeResolutionApplication
            .serialize_resolution(&resolved, Default::default())
            .map_err(invalid)?,
    )
    .map_err(invalid)?;
    let basis = serialized["basis"].clone();
    let mut policy_doc = document("policy")?;
    if mixed {
        policy_doc["policies"][0]["allowed_capabilities"] =
            json!(["architecture.dependency-analysis", "fixture.update"]);
    }
    let mut operator_facts = policy_doc["steps"]["step-condition.0"].clone();
    if mixed {
        operator_facts["authorizations"]["fixture.update"] = json!("GRANTED");
    }
    policy_doc["basis"] = basis.clone();
    policy_doc["steps"] = Value::Object(
        steps
            .iter()
            .map(|step| (step.id().as_str().into(), operator_facts.clone()))
            .collect(),
    );
    let (_, policy_context, _) = policy(&resolved, policy_doc.clone()).map_err(invalid)?;
    let application = PolicyApplication;
    let preparation = application
        .step_input(&resolved, &policy_context, &producer_step)
        .map_err(invalid)?;
    let action = application
        .step_input(&resolved, &policy_context, &action_step)
        .map_err(invalid)?;
    if !preparation.resolved
        || preparation.capabilities.is_empty()
        || preparation
            .capabilities
            .values()
            .any(|cap| cap.class() != CapabilityClass::Inspect)
        || !action.resolved
        || action.capabilities.is_empty()
    {
        return Err(SessionError::AuthorityDenied);
    }
    let task = TaskId::new("native-proposal-task").map_err(invalid)?;
    let projection = |step: &PlanStepId, id: &str| -> Result<Value, SessionError> {
        let mut projection = document("projection")?;
        projection["basis"] = basis.clone();
        projection["step"] = json!(step.as_str());
        projection["id"] = json!(id);
        projection["task"]["id"] = json!(task.as_str());
        if mixed {
            projection["process"] = rules["resolution"]["required_process"].clone();
        }
        if mixed && step == &action_step {
            projection["workflow"] = json!("action-workflow");
            projection["workflows"][0]["id"] = json!("action-workflow");
            projection["workflows"][0]["skill_ids"] = json!(["inspect", "update"]);
        }
        Ok(projection)
    };
    let producer_projection = projection(&producer_step, "native-producer-context")?;
    let action_projection = projection(&action_step, "native-action-context")?;
    let compile_native = |projection: Value| -> Result<CompiledStep, SessionError> {
        let command = compile(resolved.clone(), policy_doc.clone(), projection).map_err(invalid)?;
        ContextApplication
            .compile_step(CompileStepInput {
                resolved: &command.resolved,
                authority: &command.authority,
                policy_context: &command.policy_context,
                catalog: &command.catalog,
                projection: &command.projection,
                candidates: &command.candidates,
                selected: &command.selected,
            })
            .map_err(invalid)
    };
    compile_native(producer_projection.clone())?;
    if mixed {
        if !matches!(
            compile_native(action_projection.clone()),
            Err(SessionError::InvalidInput)
        ) {
            return Err(SessionError::AuthorityDenied);
        }
        let (authority, current, _) = policy(&resolved, policy_doc.clone()).map_err(invalid)?;
        let reports = PolicyApplication
            .evaluate(&resolved, &authority, &current)
            .map_err(invalid)?;
        if reports.steps()[&action_step].decision != PolicyDecision::RequireConsent {
            return Err(SessionError::AuthorityDenied);
        }
    } else {
        compile_native(action_projection.clone())?;
    }
    Ok(NativeProducerFixture {
        catalog,
        plan,
        rules,
        process,
        policy: policy_doc,
        basis,
        resolved,
        task,
        producer_step,
        action_step,
        capabilities: action.capabilities,
        producer_projection,
        action_projection,
    })
}

fn configure_mixed(
    catalog: &Path,
    context: &mut Value,
    rules: &mut Value,
    process: &mut Value,
    intent: &Intent,
) -> Result<(), SessionError> {
    let conditions = intent.desired_state().conditions();
    if conditions.len() != 2
        || conditions[1].operator() != ComparisonOperator::Equals
        || conditions[1].expected() != Some(&TypedValue::Boolean(true))
    {
        return Err(SessionError::UnsupportedCapability);
    }
    context["unknown_subjects"] = json!([conditions[0].subject().to_string()]);
    let provenance = Provenance::new(
        ProvenanceId::new("fixture-observed-input").map_err(invalid)?,
        SourceKind::Tool,
        SourceId::new("fixture-input").map_err(invalid)?,
        "fixture://explicit-negative-input",
    )
    .map_err(invalid)?;
    let observation = Observation::new(
        ObservationId::new("fixture-negative-input").map_err(invalid)?,
        conditions[1].subject().clone(),
        TypedValue::Boolean(false),
        provenance.id().clone(),
    )
    .map_err(invalid)?;
    let fact = Fact::new(
        FactId::new("fixture-negative-input").map_err(invalid)?,
        conditions[1].subject().clone(),
        TypedValue::Boolean(false),
        AssertionPolarity::Affirmed,
        vec![observation.id().clone()],
    )
    .map_err(invalid)?;
    context["records"] = serde_json::to_value(
        ObservationEvidenceSet::new(vec![provenance], vec![observation], vec![fact], vec![])
            .map_err(invalid)?,
    )
    .map_err(invalid)?;
    let inspect_path = catalog.join("skills/inspect.json");
    let mut update: Value =
        serde_json::from_slice(&std::fs::read(inspect_path).map_err(invalid)?).map_err(invalid)?;
    update["id"] = json!("update");
    update["name"] = json!("Explicit fixture mutation");
    update["requires"] = json!(["inspect"]);
    update["required_capability_ids"] = json!(["architecture.dependency-analysis"]);

    update["provided_capabilities"][0]["id"] = json!("fixture.update");
    update["provided_capabilities"][0]["class"] = json!("MUTATE");
    update["provided_capabilities"][0]["constraints"] = json!([]);
    std::fs::write(catalog.join("skills/update.json"), update.to_string()).map_err(invalid)?;
    let agent_path = catalog.join("agents/inspector.json");
    let mut agent: Value =
        serde_json::from_slice(&std::fs::read(&agent_path).map_err(invalid)?).map_err(invalid)?;
    agent["skill_ids"] = json!(["inspect", "update"]);
    std::fs::write(agent_path, agent.to_string()).map_err(invalid)?;
    let feature_path = catalog.join("processes/inspect.feature");
    let mut feature = std::fs::read_to_string(&feature_path).map_err(invalid)?;
    feature = feature.replace(
        "Given activity inspect requires capability architecture.dependency-analysis",
        "Given activity inspect requires capability architecture.dependency-analysis\nGiven activity update requires capability fixture.update\nGiven activity update requires capability architecture.dependency-analysis",
    );
    feature = feature.replace(
        "Given event finish",
        "Given event finish\nGiven event update",
    );
    feature.push_str("\nScenario: update\nGiven process state START\nWhen event update occurs\nThen transition to state END\nThen authorize activity update\nThen complete process\n");
    std::fs::write(feature_path, feature).map_err(invalid)?;
    let processes =
        gateway_process::ProcessRegistry::load(catalog.join("processes")).map_err(invalid)?;
    let definition = processes
        .definitions()
        .next()
        .ok_or(SessionError::InvalidInput)?;
    let digest = definition.identity().digest().as_str();
    rules["resolution"]["required_process"]["digest"] = json!(digest);
    process["instance"]["definition_digest"] = json!(digest);
    rules["planning"]["domain_change"] = json!("fixture.update");
    Ok(())
}

/// The explicit full fixture goal: two independent boolean observations. Caller
/// installs this same intent as its registered goal; no existing goal is edited.
pub(crate) fn two_condition_intent() -> Intent {
    let conditions = ["first", "second"]
        .iter()
        .map(|id| {
            DesiredCondition::new(
                ConditionId::new(*id).unwrap(),
                SubjectPath::new(["fixture", *id]).unwrap(),
                ComparisonOperator::Equals,
                Some(TypedValue::Boolean(true)),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let desired = DesiredState::new(
        DesiredStateId::new("fixture-native-goal").unwrap(),
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
    Intent::new(IntentId::new("fixture-native-intent").unwrap(), desired)
}

#[test]
fn real_operator_policy_compiles_two_actual_planned_inspection_steps_and_denies_revocation() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "cg-native-producer-plan-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    let intent = two_condition_intent();
    let scope = ContextScopeId::new("native-fixture-scope").unwrap();
    let only = intent.desired_state().conditions()[0].clone();
    let one_goal = DesiredState::new(
        intent.desired_state().id().clone(),
        vec![only.clone()],
        ConditionExpression::condition(only.id().clone()),
        vec![],
        vec![],
    )
    .unwrap();
    assert_eq!(
        real_plan(
            &root.join("single"),
            &scope,
            &Intent::new(intent.id().clone(), one_goal)
        )
        .err(),
        Some(SessionError::UnsupportedCapability),
        "a one-step plan is not patched with a dummy preparation/action"
    );
    let fixture = real_plan(&root, &scope, &intent).unwrap();
    assert_ne!(fixture.producer_step, fixture.action_step);
    assert_eq!(
        fixture.resolved.snapshot.input().desired,
        *intent.desired_state()
    );
    assert_eq!(fixture.resolved.snapshot.input().plan.steps().len(), 2);
    let (snapshot, rules) = capture(
        &fixture.catalog,
        &[
            fixture.plan.clone(),
            fixture.rules.clone(),
            fixture.process.clone(),
        ],
    )
    .unwrap();
    let fresh = DeclarativeResolutionApplication
        .resolve_plan(&snapshot, &rules)
        .unwrap();
    let artifact: Value = serde_json::from_str(
        &DeclarativeResolutionApplication
            .serialize_resolution(&fresh, Default::default())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        artifact["basis"], fixture.basis,
        "returned documents reconstruct the actual current canonical basis"
    );
    let (_, context, _) = policy(&fresh, fixture.policy.clone()).unwrap();
    let action = PolicyApplication
        .step_input(&fresh, &context, &fixture.action_step)
        .unwrap();
    assert_eq!(action.capabilities, fixture.capabilities);
    assert!(
        fixture
            .capabilities
            .values()
            .all(|capability| capability.class() == CapabilityClass::Inspect)
    );
    for projection in [&fixture.producer_projection, &fixture.action_projection] {
        let command = compile(fresh.clone(), fixture.policy.clone(), projection.clone()).unwrap();
        let compiled = ContextApplication
            .compile_step(CompileStepInput {
                resolved: &command.resolved,
                authority: &command.authority,
                policy_context: &command.policy_context,
                catalog: &command.catalog,
                projection: &command.projection,
                candidates: &command.candidates,
                selected: &command.selected,
            })
            .unwrap();
        assert_eq!(compiled.policy().decision, PolicyDecision::Allow);
        assert_eq!(
            compiled.context().execution_context().task().id(),
            &fixture.task
        );
    }
    let mut denied = fixture.policy.clone();
    denied["steps"][fixture.producer_step.as_str()]["authorizations"] = json!({});
    let command = compile(
        fixture.resolved.clone(),
        denied,
        fixture.producer_projection.clone(),
    )
    .unwrap();
    assert!(
        ContextApplication
            .compile_step(CompileStepInput {
                resolved: &command.resolved,
                authority: &command.authority,
                policy_context: &command.policy_context,
                catalog: &command.catalog,
                projection: &command.projection,
                candidates: &command.candidates,
                selected: &command.selected
            })
            .is_err()
    );
    let mut stale = fixture.policy.clone();
    stale["basis"]["registry_fingerprint"] = json!("f".repeat(64));
    assert!(policy(&fixture.resolved, stale).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn real_mixed_native_plan_compiles_inspection_and_requires_action_consent() {
    use gateway_application::context_application::ContextApplicationError;
    let root = std::env::temp_dir().join(format!("cg-native-mixed-plan-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let scope = ContextScopeId::new("native-mixed-fixture-scope").unwrap();
    let intent = two_condition_intent();
    let fixture = real_mixed_plan(&root, &scope, &intent).unwrap();
    assert_eq!(
        fixture.resolved.snapshot.input().desired,
        *intent.desired_state()
    );
    let (authority, current, _) = policy(&fixture.resolved, fixture.policy.clone()).unwrap();
    let producer = PolicyApplication
        .step_input(&fixture.resolved, &current, &fixture.producer_step)
        .unwrap();
    let action = PolicyApplication
        .step_input(&fixture.resolved, &current, &fixture.action_step)
        .unwrap();
    assert!(
        producer
            .capabilities
            .values()
            .all(|c| c.class() == CapabilityClass::Inspect)
    );
    assert_eq!(producer.capabilities.len(), 1);
    assert_eq!(action.capabilities.len(), 2);
    assert!(
        action
            .capabilities
            .values()
            .any(|c| c.class() == CapabilityClass::Mutate)
    );
    assert!(
        producer
            .capabilities
            .keys()
            .all(|id| action.capabilities.contains_key(id))
    );
    assert!(producer.facts.consents.is_empty());
    assert!(action.facts.consents.is_empty());
    let report = PolicyApplication
        .evaluate(&fixture.resolved, &authority, &current)
        .unwrap();
    assert_eq!(
        report.steps()[&fixture.producer_step].decision,
        PolicyDecision::Allow
    );
    assert_eq!(
        report.steps()[&fixture.action_step].decision,
        PolicyDecision::RequireConsent
    );
    let compile_projection = |projection: Value, policy_document: Value| {
        let c = compile(fixture.resolved.clone(), policy_document, projection).unwrap();
        ContextApplication.compile_step(CompileStepInput {
            resolved: &c.resolved,
            authority: &c.authority,
            policy_context: &c.policy_context,
            catalog: &c.catalog,
            projection: &c.projection,
            candidates: &c.candidates,
            selected: &c.selected,
        })
    };
    let actual =
        compile_projection(fixture.producer_projection.clone(), fixture.policy.clone()).unwrap();
    assert_eq!(
        actual.context().execution_context().task().id(),
        &fixture.task
    );
    assert!(matches!(
        compile_projection(fixture.action_projection.clone(), fixture.policy.clone()),
        Err(ContextApplicationError::NotAuthorized(
            PolicyDecision::RequireConsent
        ))
    ));
    let mut revoked = fixture.policy.clone();
    revoked["steps"][fixture.producer_step.as_str()]["authorizations"]["architecture.dependency-analysis"] =
        json!("DENIED");
    assert!(matches!(
        compile_projection(fixture.producer_projection.clone(), revoked),
        Err(ContextApplicationError::NotAuthorized(PolicyDecision::Deny))
    ));
    let mut stale = fixture.policy.clone();
    stale["basis"]["scope"] = json!("foreign-scope");
    assert!(policy(&fixture.resolved, stale).is_err());
    let (snapshot, rules) = capture(
        &fixture.catalog,
        &[
            fixture.plan.clone(),
            fixture.rules.clone(),
            fixture.process.clone(),
        ],
    )
    .unwrap();
    let fresh = DeclarativeResolutionApplication
        .resolve_plan(&snapshot, &rules)
        .unwrap();
    assert_eq!(fresh.report.basis, fixture.resolved.report.basis);
    assert_eq!(snapshot.plan.steps().len(), 2);
    assert_eq!(
        snapshot
            .plan
            .steps()
            .iter()
            .filter(|s| s.kind() == PlanStepKind::Change)
            .count(),
        1
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn mixed_native_provider_rules_preserve_discovery_deny_wrong_providers_and_bind_full_rules() {
    use gateway_application::resolution::ResolutionOutcome;
    let root =
        std::env::temp_dir().join(format!("cg-native-mixed-providers-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let fixture = real_mixed_plan(
        &root,
        &ContextScopeId::new("mixed-provider-scope").unwrap(),
        &two_condition_intent(),
    )
    .unwrap();
    let resolve = |rules_document: Value| {
        let (snapshot, rules) = capture(
            &fixture.catalog,
            &[
                fixture.plan.clone(),
                rules_document,
                fixture.process.clone(),
            ],
        )
        .unwrap();
        DeclarativeResolutionApplication.resolve_plan(&snapshot, &rules)
    };
    let mut missing = fixture.rules.clone();
    missing["resolution"]
        .as_object_mut()
        .unwrap()
        .remove("skill_capability_providers");
    // Omission preserves the existing bounded native provider discovery.
    assert_eq!(
        resolve(missing).unwrap().report.outcome,
        ResolutionOutcome::Resolved
    );
    let inspect_path = fixture.catalog.join("skills/inspect.json");
    let original = std::fs::read(&inspect_path).unwrap();
    let mut unavailable: Value = serde_json::from_slice(&original).unwrap();
    unavailable["provided_capabilities"] = json!([]);
    std::fs::write(&inspect_path, unavailable.to_string()).unwrap();
    if let Ok((snapshot, rules)) = capture(
        &fixture.catalog,
        &[
            fixture.plan.clone(),
            fixture.rules.clone(),
            fixture.process.clone(),
        ],
    ) {
        assert_ne!(
            DeclarativeResolutionApplication
                .resolve_plan(&snapshot, &rules)
                .unwrap()
                .report
                .outcome,
            ResolutionOutcome::Resolved
        );
    }
    std::fs::write(inspect_path, original).unwrap();
    for provider in [
        json!({"skill":"update"}),
        json!({"skill":"foreign"}),
        json!({"agent":"inspector"}),
    ] {
        let mut wrong = fixture.rules.clone();
        wrong["resolution"]["skill_capability_providers"][fixture.action_step.as_str()]["architecture.dependency-analysis"] =
            provider;
        assert_ne!(
            resolve(wrong).unwrap().report.outcome,
            ResolutionOutcome::Resolved
        );
    }
    let mut unknown_step = fixture.rules.clone();
    unknown_step["resolution"]["skill_capability_providers"]["foreign-step"] =
        json!({"architecture.dependency-analysis":{"skill":"inspect"}});
    assert!(resolve(unknown_step).is_err());
    let actual = resolve(fixture.rules.clone()).unwrap();
    let actual_fingerprint = DeclarativeResolutionApplication
        .inspect_resolution(&actual)
        .unwrap()
        .rule_fingerprint;
    let mut changed = fixture.rules.clone();
    changed["resolution"]["skill_capability_providers"][fixture.action_step.as_str()]["fixture.update"] =
        json!({"skill":"update"});
    let changed = resolve(changed).unwrap();
    assert_eq!(changed.report.outcome, ResolutionOutcome::Resolved);
    assert_ne!(
        DeclarativeResolutionApplication
            .inspect_resolution(&changed)
            .unwrap()
            .rule_fingerprint,
        actual_fingerprint
    );
    let (authority, current, _) = policy(&actual, fixture.policy.clone()).unwrap();
    assert_eq!(
        PolicyApplication
            .evaluate(&actual, &authority, &current)
            .unwrap()
            .steps()[&fixture.action_step]
            .decision,
        PolicyDecision::RequireConsent
    );
    std::fs::remove_dir_all(root).unwrap();
}
