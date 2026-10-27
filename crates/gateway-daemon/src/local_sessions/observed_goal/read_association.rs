//! Private operator-qualified association data, never an execution grant.
//!
//! `cg.connector-object/1.0` is a NEW trusted operator descriptor contract, not
//! a generic native-object entitlement. Its coordinates attest the complete
//! routing identity for the qualified mapping/profile. Task membership is
//! checked independently; knowing another row in the same scope grants nothing.
//! This read-only exact-operands baseline supports only native task coordinates
//! and native literal inputs. No free payload, arbitrary fixed constants or DSL.
use crate::mcp_client::{
    ProtocolResult,
    mapping::{MappedInventory, MappedToolSchema, MappingDocument, MappingRecord},
};
use gateway_application::{
    connector_manifest::InventoryKind,
    connectors::*,
    sessions::{SessionError, SupportedConnectorGoal},
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
    pin_scope(pin, &pin.scope)?;
    if pin.contract.as_str() != contract || pin.contract_version != SchemaVersion::V1 {
        return Err(unsupported());
    }
    let value = crate::local_sessions::definition_json::parse(
        std::str::from_utf8(bytes).map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())?;
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
    } else if pin.contract_version != SchemaVersion::V1
        || gateway_domain::model_invocation::validate_model_binding(pin).is_err()
    {
        Err(unsupported())
    } else {
        Ok(())
    }
}

pub(in crate::local_sessions) struct ScopeObjectDocument {
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
    pub(in crate::local_sessions) fn reference_bindings(&self) -> Vec<Pin> {
        self.body.parent_target.iter().cloned().collect()
    }
    pub(in crate::local_sessions) fn from_json(
        pin: &Pin,
        bytes: &[u8],
    ) -> Result<Self, SessionError> {
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

pub(in crate::local_sessions) struct ReadAssociation {
    pin: Pin,
    body: ReadAssociationDocument,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadAssociationDocument {
    schema_version: SchemaVersion,
    scope: ContextScopeId,
    task: Pin,
    projection: Pin,
    mapping: Pin,
    step: String,
    connector: NonEmptyText,
    operations: Vec<Operation>,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Purpose {
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
#[derive(Clone)]
pub(in crate::local_sessions) struct ReadSubjectBinding {
    pub(in crate::local_sessions) mapping_reference: String,
    pub(in crate::local_sessions) arguments: BTreeMap<NonEmptyText, TypedValue>,
    pub(in crate::local_sessions) source: gateway_domain::SourceId,
    pub(in crate::local_sessions) object_field: String,
    pub(in crate::local_sessions) condition_ids: BTreeSet<gateway_domain::ConditionId>,
    pub(in crate::local_sessions) subjects: BTreeSet<SubjectPath>,
}
pub(in crate::local_sessions) struct ReadAssociationContext<'a> {
    pub(in crate::local_sessions) task_pin: &'a Pin,
    pub(in crate::local_sessions) task: &'a SemanticTaskIR,
    pub(in crate::local_sessions) goal: &'a SupportedConnectorGoal,
    pub(in crate::local_sessions) projection: &'a Pin,
    pub(in crate::local_sessions) step: &'a PlanStepId,
    pub(in crate::local_sessions) execution: &'a NonEmptyText,
    pub(in crate::local_sessions) mapping_pin: &'a Pin,
    pub(in crate::local_sessions) mapping_document: &'a Value,
    pub(in crate::local_sessions) inventory: &'a MappedInventory,
    pub(in crate::local_sessions) schemas: &'a BTreeMap<CapabilityId, MappedToolSchema>,
    pub(in crate::local_sessions) subjects: &'a [ReadSubjectBinding],
    pub(in crate::local_sessions) objects: &'a [ScopeObjectDocument],
    pub(in crate::local_sessions) scope_export: Option<&'a Pin>,
}
pub(in crate::local_sessions) struct BoundReadAssociation {
    target: ConnectorTarget,
    revision: gateway_domain::ContentDigest,
    task: gateway_domain::TaskId,
    step: PlanStepId,
    execution: NonEmptyText,
    operations: BTreeMap<(String, Purpose), Vec<BoundOperation>>,
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
pub(in crate::local_sessions) struct MatchedRead<'a> {
    operation: &'a BoundOperation,
    request: &'a ConnectorRequest,
}
pub(in crate::local_sessions) struct FixedReadJoin<'a> {
    pub(in crate::local_sessions) mapping_reference: &'a str,
    pub(in crate::local_sessions) source: Option<&'a gateway_domain::SourceId>,
    pub(in crate::local_sessions) arguments: &'a BTreeMap<NonEmptyText, TypedValue>,
    pub(in crate::local_sessions) scope_export: Option<&'a Pin>,
}
impl ReadAssociation {
    /// Typed closure edges, never a generic JSON walk or an authorization result.
    pub(in crate::local_sessions) fn reference_bindings(&self) -> Vec<Pin> {
        let mut pins = vec![
            self.body.task.clone(),
            self.body.projection.clone(),
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
    pub(in crate::local_sessions) fn from_json(
        pin: &Pin,
        bytes: &[u8],
    ) -> Result<Self, SessionError> {
        let body: ReadAssociationDocument =
            decode_pinned(pin, bytes, "cg.connector-read-association")?;
        if body.schema_version != SchemaVersion::V1 || body.scope != pin.scope {
            return Err(invalid());
        }
        if body.operations.is_empty() || body.operations.len() > OPERATIONS {
            return Err(SessionError::LimitExceeded);
        }
        pin_scope(&body.task, &body.scope)?;
        pin_scope(&body.projection, &body.scope)?;
        pin_scope(&body.mapping, &body.scope)?;
        if body.task.contract.as_str() != "cg.semantic-task"
            || body.mapping.contract.as_str() != "cg.connector-mapping"
            || body.projection.contract.as_str() != "cg.context-projection"
        {
            return Err(unsupported());
        }
        PlanStepId::new(body.step.clone()).map_err(|_| invalid())?;
        if body.connector.as_str().len() > 128 {
            return Err(invalid());
        }
        let mut keys = BTreeSet::new();
        for op in &body.operations {
            let Profile::ExactTaskOperandsV1 = op.profile;
            // Every non-selector operand must come from an actual Task literal.
            // Query languages and arbitrary pinned constants are not qualified.
            if !op.fixed_arguments.is_empty() {
                return Err(unsupported());
            }
            if !keys.insert((
                &op.mapping_reference,
                op.purpose,
                &op.object.id,
                &op.selector_arguments,
                &op.literal_arguments,
            )) || op.mapping_reference.is_empty()
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
                if provider_contract.contract.as_str() != "cg.connector-state-universe" {
                    return Err(unsupported());
                }
            }
        }
        Ok(Self {
            pin: pin.clone(),
            body,
        })
    }
    pub(in crate::local_sessions) fn admit(
        &self,
        c: ReadAssociationContext<'_>,
    ) -> Result<BoundReadAssociation, SessionError> {
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
            || self.body.projection != *c.projection
            || self.body.step != c.step.as_str()
            || self.body.mapping != *c.mapping_pin
        {
            return Err(SessionError::StaleRevision);
        }
        if c.goal.scope() != scope
            || !c.goal.steps().contains(c.step)
            || d.desired_state.as_ref() != Some(c.goal.intent().desired_state())
        {
            return Err(SessionError::ScopeDenied);
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
        if c.objects.len() > OPERATIONS
            || c.subjects.len() > OPERATIONS
            || c.schemas.len() > OPERATIONS
        {
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
        let mut covered = BTreeSet::new();
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
            if !matches!(
                record.class.operation_class(),
                ConnectorOperationClass::Read
                    | ConnectorOperationClass::Search
                    | ConnectorOperationClass::Inspect
            ) {
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
            let mut matching = c
                .subjects
                .iter()
                .filter(|s| s.mapping_reference == op.mapping_reference && s.arguments == expected);
            let route = matching.next().ok_or_else(unsupported)?;
            if matching.next().is_some() {
                return Err(SessionError::InvalidInput);
            }
            let subjects = &route.subjects;
            if subjects.is_empty()
                || subjects.len() > 256
                || subjects
                    .iter()
                    .any(|s| !object.body.subjects.contains(s) || !target.body.subjects.contains(s))
            {
                return Err(SessionError::ScopeDenied);
            }
            if record
                .source
                .as_ref()
                .is_some_and(|source| source != &route.source)
            {
                return Err(SessionError::ScopeDenied);
            }
            if route.condition_ids.is_empty() || route.condition_ids.len() > 256 {
                return Err(SessionError::InvalidInput);
            }
            for id in &route.condition_ids {
                let condition = c
                    .goal
                    .intent()
                    .desired_state()
                    .conditions()
                    .iter()
                    .find(|condition| condition.id() == id)
                    .ok_or(SessionError::ScopeDenied)?;
                let source = c
                    .goal
                    .observations()
                    .iter()
                    .find(|m| m.condition() == id)
                    .ok_or_else(unsupported)?;
                if !subjects.contains(condition.subject())
                    || source.capability() != &record.capability
                    || source.source() != &route.source
                    || source.operation_class() != record.class.operation_class()
                    || !covered.insert(id.clone())
                {
                    return Err(SessionError::ScopeDenied);
                }
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
                    if !expected.is_empty()
                        || !op.response.identity_fields.is_empty()
                        || object.body.object_reference.as_str() != entry.wire_identity
                    {
                        return Err(SessionError::ScopeDenied);
                    }
                    Some(entry.wire_identity.clone())
                }
                InventoryKind::Tool => {
                    if route.object_field != op.response.object_field {
                        return Err(SessionError::ScopeDenied);
                    }
                    // A no-selector tool cannot prove pre-effect membership in
                    // this profile. Singleton/global operations need a distinct
                    // qualified contract; response identity is too late.
                    if object.body.coordinates.is_empty() {
                        return Err(unsupported());
                    }
                    let tool = c.schemas.get(&record.capability).ok_or_else(unsupported)?;
                    if tool.target != c.inventory.inventory().target
                        || tool.capability != record.capability
                        || tool.inventory_revision != c.inventory.inventory().revision
                        || tool.argument_schema_digest != entry.argument_schema_digest
                        || tool.wire_argument_schema_digest != entry.argument_schema_digest
                        || tool.result_schema_digest != entry.result_schema_digest
                        || gateway_domain::context_content_digest(
                            &serde_json::to_vec(&tool.argument_schema).map_err(|_| invalid())?,
                        ) != entry.argument_schema_digest
                        || gateway_domain::context_content_digest(
                            &serde_json::to_vec(&tool.result_schema).map_err(|_| invalid())?,
                        ) != entry.result_schema_digest
                        || record.result_schema.as_ref() != Some(&tool.result_schema)
                    {
                        return Err(SessionError::StaleRevision);
                    }
                    let shape =
                        gateway_application::prompt_rendering::parse_connector_input_schema(
                            &tool.argument_schema,
                        )
                        .map_err(|_| unsupported())?;
                    let OutputType::Object { fields } = shape else {
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
            let bound = BoundOperation {
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
            };
            let bucket: &mut Vec<BoundOperation> = operations
                .entry((op.mapping_reference.clone(), op.purpose))
                .or_default();
            if bucket.iter().any(|prior| prior.expected == bound.expected) {
                return Err(SessionError::InvalidInput);
            }
            bucket.push(bound);
        }
        if c.goal
            .intent()
            .desired_state()
            .conditions()
            .iter()
            .any(|condition| !covered.contains(condition.id()))
        {
            return Err(SessionError::ScopeDenied);
        }
        Ok(BoundReadAssociation {
            target: c.inventory.inventory().target.clone(),
            revision: c.inventory.inventory().revision.clone(),
            task: d.id.clone(),
            step: c.step.clone(),
            execution: c.execution.clone(),
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
impl BoundReadAssociation {
    fn check<'a>(
        &'a self,
        mapping_ref: &str,
        purpose: Purpose,
        request: &'a ConnectorRequest,
        source: Option<&gateway_domain::SourceId>,
    ) -> Result<MatchedRead<'a>, SessionError> {
        let bucket = self
            .operations
            .get(&(mapping_ref.to_owned(), purpose))
            .ok_or_else(unsupported)?;
        let mut matching = bucket.iter().filter(|o| o.expected == request.arguments);
        let operation = matching.next().ok_or(SessionError::ScopeDenied)?;
        if matching.next().is_some() {
            return Err(SessionError::InvalidInput);
        }
        if request.target != self.target
            || request.inventory_revision != self.revision
            || request.task != self.task
            || request.step != self.step
            || request.execution != self.execution
            || request.capability != operation.record.capability
            || request.class != operation.record.class.operation_class()
            || request.arguments != operation.expected
            || (operation.record.kind == InventoryKind::Resource
                && source != operation.record.source.as_ref())
            || (operation.record.kind == InventoryKind::Tool && source.is_some())
        {
            return Err(SessionError::ScopeDenied);
        }
        Ok(MatchedRead { operation, request })
    }
    /// Exact matched operation metadata only; this is not an execution grant.
    pub(in crate::local_sessions) fn fixed_read_export<'a>(
        &'a self,
        request: &'a ConnectorRequest,
        mapping_reference: &str,
        source: Option<&gateway_domain::SourceId>,
    ) -> Result<Option<&'a Pin>, SessionError> {
        Ok(self
            .check(mapping_reference, Purpose::FixedRead, request, source)?
            .operation
            .export
            .as_ref())
    }
    pub(in crate::local_sessions) fn validate_fixed_read<'a>(
        &'a self,
        request: &'a ConnectorRequest,
        read: FixedReadJoin<'_>,
    ) -> Result<MatchedRead<'a>, SessionError> {
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
impl MatchedRead<'_> {
    pub(in crate::local_sessions) fn validate_response(
        &self,
        result: &ProtocolResult,
    ) -> Result<(), ConnectorFailure> {
        let fail = || ConnectorFailure {
            code: ConnectorFailureCode::ScopeMismatch,
            effect: ConnectorEffect::NoMutation,
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
#[path = "read_association_tests.rs"]
mod tests;
