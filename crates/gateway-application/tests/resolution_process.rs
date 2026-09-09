use gateway_application::{
    resolution::RequirementAlternatives, resolution_process::*, resolution_snapshot::*,
};
use gateway_domain::*;
use gateway_process::{
    ActivityConstraint, ActivityId, EvidenceTypeId, ProcessRegistry, ProcessSource,
};
use std::collections::{BTreeMap, BTreeSet};
mod support;

fn rules(preference: TemplatePreference) -> ProcessSelectionRules {
    ProcessSelectionRules {
        version: SchemaVersion::V1,
        preference,
        required_definition: None,
        activities: BTreeMap::new(),
        output_evidence: BTreeMap::new(),
        lifecycle_contracts: BTreeMap::new(),
    }
}

fn catalog(entries: &[(&str, u32)]) -> ProcessRegistry {
    ProcessRegistry::from_sources(entries.iter().map(|(id, version)| ProcessSource::new(format!("{id}-{version}.feature"), format!(
        "@process({id})\n@process-version({version})\n@cg-language(1)\nFeature: Synthetic resolution fixture\n  Rule: Process\n    Given state START is initial\n    Given state END is terminal\n    Given event finish\n    Given evidence proof\n    Given activity inspect requires capability architecture.dependency-analysis\n    Given activity inspect produces evidence proof\n    Given activity inspect constrained by lifecycle=HUMAN_INPUT\n\n    Scenario: finish work\n      Given process state START\n      When event finish occurs\n      Then transition to state END\n      Then complete process\n"
    )))).unwrap()
}

#[test]
fn optional_absence_required_absence_unique_and_tied_templates() {
    let mut input = support::fixture();
    let snapshot = ResolutionSnapshot::capture(&input).unwrap();
    let none = select_process(&snapshot, &rules(TemplatePreference::None)).unwrap();
    assert_eq!(none.outcome, ProcessSelectionOutcome::NoTemplate);
    assert!(none.candidates.is_empty());
    assert_eq!(none.basis, *snapshot.request().basis());
    assert_eq!(none.rules, rules(TemplatePreference::None));
    input.processes = catalog(&[]);
    let snapshot = ResolutionSnapshot::capture(&input).unwrap();
    let optional = select_process(&snapshot, &rules(TemplatePreference::Optional)).unwrap();
    assert_eq!(optional.outcome, ProcessSelectionOutcome::NoTemplate);
    assert!(optional.rejections.is_empty());
    assert_eq!(
        select_process(&snapshot, &rules(TemplatePreference::Required))
            .unwrap()
            .outcome,
        ProcessSelectionOutcome::Missing
    );
    input.processes = catalog(&[]);
    assert_eq!(
        select_process(
            &ResolutionSnapshot::capture(&input).unwrap(),
            &rules(TemplatePreference::Required)
        )
        .unwrap()
        .outcome,
        ProcessSelectionOutcome::Missing
    );
    input.processes = catalog(&[("one", 1)]);
    let unique = select_process(
        &ResolutionSnapshot::capture(&input).unwrap(),
        &rules(TemplatePreference::Optional),
    )
    .unwrap();
    assert_eq!(unique.outcome, ProcessSelectionOutcome::Unique);
    assert!(unique.candidates[0].binding.instance.is_none());
    let activity = &unique.candidates[0].activities.values().next().unwrap()[0];
    assert_eq!(
        activity.capabilities()[0].as_str(),
        "architecture.dependency-analysis"
    );
    assert_eq!(activity.output_evidence()[0].as_str(), "proof");
    assert_eq!(activity.constraints()[0].name(), "lifecycle");
    input.processes = catalog(&[("two", 1), ("one", 1)]);
    let tied = select_process(
        &ResolutionSnapshot::capture(&input).unwrap(),
        &rules(TemplatePreference::Required),
    )
    .unwrap();
    assert_eq!(tied.outcome, ProcessSelectionOutcome::Ambiguous);
    assert_eq!(tied.candidates.len(), 2);
    input.processes = catalog(&[("one", 1), ("two", 1)]);
    assert_eq!(
        tied,
        select_process(
            &ResolutionSnapshot::capture(&input).unwrap(),
            &rules(TemplatePreference::Required)
        )
        .unwrap()
    );
}

#[test]
fn explicit_activity_and_output_constraints_are_not_bypassed() {
    let mut input = support::fixture();
    input.processes = catalog(&[("one", 1)]);
    let snapshot = ResolutionSnapshot::capture(&input).unwrap();
    let id = input.plan.steps()[0].id().clone();
    let mut r = rules(TemplatePreference::None);
    r.activities
        .insert(id.clone(), ActivityId::new("missing").unwrap());
    assert_eq!(
        select_process(&snapshot, &r).unwrap().outcome,
        ProcessSelectionOutcome::Incompatible
    );
    r.activities
        .insert(id.clone(), ActivityId::new("inspect").unwrap());
    r.output_evidence.insert(
        id.clone(),
        BTreeSet::from([EvidenceTypeId::new("missing").unwrap()]),
    );
    assert_eq!(
        select_process(&snapshot, &r).unwrap().outcome,
        ProcessSelectionOutcome::Incompatible
    );
    r.output_evidence
        .insert(id, BTreeSet::from([EvidenceTypeId::new("proof").unwrap()]));
    assert_eq!(
        select_process(&snapshot, &r).unwrap().outcome,
        ProcessSelectionOutcome::Unique
    );
    r.version = SchemaVersion::V2;
    assert_eq!(
        select_process(&snapshot, &r),
        Err(ProcessSelectionError::UnsupportedVersion)
    );
    r.version = SchemaVersion::V1;
    r.activities.insert(
        PlanStepId::new("unknown").unwrap(),
        ActivityId::new("inspect").unwrap(),
    );
    assert_eq!(
        select_process(&snapshot, &r),
        Err(ProcessSelectionError::UnknownStep)
    );
}

#[test]
fn active_instance_never_upgrades_or_migrates() {
    let mut input = support::fixture();
    input.processes = catalog(&[("one", 1), ("one", 2)]);
    let input = support::with_process(input);
    let snapshot = ResolutionSnapshot::capture(&input).unwrap();
    let selected = select_process(&snapshot, &rules(TemplatePreference::Optional)).unwrap();
    assert_eq!(selected.outcome, ProcessSelectionOutcome::Unique);
    assert_eq!(
        selected.candidates[0].binding.definition.version().value(),
        1
    );
    assert_eq!(
        selected.candidates[0].binding.instance.as_ref().unwrap().0,
        *input.instance.as_ref().unwrap().id()
    );
    assert!(
        selected
            .rejections
            .iter()
            .any(|r| r.reason == ProcessRejectionReason::PinnedDefinition)
    );
    let mut r = rules(TemplatePreference::Optional);
    r.required_definition = Some(
        input
            .processes
            .definitions()
            .last()
            .unwrap()
            .identity()
            .clone(),
    );
    assert_eq!(
        select_process(&snapshot, &r).unwrap().outcome,
        ProcessSelectionOutcome::Incompatible
    );
    assert_eq!(snapshot.input().instance, input.instance);
}

#[test]
fn lifecycle_mapping_requires_explicit_canonical_contract() {
    let mut input = support::fixture();
    input.processes = catalog(&[("one", 1)]);
    let steps = input
        .plan
        .steps()
        .iter()
        .cloned()
        .map(|s| {
            s.with_lifecycle_requirement(
                LifecycleRequirement::new(
                    LifecycleRequirementKind::HumanInput,
                    "requires explicit lifecycle",
                )
                .unwrap(),
            )
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
    let snapshot = ResolutionSnapshot::capture(&input).unwrap();
    let mut r = rules(TemplatePreference::None);
    assert_eq!(
        select_process(&snapshot, &r).unwrap().outcome,
        ProcessSelectionOutcome::Unsupported
    );
    r.lifecycle_contracts.insert(
        LifecycleRequirementKind::HumanInput,
        ActivityConstraint::new("lifecycle", "WRONG").unwrap(),
    );
    assert_eq!(
        select_process(&snapshot, &r).unwrap().outcome,
        ProcessSelectionOutcome::Incompatible
    );
    r.lifecycle_contracts.insert(
        LifecycleRequirementKind::HumanInput,
        ActivityConstraint::new("lifecycle", "HUMAN_INPUT").unwrap(),
    );
    assert_eq!(
        select_process(&snapshot, &r).unwrap().outcome,
        ProcessSelectionOutcome::Unique
    );
}

#[test]
fn mandatory_alternative_groups_are_not_flattened_into_all_of() {
    let mut input = support::fixture();
    input.processes = catalog(&[("one", 1)]);
    let original = input.plan.capability_requirements()[0].clone();
    let second = CapabilityRequirement::new(
        CapabilityRequirementId::new("second").unwrap(),
        CapabilityId::new("unprovided").unwrap(),
        RequirementCardinality::Mandatory,
        original.originating_delta_item().clone(),
        "explicit alternative",
    )
    .unwrap();
    let ids = vec![original.id().clone(), second.id().clone()];
    let step = input.plan.steps()[0]
        .clone()
        .with_capability_requirements(ids.clone())
        .unwrap();
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.desired.id().clone(),
        input.delta.id().clone(),
        vec![original, second],
        vec![step],
    )
    .unwrap();
    assert_eq!(
        select_process(
            &ResolutionSnapshot::capture(&input).unwrap(),
            &rules(TemplatePreference::Required)
        )
        .unwrap()
        .outcome,
        ProcessSelectionOutcome::Incompatible
    );
    input.alternatives.push(RequirementAlternatives {
        step: input.plan.steps()[0].id().clone(),
        members: ids.into_iter().collect(),
        cardinality: RequirementCardinality::Mandatory,
    });
    assert_eq!(
        select_process(
            &ResolutionSnapshot::capture(&input).unwrap(),
            &rules(TemplatePreference::Required)
        )
        .unwrap()
        .outcome,
        ProcessSelectionOutcome::Unique
    );
}
