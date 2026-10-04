use super::{composition, support};
use gateway_application::{context_application::*, policy_application::*};
use gateway_application::{
    resolution_application::*, resolution_composition::*, resolution_snapshot::*,
};
use gateway_domain::*;
use gateway_policy::*;
use gateway_process::{ProcessRegistry, ProcessSource};
use std::collections::BTreeSet;

pub(crate) fn input() -> ResolutionSnapshotInput {
    let mut input = composition::fixture();
    input.processes=ProcessRegistry::from_sources([ProcessSource::new("synthetic.feature","@process(synthetic)\n@process-version(1)\n@cg-language(1)\nFeature: Synthetic CG02 compatibility\nRule: Process\nGiven state START is initial\nGiven state END is terminal\nGiven event finish\nGiven activity inspect requires capability architecture.dependency-analysis\nGiven activity inspect constrained by primary-agent=alpha\nScenario: finish\nGiven process state START\nWhen event finish occurs\nThen transition to state END\nThen authorize activity inspect\nThen complete process\n")]).unwrap();
    support::with_process(input)
}
pub(crate) fn rules() -> CompositionRules {
    let mut r = composition::rules();
    r.provider_priorities.insert(composition::skill("good"), 10);
    r
}
pub(crate) fn catalog(input: &ResolutionSnapshotInput) -> DefinitionCatalog {
    DefinitionCatalog::new(
        input
            .registry
            .agents()
            .iter()
            .map(|a| a.to_domain())
            .collect(),
        input
            .registry
            .skills()
            .iter()
            .map(|s| s.to_domain())
            .collect(),
        vec![
            WorkflowDefinition::new(
                WorkflowId::new("synthetic-workflow").unwrap(),
                "explicit fixture mapping",
                AgentId::new("alpha").unwrap(),
                [SkillId::new("good").unwrap()],
                PolicyId::new("fixture-policy").unwrap(),
            )
            .unwrap(),
        ],
        vec![
            PolicyDefinition::new(
                PolicyId::new("fixture-policy").unwrap(),
                "explicit test policy",
                [
                    CapabilityId::new("architecture.dependency-analysis").unwrap(),
                    CapabilityId::new("nested").unwrap(),
                ],
            )
            .unwrap(),
        ],
    )
    .unwrap()
}
pub(crate) fn mapping(resolved: &ResolvedPlan) -> WorkflowProjectionMapping {
    WorkflowProjectionMapping {
        basis: resolved.report.basis.clone(),
        step: resolved.report.steps[0].step.clone(),
        task: TaskId::new("fixture-task").unwrap(),
        process: resolved.report.alternatives[0][0]
            .binding
            .as_ref()
            .unwrap()
            .process
            .as_ref()
            .unwrap()
            .definition
            .clone(),
        workflow: WorkflowId::new("synthetic-workflow").unwrap(),
        decision_reference: ReferenceId::new("synthetic-cg02-cg10-mapping-decision").unwrap(),
    }
}
pub(crate) struct Fixture {
    pub(crate) resolved: ResolvedPlan,
    pub(crate) authority: PolicyAuthority,
    pub(crate) policy: PolicyContext,
    pub(crate) catalog: DefinitionCatalog,
    pub(crate) projection: ContextProjection,
}
impl Fixture {
    pub(crate) fn new() -> Self {
        Self::from_input(input())
    }
    pub(crate) fn from_input(input: ResolutionSnapshotInput) -> Self {
        let catalog = catalog(&input);
        let resolved = DeclarativeResolutionApplication
            .resolve_plan(&input, &rules())
            .unwrap();
        let authority = PolicyAuthority {
            policies: vec![
                catalog
                    .policy(&PolicyId::new("fixture-policy").unwrap())
                    .unwrap()
                    .clone(),
            ],
            capabilities: input
                .index
                .entries()
                .map(|e| (e.id().clone(), e.capability().clone()))
                .collect(),
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
                .chain(["[\"primary-agent\",\"alpha\"]".into()])
                .collect(),
            prerequisites_satisfied: true,
            ..Default::default()
        };
        let policy = PolicyContext {
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
        let projection = ContextProjection {
            mapping: mapping(&resolved),
            id: ExecutionContextId::new("context").unwrap(),
            task: TaskDescriptor::new(TaskId::new("fixture-task").unwrap(), "inspect architecture")
                .unwrap(),
            state: ExecutionState::new(
                WorkflowState::Running,
                GateState::Pending,
                BlockerState::Clear,
            )
            .unwrap(),
            state_basis: resolved.report.basis.clone(),
            state_decision: ReferenceId::new("state-mapping").unwrap(),
            target_runtime: ExecutionRuntimeId::new("runtime").unwrap(),
            knowledge_queries: vec![
                KnowledgeQuery::new("z").unwrap(),
                KnowledgeQuery::new("a").unwrap(),
                KnowledgeQuery::new("z").unwrap(),
            ],
        };
        Self {
            resolved,
            authority,
            policy,
            catalog,
            projection,
        }
    }
    pub(crate) fn compile(&self) -> Result<CompiledStep, ContextApplicationError> {
        ContextApplication.compile_step(CompileStepInput {
            resolved: &self.resolved,
            authority: &self.authority,
            policy_context: &self.policy,
            catalog: &self.catalog,
            projection: &self.projection,
            candidates: &[],
            selected: &BTreeSet::new(),
        })
    }
}
