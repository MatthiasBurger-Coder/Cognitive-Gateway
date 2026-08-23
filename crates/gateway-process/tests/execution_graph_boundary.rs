use std::path::PathBuf;

use gateway_process::ProcessRegistry;
use serde_json::Value;

const GAP_MATRIX: &str = include_str!("../../../docs/execution-graph-migration-gaps.json");
const INVENTORY: &str = include_str!("../../../docs/tiny-swarm-world-process-inventory.json");

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/execution-graph")
        .join(name);
    let source = std::fs::read_to_string(path).expect("execution graph fixture exists");
    serde_json::from_str(&source).expect("execution graph fixture is valid JSON")
}

fn string_array<'a>(value: &'a Value, key: &str) -> Vec<&'a str> {
    value[key]
        .as_array()
        .expect("fixture field is an array")
        .iter()
        .map(|item| item.as_str().expect("fixture array item is a string"))
        .collect()
}

#[test]
fn neutral_fixtures_cover_graph_groups_locks_parallelism_and_barriers() {
    let graph = fixture("neutral-graph.json");
    assert_eq!(graph["fixture_kind"], "execution_graph");
    assert_eq!(graph["expected"]["acyclic"], true);
    assert_eq!(
        string_array(&graph["expected"], "topological_order"),
        vec![
            "slice-intake",
            "slice-implementation-a",
            "slice-implementation-b",
            "slice-verify"
        ]
    );
    assert!(
        graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|node| { node["id"].is_string() && node["depends_on"].is_array() })
    );
    assert_eq!(graph["execution_groups"].as_array().unwrap().len(), 3);
    assert!(
        graph["execution_groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|group| { group["mode"] == "sequential" })
    );
    assert!(
        graph["execution_groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|group| { group["mode"] == "parallel" })
    );
    assert_eq!(graph["locks"].as_array().unwrap().len(), 2);
    assert_eq!(
        graph["join_barriers"][0]["waits_for"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(graph["parallelization"]["decision"], "explicit-input");
    assert!(graph["stream_distribution"]["metadata"].is_object());
    assert_eq!(graph["failure_routing"]["verification-failed"], "repair");
    assert_eq!(graph["expected"]["process_ir_v1_support"], "not-evaluated");
}

#[test]
fn neutral_negative_fixtures_surface_rejection_before_execution() {
    for (name, reason) in [
        ("cycle.json", "dependency-cycle"),
        ("unknown-dependency.json", "unknown-dependency"),
    ] {
        let value = fixture(name);
        assert_eq!(value["expected"]["disposition"], "reject-before-execution");
        assert_eq!(value["expected"]["reason"], reason);
    }
    let lock_conflict = fixture("lock-conflict.json");
    assert_eq!(
        lock_conflict["expected"]["disposition"],
        "reject-parallel-plan"
    );
    assert_eq!(
        lock_conflict["expected"]["reason"],
        "exclusive-lock-conflict"
    );
}

#[test]
fn machine_readable_gap_matrix_covers_inventory_gaps_and_semantic_classes() {
    let matrix: Value = serde_json::from_str(GAP_MATRIX).expect("gap matrix is valid JSON");
    let inventory: Value = serde_json::from_str(INVENTORY).expect("inventory is valid JSON");
    assert_eq!(matrix["schema_version"], "cg-04.16-v1");

    let dispositions = matrix["semantic_classes"]
        .as_array()
        .expect("semantic classes are an array");
    for required in [
        "slice-task-dependency-graph",
        "topological-ordering",
        "unknown-dependency-and-cycle-rejection",
        "execution-groups",
        "sequential-vs-parallel-input",
        "resource-locks",
        "lock-conflict-handling",
        "join-barriers",
        "typed-failure-error-routing",
        "gate-evidence-interactions",
        "stop-conditions",
        "stream-distribution-metadata",
        "runtime-orchestration-boundary",
    ] {
        assert!(
            dispositions.iter().any(|item| item["id"] == required),
            "semantic class {required} is missing"
        );
    }

    let gaps = matrix["migration_gaps"]
        .as_array()
        .expect("migration gaps are an array");
    let inventory_gaps = inventory["explicit_gaps"]
        .as_array()
        .expect("inventory gaps are an array");
    for inventory_gap in inventory_gaps {
        let id = inventory_gap["id"].as_str().unwrap();
        let matrix_gap = gaps
            .iter()
            .find(|item| item["id"] == id)
            .expect("every inventory gap has a final disposition");
        assert!(matrix_gap["disposition"].is_string());
        assert!(matrix_gap["status"].is_string());
        assert!(matrix_gap["source_units"].as_array().is_some());
    }
    assert!(gaps.iter().all(|gap| gap["disposition"] != "implicit"));
}

#[test]
fn migrated_catalog_exposes_the_execution_graph_seam_without_claiming_support() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog/processes");
    let registry = ProcessRegistry::load(root).unwrap();
    assert_eq!(registry.len(), 5);
    for definition in registry.definitions() {
        let extension = definition
            .extensions()
            .iter()
            .find(|extension| extension.kind() == "execution-graph")
            .expect("every compiled process declares the graph extension seam");
        assert!(!extension.supported());
    }
}
