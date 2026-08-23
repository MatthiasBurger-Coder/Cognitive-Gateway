use std::path::PathBuf;

use gateway_registry::Registry;

fn repository_catalog() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog")
}

#[test]
fn migrated_generic_catalog_loads_and_validates() {
    let registry = Registry::load_catalog(repository_catalog()).expect("catalog should load");

    registry
        .validate_integrity()
        .expect("catalog references should be valid");

    let agent_ids: Vec<_> = registry.agents().ids().map(ToString::to_string).collect();
    assert_eq!(
        agent_ids,
        vec![
            "analysis-storage-architect",
            "git-workspace-specialist",
            "grpc-proto-specialist",
            "performance-engineer",
            "plugin-integration-developer",
            "security-sandbox-engineer",
            "system-architect",
            "tester",
        ]
    );

    let skill_ids: Vec<_> = registry.skills().ids().map(ToString::to_string).collect();
    assert_eq!(skill_ids.len(), 37);
    assert!(skill_ids.contains(&"architecture-hexagonal".to_owned()));
    assert!(skill_ids.contains(&"contract-governance-expert".to_owned()));
    assert!(skill_ids.contains(&"observability-diagnostics".to_owned()));
}
