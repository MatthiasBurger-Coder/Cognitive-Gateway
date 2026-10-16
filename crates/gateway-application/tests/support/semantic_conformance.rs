use super::*;
use serde_json::Value as Json;

#[test]
fn portable_adversarial_resolution_corpus() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/cgsl-conformance-v1");
    let manifest: Json =
        serde_json::from_slice(&std::fs::read(root.join("resolution.json")).unwrap()).unwrap();
    assert_eq!(manifest["version"], "1.0");
    for case in manifest["cases"].as_array().unwrap() {
        let host = Host::default();
        host.denied.set(case["host"] == "deny");
        let bytes = std::fs::read(root.join(case["context"].as_str().unwrap())).unwrap();
        let context =
            InterpretationContext::admit_json(&bytes, &host, time(case["at"].as_i64().unwrap()));
        if matches!(case["state"].as_str(), Some("Scope" | "Admission")) {
            assert_eq!(
                format!("{:?}", context.unwrap_err()),
                case["state"].as_str().unwrap()
            );
            continue;
        }
        let context = context.unwrap();
        let mut q = query();
        match case["expression"].as_str().unwrap() {
            "focus" => {}
            "alias" => q = entity_query(true),
            "absent" => {
                q.expression = ReferenceExpression::Entity(EntityQuery::Canonical {
                    id: id("absent"),
                    kind: TargetKind::Service,
                })
            }
            "yesterday" => q = temporal().1,
            other => panic!("unknown fixture expression: {other}"),
        }
        let lookup = Lookup::default();
        let resolver = DeterministicInterpretationResolver {
            host: &host,
            registry: &lookup,
            project: &lookup,
        };
        let decision = resolver.resolve(&context, &q, time(100)).unwrap();
        assert_eq!(
            serde_json::to_value(decision.resolution.state()).unwrap(),
            case["state"],
            "{}",
            case["id"]
        );
        if let Some(code) = case["code"].as_str() {
            assert_eq!(decision.resolution.require_resolved(), Err(code));
        }
        if let Some(selected) = case["selected"].as_str() {
            assert_eq!(
                decision.resolution.require_resolved().unwrap(),
                &InterpretationValue::Target(target(selected))
            );
        }
        let wire: Json = serde_json::from_slice(&decision.canonical_bytes()).unwrap();
        assert_eq!(wire["candidates"], case["candidates"], "{}", case["id"]);
        for _ in 0..8 {
            assert_eq!(
                decision.canonical_bytes(),
                resolver
                    .resolve(&context, &q, time(100))
                    .unwrap()
                    .canonical_bytes()
            );
        }
    }
}
