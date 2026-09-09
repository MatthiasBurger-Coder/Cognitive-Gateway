use gateway_application::resolution_explain::SourceKind;
use gateway_application::{
    resolution::*, resolution_composition::*, resolution_explain::*, resolution_skills::*,
    resolution_snapshot::*,
};
use gateway_domain::*;
use std::collections::{BTreeMap, BTreeSet};
#[path = "support/composition.rs"]
mod composition;
mod support;

fn limits() -> TraceLimits {
    TraceLimits {
        max_nodes: 10000,
        max_optional_details: 1000,
    }
}

fn golden(case: &str, trace: &ResolutionTrace) {
    let fixtures: BTreeMap<String, BTreeSet<String>> =
        serde_json::from_str(include_str!("fixtures/resolution-trace-golden.json")).unwrap();
    assert_eq!(
        trace
            .nodes
            .iter()
            .map(|n| n.code.clone())
            .collect::<BTreeSet<_>>(),
        fixtures[case],
        "golden case {case}"
    );
}

#[test]
fn dag_and_completion_evidence_remain_reference_only() {
    use gateway_application::resolution_applicability::CompletionEvidence;
    let mut input = composition::fixture();
    let original = input.plan.steps()[0].clone();
    let dependent = PlanStep::new(
        PlanStepId::new("dependent").unwrap(),
        original.kind(),
        original.outcome().clone(),
        original.completion().clone(),
        "synthetic dependent",
    )
    .unwrap()
    .with_dependencies(vec![original.id().clone()])
    .unwrap()
    .with_capability_requirements(original.capability_requirements().to_vec())
    .unwrap()
    .with_delta_items(original.delta_items().to_vec())
    .unwrap();
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.desired.id().clone(),
        input.delta.id().clone(),
        input.plan.capability_requirements().to_vec(),
        vec![original.clone(), dependent],
    )
    .unwrap();
    let mut rules = composition::rules();
    rules
        .provider_priorities
        .insert(composition::skill("good"), 10);
    let graph = trace(&input, &rules);
    assert!(graph.edges.iter().any(|e| e.relation == "DEPENDS_ON"));
    assert!(graph.nodes.iter().any(|n| n.code == "PREDECESSOR_PENDING"));
    let snapshot = ResolutionSnapshot::capture(&input).unwrap();
    rules.applicability.completed.insert(
        original.id().clone(),
        CompletionEvidence {
            basis: snapshot.request().basis().clone(),
            contracts: BTreeSet::from([original.completion().clone()]),
            references: BTreeSet::from([EvidenceId::new("receipt-id").unwrap()]),
            status: ConditionStatus::Satisfied,
            freshness: FreshnessStatus::Fresh,
        },
    );
    let completed = trace(&input, &rules);
    assert!(completed.nodes.iter().any(
        |n| n.code == "COMPLETION_ATTESTATION_REFERENCE" && n.source.reference == "receipt-id"
    ));
    assert!(completed.edges.iter().any(|e| e.relation == "BOUND_AS"));
}

#[test]
fn process_blocking_and_contract_rejections_retain_authoritative_references() {
    use gateway_application::{DeclarativeSituationApplication, ProcessSnapshotInput};
    use gateway_process::{ProcessInstance, ProcessRegistry, ProcessSource};
    let mut input = composition::fixture();
    input.processes = ProcessRegistry::from_sources(["one","two"].map(|id| ProcessSource::new(format!("{id}.feature"),format!(
        "@process({id})\n@process-version(1)\n@cg-language(1)\nFeature: Synthetic trace\nRule: Process\nGiven state START is initial\nGiven state END is terminal\nGiven event finish\nGiven activity inspect requires capability architecture.dependency-analysis\nScenario: finish\nGiven process state START\nWhen event finish occurs\nThen transition to state END\nThen authorize activity inspect\nThen complete process\n")))).unwrap();
    input = support::with_process(input);
    let mut instance: serde_json::Value =
        serde_json::from_str(&input.instance.as_ref().unwrap().to_json().unwrap()).unwrap();
    instance["status"] = "PAUSED".into();
    instance["waiting_condition"] =
        serde_json::json!({"reason":"HUMAN_REVIEW","detail":"secret human instructions"});
    let instance = ProcessInstance::from_json(&instance.to_string()).unwrap();
    input.situation_process = Some(
        DeclarativeSituationApplication::new()
            .process_reference(ProcessSnapshotInput::new(
                input.processes.definitions().next().unwrap(),
                &instance,
            ))
            .unwrap(),
    );
    input.instance = Some(instance);
    let mut rules = composition::rules();
    rules
        .provider_priorities
        .insert(composition::agent("alpha"), 10);
    let graph = trace(&input, &rules);
    for code in [
        "SELECTED",
        "PROCESS_REJECTED",
        "PROCESS_STATUS",
        "PROCESS_WAITING",
    ] {
        assert!(graph.nodes.iter().any(|n| n.code == code));
    }
    assert!(!graph.to_json().contains("secret human instructions"));
    rules.candidates.selectors.insert(
        input.plan.capability_requirements()[0].id().clone(),
        BTreeSet::from([gateway_registry::CapabilitySelector::Constraint(
            CapabilityConstraint::new("unsupported-contract").unwrap(),
        )]),
    );
    let rejected = trace(&input, &rules);
    assert!(rejected.nodes.iter().any(|n| n.code == "CONTRACT_REJECTED"));
    assert!(
        rejected
            .nodes
            .iter()
            .any(|n| n.code == "INCOMPATIBLE_CONTRACT")
    );
}
fn trace(input: &ResolutionSnapshotInput, rules: &CompositionRules) -> ResolutionTrace {
    let report = composition::run(input, rules);
    explain_resolution(
        &ResolutionSnapshot::capture(input).unwrap(),
        &report,
        limits(),
    )
    .unwrap()
}

#[test]
fn selected_ambiguous_rejected_dependency_and_no_template_share_one_trace() {
    let input = composition::fixture();
    let mut rules = composition::rules();
    rules
        .provider_priorities
        .insert(composition::skill("good"), 10);
    let selected = trace(&input, &rules);
    golden("selected", &selected);
    assert_eq!(selected.outcome, "RESOLVED");
    assert_eq!(selected.policy_authorization, "NOT_EVALUATED");
    for code in [
        "SELECTED",
        "NO_TEMPLATE_REQUIRED",
        "CANONICAL_PROVIDER",
        "EFFECTIVE_SKILL",
        "DEPENDENCY",
        "RESPONSIBILITY",
        "DELTA_ITEM_REFERENCE",
        "NOT_GLOBALLY_SELECTED",
    ] {
        assert!(
            selected.nodes.iter().any(|n| n.code == code),
            "missing {code}"
        );
    }
    let json: ResolutionTrace = serde_json::from_str(&selected.to_json()).unwrap();
    assert_eq!(json, selected);
    let text = selected.to_text();
    for node in &selected.nodes {
        assert!(text.contains(&node.id));
        assert!(text.contains(&node.code));
    }
    for edge in &selected.edges {
        assert!(selected.nodes.iter().any(|n| n.id == edge.from));
        assert!(selected.nodes.iter().any(|n| n.id == edge.to));
    }
    rules.provider_priorities.clear();
    let ambiguous = trace(&input, &rules);
    golden("ambiguous", &ambiguous);
    assert_eq!(ambiguous.outcome, "AMBIGUOUS");
    assert!(
        ambiguous
            .nodes
            .iter()
            .any(|n| n.code == "EQUAL_RANK_ALTERNATIVE")
    );
    assert_ne!(selected.rule_fingerprint, ambiguous.rule_fingerprint);
    let mut skills = composition::skill_rules();
    skills
        .conditions
        .insert(SkillId::new("dependency").unwrap(), SkillCondition::Never);
    rules
        .skills
        .insert(input.plan.steps()[0].id().clone(), skills);
    let rejected = trace(&input, &rules);
    golden("rejected", &rejected);
    assert!(rejected.nodes.iter().any(|n| n.code == "BINDING_REJECTED"));
    assert!(
        rejected
            .nodes
            .iter()
            .any(|n| n.code == "SKILL_CONDITION" && n.attributes["status"] == "UNSATISFIED")
    );
    let mut nested = input.clone();
    composition::edit_skill(
        &mut nested,
        "good",
        "required_capability_ids",
        serde_json::json!(["nested"]),
    );
    assert!(
        trace(&nested, &rules)
            .nodes
            .iter()
            .any(|n| n.code == "REQUIRED_NOT_APPROVED")
    );
    rules
        .provider_priorities
        .insert(composition::agent("alpha"), 20);
    assert_eq!(trace(&input, &rules).outcome, "RESOLVED");
}

#[test]
fn secret_conditions_are_hashed_and_optional_details_never_displace_required_trace() {
    let input = composition::fixture();
    let mut rules = composition::rules();
    let id = input.plan.steps()[0].id().clone();
    rules.applicability.restrictions.insert(
        id.clone(),
        BTreeMap::from([(
            "secret-token-do-not-copy".into(),
            vec![SkillCondition::Never, SkillCondition::Always],
        )]),
    );
    let s = ResolutionSnapshot::capture(&input).unwrap();
    let report = composition::run(&input, &rules);
    let full = explain_resolution(&s, &report, limits()).unwrap();
    assert_eq!(full.outcome, "CONFLICTING");
    assert!(!full.to_text().contains("secret-token-do-not-copy"));
    assert!(!full.to_json().contains("secret-token-do-not-copy"));
    let compact = explain_resolution(
        &s,
        &report,
        TraceLimits {
            max_nodes: 10000,
            max_optional_details: 0,
        },
    )
    .unwrap();
    assert!(compact.omitted_optional_details > 0);
    assert!(
        compact
            .nodes
            .iter()
            .any(|n| n.code == "INTRINSIC_RESTRICTION")
    );
    let exact = explain_resolution(
        &s,
        &report,
        TraceLimits {
            max_nodes: compact.nodes.len(),
            max_optional_details: 1000,
        },
    )
    .unwrap();
    assert_eq!(exact, compact);
    assert_eq!(
        explain_resolution(
            &s,
            &report,
            TraceLimits {
                max_nodes: 1,
                max_optional_details: 0
            }
        ),
        Err(TraceError::RequiredTraceLimit)
    );
    assert_eq!(
        explain_resolution(
            &s,
            &report,
            TraceLimits {
                max_nodes: 0,
                max_optional_details: 0
            }
        ),
        Err(TraceError::InvalidLimit)
    );
    rules
        .applicability
        .restrictions
        .get_mut(&id)
        .unwrap()
        .get_mut("secret-token-do-not-copy")
        .unwrap()
        .reverse();
    assert_eq!(full, trace(&input, &rules));
}

#[test]
fn stale_tampered_search_incomplete_and_noop_are_never_explained_as_authorized() {
    let mut input = composition::fixture();
    let mut rules = composition::rules();
    let s = ResolutionSnapshot::capture(&input).unwrap();
    let mut report = composition::run(&input, &rules);
    report.outcome = ResolutionOutcome::Resolved;
    assert_eq!(
        explain_resolution(&s, &report, limits()),
        Err(TraceError::InvalidArtifact)
    );
    report.rules.version = SchemaVersion::new(2, 0).unwrap();
    assert_eq!(
        explain_resolution(&s, &report, limits()),
        Err(TraceError::InvalidArtifact)
    );
    report.basis.scope = ContextScopeId::new("other").unwrap();
    assert_eq!(
        explain_resolution(&s, &report, limits()),
        Err(TraceError::StaleBasis)
    );
    rules.max_visits = 1;
    let incomplete = trace(&input, &rules);
    assert_eq!(incomplete.outcome, "SEARCH_LIMIT");
    assert!(!incomplete.search_complete);
    assert!(
        incomplete
            .nodes
            .iter()
            .any(|n| n.code == "SEARCH_INCOMPLETE")
    );
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
    let noop = trace(&input, &composition::rules());
    golden("noop", &noop);
    assert_eq!(noop.outcome, "NO_OP");
    assert!(
        noop.nodes
            .iter()
            .all(|n| n.source.kind != SourceKind::Binding)
    );
    let codes: BTreeSet<_> = noop.nodes.iter().map(|n| n.code.as_str()).collect();
    assert_eq!(
        codes,
        BTreeSet::from([
            "NO_OP",
            "SOURCE_REFERENCE",
            "SCOPED_PROVENANCE_REFERENCE",
            "LEXICOGRAPHIC_INTEGER_PRIORITY_OPTIONAL_COUNT",
            "NO_TEMPLATE_REQUIRED"
        ])
    );
}
