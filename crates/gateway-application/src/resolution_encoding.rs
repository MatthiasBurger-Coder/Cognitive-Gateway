//! Explicit v1 semantic encoding, shared by fingerprints and artifact validation.
use crate::{
    resolution::{ContentFingerprint, ResolutionBasis},
    resolution_composition::CompositionRules,
    resolution_skills::{ConditionStatus, SkillCondition, SkillRules},
};
use gateway_domain::PlanCondition;
use gateway_registry::CapabilitySelector;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fmt::Display};

fn pairs<K: Display, V>(map: &BTreeMap<K, V>, value: impl Fn(&V) -> Value) -> Value {
    json!(
        map.iter()
            .map(|(k, v)| json!([k.to_string(), value(v)]))
            .collect::<Vec<_>>()
    )
}
pub(crate) fn basis_json(b: &ResolutionBasis) -> Value {
    json!({"plan":b.plan.as_str(),"plan_fingerprint":b.plan_fingerprint.as_str(),"admission":b.admission.as_ref().map(|a| a.as_str()),
        "situation":b.situation.as_str(),"scope":b.scope.as_str(),"situation_fingerprint":b.situation_fingerprint.as_str(),
        "registry_fingerprint":b.registry_fingerprint.as_str(),"process_catalog_fingerprint":b.process_catalog_fingerprint.as_str(),
        "process_state_fingerprint":b.process_state_fingerprint.as_str(),"rule_version":b.rule_version.to_string()})
}
pub(crate) fn fingerprint(value: &Value) -> String {
    ContentFingerprint::of_bytes(value.to_string().as_bytes())
        .as_str()
        .to_owned()
}
pub(crate) fn status(value: ConditionStatus) -> &'static str {
    match value {
        ConditionStatus::Satisfied => "SATISFIED",
        ConditionStatus::Unsatisfied => "UNSATISFIED",
        ConditionStatus::Unknown => "UNKNOWN",
        ConditionStatus::Conflicted => "CONFLICTED",
        ConditionStatus::Unsupported => "UNSUPPORTED",
    }
}
fn condition(value: &SkillCondition) -> Value {
    match value {
        SkillCondition::Always => json!(["ALWAYS"]),
        SkillCondition::Never => json!(["NEVER"]),
        SkillCondition::Mode(v) => json!(["MODE", v.as_str()]),
        SkillCondition::Profile(v) => json!(["PROFILE", v.as_str()]),
        SkillCondition::ProcessState(v) => json!(["PROCESS_STATE", v.as_str()]),
        SkillCondition::DesiredCondition(v) => json!(["DESIRED_CONDITION", v.as_str()]),
        SkillCondition::Unsupported(v) => json!(["UNSUPPORTED", v.as_str()]),
    }
}
fn contract(value: &PlanCondition) -> Value {
    match value {
        PlanCondition::DesiredCondition(id) => json!(["DESIRED_CONDITION", id.as_str()]),
        PlanCondition::Outcome(o) => {
            json!(["OUTCOME", {"kind":o.kind().as_str(),"subject":o.subject().map(ToString::to_string),
            "expected":o.expected(),"description":o.description()}])
        }
    }
}
pub(crate) fn selector(value: &CapabilitySelector) -> Value {
    match value {
        CapabilitySelector::CapabilityId(v) => json!(["CAPABILITY", v.as_str()]),
        CapabilitySelector::Class(v) => json!(["CLASS", v.as_str()]),
        CapabilitySelector::Domain(v) => json!(["DOMAIN", v.as_str()]),
        CapabilitySelector::InputKind(v) => json!(["INPUT", v.as_str()]),
        CapabilitySelector::OutputKind(v) => json!(["OUTPUT", v.as_str()]),
        CapabilitySelector::Precondition(v) => json!(["PRECONDITION", v.as_str()]),
        CapabilitySelector::Constraint(v) => json!(["CONSTRAINT", v.as_str()]),
        CapabilitySelector::ApplicabilityTag(v) => json!(["TAG", v.as_str()]),
    }
}
fn skills(r: &SkillRules) -> Value {
    json!({"version":r.version.to_string(),"roots":pairs(&r.roots, |v| json!(v.as_str())),
        "conditions":pairs(&r.conditions, condition),"providers":pairs(&r.capability_providers, |v| json!(v.canonical_source())),"max_visits":r.max_visits})
}
pub(crate) fn rules_json(r: &CompositionRules) -> Value {
    json!({"version":r.version.to_string(),"max_visits":r.max_visits,"prefer_optional":r.prefer_optional,
        "priorities":pairs(&r.provider_priorities, |v| json!(v)),
        "candidates":{"version":r.candidates.version.to_string(),"selectors":pairs(&r.candidates.selectors, |v| json!(v.iter().map(selector).collect::<Vec<_>>()))},
        "processes":{"version":r.processes.version.to_string(),"preference":match r.processes.preference {
            crate::resolution_process::TemplatePreference::None => "NONE",crate::resolution_process::TemplatePreference::Optional => "OPTIONAL",crate::resolution_process::TemplatePreference::Required => "REQUIRED"},
            "definition":r.processes.required_definition,"activities":pairs(&r.processes.activities, |v| json!(v)),
            "output_evidence":pairs(&r.processes.output_evidence, |v| json!(v)),"lifecycle":pairs(&r.processes.lifecycle_contracts, |v| json!(v))},
        "agents":{"version":r.agents.version.to_string(),"primary":pairs(&r.agents.primary, |v| json!(v.as_str())),
            "participants":pairs(&r.agents.participants, |v| json!(v.iter().map(|a| a.as_str()).collect::<Vec<_>>())),
            "process_roles":pairs(&r.agents.process_roles, |v| json!({"definition":v.definition,"activity":v.activity}))},
        "skills":pairs(&r.skills, skills),
        "applicability":{"version":r.applicability.version.to_string(),"restrictions":pairs(&r.applicability.restrictions, |v| pairs(v, |conditions| {
            let mut normalized: Vec<_> = conditions.iter().map(condition).collect(); normalized.sort_by_key(Value::to_string); normalized.dedup(); json!(normalized)
        })),"semantics":pairs(&r.applicability.semantics, condition),"activities":pairs(&r.applicability.activities, |v| json!(v)),
            "completed":pairs(&r.applicability.completed, |p| json!({"basis":basis_json(&p.basis),"contracts":p.contracts.iter().map(contract).collect::<Vec<_>>(),
                "references":p.references.iter().map(|v| v.as_str()).collect::<Vec<_>>(),"status":status(p.status),"freshness":p.freshness.as_str()}))}})
}
