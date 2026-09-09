use gateway_application::resolution::*;
use gateway_domain::*;
use gateway_registry::CapabilityProvider;
use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

fn request(cardinalities: &[RequirementCardinality]) -> ResolutionRequest {
    let requirements: Vec<_> = cardinalities
        .iter()
        .enumerate()
        .map(|(i, c)| {
            CapabilityRequirement::new(
                CapabilityRequirementId::new(format!("r{i}")).unwrap(),
                CapabilityId::new(format!("c{i}")).unwrap(),
                *c,
                DeltaItemId::new("delta-item").unwrap(),
                "explicit contract",
            )
            .unwrap()
        })
        .collect();
    let outcome = RequiredOutcome::new(RequiredOutcomeKind::Assessment, "assess").unwrap();
    let step = PlanStep::new(
        PlanStepId::new("step").unwrap(),
        PlanStepKind::Verification,
        outcome.clone(),
        PlanCondition::outcome(outcome),
        "assess",
    )
    .unwrap()
    .with_capability_requirements(requirements.iter().map(|r| r.id().clone()).collect())
    .unwrap();
    let plan = Plan::new(
        PlanId::new("plan").unwrap(),
        DesiredStateId::new("desired").unwrap(),
        DeltaId::new("delta").unwrap(),
        requirements,
        vec![step],
    )
    .unwrap();
    make_request(plan)
}

fn make_request(plan: Plan) -> ResolutionRequest {
    let fingerprint = ContentFingerprint::of_bytes(b"synthetic snapshot");
    let basis = ResolutionBasis {
        plan: plan.id().clone(),
        plan_fingerprint: ContentFingerprint::of_bytes(plan.to_json().unwrap().as_bytes()),
        admission: Some(ReferenceId::new("admission").unwrap()),
        situation: SituationId::new("situation").unwrap(),
        scope: ContextScopeId::new("scope").unwrap(),
        situation_fingerprint: fingerprint.clone(),
        registry_fingerprint: fingerprint.clone(),
        process_catalog_fingerprint: fingerprint.clone(),
        process_state_fingerprint: fingerprint,
        rule_version: SchemaVersion::V1,
    };
    ResolutionRequest::new(plan, basis, vec![]).unwrap()
}

fn step(request: &ResolutionRequest) -> StepResolution {
    let agent = AgentId::new("agent").unwrap();
    let provider = CapabilityProvider::Agent {
        agent_id: agent.clone(),
    };
    StepResolution {
        step: request.plan().steps()[0].id().clone(),
        outcome: ResolutionOutcome::Resolved,
        readiness: LifecycleReadiness::Unknown,
        binding: Some(StepBinding {
            process: None,
            primary_agent: agent,
            participating_agents: BTreeSet::new(),
            skills: BTreeMap::new(),
        }),
        requirements: request
            .plan()
            .capability_requirements()
            .iter()
            .map(|r| RequirementResolution {
                requirement: r.id().clone(),
                candidates: vec![ProviderCandidate {
                    provider: provider.clone(),
                    definition_fingerprint: ContentFingerprint::of_bytes(b"agent"),
                    reason: ResolutionReason::ContractMatch,
                }],
                selected: Some(provider.clone()),
                reason: ResolutionReason::ContractMatch,
            })
            .collect(),
        reasons: BTreeSet::from([ResolutionReason::NoTemplateRequired]),
    }
}

fn result(
    request: &ResolutionRequest,
    step: StepResolution,
) -> Result<ResolutionResult, ResolutionError> {
    ResolutionResult::new(SchemaVersion::V1, request, step.outcome, vec![step])
}

#[test]
fn wire_names_and_identifiers_fail_closed() {
    for value in [
        "RESOLVED",
        "NO_OP",
        "MISSING",
        "AMBIGUOUS",
        "CONFLICTING",
        "UNSUPPORTED",
        "INVALID_INPUT",
        "PARTIAL",
        "SEARCH_LIMIT",
    ] {
        assert_eq!(ResolutionOutcome::from_str(value).unwrap().as_str(), value);
    }
    for value in [
        "ELIGIBLE",
        "BLOCKED",
        "DEFERRED",
        "UNKNOWN",
        "NOT_APPLICABLE",
    ] {
        assert_eq!(LifecycleReadiness::from_str(value).unwrap().as_str(), value);
    }
    for value in [
        "CONTRACT_MATCH",
        "UNKNOWN_CAPABILITY",
        "INCOMPATIBLE_CONTRACT",
        "MISSING_PROVIDER",
        "MISSING_DEPENDENCY",
        "DEPENDENCY_CYCLE",
        "CONDITION_UNKNOWN",
        "CONSTRAINT_CONFLICT",
        "EQUAL_ALTERNATIVES",
        "OPTIONAL_OMITTED",
        "NO_TEMPLATE_REQUIRED",
        "PINNED_PROCESS",
        "LIFECYCLE_BLOCKED",
        "PREDECESSOR_PENDING",
        "UNSUPPORTED_CONTRACT",
        "INVALID_BASIS",
        "SEARCH_BUDGET_EXHAUSTED",
    ] {
        assert_eq!(ResolutionReason::from_str(value).unwrap().as_str(), value);
    }
    assert!(ResolutionOutcome::from_str("ALLOW").is_err());
    assert!(LifecycleReadiness::from_str("ALLOW").is_err());
    assert!(ResolutionReason::from_str("ALLOW").is_err());
    assert!(PlanStepId::new("../step").is_err());
    assert!(ContentFingerprint::parse("bad").is_err());
    assert!(ContentFingerprint::parse(&"z".repeat(64)).is_err());
    assert_eq!(
        ContentFingerprint::parse(&"A".repeat(64)).unwrap().as_str(),
        "a".repeat(64)
    );
    assert_eq!(
        ContentFingerprint::of_bytes(b"abc").as_str(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn request_pins_plan_and_rules_and_checks_alternatives() {
    let req = request(&[
        RequirementCardinality::Mandatory,
        RequirementCardinality::Optional,
    ]);
    let mut basis = req.basis().clone();
    basis.rule_version = SchemaVersion::V2;
    assert_eq!(
        ResolutionRequest::new(req.plan().clone(), basis, vec![]),
        Err(ResolutionError::UnsupportedVersion)
    );
    let mut basis = req.basis().clone();
    basis.plan = PlanId::new("other").unwrap();
    assert_eq!(
        ResolutionRequest::new(req.plan().clone(), basis, vec![]),
        Err(ResolutionError::InvalidPlan)
    );
    let mut basis = req.basis().clone();
    basis.plan_fingerprint = ContentFingerprint::of_bytes(b"tampered");
    assert_eq!(
        ResolutionRequest::new(req.plan().clone(), basis, vec![]),
        Err(ResolutionError::InvalidPlan)
    );
    let mut group = RequirementAlternatives {
        step: PlanStepId::new("missing").unwrap(),
        members: BTreeSet::new(),
        cardinality: RequirementCardinality::Mandatory,
    };
    let build = |groups| ResolutionRequest::new(req.plan().clone(), req.basis().clone(), groups);
    assert_eq!(
        build(vec![group.clone()]),
        Err(ResolutionError::InvalidReference)
    );
    group.step = req.plan().steps()[0].id().clone();
    assert_eq!(
        build(vec![group.clone()]),
        Err(ResolutionError::InvalidAlternatives)
    );
    group.members = BTreeSet::from([
        CapabilityRequirementId::new("r0").unwrap(),
        CapabilityRequirementId::new("missing").unwrap(),
    ]);
    assert_eq!(
        build(vec![group.clone()]),
        Err(ResolutionError::InvalidReference)
    );
    group.members = req
        .plan()
        .capability_requirements()
        .iter()
        .map(|r| r.id().clone())
        .collect();
    assert_eq!(
        build(vec![group.clone(), group.clone()]),
        Err(ResolutionError::DuplicateReference)
    );
    assert_eq!(build(vec![group.clone()]).unwrap().alternatives(), &[group]);
}

#[test]
fn complete_binding_is_distinct_from_lifecycle_and_policy() {
    let req = request(&[RequirementCardinality::Mandatory]);
    for readiness in [
        LifecycleReadiness::Eligible,
        LifecycleReadiness::Blocked,
        LifecycleReadiness::Deferred,
        LifecycleReadiness::Unknown,
        LifecycleReadiness::NotApplicable,
    ] {
        let mut s = step(&req);
        s.readiness = readiness;
        let result = result(&req, s).unwrap();
        assert_eq!(result.version(), SchemaVersion::V1);
        assert_eq!(result.basis(), req.basis());
        assert_eq!(result.outcome(), ResolutionOutcome::Resolved);
        assert_eq!(result.steps()[0].readiness, readiness);
        assert!(
            result.steps()[0]
                .binding
                .as_ref()
                .unwrap()
                .process
                .is_none()
        );
    }
}

#[test]
fn incomplete_outcomes_keep_diagnostics_without_binding() {
    let req = request(&[RequirementCardinality::Mandatory]);
    for outcome in [
        ResolutionOutcome::Missing,
        ResolutionOutcome::Ambiguous,
        ResolutionOutcome::Conflicting,
        ResolutionOutcome::Unsupported,
        ResolutionOutcome::InvalidInput,
        ResolutionOutcome::SearchLimit,
        ResolutionOutcome::Partial,
    ] {
        let mut s = step(&req);
        s.outcome = outcome;
        s.binding = None;
        s.requirements[0].selected = None;
        assert_eq!(result(&req, s.clone()).unwrap().outcome(), outcome);
        assert!(
            ResolutionResult::new(
                SchemaVersion::V1,
                &req,
                ResolutionOutcome::Resolved,
                vec![s]
            )
            .is_err()
        );
    }
}

#[test]
fn noop_needs_no_dummy_bindings() {
    let req = make_request(
        Plan::new(
            PlanId::new("p").unwrap(),
            DesiredStateId::new("d").unwrap(),
            DeltaId::new("delta").unwrap(),
            vec![],
            vec![],
        )
        .unwrap(),
    );
    assert!(
        ResolutionResult::new(SchemaVersion::V1, &req, ResolutionOutcome::NoOp, vec![]).is_ok()
    );
    assert!(
        ResolutionResult::new(SchemaVersion::V1, &req, ResolutionOutcome::Resolved, vec![])
            .is_err()
    );
    let outcome = RequiredOutcome::new(RequiredOutcomeKind::NoOp, "already satisfied").unwrap();
    let s = PlanStep::new(
        PlanStepId::new("noop").unwrap(),
        PlanStepKind::NoOp,
        outcome.clone(),
        PlanCondition::outcome(outcome),
        "satisfied",
    )
    .unwrap();
    let req = make_request(
        Plan::new(
            PlanId::new("p").unwrap(),
            DesiredStateId::new("d").unwrap(),
            DeltaId::new("delta").unwrap(),
            vec![],
            vec![s],
        )
        .unwrap(),
    );
    let mut s = step(&req);
    s.outcome = ResolutionOutcome::NoOp;
    s.binding = None;
    s.readiness = LifecycleReadiness::NotApplicable;
    assert!(result(&req, s.clone()).is_ok());
    s.readiness = LifecycleReadiness::Eligible;
    assert!(result(&req, s).is_err());
}

#[test]
fn result_rejects_forged_and_dangling_references() {
    let req = request(&[RequirementCardinality::Mandatory]);
    let good = step(&req);
    assert_eq!(
        ResolutionResult::new(
            SchemaVersion::V2,
            &req,
            ResolutionOutcome::Resolved,
            vec![good.clone()]
        ),
        Err(ResolutionError::UnsupportedVersion)
    );
    assert!(
        ResolutionResult::new(SchemaVersion::V1, &req, ResolutionOutcome::Resolved, vec![])
            .is_err()
    );
    assert!(
        ResolutionResult::new(
            SchemaVersion::V1,
            &req,
            ResolutionOutcome::Resolved,
            vec![good.clone(), good.clone()]
        )
        .is_err()
    );
    let mut s = good.clone();
    s.step = PlanStepId::new("other").unwrap();
    assert!(result(&req, s).is_err());
    let mut s = good.clone();
    s.requirements[0].requirement = CapabilityRequirementId::new("other").unwrap();
    assert!(result(&req, s).is_err());
    let mut s = good.clone();
    s.requirements.push(s.requirements[0].clone());
    assert!(result(&req, s).is_err());
    let mut s = good.clone();
    let candidate = s.requirements[0].candidates[0].clone();
    s.requirements[0].candidates.push(candidate);
    assert!(result(&req, s).is_err());
    let mut s = good.clone();
    s.requirements[0].candidates.clear();
    assert!(result(&req, s).is_err());
    let mut s = good.clone();
    s.requirements.clear();
    assert!(result(&req, s).is_err());
    let mut s = good.clone();
    s.binding = None;
    assert!(result(&req, s).is_err());
    let mut s = good.clone();
    s.outcome = ResolutionOutcome::NoOp;
    assert!(result(&req, s).is_err());
    let mut s = good.clone();
    s.binding
        .as_mut()
        .unwrap()
        .participating_agents
        .insert(AgentId::new("agent").unwrap());
    assert!(result(&req, s).is_err());
    let mut s = good.clone();
    s.binding.as_mut().unwrap().skills.insert(
        SkillId::new("skill").unwrap(),
        AgentId::new("other").unwrap(),
    );
    assert!(result(&req, s).is_err());
    let mut s = good.clone();
    s.binding.as_mut().unwrap().primary_agent = AgentId::new("other").unwrap();
    assert!(result(&req, s).is_err());
    let mut s = good;
    s.requirements[0].selected = None;
    assert!(result(&req, s).is_err());
}

#[test]
fn skill_providers_need_an_explicit_responsible_agent() {
    let req = request(&[RequirementCardinality::Mandatory]);
    let mut s = step(&req);
    let skill = SkillId::new("skill").unwrap();
    let provider = CapabilityProvider::Skill {
        skill_id: skill.clone(),
    };
    s.requirements[0].candidates[0].provider = provider.clone();
    s.requirements[0].selected = Some(provider);
    assert!(result(&req, s.clone()).is_err());
    s.binding
        .as_mut()
        .unwrap()
        .skills
        .insert(skill.clone(), AgentId::new("agent").unwrap());
    assert!(result(&req, s.clone()).is_ok());
    s.binding
        .as_mut()
        .unwrap()
        .skills
        .insert(skill, AgentId::new("participant").unwrap());
    s.binding
        .as_mut()
        .unwrap()
        .participating_agents
        .insert(AgentId::new("participant").unwrap());
    assert!(result(&req, s).is_ok());
}

#[test]
fn optional_is_not_an_inferred_alternative() {
    let req = request(&[
        RequirementCardinality::Mandatory,
        RequirementCardinality::Optional,
    ]);
    let mut s = step(&req);
    s.requirements[0].selected = None;
    s.requirements[0].reason = ResolutionReason::OptionalOmitted;
    assert!(result(&req, s).is_err());
    let mut s = step(&req);
    s.requirements[1].selected = None;
    assert!(result(&req, s.clone()).is_err());
    s.requirements[1].reason = ResolutionReason::OptionalOmitted;
    assert!(result(&req, s).is_ok());
    for cardinality in [
        RequirementCardinality::Mandatory,
        RequirementCardinality::Optional,
    ] {
        let group = RequirementAlternatives {
            step: req.plan().steps()[0].id().clone(),
            members: req
                .plan()
                .capability_requirements()
                .iter()
                .map(|r| r.id().clone())
                .collect(),
            cardinality,
        };
        let grouped =
            ResolutionRequest::new(req.plan().clone(), req.basis().clone(), vec![group]).unwrap();
        let mut s = step(&grouped);
        assert!(result(&grouped, s.clone()).is_err());
        s.requirements[0].selected = None;
        s.requirements[0].reason = ResolutionReason::OptionalOmitted;
        assert!(result(&grouped, s.clone()).is_ok());
        s.requirements[1].selected = None;
        s.requirements[1].reason = ResolutionReason::OptionalOmitted;
        assert_eq!(
            result(&grouped, s).is_ok(),
            cardinality == RequirementCardinality::Optional
        );
    }
}
