//! Explicit, immutable local admission configuration. No ambient project discovery.
use gateway_application::codex::*;
use gateway_domain::ContextScopeId;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    schema_version: u32,
    mappings: Vec<Mapping>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mapping {
    repository: PathBuf,
    scope: Value,
    canonical_scope: ContextScopeId,
    principal: String,
    session_id: String,
    revision: String,
    resources: Vec<Value>,
}

pub struct LocalWorkspaceResolver {
    mappings: Vec<Mapping>,
}
fn canonical_directory(path: &Path) -> Result<PathBuf, FacadeError> {
    if !path.is_absolute() || !path.is_dir() {
        return Err(FacadeError::ScopeDenied);
    }
    path.canonicalize().map_err(|_| FacadeError::ScopeDenied)
}
impl LocalWorkspaceResolver {
    pub fn from_json(text: &str) -> Result<Self, FacadeError> {
        if text.len() >= 1_048_576 {
            return Err(FacadeError::LimitExceeded);
        }
        let value = crate::local_mcp::decode::decode(text.as_bytes())
            .map_err(|_| FacadeError::InvalidInput)?;
        let mut config: Admission =
            serde_json::from_value(value).map_err(|_| FacadeError::InvalidInput)?;
        if config.schema_version != 1 {
            return Err(FacadeError::UnsupportedVersion);
        }
        if config.mappings.is_empty() || config.mappings.len() > 1024 {
            return Err(FacadeError::InvalidInput);
        }
        for mapping in &mut config.mappings {
            mapping.repository = canonical_directory(&mapping.repository)?;
            mapping.binding()?.validate()?;
            let common = contracts::artifact("common.schema.json").unwrap();
            let schema = contracts::artifact("resource.schema.json").unwrap();
            let mut identities = std::collections::BTreeSet::new();
            for resource in &mapping.resources {
                if !contracts::valid(resource, &schema, &common)
                    || resource["scope"] != mapping.scope
                    || resource["provenance"]
                        .as_array()
                        .is_none_or(|p| p.is_empty())
                    || !resource["provenance"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|p| p["reference"] == resource["reference"])
                    || !identities.insert(resource["reference"].to_string())
                {
                    return Err(FacadeError::InvalidInput);
                }
                let digest = format!(
                    "sha256:{:x}",
                    Sha256::digest(resource["document"].to_string().as_bytes())
                );
                if resource["reference"]["digest"] != digest {
                    return Err(FacadeError::StaleRevision);
                }
            }
        }
        for (index, mapping) in config.mappings.iter().enumerate() {
            if config.mappings[..index].iter().any(|previous| {
                previous.canonical_scope == mapping.canonical_scope
                    && (previous.scope["workspace_id"] != mapping.scope["workspace_id"]
                        || previous.scope["project_id"] != mapping.scope["project_id"])
            }) {
                return Err(FacadeError::ScopeDenied);
            }
        }
        Ok(Self {
            mappings: config.mappings,
        })
    }
    pub fn host(&self, binding: &ScopeBinding) -> Result<LocalCodexHost, FacadeError> {
        binding.validate()?;
        let mappings: Vec<_> = self
            .mappings
            .iter()
            .filter(|m| {
                m.binding().is_ok_and(|b| {
                    b.scope == binding.scope
                        && b.canonical_scope == binding.canonical_scope
                        && b.session == binding.session
                        && b.mapping_revision == binding.mapping_revision
                })
            })
            .collect();
        if mappings.len() != 1 {
            return Err(FacadeError::ScopeDenied);
        }
        Ok(LocalCodexHost {
            binding: binding.clone(),
            resources: mappings[0].resources.clone(),
        })
    }
}
impl Mapping {
    fn binding(&self) -> Result<ScopeBinding, FacadeError> {
        Ok(ScopeBinding {
            scope: self.scope.clone(),
            canonical_scope: self.canonical_scope.clone(),
            session: SessionContext {
                principal: self.principal.clone(),
                session_id: self.session_id.clone(),
                connection_id: self.scope["binding_id"]
                    .as_str()
                    .ok_or(FacadeError::ScopeDenied)?
                    .into(),
            },
            mapping_revision: self.revision.clone(),
        })
    }
}
impl WorkspaceResolver for LocalWorkspaceResolver {
    fn resolve(&self, reference: &WorkspaceReference) -> Result<ScopeBinding, FacadeError> {
        let cwd = canonical_directory(Path::new(&reference.working_directory))?;
        let repository = canonical_directory(Path::new(&reference.repository))?;
        // Every containing root counts: overlapping configurations are ambiguous,
        // even when a caller supplies the desired repository.
        let mappings: Vec<_> = self
            .mappings
            .iter()
            .filter(|m| cwd.starts_with(&m.repository))
            .collect();
        if mappings.len() != 1 || mappings[0].repository != repository {
            return Err(FacadeError::ScopeDenied);
        }
        mappings[0].binding()
    }
}

pub struct LocalCodexHost {
    binding: ScopeBinding,
    resources: Vec<Value>,
}
impl LocalCodexHost {
    fn check(&self, call: &Call) -> Result<(), FacadeError> {
        if call.scope != self.binding.scope
            || call.canonical_scope != self.binding.canonical_scope
            || call.binding.session != self.binding.session
            || call.binding.mapping_revision != self.binding.mapping_revision
        {
            return Err(FacadeError::ScopeDenied);
        }
        Ok(())
    }
}
impl CodexHost for LocalCodexHost {
    fn authorize(&self, call: &Call) -> Result<(), FacadeError> {
        self.check(call)?;
        match call.operation.as_str() {
            "situation.inspect" | "situation.assess" | "resource.read" => {}
            _ => return Err(FacadeError::UnsupportedCapability),
        }
        // Inline content is allowed only when it matches an admitted, classified
        // immutable source. Caller content cannot choose its own classification.
        if let Some(source) = call.input.get("situation") {
            if source["kind"] == "document" {
                let admitted = self
                    .resources
                    .iter()
                    .find(|r| {
                        r["reference"]["contract"] == source["contract"]
                            && r["reference"]["contract_version"] == source["contract_version"]
                            && r["document"] == source["document"]
                    })
                    .ok_or(FacadeError::SensitivityDenied)?;
                if admitted["provenance"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["sensitivity"] == "SECRET")
                {
                    return Err(FacadeError::SensitivityDenied);
                }
            }
        }
        Ok(())
    }
    fn reference(&self, call: &Call, reference: &Value) -> Result<ReferenceRecord, FacadeError> {
        self.check(call)?;
        let resource = self
            .resources
            .iter()
            .find(|r| r["reference"] == *reference)
            .ok_or(FacadeError::ReferenceUnavailable)?;
        let provenance = resource["provenance"].as_array().unwrap().clone();
        if provenance.iter().any(|p| p["sensitivity"] == "SECRET") {
            return Err(FacadeError::SensitivityDenied);
        }
        Ok(ReferenceRecord {
            scope: self.binding.scope.clone(),
            session: self.binding.session.clone(),
            reference: reference.clone(),
            document: resource["document"].to_string(),
            provenance,
        })
    }
    fn resource_reference(
        &self,
        call: &Call,
        id: &str,
        revision: &str,
        digest: &str,
    ) -> Result<Value, FacadeError> {
        self.check(call)?;
        let matches: Vec<_> = self
            .resources
            .iter()
            .filter(|r| {
                r["reference"]["id"] == id
                    && r["reference"]["revision"] == revision
                    && r["reference"]["digest"] == digest
            })
            .collect();
        if matches.len() != 1 {
            return Err(FacadeError::ReferenceUnavailable);
        }
        Ok(matches[0]["reference"].clone())
    }
    fn project(
        &self,
        call: &Call,
        contract: &str,
        document: Value,
    ) -> Result<Projection, FacadeError> {
        self.check(call)?;
        let mut provenance = vec![];
        for resource in &self.resources {
            if resource["document"] == document
                || call.input["situation"]["document"] == resource["document"]
            {
                for p in resource["provenance"].as_array().unwrap() {
                    if p["sensitivity"] == "SECRET" {
                        return Err(FacadeError::SensitivityDenied);
                    }
                    if !provenance.contains(p) {
                        provenance.push(p.clone());
                    }
                }
            }
        }
        // Reference inputs are propagated independently by the facade.
        Ok(Projection {
            source: json!({"kind":"document","contract":contract,"contract_version":"1.0","document":document}),
            explainability: vec![],
            evidence: vec![],
            provenance,
        })
    }
}
