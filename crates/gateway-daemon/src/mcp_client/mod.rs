//! Provider-independent MCP 2025-11-25 client. Effect dispatch is private to the
//! admitted application host; public wire discovery never confers authority.
pub(crate) mod credential_file;
mod host;
mod json;
pub mod mapping;
pub mod normalization;
pub mod plugins;
mod protocol;
mod resilience;
pub use resilience::ResilienceConfig;
pub(crate) mod scalar_quotes;
mod schema;
pub mod session_budget;
pub mod transport;
pub use host::{
    ConnectorAdmission, ConnectorMapping, HostConfig, McpConnectorHost, ProtocolResult,
    WireOperation,
};

pub use mapping::MappedToolSchema;

use gateway_application::connectors::{ConnectorEffect, ConnectorFailure, ConnectorFailureCode};
use gateway_domain::NonEmptyText;
use serde_json::Value;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub const MCP_VERSION: &str = "2025-11-25";

/// Stable content-free errors. Never retain provider messages, payloads or IO errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    SessionDenied(ConnectorFailureCode),
    Configuration,
    Unavailable,
    Timeout,
    /// Explicit adapter-profile rate limit; delay is a lower bound.
    RateLimited(Duration),
    Cancelled,
    Protocol,
    IncompatibleVersion,
    Limit,
    Remote(i64),
    Unsupported,
    InvalidState,
    InventoryChanged,
    Schema,
    Authentication,
    CredentialUnavailable,
    CredentialScopeDenied,
    SecretDisclosure,
}
impl Error {
    fn failure(self, correlation: &NonEmptyText, effect: ConnectorEffect) -> ConnectorFailure {
        use ConnectorFailureCode as C;
        let code = match self {
            Self::SessionDenied(code) => code,
            Self::Unavailable => C::Unavailable,
            Self::Authentication => C::AuthenticationFailed,
            Self::CredentialUnavailable => C::CredentialUnavailable,
            Self::CredentialScopeDenied => C::CredentialScopeDenied,
            Self::SecretDisclosure => C::SecretDisclosure,
            Self::Timeout => C::Timeout,
            Self::RateLimited(_) => C::RateLimited,
            Self::Cancelled => C::Cancelled,
            Self::IncompatibleVersion => C::IncompatibleVersion,
            Self::InventoryChanged => C::InventoryChanged,
            Self::Schema => C::SchemaMismatch,
            Self::Limit => C::BudgetExhausted,
            Self::Configuration
            | Self::Protocol
            | Self::Remote(_)
            | Self::Unsupported
            | Self::InvalidState => C::ProtocolError,
        };
        ConnectorFailure {
            code,
            effect,
            correlation: correlation.clone(),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    Initializing,
    Ready,
    Degraded,
    Disabled,
    Closing,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    Health(gateway_application::connector_resilience::HealthSnapshot),
    Retry(gateway_application::connector_resilience::RetryDecision),
    State(SessionState),
    Sent,
    Completed,
    Failed(Error),
    Invalidated,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub kind: EventKind,
    pub request_id: Option<u64>,
    pub correlation: NonEmptyText,
}
/// Implementations must be bounded, nonblocking and must not panic.
pub trait Observer: Send + Sync {
    fn record(&self, event: Event);
}
impl Observer for () {
    fn record(&self, _: Event) {}
}

/// Shared cancellation plus a monotonic deadline; reconnect never extends it.
#[derive(Clone)]
pub struct CallContext {
    pub correlation: NonEmptyText,
    pub deadline: Instant,
    cancelled: Arc<AtomicBool>,
    guard: Option<Arc<dyn CallGuard>>,
    final_guard: Option<Arc<dyn CallGuard>>,
}
/// Trusted session admission, invoked at every bounded I/O context check.
/// Implementations must be bounded and must not re-enter this connector host.
pub(crate) trait CallGuard: Send + Sync {
    fn check(&self) -> Result<(), Error>;
}
impl<F: Fn() -> Result<(), Error> + Send + Sync> CallGuard for F {
    fn check(&self) -> Result<(), Error> {
        self()
    }
}
impl CallContext {
    pub fn new(correlation: NonEmptyText, deadline: Instant) -> Self {
        Self {
            correlation,
            deadline,
            cancelled: Arc::new(AtomicBool::new(false)),
            guard: None,
            final_guard: None,
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn check(&self) -> Result<(), Error> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(Error::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(Error::Timeout)
        } else {
            if let Some(guard) = &self.guard {
                guard.check()?;
            }
            if let Some(guard) = &self.final_guard {
                guard.check()?;
            }
            // A trusted lookup can take time or trigger cancellation. Never
            // release I/O using the timestamp from before that callback.
            if self.cancelled.load(Ordering::Acquire) {
                Err(Error::Cancelled)
            } else if Instant::now() >= self.deadline {
                Err(Error::Timeout)
            } else {
                Ok(())
            }
        }
    }
    pub(crate) fn with_guard(&self, guard: Arc<dyn CallGuard>) -> Self {
        let guard: Arc<dyn CallGuard> = match &self.guard {
            Some(previous) => {
                let previous = previous.clone();
                Arc::new(move || {
                    previous.check()?;
                    guard.check()
                })
            }
            None => guard,
        };
        let mut context = self.clone();
        context.guard = Some(guard);
        context
    }
    pub(crate) fn with_final_guard(&self, guard: Arc<dyn CallGuard>) -> Self {
        let guard: Arc<dyn CallGuard> = match &self.final_guard {
            Some(previous) => {
                let previous = previous.clone();
                Arc::new(move || {
                    previous.check()?;
                    guard.check()
                })
            }
            None => guard,
        };
        let mut context = self.clone();
        context.final_guard = Some(guard);
        context
    }
    fn slice(&self) -> Duration {
        self.deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(5))
    }
}
#[derive(Clone)]
pub struct Limits {
    pub frame_bytes: usize,
    pub inventory_bytes: usize,
    pub items: usize,
    pub pages: usize,
    pub events_per_request: usize,
    pub call_timeout: Duration,
    pub shutdown: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            frame_bytes: 1_048_576,
            inventory_bytes: 4_194_304,
            items: 1024,
            pages: 32,
            events_per_request: 128,
            call_timeout: Duration::from_secs(30),
            shutdown: Duration::from_millis(100),
        }
    }
}
impl Limits {
    fn validate(&self) -> Result<(), Error> {
        if self.frame_bytes == 0
            || self.frame_bytes > 16_777_216
            || self.inventory_bytes == 0
            || self.inventory_bytes > 67_108_864
            || self.items == 0
            || self.items > 65_536
            || self.pages == 0
            || self.pages > 1024
            || self.events_per_request == 0
            || self.events_per_request > 4096
            || self.call_timeout.is_zero()
            || self.call_timeout > Duration::from_secs(3600)
            || self.shutdown.is_zero()
            || self.shutdown > Duration::from_secs(5)
        {
            Err(Error::Configuration)
        } else {
            Ok(())
        }
    }
}
/// Untrusted bounded protocol metadata for explicit mapping, never a canonical catalog.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProtocolInventory {
    pub tools: Vec<Value>,
    pub resources: Vec<Value>,
    pub templates: Vec<Value>,
    pub prompts: Vec<Value>,
}

#[cfg(test)]
mod context_tests;
