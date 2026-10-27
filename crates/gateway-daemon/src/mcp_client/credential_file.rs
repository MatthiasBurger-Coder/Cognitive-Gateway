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
pub struct CredentialFileResolver {
    path: PathBuf,
}
impl CredentialFileResolver {
    pub fn new(path: PathBuf) -> Result<Self, ConnectorFailureCode> {
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
    if value.len() > NAME_BYTES || value.contains('\0') || value.trim() != value {
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
mod tests {
    use super::*;
    use crate::mcp_client::{
        CallContext, Error, Limits,
        transport::{CredentialedStdioFactory, StdioConfig, TransportFactory},
    };
    use serde_json::{Value, json};
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };
    struct SecretFile {
        root: PathBuf,
        path: PathBuf,
    }
    impl SecretFile {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "cg-credential-file-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            Self {
                path: root.join("secrets.json"),
                root,
            }
        }
        fn install(&self, document: &Value) {
            std::fs::write(&self.path, document.to_string()).unwrap();
        }
        fn resolver(&self) -> CredentialFileResolver {
            CredentialFileResolver::new(self.path.clone()).unwrap()
        }
    }
    impl Drop for SecretFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn profile() -> CredentialProfileRef {
        CredentialProfileRef::new(NonEmptyText::new("operator.records").unwrap())
    }
    fn document(secret: &str) -> Value {
        json!({"schema_version":1,"profiles":{"operator.records":{
            "connector":"records","server":"records-peer","tools":["records.read"],"generation":1,
            "environment":{"API_TOKEN":secret}
        }}})
    }
    fn denied(resolver: &CredentialFileResolver) {
        let error = resolver.resolve(&profile()).err().unwrap();
        assert_eq!(error, ConnectorFailureCode::CredentialUnavailable);
        assert!(!format!("{error:?}").contains("canary-secret"));
    }
    #[test]
    fn loading_rotation_and_profile_removal_use_only_current_exact_profile() {
        let file = SecretFile::new();
        let resolver = file.resolver();
        denied(&resolver);
        file.install(&document("canary-secret-old"));
        let old = resolver.resolve(&profile()).unwrap();
        assert_eq!(old.environment["API_TOKEN"], "canary-secret-old");
        assert!(old.scope.permits("records", "records-peer", "records.read"));
        assert!(!old.scope.permits("records", "other", "records.read"));
        assert!(
            !old.scope
                .permits("records", "records-peer", "records.write")
        );
        let mut next = document("canary-secret-new");
        next["profiles"]["operator.records"]["generation"] = json!(2);
        file.install(&next);
        let new = resolver.resolve(&profile()).unwrap();
        assert_eq!(new.generation, 2);
        assert_eq!(new.environment["API_TOKEN"], "canary-secret-new");
        next["profiles"]["other"] = next["profiles"]["operator.records"].clone();
        next["profiles"]
            .as_object_mut()
            .unwrap()
            .remove("operator.records");
        file.install(&next);
        denied(&resolver);
        assert!(
            resolver
                .resolve(&CredentialProfileRef::new(
                    NonEmptyText::new("other").unwrap()
                ))
                .is_ok()
        );
    }
    #[test]
    fn strict_document_rejects_ambiguity_malformed_data_and_unbounded_material() {
        let file = SecretFile::new();
        let resolver = file.resolver();
        assert!(CredentialFileResolver::new("relative.json".into()).is_err());
        assert!(
            resolver
                .resolve(&CredentialProfileRef::new(
                    NonEmptyText::new("p".repeat(NAME_BYTES + 1)).unwrap()
                ))
                .is_err()
        );
        assert!(
            CredentialFileResolver::new(file.root.clone())
                .unwrap()
                .resolve(&profile())
                .is_err()
        );
        for raw in [br#"{"schema_version":1,"schema_version":1,"profiles":{}}"#.as_slice(),
            br#"{"schema_version":1,"profiles":{"same":{},"same":{}}}"#,
            br#"{"schema_version":1,"profiles":{"p":{"environment":{"API_TOKEN":"canary-secret","API_TOKEN":"other"}}}}"#,
            b"\xff", b"{} trailing", b"{\"canary-secret\":" ] {
            std::fs::write(&file.path, raw).unwrap(); denied(&resolver);
        }
        let deep = format!("{}0{}", "[".repeat(66), "]".repeat(66));
        std::fs::write(&file.path, deep).unwrap();
        denied(&resolver);
        std::fs::write(&file.path, vec![b' '; FILE_BYTES + 1]).unwrap();
        denied(&resolver);
        for case in [
            "version",
            "unknown-root",
            "unknown-profile",
            "invalid-unrelated-profile",
            "bad-key",
            "empty-value",
            "nul",
            "value-bound",
            "env-bound",
            "tools-bound",
            "duplicate-tool",
            "wildcard",
            "name-bound",
            "profiles-bound",
            "generation-type",
            "empty-id",
        ] {
            let mut value = document("canary-secret");
            match case {
                "version" => value["schema_version"] = json!(2),
                "unknown-root" => value["secret"] = json!("canary-secret"),
                "unknown-profile" => value["profiles"]["operator.records"]["extra"] = json!(true),
                "invalid-unrelated-profile" => {
                    value["profiles"]["other"] = value["profiles"]["operator.records"].clone();
                    value["profiles"]["other"]["extra"] = json!(true);
                }
                "bad-key" => {
                    value["profiles"]["operator.records"]["environment"] =
                        json!({"PATH":"canary-secret"})
                }
                "empty-value" => {
                    value["profiles"]["operator.records"]["environment"]["API_TOKEN"] = json!("")
                }
                "nul" => {
                    value["profiles"]["operator.records"]["environment"]["API_TOKEN"] =
                        json!("canary-secret\0")
                }
                "value-bound" => {
                    value["profiles"]["operator.records"]["environment"]["API_TOKEN"] =
                        json!("x".repeat(VALUE_BYTES + 1))
                }
                "env-bound" => {
                    value["profiles"]["operator.records"]["environment"] = serde_json::to_value(
                        (0..=ENVIRONMENT)
                            .map(|n| (format!("TOKEN_{n}"), "x"))
                            .collect::<BTreeMap<_, _>>(),
                    )
                    .unwrap()
                }
                "tools-bound" => {
                    value["profiles"]["operator.records"]["tools"] =
                        json!((0..=TOOLS).map(|n| format!("tool{n}")).collect::<Vec<_>>())
                }
                "duplicate-tool" => {
                    value["profiles"]["operator.records"]["tools"] = json!(["read", "read"])
                }
                "wildcard" => value["profiles"]["operator.records"]["tools"] = json!(["*"]),
                "name-bound" => {
                    value["profiles"]["operator.records"]["server"] =
                        json!("s".repeat(NAME_BYTES + 1))
                }
                "profiles-bound" => {
                    value["profiles"] = serde_json::to_value(
                        (0..=PROFILES)
                            .map(|n| {
                                (
                                    format!("profile{n}"),
                                    value["profiles"]["operator.records"].clone(),
                                )
                            })
                            .collect::<BTreeMap<_, _>>(),
                    )
                    .unwrap()
                }
                "generation-type" => {
                    value["profiles"]["operator.records"]["generation"] = json!(-1)
                }
                _ => {
                    let entry = value["profiles"]["operator.records"].clone();
                    value["profiles"] = json!({"":entry});
                }
            }
            file.install(&value);
            denied(&resolver);
        }
    }
    #[test]
    fn existing_factory_injects_material_and_rotates_scope_only_on_reconnect() {
        let file = SecretFile::new();
        file.install(&document("canary-secret-old"));
        let factory = CredentialedStdioFactory::new(StdioConfig {
            executable: "/usr/bin/python3".into(),
            arguments: vec!["-c".into(), "import os,json; print(json.dumps({'injected':os.environ.get('API_TOKEN') == 'canary-secret-old', 'rotated':os.environ.get('API_TOKEN') == 'canary-secret-new', 'inherited':'HOME' in os.environ}),flush=True)".into()],
            directory: file.root.clone(), environment: BTreeMap::new(),
        }, Arc::new(file.resolver()), profile(), "records", "records-peer");
        let context = CallContext::new(
            NonEmptyText::new("credential-probe").unwrap(),
            Instant::now() + Duration::from_secs(5),
        );
        let mut transport = factory.open(&context, &Limits::default()).unwrap();
        let safe: Value = serde_json::from_slice(&transport.receive(&context).unwrap()).unwrap();
        assert_eq!(
            safe,
            json!({"injected":true,"rotated":false,"inherited":false})
        );
        assert!(factory.contains_secret("echo canary-secret-old"));
        let mut next = document("canary-secret-new");
        next["profiles"]["operator.records"]["tools"] = json!(["records.write"]);
        next["profiles"]["operator.records"]["generation"] = json!(2);
        file.install(&next);
        assert!(
            factory
                .permits_tool("records", "records-peer", "records.read")
                .is_ok()
        );
        assert_eq!(
            factory.permits_tool("records", "records-peer", "records.write"),
            Err(Error::CredentialScopeDenied)
        );
        transport.close();
        let mut reconnected = factory.open(&context, &Limits::default()).unwrap();
        let safe: Value = serde_json::from_slice(&reconnected.receive(&context).unwrap()).unwrap();
        assert_eq!(
            safe,
            json!({"injected":false,"rotated":true,"inherited":false})
        );
        assert!(factory.contains_secret("echo canary-secret-new"));
        assert!(!factory.contains_secret("echo canary-secret-old"));
        assert!(
            factory
                .permits_tool("records", "records-peer", "records.write")
                .is_ok()
        );
        reconnected.close();
        file.install(&json!({"schema_version":1,"profiles":{}}));
        assert_eq!(
            factory.open(&context, &Limits::default()).err().unwrap(),
            Error::CredentialUnavailable
        );
        assert_eq!(
            factory.permits_tool("records", "records-peer", "records.write"),
            Err(Error::CredentialScopeDenied)
        );
    }
}
