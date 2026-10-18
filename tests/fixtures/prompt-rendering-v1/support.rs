#[allow(dead_code)]
#[path = "../example-selection-v1/support.rs"]
pub mod examples;
#[allow(dead_code)]
#[path = "../model-invocation-v1/support.rs"]
pub mod model;
use examples::*;
use gateway_context::*;
use gateway_domain::*;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
fn projection() -> ExecutionContextIR {
    ExecutionContextIR::new_v1(
        ExecutionContextId::new("context").unwrap(),
        TaskDescriptor::new(task().data().id.clone(), "Inspect artifact").unwrap(),
        WorkflowId::new("inspect").unwrap(),
        AgentId::new("inspector").unwrap(),
        [SkillId::new("inspect").unwrap()],
        OperatingMode::Development,
        ExecutionProfile::FullPath,
        ExecutionState::new(
            WorkflowState::Running,
            GateState::Pending,
            BlockerState::Clear,
        )
        .unwrap(),
        PolicyId::new("read-only").unwrap(),
        [],
        [],
        [],
        ExecutionRuntimeId::new("offline").unwrap(),
    )
    .unwrap()
}
fn compiled(fragments: &[ContextFragment]) -> CompiledContext {
    CompiledContext::assemble(
        projection(),
        scope(),
        PlanStepId::new("step").unwrap(),
        fragments,
        &fragments.iter().map(|f| f.id().clone()).collect(),
    )
    .unwrap()
}
fn context_pin(c: &CompiledContext) -> ResolvedReference<ReferenceId> {
    binding(
        "compiled-context",
        &id("r1"),
        &scope(),
        &serde_json::from_str::<Value>(&c.to_json().unwrap()).unwrap(),
    )
}
fn contracts(t: &SemanticTaskIR) -> ContextHandoffContracts {
    use gateway_domain::semantic_output::*;
    ContextHandoffContracts {
        output: OutputContract {
            schema_version: SchemaVersion::V1,
            schema: t.data().output_contract.schema.clone(),
            shape: OutputType::String {},
        },
        verification: VerificationContract {
            schema_version: SchemaVersion::V1,
            rules: t
                .data()
                .verification_contract
                .checks
                .iter()
                .map(|check| VerificationRule {
                    check: *check,
                    rule: pin(id(&format!("rule-{check:?}"))),
                    verifier: pin(id("verifier")),
                })
                .collect(),
        },
        verification_revision: id("r1"),
    }
}

pub fn handoff() -> PlannedContextHandoff {
    handoff_for(task())
}
pub fn handoff_for(task: SemanticTaskIR) -> PlannedContextHandoff {
    let quality = QualityMetadata::new(
        TrustClass::ObservedEvidence,
        SensitivityClass::Normal,
        Confidence::Unknown,
        FreshnessStatus::Fresh,
        Uncertainty::None,
    );
    let mut evidence_requirement = context_requirement();
    evidence_requirement.id = id("evidence");
    evidence_requirement.class = ContextClass::Evidence;
    evidence_requirement.necessity = ContextNecessity::Mandatory;
    evidence_requirement.information.accepted_trust =
        BTreeSet::from([TrustClass::ObservedEvidence]);
    evidence_requirement.budget.context = ContextBudget::new(
        TokenBudget(200),
        BTreeMap::from([(ContextBudgetClass::Evidence, TokenBudget(200))]),
    )
    .unwrap();
    let fragment = ContextFragment::external(
        id("observed-artifact"),
        FragmentKind::Evidence,
        include_str!("hostile.txt"),
        FragmentMetadata {
            provenance: KnowledgeProvenance::new("artifact-source", Some("7")).unwrap(),
            evidence: BTreeSet::from([id("observation")]),
            quality,
            rationale: NonEmptyText::new("Mandatory observation").unwrap(),
            validation: None,
        },
        scope(),
        PlanStepId::new("step").unwrap(),
    )
    .unwrap();
    let mut source_pin = model::pin("artifact-source");
    source_pin.digest = context_content_digest(fragment.content().as_bytes());
    let evidence_source = ContextSourceMetadata {
        pin: source_pin,
        payload_digest: Some(context_content_digest(fragment.content().as_bytes())),
        class: ContextSourceClass::Repository,
        covers: BTreeSet::from([id("evidence")]),
        quality,
        evidence: BTreeSet::from([EvidenceId::new("observation").unwrap()]),
        provenances: BTreeSet::new(),
        tokens: 10,
        available: true,
        present: Some(ContextSatisfaction::Fragment(fragment.id().clone())),
    };
    let req = ContextRequirementsIR::new(
        SchemaVersion::V1,
        id("r1"),
        ContextPlanningBasis {
            task: {
                let mut pin = model::invocation_data().task;
                pin.digest = task.content_digest().unwrap();
                pin
            },
            situation: None,
            plan: None,
        },
        vec![context_requirement(), evidence_requirement],
        TokenBudget(400),
    )
    .unwrap();
    let r = ExampleRequirement::from_context(&req, &task, &id("examples"), policy())
        .unwrap()
        .unwrap();
    let mut hostile = candidate("malicious");
    change_pair(
        &mut hostile,
        include_str!("hostile.txt"),
        "Report properties",
    );
    let set = gateway_application::example_selection::select_examples(
        Some(&r),
        id("s1"),
        &[hostile],
        &BTreeSet::new(),
        None,
    )
    .unwrap()
    .unwrap();
    assert!(!set.selected().is_empty());
    let plan = gateway_application::context_planning::select_context_sources(
        &req,
        id("p1"),
        &[set.source().unwrap(), evidence_source],
    )
    .unwrap();
    let c = compiled(&[fragment]);
    ContextCompiler::handoff_planned_context_with_examples(
        task.clone(),
        req,
        plan,
        c.clone(),
        context_pin(&c),
        &[],
        ContextHandoffWithExamples {
            contracts: contracts(&task),
            expected_examples: set.binding(),
            examples: set,
        },
    )
    .unwrap()
}
pub fn prepared(
    h: &PlannedContextHandoff,
    limit: u32,
    tool: bool,
) -> gateway_application::model_invocation::PreparedModelInvocation {
    use gateway_domain::model_invocation::*;
    let mut data = model::invocation_data();
    data.task = serde_json::from_value(h.to_value()["bindings"]["task"].clone()).unwrap();
    data.compiled_context =
        serde_json::from_value(h.to_value()["bindings"]["context"].clone()).unwrap();
    if tool {
        let mut pin = model::pin("tool");
        pin.digest = context_content_digest(b"tool-schema");
        let pin = ResolvedReference {
            id: CapabilityId::new("tool").unwrap(),
            scope: pin.scope,
            contract: pin.contract,
            contract_version: pin.contract_version,
            revision: pin.revision,
            digest: pin.digest,
        };
        data.permitted_capabilities.insert(pin.id.clone(), pin);
    }
    let invocation = ModelInvocation::new(
        data.clone(),
        ModelInvocationBasis {
            task: h.task(),
            output: &model::output(),
            compiled_context: &data.compiled_context,
            permitted_capabilities: &data.permitted_capabilities,
            sensitivity: data.sensitivity,
            permitted_localities: &data.permitted_localities,
            authority: &data.authority,
        },
    )
    .unwrap();
    let mut capability = model::capability_data();
    capability.context_limit = limit;
    gateway_application::model_invocation::prepare_model_invocation(
        invocation,
        ModelCapability::try_from(capability).unwrap(),
        model::provider(),
        &model::runtime(),
    )
    .unwrap()
}
