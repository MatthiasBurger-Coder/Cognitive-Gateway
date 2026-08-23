use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use gateway_registry::Registry;

fn repository_root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../{name}"))
}

fn temporary_profile() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("cognitive-gateway-neutral-profile-{nonce}"))
}

#[test]
fn an_external_project_fixture_does_not_change_a_generic_skill_graph() {
    let profile = temporary_profile();
    let profile_skill = profile.join("skills/external-project-only.json");
    fs::create_dir_all(profile_skill.parent().expect("skill has a parent"))
        .expect("neutral profile directory should be created");
    fs::create_dir_all(profile.join("agents")).expect("neutral agent boundary should be created");
    fs::write(
        &profile_skill,
        r#"{"schema_version":2,"kind":"skill","id":"external-project-only","name":"External project context","description":"Neutral external context used only as an input fixture","owner_agent_id":null,"authoritative_sources":["external context fixture"],"rules":["Do not alter generic catalog membership."],"verification":["Compare the resolved graph with the standalone result."],"requires":[],"related_skills":[],"required_capability_ids":[],"knowledge_queries":[]}"#,
    )
    .expect("neutral profile Skill should be written");

    let catalog = repository_root("catalog");
    let root = gateway_domain::SkillId::new("source-analysis-pipeline").unwrap();
    let standalone = Registry::load_catalog(&catalog)
        .expect("catalog should load without project context")
        .resolve_skill(&root)
        .expect("generic Skill should resolve without project context");
    let with_external_context = Registry::load_catalog_with_profile(&catalog, &profile)
        .expect("independent external fixture should be loadable")
        .resolve_skill(&root)
        .expect("generic Skill should resolve with independent context present");

    assert_eq!(standalone, with_external_context);
    assert_eq!(standalone.len(), 2);

    fs::remove_dir_all(profile).expect("neutral profile should be cleaned up");
}
