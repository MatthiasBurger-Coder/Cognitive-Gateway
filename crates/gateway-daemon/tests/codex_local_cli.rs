use serde_json::{Value, json};
use std::{path::PathBuf, process::Command};

struct Setup {
    root: PathBuf,
    launch: Vec<String>,
}
impl Setup {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "cg-local-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let output = Command::new("python3")
            .args([
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../scripts/bootstrap-codex-local.py"
                ),
                "--repository",
                env!("CARGO_MANIFEST_DIR"),
                "--output",
                root.to_str().unwrap(),
                "--bin-dir",
                PathBuf::from(env!("CARGO_BIN_EXE_cg-local"))
                    .parent()
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "--client-name",
                "codex",
                "--client-version",
                "1.0",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let launch =
            serde_json::from_str(&std::fs::read_to_string(root.join("launch.json")).unwrap())
                .unwrap();
        Self { root, launch }
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cg-local"));
        command.env_clear();
        if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", profile);
        }
        command.args(args).args(&self.launch).output().unwrap()
    }
}
impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn bootstrap_check_and_headless_protocol_parity() {
    let setup = Setup::new();
    let checked = setup.run(&["--check"]);
    assert!(checked.status.success());
    let health: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(health["canonical_scope"], "canonical-example");
    assert_eq!(health["mcp_protocol_version"], "2025-11-25");
    assert_eq!(health["mutations_enabled"], false);
    let smoke = Command::new("python3")
        .args([
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../scripts/check-codex-local.py"
            ),
            "--setup",
            setup.root.to_str().unwrap(),
            "--bin-dir",
            PathBuf::from(env!("CARGO_BIN_EXE_cg-local"))
                .parent()
                .unwrap()
                .to_str()
                .unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        smoke.status.success(),
        "{}",
        String::from_utf8_lossy(&smoke.stderr)
    );
}

#[test]
fn cli_keeps_facade_scope_policy_and_classification_gates() {
    let setup = Setup::new();
    let file = setup.root.join("request.json");
    let original: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    for (field, expected) in [
        ("scope", "CG_SCOPE_DENIED"),
        ("policy", "CG_POLICY_DENIED"),
        ("inline", "CG_SENSITIVITY_DENIED"),
        ("operation", "CG_INVALID_REQUEST"),
    ] {
        let mut request = original.clone();
        match field {
            "scope" => request["scope"]["project_id"] = json!("foreign"),
            "policy" => request["execution"]["execution_profile"] = json!("FAST_PATH"),
            "inline" => {
                request["input"]["situation"] = json!({"kind":"document","contract":"cg.situation","contract_version":"1.0","document":{}})
            }
            _ => request["operation"] = json!("session.start"),
        }
        std::fs::write(&file, request.to_string()).unwrap();
        let result = setup.run(&[
            "--operation",
            "situation.inspect",
            "--request",
            file.to_str().unwrap(),
        ]);
        assert_eq!(result.status.code(), Some(1), "{field}");
        let response: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(response["diagnostics"][0]["code"], expected, "{field}");
    }
}

#[test]
fn cli_rejects_bad_setup_and_credentials_without_echo() {
    let mut setup = Setup::new();
    let help = Command::new(env!("CARGO_BIN_EXE_cg-local"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--check"));
    for args in [
        vec!["--check", "--unknown", "DO_NOT_ECHO"],
        vec!["--check", "--principal", "DO_NOT_ECHO"],
        vec!["--operation", "situation.inspect"],
        vec!["--check", "--request"],
    ] {
        let output = setup.run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("DO_NOT_ECHO"));
    }
    let file = setup.root.join("request.json");
    let missing = setup.run(&[
        "--operation",
        "situation.inspect",
        "--request",
        "/nonexistent-cg-local-request",
    ]);
    assert_eq!(missing.status.code(), Some(2));
    for bytes in [vec![b' '; 1_048_576], vec![0xff]] {
        std::fs::write(&file, bytes).unwrap();
        assert_eq!(
            setup
                .run(&[
                    "--operation",
                    "situation.inspect",
                    "--request",
                    file.to_str().unwrap()
                ])
                .status
                .code(),
            Some(2)
        );
    }
    // Reading a directory fails after opening on POSIX, without echoing its path.
    #[cfg(unix)]
    assert_eq!(
        setup
            .run(&[
                "--operation",
                "situation.inspect",
                "--request",
                setup.root.to_str().unwrap()
            ])
            .status
            .code(),
        Some(2)
    );
    for contents in [
        "{\"x\":1,\"x\":2}",
        "{\"api_key\":\"DO_NOT_ECHO\"}",
        "{",
        "[]",
    ] {
        std::fs::write(&file, contents).unwrap();
        let output = setup.run(&[
            "--operation",
            "situation.inspect",
            "--request",
            file.to_str().unwrap(),
        ]);
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("DO_NOT_ECHO"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("DO_NOT_ECHO"));
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_cg-local"));
    command.env_clear();
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    let denied = command
        .env("OPENAI_API_KEY", "DO_NOT_ECHO")
        .arg("--check")
        .args(&setup.launch)
        .output()
        .unwrap();
    assert_eq!(denied.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&denied.stderr).contains("CG_CREDENTIAL_ENV_DENIED"));
    assert!(!String::from_utf8_lossy(&denied.stderr).contains("DO_NOT_ECHO"));
    setup.launch[19] = "foreign-session".into();
    assert_eq!(setup.run(&["--check"]).status.code(), Some(2));
    setup.launch[1] = "Bearer DO_NOT_ECHO".into();
    let bad = setup.run(&["--check"]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&bad.stderr).contains("DO_NOT_ECHO"));
}
