//! Scoped connector operations charged to one coordinator-minted phase slice.
//! Quotes and allocation metadata are never effect grants: current session
//! authority and the existing CG-09 host independently revalidate each call.

use super::{CallContext, Error, McpConnectorHost};
use gateway_application::{
    connectors::*,
    sessions::{
        SessionError, SessionResourceAllocation, SessionResourceAmounts,
        SessionResourceAuthorityPort,
    },
};
use gateway_domain::{NonEmptyText, ReferenceId, SourceId};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

/// A trusted hard ceiling for one logical connector application operation.
/// Discovery includes connection bootstrap, protocol negotiation and bounded
/// pagination. Wire frames remain independently bounded by the MCP host limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConnectorQuote {
    pub amounts: SessionResourceAmounts,
    pub cost_unit: ReferenceId,
}

/// Operator-owned admission of hard resource bounds, never a model price or
/// an estimate promoted by its numeric size. These methods perform bounded
/// local/control-plane lookups only, with no connector/model/retrieval effects.
/// They authenticate the exact allocation and target/request/source and refuse
/// unknown bounds. Result validators verify the returned metrics against the
/// already-admitted bound; detecting excess afterwards is not an alternative
/// to establishing a true hard upper bound before dispatch.
pub trait SessionConnectorQuotePort: Send + Sync {
    fn quote_discovery(
        &self,
        allocation: &SessionResourceAllocation,
        target: &ConnectorTarget,
    ) -> Result<SessionConnectorQuote, SessionError>;
    fn quote_request(
        &self,
        allocation: &SessionResourceAllocation,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<SessionConnectorQuote, SessionError>;
    fn validate_inventory(
        &self,
        allocation: &SessionResourceAllocation,
        quote: &SessionConnectorQuote,
        inventory: &ConnectorInventory,
    ) -> Result<(), SessionError>;
    fn validate_observation(
        &self,
        allocation: &SessionResourceAllocation,
        quote: &SessionConnectorQuote,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
        observation: &ConnectorObservation,
    ) -> Result<(), SessionError>;
}

#[derive(Default)]
struct Usage {
    amounts: SessionResourceAmounts,
    retry_attempts: u64,
    terminal: Option<ConnectorFailure>,
}

/// A retained metadata view, never an owned spending allowance. A source may
/// keep this view after run drop, but cannot extract its Arc or mint a run.
#[derive(Clone)]
pub(crate) struct ReadonlyAllocationView {
    allocation: Arc<SessionResourceAllocation>,
}
impl ReadonlyAllocationView {
    pub(crate) fn allocation(&self) -> &SessionResourceAllocation {
        &self.allocation
    }
}

/// Explicit trusted implementation contract: both joined methods validate the
/// exact allocation and the complete fresh source authority themselves. There
/// is deliberately no blanket implementation or caller-supplied flag.
pub(crate) trait QualifiedConnectorAuthority: SessionResourceAuthorityPort {}
struct FullJoinProof;

pub(crate) struct SessionConnectorComponents {
    pure_quote: Option<super::scalar_quotes::PureQuoteProof>,
    full_join: Option<FullJoinProof>,
    host: Arc<McpConnectorHost>,
    authority: Arc<dyn SessionResourceAuthorityPort>,
    quotes: Arc<dyn SessionConnectorQuotePort>,
}

impl SessionConnectorComponents {
    /// The proof is minted only after the consuming qualified host constructor
    /// and a concrete explicitly qualified authority have been joined.
    pub(crate) fn from_qualified_joined<A: QualifiedConnectorAuthority + 'static>(
        bundle: super::scalar_quotes::QualifiedScalarHost,
        authority: Arc<A>,
    ) -> Self {
        let (host, quotes, pure_quote) = bundle.into_parts();
        Self {
            pure_quote: Some(pure_quote),
            full_join: Some(FullJoinProof),
            host,
            authority,
            quotes,
        }
    }
}

struct CombinedAuthority {
    full_join: Option<FullJoinProof>,
    base: Arc<dyn SessionResourceAuthorityPort>,
    child: Arc<dyn SessionResourceAuthorityPort>,
}
impl SessionResourceAuthorityPort for CombinedAuthority {
    fn validate_allocation(
        &self,
        allocation: &SessionResourceAllocation,
    ) -> Result<(), SessionError> {
        self.base.validate_allocation(allocation)?;
        self.child.validate_allocation(allocation)?;
        self.base.validate_allocation(allocation)
    }
    fn validate_connector_allocation(
        &self,
        allocation: &SessionResourceAllocation,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<(), SessionError> {
        if self.full_join.is_some() {
            self.base.validate_allocation(allocation)?;
        } else {
            self.validate_allocation(allocation)?;
        }
        self.child
            .validate_connector_allocation(allocation, request, source)?;
        self.base.validate_allocation(allocation)
    }
    fn validate_connector_discovery(
        &self,
        allocation: &SessionResourceAllocation,
        target: &ConnectorTarget,
    ) -> Result<(), SessionError> {
        if self.full_join.is_some() {
            self.base.validate_allocation(allocation)?;
        } else {
            self.validate_allocation(allocation)?;
        }
        self.child
            .validate_connector_discovery(allocation, target)?;
        self.base.validate_allocation(allocation)
    }
}

// Only with_factory can construct the joined variant, retaining the concrete
// authority that brackets every joined check with current base/child checks.
// Public new() authorities retain their independent allocation guards.
enum RunAuthority {
    Unpaired(Arc<dyn SessionResourceAuthorityPort>),
    Joined(Arc<CombinedAuthority>),
}
impl std::ops::Deref for RunAuthority {
    type Target = dyn SessionResourceAuthorityPort;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Unpaired(authority) => authority.as_ref(),
            Self::Joined(authority) => authority.as_ref(),
        }
    }
}
impl RunAuthority {
    fn validate_unpaired_allocation(
        &self,
        allocation: &SessionResourceAllocation,
    ) -> Result<(), SessionError> {
        match self {
            Self::Unpaired(authority) => authority.validate_allocation(allocation),
            Self::Joined(_) => Ok(()),
        }
    }
}

/// One non-cloneable allowance. Shared read-only references used by internal
/// context guards cannot reconstruct a cursor or create another run. Holding
/// the usage claim across callbacks prevents concurrent or reentrant spending.
/// Mutation uncertainty or a failed post-result accounting contract terminates
/// this run; later operations preserve that failure without spending or I/O.
pub struct SessionConnectorRun {
    pure_quote: Option<Arc<super::scalar_quotes::PureQuoteProof>>,
    host: Arc<McpConnectorHost>,
    allocation: Arc<SessionResourceAllocation>,
    authority: Arc<RunAuthority>,
    quotes: Arc<dyn SessionConnectorQuotePort>,
    context: CallContext,
    usage: Mutex<Usage>,
}

impl SessionConnectorRun {
    pub fn new(
        host: Arc<McpConnectorHost>,
        allocation: SessionResourceAllocation,
        authority: Arc<dyn SessionResourceAuthorityPort>,
        quotes: Arc<dyn SessionConnectorQuotePort>,
        mut context: CallContext,
    ) -> Result<Self, SessionError> {
        if has_non_connector_resources(allocation.amounts()) {
            return Err(SessionError::InvalidInput);
        }
        if host.configured_target().scope != allocation.binding().scope {
            return Err(SessionError::ScopeDenied);
        }
        if allocation.amounts().connector_calls == 0 {
            return Err(SessionError::LimitExceeded);
        }
        context.deadline = context.deadline.min(allocation.deadline());
        authority.validate_allocation(&allocation)?;
        context.check().map_err(|_| SessionError::AuthorityDenied)?;
        Ok(Self::from_validated_parts(
            host,
            Arc::new(allocation),
            RunAuthority::Unpaired(authority),
            quotes,
            context,
            None,
        ))
    }

    /// Trusted composition owns the one spending cursor; components see only
    /// its sealed metadata view. The callback may perform bounded read-only
    /// operator/control-plane lookups, but must not connect, discover, invoke a
    /// connector/model provider or perform other resource-consuming transport.
    pub(crate) fn with_factory(
        allocation: SessionResourceAllocation,
        base: Arc<dyn SessionResourceAuthorityPort>,
        mut context: CallContext,
        factory: impl FnOnce(ReadonlyAllocationView) -> Result<SessionConnectorComponents, SessionError>,
    ) -> Result<Self, SessionError> {
        if has_non_connector_resources(allocation.amounts()) {
            return Err(SessionError::InvalidInput);
        }
        if allocation.amounts().connector_calls == 0 {
            return Err(SessionError::LimitExceeded);
        }
        context.deadline = context.deadline.min(allocation.deadline());
        base.validate_allocation(&allocation)?;
        context.check().map_err(|_| SessionError::AuthorityDenied)?;
        let allocation = Arc::new(allocation);
        let components = factory(ReadonlyAllocationView {
            allocation: allocation.clone(),
        })?;
        if components.host.configured_target().scope != allocation.binding().scope {
            return Err(SessionError::ScopeDenied);
        }
        let authority = Arc::new(CombinedAuthority {
            full_join: components.full_join,
            base,
            child: components.authority,
        });
        authority.validate_allocation(&allocation)?;
        context.check().map_err(|_| SessionError::AuthorityDenied)?;
        Ok(Self::from_validated_parts(
            components.host,
            allocation,
            RunAuthority::Joined(authority),
            components.quotes,
            context,
            components.pure_quote,
        ))
    }

    fn from_validated_parts(
        host: Arc<McpConnectorHost>,
        allocation: Arc<SessionResourceAllocation>,
        authority: RunAuthority,
        quotes: Arc<dyn SessionConnectorQuotePort>,
        context: CallContext,
        pure_quote: Option<super::scalar_quotes::PureQuoteProof>,
    ) -> Self {
        Self {
            pure_quote: pure_quote.map(Arc::new),
            host,
            allocation,
            authority: Arc::new(authority),
            quotes,
            context,
            usage: Mutex::new(Usage::default()),
        }
    }

    pub fn usage(&self) -> Result<SessionResourceAmounts, SessionError> {
        self.usage
            .try_lock()
            .map(|usage| usage.amounts)
            .map_err(|_| SessionError::Unavailable)
    }

    /// Cancels every cloned per-operation context without creating a new token.
    pub fn cancel(&self) {
        self.context.cancel();
    }

    fn lock(&self, correlation: &NonEmptyText) -> Result<MutexGuard<'_, Usage>, ConnectorFailure> {
        self.usage.try_lock().map_err(|error| {
            failure(
                match error {
                    TryLockError::WouldBlock => ConnectorFailureCode::Backpressure,
                    TryLockError::Poisoned(_) => ConnectorFailureCode::Unavailable,
                },
                correlation,
                ConnectorEffect::NotDispatched,
            )
        })
    }

    fn context(&self, correlation: &NonEmptyText) -> CallContext {
        let mut context = self.context.clone();
        context.correlation = correlation.clone();
        context
    }

    fn reserve(
        &self,
        usage: &mut Usage,
        quote: &SessionConnectorQuote,
        readonly: bool,
        correlation: &NonEmptyText,
    ) -> Result<(), ConnectorFailure> {
        if quote.cost_unit != *self.allocation.cost_unit()
            || quote.amounts.connector_calls != 1
            || has_non_connector_resources(&quote.amounts)
            || (readonly && quote.amounts.retrieval_rounds == 0)
        {
            return Err(failure(
                ConnectorFailureCode::PermissionDenied,
                correlation,
                ConnectorEffect::NotDispatched,
            ));
        }
        let next = usage
            .amounts
            .checked_add(&quote.amounts)
            .map_err(|error| session_failure(error, correlation, ConnectorEffect::NotDispatched))?;
        if !next.within(self.allocation.amounts()) {
            return Err(failure(
                ConnectorFailureCode::BudgetExhausted,
                correlation,
                ConnectorEffect::NotDispatched,
            ));
        }
        // Persisted parent already retained the entire phase ceiling. This
        // local child debit is never refunded, including transport uncertainty.
        usage.amounts = next;
        Ok(())
    }

    fn operation_guard(
        &self,
        operation: Operation,
        quote: SessionConnectorQuote,
    ) -> Arc<dyn super::CallGuard> {
        let allocation = self.allocation.clone();
        let authority = self.authority.clone();
        let quotes = self.quotes.clone();
        let pure_quote = self.pure_quote.clone();
        Arc::new(move || {
            authority
                .validate_unpaired_allocation(&allocation)
                .map_err(session_guard_error)?;
            let current = match &operation {
                Operation::Discovery(target) => {
                    if pure_quote.is_none() {
                        authority
                            .validate_connector_discovery(&allocation, target)
                            .map_err(session_guard_error)?;
                    }
                    quotes.quote_discovery(&allocation, target)
                }
                Operation::Request(request, source) => {
                    if pure_quote.is_none() {
                        authority
                            .validate_connector_allocation(&allocation, request, source.as_ref())
                            .map_err(session_guard_error)?;
                    }
                    quotes.quote_request(&allocation, request, source.as_ref())
                }
            }
            .map_err(session_guard_error)?;
            if current != quote {
                return Err(Error::SessionDenied(ConnectorFailureCode::BudgetExhausted));
            }
            match &operation {
                Operation::Discovery(target) => {
                    authority.validate_connector_discovery(&allocation, target)
                }
                Operation::Request(request, source) => {
                    authority.validate_connector_allocation(&allocation, request, source.as_ref())
                }
            }
            .map_err(session_guard_error)?;
            authority
                .validate_unpaired_allocation(&allocation)
                .map_err(session_guard_error)
        })
    }

    fn final_guard(&self, operation: Operation) -> Arc<dyn super::CallGuard> {
        let allocation = self.allocation.clone();
        let authority = self.authority.clone();
        Arc::new(move || {
            authority
                .validate_unpaired_allocation(&allocation)
                .map_err(session_guard_error)?;
            match &operation {
                Operation::Discovery(target) => {
                    authority.validate_connector_discovery(&allocation, target)
                }
                Operation::Request(request, source) => {
                    authority.validate_connector_allocation(&allocation, request, source.as_ref())
                }
            }
            .map_err(session_guard_error)?;
            authority
                .validate_unpaired_allocation(&allocation)
                .map_err(session_guard_error)
        })
    }

    /// Retries use the original cancellation/deadline and the same non-cloneable
    /// aggregate spending cursor. Every attempt and reconnect/discovery is charged.
    pub fn invoke_with_retry(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
        policy: &gateway_application::connector_resilience::RetryPolicy,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        use gateway_application::connector_resilience::RetryDecision;
        let correlation = &request.correlation;
        let mut usage = self.lock(correlation)?;
        if let Some(terminal) = &usage.terminal {
            return Err(terminal.clone());
        }
        let mut context = self.context(correlation);
        context.deadline = context
            .deadline
            .min(std::time::Instant::now() + self.host.operation_timeout());
        context
            .check()
            .map_err(|e| e.failure(correlation, ConnectorEffect::NotDispatched))?;
        let support = self
            .host
            .recovery_support(request, source)
            .unwrap_or_default();
        let mut attempts = 0_u32;
        let admitted_deadline = std::cell::Cell::new(context.deadline);
        loop {
            attempts += 1;
            let result =
                self.request_once(&mut usage, request, source, &context, &admitted_deadline);
            context.deadline = context.deadline.min(admitted_deadline.get());
            let error = match result {
                Ok(observation) => return Ok(observation),
                Err(error) => error,
            };
            let decision = policy.decide(
                &error,
                &support,
                attempts,
                context
                    .deadline
                    .saturating_duration_since(std::time::Instant::now()),
                Some(self.host.health_snapshot().retry_after),
            );
            self.host.record_retry(&context, decision.clone());
            let RetryDecision::RetryAfter(delay) = decision else {
                return Err(error);
            };
            let until = std::time::Instant::now() + delay;
            while std::time::Instant::now() < until {
                context
                    .check()
                    .map_err(|e| e.failure(correlation, error.effect))?;
                self.authority
                    .validate_allocation(&self.allocation)
                    .and_then(|_| {
                        self.authority.validate_connector_allocation(
                            &self.allocation,
                            request,
                            source,
                        )
                    })
                    .map_err(|e| session_failure(e, correlation, error.effect))?;
                std::thread::sleep(
                    until
                        .saturating_duration_since(std::time::Instant::now())
                        .min(std::time::Duration::from_millis(5)),
                );
            }
            context
                .check()
                .map_err(|e| e.failure(correlation, error.effect))?;
            usage.retry_attempts = usage.retry_attempts.checked_add(1).ok_or_else(|| {
                failure(
                    ConnectorFailureCode::BudgetExhausted,
                    correlation,
                    ConnectorEffect::NotDispatched,
                )
            })?;
            if matches!(
                error.code,
                ConnectorFailureCode::Unavailable | ConnectorFailureCode::Timeout
            ) {
                self.discover_once(&mut usage, &request.target, &context)?;
            }
        }
    }
    pub fn retry_attempts(&self) -> Result<u64, SessionError> {
        self.usage
            .try_lock()
            .map(|usage| usage.retry_attempts)
            .map_err(|_| SessionError::Unavailable)
    }
    fn request(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        self.invoke_with_retry(request, source, &Default::default())
    }
    fn request_once(
        &self,
        usage: &mut Usage,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
        base_context: &CallContext,
        admitted_deadline: &std::cell::Cell<std::time::Instant>,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        let correlation = &request.correlation;
        if let Some(terminal) = &usage.terminal {
            return Err(terminal.clone());
        }
        let readonly = !matches!(
            request.class,
            ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
        );
        if request.target != *self.host.configured_target()
            || request.step != self.allocation.binding().step
            || !readonly
            || (source.is_some() && !readonly)
        {
            return Err(failure(
                ConnectorFailureCode::ScopeMismatch,
                correlation,
                ConnectorEffect::NotDispatched,
            ));
        }
        let context = base_context.clone();
        context
            .check()
            .map_err(|e| e.failure(correlation, ConnectorEffect::NotDispatched))?;
        self.authority
            .validate_allocation(&self.allocation)
            .and_then(|_| {
                self.authority
                    .validate_connector_allocation(&self.allocation, request, source)
            })
            .map_err(|e| session_failure(e, correlation, ConnectorEffect::NotDispatched))?;
        let quote = self
            .quotes
            .quote_request(&self.allocation, request, source)
            .map_err(|e| session_failure(e, correlation, ConnectorEffect::NotDispatched))?;
        self.reserve(usage, &quote, readonly, correlation)?;
        let operation = Operation::Request(request.clone(), source.cloned());
        let context = context
            .with_guard(self.operation_guard(operation.clone(), quote.clone()))
            .with_final_guard(self.final_guard(operation));
        let preflight = |dispatch: &ConnectorDispatch| {
            // Retain the narrowest CG-09 deadline across every retry. A fresh
            // authorization may revoke/narrow it, but cannot restart the clock.
            use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
            let remaining = u64::try_from(dispatch.deadline.seconds())
                .ok()
                .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
                .and_then(|deadline| deadline.duration_since(SystemTime::now()).ok())
                .ok_or_else(|| {
                    admitted_deadline.set(Instant::now());
                    failure(
                        ConnectorFailureCode::Timeout,
                        correlation,
                        ConnectorEffect::NotDispatched,
                    )
                })?;
            let now = Instant::now();
            admitted_deadline
                .set(now + remaining.min(admitted_deadline.get().saturating_duration_since(now)));
            context
                .check()
                .map_err(|e| e.failure(correlation, ConnectorEffect::NotDispatched))?;
            if dispatch.request != *request
                || dispatch.budget_reservation.as_str() != self.allocation.reservation_id().as_str()
            {
                return Err(failure(
                    ConnectorFailureCode::PermissionDenied,
                    correlation,
                    ConnectorEffect::NotDispatched,
                ));
            }
            Ok(())
        };
        let result = self
            .host
            .invoke_with_context_and_preflight(request, source, &context, &preflight);
        let result = match result {
            Ok(observation) => {
                let effect = if readonly {
                    ConnectorEffect::NoMutation
                } else {
                    ConnectorEffect::Unknown
                };
                match self.quotes.validate_observation(
                    &self.allocation,
                    &quote,
                    request,
                    source,
                    &observation,
                ) {
                    Ok(()) => context
                        .check()
                        .map_err(|e| e.failure(correlation, effect))
                        .map(|_| observation),
                    Err(error) => {
                        // A broken post-result accounting contract invalidates
                        // the run even when this operation could not mutate.
                        let error = session_failure(error, correlation, effect);
                        usage.terminal = Some(error.clone());
                        Err(error)
                    }
                }
            }
            Err(error) => Err(error),
        };
        if let Err(error) = &result
            && error.effect == ConnectorEffect::Unknown
        {
            usage.terminal = Some(error.clone());
        }
        result
    }
}

impl SessionConnectorRun {
    fn discover_once(
        &self,
        usage: &mut Usage,
        target: &ConnectorTarget,
        base_context: &CallContext,
    ) -> Result<ConnectorInventory, ConnectorFailure> {
        let correlation = &base_context.correlation;
        if let Some(terminal) = &usage.terminal {
            return Err(terminal.clone());
        }
        if target != self.host.configured_target() {
            return Err(failure(
                ConnectorFailureCode::ScopeMismatch,
                correlation,
                ConnectorEffect::NotDispatched,
            ));
        }
        let context = base_context.clone();
        context
            .check()
            .map_err(|e| e.failure(correlation, ConnectorEffect::NotDispatched))?;
        self.authority
            .validate_allocation(&self.allocation)
            .and_then(|_| {
                self.authority
                    .validate_connector_discovery(&self.allocation, target)
            })
            .map_err(|e| session_failure(e, correlation, ConnectorEffect::NotDispatched))?;
        let quote = self
            .quotes
            .quote_discovery(&self.allocation, target)
            .map_err(|e| session_failure(e, correlation, ConnectorEffect::NotDispatched))?;
        self.reserve(usage, &quote, true, correlation)?;
        let operation = Operation::Discovery(target.clone());
        let context = context
            .with_guard(self.operation_guard(operation.clone(), quote.clone()))
            .with_final_guard(self.final_guard(operation));
        let preflight = || {
            context
                .check()
                .map_err(|e| e.failure(correlation, ConnectorEffect::NotDispatched))
        };
        let inventory = self
            .host
            .discover_with_context_and_preflight(target, &context, &preflight)?;
        if let Err(error) = self
            .quotes
            .validate_inventory(&self.allocation, &quote, &inventory)
        {
            let error = session_failure(error, correlation, ConnectorEffect::NoMutation);
            usage.terminal = Some(error.clone());
            return Err(error);
        }
        context
            .check()
            .map_err(|e| e.failure(correlation, ConnectorEffect::NoMutation))?;
        Ok(inventory)
    }
}
impl ConnectorApplicationPort for SessionConnectorRun {
    fn discover(&self, target: &ConnectorTarget) -> Result<ConnectorInventory, ConnectorFailure> {
        let mut usage = self.lock(&self.context.correlation)?;
        self.discover_once(&mut usage, target, &self.context)
    }
    fn invoke(&self, request: &ConnectorRequest) -> Result<ConnectorObservation, ConnectorFailure> {
        self.request(request, None)
    }
    fn read_resource(
        &self,
        request: &ConnectorRequest,
        source: &SourceId,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        self.request(request, Some(source))
    }
    fn lifecycle(
        &self,
        target: &ConnectorTarget,
        command: ConnectorLifecycleCommand,
    ) -> Result<ConnectorLifecycleState, ConnectorFailure> {
        if !matches!(
            command,
            ConnectorLifecycleCommand::Disable | ConnectorLifecycleCommand::Shutdown
        ) {
            return Err(failure(
                ConnectorFailureCode::PermissionDenied,
                &self.context.correlation,
                ConnectorEffect::NotDispatched,
            ));
        }
        if target != self.host.configured_target() {
            return Err(failure(
                ConnectorFailureCode::ScopeMismatch,
                &self.context.correlation,
                ConnectorEffect::NotDispatched,
            ));
        }
        self.context.cancel();
        self.host.lifecycle(target, command)
    }
}

#[derive(Clone)]
enum Operation {
    Discovery(ConnectorTarget),
    Request(ConnectorRequest, Option<SourceId>),
}
fn has_non_connector_resources(amounts: &SessionResourceAmounts) -> bool {
    amounts.model_attempts != 0
        || amounts.input_tokens != 0
        || amounts.output_tokens != 0
        || amounts.output_bytes != 0
        || amounts.repair_attempts != 0
        || amounts.worker_attempts != 0
        || amounts.worker_compute_units != 0
}
fn session_guard_error(error: SessionError) -> Error {
    Error::SessionDenied(session_code(error))
}
fn session_code(error: SessionError) -> ConnectorFailureCode {
    match error {
        SessionError::LimitExceeded => ConnectorFailureCode::BudgetExhausted,
        SessionError::ScopeDenied => ConnectorFailureCode::ScopeMismatch,
        SessionError::Unavailable | SessionError::StorageUnavailable => {
            ConnectorFailureCode::Unavailable
        }
        SessionError::OutcomeUnknown => ConnectorFailureCode::MutationUncertain,
        _ => ConnectorFailureCode::PermissionDenied,
    }
}
fn session_failure(
    error: SessionError,
    correlation: &NonEmptyText,
    effect: ConnectorEffect,
) -> ConnectorFailure {
    failure(session_code(error), correlation, effect)
}
fn failure(
    code: ConnectorFailureCode,
    correlation: &NonEmptyText,
    effect: ConnectorEffect,
) -> ConnectorFailure {
    ConnectorFailure {
        code,
        effect,
        correlation: correlation.clone(),
    }
}

#[cfg(test)]
#[path = "session_budget_test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "session_budget_boundary_tests.rs"]
mod boundary_tests;
