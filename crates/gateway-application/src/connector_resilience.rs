//! Deterministic connector recovery decisions. Metadata describes provider
//! support; it never grants policy authority or proves an external effect.
use crate::connectors::{ConnectorEffect, ConnectorFailure, ConnectorFailureCode as Code};
use gateway_domain::{CapabilityId, NonEmptyText};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProviderIdempotency {
    #[default]
    Unsupported,
    Idempotent,
    /// The configured argument must carry the same provider key on every attempt.
    Keyed {
        argument: NonEmptyText,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ReadBackSupport {
    #[default]
    Unsupported,
    /// A separately admitted, budgeted read can reconcile an uncertain effect.
    /// Availability alone does not establish completion or permit replay.
    Capability(CapabilityId),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RetrySafety {
    #[default]
    Never,
    ReadOnly,
    Idempotent,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecoverySupport {
    pub retry: RetrySafety,
    pub idempotency: ProviderIdempotency,
    pub read_back: ReadBackSupport,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryDecision {
    Stop(Code),
    RetryAfter(Duration),
    Reconcile(ReadBackSupport),
}

/// Maximum attempts includes the original call. Backoff is exponential and
/// saturating; provider delays are lower bounds and are never truncated early.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    max_attempts: u32,
    initial: Duration,
    maximum: Duration,
}
impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial: Duration::from_millis(50),
            maximum: Duration::from_secs(2),
        }
    }
}
impl RetryPolicy {
    pub fn new(max_attempts: u32, initial: Duration, maximum: Duration) -> Result<Self, Code> {
        if !(1..=32).contains(&max_attempts)
            || initial.is_zero()
            || initial > maximum
            || maximum > Duration::from_secs(60)
        {
            return Err(Code::BudgetExhausted);
        }
        Ok(Self {
            max_attempts,
            initial,
            maximum,
        })
    }
    pub fn decide(
        &self,
        failure: &ConnectorFailure,
        support: &RecoverySupport,
        attempts: u32,
        remaining: Duration,
        retry_after: Option<Duration>,
    ) -> RetryDecision {
        if failure.effect == ConnectorEffect::Unknown || failure.code == Code::MutationUncertain {
            return RetryDecision::Reconcile(support.read_back.clone());
        }
        if failure.effect == ConnectorEffect::Acknowledged
            || support.retry == RetrySafety::Never
            || (support.retry == RetrySafety::Idempotent
                && support.idempotency == ProviderIdempotency::Unsupported)
        {
            return RetryDecision::Stop(Code::RetryUnsafe);
        }
        if attempts == 0 || attempts >= self.max_attempts || remaining.is_zero() {
            return RetryDecision::Stop(Code::BudgetExhausted);
        }
        if !matches!(
            failure.code,
            Code::Unavailable | Code::Timeout | Code::RateLimited | Code::Backpressure
        ) {
            return RetryDecision::Stop(failure.code);
        }
        let exponent = (attempts - 1).min(31);
        let delay = self
            .initial
            .saturating_mul(1_u32 << exponent)
            .min(self.maximum)
            .max(retry_after.unwrap_or_default());
        if delay >= remaining {
            RetryDecision::Stop(Code::BudgetExhausted)
        } else {
            RetryDecision::RetryAfter(delay)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorHealth {
    Healthy,
    Degraded,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}
/// Content-free bounded counters; observation never performs provider I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthSnapshot {
    pub health: ConnectorHealth,
    pub circuit: CircuitState,
    pub in_flight: bool,
    pub attempts: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub rejected: u64,
    pub consecutive_failures: u32,
    pub retry_after: Duration,
}

/// Provider-independent recovery metadata for CG-14/session goal consumers.
/// A read-back capability must still use the ordinary admitted connector port,
/// preserve the original effect binding and acquire independent evidence.
pub trait ConnectorRecoveryPort: Send + Sync {
    fn recovery_support(
        &self,
        request: &crate::connectors::ConnectorRequest,
        source: Option<&gateway_domain::SourceId>,
    ) -> Result<RecoverySupport, ConnectorFailure>;
}
