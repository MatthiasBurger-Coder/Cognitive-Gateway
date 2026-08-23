use std::path::PathBuf;

use gateway_registry::Registry;

fn repository_catalog() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog")
}

const CATALOG_SKILLS: &[&str] = &[
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
fn catalog_loads_and_validates() {
    let registry = Registry::load_catalog(repository_catalog()).expect("catalog should load");

    registry
        .validate_integrity()
        .expect("catalog references should be valid");

    let agent_ids: Vec<_> = registry.agents().ids().map(ToString::to_string).collect();
    assert_eq!(
        agent_ids,
        vec![
            "analysis-storage-architect",
            "devops",
            "git-workspace-specialist",
            "grpc-proto-specialist",
            "performance-engineer",
            "plugin-integration-developer",
            "python-automation-developer",
            "react-frontend",
            "security-sandbox-engineer",
            "system-architect",
            "tester",
            "ux-designer",
        ]
    );

    let skill_ids: Vec<_> = registry.skills().ids().map(ToString::to_string).collect();
    assert_eq!(skill_ids, CATALOG_SKILLS);
}

#[test]
fn catalog_agents_reference_catalog_skills() {
    let registry = Registry::load_catalog(repository_catalog()).expect("catalog should load");

    for agent in registry.agents().documents() {
        let searchable = format!(
            "{} {}",
            agent.id().as_str().to_ascii_lowercase(),
            agent.description().to_ascii_lowercase()
        );
        for forbidden in [
            "project-specific",
            "external-context",
            "consumer/path",
            "origin",
            "migration",
        ] {
            assert!(
                !searchable.contains(forbidden),
                "external Agent content leaked into {}",
                agent.id()
            );
        }
        assert!(!agent.skill_ids().is_empty());
        for skill_id in agent.skill_ids() {
            assert!(
                registry.skill(skill_id).is_some(),
                "catalog Agent {} references a non-catalog Skill {}",
                agent.id(),
                skill_id
            );
        }
    }

    registry
        .validate_integrity()
        .expect("catalog Agents must validate without external context");
}

#[test]
fn catalog_skills_preserve_generic_boundaries_and_structured_shape() {
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
            "catalog skill {} lacks a normalized retrieval hint",
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
            "external term leaked into generic skill {}",
            skill.id()
        );
    }
}

#[test]
fn catalog_skills_are_complete_and_references_are_canonical() {
    let registry = Registry::load_catalog(repository_catalog()).expect("catalog should load");

    let skills = registry.skills();
    assert_eq!(skills.documents().len(), CATALOG_SKILLS.len());

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
fn catalog_skill_graph_resolves_deterministically() {
    let registry = Registry::load_catalog(repository_catalog()).expect("catalog should load");
    let root = gateway_domain::SkillId::new("source-analysis-pipeline").unwrap();

    let graph = registry
        .resolve_skill(&root)
        .expect("a catalog Skill must resolve without external context");

    assert_eq!(graph.root(), &root);
    assert_eq!(
        graph.ids().map(ToString::to_string).collect::<Vec<_>>(),
        vec!["resilience-engineering", "source-analysis-pipeline"]
    );
    assert_eq!(graph.len(), 2);
    assert_eq!(graph.skills()[1].id(), &root);
}

#[test]
fn catalog_capability_contracts_are_typed_and_project_independent() {
    let registry = Registry::load_catalog(repository_catalog()).expect("catalog should load");
    registry
        .validate_integrity()
        .expect("capability declarations should be consistent");

    let agent_capabilities: Vec<_> = registry
        .agents()
        .documents()
        .iter()
        .flat_map(|agent| agent.provided_capabilities())
        .collect();
    let skill_capabilities: Vec<_> = registry
        .skills()
        .documents()
        .iter()
        .flat_map(|skill| skill.provided_capabilities())
        .collect();

    assert!(!agent_capabilities.is_empty());
    assert!(!skill_capabilities.is_empty());
    assert!(
        skill_capabilities
            .iter()
            .any(|capability| capability.id().as_str() == "architecture.dependency-analysis")
    );

    for capability in agent_capabilities.into_iter().chain(skill_capabilities) {
        assert!(!capability.domain().as_str().is_empty());
        assert!(!capability.description().trim().is_empty());
        assert!(
            capability
                .input_kinds()
                .iter()
                .all(|kind| !kind.as_str().contains('/'))
        );
        assert!(
            capability
                .output_kinds()
                .iter()
                .all(|kind| !kind.as_str().contains('/'))
        );
        let searchable = format!(
            "{} {} {}",
            capability.id(),
            capability.domain(),
            capability.description()
        )
        .to_ascii_lowercase();
        for forbidden in ["project", "repository path", "workflow state"] {
            assert!(
                !searchable.contains(forbidden),
                "project-specific capability content leaked into {}",
                capability.id()
            );
        }
    }
}
