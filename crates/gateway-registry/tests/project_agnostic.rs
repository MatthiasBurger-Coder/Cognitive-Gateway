use std::path::PathBuf;

use gateway_domain::{AgentId, SkillId};
use gateway_registry::Registry;

fn repository_catalog() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog")
}

#[test]
fn analysis_storage_agent_and_skill_resolve_from_the_catalog_alone() {
    let catalog = Registry::load_catalog(repository_catalog())
        .expect("the generic catalog must load without a consuming project");
    catalog
        .validate_integrity()
        .expect("the generic Agent/Skill graph must validate without external context");

    let agent_id = AgentId::new("analysis-storage-architect").unwrap();
    let skill_id = SkillId::new("analysis-storage-architect").unwrap();
    let resilience_id = SkillId::new("resilience-engineering").unwrap();

    let agent = catalog
        .agent(&agent_id)
        .expect("the storage specialist must be a catalog Agent");
    assert_eq!(
        agent
            .skill_ids()
            .iter()
            .map(SkillId::as_str)
            .collect::<Vec<_>>(),
        [
            "analysis-storage-architect",
            "architecture-hexagonal",
            "quality-testing-strategy"
        ]
    );
    assert!(
        agent
            .to_json()
            .unwrap()
            .contains("analysis-storage-architect")
    );

    let skill = catalog
        .skill(&skill_id)
        .expect("the storage specialist must be a catalog Skill");
    assert!(skill.owner_agent_id().is_none());
    assert_eq!(skill.requires(), std::slice::from_ref(&resilience_id));
    assert!(!skill.authoritative_sources().is_empty());
    assert!(!skill.rules().is_empty());
    assert!(!skill.verification().is_empty());

    let graph = catalog
        .resolve_skill(&skill_id)
        .expect("the catalog Skill must resolve without external context");
    assert_eq!(graph.root(), &skill_id);
    assert_eq!(
        graph.ids().map(SkillId::as_str).collect::<Vec<_>>(),
        ["resilience-engineering", "analysis-storage-architect"]
    );
}

#[test]
fn catalog_and_explicit_boundaries_produce_the_same_project_independent_registry() {
    let catalog = repository_catalog();
    let conventional = Registry::load_catalog(&catalog).expect("catalog should load");
    let explicit = Registry::load_from_directories(catalog.join("agents"), catalog.join("skills"))
        .expect("explicit catalog boundaries should load");

    assert_eq!(conventional, explicit);
    assert!(
        conventional
            .agent(&AgentId::new("analysis-storage-architect").unwrap())
            .is_some()
    );
    assert!(
        conventional
            .skill(&SkillId::new("analysis-storage-architect").unwrap())
            .is_some()
    );
}
