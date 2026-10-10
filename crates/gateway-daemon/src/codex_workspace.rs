//! Explicit, immutable local admission configuration. No ambient project discovery.
use gateway_application::codex::*;
use gateway_domain::ContextScopeId;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Read;
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
    #[serde(default)]
    canonical: Option<crate::codex_canonical::CanonicalAdmission>,
    #[serde(default)]
    sessions: Option<crate::local_sessions::SessionAdmission>,
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
        if !gateway_application::codex::security::credential_free(&value) {
            return Err(FacadeError::SensitivityDenied);
        }
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
            if let Some(canonical) = &mut mapping.canonical {
                if mapping
                    .resources
                    .iter()
                    .any(|r| r["reference"]["id"] == "local-resolution")
                {
                    return Err(FacadeError::InvalidInput);
                }
                canonical.catalog = canonical_directory(&canonical.catalog)?;
                if !canonical.catalog.starts_with(&mapping.repository) {
                    return Err(FacadeError::ScopeDenied);
                }
                for (reference, contract) in [
                    (&canonical.plan, "cg.plan"),
                    (&canonical.rules, "cg.composition-rules"),
                    (&canonical.process, "cg.process-snapshot"),
                ] {
                    if reference["contract"] != contract {
                        return Err(FacadeError::InvalidInput);
                    }
                }
                canonical.pin(&mapping.resources)?;
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
            canonical: mappings[0].canonical.clone(),
            sessions: mappings[0].sessions.clone(),
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
    pub(crate) binding: ScopeBinding,
    pub(crate) resources: Vec<Value>,
    pub(crate) canonical: Option<crate::codex_canonical::CanonicalAdmission>,
    pub(crate) sessions: Option<crate::local_sessions::SessionAdmission>,
}
impl LocalCodexHost {
    fn canonical_provenance(&self, reference: &Value) -> Result<Vec<Value>, FacadeError> {
        let canonical = self
            .canonical
            .as_ref()
            .ok_or(FacadeError::UnsupportedCapability)?;
        let mut provenance = vec![];
        for source in [&canonical.plan, &canonical.rules, &canonical.process] {
            let resource = self
                .resources
                .iter()
                .find(|r| r["reference"] == *source)
                .ok_or(FacadeError::ReferenceUnavailable)?;
            for p in resource["provenance"].as_array().unwrap() {
                if p["sensitivity"] == "SECRET" {
                    return Err(FacadeError::SensitivityDenied);
                }
                if !provenance.contains(p) {
                    provenance.push(p.clone());
                }
            }
        }
        // Generated identity exposes the exact immutable result and its source lineage.
        let sensitivity = provenance
            .iter()
            .map(|p| p["sensitivity"].as_str().unwrap())
            .max_by_key(|s| match *s {
                "CONFIDENTIAL" => 4,
                "INTERNAL" => 3,
                "NORMAL" => 2,
                _ => 1,
            })
            .unwrap_or("NORMAL");
        provenance.push(json!({"reference":reference,"source_id":self.binding.session.connection_id,
            "source_revision":self.binding.mapping_revision,"freshness":"current","sensitivity":sensitivity,
            "lineage":[canonical.plan.clone(),canonical.rules.clone(),canonical.process.clone()]}));
        Ok(provenance)
    }
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
    fn operation_policy(&self, call: &Call) -> Result<OperationPolicy, FacadeError> {
        self.check(call)?;
        // The immutable workspace admission grants only these local inspections.
        // Discovery, execution depth and input documents cannot enlarge this list.
        let mut operations = vec!["situation.inspect", "situation.assess", "resource.read"];
        if self.canonical.is_some() {
            operations.extend(["capabilities.resolve", "state.explain", "context.compile"]);
        }
        let ids: Vec<_> = operations
            .into_iter()
            .map(operation_capability)
            .collect::<Result<_, _>>()?;
        let authority = gateway_policy::PolicyAuthority {
            policies: vec![
                gateway_domain::PolicyDefinition::new(
                    gateway_domain::PolicyId::new("local-workspace-inspection").unwrap(),
                    "Admitted immutable local inspection only",
                    ids.clone(),
                )
                .unwrap(),
            ],
            capabilities: ids
                .iter()
                .map(|id| {
                    (
                        id.clone(),
                        gateway_domain::CapabilityDefinition::new(
                            id.clone(),
                            gateway_domain::CapabilityClass::Inspect,
                        ),
                    )
                })
                .collect(),
            ..Default::default()
        };
        Ok(OperationPolicy {
            authority,
            facts: gateway_policy::StepFacts {
                authorizations: ids
                    .into_iter()
                    .map(|id| (id, gateway_policy::Approval::Granted))
                    .collect(),
                ..Default::default()
            },
            process: gateway_policy::ProcessReadiness::NotApplicable,
            operating_mode: gateway_domain::OperatingMode::Development,
            execution_profile: gateway_domain::ExecutionProfile::FullPath,
            mutations_enabled: false,
        })
    }

    fn authorize(&self, call: &Call) -> Result<(), FacadeError> {
        self.check(call)?;
        match call.operation.as_str() {
            "situation.inspect" | "situation.assess" | "resource.read" => {}
            "capabilities.resolve" | "state.explain" | "context.compile"
                if self.canonical.is_some() => {}
            _ => return Err(FacadeError::UnsupportedCapability),
        }
        if let Some(canonical) = &self.canonical {
            if call.operation == "capabilities.resolve"
                && [
                    ("plan", &canonical.plan),
                    ("rules", &canonical.rules),
                    ("process", &canonical.process),
                ]
                .iter()
                .any(|(key, reference)| call.input[*key] != **reference)
            {
                return Err(FacadeError::ReferenceUnavailable);
            }
            if matches!(call.operation.as_str(), "state.explain" | "context.compile")
                && call.input["resolution"]["id"] != "local-resolution"
            {
                return Err(FacadeError::ReferenceUnavailable);
            }
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
        if reference["id"] == "local-resolution" && self.canonical.is_some() {
            let canonical = self
                .canonical
                .as_ref()
                .ok_or(FacadeError::UnsupportedCapability)?;
            let document = canonical.artifact(&self.resources, call)?;
            if *reference
                != crate::codex_canonical::resolution_reference(
                    &document,
                    &self.binding.mapping_revision,
                )
            {
                return Err(FacadeError::StaleRevision);
            }
            return Ok(ReferenceRecord {
                scope: self.binding.scope.clone(),
                session: self.binding.session.clone(),
                reference: reference.clone(),
                document: document.to_string(),
                provenance: self.canonical_provenance(reference)?,
            });
        }
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
        if id == "local-resolution" && self.canonical.is_some() {
            let canonical = self
                .canonical
                .as_ref()
                .ok_or(FacadeError::UnsupportedCapability)?;
            let document = canonical.artifact(&self.resources, call)?;
            let reference = crate::codex_canonical::resolution_reference(
                &document,
                &self.binding.mapping_revision,
            );
            if reference["revision"] != revision || reference["digest"] != digest {
                return Err(FacadeError::StaleRevision);
            }
            return Ok(reference);
        }
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
    fn resolution(
        &self,
        call: &Call,
        documents: &[Value],
    ) -> Result<
        (
            gateway_application::resolution_snapshot::ResolutionSnapshotInput,
            gateway_application::resolution_composition::CompositionRules,
        ),
        FacadeError,
    > {
        self.check(call)?;
        let canonical = self
            .canonical
            .as_ref()
            .ok_or(FacadeError::UnsupportedCapability)?;
        if canonical.documents(&self.resources)? != documents {
            return Err(FacadeError::StaleRevision);
        }
        canonical.capture(&self.resources, call)
    }
    fn resolved(
        &self,
        call: &Call,
        _document: &Value,
    ) -> Result<gateway_application::resolution_application::ResolvedPlan, FacadeError> {
        self.check(call)?;
        self.canonical
            .as_ref()
            .ok_or(FacadeError::UnsupportedCapability)?
            .resolve(&self.resources, call)
    }
    fn compile(&self, call: &Call, documents: &[Value]) -> Result<CompileCommand, FacadeError> {
        self.check(call)?;
        self.canonical
            .as_ref()
            .ok_or(FacadeError::UnsupportedCapability)?
            .compile(&self.resources, call, documents)
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
        if contract == "cg.resolution" && self.canonical.is_some() {
            let reference = crate::codex_canonical::resolution_reference(
                &document,
                &self.binding.mapping_revision,
            );
            provenance.extend(self.canonical_provenance(&reference)?);
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

/// Shared admission path for MCP and the operator CLI. Claims never select authority.
pub fn admit_local(
    admission: &str,
    cwd: &str,
    repository: &str,
    session: &str,
    principal: &str,
    scope: &Value,
) -> Result<(ScopeBinding, crate::local_sessions::LocalApplication), FacadeError> {
    let file = std::fs::File::open(admission).map_err(|_| FacadeError::InvalidInput)?;
    let mut text = String::new();
    file.take(crate::local_mcp::MAX_FRAME_BYTES as u64)
        .read_to_string(&mut text)
        .map_err(|_| FacadeError::InvalidInput)?;
    let resolver = LocalWorkspaceResolver::from_json(&text)?;
    let binding = resolver.resolve(&WorkspaceReference {
        working_directory: cwd.into(),
        repository: repository.into(),
    })?;
    if binding.scope != *scope
        || binding.session.principal != principal
        || binding.session.session_id != session
    {
        return Err(FacadeError::ScopeDenied);
    }
    let host = resolver.host(&binding)?;
    let enabled = host
        .sessions
        .as_ref()
        .is_some_and(|sessions| sessions.enabled);
    let facade = CodexFacade::with_binding(binding.clone(), host)?;
    let application = crate::local_sessions::LocalApplication::new(
        facade,
        binding.clone(),
        admission,
        cwd,
        repository,
        enabled,
    );
    Ok((binding, application))
}
