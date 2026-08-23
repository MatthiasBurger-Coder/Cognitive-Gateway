use std::collections::HashSet;

use crate::{
    AgentId, CapabilityId, Constraint, DefinitionCatalog, ExecutionContextId, ExecutionProfile,
    ExecutionRuntimeId, ExecutionState, KnowledgeQuery, OperatingMode, PolicyId, SchemaVersion,
    SkillId, ValidationError, WorkflowId, relationships::unique_relationships,
    task::TaskDescriptor,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionContext {
    pub task: TaskDescriptor,
    pub operating_mode: OperatingMode,
    pub execution_profile: ExecutionProfile,
}

/// Versioned, provider-independent execution contract consumed by runtimes.
///
/// `ExecutionContextIR` contains references and validated value objects only.
/// It does not contain prompts, model names, runtime handles, transport data,
/// or executable behavior. The runtime target is an opaque, typed identity so
/// adapters can map it to Codex, a local model, or another implementation
/// without changing this domain type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionContextIR {
    schema_version: SchemaVersion,
    id: ExecutionContextId,
    task: TaskDescriptor,
    workflow_id: WorkflowId,
    primary_agent_id: AgentId,
    skill_ids: Vec<SkillId>,
    operating_mode: OperatingMode,
    execution_profile: ExecutionProfile,
    state: ExecutionState,
    policy_id: PolicyId,
    knowledge_queries: Vec<KnowledgeQuery>,
    approved_capability_ids: Vec<CapabilityId>,
    constraints: Vec<Constraint>,
    target_runtime: ExecutionRuntimeId,
}

impl ExecutionContextIR {
    /// Creates a validated v1 execution context.
    ///
    /// All collections are owned and retain caller order. Empty knowledge and
    /// capability lists are intentional: they mean "no knowledge requested"
    /// and "no capability approved", respectively. Skills are required and
    /// must not be empty because they are part of the execution semantics.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: SchemaVersion,
        id: ExecutionContextId,
        task: TaskDescriptor,
        workflow_id: WorkflowId,
        primary_agent_id: AgentId,
        skill_ids: impl IntoIterator<Item = SkillId>,
        operating_mode: OperatingMode,
        execution_profile: ExecutionProfile,
        state: ExecutionState,
        policy_id: PolicyId,
        knowledge_queries: impl IntoIterator<Item = KnowledgeQuery>,
        approved_capability_ids: impl IntoIterator<Item = CapabilityId>,
        constraints: impl IntoIterator<Item = Constraint>,
        target_runtime: ExecutionRuntimeId,
    ) -> Result<Self, ValidationError> {
        if schema_version != SchemaVersion::V1 {
            return Err(ValidationError::UnsupportedSchemaVersion {
                expected: "1.0",
                actual: schema_version.to_string(),
            });
        }

        let skill_ids = unique_relationships(skill_ids, "skill_ids")?;
        if skill_ids.is_empty() {
            return Err(ValidationError::EmptyRelationship { field: "skill_ids" });
        }

        let approved_capability_ids =
            unique_relationships(approved_capability_ids, "approved_capability_ids")?;
        let constraints = unique_relationships(constraints, "constraints")?;
        if has_duplicate(constraints.iter().map(Constraint::id)) {
            return Err(ValidationError::DuplicateRelationship {
                field: "constraints",
            });
        }

        let context = Self {
            schema_version,
            id,
            task,
            workflow_id,
            primary_agent_id,
            skill_ids,
            operating_mode,
            execution_profile,
            state,
            policy_id,
            knowledge_queries: knowledge_queries.into_iter().collect(),
            approved_capability_ids,
            constraints,
            target_runtime,
        };
        context.validate()?;
        Ok(context)
    }

    /// Creates a v1 context without making the fixed supported version
    /// implicit in the resulting value.
    #[allow(clippy::too_many_arguments)]
    pub fn new_v1(
        id: ExecutionContextId,
        task: TaskDescriptor,
        workflow_id: WorkflowId,
        primary_agent_id: AgentId,
        skill_ids: impl IntoIterator<Item = SkillId>,
        operating_mode: OperatingMode,
        execution_profile: ExecutionProfile,
        state: ExecutionState,
        policy_id: PolicyId,
        knowledge_queries: impl IntoIterator<Item = KnowledgeQuery>,
        approved_capability_ids: impl IntoIterator<Item = CapabilityId>,
        constraints: impl IntoIterator<Item = Constraint>,
        target_runtime: ExecutionRuntimeId,
    ) -> Result<Self, ValidationError> {
        Self::new(
            SchemaVersion::V1,
            id,
            task,
            workflow_id,
            primary_agent_id,
            skill_ids,
            operating_mode,
            execution_profile,
            state,
            policy_id,
            knowledge_queries,
            approved_capability_ids,
            constraints,
            target_runtime,
        )
    }

    /// Fallible constructor alias for parsing boundaries.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        schema_version: SchemaVersion,
        id: ExecutionContextId,
        task: TaskDescriptor,
        workflow_id: WorkflowId,
        primary_agent_id: AgentId,
        skill_ids: impl IntoIterator<Item = SkillId>,
        operating_mode: OperatingMode,
        execution_profile: ExecutionProfile,
        state: ExecutionState,
        policy_id: PolicyId,
        knowledge_queries: impl IntoIterator<Item = KnowledgeQuery>,
        approved_capability_ids: impl IntoIterator<Item = CapabilityId>,
        constraints: impl IntoIterator<Item = Constraint>,
        target_runtime: ExecutionRuntimeId,
    ) -> Result<Self, ValidationError> {
        Self::new(
            schema_version,
            id,
            task,
            workflow_id,
            primary_agent_id,
            skill_ids,
            operating_mode,
            execution_profile,
            state,
            policy_id,
            knowledge_queries,
            approved_capability_ids,
            constraints,
            target_runtime,
        )
    }

    /// Validates invariants that do not require a definition catalog.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != SchemaVersion::V1 {
            return Err(ValidationError::UnsupportedSchemaVersion {
                expected: "1.0",
                actual: self.schema_version.to_string(),
            });
        }
        if self.skill_ids.is_empty() {
            return Err(ValidationError::EmptyRelationship { field: "skill_ids" });
        }
        if has_duplicate(self.skill_ids.iter()) {
            return Err(ValidationError::DuplicateRelationship { field: "skill_ids" });
        }
        if has_duplicate(self.approved_capability_ids.iter()) {
            return Err(ValidationError::DuplicateRelationship {
                field: "approved_capability_ids",
            });
        }
        if has_duplicate(self.constraints.iter().map(Constraint::id)) {
            return Err(ValidationError::DuplicateRelationship {
                field: "constraints",
            });
        }
        for constraint in &self.constraints {
            constraint.validate_for(self.operating_mode, self.execution_profile)?;
        }
        Ok(())
    }

    /// Validates references and authority against a complete definition
    /// catalog. The catalog remains external so this IR stays a small value
    /// object and can be built at the application boundary.
    pub fn validate_against(&self, catalog: &DefinitionCatalog) -> Result<(), ValidationError> {
        self.validate()?;
        catalog.validate()?;

        let workflow = catalog.workflow(&self.workflow_id).ok_or_else(|| {
            ValidationError::MissingDefinition {
                kind: "workflow",
                id: self.workflow_id.to_string(),
            }
        })?;
        if workflow.primary_agent_id() != &self.primary_agent_id {
            return Err(ValidationError::InvalidStateCombination {
                reason: "execution context agent must match workflow primary agent",
            });
        }

        if catalog.agent(&self.primary_agent_id).is_none() {
            return Err(ValidationError::MissingDefinition {
                kind: "agent",
                id: self.primary_agent_id.to_string(),
            });
        }

        let mut workflow_skill_closure = HashSet::new();
        for skill_id in workflow.skill_ids() {
            collect_skill_closure(skill_id, catalog, &mut workflow_skill_closure)?;
            if !self.skill_ids.contains(skill_id) {
                return Err(ValidationError::InvalidStateCombination {
                    reason: "execution context must include every workflow skill",
                });
            }
        }
        for skill_id in &self.skill_ids {
            let skill =
                catalog
                    .skill(skill_id)
                    .ok_or_else(|| ValidationError::MissingDefinition {
                        kind: "skill",
                        id: skill_id.to_string(),
                    })?;
            if !workflow_skill_closure.contains(skill_id) {
                return Err(ValidationError::InvalidStateCombination {
                    reason: "execution context skill is not part of the workflow",
                });
            }
            for dependency_id in skill.dependency_ids() {
                if !self.skill_ids.contains(dependency_id) {
                    return Err(ValidationError::InvalidStateCombination {
                        reason: "execution context must include every selected skill dependency",
                    });
                }
            }
            for capability_id in skill.required_capability_ids() {
                if !self.approved_capability_ids.contains(capability_id) {
                    return Err(ValidationError::InvalidStateCombination {
                        reason: "every selected skill capability must be approved",
                    });
                }
            }
        }

        let policy =
            catalog
                .policy(&self.policy_id)
                .ok_or_else(|| ValidationError::MissingDefinition {
                    kind: "policy",
                    id: self.policy_id.to_string(),
                })?;
        for capability_id in &self.approved_capability_ids {
            if policy.denied_capability_ids().contains(capability_id)
                || !policy.allowed_capability_ids().contains(capability_id)
            {
                return Err(ValidationError::ConflictingRelationship {
                    field: "approved_capability_ids",
                });
            }
        }
        Ok(())
    }

    /// Returns the schema version carried by this IR.
    #[must_use]
    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    /// Returns the execution-context identity.
    #[must_use]
    pub fn id(&self) -> &ExecutionContextId {
        &self.id
    }

    /// Returns the task descriptor.
    #[must_use]
    pub fn task(&self) -> &TaskDescriptor {
        &self.task
    }

    /// Returns the selected workflow identity.
    #[must_use]
    pub fn workflow_id(&self) -> &WorkflowId {
        &self.workflow_id
    }

    /// Returns the selected primary agent identity.
    #[must_use]
    pub fn primary_agent_id(&self) -> &AgentId {
        &self.primary_agent_id
    }

    /// Returns the ordered effective skill identities.
    #[must_use]
    pub fn skill_ids(&self) -> &[SkillId] {
        &self.skill_ids
    }

    /// Returns the operating mode.
    #[must_use]
    pub const fn operating_mode(&self) -> OperatingMode {
        self.operating_mode
    }

    /// Returns the execution profile.
    #[must_use]
    pub const fn execution_profile(&self) -> ExecutionProfile {
        self.execution_profile
    }

    /// Returns the coordinated workflow, gate and blocker state.
    #[must_use]
    pub const fn state(&self) -> ExecutionState {
        self.state
    }

    /// Returns the policy identity used for capability evaluation.
    #[must_use]
    pub fn policy_id(&self) -> &PolicyId {
        &self.policy_id
    }

    /// Returns ordered retrieval requests. Retrieval results are deliberately
    /// not embedded in the IR; they belong to an adapter response.
    #[must_use]
    pub fn knowledge_queries(&self) -> &[KnowledgeQuery] {
        &self.knowledge_queries
    }

    /// Returns the capabilities approved for this execution.
    #[must_use]
    pub fn approved_capability_ids(&self) -> &[CapabilityId] {
        &self.approved_capability_ids
    }

    /// Alias for callers that use the shorter domain term.
    #[must_use]
    pub fn capabilities(&self) -> &[CapabilityId] {
        self.approved_capability_ids()
    }

    /// Returns the ordered constraints applied to this execution.
    #[must_use]
    pub fn constraints(&self) -> &[Constraint] {
        &self.constraints
    }

    /// Returns the provider-independent target runtime identity.
    #[must_use]
    pub fn target_runtime(&self) -> &ExecutionRuntimeId {
        &self.target_runtime
    }

    /// Alias matching the kernel terminology used by the reference IR.
    #[must_use]
    pub fn runtime_id(&self) -> &ExecutionRuntimeId {
        self.target_runtime()
    }
}

/// Compatibility alias for code that spells the acronym as a word.
pub type ExecutionContextIr = ExecutionContextIR;

fn has_duplicate<T>(values: impl IntoIterator<Item = T>) -> bool
where
    T: Eq + std::hash::Hash,
{
    let mut seen = HashSet::new();
    values.into_iter().any(|value| !seen.insert(value))
}

fn collect_skill_closure(
    skill_id: &SkillId,
    catalog: &DefinitionCatalog,
    closure: &mut HashSet<SkillId>,
) -> Result<(), ValidationError> {
    if !closure.insert(skill_id.clone()) {
        return Ok(());
    }
    let skill = catalog
        .skill(skill_id)
        .ok_or_else(|| ValidationError::MissingDefinition {
            kind: "skill",
            id: skill_id.to_string(),
        })?;
    for dependency_id in skill.dependency_ids() {
        collect_skill_closure(dependency_id, catalog, closure)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ExecutionContext, ExecutionContextIR};
    use crate::{
        AgentDefinition, AgentId, BlockerState, CapabilityId, Constraint, ConstraintKind,
        DefinitionCatalog, ExecutionContextId, ExecutionProfile, ExecutionRuntimeId,
        ExecutionState, GateState, KnowledgeQuery, OperatingMode, PolicyDefinition, PolicyId,
        SchemaVersion, SkillDefinition, SkillId, TaskDescriptor, TaskId, ValidationError,
        WorkflowDefinition, WorkflowId, WorkflowState,
    };

    fn ids() -> (
        ExecutionContextId,
        WorkflowId,
        AgentId,
        PolicyId,
        SkillId,
        SkillId,
        CapabilityId,
    ) {
        (
            ExecutionContextId::new("context-1").unwrap(),
            WorkflowId::new("issue-implementation").unwrap(),
            AgentId::new("senior-developer").unwrap(),
            PolicyId::new("safe-development").unwrap(),
            SkillId::new("quality-gate").unwrap(),
            SkillId::new("issue-workflow").unwrap(),
            CapabilityId::new("quality.run").unwrap(),
        )
    }

    fn task() -> TaskDescriptor {
        TaskDescriptor::new(TaskId::new("issue-252").unwrap(), "repair")
            .unwrap()
            .with_classification("runtime_bugfix", 0.94)
            .unwrap()
    }

    fn context() -> ExecutionContextIR {
        let (context_id, workflow_id, agent_id, policy_id, quality, issue, capability) = ids();
        ExecutionContextIR::new_v1(
            context_id,
            task(),
            workflow_id,
            agent_id,
            [quality, issue],
            OperatingMode::Hardening,
            ExecutionProfile::FullPath,
            ExecutionState::new(
                WorkflowState::Running,
                GateState::InProgress,
                BlockerState::Clear,
            )
            .unwrap(),
            policy_id,
            [KnowledgeQuery::new("quality gate history").unwrap()],
            [capability],
            [Constraint::new(
                crate::ConstraintId::new("feature-freeze").unwrap(),
                ConstraintKind::FeatureFreeze,
            )],
            ExecutionRuntimeId::new("runtime-default").unwrap(),
        )
        .unwrap()
    }

    fn catalog() -> DefinitionCatalog {
        let (_, workflow_id, agent_id, policy_id, quality, issue, capability) = ids();
        let issue_skill = SkillDefinition::new(issue.clone(), "Issue workflow", [], []).unwrap();
        let quality_skill = SkillDefinition::new(
            quality.clone(),
            "Run quality checks",
            [issue.clone()],
            [capability.clone()],
        )
        .unwrap();
        let agent =
            AgentDefinition::new(agent_id.clone(), "Implements issues", [quality.clone()]).unwrap();
        let workflow = WorkflowDefinition::new(
            workflow_id,
            "Issue implementation",
            agent_id,
            [quality],
            policy_id.clone(),
        )
        .unwrap();
        let policy = PolicyDefinition::new(policy_id, "Safe development", [capability]).unwrap();
        DefinitionCatalog::new(
            vec![agent],
            vec![issue_skill, quality_skill],
            vec![workflow],
            vec![policy],
        )
        .unwrap()
    }

    #[test]
    fn represents_the_reference_execution_context_without_provider_fields() {
        let context = context();

        assert_eq!(context.schema_version(), SchemaVersion::V1);
        assert_eq!(context.id().as_str(), "context-1");
        assert_eq!(context.task().intent(), "repair");
        assert_eq!(context.task().task_type(), Some("runtime_bugfix"));
        assert_eq!(context.task().confidence().unwrap().as_fraction(), 0.94);
        assert_eq!(context.workflow_id().as_str(), "issue-implementation");
        assert_eq!(context.primary_agent_id().as_str(), "senior-developer");
        assert_eq!(context.skill_ids()[0].as_str(), "quality-gate");
        assert_eq!(context.operating_mode(), OperatingMode::Hardening);
        assert_eq!(context.execution_profile(), ExecutionProfile::FullPath);
        assert_eq!(context.state().gate(), GateState::InProgress);
        assert_eq!(context.policy_id().as_str(), "safe-development");
        assert_eq!(
            context.knowledge_queries()[0].as_str(),
            "quality gate history"
        );
        assert_eq!(context.capabilities()[0].as_str(), "quality.run");
        assert_eq!(
            context.constraints()[0].kind(),
            ConstraintKind::FeatureFreeze
        );
        assert_eq!(context.target_runtime().as_str(), "runtime-default");
    }

    #[test]
    fn accepts_the_v1_alias_and_empty_optional_collections() {
        let (context_id, workflow_id, agent_id, policy_id, quality, _, _) = ids();
        let context = ExecutionContextIR::new(
            SchemaVersion::V1,
            context_id,
            task(),
            workflow_id,
            agent_id,
            [quality],
            OperatingMode::Development,
            ExecutionProfile::FastPath,
            ExecutionState::new(
                WorkflowState::Pending,
                GateState::Pending,
                BlockerState::Clear,
            )
            .unwrap(),
            policy_id,
            [],
            [],
            [],
            ExecutionRuntimeId::new("runtime").unwrap(),
        )
        .unwrap();

        assert!(context.knowledge_queries().is_empty());
        assert!(context.approved_capability_ids().is_empty());
        assert!(context.constraints().is_empty());
        assert_eq!(context.runtime_id().as_str(), "runtime");
    }

    #[test]
    fn keeps_operating_mode_and_execution_profile_independent_in_the_ir() {
        for operating_mode in [
            OperatingMode::Development,
            OperatingMode::Hardening,
            OperatingMode::ReleaseQualification,
        ] {
            for execution_profile in [
                ExecutionProfile::FastPath,
                ExecutionProfile::NormalPath,
                ExecutionProfile::FullPath,
            ] {
                let (_, workflow_id, agent_id, policy_id, quality, _, _) = ids();
                let context = ExecutionContextIR::new_v1(
                    ExecutionContextId::new("context-combination").unwrap(),
                    task(),
                    workflow_id,
                    agent_id,
                    [quality],
                    operating_mode,
                    execution_profile,
                    ExecutionState::new(
                        WorkflowState::Pending,
                        GateState::Pending,
                        BlockerState::Clear,
                    )
                    .unwrap(),
                    policy_id,
                    [],
                    [],
                    [],
                    ExecutionRuntimeId::new("runtime").unwrap(),
                )
                .unwrap();
                assert_eq!(context.operating_mode(), operating_mode);
                assert_eq!(context.execution_profile(), execution_profile);
            }
        }
    }

    #[test]
    fn validates_against_definition_catalog_and_closes_skill_dependencies() {
        context().validate_against(&catalog()).unwrap();
    }

    #[test]
    fn rejects_versions_other_than_v1() {
        let (context_id, workflow_id, agent_id, policy_id, quality, _, _) = ids();
        assert!(matches!(
            ExecutionContextIR::new(
                SchemaVersion::new(1, 1).unwrap(),
                context_id,
                task(),
                workflow_id,
                agent_id,
                [quality],
                OperatingMode::Development,
                ExecutionProfile::FastPath,
                ExecutionState::new(
                    WorkflowState::Pending,
                    GateState::Pending,
                    BlockerState::Clear,
                )
                .unwrap(),
                policy_id,
                [],
                [],
                [],
                ExecutionRuntimeId::new("runtime").unwrap(),
            ),
            Err(ValidationError::UnsupportedSchemaVersion { .. })
        ));
    }

    #[test]
    fn try_new_forwards_all_fields_to_the_same_v1_validation_boundary() {
        let (context_id, workflow_id, agent_id, policy_id, quality, _, _) = ids();
        let context = ExecutionContextIR::try_new(
            SchemaVersion::V1,
            context_id,
            task(),
            workflow_id,
            agent_id,
            [quality],
            OperatingMode::Development,
            ExecutionProfile::FastPath,
            ExecutionState::new(
                WorkflowState::Pending,
                GateState::Pending,
                BlockerState::Clear,
            )
            .unwrap(),
            policy_id,
            [],
            [],
            [],
            ExecutionRuntimeId::new("runtime").unwrap(),
        )
        .unwrap();
        assert_eq!(context.schema_version(), SchemaVersion::V1);
    }

    #[test]
    fn validate_rechecks_local_invariants_at_a_boundary() {
        let mut ir = context();
        ir.schema_version = SchemaVersion::new(1, 1).unwrap();
        assert!(matches!(
            ir.validate(),
            Err(ValidationError::UnsupportedSchemaVersion { .. })
        ));

        let mut ir = context();
        ir.skill_ids.clear();
        assert!(matches!(
            ir.validate(),
            Err(ValidationError::EmptyRelationship { field: "skill_ids" })
        ));

        let mut ir = context();
        let skill = ir.skill_ids[0].clone();
        ir.skill_ids.push(skill);
        assert!(matches!(
            ir.validate(),
            Err(ValidationError::DuplicateRelationship { field: "skill_ids" })
        ));

        let mut ir = context();
        let capability = ir.approved_capability_ids[0].clone();
        ir.approved_capability_ids.push(capability);
        assert!(matches!(
            ir.validate(),
            Err(ValidationError::DuplicateRelationship {
                field: "approved_capability_ids"
            })
        ));

        let mut ir = context();
        let constraint_id = ir.constraints[0].id().clone();
        ir.constraints.push(Constraint::new(
            constraint_id,
            ConstraintKind::LiveMutationRequiresConsent,
        ));
        assert!(matches!(
            ir.validate(),
            Err(ValidationError::DuplicateRelationship {
                field: "constraints"
            })
        ));
    }

    #[test]
    fn rejects_empty_and_duplicate_semantic_relationships() {
        let (context_id, workflow_id, agent_id, policy_id, quality, issue, capability) = ids();
        let state = ExecutionState::new(
            WorkflowState::Pending,
            GateState::Pending,
            BlockerState::Clear,
        )
        .unwrap();
        assert!(matches!(
            ExecutionContextIR::new(
                SchemaVersion::V1,
                context_id.clone(),
                task(),
                workflow_id.clone(),
                agent_id.clone(),
                [],
                OperatingMode::Development,
                ExecutionProfile::FastPath,
                state,
                policy_id.clone(),
                [],
                [],
                [],
                ExecutionRuntimeId::new("runtime").unwrap(),
            ),
            Err(ValidationError::EmptyRelationship { field: "skill_ids" })
        ));
        assert!(matches!(
            ExecutionContextIR::new(
                SchemaVersion::V1,
                context_id,
                task(),
                workflow_id,
                agent_id,
                [quality.clone(), issue, quality],
                OperatingMode::Development,
                ExecutionProfile::FastPath,
                state,
                policy_id,
                [],
                [capability.clone(), capability],
                [],
                ExecutionRuntimeId::new("runtime").unwrap(),
            ),
            Err(ValidationError::DuplicateRelationship { field: "skill_ids" })
        ));
    }

    #[test]
    fn rejects_duplicate_constraint_ids_even_when_kinds_differ() {
        let (context_id, workflow_id, agent_id, policy_id, quality, _, _) = ids();
        let id = crate::ConstraintId::new("same-constraint").unwrap();
        let result = ExecutionContextIR::new_v1(
            context_id,
            task(),
            workflow_id,
            agent_id,
            [quality],
            OperatingMode::Development,
            ExecutionProfile::FastPath,
            ExecutionState::new(
                WorkflowState::Pending,
                GateState::Pending,
                BlockerState::Clear,
            )
            .unwrap(),
            policy_id,
            [],
            [],
            [
                Constraint::new(id.clone(), ConstraintKind::FeatureFreeze),
                Constraint::new(id, ConstraintKind::LiveMutationRequiresConsent),
            ],
            ExecutionRuntimeId::new("runtime").unwrap(),
        );
        assert!(matches!(
            result,
            Err(ValidationError::DuplicateRelationship {
                field: "constraints"
            })
        ));
    }

    #[test]
    fn rejects_a_constraint_that_conflicts_with_the_selected_mode_and_profile() {
        let (context_id, workflow_id, agent_id, policy_id, quality, _, _) = ids();
        let result = ExecutionContextIR::new_v1(
            context_id,
            task(),
            workflow_id,
            agent_id,
            [quality],
            OperatingMode::ReleaseQualification,
            ExecutionProfile::NormalPath,
            ExecutionState::new(
                WorkflowState::Pending,
                GateState::Pending,
                BlockerState::Clear,
            )
            .unwrap(),
            policy_id,
            [],
            [],
            [Constraint::new(
                crate::ConstraintId::new("release-depth").unwrap(),
                ConstraintKind::RequireFullPathForReleaseQualification,
            )],
            ExecutionRuntimeId::new("runtime").unwrap(),
        );
        assert!(matches!(
            result,
            Err(ValidationError::InvalidStateCombination { .. })
        ));
    }

    #[test]
    fn rejects_invalid_catalog_compositions() {
        let mut context = context();
        assert!(matches!(context.validate_against(&catalog()), Ok(())));

        context.primary_agent_id = AgentId::new("other-agent").unwrap();
        assert!(matches!(
            context.validate_against(&catalog()),
            Err(ValidationError::InvalidStateCombination { .. })
        ));
    }

    #[test]
    fn rejects_capabilities_not_authorized_by_the_selected_policy() {
        let mut context = context();
        context
            .approved_capability_ids
            .push(CapabilityId::new("runtime.inspect").unwrap());

        assert!(matches!(
            context.validate_against(&catalog()),
            Err(ValidationError::ConflictingRelationship {
                field: "approved_capability_ids"
            })
        ));
    }

    #[test]
    fn rejects_required_capabilities_that_are_not_approved() {
        let mut context = context();
        context.approved_capability_ids.clear();

        assert!(matches!(
            context.validate_against(&catalog()),
            Err(ValidationError::InvalidStateCombination { .. })
        ));
    }

    #[test]
    fn rejects_missing_workflow_skill_and_policy_references() {
        let mut ir = context();
        ir.workflow_id = WorkflowId::new("missing-workflow").unwrap();
        assert!(matches!(
            ir.validate_against(&catalog()),
            Err(ValidationError::MissingDefinition {
                kind: "workflow",
                ..
            })
        ));

        let mut ir = context();
        ir.skill_ids
            .retain(|skill| skill.as_str() != "quality-gate");
        assert!(matches!(
            ir.validate_against(&catalog()),
            Err(ValidationError::InvalidStateCombination { .. })
        ));

        let mut ir = context();
        ir.policy_id = PolicyId::new("missing-policy").unwrap();
        assert!(matches!(
            ir.validate_against(&catalog()),
            Err(ValidationError::MissingDefinition { kind: "policy", .. })
        ));
    }

    #[test]
    fn rejects_an_unknown_selected_skill_reference() {
        let mut context = context();
        context
            .skill_ids
            .push(SkillId::new("unknown-skill").unwrap());

        assert!(matches!(
            context.validate_against(&catalog()),
            Err(ValidationError::MissingDefinition { kind: "skill", .. })
        ));
    }

    #[test]
    fn rejects_an_incomplete_effective_skill_dependency_closure() {
        let mut context = context();
        context
            .skill_ids
            .retain(|skill| skill.as_str() != "issue-workflow");

        assert!(matches!(
            context.validate_against(&catalog()),
            Err(ValidationError::InvalidStateCombination { .. })
        ));
    }

    #[test]
    fn keeps_the_legacy_context_value_available() {
        let legacy = ExecutionContext {
            task: task(),
            operating_mode: OperatingMode::Hardening,
            execution_profile: ExecutionProfile::FullPath,
        };
        assert_eq!(legacy.task.id().as_str(), "issue-252");
    }
}
