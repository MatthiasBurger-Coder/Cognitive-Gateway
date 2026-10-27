//! Strict, content-addressed readback records for restart-safe CG-14 verification.
//!
//! Connector arguments are never persisted here. Their complete typed request
//! identity is hashed and cross-linked to the proposal batch result. Normalized
//! CG-06 observations/facts/evidence are retained through the domain serializer.

use super::*;
use crate::{
    connector_results::validate_independent_post_effect_read,
    connectors::{
        ConnectorObservation, ConnectorObservationBasis, ConnectorOperationClass, ConnectorRequest,
    },
    external_context::{ScopedObservationBatch, SourceSnapshot},
};
use gateway_domain::{
    CapabilityId, ConditionId, ContentDigest, ContextScopeId, NonEmptyText, ObservationEvidenceSet,
    PlanStepId, SourceId, SourceKind, SourceTimestamp, TaskId, UnixTimestamp,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::str::FromStr;

pub const CONNECTOR_READBACK_CONTRACT: &str = "cg.connector-post-effect-readback";
pub const CONNECTOR_READBACK_VERSION: &str = "1.0";
pub const CONNECTOR_GOAL_READBACK_CONTRACT: &str = "cg.connector-goal-verification-readback";
pub const CONNECTOR_GOAL_READBACK_VERSION: &str = "1.0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedConnectorReadback {
    pub binding: ConnectorStepBinding,
    pub proposal_ordinal: Option<u32>,
    pub goal_condition: Option<ConditionId>,
    pub read_request: ConnectorRequestIdentity,
    pub original_mutation_request_digest: Option<ContentDigest>,
    pub observation: ConnectorObservation,
    pub batch: ScopedObservationBatch,
    /// Conditions covered by this exact read capability/source pair under the
    /// registered goal. This is reconstructed, never accepted from the caller.
    pub covered_conditions: Vec<ConditionId>,
}

/// Typed input for recording one final mapped source read.
pub struct GoalVerificationReadbackInput<'a> {
    pub condition: &'a ConditionId,
    pub read_request: &'a ConnectorRequest,
    pub observation: &'a ConnectorObservation,
    pub batch: &'a ScopedObservationBatch,
}

/// Safe identity for a connector request. Arguments are bound by digest so a
/// readback archive can prove exact request linkage without retaining values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorRequestIdentity {
    pub connector: String,
    pub scope: String,
    pub capability: String,
    pub operation_class: String,
    pub inventory_revision: String,
    pub arguments_digest: String,
    pub task: String,
    pub step: String,
    pub execution: String,
    pub correlation: String,
}

impl ConnectorRequestIdentity {
    /// Build the canonical content-free identity for one typed request. The
    /// digest binds every request field and the complete typed argument map.
    pub fn from_request(request: &ConnectorRequest) -> Result<Self, SessionError> {
        request_identity(request)
    }

    /// Validate the typed identity fields reconstructed from a persisted
    /// consent record. The digest itself is checked against a live request
    /// before use.
    pub fn validate(&self) -> Result<(), SessionError> {
        gateway_domain::NonEmptyText::new(self.connector.clone())
            .map_err(|_| SessionError::InvalidInput)?;
        ContextScopeId::new(self.scope.clone()).map_err(|_| SessionError::InvalidInput)?;
        CapabilityId::new(self.capability.clone()).map_err(|_| SessionError::InvalidInput)?;
        parse_operation_class(&self.operation_class)?;
        ContentDigest::new(self.inventory_revision.clone())
            .map_err(|_| SessionError::InvalidInput)?;
        ContentDigest::new(self.arguments_digest.clone())
            .map_err(|_| SessionError::InvalidInput)?;
        TaskId::new(self.task.clone()).map_err(|_| SessionError::InvalidInput)?;
        PlanStepId::new(self.step.clone()).map_err(|_| SessionError::InvalidInput)?;
        NonEmptyText::new(self.execution.clone()).map_err(|_| SessionError::InvalidInput)?;
        NonEmptyText::new(self.correlation.clone()).map_err(|_| SessionError::InvalidInput)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadbackEnvelope {
    contract: String,
    contract_version: String,
    binding: ConnectorStepBinding,
    payload: ReadbackPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadbackPayload {
    proposal_ordinal: u32,
    read_request: ConnectorRequestIdentity,
    original_mutation_request_digest: String,
    observation: ObservationWire,
    scoped_batch: ScopedBatchWire,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GoalReadbackEnvelope {
    contract: String,
    contract_version: String,
    binding: ConnectorStepBinding,
    payload: GoalReadbackPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GoalReadbackPayload {
    condition: String,
    read_request: ConnectorRequestIdentity,
    observation: ObservationWire,
    scoped_batch: ScopedBatchWire,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservationWire {
    task: String,
    step: String,
    execution: String,
    object_reference: String,
    source_kind: String,
    source_digest: Option<String>,
    observed_at: Option<i64>,
    authenticated_source: String,
    quality: Value,
    effect: String,
    partial: bool,
    verifies_request_digest: Option<String>,
    target_connector: String,
    target_scope: String,
    inventory_revision: String,
    correlation: String,
    source: String,
    source_revision: Option<String>,
    content_digest: String,
    acquired_at: i64,
    basis: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopedBatchWire {
    scope: String,
    source: String,
    source_kind: String,
    change_token: Option<String>,
    digest: Option<String>,
    records: String,
    quality_metadata: Vec<QualityWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QualityWire {
    subject: Vec<String>,
    metadata: Vec<Value>,
}

/// Hash every typed request field, including the complete canonical argument
/// map. The serialized preimage is transient and is never persisted or logged.
pub fn connector_request_digest(request: &ConnectorRequest) -> Result<ContentDigest, SessionError> {
    let arguments =
        serde_json::to_value(&request.arguments).map_err(|_| SessionError::InvalidInput)?;
    let preimage = serde_json::to_vec(&json!({
        "target":{"connector":request.target.connector.as_str(),"scope":request.target.scope.as_str()},
        "capability":request.capability.as_str(),
        "operation_class":operation_class(request.class),
        "inventory_revision":request.inventory_revision.as_str(),
        "arguments":arguments,
        "task":request.task.as_str(),
        "step":request.step.as_str(),
        "execution":request.execution.as_str(),
        "correlation":request.correlation.as_str(),
    }))
    .map_err(|_| SessionError::InvalidInput)?;
    ContentDigest::new(format!("{:x}", Sha256::digest(preimage)))
        .map_err(|_| SessionError::InvalidInput)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchResultEnvelope {
    contract: String,
    contract_version: String,
    binding: ConnectorStepBinding,
    payload: BatchResultPayload,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchResultPayload {
    invocation: String,
    all_calls_returned: bool,
    effect: String,
    results: Vec<BatchProposalIdentity>,
    failure: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchProposalIdentity {
    ordinal: u32,
    request_digest: String,
    capability: String,
    operation_class: String,
    inventory_revision: String,
    execution: String,
    correlation: String,
    observation: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedProposalRequestIdentity {
    pub digest: ContentDigest,
    pub execution: String,
    pub correlation: String,
}

/// Read the exact mutation request digest from the strict immutable batch
/// record. The coordinator uses this to bind a separately stored readback to
/// the proposal result at the same ordinal after a process restart.
pub fn mutation_request_digest_from_batch_record(
    bytes: &[u8],
    expected_binding: &ConnectorStepBinding,
    expected_ordinal: u32,
) -> Result<ContentDigest, SessionError> {
    Ok(
        proposal_request_identity_from_batch_record(bytes, expected_binding, expected_ordinal)?
            .digest,
    )
}

pub fn proposal_request_identity_from_batch_record(
    bytes: &[u8],
    expected_binding: &ConnectorStepBinding,
    expected_ordinal: u32,
) -> Result<PersistedProposalRequestIdentity, SessionError> {
    if bytes.is_empty() || bytes.len() > MAX_CONNECTOR_RECORD_BYTES {
        return Err(SessionError::LimitExceeded);
    }
    let envelope: BatchResultEnvelope =
        serde_json::from_slice(bytes).map_err(|_| SessionError::InvalidInteraction)?;
    if envelope.contract != "cg.connector-proposal-batch-result"
        || envelope.contract_version != "1.0"
        || envelope.binding != *expected_binding
        || envelope.payload.invocation != expected_binding.invocation.invocation_digest.as_str()
        || envelope.payload.results.len() > MAX_CONNECTOR_PROPOSALS_PER_STEP
        || envelope.payload.all_calls_returned != envelope.payload.failure.is_none()
        || !matches!(
            envelope.payload.effect.as_str(),
            "not_dispatched" | "no_mutation" | "acknowledged" | "unknown"
        )
        || envelope
            .payload
            .results
            .iter()
            .enumerate()
            .any(|(index, result)| result.ordinal as usize != index)
        || envelope
            .payload
            .results
            .iter()
            .filter(|result| result.ordinal == expected_ordinal)
            .count()
            != 1
    {
        return Err(SessionError::InvalidInteraction);
    }
    let result = envelope
        .payload
        .results
        .iter()
        .find(|result| result.ordinal == expected_ordinal)
        .ok_or(SessionError::Unavailable)?;
    if result.capability.is_empty()
        || result.inventory_revision.is_empty()
        || result.execution.is_empty()
        || result.correlation.is_empty()
        || result.observation.is_null()
        || !matches!(
            result.operation_class.as_str(),
            "mutate" | "admin" | "read" | "search" | "inspect"
        )
        || ContentDigest::new(result.request_digest.clone()).is_err()
    {
        return Err(SessionError::InvalidInteraction);
    }
    Ok(PersistedProposalRequestIdentity {
        digest: ContentDigest::new(result.request_digest.clone())
            .map_err(|_| SessionError::InvalidInput)?,
        execution: result.execution.clone(),
        correlation: result.correlation.clone(),
    })
}

fn request_identity(request: &ConnectorRequest) -> Result<ConnectorRequestIdentity, SessionError> {
    let identity = ConnectorRequestIdentity {
        connector: request.target.connector.as_str().to_owned(),
        scope: request.target.scope.as_str().to_owned(),
        capability: request.capability.as_str().to_owned(),
        operation_class: operation_class(request.class).to_owned(),
        inventory_revision: request.inventory_revision.as_str().to_owned(),
        arguments_digest: connector_request_digest(request)?.as_str().to_owned(),
        task: request.task.as_str().to_owned(),
        step: request.step.as_str().to_owned(),
        execution: request.execution.as_str().to_owned(),
        correlation: request.correlation.as_str().to_owned(),
    };
    identity.validate()?;
    Ok(identity)
}

/// Encode an independently validated post-effect readback into the canonical
/// immutable record format. The only untrusted bytes accepted are the already
/// normalized typed CG-06 records; raw provider payloads have no field here.
pub fn encode_connector_readback(
    binding: &ConnectorStepBinding,
    ordinal: u32,
    mutation: &ConnectorRequest,
    read_request: &ConnectorRequest,
    observation: &ConnectorObservation,
    batch: &ScopedObservationBatch,
    goal: &SupportedConnectorGoal,
) -> Result<Vec<u8>, SessionError> {
    validate_readback(
        binding,
        ordinal,
        mutation,
        read_request,
        observation,
        batch,
        goal,
    )?;
    let verified_mutation = observation
        .metadata
        .verifies
        .as_ref()
        .ok_or(SessionError::InvalidInteraction)?;
    let records = observation
        .records
        .to_json()
        .map_err(|_| SessionError::InvalidInput)?;
    let quality_metadata = batch
        .quality_metadata()
        .iter()
        .map(|(subject, metadata)| {
            Ok(QualityWire {
                subject: subject
                    .segments()
                    .iter()
                    .map(|segment| segment.as_str().to_owned())
                    .collect(),
                metadata: metadata
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| SessionError::InvalidInput)?,
            })
        })
        .collect::<Result<Vec<_>, SessionError>>()?;
    let payload = ReadbackPayload {
        proposal_ordinal: ordinal,
        read_request: request_identity(read_request)?,
        original_mutation_request_digest: connector_request_digest(verified_mutation)?
            .as_str()
            .to_owned(),
        observation: ObservationWire {
            task: observation.metadata.task.as_str().to_owned(),
            step: observation.metadata.step.as_str().to_owned(),
            execution: observation.metadata.execution.as_str().to_owned(),
            object_reference: observation.metadata.object_reference.as_str().to_owned(),
            source_kind: observation.metadata.source_kind.as_str().to_owned(),
            source_digest: observation
                .metadata
                .source_digest
                .as_ref()
                .map(|value| value.as_str().to_owned()),
            observed_at: observation.metadata.observed_at.map(UnixTimestamp::seconds),
            authenticated_source: observation
                .metadata
                .authenticated_source
                .as_str()
                .to_owned(),
            quality: serde_json::to_value(observation.metadata.quality)
                .map_err(|_| SessionError::InvalidInput)?,
            effect: effect_name(observation.metadata.effect).to_owned(),
            partial: observation.metadata.partial,
            verifies_request_digest: Some(
                connector_request_digest(verified_mutation)?
                    .as_str()
                    .to_owned(),
            ),
            target_connector: observation.target.connector.as_str().to_owned(),
            target_scope: observation.target.scope.as_str().to_owned(),
            inventory_revision: observation.inventory_revision.as_str().to_owned(),
            correlation: observation.correlation.as_str().to_owned(),
            source: observation.source.as_str().to_owned(),
            source_revision: observation
                .source_revision
                .as_ref()
                .map(|value| value.as_str().to_owned()),
            content_digest: observation.content_digest.as_str().to_owned(),
            acquired_at: observation.acquired_at.seconds(),
            basis: basis_name(observation.basis).to_owned(),
        },
        scoped_batch: ScopedBatchWire {
            scope: batch.scope().as_str().to_owned(),
            source: batch.snapshot().source_id().as_str().to_owned(),
            source_kind: batch.snapshot().source_kind().as_str().to_owned(),
            change_token: batch
                .snapshot()
                .change_token()
                .map(|value| value.as_str().to_owned()),
            digest: batch
                .snapshot()
                .digest()
                .map(|value| value.as_str().to_owned()),
            records,
            quality_metadata,
        },
    };
    let envelope = ReadbackEnvelope {
        contract: CONNECTOR_READBACK_CONTRACT.to_owned(),
        contract_version: CONNECTOR_READBACK_VERSION.to_owned(),
        binding: binding.clone(),
        payload,
    };
    let bytes = serde_json::to_vec(&envelope).map_err(|_| SessionError::InvalidInput)?;
    if bytes.len() > MAX_CONNECTOR_RECORD_BYTES {
        return Err(SessionError::LimitExceeded);
    }
    Ok(bytes)
}

/// Persist a final read-only source observation for one registered goal
/// condition. This record has no proposal ordinal or mutation link.
pub fn encode_goal_verification_readback(
    binding: &ConnectorStepBinding,
    condition: &ConditionId,
    read_request: &ConnectorRequest,
    observation: &ConnectorObservation,
    batch: &ScopedObservationBatch,
    goal: &SupportedConnectorGoal,
) -> Result<Vec<u8>, SessionError> {
    if binding.scope != *goal.scope()
        || binding.step != *goal.steps().last().ok_or(SessionError::InvalidInput)?
        || read_request.class != ConnectorOperationClass::Read
        || read_request.target.scope != *goal.scope()
        || read_request.task != binding.task
        || read_request.step != binding.step
        || observation.basis != ConnectorObservationBasis::SourceRead
        || observation.metadata.verifies.is_some()
        || observation.metadata.partial
        || observation.metadata.effect != crate::connectors::ConnectorEffect::NoMutation
        || observation.target != read_request.target
        || observation.metadata.task != binding.task
        || observation.metadata.step != binding.step
        || observation.metadata.execution != read_request.execution
        || observation.correlation != read_request.correlation
        || batch.scope() != goal.scope()
        || batch.records() != &observation.records
        || observation.content_digest
            != batch
                .snapshot()
                .digest()
                .cloned()
                .unwrap_or_else(|| observation.content_digest.clone())
        || observation.source != *batch.snapshot().source_id()
        || observation.metadata.source_kind != batch.snapshot().source_kind()
        || observation.source_revision != batch.snapshot().change_token().cloned()
    {
        return Err(SessionError::InvalidInteraction);
    }
    let mapped = mapped_conditions(
        goal,
        observation.source.as_str(),
        read_request.capability.as_str(),
        &observation.records,
    );
    if !mapped.contains(condition) {
        return Err(SessionError::ScopeDenied);
    }
    let payload = goal_source_payload(condition, read_request, observation, batch)?;
    // Use the distinct contract/payload shape; no mutation-link field is emitted.
    let envelope = GoalReadbackEnvelope {
        contract: CONNECTOR_GOAL_READBACK_CONTRACT.to_owned(),
        contract_version: CONNECTOR_GOAL_READBACK_VERSION.to_owned(),
        binding: binding.clone(),
        payload,
    };
    let bytes = serde_json::to_vec(&envelope).map_err(|_| SessionError::InvalidInput)?;
    if bytes.len() > MAX_CONNECTOR_RECORD_BYTES {
        return Err(SessionError::LimitExceeded);
    }
    Ok(bytes)
}

pub fn encode_goal_verification_input(
    binding: &ConnectorStepBinding,
    input: GoalVerificationReadbackInput<'_>,
    goal: &SupportedConnectorGoal,
) -> Result<Vec<u8>, SessionError> {
    encode_goal_verification_readback(
        binding,
        input.condition,
        input.read_request,
        input.observation,
        input.batch,
        goal,
    )
}

/// Decode a single final source read; complete-set coverage is checked separately.
pub fn decode_goal_verification_readback(
    repository: &impl SessionRepositoryPort,
    owner: &OwnerBinding,
    reference: &ConnectorObservationRef,
    goal: &SupportedConnectorGoal,
) -> Result<PersistedConnectorReadback, SessionError> {
    if reference.role != ConnectorObservationRole::GoalVerificationReadback
        || reference.proposal_ordinal.is_some()
        || reference.goal_condition.is_none()
        || reference.binding.owner != *owner
        || repository.record_kind(owner, &reference.record)? != RecordKind::ConnectorObservation
    {
        return Err(SessionError::InvalidInteraction);
    }
    let bytes = repository.load_artifact(owner, &reference.record)?;
    if bytes.is_empty()
        || bytes.len() > MAX_CONNECTOR_RECORD_BYTES
        || reference.record.digest() != format!("sha256:{:x}", Sha256::digest(&bytes))
    {
        return Err(SessionError::InvalidInteraction);
    }
    let envelope: GoalReadbackEnvelope =
        serde_json::from_slice(&bytes).map_err(|_| SessionError::InvalidInteraction)?;
    let condition = reference.goal_condition.as_ref().unwrap();
    if envelope.contract != CONNECTOR_GOAL_READBACK_CONTRACT
        || envelope.contract_version != CONNECTOR_GOAL_READBACK_VERSION
        || envelope.binding != reference.binding
        || envelope.binding.scope != *goal.scope()
        || envelope.binding.step != *goal.steps().last().ok_or(SessionError::InvalidInput)?
        || envelope.payload.condition != condition.as_str()
    {
        return Err(SessionError::InvalidInteraction);
    }
    let synthetic = ReadbackPayload {
        proposal_ordinal: 0,
        read_request: envelope.payload.read_request,
        original_mutation_request_digest: String::new(),
        observation: envelope.payload.observation,
        scoped_batch: envelope.payload.scoped_batch,
    };
    let (observation, batch) = decode_payload(&synthetic, goal, "source_read", None)?;
    let mapped = mapped_conditions(
        goal,
        observation.source.as_ref(),
        &synthetic.read_request.capability,
        &observation.records,
    );
    if !mapped.contains(condition) {
        return Err(SessionError::ScopeDenied);
    }
    Ok(PersistedConnectorReadback {
        binding: envelope.binding,
        proposal_ordinal: None,
        goal_condition: Some(condition.clone()),
        read_request: synthetic.read_request,
        original_mutation_request_digest: None,
        observation,
        batch,
        covered_conditions: mapped,
    })
}

pub fn validate_complete_goal_readback_set(
    goal: &SupportedConnectorGoal,
    readbacks: &[PersistedConnectorReadback],
) -> Result<(), SessionError> {
    let expected: std::collections::BTreeSet<_> = goal
        .intent()
        .desired_state()
        .conditions()
        .iter()
        .map(|c| c.id().clone())
        .collect();
    let actual: Vec<_> = readbacks
        .iter()
        .filter(|r| r.goal_condition.is_some())
        .filter_map(|r| r.goal_condition.clone())
        .collect();
    let unique: std::collections::BTreeSet<_> = actual.iter().cloned().collect();
    if actual.len() != readbacks.len() || unique.len() != actual.len() || unique != expected {
        return Err(SessionError::InvalidInteraction);
    }
    Ok(())
}

/// Canonical immutable final-read manifest, reconstructed from checkpoint truth.
/// Old final Evidence remains historical; this helper never upgrades receipts.
pub(crate) fn goal_verification_manifest(
    checkpoint: &SessionCheckpoint,
    last_step: &PlanStepId,
) -> Result<Vec<ConnectorObservationRef>, SessionError> {
    let goal = checkpoint
        .goal
        .as_connector()
        .ok_or(SessionError::InvalidState)?;
    if goal.steps().last() != Some(last_step) {
        return Err(SessionError::InvalidInteraction);
    }
    let mut references = checkpoint
        .connector_observations
        .iter()
        .filter(|reference| {
            reference.role == ConnectorObservationRole::GoalVerificationReadback
                && reference.binding.step == *last_step
        })
        .cloned()
        .collect::<Vec<_>>();
    if references.is_empty() || references.len() > MAX_CONNECTOR_GOAL_CONDITIONS {
        return Err(SessionError::InvalidInteraction);
    }
    references.sort_by(|a, b| a.goal_condition.cmp(&b.goal_condition));
    let binding = &references[0].binding;
    if binding.owner != checkpoint.owner
        || binding.scope != *goal.scope()
        || binding.session != checkpoint.snapshot.session
        || binding.run != checkpoint.snapshot.run
        || binding.revision > checkpoint.snapshot.revision
        || checkpoint.snapshot.dispatch.as_ref() != Some(&binding.dispatch)
        || references.iter().any(|r| {
            r.binding != *binding || r.proposal_ordinal.is_some() || r.goal_condition.is_none()
        })
        || !checkpoint.connector_results.iter().any(|r| {
            r.binding == *binding && r.after_snapshot_state == ConnectorAfterSnapshotState::Captured
        })
        || !checkpoint
            .connector_step_snapshots
            .iter()
            .any(|r| r.phase == ConnectorSnapshotPhase::After && r.binding == *binding)
    {
        return Err(SessionError::InvalidInteraction);
    }
    Ok(references)
}

/// Bind the exact decoded final reads to their actual persisted references.
/// Callers decode these references through `decode_goal_verification_readback`;
/// runtime and coordinator both reconstruct this same native readset without I/O.
pub fn build_goal_verification_readset(
    checkpoint: &SessionCheckpoint,
    last_step: &PlanStepId,
    readbacks: &[PersistedConnectorReadback],
) -> Result<crate::retained_observations::RetainedObservationReadset, SessionError> {
    use crate::retained_observations::{RetainedObservationMember, RetainedObservationReadset};
    let goal = checkpoint
        .goal
        .as_connector()
        .ok_or(SessionError::InvalidState)?;
    validate_complete_goal_readback_set(goal, readbacks)?;
    let references = goal_verification_manifest(checkpoint, last_step)?;
    if references.len() != readbacks.len() {
        return Err(SessionError::InvalidInteraction);
    }
    let mut members = Vec::with_capacity(references.len());
    for reference in &references {
        let condition = reference
            .goal_condition
            .as_ref()
            .ok_or(SessionError::InvalidInteraction)?;
        let readback = readbacks
            .iter()
            .find(|r| r.goal_condition.as_ref() == Some(condition))
            .ok_or(SessionError::InvalidInteraction)?;
        if readback.binding != reference.binding
            || readback.proposal_ordinal.is_some()
            || readback.original_mutation_request_digest.is_some()
            || !readback.covered_conditions.contains(condition)
            || readback.observation.basis != ConnectorObservationBasis::SourceRead
            || readback.observation.records != *readback.batch.records()
        {
            return Err(SessionError::InvalidInteraction);
        }
        let binding = gateway_domain::context_content_digest(
            &serde_json::to_vec(&("cg.session-goal-verification-member/2.0", reference))
                .map_err(|_| SessionError::InvalidInput)?,
        );
        // A version domain identifies the real source/object, not the record,
        // condition, acquisition correlation or source version being checked.
        let version_domain = gateway_domain::context_content_digest(
            &serde_json::to_vec(&(
                "cg.connector-source-object/1.0",
                goal.scope(),
                readback.observation.target.connector.as_str(),
                readback.observation.source.as_str(),
                readback.observation.metadata.source_kind.as_str(),
                readback.observation.metadata.object_reference.as_str(),
            ))
            .map_err(|_| SessionError::InvalidInput)?,
        );
        members.push(RetainedObservationMember {
            binding,
            version_domain,
            batch: readback.batch.clone(),
        });
    }
    RetainedObservationReadset::new(goal.scope().clone(), members)
        .map_err(|_| SessionError::InvalidInteraction)
}

/// Decode and validate a stored record against its immutable journal reference,
/// owner-scoped content address, registered goal, and proposal request digest.
pub fn decode_connector_readback(
    repository: &impl SessionRepositoryPort,
    owner: &OwnerBinding,
    reference: &ConnectorObservationRef,
    goal: &SupportedConnectorGoal,
    expected_mutation_request_digest: &ContentDigest,
) -> Result<PersistedConnectorReadback, SessionError> {
    if reference.role != ConnectorObservationRole::PostEffectReadback
        || reference.proposal_ordinal.is_none()
        || reference.goal_condition.is_some()
        || repository.record_kind(owner, &reference.record)? != RecordKind::ConnectorObservation
    {
        return Err(SessionError::InvalidInteraction);
    }
    let bytes = repository.load_artifact(owner, &reference.record)?;
    if bytes.is_empty()
        || bytes.len() > MAX_CONNECTOR_RECORD_BYTES
        || reference.record.digest() != format!("sha256:{:x}", Sha256::digest(&bytes))
    {
        return Err(SessionError::InvalidInteraction);
    }
    let envelope: ReadbackEnvelope =
        serde_json::from_slice(&bytes).map_err(|_| SessionError::InvalidInteraction)?;
    let binding = &reference.binding;
    if envelope.contract != CONNECTOR_READBACK_CONTRACT
        || envelope.contract_version != CONNECTOR_READBACK_VERSION
        || envelope.binding != *binding
        || binding.owner != *owner
        || binding.scope != *goal.scope()
        || binding.step != *goal.steps().last().ok_or(SessionError::InvalidInput)?
        || Some(envelope.payload.proposal_ordinal) != reference.proposal_ordinal
        || envelope.payload.original_mutation_request_digest
            != expected_mutation_request_digest.as_str()
        || envelope
            .payload
            .observation
            .verifies_request_digest
            .as_deref()
            != Some(expected_mutation_request_digest.as_str())
    {
        return Err(SessionError::InvalidInteraction);
    }
    let (observation, batch) = decode_payload(
        &envelope.payload,
        goal,
        "independent_post_effect_read",
        Some(expected_mutation_request_digest),
    )?;
    let covered_conditions = mapped_conditions(
        goal,
        &envelope.payload.observation.source,
        &envelope.payload.read_request.capability,
        &observation.records,
    );
    if covered_conditions.is_empty() {
        return Err(SessionError::ScopeDenied);
    }
    Ok(PersistedConnectorReadback {
        binding: binding.clone(),
        proposal_ordinal: Some(envelope.payload.proposal_ordinal),
        goal_condition: None,
        read_request: envelope.payload.read_request,
        original_mutation_request_digest: Some(expected_mutation_request_digest.clone()),
        observation,
        batch,
        covered_conditions,
    })
}

/// Owner-scoped journal loader that verifies the immutable result/readback
/// cross-link before returning normalized evidence to the CG-14 boundary.
pub fn load_connector_readback(
    repository: &impl SessionRepositoryPort,
    owner: &OwnerBinding,
    observation_ref: &ConnectorObservationRef,
    result_ref: &ConnectorResultRef,
    goal: &SupportedConnectorGoal,
) -> Result<PersistedConnectorReadback, SessionError> {
    let read_binding = &observation_ref.binding;
    let result_binding = &result_ref.binding;
    if read_binding.owner != *owner || result_binding.owner != *owner {
        return Err(SessionError::ScopeDenied);
    }
    if observation_ref.role != ConnectorObservationRole::PostEffectReadback
        || read_binding.scope != *goal.scope()
        || result_binding.scope != *goal.scope()
        || read_binding.session != result_binding.session
        || read_binding.run != result_binding.run
        || read_binding.task != result_binding.task
        || read_binding.step != result_binding.step
        || read_binding.dispatch != result_binding.dispatch
        || read_binding.invocation != result_binding.invocation
        || result_binding.revision > read_binding.revision
        || repository.record_kind(owner, &result_ref.record)? != RecordKind::ConnectorResult
    {
        return Err(SessionError::InvalidInteraction);
    }
    let result_bytes = repository.load_artifact(owner, &result_ref.record)?;
    if result_bytes.is_empty()
        || result_bytes.len() > MAX_CONNECTOR_RECORD_BYTES
        || result_ref.record.digest() != format!("sha256:{:x}", Sha256::digest(&result_bytes))
    {
        return Err(SessionError::InvalidInteraction);
    }
    let mutation_identity = proposal_request_identity_from_batch_record(
        &result_bytes,
        result_binding,
        observation_ref
            .proposal_ordinal
            .ok_or(SessionError::InvalidInteraction)?,
    )?;
    let readback = decode_connector_readback(
        repository,
        owner,
        observation_ref,
        goal,
        &mutation_identity.digest,
    )?;
    if readback.read_request.execution == mutation_identity.execution
        || readback.read_request.correlation == mutation_identity.correlation
    {
        return Err(SessionError::InvalidInteraction);
    }
    Ok(readback)
}

fn decode_payload(
    payload: &ReadbackPayload,
    goal: &SupportedConnectorGoal,
    expected_basis: &str,
    expected_mutation_digest: Option<&ContentDigest>,
) -> Result<(ConnectorObservation, ScopedObservationBatch), SessionError> {
    let o = &payload.observation;
    let read = &payload.read_request;
    let scope =
        ContextScopeId::new(o.target_scope.clone()).map_err(|_| SessionError::InvalidInput)?;
    let request_scope =
        ContextScopeId::new(read.scope.clone()).map_err(|_| SessionError::InvalidInput)?;
    let source = SourceId::new(o.source.clone()).map_err(|_| SessionError::InvalidInput)?;
    let source_kind =
        SourceKind::from_str(&o.source_kind).map_err(|_| SessionError::InvalidInput)?;
    let records = ObservationEvidenceSet::from_json(&payload.scoped_batch.records)
        .map_err(|_| SessionError::InvalidInteraction)?;
    let snapshot = SourceSnapshot::new(
        SourceId::new(payload.scoped_batch.source.clone())
            .map_err(|_| SessionError::InvalidInput)?,
        SourceKind::from_str(&payload.scoped_batch.source_kind)
            .map_err(|_| SessionError::InvalidInput)?,
        payload
            .scoped_batch
            .change_token
            .as_ref()
            .map(|v| SourceTimestamp::new(v.clone()).map_err(|_| SessionError::InvalidInput))
            .transpose()?,
        payload
            .scoped_batch
            .digest
            .as_ref()
            .map(|v| ContentDigest::new(v.clone()).map_err(|_| SessionError::InvalidInput))
            .transpose()?,
    )
    .map_err(|_| SessionError::InvalidInteraction)?;
    let batch_scope = ContextScopeId::new(payload.scoped_batch.scope.clone())
        .map_err(|_| SessionError::InvalidInput)?;
    let mut batch = ScopedObservationBatch::new(batch_scope, snapshot, records.clone())
        .map_err(|_| SessionError::InvalidInteraction)?;
    let mut quality_subjects = std::collections::BTreeSet::new();
    for quality in &payload.scoped_batch.quality_metadata {
        let subject = gateway_domain::SubjectPath::new(quality.subject.iter().map(String::as_str))
            .map_err(|_| SessionError::InvalidInteraction)?;
        if !quality_subjects.insert(subject.clone()) {
            return Err(SessionError::InvalidInteraction);
        }
        let values = quality
            .metadata
            .iter()
            .map(|value| {
                serde_json::from_value(value.clone()).map_err(|_| SessionError::InvalidInteraction)
            })
            .collect::<Result<Vec<gateway_domain::QualityMetadata>, SessionError>>()?;
        batch = batch.with_quality_metadata(subject, values);
    }
    let capability =
        CapabilityId::new(read.capability.clone()).map_err(|_| SessionError::InvalidInput)?;
    ContentDigest::new(read.arguments_digest.clone()).map_err(|_| SessionError::InvalidInput)?;
    let operation = parse_operation_class(&read.operation_class)?;
    let inventory_revision = ContentDigest::new(read.inventory_revision.clone())
        .map_err(|_| SessionError::InvalidInput)?;
    if read.connector.is_empty()
        || read.scope != goal.scope().as_str()
        || request_scope != scope
        || scope != *goal.scope()
        || read.task != o.task
        || read.step != o.step
        || read.execution != o.execution
        || read.correlation != o.correlation
        || read.inventory_revision != o.inventory_revision
        || read.connector != o.target_connector
        || read.capability.is_empty()
        || !matches!(
            operation,
            ConnectorOperationClass::Read
                | ConnectorOperationClass::Search
                | ConnectorOperationClass::Inspect
        )
        || o.basis != expected_basis
        || o.verifies_request_digest.as_deref()
            != expected_mutation_digest.map(ContentDigest::as_str)
        || o.effect != "no_mutation"
        || SourceKind::from_str(&payload.scoped_batch.source_kind)
            .map_err(|_| SessionError::InvalidInput)?
            != source_kind
        || payload.scoped_batch.source != source.as_str()
        || payload.scoped_batch.digest.as_deref() != Some(o.content_digest.as_str())
        || payload.scoped_batch.change_token != o.source_revision
        || payload.scoped_batch.scope != o.target_scope
        || mapped_conditions(goal, &o.source, capability.as_str(), &records).is_empty()
    {
        return Err(SessionError::InvalidInteraction);
    }
    let quality: gateway_domain::QualityMetadata =
        serde_json::from_value(o.quality.clone()).map_err(|_| SessionError::InvalidInteraction)?;
    if quality.trust() != gateway_domain::TrustClass::ObservedEvidence
        || quality.freshness() == gateway_domain::FreshnessStatus::Stale
        || quality.conflict() == gateway_domain::ConflictStatus::Unresolved
        || quality.uncertainty() == gateway_domain::Uncertainty::Unknown
        || o.partial
        || batch.records() != &records
    {
        return Err(SessionError::InvalidInteraction);
    }
    if batch.quality_metadata().values().flatten().any(|item| {
        item.trust() != gateway_domain::TrustClass::ObservedEvidence
            || item.freshness() == gateway_domain::FreshnessStatus::Stale
            || item.conflict() == gateway_domain::ConflictStatus::Unresolved
            || item.uncertainty() == gateway_domain::Uncertainty::Unknown
            || item.sensitivity() == gateway_domain::SensitivityClass::Secret
    }) {
        return Err(SessionError::InvalidInteraction);
    }
    let metadata = crate::connector_results::ConnectorResultMetadata {
        task: TaskId::new(o.task.clone()).map_err(|_| SessionError::InvalidInput)?,
        step: PlanStepId::new(o.step.clone()).map_err(|_| SessionError::InvalidInput)?,
        execution: NonEmptyText::new(o.execution.clone())
            .map_err(|_| SessionError::InvalidInput)?,
        object_reference: NonEmptyText::new(o.object_reference.clone())
            .map_err(|_| SessionError::InvalidInput)?,
        source_kind,
        source_digest: o
            .source_digest
            .as_ref()
            .map(|v| ContentDigest::new(v.clone()).map_err(|_| SessionError::InvalidInput))
            .transpose()?,
        observed_at: o.observed_at.map(UnixTimestamp::new),
        authenticated_source: NonEmptyText::new(o.authenticated_source.clone())
            .map_err(|_| SessionError::InvalidInput)?,
        quality,
        effect: parse_effect(&o.effect)?,
        partial: o.partial,
        verifies: None,
        non_authoritative_raw: None,
    };
    let observation = ConnectorObservation {
        metadata,
        target: crate::connectors::ConnectorTarget {
            connector: NonEmptyText::new(o.target_connector.clone())
                .map_err(|_| SessionError::InvalidInput)?,
            scope: scope.clone(),
        },
        inventory_revision,
        correlation: NonEmptyText::new(o.correlation.clone())
            .map_err(|_| SessionError::InvalidInput)?,
        source,
        source_revision: o
            .source_revision
            .as_ref()
            .map(|v| SourceTimestamp::new(v.clone()).map_err(|_| SessionError::InvalidInput))
            .transpose()?,
        content_digest: ContentDigest::new(o.content_digest.clone())
            .map_err(|_| SessionError::InvalidInput)?,
        acquired_at: UnixTimestamp::new(o.acquired_at),
        records,
        basis: match expected_basis {
            "source_read" => ConnectorObservationBasis::SourceRead,
            "independent_post_effect_read" => ConnectorObservationBasis::IndependentPostEffectRead,
            _ => return Err(SessionError::InvalidInteraction),
        },
    };
    Ok((observation, batch))
}

fn validate_readback(
    binding: &ConnectorStepBinding,
    ordinal: u32,
    mutation: &ConnectorRequest,
    read_request: &ConnectorRequest,
    observation: &ConnectorObservation,
    batch: &ScopedObservationBatch,
    goal: &SupportedConnectorGoal,
) -> Result<(), SessionError> {
    if binding.scope != *goal.scope()
        || binding.task != mutation.task
        || binding.step != mutation.step
        || binding.scope != mutation.target.scope
        || observation.metadata.verifies.as_ref() != Some(mutation)
        || observation.metadata.partial
        || observation.target.scope != *goal.scope()
        || batch.scope() != goal.scope()
        || batch.records() != &observation.records
        || mapped_conditions(
            goal,
            observation.source.as_str(),
            read_request.capability.as_str(),
            &observation.records,
        )
        .is_empty()
        || validate_independent_post_effect_read(mutation, read_request, observation).is_err()
        || ordinal > MAX_CONNECTOR_PROPOSALS_PER_STEP as u32
    {
        return Err(SessionError::InvalidInteraction);
    }
    if observation.content_digest
        != batch
            .snapshot()
            .digest()
            .cloned()
            .unwrap_or_else(|| observation.content_digest.clone())
        || observation.source != *batch.snapshot().source_id()
        || observation.metadata.source_kind != batch.snapshot().source_kind()
        || observation.source_revision != batch.snapshot().change_token().cloned()
    {
        return Err(SessionError::InvalidInteraction);
    }
    Ok(())
}

fn mapped_conditions(
    goal: &SupportedConnectorGoal,
    source: &str,
    capability: &str,
    records: &ObservationEvidenceSet,
) -> Vec<ConditionId> {
    goal.observations()
        .iter()
        .filter(|mapping| {
            if mapping.source().as_str() != source || mapping.capability().as_str() != capability {
                return false;
            }
            let Some(condition) = goal
                .intent()
                .desired_state()
                .conditions()
                .iter()
                .find(|condition| condition.id() == mapping.condition())
            else {
                return false;
            };
            records
                .observations()
                .iter()
                .any(|observation| observation.subject() == condition.subject())
                && records
                    .facts()
                    .iter()
                    .any(|fact| fact.subject() == condition.subject())
        })
        .map(|mapping| mapping.condition().clone())
        .collect()
}

fn operation_class(class: ConnectorOperationClass) -> &'static str {
    match class {
        ConnectorOperationClass::Read => "read",
        ConnectorOperationClass::Search => "search",
        ConnectorOperationClass::Inspect => "inspect",
        ConnectorOperationClass::Mutate => "mutate",
        ConnectorOperationClass::Admin => "admin",
    }
}

fn parse_operation_class(value: &str) -> Result<ConnectorOperationClass, SessionError> {
    match value {
        "read" => Ok(ConnectorOperationClass::Read),
        "search" => Ok(ConnectorOperationClass::Search),
        "inspect" => Ok(ConnectorOperationClass::Inspect),
        "mutate" => Ok(ConnectorOperationClass::Mutate),
        "admin" => Ok(ConnectorOperationClass::Admin),
        _ => Err(SessionError::InvalidInput),
    }
}

fn effect_name(effect: crate::connectors::ConnectorEffect) -> &'static str {
    match effect {
        crate::connectors::ConnectorEffect::NotDispatched => "not_dispatched",
        crate::connectors::ConnectorEffect::NoMutation => "no_mutation",
        crate::connectors::ConnectorEffect::Acknowledged => "acknowledged",
        crate::connectors::ConnectorEffect::Unknown => "unknown",
    }
}

fn parse_effect(value: &str) -> Result<crate::connectors::ConnectorEffect, SessionError> {
    match value {
        "not_dispatched" => Ok(crate::connectors::ConnectorEffect::NotDispatched),
        "no_mutation" => Ok(crate::connectors::ConnectorEffect::NoMutation),
        "acknowledged" => Ok(crate::connectors::ConnectorEffect::Acknowledged),
        "unknown" => Ok(crate::connectors::ConnectorEffect::Unknown),
        _ => Err(SessionError::InvalidInput),
    }
}

fn basis_name(basis: ConnectorObservationBasis) -> &'static str {
    match basis {
        ConnectorObservationBasis::ProviderAcknowledgement => "provider_acknowledgement",
        ConnectorObservationBasis::SourceRead => "source_read",
        ConnectorObservationBasis::IndependentPostEffectRead => "independent_post_effect_read",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        connector_proposals::{DispatchedProposal, ProposalBatch},
        connector_results::ConnectorResultMetadata,
        connectors::{ConnectorEffect, ConnectorObservationBasis, ConnectorTarget},
        sessions::{BindingId, ClientOwnerId, PrincipalId, ProjectId, WorkspaceId},
    };
    use gateway_domain::{
        AssertionPolarity, ComparisonOperator, ConditionExpression, Confidence, DesiredCondition,
        DesiredState, DesiredStateId, Evidence, EvidenceContent, EvidenceId, EvidenceKind,
        EvidenceLink, EvidenceRelation, Fact, FactId, FreshnessStatus, Intent, IntentId,
        Observation, ObservationId, PlanStepId, Provenance, ProvenanceId, SensitivityClass,
        SubjectPath, TrustClass, TypedValue, Uncertainty,
    };
    use std::{collections::BTreeMap, sync::Mutex};

    fn owner() -> OwnerBinding {
        OwnerBinding {
            principal: PrincipalId::new("operator").unwrap(),
            workspace: WorkspaceId::new("workspace").unwrap(),
            project: ProjectId::new("project").unwrap(),
            binding: BindingId::new("binding").unwrap(),
            client_owner: ClientOwnerId::new("client-owner").unwrap(),
        }
    }

    fn reference(id: &str, bytes: &[u8]) -> RecordRef {
        RecordRef::new(
            RecordId::new(id).unwrap(),
            RecordId::new("1").unwrap(),
            format!("sha256:{:x}", Sha256::digest(bytes)),
        )
        .unwrap()
    }

    fn binding() -> ConnectorStepBinding {
        ConnectorStepBinding {
            owner: owner(),
            scope: ContextScopeId::new("project-scope").unwrap(),
            session: SessionId::new("session").unwrap(),
            run: RunId::new("run").unwrap(),
            revision: Revision::new(5).unwrap(),
            task: TaskId::new("task").unwrap(),
            step: PlanStepId::new("step-2").unwrap(),
            dispatch: DispatchId::new("dispatch-2").unwrap(),
            invocation: gateway_domain::prompt_ir::InvocationIrIdentity {
                schema_version: gateway_domain::prompt_ir::INVOCATION_IR_VERSION,
                invocation_id: gateway_domain::ReferenceId::new("invocation-2").unwrap(),
                invocation_digest: ContentDigest::new("a".repeat(64)).unwrap(),
                handoff_digest: ContentDigest::new("b".repeat(64)).unwrap(),
                tools_digest: ContentDigest::new("c".repeat(64)).unwrap(),
                ir_digest: ContentDigest::new("d".repeat(64)).unwrap(),
            },
        }
    }

    fn goal() -> SupportedConnectorGoal {
        let condition = DesiredCondition::new(
            ConditionId::new("ready").unwrap(),
            SubjectPath::new(["system", "ready"]).unwrap(),
            ComparisonOperator::Equals,
            Some(TypedValue::Boolean(true)),
        )
        .unwrap();
        let desired = DesiredState::new(
            DesiredStateId::new("desired").unwrap(),
            vec![condition.clone()],
            ConditionExpression::condition(condition.id().clone()),
            vec![],
            vec![],
        )
        .unwrap();
        SupportedConnectorGoal::from_host_registration(
            Intent::new(IntentId::new("intent").unwrap(), desired),
            ContextScopeId::new("project-scope").unwrap(),
            reference("plan", b"plan"),
            vec![
                PlanStepId::new("step-1").unwrap(),
                PlanStepId::new("step-2").unwrap(),
            ],
            vec![
                ConditionObservationMapping::new(
                    condition.id().clone(),
                    SourceId::new("registered-state").unwrap(),
                    CapabilityId::new("connector.read-state").unwrap(),
                    ConnectorOperationClass::Read,
                )
                .unwrap(),
            ],
        )
        .unwrap()
    }

    fn requests() -> (ConnectorRequest, ConnectorRequest) {
        let target = ConnectorTarget {
            connector: NonEmptyText::new("state-connector").unwrap(),
            scope: ContextScopeId::new("project-scope").unwrap(),
        };
        let task = TaskId::new("task").unwrap();
        let step = PlanStepId::new("step-2").unwrap();
        let mutation = ConnectorRequest {
            target: target.clone(),
            capability: CapabilityId::new("connector.write-state").unwrap(),
            class: ConnectorOperationClass::Mutate,
            inventory_revision: ContentDigest::new("1".repeat(64)).unwrap(),
            arguments: BTreeMap::from([(
                NonEmptyText::new("value").unwrap(),
                TypedValue::string("sensitive typed argument").unwrap(),
            )]),
            task: task.clone(),
            step: step.clone(),
            execution: NonEmptyText::new("mutation-execution").unwrap(),
            correlation: NonEmptyText::new("mutation-correlation").unwrap(),
        };
        let read = ConnectorRequest {
            target,
            capability: CapabilityId::new("connector.read-state").unwrap(),
            class: ConnectorOperationClass::Read,
            inventory_revision: mutation.inventory_revision.clone(),
            arguments: BTreeMap::new(),
            task,
            step,
            execution: NonEmptyText::new("independent-read-execution").unwrap(),
            correlation: NonEmptyText::new("independent-read-correlation").unwrap(),
        };
        (mutation, read)
    }

    fn observed(
        mutation: &ConnectorRequest,
        read: &ConnectorRequest,
        goal: &SupportedConnectorGoal,
    ) -> (ConnectorObservation, ScopedObservationBatch) {
        let source = SourceId::new("registered-state").unwrap();
        let source_revision = SourceTimestamp::new("revision-11").unwrap();
        let acquired = UnixTimestamp::new(100);
        let provenance = Provenance::new(
            ProvenanceId::new("read-provenance").unwrap(),
            SourceKind::Tool,
            source.clone(),
            "connector://state/1",
        )
        .unwrap()
        .with_producer("trusted-connector-host")
        .unwrap()
        .with_acquired_at(SourceTimestamp::new("100").unwrap())
        .with_source_timestamp(source_revision.clone());
        let subject = goal.intent().desired_state().conditions()[0]
            .subject()
            .clone();
        let observation = Observation::new(
            ObservationId::new("read-observation").unwrap(),
            subject.clone(),
            TypedValue::Boolean(true),
            provenance.id().clone(),
        )
        .unwrap();
        let fact = Fact::new(
            FactId::new("read-fact").unwrap(),
            subject.clone(),
            TypedValue::Boolean(true),
            AssertionPolarity::Affirmed,
            vec![observation.id().clone()],
        )
        .unwrap();
        let evidence = Evidence::new(
            EvidenceId::new("read-evidence").unwrap(),
            EvidenceKind::Report,
            "Read registered state",
            EvidenceContent::inline("The registered state is ready.").unwrap(),
            provenance.id().clone(),
            vec![EvidenceLink::new(
                fact.id().clone(),
                EvidenceRelation::Supports,
            )],
        )
        .unwrap();
        let records = ObservationEvidenceSet::new(
            vec![provenance],
            vec![observation],
            vec![fact],
            vec![evidence],
        )
        .unwrap();
        let content_digest = ContentDigest::new("e".repeat(64)).unwrap();
        let quality = gateway_domain::QualityMetadata::new(
            TrustClass::ObservedEvidence,
            SensitivityClass::Public,
            Confidence::score(0.95).unwrap(),
            FreshnessStatus::Fresh,
            Uncertainty::None,
        );
        let batch = ScopedObservationBatch::new(
            goal.scope().clone(),
            SourceSnapshot::new(
                source.clone(),
                SourceKind::Tool,
                Some(source_revision.clone()),
                Some(content_digest.clone()),
            )
            .unwrap(),
            records.clone(),
        )
        .unwrap()
        .with_quality_metadata(subject, vec![quality]);
        let metadata = ConnectorResultMetadata {
            task: read.task.clone(),
            step: read.step.clone(),
            execution: read.execution.clone(),
            object_reference: NonEmptyText::new("connector://state/1").unwrap(),
            source_kind: SourceKind::Tool,
            source_digest: Some(content_digest.clone()),
            observed_at: Some(acquired),
            authenticated_source: NonEmptyText::new("trusted-connector-host").unwrap(),
            quality,
            effect: ConnectorEffect::NoMutation,
            partial: false,
            verifies: Some(mutation.clone()),
            non_authoritative_raw: None,
        };
        let observation = ConnectorObservation {
            metadata,
            target: read.target.clone(),
            inventory_revision: read.inventory_revision.clone(),
            correlation: read.correlation.clone(),
            source,
            source_revision: Some(source_revision),
            content_digest,
            acquired_at: acquired,
            records,
            basis: ConnectorObservationBasis::IndependentPostEffectRead,
        };
        (observation, batch)
    }

    #[derive(Default)]
    struct Repository {
        owner: Mutex<Option<OwnerBinding>>,
        records: Mutex<BTreeMap<String, (RecordKind, Vec<u8>)>>,
        result_ref: Mutex<Option<ConnectorResultRef>>,
    }

    impl Repository {
        fn put(&self, owner: &OwnerBinding, record: RecordRef, kind: RecordKind, bytes: Vec<u8>) {
            self.owner
                .lock()
                .unwrap()
                .get_or_insert_with(|| owner.clone());
            self.records
                .lock()
                .unwrap()
                .insert(record.id().as_str().to_owned(), (kind, bytes));
        }
    }

    impl ArtifactStoragePort for Repository {
        fn load_artifact(
            &self,
            owner: &OwnerBinding,
            reference: &RecordRef,
        ) -> Result<Vec<u8>, SessionError> {
            if self.owner.lock().unwrap().as_ref() != Some(owner) {
                return Err(SessionError::ScopeDenied);
            }
            self.records
                .lock()
                .unwrap()
                .get(reference.id().as_str())
                .map(|(_, bytes)| bytes.clone())
                .ok_or(SessionError::Unavailable)
        }
    }

    impl SessionJournalPort for Repository {
        fn load(&self, _: &OwnerBinding, _: &SessionId) -> Result<FencedSession, SessionError> {
            Err(SessionError::Unavailable)
        }
        fn command_outcome(&self, _: &CommandKey) -> Result<CommandOutcome, SessionError> {
            Err(SessionError::Unavailable)
        }
        fn create(
            &self,
            _: &CommandId,
            _: SessionCheckpoint,
        ) -> Result<FencedSession, SessionError> {
            Err(SessionError::Unavailable)
        }
        fn append(
            &self,
            _: &OwnerBinding,
            _: JournalAppend,
        ) -> Result<FencedSession, SessionError> {
            Err(SessionError::Unavailable)
        }
    }

    impl SessionRepositoryPort for Repository {
        fn record_kind(
            &self,
            owner: &OwnerBinding,
            reference: &RecordRef,
        ) -> Result<RecordKind, SessionError> {
            if self.owner.lock().unwrap().as_ref() != Some(owner) {
                return Err(SessionError::ScopeDenied);
            }
            self.records
                .lock()
                .unwrap()
                .get(reference.id().as_str())
                .map(|(kind, _)| *kind)
                .ok_or(SessionError::Unavailable)
        }
    }

    fn write_records(
        repo: &Repository,
        goal: &SupportedConnectorGoal,
        binding: &ConnectorStepBinding,
        mutation: &ConnectorRequest,
        read: &ConnectorRequest,
    ) -> ConnectorObservationRef {
        let (observation, batch) = observed(mutation, read, goal);
        let read_bytes =
            encode_connector_readback(binding, 0, mutation, read, &observation, &batch, goal)
                .unwrap();
        let read_ref = reference("readback", &read_bytes);
        let observation_ref = ConnectorObservationRef {
            binding: binding.clone(),
            proposal_ordinal: Some(0),
            goal_condition: None,
            role: ConnectorObservationRole::PostEffectReadback,
            record: read_ref.clone(),
        };
        let dispatched = observation.clone();
        let proposal_batch = ProposalBatch {
            invocation: binding.invocation.invocation_digest.clone(),
            all_calls_returned: true,
            results: vec![DispatchedProposal {
                ordinal: 0,
                request: mutation.clone(),
                observation: dispatched,
            }],
            failure: None,
        };
        let batch_bytes = crate::sessions::proposal_batch_record(binding, &proposal_batch).unwrap();
        let result_ref = reference("result", &batch_bytes);
        let connector_result_ref = ConnectorResultRef {
            binding: binding.clone(),
            record: result_ref.clone(),
            after_snapshot_state: ConnectorAfterSnapshotState::Unknown,
        };
        repo.put(
            &binding.owner,
            read_ref,
            RecordKind::ConnectorObservation,
            read_bytes,
        );
        repo.put(
            &binding.owner,
            result_ref,
            RecordKind::ConnectorResult,
            batch_bytes,
        );
        *repo.result_ref.lock().unwrap() = Some(connector_result_ref);
        observation_ref
    }

    #[test]
    fn canonical_readback_round_trips_and_links_exact_mutation_request_digest() {
        let goal = goal();
        let binding = binding();
        let (mutation, read) = requests();
        let repo = Repository::default();
        let observation_ref = write_records(&repo, &goal, &binding, &mutation, &read);
        let result_ref = repo.result_ref.lock().unwrap().clone().unwrap();
        let loaded =
            load_connector_readback(&repo, &binding.owner, &observation_ref, &result_ref, &goal)
                .unwrap();
        assert_eq!(loaded.binding, binding);
        assert_eq!(loaded.proposal_ordinal, Some(0));
        assert_eq!(loaded.read_request.capability, "connector.read-state");
        assert_eq!(
            loaded.original_mutation_request_digest,
            Some(connector_request_digest(&mutation).unwrap())
        );
        assert_eq!(
            loaded.covered_conditions,
            vec![goal.intent().desired_state().conditions()[0].id().clone()]
        );
        assert_eq!(loaded.batch.records().facts().len(), 1);
        let bytes = repo.records.lock().unwrap()["readback"].1.clone();
        assert!(
            !bytes
                .windows(b"sensitive typed argument".len())
                .any(|w| w == b"sensitive typed argument")
        );
    }

    #[test]
    fn goal_verification_readback_is_source_read_and_covers_complete_condition_set() {
        let goal = goal();
        let binding = binding();
        let (mutation, read) = requests();
        let (mut observation, batch) = observed(&mutation, &read, &goal);
        observation.basis = ConnectorObservationBasis::SourceRead;
        observation.metadata.verifies = None;
        let condition = goal.intent().desired_state().conditions()[0].id().clone();
        let bytes = encode_goal_verification_readback(
            &binding,
            &condition,
            &read,
            &observation,
            &batch,
            &goal,
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["contract"], CONNECTOR_GOAL_READBACK_CONTRACT);
        assert!(value["payload"]["condition"].is_string());
        assert!(value["payload"]["proposal_ordinal"].is_null());
        assert!(value["payload"]["original_mutation_request_digest"].is_null());
        let record = reference("goal-read", &bytes);
        let reference = ConnectorObservationRef {
            binding: binding.clone(),
            proposal_ordinal: None,
            goal_condition: Some(condition.clone()),
            role: ConnectorObservationRole::GoalVerificationReadback,
            record: record.clone(),
        };
        let repo = Repository::default();
        repo.put(
            &binding.owner,
            record,
            RecordKind::ConnectorObservation,
            bytes,
        );
        let loaded =
            decode_goal_verification_readback(&repo, &binding.owner, &reference, &goal).unwrap();
        assert_eq!(loaded.goal_condition, Some(condition));
        assert_eq!(
            loaded.observation.basis,
            ConnectorObservationBasis::SourceRead
        );
        assert_eq!(loaded.original_mutation_request_digest, None);
        validate_complete_goal_readback_set(&goal, &[loaded]).unwrap();
    }

    #[test]
    fn rehashed_source_and_posteffect_readbacks_cannot_relabel_snapshot_version() {
        let goal = goal();
        let binding = binding();
        let (mutation, read) = requests();
        let condition = goal.intent().desired_state().conditions()[0].id().clone();
        for source_read in [false, true] {
            let (mut observation, batch) = observed(&mutation, &read, &goal);
            let bytes = if source_read {
                observation.basis = ConnectorObservationBasis::SourceRead;
                observation.metadata.verifies = None;
                encode_goal_verification_readback(
                    &binding,
                    &condition,
                    &read,
                    &observation,
                    &batch,
                    &goal,
                )
                .unwrap()
            } else {
                encode_connector_readback(
                    &binding,
                    0,
                    &mutation,
                    &read,
                    &observation,
                    &batch,
                    &goal,
                )
                .unwrap()
            };
            for token in [json!("forged-source-version"), Value::Null] {
                let mut value: Value = serde_json::from_slice(&bytes).unwrap();
                value["payload"]["scoped_batch"]["change_token"] = token;
                let bytes = serde_json::to_vec(&value).unwrap();
                let record = reference("relabelled-version", &bytes);
                let reference = ConnectorObservationRef {
                    binding: binding.clone(),
                    proposal_ordinal: if source_read { None } else { Some(0) },
                    goal_condition: if source_read {
                        Some(condition.clone())
                    } else {
                        None
                    },
                    role: if source_read {
                        ConnectorObservationRole::GoalVerificationReadback
                    } else {
                        ConnectorObservationRole::PostEffectReadback
                    },
                    record: record.clone(),
                };
                let repo = Repository::default();
                repo.put(
                    &binding.owner,
                    record,
                    RecordKind::ConnectorObservation,
                    bytes,
                );
                let result = if source_read {
                    decode_goal_verification_readback(&repo, &binding.owner, &reference, &goal)
                } else {
                    decode_connector_readback(
                        &repo,
                        &binding.owner,
                        &reference,
                        &goal,
                        &connector_request_digest(&mutation).unwrap(),
                    )
                };
                assert_eq!(result, Err(SessionError::InvalidInteraction));
            }
        }
    }

    #[test]
    fn duplicate_subject_quality_cannot_overwrite_weak_metadata_with_fresh() {
        let goal = goal();
        let binding = binding();
        let (mutation, read) = requests();
        let (mut observation, batch) = observed(&mutation, &read, &goal);
        observation.basis = ConnectorObservationBasis::SourceRead;
        observation.metadata.verifies = None;
        let condition = goal.intent().desired_state().conditions()[0].id().clone();
        let bytes = encode_goal_verification_readback(
            &binding,
            &condition,
            &read,
            &observation,
            &batch,
            &goal,
        )
        .unwrap();
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        let fresh = value["payload"]["scoped_batch"]["quality_metadata"][0].clone();
        let mut weak = fresh.clone();
        weak["metadata"] = json!([gateway_domain::QualityMetadata::new(
            TrustClass::ObservedEvidence,
            SensitivityClass::Public,
            Confidence::Unknown,
            FreshnessStatus::Stale,
            Uncertainty::None,
        )]);
        value["payload"]["scoped_batch"]["quality_metadata"] = json!([weak, fresh]);
        let bytes = serde_json::to_vec(&value).unwrap();
        let record = reference("duplicate-quality-read", &bytes);
        let reference = ConnectorObservationRef {
            binding: binding.clone(),
            proposal_ordinal: None,
            goal_condition: Some(condition),
            role: ConnectorObservationRole::GoalVerificationReadback,
            record: record.clone(),
        };
        let repository = Repository::default();
        repository.put(
            &binding.owner,
            record,
            RecordKind::ConnectorObservation,
            bytes,
        );
        assert_eq!(
            decode_goal_verification_readback(&repository, &binding.owner, &reference, &goal),
            Err(SessionError::InvalidInteraction)
        );
    }

    #[test]
    fn shared_source_batch_can_cover_multiple_conditions_but_final_set_stays_exact() {
        let original = goal();
        let first = original.intent().desired_state().conditions()[0].clone();
        let second = DesiredCondition::new(
            ConditionId::new("also-ready").unwrap(),
            first.subject().clone(),
            ComparisonOperator::Equals,
            Some(TypedValue::Boolean(true)),
        )
        .unwrap();
        let desired = DesiredState::new(
            DesiredStateId::new("shared-desired").unwrap(),
            vec![first.clone(), second.clone()],
            ConditionExpression::all(vec![
                ConditionExpression::condition(first.id().clone()),
                ConditionExpression::condition(second.id().clone()),
            ])
            .unwrap(),
            vec![],
            vec![],
        )
        .unwrap();
        let goal = SupportedConnectorGoal::from_host_registration(
            Intent::new(IntentId::new("shared-intent").unwrap(), desired),
            original.scope().clone(),
            original.plan().clone(),
            original.steps().to_vec(),
            [first.id(), second.id()]
                .into_iter()
                .map(|condition| {
                    ConditionObservationMapping::new(
                        condition.clone(),
                        SourceId::new("registered-state").unwrap(),
                        CapabilityId::new("connector.read-state").unwrap(),
                        ConnectorOperationClass::Read,
                    )
                    .unwrap()
                })
                .collect(),
        )
        .unwrap();
        let binding = binding();
        let (mutation, read) = requests();
        let (mut observation, batch) = observed(&mutation, &read, &goal);
        observation.basis = ConnectorObservationBasis::SourceRead;
        observation.metadata.verifies = None;
        let repo = Repository::default();
        let mut readbacks = Vec::new();
        for (ordinal, condition) in goal
            .intent()
            .desired_state()
            .conditions()
            .iter()
            .enumerate()
        {
            let bytes = encode_goal_verification_readback(
                &binding,
                condition.id(),
                &read,
                &observation,
                &batch,
                &goal,
            )
            .unwrap();
            let record = reference(&format!("shared-goal-read-{ordinal}"), &bytes);
            let reference = ConnectorObservationRef {
                binding: binding.clone(),
                proposal_ordinal: None,
                goal_condition: Some(condition.id().clone()),
                role: ConnectorObservationRole::GoalVerificationReadback,
                record: record.clone(),
            };
            repo.put(
                &binding.owner,
                record,
                RecordKind::ConnectorObservation,
                bytes,
            );
            let loaded =
                decode_goal_verification_readback(&repo, &binding.owner, &reference, &goal)
                    .unwrap();
            assert_eq!(loaded.covered_conditions.len(), 2);
            readbacks.push(loaded);
        }
        validate_complete_goal_readback_set(&goal, &readbacks).unwrap();
        assert!(validate_complete_goal_readback_set(&goal, &readbacks[..1]).is_err());
        assert!(
            validate_complete_goal_readback_set(
                &goal,
                &[readbacks[0].clone(), readbacks[0].clone()]
            )
            .is_err()
        );
        assert!(
            encode_goal_verification_readback(
                &binding,
                &ConditionId::new("unmapped").unwrap(),
                &read,
                &observation,
                &batch,
                &goal
            )
            .is_err()
        );
    }

    #[test]
    fn readback_reload_rejects_missing_corrupt_wrong_owner_and_wrong_capability() {
        let goal = goal();
        let binding = binding();
        let (mutation, read) = requests();
        let repo = Repository::default();
        let observation_ref = write_records(&repo, &goal, &binding, &mutation, &read);
        let result_ref = repo.result_ref.lock().unwrap().clone().unwrap();
        let foreign = OwnerBinding {
            project: ProjectId::new("other-project").unwrap(),
            ..binding.owner.clone()
        };
        assert_eq!(
            load_connector_readback(&repo, &foreign, &observation_ref, &result_ref, &goal),
            Err(SessionError::ScopeDenied)
        );
        let wrong_capability = ConnectorRequest {
            capability: CapabilityId::new("connector.other-read").unwrap(),
            ..read.clone()
        };
        let (observation, batch) = observed(&mutation, &wrong_capability, &goal);
        assert_eq!(
            encode_connector_readback(
                &binding,
                0,
                &mutation,
                &wrong_capability,
                &observation,
                &batch,
                &goal
            ),
            Err(SessionError::InvalidInteraction)
        );
        let mut corrupt = repo.records.lock().unwrap();
        let record = corrupt.get_mut("readback").unwrap();
        record.1[0] ^= 0x01;
        drop(corrupt);
        assert_eq!(
            load_connector_readback(&repo, &binding.owner, &observation_ref, &result_ref, &goal),
            Err(SessionError::InvalidInteraction)
        );
        repo.records.lock().unwrap().remove("readback");
        assert_eq!(
            load_connector_readback(&repo, &binding.owner, &observation_ref, &result_ref, &goal),
            Err(SessionError::Unavailable)
        );
    }
}

fn goal_source_payload(
    condition: &ConditionId,
    read_request: &ConnectorRequest,
    observation: &ConnectorObservation,
    batch: &ScopedObservationBatch,
) -> Result<GoalReadbackPayload, SessionError> {
    let records = observation
        .records
        .to_json()
        .map_err(|_| SessionError::InvalidInput)?;
    let quality_metadata = batch
        .quality_metadata()
        .iter()
        .map(|(subject, metadata)| {
            Ok(QualityWire {
                subject: subject
                    .segments()
                    .iter()
                    .map(|s| s.as_str().to_owned())
                    .collect(),
                metadata: metadata
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| SessionError::InvalidInput)?,
            })
        })
        .collect::<Result<Vec<_>, SessionError>>()?;
    let wire = ObservationWire {
        task: observation.metadata.task.as_str().to_owned(),
        step: observation.metadata.step.as_str().to_owned(),
        execution: observation.metadata.execution.as_str().to_owned(),
        object_reference: observation.metadata.object_reference.as_str().to_owned(),
        source_kind: observation.metadata.source_kind.as_str().to_owned(),
        source_digest: observation
            .metadata
            .source_digest
            .as_ref()
            .map(|v| v.as_str().to_owned()),
        observed_at: observation.metadata.observed_at.map(UnixTimestamp::seconds),
        authenticated_source: observation
            .metadata
            .authenticated_source
            .as_str()
            .to_owned(),
        quality: serde_json::to_value(observation.metadata.quality)
            .map_err(|_| SessionError::InvalidInput)?,
        effect: effect_name(observation.metadata.effect).to_owned(),
        partial: observation.metadata.partial,
        verifies_request_digest: None,
        target_connector: observation.target.connector.as_str().to_owned(),
        target_scope: observation.target.scope.as_str().to_owned(),
        inventory_revision: observation.inventory_revision.as_str().to_owned(),
        correlation: observation.correlation.as_str().to_owned(),
        source: observation.source.as_str().to_owned(),
        source_revision: observation
            .source_revision
            .as_ref()
            .map(|v| v.as_str().to_owned()),
        content_digest: observation.content_digest.as_str().to_owned(),
        acquired_at: observation.acquired_at.seconds(),
        basis: basis_name(observation.basis).to_owned(),
    };
    let batch_wire = ScopedBatchWire {
        scope: batch.scope().as_str().to_owned(),
        source: batch.snapshot().source_id().as_str().to_owned(),
        source_kind: batch.snapshot().source_kind().as_str().to_owned(),
        change_token: batch
            .snapshot()
            .change_token()
            .map(|v| v.as_str().to_owned()),
        digest: batch.snapshot().digest().map(|v| v.as_str().to_owned()),
        records,
        quality_metadata,
    };
    Ok(GoalReadbackPayload {
        condition: condition.as_str().to_owned(),
        read_request: request_identity(read_request)?,
        observation: wire,
        scoped_batch: batch_wire,
    })
}
pub(super) fn encode_pre_execution_source(
    condition: &ConditionId,
    request: &ConnectorRequest,
    observation: &ConnectorObservation,
    batch: &ScopedObservationBatch,
) -> Result<Value, SessionError> {
    crate::connector_results::ConnectorResult::validate_for_request(request, observation)
        .map_err(|_| SessionError::InvalidInteraction)?;
    serde_json::to_value(goal_source_payload(condition, request, observation, batch)?)
        .map_err(|_| SessionError::InvalidInput)
}
pub(super) fn decode_pre_execution_source(
    value: Value,
    goal: &SupportedConnectorGoal,
) -> Result<
    (
        ConditionId,
        ConnectorRequestIdentity,
        ConnectorObservation,
        ScopedObservationBatch,
    ),
    SessionError,
> {
    let p: GoalReadbackPayload =
        serde_json::from_value(value).map_err(|_| SessionError::InvalidInteraction)?;
    let condition = ConditionId::new(p.condition).map_err(|_| SessionError::InvalidInput)?;
    let request = p.read_request.clone();
    let payload = ReadbackPayload {
        proposal_ordinal: 0,
        read_request: p.read_request,
        original_mutation_request_digest: String::new(),
        observation: p.observation,
        scoped_batch: p.scoped_batch,
    };
    let (observation, batch) = decode_payload(&payload, goal, "source_read", None)?;
    if !mapped_conditions(
        goal,
        observation.source.as_str(),
        &request.capability,
        &observation.records,
    )
    .contains(&condition)
    {
        return Err(SessionError::ScopeDenied);
    }
    Ok((condition, request, observation, batch))
}
