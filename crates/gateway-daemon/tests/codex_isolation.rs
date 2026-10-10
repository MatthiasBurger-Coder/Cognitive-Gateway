use gateway_application::codex::*;
use gateway_daemon::codex_workspace::LocalWorkspaceResolver;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

struct Fixture {
    root: PathBuf,
    request: Value,
    config: Value,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "cg-isolation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("a/sub")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        let request: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-v1/inline.situation.inspect.request.json"
        ))
        .unwrap();
        let scope = request["scope"].clone();
        let document = request["input"]["situation"]["document"].clone();
        let reference = json!({"id":"situation-a","contract":"cg.situation","contract_version":"1.0","revision":"7",
            "digest":format!("sha256:{:x}",Sha256::digest(document.to_string().as_bytes()))});
        let resource = json!({"schema_version":"1.0","scope":scope,"reference":reference,"document":document,
            "provenance":[{"reference":reference,"source_id":"source-a","source_revision":"source-7","freshness":"stale",
                "sensitivity":"CONFIDENTIAL","lineage":[]}]});
        let config = json!({"schema_version":1,"mappings":[{"repository":root.join("a"),"scope":scope,"canonical_scope":"canonical-a",
            "principal":"operator","session_id":"codex-session-a","revision":"mapping-2","resources":[resource]}]});
        Self {
            root,
            request,
            config,
        }
    }
    fn resolver(&self) -> LocalWorkspaceResolver {
        LocalWorkspaceResolver::from_json(&self.config.to_string()).unwrap()
    }
    fn claim(&self) -> WorkspaceReference {
        WorkspaceReference {
            working_directory: self.root.join("a/sub").to_string_lossy().into(),
            repository: self.root.join("a").to_string_lossy().into(),
        }
    }
    fn app(&self) -> CodexFacade<gateway_daemon::codex_workspace::LocalCodexHost> {
        let resolver = self.resolver();
        let binding = resolver.resolve(&self.claim()).unwrap();
        let host = resolver.host(&binding).unwrap();
        CodexFacade::with_binding(binding, host).unwrap()
    }
    fn reference_request(&self) -> Value {
        let mut request = self.request.clone();
        request["input"]["situation"] = json!({"kind":"reference","reference":self.config["mappings"][0]["resources"][0]["reference"]});
        request
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn code(response: &Value) -> &str {
    response["diagnostics"][0]["code"].as_str().unwrap()
}

#[test]
fn resolver_rejects_unmapped_ambiguous_relative_and_wrong_repository_claims() {
    let mut f = Fixture::new();
    let resolver = f.resolver();
    let binding = resolver.resolve(&f.claim()).unwrap();
    assert_eq!(binding.canonical_scope.as_str(), "canonical-a");
    assert_eq!(binding.session.session_id, "codex-session-a");
    let mut claim = f.claim();
    claim.working_directory = f.root.join("b").to_string_lossy().into();
    assert!(matches!(
        resolver.resolve(&claim),
        Err(FacadeError::ScopeDenied)
    ));
    claim = f.claim();
    claim.repository = f.root.join("b").to_string_lossy().into();
    assert!(resolver.resolve(&claim).is_err());
    claim = f.claim();
    claim.working_directory = "a/sub".into();
    assert!(resolver.resolve(&claim).is_err());
    let mut nested = f.config["mappings"][0].clone();
    nested["repository"] = json!(f.root.join("a/sub"));
    f.config["mappings"].as_array_mut().unwrap().push(nested);
    assert!(matches!(
        f.resolver().resolve(&f.claim()),
        Err(FacadeError::ScopeDenied)
    ));
    f.config["mappings"][1] = f.config["mappings"][0].clone();
    assert!(f.resolver().resolve(&f.claim()).is_err());
}
#[cfg(unix)]
#[test]
fn symlink_escape_cannot_reuse_project_scope() {
    let f = Fixture::new();
    std::os::unix::fs::symlink(f.root.join("b"), f.root.join("a/escape")).unwrap();
    let mut claim = f.claim();
    claim.working_directory = f.root.join("a/escape").to_string_lossy().into();
    assert!(matches!(
        f.resolver().resolve(&claim),
        Err(FacadeError::ScopeDenied)
    ));
}
#[test]
fn session_principal_connection_and_project_cannot_broaden_authority() {
    let f = Fixture::new();
    let resolver = f.resolver();
    let binding = resolver.resolve(&f.claim()).unwrap();
    for field in [
        "session",
        "principal",
        "connection",
        "canonical",
        "revision",
    ] {
        let host = resolver.host(&binding).unwrap();
        let mut forged = binding.clone();
        match field {
            "session" => forged.session.session_id = "session-b".into(),
            "principal" => forged.session.principal = "principal-b".into(),
            "connection" => {
                forged.scope["binding_id"] = json!("binding-b");
                forged.session.connection_id = "binding-b".into();
            }
            "canonical" => {
                forged.canonical_scope = gateway_domain::ContextScopeId::new("canonical-b").unwrap()
            }
            _ => forged.mapping_revision = "mapping-3".into(),
        }
        assert!(resolver.host(&forged).is_err());
        let app = CodexFacade::with_binding(forged.clone(), host).unwrap();
        let mut request = f.reference_request();
        request["scope"] = forged.scope;
        assert_eq!(
            code(&app.execute("situation.inspect", &request)),
            "CG_SCOPE_DENIED"
        );
    }
    let app = f.app();
    for field in ["workspace_id", "project_id", "binding_id"] {
        let mut request = f.reference_request();
        request["scope"][field] = json!("foreign");
        let response = app.execute("situation.inspect", &request);
        assert_eq!(code(&response), "CG_SCOPE_DENIED");
        assert!(response["provenance"].as_array().unwrap().is_empty());
    }
    let mut session: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/codex-v1/session.inspect.request.json"
    ))
    .unwrap();
    session["input"]["session_id"] = json!("foreign-session");
    assert_eq!(
        code(&app.execute("session.inspect", &session)),
        "CG_UNSUPPORTED_CAPABILITY"
    );
}
#[test]
fn immutable_reference_provenance_survives_inline_and_reference_mapping() {
    let f = Fixture::new();
    let app = f.app();
    for request in [&f.request, &f.reference_request()] {
        let response = app.execute("situation.inspect", request);
        assert_eq!(response["status"], "ok", "{response}");
        assert_eq!(response["scope"], request["scope"]);
        assert_eq!(response["correlation"], request["correlation"]);
        assert_eq!(
            response["provenance"],
            f.config["mappings"][0]["resources"][0]["provenance"]
        );
        assert_eq!(response["explainability"][0]["id"], "codex-session-a");
        assert_eq!(response["explainability"][0]["revision"], "mapping-2");
        let trace = &response["explainability"][0];
        let resource = app
            .read_resource(
                &request["scope"],
                trace["id"].as_str().unwrap(),
                trace["revision"].as_str().unwrap(),
                trace["digest"].as_str().unwrap(),
            )
            .unwrap();
        assert_eq!(
            resource["document"]["basis"]["session_id"],
            "codex-session-a"
        );
        assert_eq!(
            resource["document"]["basis"]["canonical_scope"],
            "canonical-a"
        );
        assert_eq!(
            resource["document"]["policy_authorization"],
            "NOT_EVALUATED"
        );
        assert_eq!(
            resource["reference"]["digest"],
            format!(
                "sha256:{:x}",
                Sha256::digest(resource["document"].to_string().as_bytes())
            )
        );
    }
    let mut request = f.reference_request();
    request["input"]["situation"]["reference"]["revision"] = json!("8");
    assert_eq!(
        code(&app.execute("situation.inspect", &request)),
        "CG_REFERENCE_UNAVAILABLE"
    );
    let reference = &f.config["mappings"][0]["resources"][0]["reference"];
    let resource = app
        .read_resource(
            &f.request["scope"],
            reference["id"].as_str().unwrap(),
            "7",
            reference["digest"].as_str().unwrap(),
        )
        .unwrap();
    assert_eq!(resource, f.config["mappings"][0]["resources"][0]);
    let mut scope = f.request["scope"].clone();
    scope["project_id"] = json!("foreign");
    assert!(matches!(
        app.read_resource(
            &scope,
            "situation-a",
            "7",
            reference["digest"].as_str().unwrap()
        ),
        Err(FacadeError::ScopeDenied)
    ));
}
#[test]
fn secrets_and_unclassified_inline_content_are_refused_without_echo() {
    let mut f = Fixture::new();
    f.config["mappings"][0]["resources"][0]["provenance"][0]["sensitivity"] = json!("SECRET");
    let app = f.app();
    for request in [&f.request, &f.reference_request()] {
        let response = app.execute("situation.inspect", request);
        assert_eq!(code(&response), "CG_SENSITIVITY_DENIED");
        assert!(!response.to_string().contains("PRIVATE_REPORT"));
        assert!(!response.to_string().contains("source-a"));
        assert_eq!(response["correlation"], request["correlation"]);
    }
    let mut request = f.request.clone();
    request["input"]["situation"]["document"]["private"] = json!("DO_NOT_ECHO");
    let response = app.execute("situation.inspect", &request);
    assert_eq!(code(&response), "CG_SENSITIVITY_DENIED");
    assert!(!response.to_string().contains("DO_NOT_ECHO"));
    let binding = f.resolver().resolve(&f.claim()).unwrap();
    let resource = &f.config["mappings"][0]["resources"][0];
    assert_eq!(
        binding.cache_key(
            "situation.inspect",
            &[resource["reference"].clone()],
            resource["provenance"].as_array().unwrap()
        ),
        Err(FacadeError::SensitivityDenied)
    );
}
#[test]
fn cache_partition_changes_for_session_scope_revision_and_source_metadata() {
    let f = Fixture::new();
    let binding = f.resolver().resolve(&f.claim()).unwrap();
    let resource = &f.config["mappings"][0]["resources"][0];
    let refs = vec![resource["reference"].clone()];
    let provenance = resource["provenance"].as_array().unwrap();
    let key = binding
        .cache_key("situation.inspect", &refs, provenance)
        .unwrap();
    assert!(!key.contains("PRIVATE_REPORT"));
    assert!(!key.contains("source-a"));
    assert!(binding.cache_key("situation.inspect", &refs, &[]).is_err());
    let mut other = binding.clone();
    other.session.session_id = "session-b".into();
    assert_ne!(
        key,
        other
            .cache_key("situation.inspect", &refs, provenance)
            .unwrap()
    );
    other = binding.clone();
    other.mapping_revision = "mapping-3".into();
    assert_ne!(
        key,
        other
            .cache_key("situation.inspect", &refs, provenance)
            .unwrap()
    );
    other = binding.clone();
    other.scope["project_id"] = json!("project-b");
    assert_ne!(
        key,
        other
            .cache_key("situation.inspect", &refs, provenance)
            .unwrap()
    );
    let mut changed = provenance.clone();
    changed[0]["freshness"] = json!("current");
    assert_ne!(
        key,
        binding
            .cache_key("situation.inspect", &refs, &changed)
            .unwrap()
    );
}
#[test]
fn admission_rejects_bad_versions_duplicate_keys_digests_and_provenance() {
    let f = Fixture::new();
    for change in ["version", "digest", "scope", "provenance", "unknown"] {
        let mut config = f.config.clone();
        match change {
            "version" => config["schema_version"] = json!(2),
            "digest" => {
                config["mappings"][0]["resources"][0]["reference"]["digest"] =
                    json!(format!("sha256:{}", "0".repeat(64)))
            }
            "scope" => {
                config["mappings"][0]["resources"][0]["scope"]["project_id"] = json!("foreign")
            }
            "provenance" => config["mappings"][0]["resources"][0]["provenance"] = json!([]),
            _ => config["credentials"] = json!("DO_NOT_ECHO"),
        }
        assert!(LocalWorkspaceResolver::from_json(&config.to_string()).is_err());
    }
    assert!(
        LocalWorkspaceResolver::from_json(
            "{\"schema_version\":1,\"schema_version\":1,\"mappings\":[]}"
        )
        .is_err()
    );
}

#[test]
fn admission_credentials_are_refused_before_retention() {
    let f = Fixture::new();
    for field in ["credentials", "document", "metadata", "principal"] {
        let mut config = f.config.clone();
        match field {
            "credentials" => config["credentials"] = json!({"api_key":"OPAQUE_FAKE_CREDENTIAL"}),
            "document" => {
                config["mappings"][0]["resources"][0]["document"]["note"] =
                    json!("Bearer FAKE_CREDENTIAL")
            }
            "metadata" => {
                config["mappings"][0]["resources"][0]["provenance"][0]["source_id"] =
                    json!("sk-proj-FAKE_CREDENTIAL_0123456789")
            }
            _ => config["mappings"][0]["principal"] = json!("sk-proj-FAKE_CREDENTIAL_0123456789"),
        }
        assert!(matches!(
            LocalWorkspaceResolver::from_json(&config.to_string()),
            Err(FacadeError::SensitivityDenied)
        ));
    }
}
#[test]
fn real_stdio_admitted_launch_queries_and_resources_preserve_isolation() {
    let f = Fixture::new();
    let config_file = f.root.join("admission.json");
    std::fs::write(&config_file, f.config.to_string()).unwrap();
    let resource = &f.config["mappings"][0]["resources"][0];
    let uri = format!(
        "cg://workspaces/workspace-example/projects/project-example/bindings/binding-example/references/situation-a/7/{}",
        resource["reference"]["digest"].as_str().unwrap()
    );
    let frames = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"codex","version":"1.0"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"cg_situation_inspect_v1","arguments":f.reference_request()}}),
        json!({"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":uri}}),
        json!({"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":uri.replace("project-example","foreign")}}),
    ];
    let mut command = Command::new(env!("CARGO_BIN_EXE_cg-mcp"));
    command.env_clear();
    // Instrumentation is the sole coverage allowlist entry.
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    let mut child = command
        .args([
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
            "--admission",
            config_file.to_str().unwrap(),
            "--cwd",
            f.root.join("a/sub").to_str().unwrap(),
            "--repository",
            f.root.join("a").to_str().unwrap(),
            "--session",
            "codex-session-a",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for frame in frames {
        writeln!(stdin, "{frame}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let responses: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        responses[1]["result"]["structuredContent"]["status"], "ok",
        "{}",
        responses[1]
    );
    let actual: Value = serde_json::from_str(
        responses[2]["result"]["contents"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual, *resource);
    assert_eq!(responses[3]["error"]["data"]["code"], "CG_SCOPE_DENIED");
    assert!(output.stderr.is_empty());
}

#[test]
fn two_admitted_projects_with_overlapping_source_ids_cannot_share_context() {
    let mut f = Fixture::new();
    let mut other = f.config["mappings"][0].clone();
    other["repository"] = json!(f.root.join("b"));
    other["canonical_scope"] = json!("canonical-b");
    other["session_id"] = json!("codex-session-b");
    other["scope"] =
        json!({"workspace_id":"workspace-b","project_id":"project-b","binding_id":"connection-b"});
    other["resources"][0]["scope"] = other["scope"].clone();
    other["resources"][0]["document"]["context"]["id"] = json!("context-b");
    let digest = format!(
        "sha256:{:x}",
        Sha256::digest(other["resources"][0]["document"].to_string().as_bytes())
    );
    other["resources"][0]["reference"]["digest"] = json!(digest);
    other["resources"][0]["provenance"][0]["reference"] =
        other["resources"][0]["reference"].clone();
    other["resources"][0]["provenance"][0]["source_id"] = json!("source-b");
    f.config["mappings"]
        .as_array_mut()
        .unwrap()
        .push(other.clone());
    let resolver = f.resolver();
    let mut claim = f.claim();
    claim.repository = f.root.join("b").to_string_lossy().into();
    claim.working_directory = claim.repository.clone();
    let binding = resolver.resolve(&claim).unwrap();
    let host = resolver.host(&binding).unwrap();
    let app_b = CodexFacade::with_binding(binding, host).unwrap();
    let app_a = f.app();
    let request_a = f.reference_request();
    let mut request_b = request_a.clone();
    request_b["scope"] = other["scope"].clone();
    request_b["input"]["situation"]["reference"] = other["resources"][0]["reference"].clone();
    assert_eq!(
        app_b.execute("situation.inspect", &request_b)["provenance"][0]["source_id"],
        "source-b"
    );
    assert_eq!(
        app_a.execute("situation.inspect", &request_a)["provenance"][0]["source_id"],
        "source-a"
    );
    let mut foreign = request_a.clone();
    foreign["input"] = request_b["input"].clone();
    let response = app_a.execute("situation.inspect", &foreign);
    assert_eq!(code(&response), "CG_REFERENCE_UNAVAILABLE");
    assert!(!response.to_string().contains("source-b"));
    let mut invalid = f.config.clone();
    invalid["mappings"][1]["canonical_scope"] = json!("canonical-a");
    assert!(matches!(
        LocalWorkspaceResolver::from_json(&invalid.to_string()),
        Err(FacadeError::ScopeDenied)
    ));
}

#[test]
fn launch_admission_denials_do_not_emit_protocol_or_configuration_content() {
    let f = Fixture::new();
    let config_file = f.root.join("admission.json");
    std::fs::write(&config_file, f.config.to_string()).unwrap();
    let args: Vec<String> = [
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
        "--admission",
        config_file.to_str().unwrap(),
        "--cwd",
        f.root.join("a/sub").to_str().unwrap(),
        "--repository",
        f.root.join("a").to_str().unwrap(),
        "--session",
        "codex-session-a",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for (index, value) in [
        (19, "foreign-session"),
        (5, "foreign-principal"),
        (9, "foreign-project"),
        (15, "/nonexistent-cg-root"),
        (13, "/nonexistent-cg-admission"),
        (18, "--cwd"),
    ] {
        let mut bad = args.clone();
        bad[index] = value.into();
        let mut command = Command::new(env!("CARGO_BIN_EXE_cg-mcp"));
        command.env_clear();
        if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", profile);
        }
        let output = command.args(&bad).stdin(Stdio::null()).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "Invalid local MCP launch binding; use cg-mcp --help.\n"
        );
    }
}

#[test]
fn local_policy_rejects_execution_authority_changes_and_session_mutations() {
    let fixture = Fixture::new();
    let app = fixture.app();
    for (field, value) in [
        ("operating_mode", "HARDENING"),
        ("execution_profile", "FAST_PATH"),
    ] {
        let mut request = fixture.request.clone();
        request["execution"][field] = json!(value);
        let response = app.execute("situation.inspect", &request);
        assert_eq!(response["diagnostics"][0]["code"], "CG_POLICY_DENIED");
    }
    for operation in [
        "session.start",
        "session.approve",
        "session.continue",
        "session.cancel",
        "session.clarify",
    ] {
        let mut request: Value = serde_json::from_str(
            &std::fs::read_to_string(format!(
                "{}/../../tests/fixtures/codex-v1/{operation}.request.json",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap(),
        )
        .unwrap();
        request["scope"] = fixture.request["scope"].clone();
        assert_eq!(
            app.execute(operation, &request)["diagnostics"][0]["code"],
            "CG_UNSUPPORTED_CAPABILITY"
        );
    }
}
