use gateway_application::{
    resolution_agents::*, resolution_candidates::CandidateRules, resolution_snapshot::*,
};
use gateway_domain::*;
use gateway_process::{ActivityId, ProcessRegistry, ProcessSource};
use gateway_registry::Registry;
use std::collections::{BTreeMap, BTreeSet};
mod support;

fn candidate_rules() -> CandidateRules {
    CandidateRules {
        version: SchemaVersion::V1,
        selectors: BTreeMap::new(),
    }
}
fn rules() -> AgentRules {
    AgentRules {
        version: SchemaVersion::V1,
        primary: BTreeMap::new(),
        participants: BTreeMap::new(),
        process_roles: BTreeMap::new(),
    }
}

fn input(owner: Option<&str>, link: bool, direct: bool) -> ResolutionSnapshotInput {
    let mut input = support::fixture();
    let original = input
        .registry
        .skill(&SkillId::new("architecture-hexagonal").unwrap())
        .unwrap();
    let mut skill: serde_json::Value = serde_json::from_str(&original.to_json().unwrap()).unwrap();
    skill["id"] = serde_json::json!("skill");
    skill["owner_agent_id"] = serde_json::json!(owner);
    skill["related_skills"] = serde_json::json!([]);
    let capability = skill["provided_capabilities"].clone();
    let mut unrelated = skill.clone();
    unrelated["id"] = serde_json::json!("unrelated");
    unrelated["owner_agent_id"] = serde_json::Value::Null;
    unrelated["provided_capabilities"] = serde_json::json!([]);
    let agents = ["alpha", "beta"]
        .into_iter()
        .map(|id| {
            AgentDefinitionDocument::from_json(&serde_json::json!({
        "schema_version": 2, "kind": "agent", "id": id, "description": "synthetic agent",
        "skill_ids": if link && id == "alpha" { vec!["skill"] } else { vec!["unrelated"] },
        "provided_capabilities": if direct { capability.clone() } else { serde_json::json!([]) }
    }).to_string()).unwrap()
        })
        .collect::<Vec<_>>();
    input.registry = Registry::from_documents(
        agents,
        [
            SkillDefinitionDocument::from_json(&skill.to_string()).unwrap(),
            SkillDefinitionDocument::from_json(&unrelated.to_string()).unwrap(),
        ],
    )
    .unwrap();
    input.index = input.registry.capability_index().unwrap();
    input
}

fn bind(input: &ResolutionSnapshotInput, rules: &AgentRules) -> AgentBindingCandidates {
    bind_agent_candidates(
        &ResolutionSnapshot::capture(input).unwrap(),
        &candidate_rules(),
        rules,
    )
    .unwrap()
}

#[test]
fn canonical_direct_owner_and_skill_links_remain_separate_responsibilities() {
    let input = input(Some("beta"), true, true);
    let result = bind(&input, &rules());
    let step = &result.steps[0];
    assert_eq!(step.primary_candidates.len(), 2);
    let responsibilities = step.responsibilities.values().next().unwrap();
    assert!(
        responsibilities
            .iter()
            .any(|r| r.source == ResponsibilitySource::DirectProvider)
    );
    assert!(
        responsibilities
            .iter()
            .any(|r| r.source == ResponsibilitySource::SkillOwner && r.agent.as_str() == "beta")
    );
    assert!(
        responsibilities
            .iter()
            .any(|r| matches!(r.source, ResponsibilitySource::AgentSkill(_))
                && r.agent.as_str() == "alpha")
    );
    assert!(step.diagnostics.is_empty());
    assert_eq!(result.rules, rules());
    assert_eq!(result.candidate_rules, candidate_rules());
    assert_eq!(
        result.basis,
        *ResolutionSnapshot::capture(&input)
            .unwrap()
            .request()
            .basis()
    );
}

#[test]
fn unowned_unlinked_skill_never_gets_an_arbitrary_agent() {
    let input = input(None, false, false);
    let step = &bind(&input, &rules()).steps[0];
    assert!(step.primary_candidates.is_empty());
    assert!(step.diagnostics.contains(&AgentDiagnostic::UnboundSkill(
        SkillId::new("skill").unwrap()
    )));
    assert!(step.diagnostics.contains(&AgentDiagnostic::MissingPrimary));
    assert!(step.responsibilities.values().all(BTreeSet::is_empty));
}

#[test]
fn primary_and_required_participants_keep_distinct_capability_responsibilities() {
    let input = input(None, true, true);
    let id = input.plan.steps()[0].id().clone();
    let mut r = rules();
    r.primary.insert(id.clone(), AgentId::new("alpha").unwrap());
    r.participants
        .insert(id.clone(), BTreeSet::from([AgentId::new("beta").unwrap()]));
    let result = bind(&input, &r);
    assert_eq!(
        result.steps[0].primary_candidates,
        BTreeSet::from([AgentId::new("alpha").unwrap()])
    );
    assert!(
        result.steps[0]
            .required_participants
            .contains(&AgentId::new("beta").unwrap())
    );
    assert!(result.steps[0].diagnostics.is_empty());
    r.primary
        .insert(id.clone(), AgentId::new("unknown").unwrap());
    assert!(
        bind(&input, &r).steps[0]
            .diagnostics
            .contains(&AgentDiagnostic::IncompatiblePrimary(
                AgentId::new("unknown").unwrap()
            ))
    );
    r.participants
        .insert(id, BTreeSet::from([AgentId::new("unknown").unwrap()]));
    assert!(
        bind(&input, &r).steps[0]
            .diagnostics
            .contains(&AgentDiagnostic::MissingParticipant(
                AgentId::new("unknown").unwrap()
            ))
    );
}

fn role_input(primary: &str, capability: &str) -> ResolutionSnapshotInput {
    let mut input = input(None, true, true);
    input.processes = ProcessRegistry::from_sources([ProcessSource::new("roles.feature", format!(
        "@process(roles)\n@process-version(1)\n@cg-language(1)\nFeature: Synthetic role fixture\n  Rule: Process\n    Given state START is initial\n    Given state END is terminal\n    Given event finish\n    Given activity inspect requires capability {capability}\n    Given activity inspect constrained by primary-agent={primary}\n    Given activity inspect constrained by participating-agent=beta\n    Given activity inspect constrained by lifecycle=explicit\n\n    Scenario: finish\n      Given process state START\n      When event finish occurs\n      Then transition to state END\n      Then complete process\n"
    ))]).unwrap();
    input
}

fn role_rules(input: &ResolutionSnapshotInput) -> AgentRules {
    let mut r = rules();
    r.process_roles.insert(
        input.plan.steps()[0].id().clone(),
        ProcessRoleReference {
            definition: input
                .processes
                .definitions()
                .next()
                .unwrap()
                .identity()
                .clone(),
            activity: ActivityId::new("inspect").unwrap(),
        },
    );
    r
}

#[test]
fn canonical_process_roles_cannot_bypass_capabilities_or_conflicting_primary() {
    let input = role_input("alpha", "architecture.dependency-analysis");
    let mut r = role_rules(&input);
    let result = bind(&input, &r);
    assert_eq!(
        result.steps[0].primary_candidates,
        BTreeSet::from([AgentId::new("alpha").unwrap()])
    );
    assert_eq!(
        result.steps[0]
            .process_activity
            .as_ref()
            .unwrap()
            .constraints()
            .len(),
        3
    );
    r.primary.insert(
        input.plan.steps()[0].id().clone(),
        AgentId::new("beta").unwrap(),
    );
    assert!(
        bind(&input, &r).steps[0]
            .diagnostics
            .contains(&AgentDiagnostic::ConflictingPrimary)
    );
    let wrong = role_input("alpha", "different.capability");
    assert!(
        bind(&wrong, &role_rules(&wrong)).steps[0]
            .diagnostics
            .iter()
            .any(|d| matches!(d, AgentDiagnostic::ProcessCapabilityMismatch(_)))
    );
    let malformed = role_input("../invalid", "architecture.dependency-analysis");
    assert_eq!(
        bind_agent_candidates(
            &ResolutionSnapshot::capture(&malformed).unwrap(),
            &candidate_rules(),
            &role_rules(&malformed)
        ),
        Err(AgentBindingError::InvalidRole)
    );
}

#[test]
fn invalid_rules_and_process_refs_fail_closed() {
    let input = support::with_process(support::fixture());
    let snapshot = ResolutionSnapshot::capture(&input).unwrap();
    let mut r = rules();
    r.version = SchemaVersion::V2;
    assert_eq!(
        bind_agent_candidates(&snapshot, &candidate_rules(), &r),
        Err(AgentBindingError::UnsupportedVersion)
    );
    let mut r = rules();
    r.primary.insert(
        PlanStepId::new("unknown").unwrap(),
        AgentId::new("alpha").unwrap(),
    );
    assert_eq!(
        bind_agent_candidates(&snapshot, &candidate_rules(), &r),
        Err(AgentBindingError::UnknownStep)
    );
    let mut candidate = candidate_rules();
    candidate.version = SchemaVersion::V2;
    assert_eq!(
        bind_agent_candidates(&snapshot, &candidate, &rules()),
        Err(AgentBindingError::InvalidCandidateRules)
    );
    let mut r = role_rules(&input);
    assert_eq!(
        bind_agent_candidates(&snapshot, &candidate_rules(), &r),
        Err(AgentBindingError::InvalidProcessReference)
    );
    let definition = input.processes.definitions().last().unwrap();
    r.process_roles.values_mut().next().unwrap().definition = definition.identity().clone();
    assert_eq!(
        bind_agent_candidates(&snapshot, &candidate_rules(), &r),
        Err(AgentBindingError::InvalidProcessReference)
    );
}

#[test]
fn noop_has_no_artificial_primary() {
    let mut input = input(None, false, false);
    let old = &input.delta.items()[0];
    let item = DeltaItem::new(
        old.id().clone(),
        input.desired.id().clone(),
        old.condition().clone(),
        DeltaKind::Satisfied,
        old.basis().clone(),
        RequiredOutcome::new(RequiredOutcomeKind::NoOp, "already satisfied").unwrap(),
        "satisfied",
    )
    .unwrap();
    input.delta = Delta::new(
        input.delta.id().clone(),
        input.desired.id().clone(),
        Some(input.situation.situation().id().clone()),
        vec![item],
    )
    .unwrap();
    input.plan = gateway_domain::plan(&input.desired, &input.delta, &[], &PlannerRules::default())
        .unwrap()
        .plan()
        .unwrap()
        .clone();
    let result = bind(&input, &rules());
    assert!(
        result
            .steps
            .iter()
            .all(|s| s.primary_candidates.is_empty() && s.responsibilities.is_empty())
    );
}
