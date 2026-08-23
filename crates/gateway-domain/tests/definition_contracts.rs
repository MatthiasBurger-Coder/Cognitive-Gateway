//! Acceptance tests for the versioned repository Agent and Skill contracts.

use gateway_domain::{
    AgentDefinitionDocument, DefinitionKind, MigrationStatus, SerializationError,
    SkillDefinitionDocument, ValidationError,
};

const AGENT_FIXTURE: &str = include_str!("../../../schemas/examples/agent-system-architect.json");
const SKILL_FIXTURE: &str =
    include_str!("../../../schemas/examples/skill-architecture-hexagonal.json");

#[test]
fn normalized_tsw_fixtures_validate_and_preserve_provenance() {
    let agent = AgentDefinitionDocument::from_json(AGENT_FIXTURE).unwrap();
    assert_eq!(agent.kind(), DefinitionKind::Agent);
    assert_eq!(agent.id().as_str(), "system-architect");
    assert_eq!(agent.origin().project(), "Tiny-Swarm-World");
    assert_eq!(
        agent.origin().source(),
        ".agents/roles/senior-system-architect.md"
    );
    assert_eq!(agent.origin().migration_status(), MigrationStatus::Migrated);

    let skill = SkillDefinitionDocument::from_json(SKILL_FIXTURE).unwrap();
    assert_eq!(skill.kind(), DefinitionKind::Skill);
    assert_eq!(skill.id().as_str(), "architecture-hexagonal");
    assert_eq!(skill.owner_agent_id().unwrap().as_str(), "system-architect");
    assert_eq!(
        skill.required_capability_ids()[0].as_str(),
        "repository.read"
    );
    assert_eq!(skill.origin().migration_status(), MigrationStatus::Merged);
}

#[test]
fn fixtures_round_trip_to_canonical_json() {
    let agent = AgentDefinitionDocument::from_json(AGENT_FIXTURE).unwrap();
    let skill = SkillDefinitionDocument::from_json(SKILL_FIXTURE).unwrap();

    assert_eq!(
        AgentDefinitionDocument::from_json(&agent.to_json().unwrap()).unwrap(),
        agent
    );
    assert_eq!(
        SkillDefinitionDocument::from_json(&skill.to_json().unwrap()).unwrap(),
        skill
    );
}

#[test]
fn malformed_documents_fail_without_coercion_or_runtime_leakage() {
    let malformed = [
        AGENT_FIXTURE.replace("\"1.0\"", "1"),
        AGENT_FIXTURE.replace("system-architect", "../system-architect"),
        AGENT_FIXTURE.replace("\"origin\"", "\"model\":\"provider-model\",\"origin\""),
        SKILL_FIXTURE.replace("\"MERGED\"", "\"UNKNOWN\""),
    ];

    assert!(
        malformed
            .iter()
            .all(|value| AgentDefinitionDocument::from_json(value).is_err())
    );
    assert!(matches!(
        SkillDefinitionDocument::from_json(&malformed[3]),
        Err(SerializationError::Validation(
            ValidationError::UnknownDomainValue {
                field: "migration_status",
                ..
            }
        ))
    ));

    let invalid_skill = SKILL_FIXTURE.replace(
        "\"dependency_ids\": []",
        "\"dependency_ids\": [\"architecture-hexagonal\"]",
    );
    assert!(matches!(
        SkillDefinitionDocument::from_json(&invalid_skill),
        Err(SerializationError::Validation(
            ValidationError::SelfReference { .. }
        ))
    ));
}
