//! Explicit CG-owned mapping v1. Discovery yields derived data, never authority.
use super::{schema, *};
use gateway_application::{connector_manifest::*, connectors::*};
use gateway_domain::{
    CapabilityClass, CapabilityDefinition, CapabilityId, ContentDigest, SourceId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAPPING_VERSION: &str = "1.0";
pub const MAX_MAPPING_BYTES: usize = 262_144;

/// Content-free deterministic diagnostics; no remote names, arguments or payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingError {
    Encoding,
    Version,
    Metadata,
    DuplicateMapping,
    ConflictingMapping,
    Target,
    Discovery,
    UnknownEntry,
    MissingEntry,
    UnresolvedReference,
    SchemaDigest,
    Schema,
    StaleInventory,
    Request,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OperationClass {
    Read,
    Search,
    Inspect,
    Mutate,
    Admin,
}
impl OperationClass {
    pub fn operation_class(self) -> ConnectorOperationClass {
        match self {
            Self::Read => ConnectorOperationClass::Read,
            Self::Search => ConnectorOperationClass::Search,
            Self::Inspect => ConnectorOperationClass::Inspect,
            Self::Mutate => ConnectorOperationClass::Mutate,
            Self::Admin => ConnectorOperationClass::Admin,
        }
    }
    pub fn capability_class(self) -> CapabilityClass {
        match self {
            Self::Read | Self::Search | Self::Inspect => CapabilityClass::Inspect,
            Self::Mutate | Self::Admin => CapabilityClass::Mutate,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffects {
    None,
    Mutation,
    Administrative,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency {
    NonIdempotent,
    Idempotent,
    ProviderKey,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrySafety {
    Never,
    ReadOnly,
    Idempotent,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentHint {
    Policy,
    Required,
}

/// An immutable reference binds both canonical and wire identity. Hints do not
/// authorize retries/consent; application policy and reconciliation still decide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingRecord {
    pub reference: String,
    pub kind: InventoryKind,
    pub name: String,
    pub capability: CapabilityId,
    pub source: Option<SourceId>,
    pub class: OperationClass,
    pub side_effects: SideEffects,
    pub idempotency: Idempotency,
    pub idempotency_key: Option<String>,
    pub retry_safety: RetrySafety,
    pub consent: ConsentHint,
    /// Required for tools, including tools without advertised outputSchema.
    /// Resources use the fixed MCP text/blob result envelope instead.
    pub result_schema: Option<Value>,
}
impl MappingRecord {
    /// Existing CG-03 type for explicit resolution consumers; never registers it.
    pub fn capability_definition(&self) -> CapabilityDefinition {
        CapabilityDefinition::new(self.capability.clone(), self.class.capability_class())
    }
    fn validate(&self) -> Result<(), MappingError> {
        let expected = match self.class {
            OperationClass::Read | OperationClass::Search | OperationClass::Inspect => {
                SideEffects::None
            }
            OperationClass::Mutate => SideEffects::Mutation,
            OperationClass::Admin => SideEffects::Administrative,
        };
        if !reference(&self.reference)
            || self.name.is_empty()
            || self.name.len() > 96
            || self.side_effects != expected
            || (self.idempotency == Idempotency::ProviderKey) != self.idempotency_key.is_some()
            || self
                .idempotency_key
                .as_ref()
                .is_some_and(|k| k.is_empty() || k.len() > 96)
            || (self.retry_safety == RetrySafety::ReadOnly && expected != SideEffects::None)
            || (self.retry_safety == RetrySafety::Idempotent
                && self.idempotency == Idempotency::NonIdempotent)
            || (self.kind == InventoryKind::Resource
                && (self.source.is_none()
                    || !matches!(self.class, OperationClass::Read | OperationClass::Inspect)
                    || self.idempotency_key.is_some()
                    || self.result_schema.is_some()))
            || self.kind == InventoryKind::Prompt
        {
            return Err(MappingError::Metadata);
        }
        if self.kind == InventoryKind::Tool {
            object_schema(self.result_schema.as_ref().ok_or(MappingError::Schema)?)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    mapping_version: String,
    records: Vec<MappingRecord>,
}

/// Only a bounded, strict parser produces this read-only mapping document.
#[derive(Debug, Clone)]
pub struct MappingDocument(Document);
impl MappingDocument {
    pub fn from_json(bytes: &[u8]) -> Result<Self, MappingError> {
        if bytes.len() > MAX_MAPPING_BYTES {
            return Err(MappingError::Encoding);
        }
        let value = super::json::decode(bytes).map_err(|_| MappingError::Encoding)?;
        let mut doc: Document =
            serde_json::from_value(value).map_err(|_| MappingError::Encoding)?;
        if doc.mapping_version != MAPPING_VERSION {
            return Err(MappingError::Version);
        }
        if doc.records.len() > 256 {
            return Err(MappingError::Metadata);
        }
        let (mut refs, mut wire, mut capabilities, mut sources) = (
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
        );
        for r in &doc.records {
            r.validate()?;
            if !refs.insert(&r.reference) || !wire.insert((&r.kind, &r.name)) {
                return Err(MappingError::DuplicateMapping);
            }
            if !capabilities.insert(&r.capability)
                || r.source.as_ref().is_some_and(|s| !sources.insert(s))
            {
                return Err(MappingError::ConflictingMapping);
            }
        }
        doc.records.sort_by(|a, b| a.reference.cmp(&b.reference));
        Ok(Self(doc))
    }
    pub fn records(&self) -> &[MappingRecord] {
        &self.0.records
    }
    pub fn canonical_json(&self) -> Vec<u8> {
        canonical(&self.0)
    }
    pub fn digest(&self) -> ContentDigest {
        hash(&self.0)
    }
}
fn reference(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 96
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-".contains(&b))
}
fn canonical(value: &impl Serialize) -> Vec<u8> {
    serde_json::to_vec(&serde_json::to_value(value).expect("mapping serialization"))
        .expect("mapping encoding")
}
fn hash(value: &impl Serialize) -> ContentDigest {
    ContentDigest::new(format!("{:x}", Sha256::digest(canonical(value)))).expect("SHA-256 digest")
}
/// Manifest entry digest: versioned canonical bundle of the complete wire entry.
/// Includes annotations as untrusted hints, never as operation classification.
pub fn discovery_schema_digest(kind: InventoryKind, entry: &Value) -> ContentDigest {
    hash(&json!({"discovery_schema_version":MAPPING_VERSION,"kind":kind,"entry":entry}))
}
fn object_schema(value: &Value) -> Result<(), MappingError> {
    if value["type"] != "object" {
        return Err(MappingError::Schema);
    }
    schema::validate(value).map_err(|_| MappingError::Schema)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MappedEntry {
    pub mapping: MappingRecord,
    pub wire_identity: String,
    pub argument_schema_digest: ContentDigest,
    pub result_schema_digest: ContentDigest,
}
/// Complete invocation schemas recovered from the current validated discovery.
/// The schemas are data for output validation and never grant connector access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedToolSchema {
    pub target: ConnectorTarget,
    pub capability: CapabilityId,
    pub inventory_revision: ContentDigest,
    /// Complete raw wire-schema digest, including documentation metadata.
    pub argument_schema_digest: ContentDigest,
    /// Complete wire-schema digest, including documentation metadata.
    pub wire_argument_schema_digest: ContentDigest,
    pub result_schema_digest: ContentDigest,
    pub argument_schema: serde_json::Value,
    pub result_schema: serde_json::Value,
}
/// Complete atomic snapshot; private fields prevent construction of an unchecked index.
#[derive(Debug, Clone)]
pub struct MappedInventory {
    inventory: ConnectorInventory,
    entries: Vec<MappedEntry>,
    canonical: Vec<u8>,
}
impl MappedInventory {
    pub fn inventory(&self) -> &ConnectorInventory {
        &self.inventory
    }
    pub fn entries(&self) -> &[MappedEntry] {
        &self.entries
    }
    pub fn canonical_json(&self) -> &[u8] {
        &self.canonical
    }
}

/// Pure provider-independent outer mapper, configured by the trusted host.
/// There are no catalog, policy, credential or transport handles.
#[derive(Clone)]
pub struct DiscoveryMapper {
    target: ConnectorTarget,
    manifest: ConnectorManifest,
    document: MappingDocument,
}
impl DiscoveryMapper {
    pub fn new(
        target: ConnectorTarget,
        manifest: ConnectorManifest,
        document: MappingDocument,
    ) -> Result<Self, MappingError> {
        if target.connector.as_str() != manifest.connector_id() {
            return Err(MappingError::Target);
        }
        Ok(Self {
            target,
            manifest,
            document,
        })
    }
    pub fn snapshot(&self, wire: &ProtocolInventory) -> Result<MappedInventory, MappingError> {
        if canonical(&json!([
            wire.tools,
            wire.resources,
            wire.templates,
            wire.prompts
        ]))
        .len()
            > 4_194_304
            || wire.tools.len() + wire.resources.len() + wire.templates.len() + wire.prompts.len()
                > 1024
        {
            return Err(MappingError::Discovery);
        }
        let mut entries = Vec::new();
        let mut seen = BTreeSet::new();
        let mut discovered = Vec::new();
        for (kind, list) in [
            (InventoryKind::Tool, &wire.tools),
            (InventoryKind::Resource, &wire.resources),
            (InventoryKind::Prompt, &wire.prompts),
        ] {
            let mut identities = BTreeSet::new();
            for item in list {
                let name = item["name"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or(MappingError::Discovery)?;
                let identity = if kind == InventoryKind::Resource {
                    item["uri"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .ok_or(MappingError::Discovery)?
                } else {
                    name
                };
                if !identities.insert(identity) || !seen.insert((kind.clone(), name.to_owned())) {
                    return Err(MappingError::Discovery);
                }
                let declared = self
                    .manifest
                    .definition()
                    .inventory
                    .iter()
                    .find(|e| e.kind == kind && e.name == name)
                    .ok_or(MappingError::UnknownEntry)?;
                if discovery_schema_digest(kind.clone(), item).as_str() != declared.schema_digest {
                    return Err(MappingError::SchemaDigest);
                }
                discovered.push(json!({"kind":kind,"identity":identity,"entry":item}));
                if kind == InventoryKind::Prompt {
                    continue;
                }
                let ref_key = if kind == InventoryKind::Tool {
                    &declared.capability_mapping_ref
                } else {
                    &declared.source_mapping_ref
                };
                let r = self
                    .document
                    .records()
                    .iter()
                    .find(|r| Some(&r.reference) == ref_key.as_ref())
                    .ok_or(MappingError::UnresolvedReference)?;
                if r.kind != kind
                    || r.name != name
                    || declared
                        .capability_mapping_ref
                        .iter()
                        .chain(declared.source_mapping_ref.iter())
                        .any(|v| *v != r.reference)
                    || declared.source_mapping_ref.is_some() != r.source.is_some()
                {
                    return Err(MappingError::UnresolvedReference);
                }
                let (input, output) = if kind == InventoryKind::Tool {
                    object_schema(&item["inputSchema"])?;
                    let output = r.result_schema.as_ref().ok_or(MappingError::Schema)?;
                    if item.get("outputSchema").is_some_and(|s| s != output) {
                        return Err(MappingError::Schema);
                    }
                    if let Some(key) = &r.idempotency_key
                        && (item["inputSchema"]["properties"][key]["type"] != "string"
                            || !item["inputSchema"]["required"]
                                .as_array()
                                .is_some_and(|a| a.contains(&json!(key))))
                    {
                        return Err(MappingError::Schema);
                    }
                    (hash(&item["inputSchema"]), hash(output))
                } else {
                    (
                        hash(
                            &json!({"type":"object","properties":{},"additionalProperties":false}),
                        ),
                        hash(
                            &json!({"resource_result_version":"1.0","content":"text_or_blob","uri":identity}),
                        ),
                    )
                };
                entries.push(MappedEntry {
                    mapping: r.clone(),
                    wire_identity: identity.into(),
                    argument_schema_digest: input,
                    result_schema_digest: output,
                });
            }
        }
        if seen.len() != self.manifest.definition().inventory.len() {
            return Err(MappingError::MissingEntry);
        }
        if entries.len() != self.document.records().len() {
            return Err(MappingError::UnresolvedReference);
        }
        let mut templates = wire.templates.clone();
        let mut template_ids = BTreeSet::new();
        for t in &templates {
            let id = t["uriTemplate"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or(MappingError::Discovery)?;
            if !template_ids.insert(id) || !t["name"].as_str().is_some_and(|s| !s.is_empty()) {
                return Err(MappingError::Discovery);
            }
        }
        templates.sort_by(|a, b| a["uriTemplate"].as_str().cmp(&b["uriTemplate"].as_str()));
        discovered.sort_by_key(canonical);
        entries.sort_by(|a, b| a.mapping.capability.cmp(&b.mapping.capability));
        let audit = json!({"inventory_version":MAPPING_VERSION,"connector":self.target.connector.as_str(),"scope":self.target.scope.as_str(),"manifest_digest":self.manifest.digest(),"protocol":self.manifest.definition().protocol.version,"mapping_digest":self.document.digest(),"discovery":discovered,"templates":templates,"entries":entries});
        let revision = hash(&audit);
        let mut resources: Vec<_> = entries
            .iter()
            .filter_map(|e| e.mapping.source.clone())
            .collect();
        resources.sort();
        let capabilities = entries
            .iter()
            .map(|e| ConnectorCapability {
                capability: e.mapping.capability.clone(),
                class: e.mapping.class.operation_class(),
                argument_schema_digest: e.argument_schema_digest.clone(),
                result_schema_digest: e.result_schema_digest.clone(),
            })
            .collect();
        Ok(MappedInventory {
            inventory: ConnectorInventory {
                target: self.target.clone(),
                revision,
                capabilities,
                resources,
            },
            entries,
            canonical: canonical(&audit),
        })
    }
    /// Return full tool schemas only when the exact complete inventory revision
    /// remains current and the manifest/mapping still bind the same schemas.
    pub fn tool_schema(
        &self,
        wire: &ProtocolInventory,
        capability: &CapabilityId,
        expected_revision: &ContentDigest,
    ) -> Result<MappedToolSchema, MappingError> {
        let snapshot = self.snapshot(wire)?;
        if &snapshot.inventory.revision != expected_revision {
            return Err(MappingError::StaleInventory);
        }
        let entry = snapshot
            .entries
            .iter()
            .find(|entry| {
                entry.mapping.kind == InventoryKind::Tool && &entry.mapping.capability == capability
            })
            .ok_or(MappingError::UnknownEntry)?;
        let wire_tool = wire
            .tools
            .iter()
            .find(|tool| tool["name"].as_str() == Some(entry.mapping.name.as_str()))
            .ok_or(MappingError::MissingEntry)?;
        let argument_schema = wire_tool["inputSchema"].clone();
        let result_schema = entry
            .mapping
            .result_schema
            .clone()
            .ok_or(MappingError::Schema)?;
        let wire_argument_schema_digest = hash(&argument_schema);
        let argument_schema_digest = hash(&argument_schema);
        let result_schema_digest = hash(&result_schema);
        if argument_schema_digest != entry.argument_schema_digest
            || result_schema_digest != entry.result_schema_digest
        {
            return Err(MappingError::SchemaDigest);
        }
        Ok(MappedToolSchema {
            target: snapshot.inventory.target,
            capability: capability.clone(),
            inventory_revision: snapshot.inventory.revision,
            argument_schema_digest,
            wire_argument_schema_digest,
            result_schema_digest,
            argument_schema,
            result_schema,
        })
    }
    pub fn resolve(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
        wire: &ProtocolInventory,
    ) -> Result<WireOperation, MappingError> {
        if dispatch.request.target != self.target {
            return Err(MappingError::Target);
        }
        let snapshot = self.snapshot(wire)?;
        if dispatch.request.inventory_revision != snapshot.inventory.revision {
            return Err(MappingError::StaleInventory);
        }
        let r = self.record(dispatch, source)?;
        let entry = snapshot
            .entries
            .iter()
            .find(|e| e.mapping.reference == r.reference)
            .ok_or(MappingError::Request)?;
        if r.kind == InventoryKind::Resource {
            if !dispatch.request.arguments.is_empty() {
                return Err(MappingError::Request);
            }
            Ok(WireOperation::Resource {
                uri: entry.wire_identity.clone(),
            })
        } else {
            let input = &wire
                .tools
                .iter()
                .find(|t| t["name"] == r.name)
                .ok_or(MappingError::Request)?["inputSchema"];
            let args = dispatch
                .request
                .arguments
                .iter()
                .map(|(k, v)| {
                    Ok((
                        k.as_str().to_owned(),
                        super::host::typed(v).map_err(|_| MappingError::Request)?,
                    ))
                })
                .collect::<Result<serde_json::Map<_, _>, MappingError>>()?;
            schema::accepts(input, &Value::Object(args)).map_err(|_| MappingError::Request)?;
            Ok(WireOperation::Tool {
                name: entry.wire_identity.clone(),
                arguments: dispatch.request.arguments.clone(),
            })
        }
    }
    fn record(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
    ) -> Result<&MappingRecord, MappingError> {
        let r = self
            .document
            .records()
            .iter()
            .find(|r| r.capability == dispatch.request.capability)
            .ok_or(MappingError::Request)?;
        if dispatch.request.target != self.target
            || r.class.operation_class() != dispatch.request.class
            || (r.kind == InventoryKind::Resource && source != r.source.as_ref())
            || (r.kind == InventoryKind::Tool && source.is_some())
        {
            return Err(MappingError::Request);
        }
        Ok(r)
    }
}

/// Required #228 extension. There is no pass-through or default normalizer.
pub trait ConnectorResultNormalizer: Send + Sync {
    fn normalize(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
        mapping: &MappingRecord,
        result: ProtocolResult,
    ) -> Result<ConnectorObservation, Error>;
}
/// Read-only data retained at the actual checked discovery mapping boundary.
/// A matching snapshot is evidence data, never a current authorization grant.
#[derive(Default)]
pub(super) struct MappedInventoryCache(std::sync::Mutex<Option<MappedInventory>>);
impl MappedInventoryCache {
    pub(super) fn snapshot_for(
        &self,
        summary: &ConnectorInventory,
    ) -> Result<MappedInventory, Error> {
        let retained = self.0.lock().map_err(|_| Error::Unavailable)?;
        retained
            .as_ref()
            .filter(|s| s.inventory() == summary)
            .cloned()
            .ok_or(Error::InventoryChanged)
    }
}

/// Production mapping adapter for the existing admitted MCP host.
pub struct CanonicalConnectorMapping {
    mapper: DiscoveryMapper,
    normalizer: Box<dyn ConnectorResultNormalizer>,
    retained: Option<Arc<MappedInventoryCache>>,
}
impl CanonicalConnectorMapping {
    pub fn new(mapper: DiscoveryMapper, normalizer: Box<dyn ConnectorResultNormalizer>) -> Self {
        Self {
            mapper,
            normalizer,
            retained: None,
        }
    }
}
impl CanonicalConnectorMapping {
    pub(super) fn new_retaining(
        mapper: DiscoveryMapper,
        normalizer: Box<dyn ConnectorResultNormalizer>,
        retained: Arc<MappedInventoryCache>,
    ) -> Self {
        Self {
            mapper,
            normalizer,
            retained: Some(retained),
        }
    }
}
impl ConnectorMapping for CanonicalConnectorMapping {
    fn inventory(
        &self,
        target: &ConnectorTarget,
        manifest: &ConnectorManifest,
        wire: &ProtocolInventory,
    ) -> Result<ConnectorInventory, Error> {
        // Clear a prior snapshot even when this new mapping attempt is denied.
        let mut retained = self
            .retained
            .as_ref()
            .map(|r| r.0.lock().map_err(|_| Error::Unavailable))
            .transpose()?;
        if let Some(slot) = retained.as_mut() {
            **slot = None;
        }
        if *target != self.mapper.target || *manifest != self.mapper.manifest {
            return Err(Error::Configuration);
        }
        let snapshot = self.mapper.snapshot(wire).map_err(|_| Error::Schema)?;
        let summary = snapshot.inventory.clone();
        if let Some(slot) = retained.as_mut() {
            **slot = Some(snapshot);
        }
        Ok(summary)
    }
    fn operation(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
        wire: &ProtocolInventory,
    ) -> Result<WireOperation, Error> {
        self.mapper.resolve(dispatch, source, wire).map_err(|e| {
            if e == MappingError::StaleInventory {
                Error::InventoryChanged
            } else {
                Error::Schema
            }
        })
    }
    fn normalize(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
        result: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        let r = self
            .mapper
            .record(dispatch, source)
            .map_err(|_| Error::Schema)?;
        match &result {
            ProtocolResult::Tool {
                structured,
                is_error,
                ..
            } if r.kind == InventoryKind::Tool => {
                if !is_error {
                    schema::accepts(
                        r.result_schema.as_ref().ok_or(Error::Schema)?,
                        structured.as_ref().ok_or(Error::Schema)?,
                    )?;
                }
            }
            ProtocolResult::Resource { .. } if r.kind == InventoryKind::Resource => (),
            _ => return Err(Error::Schema),
        }
        self.normalizer.normalize(dispatch, source, r, result)
    }
    fn tool_schema(
        &self,
        target: &ConnectorTarget,
        manifest: &ConnectorManifest,
        wire: &ProtocolInventory,
        capability: &CapabilityId,
        inventory_revision: &ContentDigest,
    ) -> Result<MappedToolSchema, Error> {
        if *target != self.mapper.target || *manifest != self.mapper.manifest {
            return Err(Error::Configuration);
        }
        self.mapper
            .tool_schema(wire, capability, inventory_revision)
            .map_err(|error| {
                if error == MappingError::StaleInventory {
                    Error::InventoryChanged
                } else {
                    Error::Schema
                }
            })
    }
}

#[cfg(test)]
#[path = "mapping_tests.rs"]
mod tests;
