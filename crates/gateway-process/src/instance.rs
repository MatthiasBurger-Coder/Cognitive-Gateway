//! Runtime-independent Process Instance model with definition pinning.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use serde::{Deserialize, Serialize};

use crate::{
    ActivityId, BlockerId, EventOccurrenceId, GateId, GateStatus, ProcessDefinition,
    ProcessDefinitionDigest, ProcessDefinitionId, ProcessDefinitionVersion, ProcessInstanceId,
    ProcessInstanceRevision, ProcessValidator, StateId, TransitionId,
};

/// Runtime lifecycle status of a process instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessInstanceStatus {
    Running,
    Waiting,
    Paused,
    Blocked,
    Completed,
    Failed,
}

/// Snapshot of a blocker attached to an instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockerRuntimeState {
    id: BlockerId,
    reason: String,
    active: bool,
    resolvable: bool,
}

impl BlockerRuntimeState {
    pub fn new(
        id: BlockerId,
        reason: impl Into<String>,
        resolvable: bool,
    ) -> Result<Self, InstanceError> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(InstanceError::new(
                "INVALID_BLOCKER",
                "blocker reason cannot be empty",
            ));
        }
        Ok(Self {
            id,
            reason,
            active: true,
            resolvable,
        })
    }
    #[must_use]
    pub fn id(&self) -> &BlockerId {
        &self.id
    }
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
    #[must_use]
    pub const fn active(&self) -> bool {
        self.active
    }
    #[must_use]
    pub const fn resolvable(&self) -> bool {
        self.resolvable
    }
    pub fn resolve(&mut self) -> Result<(), InstanceError> {
        if !self.resolvable {
            return Err(InstanceError::new(
                "BLOCKER_NOT_RESOLVABLE",
                "blocker cannot be resolved",
            ));
        }
        self.active = false;
        Ok(())
    }
}

/// Auditable deterministic state transition history item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionHistoryEntry {
    revision: ProcessInstanceRevision,
    transition: TransitionId,
    from: StateId,
    to: StateId,
    occurrence: Option<EventOccurrenceId>,
    reason: String,
}

impl TransitionHistoryEntry {
    #[must_use]
    pub const fn revision(&self) -> ProcessInstanceRevision {
        self.revision
    }
    #[must_use]
    pub fn transition(&self) -> &TransitionId {
        &self.transition
    }
    #[must_use]
    pub fn from(&self) -> &StateId {
        &self.from
    }
    #[must_use]
    pub fn to(&self) -> &StateId {
        &self.to
    }
    #[must_use]
    pub fn occurrence(&self) -> Option<&EventOccurrenceId> {
        self.occurrence.as_ref()
    }
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// The only projection accepted by the instance state transition boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionProjection {
    expected_revision: ProcessInstanceRevision,
    transition: TransitionId,
    target_state: StateId,
    occurrence: Option<EventOccurrenceId>,
    status: ProcessInstanceStatus,
    reason: String,
}

impl TransitionProjection {
    pub fn new(
        expected_revision: ProcessInstanceRevision,
        transition: TransitionId,
        target_state: StateId,
        status: ProcessInstanceStatus,
        reason: impl Into<String>,
    ) -> Result<Self, InstanceError> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(InstanceError::new(
                "INVALID_TRANSITION",
                "transition reason cannot be empty",
            ));
        }
        Ok(Self {
            expected_revision,
            transition,
            target_state,
            occurrence: None,
            status,
            reason,
        })
    }
    #[must_use]
    pub fn with_occurrence(mut self, occurrence: EventOccurrenceId) -> Self {
        self.occurrence = Some(occurrence);
        self
    }
    #[must_use]
    pub const fn expected_revision(&self) -> ProcessInstanceRevision {
        self.expected_revision
    }
    #[must_use]
    pub fn target_state(&self) -> &StateId {
        &self.target_state
    }
    #[must_use]
    pub fn transition(&self) -> &TransitionId {
        &self.transition
    }
    #[must_use]
    pub fn occurrence(&self) -> Option<&EventOccurrenceId> {
        self.occurrence.as_ref()
    }
}

/// A process execution pinned to one immutable definition identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInstance {
    id: ProcessInstanceId,
    definition_id: ProcessDefinitionId,
    definition_version: ProcessDefinitionVersion,
    definition_digest: ProcessDefinitionDigest,
    revision: ProcessInstanceRevision,
    current_state: StateId,
    previous_state: Option<StateId>,
    status: ProcessInstanceStatus,
    active_gates: BTreeMap<GateId, GateStatus>,
    blockers: BTreeMap<BlockerId, BlockerRuntimeState>,
    evidence: BTreeSet<crate::EvidenceTypeId>,
    retry_attempts: BTreeMap<ActivityId, u32>,
    context_references: BTreeSet<String>,
    history: Vec<TransitionHistoryEntry>,
}

/// Stable instance-domain failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceError {
    code: &'static str,
    message: String,
}

impl InstanceError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InstanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}
impl Error for InstanceError {}

impl ProcessInstance {
    /// Starts an instance only after the definition has passed static validation.
    pub fn start(
        definition: &ProcessDefinition,
        id: ProcessInstanceId,
    ) -> Result<Self, InstanceError> {
        let report = ProcessValidator::validate(definition);
        if !report.is_valid() {
            return Err(InstanceError::new(
                "DEFINITION_NOT_VALIDATED",
                report.diagnostics().first().map_or_else(
                    || "definition failed static validation".to_owned(),
                    |diagnostic| diagnostic.message().to_owned(),
                ),
            ));
        }
        let identity = definition.identity();
        Ok(Self {
            id,
            definition_id: identity.id().clone(),
            definition_version: identity.version(),
            definition_digest: identity.digest().clone(),
            revision: ProcessInstanceRevision::initial(),
            current_state: definition.initial_state().id().clone(),
            previous_state: None,
            status: ProcessInstanceStatus::Running,
            active_gates: BTreeMap::new(),
            blockers: BTreeMap::new(),
            evidence: BTreeSet::new(),
            retry_attempts: BTreeMap::new(),
            context_references: BTreeSet::new(),
            history: Vec::new(),
        })
    }

    #[must_use]
    pub fn id(&self) -> &ProcessInstanceId {
        &self.id
    }
    #[must_use]
    pub fn definition_id(&self) -> &ProcessDefinitionId {
        &self.definition_id
    }
    #[must_use]
    pub const fn definition_version(&self) -> ProcessDefinitionVersion {
        self.definition_version
    }
    #[must_use]
    pub fn definition_digest(&self) -> &ProcessDefinitionDigest {
        &self.definition_digest
    }
    #[must_use]
    pub const fn revision(&self) -> ProcessInstanceRevision {
        self.revision
    }
    #[must_use]
    pub fn current_state(&self) -> &StateId {
        &self.current_state
    }
    #[must_use]
    pub fn previous_state(&self) -> Option<&StateId> {
        self.previous_state.as_ref()
    }
    #[must_use]
    pub const fn status(&self) -> ProcessInstanceStatus {
        self.status
    }
    #[must_use]
    pub fn active_gates(&self) -> &BTreeMap<GateId, GateStatus> {
        &self.active_gates
    }
    #[must_use]
    pub fn blockers(&self) -> &BTreeMap<BlockerId, BlockerRuntimeState> {
        &self.blockers
    }
    #[must_use]
    pub fn evidence(&self) -> &BTreeSet<crate::EvidenceTypeId> {
        &self.evidence
    }
    #[must_use]
    pub fn retry_attempts(&self) -> &BTreeMap<ActivityId, u32> {
        &self.retry_attempts
    }
    #[must_use]
    pub fn context_references(&self) -> &BTreeSet<String> {
        &self.context_references
    }
    #[must_use]
    pub fn history(&self) -> &[TransitionHistoryEntry] {
        &self.history
    }

    /// Rejects stale callers before an authoritative mutation is attempted.
    pub fn require_revision(&self, expected: ProcessInstanceRevision) -> Result<(), InstanceError> {
        if self.revision == expected {
            Ok(())
        } else {
            Err(InstanceError::new(
                "STALE_REVISION",
                format!(
                    "expected revision {}, current is {}",
                    expected.value(),
                    self.revision.value()
                ),
            ))
        }
    }

    /// Confirms that the instance remains pinned to this exact definition.
    pub fn require_definition(&self, definition: &ProcessDefinition) -> Result<(), InstanceError> {
        let identity = definition.identity();
        if self.definition_id == *identity.id()
            && self.definition_version == identity.version()
            && self.definition_digest == *identity.digest()
        {
            Ok(())
        } else {
            Err(InstanceError::new(
                "DEFINITION_IDENTITY_CONFLICT",
                "instance definition ID, version or digest does not match",
            ))
        }
    }

    /// Applies a previously evaluated transition projection with a revision
    /// precondition. There is no arbitrary state-assignment operation.
    pub fn apply_projection(
        &mut self,
        definition: &ProcessDefinition,
        projection: TransitionProjection,
    ) -> Result<(), InstanceError> {
        self.require_definition(definition)?;
        self.require_revision(projection.expected_revision)?;
        let transition = definition
            .transitions()
            .iter()
            .find(|item| item.id() == &projection.transition)
            .ok_or_else(|| {
                InstanceError::new(
                    "UNKNOWN_TRANSITION",
                    "transition is not in pinned definition",
                )
            })?;
        if transition.from() != &self.current_state || transition.to() != &projection.target_state {
            return Err(InstanceError::new(
                "ILLEGAL_STATE_PROJECTION",
                "projection does not match the current state or declared transition",
            ));
        }
        let revision = self
            .revision
            .next()
            .map_err(|error| InstanceError::new("REVISION_OVERFLOW", error.to_string()))?;
        self.previous_state = Some(self.current_state.clone());
        let from = self.current_state.clone();
        self.current_state = projection.target_state.clone();
        self.revision = revision;
        self.status = projection.status;
        self.history.push(TransitionHistoryEntry {
            revision,
            transition: projection.transition,
            from,
            to: projection.target_state,
            occurrence: projection.occurrence,
            reason: projection.reason,
        });
        Ok(())
    }

    pub fn set_gate_status(&mut self, gate: GateId, status: GateStatus) {
        self.active_gates.insert(gate, status);
    }

    pub fn record_evidence(&mut self, evidence: crate::EvidenceTypeId) {
        self.evidence.insert(evidence);
    }

    pub fn record_blocker(&mut self, blocker: BlockerRuntimeState) {
        self.blockers.insert(blocker.id().clone(), blocker);
    }

    pub fn add_context_reference(
        &mut self,
        reference: impl Into<String>,
    ) -> Result<(), InstanceError> {
        let reference = reference.into();
        if reference.trim().is_empty() || reference.chars().any(char::is_control) {
            return Err(InstanceError::new(
                "INVALID_CONTEXT_REFERENCE",
                "context reference is empty or contains control characters",
            ));
        }
        self.context_references.insert(reference);
        Ok(())
    }

    pub fn increment_retry(&mut self, activity: ActivityId) -> Result<u32, InstanceError> {
        let attempts = self.retry_attempts.entry(activity).or_default();
        *attempts = attempts
            .checked_add(1)
            .ok_or_else(|| InstanceError::new("RETRY_OVERFLOW", "retry counter overflow"))?;
        Ok(*attempts)
    }

    pub fn to_json(&self) -> Result<String, InstanceError> {
        serde_json::to_string(self)
            .map_err(|error| InstanceError::new("SERIALIZATION_ERROR", error.to_string()))
    }

    pub fn from_json(value: &str) -> Result<Self, InstanceError> {
        let instance = serde_json::from_str(value)
            .map_err(|error| InstanceError::new("SERIALIZATION_ERROR", error.to_string()))?;
        Ok(instance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EventTypeDefinition, EventTypeId, GuardExpression, ProcessDefinitionBuilder,
        ProcessDefinitionVersion, StateDefinition,
    };

    fn definition() -> ProcessDefinition {
        ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("instance-example").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .with_states([
            StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
            StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap(),
        ])
        .with_events([EventTypeDefinition::new(
            EventTypeId::new("finish").unwrap(),
        )])
        .with_transitions([crate::TransitionDefinition::new(
            TransitionId::new("finish").unwrap(),
            StateId::new("start").unwrap(),
            EventTypeId::new("finish").unwrap(),
            StateId::new("done").unwrap(),
            GuardExpression::Always,
        )])
        .build()
        .unwrap()
    }

    #[test]
    fn starts_pinned_instance_and_preserves_identity_on_round_trip() {
        let definition = definition();
        let instance =
            ProcessInstance::start(&definition, ProcessInstanceId::new("run-1").unwrap()).unwrap();
        assert_eq!(instance.current_state().as_str(), "start");
        assert_eq!(instance.revision().value(), 0);
        assert_eq!(
            instance,
            ProcessInstance::from_json(&instance.to_json().unwrap()).unwrap()
        );
        assert!(instance.require_definition(&definition).is_ok());
    }

    #[test]
    fn applies_only_matching_revision_and_declared_transition() {
        let definition = definition();
        let mut instance =
            ProcessInstance::start(&definition, ProcessInstanceId::new("run-1").unwrap()).unwrap();
        let projection = TransitionProjection::new(
            ProcessInstanceRevision::initial(),
            TransitionId::new("finish").unwrap(),
            StateId::new("done").unwrap(),
            ProcessInstanceStatus::Completed,
            "event accepted",
        )
        .unwrap()
        .with_occurrence(EventOccurrenceId::new("occurrence-1").unwrap());
        instance.apply_projection(&definition, projection).unwrap();
        assert_eq!(instance.status(), ProcessInstanceStatus::Completed);
        assert_eq!(instance.revision().value(), 1);
        assert_eq!(instance.previous_state().unwrap().as_str(), "start");
        assert_eq!(
            instance.history()[0].occurrence().unwrap().as_str(),
            "occurrence-1"
        );
        let stale = TransitionProjection::new(
            ProcessInstanceRevision::initial(),
            TransitionId::new("finish").unwrap(),
            StateId::new("done").unwrap(),
            ProcessInstanceStatus::Completed,
            "duplicate",
        )
        .unwrap();
        assert_eq!(
            instance
                .apply_projection(&definition, stale)
                .unwrap_err()
                .code(),
            "STALE_REVISION"
        );
    }

    #[test]
    fn tracks_typed_lifecycle_snapshots_and_rejects_identity_conflicts() {
        let definition = definition();
        let mut instance =
            ProcessInstance::start(&definition, ProcessInstanceId::new("run-1").unwrap()).unwrap();
        let gate = GateId::new("review").unwrap();
        instance.set_gate_status(gate, GateStatus::Passed);
        instance.record_evidence(crate::EvidenceTypeId::new("report").unwrap());
        instance.record_blocker(
            BlockerRuntimeState::new(BlockerId::new("wait").unwrap(), "review", true).unwrap(),
        );
        instance
            .add_context_reference("input:repository.snapshot")
            .unwrap();
        assert_eq!(instance.active_gates().len(), 1);
        assert_eq!(instance.evidence().len(), 1);
        assert_eq!(instance.blockers().len(), 1);
        assert_eq!(instance.context_references().len(), 1);
        assert_eq!(
            instance
                .increment_retry(ActivityId::new("verify").unwrap())
                .unwrap(),
            1
        );
        let mut changed = definition.clone();
        let other = ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("other").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .with_states(changed.states().to_vec())
        .with_events(changed.events().to_vec())
        .with_transitions(changed.transitions().to_vec())
        .build()
        .unwrap();
        assert_eq!(
            instance.require_definition(&other).unwrap_err().code(),
            "DEFINITION_IDENTITY_CONFLICT"
        );
        changed = definition;
        assert_eq!(changed.identity().id().as_str(), "instance-example");
    }
}
