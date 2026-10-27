use super::*;
use crate::local_sessions::runtime_admission::tests::Fixture;

fn read(f: &mut Fixture, id: &str) -> Value {
    let document = f.runtime()["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["pin"]["id"] == id)
        .unwrap()
        .clone();
    serde_json::from_slice(&std::fs::read(document["path"].as_str().unwrap()).unwrap()).unwrap()
}
fn put(f: &mut Fixture, id: &str, kind: &str, value: Value) -> Value {
    let pin = json!({"id":id,"scope":f.runtime()["scope"],"contract":kind,"contract_version":"1.0","revision":"operator-model-definition-1","digest":context_content_digest(&canonical(&value).unwrap())});
    let path = f.root.join(format!("{id}.json"));
    std::fs::write(&path, canonical(&value).unwrap()).unwrap();
    f.runtime()["documents"]
        .as_array_mut()
        .unwrap()
        .retain(|d| d["pin"]["id"] != id);
    f.runtime()["documents"]
        .as_array_mut()
        .unwrap()
        .push(json!({"pin":pin,"path":path}));
    pin
}
fn replace(f: &mut Fixture, id: &str, value: Value) -> Value {
    f.rewrite(id, value);
    f.runtime()["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["pin"]["id"] == id)
        .unwrap()["pin"]
        .clone()
}
/// Installs explicit native declarations around the *actual* current task and
/// native CLI basis. No native IR, Context, policy report or output is injected.
pub(in crate::local_sessions::runtime_admission) fn install(
    f: &mut Fixture,
    endpoint: &str,
    credential_variable: Option<&str>,
    native: &crate::declarative_cli::host_mapping::native_producer_fixture::NativeProducerFixture,
) {
    let task_pin = f.runtime()["steps"][0]["producer"]["task"].clone();
    let task = read(f, task_pin["id"].as_str().unwrap());
    let mut alias = task_pin.clone();
    alias["contract"] = json!("semantic-task-ir");
    let declaration = put(
        f,
        "model-declaration",
        "cg.connector-model-provenance",
        json!({"schema_version":"1.0","declaration":"OPERATOR_DECLARATION_V1"}),
    );
    let target = put(
        f,
        "native-model",
        "cg.connector-model-target",
        json!({"schema_version":"1.0","runtime_id":"controlled-http-model-v1"}),
    );
    let tokenizer = put(
        f,
        "native-tokenizer",
        "cg.connector-model-tokenizer",
        json!({"schema_version":"1.0","method":"UTF8_WIRE_BYTES_PLUS256_V1"}),
    );
    let adapter = put(
        f,
        "native-adapter",
        "cg.connector-model-adapter",
        json!({"schema_version":"1.0","renderer":"CHAT_COMPLETIONS_SESSION_TOOLS_PORTABLE_RUNTIME_V4","wire_mode":"PORTABLE_V1","response_mode":"TOOL_PROPOSAL_TEXT_V1","tokenizer":tokenizer,"endpoint":endpoint,"credential_profile":null,"credential_source":credential_variable.map(|variable|json!({"kind":"ENVIRONMENT_BEARER_V1","variable":variable})),"pricing":{"cost_unit":"credits","fixed_per_attempt_microunits":0,"input_microunits_per_unit":0,"output_microunits_per_unit":0}}),
    );
    let provider = put(
        f,
        "native-provider",
        "cg.connector-model-provider",
        json!({"schema_version":"1.0","adapter":adapter,"locality":"ON_DEVICE","maximum_sensitivity":"CONFIDENTIAL","controls":{"TEMPERATURE":{"kind":"SCALAR","min":0.0,"max":2.0}},"defaults":{"TEMPERATURE":{"kind":"SCALAR","value":0.0}},"unexposed_defaults":[],"provenance":declaration}),
    );
    let capability = put(
        f,
        "native-model-capability",
        "cg.connector-model-capability",
        json!({"schema_version":"1.0","model":target,"provider":provider,"task_classes":["EXTRACTION"],"input_contracts":["prompt-ir-v1"],"output_contracts":{"ArtifactInspection":task["output_contract"]["schema"]},"context_limit":262144,"max_output_tokens":8192,"structured_output_modes":["TOOL_PROPOSAL"],"tool_calling":true,"reasoning_strategies":["bounded"],"provenance":declaration}),
    );
    let generation = put(
        f,
        "native-generation",
        "cg.connector-model-generation",
        json!({"schema_version":"1.0","controls":{"TEMPERATURE":{"kind":"SCALAR","value":0.0}},"max_output_tokens":4096,"structured_output_mode":"TOOL_PROPOSAL","reproducibility_class":"STOCHASTIC"}),
    );
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let status = put(
        f,
        "native-status",
        "cg.connector-model-status",
        json!({"schema_version":"1.0","provider":provider,"availability":"AVAILABLE","valid_from_ms":now-1,"valid_until_ms":now+3_600_000}),
    );
    let analysis = replace(
        f,
        "analysis",
        json!({"schema_version":"1.0","profile":"NATIVE_SESSION_MODEL_V1","budget_per_requirement":{"results":32,"rounds":2,"latency_ms":30000,"cost":0,"cost_unit":"credits","tokens":131072,"context_total":131072,"context_reservations":[{"class":"EVIDENCE","units":32768},{"class":"RUNTIME_STATE","units":32768},{"class":"KNOWLEDGE","units":32768},{"class":"AUTHORITY_RESERVED","units":8192},{"class":"TASK_RESERVED","units":16384},{"class":"OUTPUT_CONTRACT_RESERVED","units":8192}]},"total_tokens":262144,"maximum_sensitivity":"INTERNAL","additional":[]}),
    );
    let task_native: SemanticTaskIR = serde_json::from_value(task.clone()).unwrap();
    let inputs = derive_planning_inputs(
        &native.resolved.snapshot.input().delta,
        task_native.data().desired_state.as_ref().unwrap(),
        &Default::default(),
    )
    .unwrap();
    let mut covers = vec![json!("target-state")];
    for input in inputs {
        covers.push(json!(format!("planning-{}", input.id().as_str())));
    }
    let source = put(
        f,
        "native-before-read",
        "cg.context-source-metadata",
        json!({"schema_version":"1.0","profile":"NATIVE_SESSION_MODEL_V1","class":"TOOLS_MCP","covers":covers,"satisfaction":{"kind":"BEFORE_READ","target":task["target"]["reference"],"mapping_reference":"mapping.records.read.v1","source":"records.one"}}),
    );
    let mut verification = read(f, "verification");
    for rule in verification["rules"].as_array_mut().unwrap() {
        let rule_id = rule["rule"]["id"].as_str().unwrap().to_owned();
        rule["rule"] = replace(
            f,
            &rule_id,
            json!({"schema_version":"1.0","profile":"NATIVE_MODEL_PROPOSAL_RULE_V1","check":rule["check"]}),
        );
        let verifier_id = rule["verifier"]["id"].as_str().unwrap().to_owned();
        rule["verifier"] = replace(
            f,
            &verifier_id,
            json!({"schema_version":"1.0","profile":"NATIVE_MODEL_PROPOSAL_VERIFIER_V1"}),
        );
    }
    replace(f, "verification", verification);
    let model = replace(
        f,
        "model",
        json!({"schema_version":"1.0","profile":"NATIVE_SESSION_MODEL_V1","task":task_pin,"invocation_task":alias,"provider":provider,"capability":capability,"generation":generation,"status":status,"analysis":analysis,"sources":[source],"task_class":"EXTRACTION","input_contract":"prompt-ir-v1","strategy":"bounded","sensitivity":"INTERNAL","permitted_localities":["ON_DEVICE"],"input_tokens":131072,"timeout_ms":30000,"max_attempts":1}),
    );
    f.runtime()["steps"][0]["producer"]["model"] = model;
    f.runtime()["steps"][0]["producer"]["source_metadata"] = json!([source]);
}

#[test]
fn native_basis_artifacts_bind_real_cli_plan_and_situation_without_operator_aliases() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut f = Fixture::with_scope(&format!("model-basis-{nonce}"));
    let native = f.install_actual_mixed_model();
    install(&mut f, "http://127.0.0.1:1", None, &native);
    let host = f.host().unwrap();
    let runtime = host
        .sessions
        .as_ref()
        .unwrap()
        .connector_runtime
        .as_ref()
        .unwrap();
    let loaded = runtime.load(&host).unwrap();
    let ProducerAdmission::Model(admission) = &runtime.0.steps[0].producer else {
        panic!()
    };
    let definitions = read_model(&admission.model, &loaded.documents).unwrap();
    let artifacts =
        NativeBasisArtifacts::capture(&native.resolved, definitions.model.invocation_task.clone())
            .unwrap();
    assert_eq!(
        artifacts.plan,
        canonical(&native.resolved.snapshot.input().plan).unwrap()
    );
    assert_eq!(
        artifacts.situation,
        canonical(native.resolved.snapshot.input().situation.situation()).unwrap()
    );
    assert_eq!(
        artifacts.basis.plan.as_ref().unwrap().digest,
        context_content_digest(&artifacts.plan)
    );
    assert_eq!(
        artifacts.basis.situation.as_ref().unwrap().digest,
        context_content_digest(&artifacts.situation)
    );
    assert_eq!(
        artifacts.payload().unwrap()["storage"],
        "CAPTURED_IN_MEMORY"
    );
    let mut alias = definitions.model.invocation_task.clone();
    alias.scope = ContextScopeId::new("other-project").unwrap();
    assert!(matches!(
        NativeBasisArtifacts::capture(&native.resolved, alias),
        Err(SessionError::ScopeDenied)
    ));
}

use super::super::preparing_test_support::NativePreparingProbe;
use crate::local_sessions::*;
use gateway_application::sessions::*;
use std::cell::Cell;

pub(in crate::local_sessions::runtime_admission) const PEER: &str = r#"import json,sys
for line in sys.stdin:
 q=json.loads(line);m=q.get('method')
 if 'id' not in q:continue
 if m=='initialize':r={'protocolVersion':'2025-11-25','capabilities':{'tools':{}},'serverInfo':{'name':'example.records.local','version':'1.0.0'}}
 elif m=='tools/list':r={'tools':[{'name':name,'inputSchema':{'type':'object','properties':{'query':{'type':'string'}},'required':['query'],'additionalProperties':False}} for name in ['records.read','records.update']]}
 elif m in ['resources/list','resources/templates/list','prompts/list']:r={ {'resources/list':'resources','resources/templates/list':'resourceTemplates','prompts/list':'prompts'}[m]:[] }
 elif m=='tools/call':
  assert q['params']['name']=='records.read','model proposal may not execute'
  assert q['params']['arguments']=={'query':'native-fixture'}
  r={'content':[],'structuredContent':{'object':'native-fixture','id':'native-fixture','first':True,'second':False},'isError':False}
 elif m=='ping':r={}
 else:raise ValueError('unexpected method')
 print(json.dumps({'jsonrpc':'2.0','id':q['id'],'result':r}),flush=True)
"#;

pub(in crate::local_sessions::runtime_admission) struct ControlledProvider {
    endpoint: String,
    calls: Arc<std::sync::atomic::AtomicUsize>,
    proposal: std::sync::mpsc::Sender<String>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}
pub(in crate::local_sessions::runtime_admission) fn http_provider() -> ControlledProvider {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::atomic::{AtomicUsize, Ordering},
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopping = Arc::clone(&stop);
    let (proposal, received_proposal) = std::sync::mpsc::channel::<String>();
    let thread = std::thread::spawn(move || {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(40);
        let mut stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if stopping.load(Ordering::SeqCst) || std::time::Instant::now() >= until {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => panic!("{e}"),
            }
        };
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buf = [0; 8192];
        let header_end = loop {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
            assert!(bytes.len() < 1_048_576);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                break end + 4;
            }
        };
        let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
        let length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|v| v.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        while bytes.len() < header_end + length {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
        }
        let body: Value = serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
        assert_eq!(body["model"], "controlled-http-model-v1");
        assert!(!body["tools"].as_array().unwrap().is_empty());
        let name = received_proposal
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(
            body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["function"]["name"] == name)
        );
        assert!(name.starts_with("cg_tool_"));
        observed.fetch_add(1, Ordering::SeqCst);
        let response=json!({"model":"controlled-http-model-v1","choices":[{"message":{"content":null,"tool_calls":[{"id":"native-proposal-1","type":"function","function":{"name":name,"arguments":"{\"query\":\"native-fixture\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}).to_string();
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
    });
    ControlledProvider {
        endpoint,
        calls,
        proposal,
        stop,
        thread,
    }
}

#[test]
fn actual_native_mixed_producer_reads_and_prepares_verified_http_proposal_under_real_pg_claim() {
    run_native_model(false);
}

#[test]
fn missing_explicit_environment_credential_makes_no_http_attempt_under_real_pg_claim() {
    run_native_model(true);
}

fn run_native_model(missing_credential: bool) {
    let Ok(connection) = std::env::var("CG_COGNITIVE_TEST_DATABASE") else {
        return;
    };
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut f = Fixture::with_scope(&format!("native-model-e2e-{nonce}"));
    let native = f.install_actual_mixed_model();
    let provider = http_provider();
    let variable = format!("CG_ABSENT_NATIVE_FACTORY_BEARER_{nonce}");
    install(
        &mut f,
        provider.endpoint(),
        missing_credential.then_some(variable.as_str()),
        &native,
    );
    let peer = f.root.join("model-peer.py");
    std::fs::write(&peer, PEER).unwrap();
    f.runtime()["stdio"]["executable"] = json!("/usr/bin/python3");
    f.runtime()["stdio"]["arguments"] = json!([peer]);
    let profiles = f.root.join("profiles.json");
    std::fs::write(&profiles,json!({"schema_version":1,"profiles":{"records.local":{"connector":"example.records","server":"example.records.local","tools":["records.read","records.update"],"generation":1,"environment":{}}}}).to_string()).unwrap();
    f.runtime()["credential_store"] = json!(profiles);
    let mut plan: SessionResourcePlan =
        serde_json::from_value(f.runtime()["steps"][0]["resources"].clone()).unwrap();
    plan.preparation = SessionResourceAmounts {
        connector_calls: 2,
        retrieval_rounds: 2,
        retrieval_results: 12,
        retrieval_tokens: 900_000,
        model_attempts: 1,
        input_tokens: 131072,
        output_tokens: 4096,
        output_bytes: 65536,
        cost_microunits: 0,
        ..Default::default()
    };
    f.runtime()["steps"][0]["resources"] = json!(plan);
    f.wire["mappings"][0]["sessions"]["resource_limits"] = json!(SessionResourceLimits {
        cost_unit: ReferenceId::new("credits").unwrap(),
        amounts: plan.total().unwrap()
    });
    let database = f.root.join("database.txt");
    std::fs::write(&database, connection).unwrap();
    f.wire["mappings"][0]["sessions"]["store_file"] = json!(database);
    let admission = f.root.join("admission.json");
    std::fs::write(&admission, f.wire.to_string()).unwrap();
    let host = f.host().unwrap();
    let config = host.sessions.as_ref().unwrap();
    let cwd = f.root.to_string_lossy().into_owned();
    let local = LocalSessionHost::new(Arc::new(LocalSessionAccess {
        binding: host.binding.clone(),
        admission: admission.to_string_lossy().into_owned(),
        cwd: cwd.clone(),
        repository: cwd,
        store_connection: Default::default(),
    }));
    let owner = local.owner().unwrap();
    let completed = Cell::new(false);
    let callback = |local: &LocalSessionHost,
                    cp: &SessionCheckpoint,
                    resources: &mut SessionPhaseResources|
     -> Result<(), SessionError> {
        let started = std::time::Instant::now();
        let connector = resources.allocate(SessionResourceAmounts {
            connector_calls: 2,
            retrieval_rounds: 2,
            retrieval_results: 12,
            retrieval_tokens: 900_000,
            ..Default::default()
        })?;
        let model = resources.allocate(SessionResourceAmounts {
            model_attempts: 1,
            input_tokens: 131072,
            output_tokens: 4096,
            output_bytes: 65536,
            ..Default::default()
        })?;
        let capture = super::super::target_factory::PreparedTarget::capture(
            Arc::clone(&local.access),
            &connector,
            None,
        )?;
        let context = crate::mcp_client::CallContext::new(
            NonEmptyText::new("native-model-e2e").unwrap(),
            connector.deadline(),
        );
        let mut target = capture.build(connector, context, Arc::new(()))?;
        checked("discovery", &started, target.discover())?;
        let producer = checked(
            "model capture",
            &started,
            NativeModelProducerPlan::capture(&target, cp, &model),
        )?;
        let mutate = native
            .capabilities
            .iter()
            .find(|(_, definition)| definition.class() == CapabilityClass::Mutate)
            .unwrap()
            .0;
        let tool = producer.tools.get(mutate).unwrap();
        provider.propose(tool);
        let acquired = checked(
            "context acquisition",
            &started,
            producer.acquire(&mut target),
        )?;
        let mut run = checked(
            "run start",
            &started,
            acquired.start(
                &target,
                model,
                RequestContext::new(
                    "native-model-call".into(),
                    std::time::Instant::now() + std::time::Duration::from_secs(30),
                ),
            ),
        )?;
        let result = run.attempt(
            &target,
            gateway_application::closed_loop::LoopDecision::Continue,
        );
        if missing_credential {
            assert!(matches!(result, Err(SessionError::Unavailable)));
            assert_eq!(run.usage().model_attempts, 0);
            assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        } else {
            let turn = checked("model attempt", &started, result)?;
            assert_eq!(turn.verified_output().output().data().tools.len(), 1);
            assert_eq!(
                &turn.verified_output().output().data().tools[0]
                    .capability
                    .id,
                mutate
            );
            assert_eq!(run.usage().model_attempts, 1);
        }
        assert_eq!(target.usage()?.connector_calls, 2);
        assert!(
            target.context_source().action_context().is_none(),
            "proposal verification is not a mutation grant"
        );
        Ok(())
    };
    let probe = NativePreparingProbe {
        local,
        plan,
        completed: &completed,
        callback: &callback,
    };
    let repository = store(&probe.local.current().unwrap()).unwrap();
    let coordinator = SessionCoordinator::new(repository, probe);
    let started = coordinator
        .execute(
            &owner,
            SessionCommand::Start {
                command: CommandId::new(format!("model-start-{nonce}")).unwrap(),
                intent: config.intent.clone(),
                execution: config.execution,
            },
        )
        .unwrap();
    let result = coordinator.execute(
        &owner,
        SessionCommand::Continue {
            at: Mutation {
                session: started.session,
                expected_revision: started.revision,
                command: CommandId::new(format!("model-continue-{nonce}")).unwrap(),
            },
        },
    );
    assert!(
        result.is_err(),
        "component probe deliberately returns no executable turn"
    );
    assert!(
        completed.get(),
        "genuine native producer pipeline failed before output verification"
    );
    provider.finish(usize::from(!missing_credential));
}

fn checked<T>(
    stage: &str,
    started: &std::time::Instant,
    result: Result<T, SessionError>,
) -> Result<T, SessionError> {
    eprintln!(
        "native model stage={stage}; elapsed_ms={}; error={:?}",
        started.elapsed().as_millis(),
        result.as_ref().err()
    );
    result
}

impl ControlledProvider {
    pub(in crate::local_sessions::runtime_admission) fn endpoint(&self) -> &str {
        &self.endpoint
    }
    pub(in crate::local_sessions::runtime_admission) fn propose(&self, tool: &ToolDefinition) {
        let digest = context_content_digest(&serde_json::to_vec(tool).unwrap());
        self.proposal
            .send(format!("cg_tool_{}", &digest.as_str()[..56]))
            .unwrap();
    }
    pub(in crate::local_sessions::runtime_admission) fn finish(self, expected_calls: usize) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.thread.join().unwrap();
        assert_eq!(
            self.calls.load(std::sync::atomic::Ordering::SeqCst),
            expected_calls
        );
    }
}
