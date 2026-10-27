//! Exact historical mapped inventory DATA. This codec performs no discovery and
//! never claims that a server still exposes the retained operations.
use super::*;
use gateway_application::connectors::{
    ConnectorCapability, ConnectorInventory, ConnectorOperationClass, ConnectorTarget,
};
use gateway_domain::{CapabilityId, ContentDigest};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Inventory {
    contract: String,
    contract_version: SchemaVersion,
    connector: NonEmptyText,
    scope: ContextScopeId,
    revision: ContentDigest,
    capabilities: Vec<Capability>,
    resources: Vec<SourceId>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Capability {
    capability: CapabilityId,
    class: Class,
    argument_schema_digest: ContentDigest,
    result_schema_digest: ContentDigest,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Class {
    Read,
    Search,
    Inspect,
    Mutate,
    Admin,
}
fn invalid() -> SessionError {
    SessionError::InvalidInteraction
}
fn digest(value: &ContentDigest) -> bool {
    value.as_str().len() == 64
        && value
            .as_str()
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn class(value: ConnectorOperationClass) -> Class {
    match value {
        ConnectorOperationClass::Read => Class::Read,
        ConnectorOperationClass::Search => Class::Search,
        ConnectorOperationClass::Inspect => Class::Inspect,
        ConnectorOperationClass::Mutate => Class::Mutate,
        ConnectorOperationClass::Admin => Class::Admin,
    }
}
impl Class {
    fn native(self) -> ConnectorOperationClass {
        match self {
            Self::Read => ConnectorOperationClass::Read,
            Self::Search => ConnectorOperationClass::Search,
            Self::Inspect => ConnectorOperationClass::Inspect,
            Self::Mutate => ConnectorOperationClass::Mutate,
            Self::Admin => ConnectorOperationClass::Admin,
        }
    }
}

pub(super) fn encode(actual: &ConnectorInventory) -> Result<Value, SessionError> {
    serde_json::to_value(Inventory {
        contract: "cg.native-historical-connector-inventory".into(),
        contract_version: SchemaVersion::V1,
        connector: actual.target.connector.clone(),
        scope: actual.target.scope.clone(),
        revision: actual.revision.clone(),
        capabilities: actual
            .capabilities
            .iter()
            .map(|c| Capability {
                capability: c.capability.clone(),
                class: class(c.class),
                argument_schema_digest: c.argument_schema_digest.clone(),
                result_schema_digest: c.result_schema_digest.clone(),
            })
            .collect(),
        resources: actual.resources.clone(),
    })
    .map_err(|_| invalid())
}
pub(super) fn decode(
    value: Value,
    runtime: &RuntimeAdmission,
) -> Result<ConnectorInventory, SessionError> {
    runtime.0.limits.validate()?;
    let data: Inventory = serde_json::from_value(value).map_err(|_| invalid())?;
    if data.contract != "cg.native-historical-connector-inventory"
        || data.contract_version != SchemaVersion::V1
        || data.connector != runtime.0.connector
        || data.scope != runtime.0.scope
        || !digest(&data.revision)
        || data
            .capabilities
            .len()
            .checked_add(data.resources.len())
            .is_none_or(|n| n > runtime.0.limits.items)
        || data
            .capabilities
            .windows(2)
            .any(|pair| pair[0].capability >= pair[1].capability)
        || data.resources.windows(2).any(|pair| pair[0] >= pair[1])
        || data
            .capabilities
            .iter()
            .any(|c| !digest(&c.argument_schema_digest) || !digest(&c.result_schema_digest))
    {
        return Err(invalid());
    }
    Ok(ConnectorInventory {
        target: ConnectorTarget {
            connector: data.connector,
            scope: data.scope,
        },
        revision: data.revision,
        capabilities: data
            .capabilities
            .into_iter()
            .map(|c| ConnectorCapability {
                capability: c.capability,
                class: c.class.native(),
                argument_schema_digest: c.argument_schema_digest,
                result_schema_digest: c.result_schema_digest,
            })
            .collect(),
        resources: data.resources,
    })
}
