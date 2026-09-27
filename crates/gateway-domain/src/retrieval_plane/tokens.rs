use super::{RetrievalError, RetrievedFragment, TokenEstimatorId, TokenEstimatorVersion};
use crate::{ContextScopeId, NonEmptyText};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenEstimateRequest {
    pub scope: ContextScopeId,
    pub fragments: Vec<RetrievedFragment>,
    pub target: Option<NonEmptyText>,
}
impl TokenEstimateRequest {
    pub fn validate(&self) -> Result<(), RetrievalError> {
        if self.fragments.iter().any(|f| f.scope != self.scope) {
            return Err(RetrievalError::ScopeMismatch);
        }
        Ok(())
    }
}
/// Exact counts are tied to an explicit target and tokenizer version. Unknown is never zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenCount {
    Exact {
        tokens: u64,
        target: NonEmptyText,
    },
    Estimated {
        tokens: u64,
        upper_bound: Option<u64>,
        semantics: NonEmptyText,
    },
    Unknown {
        reason: NonEmptyText,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenEstimate {
    pub estimator: TokenEstimatorId,
    pub version: TokenEstimatorVersion,
    pub count: TokenCount,
}
impl TokenEstimate {
    /// A hard budget can use exact counts or explicit conservative upper bounds only.
    pub fn budget_bound(&self) -> Result<u64, RetrievalError> {
        match &self.count {
            TokenCount::Exact { tokens, .. } => Ok(*tokens),
            TokenCount::Estimated {
                tokens,
                upper_bound: Some(bound),
                ..
            } if bound >= tokens => Ok(*bound),
            _ => Err(RetrievalError::InvalidEstimate),
        }
    }
}
