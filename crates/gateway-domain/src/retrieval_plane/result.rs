use super::*;
use crate::{
    ContentDigest, ContextScopeId, EvidenceId, FreshnessRequirement, FreshnessStatus, NonEmptyText,
    Provenance, QualityMetadata, ReferenceId,
};
use std::collections::BTreeSet;

/// Advisory material; the reference ID is compatible with CG-10 ContextFragment identity.
/// A snapshot digest is required even if the source has no revision label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievedFragment {
    pub id: ReferenceId,
    pub scope: ContextScopeId,
    pub content: NonEmptyText,
    pub provenance: Provenance,
    pub snapshot: ContentDigest,
    pub quality: QualityMetadata,
    pub evidence: BTreeSet<EvidenceId>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RetrievalReason {
    ExplicitSelection,
    Relevant,
    TrustRejected,
    FreshnessRejected,
    SensitivityRejected,
    Duplicate,
    BudgetReached,
    EvidenceSatisfied,
    NoMatches,
    ServiceUnavailable,
    Unsupported,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RetrievalExplanationTarget {
    Source(RetrievalSourceId),
    Strategy(RetrievalStrategyId),
    Result(ReferenceId),
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RetrievalExplanation {
    pub target: RetrievalExplanationTarget,
    pub selected: bool,
    pub reason: RetrievalReason,
    pub detail: NonEmptyText,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalStatus {
    Complete,
    Partial,
    Degraded,
    Failed,
}

/// Score is relevance in millionths on the shared [0, 1] scale, never raw provider
/// distance. Sort descending score, then source ID, strategy ID, fragment ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalResult {
    pub fragment: RetrievedFragment,
    pub source: RetrievalSourceId,
    pub strategy: RetrievalStrategyId,
    pub score: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalBatchInput {
    pub version: RetrievalVersion,
    pub plan: RetrievalPlanId,
    pub scope: ContextScopeId,
    pub round: RetrievalRound,
    pub status: RetrievalStatus,
    pub reason: RetrievalReason,
    pub results: Vec<RetrievalResult>,
    pub usage: BudgetUsage,
    pub explanations: BTreeSet<RetrievalExplanation>,
}
/// Immutable validated result envelope, including empty and failed results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalBatch(RetrievalBatchInput);
impl RetrievalBatch {
    pub fn new(
        mut input: RetrievalBatchInput,
        plan: &RetrievalPlan,
    ) -> Result<Self, RetrievalError> {
        let request = plan.request().input();
        if input.scope != request.scope {
            return Err(RetrievalError::ScopeMismatch);
        }
        if input.plan != *plan.id()
            || input.version != plan.version()
            || input.round.0.get() > request.budget.rounds.0.get()
            || input.usage.rounds != input.round.0.get()
            || input.results.len() as u64 > input.usage.results
        {
            return Err(RetrievalError::InvalidResult);
        }
        input.usage.validate(&request.budget)?;
        let status_valid = match input.status {
            RetrievalStatus::Complete => matches!(
                input.reason,
                RetrievalReason::EvidenceSatisfied
                    | RetrievalReason::NoMatches
                    | RetrievalReason::BudgetReached
            ),
            RetrievalStatus::Partial => input.reason == RetrievalReason::BudgetReached,
            RetrievalStatus::Degraded => input.reason == RetrievalReason::ServiceUnavailable,
            RetrievalStatus::Failed => {
                input.results.is_empty()
                    && matches!(
                        input.reason,
                        RetrievalReason::ServiceUnavailable | RetrievalReason::Unsupported
                    )
            }
        };
        if !status_valid {
            return Err(RetrievalError::InvalidResult);
        }
        for explanation in &input.explanations {
            let (known, optional, returned) = match &explanation.target {
                RetrievalExplanationTarget::Source(id) => (
                    request.sources.iter().any(|s| &s.id == id),
                    request.sources.iter().any(|s| &s.id == id && s.optional),
                    input.results.iter().any(|r| &r.source == id),
                ),
                RetrievalExplanationTarget::Strategy(id) => (
                    request.strategies.iter().any(|s| &s.id == id),
                    request.strategies.iter().any(|s| &s.id == id && s.optional),
                    input.results.iter().any(|r| &r.strategy == id),
                ),
                RetrievalExplanationTarget::Result(id) => (
                    true,
                    false,
                    input.results.iter().any(|r| &r.fragment.id == id),
                ),
            };
            if !known
                || (!explanation.selected && returned)
                || (explanation.reason == RetrievalReason::ServiceUnavailable
                    && (explanation.selected
                        || (input.status == RetrievalStatus::Degraded && !optional)))
            {
                return Err(RetrievalError::InvalidResult);
            }
        }
        let mut ids = BTreeSet::new();
        let mut evidence = BTreeSet::new();
        for result in &input.results {
            let fragment = &result.fragment;
            if fragment.scope != input.scope {
                return Err(RetrievalError::ScopeMismatch);
            }
            if !ids.insert(fragment.id.clone()) {
                return Err(RetrievalError::DuplicateIdentity);
            }
            if !(0..=1_000_000).contains(&result.score)
                || !request.sources.iter().any(|s| s.id == result.source)
                || !request.strategies.iter().any(|s| s.id == result.strategy)
                || !request
                    .required
                    .accepted_trust
                    .contains(&fragment.quality.trust())
                || fragment.quality.sensitivity() > request.required.maximum_sensitivity
                || (request.required.requirements.freshness() == FreshnessRequirement::Fresh
                    && fragment.quality.freshness() != FreshnessStatus::Fresh)
                || !input.explanations.iter().any(|e| {
                    e.target == RetrievalExplanationTarget::Result(fragment.id.clone())
                        && e.selected
                        && e.reason == RetrievalReason::Relevant
                })
            {
                return Err(RetrievalError::InvalidResult);
            }
            evidence.extend(fragment.evidence.iter().cloned());
        }
        if input.reason == RetrievalReason::NoMatches && !input.results.is_empty() {
            return Err(RetrievalError::InvalidResult);
        }
        if input.reason == RetrievalReason::EvidenceSatisfied
            && !matches!(request.stop, Some(StopCondition::EvidenceSatisfied(n)) if evidence.len() as u64 >= n.get())
        {
            return Err(RetrievalError::InvalidResult);
        }
        if input.reason == RetrievalReason::BudgetReached
            && !plan.should_stop(&input.usage, &BTreeSet::new())?
        {
            return Err(RetrievalError::InvalidResult);
        }
        if input.status == RetrievalStatus::Degraded
            && !input
                .explanations
                .iter()
                .any(|e| e.reason == RetrievalReason::ServiceUnavailable)
        {
            return Err(RetrievalError::InvalidResult);
        }
        input.results.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.source.cmp(&b.source))
                .then_with(|| a.strategy.cmp(&b.strategy))
                .then_with(|| a.fragment.id.cmp(&b.fragment.id))
        });
        Ok(Self(input))
    }
    pub fn input(&self) -> &RetrievalBatchInput {
        &self.0
    }
}
