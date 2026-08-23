use std::path::PathBuf;

use gateway_registry::Registry;

fn repository_catalog() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog")
}

const MIGRATED_GENERIC_SKILLS: &[&str] = &[
    "adr-steward",
    "analysis-storage-architect",
    "arc42-architecture-governance",
    "architecture-hexagonal",
    "audit-evidence-manager",
    "contract-governance-expert",
    "data-ownership-persistence-steward",
    "devops-ci-cd",
    "distributed-systems-architect",
    "documentation-sync",
    "frontend-hexagonal",
    "frontend-ux-guidelines",
    "git-large-repository-specialist",
    "grpc-ingestion",
    "grpc-streaming-specialist",
    "ingestion-handoff-review",
    "isms-light-security-governance-expert",
    "live-evidence-validation-expert",
    "microservice-runtime-readiness-expert",
    "observability-diagnostics",
    "observability-runtime-diagnostics",
    "performance-scalability-engineer",
    "protobuf-contracts",
    "qms-light-governance-expert",
    "quality-architecture-validation",
    "quality-gate",
    "quality-gate-governance",
    "quality-testing-strategy",
    "replay-runtime-correlation-specialist",
    "resilience-engineering",
    "security-sandbox-specialist",
    "security-threat-modeling",
    "service-decomposition-bounded-context",
    "source-analysis-pipeline",
    "supply-chain-security-expert",
    "traceability-engineer",
    "workspace-lifecycle-specialist",
];

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
    assert_eq!(skill_ids, MIGRATED_GENERIC_SKILLS);
}

#[test]
fn migrated_skills_preserve_provenance_and_generic_boundaries() {
    let registry = Registry::load_catalog(repository_catalog()).expect("catalog should load");

    let merged_ids = [
        "architecture-hexagonal",
        "contract-governance-expert",
        "observability-diagnostics",
    ];
    let forbidden_generic_terms = [
        "tiny-swarm-world",
        "docker",
        "swarm",
        "lxd",
        "incus",
        "portainer",
        "linux",
        "wsl",
        "kubernetes",
        "joern",
        "react",
        "java",
        "spring",
        "maven",
        "gradle",
        "jenkins",
        "nexus",
        "sonarqube",
        "nginx",
    ];

    for skill in registry.skills().documents() {
        assert_eq!(skill.origin().project(), "Tiny-Swarm-World");
        assert!(skill.origin().source().contains(".agents/skills/"));
        assert!(
            !skill.knowledge_queries().is_empty(),
            "migrated skill {} lacks a normalized retrieval hint",
            skill.id()
        );

        let searchable = format!(
            "{} {}",
            skill.description().to_ascii_lowercase(),
            skill
                .knowledge_queries()
                .iter()
                .map(|query| query.as_str().to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(" ")
        );
        assert!(
            !forbidden_generic_terms
                .iter()
                .any(|term| searchable.contains(term)),
            "TSW-specific term leaked into generic skill {}",
            skill.id()
        );

        if merged_ids.contains(&skill.id().as_str()) {
            assert_eq!(
                skill.origin().migration_status(),
                gateway_domain::MigrationStatus::Merged
            );
            assert!(skill.origin().source().contains("; "));
        } else {
            assert_eq!(
                skill.origin().migration_status(),
                gateway_domain::MigrationStatus::Migrated
            );
            assert!(!skill.origin().source().contains("; "));
        }
    }
}
