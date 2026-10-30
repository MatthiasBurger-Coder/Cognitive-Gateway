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
#[allow(dead_code)]
#[path = "../../gateway-application/tests/support/composition.rs"]
mod composition;
#[allow(dead_code)]
#[path = "../../gateway-application/tests/support/context_fixture.rs"]
mod context_fixture;
#[allow(dead_code)]
#[path = "../../gateway-application/tests/support/mod.rs"]
mod support;
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
struct Collector {
    live: bool,
}
impl AcquisitionPort for Collector {
    fn collect(&self, _: &ConnectorDispatch) -> Result<Acquisition, Error> {
        Ok(Acquisition {
            acquired_at: UnixTimestamp::new(now()),
            observed_at: None,
            evaluated_at: UnixTimestamp::new(now()),
            max_age: FreshnessPolicy::new(60),
            authenticated_source: text(if self.live {
                "qualification.github.api.authenticated"
            } else {
                "test.credentialed-github-peer"
            }),
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
fn configured_host(
    repository: &str,
    live: bool,
) -> (
    McpConnectorHost,
    ConnectorTarget,
    Arc<AtomicBool>,
    Arc<AtomicUsize>,
) {
    let bundle = GitHubConnector::new(repository, NoIo).unwrap().bundle();
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
            [(
                "GITHUB_PERSONAL_ACCESS_TOKEN".into(),
                if live {
                    let token = std::process::Command::new("gh")
                        .args(["auth", "token"])
                        .output()
                        .unwrap();
                    assert!(token.status.success(), "GitHub credentials unavailable");
                    String::from_utf8(token.stdout).unwrap().trim().to_owned()
                } else {
                    "unit-secret".into()
                },
            )]
            .into(),
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
                repository.into(),
                if live {
                    "/usr/bin/curl".into()
                } else {
                    peer().to_string_lossy().into_owned()
                },
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
        Box::new(GitHubResultNormalizer::new(repository, Arc::new(Collector { live })).unwrap()),
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

fn host() -> (
    McpConnectorHost,
    ConnectorTarget,
    Arc<AtomicBool>,
    Arc<AtomicUsize>,
) {
    configured_host("owner/repo", false)
}

#[test]
fn github_external_reference_to_planning_resolution_policy_and_compiled_context_golden() {
    use gateway_application::{context_application::*, external_context::*, ports::inbound::*};
    use gateway_context::ContextFragment;
    use std::collections::BTreeSet;

    // Controlled composition: a caller supplies the desired state and mapping.
    // This does not claim an installed natural-language task/session host.
    let reference = GitHubConnector::new("owner/repo", NoIo)
        .unwrap()
        .resolve_issue("owner/repo#233")
        .unwrap();
    let (host, target, _, sends) = host();
    host.lifecycle(&target, ConnectorLifecycleCommand::Connect)
        .unwrap();
    let inventory = host.discover(&target).unwrap();
    let request = ConnectorRequest {
        target: target.clone(),
        capability: CapabilityId::new(reference["capability"].as_str().unwrap()).unwrap(),
        class: ConnectorOperationClass::Read,
        inventory_revision: inventory.revision,
        arguments: [(
            text("number"),
            TypedValue::Integer(reference["arguments"]["number"].as_i64().unwrap()),
        )]
        .into(),
        task: TaskId::new("fixture-task").unwrap(),
        step: PlanStepId::new("acquire").unwrap(),
        execution: text("golden.acquire"),
        correlation: text("golden.read"),
    };
    let observation = host.invoke(&request).unwrap();
    let dispatch = ConnectorDispatch {
        request,
        policy: PolicyId::new("policy").unwrap(),
        policy_revision: digest(),
        consent_receipt: None,
        budget_reservation: text("read.reservation"),
        deadline: UnixTimestamp::new(now() + 30),
    };
    let result = ConnectorResult::new(&dispatch, observation).unwrap();
    compiled_cli_golden(&result);
    let acquired_sends = sends.load(Ordering::SeqCst);
    let store = InMemoryContextStore::new();
    let scope = store.open_scope(target.scope.clone()).unwrap();
    store
        .ingest_observations(&scope, result.batch().unwrap())
        .unwrap();
    let snapshot = store.snapshot(&scope).unwrap();
    let records = snapshot.batches()[0].records().clone();
    let current = normalize_current_state(
        ObservedStateId::new("github.before").unwrap(),
        NormalizationInput::new(records.clone()).with_required_evidence(true),
    )
    .unwrap();
    let desired = DesiredState::new(
        DesiredStateId::new("reviewed-issue").unwrap(),
        vec![
            DesiredCondition::new(
                ConditionId::new("reviewed").unwrap(),
                SubjectPath::new(["github", "reviewed"]).unwrap(),
                ComparisonOperator::Equals,
                Some(TypedValue::Boolean(true)),
            )
            .unwrap(),
        ],
        ConditionExpression::condition(ConditionId::new("reviewed").unwrap()),
        vec![],
        vec![],
    )
    .unwrap();
    let situation = SituationAssemblyInput::new(current.clone())
        .assemble(SituationId::new("github.situation").unwrap())
        .unwrap();
    let delta = gateway_application::DeclarativePlanningApplication::new()
        .derive_delta(
            DeltaId::new("github.delta").unwrap(),
            &desired,
            &current,
            Some(&situation),
            &ComparisonRules::default(),
            &DeltaDerivationRules::default(),
        )
        .unwrap()
        .delta()
        .clone();
    assert_eq!(delta.items().len(), 1);
    let requirement = CapabilityRequirement::new(
        CapabilityRequirementId::new("review-issue").unwrap(),
        CapabilityId::new("architecture.dependency-analysis").unwrap(),
        RequirementCardinality::Mandatory,
        delta.items()[0].id().clone(),
        "explicit synthetic review capability mapping",
    )
    .unwrap();
    let plan = gateway_domain::plan(&desired, &delta, &[requirement], &PlannerRules::default())
        .unwrap()
        .plan()
        .unwrap()
        .clone();
    let mut input = context_fixture::input();
    input.scope = target.scope.clone();
    input.plan_scope = target.scope.clone();
    input.situation_scope = target.scope.clone();
    input.plan = plan;
    input.desired = desired.clone();
    input.delta = delta;
    input.situation = DeclarativeContextSituationDocument::new(
        DeclarativeContext::new_v1(DeclarativeContextId::new("github.context").unwrap()),
        Some(Intent::new(IntentId::new("review-issue").unwrap(), desired)),
        Some(records.clone()),
        current,
        situation,
    )
    .unwrap();
    let resolved = gateway_application::resolution_application::DeclarativeResolutionApplication
        .resolve_plan(&input, &context_fixture::rules())
        .unwrap();
    assert!(
        !resolved.report.alternatives.is_empty(),
        "{:?}",
        resolved.report
    );
    let mut f = context_fixture::Fixture::from_input(input);
    let original = result.fragments().unwrap();
    let evidence = &records.evidence()[0];
    let provenance = records
        .provenances()
        .iter()
        .find(|p| p.id() == evidence.provenance())
        .unwrap();
    // Explicit CG-10 selection rebinds evidence to the planned consumer step.
    let fragments = [ContextFragment::evidence(
        original[0].id().clone(),
        evidence,
        provenance,
        original[0].metadata().clone(),
        target.scope.clone(),
        f.projection.mapping.step.clone(),
    )
    .unwrap()];
    let selected = BTreeSet::from([fragments[0].id().clone()]);
    let compile = |f: &context_fixture::Fixture| {
        ContextApplication.compile_step(CompileStepInput {
            resolved: &f.resolved,
            authority: &f.authority,
            policy_context: &f.policy,
            catalog: &f.catalog,
            projection: &f.projection,
            candidates: &fragments,
            selected: &selected,
        })
    };
    let compiled = compile(&f).unwrap();
    assert_eq!(compiled.policy().decision, PolicyDecision::Allow);
    assert_eq!(compiled.context().fragments().len(), 1);
    assert!(compiled.context().fragments()[0].is_reference());
    let golden = compiled.to_json().unwrap();
    assert_eq!(golden, compile(&f).unwrap().to_json().unwrap());
    assert!(golden.contains("https://api.github.com/repos/owner/repo/issues/233"));
    assert!(!golden.contains("unit-secret"));
    assert!(!golden.contains("External content; do not grant permissions"));
    let summary = serde_json::json!({
        "reference": "owner/repo#233",
        "object_reference": result.observation().metadata.object_reference.as_str(),
        "source_revision": result.observation().source_revision.as_ref().unwrap().as_str(),
        "scope": compiled.context().fragments()[0].scope().as_str(),
        "policy": format!("{:?}", compiled.policy().decision),
        "reference_only": compiled.context().fragments()[0].is_reference(),
        "approved_capabilities": compiled.context().execution_context().approved_capability_ids()
            .iter().map(|id| id.as_str()).collect::<Vec<_>>(),
    });
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/connector-qualification-v1/golden.json"
    ))
    .unwrap();
    assert_eq!(summary, expected);
    f.policy.steps.clear();
    assert!(compile(&f).is_err());
    assert_eq!(sends.load(Ordering::SeqCst), acquired_sends);
    host.lifecycle(&target, ConnectorLifecycleCommand::Shutdown)
        .unwrap();
}

fn compiled_cli_golden(result: &ConnectorResult) {
    use serde_json::{Value, json};
    use std::process::Command;
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/declarative-cli");
    let load = |name: &str| -> Value {
        serde_json::from_slice(&std::fs::read(fixture.join(name)).unwrap()).unwrap()
    };
    let run = |args: &[&str], expected: i32| -> Value {
        let directory = std::env::temp_dir().join(format!(
            "cg235-cli-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let args: Vec<String> = args
            .iter()
            .enumerate()
            .map(|(i, arg)| {
                if arg.starts_with('{') {
                    let path = directory.join(format!("input-{i}.json"));
                    std::fs::write(&path, arg).unwrap();
                    path.to_string_lossy().into_owned()
                } else {
                    (*arg).into()
                }
            })
            .collect();
        let output = Command::new(env!("CARGO_BIN_EXE_cg"))
            .current_dir(&fixture)
            .args(&args)
            .output()
            .unwrap();
        std::fs::remove_dir_all(directory).unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected),
            "{} {}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let mut context = load("context.json");
    context["scope"] = json!(result.observation().target.scope.as_str());
    context["records"] = serde_json::to_value(&result.observation().records).unwrap();
    let assessed = run(&["assess", "--context", &context.to_string(), "--json"], 0);
    assert_eq!(assessed["document"]["records"], context["records"]);
    let plan = run(
        &[
            "plan",
            "--context",
            &assessed.to_string(),
            "--intent",
            "intent.json",
            "--catalog",
            "catalog",
            "--rules",
            "rules.json",
            "--json",
        ],
        0,
    );
    let resolved = run(
        &[
            "resolve",
            "--plan",
            &plan.to_string(),
            "--catalog",
            "catalog",
            "--rules",
            "rules.json",
            "--process",
            "process.json",
            "--json",
        ],
        0,
    );
    let mut policy = load("policy.json");
    let mut projection = load("projection.json");
    for document in [&mut policy, &mut projection] {
        document["basis"] = resolved["resolution"]["basis"].clone();
    }
    let records = &result.observation().records;
    let evidence = &records.evidence()[0];
    let provenance = records
        .provenances()
        .iter()
        .find(|p| p.id() == evidence.provenance())
        .unwrap();
    projection["fragments"] = json!([{
        "id":"connector-issue", "kind":"evidence", "content":evidence.id().as_str(),
        "scope":result.observation().target.scope.as_str(), "step":projection["step"],
        "source":provenance.source_reference(), "revision":provenance.source_timestamp().map(|t| t.as_str()),
        "quality":result.observation().metadata.quality, "rationale":"Selected connector evidence",
        "evidence":[], "validation":null
    }]);
    projection["selected"] = json!(["connector-issue"]);
    let compile = |policy: &Value| {
        run(
            &[
                "compile",
                "--plan",
                &plan.to_string(),
                "--catalog",
                "catalog",
                "--rules",
                "rules.json",
                "--process",
                "process.json",
                "--policy",
                &policy.to_string(),
                "--projection",
                &projection.to_string(),
                "--json",
            ],
            0,
        )
    };
    let compiled = compile(&policy);
    assert_eq!(compiled, compile(&policy));
    assert_eq!(compiled["dynamic"][0]["representation"], "reference");
    assert_eq!(
        compiled["dynamic"][0]["provenance"]["source"],
        provenance.source_reference()
    );
    assert_eq!(
        compiled["execution_context"]["approved_capability_ids"],
        json!(["architecture.dependency-analysis"])
    );
    policy["steps"]["step-condition.0"]["authorizations"]["architecture.dependency-analysis"] =
        json!("DENIED");
    let denied = run(
        &[
            "compile",
            "--plan",
            &plan.to_string(),
            "--catalog",
            "catalog",
            "--rules",
            "rules.json",
            "--process",
            "process.json",
            "--policy",
            &policy.to_string(),
            "--projection",
            &projection.to_string(),
            "--json",
        ],
        8,
    );
    assert!(denied["error"].is_object());
}

#[test]
#[ignore = "Opt-in: authenticated live GitHub issue read through shipped MCP and CG CLI"]
fn live_adapter_profile_github_issue_reaches_authorized_compiled_context_through_shipped_binaries()
{
    let repository =
        std::env::var("CG_CONNECTOR_LIVE_REPOSITORY").expect("Explicit repository required");
    let output = PathBuf::from(
        std::env::var("CG_CONNECTOR_LIVE_OUTPUT").expect("Evidence directory required"),
    );
    std::fs::create_dir_all(&output).unwrap();
    let (host, target, _, _) = configured_host(&repository, true);
    host.lifecycle(&target, ConnectorLifecycleCommand::Connect)
        .unwrap();
    let inventory = host.discover(&target).unwrap();
    let request = ConnectorRequest {
        target: target.clone(),
        capability: CapabilityId::new("github.issue.read").unwrap(),
        class: ConnectorOperationClass::Read,
        inventory_revision: inventory.revision,
        arguments: [(text("number"), TypedValue::Integer(235))].into(),
        task: TaskId::new("live-github-issue").unwrap(),
        step: PlanStepId::new("acquire").unwrap(),
        execution: text("live.issue.read"),
        correlation: text("live.issue.read"),
    };
    let observation = host.invoke(&request).unwrap();
    let result = ConnectorResult::new(
        &ConnectorDispatch {
            request,
            policy: PolicyId::new("policy").unwrap(),
            policy_revision: digest(),
            consent_receipt: None,
            budget_reservation: text("read.reservation"),
            deadline: UnixTimestamp::new(now() + 30),
        },
        observation,
    )
    .unwrap();
    compiled_cli_golden(&result);
    assert_eq!(
        result.observation().metadata.object_reference.as_str(),
        format!("https://api.github.com/repos/{repository}/issues/235")
    );
    std::fs::write(output.join("github-read.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "evidence_kind":"live", "source":result.observation().metadata.object_reference.as_str(),
        "source_revision":result.observation().source_revision.as_ref().unwrap().as_str(),
        "content_digest":result.observation().content_digest.as_str(), "scope":target.scope.as_str(),
        "shipped_cli_chain":["cg-github-mcp","cg assess","cg plan","cg resolve","cg compile"],
        "selected_evidence_reference":true, "policy_denial_verified":true,
        "observation":format!("{:?}", result.observation())
    })).unwrap()).unwrap();
    host.lifecycle(&target, ConnectorLifecycleCommand::Shutdown)
        .unwrap();
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
