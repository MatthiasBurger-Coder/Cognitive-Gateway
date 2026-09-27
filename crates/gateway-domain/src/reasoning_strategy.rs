//! Versioned, provider independent attempt strategy. This contract describes
//! one already authorized step; it cannot grant tools or change its scope.
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;

pub const REASONING_STRATEGY_VERSION: &str = "1.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReasoningStrategy {
    Direct,
    RetrievalAssisted,
    PlanExecute,
    Verify,
    MultiPass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReasoningCapability {
    Retrieval,
    Planning,
    Verification,
    MultiplePasses,
}

impl ReasoningStrategy {
    pub fn required_capability(self) -> Option<ReasoningCapability> {
        match self {
            Self::Direct => None,
            Self::RetrievalAssisted => Some(ReasoningCapability::Retrieval),
            Self::PlanExecute => Some(ReasoningCapability::Planning),
            Self::Verify => Some(ReasoningCapability::Verification),
            Self::MultiPass => Some(ReasoningCapability::MultiplePasses),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerificationExpectation {
    None,
    EvidenceBacked,
}

/// All counts are aggregate limits for the step, including retries and fallback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyBudget {
    pub iterations: u64,
    pub retrieval_rounds: u64,
    pub cost: u64,
    pub cost_unit: String,
    pub latency_ms: u64,
    pub tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyUsage {
    pub iterations: u64,
    pub retrieval_rounds: u64,
    pub cost: u64,
    pub latency_ms: u64,
    pub tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrategyError {
    UnsupportedVersion,
    InvalidBudget,
    InvalidCapabilities,
    InvalidFallback,
    UnsupportedStrategy,
    IncompatibleFallback,
    BudgetExceeded,
    ArithmeticOverflow,
    InsufficientEvidence,
    ContextIncomplete,
}

impl StrategyUsage {
    pub fn validate(&self, budget: &StrategyBudget) -> Result<(), StrategyError> {
        if self.iterations > budget.iterations
            || self.retrieval_rounds > budget.retrieval_rounds
            || self.cost > budget.cost
            || self.latency_ms > budget.latency_ms
            || self.tokens > budget.tokens
        {
            return Err(StrategyError::BudgetExceeded);
        }
        Ok(())
    }

    pub fn checked_add(
        &self,
        delta: &Self,
        budget: &StrategyBudget,
    ) -> Result<Self, StrategyError> {
        let add = |a: u64, b: u64| a.checked_add(b).ok_or(StrategyError::ArithmeticOverflow);
        let next = Self {
            iterations: add(self.iterations, delta.iterations)?,
            retrieval_rounds: add(self.retrieval_rounds, delta.retrieval_rounds)?,
            cost: add(self.cost, delta.cost)?,
            latency_ms: add(self.latency_ms, delta.latency_ms)?,
            tokens: add(self.tokens, delta.tokens)?,
        };
        next.validate(budget)?;
        Ok(next)
    }
}

/// Fallback records a deliberate semantic degradation. Output and verification
/// remain fixed by the enclosing contract and cannot be rewritten by an adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyFallback {
    pub strategy: ReasoningStrategy,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReasoningStrategyContract {
    version: String,
    strategy: ReasoningStrategy,
    required_capabilities: BTreeSet<ReasoningCapability>,
    output_contract: String,
    verification: VerificationExpectation,
    budget: StrategyBudget,
    fallback: Option<StrategyFallback>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrategyWire {
    version: String,
    strategy: ReasoningStrategy,
    required_capabilities: BTreeSet<ReasoningCapability>,
    output_contract: String,
    verification: VerificationExpectation,
    budget: StrategyBudget,
    fallback: Option<StrategyFallback>,
}

impl<'de> Deserialize<'de> for ReasoningStrategyContract {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = StrategyWire::deserialize(deserializer)?;
        Self::new(
            wire.strategy,
            wire.required_capabilities,
            wire.output_contract,
            wire.verification,
            wire.budget,
            wire.fallback,
            &wire.version,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for StrategyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl ReasoningStrategyContract {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        strategy: ReasoningStrategy,
        required_capabilities: BTreeSet<ReasoningCapability>,
        output_contract: String,
        verification: VerificationExpectation,
        budget: StrategyBudget,
        fallback: Option<StrategyFallback>,
        version: &str,
    ) -> Result<Self, StrategyError> {
        if version != REASONING_STRATEGY_VERSION {
            return Err(StrategyError::UnsupportedVersion);
        }
        if budget.iterations == 0
            || budget.latency_ms == 0
            || budget.tokens == 0
            || crate::ReferenceId::new(budget.cost_unit.clone()).is_err()
        {
            return Err(StrategyError::InvalidBudget);
        }
        if output_contract.trim().is_empty() {
            return Err(StrategyError::InvalidCapabilities);
        }
        if strategy == ReasoningStrategy::RetrievalAssisted && budget.retrieval_rounds == 0 {
            return Err(StrategyError::InvalidBudget);
        }
        if let Some(capability) = strategy.required_capability()
            && !required_capabilities.contains(&capability)
        {
            return Err(StrategyError::InvalidCapabilities);
        }
        if strategy == ReasoningStrategy::Verify
            && verification != VerificationExpectation::EvidenceBacked
        {
            return Err(StrategyError::InvalidCapabilities);
        }
        if fallback
            .as_ref()
            .is_some_and(|f| f.strategy == strategy || f.reason.trim().is_empty())
        {
            return Err(StrategyError::InvalidFallback);
        }
        Ok(Self {
            version: version.to_owned(),
            strategy,
            required_capabilities,
            output_contract,
            verification,
            budget,
            fallback,
        })
    }

    pub fn strategy(&self) -> ReasoningStrategy {
        self.strategy
    }
    pub fn budget(&self) -> &StrategyBudget {
        &self.budget
    }
    pub fn verification(&self) -> VerificationExpectation {
        self.verification
    }
    pub fn output_contract(&self) -> &str {
        &self.output_contract
    }
    pub fn required_capabilities(&self) -> &BTreeSet<ReasoningCapability> {
        &self.required_capabilities
    }

    pub fn select(
        &self,
        supported: &BTreeSet<ReasoningStrategy>,
        capabilities: &BTreeSet<ReasoningCapability>,
    ) -> Result<StrategySelection, StrategyError> {
        if supported.contains(&self.strategy) && self.required_capabilities.is_subset(capabilities)
        {
            return Ok(StrategySelection {
                requested: self.strategy,
                selected: self.strategy,
                fallback_reason: None,
            });
        }
        let fallback = self
            .fallback
            .as_ref()
            .ok_or(StrategyError::UnsupportedStrategy)?;
        let intrinsic = fallback.strategy.required_capability();
        // A fallback may drop only the requested strategy's own mechanism.
        let remaining: BTreeSet<_> = self
            .required_capabilities
            .iter()
            .copied()
            .filter(|capability| Some(*capability) != self.strategy.required_capability())
            .collect();
        if !supported.contains(&fallback.strategy)
            || !remaining.is_subset(capabilities)
            || intrinsic.is_some_and(|capability| !capabilities.contains(&capability))
            || (fallback.strategy == ReasoningStrategy::RetrievalAssisted
                && self.budget.retrieval_rounds == 0)
            || (fallback.strategy == ReasoningStrategy::Verify
                && self.verification != VerificationExpectation::EvidenceBacked)
        {
            return Err(StrategyError::IncompatibleFallback);
        }
        Ok(StrategySelection {
            requested: self.strategy,
            selected: fallback.strategy,
            fallback_reason: Some(fallback.reason.clone()),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StrategySelection {
    pub requested: ReasoningStrategy,
    pub selected: ReasoningStrategy,
    pub fallback_reason: Option<String>,
}
