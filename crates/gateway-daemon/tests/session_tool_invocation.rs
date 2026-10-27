#[path = "support/session_resource_allocation.rs"]
#[allow(dead_code)] // Connector-only helpers are exercised by the MCP integration suite.
mod session_resource_allocation;
#[path = "../../../tests/fixtures/prompt-rendering-v1/support.rs"]
#[allow(dead_code)] // Shared fixture module exports helpers used by sibling integration tests.
mod support;

use gateway_application::sessions::{
    SessionResourceAmounts, SessionResourceAuthorityPort, SessionResourcePlan,
};
use gateway_application::{
    codex::RequestContext,
    invocation_resilience::{BoundedInvocationPort, BoundedInvocationRequest},
    model_invocation::PreparedModelInvocation,
    output_verification::{ConstraintReport, OutputVerificationPort, ParsedOutput, VerifiedOutput},
    prompt_ir::{InvocationAdmissionPort, InvocationIr, admit_invocation_ir, render_invocation_ir},
    prompt_rendering::{
        PromptRendererPort, RenderInput, TokenCountRequest, TokenizerPort, ToolDefinition,
        render_data,
    },
};
use gateway_daemon::invocation_host::session_tools::{
    ProviderFinish, ProviderTokenUsage, ProviderToolCall, ProviderToolCompletion,
    ProviderToolFailure, SessionToolAttemptExecution, SessionToolAuthorityPort,
    SessionToolExecution, SessionToolInvocation, SessionToolInvocationError,
    SessionToolProviderAdapter, SessionToolProviderPort, SessionToolRun,
    SessionToolSemanticReferencePort, SessionToolVerificationHost,
};
use gateway_daemon::model_adapters::{
    AdapterKind, RuntimeModelBinding, SessionToolHttpAdapter, SessionToolHttpRenderer,
    SessionToolResponseMode, SessionToolWireMode,
};
use gateway_domain::{
    CapabilityId, ContentDigest, ResolvedReference, SchemaVersion, SemanticReferenceBinding,
    SemanticTaskData, ValidationError, context_content_digest, invocation_resilience::*,
    model_invocation::*, output_verification::*, prompt_ir::*, prompt_rendering::*,
    semantic_output::*, *,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use support::{examples::id, model::pin};

struct Host;
impl SemanticReferenceValidator for Host {
    fn validate(
        &self,
        _: &SemanticTaskData,
        _: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl InvocationAdmissionPort for Host {
    fn validate_current(&self, _: &InvocationIr) -> Result<(), InvocationIrError> {
        Ok(())
    }
}
impl OutputVerificationPort for Host {
    fn resolve_reference(
        &self,
        _: &InvocationIr,
        _: &OutputReference,
    ) -> Result<(), gateway_domain::output_verification::VerificationError> {
        Err(gateway_domain::output_verification::VerificationError::UnsupportedReference)
    }

    fn evaluate(
        &self,
        ir: &InvocationIr,
        output: &ParsedOutput,
    ) -> Result<ConstraintReport, gateway_domain::output_verification::VerificationError> {
        let binding = output.binding();
        let checks = ir
            .prompt()
            .handoff()
            .task()
            .data()
            .verification_contract
            .checks
            .iter()
            .filter(|check| **check != SemanticVerificationCheck::OutputSchemaValid)
            .map(|check| (*check, CheckOutcome::Passed))
            .collect();
        Ok(ConstraintReport {
            binding,
            policy: CheckOutcome::Passed,
            process: CheckOutcome::Passed,
            task_completeness: CheckOutcome::Passed,
            semantic_output: CheckOutcome::Passed,
            checks,
        })
    }
}

struct Renderer;
impl PromptRendererPort for Renderer {
    fn identity(&self) -> (ReferenceId, ReferenceId) {
        (id("connector-session-renderer"), id("1"))
    }
    fn render(&self, input: &RenderInput) -> Result<Value, RenderDiagnostic> {
        let tools = input
            .tools()
            .iter()
            .map(|(id, tool)| {
                json!({
                    "type":"function",
                    "function":{"name":id.as_str(),"description":tool.description.as_str(),
                        "parameters":tool.input.json_schema(),"strict":true}
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({"messages":[{"role":"user","content":render_data(input)}],"tools":tools}))
    }
}

struct Counter;
impl TokenizerPort for Counter {
    fn count(&self, request: &TokenCountRequest<'_>) -> Result<TokenCountResult, RenderDiagnostic> {
        Ok(TokenCountResult {
            profile: request.profile.clone(),
            payload_digest: request.payload_digest.clone(),
            mode: CountMode::Estimated,
            provenance: pin("connector-session-test-counter"),
            usage: TokenUsage {
                instruction_tokens: 0,
                task_tokens: 0,
                context_tokens: 0,
                evidence_tokens: 0,
                tool_schema_tokens: 0,
                framing_tokens: request.payload.len() as u64,
            },
        })
    }
}

#[derive(Clone)]
struct FixedProvider {
    response: Arc<Mutex<Result<ProviderToolCompletion, ProviderToolFailure>>>,
    seen_tools: Arc<Mutex<Option<Value>>>,
}
impl SessionToolProviderPort for FixedProvider {
    fn complete(
        &self,
        request: BoundedInvocationRequest<'_>,
    ) -> Result<ProviderToolCompletion, ProviderToolFailure> {
        *self.seen_tools.lock().unwrap() =
            Some(serde_json::to_value(request.ir.prompt().tools()).unwrap());
        self.response.lock().unwrap().clone()
    }
}

struct Fixture {
    session: SessionToolInvocation,
    host: Host,
    definition: ToolDefinition,
    model_id: String,
    prepared: PreparedModelInvocation,
}

fn fixture() -> Fixture {
    fixture_with_schema(json!({
        "type":"object",
        "properties":{"record_id":{"type":"string"}},
        "required":["record_id"],
        "additionalProperties":false
    }))
}

fn fixture_with_schema(schema: Value) -> Fixture {
    fixture_with_handoff(schema, support::handoff())
}

fn fixture_with_handoff(schema: Value, handoff: gateway_context::PlannedContextHandoff) -> Fixture {
    let mut invocation = support::model::invocation_data();
    invocation.task =
        serde_json::from_value(handoff.to_value()["bindings"]["task"].clone()).unwrap();
    invocation.compiled_context =
        serde_json::from_value(handoff.to_value()["bindings"]["context"].clone()).unwrap();
    invocation.output = serde_json::from_value(handoff.to_value()["output"].clone()).unwrap();
    let mut generation: GenerationProfileData = invocation.generation.data().clone();
    generation.structured_output_mode = StructuredOutputMode::ToolProposal;
    generation.controls.remove(&GenerationControl::TopK);
    invocation.generation = GenerationProfile::try_from(generation).unwrap();

    let scope = invocation.task.scope.clone();
    let capability_id = CapabilityId::new("crm.read_record").unwrap();
    let binding = ResolvedReference {
        id: capability_id.clone(),
        scope: scope.clone(),
        contract: id("connector-capability"),
        contract_version: SchemaVersion::V1,
        revision: id("inventory-r1"),
        digest: ContentDigest::new("d".repeat(64)).unwrap(),
    };
    let target = gateway_application::connectors::ConnectorTarget {
        connector: NonEmptyText::new("crm").unwrap(),
        scope: scope.clone(),
    };
    let schema_digest = context_content_digest(&serde_json::to_vec(&schema).unwrap());
    let definition = ToolDefinition::from_connector_schema(
        binding.clone(),
        NonEmptyText::new("Read one CRM record").unwrap(),
        target,
        ContentDigest::new("b".repeat(64)).unwrap(),
        schema_digest,
        &schema,
    )
    .unwrap();
    invocation
        .permitted_capabilities
        .insert(capability_id, binding);

    let model = ModelInvocation::new(
        invocation.clone(),
        ModelInvocationBasis {
            task: handoff.task(),
            output: &invocation.output,
            compiled_context: &invocation.compiled_context,
            permitted_capabilities: &invocation.permitted_capabilities,
            sensitivity: invocation.sensitivity,
            permitted_localities: &invocation.permitted_localities,
            authority: &invocation.authority,
        },
    )
    .unwrap();
    let mut capability_data = support::model::capability_data();
    capability_data.context_limit = 100_000;
    let prepared = gateway_application::model_invocation::prepare_model_invocation(
        model,
        ModelCapability::try_from(capability_data).unwrap(),
        support::model::provider(),
        &support::model::runtime(),
    )
    .unwrap();
    let model_id = prepared.capability().data().model.id.as_str().to_owned();
    let session = SessionToolInvocation::admit(
        handoff.clone(),
        prepared.clone(),
        BTreeMap::from([(definition.binding.id.clone(), definition.clone())]),
        &Host,
    )
    .unwrap();
    Fixture {
        session,
        host: Host,
        definition,
        model_id,
        prepared,
    }
}

fn completion(fixture: &Fixture, calls: Vec<ProviderToolCall>) -> ProviderToolCompletion {
    ProviderToolCompletion {
        model_id: fixture.model_id.clone(),
        finish: ProviderFinish::Complete,
        result: json!("record inspected"),
        calls,
        usage: Some(ProviderTokenUsage {
            input_tokens: 100,
            output_tokens: 32,
        }),
    }
}

fn invoke(
    fixture: &Fixture,
    provider: Result<ProviderToolCompletion, ProviderToolFailure>,
) -> (
    Result<VerifiedOutput, SessionToolInvocationError>,
    Arc<Mutex<Option<Value>>>,
) {
    let response = Arc::new(Mutex::new(provider));
    let seen_tools = Arc::new(Mutex::new(None));
    let adapter = SessionToolProviderAdapter::new(FixedProvider {
        response,
        seen_tools: seen_tools.clone(),
    });
    let renderer = Renderer;
    let profile = TokenizerProfile {
        schema_version: SchemaVersion::V1,
        model: fixture
            .session
            .invocation()
            .prepared()
            .capability()
            .data()
            .model
            .clone(),
        generation_profile: fixture
            .session
            .invocation()
            .prepared()
            .invocation()
            .data()
            .generation
            .content_digest(),
        tokenizer: id("connector-session-test-counter"),
        tokenizer_version: id("1"),
        renderer: renderer.identity().0,
        template_version: renderer.identity().1,
    };
    let limits = InvocationRunLimits {
        schema_version: SchemaVersion::V1,
        max_attempts: 1,
        timeout_ms: 4_000,
        invocation_input_tokens: 50_000,
        invocation_output_tokens: 2_000,
        run_input_tokens: 50_000,
        run_output_tokens: 2_000,
        cost_microunits: 100,
        output_bytes: 100_000,
        reconciliation_tolerance: 4_000,
    };
    let context = RequestContext::new(
        "session-tool-test".into(),
        Instant::now() + Duration::from_secs(4),
    );
    (
        fixture.session.invoke(SessionToolExecution {
            host: &fixture.host,
            profile,
            renderer: &renderer,
            tokenizer: &Counter,
            limits,
            context,
            reserved_cost_microunits: 10,
            transport: &adapter,
        }),
        seen_tools,
    )
}

fn serve_http_responses(responses: Vec<String>) -> (String, thread::JoinHandle<()>) {
    let count = responses.len();
    let mut responses = responses.into_iter();
    serve_http_handler(count, move |_| responses.next().unwrap())
}

fn serve_http_handler(
    count: usize,
    mut handle: impl FnMut(Value) -> String + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for _ in 0..count {
            let deadline = Instant::now() + Duration::from_secs(5);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("HTTP adapter did not connect: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0; 8192];
            let request = loop {
                let n = stream.read(&mut chunk).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..index]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(str::to_owned)
                        })
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();
                    if bytes.len() >= index + 4 + length {
                        break serde_json::from_slice::<Value>(
                            &bytes[index + 4..index + 4 + length],
                        )
                        .unwrap();
                    }
                }
            };
            let body = handle(request);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    (endpoint, server)
}

fn invoke_http_adapter(
    fixture: &Fixture,
    endpoint: String,
    credential: Option<String>,
) -> Result<VerifiedOutput, SessionToolInvocationError> {
    invoke_http_adapter_with_runtime(fixture, endpoint, credential, None)
}

fn invoke_http_adapter_with_runtime(
    fixture: &Fixture,
    endpoint: String,
    credential: Option<String>,
    runtime: Option<RuntimeModelBinding>,
) -> Result<VerifiedOutput, SessionToolInvocationError> {
    invoke_http_adapter_with_mode(
        fixture,
        endpoint,
        credential,
        runtime,
        SessionToolResponseMode::JsonOrText,
    )
}

fn invoke_http_adapter_with_mode(
    fixture: &Fixture,
    endpoint: String,
    credential: Option<String>,
    runtime: Option<RuntimeModelBinding>,
    mode: SessionToolResponseMode,
) -> Result<VerifiedOutput, SessionToolInvocationError> {
    invoke_http_adapter_with_profiles(
        fixture,
        endpoint,
        credential,
        runtime,
        mode,
        (
            SessionToolWireMode::CanonicalV1,
            SessionToolWireMode::CanonicalV1,
        ),
    )
}

fn invoke_http_adapter_with_profiles(
    fixture: &Fixture,
    endpoint: String,
    credential: Option<String>,
    runtime: Option<RuntimeModelBinding>,
    mode: SessionToolResponseMode,
    wire_modes: (SessionToolWireMode, SessionToolWireMode),
) -> Result<VerifiedOutput, SessionToolInvocationError> {
    let handoff = fixture.session.invocation().prompt().handoff().clone();
    let tools = BTreeMap::from([(
        fixture.definition.binding.id.clone(),
        fixture.definition.clone(),
    )]);
    let renderer = match runtime.clone() {
        Some(binding) => SessionToolHttpRenderer::for_runtime(
            AdapterKind::ChatCompletionsV1,
            fixture.prepared.clone(),
            handoff,
            tools,
            binding,
        ),
        None => SessionToolHttpRenderer::new(
            AdapterKind::ChatCompletionsV1,
            fixture.prepared.clone(),
            handoff,
            tools,
        ),
    }
    .unwrap()
    .with_wire_mode(wire_modes.0);
    let adapter = SessionToolHttpAdapter::new(
        AdapterKind::ChatCompletionsV1,
        endpoint,
        credential,
        runtime,
    )
    .unwrap()
    .with_response_mode(mode)
    .with_wire_mode(wire_modes.1);
    let profile = TokenizerProfile {
        schema_version: SchemaVersion::V1,
        model: fixture.prepared.capability().data().model.clone(),
        generation_profile: fixture
            .prepared
            .invocation()
            .data()
            .generation
            .content_digest(),
        tokenizer: id("connector-session-test-counter"),
        tokenizer_version: id("1"),
        renderer: renderer.identity().0,
        template_version: renderer.identity().1,
    };
    fixture.session.invoke(SessionToolExecution {
        host: &fixture.host,
        profile,
        renderer: &renderer,
        tokenizer: &Counter,
        limits: InvocationRunLimits {
            schema_version: SchemaVersion::V1,
            max_attempts: 1,
            timeout_ms: 4_000,
            invocation_input_tokens: 50_000,
            invocation_output_tokens: 2_000,
            run_input_tokens: 50_000,
            run_output_tokens: 2_000,
            cost_microunits: 100,
            output_bytes: 100_000,
            reconciliation_tolerance: 4_000,
        },
        context: RequestContext::new(
            "session-tool-http-case".into(),
            Instant::now() + Duration::from_secs(4),
        ),
        reserved_cost_microunits: 10,
        transport: &SessionToolProviderAdapter::new(adapter),
    })
}

#[test]
fn exact_inventory_schema_and_tool_name_produce_only_verified_proposals() {
    let fixture = fixture();
    let (output, seen_tools) = invoke(
        &fixture,
        Ok(completion(
            &fixture,
            vec![ProviderToolCall {
                name: "crm.read_record".into(),
                arguments: json!({"record_id":"42"}),
            }],
        )),
    );
    let verified = output.unwrap();
    assert_eq!(verified.output().data().tools.len(), 1);
    assert_eq!(
        verified.output().data().tools[0].capability,
        fixture.definition.binding
    );
    let pin = fixture.definition.connector_inventory.as_ref().unwrap();
    assert_eq!(pin.connector.as_str(), "crm");
    assert_eq!(
        pin.argument_schema_digest,
        context_content_digest(
            &serde_json::to_vec(&fixture.definition.input.json_schema()).unwrap()
        )
    );
    let rendered_tools = seen_tools.lock().unwrap().clone().unwrap();
    assert_eq!(
        rendered_tools["crm.read_record"]["input"],
        serde_json::to_value(&fixture.definition.input).unwrap()
    );
}

#[test]
fn unknown_provider_tool_name_is_rejected_before_it_can_become_verified_output() {
    let fixture = fixture();
    let (output, _) = invoke(
        &fixture,
        Ok(completion(
            &fixture,
            vec![ProviderToolCall {
                name: "crm.admin_delete".into(),
                arguments: json!({"record_id":"42"}),
            }],
        )),
    );
    assert!(output.is_err());
}

#[test]
fn malformed_provider_arguments_are_rejected_by_the_existing_output_parser() {
    let fixture = fixture();
    let (output, _) = invoke(
        &fixture,
        Ok(completion(
            &fixture,
            vec![ProviderToolCall {
                name: "crm.read_record".into(),
                arguments: json!({"record_id":42}),
            }],
        )),
    );
    assert!(matches!(
        output,
        Err(SessionToolInvocationError::Malformed(_))
    ));
}

#[test]
fn provider_refusal_and_timeout_never_yield_verified_output() {
    let fixture = fixture();
    let mut refused = completion(&fixture, vec![]);
    refused.finish = ProviderFinish::Refused;
    let (refusal, _) = invoke(&fixture, Ok(refused));
    assert!(refusal.is_err());

    let (timeout, _) = invoke(&fixture, Err(ProviderToolFailure::Transient));
    assert!(matches!(
        timeout,
        Err(SessionToolInvocationError::ProviderUnavailable)
    ));
}

#[test]
fn hard_provider_failure_and_invalid_completion_never_yield_verified_output() {
    let fixture = fixture();
    let (hard, _) = invoke(&fixture, Err(ProviderToolFailure::Hard));
    assert!(matches!(hard, Err(SessionToolInvocationError::Run(_))));

    let mut wrong_model = completion(&fixture, vec![]);
    wrong_model.model_id = "unadmitted-model".into();
    let (wrong_model, _) = invoke(&fixture, Ok(wrong_model));
    assert!(matches!(
        wrong_model,
        Err(SessionToolInvocationError::Run(_))
    ));

    let too_many = (0..=gateway_daemon::invocation_host::session_tools::MAX_SESSION_TOOL_CALLS)
        .map(|_| ProviderToolCall {
            name: "crm.read_record".into(),
            arguments: json!({"record_id":"42"}),
        })
        .collect();
    let (oversized, _) = invoke(&fixture, Ok(completion(&fixture, too_many)));
    assert!(matches!(oversized, Err(SessionToolInvocationError::Run(_))));
}

#[test]
fn invocation_error_conversion_preserves_admission_failure() {
    let error: SessionToolInvocationError = InvocationIrError::InvalidBinding.into();
    assert!(matches!(
        error,
        SessionToolInvocationError::Admission(InvocationIrError::InvalidBinding)
    ));
}

#[test]
fn provider_adapter_rejects_cancelled_and_unpinned_attempts_before_provider_call() {
    let fixture = fixture();
    let renderer = Renderer;

    let mut unpinned = fixture.definition.clone();
    unpinned.connector_inventory = None;
    let unpinned_ir = admit_invocation_ir(
        SchemaVersion::V1,
        fixture.session.invocation().prompt().handoff().clone(),
        fixture.prepared.clone(),
        BTreeMap::from([(unpinned.binding.id.clone(), unpinned)]),
        &fixture.host,
    )
    .unwrap();

    for (ir, cancelled) in [(fixture.session.invocation(), true), (&unpinned_ir, false)] {
        let profile = TokenizerProfile {
            schema_version: SchemaVersion::V1,
            model: ir.prepared().capability().data().model.clone(),
            generation_profile: ir
                .prepared()
                .invocation()
                .data()
                .generation
                .content_digest(),
            tokenizer: id("connector-session-test-counter"),
            tokenizer_version: id("1"),
            renderer: renderer.identity().0,
            template_version: renderer.identity().1,
        };
        let rendered =
            render_invocation_ir(ir, &fixture.host, profile, &renderer, &Counter).unwrap();
        let accounting = rendered.rendered().accounting();
        let binding = AttemptBinding {
            schema_version: SchemaVersion::V1,
            attempt: 1,
            invocation: ir.identity(),
            profile: accounting.count.profile.clone(),
            payload_digest: accounting.count.payload_digest.clone(),
        };
        let context = RequestContext::new(
            "session-tool-invalid-attempt".into(),
            Instant::now() + Duration::from_secs(4),
        );
        if cancelled {
            context.cancel();
        }
        let seen_tools = Arc::new(Mutex::new(None));
        let adapter = SessionToolProviderAdapter::new(FixedProvider {
            response: Arc::new(Mutex::new(Ok(completion(&fixture, vec![])))),
            seen_tools: seen_tools.clone(),
        });
        let response = adapter.invoke(BoundedInvocationRequest {
            ir,
            rendered: &rendered,
            binding: &binding,
            context: &context,
            max_output_bytes: 100_000,
            reserved_cost_microunits: 10,
        });
        assert!(matches!(
            response.output,
            Err(InvocationAttemptFailure::Hard)
        ));
        assert!(seen_tools.lock().unwrap().is_none());
    }
}

#[test]
fn session_http_adapter_posts_exact_tools_and_maps_native_tool_calls() {
    let fixture = fixture();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let expected_model = fixture.model_id.clone();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("HTTP adapter did not connect: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0; 8192];
        let (start, length) = loop {
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
            if let Some(i) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..i]);
                let len = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_owned)
                    })
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                if bytes.len() >= i + 4 + len {
                    break (i + 4, len);
                }
            }
        };
        let request: Value = serde_json::from_slice(&bytes[start..start + length]).unwrap();
        assert_eq!(request["model"], expected_model);
        assert_eq!(request["tools"][0]["function"]["name"], "crm.read_record");
        assert_eq!(
            request["tools"][0]["function"]["parameters"]["properties"]["record_id"]["type"],
            "string"
        );
        let response = json!({
            "model": expected_model,
            "choices":[{"finish_reason":"tool_calls","message":{
                "content":"found","refusal":null,
                "tool_calls":[{"type":"function","id":"call-1","function":{
                    "name":"crm.read_record","arguments":"{\"record_id\":\"42\"}"}}]
            }}],
            "usage":{"prompt_tokens":100,"completion_tokens":32}
        })
        .to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.len(),
            response
        )
        .unwrap();
    });
    let renderer = SessionToolHttpRenderer::new(
        AdapterKind::ChatCompletionsV1,
        fixture.prepared.clone(),
        fixture.session.invocation().prompt().handoff().clone(),
        BTreeMap::from([(
            fixture.definition.binding.id.clone(),
            fixture.definition.clone(),
        )]),
    )
    .unwrap();
    let adapter =
        SessionToolHttpAdapter::new(AdapterKind::ChatCompletionsV1, endpoint, None, None).unwrap();
    let profile = TokenizerProfile {
        schema_version: SchemaVersion::V1,
        model: fixture.prepared.capability().data().model.clone(),
        generation_profile: fixture
            .prepared
            .invocation()
            .data()
            .generation
            .content_digest(),
        tokenizer: id("connector-session-test-counter"),
        tokenizer_version: id("1"),
        renderer: renderer.identity().0,
        template_version: renderer.identity().1,
    };
    let limits = InvocationRunLimits {
        schema_version: SchemaVersion::V1,
        max_attempts: 1,
        timeout_ms: 4000,
        invocation_input_tokens: 50000,
        invocation_output_tokens: 2000,
        run_input_tokens: 50000,
        run_output_tokens: 2000,
        cost_microunits: 100,
        output_bytes: 100000,
        reconciliation_tolerance: 4000,
    };
    let context = RequestContext::new(
        "http-session-test".into(),
        Instant::now() + Duration::from_secs(4),
    );
    let output = fixture.session.invoke(SessionToolExecution {
        host: &fixture.host,
        profile,
        renderer: &renderer,
        tokenizer: &Counter,
        limits,
        context,
        reserved_cost_microunits: 10,
        transport: &SessionToolProviderAdapter::new(adapter),
    });
    let server_result = server.join();
    assert!(
        output.is_ok(),
        "HTTP invocation failed: {output:?}; server: {server_result:?}"
    );
    assert!(output.is_ok());
    assert_eq!(output.unwrap().output().data().tools.len(), 1);
}

#[test]
fn portable_http_tools_preserve_exact_schemas_and_return_canonical_proposals() {
    for (schema, strict) in [
        (
            json!({"type":"object","properties":{"record_id":{"type":"string"}},"required":["record_id"],"additionalProperties":false}),
            true,
        ),
        (
            json!({"type":"object","properties":{"record_id":{"type":"string"},"label":{"type":"string"}},"required":["record_id"],"additionalProperties":false}),
            false,
        ),
    ] {
        for runtime_id in [None, Some("operator-pinned-model-v1")] {
            let fixture = fixture_with_schema(schema.clone());
            let expected_schema = fixture.definition.input.json_schema();
            let model_id = runtime_id.unwrap_or(&fixture.model_id).to_owned();
            let expected_model_id = model_id.clone();
            let aliases = Arc::new(Mutex::new(Vec::new()));
            let seen_aliases = aliases.clone();
            let arguments = json!({"record_id":"42"});
            let expected_arguments = arguments.clone();
            let (endpoint, server) = serve_http_handler(2, move |request| {
                assert_eq!(request["model"], expected_model_id);
                assert_eq!(request["tools"].as_array().unwrap().len(), 1);
                let function = &request["tools"][0]["function"];
                let alias = function["name"].as_str().unwrap();
                assert_eq!(alias.len(), 64);
                assert!(
                    alias
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
                );
                assert_ne!(alias, "crm.read_record");
                assert_eq!(function["parameters"], expected_schema);
                assert_eq!(function["strict"], strict);
                seen_aliases.lock().unwrap().push(alias.to_owned());
                json!({"model":expected_model_id,"choices":[{"finish_reason":"tool_calls","message":{"content":null,"tool_calls":[{"type":"function","function":{"name":alias,"arguments":arguments.to_string()}}]}}]}).to_string()
            });
            let binding = runtime_id.map(|runtime_id| RuntimeModelBinding {
                canonical: fixture.prepared.capability().data().model.clone(),
                runtime_id: runtime_id.to_owned(),
            });
            for _ in 0..2 {
                let verified = invoke_http_adapter_with_profiles(
                    &fixture,
                    endpoint.clone(),
                    None,
                    binding.clone(),
                    SessionToolResponseMode::ToolProposalTextV1,
                    (
                        SessionToolWireMode::PortableV1,
                        SessionToolWireMode::PortableV1,
                    ),
                )
                .unwrap();
                assert_eq!(
                    verified.output().data().tools[0].capability,
                    fixture.definition.binding
                );
                assert_eq!(
                    verified.output().data().tools[0].arguments,
                    expected_arguments
                );
                assert_eq!(verified.output().data().result, json!(""));
            }
            server.join().unwrap();
            let aliases = aliases.lock().unwrap();
            assert_eq!(aliases[0], aliases[1]);
        }
    }
}

#[test]
fn portable_http_tools_reject_native_unknown_and_stale_inventory_aliases() {
    let mut fixture = fixture();
    let old_alias = Arc::new(Mutex::new(String::new()));
    let capture = old_alias.clone();
    let model_id = fixture.model_id.clone();
    let (endpoint, server) = serve_http_handler(1, move |request| {
        let alias = request["tools"][0]["function"]["name"].as_str().unwrap();
        *capture.lock().unwrap() = alias.to_owned();
        json!({"model":model_id,"choices":[{"finish_reason":"tool_calls","message":{"content":null,"tool_calls":[{"type":"function","function":{"name":alias,"arguments":"{\"record_id\":\"42\"}"}}]}}]}).to_string()
    });
    invoke_http_adapter_with_profiles(
        &fixture,
        endpoint,
        None,
        None,
        SessionToolResponseMode::ToolProposalTextV1,
        (
            SessionToolWireMode::PortableV1,
            SessionToolWireMode::PortableV1,
        ),
    )
    .unwrap();
    server.join().unwrap();

    fixture
        .definition
        .connector_inventory
        .as_mut()
        .unwrap()
        .revision = context_content_digest(b"new inventory");
    fixture.session = SessionToolInvocation::admit(
        fixture.session.invocation().prompt().handoff().clone(),
        fixture.prepared.clone(),
        BTreeMap::from([(
            fixture.definition.binding.id.clone(),
            fixture.definition.clone(),
        )]),
        &fixture.host,
    )
    .unwrap();
    let stale_alias = old_alias.lock().unwrap().clone();
    for returned_name in [
        "crm.read_record".to_owned(),
        "unknown_function".to_owned(),
        stale_alias.clone(),
    ] {
        let model_id = fixture.model_id.clone();
        let old_alias = stale_alias.clone();
        let (endpoint, server) = serve_http_handler(1, move |request| {
            assert_ne!(request["tools"][0]["function"]["name"], old_alias);
            json!({"model":model_id,"choices":[{"finish_reason":"tool_calls","message":{"content":null,"tool_calls":[{"type":"function","function":{"name":returned_name,"arguments":"{\"record_id\":\"42\"}"}}]}}]}).to_string()
        });
        assert!(
            invoke_http_adapter_with_profiles(
                &fixture,
                endpoint,
                None,
                None,
                SessionToolResponseMode::ToolProposalTextV1,
                (
                    SessionToolWireMode::PortableV1,
                    SessionToolWireMode::PortableV1
                )
            )
            .is_err()
        );
        server.join().unwrap();
    }
}

#[test]
fn tool_wire_profile_mismatch_is_denied_before_http() {
    let fixture = fixture();
    for modes in [
        (
            SessionToolWireMode::CanonicalV1,
            SessionToolWireMode::PortableV1,
        ),
        (
            SessionToolWireMode::PortableV1,
            SessionToolWireMode::CanonicalV1,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        assert!(
            invoke_http_adapter_with_profiles(
                &fixture,
                format!("http://{}", listener.local_addr().unwrap()),
                None,
                None,
                SessionToolResponseMode::ToolProposalTextV1,
                modes
            )
            .is_err()
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}

#[test]
fn http_tool_json_rejects_duplicate_keys_and_depth_without_text_fallback() {
    let fixture = fixture();
    let response = |arguments: &str, content: Value| {
        json!({"model":fixture.model_id,
        "choices":[{"finish_reason":"tool_calls","message":{"content":content,
        "tool_calls":[{"type":"function","function":{"name":"crm.read_record","arguments":arguments}}]}}]}).to_string()
    };
    let safe_args = r#"{"record_id":"42"}"#;
    let ordinary = response(safe_args, json!("ordinary text"));
    let model_pair = format!(
        "\"model\":{}",
        serde_json::to_string(&fixture.model_id).unwrap()
    );
    let duplicated_model =
        ordinary.replace(&model_pair, &format!("\"model\":\"foreign\",{model_pair}"));
    let mut rejected = vec![
        duplicated_model,
        response(r#"{"record_id":"foreign","record_id":"42"}"#, json!("text")),
        response(
            r#"{"record_id":"foreign","\u0072ecord_id":"42"}"#,
            json!("text"),
        ),
    ];
    for depth in [65, 150] {
        rejected.push(response(
            safe_args,
            json!(format!("{}0{}", "[".repeat(depth), "]".repeat(depth))),
        ));
    }
    let (endpoint, server) = serve_http_responses(rejected);
    for _ in 0..5 {
        assert!(invoke_http_adapter(&fixture, endpoint.clone(), None).is_err());
    }
    server.join().unwrap();
    // Ordinary malformed JSON remains legacy text. An explicitly selected text
    // profile also preserves JSON-looking text without interpreting its contents.
    for (mode, content) in [
        (
            SessionToolResponseMode::JsonOrText,
            "{ordinary text".to_owned(),
        ),
        (
            SessionToolResponseMode::ToolProposalTextV1,
            format!("{}0{}", "[".repeat(150), "]".repeat(150)),
        ),
    ] {
        let (endpoint, server) = serve_http_responses(vec![response(safe_args, json!(content))]);
        let verified = invoke_http_adapter_with_mode(&fixture, endpoint, None, None, mode).unwrap();
        assert_eq!(verified.output().data().result, json!(content));
        server.join().unwrap();
    }

    let mut output = support::model::output();
    output.shape = OutputType::Object {
        fields: BTreeMap::from([(
            "message".into(),
            OutputField {
                required: true,
                value_type: OutputType::String {},
            },
        )]),
    };
    let object_fixture = fixture_with_handoff(
        fixture.definition.input.json_schema(),
        support::handoff_for_output(support::examples::task(), output),
    );
    for (content, accepted) in [
        (r#"{"message":"last"}"#, true),
        (r#"{"message":"first","message":"last"}"#, false),
        (r#"{"message":"first","\u006dessage":"last"}"#, false),
    ] {
        let raw = response(safe_args, json!(content));
        let (endpoint, server) = serve_http_responses(vec![raw]);
        assert_eq!(
            invoke_http_adapter(&object_fixture, endpoint, None).is_ok(),
            accepted
        );
        server.join().unwrap();
    }
}

#[test]
fn escaped_provider_credentials_are_rejected_after_each_consumed_json_decode() {
    let secret = "response-secret-canary";
    for location in ["content", "arguments", "json_result"] {
        let fixture = fixture();
        let mut message = json!({"content":"ordinary text","tool_calls":[{"type":"function","function":{"name":"crm.read_record","arguments":"{\"record_id\":\"42\"}"}}]});
        let mode = match location {
            "arguments" => {
                message["tool_calls"][0]["function"]["arguments"] =
                    json!(r#"{"record_id":"\u0072esponse-secret-canary"}"#);
                SessionToolResponseMode::ToolProposalTextV1
            }
            "json_result" => {
                message["content"] = json!(r#""\u0072esponse-secret-canary""#);
                SessionToolResponseMode::JsonOrText
            }
            _ => {
                message["content"] = json!(secret);
                SessionToolResponseMode::ToolProposalTextV1
            }
        };
        let mut response = json!({"model":fixture.model_id,"choices":[{"finish_reason":"tool_calls","message":message}]}).to_string();
        if location == "content" {
            response = response.replace(secret, r"\u0072esponse-secret-canary");
        }
        assert!(!response.contains(secret));
        let (endpoint, server) = serve_http_responses(vec![response]);
        assert!(
            invoke_http_adapter_with_mode(&fixture, endpoint, Some(secret.into()), None, mode)
                .is_err()
        );
        server.join().unwrap();
    }
}

#[test]
fn explicit_tool_text_profile_accepts_only_actual_calls_and_preserves_empty_text() {
    // These transport/verification fixtures qualify the adapter mapping. The
    // native mixed-plan producer and actual consent lifecycle are separate tests.
    for content in [
        None,
        Some(Value::Null),
        Some(json!("")),
        Some(json!("{\"text\":true}")),
    ] {
        let fixture = fixture();
        let mut message = json!({"refusal":null,"tool_calls":[{"id":"actual-call","type":"function","function":{"name":"crm.read_record","arguments":"{\"record_id\":\"42\"}"}}]});
        if let Some(value) = &content {
            message["content"] = value.clone();
        }
        let response = json!({"model":fixture.model_id,"choices":[{"finish_reason":"tool_calls","message":message}],"usage":{"prompt_tokens":100,"completion_tokens":20}});
        let (endpoint, server) = serve_http_responses(vec![response.to_string()]);
        let verified = invoke_http_adapter_with_mode(
            &fixture,
            endpoint,
            None,
            None,
            SessionToolResponseMode::ToolProposalTextV1,
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(verified.output().data().tools.len(), 1);
        assert_eq!(
            verified.output().data().result,
            match content {
                Some(Value::String(text)) => Value::String(text),
                _ => json!(""),
            }
        );
    }
    for (content, calls) in [
        (Value::Null, false),
        (json!("No call was proposed"), false),
        (json!(true), true),
        (json!(1), true),
        (json!({"unexpected":"object"}), true),
        (json!([]), true),
    ] {
        let fixture = fixture();
        let message = json!({"content":content,"refusal":null,"tool_calls":if calls {json!([{"id":"actual-call","type":"function","function":{"name":"crm.read_record","arguments":"{\"record_id\":\"42\"}"}}])}else{json!([])}});
        let response = json!({"model":fixture.model_id,"choices":[{"finish_reason":"tool_calls","message":message}],"usage":{"prompt_tokens":100,"completion_tokens":20}});
        let (endpoint, server) = serve_http_responses(vec![response.to_string()]);
        assert!(
            invoke_http_adapter_with_mode(
                &fixture,
                endpoint,
                None,
                None,
                SessionToolResponseMode::ToolProposalTextV1
            )
            .is_err()
        );
        server.join().unwrap();
    }
    // The original constructor retains its strict null-result behavior.
    let fixture = fixture();
    let response = json!({"model":fixture.model_id,"choices":[{"finish_reason":"tool_calls","message":{"content":null,"refusal":null,"tool_calls":[{"id":"actual-call","type":"function","function":{"name":"crm.read_record","arguments":"{\"record_id\":\"42\"}"}}]}}],"usage":{"prompt_tokens":100,"completion_tokens":20}});
    let (endpoint, server) = serve_http_responses(vec![response.to_string()]);
    assert!(invoke_http_adapter(&fixture, endpoint, None).is_err());
    server.join().unwrap();
}

#[test]
fn session_http_adapter_rejects_malformed_and_mismatched_provider_responses() {
    let fixture = fixture();
    let valid_call = json!({
        "type":"function",
        "id":"call-1",
        "function":{"name":"crm.read_record","arguments":"{\"record_id\":\"42\"}"}
    });
    let base = |model: &str, finish: &str, message: Value| json!({"model":model,"choices":[{"finish_reason":finish,"message":message}]});
    let mut responses = vec![
        base(
            &fixture.model_id,
            "tool_calls",
            json!({
                "content":"ok","tool_calls":[valid_call.clone()]
            }),
        ),
        base("wrong-model", "tool_calls", json!({"tool_calls":[]})),
        base(&fixture.model_id, "length", json!({"tool_calls":[]})),
        base(
            &fixture.model_id,
            "tool_calls",
            json!({"refusal":"refused","tool_calls":[]}),
        ),
        base(
            &fixture.model_id,
            "tool_calls",
            json!({"tool_calls":[{
                "type":"not-a-function","function":{"name":"crm.read_record","arguments":"{}"}
            }]}),
        ),
        base(
            &fixture.model_id,
            "tool_calls",
            json!({"tool_calls":[{
                "type":"function","function":{"arguments":"{}"}
            }]}),
        ),
        base(
            &fixture.model_id,
            "tool_calls",
            json!({"tool_calls":[{
                "type":"function","function":{"name":"crm.read_record","arguments":"{"}
            }]}),
        ),
        {
            let mut response = base(
                &fixture.model_id,
                "tool_calls",
                json!({
                    "tool_calls":[valid_call.clone()]
                }),
            );
            response["usage"] = json!({"prompt_tokens":"many","completion_tokens":1});
            response
        },
        base(&fixture.model_id, "stop", json!({"content":null})),
    ]
    .into_iter()
    .map(|response| response.to_string())
    .collect::<Vec<_>>();
    responses.push("not-json".into());
    let echo_secret = "response-secret-canary";
    responses.push(echo_secret.into());
    let too_many = (0..=gateway_daemon::invocation_host::session_tools::MAX_SESSION_TOOL_CALLS)
        .map(|_| valid_call.clone())
        .collect::<Vec<_>>();
    responses.push(
        base(
            &fixture.model_id,
            "tool_calls",
            json!({"tool_calls":too_many}),
        )
        .to_string(),
    );
    let (endpoint, server) = serve_http_responses(responses);
    for index in 0..12 {
        let credential = (index == 10).then(|| echo_secret.to_owned());
        let result = invoke_http_adapter(&fixture, endpoint.clone(), credential);
        if index == 0 {
            assert!(
                result.is_ok(),
                "valid baseline provider response was rejected"
            );
        } else {
            assert!(
                result.is_err(),
                "invalid provider response was accepted at index {index}"
            );
        }
    }
    server.join().unwrap();

    let unavailable = invoke_http_adapter(&fixture, "http://127.0.0.1:1".into(), None);
    assert!(matches!(
        unavailable,
        Err(SessionToolInvocationError::ProviderUnavailable)
    ));
}

#[test]
fn no_model_or_provider_response_can_add_an_unadmitted_capability() {
    let fixture = fixture();
    let mut tools = fixture.session.invocation().prompt().tools().clone();
    let mut extra = fixture.definition.clone();
    extra.binding.id = CapabilityId::new("crm.admin_delete").unwrap();
    tools.insert(extra.binding.id.clone(), extra);
    let error = SessionToolInvocation::admit(
        fixture.session.invocation().prompt().handoff().clone(),
        fixture.session.invocation().prepared().clone(),
        tools,
        &fixture.host,
    )
    .unwrap_err();
    assert_eq!(error, InvocationIrError::InvalidBinding);
}

#[test]
fn inventory_schema_pin_drift_is_refused_during_host_admission() {
    let fixture = fixture();
    let mut definition = fixture.definition.clone();
    definition.input = OutputType::String {};
    let error = SessionToolInvocation::admit(
        fixture.session.invocation().prompt().handoff().clone(),
        fixture.session.invocation().prepared().clone(),
        BTreeMap::from([(definition.binding.id.clone(), definition)]),
        &fixture.host,
    )
    .unwrap_err();
    assert_eq!(error, InvocationIrError::InvalidBinding);
}

#[test]
fn host_admission_rejects_tools_from_different_connector_inventory_revisions() {
    let fixture = fixture();
    let handoff = fixture.session.invocation().prompt().handoff().clone();
    let mut invocation = fixture.prepared.invocation().data().clone();
    let second_id = CapabilityId::new("crm.search_records").unwrap();
    let mut second_binding = fixture.definition.binding.clone();
    second_binding.id = second_id.clone();
    second_binding.revision = id("inventory-r2");
    second_binding.digest = ContentDigest::new("c".repeat(64)).unwrap();
    let mut admitted = invocation.permitted_capabilities.clone();
    admitted.insert(second_id.clone(), second_binding.clone());
    invocation.permitted_capabilities = admitted.clone();
    let model = ModelInvocation::new(
        invocation.clone(),
        ModelInvocationBasis {
            task: handoff.task(),
            output: &invocation.output,
            compiled_context: &invocation.compiled_context,
            permitted_capabilities: &admitted,
            sensitivity: invocation.sensitivity,
            permitted_localities: &invocation.permitted_localities,
            authority: &invocation.authority,
        },
    )
    .unwrap();
    let prepared = gateway_application::model_invocation::prepare_model_invocation(
        model,
        fixture.prepared.capability().clone(),
        fixture.prepared.provider().clone(),
        &support::model::runtime(),
    )
    .unwrap();

    let mut second_tool = fixture.definition.clone();
    second_tool.binding = second_binding;
    second_tool.connector_inventory.as_mut().unwrap().revision =
        ContentDigest::new("f".repeat(64)).unwrap();
    let tools = BTreeMap::from([
        (
            fixture.definition.binding.id.clone(),
            fixture.definition.clone(),
        ),
        (second_id, second_tool),
    ]);
    let error = SessionToolInvocation::admit(handoff, prepared, tools, &fixture.host).unwrap_err();
    assert_eq!(error, InvocationIrError::InvalidBinding);
}

#[test]
fn host_admission_rejects_empty_unpinned_miskeyed_and_schema_drifted_tools() {
    let fixture = fixture();
    let handoff = support::handoff();

    assert!(matches!(
        SessionToolInvocation::admit(
            handoff.clone(),
            fixture.prepared.clone(),
            BTreeMap::new(),
            &fixture.host
        ),
        Err(InvocationIrError::InvalidBinding)
    ));

    let mut unpinned = fixture.definition.clone();
    unpinned.connector_inventory = None;
    assert!(matches!(
        SessionToolInvocation::admit(
            handoff.clone(),
            fixture.prepared.clone(),
            BTreeMap::from([(unpinned.binding.id.clone(), unpinned)]),
            &fixture.host
        ),
        Err(InvocationIrError::InvalidBinding)
    ));

    let mut miskeyed = fixture.definition.clone();
    let wrong_key = CapabilityId::new("crm.other").unwrap();
    assert!(matches!(
        SessionToolInvocation::admit(
            handoff.clone(),
            fixture.prepared.clone(),
            BTreeMap::from([(wrong_key, miskeyed.clone())]),
            &fixture.host
        ),
        Err(InvocationIrError::InvalidBinding)
    ));

    miskeyed
        .connector_inventory
        .as_mut()
        .unwrap()
        .argument_schema_digest = context_content_digest(b"different schema");
    assert!(matches!(
        SessionToolInvocation::admit(
            handoff,
            fixture.prepared.clone(),
            BTreeMap::from([(miskeyed.binding.id.clone(), miskeyed)]),
            &fixture.host
        ),
        Err(InvocationIrError::InvalidBinding)
    ));
}

#[test]
fn session_verification_host_delegates_trusted_checks_and_fails_closed_without_reference_authority()
{
    struct Authority;
    impl SessionToolAuthorityPort for Authority {
        fn validate_semantic_reference(
            &self,
            _: &SemanticTaskData,
            _: &SemanticReferenceBinding,
        ) -> Result<(), ValidationError> {
            Ok(())
        }

        fn validate_session_invocation(&self, _: &InvocationIr) -> Result<(), InvocationIrError> {
            Ok(())
        }

        fn resolve_reference(
            &self,
            _: &InvocationIr,
            _: &OutputReference,
        ) -> Result<(), gateway_domain::output_verification::VerificationError> {
            Ok(())
        }

        fn evaluate_output(
            &self,
            ir: &InvocationIr,
            output: &ParsedOutput,
        ) -> Result<ConstraintReport, gateway_domain::output_verification::VerificationError>
        {
            Ok(ConstraintReport {
                binding: output.binding(),
                policy: CheckOutcome::Passed,
                process: CheckOutcome::Passed,
                task_completeness: CheckOutcome::Passed,
                semantic_output: CheckOutcome::Passed,
                checks: ir
                    .prompt()
                    .handoff()
                    .task()
                    .data()
                    .verification_contract
                    .checks
                    .iter()
                    .filter(|check| **check != SemanticVerificationCheck::OutputSchemaValid)
                    .map(|check| (*check, CheckOutcome::Passed))
                    .collect(),
            })
        }
    }

    struct References;
    impl SessionToolSemanticReferencePort for References {
        fn validate_reference(
            &self,
            _: &SemanticTaskData,
            _: &SemanticReferenceBinding,
        ) -> Result<(), ValidationError> {
            Ok(())
        }
    }

    let fixture = fixture();
    let default_host = SessionToolVerificationHost::new(Authority);
    assert!(
        gateway_domain::semantic_task::SemanticReferenceValidator::validate(
            &default_host,
            fixture
                .session
                .invocation()
                .prompt()
                .handoff()
                .task()
                .data(),
            &SemanticReferenceBinding {
                id: "entity".into(),
                scope: fixture.prepared.invocation().data().task.scope.clone(),
                contract: id("entity-contract"),
                contract_version: SchemaVersion::V1,
                revision: id("entity-r1"),
                digest: ContentDigest::new("a".repeat(64)).unwrap(),
            },
        )
        .is_err()
    );
    assert!(
        gateway_application::prompt_ir::InvocationAdmissionPort::validate_current(
            &default_host,
            fixture.session.invocation(),
        )
        .is_ok()
    );

    let host = SessionToolVerificationHost::with_references(Authority, References);
    let semantic = SemanticReferenceBinding {
        id: "entity".into(),
        scope: fixture.prepared.invocation().data().task.scope.clone(),
        contract: id("entity-contract"),
        contract_version: SchemaVersion::V1,
        revision: id("entity-r1"),
        digest: ContentDigest::new("a".repeat(64)).unwrap(),
    };
    assert!(
        gateway_domain::semantic_task::SemanticReferenceValidator::validate(
            &host,
            fixture
                .session
                .invocation()
                .prompt()
                .handoff()
                .task()
                .data(),
            &semantic,
        )
        .is_ok()
    );

    let (result, _) = invoke(
        &fixture,
        Ok(completion(
            &fixture,
            vec![ProviderToolCall {
                name: "crm.read_record".into(),
                arguments: json!({"record_id":"rec-1"}),
            }],
        )),
    );
    let parsed = result.unwrap().output().clone();
    let reference = OutputReference {
        pointer: "/result".into(),
        binding: ResolvedReference {
            id: id("entity"),
            scope: fixture.prepared.invocation().data().task.scope.clone(),
            contract: id("entity-contract"),
            contract_version: SchemaVersion::V1,
            revision: id("entity-r1"),
            digest: ContentDigest::new("a".repeat(64)).unwrap(),
        },
    };
    assert!(
        gateway_application::output_verification::OutputVerificationPort::resolve_reference(
            &host,
            fixture.session.invocation(),
            &reference,
        )
        .is_ok()
    );
    assert!(
        gateway_application::output_verification::OutputVerificationPort::evaluate(
            &host,
            fixture.session.invocation(),
            &parsed,
        )
        .is_ok()
    );
}

#[test]
fn session_tool_provider_adapter_rejects_wrong_model_finish_call_limit_and_output_limit() {
    let fixture = fixture();

    let mut wrong_model = completion(&fixture, vec![]);
    wrong_model.model_id = "other-model".into();
    assert!(invoke(&fixture, Ok(wrong_model)).0.is_err());

    let mut refused = completion(&fixture, vec![]);
    refused.finish = ProviderFinish::Refused;
    assert!(invoke(&fixture, Ok(refused)).0.is_err());
    let mut incomplete = completion(&fixture, vec![]);
    incomplete.finish = ProviderFinish::Incomplete;
    assert!(invoke(&fixture, Ok(incomplete)).0.is_err());

    let too_many = (0..=gateway_daemon::invocation_host::session_tools::MAX_SESSION_TOOL_CALLS)
        .map(|_| ProviderToolCall {
            name: "crm.read_record".into(),
            arguments: json!({"record_id":"rec-1"}),
        })
        .collect();
    assert!(
        invoke(&fixture, Ok(completion(&fixture, too_many)))
            .0
            .is_err()
    );

    let mut oversized = completion(&fixture, vec![]);
    oversized.result = Value::String("x".repeat(110_000));
    assert!(invoke(&fixture, Ok(oversized)).0.is_err());
}

#[test]
fn session_tool_provider_observes_cancellation_after_provider_completion() {
    struct CancellingProvider;
    impl SessionToolProviderPort for CancellingProvider {
        fn complete(
            &self,
            request: BoundedInvocationRequest<'_>,
        ) -> Result<ProviderToolCompletion, ProviderToolFailure> {
            request.context.cancel();
            Ok(ProviderToolCompletion {
                model_id: request
                    .ir
                    .prepared()
                    .capability()
                    .data()
                    .model
                    .id
                    .as_str()
                    .into(),
                finish: ProviderFinish::Complete,
                result: json!("done"),
                calls: vec![],
                usage: None,
            })
        }
    }
    let fixture = fixture();
    let adapter = SessionToolProviderAdapter::new(CancellingProvider);
    let renderer = Renderer;
    let profile = TokenizerProfile {
        schema_version: SchemaVersion::V1,
        model: fixture
            .session
            .invocation()
            .prepared()
            .capability()
            .data()
            .model
            .clone(),
        generation_profile: fixture
            .session
            .invocation()
            .prepared()
            .invocation()
            .data()
            .generation
            .content_digest(),
        tokenizer: id("connector-session-test-counter"),
        tokenizer_version: id("1"),
        renderer: renderer.identity().0,
        template_version: renderer.identity().1,
    };
    assert!(
        fixture
            .session
            .invoke(SessionToolExecution {
                host: &fixture.host,
                profile,
                renderer: &renderer,
                tokenizer: &Counter,
                limits: InvocationRunLimits {
                    schema_version: SchemaVersion::V1,
                    max_attempts: 1,
                    timeout_ms: 4_000,
                    invocation_input_tokens: 50_000,
                    invocation_output_tokens: 2_000,
                    run_input_tokens: 50_000,
                    run_output_tokens: 2_000,
                    cost_microunits: 100,
                    output_bytes: 100_000,
                    reconciliation_tolerance: 4_000,
                },
                context: RequestContext::new(
                    "session-tool-cancel-test".into(),
                    Instant::now() + Duration::from_secs(4),
                ),
                reserved_cost_microunits: 10,
                transport: &adapter,
            })
            .is_err()
    );
}

#[test]
fn session_http_renderer_and_adapter_reject_missing_or_drifted_pins() {
    let fixture = fixture();
    let tools = BTreeMap::from([(
        fixture.definition.binding.id.clone(),
        fixture.definition.clone(),
    )]);
    assert!(
        SessionToolHttpRenderer::new(
            AdapterKind::ChatCompletionsV1,
            fixture.prepared.clone(),
            fixture.session.invocation().prompt().handoff().clone(),
            tools.clone(),
        )
        .is_ok()
    );
    assert!(matches!(
        SessionToolHttpRenderer::new(
            AdapterKind::ChatCompletionsV1,
            fixture.prepared.clone(),
            fixture.session.invocation().prompt().handoff().clone(),
            BTreeMap::new(),
        ),
        Err(gateway_daemon::model_adapters::AdapterDiagnostic::CapabilityMismatch)
    ));

    let mut missing_pin = fixture.definition.clone();
    missing_pin.connector_inventory = None;
    assert!(matches!(
        SessionToolHttpRenderer::new(
            AdapterKind::ChatCompletionsV1,
            fixture.prepared.clone(),
            fixture.session.invocation().prompt().handoff().clone(),
            BTreeMap::from([(missing_pin.binding.id.clone(), missing_pin)]),
        ),
        Err(gateway_daemon::model_adapters::AdapterDiagnostic::CapabilityMismatch)
    ));

    let mut schema_drift = fixture.definition.clone();
    schema_drift
        .connector_inventory
        .as_mut()
        .unwrap()
        .argument_schema_digest = ContentDigest::new("e".repeat(64)).unwrap();
    assert!(matches!(
        SessionToolHttpRenderer::new(
            AdapterKind::ChatCompletionsV1,
            fixture.prepared.clone(),
            fixture.session.invocation().prompt().handoff().clone(),
            BTreeMap::from([(schema_drift.binding.id.clone(), schema_drift)]),
        ),
        Err(gateway_daemon::model_adapters::AdapterDiagnostic::CapabilityMismatch)
    ));

    assert!(
        SessionToolHttpAdapter::new(
            AdapterKind::ChatCompletionsV1,
            "not-a-url".into(),
            None,
            None,
        )
        .is_err()
    );
    assert!(matches!(
        SessionToolHttpAdapter::new(
            AdapterKind::ChatCompletionsV1,
            "http://127.0.0.1:1".into(),
            None,
            Some(RuntimeModelBinding {
                canonical: fixture.prepared.capability().data().model.clone(),
                runtime_id: "bad model id".into(),
            }),
        ),
        Err(gateway_daemon::model_adapters::AdapterDiagnostic::InvalidBinding)
    ));

    let local_renderer = SessionToolHttpRenderer::new(
        AdapterKind::LocalServiceV1,
        fixture.prepared.clone(),
        fixture.session.invocation().prompt().handoff().clone(),
        tools,
    )
    .unwrap();
    let profile = TokenizerProfile {
        schema_version: SchemaVersion::V1,
        model: fixture.prepared.capability().data().model.clone(),
        generation_profile: fixture
            .prepared
            .invocation()
            .data()
            .generation
            .content_digest(),
        tokenizer: id("connector-session-test-counter"),
        tokenizer_version: id("1"),
        renderer: local_renderer.identity().0,
        template_version: local_renderer.identity().1,
    };
    assert!(matches!(
        render_invocation_ir(
            fixture.session.invocation(),
            &fixture.host,
            profile,
            &local_renderer,
            &Counter,
        ),
        Err(
            gateway_application::prompt_ir::InvocationPreparationError::Rendering(
                gateway_application::prompt_rendering::RenderError::Diagnostic(
                    RenderDiagnostic::UnsupportedRenderer
                )
            )
        )
    ));
}

#[test]
fn session_http_renderer_rejects_a_different_admitted_tool_set() {
    let fixture = fixture();
    let handoff = fixture.session.invocation().prompt().handoff().clone();
    let mut data = fixture.prepared.invocation().data().clone();
    data.permitted_capabilities.clear();
    let empty_capabilities = BTreeMap::new();
    let no_tools = BTreeMap::<CapabilityId, ToolDefinition>::new();
    let model = ModelInvocation::new(
        data.clone(),
        ModelInvocationBasis {
            task: handoff.task(),
            output: &data.output,
            compiled_context: &data.compiled_context,
            permitted_capabilities: &empty_capabilities,
            sensitivity: data.sensitivity,
            permitted_localities: &data.permitted_localities,
            authority: &data.authority,
        },
    )
    .unwrap();
    let prepared = gateway_application::model_invocation::prepare_model_invocation(
        model,
        fixture.prepared.capability().clone(),
        fixture.prepared.provider().clone(),
        &support::model::runtime(),
    )
    .unwrap();
    let no_tool_ir = admit_invocation_ir(
        SchemaVersion::V1,
        handoff.clone(),
        prepared.clone(),
        no_tools,
        &fixture.host,
    )
    .unwrap();
    let renderer = SessionToolHttpRenderer::new(
        AdapterKind::ChatCompletionsV1,
        fixture.prepared.clone(),
        handoff,
        BTreeMap::from([(
            fixture.definition.binding.id.clone(),
            fixture.definition.clone(),
        )]),
    )
    .unwrap();
    let profile = TokenizerProfile {
        schema_version: SchemaVersion::V1,
        model: prepared.capability().data().model.clone(),
        generation_profile: prepared.invocation().data().generation.content_digest(),
        tokenizer: id("connector-session-test-counter"),
        tokenizer_version: id("1"),
        renderer: renderer.identity().0,
        template_version: renderer.identity().1,
    };
    assert!(matches!(
        render_invocation_ir(&no_tool_ir, &fixture.host, profile, &renderer, &Counter),
        Err(
            gateway_application::prompt_ir::InvocationPreparationError::Rendering(
                gateway_application::prompt_rendering::RenderError::Diagnostic(
                    RenderDiagnostic::InvalidBinding
                )
            )
        )
    ));
}

#[test]
fn session_http_runtime_model_mapping_is_rendered_and_verified_against_canonical_pin() {
    let fixture = fixture();
    let binding = RuntimeModelBinding {
        canonical: fixture.prepared.capability().data().model.clone(),
        runtime_id: "operator/crm-model:snapshot-7".into(),
    };
    let tools = BTreeMap::from([(
        fixture.definition.binding.id.clone(),
        fixture.definition.clone(),
    )]);
    for kind in [
        AdapterKind::ChatCompletionsV1,
        AdapterKind::ChatCompletionsLegacyV1,
    ] {
        let renderer = SessionToolHttpRenderer::for_runtime(
            kind,
            fixture.prepared.clone(),
            fixture.session.invocation().prompt().handoff().clone(),
            tools.clone(),
            binding.clone(),
        )
        .unwrap();
        let profile = TokenizerProfile {
            schema_version: SchemaVersion::V1,
            model: binding.canonical.clone(),
            generation_profile: fixture
                .prepared
                .invocation()
                .data()
                .generation
                .content_digest(),
            tokenizer: id("connector-session-test-counter"),
            tokenizer_version: id("1"),
            renderer: renderer.identity().0,
            template_version: renderer.identity().1,
        };
        assert_eq!(profile.template_version.as_str(), "2");
        let rendered = render_invocation_ir(
            fixture.session.invocation(),
            &fixture.host,
            profile,
            &renderer,
            &Counter,
        )
        .unwrap();
        let body: Value = serde_json::from_str(rendered.rendered().payload()).unwrap();
        assert_eq!(body["model"], binding.runtime_id);
        assert_eq!(body["tools"][0]["function"]["name"], "crm.read_record");
        assert_eq!(
            rendered.rendered().accounting().count.profile.model,
            binding.canonical
        );
    }
    let response = |model: &str| {
        json!({
            "model":model,
            "choices":[{"finish_reason":"tool_calls","message":{
                "content":"found","refusal":null,
                "tool_calls":[{"type":"function","id":"call-1","function":{
                    "name":"crm.read_record","arguments":"{\"record_id\":\"42\"}"}}]
            }}],
            "usage":{"prompt_tokens":100,"completion_tokens":32}
        })
        .to_string()
    };
    let (endpoint, server) = serve_http_responses(vec![
        response(&binding.runtime_id),
        response(&fixture.model_id),
    ]);
    let output =
        invoke_http_adapter_with_runtime(&fixture, endpoint.clone(), None, Some(binding.clone()))
            .unwrap();
    assert_eq!(output.output().data().tools.len(), 1);
    assert!(
        invoke_http_adapter_with_runtime(&fixture, endpoint, None, Some(binding),).is_err(),
        "a response naming the canonical ID instead of the configured runtime snapshot must fail"
    );
    server.join().unwrap();
}

#[test]
fn session_http_runtime_renderer_rejects_changed_canonical_pin_and_invalid_wire_name() {
    let fixture = fixture();
    let tools = BTreeMap::from([(
        fixture.definition.binding.id.clone(),
        fixture.definition.clone(),
    )]);
    let mut changed = fixture.prepared.capability().data().model.clone();
    changed.digest = ContentDigest::new("e".repeat(64)).unwrap();
    for binding in [
        RuntimeModelBinding {
            canonical: changed,
            runtime_id: "operator/model:snapshot".into(),
        },
        RuntimeModelBinding {
            canonical: fixture.prepared.capability().data().model.clone(),
            runtime_id: "invalid model name".into(),
        },
    ] {
        assert!(matches!(
            SessionToolHttpRenderer::for_runtime(
                AdapterKind::ChatCompletionsV1,
                fixture.prepared.clone(),
                fixture.session.invocation().prompt().handoff().clone(),
                tools.clone(),
                binding,
            ),
            Err(gateway_daemon::model_adapters::AdapterDiagnostic::InvalidBinding)
        ));
    }
}

#[test]
fn session_http_runtime_mapping_drift_is_rejected_before_any_connection() {
    let fixture = fixture();
    let binding = RuntimeModelBinding {
        canonical: fixture.prepared.capability().data().model.clone(),
        runtime_id: "operator/model:snapshot-7".into(),
    };
    let renderer = SessionToolHttpRenderer::for_runtime(
        AdapterKind::ChatCompletionsV1,
        fixture.prepared.clone(),
        fixture.session.invocation().prompt().handoff().clone(),
        BTreeMap::from([(
            fixture.definition.binding.id.clone(),
            fixture.definition.clone(),
        )]),
        binding.clone(),
    )
    .unwrap();
    let profile = TokenizerProfile {
        schema_version: SchemaVersion::V1,
        model: binding.canonical.clone(),
        generation_profile: fixture
            .prepared
            .invocation()
            .data()
            .generation
            .content_digest(),
        tokenizer: id("connector-session-test-counter"),
        tokenizer_version: id("1"),
        renderer: renderer.identity().0,
        template_version: renderer.identity().1,
    };
    let rendered = render_invocation_ir(
        fixture.session.invocation(),
        &fixture.host,
        profile,
        &renderer,
        &Counter,
    )
    .unwrap();
    let accounting = rendered.rendered().accounting();
    let attempt = AttemptBinding {
        schema_version: SchemaVersion::V1,
        attempt: 1,
        invocation: fixture.session.invocation().identity(),
        profile: accounting.count.profile.clone(),
        payload_digest: accounting.count.payload_digest.clone(),
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let adapter = SessionToolHttpAdapter::new(
        AdapterKind::ChatCompletionsV1,
        format!("http://{}", listener.local_addr().unwrap()),
        None,
        Some(RuntimeModelBinding {
            runtime_id: "operator/model:snapshot-8".into(),
            ..binding
        }),
    )
    .unwrap();
    let context = RequestContext::new(
        "runtime-drift".into(),
        Instant::now() + Duration::from_secs(4),
    );
    assert_eq!(
        adapter.complete(BoundedInvocationRequest {
            ir: fixture.session.invocation(),
            rendered: &rendered,
            binding: &attempt,
            context: &context,
            max_output_bytes: 100_000,
            reserved_cost_microunits: 10,
        }),
        Err(ProviderToolFailure::Hard)
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
}

struct ExactCounter;
impl TokenizerPort for ExactCounter {
    fn count(&self, request: &TokenCountRequest<'_>) -> Result<TokenCountResult, RenderDiagnostic> {
        let mut counted = Counter.count(request)?;
        counted.mode = CountMode::Exact;
        Ok(counted)
    }
}

fn model_slice() -> SessionResourceAmounts {
    SessionResourceAmounts {
        model_attempts: 2,
        input_tokens: 100_000,
        output_tokens: 10_000,
        output_bytes: 100_000,
        cost_microunits: 100,
        ..Default::default()
    }
}
fn model_plan(amounts: SessionResourceAmounts) -> SessionResourcePlan {
    SessionResourcePlan {
        preparation: amounts,
        approved_resume: Default::default(),
        execution: Default::default(),
        after_snapshot_recovery: Default::default(),
        goal_verification: Default::default(),
    }
}
fn reserved_model_limits() -> InvocationRunLimits {
    InvocationRunLimits {
        schema_version: SchemaVersion::V1,
        max_attempts: 2,
        timeout_ms: 4_000,
        invocation_input_tokens: 50_000,
        invocation_output_tokens: 2_000,
        run_input_tokens: 100_000,
        run_output_tokens: 10_000,
        cost_microunits: 100,
        output_bytes: 100_000,
        reconciliation_tolerance: 100_000,
    }
}
fn session_profile(session: &SessionToolInvocation) -> TokenizerProfile {
    TokenizerProfile {
        schema_version: SchemaVersion::V1,
        model: session
            .invocation()
            .prepared()
            .capability()
            .data()
            .model
            .clone(),
        generation_profile: session
            .invocation()
            .prepared()
            .invocation()
            .data()
            .generation
            .content_digest(),
        tokenizer: id("connector-session-test-counter"),
        tokenizer_version: id("1"),
        renderer: Renderer.identity().0,
        template_version: Renderer.identity().1,
    }
}

struct RunProvider {
    attempts: Arc<Mutex<Vec<AttemptBinding>>>,
    malformed_first: bool,
    reported: bool,
}
impl SessionToolProviderPort for RunProvider {
    fn complete(
        &self,
        request: BoundedInvocationRequest<'_>,
    ) -> Result<ProviderToolCompletion, ProviderToolFailure> {
        let mut attempts = self.attempts.lock().unwrap();
        attempts.push(request.binding.clone());
        Ok(ProviderToolCompletion {
            model_id: request
                .ir
                .prepared()
                .capability()
                .data()
                .model
                .id
                .as_str()
                .into(),
            finish: ProviderFinish::Complete,
            result: if self.malformed_first && attempts.len() == 1 {
                json!({"wrong":"result shape"})
            } else {
                json!("record inspected")
            },
            calls: vec![ProviderToolCall {
                name: "crm.read_record".into(),
                arguments: json!({"record_id":"42"}),
            }],
            usage: self.reported.then_some(ProviderTokenUsage {
                input_tokens: 1,
                output_tokens: 1,
            }),
        })
    }
}

fn alternate_session(session: &SessionToolInvocation) -> SessionToolInvocation {
    let handoff = session.invocation().prompt().handoff().clone();
    let mut data = session.invocation().prepared().invocation().data().clone();
    let mut capability = session.invocation().prepared().capability().data().clone();
    let mut provider = session.invocation().prepared().provider().data().clone();
    capability.identity = pin("session-fallback-capability");
    capability.model = pin("session-fallback-model");
    capability.provider = pin("session-fallback-provider");
    provider.identity = capability.provider.clone();
    data.capability = capability.identity.clone();
    let model = ModelInvocation::new(
        data.clone(),
        ModelInvocationBasis {
            task: handoff.task(),
            output: &data.output,
            compiled_context: &data.compiled_context,
            permitted_capabilities: &data.permitted_capabilities,
            sensitivity: data.sensitivity,
            permitted_localities: &data.permitted_localities,
            authority: &data.authority,
        },
    )
    .unwrap();
    let mut runtime = support::model::runtime();
    runtime.provider = provider.identity.clone();
    let prepared = gateway_application::model_invocation::prepare_model_invocation(
        model,
        ModelCapability::try_from(capability).unwrap(),
        ModelProvider::try_from(provider).unwrap(),
        &runtime,
    )
    .unwrap();
    SessionToolInvocation::admit(
        handoff,
        prepared,
        session.invocation().prompt().tools().clone(),
        &Host,
    )
    .unwrap()
}

#[test]
fn reserved_model_run_keeps_attempts_and_unknown_usage_across_retry_and_provider_fallback() {
    let fixture = fixture();
    let fallback = alternate_session(&fixture.session);
    let attempts = Arc::new(Mutex::new(Vec::new()));
    let provider = SessionToolProviderAdapter::new(RunProvider {
        attempts: attempts.clone(),
        malformed_first: true,
        reported: false,
    });
    let scope = fixture
        .session
        .invocation()
        .prepared()
        .invocation()
        .data()
        .task
        .scope
        .clone();
    let retained = session_resource_allocation::with_preparation_resources(
        scope,
        model_plan(model_slice()),
        |resources, _, authority| {
            authority.allow_invocation(fixture.session.invocation());
            let allocation = resources.allocate(model_slice()).unwrap();
            assert!(resources.remaining().is_zero());
            let reservation = allocation.reservation_id().clone();
            let mut run = SessionToolRun::new(
                fixture.session.invocation().clone(),
                allocation,
                reserved_model_limits(),
                RequestContext::new(
                    "reserved-model".into(),
                    Instant::now() + Duration::from_secs(4),
                ),
                &id("test-credit"),
            )
            .unwrap();
            assert_eq!(run.reservation_id(), &reservation);
            let first = fixture.session.invoke_in(
                &mut run,
                SessionToolAttemptExecution {
                    host: &fixture.host,
                    authority,
                    profile: session_profile(&fixture.session),
                    renderer: &Renderer,
                    tokenizer: &ExactCounter,
                    reserved_cost_microunits: 10,
                    cost_unit: &id("test-credit"),
                    transport: &provider,
                    decision: gateway_application::closed_loop::LoopDecision::Continue,
                },
            );
            assert!(matches!(
                first,
                Err(SessionToolInvocationError::Malformed(_))
                    | Err(SessionToolInvocationError::Rejected(_))
            ));
            let verified = fallback
                .invoke_in(
                    &mut run,
                    SessionToolAttemptExecution {
                        host: &fixture.host,
                        authority,
                        profile: session_profile(&fallback),
                        renderer: &Renderer,
                        tokenizer: &ExactCounter,
                        reserved_cost_microunits: 10,
                        cost_unit: &id("test-credit"),
                        transport: &provider,
                        decision: gateway_application::closed_loop::LoopDecision::Replan,
                    },
                )
                .unwrap();
            assert_eq!(
                verified.record().admitted_invocation,
                fallback.invocation().identity()
            );
            assert_eq!(run.usage().attempts, 2);
            assert_eq!(
                run.records()
                    .iter()
                    .map(|r| r.binding.attempt)
                    .collect::<Vec<_>>(),
                [1, 2]
            );
            assert_eq!(run.conservative_usage().cost_microunits, 20);
            assert_eq!(
                run.conservative_usage().output_tokens,
                2 * u64::from(fixture.prepared.effective_generation().max_output_tokens)
            );
            assert!(matches!(
                fallback.invoke_in(
                    &mut run,
                    SessionToolAttemptExecution {
                        host: &fixture.host,
                        authority,
                        profile: session_profile(&fallback),
                        renderer: &Renderer,
                        tokenizer: &ExactCounter,
                        reserved_cost_microunits: 10,
                        cost_unit: &id("test-credit"),
                        transport: &provider,
                        decision: gateway_application::closed_loop::LoopDecision::Continue,
                    }
                ),
                Err(SessionToolInvocationError::Run(
                    InvocationRunDiagnostic::AttemptBudgetExceeded
                ))
            ));
            assert_eq!(attempts.lock().unwrap().len(), 2);
        },
    );
    assert_eq!(*retained.resource_ledger.unwrap().used(), model_slice());
}

#[test]
fn reserved_model_run_rejects_all_overlarge_caps_currency_scope_and_nonmodel_slices() {
    for case in [
        "attempts",
        "input",
        "invocation-input",
        "output",
        "invocation-output",
        "cost",
        "bytes",
        "currency",
        "scope",
        "nonmodel",
        "connector-calls",
    ] {
        let fixture = fixture();
        let mut amounts = model_slice();
        if case == "nonmodel" {
            amounts.retrieval_rounds = 1;
        }
        if case == "connector-calls" {
            amounts.connector_calls = 1;
        }
        let scope = if case == "scope" {
            ContextScopeId::new("foreign-scope").unwrap()
        } else {
            fixture
                .session
                .invocation()
                .prepared()
                .invocation()
                .data()
                .task
                .scope
                .clone()
        };
        session_resource_allocation::with_preparation_resources(
            scope,
            model_plan(amounts),
            |resources, _, _| {
                let allocation = resources.allocate(amounts).unwrap();
                let mut limits = reserved_model_limits();
                match case {
                    "attempts" => limits.max_attempts = 3,
                    "input" => limits.run_input_tokens = amounts.input_tokens + 1,
                    "invocation-input" => limits.invocation_input_tokens = amounts.input_tokens + 1,
                    "output" => limits.run_output_tokens = amounts.output_tokens + 1,
                    "invocation-output" => {
                        limits.invocation_output_tokens = amounts.output_tokens + 1
                    }
                    "cost" => limits.cost_microunits = amounts.cost_microunits + 1,
                    "bytes" => limits.output_bytes = amounts.output_bytes + 1,
                    _ => {}
                }
                assert!(
                    SessionToolRun::new(
                        fixture.session.invocation().clone(),
                        allocation,
                        limits,
                        RequestContext::new(
                            "reserved-invalid".into(),
                            Instant::now() + Duration::from_secs(4)
                        ),
                        &id(if case == "currency" {
                            "other-credit"
                        } else {
                            "test-credit"
                        })
                    )
                    .is_err(),
                    "{case}"
                );
            },
        );
    }
}

#[test]
fn reserved_model_run_does_not_refill_quoted_cost_or_output_from_smaller_provider_reports() {
    for cost_limited in [true, false] {
        let fixture = fixture();
        let mut amounts = model_slice();
        if cost_limited {
            amounts.cost_microunits = 10;
        } else {
            amounts.output_tokens =
                u64::from(fixture.prepared.effective_generation().max_output_tokens);
        }
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let provider = LowerCostReport(SessionToolProviderAdapter::new(RunProvider {
            attempts: attempts.clone(),
            malformed_first: false,
            reported: true,
        }));
        session_resource_allocation::with_preparation_resources(
            fixture
                .session
                .invocation()
                .prepared()
                .invocation()
                .data()
                .task
                .scope
                .clone(),
            model_plan(amounts),
            |resources, _, authority| {
                authority.allow_invocation(fixture.session.invocation());
                let mut limits = reserved_model_limits();
                limits.cost_microunits = amounts.cost_microunits;
                limits.run_output_tokens = amounts.output_tokens;
                limits.invocation_output_tokens = amounts.output_tokens;
                let mut run = SessionToolRun::new(
                    fixture.session.invocation().clone(),
                    resources.allocate(amounts).unwrap(),
                    limits,
                    RequestContext::new(
                        "reserved-no-refund".into(),
                        Instant::now() + Duration::from_secs(4),
                    ),
                    &id("test-credit"),
                )
                .unwrap();
                fixture
                    .session
                    .invoke_in(
                        &mut run,
                        SessionToolAttemptExecution {
                            host: &fixture.host,
                            authority,
                            profile: session_profile(&fixture.session),
                            renderer: &Renderer,
                            tokenizer: &ExactCounter,
                            reserved_cost_microunits: 10,
                            cost_unit: &id("test-credit"),
                            transport: &provider,
                            decision: gateway_application::closed_loop::LoopDecision::Continue,
                        },
                    )
                    .unwrap();
                assert_eq!(run.usage().cost_microunits, 0);
                assert_eq!(run.usage().output_tokens, 1);
                assert_eq!(run.conservative_usage().cost_microunits, 10);
                let expected = if cost_limited {
                    InvocationRunDiagnostic::CostBudgetExceeded
                } else {
                    InvocationRunDiagnostic::OutputTokenBudgetExceeded
                };
                assert!(
                    matches!(fixture.session.invoke_in(&mut run, SessionToolAttemptExecution {
                host:&fixture.host, authority, profile:session_profile(&fixture.session), renderer:&Renderer, tokenizer:&ExactCounter,
                reserved_cost_microunits:10, cost_unit:&id("test-credit"), transport:&provider,
                decision:gateway_application::closed_loop::LoopDecision::Continue,
            }), Err(SessionToolInvocationError::Run(error)) if error == expected)
                );
                assert_eq!(attempts.lock().unwrap().len(), 1);
            },
        );
    }
}

struct LowerCostReport<T>(T);
impl<T: BoundedInvocationPort> BoundedInvocationPort for LowerCostReport<T> {
    fn invoke(
        &self,
        request: BoundedInvocationRequest<'_>,
    ) -> gateway_application::invocation_resilience::InvocationAttemptResponse {
        let mut result = self.0.invoke(request);
        if let Some(usage) = &mut result.usage {
            usage.cost_microunits = 0;
        }
        result
    }
}

#[test]
fn reserved_model_run_rejects_estimates_changed_currency_and_lifecycle_before_provider() {
    for case in [
        "estimated",
        "currency",
        "paused",
        "cancelled",
        "deadline",
        "revoked",
        "expired-claim",
        "new-fence",
        "session-cancel",
    ] {
        let fixture = fixture();
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let provider = SessionToolProviderAdapter::new(RunProvider {
            attempts: attempts.clone(),
            malformed_first: false,
            reported: false,
        });
        session_resource_allocation::with_preparation_resources(
            fixture
                .session
                .invocation()
                .prepared()
                .invocation()
                .data()
                .task
                .scope
                .clone(),
            model_plan(model_slice()),
            |resources, _, authority| {
                authority.allow_invocation(fixture.session.invocation());
                let context = RequestContext::new(
                    "reserved-preflight".into(),
                    if case == "deadline" {
                        Instant::now()
                    } else {
                        Instant::now() + Duration::from_secs(4)
                    },
                );
                let mut run = SessionToolRun::new(
                    fixture.session.invocation().clone(),
                    resources.allocate(model_slice()).unwrap(),
                    reserved_model_limits(),
                    context.clone(),
                    &id("test-credit"),
                )
                .unwrap();
                match case {
                    "cancelled" => context.cancel(),
                    "revoked" => authority.revoke(),
                    "expired-claim" => authority.expire(),
                    "new-fence" => authority.advance_fence(),
                    "session-cancel" => authority.cancel(),
                    _ => {}
                }
                let error = fixture
                    .session
                    .invoke_in(
                        &mut run,
                        SessionToolAttemptExecution {
                            host: &fixture.host,
                            authority,
                            profile: session_profile(&fixture.session),
                            renderer: &Renderer,
                            tokenizer: if case == "estimated" {
                                &Counter
                            } else {
                                &ExactCounter
                            },
                            reserved_cost_microunits: 10,
                            cost_unit: &id(if case == "currency" {
                                "different-credit"
                            } else {
                                "test-credit"
                            }),
                            transport: &provider,
                            decision: if case == "paused" {
                                gateway_application::closed_loop::LoopDecision::Stopped
                            } else {
                                gateway_application::closed_loop::LoopDecision::Continue
                            },
                        },
                    )
                    .unwrap_err();
                assert!(
                    matches!(
                        error,
                        SessionToolInvocationError::Run(_)
                            | SessionToolInvocationError::Resource(_)
                    ),
                    "{case}: {error:?}"
                );
                assert_eq!(attempts.lock().unwrap().len(), 0, "{case}");
                assert_eq!(run.usage().attempts, 0);
                assert!(run.conservative_usage().is_zero());
            },
        );
    }
}

struct RevokeDuringRendering<'a>(&'a session_resource_allocation::PreparationAuthority);
impl PromptRendererPort for RevokeDuringRendering<'_> {
    fn identity(&self) -> (ReferenceId, ReferenceId) {
        Renderer.identity()
    }
    fn render(&self, input: &RenderInput) -> Result<Value, RenderDiagnostic> {
        let value = Renderer.render(input)?;
        self.0.revoke();
        Ok(value)
    }
}
#[test]
fn reserved_model_run_rechecks_current_claim_after_rendering_before_transport() {
    let fixture = fixture();
    let attempts = Arc::new(Mutex::new(Vec::new()));
    let provider = SessionToolProviderAdapter::new(RunProvider {
        attempts: attempts.clone(),
        malformed_first: false,
        reported: false,
    });
    session_resource_allocation::with_preparation_resources(
        fixture
            .session
            .invocation()
            .prepared()
            .invocation()
            .data()
            .task
            .scope
            .clone(),
        model_plan(model_slice()),
        |resources, _, authority| {
            authority.allow_invocation(fixture.session.invocation());
            let mut run = SessionToolRun::new(
                fixture.session.invocation().clone(),
                resources.allocate(model_slice()).unwrap(),
                reserved_model_limits(),
                RequestContext::new(
                    "claim-render-race".into(),
                    Instant::now() + Duration::from_secs(4),
                ),
                &id("test-credit"),
            )
            .unwrap();
            let renderer = RevokeDuringRendering(authority);
            assert!(matches!(
                fixture.session.invoke_in(
                    &mut run,
                    SessionToolAttemptExecution {
                        host: &fixture.host,
                        authority,
                        profile: session_profile(&fixture.session),
                        renderer: &renderer,
                        tokenizer: &ExactCounter,
                        reserved_cost_microunits: 10,
                        cost_unit: &id("test-credit"),
                        transport: &provider,
                        decision: gateway_application::closed_loop::LoopDecision::Continue,
                    }
                ),
                Err(SessionToolInvocationError::Run(
                    InvocationRunDiagnostic::AuthorityMismatch
                ))
            ));
            assert!(attempts.lock().unwrap().is_empty());
        },
    );
}

struct QualifiedBoundCounter;
impl TokenizerPort for QualifiedBoundCounter {
    fn count(&self, request: &TokenCountRequest<'_>) -> Result<TokenCountResult, RenderDiagnostic> {
        let mut counted = Counter.count(request)?;
        counted.usage.framing_tokens += 256;
        counted.provenance = ResolvedReference {
            id: id("utf8-wire-byte-upper-bound"),
            scope: request.profile.model.scope.clone(),
            contract: id("utf8-wire-byte-upper-bound"),
            contract_version: SchemaVersion::V1,
            revision: id("1"),
            digest: context_content_digest(b"UTF8 byte fallback plus 256 chat framing tokens/v1"),
        };
        Ok(counted)
    }
}
#[test]
fn reserved_model_run_accepts_only_current_qualified_conservative_estimates() {
    let fixture = fixture();
    let attempts = Arc::new(Mutex::new(Vec::new()));
    let provider = SessionToolProviderAdapter::new(RunProvider {
        attempts: attempts.clone(),
        malformed_first: false,
        reported: false,
    });
    session_resource_allocation::with_preparation_resources(
        fixture
            .session
            .invocation()
            .prepared()
            .invocation()
            .data()
            .task
            .scope
            .clone(),
        model_plan(model_slice()),
        |resources, _, authority| {
            authority.allow_invocation(fixture.session.invocation());
            let profile = session_profile(&fixture.session);
            authority.allow_qualified_bounds(profile.clone());
            let mut run = SessionToolRun::new(
                fixture.session.invocation().clone(),
                resources.allocate(model_slice()).unwrap(),
                reserved_model_limits(),
                RequestContext::new(
                    "qualified-bound".into(),
                    Instant::now() + Duration::from_secs(4),
                ),
                &id("test-credit"),
            )
            .unwrap();
            assert!(matches!(
                fixture.session.invoke_in(
                    &mut run,
                    SessionToolAttemptExecution {
                        host: &fixture.host,
                        authority,
                        profile: profile.clone(),
                        renderer: &Renderer,
                        tokenizer: &Counter,
                        reserved_cost_microunits: 10,
                        cost_unit: &id("test-credit"),
                        transport: &provider,
                        decision: gateway_application::closed_loop::LoopDecision::Continue,
                    }
                ),
                Err(SessionToolInvocationError::Resource(_))
            ));
            assert_eq!(run.usage().attempts, 0);
            assert!(attempts.lock().unwrap().is_empty());
            let verified = fixture
                .session
                .invoke_in(
                    &mut run,
                    SessionToolAttemptExecution {
                        host: &fixture.host,
                        authority,
                        profile,
                        renderer: &Renderer,
                        tokenizer: &QualifiedBoundCounter,
                        reserved_cost_microunits: 10,
                        cost_unit: &id("test-credit"),
                        transport: &provider,
                        decision: gateway_application::closed_loop::LoopDecision::Continue,
                    },
                )
                .unwrap();
            assert_eq!(
                verified.record().admitted_invocation,
                fixture.session.invocation().identity()
            );
            assert_eq!(
                run.records()[0].local_provenance,
                UsageProvenance::LocalEstimated
            );
            assert_eq!(attempts.lock().unwrap().len(), 1);
        },
    );
}

struct ClaimRevokingProvider<'a> {
    authority: &'a session_resource_allocation::PreparationAuthority,
    inner: RunProvider,
}
impl SessionToolProviderPort for ClaimRevokingProvider<'_> {
    fn complete(
        &self,
        request: BoundedInvocationRequest<'_>,
    ) -> Result<ProviderToolCompletion, ProviderToolFailure> {
        let output = self.inner.complete(request)?;
        self.authority.revoke();
        Ok(output)
    }
}
#[test]
fn reserved_model_run_retains_usage_and_rejects_result_if_claim_changes_in_flight() {
    let fixture = fixture();
    let attempts = Arc::new(Mutex::new(Vec::new()));
    session_resource_allocation::with_preparation_resources(
        fixture
            .session
            .invocation()
            .prepared()
            .invocation()
            .data()
            .task
            .scope
            .clone(),
        model_plan(model_slice()),
        |resources, _, authority| {
            authority.allow_invocation(fixture.session.invocation());
            let provider = SessionToolProviderAdapter::new(ClaimRevokingProvider {
                authority,
                inner: RunProvider {
                    attempts: attempts.clone(),
                    malformed_first: false,
                    reported: false,
                },
            });
            let mut run = SessionToolRun::new(
                fixture.session.invocation().clone(),
                resources.allocate(model_slice()).unwrap(),
                reserved_model_limits(),
                RequestContext::new(
                    "inflight-claim".into(),
                    Instant::now() + Duration::from_secs(4),
                ),
                &id("test-credit"),
            )
            .unwrap();
            assert!(matches!(
                fixture.session.invoke_in(
                    &mut run,
                    SessionToolAttemptExecution {
                        host: &fixture.host,
                        authority,
                        profile: session_profile(&fixture.session),
                        renderer: &Renderer,
                        tokenizer: &ExactCounter,
                        reserved_cost_microunits: 10,
                        cost_unit: &id("test-credit"),
                        transport: &provider,
                        decision: gateway_application::closed_loop::LoopDecision::Continue,
                    }
                ),
                Err(SessionToolInvocationError::Resource(_))
            ));
            assert_eq!(run.usage().attempts, 1);
            assert_eq!(run.conservative_usage().cost_microunits, 10);
            assert_eq!(attempts.lock().unwrap().len(), 1);
        },
    );
}

struct TerminalModelTransport {
    calls: Arc<Mutex<usize>>,
    failure: InvocationAttemptFailure,
}
impl BoundedInvocationPort for TerminalModelTransport {
    fn invoke(
        &self,
        _: BoundedInvocationRequest<'_>,
    ) -> gateway_application::invocation_resilience::InvocationAttemptResponse {
        *self.calls.lock().unwrap() += 1;
        gateway_application::invocation_resilience::InvocationAttemptResponse {
            output: Err(self.failure),
            usage: None,
        }
    }
}
#[test]
fn reserved_model_run_preserves_terminal_uncertainty_or_hard_failure_without_retry_or_refill() {
    for (failure, expected) in [
        (
            InvocationAttemptFailure::UncertainSideEffect,
            InvocationRunDiagnostic::UncertainSideEffect,
        ),
        (
            InvocationAttemptFailure::Hard,
            InvocationRunDiagnostic::HardFailure,
        ),
    ] {
        let fixture = fixture();
        let mut amounts = model_slice();
        amounts.cost_microunits = 10;
        let calls = Arc::new(Mutex::new(0));
        let provider = TerminalModelTransport {
            calls: calls.clone(),
            failure,
        };
        session_resource_allocation::with_preparation_resources(
            fixture
                .session
                .invocation()
                .prepared()
                .invocation()
                .data()
                .task
                .scope
                .clone(),
            model_plan(amounts),
            |resources, _, authority| {
                authority.allow_invocation(fixture.session.invocation());
                let mut limits = reserved_model_limits();
                limits.cost_microunits = 10;
                let mut run = SessionToolRun::new(
                    fixture.session.invocation().clone(),
                    resources.allocate(amounts).unwrap(),
                    limits,
                    RequestContext::new(
                        "terminal-model".into(),
                        Instant::now() + Duration::from_secs(4),
                    ),
                    &id("test-credit"),
                )
                .unwrap();
                for _ in 0..2 {
                    assert!(
                        matches!(fixture.session.invoke_in(&mut run,SessionToolAttemptExecution {
                    host:&fixture.host,authority,profile:session_profile(&fixture.session),renderer:&Renderer,tokenizer:&ExactCounter,
                    reserved_cost_microunits:10,cost_unit:&id("test-credit"),transport:&provider,
                    decision:gateway_application::closed_loop::LoopDecision::Continue,
                }),Err(SessionToolInvocationError::Run(error)) if error==expected)
                    );
                }
                assert_eq!(*calls.lock().unwrap(), 1);
                assert_eq!(run.usage().attempts, 1);
                assert_eq!(run.conservative_usage().cost_microunits, 10);
            },
        );
    }
}

struct VerificationControlHost<'a> {
    authority: &'a session_resource_allocation::PreparationAuthority,
    context: &'a RequestContext,
    action: &'a str,
}
impl SemanticReferenceValidator for VerificationControlHost<'_> {
    fn validate(
        &self,
        task: &SemanticTaskData,
        binding: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        Host.validate(task, binding)
    }
}
impl InvocationAdmissionPort for VerificationControlHost<'_> {
    fn validate_current(&self, ir: &InvocationIr) -> Result<(), InvocationIrError> {
        Host.validate_current(ir)
    }
}
impl OutputVerificationPort for VerificationControlHost<'_> {
    fn resolve_reference(
        &self,
        ir: &InvocationIr,
        reference: &OutputReference,
    ) -> Result<(), VerificationError> {
        Host.resolve_reference(ir, reference)
    }
    fn evaluate(
        &self,
        ir: &InvocationIr,
        output: &ParsedOutput,
    ) -> Result<ConstraintReport, VerificationError> {
        let report = Host.evaluate(ir, output)?;
        match self.action {
            "revoke" => self.authority.revoke(),
            "cancel" => self.context.cancel(),
            _ => thread::sleep(
                self.context
                    .deadline
                    .saturating_duration_since(Instant::now())
                    + Duration::from_millis(5),
            ),
        }
        Ok(report)
    }
}
#[test]
fn reserved_model_run_rechecks_claim_cancellation_and_deadline_after_verification_callbacks() {
    for action in ["revoke", "cancel", "deadline"] {
        let fixture = fixture();
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let provider = SessionToolProviderAdapter::new(RunProvider {
            attempts: attempts.clone(),
            malformed_first: false,
            reported: false,
        });
        session_resource_allocation::with_preparation_resources(
            fixture
                .session
                .invocation()
                .prepared()
                .invocation()
                .data()
                .task
                .scope
                .clone(),
            model_plan(model_slice()),
            |resources, _, authority| {
                authority.allow_invocation(fixture.session.invocation());
                let context = RequestContext::new(
                    "verification-race".into(),
                    Instant::now() + Duration::from_secs(1),
                );
                let mut run = SessionToolRun::new(
                    fixture.session.invocation().clone(),
                    resources.allocate(model_slice()).unwrap(),
                    reserved_model_limits(),
                    context.clone(),
                    &id("test-credit"),
                )
                .unwrap();
                let host = VerificationControlHost {
                    authority,
                    context: &context,
                    action,
                };
                let result = fixture.session.invoke_in(
                    &mut run,
                    SessionToolAttemptExecution {
                        host: &host,
                        authority,
                        profile: session_profile(&fixture.session),
                        renderer: &Renderer,
                        tokenizer: &ExactCounter,
                        reserved_cost_microunits: 10,
                        cost_unit: &id("test-credit"),
                        transport: &provider,
                        decision: gateway_application::closed_loop::LoopDecision::Continue,
                    },
                );
                match action {
                    "revoke" => assert!(matches!(
                        result,
                        Err(SessionToolInvocationError::Resource(_))
                    )),
                    "cancel" => assert!(matches!(
                        result,
                        Err(SessionToolInvocationError::Run(
                            InvocationRunDiagnostic::Cancelled
                        ))
                    )),
                    _ => assert!(matches!(
                        result,
                        Err(SessionToolInvocationError::Run(
                            InvocationRunDiagnostic::DeadlineExceeded
                        ))
                    )),
                }
                assert_eq!(attempts.lock().unwrap().len(), 1);
                assert_eq!(run.conservative_usage().model_attempts, 1);
                assert_eq!(run.conservative_usage().cost_microunits, 10);
            },
        );
    }
}

struct MarkQualifiedAuthority<'a> {
    inner: &'a session_resource_allocation::PreparationAuthority,
    qualified: std::sync::atomic::AtomicBool,
}
impl gateway_application::sessions::SessionResourceAuthorityPort for MarkQualifiedAuthority<'_> {
    fn validate_invocation_allocation(
        &self,
        allocation: &gateway_application::sessions::SessionResourceAllocation,
        ir: &InvocationIr,
    ) -> Result<(), gateway_application::sessions::SessionError> {
        self.inner.validate_invocation_allocation(allocation, ir)
    }
    fn validate_allocation(
        &self,
        allocation: &gateway_application::sessions::SessionResourceAllocation,
    ) -> Result<(), gateway_application::sessions::SessionError> {
        self.inner.validate_allocation(allocation)
    }
    fn validate_token_bound(
        &self,
        allocation: &gateway_application::sessions::SessionResourceAllocation,
        count: &TokenCountResult,
        payload: &str,
    ) -> Result<(), gateway_application::sessions::SessionError> {
        self.inner
            .validate_token_bound(allocation, count, payload)?;
        self.qualified
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}
struct ChangeQualifiedProfileHost<'a> {
    authority: &'a MarkQualifiedAuthority<'a>,
    changed: TokenizerProfile,
    changes: std::sync::atomic::AtomicUsize,
}
impl SemanticReferenceValidator for ChangeQualifiedProfileHost<'_> {
    fn validate(
        &self,
        task: &SemanticTaskData,
        binding: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        Host.validate(task, binding)
    }
}
impl InvocationAdmissionPort for ChangeQualifiedProfileHost<'_> {
    fn validate_current(&self, ir: &InvocationIr) -> Result<(), InvocationIrError> {
        if self
            .authority
            .qualified
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            self.authority.inner.revoke_qualified_bounds();
            self.authority
                .inner
                .allow_qualified_bounds(self.changed.clone());
            self.changes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Host.validate_current(ir)
    }
}
impl OutputVerificationPort for ChangeQualifiedProfileHost<'_> {
    fn resolve_reference(
        &self,
        ir: &InvocationIr,
        reference: &OutputReference,
    ) -> Result<(), VerificationError> {
        Host.resolve_reference(ir, reference)
    }
    fn evaluate(
        &self,
        ir: &InvocationIr,
        output: &ParsedOutput,
    ) -> Result<ConstraintReport, VerificationError> {
        Host.evaluate(ir, output)
    }
}
#[test]
fn reserved_model_run_rechecks_bound_qualification_after_current_host_callbacks() {
    let fixture = fixture();
    let attempts = Arc::new(Mutex::new(Vec::new()));
    let provider = SessionToolProviderAdapter::new(RunProvider {
        attempts: attempts.clone(),
        malformed_first: false,
        reported: false,
    });
    session_resource_allocation::with_preparation_resources(
        fixture
            .session
            .invocation()
            .prepared()
            .invocation()
            .data()
            .task
            .scope
            .clone(),
        model_plan(model_slice()),
        |resources, _, authority| {
            authority.allow_invocation(fixture.session.invocation());
            let profile = session_profile(&fixture.session);
            authority.allow_qualified_bounds(profile.clone());
            let authority = MarkQualifiedAuthority {
                inner: authority,
                qualified: std::sync::atomic::AtomicBool::new(false),
            };
            let mut changed = profile.clone();
            changed.tokenizer_version = id("different-tokenizer-version");
            let host = ChangeQualifiedProfileHost {
                authority: &authority,
                changed,
                changes: std::sync::atomic::AtomicUsize::new(0),
            };
            let mut run = SessionToolRun::new(
                fixture.session.invocation().clone(),
                resources.allocate(model_slice()).unwrap(),
                reserved_model_limits(),
                RequestContext::new(
                    "qualified-source-race".into(),
                    Instant::now() + Duration::from_secs(4),
                ),
                &id("test-credit"),
            )
            .unwrap();
            assert!(matches!(
                fixture.session.invoke_in(
                    &mut run,
                    SessionToolAttemptExecution {
                        host: &host,
                        authority: &authority,
                        profile,
                        renderer: &Renderer,
                        tokenizer: &QualifiedBoundCounter,
                        reserved_cost_microunits: 10,
                        cost_unit: &id("test-credit"),
                        transport: &provider,
                        decision: gateway_application::closed_loop::LoopDecision::Continue,
                    }
                ),
                Err(SessionToolInvocationError::Run(
                    InvocationRunDiagnostic::AuthorityMismatch
                ))
            ));
            assert!(
                authority
                    .qualified
                    .load(std::sync::atomic::Ordering::SeqCst)
            );
            assert!(host.changes.load(std::sync::atomic::Ordering::SeqCst) >= 1);
            assert!(attempts.lock().unwrap().is_empty());
            assert_eq!(run.usage().attempts, 0);
        },
    );
}

fn foreign_authority_session(session: &SessionToolInvocation) -> SessionToolInvocation {
    let handoff = session.invocation().prompt().handoff().clone();
    let mut data = session.invocation().prepared().invocation().data().clone();
    data.authority = pin("independent-current-authority");
    let model = ModelInvocation::new(
        data.clone(),
        ModelInvocationBasis {
            task: handoff.task(),
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
        model,
        session.invocation().prepared().capability().clone(),
        session.invocation().prepared().provider().clone(),
        &support::model::runtime(),
    )
    .unwrap();
    SessionToolInvocation::admit(
        handoff,
        prepared,
        session.invocation().prompt().tools().clone(),
        &Host,
    )
    .unwrap()
}
struct AllocationOnlyAuthority<'a>(&'a session_resource_allocation::PreparationAuthority);
impl SessionResourceAuthorityPort for AllocationOnlyAuthority<'_> {
    fn validate_allocation(
        &self,
        allocation: &gateway_application::sessions::SessionResourceAllocation,
    ) -> Result<(), gateway_application::sessions::SessionError> {
        self.0.validate_allocation(allocation)
    }
}
#[test]
fn reserved_model_run_requires_joined_step_producer_pins_even_for_valid_same_scope_invocations() {
    for case in ["no-pin", "default-deny", "foreign-authority"] {
        let fixture = fixture();
        let foreign = foreign_authority_session(&fixture.session);
        assert_eq!(
            foreign
                .invocation()
                .prepared()
                .invocation()
                .data()
                .task
                .scope,
            fixture
                .session
                .invocation()
                .prepared()
                .invocation()
                .data()
                .task
                .scope
        );
        Host.validate_current(foreign.invocation()).unwrap();
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let provider = SessionToolProviderAdapter::new(RunProvider {
            attempts: attempts.clone(),
            malformed_first: false,
            reported: false,
        });
        session_resource_allocation::with_preparation_resources(
            fixture
                .session
                .invocation()
                .prepared()
                .invocation()
                .data()
                .task
                .scope
                .clone(),
            model_plan(model_slice()),
            |resources, _, authority| {
                if case != "no-pin" {
                    authority.allow_invocation(fixture.session.invocation());
                }
                let session = if case == "foreign-authority" {
                    &foreign
                } else {
                    &fixture.session
                };
                let mut run = SessionToolRun::new(
                    session.invocation().clone(),
                    resources.allocate(model_slice()).unwrap(),
                    reserved_model_limits(),
                    RequestContext::new(
                        "joined-model-allocation".into(),
                        Instant::now() + Duration::from_secs(4),
                    ),
                    &id("test-credit"),
                )
                .unwrap();
                let only_allocation = AllocationOnlyAuthority(authority);
                let current: &dyn SessionResourceAuthorityPort = if case == "default-deny" {
                    &only_allocation
                } else {
                    authority
                };
                assert!(
                    matches!(
                        session.invoke_in(
                            &mut run,
                            SessionToolAttemptExecution {
                                host: &fixture.host,
                                authority: current,
                                profile: session_profile(session),
                                renderer: &Renderer,
                                tokenizer: &ExactCounter,
                                reserved_cost_microunits: 10,
                                cost_unit: &id("test-credit"),
                                transport: &provider,
                                decision: gateway_application::closed_loop::LoopDecision::Continue,
                            }
                        ),
                        Err(SessionToolInvocationError::Resource(_))
                    ),
                    "{case}"
                );
                assert!(attempts.lock().unwrap().is_empty());
                assert_eq!(run.usage().attempts, 0);
            },
        );
    }
}

#[test]
fn reserved_model_run_unknown_transient_output_retains_full_byte_cap_without_retry_refill() {
    let fixture = fixture();
    let calls = Arc::new(Mutex::new(0));
    let provider = TerminalModelTransport {
        calls: calls.clone(),
        failure: InvocationAttemptFailure::Transient,
    };
    session_resource_allocation::with_preparation_resources(
        fixture
            .session
            .invocation()
            .prepared()
            .invocation()
            .data()
            .task
            .scope
            .clone(),
        model_plan(model_slice()),
        |resources, _, authority| {
            authority.allow_invocation(fixture.session.invocation());
            let mut run = SessionToolRun::new(
                fixture.session.invocation().clone(),
                resources.allocate(model_slice()).unwrap(),
                reserved_model_limits(),
                RequestContext::new(
                    "unknown-transient-model".into(),
                    Instant::now() + Duration::from_secs(4),
                ),
                &id("test-credit"),
            )
            .unwrap();
            assert!(matches!(
                fixture.session.invoke_in(
                    &mut run,
                    SessionToolAttemptExecution {
                        host: &fixture.host,
                        authority,
                        profile: session_profile(&fixture.session),
                        renderer: &Renderer,
                        tokenizer: &ExactCounter,
                        reserved_cost_microunits: 10,
                        cost_unit: &id("test-credit"),
                        transport: &provider,
                        decision: gateway_application::closed_loop::LoopDecision::Continue,
                    }
                ),
                Err(SessionToolInvocationError::ProviderUnavailable)
            ));
            assert_eq!(
                run.conservative_usage().output_bytes,
                model_slice().output_bytes
            );
            assert!(matches!(
                fixture.session.invoke_in(
                    &mut run,
                    SessionToolAttemptExecution {
                        host: &fixture.host,
                        authority,
                        profile: session_profile(&fixture.session),
                        renderer: &Renderer,
                        tokenizer: &ExactCounter,
                        reserved_cost_microunits: 10,
                        cost_unit: &id("test-credit"),
                        transport: &provider,
                        decision: gateway_application::closed_loop::LoopDecision::Replan,
                    }
                ),
                Err(SessionToolInvocationError::Run(
                    InvocationRunDiagnostic::OutputBudgetExceeded
                ))
            ));
            assert_eq!(run.usage().attempts, 1);
            assert_eq!(*calls.lock().unwrap(), 1);
        },
    );
}
