//! Installed cg-mcp session qualification with actual PostgreSQL and stdio MCP.
use std::{path::Path, process::Command};
fn run(case: &str) {
    if std::env::var_os("CG_COGNITIVE_TEST_DATABASE").is_none() {
        return;
    }
    let output = Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/observed_goal_session.py"))
        .arg(case)
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .env(
            "CG_QUALIFICATION_BIN_DIR",
            Path::new(env!("CARGO_BIN_EXE_cg-mcp")).parent().unwrap(),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("INSTALLED_GOAL_QUALIFICATION_COMPLETE"),
        "actual installed completion marker absent"
    );
}
#[test]
fn installed_native_goal_reads_two_conditions_once_without_model() {
    run("one");
}
#[test]
fn installed_native_goal_reads_two_objects_with_same_mapping_and_capability() {
    run("two");
}

#[test]
fn installed_goal_marker_recovers_after_completion_storage_failure_without_more_reads() {
    run("recovery");
}
#[test]
fn installed_full_goal_false_does_not_complete() {
    run("false");
}
#[test]
fn installed_retained_goal_rejects_normalization_drift_without_more_reads() {
    run("replay-drift");
}

#[test]
fn installed_current_authority_revocation_before_read_sends_zero_tool_calls() {
    run("authority-revoked");
}
#[test]
fn installed_concurrent_cancel_changes_fence_before_read_and_sends_zero_tool_calls() {
    run("cancelled");
}
