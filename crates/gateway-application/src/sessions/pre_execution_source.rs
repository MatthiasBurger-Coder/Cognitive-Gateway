//! Invocation-free authenticated CG-06 source-read persistence. No mutation or model records.
use super::*;
use crate::{ScopedObservationBatch, SourceSnapshot, connectors::*};
use gateway_domain::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::str::FromStr;
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

fn decode_payload(
    payload: &GoalReadbackPayload,
    goal: &SupportedConnectorGoal,
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
        || o.basis != "source_read"
        || o.verifies_request_digest.is_some()
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
        basis: ConnectorObservationBasis::SourceRead,
    };
    Ok((observation, batch))
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
    if batch.records() != &observation.records {
        return Err(SessionError::InvalidInteraction);
    }
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
    let condition =
        ConditionId::new(p.condition.clone()).map_err(|_| SessionError::InvalidInput)?;
    let request = p.read_request.clone();
    let (observation, batch) = decode_payload(&p, goal)?;
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
