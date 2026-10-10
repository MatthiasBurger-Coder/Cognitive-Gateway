//! Admitted canonical records mapped through the shared outer input adapter.
use crate::declarative_cli::host_mapping;
use gateway_application::{
    codex::{Call, CompileCommand, FacadeError},
    resolution_application::{DeclarativeResolutionApplication, ResolvedPlan},
    resolution_composition::CompositionRules,
    resolution_snapshot::ResolutionSnapshotInput,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CanonicalAdmission {
    pub catalog: PathBuf,
    pub plan: Value,
    pub rules: Value,
    pub process: Value,
    pub policy: Value,
}
impl CanonicalAdmission {
    pub(crate) fn documents(&self, resources: &[Value]) -> Result<Vec<Value>, FacadeError> {
        [&self.plan, &self.rules, &self.process]
            .into_iter()
            .map(|reference| {
                let resource = resources
                    .iter()
                    .find(|record| record["reference"] == *reference)
                    .ok_or(FacadeError::ReferenceUnavailable)?;
                if resource["provenance"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["sensitivity"] == "SECRET")
                {
                    return Err(FacadeError::SensitivityDenied);
                }
                Ok(resource["document"].clone())
            })
            .collect()
    }
    pub(crate) fn capture(
        &self,
        resources: &[Value],
        call: &Call,
    ) -> Result<(ResolutionSnapshotInput, CompositionRules), FacadeError> {
        let (snapshot, rules) = host_mapping::capture(&self.catalog, &self.documents(resources)?)?;
        if snapshot.scope != call.canonical_scope {
            return Err(FacadeError::ScopeDenied);
        }
        if snapshot.operating_mode != call.operating_mode
            || snapshot.execution_profile != call.execution_profile
        {
            return Err(FacadeError::InvalidInput);
        }
        Ok((snapshot, rules))
    }
    pub(crate) fn resolve(
        &self,
        resources: &[Value],
        call: &Call,
    ) -> Result<ResolvedPlan, FacadeError> {
        let (snapshot, rules) = self.capture(resources, call)?;
        DeclarativeResolutionApplication
            .resolve_plan(&snapshot, &rules)
            .map_err(Into::into)
    }
    pub(crate) fn artifact(&self, resources: &[Value], call: &Call) -> Result<Value, FacadeError> {
        let resolved = self.resolve(resources, call)?;
        serde_json::from_str(
            &DeclarativeResolutionApplication
                .serialize_resolution(&resolved, Default::default())?,
        )
        .map_err(|_| FacadeError::Internal)
    }
    pub(crate) fn compile(
        &self,
        resources: &[Value],
        call: &Call,
        documents: &[Value],
    ) -> Result<CompileCommand, FacadeError> {
        // Candidate records are not implicitly selected or replaced by inline fragments.
        if documents.len() != 2 {
            return Err(FacadeError::UnsupportedCapability);
        }
        host_mapping::compile(
            self.resolve(resources, call)?,
            self.policy.clone(),
            documents[1].clone(),
        )
    }
}
pub(crate) fn resolution_reference(document: &Value, revision: &str) -> Value {
    json!({"id":"local-resolution", "contract":"cg.resolution", "contract_version":"1.0",
        "revision":revision, "digest":format!("sha256:{:x}", Sha256::digest(document.to_string().as_bytes()))})
}
