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
}

impl ToolDefinition {
    /// Build the model-visible closed schema from an operator-mapped MCP tool
    /// schema. The semantic digest must match the normalized wire schema; the
    /// full inventory revision remains pinned separately to catch all drift.
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
        let actual_digest = context_content_digest(
            &serde_json::to_vec(&input.json_schema())
                .map_err(|_| InvocationIrError::InvalidBinding)?,
        );
        if actual_digest != argument_schema_digest {
            return Err(InvocationIrError::InvalidBinding);
        }
        Ok(Self {
            binding,
            description,
            input,
            connector_inventory: Some(ConnectorToolInventoryBinding {
                connector: target.connector,
                scope: target.scope,
                revision: inventory_revision,
                argument_schema_digest,
            }),
        })
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

fn parse_connector_input_schema(schema: &Value) -> Result<OutputType, InvocationIrError> {
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
mod connector_schema_tests {
    use super::*;

    fn binding(scope: &ContextScopeId) -> ResolvedReference<CapabilityId> {
        ResolvedReference {
            id: CapabilityId::new("connector.records.read").unwrap(),
            scope: scope.clone(),
            contract: ReferenceId::new("connector-capability").unwrap(),
            contract_version: SchemaVersion::V1,
            revision: ReferenceId::new("1").unwrap(),
            digest: ContentDigest::new("a".repeat(64)).unwrap(),
        }
    }

    #[test]
    fn mapped_connector_schema_becomes_closed_invocation_schema_with_inventory_pin() {
        let scope = ContextScopeId::new("project").unwrap();
        let target = crate::connectors::ConnectorTarget {
            connector: NonEmptyText::new("connector").unwrap(),
            scope: scope.clone(),
        };
        let schema = json!({
            "$schema":"https://json-schema.org/draft/2020-12/schema",
            "title":"Read records",
            "type":"object",
            "description":"Query the connector",
            "properties":{
                "query":{"type":"string","description":"Search phrase"},
                "limit":{"type":"integer"}
            },
            "required":["query"],
            "additionalProperties":false
        });
        let input = parse_connector_input_schema(&schema).unwrap();
        let schema_digest =
            context_content_digest(&serde_json::to_vec(&input.json_schema()).unwrap());
        let definition = ToolDefinition::from_connector_schema(
            binding(&scope),
            NonEmptyText::new("Read records").unwrap(),
            target,
            ContentDigest::new("b".repeat(64)).unwrap(),
            schema_digest.clone(),
            &schema,
        )
        .unwrap();
        assert_eq!(definition.input, input);
        let pin = definition.connector_inventory.unwrap();
        assert_eq!(pin.revision.as_str(), "b".repeat(64));
        assert_eq!(pin.argument_schema_digest, schema_digest);
    }

    #[test]
    fn connector_schema_conversion_rejects_semantic_drift_and_unsupported_keywords() {
        let scope = ContextScopeId::new("project").unwrap();
        let target = crate::connectors::ConnectorTarget {
            connector: NonEmptyText::new("connector").unwrap(),
            scope: scope.clone(),
        };
        let schema = json!({
            "type":"object", "properties":{"query":{"type":"string"}},
            "required":["query"], "additionalProperties":false
        });
        assert_eq!(
            ToolDefinition::from_connector_schema(
                binding(&scope),
                NonEmptyText::new("Read records").unwrap(),
                target.clone(),
                ContentDigest::new("b".repeat(64)).unwrap(),
                ContentDigest::new("c".repeat(64)).unwrap(),
                &schema,
            )
            .unwrap_err(),
            InvocationIrError::InvalidBinding
        );
        let unsupported = json!({
            "type":"object", "properties":{"query":{"type":"string","enum":["a"]}},
            "additionalProperties":false
        });
        let digest = context_content_digest(
            &serde_json::to_vec(&json!({
                "type":"object", "properties":{"query":{"type":"string"}},
                "required":[], "additionalProperties":false
            }))
            .unwrap(),
        );
        assert_eq!(
            ToolDefinition::from_connector_schema(
                binding(&scope),
                NonEmptyText::new("Read records").unwrap(),
                target,
                ContentDigest::new("b".repeat(64)).unwrap(),
                digest,
                &unsupported,
            )
            .unwrap_err(),
            InvocationIrError::InvalidBinding
        );
    }

    #[test]
    fn connector_schema_validation_rejects_scope_shape_required_and_property_drift() {
        let scope = ContextScopeId::new("project").unwrap();
        let other_scope = ContextScopeId::new("other-project").unwrap();
        let target = crate::connectors::ConnectorTarget {
            connector: NonEmptyText::new("connector").unwrap(),
            scope: other_scope,
        };
        let boolean_schema = json!({
            "type":"object",
            "properties":{"enabled":{"type":"boolean"}},
            "required":["enabled"],
            "additionalProperties":false
        });
        let boolean_input = parse_connector_input_schema(&boolean_schema).unwrap();
        let boolean_digest =
            context_content_digest(&serde_json::to_vec(&boolean_input.json_schema()).unwrap());
        assert_eq!(
            ToolDefinition::from_connector_schema(
                binding(&scope),
                NonEmptyText::new("Toggle records").unwrap(),
                target.clone(),
                ContentDigest::new("b".repeat(64)).unwrap(),
                boolean_digest.clone(),
                &boolean_schema,
            )
            .unwrap_err(),
            InvocationIrError::InvalidBinding
        );

        let target = crate::connectors::ConnectorTarget {
            connector: NonEmptyText::new("connector").unwrap(),
            scope: scope.clone(),
        };
        let definition = ToolDefinition::from_connector_schema(
            binding(&scope),
            NonEmptyText::new("Toggle records").unwrap(),
            target,
            ContentDigest::new("b".repeat(64)).unwrap(),
            boolean_digest,
            &boolean_schema,
        )
        .unwrap();
        assert_eq!(
            definition.input.json_schema()["properties"]["enabled"]["type"],
            "boolean"
        );

        for invalid in [
            json!({"type":"array","items":{"type":"string"}}),
            json!({"type":"object","properties":{"query":{"type":"string"}},"required":["missing"],"additionalProperties":false}),
            json!({"type":"object","properties":{"":{"type":"string"}},"additionalProperties":false}),
            json!({"type":"object","properties":{"query":{"type":"array"}},"additionalProperties":false}),
        ] {
            assert_eq!(
                parse_connector_input_schema(&invalid),
                Err(InvocationIrError::InvalidBinding),
                "schema should be rejected: {invalid}"
            );
        }
    }
}
