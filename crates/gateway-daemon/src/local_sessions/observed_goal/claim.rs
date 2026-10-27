//! Current durable ownership, shared by the budget and native read source.
use super::super::{LocalSessionAccess, registered_connector_goal};
use crate::{codex_workspace::LocalCodexHost, session_store::PostgresSessionStore};
use gateway_application::sessions::*;
use gateway_domain::ContextScopeId;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

// Credentials remain private to the storage adapter. Neither this selector nor
// its connection string can be serialized into goal evidence or diagnostics.
struct StoreDefinition {
    path: PathBuf,
    connection: String,
    scope: ContextScopeId,
}
impl StoreDefinition {
    fn capture(host: &LocalCodexHost) -> Result<Self, SessionError> {
        let config = host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        if !config.store_file.is_absolute() {
            return Err(SessionError::InvalidInput);
        }
        let connection = super::super::read(
            config
                .store_file
                .to_str()
                .ok_or(SessionError::InvalidInput)?,
            4096,
        )?;
        if connection.trim().is_empty() {
            return Err(SessionError::Unavailable);
        }
        Ok(Self {
            path: config.store_file.clone(),
            connection: connection.trim().to_owned(),
            scope: host.binding.canonical_scope.clone(),
        })
    }
    fn matches(&self, current: &Self) -> bool {
        self.path == current.path
            && self.scope == current.scope
            && self.connection == current.connection
    }
}

pub(super) fn now_ms() -> Result<u64, SessionError> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| SessionError::Unavailable)?
            .as_millis(),
    )
    .map_err(|_| SessionError::LimitExceeded)
}

pub(super) struct GoalClaim {
    access: Arc<LocalSessionAccess>,
    store_definition: StoreDefinition,
    repository: PostgresSessionStore,
    expected: FencedSession,
}
impl GoalClaim {
    pub(super) fn capture(
        access: Arc<LocalSessionAccess>,
        checkpoint: &SessionCheckpoint,
    ) -> Result<Self, SessionError> {
        if access.owner()? != checkpoint.owner {
            return Err(SessionError::ScopeDenied);
        }
        let host = access.current()?;
        let definition = StoreDefinition::capture(&host)?;
        let repository = access.selected_repository(&host)?;
        let expected = repository.load(&checkpoint.owner, &checkpoint.snapshot.session)?;
        if expected.checkpoint != *checkpoint {
            return Err(SessionError::StaleRevision);
        }
        let result = Self {
            access,
            store_definition: definition,
            repository,
            expected,
        };
        result.current()?;
        Ok(result)
    }
    pub(super) fn repository(&self) -> &PostgresSessionStore {
        &self.repository
    }
    pub(super) fn current(&self) -> Result<LocalCodexHost, SessionError> {
        let checkpoint = &self.expected.checkpoint;
        if self.access.owner()? != checkpoint.owner {
            return Err(SessionError::ScopeDenied);
        }
        let host = self.access.current()?;
        if !self
            .store_definition
            .matches(&StoreDefinition::capture(&host)?)
        {
            return Err(SessionError::StaleRevision);
        }
        let goal = checkpoint
            .goal
            .as_connector()
            .ok_or(SessionError::UnsupportedCapability)?;
        let config = host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        if config.basis.is_some()
            || config.observed_goal_runtime.is_none()
            || host.binding.canonical_scope != *goal.scope()
            || config.execution != checkpoint.execution
            || registered_connector_goal(config, goal.scope())? != *goal
            || config.resource_limits.as_ref()
                != checkpoint
                    .resource_ledger
                    .as_ref()
                    .map(SessionResourceLedger::limits)
        {
            return Err(SessionError::StaleRevision);
        }
        self.validate_durable()?;
        Ok(host)
    }
    pub(super) fn validate_durable(&self) -> Result<(), SessionError> {
        let current = self.repository.load(
            &self.expected.checkpoint.owner,
            &self.expected.checkpoint.snapshot.session,
        )?;
        if current != self.expected {
            return Err(SessionError::StaleRevision);
        }
        let cp = &current.checkpoint;
        if cp.snapshot.state != SessionState::Dispatching || cp.pre_execution_goal.is_some() {
            return Err(SessionError::InvalidState);
        }
        let now = now_ms()?;
        if now >= cp.budget.deadline_ms || cp.lease_until_ms.is_none_or(|until| now >= until) {
            return Err(SessionError::ExpiredInteraction);
        }
        Ok(())
    }
}
impl SessionResourceAuthorityPort for GoalClaim {
    fn validate_allocation(
        &self,
        allocation: &SessionResourceAllocation,
    ) -> Result<(), SessionError> {
        allocation.amounts().validate_readonly()?;
        if std::time::Instant::now() >= allocation.deadline() {
            return Err(SessionError::ExpiredInteraction);
        }
        self.current()?;
        let cp = &self.expected.checkpoint;
        let goal = cp
            .goal
            .as_connector()
            .ok_or(SessionError::UnsupportedCapability)?;
        let binding = allocation.binding();
        let ledger = cp
            .resource_ledger
            .as_ref()
            .ok_or(SessionError::InvalidState)?;
        if allocation.phase() != SessionResourcePhase::Preparation
            || allocation.claim_revision() != cp.snapshot.revision
            || allocation.fence() != self.expected.fence
            || binding.owner != cp.owner
            || binding.scope != *goal.scope()
            || binding.session != cp.snapshot.session
            || binding.run != cp.snapshot.run
            || cp.snapshot.dispatch.as_ref() != Some(&binding.dispatch)
            || !goal.steps().contains(&binding.step)
            || allocation.cost_unit() != &ledger.limits().cost_unit
            || !ledger.reservations().iter().any(|r| {
                r.id() == allocation.reservation_id()
                    && r.binding() == binding
                    && allocation.amounts().within(r.amounts())
            })
        {
            return Err(SessionError::AuthorityDenied);
        }
        if std::time::Instant::now() >= allocation.deadline() {
            return Err(SessionError::ExpiredInteraction);
        }
        Ok(())
    }
}
