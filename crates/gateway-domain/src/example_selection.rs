//! Canonical #286 contracts. Captured metadata is authenticated by the host;
//! examples never confer authority. Validated sets are output-only values.
use crate::memory::CurationState;
use crate::*;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExampleSourceClass {
    Curated,
    Repository,
    Retrieval,
    GovernedExperience,
    PromotedProcedure,
}
impl ExampleSourceClass {
    pub fn context_class(self) -> ContextSourceClass {
        match self {
            Self::Curated | Self::PromotedProcedure => ContextSourceClass::Examples,
            Self::Repository => ContextSourceClass::Repository,
            Self::Retrieval => ContextSourceClass::Rag,
            Self::GovernedExperience => ContextSourceClass::Memory,
        }
    }
}
/// Exact compatibility gates precede all advisory similarity. Tags describe
/// language, technology and domain; architecture is a pinned definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExampleCompatibility {
    pub task: SemanticTaskType,
    pub target: TargetKind,
    pub output: ResolvedReference<ReferenceId>,
    pub architecture: Option<ResolvedReference<ReferenceId>>,
    pub tags: BTreeSet<ReferenceId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExamplePolicy {
    pub sources: BTreeSet<ExampleSourceClass>,
    /// Explicit source-scope grants pinned to a reuse policy in the task scope.
    pub reuse: BTreeMap<ContextScopeId, ResolvedReference<PolicyId>>,
    pub architecture: Option<ResolvedReference<ReferenceId>>,
    pub tags: BTreeSet<ReferenceId>,
    /// Baseline tag overlap in thousandths; task/target/output gates still apply.
    pub minimum_relevance: u16,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExampleRequirement {
    requirements: ResolvedReference<ReferenceId>,
    context: ContextRequirement,
    compatibility: ExampleCompatibility,
    policy: ExamplePolicy,
}
impl ExampleRequirement {
    /// No active #285 example requirement means selection is not activated.
    pub fn from_context(
        requirements: &ContextRequirementsIR,
        task: &SemanticTaskIR,
        id: &ReferenceId,
        policy: ExamplePolicy,
    ) -> Result<Option<Self>, ContextPlanningError> {
        use ContextPlanningError as E;
        if requirements.basis().task.id != task.data().id
            || requirements.basis().task.digest
                != task.content_digest().map_err(|_| E::BindingMismatch)?
            || requirements.basis().task.scope != task.data().target.reference.scope
        {
            return Err(E::BindingMismatch);
        }
        let Some(context) = requirements
            .requirements()
            .iter()
            .find(|r| &r.id == id && r.class == ContextClass::Examples && r.necessity.active())
        else {
            return Ok(None);
        };
        if policy.sources.is_empty()
            || policy.minimum_relevance > 1000
            || !context
                .permitted_sources
                .contains(&ContextSourceClass::Examples)
            || policy
                .sources
                .iter()
                .any(|s| !context.permitted_sources.contains(&s.context_class()))
            || policy.reuse.values().any(|p| {
                p.scope != requirements.basis().task.scope
                    || p.contract_version != SchemaVersion::V1
            })
            || policy
                .architecture
                .as_ref()
                .is_some_and(|p| p.contract_version != SchemaVersion::V1)
        {
            return Err(E::InvalidRequirements);
        }
        Ok(Some(Self {
            requirements: requirements.binding(),
            context: context.clone(),
            compatibility: ExampleCompatibility {
                task: task.data().task_type,
                target: task.data().target.kind,
                output: task.data().output_contract.schema.clone(),
                architecture: policy.architecture.clone(),
                tags: policy.tags.clone(),
            },
            policy,
        }))
    }
    pub fn context(&self) -> &ContextRequirement {
        &self.context
    }
    pub fn requirements(&self) -> &ResolvedReference<ReferenceId> {
        &self.requirements
    }
    pub fn compatibility(&self) -> &ExampleCompatibility {
        &self.compatibility
    }
    pub fn policy(&self) -> &ExamplePolicy {
        &self.policy
    }
    pub fn to_value(&self) -> Value {
        json!({"schema_version":SchemaVersion::V1,"requirements":self.requirements,
            "requirement":self.context.id,"purpose":self.context.purpose,"necessity":self.context.necessity,
            "compatibility":self.compatibility,
            "sources":self.policy.sources,"reuse":self.policy.reuse.iter().map(|(s,p)| json!({"scope":s,"policy":p})).collect::<Vec<_>>(),
            "minimum_relevance":self.policy.minimum_relevance})
    }
}
/// Input/output are the existing CG-16 captured fragments, including provenance,
/// quality and source snapshot. No rationale or hidden reasoning field exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExampleCandidate {
    pub pin: ResolvedReference<ReferenceId>,
    pub source_class: ExampleSourceClass,
    pub input: RetrievedFragment,
    pub expected_output: Option<RetrievedFragment>,
    pub compatibility: ExampleCompatibility,
    pub validation: CurationState,
    /// Exact validation record; its own digest is authenticated by the host.
    pub validation_ref: Option<ResolvedReference<ReferenceId>>,
    /// Candidate content validated by that record, separate from its record digest.
    pub validated_digest: Option<ContentDigest>,
    pub successful: bool,
    pub tokens: TokenEstimate,
}
impl ExampleCandidate {
    pub fn content_value(&self) -> Value {
        json!({"source_class":self.source_class,"input":fragment_value(&self.input),
            "expected_output":self.expected_output.as_ref().map(fragment_value),
            "compatibility":self.compatibility,"successful":self.successful})
    }
    pub fn content_digest(&self) -> ContentDigest {
        context_content_digest(self.content_value().to_string().as_bytes())
    }
    pub fn to_value(&self) -> Value {
        json!({"schema_version":self.pin.contract_version,"pin":self.pin,"data":self.content_value(),
            "validation":format!("{:?}",self.validation),"validation_ref":self.validation_ref,"validated_digest":self.validated_digest,
            "token_estimate":{"estimator":self.tokens.estimator.as_str(),"version":self.tokens.version.as_str(),
                "count":match &self.tokens.count {
                    TokenCount::Exact {tokens,target} => json!({"kind":"EXACT","tokens":tokens,"target":target}),
                    TokenCount::Estimated {tokens,upper_bound,semantics} => json!({"kind":"ESTIMATED","tokens":tokens,"upper_bound":upper_bound,"semantics":semantics}),
                    TokenCount::Unknown {reason} => json!({"kind":"UNKNOWN","reason":reason})},"bound":self.tokens.budget_bound().ok()}})
    }
    pub fn fragments(&self) -> impl Iterator<Item = &RetrievedFragment> {
        std::iter::once(&self.input).chain(self.expected_output.iter())
    }
    pub fn quality(&self) -> QualityMetadata {
        QualityMetadata::merge(&self.fragments().map(|f| f.quality).collect::<Vec<_>>())
            .expect("input exists")
    }
}
fn fragment_value(f: &RetrievedFragment) -> Value {
    json!({"id":f.id,"scope":f.scope,"content":f.content,"provenance":f.provenance,
        "snapshot":f.snapshot,"quality":f.quality,"evidence":f.evidence})
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExampleReason {
    Selected,
    NotPermitted,
    InvalidBinding,
    Unvalidated,
    Unsuccessful,
    Contaminated,
    CrossScope,
    Incompatible,
    Stale,
    Untrusted,
    SensitivityRejected,
    EvidenceMissing,
    Irrelevant,
    InvalidTokens,
    Duplicate,
    NearDuplicate,
    CountLimit,
    TokenLimit,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExampleDecision {
    pub candidate: ResolvedReference<ReferenceId>,
    pub reason: ExampleReason,
    pub relevance: u16,
    pub advisory_score: Option<i64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExampleSet {
    revision: ReferenceId,
    requirement: ExampleRequirement,
    selected: Vec<ExampleCandidate>,
    decisions: Vec<ExampleDecision>,
    usage: BudgetUsage,
}
impl ExampleSet {
    /// Validate rather than trusting caller-supplied selection or budget totals.
    pub fn new(
        revision: ReferenceId,
        requirement: ExampleRequirement,
        selected: Vec<ExampleCandidate>,
        decisions: Vec<ExampleDecision>,
    ) -> Result<Self, ContextPlanningError> {
        use ContextPlanningError as E;
        let mut ids = BTreeSet::new();
        if selected.iter().any(|c| {
            !ids.insert(&c.pin.id)
                || example_rejection(&requirement, c, &BTreeSet::new()).is_some()
                || !decisions
                    .iter()
                    .any(|d| d.candidate == c.pin && d.reason == ExampleReason::Selected)
        }) || decisions
            .iter()
            .filter(|d| d.reason == ExampleReason::Selected)
            .count()
            != selected.len()
        {
            return Err(E::InvalidSatisfaction);
        }
        for (i, c) in selected.iter().enumerate() {
            if selected[..i]
                .iter()
                .any(|p| example_duplicate(p, c).is_some())
            {
                return Err(E::InvalidSatisfaction);
            }
        }
        let tokens = selected
            .iter()
            .try_fold(0u64, |n, c| n.checked_add(c.tokens.budget_bound().ok()?))
            .ok_or(E::InfeasibleBudget)?;
        let budget = &requirement.context.budget;
        let usage = BudgetUsage {
            results: selected.len() as u64,
            rounds: 0,
            elapsed_ms: 0,
            cost: 0,
            cost_unit: budget.cost.unit.clone(),
            tokens,
            context: BTreeMap::from([(ContextBudgetClass::Knowledge, tokens)]),
        };
        usage.validate(budget).map_err(|_| E::InfeasibleBudget)?;
        if tokens > requirement.context.budget.context.total().0 {
            return Err(E::InfeasibleBudget);
        }
        Ok(Self {
            revision,
            requirement,
            selected,
            decisions,
            usage,
        })
    }
    pub fn requirement(&self) -> &ExampleRequirement {
        &self.requirement
    }
    pub fn selected(&self) -> &[ExampleCandidate] {
        &self.selected
    }
    pub fn decisions(&self) -> &[ExampleDecision] {
        &self.decisions
    }
    pub fn usage(&self) -> &BudgetUsage {
        &self.usage
    }
    pub fn to_value(&self) -> Value {
        let b = &self.requirement.context.budget;
        json!({"schema_version":SchemaVersion::V1,"revision":self.revision,
            "requirement":self.requirement.to_value(),"selected":self.selected.iter().map(|c|c.to_value()).collect::<Vec<_>>(),
            "decisions":self.decisions,"limits":{"count":b.results.0.get(),"tokens":b.tokens.0,
                "total_context_tokens":b.context.total().0,"context_tokens":b.context.reservations().get(&ContextBudgetClass::Knowledge).map_or(0,|n|n.0)},
            "usage":{"count":self.usage.results,"tokens":self.usage.tokens,"context_tokens":self.usage.tokens}})
    }
    pub fn binding(&self) -> ResolvedReference<ReferenceId> {
        binding(
            "example-set",
            &self.revision,
            &self.requirement.requirements.scope,
            &self.to_value(),
        )
    }
    /// Plan the bounded set as one dedicated satisfaction link, never a fragment.
    pub fn source(&self) -> Option<ContextSourceMetadata> {
        let quality = QualityMetadata::merge(
            &self
                .selected
                .iter()
                .map(|c| c.quality())
                .collect::<Vec<_>>(),
        )?;
        let pin = self.binding();
        Some(ContextSourceMetadata {
            payload_digest: Some(pin.digest.clone()),
            present: Some(ContextSatisfaction::Examples(pin.id.clone())),
            pin,
            class: ContextSourceClass::Examples,
            covers: BTreeSet::from([self.requirement.context.id.clone()]),
            quality,
            evidence: self
                .selected
                .iter()
                .flat_map(|c| c.fragments().flat_map(|f| f.evidence.iter().cloned()))
                .collect(),
            provenances: self
                .selected
                .iter()
                .flat_map(|c| c.fragments().map(|f| f.provenance.id().clone()))
                .collect(),
            tokens: self.usage.tokens,
            available: true,
        })
    }
}
/// CG-20A's authenticated quarantine is external to payload assertions.
/// Both original fragment IDs and example IDs are checked, preventing laundering.
pub fn example_rejection(
    r: &ExampleRequirement,
    c: &ExampleCandidate,
    quarantined: &BTreeSet<ReferenceId>,
) -> Option<ExampleReason> {
    use ExampleReason::*;
    if !r.policy.sources.contains(&c.source_class) {
        return Some(NotPermitted);
    }
    if c.pin.contract_version != SchemaVersion::V1
        || c.pin.contract.as_str() != "example"
        || c.pin.digest != c.content_digest()
        || c.fragments().any(|f| {
            f.scope != c.pin.scope
                || f.snapshot != context_content_digest(f.content.as_str().as_bytes())
        })
    {
        return Some(InvalidBinding);
    }
    if c.validation != CurationState::Validated
        || c.validated_digest.as_ref() != Some(&c.pin.digest)
        || c.validation_ref
            .as_ref()
            .is_none_or(|v| v.scope != c.pin.scope || v.contract_version != SchemaVersion::V1)
    {
        return Some(Unvalidated);
    }
    if !c.successful {
        return Some(Unsuccessful);
    }
    if quarantined.contains(&c.pin.id) || c.fragments().any(|f| quarantined.contains(&f.id)) {
        return Some(Contaminated);
    }
    if c.pin.scope != r.requirements.scope && !r.policy.reuse.contains_key(&c.pin.scope) {
        return Some(CrossScope);
    }
    let a = &r.compatibility;
    let b = &c.compatibility;
    if a.task != b.task
        || a.target != b.target
        || a.output != b.output
        || a.architecture != b.architecture
    {
        return Some(Incompatible);
    }
    if c.fragments()
        .any(|f| f.quality.freshness() != FreshnessStatus::Fresh)
    {
        return Some(Stale);
    }
    if c.fragments().any(|f| {
        !r.context
            .information
            .accepted_trust
            .contains(&f.quality.trust())
            || f.quality.trust() == TrustClass::CanonicalReference
            || f.quality.uncertainty() != Uncertainty::None
            || f.quality.conflict() != ConflictStatus::None
    }) {
        return Some(Untrusted);
    }
    if c.fragments().any(|f| {
        f.quality.sensitivity() > r.context.information.maximum_sensitivity
            || r.context
                .information
                .requirements
                .minimum_sensitivity()
                .is_some_and(|s| f.quality.sensitivity() < s)
    }) {
        return Some(SensitivityRejected);
    }
    let evidence: BTreeSet<_> = c
        .fragments()
        .flat_map(|f| f.evidence.iter().cloned())
        .collect();
    let provenance: BTreeSet<_> = c.fragments().map(|f| f.provenance.id()).collect();
    if !r
        .context
        .information
        .requirements
        .evidence()
        .iter()
        .all(|e| evidence.contains(e))
        || !r
            .context
            .information
            .requirements
            .provenances()
            .iter()
            .all(|p| provenance.contains(p))
        || matches!(r.context.stop, StopCondition::EvidenceSatisfied(n) if (evidence.len() as u64) < n.get())
    {
        return Some(EvidenceMissing);
    }
    if example_relevance(r, c) < r.policy.minimum_relevance {
        return Some(Irrelevant);
    }
    if c.tokens.budget_bound().is_err() || c.tokens.budget_bound() == Ok(0) {
        return Some(InvalidTokens);
    }
    None
}
pub fn example_relevance(r: &ExampleRequirement, c: &ExampleCandidate) -> u16 {
    let tags = &r.compatibility.tags;
    if tags.is_empty() {
        return 1000;
    }
    ((tags.intersection(&c.compatibility.tags).count() * 1000) / tags.len()) as u16
}
/// Normalized input/output token sets: exact pair or >=90% Jaccard overlap.
/// No adapter-supplied duplicate label can suppress unrelated examples.
pub fn example_duplicate(a: &ExampleCandidate, b: &ExampleCandidate) -> Option<ExampleReason> {
    let pair = |c: &ExampleCandidate| {
        (
            c.input.content.as_str().to_lowercase(),
            c.expected_output
                .as_ref()
                .map(|f| f.content.as_str().to_lowercase()),
        )
    };
    if pair(a) == pair(b) {
        return Some(ExampleReason::Duplicate);
    }
    let words = |c: &ExampleCandidate| {
        c.fragments()
            .flat_map(|f| f.content.as_str().split(|ch: char| !ch.is_alphanumeric()))
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect::<BTreeSet<_>>()
    };
    let x = words(a);
    let y = words(b);
    if !x.is_empty() && x.intersection(&y).count() * 10 >= x.union(&y).count() * 9 {
        Some(ExampleReason::NearDuplicate)
    } else {
        None
    }
}
