//! EXECUTABLE MCP + real CG-09 evaluation + canonical evidence/context. GitHub
//! HTTPS alone is replaced with a deterministic curl peer; no live effects.
use gateway_application::{
    connector_manifest::*, connector_policy::*, connector_results::ConnectorResult, connectors::*,
};
use gateway_daemon::{
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
fn peer() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/github-connector-v1/curl-peer.py")
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
            authenticated_source: text("test.credentialed-github-peer"),
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
fn host() -> (
    McpConnectorHost,
    ConnectorTarget,
    Arc<AtomicBool>,
    Arc<AtomicUsize>,
) {
    let bundle = GitHubConnector::new("owner/repo", NoIo).unwrap().bundle();
    let manifest =
        ConnectorManifest::from_json(&serde_json::to_vec(&bundle["manifest"]).unwrap()).unwrap();
    let document =
        MappingDocument::from_json(&serde_json::to_vec(&bundle["mapping"]).unwrap()).unwrap();
    let target = ConnectorTarget {
        connector: text("github.reference"),
        scope: ContextScopeId::new("project.github").unwrap(),
    };
    let profile = CredentialProfileRef::new(text("auth.github.readonly"));
    let resolver = Arc::new(ReloadableCredentialResolver::new());
    let tools = bundle["discovery"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| text(v["name"].as_str().unwrap()))
        .collect();
    resolver.replace(
        &profile,
        ResolvedCredential::new(
            [("GITHUB_PERSONAL_ACCESS_TOKEN".into(), "unit-secret".into())].into(),
            CredentialScope {
                connector: target.connector.clone(),
                server: text("github.reference.local"),
                tools,
            },
            1,
        ),
    );
    let factory = CredentialedStdioFactory::new(
        StdioConfig {
            executable: PathBuf::from(env!("CARGO_BIN_EXE_cg-github-mcp")),
            arguments: vec![
                "stdio".into(),
                "owner/repo".into(),
                peer().to_string_lossy().into_owned(),
            ],
            directory: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            environment: coverage_environment(),
        },
        resolver,
        profile,
        "github.reference",
        "github.reference.local",
    );
    let denied = Arc::new(AtomicBool::new(false));
    let admission = Arc::new(Cg09ConnectorAdmission::new(PolicySource {
        denied: denied.clone(),
        scope: target.scope.clone(),
    }));
    let mapping = CanonicalConnectorMapping::new(
        DiscoveryMapper::new(target.clone(), manifest.clone(), document).unwrap(),
        Box::new(GitHubResultNormalizer::new("owner/repo", Arc::new(Collector)).unwrap()),
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
#[test]
fn github_connector_executable_github_issue_repository_pr_ci_to_scoped_evidence_and_context_through_cg09()
 {
    let (host, target, denied, sends) = host();
    host.lifecycle(&target, ConnectorLifecycleCommand::Connect)
        .unwrap();
    let inventory = host.discover(&target).unwrap();
    for (name, args, revision) in [
        (
            "github.repository.read",
            BTreeMap::new(),
            "2026-10-29T10:00:00Z",
        ),
        (
            "github.issue.read",
            [(text("number"), TypedValue::Integer(233))].into(),
            "2026-10-29T10:01:00Z",
        ),
        (
            "github.pull_request.read",
            [(text("number"), TypedValue::Integer(7))].into(),
            "2026-10-29T10:02:00Z",
        ),
        (
            "github.workflow.read",
            [(text("number"), TypedValue::Integer(42))].into(),
            "2026-10-29T10:03:00Z",
        ),
    ] {
        let request = ConnectorRequest {
            target: target.clone(),
            capability: CapabilityId::new(name).unwrap(),
            class: ConnectorOperationClass::Read,
            inventory_revision: inventory.revision.clone(),
            arguments: args,
            task: TaskId::new("task").unwrap(),
            step: PlanStepId::new("step").unwrap(),
            execution: text("execution"),
            correlation: text("read"),
        };
        let observation = host.invoke(&request).unwrap();
        assert_eq!(observation.target.scope, target.scope);
        assert_eq!(
            observation.source_revision.as_ref().unwrap().as_str(),
            revision
        );
        let dispatch = ConnectorDispatch {
            request: request.clone(),
            policy: PolicyId::new("policy").unwrap(),
            policy_revision: digest(),
            consent_receipt: None,
            budget_reservation: text("read.reservation"),
            deadline: UnixTimestamp::new(now() + 30),
        };
        let result = ConnectorResult::new(&dispatch, observation).unwrap();
        assert_eq!(result.batch().unwrap().scope(), &target.scope);
        assert!(!result.fragments().unwrap().is_empty());
        let serialized = format!("{:?}", result.observation());
        assert!(!serialized.contains("unit-secret"));
        let sent_before = sends.load(Ordering::SeqCst);
        denied.store(true, Ordering::SeqCst);
        assert!(host.invoke(&request).is_err());
        denied.store(false, Ordering::SeqCst);
        let mut foreign = request.clone();
        foreign.target.scope = ContextScopeId::new("foreign").unwrap();
        assert!(host.invoke(&foreign).is_err());
        let mut mutate = request.clone();
        mutate.class = ConnectorOperationClass::Mutate;
        assert!(host.invoke(&mutate).is_err());
        let mut unknown = request;
        unknown.capability = CapabilityId::new("github.issue.comment").unwrap();
        assert!(host.invoke(&unknown).is_err());
        assert_eq!(sends.load(Ordering::SeqCst), sent_before);
    }
}
#[test]
fn github_connector_curl_is_bounded_secret_safe_and_projects_http_failures_without_provider_text() {
    let reader = CurlReader::new(peer(), Some("unit-secret".into())).unwrap();
    assert!(reader.get("https://evil.example/repos/owner/repo").is_err());
    assert!(reader.get("https://api.github.com/x\n").is_err());
    assert!(CurlReader::new("relative".into(), None).is_err());
    for token in ["", "bad\nheader", "bad\"quote", &"x".repeat(4097)] {
        assert!(CurlReader::new(peer(), Some(token.into())).is_err());
    }
    for (suffix, error) in [
        ("http-401", Failure::Authentication),
        ("http-403", Failure::Permission),
        ("http-404", Failure::NotFound),
        ("http-429", Failure::RateLimited),
        ("http-500", Failure::Unavailable),
        ("failure", Failure::Unavailable),
        ("oversized", Failure::Payload),
        ("malformed", Failure::Payload),
        ("disclosure", Failure::Payload),
        ("encoded-disclosure", Failure::Payload),
    ] {
        assert_eq!(
            reader.get(&format!("https://api.github.com/{suffix}")),
            Err(error),
            "{suffix}"
        );
    }
    assert!(
        CurlReader::new(peer(), None)
            .unwrap()
            .get("https://api.github.com/repos/owner/repo")
            .is_ok()
    );
    assert_eq!(
        CurlReader::new("/nonexistent/curl".into(), None)
            .unwrap()
            .get("https://api.github.com/x"),
        Err(Failure::Unavailable)
    );
}

#[test]
fn github_connector_shipped_commands_export_config_resolve_references_and_reject_invalid_invocations()
 {
    use std::process::{Command, Stdio};
    let binary = env!("CARGO_BIN_EXE_cg-github-mcp");
    for args in [
        vec!["describe", "owner/repo"],
        vec!["resolve", "owner/repo", "owner/repo#233"],
        vec!["stdio", "owner/repo"],
        vec!["stdio", "owner/repo", peer().to_str().unwrap()],
    ] {
        let result = Command::new(binary)
            .args(args)
            .env_clear()
            .envs(coverage_environment())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    for args in [
        vec![],
        vec!["stdio"],
        vec!["describe", "invalid"],
        vec!["resolve", "owner/repo", "foreign/repo#233"],
        vec!["unknown", "owner/repo"],
        vec!["resolve", "owner/repo"],
    ] {
        let result = Command::new(binary)
            .args(args)
            .env_clear()
            .envs(coverage_environment())
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("unit-secret"));
    }
}
