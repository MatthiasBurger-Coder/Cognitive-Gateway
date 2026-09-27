//! Deterministic hybrid retrieval composition over CG-15 result contracts.
//!
//! Source adapters still own repository/Git/vector access. This module combines
//! their validated candidates while keeping scope, trust, sensitivity and source
//! lineage attached to the original immutable fragment.
use crate::graph_retrieval::GraphPath;
use gateway_domain::{
    NonEmptyText, ReferenceId, RetrievalError, RetrievalResult, RetrievalSourceId,
    RetrievalStrategyId,
};
use std::collections::{BTreeMap, BTreeSet};

/// Structural location supplied by a document, repository or symbol adapter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContextualLocation {
    pub document: NonEmptyText,
    pub section: Option<NonEmptyText>,
    pub symbol: Option<NonEmptyText>,
    pub revision: NonEmptyText,
}

/// Scores use the CG-15 millionths scale. Exact lexical identity is retained as
/// an explicit signal so semantic similarity cannot hide an identifier hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HybridCandidate {
    result: RetrievalResult,
    contributors: Vec<RetrievalResult>,
    pub location: Option<ContextualLocation>,
    pub exact_match: bool,
    pub lexical_score: Option<u32>,
    pub semantic_score: Option<u32>,
    /// Inspectable graph lineage when a graph adapter produced this hit.
    pub graph_paths: Vec<GraphPath>,
}

impl HybridCandidate {
    pub fn new(
        result: RetrievalResult,
        location: Option<ContextualLocation>,
        exact_match: bool,
        lexical_score: Option<u32>,
        semantic_score: Option<u32>,
    ) -> Result<Self, RetrievalError> {
        if result.score < 0
            || result.score > 1_000_000
            || lexical_score.is_some_and(|score| score > 1_000_000)
            || semantic_score.is_some_and(|score| score > 1_000_000)
            || (lexical_score.is_none() && semantic_score.is_none())
        {
            return Err(RetrievalError::InvalidResult);
        }
        Ok(Self {
            result,
            contributors: Vec::new(),
            location,
            exact_match,
            lexical_score,
            semantic_score,
            graph_paths: Vec::new(),
        })
    }

    #[must_use]
    pub const fn result(&self) -> &RetrievalResult {
        &self.result
    }

    /// Additional identical-content hits retain their own source lineage.
    #[must_use]
    pub fn contributors(&self) -> &[RetrievalResult] {
        &self.contributors
    }

    /// Final ordering scores remain bounded by the CG-15 relevance scale.
    pub fn set_final_score(&mut self, score: u32) -> Result<(), RetrievalError> {
        if score > 1_000_000 {
            return Err(RetrievalError::InvalidResult);
        }
        self.result.score = i64::from(score);
        Ok(())
    }
}

/// Explicit fusion weights. Both weights are integer millionths and their sum
/// must be nonzero; absent signals do not dilute the available signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FusionPolicy {
    pub lexical_weight: u32,
    pub semantic_weight: u32,
    pub exact_match_floor: u32,
}

impl FusionPolicy {
    pub fn validate(self) -> Result<Self, RetrievalError> {
        if self.lexical_weight > 1_000_000
            || self.semantic_weight > 1_000_000
            || self.exact_match_floor > 1_000_000
            || self.lexical_weight + self.semantic_weight == 0
        {
            return Err(RetrievalError::InvalidPlan);
        }
        Ok(self)
    }
}

/// Deterministic hybrid ranking. Duplicate content is collapsed after fusion;
/// exact matches sort ahead of semantic-only matches regardless of score.
pub fn fuse_candidates(
    candidates: impl IntoIterator<Item = HybridCandidate>,
    policy: FusionPolicy,
) -> Result<Vec<HybridCandidate>, RetrievalError> {
    let policy = policy.validate()?;
    let mut merged: BTreeMap<_, HybridCandidate> = BTreeMap::new();
    for mut candidate in candidates {
        let normalized = canonical_content(candidate.result.fragment.content.as_str());
        if normalized.is_empty() {
            return Err(RetrievalError::InvalidResult);
        }
        let fragment = &candidate.result.fragment;
        let key = (
            fragment.scope.clone(),
            normalized,
            fragment.provenance.clone(),
            fragment.snapshot.clone(),
            fragment.quality,
            fragment.evidence.clone(),
        );
        match merged.get_mut(&key) {
            Some(existing) => {
                let exact_match = existing.exact_match || candidate.exact_match;
                let lexical_score = max_opt(existing.lexical_score, candidate.lexical_score);
                let semantic_score = max_opt(existing.semantic_score, candidate.semantic_score);
                let incoming_preferred = (candidate.exact_match && !existing.exact_match)
                    || (candidate.exact_match == existing.exact_match
                        && candidate.result.score > existing.result.score)
                    || (candidate.exact_match == existing.exact_match
                        && candidate.result.score == existing.result.score
                        && (
                            &candidate.result.source,
                            &candidate.result.strategy,
                            &candidate.result.fragment.id,
                        ) < (
                            &existing.result.source,
                            &existing.result.strategy,
                            &existing.result.fragment.id,
                        ));
                if incoming_preferred {
                    // Keep the winning source's complete fragment and provenance intact.
                    let mut contributors = std::mem::take(&mut existing.contributors);
                    contributors.push(existing.result.clone());
                    candidate
                        .graph_paths
                        .extend(std::mem::take(&mut existing.graph_paths));
                    *existing = candidate;
                    existing.contributors.extend(contributors);
                } else {
                    existing.graph_paths.append(&mut candidate.graph_paths);
                    existing.contributors.push(candidate.result);
                    existing.contributors.extend(candidate.contributors);
                }
                existing.exact_match = exact_match;
                existing.lexical_score = lexical_score;
                existing.semantic_score = semantic_score;
            }
            None => {
                merged.insert(key, candidate);
            }
        }
    }
    let mut values: Vec<_> = merged.into_values().collect();
    for candidate in &mut values {
        candidate.graph_paths.sort_by(|a, b| {
            a.root.id.cmp(&b.root.id).then_with(|| {
                a.steps
                    .iter()
                    .map(|step| &step.edge.id)
                    .cmp(b.steps.iter().map(|step| &step.edge.id))
            })
        });
        candidate.graph_paths.dedup();
        candidate.contributors.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| a.strategy.cmp(&b.strategy))
                .then_with(|| a.fragment.id.cmp(&b.fragment.id))
        });
        let mut total = 0u64;
        let mut weight = 0u64;
        if let Some(score) = candidate.lexical_score {
            total += u64::from(score) * u64::from(policy.lexical_weight);
            weight += u64::from(policy.lexical_weight);
        }
        if let Some(score) = candidate.semantic_score {
            total += u64::from(score) * u64::from(policy.semantic_weight);
            weight += u64::from(policy.semantic_weight);
        }
        let fused = total.checked_div(weight).unwrap_or(0) as i64;
        candidate.result.score = if candidate.exact_match {
            fused.max(i64::from(policy.exact_match_floor))
        } else {
            fused
        };
    }
    values.sort_by(|a, b| {
        b.exact_match
            .cmp(&a.exact_match)
            .then_with(|| b.result.score.cmp(&a.result.score))
            .then_with(|| a.result.source.cmp(&b.result.source))
            .then_with(|| a.result.strategy.cmp(&b.result.strategy))
            .then_with(|| a.result.fragment.id.cmp(&b.result.fragment.id))
    });
    Ok(values)
}

/// Per-adapter failure is retained in the federated outcome, including failures
/// when other sources returned usable candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFailure {
    pub source: RetrievalSourceId,
    pub strategy: RetrievalStrategyId,
    pub reason: RetrievalError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedCandidates {
    pub candidates: Vec<HybridCandidate>,
    pub failures: BTreeSet<(RetrievalSourceId, RetrievalStrategyId, RetrievalError)>,
}

/// One lexical, semantic/vector, repository, Git or external source adapter.
/// Adapters receive only the immutable request and their explicitly selected IDs.
pub trait RetrievalSourceAdapter {
    fn source(&self) -> &RetrievalSourceId;
    fn strategy(&self) -> &RetrievalStrategyId;
    fn retrieve(
        &self,
        request: &gateway_domain::RetrievalRequest,
    ) -> Result<Vec<HybridCandidate>, RetrievalError>;
}

/// Dispatch each registered selected pair, retaining partial failures. A source
/// or strategy with no selected adapter is explicitly unavailable. Results with
/// a different scope or adapter identity are rejected as an adapter failure.
pub fn federate(
    plan: &gateway_domain::RetrievalPlan,
    adapters: &[&dyn RetrievalSourceAdapter],
) -> FederatedCandidates {
    let request = plan.request();
    let mut output = FederatedCandidates {
        candidates: Vec::new(),
        failures: BTreeSet::new(),
    };
    for source in &request.input().sources {
        if !adapters.iter().any(|adapter| {
            adapter.source() == &source.id
                && request
                    .input()
                    .strategies
                    .iter()
                    .any(|strategy| adapter.strategy() == &strategy.id)
        }) {
            if let Some(strategy) = request.input().strategies.first() {
                output.failures.insert((
                    source.id.clone(),
                    strategy.id.clone(),
                    RetrievalError::ServiceUnavailable,
                ));
            }
        }
    }
    for strategy in &request.input().strategies {
        if !adapters.iter().any(|adapter| {
            adapter.strategy() == &strategy.id
                && request
                    .input()
                    .sources
                    .iter()
                    .any(|source| adapter.source() == &source.id)
        }) {
            if let Some(source) = request.input().sources.first() {
                output.failures.insert((
                    source.id.clone(),
                    strategy.id.clone(),
                    RetrievalError::ServiceUnavailable,
                ));
            }
        }
    }
    let mut seen = BTreeSet::new();
    for adapter in adapters {
        let Some(source) = request
            .input()
            .sources
            .iter()
            .find(|source| adapter.source() == &source.id)
        else {
            continue;
        };
        let Some(strategy) = request
            .input()
            .strategies
            .iter()
            .find(|strategy| adapter.strategy() == &strategy.id)
        else {
            continue;
        };
        if !seen.insert((source.id.clone(), strategy.id.clone())) {
            output.failures.insert((
                source.id.clone(),
                strategy.id.clone(),
                RetrievalError::DuplicateIdentity,
            ));
            continue;
        }
        match adapter.retrieve(request) {
            Ok(candidates) => {
                if candidates.iter().any(|candidate| {
                    candidate.result.fragment.scope != request.input().scope
                        || candidate.result.source != source.id
                        || candidate.result.strategy != strategy.id
                }) {
                    output.failures.insert((
                        source.id.clone(),
                        strategy.id.clone(),
                        RetrievalError::InvalidResult,
                    ));
                } else {
                    output.candidates.extend(candidates);
                }
            }
            Err(error) => {
                output
                    .failures
                    .insert((source.id.clone(), strategy.id.clone(), error));
            }
        }
    }
    output.candidates.sort_by(|a, b| {
        a.result
            .source
            .cmp(&b.result.source)
            .then_with(|| a.result.strategy.cmp(&b.result.strategy))
            .then_with(|| a.result.fragment.id.cmp(&b.result.fragment.id))
    });
    output
}

/// Model rerankers return ordering signals only. Candidate identity and the
/// attached result remain owned by the deterministic pipeline.
pub trait RetrievalReranker {
    fn model(&self) -> &str;
    fn version(&self) -> &str;
    fn rank(
        &self,
        candidates: &[HybridCandidate],
    ) -> Result<Vec<(ReferenceId, u32)>, RetrievalError>;
}

/// Apply reranker ordering only when every candidate has exactly one bounded
/// score. Any malformed or unavailable output preserves the deterministic order.
pub fn rerank(candidates: Vec<HybridCandidate>, reranker: &dyn RetrievalReranker) -> RerankOutcome {
    let baseline = |candidates: Vec<HybridCandidate>| RerankOutcome {
        ranked: candidates
            .into_iter()
            .map(|candidate| RankedCandidate {
                score: candidate.result.score as u32,
                candidate,
                score_origin: ScoreOrigin::Fusion,
            })
            .collect(),
        failure: Some(RetrievalError::ServiceUnavailable),
    };
    let lineage = NonEmptyText::new(reranker.model().to_owned()).and_then(|model| {
        NonEmptyText::new(reranker.version().to_owned()).map(|version| (model, version))
    });
    let Ok(lineage) = lineage else {
        return baseline(candidates);
    };
    let Ok(scores) = reranker.rank(&candidates) else {
        return baseline(candidates);
    };
    if scores.len() != candidates.len() {
        return baseline(candidates);
    }
    let scores: BTreeMap<_, _> = scores.into_iter().collect();
    if scores.len() != candidates.len()
        || scores.values().any(|score| *score > 1_000_000)
        || candidates
            .iter()
            .any(|candidate| !scores.contains_key(&candidate.result.fragment.id))
    {
        return baseline(candidates);
    }
    let mut ranked = candidates;
    ranked.sort_by(|a, b| {
        b.exact_match
            .cmp(&a.exact_match)
            .then_with(|| scores[&b.result.fragment.id].cmp(&scores[&a.result.fragment.id]))
            .then_with(|| b.result.score.cmp(&a.result.score))
            .then_with(|| a.result.fragment.id.cmp(&b.result.fragment.id))
    });
    RerankOutcome {
        ranked: ranked
            .into_iter()
            .map(|candidate| RankedCandidate {
                score: scores[&candidate.result.fragment.id],
                candidate,
                score_origin: ScoreOrigin::Model {
                    id: lineage.0.clone(),
                    version: lineage.1.clone(),
                },
            })
            .collect(),
        failure: None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoreOrigin {
    Fusion,
    Model {
        id: NonEmptyText,
        version: NonEmptyText,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedCandidate {
    pub candidate: HybridCandidate,
    pub score: u32,
    pub score_origin: ScoreOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RerankOutcome {
    pub ranked: Vec<RankedCandidate>,
    pub failure: Option<RetrievalError>,
}

fn canonical_content(content: &str) -> String {
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn max_opt(left: Option<u32>, right: Option<u32>) -> Option<u32> {
    match (left, right) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests;
