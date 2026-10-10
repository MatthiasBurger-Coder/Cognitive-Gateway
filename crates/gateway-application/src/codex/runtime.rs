//! Provider-neutral invocation context; never an execution grant or session budget.
use super::FacadeError;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct RequestContext {
    pub correlation_id: String,
    pub deadline: Instant,
    cancelled: Arc<AtomicBool>,
}
impl RequestContext {
    pub fn new(correlation_id: String, deadline: Instant) -> Self {
        Self {
            correlation_id,
            deadline,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn check(&self) -> Result<(), FacadeError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(FacadeError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(FacadeError::Timeout)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::*;
    use serde_json::{Value, json};
    use std::time::Duration;
    #[test]
    fn deadline_and_cancellation_are_shared_without_reset() {
        let context =
            RequestContext::new("local-1".into(), Instant::now() + Duration::from_secs(1));
        context.check().unwrap();
        let nested = context.clone();
        assert_eq!(nested.deadline, context.deadline);
        context.cancel();
        assert_eq!(nested.check(), Err(FacadeError::Cancelled));
        let expired = RequestContext::new("local-2".into(), Instant::now());
        assert_eq!(expired.check(), Err(FacadeError::Timeout));
    }
    #[test]
    fn facade_propagates_context_before_policy_and_blocks_cancelled_work() {
        struct Host;
        impl CodexHost for Host {
            fn authorize(&self, call: &Call) -> Result<(), FacadeError> {
                let context = call.runtime.as_ref().unwrap();
                assert_eq!(context.correlation_id, "trusted-runtime-id");
                context.check()?;
                Err(FacadeError::PolicyDenied)
            }
        }
        let request: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/codex-v1/registry.inspect.request.json"
        ))
        .unwrap();
        let facade = CodexFacade::with_binding(
            ScopeBinding {
                scope: request["scope"].clone(),
                canonical_scope: gateway_domain::ContextScopeId::new("project-example").unwrap(),
                mapping_revision: "revision-1".into(),
                session: SessionContext {
                    principal: "operator".into(),
                    session_id: "session-1".into(),
                    connection_id: "binding-example".into(),
                },
            },
            Host,
        )
        .unwrap();
        let context = RequestContext::new(
            "trusted-runtime-id".into(),
            Instant::now() + Duration::from_secs(1),
        );
        assert_eq!(
            facade.execute_with_context("registry.inspect", &request, &context)["diagnostics"][0]["code"],
            "CG_POLICY_DENIED"
        );
        assert_eq!(
            facade.read_with_context(&request["scope"], "id", "revision", "digest", &context),
            Err(FacadeError::PolicyDenied)
        );
        context.cancel();
        assert_eq!(
            facade.execute_with_context("registry.inspect", &request, &context)["diagnostics"][0]["code"],
            "CG_CANCELLED"
        );
        assert_eq!(
            facade.read_with_context(&request["scope"], "id", "revision", "digest", &context),
            Err(FacadeError::Cancelled)
        );
        struct Port;
        impl CodexApplicationPort for Port {
            fn execute(&self, _: &str, _: &Value) -> Value {
                json!({"ok":true})
            }
        }
        assert_eq!(
            Port.execute_with_context("inspect", &json!({}), &context)["diagnostics"][0]["code"],
            "CG_CANCELLED"
        );
        assert_eq!(
            Port.read_with_context(&json!({}), "id", "revision", "digest", &context),
            Err(FacadeError::Cancelled)
        );
        let fresh = RequestContext::new("fresh".into(), Instant::now() + Duration::from_secs(1));
        assert_eq!(
            Port.execute_with_context("inspect", &json!({}), &fresh),
            json!({"ok":true})
        );
        assert_eq!(
            Port.read_with_context(&json!({}), "id", "revision", "digest", &fresh),
            Err(FacadeError::ScopeDenied)
        );
    }
}
