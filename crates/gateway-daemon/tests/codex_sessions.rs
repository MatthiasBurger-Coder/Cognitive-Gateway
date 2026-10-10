//! Actual CLI/MCP session transitions against the quality runner's PostgreSQL host.
use std::{path::Path, process::Command};
#[test]
fn shipped_shared_sessions_are_durable_verified_and_fenced() {
    // The mandatory runtime gate supplies this host; ordinary offline cargo tests
    // cannot qualify PostgreSQL recovery. No fake durable adapter is substituted.
    if std::env::var_os("CG_COGNITIVE_TEST_DATABASE").is_none() {
        return;
    }
    let output = Command::new("python3")
        .args([
            "-m",
            "unittest",
            "discover",
            "-s",
            "tests/codex-local",
            "-p",
            "test_sessions.py",
            "-v",
        ])
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
}
