//! Operator filesystem setup in the inbound infrastructure adapter.
//! Only the bundled PUBLIC example is admitted; no client settings are edited.
use crate::{codex_workspace::LocalWorkspaceResolver, local_mcp::LaunchBinding};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

const ADMISSION: &str = include_str!("../../../examples/codex-local/admission.example.json");
const REQUEST: &str = include_str!("../../../examples/codex-local/situation.inspect.request.json");

fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.is_file()
            && path
                .metadata()
                .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Return the existing --check launch arguments after writing a new setup.
pub fn create(args: &[String]) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut options = BTreeMap::new();
    let required = [
        "--repository",
        "--output",
        "--client-name",
        "--client-version",
    ];
    if !args.len().is_multiple_of(2) {
        return Err("options require values".into());
    }
    for pair in args.as_chunks::<2>().0 {
        if (!required.contains(&pair[0].as_str()) && pair[0] != "--bin-dir")
            || options.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err("unknown or duplicate option".into());
        }
    }
    if required.iter().any(|key| !options.contains_key(key)) {
        return Err("missing setup options".into());
    }
    let repository = fs::canonicalize(options["--repository"])?;
    let repository_text = repository.to_str().ok_or("UTF-8 path required")?;
    let current_exe = std::env::current_exe()?;
    let bin_dir = options.get("--bin-dir").map_or_else(
        || current_exe.parent().unwrap().to_path_buf(),
        |path| Path::new(path).to_path_buf(),
    );
    let binary = fs::canonicalize(bin_dir.join("cg-mcp"))?;
    let cli = fs::canonicalize(bin_dir.join("cg-local"))?;
    let env = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("env"))
        .find(|path| executable(path))
        .ok_or("POSIX env launcher required")?;
    let env = fs::canonicalize(env)?;
    if !repository.is_dir() || !executable(&binary) || !executable(&cli) {
        return Err("repository and executable binaries required".into());
    }
    let mut config: Value = serde_json::from_str(ADMISSION)?;
    let mapping = &mut config["mappings"][0];
    let scope = mapping["scope"].clone();
    let client_name = options["--client-name"];
    let client_version = options["--client-version"];
    LaunchBinding::new(
        client_name,
        client_version,
        mapping["principal"].as_str().unwrap(),
        scope["workspace_id"].as_str().unwrap(),
        scope["project_id"].as_str().unwrap(),
        scope["binding_id"].as_str().unwrap(),
    )
    .ok_or("invalid client identity")?;
    mapping["repository"] = json!(repository_text);
    let mut request: Value = serde_json::from_str(REQUEST)?;
    let document = request["input"]["situation"]["document"].clone();
    let reference = json!({"id":"operator-smoke", "contract":"cg.situation", "contract_version":"1.0",
        "revision":"1", "digest":format!("sha256:{:x}", Sha256::digest(document.to_string().as_bytes()))});
    mapping["resources"] = json!([{"schema_version":"1.0", "scope":scope,
        "reference":reference, "document":document, "provenance":[{"reference":reference,
        "source_id":"synthetic-operator-smoke", "source_revision":"1", "freshness":"current",
        "sensitivity":"PUBLIC", "lineage":[]}]}]);
    request["input"]["situation"] = json!({"kind":"reference", "reference":reference});
    let principal = mapping["principal"].as_str().unwrap().to_owned();
    let session = mapping["session_id"].as_str().unwrap().to_owned();
    LocalWorkspaceResolver::from_json(&config.to_string())
        .map_err(|_| "invalid bundled admission")?;
    let output = std::path::absolute(options["--output"])?;
    fs::create_dir_all(output.parent().ok_or("output parent required")?)?;
    let mut directory = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory.create(&output)?;
    let output = fs::canonicalize(output)?;
    let admission = output.join("admission.json");
    let launch = vec![
        "--client-name".into(),
        client_name.into(),
        "--client-version".into(),
        client_version.into(),
        "--principal".into(),
        principal,
        "--workspace".into(),
        scope["workspace_id"].as_str().unwrap().into(),
        "--project".into(),
        scope["project_id"].as_str().unwrap().into(),
        "--binding".into(),
        scope["binding_id"].as_str().unwrap().into(),
        "--admission".into(),
        admission.to_str().ok_or("UTF-8 path required")?.into(),
        "--cwd".into(),
        repository_text.into(),
        "--repository".into(),
        repository_text.into(),
        "--session".into(),
        session,
    ];
    let mut server_args = vec![
        "-i".to_owned(),
        binary.to_str().ok_or("UTF-8 path required")?.into(),
    ];
    server_args.extend(launch.clone());
    // JSON basic strings/arrays are valid TOML for these bounded identity/path values.
    let toml = format!(
        "[mcp_servers.cognitive_gateway]\ncommand = {}\nargs = {}\nenv_vars = []\nstartup_timeout_sec = 10\ntool_timeout_sec = 35\nenabled_tools = [\"cg_situation_inspect_v1\", \"cg_situation_assess_v1\"]\n",
        serde_json::to_string(env.to_str().ok_or("UTF-8 path required")?)?,
        serde_json::to_string(&server_args)?
    );
    for (name, value) in [
        ("admission.json", config),
        ("request.json", request),
        ("launch.json", json!(launch)),
    ] {
        fs::write(
            output.join(name),
            serde_json::to_string_pretty(&value)? + "\n",
        )?;
    }
    fs::write(output.join("config.toml"), toml)?;
    Ok([vec!["--check".into()], launch].concat())
}
