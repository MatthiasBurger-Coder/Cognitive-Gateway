//! Event identity, idempotency and atomic process mutation contracts.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use crate::{
    CausationId, CorrelationId, EventOccurrenceId, EventTypeId, InstanceError, ProcessDefinition,
    ProcessInstance, ProcessInstanceId, ProcessInstanceRevision, TransitionProjection,
};

/// One concrete delivery of a declared event type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventOccurrence {
    id: EventOccurrenceId,
    event_type: EventTypeId,
    instance_id: ProcessInstanceId,
    expected_revision: ProcessInstanceRevision,
    correlation_id: Option<CorrelationId>,
    causation_id: Option<CausationId>,
    attributes: BTreeMap<String, String>,
}

impl EventOccurrence {
    pub fn new(
        id: EventOccurrenceId,
        event_type: EventTypeId,
        instance_id: ProcessInstanceId,
        expected_revision: ProcessInstanceRevision,
    ) -> Self {
        Self {
            id,
            event_type,
            instance_id,
            expected_revision,
            correlation_id: None,
            causation_id: None,
            attributes: BTreeMap::new(),
        }
    }
    #[must_use]
    pub fn id(&self) -> &EventOccurrenceId {
        &self.id
    }
    #[must_use]
    pub fn event_type(&self) -> &EventTypeId {
        &self.event_type
    }
    #[must_use]
    pub fn instance_id(&self) -> &ProcessInstanceId {
        &self.instance_id
    }
    #[must_use]
    pub const fn expected_revision(&self) -> ProcessInstanceRevision {
        self.expected_revision
    }
    #[must_use]
    pub fn correlation_id(&self) -> Option<&CorrelationId> {
        self.correlation_id.as_ref()
    }
    #[must_use]
    pub fn causation_id(&self) -> Option<&CausationId> {
        self.causation_id.as_ref()
    }
    #[must_use]
    pub fn attributes(&self) -> &BTreeMap<String, String> {
        &self.attributes
    }
    #[must_use]
    pub fn with_correlation(mut self, value: CorrelationId) -> Self {
        self.correlation_id = Some(value);
        self
    }
    #[must_use]
    pub fn with_causation(mut self, value: CausationId) -> Self {
        self.causation_id = Some(value);
        self
    }
    pub fn with_attribute(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, MutationError> {
        let name = name.into();
        let value = value.into();
        if name.trim().is_empty()
            || name.chars().any(char::is_control)
            || value.chars().any(char::is_control)
        {
            return Err(MutationError::new(
                "INVALID_EVENT_ATTRIBUTE",
                "event attributes cannot be empty or contain controls",
            ));
        }
        self.attributes.insert(name, value);
        Ok(self)
    }
}

/// Result of an atomic mutation. A duplicate is a successful no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitOutcome {
    Applied { revision: ProcessInstanceRevision },
    Duplicate { revision: ProcessInstanceRevision },
}

/// Stable mutation boundary failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationError {
    code: &'static str,
    message: String,
}

impl MutationError {
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
impl fmt::Display for MutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}
impl Error for MutationError {}

impl From<InstanceError> for MutationError {
    fn from(error: InstanceError) -> Self {
        Self::new(error.code(), error.message())
    }
}

/// Replaceable persistence port for all-or-nothing process mutations.
pub trait AtomicProcessMutation {
    fn commit_transition(
        &mut self,
        definition: &ProcessDefinition,
        event: &EventOccurrence,
        projection: TransitionProjection,
    ) -> Result<CommitOutcome, MutationError>;
}

/// Deterministic reference store proving the atomic port semantics.
#[derive(Debug, Default, Clone)]
pub struct InMemoryProcessStore {
    instances: BTreeMap<ProcessInstanceId, ProcessInstance>,
    consumed: BTreeMap<ProcessInstanceId, BTreeSet<EventOccurrenceId>>,
    fail_next_commit: bool,
}

impl InMemoryProcessStore {
    pub fn insert(&mut self, instance: ProcessInstance) {
        let id = instance.id().clone();
        self.consumed.entry(id.clone()).or_default();
        self.instances.insert(id, instance);
    }
    #[must_use]
    pub fn instance(&self, id: &ProcessInstanceId) -> Option<&ProcessInstance> {
        self.instances.get(id)
    }
    #[must_use]
    pub fn consumed_occurrences(
        &self,
        id: &ProcessInstanceId,
    ) -> Option<&BTreeSet<EventOccurrenceId>> {
        self.consumed.get(id)
    }
    /// Makes the next commit fail before any authoritative state is changed.
    pub fn fail_next_commit(&mut self) {
        self.fail_next_commit = true;
    }
}

impl AtomicProcessMutation for InMemoryProcessStore {
    fn commit_transition(
        &mut self,
        definition: &ProcessDefinition,
        event: &EventOccurrence,
        projection: TransitionProjection,
    ) -> Result<CommitOutcome, MutationError> {
        let instance = self.instances.get(event.instance_id()).ok_or_else(|| {
            MutationError::new(
                "UNKNOWN_INSTANCE",
                "event targets an unknown process instance",
            )
        })?;
        if let Some(consumed) = self.consumed.get(event.instance_id()) {
            if consumed.contains(event.id()) {
                return Ok(CommitOutcome::Duplicate {
                    revision: instance.revision(),
                });
            }
        }
        if event.expected_revision() != projection.expected_revision() {
            return Err(MutationError::new(
                "STALE_REVISION",
                "event and transition projection expected revisions differ",
            ));
        }
        if self.fail_next_commit {
            self.fail_next_commit = false;
            return Err(MutationError::new(
                "ATOMIC_COMMIT_FAILED",
                "simulated storage failure before commit",
            ));
        }
        let mut candidate = instance.clone();
        let occurrence = event.id().clone();
        let projection = projection.with_occurrence(occurrence.clone());
        candidate
            .apply_projection(definition, projection)
            .map_err(MutationError::from)?;
        let revision = candidate.revision();
        self.instances
            .insert(event.instance_id().clone(), candidate);
        self.consumed
            .entry(event.instance_id().clone())
            .or_default()
            .insert(occurrence);
        Ok(CommitOutcome::Applied { revision })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EventTypeDefinition, GuardExpression, ProcessDefinitionBuilder, ProcessDefinitionId,
        ProcessDefinitionVersion, StateDefinition, StateId, TransitionId,
    };

    fn setup() -> (ProcessDefinition, ProcessInstance) {
        let definition = ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("mutation-example").unwrap(),
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
        .unwrap();
        let instance =
            ProcessInstance::start(&definition, ProcessInstanceId::new("run-1").unwrap()).unwrap();
        (definition, instance)
    }

    #[test]
    fn distinguishes_event_type_from_occurrence_and_commits_once() {
        let (definition, instance) = setup();
        let mut store = InMemoryProcessStore::default();
        store.insert(instance);
        let event = EventOccurrence::new(
            EventOccurrenceId::new("occurrence-1").unwrap(),
            EventTypeId::new("finish").unwrap(),
            ProcessInstanceId::new("run-1").unwrap(),
            ProcessInstanceRevision::initial(),
        )
        .with_correlation(CorrelationId::new("correlation-1").unwrap())
        .with_causation(CausationId::new("cause-1").unwrap())
        .with_attribute("result", "success")
        .unwrap();
        let projection = TransitionProjection::new(
            ProcessInstanceRevision::initial(),
            TransitionId::new("finish").unwrap(),
            StateId::new("done").unwrap(),
            crate::ProcessInstanceStatus::Completed,
            "accepted",
        )
        .unwrap();
        assert!(matches!(
            store
                .commit_transition(&definition, &event, projection.clone())
                .unwrap(),
            CommitOutcome::Applied { .. }
        ));
        assert!(matches!(
            store.commit_transition(&definition, &event, projection).unwrap(),
            CommitOutcome::Duplicate { revision } if revision.value() == 1
        ));
        assert_eq!(
            store
                .consumed_occurrences(&ProcessInstanceId::new("run-1").unwrap())
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn rejects_stale_wrong_instance_and_partial_failure_without_partial_state() {
        let (definition, instance) = setup();
        let mut store = InMemoryProcessStore::default();
        store.insert(instance);
        let wrong = EventOccurrence::new(
            EventOccurrenceId::new("occurrence-2").unwrap(),
            EventTypeId::new("finish").unwrap(),
            ProcessInstanceId::new("missing").unwrap(),
            ProcessInstanceRevision::initial(),
        );
        let projection = TransitionProjection::new(
            ProcessInstanceRevision::initial(),
            TransitionId::new("finish").unwrap(),
            StateId::new("done").unwrap(),
            crate::ProcessInstanceStatus::Completed,
            "accepted",
        )
        .unwrap();
        assert_eq!(
            store
                .commit_transition(&definition, &wrong, projection.clone())
                .unwrap_err()
                .code(),
            "UNKNOWN_INSTANCE"
        );
        let stale = EventOccurrence::new(
            EventOccurrenceId::new("occurrence-3").unwrap(),
            EventTypeId::new("finish").unwrap(),
            ProcessInstanceId::new("run-1").unwrap(),
            ProcessInstanceRevision::new(1),
        );
        assert_eq!(
            store
                .commit_transition(&definition, &stale, projection.clone())
                .unwrap_err()
                .code(),
            "STALE_REVISION"
        );
        store.fail_next_commit();
        let failing = EventOccurrence::new(
            EventOccurrenceId::new("occurrence-4").unwrap(),
            EventTypeId::new("finish").unwrap(),
            ProcessInstanceId::new("run-1").unwrap(),
            ProcessInstanceRevision::initial(),
        );
        assert_eq!(
            store
                .commit_transition(&definition, &failing, projection)
                .unwrap_err()
                .code(),
            "ATOMIC_COMMIT_FAILED"
        );
        assert_eq!(
            store
                .instance(&ProcessInstanceId::new("run-1").unwrap())
                .unwrap()
                .revision()
                .value(),
            0
        );
        assert!(
            store
                .consumed_occurrences(&ProcessInstanceId::new("run-1").unwrap())
                .unwrap()
                .is_empty()
        );
    }
}
