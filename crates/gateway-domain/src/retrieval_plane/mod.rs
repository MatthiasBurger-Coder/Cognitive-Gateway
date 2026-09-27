//! CG-15 provider-independent, advisory retrieval IR v1.
//!
//! Executable plans cannot be changed after validation:
//! ```compile_fail
//! use gateway_domain::{RetrievalPlan, TokenBudget};
//! fn overwrite(plan: &mut RetrievalPlan) {
//!     plan.request().input().budget.tokens = TokenBudget(u64::MAX);
//! }
//! ```
//! Retrieval cannot carry a capability grant:
//! ```compile_fail
//! use gateway_domain::RetrievedFragment;
//! fn grant(fragment: &mut RetrievedFragment) {
//!     fragment.capability_grants = vec![];
//! }
//! ```
//! Validated embedding vectors cannot be resized or replaced:
//! ```compile_fail
//! use gateway_domain::EmbeddingResult;
//! fn corrupt(result: &mut EmbeddingResult) {
//!     result.vectors()[0].values.clear();
//! }
//! ```
//! Negative budgets have no representation:
//! ```compile_fail
//! use gateway_domain::TokenBudget;
//! let budget = TokenBudget(-1);
//! ```
mod budget;
mod embedding;
mod plan;
mod result;
mod tokens;

pub use budget::*;
pub use embedding::*;
pub use plan::*;
pub use result::*;
pub use tokens::*;

use crate::{NonEmptyText, ValidationError};

/// Stable failure codes. These are information-processing failures, never policy decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalError {
    UnsupportedVersion,
    UnsupportedSource,
    UnsupportedStrategy,
    InvalidBudget,
    BudgetExceeded,
    ArithmeticOverflow,
    MissingStopCondition,
    InvalidPlan,
    DuplicateIdentity,
    ScopeMismatch,
    InvalidResult,
    IncompatibleEmbedding,
    InvalidEstimate,
    ServiceUnavailable,
}

macro_rules! identity {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonEmptyText);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
                Ok(Self(NonEmptyText::new(value)?))
            }
            pub fn as_str(&self) -> &str { self.0.as_str() }
        }
    )+};
}
identity!(
    RetrievalPlanId,
    RetrievalSourceId,
    RetrievalStrategyId,
    EmbeddingModelId,
    EmbeddingModelVersion,
    TokenEstimatorId,
    TokenEstimatorVersion
);

/// A version is validated at every aggregate boundary; unknown versions never fall back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetrievalVersion(u16);
impl RetrievalVersion {
    pub const V1: Self = Self(1);
    pub fn new(value: u16) -> Result<Self, RetrievalError> {
        if value != 1 {
            return Err(RetrievalError::UnsupportedVersion);
        }
        Ok(Self(value))
    }
    pub const fn number(self) -> u16 {
        self.0
    }
}
