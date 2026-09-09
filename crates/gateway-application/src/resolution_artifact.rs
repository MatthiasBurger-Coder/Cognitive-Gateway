//! Strict canonical artifacts, independently replayed against explicit sources.
use crate::{
    resolution::{ProcessBinding, StepBinding},
    resolution_composition::{
        BindingAlternative, CompositionDiagnostic, CompositionReport, CompositionRules,
        compose_resolution,
    },
    resolution_encoding::{basis_json, fingerprint, rules_json, selector},
    resolution_explain::{
        TraceLimits, diagnostic, explain_resolution, process_code, process_rejection,
    },
    resolution_skills::{EffectiveSkills, SkillNode},
    resolution_snapshot::ResolutionSnapshot,
};
use serde::{
    Deserialize, Deserializer,
    de::{Error as _, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactLimits {
    pub max_bytes: usize,
    pub max_nodes: usize,
    pub max_depth: usize,
}
impl Default for ArtifactLimits {
    fn default() -> Self {
        Self {
            max_bytes: 4 * 1024 * 1024,
            max_nodes: 200_000,
            max_depth: 64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactError {
    InvalidLimits,
    SizeLimit,
    GraphLimit,
    MalformedJson,
    UnsupportedVersion,
    UnknownField,
    StaleBasis,
    RuleMismatch,
    FingerprintMismatch,
    InvalidArtifact,
}

fn limits_valid(l: ArtifactLimits) -> Result<(), ArtifactError> {
    if l.max_bytes == 0
        || l.max_bytes > 4 * 1024 * 1024
        || l.max_nodes == 0
        || l.max_nodes > 200_000
        || l.max_depth == 0
        || l.max_depth > 64
    {
        Err(ArtifactError::InvalidLimits)
    } else {
        Ok(())
    }
}
fn check_tree(value: &Value, l: ArtifactLimits) -> Result<(), ArtifactError> {
    let mut stack = vec![(value, 1)];
    let mut nodes = 0;
    while let Some((value, depth)) = stack.pop() {
        nodes += 1;
        if nodes > l.max_nodes || depth > l.max_depth {
            return Err(ArtifactError::GraphLimit);
        }
        match value {
            Value::Array(a) => stack.extend(a.iter().map(|v| (v, depth + 1))),
            Value::Object(o) => stack.extend(o.values().map(|v| (v, depth + 1))),
            _ => {}
        }
    }
    Ok(())
}
fn check_sources(
    snapshot: &ResolutionSnapshot,
    report: &CompositionReport,
    l: ArtifactLimits,
) -> Result<(), ArtifactError> {
    limits_valid(l)?;
    if snapshot.request().plan().steps().len() > 1024
        || snapshot.request().plan().capability_requirements().len() > 4096
        || snapshot.input().registry.agents().len() + snapshot.input().registry.skills().len()
            > 4096
        || snapshot.input().processes.definitions().count() > 4096
    {
        return Err(ArtifactError::GraphLimit);
    }
    let mut weight = 0;
    for a in report
        .steps
        .iter()
        .flat_map(|s| &s.alternatives)
        .chain(report.alternatives.iter().flatten())
    {
        weight += 1 + a.chosen.len();
        if let Some(s) = &a.skills {
            weight += s.skills.len()
                + s.inclusion_paths
                    .values()
                    .flatten()
                    .map(Vec::len)
                    .sum::<usize>();
        }
        if weight > l.max_nodes {
            return Err(ArtifactError::GraphLimit);
        }
    }
    Ok(())
}

pub fn validate_resolution(
    snapshot: &ResolutionSnapshot,
    report: &CompositionReport,
    limits: ArtifactLimits,
) -> Result<(), ArtifactError> {
    check_sources(snapshot, report, limits)?;
    if report.basis != *snapshot.request().basis() {
        return Err(ArtifactError::StaleBasis);
    }
    if compose_resolution(snapshot, &report.rules).map_err(|_| ArtifactError::InvalidArtifact)?
        != *report
    {
        return Err(ArtifactError::InvalidArtifact);
    }
    Ok(())
}
pub fn serialize_resolution(
    snapshot: &ResolutionSnapshot,
    report: &CompositionReport,
    limits: ArtifactLimits,
) -> Result<String, ArtifactError> {
    let value = encode(snapshot, report, limits)?;
    let text = value.to_string();
    if text.len() > limits.max_bytes {
        return Err(ArtifactError::SizeLimit);
    }
    Ok(text)
}
pub fn parse_resolution(
    snapshot: &ResolutionSnapshot,
    rules: &CompositionRules,
    text: &str,
    limits: ArtifactLimits,
) -> Result<CompositionReport, ArtifactError> {
    limits_valid(limits)?;
    if text.len() > limits.max_bytes {
        return Err(ArtifactError::SizeLimit);
    }
    let mut value: Value = serde_json::from_str::<StrictValue>(text)
        .map_err(|_| ArtifactError::MalformedJson)?
        .0;
    check_tree(&value, limits)?;
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(ArtifactError::UnsupportedVersion);
    }
    if value.get("basis") != Some(&basis_json(snapshot.request().basis())) {
        return Err(ArtifactError::StaleBasis);
    }
    if value.get("rule_fingerprint").and_then(Value::as_str)
        != Some(fingerprint(&rules_json(rules)).as_str())
    {
        return Err(ArtifactError::RuleMismatch);
    }
    normalize(&mut value, "");
    let claimed = value
        .as_object_mut()
        .and_then(|o| o.remove("artifact_fingerprint"))
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or(ArtifactError::FingerprintMismatch)?;
    if fingerprint(&value) != claimed {
        return Err(ArtifactError::FingerprintMismatch);
    }
    value["artifact_fingerprint"] = json!(claimed);
    let report = compose_resolution(snapshot, rules).map_err(|_| ArtifactError::InvalidArtifact)?;
    let expected = encode(snapshot, &report, limits)?;
    reject_unknown(&expected, &value)?;
    if value != expected {
        return Err(ArtifactError::InvalidArtifact);
    }
    Ok(report)
}
fn reject_unknown(expected: &Value, actual: &Value) -> Result<(), ArtifactError> {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            for (k, v) in a {
                let original = e.get(k).ok_or(ArtifactError::UnknownField)?;
                reject_unknown(original, v)?;
            }
        }
        (Value::Array(e), Value::Array(a)) => {
            for (e, a) in e.iter().zip(a) {
                reject_unknown(e, a)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn encode(
    snapshot: &ResolutionSnapshot,
    report: &CompositionReport,
    limits: ArtifactLimits,
) -> Result<Value, ArtifactError> {
    check_sources(snapshot, report, limits)?;
    let trace = explain_resolution(
        snapshot,
        report,
        TraceLimits {
            max_nodes: 10_000,
            max_optional_details: 0,
        },
    )
    .map_err(|e| match e {
        crate::resolution_explain::TraceError::StaleBasis => ArtifactError::StaleBasis,
        crate::resolution_explain::TraceError::RequiredTraceLimit => ArtifactError::GraphLimit,
        _ => ArtifactError::InvalidArtifact,
    })?;
    let mut value = json!({"version":1,"basis":basis_json(&report.basis),"rule_fingerprint":fingerprint(&rules_json(&report.rules)),"plan":snapshot.request().plan(),"trace":trace,
        "requirement_alternatives":snapshot.request().alternatives().iter().map(|g|json!({"step":g.step.as_str(),"members":g.members.iter().map(|m|m.as_str()).collect::<Vec<_>>(),"cardinality":g.cardinality.as_str()})).collect::<Vec<_>>(),
        "report":{"outcome":report.outcome.as_str(),"visits":report.visits,"exhausted":report.exhausted,
            "processes":{"outcome":process_code(report.processes.outcome),"candidates":report.processes.candidates.iter().map(|p|json!({"binding":process(&p.binding),
                "activities":p.activities.iter().map(|(s,a)|(s.to_string(),json!(a))).collect::<BTreeMap<_,_>>()})).collect::<Vec<_>>(),
                "rejections":report.processes.rejections.iter().map(|r|json!({"definition":r.definition,"step":r.step.as_ref().map(|s|s.as_str()),"reason":process_rejection(r.reason)})).collect::<Vec<_>>()},
            "discovery":report.discovery.sets.iter().map(|s|json!({"step":s.step.as_str(),"requirement":s.requirement.as_str(),"cardinality":s.cardinality.as_str(),
                "candidates":s.candidates.iter().map(|c|json!({"provider":c.canonical.provider().canonical_source(),"contract":c.canonical.capability(),"definition_fingerprint":c.definition_fingerprint.as_str(),
                    "matched_selectors":c.canonical.matched_selectors().iter().map(selector).collect::<Vec<_>>() })).collect::<Vec<_>>(),
                "rejections":s.rejections.iter().map(|r|json!({"provider":r.provider().canonical_source(),"contract":r.capability(),"failed_selectors":r.failed_selectors().map(selector).collect::<Vec<_>>() })).collect::<Vec<_>>() })).collect::<Vec<_>>(),
            "steps":report.steps.iter().map(|s|json!({"step":s.step.as_str(),"outcome":s.outcome.as_str(),"alternatives":s.alternatives.iter().map(alternative).collect::<Vec<_>>(),
                "diagnostics":s.diagnostics.iter().map(reason).collect::<Vec<_>>(),"rejections":s.rejections.iter().map(|r|json!({"chosen":chosen(&r.chosen),"process":r.process.as_ref().map(process),"activity":r.activity,
                    "skills":r.skills.as_ref().map(closure),"reasons":r.reasons.iter().map(reason).collect::<Vec<_>>() })).collect::<Vec<_>>() })).collect::<Vec<_>>(),
            "alternatives":report.alternatives.iter().map(|row|row.iter().map(alternative).collect::<Vec<_>>()).collect::<Vec<_>>()}});
    normalize(&mut value, "");
    value["artifact_fingerprint"] = json!(fingerprint(&value));
    check_tree(&value, limits)?;
    Ok(value)
}
fn process(p: &ProcessBinding) -> Value {
    json!({"definition":p.definition,"instance":p.instance})
}
fn binding(b: &StepBinding) -> Value {
    json!({"process":b.process.as_ref().map(process),"primary_agent":b.primary_agent.as_str(),
    "participating_agents":b.participating_agents.iter().map(|a|a.as_str()).collect::<Vec<_>>(),"skills":b.skills.iter().map(|(s,a)|(s.to_string(),a.as_str())).collect::<BTreeMap<_,_>>()})
}
fn chosen(
    c: &BTreeMap<gateway_domain::CapabilityRequirementId, gateway_registry::CapabilityProvider>,
) -> Value {
    json!(
        c.iter()
            .map(|(r, p)| (r.to_string(), p.canonical_source()))
            .collect::<BTreeMap<_, _>>()
    )
}
fn reason(d: &CompositionDiagnostic) -> Value {
    let (code, kind, reference, attributes) = diagnostic(d);
    json!({"code":code,"kind":kind,"reference":reference,"attributes":attributes})
}
fn closure(s: &EffectiveSkills) -> Value {
    json!({"complete":s.complete,"skills":s.skills.iter().map(|s|s.as_str()).collect::<Vec<_>>(),
    "inclusion_paths":s.inclusion_paths.iter().map(|(s,paths)|json!({"skill":s.as_str(),"paths":paths.iter().map(|p|p.iter().map(|n|match n {
        SkillNode::Skill(s)=>json!(["SKILL",s.as_str()]),SkillNode::Capability(c)=>json!(["CAPABILITY",c.as_str()])}).collect::<Vec<_>>()).collect::<Vec<_>>() })).collect::<Vec<_>>(),
    "required_capabilities":s.required_capabilities.iter().map(|(id,class)|json!({"capability":id.as_str(),"class":class.map(|c|c.as_str()),"provider":s.rules.capability_providers.get(id).map(|p|p.canonical_source())})).collect::<Vec<_>>(),
    "diagnostics":s.diagnostics.iter().map(|d|reason(&CompositionDiagnostic::Skill(d.clone()))).collect::<Vec<_>>()})
}
fn alternative(a: &BindingAlternative) -> Value {
    json!({"step":a.step.as_str(),"binding":a.binding.as_ref().map(binding),"activity":a.activity,"chosen":chosen(&a.chosen),"skills":a.skills.as_ref().map(closure),
    "readiness":a.applicability.readiness.as_str(),"score":a.score,"reasons":a.applicability.reasons.iter().map(|r|reason(&CompositionDiagnostic::Applicability(r.clone()))).collect::<Vec<_>>(),
    "diagnostics":a.diagnostics.iter().map(reason).collect::<Vec<_>>()})
}

/// Only declared sets are sorted. Dependency-first Skill arrays and path-node
/// arrays, score tuples, and process history stay semantically ordered.
fn normalize(value: &mut Value, field: &str) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                normalize(value, key);
            }
        }
        Value::Array(values) => {
            for value in values.iter_mut() {
                normalize(value, "");
            }
            if matches!(
                field,
                "steps"
                    | "alternatives"
                    | "candidates"
                    | "rejections"
                    | "diagnostics"
                    | "reasons"
                    | "nodes"
                    | "edges"
                    | "paths"
                    | "inclusion_paths"
                    | "required_capabilities"
                    | "discovery"
                    | "participating_agents"
                    | "matched_selectors"
                    | "failed_selectors"
                    | "dependencies"
                    | "capability_requirements"
                    | "requirement_alternatives"
                    | "members"
            ) {
                values.sort_by_key(Value::to_string);
            }
        }
        _ => {}
    }
}

struct StrictValue(Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StrictVisitor;
        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut result = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, StrictValue>()? {
                    if result.insert(key, value.0).is_some() {
                        return Err(M::Error::custom("duplicate object key"));
                    }
                }
                Ok(StrictValue(Value::Object(result)))
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                let mut result = vec![];
                while let Some(v) = seq.next_element::<StrictValue>()? {
                    result.push(v.0);
                }
                Ok(StrictValue(Value::Array(result)))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
        }
        deserializer.deserialize_any(StrictVisitor)
    }
}
