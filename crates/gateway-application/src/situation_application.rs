//! CG-06 application operations and the read-only CG-04 process boundary.
//!
//! This facade composes already validated domain aggregates. It does not own
//! process state, reimplement CG-02 enums, or expose a mutation operation for
//! a process instance.

use gateway_domain::{
    DeclarativeContext, DeclarativeContextSituationDocument, ExecutionContext, ExecutionProfile,
    ExplainabilityTrace, Intent, NormalizationInput, ObservationEvidenceSet, ObservedState,
    OperatingMode, Situation, SituationAssemblyInput, SituationId, ValidationError,
    normalize_current_state as domain_normalize_current_state,
};
use gateway_process::{
    AuthorizedActivity, BlockerRuntimeState, GateStatus, ProcessDefinition, ProcessInspection,
    ProcessInstance, ProcessInstanceRevision, ProcessInstanceStatus, StateId,
};

use crate::external_context::ScopedContextSnapshot;

/// Stable application-boundary failure for CG-06 operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SituationApplicationError {
    code: &'static str,
    message: String,
}

impl SituationApplicationError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Returns the stable machine-readable error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Returns the diagnostic message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for SituationApplicationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SituationApplicationError {}

impl From<gateway_process::ApplicationError> for SituationApplicationError {
    fn from(error: gateway_process::ApplicationError) -> Self {
        Self::new(error.code(), error.message())
    }
}

impl From<gateway_process::InstanceError> for SituationApplicationError {
    fn from(error: gateway_process::InstanceError) -> Self {
        Self::new(error.code(), error.message())
    }
}

impl From<ValidationError> for SituationApplicationError {
    fn from(error: ValidationError) -> Self {
        Self::new("DOMAIN_VALIDATION_ERROR", error.to_string())
    }
}

/// A process input borrowed from the authoritative CG-04 process boundary.
///
/// An optional expected revision lets an application caller reject a snapshot
/// that was read for a different process revision before it is attached to a
/// Situation inspection.
#[derive(Debug, Clone, Copy)]
pub struct ProcessSnapshotInput<'a> {
    definition: &'a ProcessDefinition,
    instance: &'a ProcessInstance,
    expected_revision: Option<ProcessInstanceRevision>,
}

impl<'a> ProcessSnapshotInput<'a> {
    /// Creates a read-only process snapshot request.
    #[must_use]
    pub const fn new(definition: &'a ProcessDefinition, instance: &'a ProcessInstance) -> Self {
        Self {
            definition,
            instance,
            expected_revision: None,
        }
    }

    /// Requires the authoritative instance to still have this revision.
    #[must_use]
    pub const fn requiring_revision(mut self, revision: ProcessInstanceRevision) -> Self {
        self.expected_revision = Some(revision);
        self
    }

    /// Returns the borrowed process definition.
    #[must_use]
    pub const fn definition(&self) -> &'a ProcessDefinition {
        self.definition
    }

    /// Returns the borrowed process instance.
    #[must_use]
    pub const fn instance(&self) -> &'a ProcessInstance {
        self.instance
    }

    /// Returns the optional expected revision precondition.
    #[must_use]
    pub const fn expected_revision(&self) -> Option<ProcessInstanceRevision> {
        self.expected_revision
    }
}

/// Validated, read-only process data attached to a Situation inspection.
///
/// The wrapped [`ProcessInspection`] is produced by CG-04's
/// `ProcessApplication`; definition pinning and process semantics therefore
/// remain owned by CG-04 rather than being copied into CG-06.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSituationReference {
    inspection: ProcessInspection,
}

impl ProcessSituationReference {
    fn capture(input: ProcessSnapshotInput<'_>) -> Result<Self, SituationApplicationError> {
        validate_process_snapshot(input)?;
        if let Some(expected_revision) = input.expected_revision {
            input
                .instance
                .require_revision(expected_revision)
                .map_err(SituationApplicationError::from)?;
        }
        let inspection = gateway_process::ProcessApplication::new()
            .inspect_process(input.definition, input.instance)
            .map_err(SituationApplicationError::from)?;
        Ok(Self { inspection })
    }

    /// Returns the pinned process-definition identity.
    #[must_use]
    pub fn definition_id(&self) -> &gateway_process::ProcessDefinitionId {
        self.inspection.definition_id()
    }

    /// Returns the pinned process-definition version.
    #[must_use]
    pub const fn definition_version(&self) -> gateway_process::ProcessDefinitionVersion {
        self.inspection.definition_version()
    }

    /// Returns the pinned process-definition digest.
    #[must_use]
    pub fn definition_digest(&self) -> &gateway_process::ProcessDefinitionDigest {
        self.inspection.definition_digest()
    }

    /// Returns the authoritative process instance identity.
    #[must_use]
    pub fn instance_id(&self) -> &gateway_process::ProcessInstanceId {
        self.inspection.instance().id()
    }

    /// Returns the captured instance revision.
    #[must_use]
    pub fn instance_revision(&self) -> ProcessInstanceRevision {
        self.inspection.instance().revision()
    }

    /// Returns the authoritative current process state.
    #[must_use]
    pub fn current_state(&self) -> &StateId {
        self.inspection.instance().current_state()
    }

    /// Returns the previous process state, if a transition was committed.
    #[must_use]
    pub fn previous_state(&self) -> Option<&StateId> {
        self.inspection.instance().previous_state()
    }

    /// Returns the authoritative lifecycle status.
    #[must_use]
    pub fn status(&self) -> ProcessInstanceStatus {
        self.inspection.instance().status()
    }

    /// Returns active gate states from the process instance snapshot.
    #[must_use]
    pub fn active_gates(&self) -> &std::collections::BTreeMap<gateway_process::GateId, GateStatus> {
        self.inspection.instance().active_gates()
    }

    /// Returns active and resolved blocker records from the process snapshot.
    #[must_use]
    pub fn blockers(
        &self,
    ) -> &std::collections::BTreeMap<gateway_process::BlockerId, BlockerRuntimeState> {
        self.inspection.instance().blockers()
    }

    /// Returns process evidence references.
    #[must_use]
    pub fn evidence(&self) -> &std::collections::BTreeSet<gateway_process::EvidenceTypeId> {
        self.inspection.instance().evidence()
    }

    /// Returns retry state without changing it.
    #[must_use]
    pub fn retry_attempts(&self) -> &std::collections::BTreeMap<AuthorizedActivityId, u32> {
        self.inspection.instance().retry_attempts()
    }

    /// Returns process context references retained by CG-04.
    #[must_use]
    pub fn context_references(&self) -> &std::collections::BTreeSet<String> {
        self.inspection.instance().context_references()
    }

    /// Returns immutable transition history from the process snapshot.
    #[must_use]
    pub fn history(&self) -> &[gateway_process::TransitionHistoryEntry] {
        self.inspection.instance().history()
    }

    /// Returns the optional waiting condition.
    #[must_use]
    pub fn waiting_condition(&self) -> Option<&gateway_process::WaitingCondition> {
        self.inspection.instance().waiting_condition()
    }

    /// Returns the process activities currently authorized by the inspection.
    #[must_use]
    pub fn authorized_activities(&self) -> &[AuthorizedActivity] {
        self.inspection.authorized_activities()
    }

    /// Returns the underlying immutable process inspection projection.
    #[must_use]
    pub const fn inspection(&self) -> &ProcessInspection {
        &self.inspection
    }
}

fn validate_process_snapshot(
    input: ProcessSnapshotInput<'_>,
) -> Result<(), SituationApplicationError> {
    input.definition.verify_digest().map_err(|error| {
        SituationApplicationError::new(error.code().as_str(), error.to_string())
    })?;
    let report =
        gateway_process::ProcessApplication::new().validate_process_definition(input.definition);
    if !report.is_valid() {
        return Err(SituationApplicationError::new(
            "PROCESS_DEFINITION_INVALID",
            "process definition failed CG-04 validation",
        ));
    }
    input
        .instance
        .require_definition(input.definition)
        .map_err(SituationApplicationError::from)?;
    let instance = input.instance;
    if !input
        .definition
        .states()
        .iter()
        .any(|state| state.id() == instance.current_state())
    {
        return Err(SituationApplicationError::new(
            "INVALID_PROCESS_SNAPSHOT",
            "process instance current state is not declared by its definition",
        ));
    }
    if instance.active_gates().keys().any(|gate| {
        !input
            .definition
            .gates()
            .iter()
            .any(|item| item.id() == gate)
    }) {
        return Err(SituationApplicationError::new(
            "INVALID_PROCESS_SNAPSHOT",
            "process instance contains an undeclared gate",
        ));
    }
    if instance.blockers().keys().any(|blocker| {
        !input
            .definition
            .blockers()
            .iter()
            .any(|item| item.id() == blocker)
    }) {
        return Err(SituationApplicationError::new(
            "INVALID_PROCESS_SNAPSHOT",
            "process instance contains an undeclared blocker",
        ));
    }
    if instance.evidence().iter().any(|evidence| {
        !input
            .definition
            .evidence()
            .iter()
            .any(|item| item.evidence_type() == evidence)
    }) {
        return Err(SituationApplicationError::new(
            "INVALID_PROCESS_SNAPSHOT",
            "process instance contains undeclared evidence",
        ));
    }
    if instance.retry_attempts().keys().any(|activity| {
        !input
            .definition
            .activities()
            .iter()
            .any(|item| item.id() == activity)
    }) {
        return Err(SituationApplicationError::new(
            "INVALID_PROCESS_SNAPSHOT",
            "process instance contains an undeclared retry activity",
        ));
    }
    if (instance.status() == ProcessInstanceStatus::Paused)
        != instance.waiting_condition().is_some()
    {
        return Err(SituationApplicationError::new(
            "INVALID_PROCESS_SNAPSHOT",
            "paused process instance must retain exactly one waiting condition",
        ));
    }
    if instance
        .context_references()
        .iter()
        .any(|reference| reference.trim().is_empty() || reference.chars().any(char::is_control))
    {
        return Err(SituationApplicationError::new(
            "INVALID_PROCESS_SNAPSHOT",
            "process context references must be non-empty and control-free",
        ));
    }
    Ok(())
}

// The process crate uses ActivityId as the retry-attempt map key. Keeping the
// alias local makes the accessor signature readable without defining a CG-06
// process identity.
type AuthorizedActivityId = gateway_process::ActivityId;

/// Read-only identity inputs used when explaining one Situation snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SituationExplainability {
    context_id: gateway_domain::DeclarativeContextId,
    observed_state_id: gateway_domain::ObservedStateId,
    situation_id: SituationId,
    operating_mode: Option<OperatingMode>,
    execution_profile: Option<ExecutionProfile>,
    process_definition_id: Option<gateway_process::ProcessDefinitionId>,
    process_definition_version: Option<gateway_process::ProcessDefinitionVersion>,
    process_definition_digest: Option<gateway_process::ProcessDefinitionDigest>,
    process_instance_id: Option<gateway_process::ProcessInstanceId>,
    process_instance_revision: Option<ProcessInstanceRevision>,
    traces: Vec<ExplainabilityTrace>,
}

impl SituationExplainability {
    /// Returns the context identity used for the inspection.
    #[must_use]
    pub fn context_id(&self) -> &gateway_domain::DeclarativeContextId {
        &self.context_id
    }

    /// Returns the normalized state identity used for the inspection.
    #[must_use]
    pub fn observed_state_id(&self) -> &gateway_domain::ObservedStateId {
        &self.observed_state_id
    }

    /// Returns the Situation identity.
    #[must_use]
    pub fn situation_id(&self) -> &SituationId {
        &self.situation_id
    }

    /// Returns the reused CG-02 operating mode, if supplied.
    #[must_use]
    pub const fn operating_mode(&self) -> Option<OperatingMode> {
        self.operating_mode
    }

    /// Returns the reused CG-02 execution profile, if supplied.
    #[must_use]
    pub const fn execution_profile(&self) -> Option<ExecutionProfile> {
        self.execution_profile
    }

    /// Returns the exact process definition identity used, if supplied.
    #[must_use]
    pub fn process_definition_id(&self) -> Option<&gateway_process::ProcessDefinitionId> {
        self.process_definition_id.as_ref()
    }

    /// Returns the exact process definition version used, if supplied.
    #[must_use]
    pub const fn process_definition_version(
        &self,
    ) -> Option<gateway_process::ProcessDefinitionVersion> {
        self.process_definition_version
    }

    /// Returns the exact process definition digest used, if supplied.
    #[must_use]
    pub fn process_definition_digest(&self) -> Option<&gateway_process::ProcessDefinitionDigest> {
        self.process_definition_digest.as_ref()
    }

    /// Returns the exact process instance identity used, if supplied.
    #[must_use]
    pub fn process_instance_id(&self) -> Option<&gateway_process::ProcessInstanceId> {
        self.process_instance_id.as_ref()
    }

    /// Returns the exact process instance revision used, if supplied.
    #[must_use]
    pub const fn process_instance_revision(&self) -> Option<ProcessInstanceRevision> {
        self.process_instance_revision
    }

    /// Returns the domain explainability traces for this Situation.
    #[must_use]
    pub fn traces(&self) -> &[ExplainabilityTrace] {
        &self.traces
    }
}

/// Read-only application inspection of a declarative Situation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SituationInspection {
    context_id: gateway_domain::DeclarativeContextId,
    observed_state_id: gateway_domain::ObservedStateId,
    situation: Situation,
    operating_mode: Option<OperatingMode>,
    execution_profile: Option<ExecutionProfile>,
    process: Option<ProcessSituationReference>,
}

impl SituationInspection {
    /// Returns the context identity used for the inspection.
    #[must_use]
    pub fn context_id(&self) -> &gateway_domain::DeclarativeContextId {
        &self.context_id
    }

    /// Returns the normalized state identity used for the inspection.
    #[must_use]
    pub fn observed_state_id(&self) -> &gateway_domain::ObservedStateId {
        &self.observed_state_id
    }

    /// Returns the inspected Situation.
    #[must_use]
    pub const fn situation(&self) -> &Situation {
        &self.situation
    }

    /// Returns the reused CG-02 operating mode, if supplied.
    #[must_use]
    pub const fn operating_mode(&self) -> Option<OperatingMode> {
        self.operating_mode
    }

    /// Returns the reused CG-02 execution profile, if supplied.
    #[must_use]
    pub const fn execution_profile(&self) -> Option<ExecutionProfile> {
        self.execution_profile
    }

    /// Returns the optional read-only CG-04 process snapshot.
    #[must_use]
    pub const fn process(&self) -> Option<&ProcessSituationReference> {
        self.process.as_ref()
    }
}

/// Application facade for the complete CG-06 use-case surface.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeclarativeSituationApplication;

impl DeclarativeSituationApplication {
    /// Creates the stateless CG-06 application facade.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Validates and assembles the complete declarative context document.
    pub fn validate_declarative_context(
        &self,
        context: DeclarativeContext,
        intent: Option<Intent>,
        records: Option<ObservationEvidenceSet>,
        observed_state: ObservedState,
        situation: Situation,
    ) -> Result<DeclarativeContextSituationDocument, SituationApplicationError> {
        DeclarativeContextSituationDocument::new(
            context,
            intent,
            records,
            observed_state,
            situation,
        )
        .map_err(|error| {
            SituationApplicationError::new("DECLARATIVE_CONTEXT_INVALID", error.to_string())
        })
    }

    /// Validates a document from one explicit CG-06.07 scoped context
    /// snapshot. Multiple source batches are merged only through the existing
    /// `ObservationEvidenceSet` validation boundary.
    pub fn validate_scoped_declarative_context(
        &self,
        context: DeclarativeContext,
        scoped: &ScopedContextSnapshot,
        observed_state: ObservedState,
        situation: Situation,
    ) -> Result<DeclarativeContextSituationDocument, SituationApplicationError> {
        let batches = scoped.batches();
        let records = if batches.is_empty() {
            None
        } else {
            let provenances = batches
                .iter()
                .flat_map(|batch| batch.records().provenances().iter().cloned())
                .collect::<Vec<_>>();
            let observations = batches
                .iter()
                .flat_map(|batch| batch.records().observations().iter().cloned())
                .collect::<Vec<_>>();
            let facts = batches
                .iter()
                .flat_map(|batch| batch.records().facts().iter().cloned())
                .collect::<Vec<_>>();
            let evidence = batches
                .iter()
                .flat_map(|batch| batch.records().evidence().iter().cloned())
                .collect::<Vec<_>>();
            Some(
                ObservationEvidenceSet::new(provenances, observations, facts, evidence)
                    .map_err(SituationApplicationError::from)?,
            )
        };
        self.validate_declarative_context(
            context,
            scoped.intent().cloned(),
            records,
            observed_state,
            situation,
        )
    }

    /// Delegates deterministic normalization to the CG-06 domain function.
    pub fn normalize_current_state(
        &self,
        id: gateway_domain::ObservedStateId,
        input: NormalizationInput,
    ) -> Result<ObservedState, SituationApplicationError> {
        domain_normalize_current_state(id, input).map_err(SituationApplicationError::from)
    }

    /// Delegates deterministic Situation assembly to the CG-06 domain input.
    pub fn assess_situation(
        &self,
        input: SituationAssemblyInput,
        id: SituationId,
    ) -> Result<Situation, SituationApplicationError> {
        input.assemble(id).map_err(SituationApplicationError::from)
    }

    /// Captures a read-only, definition-pinned process reference from CG-04.
    pub fn process_reference(
        &self,
        input: ProcessSnapshotInput<'_>,
    ) -> Result<ProcessSituationReference, SituationApplicationError> {
        ProcessSituationReference::capture(input)
    }

    /// Inspects a Situation together with optional CG-02 and CG-04 snapshots.
    pub fn inspect_situation(
        &self,
        document: &DeclarativeContextSituationDocument,
        execution_context: Option<&ExecutionContext>,
        process: Option<ProcessSnapshotInput<'_>>,
    ) -> Result<SituationInspection, SituationApplicationError> {
        let process = process
            .map(ProcessSituationReference::capture)
            .transpose()?;
        Ok(SituationInspection {
            context_id: document.context().id().clone(),
            observed_state_id: document.observed_state().id().clone(),
            situation: document.situation().clone(),
            operating_mode: execution_context.map(|context| context.operating_mode),
            execution_profile: execution_context.map(|context| context.execution_profile),
            process,
        })
    }

    /// Projects the domain explainability plus exact input snapshot identities.
    #[must_use]
    pub fn explain_situation(&self, inspection: &SituationInspection) -> SituationExplainability {
        let process = inspection.process.as_ref();
        SituationExplainability {
            context_id: inspection.context_id.clone(),
            observed_state_id: inspection.observed_state_id.clone(),
            situation_id: inspection.situation.id().clone(),
            operating_mode: inspection.operating_mode,
            execution_profile: inspection.execution_profile,
            process_definition_id: process.map(|value| value.definition_id().clone()),
            process_definition_version: process.map(ProcessSituationReference::definition_version),
            process_definition_digest: process.map(|value| value.definition_digest().clone()),
            process_instance_id: process.map(|value| value.instance_id().clone()),
            process_instance_revision: process.map(ProcessSituationReference::instance_revision),
            traces: inspection.situation.explainability(),
        }
    }

    /// Serializes a complete validated CG-06 document through its domain API.
    pub fn serialize_situation(
        &self,
        document: &DeclarativeContextSituationDocument,
    ) -> Result<String, SituationApplicationError> {
        document.to_json().map_err(|error| {
            SituationApplicationError::new("SERIALIZATION_ERROR", error.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use gateway_domain::{
        ContextScopeId, DeclarativeContextId, ExecutionContext, ExecutionProfile,
        NormalizationInput, ObservationEvidenceSet, ObservedStateId, OperatingMode,
        SituationAssemblyInput, SituationId, SourceId, SourceKind, SourceTimestamp, TaskDescriptor,
        TaskId,
    };
    use gateway_process::{
        ActivityDefinition, ActivityId, BlockerDefinition, EventTypeDefinition, EventTypeId,
        EvidenceRequirement, GateDefinition, GateId, GateStatus, GuardExpression,
        ProcessApplication, ProcessDefinitionBuilder, ProcessDefinitionId, ProcessInstance,
        ProcessInstanceId, ProcessInstanceRevision, ProcessInstanceStatus, StateDefinition,
        StateId, TransitionDefinition,
    };

    use super::*;
    use crate::external_context::{InMemoryContextStore, ScopedObservationBatch, SourceSnapshot};
    use crate::ports::inbound::{ObservationEvidenceInputPort, ScopeLifecyclePort};

    fn document() -> DeclarativeContextSituationDocument {
        let state = ObservedState::new_v1(ObservedStateId::new("state-1").unwrap());
        let situation = SituationAssemblyInput::new(state.clone())
            .assemble(SituationId::new("situation-1").unwrap())
            .unwrap();
        DeclarativeContextSituationDocument::new(
            DeclarativeContext::new_v1(DeclarativeContextId::new("context-1").unwrap()),
            None,
            None,
            state,
            situation,
        )
        .unwrap()
    }

    fn process_definition(id: &str) -> gateway_process::ProcessDefinition {
        let start = StateId::new("start").unwrap();
        let done = StateId::new("done").unwrap();
        let event = EventTypeId::new("finish").unwrap();
        ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new(id).unwrap(),
            gateway_process::ProcessDefinitionVersion::new(1).unwrap(),
        )
        .with_states([
            StateDefinition::new(start.clone(), true, false).unwrap(),
            StateDefinition::new(done, false, true).unwrap(),
        ])
        .with_events([EventTypeDefinition::new(event.clone())])
        .with_activities([ActivityDefinition::new(
            ActivityId::new("inspect").unwrap(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )])
        .with_gates([GateDefinition::new(
            GateId::new("review").unwrap(),
            vec![EvidenceRequirement::new(
                gateway_process::EvidenceTypeId::new("review-result").unwrap(),
                true,
            )],
        )])
        .with_evidence([EvidenceRequirement::new(
            gateway_process::EvidenceTypeId::new("review-result").unwrap(),
            true,
        )])
        .with_blockers([BlockerDefinition::new(
            gateway_process::BlockerId::new("review-blocker").unwrap(),
            "review is required",
            true,
        )
        .unwrap()])
        .with_transitions([TransitionDefinition::new(
            gateway_process::TransitionId::new("finish").unwrap(),
            start,
            event,
            StateId::new("done").unwrap(),
            GuardExpression::Always,
        )
        .with_authorized_activity(ActivityId::new("inspect").unwrap())])
        .build()
        .unwrap()
    }

    fn process_instance(definition: &gateway_process::ProcessDefinition) -> ProcessInstance {
        let mut instance =
            ProcessInstance::start(definition, ProcessInstanceId::new("instance-1").unwrap())
                .unwrap();
        instance.set_gate_status(GateId::new("review").unwrap(), GateStatus::Blocked);
        instance.record_evidence(gateway_process::EvidenceTypeId::new("review-result").unwrap());
        instance
            .add_context_reference("context-snapshot-1")
            .unwrap();
        instance
            .increment_retry(ActivityId::new("inspect").unwrap())
            .unwrap();
        instance.record_blocker(
            gateway_process::BlockerRuntimeState::new(
                gateway_process::BlockerId::new("review-blocker").unwrap(),
                "review is required",
                true,
            )
            .unwrap(),
        );
        instance
    }

    fn tampered_instance(
        instance: &ProcessInstance,
        field: &str,
        value: serde_json::Value,
    ) -> ProcessInstance {
        let mut serialized = serde_json::to_value(instance).unwrap();
        serialized[field] = value;
        serde_json::from_value(serialized).unwrap()
    }

    #[test]
    fn facade_delegates_domain_operations_and_keeps_process_data_read_only() {
        let app = DeclarativeSituationApplication::new();
        let records =
            ObservationEvidenceSet::new(Vec::new(), Vec::new(), Vec::new(), Vec::new()).unwrap();
        let state = app
            .normalize_current_state(
                ObservedStateId::new("state-1").unwrap(),
                NormalizationInput::new(records),
            )
            .unwrap();
        let situation = app
            .assess_situation(
                SituationAssemblyInput::new(state.clone()),
                SituationId::new("situation-1").unwrap(),
            )
            .unwrap();
        let document = app
            .validate_declarative_context(
                DeclarativeContext::new_v1(DeclarativeContextId::new("context-1").unwrap()),
                None,
                None,
                state,
                situation,
            )
            .unwrap();
        let json = app.serialize_situation(&document).unwrap();
        assert_eq!(json, document.to_json().unwrap());
        let store = InMemoryContextStore::new();
        let scope = store
            .open_scope(ContextScopeId::new("scope-1").unwrap())
            .unwrap();
        let scoped = store.snapshot(&scope).unwrap();
        let scoped_document = app
            .validate_scoped_declarative_context(
                document.context().clone(),
                &scoped,
                document.observed_state().clone(),
                document.situation().clone(),
            )
            .unwrap();
        assert_eq!(scoped_document, document);
        let batch = ScopedObservationBatch::new(
            scope.id().clone(),
            SourceSnapshot::new(
                SourceId::new("source-1").unwrap(),
                SourceKind::Synthetic,
                Some(SourceTimestamp::new("2026-08-29T10:00:00Z").unwrap()),
                None,
            )
            .unwrap(),
            ObservationEvidenceSet::new(Vec::new(), Vec::new(), Vec::new(), Vec::new()).unwrap(),
        )
        .unwrap();
        store.ingest_observations(&scope, batch).unwrap();
        let scoped = store.snapshot(&scope).unwrap();
        let scoped_document = app
            .validate_scoped_declarative_context(
                document.context().clone(),
                &scoped,
                document.observed_state().clone(),
                document.situation().clone(),
            )
            .unwrap();
        assert!(scoped_document.records().is_some());
        let empty_inspection = app.inspect_situation(&document, None, None).unwrap();
        assert!(empty_inspection.operating_mode().is_none());
        assert!(empty_inspection.execution_profile().is_none());
        assert!(empty_inspection.process().is_none());
        assert!(app.explain_situation(&empty_inspection).traces().is_empty());
    }

    #[test]
    fn inspection_reuses_cg02_types_and_captures_cg04_identity_and_state() {
        let app = DeclarativeSituationApplication::new();
        let definition = process_definition("runtime-definition");
        let instance_before = process_instance(&definition);
        let snapshot_input = ProcessSnapshotInput::new(&definition, &instance_before)
            .requiring_revision(instance_before.revision());
        assert_eq!(
            snapshot_input.definition().identity().id().as_str(),
            "runtime-definition"
        );
        assert_eq!(snapshot_input.instance().id().as_str(), "instance-1");
        assert_eq!(
            snapshot_input.expected_revision().unwrap().value(),
            instance_before.revision().value()
        );
        let execution_context = ExecutionContext {
            task: TaskDescriptor::new(TaskId::new("task-1").unwrap(), "inspect").unwrap(),
            operating_mode: OperatingMode::Hardening,
            execution_profile: ExecutionProfile::FullPath,
        };
        let document = document();
        let inspection = app
            .inspect_situation(&document, Some(&execution_context), Some(snapshot_input))
            .unwrap();
        assert_eq!(inspection.context_id().as_str(), "context-1");
        assert_eq!(inspection.observed_state_id().as_str(), "state-1");
        assert_eq!(inspection.situation().id().as_str(), "situation-1");
        let process = inspection.process().unwrap();
        assert_eq!(process.definition_id().as_str(), "runtime-definition");
        assert_eq!(process.definition_version().value(), 1);
        assert_eq!(process.definition_digest().as_str().len(), 64);
        assert_eq!(process.instance_id().as_str(), "instance-1");
        assert_eq!(process.instance_revision().value(), 0);
        assert_eq!(process.current_state().as_str(), "start");
        assert!(process.previous_state().is_none());
        assert_eq!(process.status(), ProcessInstanceStatus::Running);
        assert_eq!(process.active_gates().len(), 1);
        assert_eq!(process.blockers().len(), 1);
        assert_eq!(process.evidence().len(), 1);
        assert_eq!(process.retry_attempts().len(), 1);
        assert_eq!(process.context_references().len(), 1);
        assert!(process.history().is_empty());
        assert!(process.waiting_condition().is_none());
        assert_eq!(process.authorized_activities().len(), 1);
        assert_eq!(process.inspection().instance(), &instance_before);
        assert_eq!(instance_before, *process.inspection().instance());
        assert_eq!(inspection.operating_mode(), Some(OperatingMode::Hardening));
        assert_eq!(
            inspection.execution_profile(),
            Some(ExecutionProfile::FullPath)
        );
        let explanation = app.explain_situation(&inspection);
        assert_eq!(explanation.context_id().as_str(), "context-1");
        assert_eq!(explanation.observed_state_id().as_str(), "state-1");
        assert_eq!(explanation.situation_id().as_str(), "situation-1");
        assert_eq!(
            explanation.process_definition_id().unwrap().as_str(),
            "runtime-definition"
        );
        assert_eq!(explanation.process_definition_version().unwrap().value(), 1);
        assert_eq!(
            explanation
                .process_definition_digest()
                .unwrap()
                .as_str()
                .len(),
            64
        );
        assert_eq!(
            explanation.process_instance_id().unwrap().as_str(),
            "instance-1"
        );
        assert_eq!(explanation.process_instance_revision().unwrap().value(), 0);
        assert_eq!(explanation.operating_mode(), Some(OperatingMode::Hardening));
        assert_eq!(
            explanation.execution_profile(),
            Some(ExecutionProfile::FullPath)
        );
        assert_eq!(instance_before, *process.inspection().instance());
    }

    #[test]
    fn process_identity_and_revision_mismatches_fail_closed_without_mutation() {
        let app = DeclarativeSituationApplication::new();
        let definition = process_definition("runtime-definition");
        let instance = process_instance(&definition);
        let before = instance.clone();
        let stale = ProcessSnapshotInput::new(&definition, &instance)
            .requiring_revision(ProcessInstanceRevision::new(9));
        let stale_error = app.process_reference(stale).unwrap_err();
        assert_eq!(stale_error.code(), "STALE_REVISION");

        let mismatched_definition = process_definition("different-definition");
        let mismatch_error = app
            .process_reference(ProcessSnapshotInput::new(&mismatched_definition, &instance))
            .unwrap_err();
        assert_eq!(mismatch_error.code(), "DEFINITION_IDENTITY_CONFLICT");
        assert_eq!(instance, before);

        let unknown_gate = {
            let mut value = instance.clone();
            value.set_gate_status(GateId::new("unknown-gate").unwrap(), GateStatus::Open);
            value
        };
        assert_eq!(
            app.process_reference(ProcessSnapshotInput::new(&definition, &unknown_gate))
                .unwrap_err()
                .code(),
            "INVALID_PROCESS_SNAPSHOT"
        );
        let unknown_blocker = {
            let mut value = instance.clone();
            value.record_blocker(
                gateway_process::BlockerRuntimeState::new(
                    gateway_process::BlockerId::new("unknown-blocker").unwrap(),
                    "unknown",
                    true,
                )
                .unwrap(),
            );
            value
        };
        assert_eq!(
            app.process_reference(ProcessSnapshotInput::new(&definition, &unknown_blocker))
                .unwrap_err()
                .code(),
            "INVALID_PROCESS_SNAPSHOT"
        );
        let unknown_evidence = {
            let mut value = instance.clone();
            value
                .record_evidence(gateway_process::EvidenceTypeId::new("unknown-evidence").unwrap());
            value
        };
        assert_eq!(
            app.process_reference(ProcessSnapshotInput::new(&definition, &unknown_evidence))
                .unwrap_err()
                .code(),
            "INVALID_PROCESS_SNAPSHOT"
        );
        let unknown_retry = {
            let mut value = instance.clone();
            value
                .increment_retry(ActivityId::new("unknown-activity").unwrap())
                .unwrap();
            value
        };
        assert_eq!(
            app.process_reference(ProcessSnapshotInput::new(&definition, &unknown_retry))
                .unwrap_err()
                .code(),
            "INVALID_PROCESS_SNAPSHOT"
        );
        let unknown_state = tampered_instance(&instance, "current_state", json!("unknown-state"));
        assert_eq!(
            app.process_reference(ProcessSnapshotInput::new(&definition, &unknown_state))
                .unwrap_err()
                .code(),
            "INVALID_PROCESS_SNAPSHOT"
        );
        let paused_without_condition = tampered_instance(&instance, "status", json!("PAUSED"));
        assert_eq!(
            app.process_reference(ProcessSnapshotInput::new(
                &definition,
                &paused_without_condition,
            ))
            .unwrap_err()
            .code(),
            "INVALID_PROCESS_SNAPSHOT"
        );
        let invalid_reference = tampered_instance(&instance, "context_references", json!([""]));
        assert_eq!(
            app.process_reference(ProcessSnapshotInput::new(&definition, &invalid_reference))
                .unwrap_err()
                .code(),
            "INVALID_PROCESS_SNAPSHOT"
        );
        let mut definition_value = serde_json::to_value(&definition).unwrap();
        definition_value["identity"]["digest"] = json!("0".repeat(64));
        let tampered_definition: gateway_process::ProcessDefinition =
            serde_json::from_value(definition_value).unwrap();
        assert_eq!(
            app.process_reference(ProcessSnapshotInput::new(&tampered_definition, &instance,))
                .unwrap_err()
                .code(),
            "NON_CANONICAL_DEFINITION"
        );

        let process_error = ProcessApplication::new()
            .compile_process_source("not valid process source")
            .unwrap_err();
        let application_error = SituationApplicationError::from(process_error);
        assert_eq!(application_error.code(), "COMPILATION_ERROR");
        assert!(format!("{application_error}").contains("COMPILATION_ERROR"));
        let instance_error = gateway_process::BlockerRuntimeState::new(
            gateway_process::BlockerId::new("invalid-blocker").unwrap(),
            "",
            true,
        )
        .unwrap_err();
        assert_eq!(
            SituationApplicationError::from(instance_error).code(),
            "INVALID_BLOCKER"
        );
        let domain_error =
            SituationApplicationError::from(ValidationError::InvalidDeclarativeValue {
                reason: "test validation",
            });
        assert_eq!(domain_error.code(), "DOMAIN_VALIDATION_ERROR");
    }

    #[test]
    fn invalid_document_is_reported_at_the_application_boundary() {
        let app = DeclarativeSituationApplication::new();
        let context = DeclarativeContext::new_v1(DeclarativeContextId::new("context-1").unwrap());
        let state = ObservedState::new_v1(ObservedStateId::new("state-1").unwrap());
        let invalid_situation = Situation::new_v1(SituationId::new("situation-1").unwrap());
        let error = app
            .validate_declarative_context(context, None, None, state, invalid_situation)
            .unwrap_err();
        assert_eq!(error.code(), "DECLARATIVE_CONTEXT_INVALID");
        assert!(error.message().contains("observed_state"));
    }
}
