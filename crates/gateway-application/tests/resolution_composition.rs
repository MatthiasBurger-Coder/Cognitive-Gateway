use gateway_application::{
    resolution::*, resolution_agents::*, resolution_applicability::*, resolution_candidates::*,
    resolution_composition::*, resolution_process::*, resolution_skills::*, resolution_snapshot::*,
};
use gateway_domain::*;
use gateway_process::{ActivityId, ProcessRegistry, ProcessSource};
use gateway_registry::{CapabilityProvider, Registry};
use std::collections::BTreeMap;
mod support;

fn rules() -> CompositionRules {
    CompositionRules {
        version: SchemaVersion::V1,
        candidates: CandidateRules {
            version: SchemaVersion::V1,
            selectors: BTreeMap::new(),
        },
        processes: ProcessSelectionRules {
            version: SchemaVersion::V1,
            preference: TemplatePreference::None,
            required_definition: None,
            activities: BTreeMap::new(),
            output_evidence: BTreeMap::new(),
            lifecycle_contracts: BTreeMap::new(),
        },
        agents: AgentRules {
            version: SchemaVersion::V1,
            primary: BTreeMap::new(),
            participants: BTreeMap::new(),
            process_roles: BTreeMap::new(),
        },
        skills: BTreeMap::new(),
        applicability: ApplicabilityRules {
            version: SchemaVersion::V1,
            restrictions: BTreeMap::new(),
            semantics: BTreeMap::from([("repository.available".into(), SkillCondition::Always)]),
            completed: BTreeMap::new(),
            activities: BTreeMap::new(),
        },
        provider_priorities: BTreeMap::new(),
        prefer_optional: false,
        max_visits: 10000,
    }
}
fn skill_rules() -> SkillRules {
    SkillRules {
        version: SchemaVersion::V1,
        roots: BTreeMap::new(),
        conditions: BTreeMap::new(),
        capability_providers: BTreeMap::new(),
        max_visits: 1000,
    }
}
fn skill(id: &str) -> CapabilityProvider {
    CapabilityProvider::Skill {
        skill_id: SkillId::new(id).unwrap(),
    }
}
fn agent(id: &str) -> CapabilityProvider {
    CapabilityProvider::Agent {
        agent_id: AgentId::new(id).unwrap(),
    }
}
fn run(input: &ResolutionSnapshotInput, rules: &CompositionRules) -> CompositionReport {
    compose_resolution(&ResolutionSnapshot::capture(input).unwrap(), rules).unwrap()
}
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
    let cap = template["provided_capabilities"][0].clone();
    let mut nested = cap.clone();
    nested["id"] = "nested".into();
    let skills = ["bad", "good", "dependency", "nested-provider"]
        .iter()
        .map(|id| {
            let mut s = template.clone();
            s["id"] = (*id).into();
            s["related_skills"] = serde_json::json!([]);
            s["requires"] = if *id == "bad" {
                serde_json::json!(["dependency"])
            } else {
                serde_json::json!([])
            };
            s["provided_capabilities"] = match *id {
                "dependency" => serde_json::json!([]),
                "nested-provider" => serde_json::json!([nested.clone()]),
                _ => serde_json::json!([cap.clone()]),
            };
            SkillDefinitionDocument::from_json(&s.to_string()).unwrap()
        })
        .collect::<Vec<_>>();
    let agents = ["alpha", "beta"].iter().map(|id| AgentDefinitionDocument::from_json(&serde_json::json!({
        "schema_version":2,"kind":"agent","id":id,"description":"synthetic composition fixture",
        "skill_ids": if *id == "alpha" { vec!["bad", "good", "nested-provider"] } else { vec!["dependency"] },
        "provided_capabilities":[cap.clone(), nested.clone()]
    }).to_string()).unwrap()).collect::<Vec<_>>();
    input.registry = Registry::from_documents(agents, skills).unwrap();
    input.index = input.registry.capability_index().unwrap();
    input
}
fn edit_skill(
    input: &mut ResolutionSnapshotInput,
    id: &str,
    field: &str,
    value: serde_json::Value,
) {
    let skills = input
        .registry
        .skills()
        .iter()
        .map(|s| {
            if s.id().as_str() != id {
                return s.clone();
            }
            let mut wire: serde_json::Value = serde_json::from_str(&s.to_json().unwrap()).unwrap();
            wire[field] = value.clone();
            SkillDefinitionDocument::from_json(&wire.to_string()).unwrap()
        })
        .collect::<Vec<_>>();
    input.registry =
        Registry::from_documents(input.registry.agents().documents().to_vec(), skills).unwrap();
    input.index = input.registry.capability_index().unwrap();
}

#[test]
fn greedy_trap_filters_whole_closure_before_priority_and_keeps_ties() {
    let input = fixture();
    let id = input.plan.steps()[0].id().clone();
    let mut r = rules();
    r.provider_priorities = BTreeMap::from([(skill("bad"), 100), (skill("good"), 10)]);
    let mut sr = skill_rules();
    sr.conditions
        .insert(SkillId::new("dependency").unwrap(), SkillCondition::Never);
    r.skills.insert(id, sr);
    let result = run(&input, &r);
    assert_eq!(result.outcome, ResolutionOutcome::Resolved);
    assert_eq!(
        result.alternatives[0][0].chosen.values().next(),
        Some(&skill("good"))
    );
    assert!(result.steps[0].diagnostics.iter().any(|d| matches!(
        d,
        CompositionDiagnostic::Skill(SkillDiagnostic::Condition(
            _,
            ConditionStatus::Unsatisfied,
            true
        ))
    )));
    assert_eq!(result.steps[0].outcome, ResolutionOutcome::Resolved);
    r.provider_priorities.clear();
    let tied = run(&input, &r);
    assert_eq!(tied.outcome, ResolutionOutcome::Ambiguous);
    assert!(tied.alternatives.len() >= 3);
    let mut permuted = input.clone();
    let mut agents = input.registry.agents().documents().to_vec();
    agents.reverse();
    let mut skills = input.registry.skills().documents().to_vec();
    skills.reverse();
    permuted.registry = Registry::from_documents(agents, skills).unwrap();
    permuted.index = permuted.registry.capability_index().unwrap();
    assert_eq!(tied, run(&permuted, &r));
    assert_eq!(tied, run(&input, &r));
}

#[test]
fn nested_provider_search_and_required_closure_cannot_be_omitted() {
    let mut input = fixture();
    edit_skill(
        &mut input,
        "good",
        "required_capability_ids",
        serde_json::json!(["nested"]),
    );
    let mut r = rules();
    r.provider_priorities.insert(skill("good"), 100);
    let id = input.plan.steps()[0].id().clone();
    let result = run(&input, &r);
    assert_eq!(result.outcome, ResolutionOutcome::Ambiguous);
    assert!(result.alternatives.iter().all(|row| {
        row[0]
            .skills
            .as_ref()
            .unwrap()
            .required_capabilities
            .contains_key(&CapabilityId::new("nested").unwrap())
    }));
    let mut sr = skill_rules();
    sr.capability_providers
        .insert(CapabilityId::new("nested").unwrap(), agent("alpha"));
    r.skills.insert(id.clone(), sr.clone());
    assert_eq!(run(&input, &r).outcome, ResolutionOutcome::Resolved);
    sr.capability_providers
        .insert(CapabilityId::new("nested").unwrap(), skill("good"));
    r.skills.insert(id.clone(), sr.clone());
    assert!(
        run(&input, &r).steps[0]
            .diagnostics
            .iter()
            .any(|d| matches!(
                d,
                CompositionDiagnostic::Skill(SkillDiagnostic::InvalidCapabilityProvider(_))
            ))
    );
    sr.capability_providers.insert(
        CapabilityId::new("nested").unwrap(),
        skill("nested-provider"),
    );
    r.skills.insert(id, sr);
    edit_skill(
        &mut input,
        "nested-provider",
        "required_capability_ids",
        serde_json::json!(["architecture.dependency-analysis"]),
    );
    assert!(run(&input, &r).visits > 0);
}

fn extra_requirement(
    input: &mut ResolutionSnapshotInput,
    capability: &str,
    optional: bool,
    separate: bool,
) {
    let original = &input.plan.capability_requirements()[0];
    let extra = CapabilityRequirement::new(
        CapabilityRequirementId::new("extra").unwrap(),
        CapabilityId::new(capability).unwrap(),
        if optional {
            RequirementCardinality::Optional
        } else {
            RequirementCardinality::Mandatory
        },
        original.originating_delta_item().clone(),
        "synthetic extra requirement",
    )
    .unwrap();
    let mut requirements = input.plan.capability_requirements().to_vec();
    requirements.push(extra.clone());
    let mut steps = input.plan.steps().to_vec();
    if separate {
        let s = &steps[0];
        steps.push(
            PlanStep::new(
                PlanStepId::new("independent").unwrap(),
                s.kind(),
                s.outcome().clone(),
                s.completion().clone(),
                "independent synthetic branch",
            )
            .unwrap()
            .with_capability_requirements(vec![extra.id().clone()])
            .unwrap()
            .with_delta_items(s.delta_items().to_vec())
            .unwrap(),
        );
    } else {
        steps[0] = steps[0]
            .clone()
            .with_capability_requirements(requirements.iter().map(|r| r.id().clone()).collect())
            .unwrap();
    }
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.desired.id().clone(),
        input.delta.id().clone(),
        requirements,
        steps,
    )
    .unwrap();
}

#[test]
fn optional_groups_partial_failure_and_limits_are_explicit() {
    let mut input = fixture();
    extra_requirement(&mut input, "unknown", true, false);
    let mut r = rules();
    r.provider_priorities.insert(agent("alpha"), 10);
    r.prefer_optional = true;
    let result = run(&input, &r);
    assert_eq!(result.outcome, ResolutionOutcome::Resolved);
    assert!(result.alternatives[0][0].diagnostics.contains(
        &CompositionDiagnostic::OptionalOmitted(CapabilityRequirementId::new("extra").unwrap())
    ));
    let mut grouped = fixture();
    extra_requirement(&mut grouped, "nested", false, false);
    grouped.alternatives.push(RequirementAlternatives {
        step: grouped.plan.steps()[0].id().clone(),
        members: grouped
            .plan
            .capability_requirements()
            .iter()
            .map(|r| r.id().clone())
            .collect(),
        cardinality: RequirementCardinality::Mandatory,
    });
    let result = run(&grouped, &r);
    assert_eq!(result.outcome, ResolutionOutcome::Ambiguous);
    assert!(
        result
            .alternatives
            .iter()
            .all(|row| row[0].chosen.len() == 1)
    );
    grouped.alternatives[0].cardinality = RequirementCardinality::Optional;
    assert!(
        run(&grouped, &r)
            .alternatives
            .iter()
            .all(|row| row[0].chosen.len() <= 1)
    );
    let mut partial = fixture();
    extra_requirement(&mut partial, "unknown", false, true);
    let report = run(&partial, &r);
    assert_eq!(report.outcome, ResolutionOutcome::Partial);
    assert!(
        report
            .steps
            .iter()
            .any(|s| s.outcome == ResolutionOutcome::Resolved)
    );
    r.max_visits = 1;
    let report = run(&input, &r);
    assert_eq!(report.outcome, ResolutionOutcome::SearchLimit);
    assert!(report.exhausted);
    assert_eq!(report, run(&input, &r));
    r.max_visits = 8;
    assert_eq!(run(&input, &r).outcome, ResolutionOutcome::SearchLimit);
    r = rules();
    let mut sr = skill_rules();
    sr.max_visits = 1;
    r.skills.insert(input.plan.steps()[0].id().clone(), sr);
    assert_eq!(run(&input, &r).outcome, ResolutionOutcome::SearchLimit);
}

fn processes(input: &mut ResolutionSnapshotInput, ids: &[&str], constraint: &str) {
    input.processes = ProcessRegistry::from_sources(ids.iter().map(|id| ProcessSource::new(format!("{id}.feature"), format!(
        "@process({id})\n@process-version(1)\n@cg-language(1)\nFeature: Synthetic composition\nRule: Process\nGiven state START is initial\nGiven state END is terminal\nGiven event finish\nGiven activity inspect requires capability architecture.dependency-analysis\n{constraint}Scenario: finish\nGiven process state START\nWhen event finish occurs\nThen transition to state END\nThen authorize activity inspect\nThen complete process\n")))).unwrap();
}

#[test]
fn complete_plan_uses_one_process_and_enforces_canonical_roles_and_roots() {
    let mut input = fixture();
    processes(
        &mut input,
        &["one", "two"],
        "Given activity inspect constrained by primary-agent=alpha\nGiven activity inspect constrained by required-skill=good\n",
    );
    let mut r = rules();
    r.processes.preference = TemplatePreference::Required;
    r.provider_priorities.insert(agent("alpha"), 10);
    let report = run(&input, &r);
    assert_eq!(report.outcome, ResolutionOutcome::Ambiguous);
    assert_eq!(report.alternatives.len(), 2);
    let mut two_steps = input.clone();
    extra_requirement(
        &mut two_steps,
        "architecture.dependency-analysis",
        false,
        true,
    );
    let combined = run(&two_steps, &r);
    assert_eq!(combined.alternatives.len(), 2);
    assert!(combined.alternatives.iter().all(|row| row.len() == 2
        && row[0].binding.as_ref().unwrap().process == row[1].binding.as_ref().unwrap().process));
    assert!(report.alternatives.iter().all(|row| {
        row[0]
            .binding
            .as_ref()
            .unwrap()
            .skills
            .contains_key(&SkillId::new("good").unwrap())
    }));
    r.processes.required_definition = Some(
        input
            .processes
            .definitions()
            .next()
            .unwrap()
            .identity()
            .clone(),
    );
    assert_eq!(run(&input, &r).outcome, ResolutionOutcome::Resolved);
    let active = support::with_process(input.clone());
    assert_eq!(run(&active, &r).outcome, ResolutionOutcome::Resolved);
    r.applicability.activities.insert(
        input.plan.steps()[0].id().clone(),
        ActivityId::new("other").unwrap(),
    );
    assert_eq!(run(&active, &r).outcome, ResolutionOutcome::Conflicting);
    r.applicability.activities.clear();
    r.agents.primary.insert(
        input.plan.steps()[0].id().clone(),
        AgentId::new("beta").unwrap(),
    );
    assert_eq!(run(&input, &r).outcome, ResolutionOutcome::Conflicting);
    r = rules();
    r.processes.preference = TemplatePreference::Required;
    processes(
        &mut input,
        &["one"],
        "Given activity inspect constrained by unsupported=value\n",
    );
    assert_eq!(run(&input, &r).outcome, ResolutionOutcome::Conflicting);
    r.applicability
        .semantics
        .insert("unsupported=value".into(), SkillCondition::Always);
    assert_eq!(run(&input, &r).outcome, ResolutionOutcome::Ambiguous);
    processes(&mut input, &[], "");
    assert_eq!(run(&input, &r).outcome, ResolutionOutcome::Missing);
}

#[test]
fn no_op_and_invalid_rules_never_create_bindings() {
    let mut input = fixture();
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
    input.plan = gateway_domain::plan(&input.desired, &input.delta, &[], &PlannerRules::default())
        .unwrap()
        .plan()
        .unwrap()
        .clone();
    let report = run(&input, &rules());
    assert_eq!(report.outcome, ResolutionOutcome::NoOp);
    assert!(report.steps.is_empty());
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
    let report = run(&input, &rules());
    assert_eq!(report.outcome, ResolutionOutcome::NoOp);
    assert_eq!(report.steps[0].outcome, ResolutionOutcome::NoOp);
    assert!(report.alternatives[0][0].binding.is_none());
    let s = ResolutionSnapshot::capture(&fixture()).unwrap();
    for invalid in 0..8 {
        let mut r = rules();
        match invalid {
            0 => r.version = SchemaVersion::new(2, 0).unwrap(),
            1 => r.max_visits = 0,
            2 => r.candidates.version = SchemaVersion::new(2, 0).unwrap(),
            3 => r.processes.version = SchemaVersion::new(2, 0).unwrap(),
            4 => r.agents.version = SchemaVersion::new(2, 0).unwrap(),
            5 => r.applicability.version = SchemaVersion::new(2, 0).unwrap(),
            6 => {
                r.skills
                    .insert(PlanStepId::new("unknown").unwrap(), skill_rules());
            }
            _ => {
                let mut sr = skill_rules();
                sr.max_visits = 0;
                r.skills
                    .insert(s.request().plan().steps()[0].id().clone(), sr);
            }
        }
        assert!(compose_resolution(&s, &r).is_err());
    }
}
