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
