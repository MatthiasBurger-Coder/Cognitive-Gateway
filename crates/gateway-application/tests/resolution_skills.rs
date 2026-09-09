use gateway_application::{
    resolution_candidates::CandidateRules, resolution_skills::*, resolution_snapshot::*,
};
use gateway_domain::*;
use gateway_registry::{CapabilityProvider, Registry};
use std::collections::BTreeMap;
mod support;

fn fixture() -> ResolutionSnapshotInput {
    let mut input = support::fixture();
    let template: serde_json::Value = serde_json::from_str(
        &input
            .registry
            .skill(&SkillId::new("architecture-hexagonal").unwrap())
            .unwrap()
            .to_json()
            .unwrap(),
    )
    .unwrap();
    let capability = template["provided_capabilities"][0].clone();
    let mut mutation = capability.clone();
    mutation["id"] = serde_json::json!("mutation");
    mutation["class"] = serde_json::json!("MUTATE");
    let skills = [
        ("a", vec!["b", "c"]),
        ("b", vec!["d"]),
        ("c", vec!["d"]),
        ("d", vec![]),
        ("m", vec![]),
    ]
    .into_iter()
    .map(|(id, dependencies)| {
        let mut skill = template.clone();
        skill["id"] = serde_json::json!(id);
        skill["requires"] = serde_json::json!(dependencies);
        skill["related_skills"] = if id == "a" {
            serde_json::json!(["m"])
        } else {
            serde_json::json!([])
        };
        skill["provided_capabilities"] = match id {
            "a" => serde_json::json!([capability.clone()]),
            "m" => serde_json::json!([mutation.clone()]),
            _ => serde_json::json!([]),
        };
        SkillDefinitionDocument::from_json(&skill.to_string()).unwrap()
    })
    .collect::<Vec<_>>();
    let agent = AgentDefinitionDocument::from_json(&serde_json::json!({"schema_version": 2, "kind": "agent", "id": "agent",
        "description": "synthetic graph Agent", "skill_ids": ["a"], "provided_capabilities": [capability, mutation]}).to_string()).unwrap();
    input.registry = Registry::from_documents([agent], skills).unwrap();
    input.index = input.registry.capability_index().unwrap();
    input
}
fn rules() -> SkillRules {
    SkillRules {
        version: SchemaVersion::V1,
        roots: BTreeMap::new(),
        conditions: BTreeMap::new(),
        capability_providers: BTreeMap::new(),
        max_visits: 1000,
    }
}
fn chosen(
    input: &ResolutionSnapshotInput,
) -> BTreeMap<CapabilityRequirementId, CapabilityProvider> {
    BTreeMap::from([(
        input.plan.capability_requirements()[0].id().clone(),
        CapabilityProvider::Skill {
            skill_id: SkillId::new("a").unwrap(),
        },
    )])
}
fn resolve(input: &ResolutionSnapshotInput, rules: &SkillRules) -> EffectiveSkills {
    resolve_skill_closure(
        &ResolutionSnapshot::capture(input).unwrap(),
        input.plan.steps()[0].id(),
        &chosen(input),
        &CandidateRules {
            version: SchemaVersion::V1,
            selectors: BTreeMap::new(),
        },
        rules,
    )
    .unwrap()
}
fn edit_skill(
    input: &mut ResolutionSnapshotInput,
    id: &str,
    field: &str,
    value: serde_json::Value,
) {
    let mut skills = input.registry.skills().documents().to_vec();
    let index = skills.iter().position(|s| s.id().as_str() == id).unwrap();
    let mut wire: serde_json::Value =
        serde_json::from_str(&skills[index].to_json().unwrap()).unwrap();
    wire[field] = value;
    skills[index] = SkillDefinitionDocument::from_json(&wire.to_string()).unwrap();
    input.registry =
        Registry::from_documents(input.registry.agents().documents().to_vec(), skills).unwrap();
}

#[test]
fn diamond_closure_is_dependency_first_and_retains_every_path() {
    let input = fixture();
    let result = resolve(&input, &rules());
    assert!(result.complete);
    assert_eq!(
        result
            .skills
            .iter()
            .map(SkillId::as_str)
            .collect::<Vec<_>>(),
        ["d", "b", "c", "a"]
    );
    assert_eq!(result.inclusion_paths[&SkillId::new("d").unwrap()].len(), 2);
    assert!(!result.skills.contains(&SkillId::new("m").unwrap()));
    assert!(result.required_capabilities.is_empty());
    let mut reordered = input.clone();
    let mut skills = input.registry.skills().documents().to_vec();
    skills.reverse();
    reordered.registry =
        Registry::from_documents(input.registry.agents().documents().to_vec(), skills).unwrap();
    reordered.index = reordered.registry.capability_index().unwrap();
    assert_eq!(result, resolve(&reordered, &rules()));
}

#[test]
fn conditions_are_explicit_and_mandatory_dependencies_are_never_trimmed_to_success() {
    let input = fixture();
    let snapshot = ResolutionSnapshot::capture(&input).unwrap();
    for (condition, status) in [
        (SkillCondition::Always, ConditionStatus::Satisfied),
        (SkillCondition::Never, ConditionStatus::Unsatisfied),
        (
            SkillCondition::Mode(OperatingMode::Hardening),
            ConditionStatus::Satisfied,
        ),
        (
            SkillCondition::Mode(OperatingMode::Development),
            ConditionStatus::Unsatisfied,
        ),
        (
            SkillCondition::Profile(ExecutionProfile::FullPath),
            ConditionStatus::Satisfied,
        ),
        (
            SkillCondition::Profile(ExecutionProfile::FastPath),
            ConditionStatus::Unsatisfied,
        ),
        (
            SkillCondition::ProcessState(gateway_process::StateId::new("unknown").unwrap()),
            ConditionStatus::Unknown,
        ),
        (
            SkillCondition::DesiredCondition(ConditionId::new("condition").unwrap()),
            ConditionStatus::Unknown,
        ),
        (
            SkillCondition::DesiredCondition(ConditionId::new("missing").unwrap()),
            ConditionStatus::Unsupported,
        ),
        (
            SkillCondition::Unsupported(ReferenceId::new("future-rule").unwrap()),
            ConditionStatus::Unsupported,
        ),
    ] {
        assert_eq!(evaluate_skill_condition(&snapshot, &condition), status);
        let mut r = rules();
        r.conditions.insert(SkillId::new("d").unwrap(), condition);
        assert_eq!(
            resolve(&input, &r).complete,
            status == ConditionStatus::Satisfied
        );
    }
    let mut r = rules();
    r.roots
        .insert(SkillId::new("m").unwrap(), RequirementCardinality::Optional);
    r.conditions
        .insert(SkillId::new("m").unwrap(), SkillCondition::Never);
    let result = resolve(&input, &r);
    assert!(result.complete);
    assert!(!result.skills.contains(&SkillId::new("m").unwrap()));
    assert!(result.diagnostics.contains(&SkillDiagnostic::Condition(
        SkillId::new("m").unwrap(),
        ConditionStatus::Unsatisfied,
        false
    )));
    let with_process = support::with_process(input);
    let snapshot = ResolutionSnapshot::capture(&with_process).unwrap();
    assert_eq!(
        evaluate_skill_condition(
            &snapshot,
            &SkillCondition::ProcessState(snapshot.process().unwrap().current_state().clone())
        ),
        ConditionStatus::Satisfied
    );
    assert_eq!(
        evaluate_skill_condition(
            &snapshot,
            &SkillCondition::ProcessState(gateway_process::StateId::new("not-current").unwrap())
        ),
        ConditionStatus::Unsatisfied
    );
}

#[test]
fn transitive_mutation_requirements_need_explicit_providers_and_detect_cross_cycles() {
    let mut input = fixture();
    edit_skill(
        &mut input,
        "d",
        "required_capability_ids",
        serde_json::json!(["mutation"]),
    );
    input.index = input.registry.capability_index().unwrap();
    let unbound = resolve(&input, &rules());
    assert!(!unbound.complete);
    assert_eq!(
        unbound.required_capabilities[&CapabilityId::new("mutation").unwrap()],
        Some(CapabilityClass::Mutate)
    );
    let mut r = rules();
    r.capability_providers.insert(
        CapabilityId::new("mutation").unwrap(),
        CapabilityProvider::Skill {
            skill_id: SkillId::new("m").unwrap(),
        },
    );
    assert!(resolve(&input, &r).complete);
    assert!(
        resolve(&input, &r)
            .skills
            .contains(&SkillId::new("m").unwrap())
    );
    r.capability_providers.insert(
        CapabilityId::new("mutation").unwrap(),
        CapabilityProvider::Agent {
            agent_id: AgentId::new("agent").unwrap(),
        },
    );
    assert!(resolve(&input, &r).complete);
    assert!(
        !resolve(&input, &r)
            .skills
            .contains(&SkillId::new("m").unwrap())
    );
    r.capability_providers.insert(
        CapabilityId::new("mutation").unwrap(),
        CapabilityProvider::Skill {
            skill_id: SkillId::new("a").unwrap(),
        },
    );
    assert!(
        resolve(&input, &r)
            .diagnostics
            .contains(&SkillDiagnostic::InvalidCapabilityProvider(
                CapabilityId::new("mutation").unwrap()
            ))
    );
    r.capability_providers.insert(
        CapabilityId::new("mutation").unwrap(),
        CapabilityProvider::Skill {
            skill_id: SkillId::new("m").unwrap(),
        },
    );
    edit_skill(
        &mut input,
        "m",
        "required_capability_ids",
        serde_json::json!(["architecture.dependency-analysis"]),
    );
    input.index = input.registry.capability_index().unwrap();
    r.capability_providers.insert(
        CapabilityId::new("architecture.dependency-analysis").unwrap(),
        CapabilityProvider::Skill {
            skill_id: SkillId::new("a").unwrap(),
        },
    );
    let cyclic = resolve(&input, &r);
    assert!(!cyclic.complete);
    assert!(
        cyclic
            .diagnostics
            .iter()
            .any(|d| matches!(d, SkillDiagnostic::Cycle(_)))
    );
    edit_skill(
        &mut input,
        "d",
        "required_capability_ids",
        serde_json::json!(["missing"]),
    );
    input.index = input.registry.capability_index().unwrap();
    assert!(
        resolve(&input, &r)
            .diagnostics
            .contains(&SkillDiagnostic::MissingCapability(
                CapabilityId::new("missing").unwrap()
            ))
    );
}

#[test]
fn missing_nodes_cycles_and_limits_fail_closed() {
    let input = fixture();
    let mut r = rules();
    r.max_visits = 1;
    let limited = resolve(&input, &r);
    assert!(!limited.complete);
    assert!(
        limited
            .diagnostics
            .contains(&SkillDiagnostic::LimitExceeded)
    );
    assert_eq!(limited, resolve(&input, &r));
    let mut r = rules();
    r.roots.insert(
        SkillId::new("missing").unwrap(),
        RequirementCardinality::Mandatory,
    );
    assert!(
        resolve(&input, &r)
            .diagnostics
            .contains(&SkillDiagnostic::MissingSkill(
                SkillId::new("missing").unwrap()
            ))
    );
    let mut bad = input.clone();
    edit_skill(&mut bad, "d", "requires", serde_json::json!(["missing"]));
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::MissingSkillDependency {
            skill: SkillId::new("d").unwrap(),
            dependency: SkillId::new("missing").unwrap()
        })
    );
    let mut bad = input.clone();
    edit_skill(&mut bad, "d", "requires", serde_json::json!(["a"]));
    assert!(matches!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::SkillCycle(_))
    ));
}

#[test]
fn validates_requests_and_does_not_blanket_activate_agent_skill_lists() {
    let input = fixture();
    let snapshot = ResolutionSnapshot::capture(&input).unwrap();
    let candidate = CandidateRules {
        version: SchemaVersion::V1,
        selectors: BTreeMap::new(),
    };
    let run =
        |step: &PlanStepId,
         chosen: &BTreeMap<CapabilityRequirementId, CapabilityProvider>,
         r: &SkillRules| { resolve_skill_closure(&snapshot, step, chosen, &candidate, r) };
    let mut r = rules();
    r.version = SchemaVersion::V2;
    assert_eq!(
        run(input.plan.steps()[0].id(), &chosen(&input), &r),
        Err(SkillClosureError::UnsupportedVersion)
    );
    let mut r = rules();
    r.conditions
        .insert(SkillId::new("missing").unwrap(), SkillCondition::Always);
    assert_eq!(
        run(input.plan.steps()[0].id(), &chosen(&input), &r),
        Err(SkillClosureError::InvalidConditionReference)
    );
    for max in [0, 100_001] {
        let mut r = rules();
        r.max_visits = max;
        assert_eq!(
            run(input.plan.steps()[0].id(), &chosen(&input), &r),
            Err(SkillClosureError::InvalidBudget)
        );
    }
    assert_eq!(
        run(
            &PlanStepId::new("missing").unwrap(),
            &chosen(&input),
            &rules()
        ),
        Err(SkillClosureError::UnknownStep)
    );
    let fake = BTreeMap::from([(
        input.plan.capability_requirements()[0].id().clone(),
        CapabilityProvider::Skill {
            skill_id: SkillId::new("m").unwrap(),
        },
    )]);
    assert_eq!(
        run(input.plan.steps()[0].id(), &fake, &rules()),
        Err(SkillClosureError::InvalidChosenProvider)
    );
    let direct = BTreeMap::from([(
        input.plan.capability_requirements()[0].id().clone(),
        CapabilityProvider::Agent {
            agent_id: AgentId::new("agent").unwrap(),
        },
    )]);
    assert!(
        run(input.plan.steps()[0].id(), &direct, &rules())
            .unwrap()
            .skills
            .is_empty()
    );
    let mut r = rules();
    r.roots.insert(
        SkillId::new("a").unwrap(),
        RequirementCardinality::Mandatory,
    );
    assert_eq!(
        run(input.plan.steps()[0].id(), &direct, &r)
            .unwrap()
            .skills
            .len(),
        4
    );
}
