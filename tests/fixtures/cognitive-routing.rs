use gateway_domain::cognitive_routing::*;
use std::collections::BTreeSet;
pub fn request() -> CognitiveRouteRequest {
    CognitiveRouteRequest {
        version: "1.0".into(),
        decision_reference: gateway_domain::ReferenceId::new("decision-1").unwrap(),
        task: TaskClass::Resolution,
        novelty: CognitiveLevel::Low,
        reasoning_depth: CognitiveLevel::Low,
        uncertainty: CognitiveLevel::Low,
        evidence_completeness: CognitiveLevel::High,
        input_contract: "task-v1".into(),
        output_contract: serde_json::json!({"type":"proposal"}),
        deterministic_sufficient: true,
        reflex_applicable: true,
        max_privacy: PrivacyBoundary::External,
        hardware: BTreeSet::from([HardwareRequirement::Cpu]),
        cost_unit: "microcredits".into(),
        max_cost: 10,
        max_latency_ms: 100,
        max_attempts: 5,
    }
}

pub fn candidate(id: &str, kind: CognitiveRoute) -> RouteCandidate {
    RouteCandidate {
        id: id.into(),
        contract_version: "1.0".into(),
        kind,
        model: if matches!(kind, CognitiveRoute::Deterministic | CognitiveRoute::Reflex) {
            None
        } else {
            Some(ModelIdentity {
                id: id.into(),
                version: "v1".into(),
                digest: "sha256:model".into(),
                runtime: "fixture".into(),
                runtime_version: "v1".into(),
                quantization: "q4".into(),
            })
        },
        procedure: (kind == CognitiveRoute::Reflex)
            .then(|| gateway_domain::ReferenceId::new("active-procedure-v1").unwrap()),
        qualified: true,
        available: true,
        tasks: BTreeSet::from([TaskClass::Resolution]),
        max_novelty: CognitiveLevel::High,
        max_reasoning_depth: CognitiveLevel::High,
        max_uncertainty: CognitiveLevel::High,
        min_evidence: CognitiveLevel::Low,
        input_contracts: BTreeSet::from(["task-v1".into()]),
        output_contracts: vec![request().output_contract],
        privacy: if kind == CognitiveRoute::StrongLlm {
            PrivacyBoundary::External
        } else {
            PrivacyBoundary::OnDevice
        },
        hardware: HardwareRequirement::Cpu,
        cost: 1,
        latency_ms: 10,
    }
}

pub fn snapshot(candidates: Vec<RouteCandidate>) -> ModelCapabilitySnapshot {
    ModelCapabilitySnapshot {
        version: "1.0".into(),
        configuration_digest: "sha256:configuration".into(),
        cost_unit: "microcredits".into(),
        candidates,
    }
}
