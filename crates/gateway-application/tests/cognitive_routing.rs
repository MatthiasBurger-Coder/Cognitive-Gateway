use gateway_application::cognitive_routing::*;
use gateway_domain::cognitive_routing::*;
use std::collections::BTreeSet;

#[path = "../../../tests/fixtures/cognitive-routing.rs"]
mod fixtures;
use fixtures::*;

#[test]
fn precedence_and_no_model_are_reproducible_across_registry_order() {
    let mut candidates = vec![
        candidate("strong", CognitiveRoute::StrongLlm),
        candidate("specialized", CognitiveRoute::SpecializedLocal),
        candidate("slm", CognitiveRoute::LocalSlm),
        candidate("reflex", CognitiveRoute::Reflex),
        candidate("deterministic", CognitiveRoute::Deterministic),
    ];
    let first = select_route(&request(), &snapshot(candidates.clone())).unwrap();
    assert_eq!(first.selected.as_ref().unwrap().model, None);
    candidates.reverse();
    assert_eq!(
        first,
        select_route(&request(), &snapshot(candidates)).unwrap()
    );
    assert!(
        first
            .alternatives
            .iter()
            .skip(1)
            .all(|a| a.reasons.contains(&RouteRejection::LowerPrecedence))
    );
    let mut req = request();
    req.deterministic_sufficient = false;
    assert_eq!(
        select_route(
            &req,
            &snapshot(vec![
                candidate("d", CognitiveRoute::Deterministic),
                candidate("r", CognitiveRoute::Reflex)
            ])
        )
        .unwrap()
        .selected
        .unwrap()
        .kind,
        CognitiveRoute::Reflex
    );
}

#[test]
fn all_hard_filters_are_explained_together() {
    let mut c = candidate("strong", CognitiveRoute::StrongLlm);
    c.available = false;
    c.qualified = false;
    c.tasks = BTreeSet::from([TaskClass::Ranking]);
    c.max_novelty = CognitiveLevel::Low;
    c.max_reasoning_depth = CognitiveLevel::Low;
    c.max_uncertainty = CognitiveLevel::Low;
    c.min_evidence = CognitiveLevel::High;
    c.input_contracts = BTreeSet::from(["other".into()]);
    c.output_contracts = vec![serde_json::json!("other")];
    c.hardware = HardwareRequirement::Gpu;
    c.cost = 11;
    c.latency_ms = 101;
    let mut req = request();
    req.max_privacy = PrivacyBoundary::OnDevice;
    req.novelty = CognitiveLevel::High;
    req.reasoning_depth = CognitiveLevel::High;
    req.uncertainty = CognitiveLevel::High;
    req.evidence_completeness = CognitiveLevel::Low;
    let explanation = select_route(&req, &snapshot(vec![c])).unwrap();
    assert!(explanation.selected.is_none());
    assert_eq!(explanation.alternatives[0].reasons.len(), 13);
    req.reflex_applicable = false;
    req.deterministic_sufficient = false;
    let trace = select_route(
        &req,
        &snapshot(vec![
            candidate("d", CognitiveRoute::Deterministic),
            candidate("r", CognitiveRoute::Reflex),
        ]),
    )
    .unwrap();
    assert!(
        trace.alternatives[0]
            .reasons
            .contains(&RouteRejection::DeterministicInsufficient)
    );
    assert!(
        trace.alternatives[1]
            .reasons
            .contains(&RouteRejection::ReflexInapplicable)
    );
}

#[test]
fn stable_ties_and_versioned_contract_roundtrip() {
    let mut b = candidate("b", CognitiveRoute::LocalSlm);
    let a = candidate("a", CognitiveRoute::LocalSlm);
    let mut s = snapshot(vec![b.clone(), a.clone()]);
    assert_eq!(
        select_route(&request(), &s).unwrap().selected.unwrap().id,
        "a"
    );
    b.cost = 0;
    s.candidates[0] = b;
    assert_eq!(
        select_route(&request(), &s).unwrap().selected.unwrap().id,
        "b"
    );
    let encoded = serde_json::to_string(&s).unwrap();
    assert_eq!(
        s,
        serde_json::from_str::<ModelCapabilitySnapshot>(&encoded).unwrap()
    );
    let mut value = serde_json::to_value(request()).unwrap();
    value["grant_authority"] = true.into();
    assert!(serde_json::from_value::<CognitiveRouteRequest>(value).is_err());
}

#[test]
fn cost_units_are_not_implicitly_converted() {
    let mut req = request();
    req.cost_unit = "other".into();
    assert_eq!(
        select_route(&req, &snapshot(vec![])),
        Err(RoutingError::InvalidRequest)
    );
}

#[test]
fn qualified_registry_exposes_a_stable_port_snapshot() {
    let expected = snapshot(vec![candidate("local", CognitiveRoute::LocalSlm)]);
    let registry =
        gateway_registry::model_capabilities::ModelCapabilityRegistry::new(expected.clone())
            .unwrap();
    assert_eq!(ModelCapabilityPort::snapshot(&registry).unwrap(), expected);
}
