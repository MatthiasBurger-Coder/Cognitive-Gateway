use super::RetrievalError;
use crate::NonEmptyText;
use std::{collections::BTreeMap, num::NonZeroU64};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultBudget(pub NonZeroU64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundBudget(pub NonZeroU64);
/// End-to-end elapsed milliseconds, including service calls and retries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatencyBudget(pub NonZeroU64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenBudget(pub u64);
/// Integer units with an explicit accounting definition (for example microcredits).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostBudget {
    pub maximum: u64,
    pub unit: NonEmptyText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextBudgetClass {
    AuthorityReserved,
    TaskReserved,
    OutputContractReserved,
    Evidence,
    Knowledge,
    Memory,
    RuntimeState,
    SafetyMargin,
}

/// Fixed disjoint reservations. Unallocated tokens are unavailable; no borrowing is implicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBudget {
    total: TokenBudget,
    reservations: BTreeMap<ContextBudgetClass, TokenBudget>,
}
impl ContextBudget {
    pub fn new(
        total: TokenBudget,
        reservations: BTreeMap<ContextBudgetClass, TokenBudget>,
    ) -> Result<Self, RetrievalError> {
        let sum = reservations.values().try_fold(0u64, |sum, value| {
            sum.checked_add(value.0)
                .ok_or(RetrievalError::ArithmeticOverflow)
        })?;
        if sum > total.0 {
            return Err(RetrievalError::InvalidBudget);
        }
        Ok(Self {
            total,
            reservations,
        })
    }
    pub fn total(&self) -> TokenBudget {
        self.total
    }
    pub fn reservations(&self) -> &BTreeMap<ContextBudgetClass, TokenBudget> {
        &self.reservations
    }
    pub fn validate_usage(
        &self,
        usage: &BTreeMap<ContextBudgetClass, u64>,
    ) -> Result<(), RetrievalError> {
        for (class, used) in usage {
            if *used > self.reservations.get(class).map_or(0, |v| v.0) {
                return Err(RetrievalError::BudgetExceeded);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalBudget {
    pub results: ResultBudget,
    pub rounds: RoundBudget,
    pub latency: LatencyBudget,
    pub cost: CostBudget,
    /// Cumulative retrieval tokens, including repeated input/output and embedding work.
    pub tokens: TokenBudget,
    pub context: ContextBudget,
}

/// Cumulative usage, never a signed delta. Context counts refer to retained context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetUsage {
    pub results: u64,
    pub rounds: u64,
    pub elapsed_ms: u64,
    pub cost: u64,
    pub cost_unit: NonEmptyText,
    pub tokens: u64,
    pub context: BTreeMap<ContextBudgetClass, u64>,
}
impl BudgetUsage {
    pub fn validate(&self, budget: &RetrievalBudget) -> Result<(), RetrievalError> {
        if self.cost_unit != budget.cost.unit {
            return Err(RetrievalError::InvalidBudget);
        }
        if self.results > budget.results.0.get()
            || self.rounds > budget.rounds.0.get()
            || self.elapsed_ms > budget.latency.0.get()
            || self.cost > budget.cost.maximum
            || self.tokens > budget.tokens.0
        {
            return Err(RetrievalError::BudgetExceeded);
        }
        budget.context.validate_usage(&self.context)
    }
    /// Checked cumulative accounting. Retained context is supplied as the new snapshot.
    pub fn checked_add(&self, delta: &Self) -> Result<Self, RetrievalError> {
        if self.cost_unit != delta.cost_unit {
            return Err(RetrievalError::InvalidBudget);
        }
        let add = |a: u64, b: u64| a.checked_add(b).ok_or(RetrievalError::ArithmeticOverflow);
        Ok(Self {
            results: add(self.results, delta.results)?,
            rounds: add(self.rounds, delta.rounds)?,
            elapsed_ms: add(self.elapsed_ms, delta.elapsed_ms)?,
            cost: add(self.cost, delta.cost)?,
            cost_unit: self.cost_unit.clone(),
            tokens: add(self.tokens, delta.tokens)?,
            context: delta.context.clone(),
        })
    }
}
