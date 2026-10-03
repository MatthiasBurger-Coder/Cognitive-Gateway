//! CG-26 deterministic advisory selection and bounded execution coordination.
use crate::context_application::CompiledStep;
use gateway_domain::cognitive_routing::*;
use serde::Serialize;
use std::collections::BTreeSet;

/// Host-owned, immutable qualified capability/configuration snapshot.
pub trait ModelCapabilityPort {
    fn snapshot(&self) -> Result<ModelCapabilitySnapshot, RoutingError>;
}

impl ModelCapabilityPort for gateway_registry::model_capabilities::ModelCapabilityRegistry {
    fn snapshot(&self) -> Result<ModelCapabilitySnapshot, RoutingError> {
        Ok(self.snapshot().clone())
    }
}

/// Hard filters precede lexicographic ranking: route kind, cost, latency, ID.
/// All alternatives retain every failed filter; eligible losers record precedence.
pub fn select_route(
    request: &CognitiveRouteRequest,
    snapshot: &ModelCapabilitySnapshot,
) -> Result<RouteExplanation, RoutingError> {
    evaluate(request, snapshot, &BTreeSet::new())
}

fn evaluate(
    request: &CognitiveRouteRequest,
    snapshot: &ModelCapabilitySnapshot,
    attempted: &BTreeSet<String>,
) -> Result<RouteExplanation, RoutingError> {
    request.validate()?;
    snapshot.validate()?;
    if request.cost_unit != snapshot.cost_unit {
        return Err(RoutingError::InvalidRequest);
    }
    let mut candidates = snapshot.candidates.clone();
    candidates.sort_by(|a, b| {
        (a.kind, a.cost, a.latency_ms, &a.id).cmp(&(b.kind, b.cost, b.latency_ms, &b.id))
    });
    let mut selected = None;
    let mut alternatives = Vec::new();
    for c in candidates {
        let mut reasons = BTreeSet::new();
        let checks = [
            (!c.qualified, RouteRejection::Unqualified),
            (!c.available, RouteRejection::Unavailable),
            (!c.tasks.contains(&request.task), RouteRejection::TaskClass),
            (c.max_novelty < request.novelty, RouteRejection::Novelty),
            (
                c.max_reasoning_depth < request.reasoning_depth,
                RouteRejection::ReasoningDepth,
            ),
            (
                c.max_uncertainty < request.uncertainty,
                RouteRejection::Uncertainty,
            ),
            (
                c.min_evidence > request.evidence_completeness,
                RouteRejection::EvidenceIncomplete,
            ),
            (
                !c.input_contracts.contains(&request.input_contract),
                RouteRejection::InputContract,
            ),
            (
                !c.output_contracts.contains(&request.output_contract),
                RouteRejection::OutputContract,
            ),
            (c.privacy > request.max_privacy, RouteRejection::Privacy),
            (
                !request.hardware.contains(&c.hardware),
                RouteRejection::Hardware,
            ),
            (c.cost > request.max_cost, RouteRejection::Cost),
            (
                c.latency_ms > request.max_latency_ms,
                RouteRejection::Latency,
            ),
            (
                c.kind == CognitiveRoute::Deterministic && !request.deterministic_sufficient,
                RouteRejection::DeterministicInsufficient,
            ),
            (
                c.kind == CognitiveRoute::Reflex && !request.reflex_applicable,
                RouteRejection::ReflexInapplicable,
            ),
            (
                attempted.contains(&c.id),
                RouteRejection::PreviouslyAttempted,
            ),
        ];
        for (rejected, reason) in checks {
            if rejected {
                reasons.insert(reason);
            }
        }
        if reasons.is_empty() {
            if selected.is_none() {
                selected = Some(c.clone());
            } else {
                reasons.insert(RouteRejection::LowerPrecedence);
            }
        }
        alternatives.push(RouteEvaluation {
            candidate: c,
            reasons,
        });
    }
    Ok(RouteExplanation {
        version: COGNITIVE_ROUTING_VERSION.into(),
        request: request.clone(),
        configuration_digest: snapshot.configuration_digest.clone(),
        selected,
        alternatives,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RouteAttemptOutcome {
    Success,
    Unavailable,
    ModelFailure,
    InvalidOutput,
}

pub struct RouteHandoff<'a> {
    pub candidate: &'a RouteCandidate,
    pub request: &'a CognitiveRouteRequest,
    pub step: &'a CompiledStep,
}

/// Execution report pins the actual model artifact, including failed calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteAttemptReport {
    pub model: Option<ModelIdentity>,
    pub outcome: RouteAttemptOutcome,
    pub output_reference: Option<gateway_domain::ReferenceId>,
    pub cost_unit: String,
    pub cost: u64,
    pub latency_ms: u64,
}

/// Trusted host boundary. Every attempt obtains a freshly compiled CG-10 step
/// (current Process/Policy), and reflex adapters must use the CG-25 engine.
/// Models receive read-only context and produce proposals, never transitions.
pub trait CognitiveRouteRuntime {
    fn prepare(
        &self,
        request: &CognitiveRouteRequest,
        candidate: &RouteCandidate,
    ) -> Result<CompiledStep, crate::context_application::ContextApplicationError>;
    fn attempt(&self, handoff: RouteHandoff<'_>) -> RouteAttemptReport;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RoutingDisposition {
    Success,
    NoCompatibleRoute,
    AttemptsExhausted,
    ProcessOrPolicyDenied,
    OutputContractMismatch,
    InvalidReport,
    BudgetExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteExecutionProvenance {
    pub candidate_id: String,
    pub context_id: String,
    pub resolution_basis: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoutingTelemetry {
    pub version: String,
    pub decisions: Vec<RouteExplanation>,
    pub attempts: Vec<RouteAttemptReport>,
    pub execution_provenance: Vec<RouteExecutionProvenance>,
    pub disposition: RoutingDisposition,
    /// Conservative accounting: max(reservation, measurement), even on failure.
    pub consumed_cost: u64,
    pub consumed_latency_ms: u64,
}

impl RoutingTelemetry {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// One frozen registry snapshot per operation. Never retry a failed candidate;
/// fallback reevaluates all original hard criteria against remaining budgets.
pub fn route_and_execute(
    request: &CognitiveRouteRequest,
    registry: &dyn ModelCapabilityPort,
    runtime: &dyn CognitiveRouteRuntime,
) -> Result<RoutingTelemetry, RoutingError> {
    request.validate()?;
    let snapshot = registry.snapshot()?;
    snapshot.validate()?;
    let mut remaining = request.clone();
    let mut attempted = BTreeSet::new();
    let mut telemetry = RoutingTelemetry {
        version: COGNITIVE_ROUTING_VERSION.into(),
        decisions: vec![],
        attempts: vec![],
        execution_provenance: vec![],
        disposition: RoutingDisposition::AttemptsExhausted,
        consumed_cost: 0,
        consumed_latency_ms: 0,
    };
    for _ in 0..request.max_attempts {
        let decision = evaluate(&remaining, &snapshot, &attempted)?;
        let candidate = decision.selected.clone();
        telemetry.decisions.push(decision);
        let Some(candidate) = candidate else {
            telemetry.disposition = RoutingDisposition::NoCompatibleRoute;
            break;
        };
        let step = match runtime.prepare(request, &candidate) {
            Ok(step) => step,
            Err(_) => {
                telemetry.disposition = RoutingDisposition::ProcessOrPolicyDenied;
                break;
            }
        };
        if step.output_contract() != &request.output_contract {
            telemetry.disposition = RoutingDisposition::OutputContractMismatch;
            break;
        }
        attempted.insert(candidate.id.clone());
        // Admission reserves the entire declared bound before handing control away.
        remaining.max_cost -= candidate.cost;
        remaining.max_latency_ms -= candidate.latency_ms;
        telemetry
            .execution_provenance
            .push(RouteExecutionProvenance {
                candidate_id: candidate.id.clone(),
                context_id: step.context().execution_context().id().as_str().into(),
                resolution_basis: crate::resolution_encoding::basis_json(step.basis()),
            });
        let report = runtime.attempt(RouteHandoff {
            candidate: &candidate,
            request,
            step: &step,
        });
        let cost = candidate.cost.max(report.cost);
        let latency = candidate.latency_ms.max(report.latency_ms);
        let totals = telemetry
            .consumed_cost
            .checked_add(cost)
            .zip(telemetry.consumed_latency_ms.checked_add(latency));
        let invalid = report.model != candidate.model
            || report.cost_unit != request.cost_unit
            || (report.outcome == RouteAttemptOutcome::Success)
                != report.output_reference.is_some();
        let exceeded = report.cost > candidate.cost
            || report.latency_ms > candidate.latency_ms
            || totals.is_none();
        let success = report.outcome == RouteAttemptOutcome::Success;
        telemetry.attempts.push(report);
        if let Some((cost, latency)) = totals {
            telemetry.consumed_cost = cost;
            telemetry.consumed_latency_ms = latency;
        } else {
            telemetry.consumed_cost = u64::MAX;
            telemetry.consumed_latency_ms = u64::MAX;
        }
        if invalid {
            telemetry.disposition = RoutingDisposition::InvalidReport;
            break;
        }
        if exceeded {
            telemetry.disposition = RoutingDisposition::BudgetExceeded;
            break;
        }
        if success {
            telemetry.disposition = RoutingDisposition::Success;
            break;
        }
        if remaining.max_latency_ms == 0 {
            telemetry.disposition = RoutingDisposition::BudgetExceeded;
            break;
        }
    }
    Ok(telemetry)
}
