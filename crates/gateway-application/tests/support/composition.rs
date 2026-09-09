use super::support;
use gateway_application::{
    resolution_agents::*, resolution_applicability::*, resolution_candidates::*,
    resolution_composition::*, resolution_process::*, resolution_skills::*, resolution_snapshot::*,
};
use gateway_domain::*;
use gateway_registry::{CapabilityProvider, Registry};
use std::collections::BTreeMap;

pub fn rules() -> CompositionRules {
    CompositionRules {
        version: SchemaVersion::V1,
        candidates: CandidateRules {
            version: SchemaVersion::V1,
            selectors: BTreeMap::new(),
        },
        processes: ProcessSelectionRules {
            version: SchemaVersion::V1,
            preference: TemplatePreference::None,
            required_definition: None,
            activities: BTreeMap::new(),
            output_evidence: BTreeMap::new(),
            lifecycle_contracts: BTreeMap::new(),
        },
        agents: AgentRules {
            version: SchemaVersion::V1,
            primary: BTreeMap::new(),
            participants: BTreeMap::new(),
            process_roles: BTreeMap::new(),
        },
        skills: BTreeMap::new(),
        applicability: ApplicabilityRules {
            version: SchemaVersion::V1,
            restrictions: BTreeMap::new(),
            semantics: BTreeMap::from([("repository.available".into(), SkillCondition::Always)]),
            completed: BTreeMap::new(),
            activities: BTreeMap::new(),
        },
        provider_priorities: BTreeMap::new(),
        prefer_optional: false,
        max_visits: 10000,
    }
}
pub fn skill_rules() -> SkillRules {
    SkillRules {
        version: SchemaVersion::V1,
        roots: BTreeMap::new(),
        conditions: BTreeMap::new(),
        capability_providers: BTreeMap::new(),
        max_visits: 1000,
    }
}
pub fn skill(id: &str) -> CapabilityProvider {
    CapabilityProvider::Skill {
        skill_id: SkillId::new(id).unwrap(),
    }
}
pub fn agent(id: &str) -> CapabilityProvider {
    CapabilityProvider::Agent {
        agent_id: AgentId::new(id).unwrap(),
    }
}
pub fn run(input: &ResolutionSnapshotInput, rules: &CompositionRules) -> CompositionReport {
    compose_resolution(&ResolutionSnapshot::capture(input).unwrap(), rules).unwrap()
}
pub fn fixture() -> ResolutionSnapshotInput {
    let mut input = support::fixture();
    let template: serde_json::Value = serde_json::from_str(
        &input
            .registry
            .skill(&SkillId::new("architecture-hexagonal").unwrap())
            .unwrap()
            .to_json()
            .unwrap(),
    )
    .unwrap();
    let cap = template["provided_capabilities"][0].clone();
    let mut nested = cap.clone();
    nested["id"] = "nested".into();
    let skills = ["bad", "good", "dependency", "nested-provider"]
        .iter()
        .map(|id| {
            let mut s = template.clone();
            s["id"] = (*id).into();
            s["related_skills"] = serde_json::json!([]);
            s["requires"] = if *id == "bad" {
                serde_json::json!(["dependency"])
            } else {
                serde_json::json!([])
            };
            s["provided_capabilities"] = match *id {
                "dependency" => serde_json::json!([]),
                "nested-provider" => serde_json::json!([nested.clone()]),
                _ => serde_json::json!([cap.clone()]),
            };
            SkillDefinitionDocument::from_json(&s.to_string()).unwrap()
        })
        .collect::<Vec<_>>();
    let agents = ["alpha", "beta"].iter().map(|id| AgentDefinitionDocument::from_json(&serde_json::json!({
        "schema_version":2,"kind":"agent","id":id,"description":"synthetic composition fixture",
        "skill_ids": if *id == "alpha" { vec!["bad", "good", "nested-provider"] } else { vec!["dependency"] },
        "provided_capabilities":[cap.clone(), nested.clone()]
    }).to_string()).unwrap()).collect::<Vec<_>>();
    input.registry = Registry::from_documents(agents, skills).unwrap();
    input.index = input.registry.capability_index().unwrap();
    input
}
pub fn edit_skill(
    input: &mut ResolutionSnapshotInput,
    id: &str,
    field: &str,
    value: serde_json::Value,
) {
    let skills = input
        .registry
        .skills()
        .iter()
        .map(|s| {
            if s.id().as_str() != id {
                return s.clone();
            }
            let mut wire: serde_json::Value = serde_json::from_str(&s.to_json().unwrap()).unwrap();
            wire[field] = value.clone();
            SkillDefinitionDocument::from_json(&wire.to_string()).unwrap()
        })
        .collect::<Vec<_>>();
    input.registry =
        Registry::from_documents(input.registry.agents().documents().to_vec(), skills).unwrap();
    input.index = input.registry.capability_index().unwrap();
}
