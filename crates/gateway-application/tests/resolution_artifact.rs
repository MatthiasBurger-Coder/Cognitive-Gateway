use gateway_application::{
    resolution::*, resolution_artifact::*, resolution_composition::*, resolution_skills::*,
    resolution_snapshot::*,
};
use gateway_domain::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[path = "support/composition.rs"]
mod composition;
mod support;

fn roundtrip(
    input: &ResolutionSnapshotInput,
    rules: &CompositionRules,
) -> (ResolutionSnapshot, CompositionReport, String) {
    let snapshot = ResolutionSnapshot::capture(input).unwrap();
    let report = composition::run(input, rules);
    validate_resolution(&snapshot, &report, ArtifactLimits::default()).unwrap();
    let text = serialize_resolution(&snapshot, &report, ArtifactLimits::default()).unwrap();
    assert_eq!(
        parse_resolution(&snapshot, rules, &text, ArtifactLimits::default()).unwrap(),
        report
    );
    assert_eq!(
        serialize_resolution(&snapshot, &report, ArtifactLimits::default()).unwrap(),
        text
    );
    (snapshot, report, text)
}
fn rehash(value: &mut Value) {
    value
        .as_object_mut()
        .unwrap()
        .remove("artifact_fingerprint");
    value["artifact_fingerprint"] =
        json!(ContentFingerprint::of_bytes(value.to_string().as_bytes()).as_str());
}

#[test]
fn missing_partial_and_unsupported_work_remain_non_executable_on_roundtrip() {
    let mut input = composition::fixture();
    let original = input.plan.steps()[0].clone();
    let original_req = input.plan.capability_requirements()[0].clone();
    let missing = CapabilityRequirement::new(
        CapabilityRequirementId::new("missing").unwrap(),
        CapabilityId::new("unknown").unwrap(),
        RequirementCardinality::Mandatory,
        original_req.originating_delta_item().clone(),
        "missing requirement",
    )
    .unwrap();
    let separate = PlanStep::new(
        PlanStepId::new("other").unwrap(),
        original.kind(),
        original.outcome().clone(),
        original.completion().clone(),
        "independent",
    )
    .unwrap()
    .with_capability_requirements(vec![missing.id().clone()])
    .unwrap()
    .with_delta_items(original.delta_items().to_vec())
    .unwrap();
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.desired.id().clone(),
        input.delta.id().clone(),
        vec![original_req.clone(), missing.clone()],
        vec![original.clone(), separate.clone()],
    )
    .unwrap();
    assert_eq!(
        roundtrip(&input, &composition::rules()).1.outcome,
        ResolutionOutcome::Partial
    );
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.desired.id().clone(),
        input.delta.id().clone(),
        vec![missing],
        vec![separate],
    )
    .unwrap();
    assert_eq!(
        roundtrip(&input, &composition::rules()).1.outcome,
        ResolutionOutcome::Missing
    );
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.desired.id().clone(),
        input.delta.id().clone(),
        vec![original_req],
        vec![
            original.with_lifecycle_requirement(
                LifecycleRequirement::new(
                    LifecycleRequirementKind::HumanInput,
                    "explicit human input",
                )
                .unwrap(),
            ),
        ],
    )
    .unwrap();
    assert_eq!(
        roundtrip(&input, &composition::rules()).1.outcome,
        ResolutionOutcome::Unsupported
    );
}

#[test]
fn complete_ambiguous_conflicting_search_limit_no_template_and_noop_roundtrip() {
    let input = composition::fixture();
    let mut rules = composition::rules();
    let (_, report, _) = roundtrip(&input, &rules);
    assert_eq!(report.outcome, ResolutionOutcome::Ambiguous);
    rules
        .provider_priorities
        .insert(composition::skill("good"), 10);
    let (_, report, text) = roundtrip(&input, &rules);
    assert_eq!(report.outcome, ResolutionOutcome::Resolved);
    assert!(text.contains("NOT_EVALUATED"));
    rules.applicability.semantics.clear();
    assert_eq!(
        roundtrip(&input, &rules).1.outcome,
        ResolutionOutcome::Conflicting
    );
    rules = composition::rules();
    rules.max_visits = 1;
    assert_eq!(
        roundtrip(&input, &rules).1.outcome,
        ResolutionOutcome::SearchLimit
    );
    let mut noop = input.clone();
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
    noop.plan = gateway_domain::plan(&noop.desired, &noop.delta, &[], &PlannerRules::default())
        .unwrap()
        .plan()
        .unwrap()
        .clone();
    assert_eq!(
        roundtrip(&noop, &composition::rules()).1.outcome,
        ResolutionOutcome::NoOp
    );
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
    roundtrip(&noop, &composition::rules());
}

#[test]
fn tampered_contracts_closure_references_status_and_unknown_fields_fail_even_with_rehashed_content()
{
    let mut input = composition::fixture();
    composition::edit_skill(
        &mut input,
        "good",
        "required_capability_ids",
        json!(["nested"]),
    );
    let mut rules = composition::rules();
    rules
        .provider_priorities
        .insert(composition::skill("good"), 10);
    let mut sr = composition::skill_rules();
    roundtrip(&input, &rules);
    sr.capability_providers.insert(
        CapabilityId::new("nested").unwrap(),
        composition::agent("alpha"),
    );
    rules.skills.insert(input.plan.steps()[0].id().clone(), sr);
    let (snapshot, report, text) = roundtrip(&input, &rules);
    let original: Value = serde_json::from_str(&text).unwrap();
    for mutation in 0..8 {
        let mut wire = original.clone();
        match mutation {
            0 => wire["report"]["outcome"] = json!("MISSING"),
            1 => wire["report"]["alternatives"][0][0]["binding"]["primary_agent"] = json!("forged"),
            2 => wire["report"]["alternatives"][0][0]["skills"]["skills"] = json!([]),
            3 => wire["trace"]["policy_authorization"] = json!("ALLOW"),
            4 => wire["trace"]["nodes"][0]["source"]["reference"] = json!("dangling"),
            5 => {
                for c in wire["report"]["discovery"][0]["candidates"]
                    .as_array_mut()
                    .unwrap()
                {
                    c["contract"]["class"] = json!("MUTATE");
                }
            }
            6 => {
                let duplicate = wire["report"]["steps"][0].clone();
                wire["report"]["steps"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            _ => wire["report"]["new_governance_field"] = json!("must not disappear"),
        }
        rehash(&mut wire);
        let error = parse_resolution(
            &snapshot,
            &rules,
            &wire.to_string(),
            ArtifactLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ArtifactError::InvalidArtifact
                | ArtifactError::FingerprintMismatch
                | ArtifactError::UnknownField
        ));
        if mutation == 0 {
            assert_eq!(error, ArtifactError::InvalidArtifact);
        }
        if mutation == 7 {
            assert_eq!(error, ArtifactError::UnknownField);
        }
    }
    let mut bad = report.clone();
    bad.outcome = ResolutionOutcome::Missing;
    assert_eq!(
        validate_resolution(&snapshot, &bad, ArtifactLimits::default()),
        Err(ArtifactError::InvalidArtifact)
    );
    assert_eq!(
        serialize_resolution(&snapshot, &bad, ArtifactLimits::default()),
        Err(ArtifactError::InvalidArtifact)
    );
    bad.basis.scope = ContextScopeId::new("other").unwrap();
    assert_eq!(
        validate_resolution(&snapshot, &bad, ArtifactLimits::default()),
        Err(ArtifactError::StaleBasis)
    );
    assert_eq!(
        serialize_resolution(&snapshot, &bad, ArtifactLimits::default()),
        Err(ArtifactError::StaleBasis)
    );
    bad = report;
    bad.rules.version = SchemaVersion::new(2, 0).unwrap();
    assert_eq!(
        validate_resolution(&snapshot, &bad, ArtifactLimits::default()),
        Err(ArtifactError::InvalidArtifact)
    );
}

#[test]
fn strict_json_versions_duplicates_basis_rules_and_size_depth_limits() {
    let input = composition::fixture();
    let rules = composition::rules();
    let (snapshot, report, text) = roundtrip(&input, &rules);
    for json in ["null", "true", "-1", "1.5", "\"text\"", "[]"] {
        assert_eq!(
            parse_resolution(&snapshot, &rules, json, ArtifactLimits::default()),
            Err(ArtifactError::UnsupportedVersion)
        );
    }
    for json in [
        "{",
        "{\"a\":1,\"a\":2}",
        "{\"a\":{\"x\":1,\"x\":2}}",
        "{} trailing",
    ] {
        assert_eq!(
            parse_resolution(&snapshot, &rules, json, ArtifactLimits::default()),
            Err(ArtifactError::MalformedJson)
        );
    }
    let original: Value = serde_json::from_str(&text).unwrap();
    let mut wire = original.clone();
    wire["version"] = json!(2);
    assert_eq!(
        parse_resolution(
            &snapshot,
            &rules,
            &wire.to_string(),
            ArtifactLimits::default()
        ),
        Err(ArtifactError::UnsupportedVersion)
    );
    wire = original.clone();
    wire["basis"]["process_state_fingerprint"] = json!("stale");
    assert_eq!(
        parse_resolution(
            &snapshot,
            &rules,
            &wire.to_string(),
            ArtifactLimits::default()
        ),
        Err(ArtifactError::StaleBasis)
    );
    wire = original.clone();
    wire["rule_fingerprint"] = json!("changed");
    assert_eq!(
        parse_resolution(
            &snapshot,
            &rules,
            &wire.to_string(),
            ArtifactLimits::default()
        ),
        Err(ArtifactError::RuleMismatch)
    );
    wire = original.clone();
    wire["artifact_fingerprint"] = json!("forged");
    assert_eq!(
        parse_resolution(
            &snapshot,
            &rules,
            &wire.to_string(),
            ArtifactLimits::default()
        ),
        Err(ArtifactError::FingerprintMismatch)
    );
    wire = original;
    wire.as_object_mut().unwrap().remove("artifact_fingerprint");
    assert_eq!(
        parse_resolution(
            &snapshot,
            &rules,
            &wire.to_string(),
            ArtifactLimits::default()
        ),
        Err(ArtifactError::FingerprintMismatch)
    );
    let tiny = ArtifactLimits {
        max_bytes: 1,
        ..ArtifactLimits::default()
    };
    assert_eq!(
        serialize_resolution(&snapshot, &report, tiny),
        Err(ArtifactError::SizeLimit)
    );
    assert_eq!(
        parse_resolution(&snapshot, &rules, &text, tiny),
        Err(ArtifactError::SizeLimit)
    );
    let nodes = ArtifactLimits {
        max_nodes: 1,
        ..ArtifactLimits::default()
    };
    assert_eq!(
        serialize_resolution(&snapshot, &report, nodes),
        Err(ArtifactError::GraphLimit)
    );
    assert_eq!(
        parse_resolution(&snapshot, &rules, &text, nodes),
        Err(ArtifactError::GraphLimit)
    );
    let shallow = ArtifactLimits {
        max_depth: 2,
        ..ArtifactLimits::default()
    };
    assert_eq!(
        serialize_resolution(&snapshot, &report, shallow),
        Err(ArtifactError::GraphLimit)
    );
    let invalid = ArtifactLimits {
        max_depth: 0,
        ..ArtifactLimits::default()
    };
    assert_eq!(
        parse_resolution(&snapshot, &rules, &text, invalid),
        Err(ArtifactError::InvalidLimits)
    );
    assert_eq!(
        serialize_resolution(&snapshot, &report, invalid),
        Err(ArtifactError::InvalidLimits)
    );
    let deep = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    assert_eq!(
        parse_resolution(&snapshot, &rules, &deep, ArtifactLimits::default()),
        Err(ArtifactError::GraphLimit)
    );
}

#[test]
fn canonical_sets_can_be_permuted_but_ordered_skill_paths_and_process_pins_cannot() {
    let input = support::with_process(composition::fixture());
    let mut rules = composition::rules();
    rules
        .provider_priorities
        .insert(composition::skill("bad"), 10);
    let (snapshot, _, text) = roundtrip(&input, &rules);
    let mut wire: Value = serde_json::from_str(&text).unwrap();
    let mut forged_pin = wire.clone();
    assert!(
        !forged_pin["report"]["processes"]["candidates"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    forged_pin["report"]["processes"]["candidates"][0]["binding"]["definition"]["digest"] =
        json!("forged-digest");
    rehash(&mut forged_pin);
    assert!(
        parse_resolution(
            &snapshot,
            &rules,
            &forged_pin.to_string(),
            ArtifactLimits::default()
        )
        .is_err()
    );
    wire["trace"]["nodes"].as_array_mut().unwrap().reverse();
    wire["report"]["steps"].as_array_mut().unwrap().reverse();
    // Fingerprint is semantic: permutation of these declared sets keeps it valid.
    parse_resolution(
        &snapshot,
        &rules,
        &wire.to_string(),
        ArtifactLimits::default(),
    )
    .unwrap();
    let mut without_process = composition::fixture();
    composition::edit_skill(
        &mut without_process,
        "good",
        "requires",
        json!(["dependency"]),
    );
    let mut r = composition::rules();
    r.provider_priorities.insert(composition::skill("good"), 10);
    let (s, _, text) = roundtrip(&without_process, &r);
    let mut wire: Value = serde_json::from_str(&text).unwrap();
    wire["report"]["alternatives"][0][0]["skills"]["skills"]
        .as_array_mut()
        .unwrap()
        .reverse();
    rehash(&mut wire);
    assert!(parse_resolution(&s, &r, &wire.to_string(), ArtifactLimits::default()).is_err());
    let mut sr = composition::skill_rules();
    sr.conditions
        .insert(SkillId::new("good").unwrap(), SkillCondition::Never);
    r.skills
        .insert(without_process.plan.steps()[0].id().clone(), sr);
    roundtrip(&without_process, &r);
    let mut omitted = composition::fixture();
    let original = &omitted.plan.capability_requirements()[0];
    let extra = CapabilityRequirement::new(
        CapabilityRequirementId::new("extra").unwrap(),
        CapabilityId::new("unknown").unwrap(),
        RequirementCardinality::Optional,
        original.originating_delta_item().clone(),
        "optional",
    )
    .unwrap();
    let reqs = vec![original.clone(), extra];
    let steps = vec![
        omitted.plan.steps()[0]
            .clone()
            .with_capability_requirements(reqs.iter().map(|r| r.id().clone()).collect())
            .unwrap(),
    ];
    omitted.plan = Plan::new(
        omitted.plan.id().clone(),
        omitted.desired.id().clone(),
        omitted.delta.id().clone(),
        reqs,
        steps,
    )
    .unwrap();
    r = composition::rules();
    r.candidates.selectors = BTreeMap::new();
    roundtrip(&omitted, &r);
    omitted.alternatives.push(RequirementAlternatives {
        step: omitted.plan.steps()[0].id().clone(),
        members: omitted
            .plan
            .capability_requirements()
            .iter()
            .map(|r| r.id().clone())
            .collect::<BTreeSet<_>>(),
        cardinality: RequirementCardinality::Mandatory,
    });
    roundtrip(&omitted, &r);
}
