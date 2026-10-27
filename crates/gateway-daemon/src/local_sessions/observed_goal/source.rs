//! Current native CG09/claim joins for the sealed read-only connector run.
use super::{
    claim::{GoalClaim, now_ms},
    definitions::*,
    read_association::*,
};
use crate::codex_workspace::LocalCodexHost;
use crate::mcp_client::{
    Error, ProtocolResult,
    normalization::*,
    scalar_quotes::ScalarResponseAdmission,
    session_budget::{QualifiedConnectorAuthority, ReadonlyAllocationView},
};
use gateway_application::{
    connector_manifest::InventoryKind, connector_policy::*, connectors::*, sessions::*,
};
use gateway_domain::*;
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(test)]
#[path = "source_boundary_tests.rs"]
pub(in crate::local_sessions) mod boundary_tests;
pub(super) struct GoalSource {
    pub(super) claim: Arc<GoalClaim>,
    pub(super) defs: Arc<ReadonlyDefinitions>,
    pub(super) checkpoint: SessionCheckpoint,
    pub(super) view: ReadonlyAllocationView,
    association: OnceLock<BoundReadAssociation>,
    atomic: OnceLock<super::atomic_association::BoundAtomicRead>,
    start: Mutex<Option<(ContentDigest, UnixTimestamp)>>,
    dispatched: Mutex<Option<ConnectorDispatch>>,
}
impl GoalSource {
    pub(super) fn new(
        claim: Arc<GoalClaim>,
        defs: Arc<ReadonlyDefinitions>,
        checkpoint: SessionCheckpoint,
        view: ReadonlyAllocationView,
    ) -> Self {
        Self {
            claim,
            defs,
            checkpoint,
            view,
            association: OnceLock::new(),
            atomic: OnceLock::new(),
            start: Mutex::new(None),
            dispatched: Mutex::new(None),
        }
    }
    pub(super) fn current(&self) -> Result<(), SessionError> {
        self.current_host().map(|_| ())
    }
    // Return the same host whose complete current definition and durable claim
    // were checked in this call; no host or authority is retained across calls.
    fn current_host(&self) -> Result<LocalCodexHost, SessionError> {
        let a = self.view.allocation();
        if std::time::Instant::now() >= a.deadline() {
            return Err(SessionError::ExpiredInteraction);
        }
        let host = self.claim.current()?;
        self.defs.validate_current_metadata(&host)?;
        self.claim.validate_durable()?;
        if std::time::Instant::now() >= a.deadline() {
            return Err(SessionError::ExpiredInteraction);
        }
        Ok(host)
    }
    pub(super) fn target(&self) -> ConnectorTarget {
        ConnectorTarget {
            connector: self.defs.runtime.connector.clone(),
            scope: self.defs.runtime.scope.clone(),
        }
    }
    pub(super) fn execution(&self) -> Result<NonEmptyText, SessionError> {
        NonEmptyText::new(
            self.defs
                .compiled
                .context()
                .execution_context()
                .id()
                .as_str(),
        )
        .map_err(|_| SessionError::InvalidInput)
    }
    pub(super) fn install_association(
        &self,
        bound: BoundReadAssociation,
    ) -> Result<(), SessionError> {
        self.current()?;
        self.association
            .set(bound)
            .map_err(|_| SessionError::InvalidState)?;
        self.current()
    }
    pub(super) fn install_atomic(
        &self,
        bound: super::atomic_association::BoundAtomicRead,
    ) -> Result<(), SessionError> {
        self.atomic
            .set(bound)
            .map_err(|_| SessionError::InvalidState)?;
        self.current()
    }
    fn check_allocation_identity(&self, a: &SessionResourceAllocation) -> Result<(), SessionError> {
        if !std::ptr::eq(a, self.view.allocation()) {
            return Err(SessionError::AuthorityDenied);
        }
        Ok(())
    }
    pub(super) fn route<'a>(
        &'a self,
        r: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<&'a ReadNormalization, SessionError> {
        if r.target != self.target()
            || r.task != self.defs.task.data().id
            || r.step != self.defs.native.step
            || r.execution != self.execution()?
        {
            return Err(SessionError::ScopeDenied);
        }
        let mut matches = self
            .defs
            .normalizations
            .iter()
            .filter(|n| n.read.capability == r.capability && n.read.arguments == r.arguments);
        let route = matches.next().ok_or(SessionError::ScopeDenied)?;
        if matches.next().is_some() {
            return Err(SessionError::InvalidInput);
        }
        let record = self
            .defs
            .mapping
            .records()
            .iter()
            .find(|m| {
                m.reference == route.read.mapping_reference
                    && m.capability == r.capability
                    && m.source.as_ref() == Some(&route.read.source)
                    && m.class.operation_class() == r.class
            })
            .ok_or(SessionError::ScopeDenied)?;
        if (record.kind == InventoryKind::Resource && source != Some(&route.read.source))
            || (record.kind == InventoryKind::Tool && source.is_some())
        {
            return Err(SessionError::ScopeDenied);
        }
        if self
            .defs
            .before
            .as_ref()
            .is_some_and(|world| world.read == route.read)
        {
            self.atomic
                .get()
                .ok_or(SessionError::InvalidState)?
                .validate(r, source)?;
            return Ok(route);
        }
        let association = self.association.get().ok_or(SessionError::InvalidState)?;
        let scope_export =
            association.fixed_read_export(r, &route.read.mapping_reference, source)?;
        association.validate_fixed_read(
            r,
            FixedReadJoin {
                mapping_reference: &route.read.mapping_reference,
                source,
                arguments: &route.read.arguments,
                scope_export,
            },
        )?;
        Ok(route)
    }
    pub(super) fn export(&self) -> Result<Option<&Pin>, SessionError> {
        let mut exports = self
            .defs
            .documents
            .values()
            .filter(|(p, _)| p.contract.as_str() == "cg.connector-state-universe");
        let first = exports.next().map(|(p, _)| p);
        if exports.next().is_some() {
            return Err(SessionError::InvalidInput);
        }
        Ok(first)
    }
    pub(super) fn begin(
        &self,
        r: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<(), SessionError> {
        self.current()?;
        self.route(r, source)?;
        *self
            .dispatched
            .lock()
            .map_err(|_| SessionError::Unavailable)? = None;
        let start = UnixTimestamp::new((now_ms()? / 1000) as i64);
        *self.start.lock().map_err(|_| SessionError::Unavailable)? =
            Some((connector_request_digest(r)?, start));
        Ok(())
    }
    pub(super) fn batch(
        &self,
        request: &ConnectorRequest,
        observation: &ConnectorObservation,
    ) -> Result<gateway_application::ScopedObservationBatch, SessionError> {
        let actual = self
            .dispatched
            .lock()
            .map_err(|_| SessionError::Unavailable)?;
        let dispatch = actual
            .as_ref()
            .filter(|d| d.request == *request)
            .ok_or(SessionError::InvalidState)?;
        gateway_application::connector_results::ConnectorResult::new(dispatch, observation.clone())
            .map_err(|_| SessionError::InvalidInput)?
            .batch()
            .map_err(|_| SessionError::InvalidInput)
    }
    pub(super) fn global_consent_required(&self) -> Result<bool, SessionError> {
        let host = self.current_host()?;
        Ok(host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?
            .consent_required)
    }
    fn snapshot(
        &self,
        r: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, SessionError> {
        let host = self.current_host()?;
        let route = self.route(r, source)?;
        let mapping = self
            .defs
            .mapping
            .records()
            .iter()
            .find(|m| m.reference == route.read.mapping_reference)
            .ok_or(SessionError::InvalidInput)?;
        let global = host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?
            .consent_required;
        let mut facts = self.defs.native.facts.clone();
        facts.consents.clear();
        let native = &self.defs.native;
        let cap = native
            .capabilities
            .get(&r.capability)
            .ok_or(SessionError::AuthorityDenied)?;
        let consent_required = global
            || mapping.consent == crate::mcp_client::mapping::ConsentHint::Required
            || cap.requires_mutation_policy();
        let result = ConnectorPolicySnapshot {
            step: native.step.clone(),
            resolved: native.resolved,
            has_prerequisites: native.has_prerequisites,
            constraints: native.constraints.clone(),
            preconditions: native.preconditions.clone(),
            authority: self.defs.command.authority.clone(),
            facts,
            operating_mode: native.operating_mode,
            execution_profile: native.execution_profile,
            process: native.process,
            consent_required,
            administrative_capabilities: Default::default(),
            policy: self
                .defs
                .command
                .catalog
                .workflow(&self.defs.command.projection.mapping.workflow)
                .ok_or(SessionError::InvalidInput)?
                .policy_id()
                .clone(),
            policy_revision: self.defs.runtime.policy.digest.clone(),
            consent_receipt: None,
            budget_reservation: NonEmptyText::new(self.view.allocation().reservation_id().as_str())
                .map_err(|_| SessionError::InvalidInput)?,
            deadline: UnixTimestamp::new(
                (self
                    .checkpoint
                    .lease_until_ms
                    .ok_or(SessionError::InvalidState)?
                    .min(self.checkpoint.budget.deadline_ms)
                    / 1000) as i64,
            ),
        };
        self.current()?;
        Ok(result)
    }
}
impl SessionResourceAuthorityPort for GoalSource {
    fn validate_allocation(&self, a: &SessionResourceAllocation) -> Result<(), SessionError> {
        self.check_allocation_identity(a)?;
        self.current()
    }
    fn validate_connector_allocation(
        &self,
        a: &SessionResourceAllocation,
        r: &ConnectorRequest,
        s: Option<&SourceId>,
    ) -> Result<(), SessionError> {
        self.check_allocation_identity(a)?;
        self.route(r, s)?;
        self.current()
    }
    fn validate_connector_discovery(
        &self,
        a: &SessionResourceAllocation,
        t: &ConnectorTarget,
    ) -> Result<(), SessionError> {
        self.check_allocation_identity(a)?;
        if t != &self.target() {
            return Err(SessionError::ScopeDenied);
        }
        self.current()
    }
}
impl QualifiedConnectorAuthority for GoalSource {}
impl ConnectorPolicySource for GoalSource {
    fn admit_target(&self, t: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        self.current()
            .and_then(|_| {
                if t == &self.target() {
                    Ok(())
                } else {
                    Err(SessionError::ScopeDenied)
                }
            })
            .map_err(|e| failure(e, &NonEmptyText::new("target").expect("literal")))
    }
    fn authorize(
        &self,
        r: &ConnectorRequest,
        s: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.snapshot(r, s).map_err(|e| failure(e, &r.correlation))
    }
    fn revalidate(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.snapshot(&d.request, s)
            .map_err(|e| failure(e, &d.request.correlation))
    }
    fn record_decision(&self, e: &ConnectorPolicyEvaluation<'_>) -> Result<(), ConnectorFailure> {
        self.claim
            .repository()
            .record_observation_policy_decision(&self.checkpoint.owner, self.view.allocation(), e)
            .map_err(|err| failure(err, &e.request().correlation))?;
        if e.outcome() == ConnectorPolicyOutcome::Allowed
            && let ConnectorPolicyStage::PreEffect(dispatch) = e.stage()
        {
            *self
                .dispatched
                .lock()
                .map_err(|_| failure(SessionError::Unavailable, &e.request().correlation))? =
                Some(dispatch.clone());
        }
        Ok(())
    }
}
// Arc delegation retains one exact source across the native admission, resource,
// normalization and response-membership boundaries; it adds no admission path.
pub(super) struct PolicySource(pub(super) Arc<GoalSource>);
impl ConnectorPolicySource for PolicySource {
    fn admit_target(&self, t: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        self.0.admit_target(t)
    }
    fn authorize(
        &self,
        r: &ConnectorRequest,
        s: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.0.authorize(r, s)
    }
    fn revalidate(
        &self,
        d: &ConnectorDispatch,
        s: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.0.revalidate(d, s)
    }
    fn record_decision(&self, e: &ConnectorPolicyEvaluation<'_>) -> Result<(), ConnectorFailure> {
        self.0.record_decision(e)
    }
}
impl ScalarResponseAdmission for GoalSource {
    fn validate(
        &self,
        r: &ConnectorRequest,
        s: Option<&SourceId>,
        result: &ProtocolResult,
    ) -> Result<(), ConnectorFailure> {
        let route = self
            .route(r, s)
            .map_err(|e| response_failure(e, &r.correlation))?;
        if self
            .defs
            .before
            .as_ref()
            .is_some_and(|world| world.read == route.read)
        {
            return self
                .atomic
                .get()
                .ok_or_else(|| response_failure(SessionError::InvalidState, &r.correlation))?
                .response(r, result);
        }
        let association = self
            .association
            .get()
            .ok_or_else(|| response_failure(SessionError::InvalidState, &r.correlation))?;
        let scope_export = association
            .fixed_read_export(r, &route.read.mapping_reference, s)
            .map_err(|e| response_failure(e, &r.correlation))?;
        association
            .validate_fixed_read(
                r,
                FixedReadJoin {
                    mapping_reference: &route.read.mapping_reference,
                    source: s,
                    arguments: &route.read.arguments,
                    scope_export,
                },
            )
            .map_err(|e| response_failure(e, &r.correlation))?
            .validate_response(result)
    }
}
impl AcquisitionPort for Arc<GoalSource> {
    fn collect(&self, d: &ConnectorDispatch) -> Result<Acquisition, Error> {
        let mapping = self
            .defs
            .mapping
            .records()
            .iter()
            .find(|m| m.capability == d.request.capability)
            .ok_or(Error::Schema)?;
        let source = if mapping.kind == InventoryKind::Resource {
            mapping.source.as_ref()
        } else {
            None
        };
        let route = self.route(&d.request, source).map_err(|_| Error::Schema)?;
        let started = self.start.lock().map_err(|_| Error::Unavailable)?;
        let (digest, start) = started.as_ref().ok_or(Error::Schema)?;
        if digest != &connector_request_digest(&d.request).map_err(|_| Error::Schema)? {
            return Err(Error::Schema);
        }
        let evaluated =
            UnixTimestamp::new((now_ms().map_err(|_| Error::Unavailable)? / 1000) as i64);
        let ReadSemantics::DirectCurrentReadProfile {
            max_age_seconds,
            time_basis: ReadTimeBasis::RequestStartLowerBound,
        } = route.document.acquisition.semantics;
        Ok(Acquisition {
            acquired_at: evaluated,
            observed_at: Some(*start),
            evaluated_at: evaluated,
            max_age: FreshnessPolicy::new(max_age_seconds),
            authenticated_source: route.document.acquisition.authenticated_profile.clone(),
            sensitivity: route.document.acquisition.sensitivity,
            // Exact schema/identity-checked scalar acquisition is not a
            // probabilistic estimate; this is the pinned host read contract.
            confidence: Confidence::NotApplicable,
            uncertainty: Uncertainty::None,
            conflict: ConflictStatus::None,
            partial: false,
            basis: ConnectorObservationBasis::SourceRead,
            effect: ConnectorEffect::NoMutation,
            verifies: None,
            non_authoritative_raw: None,
        })
    }
}
impl QualifiedAcquisitionPort for Arc<GoalSource> {}
pub(super) fn failure(e: SessionError, correlation: &NonEmptyText) -> ConnectorFailure {
    ConnectorFailure {
        code: match e {
            SessionError::ScopeDenied => ConnectorFailureCode::ScopeMismatch,
            SessionError::LimitExceeded => ConnectorFailureCode::BudgetExhausted,
            SessionError::StorageUnavailable | SessionError::Unavailable => {
                ConnectorFailureCode::Unavailable
            }
            SessionError::OutcomeUnknown => ConnectorFailureCode::MutationUncertain,
            _ => ConnectorFailureCode::PermissionDenied,
        },
        effect: ConnectorEffect::NotDispatched,
        correlation: correlation.clone(),
    }
}

fn response_failure(e: SessionError, correlation: &NonEmptyText) -> ConnectorFailure {
    let mut f = failure(e, correlation);
    f.effect = ConnectorEffect::NoMutation;
    f
}
