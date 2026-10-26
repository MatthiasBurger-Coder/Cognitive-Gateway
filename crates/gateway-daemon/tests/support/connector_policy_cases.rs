//! #369 COMPONENT evidence: real CG09 + MCP host + independent stdio peer.
//! The source is an explicit trusted-record fixture, not a production source;
//! peer acknowledgements do not qualify real mutations or goal completion.
use super::*;
use gateway_application::connector_policy::*;
use gateway_policy::*;

struct BoundState {
    request: ConnectorRequest,
    source: Option<SourceId>,
    snapshot: ConnectorPolicySnapshot,
    audits: usize,
    revoke_on_pre_effect_audit: bool,
}
struct ExactBoundSource(Arc<Mutex<BoundState>>);
impl ExactBoundSource {
    fn read(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        let current = self.0.lock().unwrap();
        if current.request != *request || current.source.as_ref() != source {
            return Err(denial(request));
        }
        Ok(current.snapshot.clone())
    }
}
fn denial(request: &ConnectorRequest) -> ConnectorFailure {
    ConnectorFailure {
        code: ConnectorFailureCode::PermissionDenied,
        effect: ConnectorEffect::NotDispatched,
        correlation: request.correlation.clone(),
    }
}
impl ConnectorPolicySource for ExactBoundSource {
    fn admit_target(&self, target: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        let current = self.0.lock().unwrap();
        if &current.request.target == target {
            Ok(())
        } else {
            Err(denial(&current.request))
        }
    }
    fn authorize(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.read(request, source)
    }
    fn revalidate(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.read(&dispatch.request, source)
    }
    fn record_decision(
        &self,
        evaluation: &ConnectorPolicyEvaluation<'_>,
    ) -> Result<(), ConnectorFailure> {
        let mut current = self.0.lock().unwrap();
        current.audits += 1;
        if current.revoke_on_pre_effect_audit
            && matches!(evaluation.stage(), ConnectorPolicyStage::PreEffect(_))
        {
            let capability = current.request.capability.clone();
            current
                .snapshot
                .facts
                .consents
                .insert(capability, Approval::Denied);
        }
        Ok(())
    }
}
struct PolicyMapping {
    state: Arc<Mutex<BoundState>>,
    drift: Option<&'static str>,
}
impl ConnectorMapping for PolicyMapping {
    fn inventory(
        &self,
        t: &ConnectorTarget,
        m: &ConnectorManifest,
        w: &ProtocolInventory,
    ) -> Result<ConnectorInventory, Error> {
        Mapping { mode: "normal" }.inventory(t, m, w)
    }
    fn operation(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
        w: &ProtocolInventory,
    ) -> Result<WireOperation, Error> {
        if let Some(drift) = self.drift {
            let mut current = self.state.lock().unwrap();
            let capability = current.request.capability.clone();
            match drift {
                "policy" => {
                    current.snapshot.policy_revision = ContentDigest::new("b".repeat(64)).unwrap()
                }
                "receipt" => current.snapshot.consent_receipt = Some(text("replacement-consent")),
                "inventory" => {
                    current.request.inventory_revision = ContentDigest::new("b".repeat(64)).unwrap()
                }
                _ => {
                    current
                        .snapshot
                        .facts
                        .consents
                        .insert(capability, Approval::Denied);
                }
            }
        }
        Mapping { mode: "normal" }.operation(d, s, w)
    }
    fn normalize(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
        r: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        Mapping { mode: "normal" }.normalize(d, s, r)
    }
}
struct PolicyPeer {
    host: McpConnectorHost,
    state: Arc<Mutex<BoundState>>,
    root: PathBuf,
    log: PathBuf,
}
impl PolicyPeer {
    fn new(
        class: ConnectorOperationClass,
        source: Option<SourceId>,
        drift: Option<&'static str>,
    ) -> Self {
        let mut req = request();
        req.class = class;
        if class == ConnectorOperationClass::Mutate {
            req.capability = CapabilityId::new("records.z-mutate").unwrap();
        }
        let capability = req.capability.clone();
        let native = CapabilityDefinition::new(
            capability.clone(),
            if class == ConnectorOperationClass::Mutate {
                CapabilityClass::Mutate
            } else {
                CapabilityClass::Inspect
            },
        );
        let snapshot = ConnectorPolicySnapshot {
            step: req.step.clone(),
            resolved: true,
            has_prerequisites: false,
            constraints: Default::default(),
            preconditions: Default::default(),
            authority: PolicyAuthority {
                capabilities: BTreeMap::from([(capability.clone(), native)]),
                policies: vec![
                    PolicyDefinition::new(
                        PolicyId::new("operator-policy").unwrap(),
                        "controlled-peer policy",
                        [capability.clone()],
                    )
                    .unwrap(),
                ],
                ..Default::default()
            },
            facts: StepFacts {
                authorizations: BTreeMap::from([(capability.clone(), Approval::Granted)]),
                consents: BTreeMap::from([(capability, Approval::Granted)]),
                ..Default::default()
            },
            operating_mode: OperatingMode::Development,
            execution_profile: ExecutionProfile::FullPath,
            process: ProcessReadiness::Eligible,
            consent_required: false,
            administrative_capabilities: Default::default(),
            policy: PolicyId::new("operator-policy").unwrap(),
            policy_revision: digest(),
            consent_receipt: Some(text("exact-request-consent")),
            budget_reservation: text("operator-reservation"),
            deadline: UnixTimestamp::new(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
                    + 30,
            ),
        };
        let state = Arc::new(Mutex::new(BoundState {
            request: req,
            source,
            snapshot,
            audits: 0,
            revoke_on_pre_effect_audit: false,
        }));
        let root = std::env::temp_dir().join(format!(
            "cg369-peer-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let log = root.join("wire.jsonl");
        let mut transport = config("normal");
        transport.arguments.push(log.to_string_lossy().into_owned());
        let host = McpConnectorHost::new(
            HostConfig {
                target: target(),
                manifest: ConnectorManifest::from_json(include_bytes!(
                    "../../../../tests/fixtures/connector-manifest-v1/valid-stdio.json"
                ))
                .unwrap(),
                profile: ConnectorCompatibilityProfile {
                    gateway: ConnectorVersion {
                        major: 0,
                        minor: 1,
                        patch: 0,
                    },
                    transports: BTreeSet::from([ConnectorTransport::Stdio]),
                },
                limits: Limits::default(),
            },
            Box::new(transport),
            Arc::new(Cg09ConnectorAdmission::new(ExactBoundSource(state.clone()))),
            Box::new(PolicyMapping {
                state: state.clone(),
                drift,
            }),
            Arc::new(Events::default()),
        )
        .unwrap();
        ready(&host);
        assert_eq!(
            state.lock().unwrap().audits,
            0,
            "discovery has no policy grant"
        );
        Self {
            host,
            state,
            root,
            log,
        }
    }
    fn invoke(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        let context = CallContext::new(
            request.correlation.clone(),
            Instant::now() + Duration::from_secs(5),
        );
        self.host.invoke_with_context(request, source, &context)
    }
    fn methods(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap()
            .lines()
            .filter_map(|line| {
                serde_json::from_str::<serde_json::Value>(line).unwrap()["method"]
                    .as_str()
                    .map(str::to_owned)
            })
            .collect()
    }
    fn effects(&self) -> usize {
        self.methods()
            .iter()
            .filter(|m| matches!(m.as_str(), "tools/call" | "resources/read"))
            .count()
    }
    fn assert_denied_without_wire_effect(
        &self,
        result: Result<ConnectorObservation, ConnectorFailure>,
    ) {
        assert_eq!(result.unwrap_err().effect, ConnectorEffect::NotDispatched);
        assert!(
            self.methods().iter().any(|m| m == "tools/list"),
            "actual peer discovery occurred"
        );
        assert_eq!(self.effects(), 0, "peer received no effect frame");
    }
}
impl Drop for PolicyPeer {
    fn drop(&mut self) {
        let _ = self
            .host
            .lifecycle(&target(), ConnectorLifecycleCommand::Shutdown);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn cg09_exact_request_and_source_substitution_matrix_has_zero_peer_effects() {
    for field in [
        "connector",
        "scope",
        "capability",
        "class",
        "arguments",
        "task",
        "step",
        "execution",
        "correlation",
        "inventory",
        "source",
    ] {
        let original_source = SourceId::new("one").unwrap();
        let peer = PolicyPeer::new(
            ConnectorOperationClass::Read,
            Some(original_source.clone()),
            None,
        );
        let mut req = peer.state.lock().unwrap().request.clone();
        // The authenticated fixture record remains pinned to the original,
        // including typed arguments and ordered execution/correlation identity.
        let mut source = Some(original_source);
        match field {
            "connector" => req.target.connector = text("foreign-connector"),
            "scope" => req.target.scope = ContextScopeId::new("foreign-project").unwrap(),
            "capability" => req.capability = CapabilityId::new("records.z-mutate").unwrap(),
            "class" => req.class = ConnectorOperationClass::Mutate,
            "arguments" => {
                req.arguments.insert(
                    text("query"),
                    TypedValue::string("different object").unwrap(),
                );
            }
            "task" => req.task = TaskId::new("foreign-task").unwrap(),
            "step" => req.step = PlanStepId::new("foreign-step").unwrap(),
            "execution" => req.execution = text("foreign-execution"),
            "correlation" => req.correlation = text("replayed-as-another-instance"),
            "inventory" => req.inventory_revision = ContentDigest::new("c".repeat(64)).unwrap(),
            _ => source = Some(SourceId::new("foreign-source").unwrap()),
        }
        peer.assert_denied_without_wire_effect(peer.invoke(&req, source.as_ref()));
    }
}
#[test]
fn cg09_current_policy_and_consent_drift_after_authorization_has_zero_peer_effects() {
    for drift in ["policy", "receipt", "inventory", "revoked"] {
        let peer = PolicyPeer::new(ConnectorOperationClass::Mutate, None, Some(drift));
        let req = peer.state.lock().unwrap().request.clone();
        peer.assert_denied_without_wire_effect(peer.invoke(&req, None));
        assert!(
            peer.state.lock().unwrap().audits >= 1,
            "native CG09 authorization ran"
        );
    }
    let peer = PolicyPeer::new(ConnectorOperationClass::Mutate, None, None);
    peer.state.lock().unwrap().revoke_on_pre_effect_audit = true;
    let req = peer.state.lock().unwrap().request.clone();
    peer.assert_denied_without_wire_effect(peer.invoke(&req, None));
    assert_eq!(
        peer.state.lock().unwrap().audits,
        2,
        "audit revocation checked without recursive audits"
    );
}
#[test]
fn cg09_discovery_and_mapping_metadata_never_supply_authorization_or_consent() {
    for missing in [
        "authorization",
        "missing-consent",
        "denied-consent",
        "metadata-requires-consent",
        "metadata-denied-consent",
    ] {
        let class = if missing.starts_with("metadata-") {
            ConnectorOperationClass::Read
        } else {
            ConnectorOperationClass::Mutate
        };
        let peer = PolicyPeer::new(class, None, None);
        let req = {
            let mut s = peer.state.lock().unwrap();
            match missing {
                "authorization" => s.snapshot.facts.authorizations.clear(),
                "missing-consent" => {
                    s.snapshot.consent_receipt = None;
                    s.snapshot.facts.consents.clear();
                }
                "denied-consent" => {
                    let id = s.request.capability.clone();
                    s.snapshot.facts.consents.insert(id, Approval::Denied);
                }
                "metadata-denied-consent" => {
                    s.snapshot.consent_required = true;
                    let id = s.request.capability.clone();
                    s.snapshot.facts.consents.insert(id, Approval::Denied);
                }
                _ => {
                    s.snapshot.consent_required = true;
                    s.snapshot.consent_receipt = None;
                    s.snapshot.facts.consents.clear();
                }
            }
            s.request.clone()
        };
        peer.assert_denied_without_wire_effect(peer.invoke(&req, None));
        assert_eq!(peer.state.lock().unwrap().audits, 1);
    }
}
#[test]
fn cg09_real_engine_allows_exact_inspect_and_consented_mutation_on_the_same_host_boundary() {
    for class in [
        ConnectorOperationClass::Read,
        ConnectorOperationClass::Mutate,
    ] {
        let peer = PolicyPeer::new(class, None, None);
        let req = peer.state.lock().unwrap().request.clone();
        let observation = peer.invoke(&req, None).unwrap();
        assert_eq!(observation.target, req.target);
        assert_eq!(peer.effects(), 1);
        assert_eq!(peer.state.lock().unwrap().audits, 2);
    }
}
