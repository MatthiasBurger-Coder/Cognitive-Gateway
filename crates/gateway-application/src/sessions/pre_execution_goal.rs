//! Invocation-free observed-goal data. A candidate never grants an effect or
//! completes a session: the coordinator persists, reloads and verifies it again.
use super::*;
use crate::{
    closed_loop::{ClosedLoop, VerifiedOutcomeReceipt},
    connectors::{ConnectorObservation, ConnectorRequest},
    experience_patterns::OutcomeClass,
    external_context::ScopedObservationBatch,
    retained_observations::{RetainedObservationMember, RetainedObservationReadset},
};
use gateway_domain::{
    ConditionId, Confidence, ConflictStatus, ContentDigest, FreshnessStatus, NonEmptyText,
    QualityMetadata, ReferenceId, ResolvedReference, SensitivityClass, TaskId, TrustClass,
    Uncertainty,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub type PreExecutionGoalPin = ResolvedReference<ReferenceId>;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreExecutionGoalBinding {
    pub acquisition: ResourceReservationBinding,
    pub reservation: ContentDigest,
    pub claim_revision: Revision,
    pub task: TaskId,
    pub execution: NonEmptyText,
    pub context_digest: ContentDigest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreExecutionGoalRef {
    pub binding: PreExecutionGoalBinding,
    pub committed_revision: Revision,
    pub record: RecordRef,
    pub goal_digest: ContentDigest,
    pub readset_digest: ContentDigest,
    pub definition_pins: Vec<PreExecutionGoalPin>,
    pub valid_until_ms: u64,
}
impl PreExecutionGoalRef {
    pub(crate) fn validate_for(&self, cp: &SessionCheckpoint) -> Result<(), SessionError> {
        let goal = cp.goal.as_connector().ok_or(SessionError::InvalidState)?;
        let b = &self.binding.acquisition;
        if b.owner != cp.owner
            || b.scope != *goal.scope()
            || b.session != cp.snapshot.session
            || b.run != cp.snapshot.run
            || !goal.steps().contains(&b.step)
            || cp.snapshot.dispatch.as_ref() != Some(&b.dispatch)
            || self.binding.claim_revision.value() == 0
            || self.binding.claim_revision >= self.committed_revision
            || self.committed_revision > cp.snapshot.revision
            || self.goal_digest != ClosedLoop::goal_digest(goal.intent())
            || self.valid_until_ms == 0
            || self.valid_until_ms > cp.budget.deadline_ms
            || !cp.resource_ledger.as_ref().is_some_and(|l| {
                l.reservations()
                    .iter()
                    .any(|r| r.id() == &self.binding.reservation && r.binding() == b)
            })
        {
            return Err(SessionError::InvalidInteraction);
        }
        validate_pins(&self.definition_pins, goal)?;
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    contract: String,
    contract_version: String,
    purpose: String,
    binding: PreExecutionGoalBinding,
    goal_digest: ContentDigest,
    definition_pins: Vec<PreExecutionGoalPin>,
    reads: Vec<ReadWire>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadWire {
    max_age_seconds: u64,
    payload: Value,
}
pub struct PreExecutionGoalReadInput<'a> {
    pub condition: &'a ConditionId,
    pub read_request: &'a ConnectorRequest,
    pub observation: &'a ConnectorObservation,
    pub batch: &'a ScopedObservationBatch,
    pub max_age_seconds: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedPreExecutionGoalRead {
    pub condition: ConditionId,
    pub read_request: ConnectorRequestIdentity,
    pub observation: ConnectorObservation,
    pub batch: ScopedObservationBatch,
    pub max_age_seconds: u64,
}
pub struct PreExecutionGoalData {
    reference: PreExecutionGoalRef,
    bytes: Vec<u8>,
    reads: Vec<PersistedPreExecutionGoalRead>,
    readset: RetainedObservationReadset,
}
impl PreExecutionGoalData {
    pub fn new(
        goal: &SupportedConnectorGoal,
        binding: PreExecutionGoalBinding,
        mut definition_pins: Vec<PreExecutionGoalPin>,
        reads: &[PreExecutionGoalReadInput<'_>],
        now_ms: u64,
    ) -> Result<Self, SessionError> {
        validate_pins(&definition_pins, goal)?;
        definition_pins
            .sort_by_key(|p| serde_json::to_vec(p).expect("native reference serializes"));
        let mut wires = Vec::new();
        for read in reads {
            wires.push(ReadWire {
                max_age_seconds: read.max_age_seconds,
                payload: super::readback_storage::encode_pre_execution_source(
                    read.condition,
                    read.read_request,
                    read.observation,
                    read.batch,
                )?,
            });
        }
        let envelope = Envelope {
            contract: "cg.connector-pre-execution-goal".into(),
            contract_version: "1.0".into(),
            purpose: "PRE_EXECUTION_GOAL".into(),
            binding,
            goal_digest: ClosedLoop::goal_digest(goal.intent()),
            definition_pins,
            reads: wires,
        };
        let (reads, readset, valid_until_ms) = decode_values(&envelope, goal, now_ms)?;
        let bytes = serde_json::to_vec(&envelope).map_err(|_| SessionError::InvalidInput)?;
        if bytes.is_empty() || bytes.len() > MAX_CONNECTOR_RECORD_BYTES {
            return Err(SessionError::LimitExceeded);
        }
        let id = format!(
            "pre-goal-{}",
            &gateway_domain::context_content_digest(
                &serde_json::to_vec(&envelope.binding).map_err(|_| SessionError::InvalidInput)?
            )
            .as_str()[..32]
        );
        let reference = PreExecutionGoalRef {
            binding: envelope.binding.clone(),
            committed_revision: envelope.binding.claim_revision.next()?,
            record: content_reference(&id, &bytes)?,
            goal_digest: envelope.goal_digest,
            readset_digest: readset.digest().clone(),
            definition_pins: envelope.definition_pins,
            valid_until_ms,
        };
        Ok(Self {
            reference,
            bytes,
            reads,
            readset,
        })
    }
    pub fn binding(&self) -> &PreExecutionGoalBinding {
        &self.reference.binding
    }
    pub fn definition_pins(&self) -> &[PreExecutionGoalPin] {
        &self.reference.definition_pins
    }
    pub fn readset(&self) -> &RetainedObservationReadset {
        &self.readset
    }
    pub fn reads(&self) -> &[PersistedPreExecutionGoalRead] {
        &self.reads
    }
    pub fn execution(&self) -> Result<ReferenceId, SessionError> {
        pre_execution_goal_execution(&self.reference.binding)
    }
    pub(crate) fn validate_fresh(&self, now: u64) -> Result<(), SessionError> {
        if now >= self.reference.valid_until_ms {
            return Err(SessionError::ExpiredInteraction);
        };
        Ok(())
    }
}
/// Sealed candidate DATA, selected only after the actual native strong proof.
/// It cannot replace the independent persisted-data verification at completion.
pub struct PreExecutionGoalCandidate {
    data: PreExecutionGoalData,
}
impl PreExecutionGoalCandidate {
    pub fn new(
        data: PreExecutionGoalData,
        proof: &VerifiedOutcomeReceipt,
    ) -> Result<Self, SessionError> {
        if !receipt_matches(proof, &data.reference, &data.readset)? {
            return Err(SessionError::InvalidInteraction);
        }
        Ok(Self { data })
    }
    /// Encode immutable candidate data for an exact preparing checkpoint.
    /// This neither writes storage nor verifies/completes/dispatches anything.
    pub(crate) fn into_record(
        mut self,
        cp: &SessionCheckpoint,
        now: u64,
    ) -> Result<(PreExecutionGoalRef, RecordWrite), SessionError> {
        self.data.reference.valid_until_ms = self
            .data
            .reference
            .valid_until_ms
            .min(cp.budget.deadline_ms);
        self.data.reference.validate_for_successor(cp)?;
        self.data.validate_fresh(now)?;
        let r = RecordWrite {
            reference: self.data.reference.record.clone(),
            kind: RecordKind::PreExecutionGoal,
            bytes: self.data.bytes,
        };
        Ok((self.data.reference, r))
    }
}
impl PreExecutionGoalRef {
    fn validate_for_successor(&self, cp: &SessionCheckpoint) -> Result<(), SessionError> {
        let mut next = cp.clone();
        next.snapshot.revision = next.snapshot.revision.next()?;
        if cp.snapshot.state != SessionState::Preparing
            || cp.accepted_connector_consent.is_some()
            || self.binding.claim_revision != cp.snapshot.revision
            || self.committed_revision != next.snapshot.revision
        {
            return Err(SessionError::InvalidState);
        }
        self.validate_for(&next)
    }
}
pub fn pre_execution_goal_execution(
    binding: &PreExecutionGoalBinding,
) -> Result<ReferenceId, SessionError> {
    ReferenceId::new(format!(
        "goal-read-{}",
        &gateway_domain::context_content_digest(
            &serde_json::to_vec(binding).map_err(|_| SessionError::InvalidInput)?
        )
        .as_str()[..32]
    ))
    .map_err(|_| SessionError::InvalidInput)
}
pub(crate) fn receipt_matches(
    proof: &VerifiedOutcomeReceipt,
    reference: &PreExecutionGoalRef,
    readset: &RetainedObservationReadset,
) -> Result<bool, SessionError> {
    Ok(proof.scope() == &reference.binding.acquisition.scope
        && proof.execution() == &pre_execution_goal_execution(&reference.binding)?
        && proof.outcome() == OutcomeClass::Success
        && !proof.facts().is_empty()
        && !proof.evidence().is_empty()
        && proof.goal_digest() == &reference.goal_digest
        && proof.readset_digest() == Some(readset.digest())
        && proof.readset_digest() == Some(&reference.readset_digest))
}
pub fn load_pre_execution_goal(
    repository: &impl SessionRepositoryPort,
    cp: &SessionCheckpoint,
    now_ms: u64,
) -> Result<PreExecutionGoalData, SessionError> {
    let reference = cp
        .pre_execution_goal
        .as_ref()
        .ok_or(SessionError::InvalidState)?;
    reference.validate_for(cp)?;
    if repository.record_kind(&cp.owner, &reference.record)? != RecordKind::PreExecutionGoal {
        return Err(SessionError::InvalidInteraction);
    }
    let bytes = repository.load_artifact(&cp.owner, &reference.record)?;
    if bytes.is_empty()
        || bytes.len() > MAX_CONNECTOR_RECORD_BYTES
        || content_reference(reference.record.id().as_str(), &bytes)? != reference.record
    {
        return Err(SessionError::InvalidInteraction);
    }
    let envelope: Envelope =
        serde_json::from_slice(&bytes).map_err(|_| SessionError::InvalidInteraction)?;
    if envelope.contract != "cg.connector-pre-execution-goal"
        || envelope.contract_version != "1.0"
        || envelope.purpose != "PRE_EXECUTION_GOAL"
        || envelope.binding != reference.binding
        || envelope.goal_digest != reference.goal_digest
        || envelope.definition_pins != reference.definition_pins
    {
        return Err(SessionError::InvalidInteraction);
    }
    let goal = cp.goal.as_connector().ok_or(SessionError::InvalidState)?;
    let (reads, readset, until) = decode_values(&envelope, goal, now_ms)?;
    if readset.digest() != &reference.readset_digest
        || reference.valid_until_ms != until.min(cp.budget.deadline_ms)
    {
        return Err(SessionError::InvalidInteraction);
    }
    let data = PreExecutionGoalData {
        reference: reference.clone(),
        bytes,
        reads,
        readset,
    };
    data.validate_fresh(now_ms)?;
    Ok(data)
}
fn validate_pins(
    pins: &[PreExecutionGoalPin],
    goal: &SupportedConnectorGoal,
) -> Result<(), SessionError> {
    if pins.is_empty() || pins.len() > 128 {
        return Err(SessionError::LimitExceeded);
    }
    let mut unique = BTreeSet::new();
    for pin in pins {
        if pin.scope != *goal.scope()
            || gateway_domain::model_invocation::validate_model_binding(pin).is_err()
            || !unique.insert(serde_json::to_vec(pin).map_err(|_| SessionError::InvalidInput)?)
        {
            return Err(SessionError::InvalidInteraction);
        }
    }
    for role in [
        "cg.connector-runtime",
        "cg.connector-manifest",
        "cg.connector-mapping",
        "cg.connector-state-universe",
        "cg.connector-normalization",
    ] {
        if !pins.iter().any(|p| p.contract.as_str() == role) {
            return Err(SessionError::InvalidInteraction);
        }
    }
    Ok(())
}
fn strong_quality(q: QualityMetadata) -> bool {
    q.trust() == TrustClass::ObservedEvidence
        && q.confidence() != Confidence::Unknown
        && q.freshness() == FreshnessStatus::Fresh
        && q.conflict() == ConflictStatus::None
        && q.uncertainty() == Uncertainty::None
        && q.sensitivity() != SensitivityClass::Secret
}
fn decode_values(
    envelope: &Envelope,
    goal: &SupportedConnectorGoal,
    now: u64,
) -> Result<
    (
        Vec<PersistedPreExecutionGoalRead>,
        RetainedObservationReadset,
        u64,
    ),
    SessionError,
> {
    validate_pins(&envelope.definition_pins, goal)?;
    if envelope.goal_digest != ClosedLoop::goal_digest(goal.intent())
        || envelope.reads.is_empty()
        || envelope.reads.len() > MAX_CONNECTOR_GOAL_CONDITIONS
        || envelope.binding.acquisition.scope != *goal.scope()
        || !goal.steps().contains(&envelope.binding.acquisition.step)
    {
        return Err(SessionError::InvalidInteraction);
    }
    let mut conditions = BTreeSet::new();
    let mut reads = Vec::new();
    let mut members = Vec::new();
    let mut until = u64::MAX;
    for wire in &envelope.reads {
        if wire.max_age_seconds == 0 || wire.max_age_seconds > 3600 {
            return Err(SessionError::InvalidInput);
        }
        let (condition, request, observation, batch) =
            super::readback_storage::decode_pre_execution_source(wire.payload.clone(), goal)?;
        let mapping = goal
            .observations()
            .iter()
            .find(|m| m.condition() == &condition)
            .ok_or(SessionError::ScopeDenied)?;
        if !conditions.insert(condition.clone())
            || request.task != envelope.binding.task.as_str()
            || request.step != envelope.binding.acquisition.step.as_str()
            || request.execution != envelope.binding.execution.as_str()
            || request.capability != mapping.capability().as_str()
            || observation.source != *mapping.source()
            || request.operation_class
                != match mapping.operation_class() {
                    crate::connectors::ConnectorOperationClass::Read => "read",
                    crate::connectors::ConnectorOperationClass::Search => "search",
                    crate::connectors::ConnectorOperationClass::Inspect => "inspect",
                    _ => return Err(SessionError::InvalidInteraction),
                }
            || !strong_quality(observation.metadata.quality)
            || batch
                .quality_metadata()
                .values()
                .flatten()
                .any(|q| !strong_quality(*q))
            || batch.records().facts().iter().any(|f| {
                batch
                    .quality_metadata()
                    .get(f.subject())
                    .is_none_or(|m| m.is_empty())
            })
        {
            return Err(SessionError::InvalidInteraction);
        }
        let observed = observation
            .metadata
            .observed_at
            .ok_or(SessionError::InvalidInteraction)?
            .seconds();
        let observed = u64::try_from(observed)
            .map_err(|_| SessionError::InvalidInteraction)?
            .checked_mul(1000)
            .ok_or(SessionError::LimitExceeded)?;
        let expires = observed
            .checked_add(
                wire.max_age_seconds
                    .checked_mul(1000)
                    .ok_or(SessionError::LimitExceeded)?,
            )
            .ok_or(SessionError::LimitExceeded)?;
        if observed > now || now >= expires {
            return Err(SessionError::ExpiredInteraction);
        }
        until = until.min(expires);
        let member_binding = gateway_domain::context_content_digest(
            &serde_json::to_vec(&(
                "cg.pre-execution-goal-member/1.0",
                &envelope.binding,
                &condition,
                wire,
            ))
            .map_err(|_| SessionError::InvalidInput)?,
        );
        let version_domain = gateway_domain::context_content_digest(
            &serde_json::to_vec(&(
                "cg.connector-source-object/1.0",
                goal.scope(),
                observation.target.connector.as_str(),
                observation.source.as_str(),
                observation.metadata.source_kind.as_str(),
                observation.metadata.object_reference.as_str(),
            ))
            .map_err(|_| SessionError::InvalidInput)?,
        );
        members.push(RetainedObservationMember {
            binding: member_binding,
            version_domain,
            batch: batch.clone(),
        });
        reads.push(PersistedPreExecutionGoalRead {
            condition,
            read_request: request,
            observation,
            batch,
            max_age_seconds: wire.max_age_seconds,
        });
    }
    if conditions
        != goal
            .intent()
            .desired_state()
            .conditions()
            .iter()
            .map(|c| c.id().clone())
            .collect()
    {
        return Err(SessionError::InvalidInteraction);
    }
    let readset = RetainedObservationReadset::new(goal.scope().clone(), members)
        .map_err(|_| SessionError::InvalidInteraction)?;
    Ok((reads, readset, until))
}
