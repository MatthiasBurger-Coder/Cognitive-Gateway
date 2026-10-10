use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

const BINDING: [&str; 12] = [
    "--client-name",
    "codex",
    "--client-version",
    "1.0",
    "--principal",
    "operator",
    "--workspace",
    "workspace-example",
    "--project",
    "project-example",
    "--binding",
    "binding-example",
];

fn run(args: &[&str], input: &[u8]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cg-mcp"));
    command.env_clear();
    // Only LLVM instrumentation is allowlisted during coverage; no provider environment.
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    let mut child = command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    // Fixture is bounded; stdout is consumed while waiting, without a provider.
    child.wait_with_output().unwrap()
}

#[test]
fn local_mcp_subprocess_conformance_without_provider_credentials() {
    let output = run(
        &BINDING,
        include_bytes!("../../../tests/fixtures/local-mcp/lifecycle.jsonl"),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let messages: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(messages.len(), 6);
    assert_eq!(messages[0]["id"], 1);
    assert_eq!(messages[1]["id"], "discovery");
    assert_eq!(messages[2]["id"], 2);
    assert_eq!(messages[3]["result"]["tools"].as_array().unwrap().len(), 13);
    assert_eq!(
        messages[4]["result"]["structuredContent"]["diagnostics"][0]["code"],
        "CG_UNSUPPORTED_CAPABILITY"
    );
    assert_eq!(messages[5]["result"], serde_json::json!({}));
}

#[test]
fn local_mcp_launch_errors_help_and_truncated_transport_are_sanitized() {
    assert!(run(&["--help"], b"").status.success());
    for args in [
        vec![],
        vec!["--client-name"],
        vec!["--api-key", "secret"],
        BINDING[..10].to_vec(),
        BINDING
            .iter()
            .copied()
            .chain(["--extra", "secret"])
            .collect(),
        {
            let mut args = BINDING.to_vec();
            args[1] = "../secret";
            args
        },
        {
            let mut args = BINDING.to_vec();
            args[2] = "--client-name";
            args
        },
    ] {
        let output = run(&args, b"");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8(output.stderr).unwrap().contains("secret"));
    }
    let output = run(&BINDING, b"{\"secret\":1}");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "Local MCP transport terminated.\n"
    );
    assert!(run(&BINDING, b"").status.success());
}

#[test]
fn inherited_credential_environment_is_denied_without_echoing_values() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/codex-security/credentials.json"
    ))
    .unwrap();
    for name in fixture["names"].as_array().unwrap() {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cg-mcp"));
        command.env_clear();
        if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", profile);
        }
        let output = command
            .args(BINDING)
            .env(name.as_str().unwrap(), "OPAQUE_FAKE_CREDENTIAL")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "Local MCP credential environment denied; launch with a clean environment.\n"
        );
    }
    // An authentication store in HOME is neither opened nor relayed.
    let root = std::env::temp_dir().join(format!("cg-auth-store-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".codex")).unwrap();
    std::fs::write(
        root.join(".codex/auth.json"),
        r#"{"access_token":"OPAQUE_FAKE_CREDENTIAL"}"#,
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_cg-mcp"));
    command.env_clear();
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    let output = command
        .env("HOME", &root)
        .env("CODEX_HOME", root.join(".codex"))
        .args(BINDING)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn real_stdio_credentials_in_requests_and_auth_failures_never_reach_output() {
    let mut frames: Vec<Value> = include_str!("../../../tests/fixtures/local-mcp/lifecycle.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    frames.extend([
        serde_json::json!({"jsonrpc":"2.0","id":"sk-proj-FAKE_CREDENTIAL_0123456789","method":"ping"}),
        serde_json::json!({"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"cg_situation_inspect_v1","arguments":{"api_key":"OPAQUE_FAKE_CREDENTIAL"}}}),
        serde_json::json!({"jsonrpc":"2.0","id":21,"method":"resources/read","params":{"uri":"cg://contracts/1.0/catalog","_meta":{"access_token":"OPAQUE_FAKE_CREDENTIAL"}}}),
        serde_json::json!({"jsonrpc":"2.0","id":22,"method":"ping"}),
    ]);
    let input: String = frames.iter().map(|v| format!("{v}\n")).collect();
    let output = run(&BINDING, input.as_bytes());
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("FAKE_CREDENTIAL"));
    let messages: Vec<Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for response in &messages[6..9] {
        assert!(response["id"].is_null());
        assert_eq!(response["error"]["message"], "Invalid Request");
    }
    assert_eq!(messages[9]["id"], 22);
    assert_eq!(messages[9]["result"], serde_json::json!({}));
    let frame = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"OPAQUE_FAKE_CREDENTIAL","version":"1.0"}}});
    let output = run(&BINDING, format!("{frame}\n").as_bytes());
    assert!(output.stderr.is_empty());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["error"]["message"], "Client admission denied");
    assert!(!response.to_string().contains("FAKE_CREDENTIAL"));
}
