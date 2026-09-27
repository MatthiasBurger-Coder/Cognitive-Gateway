use super::*;
use crate::{
    ContextScopeId, EvidenceId, InformationRequirements, NonEmptyText, PlanStepId, ProvenanceId,
    SensitivityClass, SituationId, TrustClass,
};
use std::{collections::BTreeSet, num::NonZeroU64};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalPurpose {
    ArchitectureEvidence,
    TaskKnowledge,
    MemoryRecall,
    StateObservation,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RetrievalQuery(pub NonEmptyText);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredInformation {
    pub description: NonEmptyText,
    pub requirements: InformationRequirements,
    /// Explicit membership; TrustClass is not a numeric trust ranking.
    pub accepted_trust: BTreeSet<TrustClass>,
    pub maximum_sensitivity: SensitivityClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RetrievalSourceKind {
    Document,
    VectorIndex,
    Graph,
    Memory,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RetrievalStrategyKind {
    Lexical,
    Semantic,
    GraphTraversal,
    MemoryRecall,
}

/// Lower priority numbers run first, with ID as the tie-break. IDs identify implementations.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RetrievalSource {
    pub priority: u32,
    pub id: RetrievalSourceId,
    pub kind: RetrievalSourceKind,
    pub optional: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RetrievalStrategy {
    pub priority: u32,
    pub id: RetrievalStrategyId,
    pub kind: RetrievalStrategyKind,
    pub optional: bool,
}
/// Explicit support is negotiated before execution, never used to rewrite a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalSupport {
    pub sources: BTreeSet<(RetrievalSourceId, RetrievalSourceKind)>,
    pub strategies: BTreeSet<(RetrievalStrategyId, RetrievalStrategyKind)>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopCondition {
    BudgetExhausted,
    /// Count of distinct accepted evidence IDs; every hard budget still applies.
    EvidenceSatisfied(NonZeroU64),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RetrievalRound(pub NonZeroU64);

/// Mutable input only. Convert to RetrievalRequest before planning or execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalRequestInput {
    pub version: RetrievalVersion,
    pub scope: ContextScopeId,
    pub provenance: ProvenanceId,
    pub situation: Option<SituationId>,
    pub step: Option<PlanStepId>,
    pub purpose: RetrievalPurpose,
    pub required: RequiredInformation,
    pub queries: BTreeSet<RetrievalQuery>,
    pub sources: Vec<RetrievalSource>,
    pub strategies: Vec<RetrievalStrategy>,
    pub budget: RetrievalBudget,
    pub stop: Option<StopCondition>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalRequest(RetrievalRequestInput);
impl RetrievalRequest {
    pub fn new(mut input: RetrievalRequestInput) -> Result<Self, RetrievalError> {
        if input.stop.is_none() {
            return Err(RetrievalError::MissingStopCondition);
        }
        if input.queries.is_empty()
            || input.sources.is_empty()
            || input.strategies.is_empty()
            || input.required.accepted_trust.is_empty()
        {
            return Err(RetrievalError::InvalidPlan);
        }
        let sources: BTreeSet<_> = input.sources.iter().map(|v| &v.id).collect();
        let strategies: BTreeSet<_> = input.strategies.iter().map(|v| &v.id).collect();
        if sources.len() != input.sources.len() || strategies.len() != input.strategies.len() {
            return Err(RetrievalError::DuplicateIdentity);
        }
        input.sources.sort();
        input.strategies.sort();
        Ok(Self(input))
    }
    pub fn input(&self) -> &RetrievalRequestInput {
        &self.0
    }
}

/// An executable immutable plan. Construction does not inspect provider availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalPlan {
    id: RetrievalPlanId,
    request: RetrievalRequest,
    explanations: BTreeSet<RetrievalExplanation>,
}
impl RetrievalPlan {
    pub fn new(
        id: RetrievalPlanId,
        request: RetrievalRequest,
        support: &RetrievalSupport,
    ) -> Result<Self, RetrievalError> {
        for source in &request.0.sources {
            if !support.sources.contains(&(source.id.clone(), source.kind)) {
                return Err(RetrievalError::UnsupportedSource);
            }
        }
        for strategy in &request.0.strategies {
            if !support
                .strategies
                .contains(&(strategy.id.clone(), strategy.kind))
            {
                return Err(RetrievalError::UnsupportedStrategy);
            }
        }
        let explanations = request
            .0
            .sources
            .iter()
            .map(|source| RetrievalExplanation {
                target: RetrievalExplanationTarget::Source(source.id.clone()),
                selected: true,
                reason: RetrievalReason::ExplicitSelection,
                detail: request.0.required.description.clone(),
            })
            .chain(
                request
                    .0
                    .strategies
                    .iter()
                    .map(|strategy| RetrievalExplanation {
                        target: RetrievalExplanationTarget::Strategy(strategy.id.clone()),
                        selected: true,
                        reason: RetrievalReason::ExplicitSelection,
                        detail: request.0.required.description.clone(),
                    }),
            )
            .collect();
        Ok(Self {
            id,
            request,
            explanations,
        })
    }
    pub fn explanations(&self) -> &BTreeSet<RetrievalExplanation> {
        &self.explanations
    }
    pub fn id(&self) -> &RetrievalPlanId {
        &self.id
    }
    pub fn request(&self) -> &RetrievalRequest {
        &self.request
    }
    pub fn version(&self) -> RetrievalVersion {
        self.request.0.version
    }
    /// Stop before starting work that would exceed a limit. Unknown costs/token bounds
    /// require an explicit failure; callers must reserve an upper bound before dispatch.
    pub fn should_stop(
        &self,
        usage: &BudgetUsage,
        evidence: &BTreeSet<EvidenceId>,
    ) -> Result<bool, RetrievalError> {
        let input = self.request.input();
        usage.validate(&input.budget)?;
        Ok(usage.results == input.budget.results.0.get()
            || usage.rounds == input.budget.rounds.0.get()
            || usage.elapsed_ms == input.budget.latency.0.get()
            || usage.cost == input.budget.cost.maximum
            || usage.tokens == input.budget.tokens.0
            || matches!(input.stop, Some(StopCondition::EvidenceSatisfied(n)) if evidence.len() as u64 >= n.get()))
    }
}

impl TryFrom<&str> for RetrievalSourceKind {
    type Error = RetrievalError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "DOCUMENT" => Ok(Self::Document),
            "VECTOR_INDEX" => Ok(Self::VectorIndex),
            "GRAPH" => Ok(Self::Graph),
            "MEMORY" => Ok(Self::Memory),
            _ => Err(RetrievalError::UnsupportedSource),
        }
    }
}
impl TryFrom<&str> for RetrievalStrategyKind {
    type Error = RetrievalError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "LEXICAL" => Ok(Self::Lexical),
            "SEMANTIC" => Ok(Self::Semantic),
            "GRAPH_TRAVERSAL" => Ok(Self::GraphTraversal),
            "MEMORY_RECALL" => Ok(Self::MemoryRecall),
            _ => Err(RetrievalError::UnsupportedStrategy),
        }
    }
}
