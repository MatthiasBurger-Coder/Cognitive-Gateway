use super::*;
use crate::{
    resolution_agents::ProcessRoleReference,
    resolution_applicability::CompletionEvidence,
    resolution_encoding::{rules_json, selector},
    resolution_process::*,
    resolution_skills::{ConditionStatus, SkillCondition},
};
use gateway_domain::*;
use gateway_process::{
    ActivityConstraint, ActivityId, EvidenceTypeId, GateStatus, ProcessInstanceStatus, StateId,
};
use gateway_registry::CapabilitySelector;
#[path = "composition.rs"]
mod composition;
#[path = "mod.rs"]
mod support;

#[test]
fn every_stable_diagnostic_projection_uses_typed_references_without_raw_text() {
    let skill = SkillId::new("skill").unwrap();
    let cap = CapabilityId::new("capability").unwrap();
    let step = PlanStepId::new("step").unwrap();
    let skill_cases = [
        SkillDiagnostic::MissingSkill(skill.clone()),
        SkillDiagnostic::Condition(skill.clone(), ConditionStatus::Conflicted, true),
        SkillDiagnostic::MissingCapability(cap.clone()),
        SkillDiagnostic::UnboundCapability(cap.clone()),
        SkillDiagnostic::InvalidCapabilityProvider(cap.clone()),
        SkillDiagnostic::Cycle(vec![
            SkillNode::Skill(skill),
            SkillNode::Capability(cap.clone()),
        ]),
        SkillDiagnostic::LimitExceeded,
    ];
    let app_cases = [
        ApplicabilityReason::PredecessorPending(step.clone()),
        ApplicabilityReason::InvalidCompletion(step),
        ApplicabilityReason::Prerequisite(0, ConditionStatus::Unknown),
        ApplicabilityReason::Restriction(
            "sensitive free text".into(),
            ConditionStatus::Unsupported,
        ),
        ApplicabilityReason::ProcessUnavailable,
        ApplicabilityReason::ProcessStatus(ProcessInstanceStatus::Blocked),
        ApplicabilityReason::Gate("gate".into(), GateStatus::Blocked),
        ApplicabilityReason::Blocker("blocker".into()),
        ApplicabilityReason::Waiting,
        ApplicabilityReason::ActivityUnavailable,
        ApplicabilityReason::CapabilityUnavailable(cap),
    ];
    let mut graph = Graph {
        nodes: BTreeMap::new(),
        edges: BTreeSet::new(),
        limits: TraceLimits {
            max_nodes: 100,
            max_optional_details: 0,
        },
        optional: vec![],
        omitted: 0,
    };
    let root = graph
        .node(super::SourceKind::Plan, "plan", "MISSING", json!({}))
        .unwrap();
    for d in skill_cases
        .into_iter()
        .map(CompositionDiagnostic::Skill)
        .chain(
            app_cases
                .into_iter()
                .map(CompositionDiagnostic::Applicability),
        )
    {
        let projected = diagnostic(&d);
        assert!(
            !json!([projected.0, projected.2, projected.3])
                .to_string()
                .contains("sensitive free text")
        );
        graph.diagnostic(&root, &d).unwrap();
    }
    for o in [
        ProcessSelectionOutcome::NoTemplate,
        ProcessSelectionOutcome::Unique,
        ProcessSelectionOutcome::Ambiguous,
        ProcessSelectionOutcome::Missing,
        ProcessSelectionOutcome::Incompatible,
        ProcessSelectionOutcome::Unsupported,
    ] {
        assert!(!process_code(o).is_empty());
    }
    for r in [
        ProcessRejectionReason::DefinitionConstraint,
        ProcessRejectionReason::PinnedDefinition,
        ProcessRejectionReason::ActivityContract,
        ProcessRejectionReason::UnsupportedLifecycle,
    ] {
        assert!(!process_rejection(r).is_empty());
    }
    for o in [
        crate::resolution_candidates::CandidateOutcome::Compatible,
        crate::resolution_candidates::CandidateOutcome::UnknownCapability,
        crate::resolution_candidates::CandidateOutcome::MissingProvider,
        crate::resolution_candidates::CandidateOutcome::Incompatible,
    ] {
        assert!(!candidate_code(o).is_empty());
    }
    for d in [
        CompositionDiagnostic::MissingRequirement(CapabilityRequirementId::new("r").unwrap()),
        CompositionDiagnostic::OptionalOmitted(CapabilityRequirementId::new("r").unwrap()),
        CompositionDiagnostic::RoleConflict,
        CompositionDiagnostic::SearchLimit,
    ] {
        graph.diagnostic(&root, &d).unwrap();
    }
}

#[test]
fn explicit_rule_encoding_covers_all_semantics_and_normalizes_conjunctions() {
    let input = support::with_process(composition::fixture());
    let snapshot = ResolutionSnapshot::capture(&input).unwrap();
    let mut r = composition::rules();
    let id = input.plan.steps()[0].id().clone();
    let conditions = vec![
        SkillCondition::Always,
        SkillCondition::Never,
        SkillCondition::Mode(OperatingMode::Hardening),
        SkillCondition::Profile(ExecutionProfile::FullPath),
        SkillCondition::ProcessState(StateId::new("START").unwrap()),
        SkillCondition::DesiredCondition(ConditionId::new("condition").unwrap()),
        SkillCondition::Unsupported(ReferenceId::new("unsupported").unwrap()),
    ];
    r.applicability.restrictions.insert(
        id.clone(),
        BTreeMap::from([("source".into(), conditions.clone())]),
    );
    r.applicability.completed.insert(
        id.clone(),
        CompletionEvidence {
            basis: snapshot.request().basis().clone(),
            contracts: BTreeSet::from([
                input.plan.steps()[0].completion().clone(),
                PlanCondition::desired_condition(ConditionId::new("condition").unwrap()),
            ]),
            references: BTreeSet::from([EvidenceId::new("evidence").unwrap()]),
            status: ConditionStatus::Satisfied,
            freshness: FreshnessStatus::Fresh,
        },
    );
    r.applicability
        .activities
        .insert(id.clone(), ActivityId::new("inspect").unwrap());
    r.agents
        .primary
        .insert(id.clone(), AgentId::new("alpha").unwrap());
    r.agents
        .participants
        .insert(id.clone(), BTreeSet::from([AgentId::new("beta").unwrap()]));
    r.agents.process_roles.insert(
        id.clone(),
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
    let mut sr = composition::skill_rules();
    sr.roots.insert(
        SkillId::new("good").unwrap(),
        RequirementCardinality::Mandatory,
    );
    sr.conditions
        .insert(SkillId::new("good").unwrap(), SkillCondition::Always);
    sr.capability_providers.insert(
        CapabilityId::new("nested").unwrap(),
        composition::agent("alpha"),
    );
    r.skills.insert(id.clone(), sr);
    r.processes
        .activities
        .insert(id.clone(), ActivityId::new("inspect").unwrap());
    r.processes.output_evidence.insert(
        id.clone(),
        BTreeSet::from([EvidenceTypeId::new("proof").unwrap()]),
    );
    r.processes.lifecycle_contracts.insert(
        LifecycleRequirementKind::HumanInput,
        ActivityConstraint::new("lifecycle", "HUMAN_INPUT").unwrap(),
    );
    r.provider_priorities.insert(composition::skill("good"), 3);
    for preference in [
        TemplatePreference::None,
        TemplatePreference::Optional,
        TemplatePreference::Required,
    ] {
        r.processes.preference = preference;
        assert!(rules_json(&r).is_object());
    }
    let selectors = [
        CapabilitySelector::CapabilityId(CapabilityId::new("cap").unwrap()),
        CapabilitySelector::Class(CapabilityClass::Inspect),
        CapabilitySelector::Domain(CapabilityDomain::new("domain").unwrap()),
        CapabilitySelector::InputKind(CapabilityInputKind::new("input").unwrap()),
        CapabilitySelector::OutputKind(CapabilityOutputKind::new("output").unwrap()),
        CapabilitySelector::Precondition(CapabilityPrecondition::new("precondition").unwrap()),
        CapabilitySelector::Constraint(CapabilityConstraint::new("constraint").unwrap()),
        CapabilitySelector::ApplicabilityTag(CapabilityTag::new("tag").unwrap()),
    ];
    for s in &selectors {
        assert!(selector(s).is_array());
    }
    r.candidates.selectors.insert(
        input.plan.capability_requirements()[0].id().clone(),
        selectors.into_iter().collect(),
    );
    let encoded = rules_json(&r);
    r.applicability
        .restrictions
        .get_mut(&id)
        .unwrap()
        .get_mut("source")
        .unwrap()
        .reverse();
    assert_eq!(encoded, rules_json(&r));
    for s in [
        ConditionStatus::Satisfied,
        ConditionStatus::Unsatisfied,
        ConditionStatus::Unknown,
        ConditionStatus::Conflicted,
        ConditionStatus::Unsupported,
    ] {
        assert!(!status(s).is_empty());
    }
    let mut fixture = composition::fixture();
    composition::edit_skill(
        &mut fixture,
        "good",
        "required_capability_ids",
        json!(["nested"]),
    );
    assert!(composition::run(&fixture, &composition::rules()).visits > 0);
}
