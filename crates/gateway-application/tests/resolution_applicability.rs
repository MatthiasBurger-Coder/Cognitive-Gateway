use gateway_application::{
    DeclarativeSituationApplication, ProcessSnapshotInput, resolution::*,
    resolution_applicability::*, resolution_candidates::*, resolution_skills::*,
    resolution_snapshot::*,
};
use gateway_domain::*;
use gateway_process::{ActivityId, ProcessInstance, ProcessRegistry, ProcessSource};
use gateway_registry::CapabilityProvider;
use std::collections::{BTreeMap, BTreeSet};
mod support;

fn rules() -> ApplicabilityRules {
    ApplicabilityRules {
        version: SchemaVersion::V1,
        restrictions: BTreeMap::new(),
        semantics: BTreeMap::new(),
        completed: BTreeMap::new(),
        activities: BTreeMap::new(),
    }
}
fn candidates() -> CandidateRules {
    CandidateRules {
        version: SchemaVersion::V1,
        selectors: BTreeMap::new(),
    }
}
fn evaluate(s: &ResolutionSnapshot, id: &PlanStepId, r: &ApplicabilityRules) -> StepApplicability {
    evaluate_applicability(s, id, &BTreeMap::new(), &BTreeSet::new(), &candidates(), r).unwrap()
}
fn graph() -> ResolutionSnapshotInput {
    let mut input = support::fixture();
    let original = &input.plan.steps()[0];
    let mut requirements = vec![];
    let mut steps = vec![];
    for (name, dependencies) in [
        ("a", vec![]),
        ("b", vec!["a"]),
        ("c", vec!["a"]),
        ("d", vec!["b", "c"]),
        ("independent", vec![]),
    ] {
        let requirement = CapabilityRequirement::new(
            CapabilityRequirementId::new(name).unwrap(),
            input.plan.capability_requirements()[0].capability().clone(),
            RequirementCardinality::Mandatory,
            input.delta.items()[0].id().clone(),
            "synthetic dependency graph",
        )
        .unwrap();
        let step = PlanStep::new(
            PlanStepId::new(name).unwrap(),
            original.kind(),
            original.outcome().clone(),
            original.completion().clone(),
            "synthetic graph",
        )
        .unwrap()
        .with_capability_requirements(vec![requirement.id().clone()])
        .unwrap()
        .with_delta_items(original.delta_items().to_vec())
        .unwrap()
        .with_dependencies(
            dependencies
                .iter()
                .map(|s| PlanStepId::new(*s).unwrap())
                .collect(),
        )
        .unwrap();
        steps.push(step);
        requirements.push(requirement);
    }
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.desired.id().clone(),
        input.delta.id().clone(),
        requirements,
        steps,
    )
    .unwrap();
    input
}
fn proof(s: &ResolutionSnapshot, id: &PlanStepId) -> CompletionEvidence {
    let step = s
        .request()
        .plan()
        .steps()
        .iter()
        .find(|p| p.id() == id)
        .unwrap();
    CompletionEvidence {
        basis: s.request().basis().clone(),
        contracts: std::iter::once(step.completion().clone())
            .chain(step.verification().cloned())
            .collect(),
        references: BTreeSet::from([EvidenceId::new("receipt").unwrap()]),
        status: ConditionStatus::Satisfied,
        freshness: FreshnessStatus::Fresh,
    }
}

#[test]
fn diamond_dependencies_require_exact_fresh_scoped_attestations() {
    let input = graph();
    let s = ResolutionSnapshot::capture(&input).unwrap();
    let mut r = rules();
    for step in s.request().plan().steps() {
        let result = evaluate(&s, step.id(), &r);
        assert_eq!(result.step, *step);
        assert_eq!(result.basis, *s.request().basis());
        assert_eq!(
            result.readiness,
            if step.dependencies().is_empty() {
                LifecycleReadiness::Eligible
            } else {
                LifecycleReadiness::Deferred
            }
        );
    }
    let a = PlanStepId::new("a").unwrap();
    let b = PlanStepId::new("b").unwrap();
    let c = PlanStepId::new("c").unwrap();
    let d = PlanStepId::new("d").unwrap();
    let valid = proof(&s, &a);
    for bad in 0..7 {
        let mut p = valid.clone();
        match bad {
            0 => p.references.clear(),
            1 => p.contracts.clear(),
            2 => p.status = ConditionStatus::Conflicted,
            3 => p.freshness = FreshnessStatus::Stale,
            4 => p.freshness = FreshnessStatus::Unknown,
            5 => p.basis.scope = ContextScopeId::new("other").unwrap(),
            _ => {
                p.contracts.insert(PlanCondition::desired_condition(
                    ConditionId::new("invented").unwrap(),
                ));
            }
        }
        r.completed.insert(a.clone(), p);
        assert!(
            evaluate(&s, &b, &r)
                .reasons
                .contains(&ApplicabilityReason::InvalidCompletion(a.clone()))
        );
    }
    r.completed.insert(a, valid);
    assert_eq!(evaluate(&s, &b, &r).readiness, LifecycleReadiness::Eligible);
    assert_eq!(evaluate(&s, &c, &r).readiness, LifecycleReadiness::Eligible);
    r.completed.insert(b.clone(), proof(&s, &b));
    assert_eq!(evaluate(&s, &d, &r).readiness, LifecycleReadiness::Deferred);
    r.completed.insert(c.clone(), proof(&s, &c));
    assert_eq!(evaluate(&s, &d, &r).readiness, LifecycleReadiness::Eligible);
    assert_eq!(s.input().plan, input.plan);
}

#[test]
fn mode_profile_and_every_source_are_conjunctive() {
    for mode in [
        OperatingMode::Development,
        OperatingMode::Hardening,
        OperatingMode::ReleaseQualification,
    ] {
        for profile in [
            ExecutionProfile::FastPath,
            ExecutionProfile::NormalPath,
            ExecutionProfile::FullPath,
        ] {
            let mut input = support::fixture();
            input.operating_mode = mode;
            input.execution_profile = profile;
            let s = ResolutionSnapshot::capture(&input).unwrap();
            let id = s.request().plan().steps()[0].id();
            let mut r = rules();
            r.restrictions.insert(
                id.clone(),
                BTreeMap::from([
                    (
                        "plan".into(),
                        vec![SkillCondition::Mode(OperatingMode::Hardening)],
                    ),
                    (
                        "agent".into(),
                        vec![SkillCondition::Profile(ExecutionProfile::FullPath)],
                    ),
                    ("skill".into(), vec![SkillCondition::Always]),
                ]),
            );
            assert_eq!(
                evaluate(&s, id, &r).readiness,
                if mode == OperatingMode::Hardening && profile == ExecutionProfile::FullPath {
                    LifecycleReadiness::Eligible
                } else {
                    LifecycleReadiness::Blocked
                }
            );
            r.restrictions
                .get_mut(id)
                .unwrap()
                .insert("process".into(), vec![SkillCondition::Never]);
            assert_eq!(evaluate(&s, id, &r).readiness, LifecycleReadiness::Blocked);
        }
    }
}

#[test]
fn canonical_preconditions_never_default_true_and_rules_fail_closed() {
    let s = ResolutionSnapshot::capture(&support::fixture()).unwrap();
    let id = s.request().plan().steps()[0].id();
    let discovery = discover_candidates(&s, &candidates()).unwrap();
    let set = &discovery.sets[0];
    let chosen = BTreeMap::from([(
        set.requirement.clone(),
        set.candidates[0].canonical.provider().clone(),
    )]);
    let mut r = rules();
    let run = |r: &ApplicabilityRules| {
        evaluate_applicability(&s, id, &chosen, &BTreeSet::new(), &candidates(), r)
    };
    assert_eq!(run(&r).unwrap().readiness, LifecycleReadiness::Unknown);
    r.semantics
        .insert("repository.available".into(), SkillCondition::Always);
    assert_eq!(run(&r).unwrap().readiness, LifecycleReadiness::Eligible);
    r.semantics.insert(
        "repository.available".into(),
        SkillCondition::DesiredCondition(ConditionId::new("condition").unwrap()),
    );
    assert_eq!(run(&r).unwrap().readiness, LifecycleReadiness::Unknown);
    r.version = SchemaVersion::new(2, 0).unwrap();
    assert_eq!(run(&r), Err(ApplicabilityError::UnsupportedVersion));
    r = rules();
    r.activities.insert(
        PlanStepId::new("absent").unwrap(),
        ActivityId::new("work").unwrap(),
    );
    assert_eq!(run(&r), Err(ApplicabilityError::UnknownStep));
    assert_eq!(
        evaluate_applicability(
            &s,
            &PlanStepId::new("absent").unwrap(),
            &chosen,
            &BTreeSet::new(),
            &candidates(),
            &rules()
        ),
        Err(ApplicabilityError::UnknownStep)
    );
    let invalid = BTreeMap::from([(
        set.requirement.clone(),
        CapabilityProvider::Agent {
            agent_id: AgentId::new("absent").unwrap(),
        },
    )]);
    assert_eq!(
        evaluate_applicability(&s, id, &invalid, &BTreeSet::new(), &candidates(), &rules()),
        Err(ApplicabilityError::InvalidProvider)
    );
    let mut bad = candidates();
    bad.version = SchemaVersion::new(2, 0).unwrap();
    assert_eq!(
        evaluate_applicability(&s, id, &chosen, &BTreeSet::new(), &bad, &rules()),
        Err(ApplicabilityError::InvalidRules)
    );
    let mut r = rules();
    r.activities
        .insert(id.clone(), ActivityId::new("work").unwrap());
    assert!(
        evaluate(&s, id, &r)
            .reasons
            .contains(&ApplicabilityReason::ProcessUnavailable)
    );
}

fn process_input(status: &str, gate: &str) -> ResolutionSnapshotInput {
    let mut input = support::fixture();
    input.processes = ProcessRegistry::from_sources([ProcessSource::new("synthetic.feature", "@process(synthetic)\n@process-version(1)\n@cg-language(1)\nFeature: Synthetic applicability\nRule: Process\nGiven state START is initial\nGiven state END is terminal\nGiven event finish\nGiven gate review\nGiven blocker stop reason review needed resolvable\nGiven activity inspect requires capability architecture.dependency-analysis\nGiven activity inspect constrained by mode=HARDENING\nScenario: finish\nGiven process state START\nWhen event finish occurs\nThen transition to state END\nThen authorize activity inspect\nThen complete process\n")]).unwrap();
    input = support::with_process(input);
    let mut json: serde_json::Value =
        serde_json::from_str(&input.instance.as_ref().unwrap().to_json().unwrap()).unwrap();
    json["status"] = status.into();
    if status == "PAUSED" {
        json["waiting_condition"] =
            serde_json::json!({"reason":"HUMAN_REVIEW", "detail":"review required"});
    }
    json["blockers"] = serde_json::json!({"stop":{"id":"stop", "reason":"review needed", "active":status == "BLOCKED", "resolvable":true}});
    json["active_gates"] = serde_json::json!({"review":gate});
    let instance = ProcessInstance::from_json(&json.to_string()).unwrap();
    let definition = input.processes.definitions().next().unwrap();
    input.situation_process = Some(
        DeclarativeSituationApplication::new()
            .process_reference(ProcessSnapshotInput::new(definition, &instance))
            .unwrap(),
    );
    input.instance = Some(instance);
    input
}

#[test]
fn prerequisites_preserve_future_contracts_and_require_explicit_evidence() {
    let mut input = graph();
    let steps = input
        .plan
        .steps()
        .iter()
        .map(|s| {
            if s.id().as_str() == "b" {
                s.clone()
                    .with_prerequisite(PlanCondition::desired_condition(
                        ConditionId::new("condition").unwrap(),
                    ))
                    .unwrap()
            } else if s.id().as_str() == "independent" {
                s.clone()
                    .with_prerequisites(vec![
                        PlanCondition::outcome(
                            RequiredOutcome::new(
                                RequiredOutcomeKind::Assessment,
                                "prior assessment",
                            )
                            .unwrap(),
                        ),
                        PlanCondition::desired_condition(ConditionId::new("condition").unwrap()),
                    ])
                    .unwrap()
            } else {
                s.clone()
                    .with_verification(PlanCondition::desired_condition(
                        ConditionId::new("condition").unwrap(),
                    ))
            }
        })
        .collect();
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.desired.id().clone(),
        input.delta.id().clone(),
        input.plan.capability_requirements().to_vec(),
        steps,
    )
    .unwrap();
    let s = ResolutionSnapshot::capture(&input).unwrap();
    let b = PlanStepId::new("b").unwrap();
    let a = PlanStepId::new("a").unwrap();
    let mut r = rules();
    assert_eq!(evaluate(&s, &b, &r).readiness, LifecycleReadiness::Deferred);
    r.completed.insert(a.clone(), proof(&s, &a));
    assert_eq!(evaluate(&s, &b, &r).readiness, LifecycleReadiness::Eligible);
    let result = evaluate(&s, &PlanStepId::new("independent").unwrap(), &r);
    assert_eq!(result.readiness, LifecycleReadiness::Unknown);
    assert_eq!(result.reasons.len(), 2);
    let mut revised = input.clone();
    revised.operating_mode = OperatingMode::Development;
    let newer = ResolutionSnapshot::capture(&revised).unwrap();
    assert!(!s.same_basis(&newer));
    assert_eq!(
        evaluate(&newer, &b, &r).readiness,
        LifecycleReadiness::Deferred
    );
}

#[test]
fn process_status_gates_and_activity_are_read_only_authority() {
    for status in [
        "RUNNING",
        "WAITING",
        "PAUSED",
        "BLOCKED",
        "COMPLETED",
        "FAILED",
    ] {
        let input = process_input(status, "PASSED");
        let before = input.instance.as_ref().unwrap().to_json().unwrap();
        let s = ResolutionSnapshot::capture(&input).unwrap();
        let id = s.request().plan().steps()[0].id();
        let mut r = rules();
        assert_eq!(evaluate(&s, id, &r).readiness, LifecycleReadiness::Blocked);
        r.activities
            .insert(id.clone(), ActivityId::new("inspect").unwrap());
        assert!(evaluate(&s, id, &r).reasons.iter().any(|r| matches!(
            r,
            ApplicabilityReason::Restriction(_, ConditionStatus::Unsupported)
        )));
        r.semantics.insert(
            "mode=HARDENING".into(),
            SkillCondition::Mode(OperatingMode::Hardening),
        );
        assert_eq!(
            evaluate(&s, id, &r).readiness,
            if status == "RUNNING" {
                LifecycleReadiness::Eligible
            } else {
                LifecycleReadiness::Blocked
            }
        );
        let result = evaluate_applicability(
            &s,
            id,
            &BTreeMap::new(),
            &BTreeSet::from([CapabilityId::new("absent").unwrap()]),
            &candidates(),
            &r,
        )
        .unwrap();
        assert!(
            result
                .reasons
                .contains(&ApplicabilityReason::CapabilityUnavailable(
                    CapabilityId::new("absent").unwrap()
                ))
        );
        assert_eq!(input.instance.as_ref().unwrap().to_json().unwrap(), before);
    }
    for gate in [
        "OPEN",
        "FAILED",
        "BLOCKED",
        "WAITING_FOR_EVIDENCE",
        "WAITING_FOR_AUTHORIZATION",
    ] {
        let s = ResolutionSnapshot::capture(&process_input("RUNNING", gate)).unwrap();
        assert!(
            evaluate(&s, s.request().plan().steps()[0].id(), &rules())
                .reasons
                .iter()
                .any(|r| matches!(r, ApplicabilityReason::Gate(_, _)))
        );
    }
}
