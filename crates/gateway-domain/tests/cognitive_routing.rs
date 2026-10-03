use gateway_domain::cognitive_routing::*;
#[path = "../../../tests/fixtures/cognitive-routing.rs"]
mod fixtures;
use fixtures::*;

#[test]
fn malformed_requests_and_registries_fail_closed() {
    let base = snapshot(vec![candidate("slm", CognitiveRoute::LocalSlm)]);
    for change in 0..6 {
        let mut r = request();
        match change {
            0 => r.version = "2".into(),
            1 => r.input_contract.clear(),
            2 => r.output_contract = serde_json::Value::Null,
            3 => r.cost_unit.clear(),
            4 => r.max_attempts = 0,
            _ => r.max_latency_ms = 0,
        }
        assert_eq!(r.validate(), Err(RoutingError::InvalidRequest));
    }
    for change in 0..23 {
        let mut s = base.clone();
        match change {
            0 => s.version = "2".into(),
            1 => s.configuration_digest.clear(),
            2 => s.cost_unit.clear(),
            3 => s.candidates[0].id.clear(),
            4 => s.candidates.push(s.candidates[0].clone()),
            5 => s.candidates[0].contract_version = "2".into(),
            6 => s.candidates[0].tasks.clear(),
            7 => s.candidates[0].input_contracts.clear(),
            8 => s.candidates[0]
                .input_contracts
                .insert(" ".into())
                .then_some(())
                .unwrap(),
            9 => s.candidates[0].output_contracts.clear(),
            10 => s.candidates[0].output_contracts = vec![serde_json::Value::Null],
            11 => s.candidates[0].model = None,
            12 => s.candidates[0].kind = CognitiveRoute::Deterministic,
            13 => {
                s.candidates[0].procedure =
                    Some(gateway_domain::ReferenceId::new("unapproved").unwrap())
            }
            14 => s.candidates[0].privacy = PrivacyBoundary::External,
            15..=20 => {
                let m = s.candidates[0].model.as_mut().unwrap();
                match change {
                    15 => m.id.clear(),
                    16 => m.version.clear(),
                    17 => m.digest.clear(),
                    18 => m.runtime.clear(),
                    19 => m.runtime_version.clear(),
                    _ => m.quantization.clear(),
                }
            }
            21 => {
                s.candidates[0] = candidate("r", CognitiveRoute::Reflex);
                s.candidates[0].procedure = None;
            }
            _ => {
                s.candidates[0] = candidate("s", CognitiveRoute::SpecializedLocal);
                s.candidates[0].privacy = PrivacyBoundary::PrivateNetwork;
            }
        }
        assert_eq!(
            s.validate(),
            Err(RoutingError::InvalidRegistry),
            "change {change}"
        );
    }
}

#[test]
fn admitted_contracts_roundtrip_with_all_route_identities() {
    request().validate().unwrap();
    let candidates = [
        CognitiveRoute::Deterministic,
        CognitiveRoute::Reflex,
        CognitiveRoute::LocalSlm,
        CognitiveRoute::SpecializedLocal,
        CognitiveRoute::StrongLlm,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, kind)| candidate(&format!("route-{i}"), kind))
    .collect();
    let snapshot = snapshot(candidates);
    snapshot.validate().unwrap();
    let wire = serde_json::to_string(&snapshot).unwrap();
    assert_eq!(
        serde_json::from_str::<ModelCapabilitySnapshot>(&wire).unwrap(),
        snapshot
    );
    let wire = serde_json::to_string(&request()).unwrap();
    assert_eq!(
        serde_json::from_str::<CognitiveRouteRequest>(&wire).unwrap(),
        request()
    );
    let mut value = serde_json::to_value(&snapshot).unwrap();
    value["candidates"][0]["authority"] = true.into();
    assert!(serde_json::from_value::<ModelCapabilitySnapshot>(value).is_err());
}
