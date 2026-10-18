//! EPIC-06.02: strict invocation eligibility composed with the CG-26 router.
//! Captured, host-admitted alternatives may differ only in their capability pin.
use crate::cognitive_routing::*;
use crate::context_application::{CompiledStep, ContextApplicationError};
use crate::model_invocation::{PreparedModelInvocation, prepare_model_invocation};
use gateway_domain::cognitive_routing::*;
use gateway_domain::model_invocation::*;
use gateway_domain::{ContentDigest, SchemaVersion, context_content_digest};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Host-owned frozen declarations plus independent health observations. No SDK types.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InvocationRouteBinding {
    pub invocation: ModelInvocation,
    pub capability: ModelCapability,
    pub provider: ModelProvider,
    pub runtime: ModelRuntimeStatus,
}

/// Bindings are keyed by CG-26 candidate ID, with exactly one for every model route.
/// Deterministic/reflex routes need no model contracts or inference service.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InvocationRoutingSnapshot {
    pub schema_version: SchemaVersion,
    pub routes: ModelCapabilitySnapshot,
    pub bindings: BTreeMap<String, InvocationRouteBinding>,
}
/// Output-only negotiation result; preparation cannot be forged by deserialization.
#[derive(Debug, Clone, Serialize)]
pub struct InvocationRouteResolution {
    pub schema_version: SchemaVersion,
    pub snapshot_digest: ContentDigest,
    pub decision: RouteExplanation,
    pub prepared: Option<PreparedModelInvocation>,
}

struct Admission {
    rejections: BTreeMap<String, BTreeSet<RouteRejection>>,
    prepared: BTreeMap<String, PreparedModelInvocation>,
    digest: ContentDigest,
}

fn admit(
    request: &CognitiveRouteRequest,
    snapshot: &InvocationRoutingSnapshot,
) -> Result<Admission, RoutingError> {
    request.validate()?;
    snapshot.routes.validate()?;
    if snapshot.schema_version != SchemaVersion::V1 {
        return Err(RoutingError::InvalidRegistry);
    }
    let model_ids: BTreeSet<_> = snapshot
        .routes
        .candidates
        .iter()
        .filter(|c| c.model.is_some())
        .map(|c| c.id.clone())
        .collect();
    if model_ids != snapshot.bindings.keys().cloned().collect() {
        return Err(RoutingError::InvalidRegistry);
    }
    let mut common = None;
    let mut canonical = snapshot.clone();
    canonical.routes.candidates.sort_by(|a, b| a.id.cmp(&b.id));
    let mut admission = Admission {
        rejections: BTreeMap::new(),
        prepared: BTreeMap::new(),
        digest: context_content_digest(&serde_json::to_vec(&canonical).expect("finite contracts")),
    };
    for c in &snapshot.routes.candidates {
        let Some(binding) = snapshot.bindings.get(&c.id) else {
            continue;
        };
        let d = binding.invocation.data();
        // Exact task/context/output/tools/authority/generation/budgets across fallback.
        // Capability pin is the sole pre-admitted variation; never manufacture admission.
        let mut basis = d.clone();
        basis.capability = d.authority.clone();
        let encoded = serde_json::to_vec(&basis).expect("finite invocation");
        if let Some(previous) = &common {
            if previous != &encoded {
                return Err(RoutingError::InvalidRequest);
            }
        } else {
            common = Some(encoded);
        }
        if d.task_class != request.task
            || d.input_contract.as_str() != request.input_contract
            || request.max_attempts > u32::from(d.max_attempts)
            || request.max_latency_ms > u64::from(d.timeout_ms)
        {
            return Err(RoutingError::InvalidRequest);
        }
        let reasons = admission.rejections.entry(c.id.clone()).or_default();
        if request.deterministic_sufficient {
            reasons.insert(RouteRejection::ModelNotRequired);
        }
        let capability = binding.capability.data();
        let provider = binding.provider.data();
        let model = c.model.as_ref().expect("validated model route");
        if model.id != capability.model.id.as_str()
            || model.version != capability.model.revision.as_str()
            || model.digest != capability.model.digest.as_str()
            || model.runtime != provider.adapter.id.as_str()
            || model.runtime_version != provider.adapter.revision.as_str()
            || c.privacy != provider.locality
        {
            reasons.insert(RouteRejection::Invocation(
                ModelContractError::InvalidBinding,
            ));
        }
        match prepare_model_invocation(
            binding.invocation.clone(),
            binding.capability.clone(),
            binding.provider.clone(),
            &binding.runtime,
        ) {
            Ok(prepared) => {
                admission.prepared.insert(c.id.clone(), prepared);
            }
            Err(error) => {
                reasons.insert(RouteRejection::Invocation(error));
            }
        }
    }
    Ok(admission)
}

pub fn resolve_invocation_route(
    request: &CognitiveRouteRequest,
    snapshot: &InvocationRoutingSnapshot,
) -> Result<InvocationRouteResolution, RoutingError> {
    let mut admission = admit(request, snapshot)?;
    let decision = evaluate(
        request,
        &snapshot.routes,
        &BTreeSet::new(),
        &admission.rejections,
    )?;
    let prepared = decision
        .selected
        .as_ref()
        .and_then(|c| admission.prepared.remove(&c.id));
    Ok(InvocationRouteResolution {
        schema_version: SchemaVersion::V1,
        snapshot_digest: admission.digest,
        decision,
        prepared,
    })
}

/// Trusted host retains CG-10 / Process / Policy ownership. Every fallback prepares
/// afresh. #263 owns full compiled-context binding; #269 owns inference adapters.
/// A model handoff always contains strictly negotiated contracts and controls;
/// deterministic/reflex handoffs contain None and MUST NOT call an inference port.
pub trait InvocationRouteRuntime {
    fn prepare(
        &self,
        request: &CognitiveRouteRequest,
        candidate: &RouteCandidate,
    ) -> Result<CompiledStep, ContextApplicationError>;
    fn attempt(
        &self,
        handoff: RouteHandoff<'_>,
        prepared: Option<&PreparedModelInvocation>,
    ) -> RouteAttemptReport;
}

#[derive(Debug, Clone, Serialize)]
pub struct InvocationRoutingTelemetry {
    pub schema_version: SchemaVersion,
    pub snapshot_digest: ContentDigest,
    /// Full captured contracts, health provenance and effective controls for audit.
    pub snapshot: InvocationRoutingSnapshot,
    pub effective_invocations: BTreeMap<String, PreparedModelInvocation>,
    pub routing: RoutingTelemetry,
}

pub fn route_invocations_and_execute(
    request: &CognitiveRouteRequest,
    snapshot: &InvocationRoutingSnapshot,
    runtime: &dyn InvocationRouteRuntime,
) -> Result<InvocationRoutingTelemetry, RoutingError> {
    let admission = admit(request, snapshot)?;
    struct FrozenRoutes<'a>(&'a ModelCapabilitySnapshot);
    impl ModelCapabilityPort for FrozenRoutes<'_> {
        fn snapshot(&self) -> Result<ModelCapabilitySnapshot, RoutingError> {
            Ok(self.0.clone())
        }
    }
    struct Runtime<'a> {
        host: &'a dyn InvocationRouteRuntime,
        prepared: &'a BTreeMap<String, PreparedModelInvocation>,
    }
    impl CognitiveRouteRuntime for Runtime<'_> {
        fn prepare(
            &self,
            request: &CognitiveRouteRequest,
            candidate: &RouteCandidate,
        ) -> Result<CompiledStep, ContextApplicationError> {
            self.host.prepare(request, candidate)
        }
        fn attempt(&self, handoff: RouteHandoff<'_>) -> RouteAttemptReport {
            let prepared = self.prepared.get(&handoff.candidate.id);
            self.host.attempt(handoff, prepared)
        }
    }
    let routing = route_with_rejections(
        request,
        &FrozenRoutes(&snapshot.routes),
        &Runtime {
            host: runtime,
            prepared: &admission.prepared,
        },
        &admission.rejections,
    )?;
    Ok(InvocationRoutingTelemetry {
        schema_version: SchemaVersion::V1,
        snapshot_digest: admission.digest,
        snapshot: snapshot.clone(),
        effective_invocations: admission.prepared,
        routing,
    })
}
