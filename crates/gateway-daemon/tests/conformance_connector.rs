//! Cross-provider executable qualification; controlled fixtures, no live service.
use gateway_application::{
    connector_manifest::*, connector_policy::*, connector_results::ConnectorResult, connectors::*,
};
use gateway_daemon::{
    conformance_connector::{self, DocumentResultNormalizer, Profile},
    github_connector::*,
    mcp_client::{mapping::*, normalization::*, transport::*, *},
};
use gateway_domain::*;
use gateway_policy::*;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
fn text(s: &str) -> NonEmptyText {
    NonEmptyText::new(s).unwrap()
}
fn digest() -> ContentDigest {
    ContentDigest::new("a".repeat(64)).unwrap()
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn coverage_environment() -> BTreeMap<String, String> {
    std::env::var("LLVM_PROFILE_FILE")
        .ok()
        .map(|v| ("LLVM_PROFILE_FILE".into(), v))
        .into_iter()
        .collect()
}
fn github_peer() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/conformance-connector-v1/github-peer.py")
}
struct Sends(Arc<AtomicUsize>);
impl Observer for Sends {
    fn record(&self, e: Event) {
        if e.kind == EventKind::Sent {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}
struct NoIo;
impl GitHubReader for NoIo {
    fn get(&self, _: &str) -> Result<Vec<u8>, Failure> {
        panic!("export must not perform IO")
    }
}
struct Collector;
impl AcquisitionPort for Collector {
    fn collect(&self, _: &ConnectorDispatch) -> Result<Acquisition, Error> {
        Ok(Acquisition {
            acquired_at: UnixTimestamp::new(now()),
            observed_at: None,
            evaluated_at: UnixTimestamp::new(now()),
            max_age: FreshnessPolicy::new(60),
            authenticated_source: text("test.controlled-local-source"),
            sensitivity: SensitivityClass::Internal,
            confidence: Confidence::Unknown,
            uncertainty: Uncertainty::Unknown,
            conflict: ConflictStatus::None,
            partial: false,
            basis: ConnectorObservationBasis::SourceRead,
            effect: ConnectorEffect::NoMutation,
            verifies: None,
            non_authoritative_raw: None,
        })
    }
}
struct PolicySource {
    denied: Arc<AtomicBool>,
    scope: ContextScopeId,
}
impl PolicySource {
    fn snapshot(&self, r: &ConnectorRequest) -> ConnectorPolicySnapshot {
        let cap = r.capability.clone();
        ConnectorPolicySnapshot {
            step: r.step.clone(),
            resolved: true,
            has_prerequisites: false,
            constraints: Default::default(),
            preconditions: Default::default(),
            authority: PolicyAuthority {
                policies: vec![
                    PolicyDefinition::new(
                        PolicyId::new("policy").unwrap(),
                        "scoped read",
                        [cap.clone()],
                    )
                    .unwrap(),
                ],
                capabilities: [(
                    cap.clone(),
                    CapabilityDefinition::new(cap.clone(), CapabilityClass::Inspect),
                )]
                .into(),
                ..Default::default()
            },
            facts: StepFacts {
                authorizations: [(
                    cap,
                    if self.denied.load(Ordering::SeqCst) {
                        Approval::Denied
                    } else {
                        Approval::Granted
                    },
                )]
                .into(),
                ..Default::default()
            },
            operating_mode: OperatingMode::Development,
            execution_profile: ExecutionProfile::FullPath,
            process: ProcessReadiness::Eligible,
            consent_required: false,
            administrative_capabilities: Default::default(),
            policy: PolicyId::new("policy").unwrap(),
            policy_revision: digest(),
            consent_receipt: None,
            budget_reservation: text("read.reservation"),
            deadline: UnixTimestamp::new(now() + 30),
        }
    }
}
impl ConnectorPolicySource for PolicySource {
    fn admit_target(&self, t: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        if t.scope != self.scope {
            return Err(ConnectorFailure {
                code: ConnectorFailureCode::ScopeMismatch,
                effect: ConnectorEffect::NotDispatched,
                correlation: text("read"),
            });
        }
        Ok(())
    }
    fn authorize(
        &self,
        r: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        Ok(self.snapshot(r))
    }
    fn revalidate(
        &self,
        d: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        Ok(self.snapshot(&d.request))
    }
    fn record_decision(&self, _: &ConnectorPolicyEvaluation<'_>) -> Result<(), ConnectorFailure> {
        Ok(())
    }
}

fn host(
    github: bool,
    profile: Profile,
    variation: Option<&str>,
) -> (
    McpConnectorHost,
    ConnectorTarget,
    Arc<AtomicBool>,
    Arc<AtomicUsize>,
) {
    let mut bundle = if github {
        GitHubConnector::new("owner/repo", NoIo).unwrap().bundle()
    } else {
        conformance_connector::bundle(profile)
    };
    // Configuration alone gives GitHub's file reader the SAME canonical capability
    // and subject as the independent document server; wire names stay different.
    if github {
        for record in bundle["mapping"]["records"].as_array_mut().unwrap() {
            if record["name"] == "github.file.read" {
                record["capability"] = serde_json::json!("document.read");
            }
        }
    }
    // Bind unsupported schemas deliberately: rejection must come from schema
    // compatibility, rather than merely detecting a changed inventory digest.
    if matches!(variation, Some("nested-schema" | "open-schema")) {
        let mut advertised = bundle["discovery"]["tools"][0].clone();
        if variation == Some("nested-schema") {
            advertised["inputSchema"]["properties"]["key"] =
                serde_json::json!({"type":"object","properties":{}});
        } else {
            advertised["inputSchema"]["additionalProperties"] = serde_json::json!(true);
        }
        let mut definition = bundle["manifest"]["manifest"].clone();
        definition["inventory"][0]["schema_digest"] =
            serde_json::json!(discovery_schema_digest(InventoryKind::Tool, &advertised).as_str());
        let pinned =
            ConnectorManifest::from_definition_json(&serde_json::to_vec(&definition).unwrap())
                .unwrap();
        bundle["manifest"] = serde_json::from_slice(&pinned.canonical_json()).unwrap();
    }
    let manifest =
        ConnectorManifest::from_json(&serde_json::to_vec(&bundle["manifest"]).unwrap()).unwrap();
    let document =
        MappingDocument::from_json(&serde_json::to_vec(&bundle["mapping"]).unwrap()).unwrap();
    let target = ConnectorTarget {
        connector: text(if github {
            "github.reference"
        } else {
            "conformance.documents"
        }),
        scope: ContextScopeId::new("project.documents").unwrap(),
    };
    let (executable, arguments) = if github {
        (
            PathBuf::from(env!("CARGO_BIN_EXE_cg-github-mcp")),
            vec![
                "stdio".into(),
                "owner/repo".into(),
                github_peer().to_string_lossy().into_owned(),
            ],
        )
    } else if let Some(mode) = variation {
        (
            PathBuf::from("/usr/bin/python3"),
            vec![
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/fixtures/conformance-connector-v1/peer.py")
                    .to_string_lossy()
                    .into_owned(),
                mode.into(),
            ],
        )
    } else {
        (
            PathBuf::from(env!("CARGO_BIN_EXE_cg-conformance-mcp")),
            vec![
                "stdio".into(),
                match profile {
                    Profile::Standard => "standard",
                    Profile::Legacy => "legacy",
                }
                .into(),
            ],
        )
    };
    let factory = StdioConfig {
        executable,
        arguments,
        directory: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        environment: coverage_environment(),
    };
    let denied = Arc::new(AtomicBool::new(false));
    let admission = Arc::new(Cg09ConnectorAdmission::new(PolicySource {
        denied: denied.clone(),
        scope: target.scope.clone(),
    }));
    let normalizer: Box<dyn ConnectorResultNormalizer> = if github {
        Box::new(
            CanonicalResultNormalizer::new(
                NormalizationProfile {
                    source: SourceId::new("source.github.file.read").unwrap(),
                    source_kind: SourceKind::Tool,
                    object_field: "object".into(),
                    revision_field: Some("revision".into()),
                    digest_field: None,
                    fields: [(
                        "content".into(),
                        SubjectPath::new(["document", "content"]).unwrap(),
                    )]
                    .into(),
                },
                Box::new(Collector),
            )
            .unwrap(),
        )
    } else {
        Box::new(DocumentResultNormalizer::new(Box::new(Collector)).unwrap())
    };
    let mapping = CanonicalConnectorMapping::new(
        DiscoveryMapper::new(target.clone(), manifest.clone(), document).unwrap(),
        normalizer,
    );
    let sends = Arc::new(AtomicUsize::new(0));
    let host = McpConnectorHost::new(
        HostConfig {
            target: target.clone(),
            manifest,
            profile: ConnectorCompatibilityProfile {
                gateway: ConnectorVersion {
                    major: 0,
                    minor: 1,
                    patch: 0,
                },
                transports: [ConnectorTransport::Stdio].into(),
            },
            limits: Limits::default(),
        },
        Box::new(factory),
        admission,
        Box::new(mapping),
        Arc::new(Sends(sends.clone())),
    )
    .unwrap();
    (host, target, denied, sends)
}
fn request(target: &ConnectorTarget, revision: ContentDigest, github: bool) -> ConnectorRequest {
    ConnectorRequest {
        target: target.clone(),
        capability: CapabilityId::new("document.read").unwrap(),
        class: ConnectorOperationClass::Read,
        inventory_revision: revision,
        arguments: if github {
            [
                (text("path"), TypedValue::String(text("README.md"))),
                (text("ref"), TypedValue::String(text("main"))),
            ]
            .into()
        } else {
            [(text("key"), TypedValue::String(text("guide")))].into()
        },
        task: TaskId::new("task").unwrap(),
        step: PlanStepId::new("step").unwrap(),
        execution: text("execution"),
        correlation: text("read"),
    }
}
fn dispatch(r: ConnectorRequest) -> ConnectorDispatch {
    ConnectorDispatch {
        request: r,
        policy: PolicyId::new("policy").unwrap(),
        policy_revision: digest(),
        consent_receipt: None,
        budget_reservation: text("read.reservation"),
        deadline: UnixTimestamp::new(now() + 30),
    }
}
#[test]
fn cross_provider_same_capability_policy_runtime_and_context_contracts() {
    for (github, profile) in [
        (true, Profile::Standard),
        (false, Profile::Standard),
        (false, Profile::Legacy),
    ] {
        let (host, target, denied, sends) = host(github, profile, None);
        host.lifecycle(&target, ConnectorLifecycleCommand::Connect)
            .unwrap();
        let inventory = host.discover(&target).unwrap();
        assert_eq!(host.discover(&target).unwrap().revision, inventory.revision);
        let request = request(&target, inventory.revision, github);
        let observation = host.invoke(&request).unwrap();
        assert_eq!(
            observation.source.as_str(),
            if github {
                "source.github.file.read"
            } else {
                "source.conformance.document"
            }
        );
        assert_eq!(
            observation.metadata.object_reference.as_str(),
            if github {
                "https://api.github.com/repos/owner/repo/contents/README.md?ref=main"
            } else {
                conformance_connector::URI
            }
        );
        assert_eq!(observation.metadata.effect, ConnectorEffect::NoMutation);
        assert_eq!(
            observation.metadata.quality.confidence(),
            Confidence::Unknown
        );
        assert_eq!(
            observation.metadata.quality.sensitivity(),
            SensitivityClass::Internal
        );
        assert_eq!(observation.metadata.task, request.task);
        assert_eq!(observation.metadata.execution, request.execution);
        assert!(observation.source_revision.is_some());
        assert_eq!(
            observation.records.observations()[0].subject(),
            &SubjectPath::new(["document", "content"]).unwrap()
        );
        let result = ConnectorResult::new(&dispatch(request.clone()), observation).unwrap();
        assert_eq!(result.batch().unwrap().scope(), &target.scope);
        assert!(!result.fragments().unwrap().is_empty());
        assert!(format!("{:?}", result.observation()).contains("document"));
        let sent = sends.load(Ordering::SeqCst);
        denied.store(true, Ordering::SeqCst);
        assert!(host.invoke(&request).is_err());
        denied.store(false, Ordering::SeqCst);
        for kind in 0..5 {
            let mut bad = request.clone();
            match kind {
                0 => bad.target.scope = ContextScopeId::new("foreign").unwrap(),
                1 => bad.class = ConnectorOperationClass::Mutate,
                2 => bad.capability = CapabilityId::new("document.write").unwrap(),
                3 => bad.inventory_revision = digest(),
                _ => {
                    bad.arguments
                        .insert(text("unknown"), TypedValue::Boolean(true));
                }
            }
            assert!(host.invoke(&bad).is_err());
        }
        assert_eq!(
            sends.load(Ordering::SeqCst),
            sent,
            "denials must send no wire requests"
        );
        host.lifecycle(&target, ConnectorLifecycleCommand::Shutdown)
            .unwrap();
    }
}
#[test]
fn independent_protocol_schema_and_failure_matrix() {
    let matrix: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/conformance-connector-v1/variations.json"
    ))
    .unwrap();
    for case in matrix.as_array().unwrap() {
        let mode = case["mode"].as_str().unwrap();
        let (host, target, _, _) = host(false, Profile::Standard, Some(mode));
        let connected = host.lifecycle(&target, ConnectorLifecycleCommand::Connect);
        if case["stage"] == "connect" {
            assert_eq!(
                format!("{:?}", connected.unwrap_err().code),
                case["code"].as_str().unwrap(),
                "{mode}"
            );
            continue;
        }
        connected.unwrap();
        let inventory = host.discover(&target);
        if case["stage"] == "discover" {
            assert_eq!(
                format!("{:?}", inventory.unwrap_err().code),
                case["code"].as_str().unwrap(),
                "{mode}"
            );
            continue;
        }
        let r = request(&target, inventory.unwrap().revision, false);
        let error = host.invoke(&r).unwrap_err();
        assert_eq!(
            format!("{:?}", error.code),
            case["code"].as_str().unwrap(),
            "{mode}"
        );
        assert_eq!(error.effect, ConnectorEffect::NoMutation);
        assert!(!format!("{error:?}").contains("private-provider-text"));
    }
}
#[test]
fn missing_document_is_typed_and_never_becomes_evidence() {
    let (host, target, _, _) = host(false, Profile::Standard, None);
    host.lifecycle(&target, ConnectorLifecycleCommand::Connect)
        .unwrap();
    let mut r = request(&target, host.discover(&target).unwrap().revision, false);
    r.arguments
        .insert(text("key"), TypedValue::String(text("missing")));
    assert_eq!(
        host.invoke(&r).unwrap_err().code,
        ConnectorFailureCode::NotFound
    );
}

#[test]
fn server_lifecycle_framing_and_fixed_diagnostics() {
    use serde_json::json;
    let frames = [
        json!({"jsonrpc":"2.0","id":0,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":MCP_VERSION}}),
        json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":MCP_VERSION}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":"ping","method":"ping"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"documents.fetch","arguments":{"key":"guide"}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"documents.fetch","arguments":{"key":"private-provider-text"}}}),
        json!({"jsonrpc":"2.0","id":6,"method":"documents.write"}),
    ];
    let input = frames.iter().map(|v| format!("{v}\n")).collect::<String>();
    for profile in [Profile::Standard, Profile::Legacy] {
        let mut output = Vec::new();
        conformance_connector::serve(profile, input.as_bytes(), &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(!text.contains("private-provider-text"));
        let responses = text
            .lines()
            .map(|s| serde_json::from_str::<serde_json::Value>(s).unwrap())
            .collect::<Vec<_>>();
        assert!(responses[0].get("error").is_some());
        assert_eq!(
            responses[5]["result"]["structuredContent"]["uri"],
            conformance_connector::URI
        );
        assert_eq!(responses[6]["result"]["isError"], true);
    }
    for malformed in [
        "garbage".to_string(),
        json!({"jsonrpc":"1.0","id":1,"method":"ping"}).to_string(),
        json!({"jsonrpc":"2.0","id":null,"method":"ping"}).to_string(),
        json!({"jsonrpc":"2.0","id":1}).to_string(),
        "x".repeat(262_145),
        "{\"jsonrpc\":\"2.0\",\"jsonrpc\":\"2.0\"}".to_string(),
    ] {
        assert!(
            conformance_connector::serve(Profile::Standard, malformed.as_bytes(), Vec::new())
                .is_err()
        );
    }
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }
    assert!(conformance_connector::serve(Profile::Standard, input.as_bytes(), Broken).is_err());
}
#[test]
fn executable_commands_and_exports_match_pinned_fixtures() {
    use std::process::{Command, Stdio};
    let binary = env!("CARGO_BIN_EXE_cg-conformance-mcp");
    for (name, profile) in [("standard", Profile::Standard), ("legacy", Profile::Legacy)] {
        let export = Command::new(binary)
            .args(["describe", name])
            .env_clear()
            .envs(coverage_environment())
            .output()
            .unwrap();
        assert!(export.status.success());
        let bundle: serde_json::Value = serde_json::from_slice(&export.stdout).unwrap();
        assert_eq!(bundle, conformance_connector::bundle(profile));
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../tests/fixtures/conformance-connector-v1/{name}.json"
        ));
        assert_eq!(
            bundle,
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(path).unwrap()).unwrap()
        );
        assert!(
            Command::new(binary)
                .args(["stdio", name])
                .env_clear()
                .envs(coverage_environment())
                .stdin(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    }
    for args in [
        vec![],
        vec!["stdio"],
        vec!["describe", "invalid"],
        vec!["unknown", "standard"],
        vec!["stdio", "standard", "extra"],
    ] {
        assert!(
            !Command::new(binary)
                .args(args)
                .env_clear()
                .envs(coverage_environment())
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}
#[test]
fn document_projection_rejects_substitution_and_unknown_error_messages() {
    use serde_json::json;
    let bundle = conformance_connector::bundle(Profile::Standard);
    let mapping: MappingRecord =
        serde_json::from_value(bundle["mapping"]["records"][0].clone()).unwrap();
    let target = ConnectorTarget {
        connector: text("conformance.documents"),
        scope: ContextScopeId::new("scope").unwrap(),
    };
    let normalizer = DocumentResultNormalizer::new(Box::new(Collector)).unwrap();
    let d = dispatch(request(&target, digest(), false));
    let result = || ProtocolResult::Tool {
        content: vec![],
        structured: Some(
            json!({"uri":conformance_connector::URI,"body":"document","version":"v1"}),
        ),
        is_error: false,
    };
    let mut wrong = mapping.clone();
    wrong.name = "foreign.fetch".into();
    assert_eq!(
        normalizer.normalize(&d, None, &wrong, result()),
        Err(Error::Schema)
    );
    let mut wrong_d = d.clone();
    wrong_d.request.capability = CapabilityId::new("other.read").unwrap();
    assert_eq!(
        normalizer.normalize(&wrong_d, None, &mapping, result()),
        Err(Error::Schema)
    );
    wrong_d = d.clone();
    wrong_d.request.arguments.clear();
    assert_eq!(
        normalizer.normalize(&wrong_d, None, &mapping, result()),
        Err(Error::Schema)
    );
    let unknown = ProtocolResult::Tool {
        content: vec![json!({"type":"text","text":"private-provider-text"})],
        structured: None,
        is_error: true,
    };
    assert_eq!(
        normalizer.normalize(&d, None, &mapping, unknown),
        Err(Error::Schema)
    );
    assert_eq!(
        normalizer.normalize(
            &d,
            Some(&SourceId::new("foreign").unwrap()),
            &mapping,
            result()
        ),
        Err(Error::Schema)
    );
}
