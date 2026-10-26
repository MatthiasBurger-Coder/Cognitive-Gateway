use super::*;
use gateway_application::resolution_application::{DeclarativeResolutionApplication, ResolvedPlan};
use gateway_process::{ProcessRegistry, ProcessSource};

fn text(s: &str) -> NonEmptyText {
    NonEmptyText::new(s).unwrap()
}
fn digest(v: &impl serde::Serialize) -> ContentDigest {
    context_content_digest(&serde_json::to_vec(v).unwrap())
}
#[derive(Clone)]
pub struct Coordinates {
    pub target: ConnectorTarget,
    pub step: PlanStepId,
    pub execution: NonEmptyText,
}
#[derive(Clone)]
struct Session(Arc<Mutex<Coordinates>>);
impl ConnectorSessionBindingPort for Session {
    fn current_binding(&self, _: &InvocationIr) -> Result<ConnectorSessionBinding, ProposalError> {
        let b = self.0.lock().unwrap();
        Ok(ConnectorSessionBinding {
            target: b.target.clone(),
            step: b.step.clone(),
            execution: b.execution.clone(),
        })
    }
}
pub struct NativePolicy {
    pub resolved: ResolvedPlan,
    pub authority: PolicyAuthority,
    pub context: PolicyContext,
    pub step: PlanStepId,
    pub capability: CapabilityId,
}
fn native_policy(mutate: bool) -> NativePolicy {
    let mut input = composition::fixture();
    input.scope = ContextScopeId::new("project-x").unwrap();
    input.plan_scope = input.scope.clone();
    input.situation_scope = input.scope.clone();
    input.operating_mode = OperatingMode::Development;
    let capability = CapabilityId::new(if mutate {
        "fixture.update"
    } else {
        "architecture.dependency-analysis"
    })
    .unwrap();
    if mutate {
        // A real registered fixture definition and native changed-state plan,
        // rather than changing the resolved flag or borrowing an Inspect grant.
        let mut skill: Value = serde_json::from_str(
            &input
                .registry
                .skill(&SkillId::new("good").unwrap())
                .unwrap()
                .to_json()
                .unwrap(),
        )
        .unwrap();
        skill["id"] = json!("update");
        skill["provided_capabilities"][0]["id"] = json!(capability);
        skill["provided_capabilities"][0]["class"] = json!("MUTATE");
        skill["provided_capabilities"][0]["constraints"] = json!([]);
        let mut skills = input.registry.skills().documents().to_vec();
        skills.push(SkillDefinitionDocument::from_json(&skill.to_string()).unwrap());
        let cap = skill["provided_capabilities"][0].clone();
        let agents: Vec<_> = input
            .registry
            .agents()
            .documents()
            .iter()
            .map(|a| {
                let mut a: Value = serde_json::from_str(&a.to_json().unwrap()).unwrap();
                if a["id"] == "alpha" {
                    a["skill_ids"].as_array_mut().unwrap().push(json!("update"));
                    a["provided_capabilities"]
                        .as_array_mut()
                        .unwrap()
                        .push(cap.clone());
                }
                AgentDefinitionDocument::from_json(&a.to_string()).unwrap()
            })
            .collect();
        input.registry = gateway_registry::Registry::from_documents(agents, skills).unwrap();
        input.index = input.registry.capability_index().unwrap();
        let subject = input.desired.conditions()[0].subject().clone();
        let p = Provenance::new(
            ProvenanceId::new("before.p").unwrap(),
            SourceKind::Tool,
            SourceId::new("before").unwrap(),
            "fixture://before",
        )
        .unwrap();
        let o = Observation::new(
            ObservationId::new("before.o").unwrap(),
            subject.clone(),
            TypedValue::Boolean(false),
            p.id().clone(),
        )
        .unwrap();
        let f = Fact::new(
            FactId::new("before.f").unwrap(),
            subject,
            TypedValue::Boolean(false),
            AssertionPolarity::Affirmed,
            vec![o.id().clone()],
        )
        .unwrap();
        let current = normalize_current_state(
            ObservedStateId::new("before").unwrap(),
            NormalizationInput::new(
                ObservationEvidenceSet::new(vec![p], vec![o], vec![f], vec![]).unwrap(),
            ),
        )
        .unwrap();
        let situation = SituationAssemblyInput::new(current.clone())
            .assemble(SituationId::new("before-situation").unwrap())
            .unwrap();
        input.delta = gateway_application::DeclarativePlanningApplication::new()
            .derive_delta(
                DeltaId::new("change").unwrap(),
                &input.desired,
                &current,
                Some(&situation),
                &ComparisonRules::default(),
                &DeltaDerivationRules::default(),
            )
            .unwrap()
            .delta()
            .clone();
        let requirements = input
            .delta
            .items()
            .iter()
            .enumerate()
            .map(|(i, d)| {
                CapabilityRequirement::new(
                    CapabilityRequirementId::new(format!("update-{i}")).unwrap(),
                    capability.clone(),
                    RequirementCardinality::Mandatory,
                    d.id().clone(),
                    "Explicit fixture update",
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        input.plan = gateway_domain::plan(
            &input.desired,
            &input.delta,
            &requirements,
            &PlannerRules::default(),
        )
        .unwrap()
        .plan()
        .unwrap()
        .clone();
        input.situation = DeclarativeContextSituationDocument::new(
            DeclarativeContext::new_v1(DeclarativeContextId::new("change-context").unwrap()),
            Some(Intent::new(
                IntentId::new("change-intent").unwrap(),
                input.desired.clone(),
            )),
            None,
            current,
            situation,
        )
        .unwrap();
    }
    let step = input.plan.steps()[0].id().clone();
    input.processes=ProcessRegistry::from_sources([ProcessSource::new("controlled.feature",format!("@process(controlled)\n@process-version(1)\n@cg-language(1)\nFeature: Controlled connector\nRule: Process\nGiven state START is initial\nGiven state END is terminal\nGiven event operate\nGiven activity operate requires capability {capability}\nGiven activity operate constrained by primary-agent=alpha\nScenario: operate\nGiven process state START\nWhen event operate occurs\nThen transition to state END\nThen authorize activity operate\nThen complete process\n"))]).unwrap();
    input = support::with_process(input);
    let mut rules = composition::rules();
    rules.provider_priorities.insert(
        composition::skill(if mutate { "update" } else { "good" }),
        10,
    );
    rules.processes.activities.insert(
        step.clone(),
        gateway_process::ActivityId::new("operate").unwrap(),
    );
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(&input, &rules)
        .unwrap();
    assert_eq!(
        resolved.report.outcome,
        gateway_application::resolution::ResolutionOutcome::Resolved
    );
    let authority = PolicyAuthority {
        policies: vec![
            PolicyDefinition::new(
                PolicyId::new("fixture-policy").unwrap(),
                "Explicit fixture policy",
                input.index.ids().cloned(),
            )
            .unwrap(),
        ],
        capabilities: input
            .index
            .entries()
            .map(|e| (e.id().clone(), e.capability().clone()))
            .collect(),
        ..Default::default()
    };
    let facts = StepFacts {
        authorizations: authority
            .capabilities
            .keys()
            .map(|id| (id.clone(), Approval::Granted))
            .collect(),
        consents: authority
            .capabilities
            .keys()
            .map(|id| (id.clone(), Approval::Granted))
            .collect(),
        evidence: authority
            .capabilities
            .values()
            .flat_map(|c| c.preconditions().iter().map(ToString::to_string))
            .collect(),
        satisfied_constraints: authority
            .capabilities
            .values()
            .flat_map(|c| c.constraints().iter().map(ToString::to_string))
            .chain(["[\"primary-agent\",\"alpha\"]".into()])
            .collect(),
        prerequisites_satisfied: true,
        ..Default::default()
    };
    let context = PolicyContext {
        basis: resolved.report.basis.clone(),
        operating_mode: input.operating_mode,
        execution_profile: input.execution_profile,
        steps: [(step.clone(), facts)].into(),
    };
    let actual = PolicyApplication
        .step_input(&resolved, &context, &step)
        .unwrap();
    assert!(actual.resolved);
    assert_eq!(actual.process, ProcessReadiness::Eligible);
    assert!(actual.constraints.contains("[\"primary-agent\",\"alpha\"]"));
    assert!(actual.capabilities.contains_key(&capability));
    NativePolicy {
        resolved,
        authority,
        context,
        step,
        capability,
    }
}
pub struct PolicyState {
    pub native: NativePolicy,
    pub audits: usize,
    pub change_before_effect: bool,
    pub deny_after: Option<usize>,
    returned: usize,
    revision: ContentDigest,
    target: ConnectorTarget,
    task: TaskId,
    execution: NonEmptyText,
    object: String,
    authorized: Vec<ConnectorRequest>,
    deadline: UnixTimestamp,
    inventory: Option<ContentDigest>,
}
struct CurrentPolicy(Arc<Mutex<PolicyState>>);
impl CurrentPolicy {
    fn snapshot(&self, r: &ConnectorRequest) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        let state = self.0.lock().unwrap();
        let fail = || ConnectorFailure {
            code: ConnectorFailureCode::PermissionDenied,
            effect: ConnectorEffect::NotDispatched,
            correlation: r.correlation.clone(),
        };
        if r.target != state.target
            || r.task != state.task
            || r.step != state.native.step
            || r.execution != state.execution
            || r.capability != state.native.capability
            || state.inventory.as_ref() != Some(&r.inventory_revision)
            || r.arguments.get(&text("query")) != Some(&TypedValue::string(&state.object).unwrap())
        {
            return Err(fail());
        }
        let mut native = PolicyApplication
            .step_input(&state.native.resolved, &state.native.context, &r.step)
            .map_err(|_| fail())?;
        if state.deny_after.is_some_and(|n| state.returned >= n) {
            native
                .facts
                .authorizations
                .insert(r.capability.clone(), Approval::Denied);
        }
        let receipt=(native.facts.consents.get(&r.capability)==Some(&Approval::Granted)).then(||text(&format!("fixture-consent-{}",digest(&json!({"target":r.target.connector.as_str(),"scope":r.target.scope,"capability":r.capability,"arguments":r.arguments,"task":r.task,"step":r.step.as_str(),"execution":r.execution.as_str(),"correlation":r.correlation.as_str(),"inventory":r.inventory_revision})))));
        Ok(ConnectorPolicySnapshot {
            step: native.step,
            resolved: native.resolved,
            has_prerequisites: native.has_prerequisites,
            constraints: native.constraints,
            preconditions: native.preconditions,
            authority: state.native.authority.clone(),
            facts: native.facts,
            operating_mode: native.operating_mode,
            execution_profile: native.execution_profile,
            process: native.process,
            consent_required: false,
            administrative_capabilities: BTreeSet::new(),
            policy: PolicyId::new("fixture-policy").unwrap(),
            policy_revision: state.revision.clone(),
            consent_receipt: receipt,
            budget_reservation: text("controlled-fixture-reservation"),
            deadline: state.deadline,
        })
    }
}
impl ConnectorPolicySource for CurrentPolicy {
    fn admit_target(&self, t: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        if t == &self.0.lock().unwrap().target {
            Ok(())
        } else {
            Err(ConnectorFailure {
                code: ConnectorFailureCode::ScopeMismatch,
                effect: ConnectorEffect::NotDispatched,
                correlation: text("target"),
            })
        }
    }
    fn authorize(
        &self,
        r: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        let snapshot = self.snapshot(r)?;
        self.0.lock().unwrap().authorized.push(r.clone());
        Ok(snapshot)
    }
    fn revalidate(
        &self,
        d: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        if !self.0.lock().unwrap().authorized.contains(&d.request) {
            return Err(ConnectorFailure {
                code: ConnectorFailureCode::PermissionDenied,
                effect: ConnectorEffect::NotDispatched,
                correlation: d.request.correlation.clone(),
            });
        }
        self.snapshot(&d.request)
    }
    fn record_decision(&self, _: &ConnectorPolicyEvaluation<'_>) -> Result<(), ConnectorFailure> {
        self.0.lock().unwrap().audits += 1;
        Ok(())
    }
}

pub struct Admission {
    task: SemanticTaskIR,
    expected_handoff: Value,
    expected_prepared: Value,
    tools: BTreeMap<CapabilityId, ToolDefinition>,
    producer: NativePolicy,
    pub references_revoked: AtomicBool,
}
fn reference<I: AsRef<str>>(p: &ResolvedReference<I>) -> SemanticReferenceBinding {
    SemanticReferenceBinding {
        id: p.id.as_ref().into(),
        scope: p.scope.clone(),
        contract: p.contract.clone(),
        contract_version: p.contract_version,
        revision: p.revision.clone(),
        digest: p.digest.clone(),
    }
}
impl SemanticReferenceValidator for Admission {
    fn validate(
        &self,
        task: &SemanticTaskData,
        b: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        let registered = [
            reference(&self.task.data().target.reference),
            reference(&self.task.data().output_contract.schema),
        ];
        if self.references_revoked.load(Ordering::SeqCst)
            || task != self.task.data()
            || !registered.contains(b)
        {
            return Err(ValidationError::InvalidStateCombination {
                reason: "fixture native reference not current",
            });
        }
        Ok(())
    }
}
impl InvocationAdmissionPort for Admission {
    fn validate_current(&self, ir: &InvocationIr) -> Result<(), InvocationIrError> {
        if ir.prompt().handoff().to_value() != self.expected_handoff
            || serde_json::to_value(ir.prepared()).unwrap() != self.expected_prepared
            || ir.prompt().tools() != &self.tools
        {
            return Err(InvocationIrError::StaleInput);
        }
        Ok(())
    }
}
#[derive(Clone)]
struct Admitted(Arc<Admission>);
impl SemanticReferenceValidator for Admitted {
    fn validate(
        &self,
        t: &SemanticTaskData,
        b: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        self.0.validate(t, b)
    }
}
impl InvocationAdmissionPort for Admitted {
    fn validate_current(&self, ir: &InvocationIr) -> Result<(), InvocationIrError> {
        self.0.validate_current(ir)
    }
}
impl OutputVerificationPort for Admission {
    fn resolve_reference(
        &self,
        _: &InvocationIr,
        _: &OutputReference,
    ) -> Result<(), VerificationError> {
        Err(VerificationError::UnsupportedReference)
    }
    fn evaluate(
        &self,
        ir: &InvocationIr,
        output: &ParsedOutput,
    ) -> Result<ConstraintReport, VerificationError> {
        self.validate_current(ir)
            .map_err(|_| VerificationError::AuthorityDenied)?;
        validate_output_proposal(
            &ir.identity(),
            &ir.prepared().invocation().data().output,
            output.data(),
        )?;
        for p in &output.data().tools {
            let Some(t) = self.tools.get(&p.capability.id) else {
                return Err(VerificationError::InvalidToolProposal);
            };
            if t.binding != p.capability || !t.input.accepts(&p.arguments) {
                return Err(VerificationError::InvalidToolProposal);
            }
        }
        let native = PolicyApplication
            .step_input(
                &self.producer.resolved,
                &self.producer.context,
                &self.producer.step,
            )
            .map_err(|_| VerificationError::AuthorityDenied)?;
        let report = PolicyEngine::evaluate(&self.producer.authority, &native.as_input());
        if report.decision != PolicyDecision::Allow {
            return Err(VerificationError::AuthorityDenied);
        }
        let mut checks = BTreeMap::new();
        for check in &ir
            .prompt()
            .handoff()
            .task()
            .data()
            .verification_contract
            .checks
        {
            match check {
                // The native assessor owns schema validity; host outcomes must not override it.
                SemanticVerificationCheck::OutputSchemaValid => (),
                SemanticVerificationCheck::NoUnresolvedReference
                    if output.data().references.is_empty() =>
                {
                    checks.insert(*check, CheckOutcome::Passed);
                }
                _ => return Err(VerificationError::VerificationUnavailable),
            }
        }
        Ok(ConstraintReport {
            binding: output.binding(),
            policy: CheckOutcome::Passed,
            process: CheckOutcome::Passed,
            task_completeness: CheckOutcome::Passed,
            semantic_output: CheckOutcome::Passed,
            checks,
        })
    }
}
struct Collector;
impl AcquisitionPort for Collector {
    fn collect(&self, d: &ConnectorDispatch) -> Result<Acquisition, Error> {
        let now = UnixTimestamp::new(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .try_into()
                .unwrap(),
        );
        Ok(Acquisition {
            acquired_at: now,
            observed_at: None,
            evaluated_at: now,
            max_age: FreshnessPolicy::new(60),
            authenticated_source: text("controlled-peer-profile"),
            sensitivity: SensitivityClass::Internal,
            conflict: ConflictStatus::None,
            partial: false,
            basis: ConnectorObservationBasis::ProviderAcknowledgement,
            effect: if matches!(
                d.request.class,
                ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
            ) {
                ConnectorEffect::Acknowledged
            } else {
                ConnectorEffect::NoMutation
            },
            verifies: None,
            non_authoritative_raw: None,
        })
    }
}
struct Mapped {
    inner: CanonicalConnectorMapping,
    policy: Arc<Mutex<PolicyState>>,
}
impl ConnectorMapping for Mapped {
    fn inventory(
        &self,
        t: &ConnectorTarget,
        m: &ConnectorManifest,
        w: &ProtocolInventory,
    ) -> Result<ConnectorInventory, Error> {
        self.inner.inventory(t, m, w)
    }
    fn operation(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
        w: &ProtocolInventory,
    ) -> Result<WireOperation, Error> {
        let operation = self.inner.operation(d, s, w)?;
        let mut policy = self.policy.lock().unwrap();
        if policy.change_before_effect {
            policy.revision = context_content_digest(b"operator-policy-changed-before-effect");
        }
        Ok(operation)
    }
    fn normalize(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
        r: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        let observation = self.inner.normalize(d, s, r)?;
        self.policy.lock().unwrap().returned += 1;
        Ok(observation)
    }
}
#[derive(Clone)]
struct Connection {
    host: Arc<McpConnectorHost>,
    coordinates: Arc<Mutex<Coordinates>>,
    fault: Arc<AtomicUsize>,
}
impl ConnectorApplicationPort for Connection {
    fn discover(&self, t: &ConnectorTarget) -> Result<ConnectorInventory, ConnectorFailure> {
        let mut i = self.host.discover(t)?;
        match self.fault.load(Ordering::SeqCst) {
            1 => i.revision = context_content_digest(b"changed inventory"),
            2 => {
                i.capabilities[0].argument_schema_digest =
                    context_content_digest(b"changed input schema")
            }
            3 => {
                self.coordinates.lock().unwrap().step =
                    PlanStepId::new("different-current-step").unwrap()
            }
            4 => self.coordinates.lock().unwrap().execution = text("different-current-execution"),
            _ => {}
        }
        Ok(i)
    }
    fn invoke(&self, r: &ConnectorRequest) -> Result<ConnectorObservation, ConnectorFailure> {
        self.host.invoke(r)
    }
    fn read_resource(
        &self,
        r: &ConnectorRequest,
        s: &SourceId,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        self.host.read_resource(r, s)
    }
    fn lifecycle(
        &self,
        t: &ConnectorTarget,
        c: ConnectorLifecycleCommand,
    ) -> Result<ConnectorLifecycleState, ConnectorFailure> {
        self.host.lifecycle(t, c)
    }
}
const PEER: &str = r#"import json,sys
log=sys.argv[1]
mode=sys.argv[2]
count=0
for line in sys.stdin:
 r=json.loads(line)
 with open(log,'a') as f:f.write(json.dumps(r)+'\n')
 if 'id' not in r:continue
 m=r['method']
 if m=='initialize':v={'protocolVersion':'2025-11-25','capabilities':{'tools':{}},'serverInfo':{'name':'controlled-proposal-peer','version':'1.0.0'}}
 elif m=='tools/list':v={'tools':[{'name':'records.read','inputSchema':{'type':'object','description':'Real descriptive schema metadata','properties':{'query':{'type':'string','description':'Exact admitted artifact identifier'}},'required':['query'],'additionalProperties':False}}]}
 elif m=='tools/call':
  count+=1
  if mode=='eof-second' and count==2:sys.exit(0)
  v={'content':[{'type':'text','text':'controlled acknowledgement'}],'structuredContent':{'object':r['params']['arguments']['query'],'value':'observed'},'isError':False}
 elif m=='ping':v={}
 else:raise AssertionError(m)
 print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':v}),flush=True)
"#;
pub struct Fixture {
    pub ir: InvocationIr,
    pub admission: Arc<Admission>,
    pub policy: Arc<Mutex<PolicyState>>,
    pub coordinates: Arc<Mutex<Coordinates>>,
    pub discovery_fault: Arc<AtomicUsize>,
    connection: Connection,
    root: PathBuf,
    log: PathBuf,
}
impl Fixture {
    pub fn new(mutate: bool, mode: &str) -> Self {
        Self::build(mutate, mode, true)
    }
    pub fn without_pin() -> Self {
        Self::build(false, "normal", false)
    }
    fn build(mutate: bool, mode: &str, with_pin: bool) -> Self {
        let native = native_policy(mutate);
        let h = prompt::handoff();
        let target = ConnectorTarget {
            connector: text("example.records"),
            scope: h.task().data().target.reference.scope.clone(),
        };
        let object = h.task().data().target.reference.id.as_str().to_owned();
        let task = h.task().data().id.clone();
        let step = native.step.clone();
        let execution = text(h.context().execution_context().id().as_str());
        let deadline = UnixTimestamp::new(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
                + 60,
        );
        let policy = Arc::new(Mutex::new(PolicyState {
            native,
            audits: 0,
            change_before_effect: false,
            deny_after: None,
            returned: 0,
            revision: context_content_digest(b"registered-fixture-policy-1"),
            target: target.clone(),
            task,
            execution: execution.clone(),
            object,
            authorized: Vec::new(),
            deadline,
            inventory: None,
        }));
        let root = std::env::temp_dir().join(format!(
            "cg401-peer-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("peer.py");
        std::fs::write(&script, PEER).unwrap();
        let log = root.join("wire.jsonl");
        let mut declaration: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/connector-mapping-v1/manifest.json"
        ))
        .unwrap();
        let manifest_value = &mut declaration["manifest"];
        manifest_value["inventory"]
            .as_array_mut()
            .unwrap()
            .retain(|v| v["kind"] == "tool");
        // The manifest's schema digest must describe exactly this peer's raw
        // schema, including supported descriptive metadata.
        let schema = json!({"type":"object","description":"Real descriptive schema metadata","properties":{"query":{"type":"string","description":"Exact admitted artifact identifier"}},"required":["query"],"additionalProperties":false});
        let tool_wire = json!({"name":"records.read","inputSchema":schema});
        manifest_value["inventory"][0]["source_mapping_ref"] = json!("mapping.records.read.v1");
        manifest_value["inventory"][0]["schema_digest"] =
            json!(discovery_schema_digest(InventoryKind::Tool, &tool_wire));
        let manifest =
            ConnectorManifest::from_definition_json(&serde_json::to_vec(manifest_value).unwrap())
                .unwrap();
        let mut mapping: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/connector-mapping-v1/mapping.json"
        ))
        .unwrap();
        mapping["records"].as_array_mut().unwrap().truncate(1);
        mapping["records"][0]["source"] = json!("controlled-observation");
        mapping["records"][0]["capability"] = json!(policy.lock().unwrap().native.capability);
        mapping["records"][0]["result_schema"] = json!({"type":"object","properties":{"object":{"type":"string"},"value":{"type":"string"}},"required":["object","value"],"additionalProperties":false});
        if mutate {
            mapping["records"][0]["class"] = json!("MUTATE");
            mapping["records"][0]["side_effects"] = json!("mutation");
            mapping["records"][0]["retry_safety"] = json!("never");
            mapping["records"][0]["idempotency"] = json!("non_idempotent");
        }
        let document = MappingDocument::from_json(&serde_json::to_vec(&mapping).unwrap()).unwrap();
        let normalizer = CanonicalResultNormalizer::new(
            NormalizationProfile {
                source: SourceId::new("controlled-observation").unwrap(),
                source_kind: SourceKind::Tool,
                object_field: "object".into(),
                revision_field: None,
                digest_field: None,
                fields: [(
                    "value".into(),
                    SubjectPath::new(["controlled", "value"]).unwrap(),
                )]
                .into(),
            },
            Box::new(Collector),
        )
        .unwrap();
        let mapper = CanonicalConnectorMapping::new(
            DiscoveryMapper::new(target.clone(), manifest.clone(), document).unwrap(),
            Box::new(normalizer),
        );
        let host = Arc::new(
            McpConnectorHost::new(
                HostConfig {
                    target: target.clone(),
                    manifest,
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
                Box::new(StdioConfig {
                    executable: PathBuf::from("/usr/bin/python3"),
                    arguments: vec![
                        "-I".into(),
                        script.to_string_lossy().into_owned(),
                        log.to_string_lossy().into_owned(),
                        mode.into(),
                    ],
                    directory: root.clone(),
                    environment: BTreeMap::new(),
                }),
                Arc::new(Cg09ConnectorAdmission::new(CurrentPolicy(policy.clone()))),
                Box::new(Mapped {
                    inner: mapper,
                    policy: policy.clone(),
                }),
                Arc::new(QuietObserver),
            )
            .unwrap(),
        );
        host.lifecycle(&target, ConnectorLifecycleCommand::Connect)
            .unwrap();
        let inventory = host.discover(&target).unwrap();
        policy.lock().unwrap().inventory = Some(inventory.revision.clone());
        let current = policy.lock().unwrap();
        let definition = &current.native.authority.capabilities[&current.native.capability];
        let pin = ResolvedReference {
            id: definition.id().clone(),
            scope: target.scope.clone(),
            contract: ReferenceId::new("cg.fixture-registered-capability").unwrap(),
            contract_version: SchemaVersion::V1,
            revision: ReferenceId::new("fixture-registration-1").unwrap(),
            digest: digest(definition),
        };
        drop(current);
        let mut tool = ToolDefinition::from_connector_schema(
            pin.clone(),
            text("Controlled native capability"),
            target.clone(),
            inventory.revision.clone(),
            inventory.capabilities[0].argument_schema_digest.clone(),
            &schema,
        )
        .unwrap();
        if !with_pin {
            tool.connector_inventory = None;
        }
        let tools = BTreeMap::from([(pin.id.clone(), tool)]);
        let base = prompt::prepared(&h, 100_000, false);
        let mut data = base.invocation().data().clone();
        data.invocation_id = ReferenceId::new(format!(
            "fixture-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
        .unwrap();
        data.permitted_capabilities = BTreeMap::from([(pin.id.clone(), pin)]);
        let invocation = ModelInvocation::new(
            data.clone(),
            ModelInvocationBasis {
                task: h.task(),
                output: &data.output,
                compiled_context: &data.compiled_context,
                permitted_capabilities: &data.permitted_capabilities,
                sensitivity: data.sensitivity,
                permitted_localities: &data.permitted_localities,
                authority: &data.authority,
            },
        )
        .unwrap();
        let prepared = gateway_application::model_invocation::prepare_model_invocation(
            invocation,
            base.capability().clone(),
            base.provider().clone(),
            &prompt::model::runtime(),
        )
        .unwrap();
        let admission = Arc::new(Admission {
            task: h.task().clone(),
            expected_handoff: h.to_value(),
            expected_prepared: serde_json::to_value(&prepared).unwrap(),
            tools: tools.clone(),
            producer: native_policy(false),
            references_revoked: AtomicBool::new(false),
        });
        let ir =
            admit_invocation_ir(SchemaVersion::V1, h, prepared, tools, admission.as_ref()).unwrap();
        let coordinates = Arc::new(Mutex::new(Coordinates {
            target,
            step,
            execution,
        }));
        let discovery_fault = Arc::new(AtomicUsize::new(0));
        let connection = Connection {
            host,
            coordinates: coordinates.clone(),
            fault: discovery_fault.clone(),
        };
        Self {
            ir,
            admission,
            policy,
            coordinates,
            discovery_fault,
            connection,
            root,
            log,
        }
    }
    pub fn wire(&self, n: usize) -> Value {
        let tool = self.ir.prompt().tools().values().next().unwrap();
        json!({"schema_version":"1.0","invocation":self.ir.identity(),"result":"Proposed bounded connector operations","references":[],"tools":(0..n).map(|_|json!({"capability":tool.binding,"arguments":{"query":self.ir.prompt().handoff().task().data().target.reference.id}})).collect::<Vec<_>>()})
    }
    pub fn verified(&self, n: usize) -> VerifiedOutput {
        let bytes = serde_json::to_vec(&self.wire(n)).unwrap();
        let parsed =
            gateway_application::output_parsing::parse_invocation_output(&self.ir, &bytes).unwrap();
        verify_output(&self.ir, parsed, self.admission.as_ref(), vec![]).unwrap()
    }
    pub fn dispatch(
        &self,
        ir: &InvocationIr,
        verified: &VerifiedOutput,
    ) -> Result<ProposalBatch, ProposalError> {
        ConnectorProposalDispatcher::new(
            CanonicalConnectorProposalBinder::new(
                Admitted(self.admission.clone()),
                Session(self.coordinates.clone()),
                self.connection.clone(),
            ),
            self.connection.clone(),
        )
        .dispatch(ir, verified)
    }
    pub fn calls(&self) -> usize {
        std::fs::read_to_string(&self.log)
            .unwrap()
            .lines()
            .filter(|line| serde_json::from_str::<Value>(line).unwrap()["method"] == "tools/call")
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let target = self.connection.host_target();
        let _ = self
            .connection
            .host
            .lifecycle(&target, ConnectorLifecycleCommand::Shutdown);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
impl Connection {
    fn host_target(&self) -> ConnectorTarget {
        self.coordinates.lock().unwrap().target.clone()
    }
}

struct QuietObserver;
impl Observer for QuietObserver {
    fn record(&self, _: Event) {}
}
