//! Strict adapter documents. Domain values use their existing validated serde contracts.
use super::*;
use gateway_application::{
    resolution_agents::AgentRules,
    resolution_applicability::ApplicabilityRules,
    resolution_candidates::CandidateRules,
    resolution_composition::CompositionRules,
    resolution_process::{ProcessSelectionRules, TemplatePreference},
    resolution_skills::SkillCondition,
};
use gateway_domain::*;
use gateway_policy::{Approval, StepFacts, WorkClass};
use gateway_process::{
    ActivityId, DefinitionIdentity, ProcessInstance, ProcessInstanceRevision, StateId,
};
use gateway_registry::CapabilityProvider;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub(super) use gateway_application::codex::assessment::{Assessment, AssessmentInput};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlanDocument {
    pub schema_version: u32,
    pub assessment: Assessment,
    pub desired_state: DesiredState,
    pub delta: Delta,
    pub plan: Plan,
    pub explanation: String,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlanningBindings {
    pub domain_change: Option<CapabilityId>,
    pub evidence_acquisition: Option<CapabilityId>,
    pub observation: Option<CapabilityId>,
    pub input_acquisition: Option<CapabilityId>,
    pub conflict_resolution: Option<CapabilityId>,
    pub assessment: Option<CapabilityId>,
}
impl PlanningBindings {
    pub fn build(self) -> CapabilityRequirementRules {
        let mut rules = CapabilityRequirementRules::default();
        if let Some(id) = self.domain_change {
            rules = rules.with_domain_change(id);
        }
        if let Some(id) = self.evidence_acquisition {
            rules = rules.with_evidence_acquisition(id);
        }
        if let Some(id) = self.observation {
            rules = rules.with_observation(id);
        }
        if let Some(id) = self.input_acquisition {
            rules = rules.with_input_acquisition(id);
        }
        if let Some(id) = self.conflict_resolution {
            rules = rules.with_conflict_resolution(id);
        }
        if let Some(id) = self.assessment {
            rules = rules.with_assessment(id);
        }
        rules
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum ConditionInput {
    Always,
    Never,
    Mode(OperatingMode),
    Profile(ExecutionProfile),
    ProcessState(StateId),
    DesiredCondition(ConditionId),
    Unsupported(ReferenceId),
}
impl ConditionInput {
    fn build(self) -> SkillCondition {
        match self {
            Self::Always => SkillCondition::Always,
            Self::Never => SkillCondition::Never,
            Self::Mode(v) => SkillCondition::Mode(v),
            Self::Profile(v) => SkillCondition::Profile(v),
            Self::ProcessState(v) => SkillCondition::ProcessState(v),
            Self::DesiredCondition(v) => SkillCondition::DesiredCondition(v),
            Self::Unsupported(v) => SkillCondition::Unsupported(v),
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Provider {
    Agent(AgentId),
    Skill(SkillId),
}
impl Provider {
    fn build(self) -> CapabilityProvider {
        match self {
            Self::Agent(agent_id) => CapabilityProvider::Agent { agent_id },
            Self::Skill(skill_id) => CapabilityProvider::Skill { skill_id },
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Priority {
    provider: Provider,
    priority: i32,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResolutionRules {
    #[serde(default, deserialize_with = "step_map")]
    primary_agents: BTreeMap<PlanStepId, AgentId>,
    #[serde(default, deserialize_with = "step_map")]
    participants: BTreeMap<PlanStepId, BTreeSet<AgentId>>,
    pub required_process: Option<DefinitionIdentity>,
    #[serde(default, deserialize_with = "step_map")]
    activities: BTreeMap<PlanStepId, ActivityId>,
    #[serde(default)]
    semantics: BTreeMap<String, ConditionInput>,
    #[serde(default)]
    priorities: Vec<Priority>,
}
impl ResolutionRules {
    pub fn build(self) -> Result<CompositionRules, CliError> {
        let mut priorities = BTreeMap::new();
        for item in self.priorities {
            if priorities
                .insert(item.provider.build(), item.priority)
                .is_some()
            {
                return Err(CliError::new(
                    3,
                    "INVALID_INPUT",
                    "duplicate provider priority",
                ));
            }
        }
        Ok(CompositionRules {
            version: SchemaVersion::V1,
            candidates: CandidateRules {
                version: SchemaVersion::V1,
                selectors: BTreeMap::new(),
            },
            processes: ProcessSelectionRules {
                version: SchemaVersion::V1,
                preference: if self.required_process.is_some() {
                    TemplatePreference::Required
                } else {
                    TemplatePreference::None
                },
                required_definition: self.required_process,
                activities: self.activities.clone(),
                output_evidence: BTreeMap::new(),
                lifecycle_contracts: BTreeMap::new(),
            },
            agents: AgentRules {
                version: SchemaVersion::V1,
                primary: self.primary_agents,
                participants: self.participants,
                process_roles: BTreeMap::new(),
            },
            skills: BTreeMap::new(),
            applicability: ApplicabilityRules {
                version: SchemaVersion::V1,
                restrictions: BTreeMap::new(),
                semantics: self
                    .semantics
                    .into_iter()
                    .map(|(k, v)| (k, v.build()))
                    .collect(),
                completed: BTreeMap::new(),
                activities: self.activities,
            },
            provider_priorities: priorities,
            prefer_optional: false,
            max_visits: 10000,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Rules {
    pub schema_version: u32,
    #[serde(default)]
    pub planning: PlanningBindings,
    #[serde(default)]
    pub resolution: ResolutionRules,
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            schema_version: 1,
            planning: PlanningBindings::default(),
            resolution: ResolutionRules::default(),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProcessInput {
    pub schema_version: u32,
    pub instance: ProcessInstance,
    pub expected_revision: ProcessInstanceRevision,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PolicyDefinitionInput {
    id: PolicyId,
    description: String,
    #[serde(default)]
    allowed_capabilities: Vec<CapabilityId>,
    #[serde(default)]
    denied_capabilities: Vec<CapabilityId>,
}
impl PolicyDefinitionInput {
    pub fn build(self) -> Result<PolicyDefinition, CliError> {
        checked(
            PolicyDefinition::with_denied_capabilities(
                self.id,
                self.description,
                self.allowed_capabilities,
                self.denied_capabilities,
            ),
            8,
            "INVALID_POLICY",
        )
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ApprovalInput {
    Granted,
    Denied,
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WorkClassInput {
    Feature,
    Maintenance,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FactsInput {
    #[serde(default)]
    authorizations: BTreeMap<CapabilityId, ApprovalInput>,
    #[serde(default)]
    consents: BTreeMap<CapabilityId, ApprovalInput>,
    #[serde(default)]
    evidence: BTreeSet<String>,
    #[serde(default)]
    satisfied_constraints: BTreeSet<String>,
    work_class: Option<WorkClassInput>,
    #[serde(default)]
    prerequisites_satisfied: bool,
}
impl FactsInput {
    pub fn build(self) -> StepFacts {
        let approval = |v| match v {
            ApprovalInput::Granted => Approval::Granted,
            ApprovalInput::Denied => Approval::Denied,
        };
        StepFacts {
            authorizations: self
                .authorizations
                .into_iter()
                .map(|(k, v)| (k, approval(v)))
                .collect(),
            consents: self
                .consents
                .into_iter()
                .map(|(k, v)| (k, approval(v)))
                .collect(),
            evidence: self.evidence,
            satisfied_constraints: self.satisfied_constraints,
            work_class: self.work_class.map(|v| match v {
                WorkClassInput::Feature => WorkClass::Feature,
                WorkClassInput::Maintenance => WorkClass::Maintenance,
            }),
            prerequisites_satisfied: self.prerequisites_satisfied,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PolicyInput {
    pub schema_version: u32,
    pub basis: Value,
    pub operating_mode: OperatingMode,
    pub execution_profile: ExecutionProfile,
    pub policies: Vec<PolicyDefinitionInput>,
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    #[serde(default)]
    pub required_evidence: BTreeMap<CapabilityId, BTreeSet<String>>,
    #[serde(default, deserialize_with = "step_map")]
    pub steps: BTreeMap<PlanStepId, FactsInput>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkflowInput {
    id: WorkflowId,
    description: String,
    primary_agent_id: AgentId,
    skill_ids: Vec<SkillId>,
    policy_id: PolicyId,
}
impl WorkflowInput {
    pub fn build(self) -> Result<WorkflowDefinition, CliError> {
        checked(
            WorkflowDefinition::new(
                self.id,
                self.description,
                self.primary_agent_id,
                self.skill_ids,
                self.policy_id,
            ),
            9,
            "INVALID_WORKFLOW",
        )
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectionInput {
    pub schema_version: u32,
    pub basis: Value,
    #[serde(deserialize_with = "step_id")]
    pub step: PlanStepId,
    pub process: DefinitionIdentity,
    pub workflow: WorkflowId,
    pub decision_reference: ReferenceId,
    pub state_decision: ReferenceId,
    pub id: ExecutionContextId,
    pub task: TaskDescriptor,
    pub state: ExecutionState,
    pub target_runtime: ExecutionRuntimeId,
    #[serde(default)]
    pub knowledge_queries: Vec<KnowledgeQuery>,
    pub workflows: Vec<WorkflowInput>,
    #[serde(default)]
    pub fragments: Vec<FragmentInput>,
    #[serde(default)]
    pub selected: BTreeSet<ReferenceId>,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExternalKind {
    Evidence,
    Knowledge,
    Memory,
    UserInput,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FragmentInput {
    id: ReferenceId,
    kind: ExternalKind,
    content: String,
    scope: ContextScopeId,
    #[serde(deserialize_with = "step_id")]
    step: PlanStepId,
    source: String,
    revision: Option<String>,
    quality: QualityMetadata,
    rationale: String,
    #[serde(default)]
    evidence: BTreeSet<ReferenceId>,
    validation: Option<ReferenceId>,
}
impl FragmentInput {
    pub fn build(
        self,
        records: Option<&ObservationEvidenceSet>,
    ) -> Result<gateway_context::ContextFragment, CliError> {
        use gateway_context::{ContextFragment, FragmentKind, FragmentMetadata};
        let metadata = FragmentMetadata {
            provenance: checked(
                KnowledgeProvenance::new(self.source, self.revision),
                9,
                "INVALID_FRAGMENT",
            )?,
            evidence: self.evidence,
            quality: self.quality,
            rationale: checked(NonEmptyText::new(self.rationale), 9, "INVALID_FRAGMENT")?,
            validation: self.validation,
        };
        if matches!(self.kind, ExternalKind::Evidence) {
            let evidence = records
                .and_then(|r| {
                    r.evidence()
                        .iter()
                        .find(|e| e.id().as_str() == self.content)
                })
                .ok_or_else(|| {
                    CliError::new(
                        9,
                        "INVALID_FRAGMENT",
                        "evidence reference is absent from the captured records",
                    )
                })?;
            let provenance = records
                .and_then(|r| {
                    r.provenances()
                        .iter()
                        .find(|p| p.id() == evidence.provenance())
                })
                .ok_or_else(|| {
                    CliError::new(
                        9,
                        "INVALID_FRAGMENT",
                        "evidence provenance is absent from the captured records",
                    )
                })?;
            return checked(
                ContextFragment::evidence(
                    self.id, evidence, provenance, metadata, self.scope, self.step,
                ),
                9,
                "INVALID_FRAGMENT",
            );
        }
        let kind = match self.kind {
            ExternalKind::Evidence => FragmentKind::Evidence,
            ExternalKind::Knowledge => FragmentKind::Knowledge,
            ExternalKind::Memory => FragmentKind::Memory,
            ExternalKind::UserInput => FragmentKind::UserInput,
        };
        checked(
            ContextFragment::external(self.id, kind, self.content, metadata, self.scope, self.step),
            9,
            "INVALID_FRAGMENT",
        )
    }
}

fn step_id<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<PlanStepId, D::Error> {
    PlanStepId::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}
fn step_map<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<BTreeMap<PlanStepId, T>, D::Error> {
    BTreeMap::<String, T>::deserialize(deserializer)?
        .into_iter()
        .map(|(id, v)| {
            PlanStepId::new(id)
                .map(|id| (id, v))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}
