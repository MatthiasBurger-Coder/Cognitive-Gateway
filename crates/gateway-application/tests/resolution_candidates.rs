use gateway_application::{resolution_candidates::*, resolution_snapshot::*};
use gateway_domain::*;
use gateway_registry::{CapabilitySelector, Registry};
use std::collections::{BTreeMap, BTreeSet};
mod support;

fn rules() -> CandidateRules {
    CandidateRules {
        version: SchemaVersion::V1,
        selectors: BTreeMap::new(),
    }
}

fn replace_requirement(
    input: &mut ResolutionSnapshotInput,
    capability: &str,
    cardinality: RequirementCardinality,
    preconditions: Vec<CapabilityPrecondition>,
    constraints: Vec<CapabilityConstraint>,
) {
    let original = &input.plan.capability_requirements()[0];
    let requirement = CapabilityRequirement::new_with_metadata(
        original.id().clone(),
        CapabilityId::new(capability).unwrap(),
        cardinality,
        original.originating_delta_item().clone(),
        preconditions,
        constraints,
        "explicit requirement",
    )
    .unwrap();
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.plan.desired_state().clone(),
        input.plan.delta().clone(),
        vec![requirement],
        input.plan.steps().to_vec(),
    )
    .unwrap();
}

#[test]
fn real_index_returns_all_providers_and_retains_unknown_applicability() {
    let input = support::with_process(support::fixture());
    let snapshot = ResolutionSnapshot::capture(&input).unwrap();
    let report = discover_candidates(&snapshot, &rules()).unwrap();
    assert_eq!(report.basis, *snapshot.request().basis());
    assert_eq!(report.rules, rules());
    let set = &report.sets[0];
    assert_eq!(set.outcome, CandidateOutcome::Compatible);
    assert!(set.candidates.len() >= 2);
    assert!(set.rejections.is_empty());
    assert_eq!(
        set.requirement,
        *input.plan.capability_requirements()[0].id()
    );
    assert_eq!(set.cardinality, RequirementCardinality::Mandatory);
    for candidate in &set.candidates {
        assert_eq!(
            candidate.canonical.capability().class(),
            CapabilityClass::Inspect
        );
        assert!(
            candidate
                .unresolved_preconditions
                .contains(&CapabilityPrecondition::new("repository.available").unwrap())
        );
        assert_eq!(candidate.definition_fingerprint.as_str().len(), 64);
        assert!(
            candidate
                .canonical
                .matched_selectors()
                .contains(&CapabilitySelector::Class(CapabilityClass::Inspect))
        );
    }
    assert_eq!(snapshot.input(), &input);
}

#[test]
fn class_input_output_and_intrinsic_selectors_reject_exactly() {
    let mut input = support::fixture();
    replace_requirement(
        &mut input,
        "architecture.dependency-analysis",
        RequirementCardinality::Mandatory,
        vec![CapabilityPrecondition::new("repository.available").unwrap()],
        vec![CapabilityConstraint::new("read-only").unwrap()],
    );
    let snapshot = ResolutionSnapshot::capture(&input).unwrap();
    let id = input.plan.capability_requirements()[0].id().clone();
    for selector in [
        CapabilitySelector::Class(CapabilityClass::Mutate),
        CapabilitySelector::InputKind(CapabilityInputKind::new("wrong.input").unwrap()),
        CapabilitySelector::OutputKind(CapabilityOutputKind::new("wrong.output").unwrap()),
        CapabilitySelector::Domain(CapabilityDomain::new("wrong.domain").unwrap()),
        CapabilitySelector::ApplicabilityTag(CapabilityTag::new("unknown-tag").unwrap()),
        CapabilitySelector::Precondition(CapabilityPrecondition::new("unknown-condition").unwrap()),
        CapabilitySelector::Constraint(CapabilityConstraint::new("unknown-constraint").unwrap()),
    ] {
        let mut rules = rules();
        rules
            .selectors
            .insert(id.clone(), BTreeSet::from([selector.clone()]));
        let result = discover_candidates(&snapshot, &rules).unwrap();
        assert_eq!(result.sets[0].outcome, CandidateOutcome::Incompatible);
        assert!(result.sets[0].candidates.is_empty());
        assert!(result.sets[0].rejections.len() >= 2);
        for rejected in &result.sets[0].rejections {
            assert!(rejected.failed_selectors().any(|s| s == &selector));
        }
    }
    let mut rules = rules();
    rules.selectors.insert(
        id,
        BTreeSet::from([
            CapabilitySelector::CapabilityId(
                CapabilityId::new("architecture.dependency-analysis").unwrap(),
            ),
            CapabilitySelector::InputKind(CapabilityInputKind::new("repository.snapshot").unwrap()),
            CapabilitySelector::OutputKind(
                CapabilityOutputKind::new("architecture.dependency-graph").unwrap(),
            ),
        ]),
    );
    assert_eq!(
        discover_candidates(&snapshot, &rules).unwrap().sets[0].outcome,
        CandidateOutcome::Compatible
    );
}

#[test]
fn required_capability_references_never_become_providers() {
    let mut input = support::fixture();
    replace_requirement(
        &mut input,
        "unprovided",
        RequirementCardinality::Mandatory,
        vec![],
        vec![],
    );
    let unknown =
        discover_candidates(&ResolutionSnapshot::capture(&input).unwrap(), &rules()).unwrap();
    assert_eq!(unknown.sets[0].outcome, CandidateOutcome::UnknownCapability);
    assert_eq!(
        unknown.sets[0].cardinality,
        RequirementCardinality::Mandatory
    );
    let mut skills = input.registry.skills().documents().to_vec();
    let mut wire: serde_json::Value = serde_json::from_str(&skills[0].to_json().unwrap()).unwrap();
    wire["required_capability_ids"] = serde_json::json!(["unprovided"]);
    skills[0] = SkillDefinitionDocument::from_json(&wire.to_string()).unwrap();
    input.registry =
        Registry::from_documents(input.registry.agents().documents().to_vec(), skills).unwrap();
    input.index = input.registry.capability_index().unwrap();
    let missing =
        discover_candidates(&ResolutionSnapshot::capture(&input).unwrap(), &rules()).unwrap();
    assert_eq!(missing.sets[0].outcome, CandidateOutcome::MissingProvider);
    assert!(missing.sets[0].candidates.is_empty());
}

#[test]
fn unknown_rules_and_hidden_capability_substitution_fail_closed() {
    let snapshot = ResolutionSnapshot::capture(&support::fixture()).unwrap();
    let mut invalid = rules();
    invalid.version = SchemaVersion::V2;
    assert_eq!(
        discover_candidates(&snapshot, &invalid),
        Err(CandidateError::UnsupportedVersion)
    );
    let mut invalid = rules();
    invalid.selectors.insert(
        CapabilityRequirementId::new("unknown").unwrap(),
        BTreeSet::new(),
    );
    assert_eq!(
        discover_candidates(&snapshot, &invalid),
        Err(CandidateError::UnknownRequirement)
    );
    let mut invalid = rules();
    invalid.selectors.insert(
        snapshot.request().plan().capability_requirements()[0]
            .id()
            .clone(),
        BTreeSet::from([CapabilitySelector::CapabilityId(
            CapabilityId::new("retrieved.fake-capability").unwrap(),
        )]),
    );
    assert_eq!(
        discover_candidates(&snapshot, &invalid),
        Err(CandidateError::CapabilitySubstitution)
    );
}

#[test]
fn reordered_catalog_and_rules_produce_identical_candidate_sets() {
    let mut input = support::fixture();
    let a = discover_candidates(&ResolutionSnapshot::capture(&input).unwrap(), &rules()).unwrap();
    let mut agents = input.registry.agents().documents().to_vec();
    agents.reverse();
    let mut skills = input.registry.skills().documents().to_vec();
    skills.reverse();
    input.registry = Registry::from_documents(agents, skills).unwrap();
    input.index = input.registry.capability_index().unwrap();
    assert_eq!(
        a,
        discover_candidates(&ResolutionSnapshot::capture(&input).unwrap(), &rules()).unwrap()
    );
}

#[test]
fn optional_no_match_is_preserved_without_substituting_mandatory_work() {
    let mut input = support::fixture();
    let optional = CapabilityRequirement::new(
        CapabilityRequirementId::new("optional").unwrap(),
        CapabilityId::new("unknown-optional").unwrap(),
        RequirementCardinality::Optional,
        input.delta.items()[0].id().clone(),
        "optional work",
    )
    .unwrap();
    let mut requirements = input.plan.capability_requirements().to_vec();
    requirements.push(optional);
    let steps = input
        .plan
        .steps()
        .iter()
        .cloned()
        .map(|s| {
            s.with_capability_requirements(requirements.iter().map(|r| r.id().clone()).collect())
                .unwrap()
        })
        .collect();
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.desired.id().clone(),
        input.delta.id().clone(),
        requirements,
        steps,
    )
    .unwrap();
    let report =
        discover_candidates(&ResolutionSnapshot::capture(&input).unwrap(), &rules()).unwrap();
    assert!(
        report
            .sets
            .iter()
            .any(|s| s.cardinality == RequirementCardinality::Optional
                && s.outcome == CandidateOutcome::UnknownCapability)
    );
    assert!(
        report
            .sets
            .iter()
            .any(|s| s.cardinality == RequirementCardinality::Mandatory
                && s.outcome == CandidateOutcome::Compatible)
    );
}

#[test]
fn mutation_outcomes_cannot_be_downgraded_to_inspection() {
    let mut input = support::fixture();
    let old = &input.delta.items()[0];
    let item = DeltaItem::new(
        old.id().clone(),
        input.desired.id().clone(),
        old.condition().clone(),
        DeltaKind::UnsatisfiedCondition,
        old.basis().clone(),
        RequiredOutcome::new(RequiredOutcomeKind::DomainChange, "change required").unwrap(),
        "change required",
    )
    .unwrap();
    input.delta = Delta::new(
        input.delta.id().clone(),
        input.desired.id().clone(),
        Some(input.situation.situation().id().clone()),
        vec![item],
    )
    .unwrap();
    let requirement = input.plan.capability_requirements()[0].clone();
    let verification = CapabilityRequirement::new(
        CapabilityRequirementId::new("verify").unwrap(),
        requirement.capability().clone(),
        RequirementCardinality::Mandatory,
        requirement.originating_delta_item().clone(),
        "verify change",
    )
    .unwrap();
    input.plan = gateway_domain::plan(
        &input.desired,
        &input.delta,
        &[requirement],
        &PlannerRules::default().with_verification_requirement(verification),
    )
    .unwrap()
    .plan()
    .unwrap()
    .clone();
    let report =
        discover_candidates(&ResolutionSnapshot::capture(&input).unwrap(), &rules()).unwrap();
    assert!(report.sets.iter().any(|s| {
        s.outcome == CandidateOutcome::Incompatible
            && s.query
                .selectors()
                .any(|s| s == &CapabilitySelector::Class(CapabilityClass::Mutate))
    }));
    assert!(
        report
            .sets
            .iter()
            .any(|s| s.outcome == CandidateOutcome::Compatible)
    );
}
