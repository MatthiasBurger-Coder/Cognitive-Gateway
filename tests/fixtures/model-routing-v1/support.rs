#[allow(dead_code)]
#[path = "../cognitive-routing.rs"]
pub mod cg26;
#[allow(dead_code)]
#[path = "../model-invocation-v1/support.rs"]
pub mod contracts;
use gateway_application::model_routing::*;
use gateway_domain::{cognitive_routing::*, model_invocation::*, *};
use std::collections::BTreeMap;

pub fn binding(id: &str) -> InvocationRouteBinding {
    let mut c = contracts::capability_data();
    c.identity = contracts::pin(id);
    let mut d = contracts::invocation_data();
    d.capability = c.identity.clone();
    InvocationRouteBinding {
        invocation: contracts::admit(d).unwrap(),
        capability: ModelCapability::try_from(c).unwrap(),
        provider: contracts::provider(),
        runtime: contracts::runtime(),
    }
}
pub fn candidate(id: &str, kind: CognitiveRoute) -> RouteCandidate {
    let mut c = cg26::candidate(id, kind);
    c.tasks = [TaskClass::Extraction].into();
    c.input_contracts = ["prompt-ir-v1".into()].into();
    if let Some(m) = &mut c.model {
        m.id = "model".into();
        m.version = "7".into();
        m.digest = "a".repeat(64);
        m.runtime = "adapter".into();
        m.runtime_version = "7".into();
    }
    c
}
pub fn request() -> CognitiveRouteRequest {
    let mut r = cg26::request();
    r.task = TaskClass::Extraction;
    r.input_contract = "prompt-ir-v1".into();
    r.deterministic_sufficient = false;
    r.max_attempts = 2;
    r
}
pub fn snapshot() -> InvocationRoutingSnapshot {
    InvocationRoutingSnapshot {
        schema_version: SchemaVersion::V1,
        routes: cg26::snapshot(vec![
            candidate("local-a", CognitiveRoute::LocalSlm),
            candidate("local-b", CognitiveRoute::LocalSlm),
        ]),
        bindings: BTreeMap::from([
            ("local-a".into(), binding("local-a")),
            ("local-b".into(), binding("local-b")),
        ]),
    }
}
