//! #408 actual installed native session + credentialed atomic stdio provider + PG.
use std::{path::Path, process::Command};
fn run(case: &str) {
    if std::env::var_os("CG_COGNITIVE_TEST_DATABASE").is_none() {
        return;
    }
    let result = Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/complete_before_session.py"))
        .arg(case)
        .env(
            "CG_QUALIFICATION_BIN_DIR",
            Path::new(env!("CARGO_BIN_EXE_cg-mcp")).parent().unwrap(),
        )
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8_lossy(&result.stdout).contains("INSTALLED_COMPLETE_BEFORE_QUALIFIED")
    );
}
#[test]
fn installed_atomic_world_retains_three_same_source_objects_including_non_goal_data() {
    run("world");
}

#[test]
fn installed_atomic_world_denies_partial_foreign_inconsistent_and_oversized_exports() {
    for case in [
        "membership",
        "epoch",
        "version",
        "object",
        "identity",
        "missing",
        "extra",
        "type",
        "oversized",
    ] {
        run(case);
    }
}
#[test]
fn installed_atomic_world_enforces_live_authority_budgets_claims_and_atomic_commit() {
    for case in [
        "budget",
        "schema-drift",
        "credential",
        "commit-failure",
        "revoke",
        "cancel",
        "config-drift",
        "quote-drift",
        "deadline",
    ] {
        run(case);
    }
}
#[test]
fn installed_already_satisfied_goal_preserves_goal_path_without_world_export() {
    run("satisfied");
}

#[test]
fn installed_before_remains_historical_after_freshness_expires_without_reacquisition() {
    run("age");
}
