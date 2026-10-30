//! Opt-in authenticated external effects. Never run by offline workspace tests.
use gateway_application::{
    connector_manifest::*,
    connector_policy::*,
    connector_resilience::{self as recovery, ConnectorRecoveryPort},
    connector_results::ConnectorResult,
    connectors::*,
};
use gateway_daemon::mcp_client::{mapping::*, normalization::*, transport::*, *};
use gateway_domain::*;
use gateway_policy::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn text(s: &str) -> NonEmptyText {
    NonEmptyText::new(s).unwrap()
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn digest() -> ContentDigest {
    ContentDigest::new("a".repeat(64)).unwrap()
}
fn gh(args: &[&str]) -> Vec<u8> {
    let output = Command::new("gh")
        .args(args)
        .output()
        .expect("gh executable required");
    assert!(
        output.status.success(),
        "GitHub operation failed (provider diagnostics withheld)"
    );
    output.stdout
}
struct Cleanup {
    repository: String,
    branch: String,
    receipt: PathBuf,
}
impl Cleanup {
    fn remove(&self) {
        if self.receipt.exists() {
            let receipt: Value =
                serde_json::from_slice(&std::fs::read(&self.receipt).unwrap()).unwrap();
            assert_eq!(receipt["branch"], self.branch);
            gh(&[
                "api",
                "--method",
                "DELETE",
                &format!("repos/{}/git/refs/heads/{}", self.repository, self.branch),
            ]);
            std::fs::remove_file(&self.receipt).unwrap();
        }
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        self.remove();
    }
}

struct Source {
    target: ConnectorTarget,
    current: Arc<Mutex<ConnectorRequest>>,
    consent: Arc<AtomicBool>,
}
impl Source {
    fn snapshot(&self, r: &ConnectorRequest) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        if self.current.lock().unwrap().ne(r) {
            return Err(denied(r));
        }
        let mutate = r.class == ConnectorOperationClass::Mutate;
        let cap = r.capability.clone();
        Ok(ConnectorPolicySnapshot {
            step: r.step.clone(),
            resolved: true,
            has_prerequisites: false,
            constraints: Default::default(),
            preconditions: Default::default(),
            authority: PolicyAuthority {
                policies: vec![
                    PolicyDefinition::new(
                        PolicyId::new("live-policy").unwrap(),
                        "exact isolated ref operation",
                        [cap.clone()],
                    )
                    .unwrap(),
                ],
                capabilities: [(
                    cap.clone(),
                    CapabilityDefinition::new(
                        cap.clone(),
                        if mutate {
                            CapabilityClass::Mutate
                        } else {
                            CapabilityClass::Inspect
                        },
                    ),
                )]
                .into(),
                ..Default::default()
            },
            facts: StepFacts {
                authorizations: [(cap.clone(), Approval::Granted)].into(),
                consents: [(
                    cap,
                    if self.consent.load(Ordering::SeqCst) {
                        Approval::Granted
                    } else {
                        Approval::Denied
                    },
                )]
                .into(),
                ..Default::default()
            },
            operating_mode: OperatingMode::Development,
            execution_profile: ExecutionProfile::FullPath,
            process: ProcessReadiness::Eligible,
            consent_required: mutate,
            administrative_capabilities: Default::default(),
            policy: PolicyId::new("live-policy").unwrap(),
            policy_revision: digest(),
            consent_receipt: mutate.then(|| text("exact-live-ref-consent")),
            budget_reservation: text("live-bounded-operation"),
            deadline: UnixTimestamp::new(now() + 60),
        })
    }
}
fn denied(r: &ConnectorRequest) -> ConnectorFailure {
    ConnectorFailure {
        code: ConnectorFailureCode::PermissionDenied,
        effect: ConnectorEffect::NotDispatched,
        correlation: r.correlation.clone(),
    }
}
impl ConnectorPolicySource for Source {
    fn admit_target(&self, t: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        if *t == self.target {
            Ok(())
        } else {
            Err(denied(&self.current.lock().unwrap()))
        }
    }
    fn authorize(
        &self,
        r: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.snapshot(r)
    }
    fn revalidate(
        &self,
        d: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.snapshot(&d.request)
    }
    fn record_decision(&self, _: &ConnectorPolicyEvaluation<'_>) -> Result<(), ConnectorFailure> {
        Ok(())
    }
}
struct Collector {
    original: Option<ConnectorRequest>,
}
impl AcquisitionPort for Collector {
    fn collect(&self, d: &ConnectorDispatch) -> Result<Acquisition, Error> {
        let mutation = d.request.class == ConnectorOperationClass::Mutate;
        Ok(Acquisition {
            acquired_at: UnixTimestamp::new(now()),
            observed_at: None,
            evaluated_at: UnixTimestamp::new(now()),
            max_age: FreshnessPolicy::new(60),
            authenticated_source: text("qualification.github.api.authenticated"),
            sensitivity: SensitivityClass::Internal,
            confidence: Confidence::Unknown,
            uncertainty: Uncertainty::Unknown,
            conflict: ConflictStatus::None,
            partial: false,
            basis: if mutation {
                ConnectorObservationBasis::ProviderAcknowledgement
            } else {
                ConnectorObservationBasis::IndependentPostEffectRead
            },
            effect: if mutation {
                ConnectorEffect::Acknowledged
            } else {
                ConnectorEffect::NoMutation
            },
            verifies: if mutation {
                None
            } else {
                self.original.clone()
            },
            non_authoritative_raw: None,
        })
    }
}
fn bundle() -> Value {
    let mut bundle: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/conformance-connector-v1/standard.json"
    ))
    .unwrap();
    let output = json!({"type":"object","properties":{"uri":{"type":"string"},"sha":{"type":"string"},"exists":{"type":"boolean"}},"required":["uri","sha","exists"],"additionalProperties":false});
    let mut tools = vec![];
    let mut entries = vec![];
    let mut records = vec![];
    for (name, mutate) in [("refs.create", true), ("refs.read", false)] {
        let mut properties = json!({"ref":{"type":"string"}});
        let mut required = vec!["ref"];
        if mutate {
            properties["sha"] = json!({"type":"string"});
            required.push("sha");
        }
        let tool = json!({"name":name,"description":"Isolated live GitHub qualification ref","inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"outputSchema":output});
        let mapping = format!("mapping.{name}.v1");
        entries.push(json!({"kind":"tool","name":name,"schema_digest":discovery_schema_digest(InventoryKind::Tool,&tool).as_str(),"capability_mapping_ref":mapping,"source_mapping_ref":mapping}));
        records.push(json!({"kind":"tool","name":name,"reference":mapping,"capability":name,"class":if mutate {"MUTATE"} else {"READ"},
            "consent":if mutate {"required"} else {"policy"},"idempotency":if mutate {"non_idempotent"} else {"idempotent"},
            "retry_safety":if mutate {"never"} else {"read_only"},"side_effects":if mutate {"mutation"} else {"none"},"source":format!("source.{name}"),"result_schema":output}));
        tools.push(tool);
    }
    bundle["discovery"]["tools"] = json!(tools);
    let mut definition = bundle["manifest"]["manifest"].clone();
    definition["inventory"] = json!(entries);
    definition["connector_id"] = json!("qualification.github.live");
    definition["server_id"] = json!("qualification.github.live");
    definition["profiles"]["authentication"] = json!("auth.github.qualification");
    let manifest =
        ConnectorManifest::from_definition_json(&serde_json::to_vec(&definition).unwrap()).unwrap();
    bundle["manifest"] = serde_json::from_slice(&manifest.canonical_json()).unwrap();
    bundle["mapping"]["records"] = json!(records);
    bundle
}
fn host(
    directory: &Path,
    repository: &str,
    branch: &str,
    mode: &str,
    request: &ConnectorRequest,
    original: Option<ConnectorRequest>,
) -> (
    McpConnectorHost,
    Arc<Mutex<ConnectorRequest>>,
    Arc<AtomicBool>,
) {
    let bundle = bundle();
    let manifest =
        ConnectorManifest::from_json(&serde_json::to_vec(&bundle["manifest"]).unwrap()).unwrap();
    let document =
        MappingDocument::from_json(&serde_json::to_vec(&bundle["mapping"]).unwrap()).unwrap();
    let bundle_path = directory.join("bundle.json");
    std::fs::write(&bundle_path, serde_json::to_vec(&bundle).unwrap()).unwrap();
    let token = String::from_utf8(gh(&["auth", "token"]))
        .unwrap()
        .trim()
        .to_owned();
    let resolver = Arc::new(ReloadableCredentialResolver::new());
    let profile = CredentialProfileRef::new(text("auth.github.qualification"));
    resolver.replace(
        &profile,
        ResolvedCredential::new(
            [("GITHUB_PERSONAL_ACCESS_TOKEN".into(), token)].into(),
            CredentialScope {
                connector: request.target.connector.clone(),
                server: text("qualification.github.live"),
                tools: [text("refs.create"), text("refs.read")].into(),
            },
            1,
        ),
    );
    let current = Arc::new(Mutex::new(request.clone()));
    let consent = Arc::new(AtomicBool::new(false));
    let factory = CredentialedStdioFactory::new(
        StdioConfig {
            executable: "/usr/bin/python3".into(),
            arguments: vec![
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/fixtures/connector-qualification-v1/live-peer.py")
                    .to_string_lossy()
                    .into_owned(),
                bundle_path.to_string_lossy().into_owned(),
                repository.into(),
                branch.into(),
                mode.into(),
                directory
                    .join("created.json")
                    .to_string_lossy()
                    .into_owned(),
            ],
            directory: directory.into(),
            environment: BTreeMap::new(),
        },
        resolver,
        profile,
        "qualification.github.live",
        "qualification.github.live",
    );
    let normalizer = CanonicalResultNormalizer::new(
        NormalizationProfile {
            source: SourceId::new(format!("source.{}", request.capability.as_str())).unwrap(),
            source_kind: SourceKind::Tool,
            object_field: "uri".into(),
            revision_field: Some("sha".into()),
            digest_field: None,
            fields: [
                (
                    "sha".into(),
                    SubjectPath::new(["github", "branch", "sha"]).unwrap(),
                ),
                (
                    "exists".into(),
                    SubjectPath::new(["github", "branch", "exists"]).unwrap(),
                ),
            ]
            .into(),
        },
        Box::new(Collector { original }),
    )
    .unwrap();
    let host = McpConnectorHost::new(
        HostConfig {
            target: request.target.clone(),
            manifest: manifest.clone(),
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
        Arc::new(Cg09ConnectorAdmission::new(Source {
            target: request.target.clone(),
            current: current.clone(),
            consent: consent.clone(),
        })),
        Box::new(CanonicalConnectorMapping::new(
            DiscoveryMapper::new(request.target.clone(), manifest, document).unwrap(),
            Box::new(normalizer),
        )),
        Arc::new(()),
    )
    .unwrap();
    (host, current, consent)
}

#[test]
#[ignore = "Opt-in: creates and removes uniquely named live GitHub branches"]
fn live_adapter_profile_controlled_effect_acknowledgement_and_lost_response_require_independent_readback()
 {
    let repository =
        std::env::var("CG_CONNECTOR_LIVE_REPOSITORY").expect("Explicit live repository required");
    let output = PathBuf::from(
        std::env::var("CG_CONNECTOR_LIVE_OUTPUT").expect("Evidence directory required"),
    );
    std::fs::create_dir_all(&output).unwrap();
    let mut scenarios = vec![];
    let sha = String::from_utf8(gh(&[
        "api",
        &format!("repos/{repository}/git/ref/heads/main"),
        "--jq",
        ".object.sha",
    ]))
    .unwrap()
    .trim()
    .to_owned();
    for mode in ["ack", "lost-response"] {
        let branch = format!(
            "cg-qualification-235-{}-{mode}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let directory = output.join(mode);
        std::fs::create_dir(&directory).unwrap();
        let cleanup = Cleanup {
            repository: repository.clone(),
            branch: branch.clone(),
            receipt: directory.join("created.json"),
        };
        let mut mutation = ConnectorRequest {
            target: ConnectorTarget {
                connector: text("qualification.github.live"),
                scope: ContextScopeId::new("qualification.epic07").unwrap(),
            },
            capability: CapabilityId::new("refs.create").unwrap(),
            class: ConnectorOperationClass::Mutate,
            inventory_revision: digest(),
            arguments: [
                (
                    text("ref"),
                    TypedValue::string(format!("refs/heads/{branch}")).unwrap(),
                ),
                (text("sha"), TypedValue::string(&sha).unwrap()),
            ]
            .into(),
            task: TaskId::new("live-ref").unwrap(),
            step: PlanStepId::new("create").unwrap(),
            execution: text("live.effect"),
            correlation: text("live.create"),
        };
        let (writer, current, consent) =
            host(&directory, &repository, &branch, mode, &mutation, None);
        writer
            .lifecycle(&mutation.target, ConnectorLifecycleCommand::Connect)
            .unwrap();
        mutation.inventory_revision = writer.discover(&mutation.target).unwrap().revision;
        *current.lock().unwrap() = mutation.clone();
        let denied = writer.invoke(&mutation).unwrap_err();
        assert_eq!(denied.effect, ConnectorEffect::NotDispatched);
        assert!(!cleanup.receipt.exists());
        consent.store(true, Ordering::SeqCst);
        let recovery_port: &dyn ConnectorRecoveryPort = &writer;
        let support = recovery_port.recovery_support(&mutation, None).unwrap();
        let port: &dyn ConnectorApplicationPort = &writer;
        match port.invoke(&mutation) {
            Ok(observation) => {
                assert_eq!(mode, "ack");
                let result =
                    ConnectorResult::new(&dispatch(mutation.clone()), observation).unwrap();
                assert!(result.batch().is_err());
            }
            Err(error) => {
                assert_eq!(mode, "lost-response");
                assert_eq!(error.effect, ConnectorEffect::Unknown);
                // #276 consumes this typed refusal; no durable recovery guarantee is claimed.
                assert_eq!(support.retry, recovery::RetrySafety::Never);
                assert_eq!(
                    recovery::RetryPolicy::default().decide(
                        &error,
                        &support,
                        1,
                        Duration::from_secs(30),
                        None
                    ),
                    recovery::RetryDecision::Reconcile(recovery::ReadBackSupport::Unsupported)
                );
            }
        }
        assert!(
            cleanup.receipt.exists(),
            "actual remote effect must have occurred"
        );
        let attempts = writer.health_snapshot().attempts;
        let read_directory = directory.join("independent");
        std::fs::create_dir(&read_directory).unwrap();
        let mut read = mutation.clone();
        read.capability = CapabilityId::new("refs.read").unwrap();
        read.class = ConnectorOperationClass::Read;
        read.arguments.remove(&text("sha"));
        read.step = PlanStepId::new("verify").unwrap();
        read.execution = text("live.independent-read");
        read.correlation = text("live.readback");
        let (reader, binding, _) = host(
            &read_directory,
            &repository,
            &branch,
            "ack",
            &read,
            Some(mutation.clone()),
        );
        reader
            .lifecycle(&read.target, ConnectorLifecycleCommand::Connect)
            .unwrap();
        read.inventory_revision = reader.discover(&read.target).unwrap().revision;
        *binding.lock().unwrap() = read.clone();
        let observation = reader.invoke(&read).unwrap();
        assert_eq!(observation.source_revision.as_ref().unwrap().as_str(), sha);
        assert_eq!(
            observation.basis,
            ConnectorObservationBasis::IndependentPostEffectRead
        );
        let result = ConnectorResult::new(&dispatch(read.clone()), observation).unwrap();
        assert!(result.batch().is_ok());
        assert!(result.fragments().is_ok());
        assert_eq!(
            result.observation().metadata.verifies.as_ref(),
            Some(&mutation)
        );
        assert_eq!(
            writer.health_snapshot().attempts,
            attempts,
            "readback must never resend the mutation"
        );
        let metadata = writer.recovery_support(&mutation, None);
        if mode == "ack" {
            assert!(metadata.is_ok());
        }
        let evidence = json!({"mode":mode,"branch":branch,"sha":sha,"basis":"IndependentPostEffectRead","observation":format!("{:?}",result.observation()),"mutation_replayed":false,"consent_denied_before_dispatch":true});
        reader
            .lifecycle(&read.target, ConnectorLifecycleCommand::Shutdown)
            .unwrap();
        let _ = writer.lifecycle(&mutation.target, ConnectorLifecycleCommand::Shutdown);
        cleanup.remove();
        scenarios.push(evidence);
    }
    std::fs::write(output.join("observations.json"),serde_json::to_vec_pretty(&json!({"evidence_kind":"live","repository":repository,"scenarios":scenarios,"cleanup":"removed-owned-isolated-refs","external_exactly_once_claimed":false})).unwrap()).unwrap();
}
fn dispatch(request: ConnectorRequest) -> ConnectorDispatch {
    ConnectorDispatch {
        request,
        policy: PolicyId::new("live-policy").unwrap(),
        policy_revision: digest(),
        consent_receipt: Some(text("exact-live-ref-consent")),
        budget_reservation: text("live-bounded-operation"),
        deadline: UnixTimestamp::new(now() + 60),
    }
}
