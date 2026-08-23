use std::path::PathBuf;

use gateway_registry::Registry;

fn repository_profile() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../profiles/tiny-swarm-world")
}

const MIGRATED_TSW_AGENTS: &[&str] = &[
    "devops",
    "python-automation-developer",
    "react-frontend",
    "ux-designer",
];

const MIGRATED_TSW_SKILLS: &[&str] = &[
    "acceptance-checks",
    "analytics-persistence-review",
    "architecture-archunit-hexagonal",
    "architecture-modular-monorepo",
    "bdd-expert",
    "build-gradle",
    "console-status-ui-developer",
    "devops-docker",
    "devops-kubernetes",
    "docker-engine-installation",
    "docker-registry-bootstrap",
    "docker-swarm-initialization",
    "documentation-audience-architect",
    "documentation-generation",
    "frontend-developer",
    "frontend-react",
    "idempotent-platform-automation",
    "image-build-publish",
    "image-verification",
    "image-versioning-tagging",
    "jenkins-bootstrap",
    "kubernetes-expert",
    "linux-host-preparation",
    "llm-analysis-expert",
    "mapping-dsl-expert",
    "network-topology-design",
    "nexus-bootstrap",
    "owasp-asvs-local-infrastructure-expert",
    "platform-layout-governance",
    "platform-quality-gates",
    "platform-reset-and-recovery",
    "platform-verification",
    "portainer-bootstrap",
    "python-automation",
    "python-cli-automation",
    "python-pip-packaging-expert",
    "python-test-automation",
    "quality-archunit-review",
    "registry-infrastructure",
    "reverse-proxy-routing",
    "secrets-and-config-management",
    "setup-bootstrap-expert",
    "sonarqube-bootstrap",
    "strangler-command-adapter-pattern",
    "swagger-ui-bootstrap",
    "swarm-node-management",
    "swarm-stack-deployment",
    "swarm-volume-network-governance",
    "tdd-expert",
    "terminal-status-dashboard",
    "testing-junit6",
    "tiny-swarm-world-system-architecture",
];

const SCOPE_GATED_SKILLS: &[&str] = &[
    "code-property-graph-joern-specialist",
    "joern-semantic-analysis",
    "replay-graph-llm-review",
    "sca-migration-expert",
];

#[test]
fn migrated_tsw_profile_loads() {
    let registry = Registry::load_profile(repository_profile()).expect("profile should load");

    let agent_ids: Vec<_> = registry.agents().ids().map(ToString::to_string).collect();
    assert_eq!(agent_ids, MIGRATED_TSW_AGENTS);

    let skill_ids: Vec<_> = registry.skills().ids().map(ToString::to_string).collect();
    assert_eq!(skill_ids, MIGRATED_TSW_SKILLS);
}

#[test]
fn profile_definitions_preserve_scope_and_structured_content() {
    let registry = Registry::load_profile(repository_profile()).expect("profile should load");

    for agent in registry.agents().documents() {
        assert_eq!(agent.schema_version().major(), 2);
    }

    for skill in registry.skills().documents() {
        assert_eq!(skill.schema_version().major(), 2);
        assert!(!skill.name().is_empty());
        assert!(skill.owner_agent_id().is_none());
        assert!(skill.dependency_ids().is_empty());
        assert!(skill.required_capability_ids().is_empty());
        assert_eq!(skill.knowledge_queries().len(), 1);

        assert!(
            skill
                .authoritative_sources()
                .iter()
                .all(|value| !value.as_str().is_empty())
        );
    }

    for scope_gated_id in SCOPE_GATED_SKILLS {
        assert!(
            !registry
                .skills()
                .ids()
                .any(|skill_id| skill_id.as_str() == *scope_gated_id),
            "scope-gated skill {scope_gated_id} must remain unmaterialized"
        );
    }
}

#[test]
fn combined_catalog_and_profile_has_no_boundary_collisions() {
    let catalog = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
    let registry = Registry::load_catalog_with_profile(catalog, repository_profile())
        .expect("combined registry should load");

    registry
        .validate_integrity()
        .expect("combined references should be valid");
    assert_eq!(registry.agents().len(), 12);
    assert_eq!(registry.skills().len(), 89);
}
