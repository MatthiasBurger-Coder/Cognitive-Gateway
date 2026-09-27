//! Semantic TAG assembly. No fragment grants permission or changes execution IR.
use gateway_domain::{
    ContextScopeId, Evidence, ExecutionContextIR, KnowledgeProvenance, NonEmptyText, PlanStepId,
    Provenance, QualityMetadata, ReferenceId, RetrievedKnowledge, SensitivityClass, TrustClass,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FragmentKind {
    Authority,
    Workflow,
    Agent,
    Skills,
    Knowledge,
    Evidence,
    Memory,
    Task,
    UserInput,
    OutputContract,
    Constraints,
    RuntimeState,
}

/// These categories cannot be supplied through the external data boundary.
impl FragmentKind {
    fn external(self) -> bool {
        matches!(
            self,
            Self::Knowledge | Self::Evidence | Self::Memory | Self::UserInput
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    InvalidMetadata,
    InvalidTrust,
    ConflictingFragment,
    MissingSelection,
    ScopeMismatch,
    InvalidProjection,
}

/// The host supplies this policy from an authenticated disclosure decision.
/// It is deliberately separate from retrieved content and model output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextDisclosurePolicy {
    pub maximum_sensitivity: SensitivityClass,
    pub include_caller_input: bool,
    /// Audit exports can omit all external payloads, even those labeled public.
    pub include_external_content: bool,
}

/// Explicit source metadata; quality is carried unchanged, never interpreted as permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentMetadata {
    pub provenance: KnowledgeProvenance,
    pub evidence: BTreeSet<ReferenceId>,
    pub quality: QualityMetadata,
    pub rationale: NonEmptyText,
    pub validation: Option<ReferenceId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextFragment {
    id: ReferenceId,
    kind: FragmentKind,
    content: NonEmptyText,
    metadata: FragmentMetadata,
    scope: ContextScopeId,
    step: PlanStepId,
    reference_only: bool,
}
impl ContextFragment {
    /// Typed external boundary: caller data cannot introduce authority/catalog fragments.
    pub fn external(
        id: ReferenceId,
        kind: FragmentKind,
        content: impl Into<String>,
        metadata: FragmentMetadata,
        scope: ContextScopeId,
        step: PlanStepId,
    ) -> Result<Self, CompileError> {
        let trust = metadata.quality.trust();
        let valid = match kind {
            FragmentKind::Knowledge => trust == TrustClass::RetrievedContent,
            FragmentKind::Evidence => trust == TrustClass::ObservedEvidence,
            FragmentKind::Memory => trust == TrustClass::DerivedAssessment,
            FragmentKind::UserInput => trust == TrustClass::CallerInput,
            _ => false,
        };
        if !kind.external() || !valid {
            return Err(CompileError::InvalidTrust);
        }
        if kind == FragmentKind::Memory
            && (metadata.provenance.revision().is_none() || metadata.validation.is_none())
        {
            return Err(CompileError::InvalidMetadata);
        }
        Ok(Self {
            id,
            kind,
            content: NonEmptyText::new(content).map_err(|_| CompileError::InvalidMetadata)?,
            metadata,
            scope,
            step,
            reference_only: false,
        })
    }
    /// Emits a governed memory payload reference without treating it as inline text.
    pub fn memory_reference(
        id: ReferenceId,
        reference: ReferenceId,
        metadata: FragmentMetadata,
        scope: ContextScopeId,
        step: PlanStepId,
    ) -> Result<Self, CompileError> {
        let mut fragment = Self::external(
            id,
            FragmentKind::Memory,
            reference.as_str(),
            metadata,
            scope,
            step,
        )?;
        fragment.reference_only = true;
        Ok(fragment)
    }
    /// Retains retrieval content and source revision exactly; metadata cannot relabel its source.
    pub fn knowledge(
        id: ReferenceId,
        knowledge: &RetrievedKnowledge,
        metadata: FragmentMetadata,
        scope: ContextScopeId,
        step: PlanStepId,
    ) -> Result<Self, CompileError> {
        if &metadata.provenance != knowledge.provenance() {
            return Err(CompileError::InvalidMetadata);
        }
        Self::external(
            id,
            FragmentKind::Knowledge,
            knowledge.content(),
            metadata,
            scope,
            step,
        )
    }
    /// Emits only an evidence reference and its verified source link, not the full
    /// observation set. The owning CG-06 boundary retains the complete record.
    pub fn evidence(
        id: ReferenceId,
        evidence: &Evidence,
        provenance: &Provenance,
        mut metadata: FragmentMetadata,
        scope: ContextScopeId,
        step: PlanStepId,
    ) -> Result<Self, CompileError> {
        if evidence.provenance() != provenance.id() {
            return Err(CompileError::InvalidMetadata);
        }
        let actual_provenance = KnowledgeProvenance::new(
            provenance.source_reference(),
            provenance.source_timestamp().map(|t| t.as_str()),
        )
        .map_err(|_| CompileError::InvalidMetadata)?;
        if metadata.provenance != actual_provenance {
            return Err(CompileError::InvalidMetadata);
        }
        let verified_links = BTreeSet::from([
            ReferenceId::new(evidence.id().as_str()).map_err(|_| CompileError::InvalidMetadata)?,
            ReferenceId::new(provenance.id().as_str())
                .map_err(|_| CompileError::InvalidMetadata)?,
        ]);
        if !metadata.evidence.is_subset(&verified_links) {
            return Err(CompileError::InvalidMetadata);
        }
        metadata.evidence = verified_links;
        let mut fragment = Self::external(
            id,
            FragmentKind::Evidence,
            evidence.id().as_str(),
            metadata,
            scope,
            step,
        )?;
        fragment.reference_only = true;
        Ok(fragment)
    }
    pub fn id(&self) -> &ReferenceId {
        &self.id
    }
    pub fn scope(&self) -> &ContextScopeId {
        &self.scope
    }
    pub fn step(&self) -> &PlanStepId {
        &self.step
    }
    pub fn kind(&self) -> FragmentKind {
        self.kind
    }
    /// Distinguishes a source reference from inline data for runtime renderers.
    pub fn is_reference(&self) -> bool {
        self.reference_only
    }
    pub fn content(&self) -> &str {
        self.content.as_str()
    }
    pub fn metadata(&self) -> &FragmentMetadata {
        &self.metadata
    }
    fn json(&self, policy: Option<ContextDisclosurePolicy>) -> serde_json::Value {
        let redacted = policy.is_some_and(|policy| {
            !policy.include_external_content
                || self.metadata.quality.sensitivity() > policy.maximum_sensitivity
                || (self.kind == FragmentKind::UserInput && !policy.include_caller_input)
        });
        if redacted {
            return serde_json::json!({
                "id": self.id.as_str(), "kind": self.kind,
                "content": "[REDACTED]", "representation": "redacted",
                "scope": self.scope.as_str(), "step": self.step.as_str(),
                "provenance": {"source": "[REDACTED]", "revision": null},
                "evidence": [], "quality": self.metadata.quality,
                "rationale": "[REDACTED]", "validation": null,
            });
        }
        serde_json::json!({
            "id": self.id.as_str(), "kind": self.kind, "content": self.content(),
            "representation": if self.reference_only { "reference" } else { "inline" },
            "scope": self.scope.as_str(), "step": self.step.as_str(),
            "provenance": {"source": self.metadata.provenance.source(), "revision": self.metadata.provenance.revision()},
            "evidence": self.metadata.evidence.iter().map(ReferenceId::as_str).collect::<Vec<_>>(),
            "quality": self.metadata.quality, "rationale": self.metadata.rationale.as_str(),
            "validation": self.metadata.validation.as_ref().map(ReferenceId::as_str),
        })
    }
}

/// Output only. The existing CG-02 execution contract remains the sole execution IR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledContext {
    projection: ExecutionContextIR,
    scope: ContextScopeId,
    step: PlanStepId,
    dynamic: Vec<ContextFragment>,
}
impl CompiledContext {
    /// Pure assembly of an already validated projection. Authorization belongs to the
    /// application entry point; this function is not an execution permission boundary.
    pub fn assemble(
        projection: ExecutionContextIR,
        scope: ContextScopeId,
        step: PlanStepId,
        candidates: &[ContextFragment],
        selected: &BTreeSet<ReferenceId>,
    ) -> Result<Self, CompileError> {
        projection
            .validate()
            .map_err(|_| CompileError::InvalidProjection)?;
        let mut fragments = BTreeMap::new();
        for fragment in candidates.iter().filter(|f| selected.contains(&f.id)) {
            if fragment.scope != scope || fragment.step != step {
                return Err(CompileError::ScopeMismatch);
            }
            if let Some(previous) = fragments.insert(fragment.id.clone(), fragment.clone()) {
                if previous != *fragment {
                    return Err(CompileError::ConflictingFragment);
                }
            }
        }
        if fragments.len() != selected.len() {
            return Err(CompileError::MissingSelection);
        }
        // Identical payloads with different IDs are only coalesced when their complete
        // source, trust and selection metadata match; provenance is never discarded.
        let mut dynamic: Vec<ContextFragment> = Vec::new();
        for fragment in fragments.into_values() {
            if !dynamic.iter().any(|f| {
                f.kind == fragment.kind
                    && f.content == fragment.content
                    && f.metadata == fragment.metadata
                    && f.reference_only == fragment.reference_only
            }) {
                dynamic.push(fragment);
            }
        }
        dynamic.sort_by(|a, b| (a.kind, &a.id).cmp(&(b.kind, &b.id)));
        Ok(Self {
            projection,
            scope,
            step,
            dynamic,
        })
    }
    pub fn execution_context(&self) -> &ExecutionContextIR {
        &self.projection
    }
    pub fn fragments(&self) -> &[ContextFragment] {
        &self.dynamic
    }
    /// Stable catalog references are deliberately separate from dynamic data.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        self.serialize(None)
    }
    /// Export a disclosure-limited envelope. Redacted fragments retain their
    /// typed quality and ID, but no source-controlled descriptive strings.
    pub fn to_json_with_policy(
        &self,
        policy: ContextDisclosurePolicy,
    ) -> Result<String, serde_json::Error> {
        self.serialize(Some(policy))
    }
    fn serialize(
        &self,
        policy: Option<ContextDisclosurePolicy>,
    ) -> Result<String, serde_json::Error> {
        let mut value = serde_json::json!({
            "schema_version": 1, "scope": self.scope.as_str(), "step": self.step.as_str(),
            "stable": {
                "authority": [self.projection.policy_id().as_str()],
                "workflow": self.projection.workflow_id().as_str(),
                "agent": self.projection.primary_agent_id().as_str(),
                "skills": self.projection.skill_ids().iter().map(|id| id.as_str()).collect::<Vec<_>>(),
            },
            "dynamic": self.dynamic.iter().map(|fragment| fragment.json(policy)).collect::<Vec<_>>(),
            "execution_context": self.projection,
        });
        if policy.is_some_and(|policy| !policy.include_caller_input) {
            value["execution_context"] = serde_json::json!({
                "id": self.projection.id().as_str(), "representation": "redacted",
            });
        }
        serde_json::to_string(&value)
    }
    /// Diagnostics intentionally omit user input and external payload bytes.
    pub fn explain(&self) -> String {
        format!(
            "Context {} for step {}: {} selected data fragments; workflow {}; agent {}; {} capabilities. Trust and provenance retained.",
            self.projection.id(),
            self.step,
            self.dynamic.len(),
            self.projection.workflow_id(),
            self.projection.primary_agent_id(),
            self.projection.approved_capability_ids().len()
        )
    }
}
