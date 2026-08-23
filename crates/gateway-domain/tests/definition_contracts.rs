//! Acceptance tests for the versioned, self-contained Agent and Skill contracts.

use gateway_domain::{
    AgentDefinitionDocument, DefinitionKind, SerializationError, SkillDefinitionDocument,
    ValidationError,
};

const AGENT_FIXTURE: &str = include_str!("../../../schemas/examples/agent-system-architect.json");
const SKILL_FIXTURE: &str =
    include_str!("../../../schemas/examples/skill-architecture-hexagonal.json");

#[test]
fn fixtures_validate_and_preserve_complete_skill_content() {
    let agent = AgentDefinitionDocument::from_json(AGENT_FIXTURE).unwrap();
    assert_eq!(agent.kind(), DefinitionKind::Agent);
    assert_eq!(agent.schema_version().major(), 2);
    assert_eq!(agent.id().as_str(), "system-architect");

    let skill = SkillDefinitionDocument::from_json(SKILL_FIXTURE).unwrap();
    assert_eq!(skill.kind(), DefinitionKind::Skill);
    assert_eq!(skill.id().as_str(), "architecture-hexagonal");
    assert_eq!(skill.name(), "Hexagonal Architecture Expert");
    assert_eq!(skill.authoritative_sources().len(), 2);
    assert_eq!(skill.rules().len(), 2);
    assert_eq!(skill.verification().len(), 1);
    assert_eq!(
        skill.related_skills()[0].as_str(),
        "quality-architecture-validation"
    );
    assert_eq!(
        skill.required_capability_ids()[0].as_str(),
        "repository.read"
    );
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
fn obsolete_origin_and_content_reference_fields_are_rejected() {
    let with_origin = SKILL_FIXTURE.replace(
        "\"knowledge_queries\"",
        "\"origin\": {\"project\": \"old\", \"source\": \"SKILL.md\", \"migration_status\": \"MIGRATED\"}, \"knowledge_queries\"",
    );
    let with_content_ref = SKILL_FIXTURE.replace(
        "\"knowledge_queries\"",
        "\"content_ref\": \"SKILL.md\", \"knowledge_queries\"",
    );

    assert!(SkillDefinitionDocument::from_json(&with_origin).is_err());
    assert!(SkillDefinitionDocument::from_json(&with_content_ref).is_err());
}

#[test]
fn old_version_and_invalid_structured_values_fail_closed() {
    let old_version = SKILL_FIXTURE.replace("\"schema_version\": 2", "\"schema_version\": \"1.0\"");
    assert!(SkillDefinitionDocument::from_json(&old_version).is_err());

    let invalid_name = SKILL_FIXTURE.replace("Hexagonal Architecture Expert", "   ");
    assert!(matches!(
        SkillDefinitionDocument::from_json(&invalid_name),
        Err(SerializationError::Validation(ValidationError::EmptyText {
            field: "name"
        }))
    ));

    let self_reference = SKILL_FIXTURE.replace(
        "\"requires\": []",
        "\"requires\": [\"architecture-hexagonal\"]",
    );
    assert!(matches!(
        SkillDefinitionDocument::from_json(&self_reference),
        Err(SerializationError::Validation(
            ValidationError::SelfReference { .. }
        ))
    ));
}

#[test]
fn required_and_related_references_are_distinct() {
    let related = SKILL_FIXTURE.replace(
        "\"related_skills\": [\"quality-architecture-validation\"]",
        "\"related_skills\": [\"quality-architecture-validation\", \"quality-gate\"]",
    );
    let skill = SkillDefinitionDocument::from_json(&related).unwrap();
    assert!(skill.requires().is_empty());
    assert_eq!(skill.related_skills().len(), 2);

    let overlap = SKILL_FIXTURE
        .replace("\"requires\": []", "\"requires\": [\"quality-gate\"]")
        .replace(
            "\"related_skills\": [\"quality-architecture-validation\"]",
            "\"related_skills\": [\"quality-gate\"]",
        );
    assert!(matches!(
        SkillDefinitionDocument::from_json(&overlap),
        Err(SerializationError::Validation(
            ValidationError::ConflictingRelationship { .. }
        ))
    ));
}
