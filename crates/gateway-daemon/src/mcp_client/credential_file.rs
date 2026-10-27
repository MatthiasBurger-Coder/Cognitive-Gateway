//! Operator-owned, bounded credential material resolved anew per connection.
use super::{json, transport::valid_credential_key};
use gateway_application::connectors::{
    ConnectorFailureCode, ConnectorSecretResolver, CredentialProfileRef, CredentialScope,
    ResolvedCredential,
};
use gateway_domain::NonEmptyText;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::PathBuf,
};

const FILE_BYTES: usize = 65_536;
const PROFILES: usize = 32;
const ENVIRONMENT: usize = 32;
const TOOLS: usize = 128;
const NAME_BYTES: usize = 128;
const VALUE_BYTES: usize = 8192;

/// Separate operator secret file, never a main-admission document. No material
/// is cached: reconnect resolves the currently installed file. Existing live
/// connections keep their existing factory-owned credential scope and secrets.
pub(crate) struct CredentialFileResolver {
    path: PathBuf,
}
impl CredentialFileResolver {
    pub(crate) fn new(path: PathBuf) -> Result<Self, ConnectorFailureCode> {
        if !path.is_absolute() {
            return Err(ConnectorFailureCode::CredentialUnavailable);
        }
        Ok(Self { path })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema_version: u32,
    profiles: BTreeMap<String, Profile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    connector: String,
    server: String,
    tools: Vec<String>,
    generation: u64,
    environment: BTreeMap<String, String>,
}
fn name(value: String) -> Result<NonEmptyText, ConnectorFailureCode> {
    if value.len() > NAME_BYTES || value.chars().any(char::is_control) || value.trim() != value {
        return Err(ConnectorFailureCode::CredentialUnavailable);
    }
    NonEmptyText::new(value).map_err(|_| ConnectorFailureCode::CredentialUnavailable)
}
fn material(profile: Profile) -> Result<ResolvedCredential, ConnectorFailureCode> {
    let denied = ConnectorFailureCode::CredentialUnavailable;
    if profile.tools.len() > TOOLS || profile.environment.len() > ENVIRONMENT {
        return Err(denied);
    }
    let tool_count = profile.tools.len();
    let tools: BTreeSet<_> = profile
        .tools
        .into_iter()
        .map(name)
        .collect::<Result<_, _>>()?;
    if tools.len() != tool_count || tools.iter().any(|tool| tool.as_str().contains('*')) {
        return Err(denied);
    }
    for (key, value) in &profile.environment {
        if !valid_credential_key(key)
            || value.is_empty()
            || value.len() > VALUE_BYTES
            || value.contains('\0')
        {
            return Err(denied);
        }
    }
    Ok(ResolvedCredential::new(
        profile.environment,
        CredentialScope {
            connector: name(profile.connector)?,
            server: name(profile.server)?,
            tools,
        },
        profile.generation,
    ))
}
impl ConnectorSecretResolver for CredentialFileResolver {
    fn resolve(
        &self,
        profile: &CredentialProfileRef,
    ) -> Result<ResolvedCredential, ConnectorFailureCode> {
        let denied = ConnectorFailureCode::CredentialUnavailable;
        if profile.as_str().len() > NAME_BYTES {
            return Err(denied);
        }
        if !std::fs::metadata(&self.path).map_err(|_| denied)?.is_file() {
            return Err(denied);
        }
        let file = File::open(&self.path).map_err(|_| denied)?;
        if !file.metadata().map_err(|_| denied)?.is_file() {
            return Err(denied);
        }
        let mut bytes = Vec::new();
        file.take((FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| denied)?;
        if bytes.len() > FILE_BYTES {
            return Err(denied);
        }
        let document: Document = serde_json::from_value(json::decode(&bytes).map_err(|_| denied)?)
            .map_err(|_| denied)?;
        if document.schema_version != 1 || document.profiles.len() > PROFILES {
            return Err(denied);
        }
        let mut credentials = BTreeMap::new();
        for (id, entry) in document.profiles {
            let id = name(id)?;
            credentials.insert(id, material(entry)?);
        }
        credentials
            .remove(&name(profile.as_str().into())?)
            .ok_or(denied)
    }
}

#[cfg(test)]
#[path = "credential_file_tests.rs"]
mod tests;
