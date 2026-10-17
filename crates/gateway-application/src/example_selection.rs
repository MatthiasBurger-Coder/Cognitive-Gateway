//! Deterministic example orchestration. No acquisition, validation promotion or rendering.
use gateway_domain::*;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

/// Optional embedding/model adapter; scores only order already eligible examples
/// with equal baseline relevance. Failure yields the deterministic baseline.
pub trait ExampleRanker {
    fn scores(
        &self,
        requirement: &ExampleRequirement,
        eligible: &[ExampleCandidate],
    ) -> Result<BTreeMap<ReferenceId, i64>, NonEmptyText>;
}
pub fn select_examples(
    requirement: Option<&ExampleRequirement>,
    revision: ReferenceId,
    candidates: &[ExampleCandidate],
    quarantined: &BTreeSet<ReferenceId>,
    ranker: Option<&dyn ExampleRanker>,
) -> Result<Option<ExampleSet>, ContextPlanningError> {
    let Some(r) = requirement else {
        return Ok(None);
    };
    let mut identities = BTreeSet::new();
    if candidates.iter().any(|c| !identities.insert(&c.pin.id)) {
        return Err(ContextPlanningError::ConflictingRequirements);
    }
    let mut eligible = Vec::new();
    let mut decisions = Vec::new();
    let mut ordered: Vec<_> = candidates.iter().collect();
    ordered.sort_by_key(|c| &c.pin.id);
    for c in ordered {
        if let Some(reason) = example_rejection(r, c, quarantined) {
            decisions.push(ExampleDecision {
                candidate: c.pin.clone(),
                reason,
                relevance: example_relevance(r, c),
                advisory_score: None,
            });
        } else {
            eligible.push(c.clone());
        }
    }
    let scores = ranker
        .and_then(|a| a.scores(r, &eligible).ok())
        .unwrap_or_default();
    let mut selected = Vec::new();
    let mut tokens = 0u64;
    let budget = &r.context().budget;
    let limit = budget.tokens.0.min(budget.context.total().0).min(
        budget
            .context
            .reservations()
            .get(&ContextBudgetClass::Knowledge)
            .map_or(0, |v| v.0),
    );
    let mut covered = BTreeSet::new();
    while !eligible.is_empty() {
        eligible.sort_by_key(|c| {
            (
                Reverse(example_relevance(r, c)),
                Reverse(c.compatibility.tags.difference(&covered).count()),
                Reverse(scores.get(&c.pin.id).copied().unwrap_or(0)),
                c.tokens.budget_bound().expect("eligible bound"),
                c.pin.id.clone(),
            )
        });
        let c = eligible.remove(0);
        let bound = c.tokens.budget_bound().expect("eligible bound");
        let reason = if let Some(reason) = selected.iter().find_map(|s| example_duplicate(s, &c)) {
            reason
        } else if selected.len() as u64 >= budget.results.0.get() {
            ExampleReason::CountLimit
        } else if tokens.checked_add(bound).is_none_or(|n| n > limit) {
            ExampleReason::TokenLimit
        } else {
            tokens += bound;
            covered.extend(c.compatibility.tags.iter().cloned());
            selected.push(c.clone());
            ExampleReason::Selected
        };
        decisions.push(ExampleDecision {
            candidate: c.pin.clone(),
            reason,
            relevance: example_relevance(r, &c),
            advisory_score: scores.get(&c.pin.id).copied(),
        });
    }
    ExampleSet::new(revision, r.clone(), selected, decisions).map(Some)
}
