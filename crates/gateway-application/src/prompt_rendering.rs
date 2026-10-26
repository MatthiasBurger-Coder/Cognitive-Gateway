//! Pure #264 orchestration. Current Process/Policy authentication remains #263's
//! host responsibility. Rendering does not authorize or dispatch an invocation.
use crate::model_invocation::PreparedModelInvocation;
use gateway_context::PlannedContextHandoff;
use gateway_domain::prompt_ir::InvocationIrError;
use gateway_domain::semantic_output::{OutputField, OutputType};
use gateway_domain::{model_invocation::validate_model_binding, prompt_rendering::*, *};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolDefinition {
    pub binding: ResolvedReference<CapabilityId>,
    pub description: NonEmptyText,
    pub input: OutputType,
    /// Exact operator-mapped connector inventory used to admit this tool schema.
    /// Connector dispatch rejects missing or drifted inventory bindings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector_inventory: Option<ConnectorToolInventoryBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectorToolInventoryBinding {
    pub connector: NonEmptyText,
    pub scope: ContextScopeId,
    pub revision: ContentDigest,
    pub argument_schema_digest: ContentDigest,
    /// Complete mapped wire schema, including descriptive metadata. Its content
    /// identity is distinct from the parsed model argument type.
    pub argument_schema: Value,
}

impl ToolDefinition {
    /// Build the model-visible closed schema from an operator-mapped MCP tool
    /// schema. Preserve its exact content pin while validating the supported
    /// argument semantics; the inventory revision catches other wire drift.
    pub fn from_connector_schema(
        binding: ResolvedReference<CapabilityId>,
        description: NonEmptyText,
        target: crate::connectors::ConnectorTarget,
        inventory_revision: ContentDigest,
        argument_schema_digest: ContentDigest,
        schema: &Value,
    ) -> Result<Self, InvocationIrError> {
        if target.scope != binding.scope {
            return Err(InvocationIrError::InvalidBinding);
        }
        let input = parse_connector_input_schema(schema)?;
        let tool = Self {
            binding,
            description,
            input,
            connector_inventory: Some(ConnectorToolInventoryBinding {
                connector: target.connector,
                scope: target.scope,
                revision: inventory_revision,
                argument_schema_digest,
                argument_schema: schema.clone(),
            }),
        };
        tool.validate_connector_binding()?;
        Ok(tool)
    }

    /// Shared live admission/binding check. Raw content identity and accepted
    /// argument semantics must both match; metadata never grants permission.
    pub(crate) fn validate_connector_binding(&self) -> Result<(), InvocationIrError> {
        if let Some(pin) = &self.connector_inventory {
            let raw_digest = context_content_digest(
                &serde_json::to_vec(&pin.argument_schema)
                    .map_err(|_| InvocationIrError::InvalidBinding)?,
            );
            if pin.scope != self.binding.scope
                || raw_digest != pin.argument_schema_digest
                || parse_connector_input_schema(&pin.argument_schema)? != self.input
            {
                return Err(InvocationIrError::InvalidBinding);
            }
        }
        Ok(())
    }
}

fn schema_keywords_are_supported(
    schema: &Value,
    allow_object_shape: bool,
) -> Result<(), InvocationIrError> {
    let object = schema
        .as_object()
        .ok_or(InvocationIrError::InvalidBinding)?;
    if object.keys().any(|key| {
        !matches!(key.as_str(), "$schema" | "description" | "title" | "type")
            && !(allow_object_shape
                && matches!(
                    key.as_str(),
                    "properties" | "required" | "additionalProperties"
                ))
    }) || schema
        .get("$schema")
        .is_some_and(|value| value.as_str() != Some("https://json-schema.org/draft/2020-12/schema"))
        || ["description", "title"]
            .iter()
            .any(|key| schema.get(key).is_some_and(|value| !value.is_string()))
    {
        return Err(InvocationIrError::InvalidBinding);
    }
    Ok(())
}

/// Parse the supported closed scalar connector argument schema as data. This
/// neither binds a capability/inventory nor admits an invocation or effect.
pub fn parse_connector_input_schema(schema: &Value) -> Result<OutputType, InvocationIrError> {
    schema_keywords_are_supported(schema, true)?;
    if schema["type"] != "object" || schema["additionalProperties"] != false {
        return Err(InvocationIrError::InvalidBinding);
    }
    let properties = schema["properties"]
        .as_object()
        .ok_or(InvocationIrError::InvalidBinding)?;
    let required = schema
        .get("required")
        .map(|value| value.as_array().ok_or(InvocationIrError::InvalidBinding))
        .transpose()?
        .cloned()
        .unwrap_or_default();
    let mut required_names = std::collections::BTreeSet::new();
    for value in required {
        let name = value.as_str().ok_or(InvocationIrError::InvalidBinding)?;
        if !properties.contains_key(name) || !required_names.insert(name.to_owned()) {
            return Err(InvocationIrError::InvalidBinding);
        }
    }
    let mut fields = BTreeMap::new();
    for (name, property) in properties {
        if name.is_empty() {
            return Err(InvocationIrError::InvalidBinding);
        }
        schema_keywords_are_supported(property, false)?;
        let value_type = match property["type"].as_str() {
            Some("string") => OutputType::String {},
            Some("integer") => OutputType::Integer {},
            Some("boolean") => OutputType::Boolean {},
            _ => return Err(InvocationIrError::InvalidBinding),
        };
        fields.insert(
            name.clone(),
            OutputField {
                required: required_names.contains(name),
                value_type,
            },
        );
    }
    Ok(OutputType::Object { fields })
}
/// Complete immutable canonical sections, not an independently constructible IR.
#[derive(Debug)]
pub struct RenderInput {
    handoff: Value,
    tools: BTreeMap<CapabilityId, ToolDefinition>,
    output_schema: Value,
}
impl RenderInput {
    pub fn handoff(&self) -> &Value {
        &self.handoff
    }
    pub fn tools(&self) -> &BTreeMap<CapabilityId, ToolDefinition> {
        &self.tools
    }
    pub fn output_schema(&self) -> &Value {
        &self.output_schema
    }
}
/// Runtime-specific layouts are implemented only by outer adapters.
pub trait PromptRendererPort {
    fn identity(&self) -> (ReferenceId, ReferenceId);
    fn render(&self, input: &RenderInput) -> Result<Value, RenderDiagnostic>;
}
/// Contains the actual serialized runtime payload including all framing/schemas.
/// Implementations count that payload, not merely the semantic component hints.
pub struct TokenCountRequest<'a> {
    pub profile: &'a TokenizerProfile,
    pub payload: &'a str,
    pub payload_digest: &'a ContentDigest,
    pub sections: &'a RenderInput,
}
pub trait TokenizerPort {
    /// Prefer exact tokenization; estimated results must be conservative upper
    /// bounds for this model/template and carry explicit method provenance.
    fn count(&self, request: &TokenCountRequest<'_>) -> Result<TokenCountResult, RenderDiagnostic>;
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "detail", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RenderError {
    Diagnostic(RenderDiagnostic),
    ContextLimitExceeded(Box<TokenBudgetSnapshot>),
}
impl From<RenderDiagnostic> for RenderError {
    fn from(value: RenderDiagnostic) -> Self {
        Self::Diagnostic(value)
    }
}
/// Only successful binding, rendering and accounting can construct this result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderedInvocation {
    invocation_digest: ContentDigest,
    handoff_digest: ContentDigest,
    payload: String,
    accounting: TokenBudgetSnapshot,
}
impl RenderedInvocation {
    pub fn payload(&self) -> &str {
        &self.payload
    }
    pub fn accounting(&self) -> &TokenBudgetSnapshot {
        &self.accounting
    }
}

pub fn render_invocation(
    handoff: &PlannedContextHandoff,
    prepared: &PreparedModelInvocation,
    tools: BTreeMap<CapabilityId, ToolDefinition>,
    profile: TokenizerProfile,
    renderer: &dyn PromptRendererPort,
    tokenizer: &dyn TokenizerPort,
) -> Result<RenderedInvocation, RenderError> {
    use RenderDiagnostic as D;
    let invocation = prepared.invocation().data();
    let full = handoff.to_value();
    if full["bindings"]["task"] != serde_json::to_value(&invocation.task).expect("pin JSON")
        || full["bindings"]["context"]
            != serde_json::to_value(&invocation.compiled_context).expect("pin JSON")
        || full["output"] != serde_json::to_value(&invocation.output).expect("output JSON")
        || profile.model != prepared.capability().data().model
        || profile.generation_profile != invocation.generation.content_digest()
    {
        return Err(D::InvalidBinding.into());
    }
    if profile.schema_version != PROMPT_RENDERING_VERSION
        || profile.tokenizer_version.as_str() == "latest"
    {
        return Err(D::InvalidBinding.into());
    }
    let (renderer_id, template_version) = renderer.identity();
    if profile.renderer != renderer_id || profile.template_version != template_version {
        return Err(D::UnsupportedRenderer.into());
    }
    // Tool schemas are host-authenticated definitions. Require the entire exact
    // authorized inventory; neither omit a definition nor expose an extra tool.
    if tools.len() != invocation.permitted_capabilities.len()
        || tools
            .iter()
            .any(|(id, tool)| invocation.permitted_capabilities.get(id) != Some(&tool.binding))
    {
        return Err(D::InvalidBinding.into());
    }
    let handoff_digest = context_content_digest(&serde_json::to_vec(&full).expect("handoff JSON"));
    let input = RenderInput {
        handoff: full,
        tools,
        output_schema: invocation.output.json_schema(),
    };
    let payload =
        serde_json::to_string(&renderer.render(&input)?).map_err(|_| D::RenderingFailed)?;
    let payload_digest = context_content_digest(payload.as_bytes());
    let count = tokenizer.count(&TokenCountRequest {
        profile: &profile,
        payload: &payload,
        payload_digest: &payload_digest,
        sections: &input,
    })?;
    if count.profile != profile
        || count.payload_digest != payload_digest
        || validate_model_binding(&count.provenance).is_err()
    {
        return Err(D::InvalidAccounting.into());
    }
    let total_input_tokens = count
        .usage
        .total()
        .filter(|n| *n > 0)
        .ok_or(D::InvalidAccounting)?;
    let reserved_output_tokens = u64::from(prepared.effective_generation().max_output_tokens);
    let total = total_input_tokens
        .checked_add(reserved_output_tokens)
        .ok_or(D::InvalidAccounting)?;
    let accounting = TokenBudgetSnapshot {
        count,
        reserved_output_tokens,
        total_input_tokens,
        context_limit: u64::from(prepared.capability().data().context_limit),
    };
    if total > accounting.context_limit {
        return Err(RenderError::ContextLimitExceeded(Box::new(accounting)));
    }
    Ok(RenderedInvocation {
        invocation_digest: prepared.invocation().content_digest(),
        handoff_digest,
        payload,
        accounting,
    })
}

/// Common lossless semantic grouping for outer renderers. Every handoff section
/// except catalog instructions stays in the non-authority data object.
pub fn render_data(input: &RenderInput) -> Value {
    let mut data = input.handoff.clone();
    data.as_object_mut()
        .expect("canonical handoff object")
        .remove("instructions");
    json!({"semantic":data, "tools":input.tools, "output_schema":input.output_schema})
}

#[cfg(test)]
#[path = "prompt_rendering/connector_schema_tests.rs"]
mod connector_schema_tests;
