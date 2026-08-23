//! Executable reference scenarios for the CG-02 domain acceptance slice.
//!
//! These fixtures intentionally use only domain values. A runtime adapter is
//! not needed to prove that the gateway can represent, validate and serialize
//! an execution decision.

use gateway_domain::{
    AgentDefinition, AgentId, BlockerState, CapabilityClass, CapabilityDefinition, CapabilityId,
    Constraint, ConstraintId, ConstraintKind, DefinitionCatalog, ExecutionContextIR,
    ExecutionContextId, ExecutionProfile, ExecutionRuntimeId, ExecutionState, GateState,
    KnowledgeQuery, OperatingMode, PolicyDefinition, PolicyId, SchemaVersion, SkillDefinition,
    SkillId, TaskDescriptor, TaskId, ValidationError, WorkflowDefinition, WorkflowId,
    WorkflowState,
};

const ENGINE_SKILL: &str = "docker-engine-installation";
const SWARM_SKILL: &str = "docker-swarm-initialization";
const EVIDENCE_SKILL: &str = "live-evidence-validation";

fn reference_catalog(policy: PolicyDefinition) -> DefinitionCatalog {
    let agent_id = AgentId::new("senior-devops").unwrap();
    let engine = SkillDefinition::new(
        SkillId::new(ENGINE_SKILL).unwrap(),
        "Inspect and prepare the Docker engine",
        [],
        [CapabilityId::new("runtime.inspect_docker").unwrap()],
    )
    .unwrap()
    .with_owner(agent_id.clone())
    .with_knowledge_queries([KnowledgeQuery::new("docker worker ready failures").unwrap()]);
    let swarm = SkillDefinition::new(
        SkillId::new(SWARM_SKILL).unwrap(),
        "Inspect and initialize the Docker Swarm",
        [SkillId::new(ENGINE_SKILL).unwrap()],
        [CapabilityId::new("runtime.inspect_swarm").unwrap()],
    )
    .unwrap()
    .with_owner(agent_id.clone())
    .with_knowledge_queries([KnowledgeQuery::new("swarm initialization").unwrap()]);
    let evidence = SkillDefinition::new(
        SkillId::new(EVIDENCE_SKILL).unwrap(),
        "Run and record runtime verification evidence",
        [],
        [CapabilityId::new("quality.run_runtime_checks").unwrap()],
    )
    .unwrap()
    .with_owner(agent_id.clone())
    .with_knowledge_queries([KnowledgeQuery::new("previous worker startup incidents").unwrap()]);

    DefinitionCatalog::new(
        vec![
            AgentDefinition::new(
                agent_id,
                "Owns runtime installation, networking and release evidence",
                [
                    SkillId::new(ENGINE_SKILL).unwrap(),
                    SkillId::new(SWARM_SKILL).unwrap(),
                    SkillId::new(EVIDENCE_SKILL).unwrap(),
                ],
            )
            .unwrap(),
        ],
        vec![engine, swarm, evidence],
        vec![
            WorkflowDefinition::new(
                WorkflowId::new("classic-rc1").unwrap(),
                "Qualify a classic release candidate",
                AgentId::new("senior-devops").unwrap(),
                [
                    SkillId::new(ENGINE_SKILL).unwrap(),
                    SkillId::new(SWARM_SKILL).unwrap(),
                    SkillId::new(EVIDENCE_SKILL).unwrap(),
                ],
                PolicyId::new("classic-rc1-policy").unwrap(),
            )
            .unwrap(),
        ],
        vec![policy],
    )
    .unwrap()
}

fn reference_policy() -> PolicyDefinition {
    PolicyDefinition::with_denied_capabilities(
        PolicyId::new("classic-rc1-policy").unwrap(),
        "Release qualification permits inspection and runtime checks",
        [
            CapabilityId::new("runtime.inspect_docker").unwrap(),
            CapabilityId::new("runtime.inspect_swarm").unwrap(),
            CapabilityId::new("quality.run_runtime_checks").unwrap(),
        ],
        [CapabilityId::new("runtime.mutate").unwrap()],
    )
    .unwrap()
}

fn reference_task() -> TaskDescriptor {
    TaskDescriptor::new(TaskId::new("issue-252").unwrap(), "repair")
        .unwrap()
        .with_classification("runtime_bugfix", 0.94)
        .unwrap()
}

fn reference_constraints() -> [Constraint; 2] {
    [
        Constraint::new(
            ConstraintId::new("feature-freeze").unwrap(),
            ConstraintKind::FeatureFreeze,
        ),
        Constraint::new(
            ConstraintId::new("live-mutation-consent").unwrap(),
            ConstraintKind::LiveMutationRequiresConsent,
        ),
    ]
}

fn reference_capabilities() -> [CapabilityId; 3] {
    [
        CapabilityId::new("runtime.inspect_docker").unwrap(),
        CapabilityId::new("runtime.inspect_swarm").unwrap(),
        CapabilityId::new("quality.run_runtime_checks").unwrap(),
    ]
}

fn reference_context(
    state: ExecutionState,
    operating_mode: OperatingMode,
    execution_profile: ExecutionProfile,
    approved_capability_ids: impl IntoIterator<Item = CapabilityId>,
    constraints: impl IntoIterator<Item = Constraint>,
) -> ExecutionContextIR {
    ExecutionContextIR::new_v1(
        ExecutionContextId::new("context-issue-252").unwrap(),
        reference_task(),
        WorkflowId::new("classic-rc1").unwrap(),
        AgentId::new("senior-devops").unwrap(),
        [
            SkillId::new(ENGINE_SKILL).unwrap(),
            SkillId::new(SWARM_SKILL).unwrap(),
            SkillId::new(EVIDENCE_SKILL).unwrap(),
        ],
        operating_mode,
        execution_profile,
        state,
        PolicyId::new("classic-rc1-policy").unwrap(),
        [
            KnowledgeQuery::new("docker worker ready failures").unwrap(),
            KnowledgeQuery::new("swarm initialization").unwrap(),
            KnowledgeQuery::new("previous worker startup incidents").unwrap(),
        ],
        approved_capability_ids,
        constraints,
        ExecutionRuntimeId::new("codex").unwrap(),
    )
    .unwrap()
}

fn complete_reference_context() -> ExecutionContextIR {
    reference_context(
        ExecutionState::new(
            WorkflowState::Running,
            GateState::InProgress,
            BlockerState::Clear,
        )
        .unwrap(),
        OperatingMode::Hardening,
        ExecutionProfile::FullPath,
        [
            CapabilityId::new("runtime.inspect_docker").unwrap(),
            CapabilityId::new("runtime.inspect_swarm").unwrap(),
            CapabilityId::new("quality.run_runtime_checks").unwrap(),
        ],
        reference_constraints(),
    )
}

#[test]
fn epic_reference_context_preserves_every_domain_decision() {
    let context = complete_reference_context();
    context
        .validate_against(&reference_catalog(reference_policy()))
        .unwrap();

    assert_eq!(context.schema_version(), SchemaVersion::V1);
    assert_eq!(context.id().as_str(), "context-issue-252");
    assert_eq!(context.task().id().as_str(), "issue-252");
    assert_eq!(context.task().intent(), "repair");
    assert_eq!(context.task().task_type(), Some("runtime_bugfix"));
    assert_eq!(context.task().confidence().unwrap().as_fraction(), 0.94);
    assert_eq!(context.workflow_id().as_str(), "classic-rc1");
    assert_eq!(context.primary_agent_id().as_str(), "senior-devops");
    assert_eq!(
        context
            .skill_ids()
            .iter()
            .map(SkillId::as_str)
            .collect::<Vec<_>>(),
        vec![ENGINE_SKILL, SWARM_SKILL, EVIDENCE_SKILL]
    );
    assert_eq!(context.operating_mode(), OperatingMode::Hardening);
    assert_eq!(context.execution_profile(), ExecutionProfile::FullPath);
    assert_eq!(context.state().workflow(), WorkflowState::Running);
    assert_eq!(context.state().gate(), GateState::InProgress);
    assert_eq!(context.state().blocker(), BlockerState::Clear);
    assert_eq!(context.policy_id().as_str(), "classic-rc1-policy");
    assert_eq!(
        context
            .knowledge_queries()
            .iter()
            .map(KnowledgeQuery::as_str)
            .collect::<Vec<_>>(),
        vec![
            "docker worker ready failures",
            "swarm initialization",
            "previous worker startup incidents"
        ]
    );
    assert_eq!(
        context
            .approved_capability_ids()
            .iter()
            .map(CapabilityId::as_str)
            .collect::<Vec<_>>(),
        vec![
            "runtime.inspect_docker",
            "runtime.inspect_swarm",
            "quality.run_runtime_checks"
        ]
    );
    assert_eq!(context.constraints()[0].id().as_str(), "feature-freeze");
    assert_eq!(
        context.constraints()[0].kind(),
        ConstraintKind::FeatureFreeze
    );
    assert_eq!(
        context.constraints()[1].kind(),
        ConstraintKind::LiveMutationRequiresConsent
    );
    assert_eq!(context.target_runtime().as_str(), "codex");
}

#[test]
fn all_operating_mode_and_execution_profile_pairs_are_valid() {
    let modes = [
        OperatingMode::Development,
        OperatingMode::Hardening,
        OperatingMode::ReleaseQualification,
    ];
    let profiles = [
        ExecutionProfile::FastPath,
        ExecutionProfile::NormalPath,
        ExecutionProfile::FullPath,
    ];
    let catalog = reference_catalog(reference_policy());

    for mode in modes {
        for profile in profiles {
            let context = reference_context(
                ExecutionState::new(
                    WorkflowState::Pending,
                    GateState::Pending,
                    BlockerState::Clear,
                )
                .unwrap(),
                mode,
                profile,
                reference_capabilities(),
                [],
            );
            context.validate_against(&catalog).unwrap();
            assert_eq!(context.operating_mode(), mode);
            assert_eq!(context.execution_profile(), profile);
        }
    }
}

#[test]
fn representative_state_scenarios_remain_valid_in_the_complete_ir() {
    let catalog = reference_catalog(reference_policy());
    let scenarios = [
        (
            "pending",
            ExecutionState::new(
                WorkflowState::Pending,
                GateState::Pending,
                BlockerState::Clear,
            )
            .unwrap(),
        ),
        (
            "running",
            ExecutionState::new(
                WorkflowState::Running,
                GateState::InProgress,
                BlockerState::Clear,
            )
            .unwrap(),
        ),
        (
            "paused",
            ExecutionState::new(
                WorkflowState::Paused,
                GateState::Pending,
                BlockerState::Resolved,
            )
            .unwrap(),
        ),
        (
            "blocked",
            ExecutionState::new(
                WorkflowState::Blocked,
                GateState::Blocked,
                BlockerState::Active,
            )
            .unwrap(),
        ),
        (
            "completed",
            ExecutionState::new(
                WorkflowState::Completed,
                GateState::Passed,
                BlockerState::Clear,
            )
            .unwrap(),
        ),
        (
            "failed",
            ExecutionState::new(
                WorkflowState::Failed,
                GateState::Failed,
                BlockerState::Resolved,
            )
            .unwrap(),
        ),
        (
            "cancelled",
            ExecutionState::new(
                WorkflowState::Cancelled,
                GateState::Skipped,
                BlockerState::Resolved,
            )
            .unwrap(),
        ),
    ];

    for (name, state) in scenarios {
        let context = reference_context(
            state,
            OperatingMode::Hardening,
            ExecutionProfile::FullPath,
            reference_capabilities(),
            [],
        );
        context
            .validate_against(&catalog)
            .unwrap_or_else(|error| panic!("{name} reference state should validate: {error}"));
    }
}

#[test]
fn capability_classes_and_constraints_keep_authority_separate_from_knowledge() {
    let inspect = CapabilityDefinition::new(
        CapabilityId::new("runtime.inspect_swarm").unwrap(),
        CapabilityClass::Inspect,
    );
    let mutate = CapabilityDefinition::new(
        CapabilityId::new("runtime.mutate").unwrap(),
        CapabilityClass::Mutate,
    );

    assert!(!inspect.requires_mutation_policy());
    assert!(mutate.requires_mutation_policy());
    assert_eq!(inspect.id().as_str(), "runtime.inspect_swarm");
    assert_eq!(mutate.id().as_str(), "runtime.mutate");
    let context = complete_reference_context();
    assert_eq!(context.knowledge_queries().len(), 3);
    assert_eq!(context.approved_capability_ids().len(), 3);
    assert_eq!(context.constraints().len(), 2);
}

#[test]
fn invalid_authority_and_dimension_combinations_fail_closed() {
    assert!(matches!(
        ExecutionState::new(
            WorkflowState::Completed,
            GateState::Failed,
            BlockerState::Clear,
        ),
        Err(ValidationError::InvalidStateCombination { .. })
    ));

    let catalog = reference_catalog(reference_policy());
    let missing_capability = reference_context(
        ExecutionState::new(
            WorkflowState::Pending,
            GateState::Pending,
            BlockerState::Clear,
        )
        .unwrap(),
        OperatingMode::Development,
        ExecutionProfile::FastPath,
        [],
        [],
    );
    assert!(matches!(
        missing_capability.validate_against(&catalog),
        Err(ValidationError::InvalidStateCombination { .. })
    ));

    let denied_policy = PolicyDefinition::with_denied_capabilities(
        PolicyId::new("classic-rc1-policy").unwrap(),
        "Only Docker inspection is allowed",
        [CapabilityId::new("runtime.inspect_docker").unwrap()],
        [CapabilityId::new("runtime.inspect_swarm").unwrap()],
    )
    .unwrap();
    let denied = complete_reference_context();
    assert!(matches!(
        denied.validate_against(&reference_catalog(denied_policy)),
        Err(ValidationError::ConflictingRelationship {
            field: "approved_capability_ids"
        })
    ));

    let release_depth = ExecutionContextIR::new_v1(
        ExecutionContextId::new("invalid-release-depth").unwrap(),
        reference_task(),
        WorkflowId::new("classic-rc1").unwrap(),
        AgentId::new("senior-devops").unwrap(),
        [SkillId::new(ENGINE_SKILL).unwrap()],
        OperatingMode::ReleaseQualification,
        ExecutionProfile::NormalPath,
        ExecutionState::new(
            WorkflowState::Pending,
            GateState::Pending,
            BlockerState::Clear,
        )
        .unwrap(),
        PolicyId::new("classic-rc1-policy").unwrap(),
        [],
        [],
        [Constraint::new(
            ConstraintId::new("release-depth").unwrap(),
            ConstraintKind::RequireFullPathForReleaseQualification,
        )],
        ExecutionRuntimeId::new("codex").unwrap(),
    );
    assert!(matches!(
        release_depth,
        Err(ValidationError::InvalidStateCombination { .. })
    ));
}

#[test]
fn complete_context_round_trip_is_deterministic_and_provider_neutral() {
    let original = complete_reference_context();
    let compact = original.to_json().unwrap();
    let pretty = original.to_json_pretty().unwrap();

    assert_eq!(ExecutionContextIR::from_json(&compact).unwrap(), original);
    assert_eq!(ExecutionContextIR::from_json(&pretty).unwrap(), original);
    assert_eq!(
        ExecutionContextIR::from_json(&compact)
            .unwrap()
            .to_json()
            .unwrap(),
        compact
    );
    assert!(compact.contains("runtime.inspect_docker"));
    assert!(compact.contains("LIVE_MUTATION_REQUIRES_CONSENT"));
    assert!(!compact.contains("provider"));
    assert!(!compact.contains("model"));
    assert!(!compact.contains("prompt"));
}

#[test]
fn malformed_reference_payloads_are_rejected_without_coercion() {
    let original = complete_reference_context();
    let mut value: serde_json::Value = serde_json::from_str(&original.to_json().unwrap()).unwrap();

    value["operating_mode"] = serde_json::json!("production");
    assert!(ExecutionContextIR::from_json(&value.to_string()).is_err());

    value["operating_mode"] = serde_json::json!("HARDENING");
    value["state"]["gate_state"] = serde_json::json!("UNKNOWN");
    assert!(ExecutionContextIR::from_json(&value.to_string()).is_err());

    value["state"]["gate_state"] = serde_json::json!("IN_PROGRESS");
    value["schema_version"] = serde_json::json!("1.1");
    assert!(matches!(
        ExecutionContextIR::from_json(&value.to_string()),
        Err(gateway_domain::SerializationError::Validation(
            ValidationError::UnsupportedSchemaVersion { .. }
        ))
    ));

    value["schema_version"] = serde_json::json!("1.0");
    value["skill_ids"] = serde_json::json!([ENGINE_SKILL, ENGINE_SKILL]);
    assert!(matches!(
        ExecutionContextIR::from_json(&value.to_string()),
        Err(gateway_domain::SerializationError::Validation(
            ValidationError::DuplicateRelationship { field: "skill_ids" }
        ))
    ));

    value["skill_ids"] = serde_json::json!([ENGINE_SKILL, SWARM_SKILL, EVIDENCE_SKILL]);
    value["id"] = serde_json::json!("../escape");
    assert!(ExecutionContextIR::from_json(&value.to_string()).is_err());
}
