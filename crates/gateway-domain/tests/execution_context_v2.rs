use gateway_domain::{ExecutionContextIRV2, ExecutionProjectionIssue, ExecutionProjectionStatus};

#[test]
fn v2_preserves_non_projectable_shapes_without_granting_execution() {
    let mut context = ExecutionContextIRV2::new(
        "handoff-1",
        "basis-1",
        ExecutionProjectionStatus::MultipleAgents,
    )
    .unwrap();
    context.participating_agent_ids = vec!["agent-a".into(), "agent-b".into()];
    context.issues = vec![ExecutionProjectionIssue::MultipleAgents];
    context.validate().unwrap();
    let json = context.to_json().unwrap();
    let restored = ExecutionContextIRV2::from_json(&json).unwrap();
    assert_eq!(restored, context);
    assert_ne!(restored.status, ExecutionProjectionStatus::Executable);
}

#[test]
fn v2_executable_profile_requires_complete_fields_and_rejects_tampering() {
    let context = ExecutionContextIRV2::new(
        "handoff-2",
        "basis-2",
        ExecutionProjectionStatus::Executable,
    )
    .unwrap_err();
    assert!(context.to_string().contains("workflow"));

    let json = r#"{"schema_version":"2.0","id":"h","task":null,"workflow_id":null,"primary_agent_id":null,"participating_agent_ids":[],"skill_ids":[],"operating_mode":"HARDENING","execution_profile":"FULL_PATH","state":null,"policy_id":null,"approved_capability_ids":[],"constraints":[],"target_runtime":null,"resolution_basis":"b","status":"EXECUTABLE","issues":[]}"#;
    assert!(ExecutionContextIRV2::from_json(json).is_err());
}

#[test]
fn v2_rejects_unknown_fields_and_wrong_versions() {
    let json = r#"{"schema_version":"2.1","id":"h","task":null,"workflow_id":null,"primary_agent_id":null,"participating_agent_ids":[],"skill_ids":[],"operating_mode":"HARDENING","execution_profile":"FULL_PATH","state":null,"policy_id":null,"approved_capability_ids":[],"constraints":[],"target_runtime":null,"resolution_basis":"b","status":"NO_TEMPLATE","issues":[],"extra":true}"#;
    assert!(ExecutionContextIRV2::from_json(json).is_err());
}
