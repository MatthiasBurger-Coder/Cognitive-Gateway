//! Real shipped-binary canonical acceptance; preserves instrumentation in child CG only.
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[test]
fn shipped_host_canonical_operations_and_authority_negatives() {
    let bin_dir = std::env::var_os("CG_QUALIFICATION_BIN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_BIN_EXE_cg-mcp"))
                .parent()
                .unwrap()
                .into()
        });
    let output = Command::new("python3")
        .args([
            "-m",
            "unittest",
            "discover",
            "-s",
            "tests/codex-local",
            "-p",
            "test_canonical_host.py",
            "-v",
        ])
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .env("CG_QUALIFICATION_BIN_DIR", bin_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
