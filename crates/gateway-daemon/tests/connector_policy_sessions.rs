//! Installed policy denials use the shipped CLI/MCP composition and real stores.
use std::{path::Path, process::Command};

fn run(case: &str) {
    if std::env::var_os("CG_COGNITIVE_TEST_DATABASE").is_none() {
        return;
    }
    let output = Command::new("python3")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/connector_policy_session.py"),
        )
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
        String::from_utf8_lossy(&output.stdout).contains("INSTALLED_POLICY_QUALIFICATION_COMPLETE")
    );
}

#[test]
fn installed_explicit_native_policy_denial_sends_zero() {
    run("policy-denied");
}
#[test]
fn installed_missing_native_evidence_sends_zero() {
    run("missing-evidence");
}
#[test]
fn installed_mapping_consent_requirement_sends_zero() {
    run("mapping-consent");
}
#[test]
fn installed_global_consent_requirement_sends_zero() {
    run("global-consent");
}
#[test]
fn installed_declared_consent_fact_cannot_replace_live_receipt() {
    run("forged-consent");
}
#[test]
fn installed_mutation_cannot_inherit_read_authority() {
    run("mutate-as-read");
}
#[test]
fn installed_admin_cannot_inherit_read_authority() {
    run("admin-as-read");
}
#[test]
fn installed_consent_change_after_admission_sends_zero() {
    run("late-consent");
}
#[test]
fn installed_policy_change_after_admission_sends_zero() {
    run("late-policy");
}
#[test]
fn installed_tool_change_after_admission_sends_zero() {
    run("late-inventory");
}
