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
fn migrated_skills_preserve_generic_boundaries_and_structured_shape() {
    let registry = Registry::load_catalog(repository_catalog()).expect("catalog should load");

    let forbidden_generic_terms = [
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
        assert_eq!(skill.schema_version().major(), 2);
        assert!(!skill.name().is_empty());
        assert!(
            skill
                .authoritative_sources()
                .iter()
                .all(|value| !value.as_str().is_empty())
        );
        assert!(skill.rules().iter().all(|value| !value.as_str().is_empty()));
        assert!(
            skill
                .verification()
                .iter()
                .all(|value| !value.as_str().is_empty())
        );
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
            "project-specific term leaked into generic skill {}",
            skill.id()
        );
    }
}

#[test]
fn catalog_skills_are_complete_and_references_are_canonical() {
    let registry = Registry::load_catalog(repository_catalog()).expect("catalog should load");

    let skills = registry.skills();
    assert_eq!(skills.documents().len(), MIGRATED_GENERIC_SKILLS.len());

    for skill in skills.documents() {
        assert!(
            !skill.authoritative_sources().is_empty(),
            "skill {} lacks authoritative source selectors",
            skill.id()
        );
        assert!(
            !skill.rules().is_empty(),
            "skill {} lacks rules",
            skill.id()
        );
        assert!(
            !skill.verification().is_empty(),
            "skill {} lacks verification guidance",
            skill.id()
        );

        for dependency in skill.requires() {
            assert!(
                skills.get(dependency).is_some(),
                "skill {} has an unresolved required reference {}",
                skill.id(),
                dependency
            );
            assert!(!skill.related_skills().contains(dependency));
        }
        for related in skill.related_skills() {
            assert!(
                skills.get(related).is_some(),
                "skill {} has an unresolved related reference {}",
                skill.id(),
                related
            );
        }
    }

    let resilience = gateway_domain::SkillId::new("resilience-engineering").unwrap();
    let resilience_dependents: Vec<_> = skills
        .documents()
        .iter()
        .filter(|skill| skill.requires().contains(&resilience))
        .map(|skill| skill.id().as_str())
        .collect();
    assert_eq!(
        resilience_dependents,
        vec![
            "analysis-storage-architect",
            "devops-ci-cd",
            "distributed-systems-architect",
            "git-large-repository-specialist",
            "grpc-ingestion",
            "grpc-streaming-specialist",
            "observability-diagnostics",
            "performance-scalability-engineer",
            "quality-gate-governance",
            "source-analysis-pipeline",
            "workspace-lifecycle-specialist",
        ]
    );
}

#[test]
fn complete_generic_skill_graph_resolves_without_a_consuming_project() {
    let registry = Registry::load_catalog(repository_catalog()).expect("catalog should load");
    let root = gateway_domain::SkillId::new("source-analysis-pipeline").unwrap();

    let graph = registry
        .resolve_skill(&root)
        .expect("a generic Skill must resolve without a project profile");

    assert_eq!(graph.root(), &root);
    assert_eq!(
        graph.ids().map(ToString::to_string).collect::<Vec<_>>(),
        vec!["resilience-engineering", "source-analysis-pipeline"]
    );
    assert_eq!(graph.len(), 2);
    assert_eq!(graph.skills()[1].id(), &root);
}
