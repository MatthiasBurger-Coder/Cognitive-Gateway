//! Stable application-facing ports over the deterministic process core.
//!
//! This module composes the existing compiler, validator, registry, evaluator
//! and atomic mutation ports. It contains no persistence, provider or runtime
//! adapter implementation.

use serde::Serialize;

use crate::{
    ActivityId, AtomicProcessMutation, AuthorizedActivity, BlockerId, BlockerRuntimeState,
    CommitOutcome, CompilationError, CompilationResult, ConstraintEvaluation, EvaluationInputs,
    EventOccurrence, EventOccurrenceId, EventTypeId, EvidenceTypeId, GuardEvaluation,
    InstanceError, LifecycleController, MutationError, PauseReason, ProcessDefinition,
    ProcessDefinitionDigest, ProcessDefinitionId, ProcessDefinitionVersion, ProcessInstance,
    ProcessInstanceId, ProcessInstanceRevision, ProcessRegistry, ProcessRegistryError,
    ProcessValidator, RetryOutcome, SemanticCompiler, StateId, TransitionDecision, TransitionId,
    ValidationReport,
};

/// Stable application-boundary failure. It preserves the reason code while
/// keeping parser, storage and provider error types out of the public API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationError {
    code: &'static str,
    message: String,
}

impl ApplicationError {
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

impl std::fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApplicationError {}

impl From<CompilationError> for ApplicationError {
    fn from(error: CompilationError) -> Self {
        Self::new("COMPILATION_ERROR", error.to_string())
    }
}

impl From<ProcessRegistryError> for ApplicationError {
    fn from(error: ProcessRegistryError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

impl From<InstanceError> for ApplicationError {
    fn from(error: InstanceError) -> Self {
        Self::new(error.code(), error.message())
    }
}

impl From<MutationError> for ApplicationError {
    fn from(error: MutationError) -> Self {
        Self::new(error.code(), error.message())
    }
}

/// One stable, machine-readable catalog entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProcessDefinitionSummary {
    source_path: String,
    id: ProcessDefinitionId,
    version: ProcessDefinitionVersion,
    digest: ProcessDefinitionDigest,
}

impl ProcessDefinitionSummary {
    #[must_use]
    pub fn source_path(&self) -> &str {
        &self.source_path
    }

    #[must_use]
    pub fn id(&self) -> &ProcessDefinitionId {
        &self.id
    }

    #[must_use]
    pub const fn version(&self) -> ProcessDefinitionVersion {
        self.version
    }

    #[must_use]
    pub fn digest(&self) -> &ProcessDefinitionDigest {
        &self.digest
    }
}

/// Read-only inspection projection for one pinned process instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProcessInspection {
    definition_id: ProcessDefinitionId,
    definition_version: ProcessDefinitionVersion,
    definition_digest: ProcessDefinitionDigest,
    instance: ProcessInstance,
    authorized_activities: Vec<AuthorizedActivity>,
}

impl ProcessInspection {
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
    pub fn instance(&self) -> &ProcessInstance {
        &self.instance
    }

    #[must_use]
    pub fn authorized_activities(&self) -> &[AuthorizedActivity] {
        &self.authorized_activities
    }

    /// Returns deterministic JSON without exposing application implementation
    /// details or provider-specific runtime values.
    pub fn to_json(&self) -> Result<String, ApplicationError> {
        serde_json::to_string(self)
            .map_err(|error| ApplicationError::new("SERIALIZATION_ERROR", error.to_string()))
    }
}

/// Result of applying an event. Evaluation rejection is a value, while a
/// mutation/storage failure remains an application error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyEventResult {
    Rejected {
        decision: TransitionDecision,
    },
    Committed {
        decision: TransitionDecision,
        outcome: CommitOutcome,
    },
}

impl ApplyEventResult {
    #[must_use]
    pub fn decision(&self) -> &TransitionDecision {
        match self {
            Self::Rejected { decision } | Self::Committed { decision, .. } => decision,
        }
    }

    #[must_use]
    pub const fn outcome(&self) -> Option<CommitOutcome> {
        match self {
            Self::Rejected { .. } => None,
            Self::Committed { outcome, .. } => Some(*outcome),
        }
    }
}

/// A simulation result is explicitly hypothetical and never commits state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulationResult {
    decision: TransitionDecision,
    hypothetical: bool,
}

impl SimulationResult {
    #[must_use]
    pub const fn decision(&self) -> &TransitionDecision {
        &self.decision
    }

    #[must_use]
    pub const fn hypothetical(&self) -> bool {
        self.hypothetical
    }
}

/// Machine-readable compilation trace projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompilationTrace {
    line: usize,
    column: usize,
    construct: String,
    target: String,
}

impl CompilationTrace {
    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }

    #[must_use]
    pub fn construct(&self) -> &str {
        &self.construct
    }

    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }
}

/// Machine-readable compilation explanation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompilationExplanation {
    definition_id: ProcessDefinitionId,
    definition_version: ProcessDefinitionVersion,
    definition_digest: ProcessDefinitionDigest,
    trace: Vec<CompilationTrace>,
}

impl CompilationExplanation {
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
    pub fn trace(&self) -> &[CompilationTrace] {
        &self.trace
    }

    pub fn to_json(&self) -> Result<String, ApplicationError> {
        serde_json::to_string(self)
            .map_err(|error| ApplicationError::new("SERIALIZATION_ERROR", error.to_string()))
    }

    #[must_use]
    pub fn human_readable(&self) -> String {
        format!(
            "compiled {} v{} ({}) with {} trace entries",
            self.definition_id,
            self.definition_version,
            self.definition_digest,
            self.trace.len()
        )
    }
}

/// Machine-readable runtime decision explanation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeExplanation {
    definition_id: ProcessDefinitionId,
    definition_version: ProcessDefinitionVersion,
    definition_digest: ProcessDefinitionDigest,
    instance_id: ProcessInstanceId,
    instance_revision: ProcessInstanceRevision,
    event_occurrence_id: EventOccurrenceId,
    event_type: EventTypeId,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    previous_state: StateId,
    resulting_state: Option<StateId>,
    matched_transition: Option<TransitionId>,
    accepted: bool,
    reason_code: String,
    reason: String,
    guard_evaluations: Vec<GuardEvaluation>,
    constraint_evaluations: Vec<ConstraintEvaluation>,
    authorized_activity: Option<AuthorizedActivity>,
}

impl RuntimeExplanation {
    pub fn to_json(&self) -> Result<String, ApplicationError> {
        serde_json::to_string(self)
            .map_err(|error| ApplicationError::new("SERIALIZATION_ERROR", error.to_string()))
    }

    #[must_use]
    pub fn human_readable(&self) -> String {
        let transition = self
            .matched_transition
            .as_ref()
            .map_or_else(|| "none".to_owned(), ToString::to_string);
        format!(
            "{} {} for {} at revision {}: {} ({}) via {}",
            if self.accepted {
                "accepted"
            } else {
                "rejected"
            },
            self.event_type,
            self.instance_id,
            self.instance_revision.value(),
            self.reason_code,
            self.reason,
            transition
        )
    }

    #[must_use]
    pub fn reason_code(&self) -> &str {
        &self.reason_code
    }

    #[must_use]
    pub fn authorized_activity(&self) -> Option<&AuthorizedActivity> {
        self.authorized_activity.as_ref()
    }
}

/// Stateless application service. Every operation delegates to one of the
/// canonical process-core contracts; it does not own authoritative state.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessApplication;

impl ProcessApplication {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub fn compile_process_source(
        &self,
        source: &str,
    ) -> Result<CompilationResult, ApplicationError> {
        SemanticCompiler::compile(source).map_err(ApplicationError::from)
    }

    #[must_use]
    pub fn validate_process_definition(&self, definition: &ProcessDefinition) -> ValidationReport {
        ProcessValidator::validate(definition)
    }

    #[must_use]
    pub fn validate_process_definition_with_capabilities(
        &self,
        definition: &ProcessDefinition,
        capabilities: &[gateway_domain::CapabilityId],
    ) -> ValidationReport {
        ProcessValidator::validate_with_capabilities(definition, capabilities)
    }

    pub fn list_process_definitions(
        &self,
        registry: &ProcessRegistry,
    ) -> Vec<ProcessDefinitionSummary> {
        registry
            .entries()
            .iter()
            .map(|entry| {
                let identity = entry.definition().identity();
                ProcessDefinitionSummary {
                    source_path: entry.source_path().to_owned(),
                    id: identity.id().clone(),
                    version: identity.version(),
                    digest: identity.digest().clone(),
                }
            })
            .collect()
    }

    #[must_use]
    pub fn get_process_definition<'a>(
        &self,
        registry: &'a ProcessRegistry,
        id: &ProcessDefinitionId,
        version: ProcessDefinitionVersion,
    ) -> Option<&'a ProcessDefinition> {
        registry.get(id, version)
    }

    #[must_use]
    pub fn resolve_process_definition<'a>(
        &self,
        registry: &'a ProcessRegistry,
        id: &ProcessDefinitionId,
        version: Option<ProcessDefinitionVersion>,
    ) -> Option<&'a ProcessDefinition> {
        registry.resolve(id, version)
    }

    pub fn start_process(
        &self,
        definition: &ProcessDefinition,
        id: ProcessInstanceId,
    ) -> Result<ProcessInstance, ApplicationError> {
        ProcessInstance::start(definition, id).map_err(ApplicationError::from)
    }

    pub fn inspect_process(
        &self,
        definition: &ProcessDefinition,
        instance: &ProcessInstance,
    ) -> Result<ProcessInspection, ApplicationError> {
        instance
            .require_definition(definition)
            .map_err(ApplicationError::from)?;
        let mut authorized_activities = Vec::new();
        for transition in definition
            .transitions()
            .iter()
            .filter(|transition| transition.from() == instance.current_state())
        {
            if let Some(activity) = transition.authorized_activity() {
                if let Some(definition) = definition
                    .activities()
                    .iter()
                    .find(|candidate| candidate.id() == activity)
                {
                    let projected = AuthorizedActivity::from_definition(definition);
                    if !authorized_activities.contains(&projected) {
                        authorized_activities.push(projected);
                    }
                }
            }
        }
        authorized_activities.sort_by(|left, right| left.id().cmp(right.id()));
        let identity = definition.identity();
        Ok(ProcessInspection {
            definition_id: identity.id().clone(),
            definition_version: identity.version(),
            definition_digest: identity.digest().clone(),
            instance: instance.clone(),
            authorized_activities,
        })
    }

    #[must_use]
    pub fn evaluate_event(
        &self,
        definition: &ProcessDefinition,
        instance: &ProcessInstance,
        event: &EventOccurrence,
        inputs: &EvaluationInputs,
    ) -> TransitionDecision {
        crate::TransitionEvaluator::evaluate(definition, instance, event, inputs)
    }

    pub fn commit_transition<M: AtomicProcessMutation>(
        &self,
        mutation: &mut M,
        definition: &ProcessDefinition,
        event: &EventOccurrence,
        decision: &TransitionDecision,
    ) -> Result<CommitOutcome, ApplicationError> {
        if decision.occurrence() != event.id() {
            return Err(ApplicationError::new(
                "OCCURRENCE_IDENTITY_CONFLICT",
                "decision and event occurrence identities differ",
            ));
        }
        if !decision.accepted() {
            return Err(ApplicationError::new(
                decision.code().as_str(),
                decision.reason(),
            ));
        }
        let transition_id = decision.matched_transition().ok_or_else(|| {
            ApplicationError::new("MISSING_TRANSITION", "accepted decision has no transition")
        })?;
        let transition = definition
            .transitions()
            .iter()
            .find(|candidate| candidate.id() == transition_id)
            .ok_or_else(|| ApplicationError::new("UNKNOWN_TRANSITION", "transition is missing"))?;
        if transition.event() != event.event_type() {
            return Err(ApplicationError::new(
                "EVENT_TYPE_CONFLICT",
                "decision transition and event type identities differ",
            ));
        }
        let projection = decision.projection().cloned().ok_or_else(|| {
            ApplicationError::new(
                "MISSING_TRANSITION_PROJECTION",
                "accepted decision has no transition projection",
            )
        })?;
        mutation
            .commit_transition(definition, event, projection)
            .map_err(ApplicationError::from)
    }

    pub fn apply_event_atomically<M: AtomicProcessMutation>(
        &self,
        mutation: &mut M,
        definition: &ProcessDefinition,
        instance: &ProcessInstance,
        event: &EventOccurrence,
        inputs: &EvaluationInputs,
    ) -> Result<ApplyEventResult, ApplicationError> {
        let decision = self.evaluate_event(definition, instance, event, inputs);
        if !decision.accepted() {
            return Ok(ApplyEventResult::Rejected { decision });
        }
        let outcome = self.commit_transition(mutation, definition, event, &decision)?;
        Ok(ApplyEventResult::Committed { decision, outcome })
    }

    #[must_use]
    pub fn simulate_transition(
        &self,
        definition: &ProcessDefinition,
        instance: &ProcessInstance,
        event: &EventOccurrence,
        inputs: &EvaluationInputs,
    ) -> SimulationResult {
        SimulationResult {
            decision: self.evaluate_event(definition, instance, event, inputs),
            hypothetical: true,
        }
    }

    pub fn explain_compilation(&self, result: &CompilationResult) -> CompilationExplanation {
        let identity = result.definition().identity();
        CompilationExplanation {
            definition_id: identity.id().clone(),
            definition_version: identity.version(),
            definition_digest: identity.digest().clone(),
            trace: result
                .trace()
                .iter()
                .map(|entry| CompilationTrace {
                    line: entry.location().line(),
                    column: entry.location().column(),
                    construct: entry.construct().to_owned(),
                    target: entry.target().to_owned(),
                })
                .collect(),
        }
    }

    pub fn explain_transition(
        &self,
        definition: &ProcessDefinition,
        instance: &ProcessInstance,
        event: &EventOccurrence,
        decision: &TransitionDecision,
    ) -> RuntimeExplanation {
        RuntimeExplanation {
            definition_id: definition.identity().id().clone(),
            definition_version: definition.identity().version(),
            definition_digest: definition.identity().digest().clone(),
            instance_id: instance.id().clone(),
            instance_revision: instance.revision(),
            event_occurrence_id: event.id().clone(),
            event_type: event.event_type().clone(),
            correlation_id: event.correlation_id().map(ToString::to_string),
            causation_id: event.causation_id().map(ToString::to_string),
            previous_state: decision.previous_state().clone(),
            resulting_state: decision.resulting_state().cloned(),
            matched_transition: decision.matched_transition().cloned(),
            accepted: decision.accepted(),
            reason_code: decision.code().as_str().to_owned(),
            reason: decision.reason().to_owned(),
            guard_evaluations: decision.guard_evaluations().to_vec(),
            constraint_evaluations: decision.constraint_evaluations().to_vec(),
            authorized_activity: decision.authorized_activity_definition().cloned(),
        }
    }

    pub fn record_evidence(
        &self,
        definition: &ProcessDefinition,
        instance: &mut ProcessInstance,
        evidence: EvidenceTypeId,
    ) -> Result<(), ApplicationError> {
        instance
            .require_definition(definition)
            .map_err(ApplicationError::from)?;
        if !definition
            .evidence()
            .iter()
            .any(|candidate| candidate.evidence_type() == &evidence)
        {
            return Err(ApplicationError::new(
                "UNKNOWN_EVIDENCE",
                "evidence is not declared by the definition",
            ));
        }
        instance.record_evidence(evidence);
        Ok(())
    }

    pub fn record_blocker(
        &self,
        definition: &ProcessDefinition,
        instance: &mut ProcessInstance,
        blocker: BlockerRuntimeState,
    ) -> Result<(), ApplicationError> {
        instance
            .require_definition(definition)
            .map_err(ApplicationError::from)?;
        if !definition
            .blockers()
            .iter()
            .any(|candidate| candidate.id() == blocker.id())
        {
            return Err(ApplicationError::new(
                "UNKNOWN_BLOCKER",
                "blocker is not declared by the definition",
            ));
        }
        instance.record_blocker(blocker);
        Ok(())
    }

    pub fn resolve_blocker(
        &self,
        instance: &mut ProcessInstance,
        blocker: &BlockerId,
    ) -> Result<(), ApplicationError> {
        instance
            .resolve_blocker(blocker)
            .map_err(ApplicationError::from)
    }

    pub fn pause_process(
        &self,
        instance: &mut ProcessInstance,
        reason: PauseReason,
        detail: impl Into<String>,
    ) -> Result<(), ApplicationError> {
        LifecycleController::pause(instance, reason, detail).map_err(ApplicationError::from)
    }

    pub fn resume_process(
        &self,
        instance: &mut ProcessInstance,
        condition_revalidated: bool,
    ) -> Result<(), ApplicationError> {
        LifecycleController::resume(instance, condition_revalidated).map_err(ApplicationError::from)
    }

    pub fn retry_process(
        &self,
        instance: &mut ProcessInstance,
        activity: ActivityId,
        max_attempts: u32,
    ) -> Result<RetryOutcome, ApplicationError> {
        LifecycleController::retry(instance, activity, max_attempts).map_err(ApplicationError::from)
    }

    pub fn complete_process<M: AtomicProcessMutation>(
        &self,
        mutation: &mut M,
        definition: &ProcessDefinition,
        instance: &ProcessInstance,
        event: &EventOccurrence,
        inputs: &EvaluationInputs,
    ) -> Result<ApplyEventResult, ApplicationError> {
        let decision = self.evaluate_event(definition, instance, event, inputs);
        if !decision.accepted() {
            return Ok(ApplyEventResult::Rejected { decision });
        }
        let transition_id = decision.matched_transition().ok_or_else(|| {
            ApplicationError::new("MISSING_TRANSITION", "accepted decision has no transition")
        })?;
        let transition = definition
            .transitions()
            .iter()
            .find(|candidate| candidate.id() == transition_id)
            .ok_or_else(|| ApplicationError::new("UNKNOWN_TRANSITION", "transition is missing"))?;
        let target_is_terminal = definition
            .states()
            .iter()
            .any(|state| state.id() == transition.to() && state.is_terminal());
        if !transition.completes() && !target_is_terminal {
            return Err(ApplicationError::new(
                "NOT_COMPLETION_TRANSITION",
                "event does not complete the process",
            ));
        }
        let outcome = self.commit_transition(mutation, definition, event, &decision)?;
        Ok(ApplyEventResult::Committed { decision, outcome })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EventOccurrenceId, EventTypeDefinition, EventTypeId, GuardExpression, InMemoryProcessStore,
        ProcessDefinitionBuilder, ProcessDefinitionId, ProcessInstanceId, ProcessInstanceStatus,
        ProcessSource, StateDefinition, TransitionDecisionCode, TransitionDefinition,
    };

    const SOURCE: &str = include_str!("../fixtures/strict-cognitive-gherkin/valid.feature");

    fn definition() -> ProcessDefinition {
        ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("application-example").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .with_states([
            StateDefinition::new(crate::StateId::new("start").unwrap(), true, false).unwrap(),
            StateDefinition::new(crate::StateId::new("done").unwrap(), false, true).unwrap(),
        ])
        .with_events([EventTypeDefinition::new(
            crate::EventTypeId::new("finish").unwrap(),
        )])
        .with_transitions([TransitionDefinition::new(
            TransitionId::new("finish").unwrap(),
            crate::StateId::new("start").unwrap(),
            EventTypeId::new("finish").unwrap(),
            crate::StateId::new("done").unwrap(),
            GuardExpression::Always,
        )])
        .build()
        .unwrap()
    }

    fn event(instance: &ProcessInstance) -> EventOccurrence {
        EventOccurrence::new(
            EventOccurrenceId::new("occurrence-1").unwrap(),
            EventTypeId::new("finish").unwrap(),
            instance.id().clone(),
            instance.revision(),
        )
    }

    #[test]
    fn delegates_catalog_compile_validate_and_resolution_ports() {
        let app = ProcessApplication::new();
        let result = app.compile_process_source(SOURCE).unwrap();
        assert!(
            app.validate_process_definition(result.definition())
                .is_valid()
        );
        let explanation = app.explain_compilation(&result);
        assert!(!explanation.trace().is_empty());
        assert!(
            explanation
                .to_json()
                .unwrap()
                .contains("canonical-issue-lifecycle")
        );
        assert!(explanation.human_readable().contains("compiled"));

        let registry =
            ProcessRegistry::from_sources([ProcessSource::new("one.feature", SOURCE)]).unwrap();
        let summaries = app.list_process_definitions(&registry);
        assert_eq!(summaries.len(), 1);
        let id = ProcessDefinitionId::new("canonical-issue-lifecycle").unwrap();
        let version = ProcessDefinitionVersion::new(1).unwrap();
        assert!(
            app.get_process_definition(&registry, &id, version)
                .is_some()
        );
        assert!(
            app.resolve_process_definition(&registry, &id, None)
                .is_some()
        );
    }

    #[test]
    fn separates_simulation_evaluation_and_atomic_commit() {
        let app = ProcessApplication::new();
        let definition = definition();
        let instance = app
            .start_process(&definition, ProcessInstanceId::new("run-1").unwrap())
            .unwrap();
        let occurrence = event(&instance);
        let simulation = app.simulate_transition(
            &definition,
            &instance,
            &occurrence,
            &EvaluationInputs::default(),
        );
        assert!(simulation.hypothetical());
        assert!(simulation.decision().accepted());
        let explanation =
            app.explain_transition(&definition, &instance, &occurrence, simulation.decision());
        assert_eq!(explanation.reason_code(), "ACCEPTED");
        assert!(explanation.to_json().unwrap().contains("occurrence-1"));
        assert!(explanation.human_readable().contains("accepted"));

        let mut store = InMemoryProcessStore::default();
        store.insert(instance.clone());
        let result = app
            .apply_event_atomically(
                &mut store,
                &definition,
                &instance,
                &occurrence,
                &EvaluationInputs::default(),
            )
            .unwrap();
        assert!(
            matches!(result.outcome(), Some(CommitOutcome::Applied { revision }) if revision.value() == 1)
        );
        let duplicate = app
            .apply_event_atomically(
                &mut store,
                &definition,
                &instance,
                &occurrence,
                &EvaluationInputs::default(),
            )
            .unwrap();
        assert!(matches!(
            duplicate.outcome(),
            Some(CommitOutcome::Duplicate { .. })
        ));
    }

    #[test]
    fn inspection_projects_declared_activities_and_lifecycle_ports_validate_inputs() {
        let app = ProcessApplication::new();
        let definition = definition();
        let mut instance = app
            .start_process(&definition, ProcessInstanceId::new("run-1").unwrap())
            .unwrap();
        let inspection = app.inspect_process(&definition, &instance).unwrap();
        assert!(inspection.authorized_activities().is_empty());
        assert!(
            inspection
                .to_json()
                .unwrap()
                .contains("application-example")
        );

        let evidence = EvidenceTypeId::new("report").unwrap();
        assert_eq!(
            app.record_evidence(&definition, &mut instance, evidence)
                .unwrap_err()
                .code(),
            "UNKNOWN_EVIDENCE"
        );
        app.pause_process(&mut instance, PauseReason::HumanReview, "review")
            .unwrap();
        assert_eq!(instance.status(), ProcessInstanceStatus::Paused);
        assert_eq!(
            app.resume_process(&mut instance, false).unwrap_err().code(),
            "WAITING_CONDITION_NOT_CLEARED"
        );
        app.resume_process(&mut instance, true).unwrap();
        assert_eq!(instance.status(), ProcessInstanceStatus::Running);
    }

    #[test]
    fn rejected_decisions_remain_values_and_commit_rejects_them() {
        let app = ProcessApplication::new();
        let definition = definition();
        let instance = app
            .start_process(&definition, ProcessInstanceId::new("run-1").unwrap())
            .unwrap();
        let unknown = EventOccurrence::new(
            EventOccurrenceId::new("unknown").unwrap(),
            EventTypeId::new("missing").unwrap(),
            instance.id().clone(),
            instance.revision(),
        );
        let decision = app.evaluate_event(
            &definition,
            &instance,
            &unknown,
            &EvaluationInputs::default(),
        );
        assert_eq!(decision.code(), TransitionDecisionCode::UnknownEvent);
        let mut store = InMemoryProcessStore::default();
        store.insert(instance);
        assert_eq!(
            app.commit_transition(&mut store, &definition, &unknown, &decision)
                .unwrap_err()
                .code(),
            "UNKNOWN_EVENT"
        );
    }
}
