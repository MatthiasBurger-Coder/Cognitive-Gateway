//! Trusted host dependencies. Client documents cannot construct these authorities.
use super::*;
use crate::{
    context_application::ContextProjection, policy_application::PolicyContext,
    resolution_application::ResolvedPlan, resolution_composition::CompositionRules,
    resolution_snapshot::ResolutionSnapshotInput,
};
use gateway_context::{ContextDisclosurePolicy, ContextFragment};
use gateway_domain::{DefinitionCatalog, ReferenceId};
use gateway_policy::PolicyAuthority;
use gateway_process::ProcessRegistry;
use gateway_registry::Registry;
use std::collections::BTreeSet;

pub struct CompileCommand {
    pub resolved: ResolvedPlan,
    pub authority: PolicyAuthority,
    pub policy_context: PolicyContext,
    pub catalog: DefinitionCatalog,
    pub projection: ContextProjection,
    pub candidates: Vec<ContextFragment>,
    pub selected: BTreeSet<ReferenceId>,
    pub disclosure: ContextDisclosurePolicy,
}

/// An admitted immutable record. Digest covers the UTF-8 canonical JSON bytes.
pub struct ReferenceRecord {
    pub scope: Value,
    pub reference: Value,
    pub document: String,
    pub session: SessionContext,
    pub provenance: Vec<Value>,
}

/// Projection is performed by the host disclosure owner after canonical validation.
/// It may return a filtered document or immutable reference and must preserve
/// admitted evidence, explanation and provenance links. Default implementation
/// fails closed; source classification is never guessed by the facade.
pub struct Projection {
    pub source: Value,
    pub explainability: Vec<Value>,
    pub evidence: Vec<Value>,
    pub provenance: Vec<Value>,
}

pub trait CodexHost {
    fn authorize(&self, _call: &Call) -> Result<(), FacadeError> {
        Err(FacadeError::UnsupportedCapability)
    }
    fn resource_reference(
        &self,
        _call: &Call,
        _id: &str,
        _revision: &str,
        _digest: &str,
    ) -> Result<Value, FacadeError> {
        Err(FacadeError::ReferenceUnavailable)
    }
    fn reference(&self, _call: &Call, _reference: &Value) -> Result<ReferenceRecord, FacadeError> {
        Err(FacadeError::ReferenceUnavailable)
    }
    /// Map admitted plan/rules/process records into their canonical command.
    /// Parse their exact supported Rust contracts; never substitute a different record.
    fn resolution(
        &self,
        _call: &Call,
        _documents: &[Value],
    ) -> Result<(ResolutionSnapshotInput, CompositionRules), FacadeError> {
        Err(FacadeError::UnsupportedCapability)
    }
    fn resolved(&self, _call: &Call, _document: &Value) -> Result<ResolvedPlan, FacadeError> {
        Err(FacadeError::UnsupportedCapability)
    }
    fn compile(&self, _call: &Call, _documents: &[Value]) -> Result<CompileCommand, FacadeError> {
        Err(FacadeError::UnsupportedCapability)
    }
    fn registry(&self, _call: &Call) -> Result<(Registry, ProcessRegistry), FacadeError> {
        Err(FacadeError::UnsupportedCapability)
    }
    /// Read existing evidence records through their Rust validation contract.
    fn evidence(
        &self,
        _call: &Call,
        _documents: &[Value],
    ) -> Result<gateway_domain::ObservationEvidenceSet, FacadeError> {
        Err(FacadeError::UnsupportedCapability)
    }
    fn project(
        &self,
        _call: &Call,
        _contract: &str,
        _document: Value,
    ) -> Result<Projection, FacadeError> {
        Err(FacadeError::SensitivityDenied)
    }
    /// Shared session services must return the immutable client owner for a task.
    /// The facade checks it independently before dispatch and after mapping.
    fn session_owner(&self, _call: &Call, _task_id: &str) -> Result<ScopeBinding, FacadeError> {
        Err(FacadeError::UnsupportedCapability)
    }
    /// Reserved for the shared session application API (#272/#273/#275).
    /// No coordinator, consent decision or state machine exists in this facade.
    fn session(&self, _call: &Call) -> Result<Value, FacadeError> {
        Err(FacadeError::UnsupportedCapability)
    }
}

#[derive(Default)]
pub struct UnavailableHost;
impl CodexHost for UnavailableHost {}

/// Driving port used by adapters; no domain crate access is required.
pub trait CodexApplicationPort {
    fn execute(&self, operation: &str, request: &Value) -> Value;
    fn read_resource(
        &self,
        _scope: &Value,
        _id: &str,
        _revision: &str,
        _digest: &str,
    ) -> Result<Value, FacadeError> {
        Err(FacadeError::ScopeDenied)
    }
}
