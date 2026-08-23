use std::path::PathBuf;

use gateway_registry::Registry;

fn repository_root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../{name}"))
}

#[test]
fn example_project_profile_loads_with_the_catalog() {
    let registry = Registry::load_catalog_with_profile(
        repository_root("catalog"),
        repository_root("profiles/example-project"),
    )
    .expect("example project profile should load");

    registry
        .validate_integrity()
        .expect("catalog and profile references should be valid");

    assert_eq!(registry.agents().len(), 12);
    assert_eq!(registry.skills().len(), 89);
}
