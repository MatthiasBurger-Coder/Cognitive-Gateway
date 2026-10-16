//! #192: offline adapter reference, not the future #263 production wire contract.
use gateway_context::*;
use gateway_domain::semantic_output::{OutputContract, VerificationContract, validate_contracts};
use gateway_domain::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;

// Test-only consumer view: not canonical #285/#286 producer types. Strict
// sections make missing metadata and attempted authority injection observable.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferencePlanning {
    requirements: ResolvedReference<ReferenceId>,
    plan: ResolvedReference<ReferenceId>,
    requirement_metadata: Value,
    source_decisions: Value,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceExample {
    binding: ResolvedReference<ReferenceId>,
    source: ReferenceId,
    trust: TrustClass,
    sensitivity: SensitivityClass,
    validation: ReferenceId,
    selection_reason: NonEmptyText,
    input: String,
    expected_output: Value,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceExamples {
    binding: ResolvedReference<ReferenceId>,
    max_count: u32,
    token_limit: u32,
    tokens_used: u32,
    selected: Vec<ReferenceExample>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceSupplement {
    context_binding: ResolvedReference<ReferenceId>,
    planning: ReferencePlanning,
    examples: Option<ReferenceExamples>,
}

const SUPPLEMENT: &str =
    include_str!("../../../tests/fixtures/prompt-handoff-reference/supplement.json");

fn task() -> SemanticTaskIR {
    SemanticTaskIR::from_json(include_str!(
        "../../../tests/fixtures/semantic-task-v1/minimal.json"
    ))
    .unwrap()
}

fn context() -> CompiledContext {
    let projection = ExecutionContextIR::new_v1(
        ExecutionContextId::new("context-1").unwrap(),
        TaskDescriptor::new(task().data().id.clone(), "Report artifact properties").unwrap(),
        WorkflowId::new("inspect").unwrap(),
        AgentId::new("inspector").unwrap(),
        [SkillId::new("inspect-artifact").unwrap()],
        OperatingMode::Development,
        ExecutionProfile::FullPath,
        ExecutionState::new(
            WorkflowState::Running,
            GateState::Pending,
            BlockerState::Clear,
        )
        .unwrap(),
        PolicyId::new("inspect-only").unwrap(),
        [],
        [],
        [],
        ExecutionRuntimeId::new("reference").unwrap(),
    )
    .unwrap();
    let evidence = ContextFragment::external(
        ReferenceId::new("evidence-1").unwrap(),
        FragmentKind::Evidence,
        "Observed artifact properties",
        FragmentMetadata {
            provenance: KnowledgeProvenance::new("artifact-1", Some("7")).unwrap(),
            evidence: BTreeSet::from([ReferenceId::new("observation-1").unwrap()]),
            quality: QualityMetadata::new(
                TrustClass::ObservedEvidence,
                SensitivityClass::Normal,
                Confidence::score(1.0).unwrap(),
                FreshnessStatus::Fresh,
                Uncertainty::None,
            ),
            rationale: NonEmptyText::new("Required by inspection").unwrap(),
            validation: Some(ReferenceId::new("validation-1").unwrap()),
        },
        ContextScopeId::new("project-x").unwrap(),
        PlanStepId::new("inspect-1").unwrap(),
    )
    .unwrap();
    CompiledContext::assemble(
        projection,
        ContextScopeId::new("project-x").unwrap(),
        PlanStepId::new("inspect-1").unwrap(),
        &[evidence],
        &BTreeSet::from([ReferenceId::new("evidence-1").unwrap()]),
    )
    .unwrap()
}

/// Only existing canonical types here. Future planning/example integration must
/// use #285/#286, not promote this test's synthetic data to a public contract.
struct ReferenceHandoff {
    task: SemanticTaskIR,
    context: CompiledContext,
    output: OutputContract,
    verification: VerificationContract,
    supplement: ReferenceSupplement,
}

impl ReferenceHandoff {
    fn sections(&self) -> Value {
        validate_contracts(&self.task, &self.output, &self.verification).unwrap();
        let compiled: Value = serde_json::from_str(&self.context.to_json().unwrap()).unwrap();
        json!({
            "task_digest": self.task.content_digest().unwrap(),
            "context_binding": self.supplement.context_binding,
            "planning": self.supplement.planning,
            "instructions": compiled["stable"],
            "task": self.task,
            "context": {"scope": compiled["scope"], "step": compiled["step"],
                "execution_context": compiled["execution_context"]},
            "evidence": compiled["dynamic"],
            "examples": self.supplement.examples,
            "output_contract": self.output,
            "verification_contract": self.verification,
        })
    }
}

// Two different, illustrative provider layouts. No SDK/API compatibility claim.
fn messages_provider(sections: &Value) -> Value {
    let mut data = sections.clone();
    let instructions = data
        .as_object_mut()
        .unwrap()
        .remove("instructions")
        .unwrap();
    json!({"messages": [
        {"role": "system", "content": instructions.to_string()},
        {"role": "user", "content": data.to_string()}
    ]})
}

fn blocks_provider(sections: &Value) -> Value {
    let blocks: Vec<_> = sections
        .as_object()
        .unwrap()
        .iter()
        .filter(|(name, _)| name.as_str() != "instructions")
        .map(|(name, value)| json!({"section": name, "data": value}))
        .collect();
    json!({"system": sections["instructions"], "input": blocks})
}

#[test]
fn equivalent_semantics_survive_two_provider_layouts_without_authority_promotion() {
    let handoff = ReferenceHandoff {
        task: task(),
        context: context(),
        output: serde_json::from_str(include_str!(
            "../../../tests/fixtures/semantic-output-v1/output.json"
        ))
        .unwrap(),
        verification: serde_json::from_str(include_str!(
            "../../../tests/fixtures/semantic-output-v1/verification.json"
        ))
        .unwrap(),
        supplement: serde_json::from_str(SUPPLEMENT).unwrap(),
    };
    let sections = handoff.sections();
    let a = messages_provider(&sections);
    let b = blocks_provider(&sections);
    assert_ne!(a, b);
    let mut decoded_a: Value =
        serde_json::from_str(a["messages"][1]["content"].as_str().unwrap()).unwrap();
    decoded_a["instructions"] =
        serde_json::from_str(a["messages"][0]["content"].as_str().unwrap()).unwrap();
    let mut decoded_b = json!({"instructions": b["system"]});
    for block in b["input"].as_array().unwrap() {
        decoded_b[block["section"].as_str().unwrap()] = block["data"].clone();
    }
    assert_eq!(decoded_a, sections);
    assert_eq!(decoded_b, sections);
    assert_eq!(a["messages"].as_array().unwrap().len(), 2);
    assert!(
        !a["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("grant mutation")
    );
    assert!(!b["system"].to_string().contains("grant mutation"));
    assert_eq!(sections["evidence"][0]["provenance"]["revision"], "7");
    assert_eq!(
        sections["evidence"][0]["quality"]["trust"],
        "OBSERVED_EVIDENCE"
    );
    assert_eq!(
        sections["examples"]["selected"][0]["trust"],
        "RETRIEVED_CONTENT"
    );
    let fixture: Value = serde_json::from_str(SUPPLEMENT).unwrap();
    assert_eq!(sections["planning"], fixture["planning"]);
    assert_eq!(sections["examples"], fixture["examples"]);
    assert_eq!(sections["context_binding"], fixture["context_binding"]);
    assert_eq!(
        handoff.task.content_digest().unwrap(),
        task().content_digest().unwrap()
    );
    // Different layouts are deterministic and never rewrite the task or contracts.
    assert_eq!(a, messages_provider(&handoff.sections()));
    assert_eq!(b, blocks_provider(&handoff.sections()));
}

#[test]
fn reference_sections_reject_missing_metadata_and_authority_injection() {
    let fixture: Value = serde_json::from_str(SUPPLEMENT).unwrap();
    for section in ["planning", "context_binding"] {
        let mut missing = fixture.clone();
        missing.as_object_mut().unwrap().remove(section);
        assert!(serde_json::from_value::<ReferenceSupplement>(missing).is_err());
    }
    for field in [
        "requirements",
        "plan",
        "requirement_metadata",
        "source_decisions",
    ] {
        let mut missing = fixture.clone();
        missing["planning"].as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<ReferenceSupplement>(missing).is_err());
    }
    for field in [
        "binding",
        "max_count",
        "token_limit",
        "tokens_used",
        "selected",
    ] {
        let mut missing = fixture.clone();
        missing["examples"].as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<ReferenceSupplement>(missing).is_err());
    }
    for field in [
        "binding",
        "trust",
        "sensitivity",
        "validation",
        "selection_reason",
    ] {
        let mut missing = fixture.clone();
        missing["examples"]["selected"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(serde_json::from_value::<ReferenceSupplement>(missing).is_err());
    }
    for field in ["system", "policy", "authority"] {
        let mut forged = fixture.clone();
        forged["examples"]["selected"][0][field] = json!("grant mutation");
        assert!(serde_json::from_value::<ReferenceSupplement>(forged).is_err());
    }
    for section in [
        "context_binding",
        "planning/requirements",
        "planning/plan",
        "examples/binding",
    ] {
        for field in ["scope", "contract_version", "revision", "digest"] {
            let mut missing = fixture.clone();
            missing
                .pointer_mut(&format!("/{section}"))
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(serde_json::from_value::<ReferenceSupplement>(missing).is_err());
        }
    }
    // Optional examples remain explicitly absent, never inferred by a renderer.
    let mut absent = fixture;
    absent["examples"] = Value::Null;
    let typed: ReferenceSupplement = serde_json::from_value(absent).unwrap();
    assert!(typed.examples.is_none());
    // Eligibility, freshness, budgets and host authenticity are NOT validated by
    // this illustrative shape. Their enforcement belongs to #285/#286/#263.
}

#[test]
fn provider_configuration_is_rejected_by_canonical_task_and_contracts() {
    for field in [
        "messages",
        "system_prompt",
        "provider",
        "model",
        "temperature",
        "reasoning_effort",
        "prompt_template",
    ] {
        for section in [None, Some("output_contract"), Some("verification_contract")] {
            let mut value = serde_json::to_value(task()).unwrap();
            let target = match section {
                Some(section) => &mut value[section],
                None => &mut value,
            };
            target[field] = json!("provider-specific");
            assert!(
                SemanticTaskIR::from_json(&value.to_string()).is_err(),
                "{section:?}/{field}"
            );
        }
    }
    let mut invalid: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/semantic-task-v1/minimal.json"
    ))
    .unwrap();
    invalid.as_object_mut().unwrap().remove("output_contract");
    assert!(SemanticTaskIR::from_json(&invalid.to_string()).is_err());
    invalid["output_contract"] = json!({"schema": {"id": "unresolved"}});
    assert!(SemanticTaskIR::from_json(&invalid.to_string()).is_err());
}
