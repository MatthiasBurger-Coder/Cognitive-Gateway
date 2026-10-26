//! Native read-only connector observation for the existing session coordinator.
mod claim;
pub(super) mod definitions;
pub(super) mod read_association;
mod source;
mod target;
mod verify;

#[cfg(test)]
pub(in crate::local_sessions) use source::boundary_tests::Fixture as NativeBoundaryFixture;

use super::{LocalSessionAccess, registered_connector_goal};
use crate::codex_workspace::LocalCodexHost;
use gateway_application::sessions::*;
use std::sync::Arc;

pub(super) struct ObservedGoalRuntime {
    access: Arc<LocalSessionAccess>,
}
impl ObservedGoalRuntime {
    pub(super) fn new(access: Arc<LocalSessionAccess>) -> Self {
        Self { access }
    }
}

fn metadata(
    access: &LocalSessionAccess,
    cp: &SessionCheckpoint,
) -> Result<(LocalCodexHost, definitions::ReadonlyDefinitions), SessionError> {
    if access.owner()? != cp.owner {
        return Err(SessionError::ScopeDenied);
    }
    if claim::now_ms()? >= cp.budget.deadline_ms {
        return Err(SessionError::ExpiredInteraction);
    }
    let host = access.current()?;
    let goal = cp
        .goal
        .as_connector()
        .ok_or(SessionError::UnsupportedCapability)?;
    let config = host
        .sessions
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    if config.basis.is_some()
        || config.execution != cp.execution
        || registered_connector_goal(config, goal.scope())? != *goal
    {
        return Err(SessionError::StaleRevision);
    }
    let limits = config
        .resource_limits
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    limits.validate()?;
    if cp
        .resource_ledger
        .as_ref()
        .is_some_and(|ledger| ledger.limits() != limits)
    {
        return Err(SessionError::StaleRevision);
    }
    let runtime = config
        .observed_goal_runtime
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    let definitions = runtime.capture(&host, goal, cp.execution)?;
    definitions.validate_current_metadata(&access.current()?)?;
    if claim::now_ms()? >= cp.budget.deadline_ms {
        return Err(SessionError::ExpiredInteraction);
    }
    Ok((host, definitions))
}

impl ReadonlyGoalObservationRuntimePort for ObservedGoalRuntime {
    fn resource_limits(
        &self,
        cp: &SessionCheckpoint,
    ) -> Result<SessionResourceLimits, SessionError> {
        let (host, defs) = metadata(&self.access, cp)?;
        let limits = host
            .sessions
            .as_ref()
            .and_then(|c| c.resource_limits.clone())
            .ok_or(SessionError::UnsupportedCapability)?;
        if limits.cost_unit != defs.quotes.cost_unit {
            return Err(SessionError::AuthorityDenied);
        }
        Ok(limits)
    }
    fn preparation_budget(
        &self,
        cp: &SessionCheckpoint,
    ) -> Result<SessionResourceAmounts, SessionError> {
        let (host, defs) = metadata(&self.access, cp)?;
        let limits = host
            .sessions
            .as_ref()
            .and_then(|c| c.resource_limits.as_ref())
            .ok_or(SessionError::UnsupportedCapability)?;
        let amounts = target::preparation_budget(&defs)?;
        if defs.quotes.cost_unit != limits.cost_unit || !amounts.within(&limits.amounts) {
            return Err(SessionError::LimitExceeded);
        }
        Ok(amounts)
    }
    fn observe_goal(
        &self,
        cp: &SessionCheckpoint,
        resources: &mut SessionPhaseResources,
    ) -> Result<Option<PreExecutionGoalCandidate>, SessionError> {
        target::capture(Arc::clone(&self.access), cp, resources)
    }
    fn verify_retained_goal(
        &self,
        cp: &SessionCheckpoint,
        data: &PreExecutionGoalData,
    ) -> Result<Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>, SessionError>
    {
        verify::retained(&self.access, cp, data)
    }
}

/// Persist only native compilation and the real effective policy overlay.
/// Connection-specific live fingerprints remain transient guard data.
pub(super) fn context_digest(
    defs: &definitions::ReadonlyDefinitions,
    global_consent_required: bool,
) -> Result<gateway_domain::ContentDigest, gateway_application::sessions::SessionError> {
    use gateway_application::sessions::SessionError;
    let compiled: serde_json::Value = serde_json::from_str(
        &defs
            .compiled
            .to_json()
            .map_err(|_| SessionError::InvalidInput)?,
    )
    .map_err(|_| SessionError::InvalidInput)?;
    let value = serde_json::json!({
        "contract":"cg.connector-observed-context/1.0",
        "compiled":compiled,
        "effective_source_policy":{"consent_required":global_consent_required}
    });
    Ok(gateway_domain::context_content_digest(
        &serde_json::to_vec(&value).map_err(|_| SessionError::InvalidInput)?,
    ))
}
