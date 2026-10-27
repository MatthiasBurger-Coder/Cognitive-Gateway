//! Private operator-qualified association data, never an execution grant.
//!
//! `cg.connector-object/1.0` is a NEW trusted operator descriptor contract, not
//! a generic native-object entitlement. Its coordinates attest the complete
//! routing identity for the qualified mapping/profile. Task membership is
//! checked independently; knowing another row in the same scope grants nothing.
//! This exact-operands baseline supports no free model payload or query DSL.
use crate::mcp_client::{
    ProtocolResult,
    mapping::{MappedInventory, MappingDocument, MappingRecord},
};
use gateway_application::{
    connector_manifest::InventoryKind, connectors::*, prompt_rendering::ToolDefinition,
    sessions::SessionError,
};
use gateway_domain::{
    CapabilityId, ContextScopeId, NonEmptyText, PlanStepId, ReferenceId, ResolvedReference,
    SchemaVersion, SemanticInputValue, SemanticTaskIR, SubjectPath, TargetKind, TypedValue,
    semantic_output::OutputType,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

type Pin = ResolvedReference<ReferenceId>;
const BYTES: usize = 262_144;
const OPERATIONS: usize = 256;
const FIELDS: usize = 64;
fn invalid() -> SessionError {
    SessionError::InvalidInput
}
fn unsupported() -> SessionError {
    SessionError::UnsupportedCapability
}
fn decode_pinned<T: serde::de::DeserializeOwned>(
    pin: &Pin,
    bytes: &[u8],
    contract: &str,
) -> Result<T, SessionError> {
    if bytes.len() > BYTES {
        return Err(SessionError::LimitExceeded);
    }
    if pin.contract.as_str() != contract || pin.contract_version != SchemaVersion::V1 {
        return Err(unsupported());
    }
    let value = crate::local_mcp::decode::decode(bytes).map_err(|_| invalid())?;
    if !gateway_application::codex::security::credential_free(&value) {
        return Err(SessionError::AuthorityDenied);
    }
    let canonical = serde_json::to_vec(&value).map_err(|_| invalid())?;
    if gateway_domain::context_content_digest(&canonical) != pin.digest {
        return Err(SessionError::StaleRevision);
    }
    serde_json::from_value(value).map_err(|_| invalid())
}
fn scalar(value: &TypedValue) -> bool {
    matches!(value,TypedValue::String(v) if v.as_str().len()<=8192)
        || matches!(value, TypedValue::Integer(_) | TypedValue::Boolean(_))
}
fn names<'a>(values: impl Iterator<Item = &'a String>) -> bool {
    values
        .into_iter()
        .all(|s| !s.is_empty() && s.len() <= 128 && !s.contains('\0'))
}
fn pin_scope(pin: &Pin, scope: &ContextScopeId) -> Result<(), SessionError> {
    if &pin.scope != scope {
        Err(SessionError::ScopeDenied)
    } else if pin.contract_version != SchemaVersion::V1 {
        Err(unsupported())
    } else {
        Ok(())
    }
}

pub(super) struct ScopeObjectDocument {
    pin: Pin,
    body: ObjectDescriptor,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectDescriptor {
    schema_version: SchemaVersion,
    id: ReferenceId,
    revision: ReferenceId,
    scope: ContextScopeId,
    kind: TargetKind,
    connector: NonEmptyText,
    parent_target: Option<Pin>,
    subjects: Vec<SubjectPath>,
    coordinates: BTreeMap<String, TypedValue>,
    object_reference: NonEmptyText,
}
impl ScopeObjectDocument {
    /// Typed document dependencies only; discovering a pin grants no access.
    pub(super) fn reference_bindings(&self) -> Vec<Pin> {
        self.body.parent_target.iter().cloned().collect()
    }
    pub(super) fn from_json(pin: &Pin, bytes: &[u8]) -> Result<Self, SessionError> {
        let body: ObjectDescriptor = decode_pinned(pin, bytes, "cg.connector-object")?;
        if body.schema_version != SchemaVersion::V1
            || body.id != pin.id
            || body.revision != pin.revision
            || body.scope != pin.scope
        {
            return Err(SessionError::StaleRevision);
        }
        if body.coordinates.len() > FIELDS
            || body.subjects.len() > 256
            || body.object_reference.as_str().len() > 8192
            || body.connector.as_str().len() > 128
            || !names(body.coordinates.keys())
            || body.coordinates.values().any(|v| !scalar(v))
            || body.subjects.iter().collect::<BTreeSet<_>>().len() != body.subjects.len()
        {
            return Err(invalid());
        }
        if let Some(parent) = &body.parent_target {
            pin_scope(parent, &pin.scope)?;
        }
        Ok(Self {
            pin: pin.clone(),
            body,
        })
    }
}

pub(super) struct SelectorAssociation {
    pin: Pin,
    body: AssociationDocument,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssociationDocument {
    schema_version: SchemaVersion,
    scope: ContextScopeId,
    task: Pin,
    action_join: Pin,
    action_projection: Pin,
    read_projection: Pin,
    mapping: Pin,
    action_step: String,
    connector: NonEmptyText,
    operations: Vec<Operation>,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Purpose {
    Proposal,
    FixedRead,
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Profile {
    ExactTaskOperandsV1,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
enum Subject {
    TaskTarget,
    InputReference { role: ReferenceId },
    ScopeExport { provider_contract: Pin },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    mapping_reference: String,
    purpose: Purpose,
    profile: Profile,
    subject: Subject,
    object: Pin,
    selector_arguments: BTreeMap<String, String>,
    literal_arguments: BTreeMap<String, ReferenceId>,
    fixed_arguments: BTreeMap<String, TypedValue>,
    proposal_data: Vec<String>,
    response: ResponseIdentity,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseIdentity {
    object_field: String,
    identity_fields: BTreeMap<String, String>,
}

/// Current source-owned captured inputs. The factory must independently retain
/// current session/resource/CG-09/consent authority; these are inspectable data.
pub(super) struct AssociationContext<'a> {
    pub(super) task_pin: &'a Pin,
    pub(super) task: &'a SemanticTaskIR,
    pub(super) action_join: &'a Pin,
    pub(super) action_projection: &'a Pin,
    pub(super) read_projection: &'a Pin,
    pub(super) read_execution: &'a NonEmptyText,
    pub(super) action_step: &'a PlanStepId,
    pub(super) execution: &'a NonEmptyText,
    pub(super) mapping_pin: &'a Pin,
    pub(super) mapping_document: &'a Value,
    pub(super) inventory: &'a MappedInventory,
    pub(super) tools: &'a BTreeMap<CapabilityId, ToolDefinition>,
    pub(super) objects: &'a [ScopeObjectDocument],
    pub(super) scope_export: Option<&'a Pin>,
}
pub(super) struct BoundAssociation {
    target: ConnectorTarget,
    revision: gateway_domain::ContentDigest,
    task: gateway_domain::TaskId,
    step: PlanStepId,
    execution: NonEmptyText,
    read_execution: NonEmptyText,
    operations: BTreeMap<(String, Purpose), BoundOperation>,
}
struct BoundOperation {
    record: MappingRecord,
    expected: BTreeMap<NonEmptyText, TypedValue>,
    object_reference: NonEmptyText,
    response: ResponseIdentity,
    response_values: BTreeMap<String, TypedValue>,
    resource_uri: Option<String>,
    export: Option<Pin>,
}
/// Association match only. It confers no policy approval, credential access,
/// budget, consent, dispatch or retry permission.
pub(super) struct MatchedRequest<'a> {
    operation: &'a BoundOperation,
    request: &'a ConnectorRequest,
}
pub(super) struct FixedReadJoin<'a> {
    pub(super) mapping_reference: &'a str,
    pub(super) source: Option<&'a gateway_domain::SourceId>,
    pub(super) arguments: &'a BTreeMap<NonEmptyText, TypedValue>,
    pub(super) scope_export: Option<&'a Pin>,
}
impl SelectorAssociation {
    /// Typed closure edges, never a generic JSON walk or an authorization result.
    pub(super) fn reference_bindings(&self) -> Vec<Pin> {
        let mut pins = vec![
            self.body.task.clone(),
            self.body.action_join.clone(),
            self.body.action_projection.clone(),
            self.body.read_projection.clone(),
            self.body.mapping.clone(),
        ];
        for operation in &self.body.operations {
            pins.push(operation.object.clone());
            if let Subject::ScopeExport { provider_contract } = &operation.subject {
                pins.push(provider_contract.clone());
            }
        }
        pins
    }
    pub(super) fn from_json(pin: &Pin, bytes: &[u8]) -> Result<Self, SessionError> {
        let body: AssociationDocument =
            decode_pinned(pin, bytes, "cg.connector-selector-association")?;
        if body.schema_version != SchemaVersion::V1 || body.scope != pin.scope {
            return Err(invalid());
        }
        if body.operations.is_empty() || body.operations.len() > OPERATIONS {
            return Err(SessionError::LimitExceeded);
        }
        pin_scope(&body.task, &body.scope)?;
        pin_scope(&body.action_join, &body.scope)?;
        pin_scope(&body.action_projection, &body.scope)?;
        pin_scope(&body.read_projection, &body.scope)?;
        pin_scope(&body.mapping, &body.scope)?;
        if body.task.contract.as_str() != "cg.semantic-task"
            || body.action_join.contract.as_str() != "cg.connector-producer-join"
            || body.mapping.contract.as_str() != "cg.connector-mapping"
            || body.action_projection.contract.as_str() != "cg.context-projection"
            || body.read_projection.contract.as_str() != "cg.context-projection"
        {
            return Err(unsupported());
        }
        PlanStepId::new(body.action_step.clone()).map_err(|_| invalid())?;
        if body.connector.as_str().len() > 128 {
            return Err(invalid());
        }
        let mut keys = BTreeSet::new();
        for op in &body.operations {
            let Profile::ExactTaskOperandsV1 = op.profile;
            if !op.proposal_data.is_empty() {
                return Err(unsupported());
            }
            if !keys.insert((&op.mapping_reference, op.purpose))
                || op.mapping_reference.is_empty()
                || op.mapping_reference.len() > 128
            {
                return Err(invalid());
            }
            pin_scope(&op.object, &body.scope)?;
            if op.object.contract.as_str() != "cg.connector-object" {
                return Err(unsupported());
            }
            if op.selector_arguments.len() + op.literal_arguments.len() + op.fixed_arguments.len()
                > FIELDS
                || op.response.identity_fields.len() > FIELDS
            {
                return Err(SessionError::LimitExceeded);
            }
            let mut fields = BTreeSet::new();
            for key in op
                .selector_arguments
                .keys()
                .chain(op.literal_arguments.keys())
                .chain(op.fixed_arguments.keys())
            {
                if !fields.insert(key) || !names(std::iter::once(key)) {
                    return Err(invalid());
                }
            }
            if !names(op.selector_arguments.values())
                || !names(op.response.identity_fields.keys())
                || !names(op.response.identity_fields.values())
                || op.response.object_field.is_empty()
                || op.response.object_field.len() > 128
                || op
                    .response
                    .identity_fields
                    .contains_key(&op.response.object_field)
                || op.fixed_arguments.values().any(|v| !scalar(v))
            {
                return Err(invalid());
            }
            if let Subject::ScopeExport { provider_contract } = &op.subject {
                pin_scope(provider_contract, &body.scope)?;
                if op.purpose != Purpose::FixedRead
                    || provider_contract.contract.as_str() != "cg.connector-state-universe"
                {
                    return Err(unsupported());
                }
            }
        }
        Ok(Self {
            pin: pin.clone(),
            body,
        })
    }
    pub(super) fn admit(
        &self,
        c: AssociationContext<'_>,
    ) -> Result<BoundAssociation, SessionError> {
        let d = c.task.data();
        let scope = &d.target.reference.scope;
        if self.pin.scope != *scope
            || self.body.scope != *scope
            || self.body.connector != c.inventory.inventory().target.connector
            || c.inventory.inventory().target.scope != *scope
        {
            return Err(SessionError::ScopeDenied);
        }
        if self.body.task != *c.task_pin
            || c.task_pin.contract.as_str() != "cg.semantic-task"
            || c.task_pin.id.as_str() != d.id.as_str()
            || c.task_pin.scope != *scope
            || c.task_pin.digest != c.task.content_digest().map_err(|_| invalid())?
            || self.body.action_projection != *c.action_projection
            || self.body.read_projection != *c.read_projection
            || self.body.action_join != *c.action_join
            || self.body.action_step != c.action_step.as_str()
            || self.body.mapping != *c.mapping_pin
        {
            return Err(SessionError::StaleRevision);
        }
        let bytes = serde_json::to_vec(c.mapping_document).map_err(|_| invalid())?;
        let mapping: MappingDocument = {
            if c.mapping_pin.contract.as_str() != "cg.connector-mapping"
                || gateway_domain::context_content_digest(&bytes) != c.mapping_pin.digest
            {
                return Err(SessionError::StaleRevision);
            }
            MappingDocument::from_json(&bytes).map_err(|_| invalid())?
        };
        if c.objects.len() > OPERATIONS {
            return Err(SessionError::LimitExceeded);
        }
        let mut seen = BTreeSet::new();
        for object in c.objects {
            if !seen.insert((&object.pin.id, &object.pin.scope)) {
                return Err(invalid());
            }
        }
        let target = c
            .objects
            .iter()
            .find(|o| o.pin == d.target.reference)
            .ok_or_else(unsupported)?;
        if target.body.kind != d.target.kind
            || target.body.scope != *scope
            || target.body.connector != self.body.connector
        {
            return Err(SessionError::ScopeDenied);
        }
        if d.desired_state.as_ref().is_some_and(|goal| {
            goal.conditions()
                .iter()
                .any(|condition| !target.body.subjects.contains(condition.subject()))
        }) {
            return Err(SessionError::ScopeDenied);
        }
        let mut operations = BTreeMap::new();
        for op in &self.body.operations {
            let object = c
                .objects
                .iter()
                .find(|o| o.pin == op.object)
                .ok_or_else(unsupported)?;
            if object.body.scope != *scope || object.body.connector != self.body.connector {
                return Err(SessionError::ScopeDenied);
            }
            let export = match &op.subject {
                Subject::TaskTarget => {
                    if object.pin != d.target.reference || object.body.kind != d.target.kind {
                        return Err(SessionError::ScopeDenied);
                    }
                    None
                }
                Subject::InputReference { role } => {
                    let input = d
                        .inputs
                        .iter()
                        .find(|input| &input.role == role)
                        .ok_or_else(unsupported)?;
                    let SemanticInputValue::Reference(reference) = &input.value else {
                        return Err(unsupported());
                    };
                    if reference != &object.pin
                        || object.body.parent_target.as_ref() != Some(&d.target.reference)
                    {
                        return Err(SessionError::ScopeDenied);
                    }
                    None
                }
                Subject::ScopeExport { provider_contract } => {
                    if c.scope_export != Some(provider_contract)
                        || object.body.parent_target.as_ref() != Some(&d.target.reference)
                        || !matches!(object.body.kind, TargetKind::Project | TargetKind::Service)
                    {
                        return Err(SessionError::ScopeDenied);
                    }
                    Some(provider_contract.clone())
                }
            };
            let record = mapping
                .records()
                .iter()
                .find(|r| r.reference == op.mapping_reference)
                .ok_or_else(unsupported)?;
            let entry = c
                .inventory
                .entries()
                .iter()
                .find(|e| e.mapping.reference == op.mapping_reference)
                .ok_or_else(unsupported)?;
            if entry.mapping != *record {
                return Err(SessionError::StaleRevision);
            }
            if op.purpose == Purpose::FixedRead
                && !matches!(
                    record.class.operation_class(),
                    ConnectorOperationClass::Read
                        | ConnectorOperationClass::Search
                        | ConnectorOperationClass::Inspect
                )
            {
                return Err(SessionError::AuthorityDenied);
            }
            let mut expected = BTreeMap::new();
            for (argument, coordinate) in &op.selector_arguments {
                let value = object
                    .body
                    .coordinates
                    .get(coordinate)
                    .ok_or_else(unsupported)?;
                expected.insert(
                    NonEmptyText::new(argument).map_err(|_| invalid())?,
                    value.clone(),
                );
            }
            for (argument, role) in &op.literal_arguments {
                let input = d
                    .inputs
                    .iter()
                    .find(|input| &input.role == role)
                    .ok_or_else(unsupported)?;
                let SemanticInputValue::Literal(value) = &input.value else {
                    return Err(unsupported());
                };
                if !scalar(value) {
                    return Err(unsupported());
                }
                expected.insert(
                    NonEmptyText::new(argument).map_err(|_| invalid())?,
                    value.clone(),
                );
            }
            for (argument, value) in &op.fixed_arguments {
                expected.insert(
                    NonEmptyText::new(argument).map_err(|_| invalid())?,
                    value.clone(),
                );
            }
            let mut response_values = BTreeMap::new();
            for (field, coordinate) in &op.response.identity_fields {
                response_values.insert(
                    field.clone(),
                    object
                        .body
                        .coordinates
                        .get(coordinate)
                        .ok_or_else(unsupported)?
                        .clone(),
                );
            }
            let resource_uri = match record.kind {
                InventoryKind::Resource => {
                    if op.purpose != Purpose::FixedRead
                        || !expected.is_empty()
                        || !op.response.identity_fields.is_empty()
                        || object.body.object_reference.as_str() != entry.wire_identity
                    {
                        return Err(SessionError::ScopeDenied);
                    }
                    Some(entry.wire_identity.clone())
                }
                InventoryKind::Tool => {
                    // A no-selector tool cannot prove pre-effect membership in
                    // this profile. Singleton/global operations need a distinct
                    // qualified contract; response identity is too late.
                    if object.body.coordinates.is_empty() {
                        return Err(unsupported());
                    }
                    let tool = c.tools.get(&record.capability).ok_or_else(unsupported)?;
                    let pin = tool.connector_inventory.as_ref().ok_or_else(unsupported)?;
                    if pin.connector != self.body.connector
                        || pin.scope != *scope
                        || pin.revision != c.inventory.inventory().revision
                        || pin.argument_schema_digest != entry.argument_schema_digest
                        || gateway_domain::context_content_digest(
                            &serde_json::to_vec(&tool.input.json_schema())
                                .map_err(|_| invalid())?,
                        ) != entry.argument_schema_digest
                        || tool.binding.id != record.capability
                        || tool.binding.scope != *scope
                    {
                        return Err(SessionError::StaleRevision);
                    }
                    let OutputType::Object { fields } = &tool.input else {
                        return Err(unsupported());
                    };
                    if expected.len() != fields.len()
                        || !fields.iter().all(|(name, field)| {
                            expected.iter().any(|(key, value)| {
                                key.as_str() == name && scalar_matches(&field.value_type, value)
                            })
                        })
                    {
                        return Err(SessionError::ScopeDenied);
                    }
                    let used: BTreeSet<_> = op.selector_arguments.values().collect();
                    if used != object.body.coordinates.keys().collect()
                        || !used.is_subset(&op.response.identity_fields.values().collect())
                    {
                        return Err(SessionError::ScopeDenied);
                    }
                    let schema = record.result_schema.as_ref().ok_or_else(unsupported)?;
                    if !required(schema, &op.response.object_field)
                        || schema["properties"][&op.response.object_field]["type"] != "string"
                        || response_values.iter().any(|(field, value)| {
                            !required(schema, field)
                                || !schema_scalar_matches(&schema["properties"][field], value)
                        })
                    {
                        return Err(unsupported());
                    }
                    None
                }
                InventoryKind::Prompt => return Err(unsupported()),
            };
            operations.insert(
                (op.mapping_reference.clone(), op.purpose),
                BoundOperation {
                    record: record.clone(),
                    expected,
                    object_reference: object.body.object_reference.clone(),
                    response: ResponseIdentity {
                        object_field: op.response.object_field.clone(),
                        identity_fields: op.response.identity_fields.clone(),
                    },
                    response_values,
                    resource_uri,
                    export,
                },
            );
        }
        Ok(BoundAssociation {
            target: c.inventory.inventory().target.clone(),
            revision: c.inventory.inventory().revision.clone(),
            task: d.id.clone(),
            step: c.action_step.clone(),
            execution: c.execution.clone(),
            read_execution: c.read_execution.clone(),
            operations,
        })
    }
}
fn required(schema: &Value, field: &str) -> bool {
    schema["required"]
        .as_array()
        .is_some_and(|values| values.iter().any(|value| value == field))
}
fn scalar_matches(shape: &OutputType, value: &TypedValue) -> bool {
    matches!(
        (shape, value),
        (OutputType::String {}, TypedValue::String(_))
            | (OutputType::Integer {}, TypedValue::Integer(_))
            | (OutputType::Boolean {}, TypedValue::Boolean(_))
    )
}
fn schema_scalar_matches(schema: &Value, value: &TypedValue) -> bool {
    matches!(
        (schema["type"].as_str(), value),
        (Some("string"), TypedValue::String(_))
            | (Some("integer"), TypedValue::Integer(_))
            | (Some("boolean"), TypedValue::Boolean(_))
    )
}
fn json_scalar_matches(value: &Value, expected: &TypedValue) -> bool {
    match expected {
        TypedValue::String(v) => value.as_str() == Some(v.as_str()),
        TypedValue::Integer(v) => value.as_i64() == Some(*v),
        TypedValue::Boolean(v) => value.as_bool() == Some(*v),
        _ => false,
    }
}
impl BoundAssociation {
    /// Descriptor membership only; no proposal or effect is authorized here.
    pub(super) fn proposal_capabilities(&self) -> BTreeSet<CapabilityId> {
        self.operations
            .iter()
            .filter(|((_, purpose), _)| *purpose == Purpose::Proposal)
            .map(|(_, operation)| operation.record.capability.clone())
            .collect()
    }
    fn check<'a>(
        &'a self,
        mapping_ref: &str,
        purpose: Purpose,
        request: &'a ConnectorRequest,
        source: Option<&gateway_domain::SourceId>,
    ) -> Result<MatchedRequest<'a>, SessionError> {
        let operation = self
            .operations
            .get(&(mapping_ref.to_owned(), purpose))
            .ok_or_else(unsupported)?;
        if request.target != self.target
            || request.inventory_revision != self.revision
            || request.task != self.task
            || request.step != self.step
            || request.execution
                != if purpose == Purpose::FixedRead {
                    self.read_execution.clone()
                } else {
                    self.execution.clone()
                }
            || request.capability != operation.record.capability
            || request.class != operation.record.class.operation_class()
            || request.arguments != operation.expected
            || (operation.record.kind == InventoryKind::Resource
                && source != operation.record.source.as_ref())
            || (operation.record.kind == InventoryKind::Tool && source.is_some())
        {
            return Err(SessionError::ScopeDenied);
        }
        Ok(MatchedRequest { operation, request })
    }
    pub(super) fn validate_proposal<'a>(
        &'a self,
        mapping_ref: &str,
        request: &'a ConnectorRequest,
    ) -> Result<MatchedRequest<'a>, SessionError> {
        self.check(mapping_ref, Purpose::Proposal, request, None)
    }
    pub(super) fn validate_fixed_read<'a>(
        &'a self,
        request: &'a ConnectorRequest,
        read: FixedReadJoin<'_>,
    ) -> Result<MatchedRequest<'a>, SessionError> {
        let matched = self.check(
            read.mapping_reference,
            Purpose::FixedRead,
            request,
            read.source,
        )?;
        if read.arguments != &request.arguments
            || read.scope_export != matched.operation.export.as_ref()
        {
            return Err(SessionError::ScopeDenied);
        }
        Ok(matched)
    }
}
impl MatchedRequest<'_> {
    pub(super) fn validate_response(
        &self,
        result: &ProtocolResult,
    ) -> Result<(), ConnectorFailure> {
        let fail = || ConnectorFailure {
            code: ConnectorFailureCode::ScopeMismatch,
            effect: if matches!(
                self.request.class,
                ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
            ) {
                ConnectorEffect::Unknown
            } else {
                ConnectorEffect::NoMutation
            },
            correlation: self.request.correlation.clone(),
        };
        match result {
            ProtocolResult::Resource { contents } => {
                let expected = self.operation.resource_uri.as_ref().ok_or_else(fail)?;
                if contents.len() != 1 || contents[0]["uri"].as_str() != Some(expected) {
                    return Err(fail());
                }
            }
            ProtocolResult::Tool {
                structured: Some(value),
                is_error: false,
                ..
            } if self.operation.resource_uri.is_none() => {
                if value[&self.operation.response.object_field].as_str()
                    != Some(self.operation.object_reference.as_str())
                    || self
                        .operation
                        .response_values
                        .iter()
                        .any(|(field, expected)| !json_scalar_matches(&value[field], expected))
                {
                    return Err(fail());
                }
            }
            _ => return Err(fail()),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp_client::{
        ProtocolInventory,
        mapping::{DiscoveryMapper, discovery_schema_digest},
    };
    use gateway_application::connector_manifest::ConnectorManifest;
    use gateway_domain::ContentDigest;
    use serde_json::json;
    fn pin(id: &str, contract: &str, value: &Value) -> Pin {
        Pin {
            id: ReferenceId::new(id).unwrap(),
            scope: ContextScopeId::new("project-x").unwrap(),
            contract: ReferenceId::new(contract).unwrap(),
            contract_version: SchemaVersion::V1,
            revision: ReferenceId::new("1").unwrap(),
            digest: gateway_domain::context_content_digest(&serde_json::to_vec(value).unwrap()),
        }
    }
    fn object(id: &str, reference: &str, parent: Option<&Pin>) -> ScopeObjectDocument {
        let body = json!({"schema_version":"1.0","id":id,"revision":"1","scope":"project-x","kind":"ARTIFACT","connector":"example.records","parent_target":parent,"subjects":[],"coordinates":{"id":{"kind":"STRING","value":reference}},"object_reference":reference});
        ScopeObjectDocument::from_json(
            &pin(id, "cg.connector-object", &body),
            &serde_json::to_vec(&body).unwrap(),
        )
        .unwrap()
    }
    struct Fixture {
        task: SemanticTaskIR,
        task_pin: Pin,
        join: Pin,
        action_projection: Pin,
        read_projection: Pin,
        read_execution: NonEmptyText,
        step: PlanStepId,
        execution: NonEmptyText,
        mapping: Value,
        mapping_pin: Pin,
        inventory: MappedInventory,
        tools: BTreeMap<CapabilityId, ToolDefinition>,
        objects: Vec<ScopeObjectDocument>,
        association: Value,
        export: Option<Pin>,
    }
    impl Fixture {
        fn new() -> Self {
            let objects = vec![
                object("object-one", "one", None),
                object("object-two", "two", None),
            ];
            let mut task: Value = serde_json::from_slice(include_bytes!(
                "../../../../tests/fixtures/semantic-task-v1/minimal.json"
            ))
            .unwrap();
            task["target"]["reference"] = serde_json::to_value(&objects[0].pin).unwrap();
            task["inputs"] = json!([{"role":"payload","value":{"kind":"LITERAL","value":{"kind":"STRING","value":"native text"}}}]);
            let task: SemanticTaskIR = serde_json::from_value(task).unwrap();
            let mut task_pin = pin(task.data().id.as_str(), "cg.semantic-task", &json!({}));
            task_pin.digest = task.content_digest().unwrap();
            let action_projection = pin(
                "action-projection",
                "cg.context-projection",
                &json!({"id":"action"}),
            );
            let read_projection = pin(
                "read-projection",
                "cg.context-projection",
                &json!({"id":"read"}),
            );
            let read_execution = NonEmptyText::new("read-execution").unwrap();
            let join = pin("join", "cg.connector-producer-join", &json!({"join":1}));
            let mut mapping: Value = serde_json::from_slice(include_bytes!(
                "../../../../tests/fixtures/connector-mapping-v1/mapping.json"
            ))
            .unwrap();
            mapping["records"][0]["result_schema"] = json!({"type":"object","additionalProperties":false,"properties":{"object":{"type":"string"},"id":{"type":"string"}},"required":["object","id"]});
            let mapping_pin = pin("mapping", "cg.connector-mapping", &mapping);
            let mut discovery: Value = serde_json::from_slice(include_bytes!(
                "../../../../tests/fixtures/connector-mapping-v1/discovery.json"
            ))
            .unwrap();
            discovery["tools"][0]["inputSchema"]["properties"]["payload"] =
                json!({"type":"string"});
            discovery["tools"][0]["inputSchema"]["properties"]["locked"] =
                json!({"type":"boolean"});
            discovery["tools"][0]["inputSchema"]["required"] =
                json!(["query", "payload", "locked"]);
            let wire = ProtocolInventory {
                tools: discovery["tools"].as_array().unwrap().clone(),
                resources: discovery["resources"].as_array().unwrap().clone(),
                prompts: discovery["prompts"].as_array().unwrap().clone(),
                templates: discovery["templates"].as_array().unwrap().clone(),
            };
            let envelope: Value = serde_json::from_slice(include_bytes!(
                "../../../../tests/fixtures/connector-mapping-v1/manifest.json"
            ))
            .unwrap();
            let mut manifest = envelope["manifest"].clone();
            for e in manifest["inventory"].as_array_mut().unwrap() {
                let kind: InventoryKind = serde_json::from_value(e["kind"].clone()).unwrap();
                let items = match kind {
                    InventoryKind::Tool => &wire.tools,
                    InventoryKind::Resource => &wire.resources,
                    InventoryKind::Prompt => &wire.prompts,
                };
                if let Some(item) = items.iter().find(|i| i["name"] == e["name"]) {
                    e["schema_digest"] = json!(discovery_schema_digest(kind, item));
                }
            }
            let target = ConnectorTarget {
                connector: NonEmptyText::new("example.records").unwrap(),
                scope: ContextScopeId::new("project-x").unwrap(),
            };
            let mapper = DiscoveryMapper::new(
                target,
                ConnectorManifest::from_definition_json(&serde_json::to_vec(&manifest).unwrap())
                    .unwrap(),
                MappingDocument::from_json(&serde_json::to_vec(&mapping).unwrap()).unwrap(),
            )
            .unwrap();
            let inventory = mapper.snapshot(&wire).unwrap();
            let capability = CapabilityId::new("records.read").unwrap();
            let schema = mapper
                .tool_schema(&wire, &capability, &inventory.inventory().revision)
                .unwrap();
            let tool = ToolDefinition::from_connector_schema(
                ResolvedReference {
                    id: capability.clone(),
                    scope: schema.target.scope.clone(),
                    contract: ReferenceId::new("cg.capability").unwrap(),
                    contract_version: SchemaVersion::V1,
                    revision: ReferenceId::new("1").unwrap(),
                    digest: ContentDigest::new("a".repeat(64)).unwrap(),
                },
                NonEmptyText::new("Read").unwrap(),
                schema.target.clone(),
                schema.inventory_revision.clone(),
                schema.argument_schema_digest.clone(),
                &schema.argument_schema,
            )
            .unwrap();
            let tools = BTreeMap::from([(capability, tool)]);
            let step = PlanStepId::new("step-one").unwrap();
            let execution = NonEmptyText::new("execution-one").unwrap();
            let association = json!({"schema_version":"1.0","scope":"project-x","task":task_pin,"action_join":join,"action_projection":action_projection,"read_projection":read_projection,"mapping":mapping_pin,"action_step":"step-one","connector":"example.records","operations":[{"mapping_reference":"mapping.records.read.v1","purpose":"PROPOSAL","profile":"EXACT_TASK_OPERANDS_V1","subject":{"kind":"TASK_TARGET"},"object":objects[0].pin,"selector_arguments":{"query":"id"},"literal_arguments":{"payload":"payload"},"fixed_arguments":{"locked":{"kind":"BOOLEAN","value":true}},"proposal_data":[],"response":{"object_field":"object","identity_fields":{"id":"id"}}}]});
            Self {
                task,
                task_pin,
                join,
                action_projection,
                read_projection,
                read_execution,
                step,
                execution,
                mapping,
                mapping_pin,
                inventory,
                tools,
                objects,
                association,
                export: None,
            }
        }
        fn admit(&self) -> Result<BoundAssociation, SessionError> {
            let pin = pin(
                "association",
                "cg.connector-selector-association",
                &self.association,
            );
            SelectorAssociation::from_json(&pin, &serde_json::to_vec(&self.association).unwrap())?
                .admit(AssociationContext {
                    task_pin: &self.task_pin,
                    task: &self.task,
                    action_join: &self.join,
                    action_projection: &self.action_projection,
                    read_projection: &self.read_projection,
                    read_execution: &self.read_execution,
                    action_step: &self.step,
                    execution: &self.execution,
                    mapping_pin: &self.mapping_pin,
                    mapping_document: &self.mapping,
                    inventory: &self.inventory,
                    tools: &self.tools,
                    objects: &self.objects,
                    scope_export: self.export.as_ref(),
                })
        }
        fn replace_task(&mut self, value: Value) {
            self.task = serde_json::from_value(value).unwrap();
            self.task_pin.digest = self.task.content_digest().unwrap();
            self.association["task"] = json!(self.task_pin);
        }
        fn request(&self) -> ConnectorRequest {
            ConnectorRequest {
                target: self.inventory.inventory().target.clone(),
                capability: CapabilityId::new("records.read").unwrap(),
                class: ConnectorOperationClass::Read,
                inventory_revision: self.inventory.inventory().revision.clone(),
                arguments: BTreeMap::from([
                    (
                        NonEmptyText::new("query").unwrap(),
                        TypedValue::string("one").unwrap(),
                    ),
                    (
                        NonEmptyText::new("payload").unwrap(),
                        TypedValue::string("native text").unwrap(),
                    ),
                    (
                        NonEmptyText::new("locked").unwrap(),
                        TypedValue::Boolean(true),
                    ),
                ]),
                task: self.task.data().id.clone(),
                step: self.step.clone(),
                execution: self.execution.clone(),
                correlation: NonEmptyText::new("correlation-one").unwrap(),
            }
        }
    }
    #[test]
    fn actual_task_selectors_and_response_membership_are_exact() {
        let f = Fixture::new();
        let bound = f.admit().unwrap();
        let request = f.request();
        assert_eq!(
            bound.proposal_capabilities(),
            BTreeSet::from([request.capability.clone()])
        );
        let matched = bound
            .validate_proposal("mapping.records.read.v1", &request)
            .unwrap();
        assert!(
            matched
                .validate_response(&ProtocolResult::Tool {
                    content: vec![],
                    structured: Some(json!({"object":"one","id":"one"})),
                    is_error: false
                })
                .is_ok()
        );
        for body in [
            json!({"object":"two","id":"one"}),
            json!({"object":"one","id":"two"}),
            json!({"object":"one"}),
        ] {
            let failure = matched
                .validate_response(&ProtocolResult::Tool {
                    content: vec![],
                    structured: Some(body),
                    is_error: false,
                })
                .unwrap_err();
            assert_eq!(failure.effect, ConnectorEffect::NoMutation);
        }
        for args in [json!({"query":{"kind":"STRING","value":"two"}}), json!({})] {
            let mut other = request.clone();
            other.arguments = serde_json::from_value(args).unwrap();
            assert!(
                bound
                    .validate_proposal("mapping.records.read.v1", &other)
                    .is_err()
            );
        }
        let mut other = request.clone();
        other.execution = NonEmptyText::new("other-execution").unwrap();
        assert!(
            bound
                .validate_proposal("mapping.records.read.v1", &other)
                .is_err()
        );
    }
    #[test]
    fn same_scope_foreign_object_and_inexact_pins_are_denied() {
        let mut f = Fixture::new();
        f.association["operations"][0]["object"] = json!(f.objects[1].pin);
        assert!(matches!(f.admit(), Err(SessionError::ScopeDenied)));
        let mut f = Fixture::new();
        f.task_pin.revision = ReferenceId::new("other").unwrap();
        assert!(matches!(f.admit(), Err(SessionError::StaleRevision)));
        let mut f = Fixture::new();
        f.mapping["records"][0]["name"] = json!("other");
        assert!(matches!(f.admit(), Err(SessionError::StaleRevision)));
    }
    #[test]
    fn operator_profile_rejects_free_data_overlaps_and_unknown_fields() {
        let mut f = Fixture::new();
        f.association["operations"][0]["proposal_data"] = json!(["query"]);
        assert!(matches!(
            f.admit(),
            Err(SessionError::UnsupportedCapability)
        ));
        let mut f = Fixture::new();
        f.association["operations"][0]["fixed_arguments"] =
            json!({"query":{"kind":"STRING","value":"one"}});
        assert!(f.admit().is_err());
        let mut f = Fixture::new();
        f.association["operations"][0]["arbitrary_dsl"] = json!("allowed");
        assert!(f.admit().is_err());
        let mut f = Fixture::new();
        f.association["credentials"] = json!("secret");
        assert!(f.admit().is_err());
    }
    #[test]
    fn response_failure_after_mutation_is_unknown() {
        let f = Fixture::new();
        let mut bound = f.admit().unwrap();
        let mut request = f.request();
        // This tests classification only: no mutation is dispatched or granted.
        request.class = ConnectorOperationClass::Mutate;
        let operation = bound
            .operations
            .get_mut(&("mapping.records.read.v1".into(), Purpose::Proposal))
            .unwrap();
        let matched = MatchedRequest {
            operation,
            request: &request,
        };
        assert_eq!(
            matched
                .validate_response(&ProtocolResult::Tool {
                    content: vec![],
                    structured: Some(json!({"object":"other","id":"one"})),
                    is_error: false
                })
                .unwrap_err()
                .effect,
            ConnectorEffect::Unknown
        );
    }
    #[test]
    fn native_literal_and_pinned_constant_cannot_be_replaced_by_model() {
        let f = Fixture::new();
        let bound = f.admit().unwrap();
        for (key, value) in [
            ("payload", TypedValue::string("other text").unwrap()),
            ("locked", TypedValue::Boolean(false)),
            ("query", TypedValue::Integer(1)),
        ] {
            let mut request = f.request();
            request
                .arguments
                .insert(NonEmptyText::new(key).unwrap(), value);
            assert!(matches!(
                bound.validate_proposal("mapping.records.read.v1", &request),
                Err(SessionError::ScopeDenied)
            ));
        }
        let mut f = Fixture::new();
        f.association["operations"][0]["literal_arguments"]["payload"] = json!("absent-input");
        assert!(matches!(
            f.admit(),
            Err(SessionError::UnsupportedCapability)
        ));
    }
    #[test]
    fn referenced_input_requires_exact_native_role_and_parent_target() {
        let mut f = Fixture::new();
        f.objects
            .push(object("child", "child", Some(&f.objects[0].pin)));
        let mut task = serde_json::to_value(&f.task).unwrap();
        task["inputs"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"child","value":{"kind":"REFERENCE","value":f.objects[2].pin}}));
        f.replace_task(task);
        f.association["operations"][0]["subject"] =
            json!({"kind":"INPUT_REFERENCE","role":"child"});
        f.association["operations"][0]["object"] = json!(f.objects[2].pin);
        let bound = f.admit().unwrap();
        let mut request = f.request();
        request.arguments.insert(
            NonEmptyText::new("query").unwrap(),
            TypedValue::string("child").unwrap(),
        );
        assert!(
            bound
                .validate_proposal("mapping.records.read.v1", &request)
                .is_ok()
        );
        // A legitimate input in the same scope still needs its exact parent.
        f.objects[2] = object("child", "child", Some(&f.objects[1].pin));
        let mut task = serde_json::to_value(&f.task).unwrap();
        task["inputs"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|input| input["role"] == "child")
            .unwrap()["value"]["value"] = json!(f.objects[2].pin);
        f.replace_task(task);
        f.association["operations"][0]["object"] = json!(f.objects[2].pin);
        assert!(matches!(f.admit(), Err(SessionError::ScopeDenied)));
        f.association["operations"][0]["subject"]["role"] = json!("payload");
        assert!(matches!(
            f.admit(),
            Err(SessionError::UnsupportedCapability)
        ));
    }
    #[test]
    fn fixed_resource_read_binds_exact_uri_source_and_arguments() {
        let mut f = Fixture::new();
        let body = json!({"schema_version":"1.0","id":"resource","revision":"1","scope":"project-x","kind":"ARTIFACT","connector":"example.records","parent_target":null,"subjects":[],"coordinates":{},"object_reference":"records://one"});
        let resource = ScopeObjectDocument::from_json(
            &pin("resource", "cg.connector-object", &body),
            &serde_json::to_vec(&body).unwrap(),
        )
        .unwrap();
        f.objects.push(resource);
        let mut task = serde_json::to_value(&f.task).unwrap();
        task["target"]["reference"] = json!(f.objects[2].pin);
        f.replace_task(task);
        f.association["operations"] = json!([{"mapping_reference":"mapping.records.source.v1","purpose":"FIXED_READ","profile":"EXACT_TASK_OPERANDS_V1","subject":{"kind":"TASK_TARGET"},"object":f.objects[2].pin,"selector_arguments":{},"literal_arguments":{},"fixed_arguments":{},"proposal_data":[],"response":{"object_field":"uri","identity_fields":{}}}]);
        let bound = f.admit().unwrap();
        let mut request = f.request();
        request.capability = CapabilityId::new("records.resource.read").unwrap();
        request.execution = f.read_execution.clone();
        request.arguments.clear();
        let source = gateway_domain::SourceId::new("records.one").unwrap();
        let join = || FixedReadJoin {
            mapping_reference: "mapping.records.source.v1",
            source: Some(&source),
            arguments: &request.arguments,
            scope_export: None,
        };
        let matched = bound.validate_fixed_read(&request, join()).unwrap();
        assert!(
            matched
                .validate_response(&ProtocolResult::Resource {
                    contents: vec![json!({"uri":"records://one","text":"data"})]
                })
                .is_ok()
        );
        assert!(
            matched
                .validate_response(&ProtocolResult::Resource {
                    contents: vec![json!({"uri":"records://two","text":"data"})]
                })
                .is_err()
        );
        assert!(
            bound
                .validate_proposal("mapping.records.source.v1", &request)
                .is_err()
        );
        assert!(
            bound
                .validate_fixed_read(
                    &request,
                    FixedReadJoin {
                        source: None,
                        ..join()
                    }
                )
                .is_err()
        );
        f.association["operations"][0]["object"] = json!(f.objects[0].pin);
        assert!(f.admit().is_err());
    }
    #[test]
    fn complete_scope_export_is_separate_pinned_host_read_only() {
        let mut f = Fixture::new();
        let export = pin(
            "complete-export",
            "cg.connector-state-universe",
            &json!({"universe":"registered"}),
        );
        f.export = Some(export.clone());
        let body = json!({"schema_version":"1.0","id":"export-object","revision":"1","scope":"project-x","kind":"PROJECT","connector":"example.records","parent_target":f.objects[0].pin,"subjects":[],"coordinates":{"id":{"kind":"STRING","value":"all"}},"object_reference":"scope-export"});
        f.objects.push(
            ScopeObjectDocument::from_json(
                &pin("export-object", "cg.connector-object", &body),
                &serde_json::to_vec(&body).unwrap(),
            )
            .unwrap(),
        );
        f.association["operations"][0]["subject"] =
            json!({"kind":"SCOPE_EXPORT","provider_contract":export});
        f.association["operations"][0]["object"] = json!(f.objects[2].pin);
        f.association["operations"][0]["purpose"] = json!("FIXED_READ");
        let bound = f.admit().unwrap();
        assert!(
            bound.proposal_capabilities().is_empty(),
            "scope-export fixed reads do not become model proposal tools"
        );
        let mut request = f.request();
        request.execution = f.read_execution.clone();
        request.arguments.insert(
            NonEmptyText::new("query").unwrap(),
            TypedValue::string("all").unwrap(),
        );
        assert!(
            bound
                .validate_fixed_read(
                    &request,
                    FixedReadJoin {
                        mapping_reference: "mapping.records.read.v1",
                        source: None,
                        arguments: &request.arguments,
                        scope_export: f.export.as_ref()
                    }
                )
                .is_ok()
        );
        assert!(
            bound
                .validate_proposal("mapping.records.read.v1", &request)
                .is_err()
        );
        assert!(
            bound
                .validate_fixed_read(
                    &request,
                    FixedReadJoin {
                        mapping_reference: "mapping.records.read.v1",
                        source: None,
                        arguments: &request.arguments,
                        scope_export: None
                    }
                )
                .is_err()
        );
        f.export = None;
        assert!(matches!(f.admit(), Err(SessionError::ScopeDenied)));
    }
    #[test]
    fn typed_closure_preserves_every_exact_pin_and_rejects_digest_drift() {
        let f = Fixture::new();
        let p = pin(
            "association",
            "cg.connector-selector-association",
            &f.association,
        );
        let association =
            SelectorAssociation::from_json(&p, &serde_json::to_vec(&f.association).unwrap())
                .unwrap();
        assert_eq!(
            association.reference_bindings(),
            vec![
                f.task_pin.clone(),
                f.join.clone(),
                f.action_projection.clone(),
                f.read_projection.clone(),
                f.mapping_pin.clone(),
                f.objects[0].pin.clone()
            ]
        );
        let child = object("child", "child", Some(&f.objects[0].pin));
        assert_eq!(child.reference_bindings(), vec![f.objects[0].pin.clone()]);
        assert!(f.objects[0].reference_bindings().is_empty());
        let mut changed = f.association.clone();
        changed["operations"][0]["object"]["revision"] = json!("2");
        assert!(matches!(
            SelectorAssociation::from_json(&p, &serde_json::to_vec(&changed).unwrap()),
            Err(SessionError::StaleRevision)
        ));
        let mut foreign = p.clone();
        foreign.scope = ContextScopeId::new("foreign-scope").unwrap();
        assert!(
            SelectorAssociation::from_json(&foreign, &serde_json::to_vec(&f.association).unwrap())
                .is_err()
        );
        assert!(matches!(
            SelectorAssociation::from_json(&p, &vec![b' '; BYTES + 1]),
            Err(SessionError::LimitExceeded)
        ));
    }
    #[test]
    fn empty_coordinates_cannot_hide_a_fixed_foreign_routing_argument() {
        let mut f = Fixture::new();
        let body = json!({"schema_version":"1.0","id":"singleton","revision":"1","scope":"project-x","kind":"ARTIFACT","connector":"example.records","parent_target":null,"subjects":[],"coordinates":{},"object_reference":"one"});
        f.objects.push(
            ScopeObjectDocument::from_json(
                &pin("singleton", "cg.connector-object", &body),
                &serde_json::to_vec(&body).unwrap(),
            )
            .unwrap(),
        );
        let mut task = serde_json::to_value(&f.task).unwrap();
        task["target"]["reference"] = json!(f.objects[2].pin);
        f.replace_task(task);
        let op = &mut f.association["operations"][0];
        op["object"] = json!(f.objects[2].pin);
        op["selector_arguments"] = json!({});
        op["fixed_arguments"]["query"] = json!({"kind":"STRING","value":"foreign-row"});
        op["response"]["identity_fields"] = json!({});
        assert!(matches!(
            f.admit(),
            Err(SessionError::UnsupportedCapability)
        ));
    }
    #[test]
    fn actual_target_kind_and_desired_subject_universe_must_match() {
        let mut f = Fixture::new();
        let mut task = serde_json::to_value(&f.task).unwrap();
        task["target"]["kind"] = json!("SERVICE");
        f.replace_task(task);
        assert!(matches!(f.admit(), Err(SessionError::ScopeDenied)));
        let mut f = Fixture::new();
        let goal: Value = serde_json::from_slice(include_bytes!(
            "../../../../tests/fixtures/semantic-task-v1/performance-analysis.json"
        ))
        .unwrap();
        let mut task = serde_json::to_value(&f.task).unwrap();
        task["desired_state"] = goal["desired_state"].clone();
        f.replace_task(task);
        assert!(matches!(f.admit(), Err(SessionError::ScopeDenied)));
        let body = json!({"schema_version":"1.0","id":"object-one","revision":"1","scope":"project-x","kind":"ARTIFACT","connector":"example.records","parent_target":null,"subjects":["ServiceX.p95_ms"],"coordinates":{"id":{"kind":"STRING","value":"one"}},"object_reference":"one"});
        f.objects[0] = ScopeObjectDocument::from_json(
            &pin("object-one", "cg.connector-object", &body),
            &serde_json::to_vec(&body).unwrap(),
        )
        .unwrap();
        let mut task = serde_json::to_value(&f.task).unwrap();
        task["target"]["reference"] = json!(f.objects[0].pin);
        f.replace_task(task);
        f.association["operations"][0]["object"] = json!(f.objects[0].pin);
        assert!(f.admit().is_ok());
    }
    #[test]
    fn fixed_reads_require_actual_read_context_and_keep_the_action_step_join() {
        let mut f = Fixture::new();
        f.association["operations"][0]["purpose"] = json!("FIXED_READ");
        let bound = f.admit().unwrap();
        let mut request = f.request();
        let validate = |request: &ConnectorRequest| {
            bound
                .validate_fixed_read(
                    request,
                    FixedReadJoin {
                        mapping_reference: "mapping.records.read.v1",
                        source: None,
                        arguments: &request.arguments,
                        scope_export: None,
                    },
                )
                .map(|_| ())
        };
        assert!(
            matches!(validate(&request), Err(SessionError::ScopeDenied)),
            "planned ActionContext does not substitute for ReadContext"
        );
        request.execution = f.read_execution.clone();
        assert!(validate(&request).is_ok());
        assert_eq!(request.step, f.step);
        request.step = PlanStepId::new("producer-step").unwrap();
        assert!(
            matches!(validate(&request), Err(SessionError::ScopeDenied)),
            "read context is separate; native request policy step remains ActionStep"
        );
        f.read_projection.revision = ReferenceId::new("foreign-read-projection").unwrap();
        assert!(matches!(f.admit(), Err(SessionError::StaleRevision)));
    }
}
