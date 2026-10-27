#[allow(dead_code)]
#[path = "../../../tests/fixtures/prompt-rendering-v1/support.rs"]
mod support;
use gateway_application::{
    codex::RequestContext, invocation_resilience::*, prompt_ir::*, prompt_rendering::*,
};
use gateway_daemon::model_adapters::*;
use gateway_domain::{
    invocation_resilience::*, model_invocation::*, prompt_ir::InvocationIrError,
    prompt_rendering::*, *,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};
use support::{examples::id, model::pin};

// Test-only store. Production admission belongs to the existing trusted host.
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
struct Counter;
impl TokenizerPort for Counter {
    fn count(&self, r: &TokenCountRequest<'_>) -> Result<TokenCountResult, RenderDiagnostic> {
        Ok(TokenCountResult {
            profile: r.profile.clone(),
            payload_digest: r.payload_digest.clone(),
            mode: CountMode::Estimated,
            provenance: pin("utf8-byte-upper-bound"),
            usage: TokenUsage {
                instruction_tokens: 0,
                task_tokens: 0,
                context_tokens: 0,
                evidence_tokens: 0,
                tool_schema_tokens: 0,
                framing_tokens: r.payload.len() as u64,
            },
        })
    }
}
fn fixture(
    kind: AdapterKind,
    edit: impl FnOnce(&mut ModelInvocationData, &mut ModelProviderData),
) -> (InvocationIr, RenderedInvocationIr, AttemptBinding) {
    fixture_with_model(kind, edit, None)
}
fn fixture_with_model(
    kind: AdapterKind,
    edit: impl FnOnce(&mut ModelInvocationData, &mut ModelProviderData),
    model: Option<(ResolvedReference<ReferenceId>, u32)>,
) -> (InvocationIr, RenderedInvocationIr, AttemptBinding) {
    let mut output = support::model::output();
    output.shape = gateway_domain::semantic_output::OutputType::Object {
        fields: BTreeMap::from([(
            "message".to_string(),
            gateway_domain::semantic_output::OutputField {
                required: true,
                value_type: gateway_domain::semantic_output::OutputType::String {},
            },
        )]),
    };
    let h = support::handoff_for_output(support::examples::task(), output.clone());
    let mut d = support::model::invocation_data();
    d.output = output.clone();
    d.task = serde_json::from_value(h.to_value()["bindings"]["task"].clone()).unwrap();
    d.compiled_context =
        serde_json::from_value(h.to_value()["bindings"]["context"].clone()).unwrap();
    let mut provider = support::model::provider_data();
    let mut g = d.generation.data().clone();
    if kind != AdapterKind::LocalServiceV1 {
        g.controls.remove(&GenerationControl::TopK);
        provider.controls.remove(&GenerationControl::TopK);
        provider.unexposed_defaults.remove(&GenerationControl::TopK);
    }
    d.generation = GenerationProfile::try_from(g).unwrap();
    edit(&mut d, &mut provider);
    let inv = ModelInvocation::new(
        d.clone(),
        ModelInvocationBasis {
            task: h.task(),
            output: &output,
            compiled_context: &d.compiled_context,
            permitted_capabilities: &d.permitted_capabilities,
            sensitivity: d.sensitivity,
            permitted_localities: &d.permitted_localities,
            authority: &d.authority,
        },
    )
    .unwrap();
    let mut c = support::model::capability_data();
    c.context_limit = 100_000;
    c.structured_output_modes.insert(StructuredOutputMode::Json);
    if let Some((model, limit)) = model {
        c.model = model;
        c.context_limit = limit;
    }
    let p = gateway_application::model_invocation::prepare_model_invocation(
        inv,
        ModelCapability::try_from(c).unwrap(),
        ModelProvider::try_from(provider).unwrap(),
        &support::model::runtime(),
    )
    .unwrap();
    let renderer = AdapterRenderer::new(kind, p.clone()).unwrap();
    let profile = TokenizerProfile {
        schema_version: SchemaVersion::V1,
        model: p.capability().data().model.clone(),
        generation_profile: d.generation.content_digest(),
        tokenizer: id("utf8-byte-upper-bound"),
        tokenizer_version: id("1"),
        renderer: renderer.identity().0,
        template_version: renderer.identity().1,
    };
    let ir = admit_invocation_ir(SchemaVersion::V1, h, p, BTreeMap::new(), &Host).unwrap();
    let rendered = render_invocation_ir(&ir, &Host, profile, &renderer, &Counter).unwrap();
    let binding = AttemptBinding {
        schema_version: SchemaVersion::V1,
        attempt: 1,
        invocation: ir.identity(),
        profile: rendered.rendered().accounting().count.profile.clone(),
        payload_digest: rendered
            .rendered()
            .accounting()
            .count
            .payload_digest
            .clone(),
    };
    (ir, rendered, binding)
}
fn context() -> RequestContext {
    RequestContext::new(
        "adapter-fixture".into(),
        Instant::now() + Duration::from_secs(3),
    )
}
fn call(
    adapter: &HttpModelAdapter,
    f: &(InvocationIr, RenderedInvocationIr, AttemptBinding),
    cx: &RequestContext,
    max: usize,
) -> InvocationAttemptResponse {
    adapter.invoke(BoundedInvocationRequest {
        ir: &f.0,
        rendered: &f.1,
        binding: &f.2,
        context: cx,
        max_output_bytes: max,
        reserved_cost_microunits: 7,
    })
}
fn success(kind: AdapterKind) -> Value {
    serde_json::from_str(match kind {
        AdapterKind::LocalServiceV1 => {
            include_str!("../../../tests/fixtures/model-adapters-v1/local-response.json")
        }
        AdapterKind::ChatCompletionsV1 | AdapterKind::ChatCompletionsLegacyV1 => {
            include_str!("../../../tests/fixtures/model-adapters-v1/chat-response.json")
        }
    })
    .unwrap()
}

fn server(
    kind: AdapterKind,
    status: u16,
    response: String,
    delay: Duration,
    secret: bool,
) -> (String, thread::JoinHandle<Value>) {
    server_observed(kind, status, response, delay, secret, None)
}
fn server_observed(
    kind: AdapterKind,
    status: u16,
    response: String,
    delay: Duration,
    secret: bool,
    accepted: Option<std::sync::mpsc::Sender<()>>,
) -> (String, thread::JoinHandle<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("fixture request missing: {error}"),
            }
        };
        // Accepted sockets inherit nonblocking mode on Windows.
        stream.set_nonblocking(false).unwrap();
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
                let len: usize = headers
                    .lines()
                    .find_map(|s| {
                        s.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_owned)
                    })
                    .unwrap()
                    .parse()
                    .unwrap();
                if bytes.len() >= i + 4 + len {
                    break (i + 4, len);
                }
            }
        };
        let headers = String::from_utf8_lossy(&bytes[..start]);
        let path = if kind == AdapterKind::LocalServiceV1 {
            "/v1/invoke"
        } else {
            "/v1/chat/completions"
        };
        assert!(headers.starts_with(&format!("POST {path} HTTP/1.1")));
        assert_eq!(
            headers.contains("Authorization: Bearer runtime-secret"),
            secret
        );
        let body: Value = serde_json::from_slice(&bytes[start..start + length]).unwrap();
        assert_eq!(body["messages"].as_array().unwrap().len(), 2);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert!(!body.to_string().contains("runtime-secret"));
        if let Some(accepted) = accepted {
            accepted.send(()).unwrap();
        }
        thread::sleep(delay);
        let _ = write!(
            stream,
            "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
            response.len()
        );
        body
    });
    (endpoint, handle)
}
#[test]
fn both_transports_share_contract_and_preserve_rendering_controls_and_identity() {
    for kind in [AdapterKind::LocalServiceV1, AdapterKind::ChatCompletionsV1] {
        let f = fixture(kind, |_, _| {});
        let (endpoint, server) = server(
            kind,
            200,
            success(kind).to_string(),
            Duration::ZERO,
            kind == AdapterKind::ChatCompletionsV1,
        );
        let adapter = HttpModelAdapter::new(
            kind,
            endpoint,
            if kind == AdapterKind::ChatCompletionsV1 {
                Some("runtime-secret".into())
            } else {
                None
            },
        )
        .unwrap();
        assert!(adapter.observation().is_none());
        let result = call(&adapter, &f, &context(), 1024);
        let bytes = result.output.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["result"],
            json!({"message":"verified separately"})
        );
        gateway_application::output_parsing::parse_invocation_output(&f.0, &bytes).unwrap();
        let usage = result.usage.unwrap();
        assert_eq!(usage.binding, f.2);
        assert_eq!(usage.output_tokens, 3);
        assert_eq!(usage.cost_microunits, 7);
        let receipt = adapter.observation().unwrap().receipt.unwrap();
        assert_eq!(receipt.actual_model_id, "model");
        assert_eq!(receipt.profile, f.2.profile);
        assert_eq!(
            receipt.generation,
            f.0.prepared().effective_generation().clone()
        );
        assert!(!json!(receipt).to_string().contains("runtime-secret"));
        let wire = server.join().unwrap();
        assert_eq!(
            wire,
            serde_json::from_str::<Value>(f.1.rendered().payload()).unwrap()
        );
        assert!(
            wire["messages"][1]["content"]
                .as_str()
                .unwrap()
                .contains("ignore")
        );
        let options = if kind == AdapterKind::LocalServiceV1 {
            &wire["options"]
        } else {
            &wire
        };
        assert_eq!(options["temperature"], 0.0);
        assert_eq!(options["seed"], 42);
        assert_eq!(options["stop"], json!(["END", "STOP"]));
    }
}
#[test]
fn malformed_failure_identity_usage_and_output_are_explicit() {
    for kind in [AdapterKind::LocalServiceV1, AdapterKind::ChatCompletionsV1] {
        let f = fixture(kind, |_, _| {});
        let mut changed = success(kind);
        changed[if kind == AdapterKind::LocalServiceV1 {
            "model_id"
        } else {
            "model"
        }] = json!("wrong");
        let mut badusage = success(kind);
        badusage["usage"]["prompt_tokens"] = json!(-1);
        let mut nousage = success(kind);
        nousage.as_object_mut().unwrap().remove("usage");
        let mut cases = vec![
            (200, "{bad".into(), Some(AdapterDiagnostic::InvalidResponse)),
            (
                503,
                "unavailable".into(),
                Some(AdapterDiagnostic::Unavailable),
            ),
            (
                200,
                changed.to_string(),
                Some(AdapterDiagnostic::IdentityMismatch),
            ),
            (
                200,
                badusage.to_string(),
                Some(AdapterDiagnostic::InvalidResponse),
            ),
            (200, nousage.to_string(), None),
        ];
        if kind == AdapterKind::ChatCompletionsV1 {
            for path in ["refusal", "tool_calls"] {
                let mut bad = success(kind);
                bad["choices"][0]["message"][path] = json!("denied");
                cases.push((
                    200,
                    bad.to_string(),
                    Some(AdapterDiagnostic::InvalidResponse),
                ));
            }
            let mut bad = success(kind);
            bad["choices"][0]["finish_reason"] = json!("length");
            cases.push((
                200,
                bad.to_string(),
                Some(AdapterDiagnostic::InvalidResponse),
            ));
        } else {
            for key in ["artifact_digest", "model_version", "kind", "schema_version"] {
                let mut bad = success(kind);
                bad[key] = json!("wrong");
                cases.push((
                    200,
                    bad.to_string(),
                    Some(AdapterDiagnostic::IdentityMismatch),
                ));
            }
        }
        let mut bad = success(kind);
        let content = if kind == AdapterKind::LocalServiceV1 {
            &mut bad["content"]
        } else {
            &mut bad["choices"][0]["message"]["content"]
        };
        *content = Value::Null;
        cases.push((
            200,
            bad.to_string(),
            Some(AdapterDiagnostic::InvalidResponse),
        ));
        for (status, body, expected) in cases {
            let (endpoint, server) = server(kind, status, body, Duration::ZERO, false);
            let adapter = HttpModelAdapter::new(kind, endpoint, None).unwrap();
            let result = call(&adapter, &f, &context(), 1024);
            assert_eq!(adapter.observation().unwrap().diagnostic, expected);
            assert_eq!(result.output.is_err(), expected.is_some());
            if expected.is_none() {
                assert!(result.usage.is_none());
            }
            server.join().unwrap();
        }
        let (endpoint, server) =
            server(kind, 200, success(kind).to_string(), Duration::ZERO, false);
        let adapter = HttpModelAdapter::new(kind, endpoint, None).unwrap();
        assert!(call(&adapter, &f, &context(), 1).output.is_err());
        assert_eq!(
            adapter.observation().unwrap().diagnostic,
            Some(AdapterDiagnostic::OutputLimit)
        );
        server.join().unwrap();
    }
}
#[test]
fn configuration_binding_cancellation_and_deadlines_fail_closed() {
    for endpoint in [
        "file:///tmp/model",
        "http://remote.example",
        "https://host/?key=secret",
        "https://user:pass@host",
        "https://host/#x",
        "https://host/\n",
        "https://host/\\",
        "https://host/\"",
    ] {
        assert!(
            HttpModelAdapter::new(AdapterKind::ChatCompletionsV1, endpoint.into(), None).is_err()
        );
    }
    for secret in ["", "secret\r\nInjected: yes"] {
        assert!(
            HttpModelAdapter::new(
                AdapterKind::ChatCompletionsV1,
                "https://example.com".into(),
                Some(secret.into())
            )
            .is_err()
        );
    }
    assert!(
        HttpModelAdapter::new(
            AdapterKind::LocalServiceV1,
            "http://localhost".into(),
            Some("secret".into())
        )
        .is_err()
    );
    assert!(
        HttpModelAdapter::new(
            AdapterKind::ChatCompletionsV1,
            "https://example.com".into(),
            None
        )
        .is_ok()
    );
    let mut f = fixture(AdapterKind::LocalServiceV1, |_, _| {});
    let adapter = HttpModelAdapter::new(
        AdapterKind::LocalServiceV1,
        "http://127.0.0.1:1".into(),
        None,
    )
    .unwrap();
    assert!(call(&adapter, &f, &context(), 1024).output.is_err());
    assert_eq!(
        adapter.observation().unwrap().diagnostic,
        Some(AdapterDiagnostic::Unavailable)
    );
    f.2.payload_digest = context_content_digest(b"wrong");
    assert!(call(&adapter, &f, &context(), 1024).output.is_err());
    assert_eq!(
        adapter.observation().unwrap().diagnostic,
        Some(AdapterDiagnostic::InvalidBinding)
    );
    let cx = context();
    cx.cancel();
    call(&adapter, &f, &cx, 1024);
    assert_eq!(
        adapter.observation().unwrap().diagnostic,
        Some(AdapterDiagnostic::Cancelled)
    );
    let cx = RequestContext::new("expired".into(), Instant::now());
    call(&adapter, &f, &cx, 1024);
    assert_eq!(
        adapter.observation().unwrap().diagnostic,
        Some(AdapterDiagnostic::DeadlineExceeded)
    );
    let f = fixture(AdapterKind::LocalServiceV1, |_, _| {});
    assert!(AdapterRenderer::new(AdapterKind::ChatCompletionsV1, f.0.prepared().clone()).is_err());
    for cancel in [true, false] {
        let (accepted, received) = std::sync::mpsc::channel();
        let (endpoint, server) = server_observed(
            AdapterKind::LocalServiceV1,
            200,
            success(AdapterKind::LocalServiceV1).to_string(),
            Duration::from_millis(if cancel { 200 } else { 1200 }),
            false,
            Some(accepted),
        );
        let adapter = HttpModelAdapter::new(AdapterKind::LocalServiceV1, endpoint, None).unwrap();
        let mut cx = context();
        if !cancel {
            cx.deadline = Instant::now() + Duration::from_millis(600);
        }
        let other = cx.clone();
        let canceller = thread::spawn(move || {
            // Cancel only after the complete request reached the fixture;
            // process startup and Windows scheduling are not the behavior under test.
            received.recv_timeout(Duration::from_secs(3)).unwrap();
            if cancel {
                other.cancel();
            }
        });
        assert!(call(&adapter, &f, &cx, 1024).output.is_err());
        canceller.join().unwrap();
        server.join().unwrap();
        assert_eq!(
            adapter.observation().unwrap().diagnostic,
            Some(if cancel {
                AdapterDiagnostic::Cancelled
            } else {
                AdapterDiagnostic::DeadlineExceeded
            })
        );
    }
}

#[test]
fn adapters_dispatch_through_the_existing_run_and_parser() {
    for kind in [AdapterKind::LocalServiceV1, AdapterKind::ChatCompletionsV1] {
        let f = fixture(kind, |_, _| {});
        let (endpoint, server) =
            server(kind, 200, success(kind).to_string(), Duration::ZERO, false);
        let adapter = HttpModelAdapter::new(kind, endpoint, None).unwrap();
        let limits = InvocationRunLimits {
            schema_version: SchemaVersion::V1,
            max_attempts: 1,
            timeout_ms: 3000,
            invocation_input_tokens: 100_000,
            invocation_output_tokens: 128,
            run_input_tokens: 100_000,
            run_output_tokens: 128,
            cost_microunits: 7,
            output_bytes: 4096,
            reconciliation_tolerance: 100_000,
        };
        let mut run = InvocationRun::new(f.0.clone(), limits, context()).unwrap();
        assert!(matches!(
            run.dispatch(
                &f.0,
                &f.1,
                gateway_application::closed_loop::LoopDecision::Continue,
                7,
                &Host,
                &adapter
            )
            .unwrap(),
            InvocationAttemptResult::Parsed(_)
        ));
        assert_eq!(run.usage().attempts, 1);
        assert_eq!(run.usage().output_tokens, 3);
        assert!(adapter.observation().unwrap().receipt.is_some());
        server.join().unwrap();
    }
}

/// Explicit opt-in transport qualification with synthetic, admitted inputs. It
/// is never run by the default suite and does not promote a semantic capability.
#[test]
#[ignore = "requires CG_MODEL_ADAPTER_LIVE_PROFILE and explicit --ignored"]
fn live_adapter_profile() {
    let path =
        std::env::var("CG_MODEL_ADAPTER_LIVE_PROFILE").expect("explicit profile path required");
    let profile: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(profile["schema_version"], "1.0");
    let output =
        std::env::var("CG_MODEL_ADAPTER_LIVE_OUTPUT").expect("explicit report path required");
    std::fs::write(&output,b"{\"schema_version\":\"1.0\",\"status\":\"NOT_COMPLETE\",\"semantic_qualification\":\"NOT_CLAIMED\"}\n").unwrap();
    let entries = profile["adapters"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "qualify both adapters explicitly");
    let mut receipts = Vec::new();
    for (entry, kind) in entries
        .iter()
        .zip([AdapterKind::LocalServiceV1, AdapterKind::ChatCompletionsV1])
    {
        let model = serde_json::from_value(entry["model"].clone()).unwrap();
        let f = fixture_with_model(
            kind,
            |d, p| {
                let locality = serde_json::from_value(entry["locality"].clone()).unwrap();
                p.locality = locality;
                d.permitted_localities = [locality].into_iter().collect();
            },
            Some((
                model,
                entry["context_limit"].as_u64().unwrap().try_into().unwrap(),
            )),
        );
        let credential = entry["credential_env"]
            .as_str()
            .map(|name| std::env::var(name).expect("runtime credential missing"));
        let adapter =
            HttpModelAdapter::new(kind, entry["endpoint"].as_str().unwrap().into(), credential)
                .unwrap();
        let cx = RequestContext::new(
            "explicit-live-profile".into(),
            Instant::now() + Duration::from_secs(60),
        );
        let response = call(&adapter, &f, &cx, 65536);
        let raw = response
            .output
            .expect("live transport failed; no provider body is logged");
        gateway_application::output_parsing::parse_invocation_output(&f.0, &raw)
            .expect("live output contract mismatch");
        receipts.push(adapter.observation().unwrap().receipt.unwrap());
    }
    std::fs::write(
        output,
        serde_json::to_vec_pretty(&json!({"schema_version":"1.0","status":"TRANSPORT_PASS",
        "semantic_qualification":"NOT_CLAIMED","receipts":receipts}))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn result_only_http_rejects_duplicate_outer_model_identity() {
    for kind in [
        AdapterKind::ChatCompletionsV1,
        AdapterKind::ChatCompletionsLegacyV1,
    ] {
        let f = fixture(kind, |_, _| {});
        let raw = success(kind).to_string().replace(
            "\"model\":\"model\"",
            "\"model\":\"foreign\",\"model\":\"model\"",
        );
        let (endpoint, server) = server(kind, 200, raw, Duration::ZERO, false);
        let adapter = HttpModelAdapter::new(kind, endpoint, None).unwrap();
        assert!(call(&adapter, &f, &context(), 65536).output.is_err());
        assert_eq!(
            adapter.observation().unwrap().diagnostic,
            Some(AdapterDiagnostic::InvalidResponse)
        );
        server.join().unwrap();
    }
}

#[test]
fn escaped_credentials_are_rejected_in_response_text_and_nested_result_json() {
    for kind in [
        AdapterKind::ChatCompletionsV1,
        AdapterKind::ChatCompletionsLegacyV1,
    ] {
        for nested in [false, true] {
            let f = fixture(kind, |_, _| {});
            let mut response = success(kind);
            response["choices"][0]["message"]["content"] = if nested {
                json!(r#"{"message":"\u0072untime-secret"}"#)
            } else {
                json!(r#"{"message":"runtime-secret"}"#)
            };
            let raw = response
                .to_string()
                .replace("runtime-secret", r"\u0072untime-secret");
            assert!(!raw.contains("runtime-secret"));
            let (endpoint, server) = server(kind, 200, raw, Duration::ZERO, true);
            let adapter =
                HttpModelAdapter::new(kind, endpoint, Some("runtime-secret".into())).unwrap();
            assert!(call(&adapter, &f, &context(), 65536).output.is_err());
            assert_eq!(
                adapter.observation().unwrap().diagnostic,
                Some(AdapterDiagnostic::InvalidResponse)
            );
            server.join().unwrap();
        }
    }
}

#[test]
fn unsupported_modes_remote_locality_reflection_http_denials_and_large_wire_fail_closed() {
    let kind = AdapterKind::ChatCompletionsV1;
    let f = fixture(kind, |d, _| {
        let mut g = d.generation.data().clone();
        g.structured_output_mode = StructuredOutputMode::Json;
        d.generation = GenerationProfile::try_from(g).unwrap();
    });
    let adapter = HttpModelAdapter::new(kind, "https://example.invalid".into(), None).unwrap();
    call(&adapter, &f, &context(), 1024);
    assert_eq!(
        adapter.observation().unwrap().diagnostic,
        Some(AdapterDiagnostic::CapabilityMismatch)
    );
    let adapter = HttpModelAdapter::new(kind, "http://localhost:1".into(), None).unwrap();
    for max in [0, usize::MAX] {
        call(&adapter, &f, &context(), max);
        assert_eq!(
            adapter.observation().unwrap().diagnostic,
            Some(AdapterDiagnostic::InvalidBinding)
        );
    }
    // Shared generation negotiation remains separate; renderer rejects unsupported tool mode.
    let h = support::handoff();
    let p = support::prepared(&h, 100_000, true);
    assert!(AdapterRenderer::new(AdapterKind::LocalServiceV1, p).is_err());
    for (status, body, secret, expected) in [
        (
            302,
            success(kind).to_string(),
            None,
            AdapterDiagnostic::ProviderRejected,
        ),
        (
            401,
            "denied".to_string(),
            None,
            AdapterDiagnostic::ProviderRejected,
        ),
        (
            429,
            "rate limited".to_string(),
            None,
            AdapterDiagnostic::Unavailable,
        ),
        (200, "x".repeat(30000), None, AdapterDiagnostic::OutputLimit),
        (
            200,
            success(kind)
                .to_string()
                .replace("verified separately", "runtime-secret"),
            Some("runtime-secret".to_string()),
            AdapterDiagnostic::InvalidResponse,
        ),
    ] {
        let (endpoint, server) = server(kind, status, body, Duration::ZERO, secret.is_some());
        let adapter = HttpModelAdapter::new(kind, endpoint, secret).unwrap();
        call(&adapter, &f, &context(), 1024);
        assert_eq!(adapter.observation().unwrap().diagnostic, Some(expected));
        server.join().unwrap();
    }
    for endpoint in [
        "https://",
        "https://host:0",
        "https://host:abc",
        "https://h%6fst",
    ] {
        assert!(HttpModelAdapter::new(kind, endpoint.into(), None).is_err());
    }
    // A stale renderer configuration cannot silently replace generation controls.
    struct WrongRenderer;
    impl PromptRendererPort for WrongRenderer {
        fn identity(&self) -> (ReferenceId, ReferenceId) {
            (id("chat-completions"), id("1"))
        }
        fn render(&self, _: &RenderInput) -> Result<Value, RenderDiagnostic> {
            Ok(json!({"messages":[],"model":"foreign"}))
        }
    }
    let rendered =
        render_invocation_ir(&f.0, &Host, f.2.profile.clone(), &WrongRenderer, &Counter).unwrap();
    let mut binding = f.2.clone();
    binding.payload_digest = rendered
        .rendered()
        .accounting()
        .count
        .payload_digest
        .clone();
    call(
        &adapter,
        &(f.0.clone(), rendered, binding),
        &context(),
        1024,
    );
    // Endpoint locality validation happens before body validation for this adapter.
    let adapter = HttpModelAdapter::new(kind, "http://localhost:1".into(), None).unwrap();
    let rendered =
        render_invocation_ir(&f.0, &Host, f.2.profile.clone(), &WrongRenderer, &Counter).unwrap();
    let mut binding = f.2.clone();
    binding.payload_digest = rendered
        .rendered()
        .accounting()
        .count
        .payload_digest
        .clone();
    call(&adapter, &(f.0, rendered, binding), &context(), 1024);
    assert_eq!(
        adapter.observation().unwrap().diagnostic,
        Some(AdapterDiagnostic::InvalidBinding)
    );
}

#[test]
fn versioned_negative_wire_fixtures_keep_stable_diagnostics() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/model-adapters-v1/negative-responses.json"
    ))
    .unwrap();
    let diagnostics: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/model-adapters-v1/diagnostics.json"
    ))
    .unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let kind = if case["kind"] == "local" {
            AdapterKind::LocalServiceV1
        } else {
            AdapterKind::ChatCompletionsV1
        };
        let f = fixture(kind, |_, _| {});
        let (endpoint, server) = server(
            kind,
            200,
            case["response"].to_string(),
            Duration::ZERO,
            false,
        );
        let adapter = HttpModelAdapter::new(kind, endpoint, None).unwrap();
        assert!(call(&adapter, &f, &context(), 1024).output.is_err());
        let code = json!(adapter.observation().unwrap().diagnostic.unwrap());
        assert_eq!(code, case["diagnostic"]);
        assert!(
            diagnostics["diagnostics"]
                .as_array()
                .unwrap()
                .contains(&code)
        );
        server.join().unwrap();
    }
}

#[test]
fn cancelled_adapter_preserves_run_cancellation_and_reservations() {
    let f = fixture(AdapterKind::LocalServiceV1, |_, _| {});
    let (sent, received) = std::sync::mpsc::channel();
    let (endpoint, server) = server_observed(
        AdapterKind::LocalServiceV1,
        200,
        success(AdapterKind::LocalServiceV1).to_string(),
        Duration::from_millis(200),
        false,
        Some(sent),
    );
    let adapter = HttpModelAdapter::new(AdapterKind::LocalServiceV1, endpoint, None).unwrap();
    let cx = RequestContext::new(
        "cancelled-adapter".into(),
        Instant::now() + Duration::from_secs(10),
    );
    let cancellation = cx.clone();
    let canceller = thread::spawn(move || {
        received.recv_timeout(Duration::from_secs(10)).unwrap();
        cancellation.cancel();
    });
    let limits = InvocationRunLimits {
        schema_version: SchemaVersion::V1,
        max_attempts: 1,
        timeout_ms: 10000,
        invocation_input_tokens: 100_000,
        invocation_output_tokens: 128,
        run_input_tokens: 100_000,
        run_output_tokens: 128,
        cost_microunits: 7,
        output_bytes: 4096,
        reconciliation_tolerance: 100_000,
    };
    let mut run = InvocationRun::new(f.0.clone(), limits, cx).unwrap();
    assert_eq!(
        run.dispatch(
            &f.0,
            &f.1,
            gateway_application::closed_loop::LoopDecision::Continue,
            7,
            &Host,
            &adapter
        )
        .unwrap_err(),
        InvocationRunDiagnostic::Cancelled
    );
    assert_eq!(run.usage().attempts, 1);
    assert_eq!(run.usage().output_tokens, 128);
    assert_eq!(run.usage().cost_microunits, 7);
    canceller.join().unwrap();
    server.join().unwrap();
}

// Synthetic deterministic verifier used only for adapter -> trace -> replay
// integration. Production host verifier/authority ownership remains unchanged.
impl gateway_application::output_verification::OutputVerificationPort for Host {
    fn resolve_reference(
        &self,
        _: &InvocationIr,
        _: &gateway_domain::output_verification::OutputReference,
    ) -> Result<(), gateway_domain::output_verification::VerificationError> {
        Err(gateway_domain::output_verification::VerificationError::UnsupportedReference)
    }
    fn evaluate(
        &self,
        ir: &InvocationIr,
        output: &gateway_application::output_verification::ParsedOutput,
    ) -> Result<
        gateway_application::output_verification::ConstraintReport,
        gateway_domain::output_verification::VerificationError,
    > {
        use gateway_domain::semantic_output::CheckOutcome;
        Ok(gateway_application::output_verification::ConstraintReport {
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
                .filter(|c| **c != SemanticVerificationCheck::OutputSchemaValid)
                .map(|c| (*c, CheckOutcome::Passed))
                .collect(),
        })
    }
}

#[test]
fn both_adapter_outputs_replay_after_http_server_is_gone() {
    use gateway_application::invocation_qualification::{replay_output, trace_output};
    for kind in [AdapterKind::LocalServiceV1, AdapterKind::ChatCompletionsV1] {
        let f = fixture(kind, |_, _| {});
        let (endpoint, server) =
            server(kind, 200, success(kind).to_string(), Duration::ZERO, false);
        let adapter = HttpModelAdapter::new(kind, endpoint, None).unwrap();
        let raw = call(&adapter, &f, &context(), 4096).output.unwrap();
        // The server accepts exactly one request and then shuts down. Replay
        // has no adapter/dispatch port and works after the transport is dropped.
        server.join().unwrap();
        drop(adapter);
        let trace = trace_output(&f.0, &f.1, &raw).unwrap();
        for _ in 0..2 {
            assert_eq!(
                replay_output(&f.0, &trace.replay, &raw, &Host).diagnostic,
                None
            );
        }
    }
}

#[test]
fn runtime_names_are_outer_mappings_bound_to_exact_canonical_model_pins() {
    for kind in [
        AdapterKind::ChatCompletionsV1,
        AdapterKind::ChatCompletionsLegacyV1,
    ] {
        let original = fixture(kind, |_, _| {});
        let mapping = RuntimeModelBinding {
            canonical: original.0.prepared().capability().data().model.clone(),
            runtime_id: "org/model:snapshot-7".into(),
        };
        let renderer =
            AdapterRenderer::for_runtime(kind, original.0.prepared().clone(), mapping.clone())
                .unwrap();
        let mut profile = original.2.profile.clone();
        profile.template_version = renderer.identity().1;
        let rendered =
            render_invocation_ir(&original.0, &Host, profile, &renderer, &Counter).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(rendered.rendered().payload()).unwrap()["model"],
            mapping.runtime_id
        );
        let wire: Value = serde_json::from_str(rendered.rendered().payload()).unwrap();
        let (limit, unsupported) = if kind == AdapterKind::ChatCompletionsLegacyV1 {
            ("max_tokens", "max_completion_tokens")
        } else {
            ("max_completion_tokens", "max_tokens")
        };
        assert_eq!(
            wire[limit],
            original
                .0
                .prepared()
                .effective_generation()
                .max_output_tokens
        );
        assert!(wire.get(unsupported).is_none());
        let mut binding = original.2.clone();
        binding.profile = rendered.rendered().accounting().count.profile.clone();
        binding.payload_digest = rendered
            .rendered()
            .accounting()
            .count
            .payload_digest
            .clone();
        let f = (original.0, rendered, binding);
        let mut response = success(kind);
        response["model"] = json!(mapping.runtime_id);
        let (endpoint, s) = server(kind, 200, response.to_string(), Duration::ZERO, false);
        let adapter = HttpModelAdapter::for_runtime(kind, endpoint, None, mapping.clone()).unwrap();
        assert!(call(&adapter, &f, &context(), 4096).output.is_ok());
        s.join().unwrap();
        assert_eq!(
            adapter
                .observation()
                .unwrap()
                .receipt
                .unwrap()
                .actual_model_id,
            mapping.runtime_id
        );
        let mut wrong = mapping.clone();
        wrong.canonical = pin("different-model");
        assert!(AdapterRenderer::for_runtime(kind, f.0.prepared().clone(), wrong.clone()).is_err());
        let adapter =
            HttpModelAdapter::for_runtime(kind, "http://127.0.0.1:1".into(), None, wrong).unwrap();
        assert!(call(&adapter, &f, &context(), 4096).output.is_err());
        assert_eq!(
            adapter.observation().unwrap().diagnostic,
            Some(AdapterDiagnostic::InvalidBinding)
        );
        for value in [
            String::new(),
            "model\nunsafe".into(),
            "contains space".into(),
            "x".repeat(257),
        ] {
            let mut invalid = mapping.clone();
            invalid.runtime_id = value;
            assert!(
                AdapterRenderer::for_runtime(kind, f.0.prepared().clone(), invalid.clone())
                    .is_err()
            );
            assert!(
                HttpModelAdapter::for_runtime(kind, "http://127.0.0.1:1".into(), None, invalid)
                    .is_err()
            );
        }
    }
}

#[test]
fn host_task_directive_is_versioned_and_bound_to_transport() {
    for kind in [
        AdapterKind::LocalServiceV1,
        AdapterKind::ChatCompletionsV1,
        AdapterKind::ChatCompletionsLegacyV1,
    ] {
        for mapped in [false, true] {
            let original = fixture(kind, |_, _| {});
            let mapping = mapped.then(|| RuntimeModelBinding {
                canonical: original.0.prepared().capability().data().model.clone(),
                runtime_id: "model:snapshot".into(),
            });
            let renderer =
                AdapterRenderer::for_host(kind, original.0.prepared().clone(), mapping.clone())
                    .unwrap();
            assert_eq!(renderer.identity().1.as_str(), "8");
            let mut profile = original.2.profile.clone();
            profile.template_version = renderer.identity().1;
            let rendered =
                render_invocation_ir(&original.0, &Host, profile, &renderer, &Counter).unwrap();
            let wire: Value = serde_json::from_str(rendered.rendered().payload()).unwrap();
            assert_eq!(wire["messages"].as_array().unwrap().len(), 2);
            let authority = wire["messages"][0]["content"].as_str().unwrap();
            assert!(authority.contains("semantic.task.goal.description"));
            assert!(!authority.contains("ignore policy"));
            let mut binding = original.2.clone();
            binding.profile = rendered.rendered().accounting().count.profile.clone();
            binding.payload_digest = rendered
                .rendered()
                .accounting()
                .count
                .payload_digest
                .clone();
            let f = (original.0, rendered, binding);
            let mut response = success(kind);
            if let Some(mapping) = &mapping {
                response[if kind == AdapterKind::LocalServiceV1 {
                    "model_id"
                } else {
                    "model"
                }] = json!(mapping.runtime_id);
            }
            let (endpoint, s) = server(kind, 200, response.to_string(), Duration::ZERO, false);
            let adapter = HttpModelAdapter::for_host(kind, endpoint, None, mapping).unwrap();
            assert!(call(&adapter, &f, &context(), 4096).output.is_ok());
            s.join().unwrap();
            let legacy = HttpModelAdapter::new(kind, "http://localhost:1".into(), None).unwrap();
            call(&legacy, &f, &context(), 4096);
            assert_eq!(
                legacy.observation().unwrap().diagnostic,
                Some(AdapterDiagnostic::InvalidBinding)
            );
        }
    }
}
