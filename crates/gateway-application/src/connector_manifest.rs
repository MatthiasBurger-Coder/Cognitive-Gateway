//! Versioned, reference-only connector descriptions. Validation is not admission.
//! Canonical encoding and migration rules: `docs/connector-manifest-v1.md`.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const MANIFEST_VERSION: &str = "1.0";
pub const MAX_MANIFEST_BYTES: usize = 262_144;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestError {
    Encoding,
    UnsupportedVersion,
    InvalidMetadata,
    DuplicateInventory,
    DigestMismatch,
    DuplicateIdentity,
    ConflictingIdentity,
}

/// Stable release numbers only; pre-release/build labels are outside v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorTransport {
    Stdio,
    StreamableHttp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protocol {
    pub name: String,
    pub version: String,
    pub rpc_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profiles {
    pub authentication: String,
    pub trust: String,
    pub sensitivity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryKind {
    Tool,
    Resource,
    Prompt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryEntry {
    pub kind: InventoryKind,
    pub name: String,
    pub schema_digest: String,
    pub capability_mapping_ref: Option<String>,
    pub source_mapping_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Health {
    pub contract_ref: String,
    pub interval_seconds: u32,
    pub timeout_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Enabled,
    Disabled,
    Deprecated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deprecation {
    pub replacement_connector_id: String,
    pub sunset_gateway_minor: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compatibility {
    pub port_version: String,
    pub gateway_major: u32,
    pub minimum_gateway_minor: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorManifestDefinition {
    pub manifest_version: String,
    pub connector_id: String,
    pub server_id: String,
    pub version: ConnectorVersion,
    pub server_version: ConnectorVersion,
    pub protocol: Protocol,
    pub transport: ConnectorTransport,
    pub configuration_ref: String,
    pub inventory: Vec<InventoryEntry>,
    pub profiles: Profiles,
    pub health: Health,
    pub lifecycle: Lifecycle,
    pub deprecation: Option<Deprecation>,
    pub compatibility: Compatibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    manifest: ConnectorManifestDefinition,
    digest: String,
}

/// Validated immutable snapshot. No Deserialize implementation bypasses validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorManifest(Envelope);

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-".contains(&b))
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn canonical<T: Serialize>(value: &T) -> Vec<u8> {
    // Value maps use lexical key order; v1 contains only ASCII strings and integers.
    serde_json::to_vec(&serde_json::to_value(value).expect("manifest serialization"))
        .expect("manifest encoding")
}

fn definition_digest(value: &ConnectorManifestDefinition) -> String {
    format!("{:x}", Sha256::digest(canonical(value)))
}

impl ConnectorManifestDefinition {
    fn validate(&mut self) -> Result<(), ManifestError> {
        if self.manifest_version != MANIFEST_VERSION
            || self.protocol.name != "mcp"
            || self.protocol.version != "2025-11-25"
            || self.protocol.rpc_version != "2.0"
            || self.compatibility.port_version != super::connectors::CONNECTOR_PORT_VERSION
        {
            return Err(ManifestError::UnsupportedVersion);
        }
        let refs = [
            &self.connector_id,
            &self.server_id,
            &self.configuration_ref,
            &self.profiles.authentication,
            &self.profiles.trust,
            &self.profiles.sensitivity,
            &self.health.contract_ref,
        ];
        if !refs.into_iter().all(|v| identifier(v))
            || self.health.interval_seconds == 0
            || self.health.interval_seconds > 86_400
            || self.health.timeout_seconds == 0
            || self.health.timeout_seconds > self.health.interval_seconds
            || self.inventory.len() > 256
        {
            return Err(ManifestError::InvalidMetadata);
        }
        match (&self.lifecycle, &self.deprecation) {
            (Lifecycle::Deprecated, Some(d))
                if identifier(&d.replacement_connector_id)
                    && d.replacement_connector_id != self.connector_id
                    && d.sunset_gateway_minor >= self.compatibility.minimum_gateway_minor => {}
            (Lifecycle::Enabled | Lifecycle::Disabled, None) => {}
            _ => return Err(ManifestError::InvalidMetadata),
        }
        let mut keys = BTreeSet::new();
        for entry in &self.inventory {
            if entry.name.is_empty()
                || entry.name.len() > 96
                || !entry
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                || !digest(&entry.schema_digest)
                || !entry
                    .capability_mapping_ref
                    .iter()
                    .chain(entry.source_mapping_ref.iter())
                    .all(|v| identifier(v))
                || match entry.kind {
                    InventoryKind::Tool => entry.capability_mapping_ref.is_none(),
                    InventoryKind::Resource => entry.source_mapping_ref.is_none(),
                    InventoryKind::Prompt => {
                        entry.capability_mapping_ref.is_some() || entry.source_mapping_ref.is_some()
                    }
                }
            {
                return Err(ManifestError::InvalidMetadata);
            }
            if !keys.insert((&entry.kind, &entry.name)) {
                return Err(ManifestError::DuplicateInventory);
            }
        }
        self.inventory
            .sort_by(|a, b| (&a.kind, &a.name).cmp(&(&b.kind, &b.name)));
        Ok(())
    }
}

impl ConnectorManifest {
    /// Parse an untrusted content-addressed envelope; duplicate JSON keys fail.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ManifestError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestError::Encoding);
        }
        let mut envelope: Envelope =
            serde_json::from_slice(bytes).map_err(|_| ManifestError::Encoding)?;
        envelope.manifest.validate()?;
        if !digest(&envelope.digest) || envelope.digest != definition_digest(&envelope.manifest) {
            return Err(ManifestError::DigestMismatch);
        }
        Ok(Self(envelope))
    }

    /// Author a snapshot from a strict definition. Hashing grants no trust or access.
    pub fn from_definition_json(bytes: &[u8]) -> Result<Self, ManifestError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestError::Encoding);
        }
        let mut manifest: ConnectorManifestDefinition =
            serde_json::from_slice(bytes).map_err(|_| ManifestError::Encoding)?;
        manifest.validate()?;
        Ok(Self(Envelope {
            digest: definition_digest(&manifest),
            manifest,
        }))
    }

    pub fn definition(&self) -> &ConnectorManifestDefinition {
        &self.0.manifest
    }

    pub fn canonical_json(&self) -> Vec<u8> {
        canonical(&self.0)
    }
    pub fn digest(&self) -> &str {
        &self.0.digest
    }
    pub fn connector_id(&self) -> &str {
        &self.0.manifest.connector_id
    }
    pub fn server_id(&self) -> &str {
        &self.0.manifest.server_id
    }
    pub fn version(&self) -> ConnectorVersion {
        self.0.manifest.version
    }

    /// Call only with trusted runtime configuration. Compatible never means authorized.
    pub fn compatibility(&self, runtime: &ConnectorCompatibilityProfile) -> CompatibilityDecision {
        let m = &self.0.manifest;
        if runtime.gateway.major != m.compatibility.gateway_major
            || runtime.gateway.minor < m.compatibility.minimum_gateway_minor
        {
            return CompatibilityDecision::GatewayVersion;
        }
        if !runtime.transports.contains(&m.transport) {
            return CompatibilityDecision::Transport;
        }
        match m.lifecycle {
            Lifecycle::Disabled => CompatibilityDecision::Disabled,
            Lifecycle::Deprecated
                if runtime.gateway.minor
                    >= m.deprecation
                        .as_ref()
                        .expect("validated deprecation")
                        .sunset_gateway_minor =>
            {
                CompatibilityDecision::Sunset
            }
            Lifecycle::Deprecated => CompatibilityDecision::CompatibleDeprecated,
            Lifecycle::Enabled => CompatibilityDecision::Compatible,
        }
    }

    /// No in-place update is authorized, even for compatible release numbers.
    pub fn upgrade_from(&self, previous: &Self) -> UpgradeDecision {
        if self.connector_id() != previous.connector_id()
            || self.server_id() != previous.server_id()
            || self.version().major != previous.version().major
            || self.version() < previous.version()
        {
            UpgradeDecision::Incompatible
        } else if self.digest() == previous.digest() {
            UpgradeDecision::Unchanged
        } else if self.version() == previous.version() {
            UpgradeDecision::ConflictingRelease
        } else {
            UpgradeDecision::ReadmissionRequired
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectorCompatibilityProfile {
    pub gateway: ConnectorVersion,
    /// Explicitly implemented and qualified transports, not server advertisements.
    pub transports: BTreeSet<ConnectorTransport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityDecision {
    Compatible,
    CompatibleDeprecated,
    GatewayVersion,
    Transport,
    Disabled,
    Sunset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeDecision {
    Unchanged,
    ReadmissionRequired,
    ConflictingRelease,
    Incompatible,
}

/// Validate one admission-scope snapshot atomically; retains no partial index.
/// Instance reuse across different scopes is decided by the future admission owner.
pub fn validate_manifest_set(manifests: &[ConnectorManifest]) -> Result<(), ManifestError> {
    let mut connectors = BTreeMap::new();
    let mut servers = BTreeSet::new();
    for m in manifests {
        if let Some(previous) = connectors.insert(m.connector_id(), m.digest()) {
            return Err(if previous == m.digest() {
                ManifestError::DuplicateIdentity
            } else {
                ManifestError::ConflictingIdentity
            });
        }
        if !servers.insert(m.server_id()) {
            return Err(ManifestError::ConflictingIdentity);
        }
    }
    Ok(())
}
