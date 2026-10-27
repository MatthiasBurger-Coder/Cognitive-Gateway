//! Optional HTTP transports for admitted, bounded model invocations. No SDK,
//! credentials, provider errors or raw response bodies enter canonical records.
use crate::invocation_host::session_tools::{
    MAX_SESSION_TOOL_CALLS, ProviderFinish, ProviderTokenUsage, ProviderToolCall,
    ProviderToolCompletion, ProviderToolFailure, SessionToolProviderPort,
    validate_invocation_tool_pins,
};
use gateway_application::{
    invocation_resilience::{
        BoundedInvocationPort, BoundedInvocationRequest, InvocationAttemptResponse,
    },
    model_invocation::PreparedModelInvocation,
    prompt_rendering::{PromptRendererPort, RenderInput, ToolDefinition, render_data},
};
use gateway_context::PlannedContextHandoff;
use gateway_domain::{
    CapabilityId, ReferenceId, ResolvedReference, SchemaVersion, context_content_digest,
    invocation_resilience::{InvocationAttemptFailure, ProviderUsage},
    model_invocation::{
        EffectiveGenerationRecord, GenerationControl, GenerationValue, StructuredOutputMode,
    },
    prompt_rendering::{RenderDiagnostic, TokenizerProfile},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
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
    ChatCompletionsLegacyV1,
}
impl AdapterKind {
    fn renderer(self) -> &'static str {
        match self {
            Self::LocalServiceV1 => "local-invocation",
            Self::ChatCompletionsV1 => "chat-completions",
            Self::ChatCompletionsLegacyV1 => "chat-completions-legacy",
        }
    }
    fn path(self) -> &'static str {
        match self {
            Self::LocalServiceV1 => "/v1/invoke",
            Self::ChatCompletionsV1 | Self::ChatCompletionsLegacyV1 => "/v1/chat/completions",
        }
    }
}
/// Renders the entire wire body, including generation and schema framing, before
/// token accounting. The model ID is an immutable runtime snapshot, never an alias.
pub struct AdapterRenderer {
    kind: AdapterKind,
    prepared: PreparedModelInvocation,
    runtime_model: Option<RuntimeModelBinding>,
    task_directive: bool,
    registered_role: bool,
}
/// Adapter-owned mapping from a canonical immutable pin to a provider wire name.
/// Vendor namespaces/tags never enter canonical ReferenceId contracts.
#[derive(Debug, Clone)]
pub struct RuntimeModelBinding {
    pub canonical: ResolvedReference<ReferenceId>,
    pub runtime_id: String,
}
impl RuntimeModelBinding {
    fn validate(&self) -> Result<(), AdapterDiagnostic> {
        if gateway_domain::model_invocation::validate_model_binding(&self.canonical).is_err()
            || self.runtime_id.is_empty()
            || self.runtime_id.len() > 256
            || !self
                .runtime_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-/:".contains(&c))
        {
            return Err(AdapterDiagnostic::InvalidBinding);
        }
        Ok(())
    }
}
impl AdapterRenderer {
    pub fn new(
        kind: AdapterKind,
        prepared: PreparedModelInvocation,
    ) -> Result<Self, AdapterDiagnostic> {
        controls(kind, &prepared)?;
        Ok(Self {
            kind,
            prepared,
            runtime_model: None,
            task_directive: false,
            registered_role: false,
        })
    }
    pub fn for_host(
        kind: AdapterKind,
        prepared: PreparedModelInvocation,
        binding: Option<RuntimeModelBinding>,
    ) -> Result<Self, AdapterDiagnostic> {
        let mut renderer = match binding {
            Some(binding) => Self::for_runtime(kind, prepared, binding)?,
            None => Self::new(kind, prepared)?,
        };
        renderer.task_directive = true;
        Ok(renderer)
    }
    pub fn for_registered_role(
        kind: AdapterKind,
        prepared: PreparedModelInvocation,
        binding: Option<RuntimeModelBinding>,
    ) -> Result<Self, AdapterDiagnostic> {
        let mut renderer = Self::for_host(kind, prepared, binding)?;
        renderer.registered_role = true;
        Ok(renderer)
    }
    pub fn for_runtime(
        kind: AdapterKind,
        prepared: PreparedModelInvocation,
        binding: RuntimeModelBinding,
    ) -> Result<Self, AdapterDiagnostic> {
        binding.validate()?;
        if prepared.capability().data().model != binding.canonical {
            return Err(AdapterDiagnostic::InvalidBinding);
        }
        let mut renderer = Self::new(kind, prepared)?;
        renderer.runtime_model = Some(binding);
        Ok(renderer)
    }
}
impl PromptRendererPort for AdapterRenderer {
    fn identity(&self) -> (ReferenceId, ReferenceId) {
        (
            ReferenceId::new(self.kind.renderer()).expect("static"),
            ReferenceId::new(if self.registered_role {
                "9"
            } else if self.task_directive {
                "8"
            } else if self.runtime_model.is_some() {
                "2"
            } else {
                "1"
            })
            .expect("static"),
        )
    }
    fn render(&self, input: &RenderInput) -> Result<Value, RenderDiagnostic> {
        let semantic = if self.registered_role {
            crate::prompt_renderers::SemanticRenderer::FileRequestExtractionV1
        } else if self.task_directive {
            crate::prompt_renderers::SemanticRenderer::OperatorTaskV3
        } else {
            crate::prompt_renderers::SemanticRenderer::ChatMessagesV1
        };
        let messages = semantic.render(input)?;
        wire(
            self.kind,
            &self.prepared,
            messages["messages"].clone(),
            self.runtime_model.as_ref(),
        )
        .map_err(|_| RenderDiagnostic::RenderingFailed)
    }
}

/// Tool-capable rendering is a distinct format/profile from the result-only
/// AdapterRenderer. It binds the exact admitted schema and connector inventory
/// into the request before token accounting.
pub struct SessionToolHttpRenderer {
    kind: AdapterKind,
    prepared: PreparedModelInvocation,
    handoff: PlannedContextHandoff,
    tools: BTreeMap<CapabilityId, ToolDefinition>,
    runtime_model: Option<RuntimeModelBinding>,
    wire_mode: SessionToolWireMode,
}

/// Provider function names are transport aliases, never capability identities.
/// PortableV1 also preserves optional arguments by disabling provider strict
/// decoding where the admitted native schema cannot express its strict subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionToolWireMode {
    CanonicalV1,
    PortableV1,
}

impl SessionToolWireMode {
    fn template_version(self, runtime_model: bool) -> &'static str {
        match (self, runtime_model) {
            (Self::CanonicalV1, false) => "1",
            (Self::CanonicalV1, true) => "2",
            (Self::PortableV1, false) => "3",
            (Self::PortableV1, true) => "4",
        }
    }

    fn names(
        self,
        tools: &BTreeMap<CapabilityId, ToolDefinition>,
    ) -> Result<BTreeMap<String, CapabilityId>, AdapterDiagnostic> {
        let mut names = BTreeMap::new();
        for (id, tool) in tools {
            let name = match self {
                Self::CanonicalV1 => id.as_str().to_owned(),
                Self::PortableV1 => {
                    let bytes =
                        serde_json::to_vec(tool).map_err(|_| AdapterDiagnostic::InvalidBinding)?;
                    let digest = context_content_digest(&bytes);
                    format!("cg_tool_{}", &digest.as_str()[..56])
                }
            };
            if names.insert(name, id.clone()).is_some() {
                return Err(AdapterDiagnostic::InvalidBinding);
            }
        }
        Ok(names)
    }
}

impl SessionToolHttpRenderer {
    pub fn new(
        kind: AdapterKind,
        prepared: PreparedModelInvocation,
        handoff: PlannedContextHandoff,
        tools: BTreeMap<CapabilityId, ToolDefinition>,
    ) -> Result<Self, AdapterDiagnostic> {
        let data = prepared.invocation().data();
        if data.generation.data().structured_output_mode != StructuredOutputMode::ToolProposal
            || tools.is_empty()
            || tools.len() != data.permitted_capabilities.len()
            || tools.iter().any(|(id, tool)| {
                let Some(pin) = &tool.connector_inventory else {
                    return true;
                };
                id != &tool.binding.id
                    || data.permitted_capabilities.get(id) != Some(&tool.binding)
                    || tool.binding.scope != data.task.scope
                    || pin.scope != data.task.scope
                    || serde_json::to_vec(&tool.input.json_schema())
                        .ok()
                        .map(|bytes| context_content_digest(&bytes))
                        != Some(pin.argument_schema_digest.clone())
            })
        {
            return Err(AdapterDiagnostic::CapabilityMismatch);
        }
        Ok(Self {
            kind,
            prepared,
            handoff,
            tools,
            runtime_model: None,
            wire_mode: SessionToolWireMode::CanonicalV1,
        })
    }

    /// Must match the adapter and the producer's pinned tokenizer profile.
    pub fn with_wire_mode(mut self, mode: SessionToolWireMode) -> Self {
        self.wire_mode = mode;
        self
    }

    pub fn for_runtime(
        kind: AdapterKind,
        prepared: PreparedModelInvocation,
        handoff: PlannedContextHandoff,
        tools: BTreeMap<CapabilityId, ToolDefinition>,
        binding: RuntimeModelBinding,
    ) -> Result<Self, AdapterDiagnostic> {
        binding.validate()?;
        if prepared.capability().data().model != binding.canonical {
            return Err(AdapterDiagnostic::InvalidBinding);
        }
        let mut renderer = Self::new(kind, prepared, handoff, tools)?;
        renderer.runtime_model = Some(binding);
        Ok(renderer)
    }
}

impl PromptRendererPort for SessionToolHttpRenderer {
    fn identity(&self) -> (ReferenceId, ReferenceId) {
        (
            ReferenceId::new(format!("{}-session-tools", self.kind.renderer())).expect("id"),
            ReferenceId::new(
                self.wire_mode
                    .template_version(self.runtime_model.is_some()),
            )
            .expect("version"),
        )
    }

    fn render(&self, input: &RenderInput) -> Result<Value, RenderDiagnostic> {
        if input.handoff() != &self.handoff.to_value() || input.tools() != &self.tools {
            return Err(RenderDiagnostic::InvalidBinding);
        }
        if self.kind == AdapterKind::LocalServiceV1 {
            return Err(RenderDiagnostic::UnsupportedRenderer);
        }
        let messages = json!([
            {"role":"system","content":"Connector calls are proposals. Use only the exact provided functions. Gateway validates proposals before dispatch."},
            {"role":"user","content":render_data(input).to_string()}
        ]);
        let body = tool_wire(
            self.kind,
            &self.prepared,
            &messages,
            &self.tools,
            self.runtime_model.as_ref(),
            self.wire_mode,
        )
        .map_err(|_| RenderDiagnostic::RenderingFailed)?;
        Ok(body)
    }
}

/// Explicit response interpretation for model tool proposals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionToolResponseMode {
    JsonOrText,
    /// Empty provider text remains empty text, alongside actual tool proposals.
    /// This mode supplies no success claim and supports only String outputs.
    ToolProposalTextV1,
}
impl SessionToolResponseMode {
    fn accepts_shape(self, shape: &gateway_domain::semantic_output::OutputType) -> bool {
        self == Self::JsonOrText
            || matches!(
                shape,
                gateway_domain::semantic_output::OutputType::String {}
            )
    }

    fn result(
        self,
        content: Option<&Value>,
        has_calls: bool,
    ) -> Result<Value, ProviderToolFailure> {
        match self {
            Self::JsonOrText => Ok(match content.and_then(Value::as_str) {
                Some(s) if !s.is_empty() => {
                    match crate::local_mcp::decode::decode(s.as_bytes()) {
                        Ok(value) => value,
                        // The strict decoder's duplicate/depth violations are
                        // data errors, never permission to reinterpret JSON as text.
                        Err(error) if error.is_data() => return Err(ProviderToolFailure::Hard),
                        Err(_) => Value::String(s.into()),
                    }
                }
                _ => Value::Null,
            }),
            Self::ToolProposalTextV1 if has_calls => match content {
                None | Some(Value::Null) => Ok(Value::String(String::new())),
                Some(Value::String(text)) => Ok(Value::String(text.clone())),
                _ => Err(ProviderToolFailure::Hard),
            },
            Self::ToolProposalTextV1 => Err(ProviderToolFailure::Hard),
        }
    }
}

/// Separate HTTP provider for connector-session proposals. It shares the
/// hardened curl transport, but its response mapping and renderer are isolated
/// from the result-only HttpModelAdapter.
pub struct SessionToolHttpAdapter {
    kind: AdapterKind,
    endpoint: String,
    credential: Option<String>,
    loopback: bool,
    runtime_model: Option<RuntimeModelBinding>,
    response_mode: SessionToolResponseMode,
    wire_mode: SessionToolWireMode,
}

impl SessionToolHttpAdapter {
    pub fn new(
        kind: AdapterKind,
        endpoint: String,
        credential: Option<String>,
        binding: Option<RuntimeModelBinding>,
    ) -> Result<Self, AdapterDiagnostic> {
        let base = HttpModelAdapter::new(kind, endpoint, credential)?;
        if let Some(binding) = &binding {
            binding.validate()?;
        }
        Ok(Self {
            kind,
            endpoint: base.endpoint,
            credential: base.credential,
            loopback: base.loopback,
            runtime_model: binding,
            response_mode: SessionToolResponseMode::JsonOrText,
            wire_mode: SessionToolWireMode::CanonicalV1,
        })
    }

    /// Explicit operator-selected response interpretation. The owning producer
    /// definition must bind this mode; it never changes the result-only adapter.
    pub fn with_response_mode(mut self, mode: SessionToolResponseMode) -> Self {
        self.response_mode = mode;
        self
    }

    /// Must match the renderer's exact versioned wire profile.
    pub fn with_wire_mode(mut self, mode: SessionToolWireMode) -> Self {
        self.wire_mode = mode;
        self
    }
}

impl SessionToolProviderPort for SessionToolHttpAdapter {
    fn complete(
        &self,
        r: BoundedInvocationRequest<'_>,
    ) -> Result<ProviderToolCompletion, ProviderToolFailure> {
        use AdapterDiagnostic as D;
        let fail = |d| {
            if matches!(d, D::Unavailable | D::Cancelled | D::DeadlineExceeded) {
                ProviderToolFailure::Transient
            } else {
                ProviderToolFailure::Hard
            }
        };
        check_context(&r).map_err(fail)?;
        let p = r.ir.prepared();
        if r.max_output_bytes == 0
            || r.max_output_bytes > gateway_domain::output_parsing::MAX_OUTPUT_BYTES
            || p.effective_generation().structured_output_mode != StructuredOutputMode::ToolProposal
            || !self
                .response_mode
                .accepts_shape(&p.invocation().data().output.shape)
            || self
                .runtime_model
                .as_ref()
                .is_some_and(|b| b.canonical != p.capability().data().model)
            || (p.provider().data().locality
                == gateway_domain::cognitive_routing::PrivacyBoundary::OnDevice
                && !self.loopback)
            || r.binding.invocation != r.ir.identity()
            || r.rendered.identity() != &r.ir.identity()
        {
            return Err(ProviderToolFailure::Hard);
        }
        validate_invocation_tool_pins(r.ir).map_err(|_| ProviderToolFailure::Hard)?;
        let a = r.rendered.rendered().accounting();
        if a.count.profile.renderer.as_str() != format!("{}-session-tools", self.kind.renderer())
            || a.count.profile.template_version.as_str()
                != self
                    .wire_mode
                    .template_version(self.runtime_model.is_some())
            || a.count.profile.model != p.capability().data().model
            || a.count.profile.generation_profile
                != p.invocation().data().generation.content_digest()
            || a.count.payload_digest != r.binding.payload_digest
            || a.count.profile != r.binding.profile
        {
            return Err(ProviderToolFailure::Hard);
        }
        let body: Value = serde_json::from_str(r.rendered.rendered().payload())
            .map_err(|_| ProviderToolFailure::Hard)?;
        let expected = tool_wire(
            self.kind,
            p,
            &body["messages"],
            r.ir.prompt().tools(),
            self.runtime_model.as_ref(),
            self.wire_mode,
        )
        .map_err(|_| ProviderToolFailure::Hard)?;
        if body != expected {
            return Err(ProviderToolFailure::Hard);
        }
        let transport = HttpModelAdapter {
            kind: self.kind,
            endpoint: self.endpoint.clone(),
            credential: self.credential.clone(),
            observation: Mutex::new(None),
            loopback: self.loopback,
            runtime_model: self.runtime_model.clone(),
            task_directive: false,
            registered_role: false,
        };
        let raw = http(&transport, &r, &body).map_err(fail)?;
        if self
            .credential
            .as_ref()
            .is_some_and(|s| raw.windows(s.len()).any(|w| w == s.as_bytes()))
        {
            return Err(ProviderToolFailure::Hard);
        }
        let response =
            crate::local_mcp::decode::decode(&raw).map_err(|_| ProviderToolFailure::Hard)?;
        if contains_credential(&response, self.credential.as_deref()) {
            return Err(ProviderToolFailure::Hard);
        }
        if self.kind == AdapterKind::LocalServiceV1 {
            return Err(ProviderToolFailure::Hard);
        }
        let expected_id = self
            .runtime_model
            .as_ref()
            .map_or(p.capability().data().model.id.as_str(), |b| {
                b.runtime_id.as_str()
            });
        if response["model"].as_str() != Some(expected_id)
            || response["choices"].as_array().is_none_or(|v| v.len() != 1)
        {
            return Err(ProviderToolFailure::Hard);
        }
        let choice = &response["choices"][0];
        let message = &choice["message"];
        if !message["refusal"].is_null() {
            return Err(ProviderToolFailure::Hard);
        }
        let finish = match choice["finish_reason"].as_str() {
            Some("tool_calls") => ProviderFinish::Complete,
            Some("stop") if message["tool_calls"].is_null() => ProviderFinish::Complete,
            _ => ProviderFinish::Incomplete,
        };
        if finish != ProviderFinish::Complete {
            return Err(ProviderToolFailure::Hard);
        }
        let mut calls = Vec::new();
        let wire_names = self.wire_mode.names(r.ir.prompt().tools()).map_err(fail)?;
        if let Some(native_calls) = message["tool_calls"].as_array() {
            if native_calls.len() > MAX_SESSION_TOOL_CALLS {
                return Err(ProviderToolFailure::Hard);
            }
            for call in native_calls {
                if call["type"] != "function" {
                    return Err(ProviderToolFailure::Hard);
                }
                let name = call["function"]["name"]
                    .as_str()
                    .ok_or(ProviderToolFailure::Hard)?;
                let arguments = call["function"]["arguments"]
                    .as_str()
                    .ok_or(ProviderToolFailure::Hard)?;
                let args = crate::local_mcp::decode::decode(arguments.as_bytes())
                    .map_err(|_| ProviderToolFailure::Hard)?;
                if contains_credential(&args, self.credential.as_deref()) {
                    return Err(ProviderToolFailure::Hard);
                }
                let canonical_name = if self.wire_mode == SessionToolWireMode::PortableV1 {
                    wire_names
                        .get(name)
                        .ok_or(ProviderToolFailure::Hard)?
                        .as_str()
                } else {
                    name
                };
                calls.push(ProviderToolCall {
                    name: canonical_name.into(),
                    arguments: args,
                });
            }
        }
        let result = self
            .response_mode
            .result(message.get("content"), !calls.is_empty())?;
        if contains_credential(&result, self.credential.as_deref()) {
            return Err(ProviderToolFailure::Hard);
        }
        let usage = if response["usage"].is_null() {
            None
        } else {
            Some(ProviderTokenUsage {
                input_tokens: response["usage"]["prompt_tokens"]
                    .as_u64()
                    .ok_or(ProviderToolFailure::Hard)?,
                output_tokens: response["usage"]["completion_tokens"]
                    .as_u64()
                    .ok_or(ProviderToolFailure::Hard)?,
            })
        };
        Ok(ProviderToolCompletion {
            // The provider wire name was checked above; the generic invocation
            // boundary consumes only the admitted canonical model identity.
            model_id: p.capability().data().model.id.as_str().into(),
            finish,
            result,
            calls,
            usage,
        })
    }
}
fn controls(kind: AdapterKind, p: &PreparedModelInvocation) -> Result<Value, AdapterDiagnostic> {
    let g = p.effective_generation();
    if !matches!(
        g.structured_output_mode,
        StructuredOutputMode::Json | StructuredOutputMode::JsonSchema
    ) || !p.invocation().data().permitted_capabilities.is_empty()
        || g.unexposed_defaults.contains(&GenerationControl::TopK)
            && kind != AdapterKind::LocalServiceV1
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
    runtime_model: Option<&RuntimeModelBinding>,
) -> Result<Value, AdapterDiagnostic> {
    let options = controls(kind, p)?;
    let d = p.invocation().data();
    let model = &p.capability().data().model;
    let model_id = runtime_model.map_or(model.id.as_str(), |m| m.runtime_id.as_str());
    let g = p.effective_generation();
    Ok(match kind {
        AdapterKind::LocalServiceV1 => json!({"schema_version":"1.0", "model_id":model_id,
            "artifact_digest":format!("sha256:{}",model.digest.as_str()), "model_version":model.revision,
            "input_contract":d.input_contract, "output_contract":d.output.schema.id,
            "messages":messages, "options":options, "max_output_tokens":g.max_output_tokens,
            "context_limit":p.capability().data().context_limit,
            "output_schema":d.output.json_schema(), "structured_output_mode":g.structured_output_mode}),
        AdapterKind::ChatCompletionsV1 | AdapterKind::ChatCompletionsLegacyV1 => {
            let mut body = options;
            body["model"] = json!(model_id);
            body["messages"] = messages;
            body[if kind == AdapterKind::ChatCompletionsLegacyV1 {
                "max_tokens"
            } else {
                "max_completion_tokens"
            }] = json!(g.max_output_tokens);
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
fn tool_wire(
    kind: AdapterKind,
    p: &PreparedModelInvocation,
    messages: &Value,
    tools: &BTreeMap<CapabilityId, ToolDefinition>,
    runtime_model: Option<&RuntimeModelBinding>,
    wire_mode: SessionToolWireMode,
) -> Result<Value, AdapterDiagnostic> {
    if kind == AdapterKind::LocalServiceV1 {
        return Err(AdapterDiagnostic::CapabilityMismatch);
    }
    let mut options = json!({});
    for (key, value) in &p.effective_generation().controls {
        let name = match key {
            GenerationControl::Temperature => "temperature",
            GenerationControl::TopP => "top_p",
            GenerationControl::Seed => "seed",
            GenerationControl::StopConditions => "stop",
            _ => return Err(AdapterDiagnostic::CapabilityMismatch),
        };
        options[name] = match &value.value {
            GenerationValue::Scalar(v) => json!(v),
            GenerationValue::Integer(v) => json!(v),
            GenerationValue::Stops(v) => json!(v),
        };
    }
    let mut body = options;
    let model = &p.capability().data().model;
    body["model"] = json!(runtime_model.map_or(model.id.as_str(), |m| m.runtime_id.as_str()));
    body["messages"] = messages.clone();
    body[if kind == AdapterKind::ChatCompletionsLegacyV1 {
        "max_tokens"
    } else {
        "max_completion_tokens"
    }] = json!(p.effective_generation().max_output_tokens);
    body["stream"] = json!(false);
    let names = wire_mode.names(tools)?;
    body["tools"] = Value::Array(
        names
            .iter()
            .map(|(name, id)| {
                let tool = &tools[id];
                let strict =
                    wire_mode == SessionToolWireMode::CanonicalV1 || strict_tool_shape(&tool.input);
                json!({
                    "type":"function",
                    "function":{"name":name,"description":tool.description.as_str(),
                        "parameters":tool.input.json_schema(),"strict":strict}
                })
            })
            .collect(),
    );
    body["tool_choice"] = json!("auto");
    Ok(body)
}

fn strict_tool_shape(shape: &gateway_domain::semantic_output::OutputType) -> bool {
    use gateway_domain::semantic_output::OutputType;
    match shape {
        OutputType::Object { fields } => fields
            .values()
            .all(|field| field.required && strict_tool_shape(&field.value_type)),
        OutputType::Array { items } => strict_tool_shape(items),
        _ => true,
    }
}

// Inspect exactly the JSON values the adapter consumes after each actual
// decoding boundary; escaped JSON strings can bypass a raw byte comparison.
fn contains_credential(value: &Value, credential: Option<&str>) -> bool {
    let Some(secret) = credential else {
        return false;
    };
    match value {
        Value::String(text) => text.contains(secret),
        Value::Array(items) => items
            .iter()
            .any(|item| contains_credential(item, credential)),
        Value::Object(fields) => fields
            .iter()
            .any(|(key, value)| key.contains(secret) || contains_credential(value, credential)),
        _ => false,
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct AdapterReceipt {
    pub schema_version: SchemaVersion,
    pub model: ResolvedReference<ReferenceId>,
    pub provider: ResolvedReference<ReferenceId>,
    pub profile: TokenizerProfile,
    pub generation: EffectiveGenerationRecord,
    pub actual_model_id: String,
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
    runtime_model: Option<RuntimeModelBinding>,
    task_directive: bool,
    registered_role: bool,
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
            runtime_model: None,
            task_directive: false,
            registered_role: false,
        })
    }
    pub fn for_host(
        kind: AdapterKind,
        endpoint: String,
        credential: Option<String>,
        binding: Option<RuntimeModelBinding>,
    ) -> Result<Self, AdapterDiagnostic> {
        let mut adapter = match binding {
            Some(binding) => Self::for_runtime(kind, endpoint, credential, binding)?,
            None => Self::new(kind, endpoint, credential)?,
        };
        adapter.task_directive = true;
        Ok(adapter)
    }
    pub fn for_registered_role(
        kind: AdapterKind,
        endpoint: String,
        credential: Option<String>,
        binding: Option<RuntimeModelBinding>,
    ) -> Result<Self, AdapterDiagnostic> {
        let mut adapter = Self::for_host(kind, endpoint, credential, binding)?;
        adapter.registered_role = true;
        Ok(adapter)
    }
    pub fn for_runtime(
        kind: AdapterKind,
        endpoint: String,
        credential: Option<String>,
        binding: RuntimeModelBinding,
    ) -> Result<Self, AdapterDiagnostic> {
        binding.validate()?;
        let mut adapter = Self::new(kind, endpoint, credential)?;
        adapter.runtime_model = Some(binding);
        Ok(adapter)
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
        if self
            .runtime_model
            .as_ref()
            .is_some_and(|b| b.canonical != p.capability().data().model)
        {
            return Err(D::InvalidBinding);
        }
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
            || a.count.profile.template_version.as_str()
                != if self.registered_role {
                    "9"
                } else if self.task_directive {
                    "8"
                } else if self.runtime_model.is_some() {
                    "2"
                } else {
                    "1"
                }
            || a.count.profile.model != p.capability().data().model
            || a.count.profile.generation_profile
                != p.invocation().data().generation.content_digest()
        {
            return Err(D::InvalidBinding);
        }
        let body: Value =
            serde_json::from_str(r.rendered.rendered().payload()).map_err(|_| D::InvalidBinding)?;
        if !body["messages"].is_array()
            || body
                != wire(
                    self.kind,
                    p,
                    body["messages"].clone(),
                    self.runtime_model.as_ref(),
                )?
        {
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
        let response = crate::local_mcp::decode::decode(&raw).map_err(|_| D::InvalidResponse)?;
        if contains_credential(&response, self.credential.as_deref()) {
            return Err(D::InvalidResponse);
        }
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
            AdapterKind::ChatCompletionsV1 | AdapterKind::ChatCompletionsLegacyV1 => {
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
        let expected_id = self
            .runtime_model
            .as_ref()
            .map_or(p.capability().data().model.id.as_str(), |b| {
                b.runtime_id.as_str()
            });
        if actual != Some(expected_id) {
            return Err(D::IdentityMismatch);
        }
        let content = content.ok_or(D::InvalidResponse)?;
        if serde_json::from_str::<Value>(content)
            .is_ok_and(|value| contains_credential(&value, self.credential.as_deref()))
        {
            return Err(D::InvalidResponse);
        }
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
                actual_model_id: expected_id.into(),
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
    let mut command = Command::new("curl");
    command
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
        .stderr(Stdio::null());
    // Windows sockets need the system directory even for literal loopback
    // addresses. Retain only this OS prerequisite, never ambient credentials
    // or proxy settings, in the otherwise cleared transport environment.
    #[cfg(windows)]
    command.env(
        "SystemRoot",
        std::env::var_os("SystemRoot").ok_or(D::Unavailable)?,
    );
    let mut child = command.spawn().map_err(|_| D::Unavailable)?;
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
