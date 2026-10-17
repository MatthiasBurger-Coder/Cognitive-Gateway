//! Versioned, output-only context planning contracts (#285). No acquisition or authority.
use crate::*;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContextClass {
    Authority,
    TaskInput,
    SystemState,
    DomainKnowledge,
    Evidence,
    StructuredData,
    Memory,
    Examples,
    RuntimeState,
    OutputVerification,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContextSourceClass {
    CurrentInput,
    TaskState,
    SituationState,
    CatalogAuthority,
    Repository,
    Rag,
    StructuredData,
    Memory,
    Examples,
    ToolsMcp,
    RuntimeState,
    Clarification,
}
impl ContextSourceClass {
    pub const ALL: [Self; 12] = [
        Self::CurrentInput,
        Self::TaskState,
        Self::SituationState,
        Self::CatalogAuthority,
        Self::Repository,
        Self::Rag,
        Self::StructuredData,
        Self::Memory,
        Self::Examples,
        Self::ToolsMcp,
        Self::RuntimeState,
        Self::Clarification,
    ];
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContextNecessity {
    Mandatory,
    Optional,
    Conditional {
        condition: ReferenceId,
        active: bool,
    },
}
impl ContextNecessity {
    pub fn required(&self) -> bool {
        matches!(
            self,
            Self::Mandatory | Self::Conditional { active: true, .. }
        )
    }
    pub fn active(&self) -> bool {
        !matches!(self, Self::Conditional { active: false, .. })
    }
}
/// Reuses CG-15 constraints and hard bounds without another retrieval schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRequirement {
    pub id: ReferenceId,
    pub class: ContextClass,
    pub purpose: NonEmptyText,
    pub necessity: ContextNecessity,
    /// Order is meaningful: lower index is preferred. No implicit fallback classes.
    pub permitted_sources: Vec<ContextSourceClass>,
    pub information: RequiredInformation,
    pub budget: RetrievalBudget,
    pub stop: StopCondition,
    pub priority: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContextPlanningError {
    InvalidRequirements,
    ConflictingRequirements,
    ScopeMismatch,
    InfeasibleBudget,
    SemanticAdmission,
    BindingMismatch,
    MissingMandatory,
    InvalidSatisfaction,
}
/// Exact task, Situation and Plan pins; the trusted host authenticates the basis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextPlanningBasis {
    pub task: ResolvedReference<TaskId>,
    pub situation: Option<ResolvedReference<SituationId>>,
    #[serde(serialize_with = "serialize_plan_pin")]
    pub plan: Option<ResolvedReference<PlanId>>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRequirementsIR {
    revision: ReferenceId,
    basis: ContextPlanningBasis,
    requirements: Vec<ContextRequirement>,
    total_tokens: TokenBudget,
}
impl ContextRequirementsIR {
    pub fn new(
        version: SchemaVersion,
        revision: ReferenceId,
        basis: ContextPlanningBasis,
        mut requirements: Vec<ContextRequirement>,
        total_tokens: TokenBudget,
    ) -> Result<Self, ContextPlanningError> {
        if version != SchemaVersion::V1 || requirements.is_empty() {
            return Err(ContextPlanningError::InvalidRequirements);
        }
        let scope = &basis.task.scope;
        if basis.situation.as_ref().is_some_and(|p| &p.scope != scope)
            || basis.plan.as_ref().is_some_and(|p| &p.scope != scope)
        {
            return Err(ContextPlanningError::ScopeMismatch);
        }
        requirements.sort_by(|a, b| {
            (!a.necessity.required(), a.priority, &a.id).cmp(&(
                !b.necessity.required(),
                b.priority,
                &b.id,
            ))
        });
        let ids: BTreeSet<_> = requirements.iter().map(|r| &r.id).collect();
        if ids.len() != requirements.len() {
            return Err(ContextPlanningError::ConflictingRequirements);
        }
        for r in &requirements {
            let sources: BTreeSet<_> = r.permitted_sources.iter().collect();
            if sources.is_empty()
                || sources.len() != r.permitted_sources.len()
                || r.information.accepted_trust.is_empty()
                || r.information
                    .requirements
                    .minimum_sensitivity()
                    .is_some_and(|s| s > r.information.maximum_sensitivity)
                || (r.class == ContextClass::Authority
                    && (sources != BTreeSet::from([&ContextSourceClass::CatalogAuthority])
                        || r.information.accepted_trust
                            != BTreeSet::from([TrustClass::CanonicalReference])))
                || (r.class != ContextClass::Authority
                    && (sources.contains(&ContextSourceClass::CatalogAuthority)
                        || r.information
                            .accepted_trust
                            .contains(&TrustClass::CanonicalReference)))
            {
                return Err(ContextPlanningError::InvalidRequirements);
            }
            if r.budget.tokens.0 == 0 || r.budget.context.total().0 == 0 {
                return Err(ContextPlanningError::InfeasibleBudget);
            }
        }
        Ok(Self {
            revision,
            basis,
            requirements,
            total_tokens,
        })
    }
    pub fn basis(&self) -> &ContextPlanningBasis {
        &self.basis
    }
    pub fn requirements(&self) -> &[ContextRequirement] {
        &self.requirements
    }
    pub fn total_tokens(&self) -> TokenBudget {
        self.total_tokens
    }
    pub fn to_value(&self) -> Value {
        json!({"schema_version": SchemaVersion::V1, "revision": self.revision,
            "basis": self.basis, "total_tokens": self.total_tokens.0,
            "requirements": self.requirements.iter().map(requirement_value).collect::<Vec<_>>()})
    }
    pub fn binding(&self) -> ResolvedReference<ReferenceId> {
        binding(
            "context-requirements",
            &self.revision,
            &self.basis.task.scope,
            &self.to_value(),
        )
    }
}
/// Present content is linked structurally; references are never fetched here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContextSatisfaction {
    Task,
    OutputContract,
    Catalog,
    Fragment(ReferenceId),
}
/// Trusted host metadata, not retrieved assertions. `pin` binds the captured source snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSourceMetadata {
    pub pin: ResolvedReference<ReferenceId>,
    /// Actual retained bytes, unknown until acquisition when not already present.
    pub payload_digest: Option<ContentDigest>,
    pub class: ContextSourceClass,
    pub covers: BTreeSet<ReferenceId>,
    pub quality: QualityMetadata,
    pub evidence: BTreeSet<EvidenceId>,
    pub provenances: BTreeSet<ProvenanceId>,
    pub tokens: u64,
    pub available: bool,
    pub present: Option<ContextSatisfaction>,
}
impl ContextSourceMetadata {
    pub fn to_value(&self) -> Value {
        source_value(self)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContextSelectionReason {
    SelectedPresent,
    SelectedAcquisition,
    NoneRequired,
    NotPermitted,
    NotRelevant,
    Unavailable,
    CrossScope,
    Untrusted,
    Stale,
    Conflicting,
    EvidenceMissing,
    SensitivityRejected,
    BudgetExceeded,
    LowerPreference,
    Inactive,
}
impl ContextSelectionReason {
    pub fn explanation(self) -> &'static str {
        match self {
            Self::SelectedPresent => "Required context is already present",
            Self::SelectedAcquisition => "Permitted source requires bounded acquisition",
            Self::NoneRequired => "This task has no active need for this source class",
            Self::NotPermitted => "Source class is outside the requirement",
            Self::NotRelevant => "Source does not cover this requirement",
            Self::Unavailable => "Source is unavailable",
            Self::CrossScope => "Source belongs to another scope",
            Self::Untrusted => "Source trust or uncertainty is unacceptable",
            Self::Stale => "Fresh context is required",
            Self::Conflicting => "Source has an unresolved conflict",
            Self::EvidenceMissing => "Required evidence or provenance is absent",
            Self::SensitivityRejected => "Source violates sensitivity constraints",
            Self::BudgetExceeded => "Source exceeds a hard token bound",
            Self::LowerPreference => "An earlier eligible source satisfies this requirement",
            Self::Inactive => "The conditional requirement is inactive",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextSourceDecision {
    pub requirement: Option<ReferenceId>,
    pub class: ContextSourceClass,
    pub source: Option<ReferenceId>,
    pub reason: ContextSelectionReason,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPlanIR {
    revision: ReferenceId,
    requirements: ResolvedReference<ReferenceId>,
    selected: BTreeMap<ReferenceId, ContextSourceMetadata>,
    decisions: Vec<ContextSourceDecision>,
    missing: BTreeSet<ReferenceId>,
}
impl ContextPlanIR {
    /// Only planner-selected metadata is carried. Revalidation is required at CG-10.
    pub fn new(
        revision: ReferenceId,
        requirements: &ContextRequirementsIR,
        selected: BTreeMap<ReferenceId, ContextSourceMetadata>,
        mut decisions: Vec<ContextSourceDecision>,
        missing: BTreeSet<ReferenceId>,
    ) -> Result<Self, ContextPlanningError> {
        let mut charged = BTreeMap::new();
        for (id, source) in &selected {
            let r = requirements
                .requirements()
                .iter()
                .find(|r| &r.id == id)
                .ok_or(ContextPlanningError::InvalidRequirements)?;
            if !r.necessity.active()
                || context_source_rejection(r, source, &requirements.basis().task.scope).is_some()
            {
                return Err(ContextPlanningError::InvalidRequirements);
            }
            if charged
                .insert(&source.pin.id, source)
                .is_some_and(|old| old != source)
            {
                return Err(ContextPlanningError::ConflictingRequirements);
            }
            if !decisions.iter().any(|d| {
                d.requirement.as_ref() == Some(id)
                    && d.source.as_ref() == Some(&source.pin.id)
                    && d.class == source.class
                    && d.reason
                        == if source.present.is_some() {
                            ContextSelectionReason::SelectedPresent
                        } else {
                            ContextSelectionReason::SelectedAcquisition
                        }
            }) {
                return Err(ContextPlanningError::InvalidRequirements);
            }
        }
        let total = charged
            .values()
            .try_fold(0u64, |n, s| n.checked_add(s.tokens))
            .ok_or(ContextPlanningError::InfeasibleBudget)?;
        if total > requirements.total_tokens().0 {
            return Err(ContextPlanningError::InfeasibleBudget);
        }
        let expected_missing = requirements
            .requirements()
            .iter()
            .filter(|r| r.necessity.required() && !selected.contains_key(&r.id))
            .map(|r| r.id.clone())
            .collect();
        if missing != expected_missing {
            return Err(ContextPlanningError::InvalidRequirements);
        }
        decisions.sort_by(|a, b| {
            (&a.requirement, a.class, &a.source, a.reason).cmp(&(
                &b.requirement,
                b.class,
                &b.source,
                b.reason,
            ))
        });
        Ok(Self {
            revision,
            requirements: requirements.binding(),
            selected,
            decisions,
            missing,
        })
    }
    pub fn selected(&self) -> &BTreeMap<ReferenceId, ContextSourceMetadata> {
        &self.selected
    }
    pub fn decisions(&self) -> &[ContextSourceDecision] {
        &self.decisions
    }
    pub fn missing(&self) -> &BTreeSet<ReferenceId> {
        &self.missing
    }
    pub fn requirements_binding(&self) -> &ResolvedReference<ReferenceId> {
        &self.requirements
    }
    pub fn to_value(&self) -> Value {
        json!({"schema_version": SchemaVersion::V1, "revision": self.revision,
            "requirements": self.requirements,
            "selected": self.selected.iter().map(|(id,s)| json!({"requirement":id,"source":source_value(s)})).collect::<Vec<_>>(),
            "decisions": self.decisions, "missing":self.missing})
    }
    pub fn binding(&self) -> ResolvedReference<ReferenceId> {
        binding(
            "context-plan",
            &self.revision,
            &self.requirements.scope,
            &self.to_value(),
        )
    }
}
pub fn context_content_digest(bytes: &[u8]) -> ContentDigest {
    ContentDigest::new(format!("{:x}", Sha256::digest(bytes))).expect("SHA-256 is a valid digest")
}
pub fn binding(
    contract: &str,
    revision: &ReferenceId,
    scope: &ContextScopeId,
    value: &Value,
) -> ResolvedReference<ReferenceId> {
    ResolvedReference {
        id: ReferenceId::new(contract).expect("static contract identity"),
        scope: scope.clone(),
        contract: ReferenceId::new(contract).expect("static contract identity"),
        contract_version: SchemaVersion::V1,
        revision: revision.clone(),
        digest: context_content_digest(
            serde_json::to_string(value).expect("JSON value").as_bytes(),
        ),
    }
}
fn requirement_value(r: &ContextRequirement) -> Value {
    let i = &r.information;
    let b = &r.budget;
    json!({"id":r.id,"class":r.class,"purpose":r.purpose,"necessity":r.necessity,
        "permitted_sources":r.permitted_sources,"priority":r.priority,
        "information":{"description":i.description,"trust":i.accepted_trust,
            "maximum_sensitivity":i.maximum_sensitivity,
            "freshness":i.requirements.freshness().as_str(),
            "minimum_sensitivity":i.requirements.minimum_sensitivity(),
            "evidence":i.requirements.evidence(),"provenances":i.requirements.provenances()},
        "budget":{"results":b.results.0.get(),"rounds":b.rounds.0.get(),"latency_ms":b.latency.0.get(),
            "cost":{"maximum":b.cost.maximum,"unit":b.cost.unit},"tokens":b.tokens.0,
            "context":{"total":b.context.total().0,"reservations":b.context.reservations().iter()
                .map(|(c,t)| json!({"class":format!("{c:?}"),"tokens":t.0})).collect::<Vec<_>>()}},
        "stop":match r.stop { StopCondition::BudgetExhausted => json!({"kind":"BUDGET_EXHAUSTED"}),
            StopCondition::EvidenceSatisfied(n) => json!({"kind":"EVIDENCE_SATISFIED","count":n.get()}) }})
}
fn source_value(s: &ContextSourceMetadata) -> Value {
    json!({"pin":s.pin,"payload_digest":s.payload_digest,"class":s.class,"covers":s.covers,"quality":s.quality,
        "evidence":s.evidence,"provenances":s.provenances,"tokens":s.tokens,
        "available":s.available,"present":s.present})
}
/// Shared eligibility predicate for planning and CG-10 satisfaction revalidation.
pub fn context_source_rejection(
    r: &ContextRequirement,
    s: &ContextSourceMetadata,
    scope: &ContextScopeId,
) -> Option<ContextSelectionReason> {
    use ContextSelectionReason::*;
    if &s.pin.scope != scope {
        return Some(CrossScope);
    }
    if !r.permitted_sources.contains(&s.class) {
        return Some(NotPermitted);
    }
    if !s.covers.contains(&r.id) {
        return Some(NotRelevant);
    }
    if !s.available {
        return Some(Unavailable);
    }
    let q = s.quality;
    if !r.information.accepted_trust.contains(&q.trust()) || q.uncertainty() != Uncertainty::None {
        return Some(Untrusted);
    }
    if q.conflict() != ConflictStatus::None {
        return Some(Conflicting);
    }
    if r.information.requirements.freshness() == FreshnessRequirement::Fresh
        && q.freshness() != FreshnessStatus::Fresh
    {
        return Some(Stale);
    }
    if q.sensitivity() > r.information.maximum_sensitivity
        || r.information
            .requirements
            .minimum_sensitivity()
            .is_some_and(|m| q.sensitivity() < m)
    {
        return Some(SensitivityRejected);
    }
    if s.present.is_some()
        && (s.payload_digest.is_none()
            || !r
                .information
                .requirements
                .evidence()
                .iter()
                .all(|e| s.evidence.contains(e))
            || !r
                .information
                .requirements
                .provenances()
                .iter()
                .all(|p| s.provenances.contains(p))
            || matches!(r.stop, StopCondition::EvidenceSatisfied(n) if (s.evidence.len() as u64) < n.get()))
    {
        return Some(EvidenceMissing);
    }
    if s.tokens > r.budget.context.total().0
        || s.tokens > r.budget.tokens.0
        || s.tokens
            > r.budget
                .context
                .reservations()
                .get(&context_requirement_budget_class(r.class))
                .map_or(0, |t| t.0)
    {
        return Some(BudgetExceeded);
    }
    None
}

fn serialize_plan_pin<S: serde::Serializer>(
    pin: &Option<ResolvedReference<PlanId>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    pin.as_ref()
        .map(|p| {
            json!({"id":p.id.as_str(),"scope":p.scope,"contract":p.contract,
        "contract_version":p.contract_version,"revision":p.revision,"digest":p.digest})
        })
        .serialize(serializer)
}

/// Same disjoint CG-15/CG-10 budget classes; no implicit borrowing.
pub fn context_requirement_budget_class(class: ContextClass) -> ContextBudgetClass {
    match class {
        ContextClass::Authority => ContextBudgetClass::AuthorityReserved,
        ContextClass::TaskInput => ContextBudgetClass::TaskReserved,
        ContextClass::OutputVerification => ContextBudgetClass::OutputContractReserved,
        ContextClass::Evidence => ContextBudgetClass::Evidence,
        ContextClass::Memory => ContextBudgetClass::Memory,
        ContextClass::RuntimeState => ContextBudgetClass::RuntimeState,
        _ => ContextBudgetClass::Knowledge,
    }
}
