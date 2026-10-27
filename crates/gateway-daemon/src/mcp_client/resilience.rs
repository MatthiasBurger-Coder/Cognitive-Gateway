//! Host-local availability state. One in-flight operation and no waiting queue.
use super::*;
use gateway_application::connector_resilience::*;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct ResilienceConfig {
    pub failure_threshold: u32,
    pub open_for: Duration,
}
impl Default for ResilienceConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            open_for: Duration::from_secs(30),
        }
    }
}
impl ResilienceConfig {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.failure_threshold == 0
            || self.failure_threshold > 1024
            || self.open_for.is_zero()
            || self.open_for > Duration::from_secs(3600)
        {
            Err(Error::Configuration)
        } else {
            Ok(())
        }
    }
}
struct State {
    snapshot: HealthSnapshot,
    open_until: Option<Instant>,
    rate_until: Option<Instant>,
}
pub(super) struct Resilience {
    config: ResilienceConfig,
    state: Mutex<State>,
    observer: Arc<dyn Observer>,
}
impl Resilience {
    pub fn new(config: ResilienceConfig, observer: Arc<dyn Observer>) -> Self {
        Self {
            config,
            observer,
            state: Mutex::new(State {
                snapshot: HealthSnapshot {
                    health: ConnectorHealth::Healthy,
                    circuit: CircuitState::Closed,
                    in_flight: false,
                    attempts: 0,
                    succeeded: 0,
                    failed: 0,
                    rejected: 0,
                    consecutive_failures: 0,
                    retry_after: Duration::ZERO,
                },
                open_until: None,
                rate_until: None,
            }),
        }
    }
    pub fn snapshot(&self) -> HealthSnapshot {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut snapshot = state.snapshot;
        snapshot.retry_after = state
            .rate_until
            .into_iter()
            .chain(state.open_until)
            .map(|until| until.saturating_duration_since(Instant::now()))
            .max()
            .unwrap_or_default();
        snapshot
    }
    pub fn rate_limited(&self, delay: Duration) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let until = Instant::now() + delay;
        state.rate_until = Some(state.rate_until.map_or(until, |old| old.max(until)));
    }
    fn begin(&self, context: &CallContext) -> Result<Permit<'_>, ConnectorFailure> {
        context
            .check()
            .map_err(|e| e.failure(&context.correlation, ConnectorEffect::NotDispatched))?;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let code = if state.snapshot.in_flight {
            Some(ConnectorFailureCode::Backpressure)
        } else if state.open_until.is_some_and(|until| now < until) {
            Some(ConnectorFailureCode::Unavailable)
        } else if state.rate_until.is_some_and(|until| now < until) {
            Some(ConnectorFailureCode::RateLimited)
        } else {
            None
        };
        if let Some(code) = code {
            state.snapshot.rejected = state.snapshot.rejected.saturating_add(1);
            return Err(ConnectorFailure {
                code,
                effect: ConnectorEffect::NotDispatched,
                correlation: context.correlation.clone(),
            });
        }
        if state.open_until.is_some() {
            state.snapshot.circuit = CircuitState::HalfOpen;
        }
        state.snapshot.in_flight = true;
        state.snapshot.attempts = state.snapshot.attempts.saturating_add(1);
        Ok(Permit {
            runtime: self,
            finished: false,
        })
    }
    pub fn run<T>(
        &self,
        context: &CallContext,
        operation: impl FnOnce() -> Result<T, ConnectorFailure>,
    ) -> Result<T, ConnectorFailure> {
        let permit = match self.begin(context) {
            Ok(permit) => permit,
            Err(failure) => {
                self.event(context);
                return Err(failure);
            }
        };
        self.event(context);
        let result = operation();
        permit.finish(result.as_ref().err().map(|e| e.code));
        self.event(context);
        result
    }
    fn event(&self, context: &CallContext) {
        self.observer.record(Event {
            kind: EventKind::Health(self.snapshot()),
            request_id: None,
            correlation: context.correlation.clone(),
        });
    }
    fn complete(&self, failure: Option<ConnectorFailureCode>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.snapshot.in_flight = false;
        if let Some(code) = failure {
            state.snapshot.failed = state.snapshot.failed.saturating_add(1);
            if matches!(
                code,
                ConnectorFailureCode::Unavailable
                    | ConnectorFailureCode::Timeout
                    | ConnectorFailureCode::ProtocolError
                    | ConnectorFailureCode::SchemaMismatch
                    | ConnectorFailureCode::IncompatibleVersion
                    | ConnectorFailureCode::RateLimited
                    | ConnectorFailureCode::MutationUncertain
            ) {
                state.snapshot.health = ConnectorHealth::Degraded;
                state.snapshot.consecutive_failures =
                    state.snapshot.consecutive_failures.saturating_add(1);
                if state.snapshot.circuit == CircuitState::HalfOpen
                    || state.snapshot.consecutive_failures >= self.config.failure_threshold
                {
                    state.snapshot.circuit = CircuitState::Open;
                    state.snapshot.health = ConnectorHealth::Unavailable;
                    state.open_until = Some(Instant::now() + self.config.open_for);
                }
            } else if state.snapshot.circuit == CircuitState::HalfOpen {
                // A denied/cancelled probe establishes no provider recovery.
                state.snapshot.circuit = CircuitState::Open;
                state.open_until = Some(Instant::now() + self.config.open_for);
            }
        } else {
            state.snapshot.succeeded = state.snapshot.succeeded.saturating_add(1);
            state.snapshot.consecutive_failures = 0;
            state.snapshot.health = ConnectorHealth::Healthy;
            state.snapshot.circuit = CircuitState::Closed;
            state.open_until = None;
            state.rate_until = None;
        }
    }
}
struct Permit<'a> {
    runtime: &'a Resilience,
    finished: bool,
}
impl Permit<'_> {
    fn finish(mut self, failure: Option<ConnectorFailureCode>) {
        self.runtime.complete(failure);
        self.finished = true;
    }
}
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.runtime
                .complete(Some(ConnectorFailureCode::Unavailable));
        }
    }
}

#[cfg(test)]
#[path = "resilience_tests.rs"]
mod tests;
