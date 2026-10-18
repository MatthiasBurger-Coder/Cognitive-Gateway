use gateway_application::{cognitive_routing::*, model_invocation::*, model_routing::*};
use gateway_domain::{cognitive_routing::*, model_invocation::*, *};
#[allow(dead_code)]
#[path = "../../../tests/fixtures/model-routing-v1/support.rs"]
mod fixtures;
use fixtures::*;
#[allow(dead_code)]
#[path = "support/composition.rs"]
mod composition;
#[allow(dead_code)]
#[path = "support/context_fixture.rs"]
mod context_fixture;
#[allow(dead_code)]
mod support;
use std::cell::Cell;

#[test]
fn equivalent_snapshots_resolve_stably_with_pinned_effective_controls() {
    let mut s = snapshot();
    let first = resolve_invocation_route(&request(), &s).unwrap();
    s.routes.candidates.reverse();
    let second = resolve_invocation_route(&request(), &s).unwrap();
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
    assert_eq!(first.decision.selected.unwrap().id, "local-a");
    let p = first.prepared.unwrap();
    assert_eq!(
        p.invocation().data().capability,
        p.capability().data().identity
    );
    assert_eq!(p.effective_generation().controls.len(), 5);
}

#[test]
fn contract_and_runtime_failures_have_stable_diagnostics() {
    for (case, error) in [
        (0, ModelContractError::ModelUnavailable),
        (1, ModelContractError::ContextLimitExceeded),
        (2, ModelContractError::ModelCapabilityMismatch),
        (3, ModelContractError::ModelCapabilityMismatch),
        (4, ModelContractError::ModelCapabilityMismatch),
        (5, ModelContractError::ModelCapabilityMismatch),
        (6, ModelContractError::ModelCapabilityMismatch),
        (7, ModelContractError::InvalidBinding),
        (8, ModelContractError::InvalidBinding),
        (9, ModelContractError::ModelCapabilityMismatch),
        (10, ModelContractError::ModelUnavailable),
    ] {
        let mut s = snapshot();
        let b = s.bindings.get_mut("local-a").unwrap();
        let mut c = b.capability.data().clone();
        let mut p = b.provider.data().clone();
        match case {
            0 => b.runtime.availability = ModelAvailability::Unavailable,
            1 => c.context_limit = 1024,
            2 => c.task_classes = [TaskClass::Classification].into(),
            3 => {
                c.output_contracts.values_mut().next().unwrap().revision =
                    ReferenceId::new("8").unwrap()
            }
            4 => c.reasoning_strategies = [ReferenceId::new("other").unwrap()].into(),
            5 => p.maximum_sensitivity = SensitivityClass::Public,
            6 => {
                p.locality = PrivacyBoundary::External;
                s.routes.candidates[0].privacy = PrivacyBoundary::External;
                s.routes.candidates[0].kind = CognitiveRoute::StrongLlm;
            }
            7 => s.routes.candidates[0].model.as_mut().unwrap().digest = "b".repeat(64),
            8 => b.runtime.provider = contracts::pin("foreign"),
            9 => {
                p.controls.remove(&GenerationControl::Seed);
                p.unexposed_defaults.remove(&GenerationControl::Seed);
            }
            _ => b.runtime.availability = ModelAvailability::Unknown,
        }
        b.capability = ModelCapability::try_from(c).unwrap();
        b.provider = ModelProvider::try_from(p).unwrap();
        let resolved = resolve_invocation_route(&request(), &s).unwrap();
        assert_eq!(
            resolved.decision.selected.unwrap().id,
            "local-b",
            "case {case}"
        );
        assert!(
            resolved
                .decision
                .alternatives
                .iter()
                .find(|a| a.candidate.id == "local-a")
                .unwrap()
                .reasons
                .contains(&RouteRejection::Invocation(error)),
            "case {case}"
        );
    }
}

#[test]
fn all_cg26_filters_still_apply_after_invocation_negotiation() {
    for case in 0..5 {
        let mut s = snapshot();
        let mut r = request();
        let reason = match case {
            0 => {
                s.routes.candidates[0].hardware = HardwareRequirement::Gpu;
                RouteRejection::Hardware
            }
            1 => {
                s.routes.candidates[0].cost = 11;
                RouteRejection::Cost
            }
            2 => {
                s.routes.candidates[0].latency_ms = 101;
                RouteRejection::Latency
            }
            3 => {
                s.routes.candidates[0].privacy = PrivacyBoundary::PrivateNetwork;
                r.max_privacy = PrivacyBoundary::OnDevice;
                s.routes.candidates[0].kind = CognitiveRoute::StrongLlm;
                RouteRejection::Privacy
            }
            _ => {
                s.routes.candidates[0].output_contracts = vec![serde_json::json!("wrong")];
                RouteRejection::OutputContract
            }
        };
        let result = resolve_invocation_route(&r, &s).unwrap();
        assert_eq!(result.decision.selected.unwrap().id, "local-b");
        assert!(
            result
                .decision
                .alternatives
                .iter()
                .find(|a| a.candidate.id == "local-a")
                .unwrap()
                .reasons
                .contains(&reason)
        );
    }
}

#[test]
fn malformed_registry_or_widened_fallback_is_rejected_before_execution() {
    for case in 0..9 {
        let mut s = snapshot();
        let mut r = request();
        let expected = match case {
            0 => {
                s.schema_version = SchemaVersion::V2;
                RoutingError::InvalidRegistry
            }
            1 => {
                s.bindings.remove("local-a");
                RoutingError::InvalidRegistry
            }
            2 => {
                s.bindings
                    .insert("orphan".into(), fixtures::binding("orphan"));
                RoutingError::InvalidRegistry
            }
            3 => {
                r.task = TaskClass::Resolution;
                RoutingError::InvalidRequest
            }
            4 => {
                r.input_contract = "other".into();
                RoutingError::InvalidRequest
            }
            5 => {
                r.max_attempts = 3;
                RoutingError::InvalidRequest
            }
            6 => {
                r.max_latency_ms = 10001;
                RoutingError::InvalidRequest
            }
            7 => {
                let b = s.bindings.get_mut("local-b").unwrap();
                let mut d = b.invocation.data().clone();
                d.strategy = ReferenceId::new("other").unwrap();
                b.invocation = contracts::admit(d).unwrap();
                RoutingError::InvalidRequest
            }
            _ => {
                r.cost_unit = "other".into();
                RoutingError::InvalidRequest
            }
        };
        assert_eq!(
            resolve_invocation_route(&r, &s).unwrap_err(),
            expected,
            "case {case}"
        );
    }
}

struct Runtime {
    fixture: context_fixture::Fixture,
    prepares: Cell<usize>,
    model_calls: Cell<usize>,
    calls: Cell<usize>,
    deny_on: Option<usize>,
    succeed_on: usize,
}
impl Runtime {
    fn new() -> Self {
        Self {
            fixture: context_fixture::Fixture::new(),
            prepares: Cell::new(0),
            model_calls: Cell::new(0),
            calls: Cell::new(0),
            deny_on: None,
            succeed_on: 1,
        }
    }
    fn request(&self) -> CognitiveRouteRequest {
        let mut r = request();
        r.output_contract = self.fixture.compile().unwrap().output_contract().clone();
        r
    }
}
impl InvocationRouteRuntime for Runtime {
    fn prepare(
        &self,
        _: &CognitiveRouteRequest,
        _: &RouteCandidate,
    ) -> Result<
        gateway_application::context_application::CompiledStep,
        gateway_application::context_application::ContextApplicationError,
    > {
        let n = self.prepares.get();
        self.prepares.set(n + 1);
        if self.deny_on == Some(n) {
            return Err(
                gateway_application::context_application::ContextApplicationError::UnknownStep,
            );
        }
        self.fixture.compile()
    }
    fn attempt(
        &self,
        h: RouteHandoff<'_>,
        prepared: Option<&PreparedModelInvocation>,
    ) -> RouteAttemptReport {
        assert_eq!(prepared.is_some(), h.candidate.model.is_some());
        if let Some(p) = prepared {
            self.model_calls.set(self.model_calls.get() + 1);
            assert_eq!(p.capability().data().identity.id.as_str(), h.candidate.id);
        }
        let n = self.calls.get();
        self.calls.set(n + 1);
        RouteAttemptReport {
            model: h.candidate.model.clone(),
            outcome: if n == self.succeed_on {
                RouteAttemptOutcome::Success
            } else {
                RouteAttemptOutcome::ModelFailure
            },
            output_reference: (n == self.succeed_on).then(|| ReferenceId::new("proposal").unwrap()),
            cost_unit: h.request.cost_unit.clone(),
            cost: 0,
            latency_ms: 0,
        }
    }
}
fn execution_snapshot(r: &CognitiveRouteRequest) -> InvocationRoutingSnapshot {
    let mut s = snapshot();
    for c in &mut s.routes.candidates {
        c.output_contracts = vec![r.output_contract.clone()];
    }
    s
}

#[test]
fn fallback_uses_same_contracts_and_remaining_cumulative_budget() {
    let runtime = Runtime::new();
    let r = runtime.request();
    let s = execution_snapshot(&r);
    let trace = route_invocations_and_execute(&r, &s, &runtime).unwrap();
    assert_eq!(trace.routing.disposition, RoutingDisposition::Success);
    assert_eq!(trace.routing.attempts.len(), 2);
    assert_eq!(runtime.prepares.get(), 2);
    assert_eq!(runtime.model_calls.get(), 2);
    assert_eq!(trace.routing.consumed_cost, 2);
    assert_eq!(trace.routing.decisions[1].request.max_cost, r.max_cost - 1);
    assert!(
        trace.routing.decisions[1].alternatives[0]
            .reasons
            .contains(&RouteRejection::PreviouslyAttempted)
    );
    assert!(
        serde_json::to_string(&trace)
            .unwrap()
            .contains("observation")
    );
    assert_eq!(
        trace.snapshot_digest,
        resolve_invocation_route(&r, &s).unwrap().snapshot_digest
    );
    for case in 0..3 {
        let mut runtime = Runtime::new();
        let mut r = runtime.request();
        let expected = match case {
            0 => {
                r.max_cost = 1;
                RoutingDisposition::NoCompatibleRoute
            }
            1 => {
                runtime.deny_on = Some(1);
                RoutingDisposition::ProcessOrPolicyDenied
            }
            _ => {
                r.max_attempts = 1;
                RoutingDisposition::AttemptsExhausted
            }
        };
        let trace = route_invocations_and_execute(&r, &execution_snapshot(&r), &runtime).unwrap();
        assert_eq!(trace.routing.disposition, expected);
        assert_eq!(runtime.model_calls.get(), 1);
    }
}

#[test]
fn deterministic_execution_and_failure_never_call_a_model_and_reflex_is_preserved() {
    for kind in [CognitiveRoute::Deterministic, CognitiveRoute::Reflex] {
        for available in [false, true] {
            let runtime = Runtime::new();
            let mut r = runtime.request();
            r.deterministic_sufficient = true;
            let mut s = execution_snapshot(&r);
            let mut c = candidate("no-model", kind);
            c.output_contracts = vec![r.output_contract.clone()];
            c.available = available;
            s.routes.candidates.push(c);
            let resolved = resolve_invocation_route(&r, &s).unwrap();
            assert!(resolved.prepared.is_none());
            let trace = route_invocations_and_execute(&r, &s, &runtime).unwrap();
            assert_eq!(runtime.model_calls.get(), 0);
            assert_eq!(runtime.calls.get(), usize::from(available));
            assert_eq!(
                trace.routing.disposition,
                RoutingDisposition::NoCompatibleRoute
            );
            for a in &trace.routing.decisions[0].alternatives {
                if a.candidate.model.is_some() {
                    assert!(a.reasons.contains(&RouteRejection::ModelNotRequired));
                }
            }
        }
    }
    let runtime = Runtime::new();
    let r = runtime.request();
    let s = InvocationRoutingSnapshot {
        schema_version: SchemaVersion::V1,
        routes: cg26::snapshot(vec![]),
        bindings: Default::default(),
    };
    assert!(
        resolve_invocation_route(&r, &s)
            .unwrap()
            .decision
            .selected
            .is_none()
    );
}

#[test]
fn individually_admitted_alternatives_cannot_change_meaning_or_authority() {
    for case in 0..9 {
        let mut s = snapshot();
        let b = s.bindings.get_mut("local-b").unwrap();
        let mut d = b.invocation.data().clone();
        match case {
            0 => d.compiled_context = contracts::pin("other-context"),
            1 => d.authority = contracts::pin("other-authority"),
            2 => d
                .permitted_localities
                .insert(PrivacyBoundary::External)
                .then_some(())
                .unwrap(),
            3 => d.sensitivity = SensitivityClass::Public,
            4 => d.input_tokens += 1,
            5 => d.max_attempts += 1,
            6 => d.output.shape = gateway_domain::semantic_output::OutputType::Boolean {},
            7 => {
                let mut g = d.generation.data().clone();
                g.max_output_tokens += 1;
                d.generation = GenerationProfile::try_from(g).unwrap();
            }
            _ => {
                let pin = contracts::pin("additional-tool");
                let id = CapabilityId::new("additional-tool").unwrap();
                d.permitted_capabilities.insert(
                    id.clone(),
                    ResolvedReference {
                        id,
                        scope: pin.scope,
                        contract: pin.contract,
                        contract_version: pin.contract_version,
                        revision: pin.revision,
                        digest: pin.digest,
                    },
                );
            }
        }
        let invocation = ModelInvocation::new(
            d.clone(),
            ModelInvocationBasis {
                task: &contracts::task(),
                output: &d.output,
                compiled_context: &d.compiled_context,
                permitted_capabilities: &d.permitted_capabilities,
                sensitivity: d.sensitivity,
                permitted_localities: &d.permitted_localities,
                authority: &d.authority,
            },
        )
        .unwrap();
        b.invocation = invocation;
        assert_eq!(
            resolve_invocation_route(&request(), &s).unwrap_err(),
            RoutingError::InvalidRequest,
            "case {case}"
        );
    }
}

#[test]
fn unavailable_local_provider_cannot_escalate_to_disallowed_external_provider() {
    let mut s = snapshot();
    s.bindings.get_mut("local-a").unwrap().runtime.availability = ModelAvailability::Unavailable;
    let b = s.bindings.get_mut("local-b").unwrap();
    let mut p = b.provider.data().clone();
    p.locality = PrivacyBoundary::External;
    b.provider = ModelProvider::try_from(p).unwrap();
    s.routes.candidates[1].kind = CognitiveRoute::StrongLlm;
    s.routes.candidates[1].privacy = PrivacyBoundary::External;
    let result = resolve_invocation_route(&request(), &s).unwrap();
    assert!(result.decision.selected.is_none());
    assert!(result.prepared.is_none());
    assert!(
        result.decision.alternatives[1]
            .reasons
            .contains(&RouteRejection::Invocation(
                ModelContractError::ModelCapabilityMismatch
            ))
    );
}

#[test]
fn no_model_installation_is_needed_for_successful_deterministic_or_reflex_execution() {
    for kind in [CognitiveRoute::Deterministic, CognitiveRoute::Reflex] {
        let mut runtime = Runtime::new();
        runtime.succeed_on = 0;
        let mut r = runtime.request();
        r.deterministic_sufficient = true;
        let mut c = candidate("no-model", kind);
        c.output_contracts = vec![r.output_contract.clone()];
        let s = InvocationRoutingSnapshot {
            schema_version: SchemaVersion::V1,
            routes: cg26::snapshot(vec![c]),
            bindings: Default::default(),
        };
        let trace = route_invocations_and_execute(&r, &s, &runtime).unwrap();
        assert_eq!(trace.routing.disposition, RoutingDisposition::Success);
        assert_eq!(runtime.model_calls.get(), 0);
        assert!(trace.effective_invocations.is_empty());
    }
}
