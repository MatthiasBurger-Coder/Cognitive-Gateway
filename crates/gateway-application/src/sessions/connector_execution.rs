//! Trusted session-level orchestration boundary for connector-backed steps.
//!
//! Host implementations obtain and verify model output for the exact admitted
//! invocation, then run it through `ConnectorProposalDispatcher`. Client DTOs
//! cannot construct `PreparedConnectorTurn` because `VerifiedOutput` itself is
//! application-issued. The coordinator persists the before record before it
//! calls `execute_step`.

use super::*;
use crate::{
    closed_loop::VerifiedOutcomeReceipt,
    connector_proposals::{DispatchedProposal, ProposalBatch, ProposalError},
    connector_results::validate_independent_post_effect_read,
    connectors::{
        ConnectorEffect, ConnectorObservation, ConnectorObservationBasis, ConnectorOperationClass,
        ConnectorRequest,
    },
    output_verification::VerifiedOutput,
    prompt_ir::InvocationIr,
};

pub const MAX_CONNECTOR_RECORD_BYTES: usize = 1_048_576;
pub const MAX_CONNECTOR_PROPOSALS_PER_STEP: usize = 32;
/// Maximum immutable records that one connector step can append atomically:
/// batch result + after snapshot + one post-effect read per proposal + one
/// independent final read per registered goal condition.
pub const MAX_CONNECTOR_STEP_COMMIT_RECORDS: usize =
    2 + MAX_CONNECTOR_PROPOSALS_PER_STEP + MAX_CONNECTOR_GOAL_CONDITIONS;
/// Bound the serialized payload of one connector step's atomic journal append.
/// Individual records remain capped by `MAX_CONNECTOR_RECORD_BYTES`.
pub const MAX_CONNECTOR_STEP_COMMIT_BYTES: usize = 8 * 1_048_576;

/// Canonical host-captured scoped state payload. The host must serialize the
/// complete snapshot for the registered project/scope and never include
/// connector credentials or unredacted secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedConnectorSnapshotPayload {
    scope: gateway_domain::ContextScopeId,
    bytes: Vec<u8>,
}

impl ScopedConnectorSnapshotPayload {
    pub fn new(
        scope: gateway_domain::ContextScopeId,
        bytes: Vec<u8>,
    ) -> Result<Self, SessionError> {
        if bytes.is_empty()
            || bytes.len() > MAX_CONNECTOR_RECORD_BYTES
            || serde_json::from_slice::<serde_json::Value>(&bytes).is_err()
        {
            return Err(SessionError::LimitExceeded);
        }
        Ok(Self { scope, bytes })
    }

    pub fn scope(&self) -> &gateway_domain::ContextScopeId {
        &self.scope
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

fn bound_record(
    contract: &str,
    binding: &ConnectorStepBinding,
    extra: serde_json::Value,
    payload: &[u8],
) -> Result<Vec<u8>, SessionError> {
    let value = serde_json::from_slice::<serde_json::Value>(payload)
        .map_err(|_| SessionError::InvalidInput)?;
    let mut envelope = serde_json::json!({
        "contract": contract,
        "contract_version": "1.0",
        "binding": binding,
        "payload": value,
    });
    if let (Some(object), Some(extra)) = (envelope.as_object_mut(), extra.as_object()) {
        object.extend(extra.clone());
    }
    let bytes = serde_json::to_vec(&envelope).map_err(|_| SessionError::InvalidInput)?;
    if bytes.len() > MAX_CONNECTOR_RECORD_BYTES {
        return Err(SessionError::LimitExceeded);
    }
    Ok(bytes)
}

pub fn before_snapshot_record(
    binding: &ConnectorStepBinding,
    payload: &ScopedConnectorSnapshotPayload,
) -> Result<Vec<u8>, SessionError> {
    if payload.scope() != &binding.scope {
        return Err(SessionError::ScopeDenied);
    }
    bound_record(
        "cg.connector-step-snapshot",
        binding,
        serde_json::json!({"phase":"before"}),
        payload.bytes(),
    )
}

pub fn after_snapshot_record(
    binding: &ConnectorStepBinding,
    payload: &ScopedConnectorSnapshotPayload,
) -> Result<Vec<u8>, SessionError> {
    if payload.scope() != &binding.scope {
        return Err(SessionError::ScopeDenied);
    }
    bound_record(
        "cg.connector-step-snapshot",
        binding,
        serde_json::json!({"phase":"after"}),
        payload.bytes(),
    )
}

pub fn proposal_batch_record(
    binding: &ConnectorStepBinding,
    batch: &ProposalBatch,
) -> Result<Vec<u8>, SessionError> {
    if batch.invocation != binding.invocation.invocation_digest
        || batch.results.len() > MAX_CONNECTOR_PROPOSALS_PER_STEP
        || batch
            .results
            .iter()
            .enumerate()
            .any(|(index, result)| result.ordinal != index)
        || batch
            .failure
            .as_ref()
            .is_some_and(|(ordinal, _)| *ordinal != batch.results.len())
        || (!batch.all_calls_returned && batch.failure.is_none())
        || (batch.all_calls_returned && batch.failure.is_some())
    {
        return Err(SessionError::InvalidInteraction);
    }
    let effect = aggregate_proposal_effect(batch);
    let results = batch
        .results
        .iter()
        .map(safe_dispatched_proposal)
        .collect::<Result<Vec<_>, _>>()?;
    let failure = batch.failure.as_ref().map(|(ordinal, error)| {
        let (code, effect, correlation) = safe_failure(error);
        serde_json::json!({
            "ordinal": ordinal,
            "code": code,
            "effect": connector_effect_name(effect),
            "correlation": correlation,
        })
    });
    let envelope = serde_json::json!({
        "contract": "cg.connector-proposal-batch-result",
        "contract_version": "1.0",
        "binding": binding,
        "payload": {
            "invocation": batch.invocation,
            "all_calls_returned": batch.all_calls_returned,
            "effect": connector_effect_name(effect),
            "results": results,
            "failure": failure,
        }
    });
    let bytes = serde_json::to_vec(&envelope).map_err(|_| SessionError::InvalidInput)?;
    if bytes.len() > MAX_CONNECTOR_RECORD_BYTES {
        return Err(SessionError::LimitExceeded);
    }
    Ok(bytes)
}

/// Conservative batch effect. The strongest possible effect wins so a later
/// known failure cannot erase an earlier successful or uncertain dispatch.
/// A successful zero-proposal response is ambiguous at this boundary and fails
/// closed as Unknown; an explicit pre-dispatch rejection remains NotDispatched.
pub fn aggregate_proposal_effect(batch: &ProposalBatch) -> ConnectorEffect {
    let mut effects = batch
        .results
        .iter()
        .map(|result| result.observation.metadata.effect)
        .collect::<Vec<_>>();
    if let Some((_, failure)) = &batch.failure {
        effects.push(safe_failure(failure).1);
    } else if effects.is_empty() {
        return ConnectorEffect::Unknown;
    }
    if effects.contains(&ConnectorEffect::Unknown) {
        ConnectorEffect::Unknown
    } else if effects.contains(&ConnectorEffect::Acknowledged) {
        ConnectorEffect::Acknowledged
    } else if effects.contains(&ConnectorEffect::NoMutation) {
        ConnectorEffect::NoMutation
    } else {
        ConnectorEffect::NotDispatched
    }
}

fn safe_dispatched_proposal(
    result: &DispatchedProposal,
) -> Result<serde_json::Value, SessionError> {
    let request = &result.request;
    let observation = &result.observation;
    Ok(serde_json::json!({
        "ordinal": result.ordinal,
        "request_digest": crate::sessions::connector_request_digest(request)?.as_str(),
        "capability": request.capability.as_str(),
        "operation_class": operation_class_name(request.class),
        "inventory_revision": request.inventory_revision,
        "execution": request.execution.as_str(),
        "correlation": request.correlation.as_str(),
        "observation": {
            "basis": observation_basis_name(observation.basis),
            "effect": connector_effect_name(observation.metadata.effect),
            "source": observation.source.as_str(),
            "content_digest": observation.content_digest,
            "source_revision": observation.source_revision.as_ref().map(|v| v.as_str()),
            "acquired_at": observation.acquired_at.seconds(),
        }
    }))
}

fn safe_failure(error: &ProposalError) -> (&'static str, ConnectorEffect, Option<String>) {
    match error {
        ProposalError::StaleInvocation => {
            ("stale_invocation", ConnectorEffect::NotDispatched, None)
        }
        ProposalError::InvalidProposal => {
            ("invalid_proposal", ConnectorEffect::NotDispatched, None)
        }
        ProposalError::PolicyDenied => ("policy_denied", ConnectorEffect::NotDispatched, None),
        ProposalError::Connector(failure) => (
            connector_failure_name(failure.code),
            failure.effect,
            Some(failure.correlation.as_str().to_owned()),
        ),
    }
}

fn connector_effect_name(effect: ConnectorEffect) -> &'static str {
    match effect {
        ConnectorEffect::NotDispatched => "not_dispatched",
        ConnectorEffect::NoMutation => "no_mutation",
        ConnectorEffect::Acknowledged => "acknowledged",
        ConnectorEffect::Unknown => "unknown",
    }
}

fn operation_class_name(class: ConnectorOperationClass) -> &'static str {
    match class {
        ConnectorOperationClass::Read => "read",
        ConnectorOperationClass::Search => "search",
        ConnectorOperationClass::Inspect => "inspect",
        ConnectorOperationClass::Mutate => "mutate",
        ConnectorOperationClass::Admin => "admin",
    }
}

fn observation_basis_name(basis: crate::connectors::ConnectorObservationBasis) -> &'static str {
    match basis {
        crate::connectors::ConnectorObservationBasis::ProviderAcknowledgement => {
            "provider_acknowledgement"
        }
        crate::connectors::ConnectorObservationBasis::SourceRead => "source_read",
        crate::connectors::ConnectorObservationBasis::IndependentPostEffectRead => {
            "independent_post_effect_read"
        }
    }
}

fn connector_failure_name(code: crate::connectors::ConnectorFailureCode) -> &'static str {
    use crate::connectors::ConnectorFailureCode::*;
    match code {
        Unavailable => "unavailable",
        AuthenticationFailed => "authentication_failed",
        CredentialUnavailable => "credential_unavailable",
        CredentialScopeDenied => "credential_scope_denied",
        SecretDisclosure => "secret_disclosure",
        PermissionDenied => "permission_denied",
        Timeout => "timeout",
        Cancelled => "cancelled",
        ProtocolError => "protocol_error",
        IncompatibleVersion => "incompatible_version",
        SchemaMismatch => "schema_mismatch",
        RateLimited => "rate_limited",
        NotFound => "not_found",
        StaleResult => "stale_result",
        ConflictingResult => "conflicting_result",
        MutationUncertain => "mutation_uncertain",
        RetryUnsafe => "retry_unsafe",
        ScopeMismatch => "scope_mismatch",
        InventoryChanged => "inventory_changed",
        BudgetExhausted => "budget_exhausted",
        Backpressure => "backpressure",
    }
}

pub fn readback_record(
    binding: &ConnectorStepBinding,
    ordinal: u32,
    payload: &[u8],
) -> Result<Vec<u8>, SessionError> {
    bound_record(
        "cg.connector-post-effect-readback",
        binding,
        serde_json::json!({"proposal_ordinal":ordinal}),
        payload,
    )
}

/// Exact admitted prompt and application-verified model response for one step.
#[derive(Debug, Clone)]
pub struct PreparedConnectorTurn {
    invocation: InvocationIr,
    verified: VerifiedOutput,
    before: ScopedConnectorSnapshotPayload,
}

impl PreparedConnectorTurn {
    pub fn new(
        invocation: InvocationIr,
        verified: VerifiedOutput,
        before: ScopedConnectorSnapshotPayload,
    ) -> Result<Self, SessionError> {
        if verified.record().admitted_invocation != invocation.identity()
            || verified.output().data().invocation != invocation.identity()
        {
            return Err(SessionError::StaleRevision);
        }
        Ok(Self {
            invocation,
            verified,
            before,
        })
    }

    pub fn invocation(&self) -> &InvocationIr {
        &self.invocation
    }

    pub fn verified_output(&self) -> &VerifiedOutput {
        &self.verified
    }

    pub fn before_snapshot(&self) -> &ScopedConnectorSnapshotPayload {
        &self.before
    }
}

/// An independently acquired readback correlated to one proposal ordinal.
#[derive(Debug, Clone)]
pub struct CorrelatedConnectorReadback {
    pub proposal_ordinal: u32,
    /// Exact independently admitted, read-only request used to obtain this observation.
    pub read_request: ConnectorRequest,
    pub observation: ConnectorObservation,
    pub batch: crate::ScopedObservationBatch,
}

/// Fresh final-state source read for exactly one registered CG-14 condition.
/// It answers what is true now; it never proves a particular mutation succeeded.
#[derive(Debug, Clone)]
pub struct GoalVerificationReadback {
    pub condition: gateway_domain::ConditionId,
    pub read_request: ConnectorRequest,
    pub observation: ConnectorObservation,
    pub batch: crate::ScopedObservationBatch,
}

/// Result of one connector dispatch turn. Effect is derived from the typed
/// proposal batch; individual independent readbacks remain separate records.
#[derive(Debug, Clone)]
pub struct ConnectorSessionTurn {
    pub proposal_batch: ProposalBatch,
    pub after_snapshot: Option<ScopedConnectorSnapshotPayload>,
    pub independent_readbacks: Vec<CorrelatedConnectorReadback>,
    /// Present only on the final planned step and covers every goal condition.
    pub goal_verification_readbacks: Vec<GoalVerificationReadback>,
}

impl ConnectorSessionTurn {
    pub fn validate(
        &self,
        goal: &SupportedConnectorGoal,
        step: &gateway_domain::PlanStepId,
        prepared: &PreparedConnectorTurn,
    ) -> Result<(), SessionError> {
        let invocation = prepared.invocation.identity();
        let proposals = &prepared.verified.output().data().tools;
        let batch = &self.proposal_batch;
        let effect = aggregate_proposal_effect(batch);
        if batch.invocation != invocation.invocation_digest
            || proposals.len() > MAX_CONNECTOR_PROPOSALS_PER_STEP
            || !goal.steps().contains(step)
            || prepared.before_snapshot().scope() != goal.scope()
            || self
                .after_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.scope() != goal.scope())
        {
            return Err(SessionError::InvalidInteraction);
        }
        let result_count = batch.results.len();
        if batch
            .results
            .iter()
            .enumerate()
            .any(|(index, result)| result.ordinal != index)
            || batch
                .results
                .windows(2)
                .any(|pair| pair[0].ordinal >= pair[1].ordinal)
            || batch
                .failure
                .as_ref()
                .is_some_and(|(ordinal, _)| *ordinal != result_count || *ordinal >= proposals.len())
            || (batch.all_calls_returned
                && (batch.failure.is_some() || batch.results.len() != proposals.len()))
            || (!batch.all_calls_returned && batch.failure.is_none())
            || (batch.all_calls_returned && batch.results.len() != proposals.len())
            || (effect == ConnectorEffect::NotDispatched
                && (!batch.results.is_empty() || result_count != 0))
        {
            return Err(SessionError::InvalidInteraction);
        }
        let mut readback_ordinals = std::collections::BTreeSet::new();
        for readback in &self.independent_readbacks {
            let ordinal = readback.proposal_ordinal as usize;
            let Some(result) = batch
                .results
                .iter()
                .find(|result| result.ordinal == ordinal)
            else {
                return Err(SessionError::InvalidInteraction);
            };
            let request = &result.request;
            let read_request = &readback.read_request;
            let snapshot_digest = readback
                .batch
                .snapshot()
                .digest()
                .ok_or(SessionError::InvalidInteraction)?;
            if !readback_ordinals.insert(readback.proposal_ordinal)
                || validate_independent_post_effect_read(
                    request,
                    read_request,
                    &readback.observation,
                )
                .is_err()
                || read_request.target.scope != *goal.scope()
                || readback.observation.target.scope != *goal.scope()
                || readback.batch.scope() != goal.scope()
                || readback.observation.records != *readback.batch.records()
                || readback.observation.content_digest != *snapshot_digest
            {
                return Err(SessionError::InvalidInteraction);
            }
        }
        if effect == ConnectorEffect::NotDispatched && !self.independent_readbacks.is_empty() {
            return Err(SessionError::InvalidInteraction);
        }
        if (matches!(
            effect,
            ConnectorEffect::Acknowledged | ConnectorEffect::NoMutation
        ) && self.after_snapshot.is_none())
            || (effect == ConnectorEffect::NotDispatched && self.after_snapshot.is_some())
            || (effect == ConnectorEffect::Acknowledged
                && !batch.results.iter().any(|r| {
                    matches!(
                        r.request.class,
                        ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
                    )
                }))
        {
            return Err(SessionError::InvalidInteraction);
        }
        let is_final_step = goal.steps().last() == Some(step);
        if (!is_final_step && !self.goal_verification_readbacks.is_empty())
            || (!matches!(
                effect,
                ConnectorEffect::Acknowledged | ConnectorEffect::NoMutation
            ) && !self.goal_verification_readbacks.is_empty())
        {
            return Err(SessionError::InvalidInteraction);
        }
        let mut covered = std::collections::BTreeSet::new();
        for readback in &self.goal_verification_readbacks {
            let request = &readback.read_request;
            let observation = &readback.observation;
            let Some(condition) = goal
                .intent()
                .desired_state()
                .conditions()
                .iter()
                .find(|candidate| candidate.id() == &readback.condition)
            else {
                return Err(SessionError::InvalidInteraction);
            };
            let mapped = goal.observations().iter().any(|mapping| {
                mapping.condition() == &readback.condition
                    && mapping.source() == &observation.source
                    && mapping.capability() == &request.capability
                    && matches!(
                        request.class,
                        ConnectorOperationClass::Read
                            | ConnectorOperationClass::Search
                            | ConnectorOperationClass::Inspect
                    )
            });
            let snapshot_digest = readback
                .batch
                .snapshot()
                .digest()
                .ok_or(SessionError::InvalidInteraction)?;
            if !covered.insert(readback.condition.clone())
                || !mapped
                || request.step != *step
                || request.task != prepared.invocation().prepared().invocation().data().task.id
                || request.target.scope != *goal.scope()
                || observation.basis != ConnectorObservationBasis::SourceRead
                || observation.metadata.verifies.is_some()
                || observation.metadata.effect != ConnectorEffect::NoMutation
                || observation.metadata.partial
                || observation.metadata.quality.trust()
                    != gateway_domain::TrustClass::ObservedEvidence
                || observation.metadata.quality.freshness()
                    == gateway_domain::FreshnessStatus::Stale
                || observation.metadata.quality.conflict()
                    == gateway_domain::ConflictStatus::Unresolved
                || observation.metadata.quality.uncertainty()
                    == gateway_domain::Uncertainty::Unknown
                || crate::connector_results::ConnectorResult::validate_for_request(
                    request,
                    observation,
                )
                .is_err()
                || observation.records != *readback.batch.records()
                || readback.batch.scope() != goal.scope()
                || observation.content_digest != *snapshot_digest
                || !observation
                    .records
                    .observations()
                    .iter()
                    .any(|value| value.subject() == condition.subject())
                || !observation
                    .records
                    .facts()
                    .iter()
                    .any(|value| value.subject() == condition.subject())
            {
                return Err(SessionError::InvalidInteraction);
            }
        }
        if !self.goal_verification_readbacks.is_empty() {
            let expected = goal
                .intent()
                .desired_state()
                .conditions()
                .iter()
                .map(|condition| condition.id().clone())
                .collect::<std::collections::BTreeSet<_>>();
            if covered != expected {
                return Err(SessionError::InvalidInteraction);
            }
        }
        Ok(())
    }
}

/// Coordinator-owned correlation metadata, never a wire grant. The minted
/// phase capability and current trusted authority enforce resource admission.
pub struct ConnectorPreparationContext<'a> {
    pub revision: Revision,
    pub pending: &'a PendingId,
    pub dispatch: &'a DispatchId,
    pub expires_at_ms: u64,
}

/// Trusted host boundary for one step. Implementations must use the current
/// checkpoint and registered step only. `execute_step` must dispatch only the
/// exact `VerifiedOutput`/`InvocationIr` pair from `PreparedConnectorTurn` via
/// the CG-09 gated proposal dispatcher; it must not trust client/model identity
/// fields. Readbacks are acquired after dispatch through independent reads.
pub trait ConnectorSessionRuntimePort {
    /// Pure, complete disjoint upper bounds for each resource-consuming phase.
    /// Legacy total-only envelopes cannot be used to mint phase allowances.
    fn resource_plan(
        &self,
        _checkpoint: &SessionCheckpoint,
        _step: &gateway_domain::PlanStepId,
    ) -> Result<SessionResourcePlan, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    /// Pure trusted planning only: return hard upper bounds for the whole step,
    /// including all model attempts/repairs and before/after/final reads. This
    /// method must perform no resource-consuming I/O. The coordinator persists
    /// this allowance before calling preparation; nested adapters must remain
    /// within it. Missing bounds make the route unsupported.
    fn resource_envelope(
        &self,
        _checkpoint: &SessionCheckpoint,
        _step: &gateway_domain::PlanStepId,
    ) -> Result<SessionResourceAmounts, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }

    fn initial_assessment(
        &self,
        checkpoint: &SessionCheckpoint,
    ) -> Result<InitialAssessment, SessionError>;

    fn prepare_step(
        &self,
        checkpoint: &SessionCheckpoint,
        step: &gateway_domain::PlanStepId,
    ) -> Result<PreparedConnectorTurn, SessionError>;

    /// Performs one invocation preparation and current CG-09 evaluation. If
    /// consent is required, it must return a binding plus immutable records
    /// sufficient to resume this exact verified proposal after restart.
    /// `Ready` is permitted only after current policy returns Allow.
    fn prepare_authorized_step(
        &self,
        _checkpoint: &SessionCheckpoint,
        _step: &gateway_domain::PlanStepId,
        _resources: &mut SessionPhaseResources,
        _context: ConnectorPreparationContext<'_>,
    ) -> Result<ConnectorPreparation, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }

    /// Reloads the persisted verified proposal associated with the approved
    /// binding. Implementations must not call the model or rebuild a different
    /// proposal. The dispatcher still revalidates CG-09 immediately pre-effect.
    fn resume_approved_step(
        &self,
        _checkpoint: &SessionCheckpoint,
        _step: &gateway_domain::PlanStepId,
        _resources: &mut SessionPhaseResources,
        _binding: &ConnectorConsentBinding,
        _material: &ConnectorConsentResumeMaterial,
    ) -> Result<PreparedConnectorTurn, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }

    /// Inspect authenticated retained data and current pure producer definitions
    /// before reserving approved-resume resources. No model, connector, worker
    /// or retrieval I/O is permitted here. Budgeted fresh reads and the existing
    /// actual `verify_output` reconstruction belong to `resume_approved_step`.
    /// The default preserves the existing stricter revalidation contract.
    fn preflight_retained_consent_binding(
        &self,
        checkpoint: &SessionCheckpoint,
        binding: &ConnectorConsentBinding,
        material: &ConnectorConsentResumeMaterial,
    ) -> Result<(), SessionError> {
        self.revalidate_consent_binding(checkpoint, binding, material)
    }

    /// Repeatable pure verification of invocation-free persisted goal reads.
    /// Implementations compare all current definition/provider/mapping pins and
    /// the complete registered goal. No model, retrieval or connector I/O.
    fn verify_pre_execution_goal(
        &self,
        _checkpoint: &SessionCheckpoint,
        _data: &PreExecutionGoalData,
    ) -> Result<Option<VerifiedOutcomeReceipt>, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }

    /// Pure local authority validation only; no model, connector, worker or
    /// retrieval call is permitted here. Budgeted reconstruction belongs to
    /// `resume_approved_step` under the approved-resume phase claim.
    /// Rebuilds the current exact request and CG-09 decision for an outstanding
    /// consent binding from `material`, checks the ordinal/request/policy and
    /// reruns `verify_output` over the retained raw bytes. It must never call
    /// the model. Implementations fail closed when current producer pins or
    /// invocation identity differ; this hook never dispatches.
    fn revalidate_consent_binding(
        &self,
        _checkpoint: &SessionCheckpoint,
        _binding: &ConnectorConsentBinding,
        _material: &ConnectorConsentResumeMaterial,
    ) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }

    fn execute_step(
        &self,
        checkpoint: &SessionCheckpoint,
        step: &gateway_domain::PlanStepId,
        resources: &mut SessionPhaseResources,
        dispatch: &DispatchId,
        prepared: &PreparedConnectorTurn,
    ) -> Result<ConnectorSessionTurn, SessionError>;

    /// Independently capture the complete scoped state after a failed or
    /// invalid dispatch result. This capture must not be inferred from the
    /// connector response or proposal observation.
    fn capture_after_snapshot(
        &self,
        _checkpoint: &SessionCheckpoint,
        _step: &gateway_domain::PlanStepId,
        _resources: &mut SessionPhaseResources,
        _dispatch: &DispatchId,
        _prepared: &PreparedConnectorTurn,
    ) -> Result<ScopedConnectorSnapshotPayload, SessionError> {
        Err(SessionError::Unavailable)
    }

    /// Pure local verification over retained records; no external or model
    /// acquisition is permitted. Recovery may repeat this computation without
    /// recreating any resource allowance.
    /// Returns a receipt only by running the existing ClosedLoop over the
    /// complete, persisted/reloaded final source-read set for the registered goal.
    fn verify_goal(
        &self,
        checkpoint: &SessionCheckpoint,
        step: &gateway_domain::PlanStepId,
        dispatch: &DispatchId,
        readbacks: &[PersistedConnectorReadback],
    ) -> Result<Option<VerifiedOutcomeReceipt>, SessionError>;
}

pub enum ConnectorPreparation {
    GoalAlreadySatisfied(Box<PreExecutionGoalCandidate>),
    Ready(Box<PreparedConnectorTurn>),
    ConsentRequired {
        binding: Box<ConnectorConsentBinding>,
        records: Vec<RecordWrite>,
    },
}

pub fn proposal_failure_effect(error: &ProposalError) -> ConnectorEffect {
    match error {
        ProposalError::Connector(failure) => failure.effect,
        ProposalError::InvalidProposal
        | ProposalError::StaleInvocation
        | ProposalError::PolicyDenied => ConnectorEffect::NotDispatched,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{BindingId, ClientOwnerId, PrincipalId, ProjectId, WorkspaceId};
    use crate::{
        connector_proposals::DispatchedProposal,
        connectors::{
            ConnectorFailure, ConnectorFailureCode, ConnectorObservationBasis, ConnectorTarget,
        },
    };
    use gateway_domain::{
        Confidence, FreshnessStatus, ReferenceId, SchemaVersion, SensitivityClass, SourceId,
        SourceKind, TaskId, TrustClass, Uncertainty, prompt_ir::InvocationIrIdentity,
    };
    use std::collections::BTreeMap;

    fn binding() -> ConnectorStepBinding {
        ConnectorStepBinding {
            owner: OwnerBinding {
                principal: PrincipalId::new("principal").unwrap(),
                workspace: WorkspaceId::new("workspace").unwrap(),
                project: ProjectId::new("project").unwrap(),
                binding: BindingId::new("binding").unwrap(),
                client_owner: ClientOwnerId::new("client-owner").unwrap(),
            },
            scope: gateway_domain::ContextScopeId::new("project").unwrap(),
            session: SessionId::new("session").unwrap(),
            run: RunId::new("run").unwrap(),
            revision: Revision::new(2).unwrap(),
            task: TaskId::new("task").unwrap(),
            step: gateway_domain::PlanStepId::new("step").unwrap(),
            dispatch: DispatchId::new("dispatch").unwrap(),
            invocation: InvocationIrIdentity {
                schema_version: SchemaVersion::V1,
                invocation_id: ReferenceId::new("invocation").unwrap(),
                invocation_digest: gateway_domain::ContentDigest::new("a".repeat(64)).unwrap(),
                handoff_digest: gateway_domain::ContentDigest::new("b".repeat(64)).unwrap(),
                tools_digest: gateway_domain::ContentDigest::new("c".repeat(64)).unwrap(),
                ir_digest: gateway_domain::ContentDigest::new("d".repeat(64)).unwrap(),
            },
        }
    }

    fn dispatched(effect: ConnectorEffect, class: ConnectorOperationClass) -> DispatchedProposal {
        let target = ConnectorTarget {
            connector: gateway_domain::NonEmptyText::new("connector").unwrap(),
            scope: gateway_domain::ContextScopeId::new("project").unwrap(),
        };
        let request = ConnectorRequest {
            target: target.clone(),
            capability: gateway_domain::CapabilityId::new("capability").unwrap(),
            class,
            inventory_revision: gateway_domain::ContentDigest::new("e".repeat(64)).unwrap(),
            arguments: BTreeMap::new(),
            task: TaskId::new("task").unwrap(),
            step: gateway_domain::PlanStepId::new("step").unwrap(),
            execution: gateway_domain::NonEmptyText::new("execution").unwrap(),
            correlation: gateway_domain::NonEmptyText::new("correlation").unwrap(),
        };
        let quality = gateway_domain::QualityMetadata::new(
            TrustClass::ObservedEvidence,
            SensitivityClass::Public,
            Confidence::Unknown,
            FreshnessStatus::Fresh,
            Uncertainty::None,
        );
        let records =
            gateway_domain::ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap();
        let observation = ConnectorObservation {
            metadata: crate::connector_results::ConnectorResultMetadata {
                task: request.task.clone(),
                step: request.step.clone(),
                execution: request.execution.clone(),
                object_reference: gateway_domain::NonEmptyText::new("object").unwrap(),
                source_kind: SourceKind::Tool,
                source_digest: None,
                observed_at: None,
                authenticated_source: gateway_domain::NonEmptyText::new("host").unwrap(),
                quality,
                effect,
                partial: false,
                verifies: None,
                non_authoritative_raw: None,
            },
            target,
            inventory_revision: request.inventory_revision.clone(),
            correlation: request.correlation.clone(),
            source: SourceId::new("source").unwrap(),
            source_revision: None,
            content_digest: gateway_domain::ContentDigest::new("f".repeat(64)).unwrap(),
            acquired_at: gateway_domain::UnixTimestamp::new(10),
            records,
            basis: ConnectorObservationBasis::ProviderAcknowledgement,
        };
        DispatchedProposal {
            ordinal: 0,
            request,
            observation,
        }
    }

    #[test]
    fn batch_effect_uses_conservative_precedence_for_mixed_calls() {
        let mut batch = ProposalBatch {
            invocation: gateway_domain::ContentDigest::new("a".repeat(64)).unwrap(),
            all_calls_returned: true,
            results: vec![
                dispatched(ConnectorEffect::NoMutation, ConnectorOperationClass::Read),
                dispatched(
                    ConnectorEffect::Acknowledged,
                    ConnectorOperationClass::Mutate,
                ),
            ],
            failure: None,
        };
        batch.results[1].ordinal = 1;
        assert_eq!(
            aggregate_proposal_effect(&batch),
            ConnectorEffect::Acknowledged
        );

        batch.all_calls_returned = false;
        batch.failure = Some((2, ProposalError::PolicyDenied));
        assert_eq!(
            aggregate_proposal_effect(&batch),
            ConnectorEffect::Acknowledged
        );

        batch.results[1].observation.metadata.effect = ConnectorEffect::Unknown;
        assert_eq!(aggregate_proposal_effect(&batch), ConnectorEffect::Unknown);

        batch.results[1].observation.metadata.effect = ConnectorEffect::Acknowledged;
        batch.failure = Some((
            2,
            ProposalError::Connector(ConnectorFailure {
                code: ConnectorFailureCode::MutationUncertain,
                effect: ConnectorEffect::Unknown,
                correlation: gateway_domain::NonEmptyText::new("safe-correlation").unwrap(),
            }),
        ));
        assert_eq!(aggregate_proposal_effect(&batch), ConnectorEffect::Unknown);
    }

    #[test]
    fn successful_empty_batch_is_unknown_but_explicit_denial_is_not_dispatched() {
        let mut batch = ProposalBatch {
            invocation: gateway_domain::ContentDigest::new("a".repeat(64)).unwrap(),
            all_calls_returned: true,
            results: vec![],
            failure: None,
        };
        assert_eq!(aggregate_proposal_effect(&batch), ConnectorEffect::Unknown);
        batch.all_calls_returned = false;
        batch.failure = Some((0, ProposalError::PolicyDenied));
        assert_eq!(
            aggregate_proposal_effect(&batch),
            ConnectorEffect::NotDispatched
        );
    }

    #[test]
    fn batch_record_is_derived_from_typed_batch_and_contains_only_safe_failure_fields() {
        let batch = ProposalBatch {
            invocation: gateway_domain::ContentDigest::new("a".repeat(64)).unwrap(),
            all_calls_returned: false,
            results: vec![dispatched(
                ConnectorEffect::Acknowledged,
                ConnectorOperationClass::Mutate,
            )],
            failure: Some((
                1,
                ProposalError::Connector(ConnectorFailure {
                    code: ConnectorFailureCode::Timeout,
                    effect: ConnectorEffect::Unknown,
                    correlation: gateway_domain::NonEmptyText::new("safe-correlation").unwrap(),
                }),
            )),
        };
        let bytes = proposal_batch_record(&binding(), &batch).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["contract_version"], "1.0");
        assert_eq!(value["payload"]["effect"], "unknown");
        assert_eq!(value["payload"]["results"][0]["ordinal"], 0);
        assert_eq!(value["payload"]["failure"]["code"], "timeout");
        assert_eq!(value["payload"]["failure"]["effect"], "unknown");
        assert!(value["payload"]["results"][0].get("arguments").is_none());
        assert!(value["payload"]["failure"].get("provider_error").is_none());
        assert!(
            !bytes
                .windows(b"provider raw error".len())
                .any(|w| w == b"provider raw error")
        );
    }

    #[test]
    fn before_and_after_snapshot_records_reject_scope_mismatch() {
        let binding = binding();
        let payload = ScopedConnectorSnapshotPayload::new(
            gateway_domain::ContextScopeId::new("other-project").unwrap(),
            br#"{"state":"scoped"}"#.to_vec(),
        )
        .unwrap();
        assert_eq!(
            before_snapshot_record(&binding, &payload),
            Err(SessionError::ScopeDenied)
        );
        assert_eq!(
            after_snapshot_record(&binding, &payload),
            Err(SessionError::ScopeDenied)
        );
    }

    #[test]
    fn typed_connector_failures_and_request_metadata_have_stable_safe_names() {
        use ConnectorFailureCode::*;
        let failures = [
            (Unavailable, "unavailable"),
            (AuthenticationFailed, "authentication_failed"),
            (CredentialUnavailable, "credential_unavailable"),
            (CredentialScopeDenied, "credential_scope_denied"),
            (SecretDisclosure, "secret_disclosure"),
            (PermissionDenied, "permission_denied"),
            (Timeout, "timeout"),
            (Cancelled, "cancelled"),
            (ProtocolError, "protocol_error"),
            (IncompatibleVersion, "incompatible_version"),
            (SchemaMismatch, "schema_mismatch"),
            (RateLimited, "rate_limited"),
            (NotFound, "not_found"),
            (StaleResult, "stale_result"),
            (ConflictingResult, "conflicting_result"),
            (MutationUncertain, "mutation_uncertain"),
            (RetryUnsafe, "retry_unsafe"),
            (ScopeMismatch, "scope_mismatch"),
            (InventoryChanged, "inventory_changed"),
            (BudgetExhausted, "budget_exhausted"),
            (Backpressure, "backpressure"),
        ];
        for (code, name) in failures {
            assert_eq!(connector_failure_name(code), name);
        }
        for (class, name) in [
            (ConnectorOperationClass::Read, "read"),
            (ConnectorOperationClass::Search, "search"),
            (ConnectorOperationClass::Inspect, "inspect"),
            (ConnectorOperationClass::Mutate, "mutate"),
            (ConnectorOperationClass::Admin, "admin"),
        ] {
            assert_eq!(operation_class_name(class), name);
        }
        for (basis, name) in [
            (
                ConnectorObservationBasis::ProviderAcknowledgement,
                "provider_acknowledgement",
            ),
            (ConnectorObservationBasis::SourceRead, "source_read"),
            (
                ConnectorObservationBasis::IndependentPostEffectRead,
                "independent_post_effect_read",
            ),
        ] {
            assert_eq!(observation_basis_name(basis), name);
        }
        for (effect, name) in [
            (ConnectorEffect::NotDispatched, "not_dispatched"),
            (ConnectorEffect::NoMutation, "no_mutation"),
            (ConnectorEffect::Acknowledged, "acknowledged"),
            (ConnectorEffect::Unknown, "unknown"),
        ] {
            assert_eq!(connector_effect_name(effect), name);
        }
    }

    #[test]
    fn proposal_failure_projection_keeps_pre_dispatch_and_connector_effects_distinct() {
        let correlation = gateway_domain::NonEmptyText::new("corr-safe").unwrap();
        for error in [
            ProposalError::StaleInvocation,
            ProposalError::InvalidProposal,
            ProposalError::PolicyDenied,
        ] {
            assert_eq!(safe_failure(&error).1, ConnectorEffect::NotDispatched);
            assert_eq!(
                proposal_failure_effect(&error),
                ConnectorEffect::NotDispatched
            );
            assert_eq!(safe_failure(&error).2, None);
        }
        let connector_error = ProposalError::Connector(ConnectorFailure {
            code: ConnectorFailureCode::MutationUncertain,
            effect: ConnectorEffect::Unknown,
            correlation: correlation.clone(),
        });
        assert_eq!(
            safe_failure(&connector_error).2.as_deref(),
            Some("corr-safe")
        );
        assert_eq!(
            proposal_failure_effect(&connector_error),
            ConnectorEffect::Unknown
        );
    }

    #[test]
    fn scoped_payload_and_record_envelopes_reject_malformed_or_oversized_bytes() {
        let scope = gateway_domain::ContextScopeId::new("project").unwrap();
        assert_eq!(
            ScopedConnectorSnapshotPayload::new(scope.clone(), vec![]),
            Err(SessionError::LimitExceeded)
        );
        assert_eq!(
            ScopedConnectorSnapshotPayload::new(scope.clone(), b"not-json".to_vec()),
            Err(SessionError::LimitExceeded)
        );
        assert_eq!(
            readback_record(&binding(), 0, b"not-json"),
            Err(SessionError::InvalidInput)
        );
        let payload = ScopedConnectorSnapshotPayload::new(
            scope,
            format!("\"{}\"", "x".repeat(MAX_CONNECTOR_RECORD_BYTES - 128)).into_bytes(),
        )
        .unwrap();
        assert_eq!(
            before_snapshot_record(&binding(), &payload),
            Err(SessionError::LimitExceeded)
        );
    }
}
