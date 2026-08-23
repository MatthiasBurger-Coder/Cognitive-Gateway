use std::{
    env, fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cg-registry"));
    command.current_dir(workspace_root());
    command
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("daemon crate must be below the workspace root")
        .to_path_buf()
}

fn catalog() -> PathBuf {
    workspace_root().join("catalog")
}

fn json_output(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("CLI output must be valid JSON")
}

fn json_command(arguments: &[&str]) -> Value {
    json_output(cli().args(arguments).arg("--json").output().unwrap())
}

#[test]
fn executes_every_public_command_against_the_canonical_catalog() {
    let agents = json_command(&["agent", "list"]);
    assert_eq!(agents["kind"], "agent_list");
    assert_eq!(agents["agents"][0]["id"], "analysis-storage-architect");
    assert_eq!(agents["agents"].as_array().unwrap().len(), agents["count"]);

    let agent = json_command(&["agent", "show", "system-architect"]);
    assert_eq!(agent["kind"], "agent");
    assert_eq!(agent["agent"]["id"], "system-architect");

    let skills = json_command(&["skill", "list"]);
    assert_eq!(skills["kind"], "skill_list");
    let listed = skills["skills"].as_array().unwrap();
    assert!(
        listed
            .windows(2)
            .all(|pair| { pair[0]["id"].as_str().unwrap() < pair[1]["id"].as_str().unwrap() })
    );

    let skill = json_command(&["skill", "show", "architecture-hexagonal"]);
    assert_eq!(skill["skill"]["name"], "architecture hexagonal");
    assert!(skill["skill"]["rules"].as_array().unwrap().len() >= 3);
    assert!(skill["skill"].get("provided_capabilities").is_some());

    let graph = json_command(&["skill", "graph", "architecture-hexagonal"]);
    assert_eq!(graph["root"], "architecture-hexagonal");
    assert_eq!(graph["topological_order"][0], "architecture-hexagonal");
    assert!(graph["dependencies"].is_object());

    let capabilities = json_command(&["capability", "list"]);
    assert_eq!(capabilities["kind"], "capability_list");
    assert!(capabilities["count"].as_u64().unwrap() > 0);

    let capability = json_command(&["capability", "show", "architecture.dependency-analysis"]);
    assert_eq!(capability["kind"], "capability");
    assert_eq!(
        capability["capability"]["id"],
        "architecture.dependency-analysis"
    );

    let output = json_command(&["capability", "resolve", "architecture.dependency-analysis"]);
    assert_eq!(output["outcome"], "ambiguous");
    assert_eq!(output["status"], "resolvable");
    let matches = output["matches"].as_array().unwrap();
    assert!(
        matches
            .iter()
            .any(|candidate| { candidate["provider"]["source"] == "agent:system-architect" })
    );
    assert!(
        matches[0]["matched_selectors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|selector| selector["type"] == "capability_id")
    );
}

#[test]
fn show_and_resolve_commands_reject_unknown_ids() {
    for arguments in [
        ["agent", "show", "missing-agent"],
        ["skill", "show", "missing-skill"],
        ["skill", "graph", "missing-skill"],
        ["capability", "show", "missing.capability"],
        ["capability", "resolve", "missing.capability"],
    ] {
        let output = cli().args(arguments).arg("--json").output().unwrap();
        assert_eq!(output.status.code(), Some(4), "command: {arguments:?}");
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["code"], "unknown_id", "command: {arguments:?}");
    }
}

#[test]
fn invalid_catalogs_fail_closed() {
    let unique = format!(
        "cg-registry-cli-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = env::temp_dir().join(unique);
    fs::create_dir_all(root.join("agents")).unwrap();
    fs::create_dir_all(root.join("skills")).unwrap();
    fs::write(
        root.join("agents").join("broken.json"),
        "{\"not\":\"an agent\"}",
    )
    .unwrap();
    let invalid = cli()
        .args([
            "--catalog",
            root.to_str().unwrap(),
            "agent",
            "list",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(3));
    let error: Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(error["code"], "catalog_load_error");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn human_output_is_readable_and_deterministic() {
    let first = cli()
        .args([
            "--catalog",
            catalog().to_str().unwrap(),
            "capability",
            "list",
        ])
        .output()
        .unwrap();
    let second = cli()
        .args([
            "--catalog",
            catalog().to_str().unwrap(),
            "capability",
            "list",
        ])
        .output()
        .unwrap();
    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);
    let text = String::from_utf8(first.stdout).unwrap();
    assert!(text.contains("Cognitive Gateway Registry Inspection"));
    assert!(text.contains("Capabilities:"));
    assert!(text.contains("architecture.dependency-analysis"));
}
