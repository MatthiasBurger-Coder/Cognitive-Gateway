use gateway_application::{
    connectors::ConnectorOperationClass, policy_application::*, resolution::ResolutionOutcome,
    resolution_application::*, sessions::*,
};
use gateway_domain::*;
use gateway_policy::*;
#[allow(dead_code)]
#[path = "../../gateway-application/tests/support/composition.rs"]
mod composition;
#[allow(dead_code)]
#[path = "../../gateway-application/tests/support/mod.rs"]
mod support;
pub(crate) struct NativePolicy {
    pub(crate) resolved: ResolvedPlan,
    pub(crate) authority: PolicyAuthority,
    pub(crate) context: PolicyContext,
    pub(crate) goal: SupportedConnectorGoal,
    pub(crate) capability: CapabilityId,
}
pub(crate) fn native(registered: SupportedConnectorGoal, mode: OperatingMode) -> NativePolicy {
    let scope = registered.scope().clone();
    let mut input = composition::fixture();
    input.operating_mode = mode;
    input.scope = scope.clone();
    input.plan_scope = scope.clone();
    input.situation_scope = scope.clone();
    let desired = registered.intent().desired_state().clone();
    let current = CurrentState::new_v1(ObservedStateId::new("audit-current").unwrap());
    let situation = SituationAssemblyInput::new(current.clone())
        .assemble(SituationId::new("audit-situation").unwrap())
        .unwrap();
    let delta = gateway_application::DeclarativePlanningApplication
        .derive_delta(
            DeltaId::new("audit-delta").unwrap(),
            &desired,
            &current,
            Some(&situation),
            &ComparisonRules::default(),
            &DeltaDerivationRules::default(),
        )
        .unwrap()
        .delta()
        .clone();
    let capability = CapabilityId::new("architecture.dependency-analysis").unwrap();
    let requirements = delta
        .items()
        .iter()
        .enumerate()
        .map(|(i, item)| {
            CapabilityRequirement::new(
                CapabilityRequirementId::new(format!("audit-requirement-{i}")).unwrap(),
                capability.clone(),
                RequirementCardinality::Mandatory,
                item.id().clone(),
                "Observe the actual registered condition",
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    input.plan = plan(&desired, &delta, &requirements, &PlannerRules::default())
        .unwrap()
        .plan()
        .unwrap()
        .clone();
    input.desired = desired;
    input.delta = delta;
    input.situation = DeclarativeContextSituationDocument::new(
        DeclarativeContext::new_v1(DeclarativeContextId::new("audit-context").unwrap()),
        Some(registered.intent().clone()),
        None,
        current,
        situation,
    )
    .unwrap();
    let mut rules = composition::rules();
    rules
        .provider_priorities
        .insert(composition::skill("good"), 10);
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(&input, &rules)
        .unwrap();
    assert_eq!(resolved.report.outcome, ResolutionOutcome::Resolved);
    let authority = PolicyAuthority {
        capabilities: input
            .index
            .entries()
            .map(|e| (e.id().clone(), e.capability().clone()))
            .collect(),
        policies: vec![
            PolicyDefinition::new(
                PolicyId::new("audit-native-policy").unwrap(),
                "Native audit policy",
                input.index.ids().cloned(),
            )
            .unwrap(),
        ],
        ..Default::default()
    };
    let facts = StepFacts {
        authorizations: authority
            .capabilities
            .keys()
            .map(|id| (id.clone(), Approval::Granted))
            .collect(),
        evidence: authority
            .capabilities
            .values()
            .flat_map(|c| c.preconditions().iter().map(ToString::to_string))
            .collect(),
        satisfied_constraints: authority
            .capabilities
            .values()
            .flat_map(|c| c.constraints().iter().map(ToString::to_string))
            .collect(),
        prerequisites_satisfied: true,
        ..Default::default()
    };
    let context = PolicyContext {
        basis: resolved.report.basis.clone(),
        operating_mode: input.operating_mode,
        execution_profile: input.execution_profile,
        steps: input
            .plan
            .steps()
            .iter()
            .map(|s| (s.id().clone(), facts.clone()))
            .collect(),
    };
    let goal = SupportedConnectorGoal::from_host_registration(
        registered.intent().clone(),
        scope,
        content_reference(
            "audit-native-plan",
            input.plan.to_json().unwrap().as_bytes(),
        )
        .unwrap(),
        input.plan.steps().iter().map(|s| s.id().clone()).collect(),
        registered
            .observations()
            .iter()
            .map(|m| {
                ConditionObservationMapping::new(
                    m.condition().clone(),
                    m.source().clone(),
                    capability.clone(),
                    ConnectorOperationClass::Inspect,
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap();
    for step in goal.steps() {
        let assembled = PolicyApplication
            .step_input(&resolved, &context, step)
            .unwrap();
        assert!(assembled.capabilities.contains_key(&capability));
        assert_eq!(
            PolicyEngine::evaluate(&authority, &assembled.as_input()).decision,
            PolicyDecision::Allow
        );
    }
    NativePolicy {
        resolved,
        authority,
        context,
        goal,
        capability,
    }
}
