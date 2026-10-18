//! Optional HTTP transports for admitted, bounded model invocations. No SDK,
//! credentials, provider errors or raw response bodies enter canonical records.
use gateway_application::{
    invocation_resilience::{
        BoundedInvocationPort, BoundedInvocationRequest, InvocationAttemptResponse,
    },
    model_invocation::PreparedModelInvocation,
    prompt_rendering::{PromptRendererPort, RenderInput},
};
use gateway_domain::{
    ReferenceId, ResolvedReference, SchemaVersion,
    invocation_resilience::{InvocationAttemptFailure, ProviderUsage},
    model_invocation::{
        EffectiveGenerationRecord, GenerationControl, GenerationValue, StructuredOutputMode,
    },
    prompt_rendering::{RenderDiagnostic, TokenizerProfile},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdapterDiagnostic {
    InvalidConfiguration,
    InvalidBinding,
    CapabilityMismatch,
    Unavailable,
    ProviderRejected,
    Cancelled,
    DeadlineExceeded,
    OutputLimit,
    InvalidResponse,
    IdentityMismatch,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    LocalServiceV1,
    ChatCompletionsV1,
}
impl AdapterKind {
    fn renderer(self) -> &'static str {
        match self {
            Self::LocalServiceV1 => "local-invocation",
            Self::ChatCompletionsV1 => "chat-completions",
        }
    }
    fn path(self) -> &'static str {
        match self {
            Self::LocalServiceV1 => "/v1/invoke",
            Self::ChatCompletionsV1 => "/v1/chat/completions",
        }
    }
}
/// Renders the entire wire body, including generation and schema framing, before
/// token accounting. The model ID is an immutable runtime snapshot, never an alias.
pub struct AdapterRenderer {
    kind: AdapterKind,
    prepared: PreparedModelInvocation,
}
impl AdapterRenderer {
    pub fn new(
        kind: AdapterKind,
        prepared: PreparedModelInvocation,
    ) -> Result<Self, AdapterDiagnostic> {
        controls(kind, &prepared)?;
        Ok(Self { kind, prepared })
    }
}
impl PromptRendererPort for AdapterRenderer {
    fn identity(&self) -> (ReferenceId, ReferenceId) {
        (
            ReferenceId::new(self.kind.renderer()).expect("static"),
            ReferenceId::new("1").expect("static"),
        )
    }
    fn render(&self, input: &RenderInput) -> Result<Value, RenderDiagnostic> {
        let messages = crate::prompt_renderers::SemanticRenderer::ChatMessagesV1.render(input)?;
        wire(self.kind, &self.prepared, messages["messages"].clone())
            .map_err(|_| RenderDiagnostic::RenderingFailed)
    }
}
fn controls(kind: AdapterKind, p: &PreparedModelInvocation) -> Result<Value, AdapterDiagnostic> {
    let g = p.effective_generation();
    if !matches!(
        g.structured_output_mode,
        StructuredOutputMode::Json | StructuredOutputMode::JsonSchema
    ) || !p.invocation().data().permitted_capabilities.is_empty()
        || g.unexposed_defaults.contains(&GenerationControl::TopK)
            && kind == AdapterKind::ChatCompletionsV1
    {
        return Err(AdapterDiagnostic::CapabilityMismatch);
    }
    let mut options = json!({});
    for (key, value) in &g.controls {
        let name = match key {
            GenerationControl::Temperature => "temperature",
            GenerationControl::TopP => "top_p",
            GenerationControl::TopK if kind == AdapterKind::LocalServiceV1 => "top_k",
            GenerationControl::StopConditions => "stop",
            GenerationControl::Seed => "seed",
            _ => return Err(AdapterDiagnostic::CapabilityMismatch),
        };
        options[name] = match &value.value {
            GenerationValue::Scalar(v) => json!(v),
            GenerationValue::Integer(v) => json!(v),
            GenerationValue::Stops(v) => json!(v),
        };
    }
    Ok(options)
}
fn wire(
    kind: AdapterKind,
    p: &PreparedModelInvocation,
    messages: Value,
) -> Result<Value, AdapterDiagnostic> {
    let options = controls(kind, p)?;
    let d = p.invocation().data();
    let model = &p.capability().data().model;
    let g = p.effective_generation();
    Ok(match kind {
        AdapterKind::LocalServiceV1 => json!({"schema_version":"1.0", "model_id":model.id,
            "artifact_digest":format!("sha256:{}",model.digest.as_str()), "model_version":model.revision,
            "input_contract":d.input_contract, "output_contract":d.output.schema.id,
            "messages":messages, "options":options, "max_output_tokens":g.max_output_tokens,
            "context_limit":p.capability().data().context_limit,
            "output_schema":d.output.json_schema(), "structured_output_mode":g.structured_output_mode}),
        AdapterKind::ChatCompletionsV1 => {
            let mut body = options;
            body["model"] = json!(model.id);
            body["messages"] = messages;
            body["max_completion_tokens"] = json!(g.max_output_tokens);
            body["stream"] = json!(false);
            body["response_format"] = match g.structured_output_mode {
                StructuredOutputMode::JsonSchema => json!({"type":"json_schema", "json_schema":{
                    "name":"gateway_output", "strict":true,"schema":d.output.json_schema()}}),
                _ => json!({"type":"json_object"}),
            };
            body
        }
    })
}
#[derive(Debug, Clone, Serialize)]
pub struct AdapterReceipt {
    pub schema_version: SchemaVersion,
    pub model: ResolvedReference<ReferenceId>,
    pub provider: ResolvedReference<ReferenceId>,
    pub profile: TokenizerProfile,
    pub generation: EffectiveGenerationRecord,
    pub actual_model_id: ReferenceId,
}
#[derive(Debug, Clone, Serialize)]
pub struct AdapterObservation {
    pub receipt: Option<AdapterReceipt>,
    pub diagnostic: Option<AdapterDiagnostic>,
}
/// Runtime secret is deliberately neither Debug nor Serialize. curl receives it
/// through stdin configuration, never argv, a file, prompt, receipt or stderr.
pub struct HttpModelAdapter {
    kind: AdapterKind,
    endpoint: String,
    credential: Option<String>,
    observation: Mutex<Option<AdapterObservation>>,
    loopback: bool,
}
impl HttpModelAdapter {
    pub fn new(
        kind: AdapterKind,
        endpoint: String,
        credential: Option<String>,
    ) -> Result<Self, AdapterDiagnostic> {
        let secure = endpoint.starts_with("https://");
        let authority = endpoint
            .strip_prefix("https://")
            .or_else(|| endpoint.strip_prefix("http://"))
            .ok_or(AdapterDiagnostic::InvalidConfiguration)?
            .split('/')
            .next()
            .unwrap_or("");
        let (host, port) = authority
            .rsplit_once(':')
            .map_or((authority, None), |(h, p)| (h, Some(p)));
        if host.is_empty()
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            || port.is_some_and(|p| p.parse::<u16>().map_or(true, |v| v == 0))
        {
            return Err(AdapterDiagnostic::InvalidConfiguration);
        }
        let loopback = matches!(host, "127.0.0.1" | "localhost");
        if !(secure || loopback)
            || endpoint.contains(['@', '?', '#', '\\', '"'])
            || endpoint.chars().any(char::is_whitespace)
            || credential
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.chars().any(char::is_control))
            || kind == AdapterKind::LocalServiceV1 && credential.is_some()
        {
            return Err(AdapterDiagnostic::InvalidConfiguration);
        }
        Ok(Self {
            kind,
            loopback,
            endpoint: endpoint.trim_end_matches('/').into(),
            credential,
            observation: Mutex::new(None),
        })
    }
    pub fn observation(&self) -> Option<AdapterObservation> {
        self.observation.lock().expect("observation").clone()
    }
    fn execute(
        &self,
        r: &BoundedInvocationRequest<'_>,
    ) -> Result<(Vec<u8>, Option<ProviderUsage>, AdapterReceipt), AdapterDiagnostic> {
        use AdapterDiagnostic as D;
        check_context(r)?;
        if r.max_output_bytes == 0
            || r.max_output_bytes > gateway_domain::output_parsing::MAX_OUTPUT_BYTES
        {
            return Err(D::InvalidBinding);
        }
        let p = r.ir.prepared();
        if p.provider().data().locality
            == gateway_domain::cognitive_routing::PrivacyBoundary::OnDevice
            && !self.loopback
        {
            return Err(D::CapabilityMismatch);
        }
        let a = r.rendered.rendered().accounting();
        if r.rendered.identity() != &r.ir.identity()
            || r.binding.invocation != r.ir.identity()
            || r.binding.profile != a.count.profile
            || r.binding.payload_digest != a.count.payload_digest
            || a.count.profile.renderer.as_str() != self.kind.renderer()
            || a.count.profile.template_version.as_str() != "1"
            || a.count.profile.model != p.capability().data().model
            || a.count.profile.generation_profile
                != p.invocation().data().generation.content_digest()
        {
            return Err(D::InvalidBinding);
        }
        let body: Value =
            serde_json::from_str(r.rendered.rendered().payload()).map_err(|_| D::InvalidBinding)?;
        if !body["messages"].is_array() || body != wire(self.kind, p, body["messages"].clone())? {
            return Err(D::InvalidBinding);
        }
        let raw = http(self, r, &body)?;
        if self
            .credential
            .as_ref()
            .is_some_and(|secret| raw.windows(secret.len()).any(|w| w == secret.as_bytes()))
        {
            return Err(D::InvalidResponse);
        }
        let response: Value = serde_json::from_slice(&raw).map_err(|_| D::InvalidResponse)?;
        let (actual, content, usage) = match self.kind {
            AdapterKind::LocalServiceV1 => {
                let model = &p.capability().data().model;
                if response["schema_version"] != "1.0"
                    || response["kind"] != "proposal"
                    || response["artifact_digest"] != format!("sha256:{}", model.digest.as_str())
                    || response["model_version"] != model.revision.as_str()
                {
                    return Err(D::IdentityMismatch);
                }
                (
                    response["model_id"].as_str(),
                    response["content"].as_str(),
                    &response["usage"],
                )
            }
            AdapterKind::ChatCompletionsV1 => {
                if response["choices"].as_array().is_none_or(|v| v.len() != 1)
                    || response["choices"][0]["finish_reason"] != "stop"
                    || !response["choices"][0]["message"]["tool_calls"].is_null()
                    || !response["choices"][0]["message"]["refusal"].is_null()
                {
                    return Err(D::InvalidResponse);
                }
                (
                    response["model"].as_str(),
                    response["choices"][0]["message"]["content"].as_str(),
                    &response["usage"],
                )
            }
        };
        if actual != Some(p.capability().data().model.id.as_str()) {
            return Err(D::IdentityMismatch);
        }
        let content = content.ok_or(D::InvalidResponse)?;
        // This adapter supports result-only generation. Bind transport output to
        // the admitted invocation without asking the model to invent correlation
        // metadata. Preserve raw JSON bytes so the strict parser still rejects
        // duplicates, malformed JSON and attempts to escape the result field.
        let content = format!("{{\"schema_version\":\"1.0\",\"invocation\":{},\"result\":{},\"references\":[],\"tools\":[]}}",
            serde_json::to_string(&r.ir.identity()).expect("identity JSON"), content).into_bytes();
        if content.len() > r.max_output_bytes {
            return Err(D::OutputLimit);
        }
        // Pricing is host-owned: keep the full cost reservation, never invent a provider price.
        let usage = if usage.is_null() {
            None
        } else {
            Some(ProviderUsage {
                binding: r.binding.clone(),
                input_tokens: usage["prompt_tokens"].as_u64().ok_or(D::InvalidResponse)?,
                output_tokens: usage["completion_tokens"]
                    .as_u64()
                    .ok_or(D::InvalidResponse)?,
                cost_microunits: r.reserved_cost_microunits,
            })
        };
        Ok((
            content,
            usage,
            AdapterReceipt {
                schema_version: SchemaVersion::V1,
                model: p.capability().data().model.clone(),
                provider: p.provider().data().identity.clone(),
                profile: a.count.profile.clone(),
                generation: p.effective_generation().clone(),
                actual_model_id: p.capability().data().model.id.clone(),
            },
        ))
    }
}
impl BoundedInvocationPort for HttpModelAdapter {
    fn invoke(&self, r: BoundedInvocationRequest<'_>) -> InvocationAttemptResponse {
        let result = self.execute(&r);
        let (output, usage, observation) = match result {
            Ok((bytes, usage, receipt)) => (
                Ok(bytes),
                usage,
                AdapterObservation {
                    receipt: Some(receipt),
                    diagnostic: None,
                },
            ),
            Err(diagnostic) => (
                Err(
                    if matches!(
                        diagnostic,
                        AdapterDiagnostic::Unavailable
                            | AdapterDiagnostic::Cancelled
                            | AdapterDiagnostic::DeadlineExceeded
                    ) {
                        InvocationAttemptFailure::Transient
                    } else {
                        InvocationAttemptFailure::Hard
                    },
                ),
                None,
                AdapterObservation {
                    receipt: None,
                    diagnostic: Some(diagnostic),
                },
            ),
        };
        *self.observation.lock().expect("observation") = Some(observation);
        InvocationAttemptResponse { output, usage }
    }
}
fn check_context(r: &BoundedInvocationRequest<'_>) -> Result<(), AdapterDiagnostic> {
    r.context.check().map_err(|e| match e {
        gateway_application::codex::FacadeError::Cancelled => AdapterDiagnostic::Cancelled,
        _ => AdapterDiagnostic::DeadlineExceeded,
    })
}
fn quoted(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
fn http(
    adapter: &HttpModelAdapter,
    r: &BoundedInvocationRequest<'_>,
    body: &Value,
) -> Result<Vec<u8>, AdapterDiagnostic> {
    use AdapterDiagnostic as D;
    check_context(r)?;
    // Disable user curlrc, redirects, retries and ambient credentials/proxies.
    let mut child = Command::new("curl")
        .args([
            "-q",
            "--silent",
            "--globoff",
            "--fail",
            "--write-out",
            "\n%{http_code}",
            "--proto",
            "=http,https",
            "--config",
            "-",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| D::Unavailable)?;
    let mut config = format!(
        "url = \"{}{}\"\nheader = \"Content-Type: application/json\"\ndata-binary = \"{}\"\nmax-time = {}\n",
        adapter.endpoint,
        adapter.kind.path(),
        quoted(&body.to_string()),
        r.context
            .deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_secs_f64()
            .max(0.001)
    );
    if let Some(secret) = &adapter.credential {
        config.push_str(&format!(
            "header = \"Authorization: Bearer {}\"\n",
            quoted(secret)
        ));
    }
    // Bounded readers avoid an unbounded wait_with_output allocation. Scoped
    // threads drain both pipes while the owner polls cancellation and reaps curl.
    let ceiling = r.max_output_bytes.saturating_mul(6).saturating_add(16384);
    thread::scope(|scope| {
        let mut stdin = child.stdin.take().expect("piped");
        let writer = scope.spawn(move || stdin.write_all(config.as_bytes()));
        let stdout = child.stdout.take().expect("piped");
        let reader = scope.spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .take(ceiling as u64 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let status = loop {
            if let Err(error) = check_context(r) {
                let _ = child.kill();
                let _ = child.wait();
                break Err(error);
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => thread::sleep(Duration::from_millis(5)),
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(D::Unavailable);
                }
            }
        };
        let written = writer.join().expect("writer");
        let bytes = reader.join().expect("reader").map_err(|_| D::Unavailable)?;
        if bytes.len() > ceiling {
            return Err(D::OutputLimit);
        }
        let status = status?;
        written.map_err(|_| D::Unavailable)?;
        let split = bytes
            .iter()
            .rposition(|b| *b == b'\n')
            .ok_or(D::InvalidResponse)?;
        let code = std::str::from_utf8(&bytes[split + 1..])
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or(D::InvalidResponse)?;
        if !status.success() || !(200..300).contains(&code) {
            return Err(if code == 429 || code >= 500 {
                D::Unavailable
            } else if status.code() == Some(22) || code >= 300 {
                D::ProviderRejected
            } else if status.code() == Some(28) {
                D::DeadlineExceeded
            } else {
                D::Unavailable
            });
        }
        let bytes = bytes[..split].to_vec();
        Ok(bytes)
    })
}
