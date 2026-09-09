//! One bounded semantic graph for human and machine explanations.
use crate::{
    resolution_applicability::ApplicabilityReason,
    resolution_composition::{CompositionDiagnostic, CompositionReport, compose_resolution},
    resolution_encoding::{basis_json, fingerprint, rules_json, selector, status},
    resolution_skills::{EffectiveSkills, SkillDiagnostic, SkillNode},
    resolution_snapshot::ResolutionSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
#[path = "../tests/support/explain_cases.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceError {
    InvalidLimit,
    StaleBasis,
    InvalidArtifact,
    RequiredTraceLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceLimits {
    pub max_nodes: usize,
    pub max_optional_details: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceKind {
    DesiredState,
    Situation,
    Delta,
    Plan,
    PlanStep,
    Requirement,
    Capability,
    Agent,
    Skill,
    Process,
    Rule,
    Binding,
    Constraint,
    Evidence,
    Diagnostic,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceSource {
    pub kind: SourceKind,
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceNode {
    pub id: String,
    pub source: TraceSource,
    pub code: String,
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceEdge {
    pub from: String,
    pub to: String,
    pub relation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionTrace {
    pub version: u16,
    pub basis: Value,
    pub rule_fingerprint: String,
    pub outcome: String,
    pub policy_authorization: String,
    pub search_complete: bool,
    pub nodes: Vec<TraceNode>,
    pub edges: Vec<TraceEdge>,
    pub omitted_optional_details: usize,
}
impl ResolutionTrace {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("trace contains only JSON values")
    }
    pub fn to_text(&self) -> String {
        let mut lines = vec![format!(
            "resolution={} policy_authorization={} search_complete={} omitted_optional_details={}",
            self.outcome,
            self.policy_authorization,
            self.search_complete,
            self.omitted_optional_details
        )];
        for node in &self.nodes {
            lines.push(format!(
                "{} {} {} {}",
                node.id,
                node.code,
                serde_json::to_string(&node.source).expect("trace source"),
                json!(node.attributes)
            ));
        }
        for edge in &self.edges {
            lines.push(format!("{} --{}--> {}", edge.from, edge.relation, edge.to));
        }
        lines.join("\n")
    }
}

struct Graph {
    nodes: BTreeMap<String, TraceNode>,
    edges: BTreeSet<TraceEdge>,
    limits: TraceLimits,
    optional: Vec<(String, String)>,
    omitted: usize,
}
impl Graph {
    fn node(
        &mut self,
        kind: SourceKind,
        reference: impl Into<String>,
        code: &str,
        attributes: Value,
    ) -> Result<String, TraceError> {
        let source = TraceSource {
            kind,
            reference: reference.into(),
        };
        let id = fingerprint(&json!([source, code, attributes]));
        if !self.nodes.contains_key(&id) {
            if self.nodes.len() == self.limits.max_nodes {
                return Err(TraceError::RequiredTraceLimit);
            }
            let attributes = attributes
                .as_object()
                .expect("node attributes object")
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            self.nodes.insert(
                id.clone(),
                TraceNode {
                    id: id.clone(),
                    source,
                    code: code.into(),
                    attributes,
                },
            );
        }
        Ok(id)
    }
    fn link(&mut self, from: &str, to: &str, relation: &str) {
        self.edges.insert(TraceEdge {
            from: from.into(),
            to: to.into(),
            relation: relation.into(),
        });
    }
    fn diagnostic(&mut self, parent: &str, d: &CompositionDiagnostic) -> Result<(), TraceError> {
        let (code, kind, reference, attributes) = diagnostic(d);
        let id = self.node(kind, reference, code, attributes)?;
        self.link(parent, &id, "EXPLAINS");
        Ok(())
    }
    fn provider_link(&mut self, binding: &str, provider: &gateway_registry::CapabilityProvider) {
        let node = self
            .nodes
            .values()
            .find(|n| {
                n.source.kind == provider_kind(provider)
                    && n.source.reference == provider.id()
                    && n.code == "CANONICAL_PROVIDER"
            })
            .expect("revalidated chosen provider")
            .id
            .clone();
        self.link(&node, binding, "BOUND_AS");
    }
    fn closure(&mut self, parent: &str, closure: &EffectiveSkills) -> Result<(), TraceError> {
        for skill in &closure.skills {
            let id = self.node(
                SourceKind::Skill,
                skill.as_str(),
                "EFFECTIVE_SKILL",
                json!({}),
            )?;
            self.link(parent, &id, "REQUIRES");
            for path in closure.inclusion_paths.get(skill).into_iter().flatten() {
                let mut previous = parent.to_owned();
                for node in path {
                    let (kind, reference) = match node {
                        SkillNode::Skill(s) => (SourceKind::Skill, s.as_str()),
                        SkillNode::Capability(c) => (SourceKind::Capability, c.as_str()),
                    };
                    let current = self.node(kind, reference, "DEPENDENCY", json!({}))?;
                    self.link(&previous, &current, "REQUIRES");
                    previous = current;
                }
            }
        }
        for (id, class) in &closure.required_capabilities {
            let node = self.node(SourceKind::Capability, id.as_str(), "REQUIRED_NOT_APPROVED", json!({"class":class.map(|c| c.as_str()),
                "provider":closure.rules.capability_providers.get(id).map(|p| p.canonical_source())}))?;
            self.link(parent, &node, "REQUIRES");
        }
        for d in &closure.diagnostics {
            self.diagnostic(parent, &CompositionDiagnostic::Skill(d.clone()))?;
        }
        Ok(())
    }
}

/// Recompute before explaining: a public report is a proposal, not authority.
pub fn explain_resolution(
    snapshot: &ResolutionSnapshot,
    report: &CompositionReport,
    limits: TraceLimits,
) -> Result<ResolutionTrace, TraceError> {
    if limits.max_nodes == 0 || limits.max_nodes > 100_000 || limits.max_optional_details > 100_000
    {
        return Err(TraceError::InvalidLimit);
    }
    if report.basis != *snapshot.request().basis() {
        return Err(TraceError::StaleBasis);
    }
    if compose_resolution(snapshot, &report.rules).map_err(|_| TraceError::InvalidArtifact)?
        != *report
    {
        return Err(TraceError::InvalidArtifact);
    }
    let mut g = Graph {
        nodes: BTreeMap::new(),
        edges: BTreeSet::new(),
        limits,
        optional: vec![],
        omitted: 0,
    };
    let plan = snapshot.request().plan();
    let root = g.node(SourceKind::Plan, plan.id().as_str(), report.outcome.as_str(), json!({"policy_authorization":"NOT_EVALUATED", "search_complete":!report.exhausted,"visits":report.visits}))?;
    let desired = g.node(
        SourceKind::DesiredState,
        plan.desired_state().as_str(),
        "SOURCE_REFERENCE",
        json!({}),
    )?;
    let situation = g.node(SourceKind::Situation, report.basis.situation.as_str(), "SCOPED_PROVENANCE_REFERENCE", json!({"scope":report.basis.scope.as_str(),"fingerprint":report.basis.situation_fingerprint.as_str()}))?;
    let delta = g.node(
        SourceKind::Delta,
        plan.delta().as_str(),
        "SOURCE_REFERENCE",
        json!({}),
    )?;
    g.link(&desired, &delta, "COMPARED");
    g.link(&situation, &delta, "OBSERVED");
    g.link(&delta, &root, "PLANNED");
    let rule_fingerprint = fingerprint(&rules_json(&report.rules));
    let rule = g.node(SourceKind::Rule, &rule_fingerprint, "LEXICOGRAPHIC_INTEGER_PRIORITY_OPTIONAL_COUNT", json!({"version":report.rules.version.to_string(),"prefer_optional":report.rules.prefer_optional,
        "provider_priorities":report.rules.provider_priorities.iter().map(|(p,n)| json!([p.canonical_source(),n])).collect::<Vec<_>>(),
        "operating_mode":snapshot.input().operating_mode.as_str(),"execution_profile":snapshot.input().execution_profile.as_str()}))?;
    g.link(&rule, &root, "GOVERNS");
    let process = g.node(
        SourceKind::Process,
        report.basis.process_catalog_fingerprint.as_str(),
        process_code(report.processes.outcome),
        json!({"state_basis":report.basis.process_state_fingerprint.as_str()}),
    )?;
    g.link(&root, &process, "PROCESS_DECISION");
    for rejected in &report.processes.rejections {
        let node = g.node(SourceKind::Process, rejected.definition.id().as_str(), "PROCESS_REJECTED", json!({"definition":rejected.definition,"step":rejected.step.as_ref().map(|s| s.as_str()),"reason":process_rejection(rejected.reason)}))?;
        g.link(&process, &node, "REJECTED");
    }
    for step in &report.steps {
        let step_node = g.node(
            SourceKind::PlanStep,
            step.step.as_str(),
            step.outcome.as_str(),
            json!({}),
        )?;
        g.link(&root, &step_node, "CONTAINS");
        let original = plan
            .steps()
            .iter()
            .find(|s| s.id() == &step.step)
            .expect("revalidated step");
        for dependency in original.dependencies() {
            let node = g.node(
                SourceKind::PlanStep,
                dependency.as_str(),
                "PREDECESSOR_REFERENCE",
                json!({}),
            )?;
            g.link(&step_node, &node, "DEPENDS_ON");
        }
        for set in report.discovery.sets.iter().filter(|s| s.step == step.step) {
            let requirement = g.node(
                SourceKind::Requirement,
                set.requirement.as_str(),
                candidate_code(set.outcome),
                json!({"cardinality":set.cardinality.as_str()}),
            )?;
            g.link(&step_node, &requirement, "REQUIRES");
            let original_requirement = plan
                .capability_requirements()
                .iter()
                .find(|r| r.id() == &set.requirement)
                .expect("validated requirement");
            let item = snapshot
                .input()
                .delta
                .items()
                .iter()
                .find(|d| d.id() == original_requirement.originating_delta_item())
                .expect("validated delta reference");
            let source = g.node(SourceKind::Delta, item.id().as_str(), "DELTA_ITEM_REFERENCE", json!({"condition":item.condition().as_str(),
                "capability":original_requirement.capability().as_str(),"evidence":item.basis().evidence().iter().map(|e| e.as_str()).collect::<Vec<_>>(),
                "provenance":item.basis().provenances().iter().map(|p| p.as_str()).collect::<Vec<_>>()}))?;
            g.link(&delta, &source, "CONTAINS");
            g.link(&source, &requirement, "MOTIVATES");
            for candidate in &set.candidates {
                let contract = candidate.canonical.capability();
                let cap = g.node(
                    SourceKind::Capability,
                    contract.id().as_str(),
                    "PROVIDED_NOT_APPROVED",
                    json!({"class":contract.class().as_str()}),
                )?;
                g.link(&requirement, &cap, "CONTRACT");
                let provider = g.node(
                    provider_kind(candidate.canonical.provider()),
                    candidate.canonical.provider().id(),
                    "CANONICAL_PROVIDER",
                    json!({"definition_fingerprint":candidate.definition_fingerprint.as_str()}),
                )?;
                g.link(&cap, &provider, "PROVIDED_BY");
                g.optional.push((
                    provider,
                    fingerprint(&json!(
                        candidate
                            .canonical
                            .matched_selectors()
                            .iter()
                            .map(selector)
                            .collect::<Vec<_>>()
                    )),
                ));
            }
            for rejection in &set.rejections {
                let node = g.node(provider_kind(rejection.provider()), rejection.provider().id(), "CONTRACT_REJECTED", json!({"failed_selectors_fingerprint":fingerprint(&json!(rejection.failed_selectors().map(selector).collect::<Vec<_>>()))}))?;
                g.link(&requirement, &node, "REJECTED");
            }
        }
        for d in &step.diagnostics {
            g.diagnostic(&step_node, d)?;
        }
        if let Some(proof) = report.rules.applicability.completed.get(&step.step) {
            for evidence in &proof.references {
                let node = g.node(SourceKind::Evidence, evidence.as_str(), "COMPLETION_ATTESTATION_REFERENCE", json!({"status":status(proof.status),"freshness":proof.freshness.as_str(),"basis_matches":proof.basis == report.basis}))?;
                g.link(&step_node, &node, "ATTESTED_BY");
            }
        }
        for rejected in &step.rejections {
            let chosen: Vec<_> = rejected
                .chosen
                .iter()
                .map(|(r, p)| json!([r.as_str(), p.canonical_source()]))
                .collect();
            let node = g.node(
                SourceKind::Binding,
                fingerprint(&json!([chosen, rejected.activity])),
                "BINDING_REJECTED",
                json!({"chosen":chosen,"activity":rejected.activity}),
            )?;
            g.link(&step_node, &node, "REJECTED");
            for provider in rejected.chosen.values() {
                g.provider_link(&node, provider);
            }
            for d in &rejected.reasons {
                g.diagnostic(&node, d)?;
            }
            if let Some(closure) = &rejected.skills {
                g.closure(&node, closure)?;
            }
        }
        for alternative in &step.alternatives {
            let chosen: Vec<_> = alternative
                .chosen
                .iter()
                .map(|(r, p)| json!([r.as_str(), p.canonical_source()]))
                .collect();
            let best = report
                .alternatives
                .iter()
                .any(|row| row.contains(alternative));
            let binding = alternative.binding.as_ref();
            let attributes = json!({"chosen":chosen,"primary":binding.map(|b| b.primary_agent.as_str()),
                "participants":binding.map(|b| b.participating_agents.iter().map(|a| a.as_str()).collect::<Vec<_>>()),
                "process":binding.and_then(|b| b.process.as_ref()).map(|p| json!({"definition":p.definition,"instance":p.instance})),
                "activity":alternative.activity,"score":alternative.score,"readiness":alternative.applicability.readiness.as_str()});
            let code = if report.exhausted {
                "SEARCH_INCOMPLETE_ALTERNATIVE"
            } else if best && report.alternatives.len() == 1 {
                "SELECTED"
            } else if best {
                "EQUAL_RANK_ALTERNATIVE"
            } else {
                "NOT_GLOBALLY_SELECTED"
            };
            let node = g.node(
                SourceKind::Binding,
                fingerprint(&attributes),
                code,
                attributes,
            )?;
            g.link(&step_node, &node, "BINDING");
            for provider in alternative.chosen.values() {
                g.provider_link(&node, provider);
            }
            g.link(&rule, &node, "RANKS");
            if let Some(binding) = binding {
                for (skill, agent) in &binding.skills {
                    let assigned = g.node(
                        SourceKind::Skill,
                        skill.as_str(),
                        "RESPONSIBILITY",
                        json!({"agent":agent.as_str()}),
                    )?;
                    g.link(&node, &assigned, "ASSIGNS");
                }
            }
            for d in &alternative.diagnostics {
                g.diagnostic(&node, d)?;
            }
            for reason in &alternative.applicability.reasons {
                g.diagnostic(&node, &CompositionDiagnostic::Applicability(reason.clone()))?;
            }
            if let Some(closure) = &alternative.skills {
                g.closure(&node, closure)?;
            }
        }
    }
    for (index, (provider, reference)) in std::mem::take(&mut g.optional).into_iter().enumerate() {
        if index < limits.max_optional_details && g.nodes.len() < limits.max_nodes {
            let node = g.node(
                SourceKind::Constraint,
                reference,
                "MATCHED_SELECTOR_REFERENCE",
                json!({}),
            )?;
            g.link(&provider, &node, "MATCHED");
        } else {
            g.omitted += 1;
        }
    }
    Ok(ResolutionTrace {
        version: 1,
        basis: basis_json(&report.basis),
        rule_fingerprint,
        outcome: report.outcome.as_str().into(),
        policy_authorization: "NOT_EVALUATED".into(),
        search_complete: !report.exhausted,
        nodes: g.nodes.into_values().collect(),
        edges: g.edges.into_iter().collect(),
        omitted_optional_details: g.omitted,
    })
}

fn provider_kind(p: &gateway_registry::CapabilityProvider) -> SourceKind {
    match p {
        gateway_registry::CapabilityProvider::Agent { .. } => SourceKind::Agent,
        gateway_registry::CapabilityProvider::Skill { .. } => SourceKind::Skill,
    }
}
fn candidate_code(o: crate::resolution_candidates::CandidateOutcome) -> &'static str {
    use crate::resolution_candidates::CandidateOutcome::*;
    match o {
        Compatible => "COMPATIBLE",
        UnknownCapability => "UNKNOWN_CAPABILITY",
        MissingProvider => "MISSING_PROVIDER",
        Incompatible => "INCOMPATIBLE_CONTRACT",
    }
}
pub(crate) fn process_code(o: crate::resolution_process::ProcessSelectionOutcome) -> &'static str {
    use crate::resolution_process::ProcessSelectionOutcome::*;
    match o {
        NoTemplate => "NO_TEMPLATE_REQUIRED",
        Unique => "UNIQUE_PROCESS",
        Ambiguous => "AMBIGUOUS_PROCESS",
        Missing => "MISSING_PROCESS",
        Incompatible => "INCOMPATIBLE_PROCESS",
        Unsupported => "UNSUPPORTED_PROCESS",
    }
}
pub(crate) fn process_rejection(
    r: crate::resolution_process::ProcessRejectionReason,
) -> &'static str {
    use crate::resolution_process::ProcessRejectionReason::*;
    match r {
        DefinitionConstraint => "DEFINITION_CONSTRAINT",
        PinnedDefinition => "PINNED_DEFINITION",
        ActivityContract => "ACTIVITY_CONTRACT",
        UnsupportedLifecycle => "UNSUPPORTED_LIFECYCLE",
    }
}
pub(crate) fn diagnostic(d: &CompositionDiagnostic) -> (&'static str, SourceKind, String, Value) {
    use CompositionDiagnostic::*;
    let empty = || (SourceKind::Diagnostic, "resolution".to_owned(), json!({}));
    match d {
        MissingRequirement(id) => (
            "MISSING_REQUIREMENT",
            SourceKind::Requirement,
            id.to_string(),
            json!({}),
        ),
        OptionalOmitted(id) => (
            "OPTIONAL_OMITTED",
            SourceKind::Requirement,
            id.to_string(),
            json!({}),
        ),
        RoleConflict => {
            let (k, r, a) = empty();
            ("ROLE_CONFLICT", k, r, a)
        }
        SearchLimit => {
            let (k, r, a) = empty();
            ("SEARCH_INCOMPLETE", k, r, a)
        }
        Skill(d) => skill_diagnostic(d),
        Applicability(r) => applicability_diagnostic(r),
    }
}
fn skill_diagnostic(d: &SkillDiagnostic) -> (&'static str, SourceKind, String, Value) {
    use SkillDiagnostic::*;
    match d {
        MissingSkill(id) => (
            "MISSING_SKILL",
            SourceKind::Skill,
            id.to_string(),
            json!({}),
        ),
        Condition(id, s, mandatory) => (
            "SKILL_CONDITION",
            SourceKind::Skill,
            id.to_string(),
            json!({"status":status(*s),"mandatory":mandatory}),
        ),
        MissingCapability(id) => (
            "MISSING_CAPABILITY",
            SourceKind::Capability,
            id.to_string(),
            json!({}),
        ),
        UnboundCapability(id) => (
            "UNBOUND_CAPABILITY",
            SourceKind::Capability,
            id.to_string(),
            json!({}),
        ),
        InvalidCapabilityProvider(id) => (
            "INVALID_CAPABILITY_PROVIDER",
            SourceKind::Capability,
            id.to_string(),
            json!({}),
        ),
        Cycle(path) => (
            "DEPENDENCY_CYCLE",
            SourceKind::Diagnostic,
            fingerprint(&json!(
                path.iter()
                    .map(|n| match n {
                        SkillNode::Skill(id) => format!("skill:{id}"),
                        SkillNode::Capability(id) => format!("capability:{id}"),
                    })
                    .collect::<Vec<_>>()
            )),
            json!({}),
        ),
        LimitExceeded => (
            "SKILL_SEARCH_INCOMPLETE",
            SourceKind::Diagnostic,
            "skill-closure".into(),
            json!({}),
        ),
    }
}
fn applicability_diagnostic(r: &ApplicabilityReason) -> (&'static str, SourceKind, String, Value) {
    use ApplicabilityReason::*;
    match r {
        PredecessorPending(id) => (
            "PREDECESSOR_PENDING",
            SourceKind::PlanStep,
            id.to_string(),
            json!({}),
        ),
        InvalidCompletion(id) => (
            "INVALID_COMPLETION_EVIDENCE",
            SourceKind::PlanStep,
            id.to_string(),
            json!({}),
        ),
        Prerequisite(i, s) => (
            "PREREQUISITE",
            SourceKind::Constraint,
            i.to_string(),
            json!({"status":status(*s)}),
        ),
        Restriction(text, s) => (
            "INTRINSIC_RESTRICTION",
            SourceKind::Constraint,
            fingerprint(&json!(text)),
            json!({"status":status(*s)}),
        ),
        ProcessUnavailable => (
            "PROCESS_UNAVAILABLE",
            SourceKind::Process,
            "snapshot".into(),
            json!({}),
        ),
        ProcessStatus(s) => (
            "PROCESS_STATUS",
            SourceKind::Process,
            "snapshot".into(),
            json!({"status":s}),
        ),
        Gate(id, s) => (
            "PROCESS_GATE",
            SourceKind::Process,
            id.clone(),
            json!({"status":s}),
        ),
        Blocker(id) => (
            "PROCESS_BLOCKER",
            SourceKind::Process,
            id.clone(),
            json!({}),
        ),
        Waiting => (
            "PROCESS_WAITING",
            SourceKind::Process,
            "snapshot".into(),
            json!({}),
        ),
        ActivityUnavailable => (
            "ACTIVITY_UNAVAILABLE",
            SourceKind::Process,
            "snapshot".into(),
            json!({}),
        ),
        CapabilityUnavailable(id) => (
            "ACTIVITY_CAPABILITY_UNAVAILABLE",
            SourceKind::Capability,
            id.to_string(),
            json!({}),
        ),
    }
}
