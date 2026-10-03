//! CG-26 advisory route contracts. None of these types conveys authority.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const COGNITIVE_ROUTING_VERSION: &str = "1.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CognitiveRoute {
    Deterministic,
    Reflex,
    LocalSlm,
    SpecializedLocal,
    StrongLlm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskClass {
    Resolution,
    Classification,
    Extraction,
    Ranking,
    Reasoning,
    Verification,
}

/// Ordered ordinal criteria; no floating point scores or inferred defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CognitiveLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrivacyBoundary {
    OnDevice,
    PrivateNetwork,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HardwareRequirement {
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelIdentity {
    pub id: String,
    pub version: String,
    pub digest: String,
    pub runtime: String,
    pub runtime_version: String,
    pub quantization: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteCandidate {
    pub id: String,
    pub contract_version: String,
    pub kind: CognitiveRoute,
    pub model: Option<ModelIdentity>,
    /// ACTIVE approved procedure reference for reflexes; admission is rechecked at execution.
    pub procedure: Option<crate::ReferenceId>,
    pub qualified: bool,
    pub available: bool,
    pub tasks: BTreeSet<TaskClass>,
    pub max_novelty: CognitiveLevel,
    pub max_reasoning_depth: CognitiveLevel,
    pub max_uncertainty: CognitiveLevel,
    pub min_evidence: CognitiveLevel,
    pub input_contracts: BTreeSet<String>,
    pub output_contracts: Vec<serde_json::Value>,
    pub privacy: PrivacyBoundary,
    pub hardware: HardwareRequirement,
    /// Conservative reservation per attempt in the registry's explicit cost unit.
    pub cost: u64,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilitySnapshot {
    pub version: String,
    pub configuration_digest: String,
    pub cost_unit: String,
    pub candidates: Vec<RouteCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CognitiveRouteRequest {
    pub version: String,
    pub decision_reference: crate::ReferenceId,
    pub task: TaskClass,
    pub novelty: CognitiveLevel,
    pub reasoning_depth: CognitiveLevel,
    pub uncertainty: CognitiveLevel,
    pub evidence_completeness: CognitiveLevel,
    pub input_contract: String,
    pub output_contract: serde_json::Value,
    pub deterministic_sufficient: bool,
    pub reflex_applicable: bool,
    pub max_privacy: PrivacyBoundary,
    pub hardware: BTreeSet<HardwareRequirement>,
    pub cost_unit: String,
    pub max_cost: u64,
    pub max_latency_ms: u64,
    pub max_attempts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RouteRejection {
    Unqualified,
    Unavailable,
    TaskClass,
    Novelty,
    ReasoningDepth,
    Uncertainty,
    EvidenceIncomplete,
    InputContract,
    OutputContract,
    Privacy,
    Hardware,
    Cost,
    Latency,
    DeterministicInsufficient,
    ReflexInapplicable,
    PreviouslyAttempted,
    LowerPrecedence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteEvaluation {
    pub candidate: RouteCandidate,
    pub reasons: BTreeSet<RouteRejection>,
}

/// Inspectable frozen request and capability snapshot for exact replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteExplanation {
    pub version: String,
    pub request: CognitiveRouteRequest,
    pub configuration_digest: String,
    pub selected: Option<RouteCandidate>,
    pub alternatives: Vec<RouteEvaluation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingError {
    InvalidRequest,
    InvalidRegistry,
    RegistryUnavailable,
}

impl ModelCapabilitySnapshot {
    pub fn validate(&self) -> Result<(), RoutingError> {
        let mut ids = BTreeSet::new();
        let text = |s: &str| !s.trim().is_empty();
        if self.version != COGNITIVE_ROUTING_VERSION
            || !text(&self.configuration_digest)
            || !text(&self.cost_unit)
        {
            return Err(RoutingError::InvalidRegistry);
        }
        for c in &self.candidates {
            let model_route = matches!(
                c.kind,
                CognitiveRoute::LocalSlm
                    | CognitiveRoute::SpecializedLocal
                    | CognitiveRoute::StrongLlm
            );
            if !text(&c.id)
                || !ids.insert(&c.id)
                || c.contract_version != COGNITIVE_ROUTING_VERSION
                || c.tasks.is_empty()
                || c.input_contracts.is_empty()
                || c.input_contracts.iter().any(|s| !text(s))
                || c.output_contracts.is_empty()
                || c.output_contracts.iter().any(serde_json::Value::is_null)
                || model_route != c.model.is_some()
                || (c.kind == CognitiveRoute::Reflex) != c.procedure.is_some()
                || (matches!(
                    c.kind,
                    CognitiveRoute::LocalSlm | CognitiveRoute::SpecializedLocal
                ) && c.privacy != PrivacyBoundary::OnDevice)
                || c.model.as_ref().is_some_and(|m| {
                    [
                        &m.id,
                        &m.version,
                        &m.digest,
                        &m.runtime,
                        &m.runtime_version,
                        &m.quantization,
                    ]
                    .iter()
                    .any(|s| !text(s))
                })
            {
                return Err(RoutingError::InvalidRegistry);
            }
        }
        Ok(())
    }
}

impl CognitiveRouteRequest {
    pub fn validate(&self) -> Result<(), RoutingError> {
        if self.version != COGNITIVE_ROUTING_VERSION
            || self.input_contract.trim().is_empty()
            || self.output_contract.is_null()
            || self.cost_unit.trim().is_empty()
            || self.max_attempts == 0
            || self.max_latency_ms == 0
        {
            return Err(RoutingError::InvalidRequest);
        }
        Ok(())
    }
}
