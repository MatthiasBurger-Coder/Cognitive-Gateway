//! Trusted launch bindings; identifiers are claims, never authority.
use super::{FacadeError, contracts, security};
use gateway_domain::ContextScopeId;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Clone, PartialEq, Eq)]
pub struct SessionContext {
    pub principal: String,
    pub session_id: String,
    pub connection_id: String,
}

#[derive(Clone)]
pub struct ScopeBinding {
    pub scope: Value,
    pub canonical_scope: ContextScopeId,
    pub session: SessionContext,
    pub mapping_revision: String,
}

impl ScopeBinding {
    pub fn validate(&self) -> Result<(), FacadeError> {
        if !security::credential_free(&self.scope)
            || [
                &self.session.principal,
                &self.session.session_id,
                &self.session.connection_id,
                &self.mapping_revision,
                &self.canonical_scope.to_string(),
            ]
            .iter()
            .any(|v| security::credential_text(v))
        {
            return Err(FacadeError::SensitivityDenied);
        }
        let common = contracts::artifact("common.schema.json").unwrap();
        if !contracts::valid(&self.scope, &common["$defs"]["scope"], &common)
            || ![
                &self.session.principal,
                &self.session.session_id,
                &self.session.connection_id,
                &self.mapping_revision,
            ]
            .iter()
            .all(|id| contracts::token(id))
            || self.scope["binding_id"] != self.session.connection_id
        {
            return Err(FacadeError::ScopeDenied);
        }
        Ok(())
    }

    /// A scope-only trace uses the existing graph contract. It explicitly records
    /// that policy and resolution have not been evaluated by scope admission.
    pub fn trace_document(&self) -> Value {
        use crate::resolution_explain::{ResolutionTrace, SourceKind, TraceNode, TraceSource};
        let basis = json!({"scope":self.scope,"canonical_scope":self.canonical_scope,
            "principal":self.session.principal,"session_id":self.session.session_id,
            "connection_id":self.session.connection_id,"mapping_revision":self.mapping_revision});
        let fingerprint = format!("sha256:{:x}", Sha256::digest(basis.to_string().as_bytes()));
        serde_json::to_value(ResolutionTrace {
            version: 1,
            basis: basis.clone(),
            rule_fingerprint: fingerprint,
            outcome: "SCOPE_BOUND".into(),
            policy_authorization: "NOT_EVALUATED".into(),
            search_complete: false,
            nodes: vec![TraceNode {
                id: "scope-binding".into(),
                source: TraceSource {
                    kind: SourceKind::Binding,
                    reference: self.session.connection_id.clone(),
                },
                code: "CG_SCOPE_BOUND".into(),
                attributes: basis
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            }],
            edges: vec![],
            omitted_optional_details: 0,
        })
        .expect("scope trace contains only JSON values")
    }
    /// Reference-only audit link: no cwd, repository URL, content or credentials.
    pub fn explanation(&self) -> Value {
        json!({"id":self.session.session_id,"contract":"cg.resolution-trace","contract_version":"1.0",
            "revision":self.mapping_revision,"digest":format!("sha256:{:x}",Sha256::digest(self.trace_document().to_string().as_bytes()))})
    }

    /// Deliberately unavailable for SECRET sources. This is a partition identity,
    /// not a result cache: policy/disclosure must still be evaluated on every use.
    pub fn cache_key(
        &self,
        operation: &str,
        references: &[Value],
        provenance: &[Value],
    ) -> Result<String, FacadeError> {
        self.validate()?;
        if !security::credential_free(
            &json!({"operation":operation,"references":references,"provenance":provenance}),
        ) {
            return Err(FacadeError::SensitivityDenied);
        }
        let common = contracts::artifact("common.schema.json").unwrap();
        if !contracts::token(operation)
            || references
                .iter()
                .any(|r| !contracts::valid(r, &common["$defs"]["reference"], &common))
            || provenance
                .iter()
                .any(|p| !contracts::valid(p, &common["$defs"]["provenance"], &common))
        {
            return Err(FacadeError::InvalidInput);
        }
        if references
            .iter()
            .any(|r| !provenance.iter().any(|p| p["reference"] == *r))
        {
            return Err(FacadeError::SensitivityDenied);
        }
        if provenance.iter().any(|p| p["sensitivity"] == "SECRET") {
            return Err(FacadeError::SensitivityDenied);
        }
        let basis = json!({"version":1,"binding":self.explanation(),"operation":operation,
            "references":references,"provenance":provenance});
        Ok(format!(
            "sha256:{:x}",
            Sha256::digest(basis.to_string().as_bytes())
        ))
    }
}

/// Outer adapters resolve explicit filesystem/repository claims. No ambient cwd.
pub struct WorkspaceReference {
    pub working_directory: String,
    pub repository: String,
}
pub trait WorkspaceResolver {
    fn resolve(&self, reference: &WorkspaceReference) -> Result<ScopeBinding, FacadeError>;
}
