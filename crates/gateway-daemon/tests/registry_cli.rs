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

#[test]
fn lists_agents_and_skills_in_canonical_order() {
    let agents = json_output(
        cli()
            .args([
                "--catalog",
                catalog().to_str().unwrap(),
                "--json",
                "agent",
                "list",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(agents["kind"], "agent_list");
    assert_eq!(agents["agents"][0]["id"], "analysis-storage-architect");
    assert_eq!(agents["agents"].as_array().unwrap().len(), agents["count"]);

    let skills = json_output(
        cli()
            .args([
                "--catalog",
                catalog().to_str().unwrap(),
                "skill",
                "list",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(skills["kind"], "skill_list");
    let listed = skills["skills"].as_array().unwrap();
    assert!(
        listed
            .windows(2)
            .all(|pair| { pair[0]["id"].as_str().unwrap() < pair[1]["id"].as_str().unwrap() })
    );
}

#[test]
fn shows_complete_skill_and_dependency_graph() {
    let skill = json_output(
        cli()
            .args([
                "--catalog",
                catalog().to_str().unwrap(),
                "skill",
                "show",
                "architecture-hexagonal",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(skill["skill"]["name"], "architecture hexagonal");
    assert!(skill["skill"]["rules"].as_array().unwrap().len() >= 3);
    assert!(skill["skill"].get("provided_capabilities").is_some());

    let graph = json_output(
        cli()
            .args([
                "--catalog",
                catalog().to_str().unwrap(),
                "skill",
                "graph",
                "architecture-hexagonal",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(graph["root"], "architecture-hexagonal");
    assert_eq!(graph["topological_order"][0], "architecture-hexagonal");
    assert!(graph["dependencies"].is_object());
}

#[test]
fn resolves_capability_with_provider_and_match_explanation() {
    let output = json_output(
        cli()
            .args([
                "--catalog",
                catalog().to_str().unwrap(),
                "capability",
                "resolve",
                "architecture.dependency-analysis",
                "--json",
            ])
            .output()
            .unwrap(),
    );
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
fn unknown_ids_and_invalid_catalogs_fail_closed() {
    let unknown = cli()
        .args(["capability", "show", "missing.capability", "--json"])
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(4));
    let error: Value = serde_json::from_slice(&unknown.stdout).unwrap();
    assert_eq!(error["code"], "unknown_id");

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
