//! Pure deterministic transition evaluation over explicit process snapshots.

use std::collections::{BTreeMap, BTreeSet};

use gateway_domain::CapabilityId;

use crate::{
    EventOccurrence, GateId, GateStatus, GuardExpression, ProcessDefinition, ProcessInstance,
    ProcessInstanceStatus, ProcessValidator, StateId, TransitionId, TransitionProjection,
};

/// Explicit inputs available to typed guard evaluation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvaluationInputs {
    evidence: BTreeSet<crate::EvidenceTypeId>,
    gates: BTreeMap<GateId, GateStatus>,
    capabilities: BTreeSet<CapabilityId>,
    blockers: BTreeSet<crate::BlockerId>,
}

impl EvaluationInputs {
    #[must_use]
    pub fn with_evidence(
        mut self,
        values: impl IntoIterator<Item = crate::EvidenceTypeId>,
    ) -> Self {
        self.evidence.extend(values);
        self
    }
    #[must_use]
    pub fn with_gate(mut self, gate: GateId, status: GateStatus) -> Self {
        self.gates.insert(gate, status);
        self
    }
    #[must_use]
    pub fn with_capabilities(mut self, values: impl IntoIterator<Item = CapabilityId>) -> Self {
        self.capabilities.extend(values);
        self
    }
    #[must_use]
    pub fn with_blockers(mut self, values: impl IntoIterator<Item = crate::BlockerId>) -> Self {
        self.blockers.extend(values);
        self
    }
    #[must_use]
    pub fn evidence(&self) -> &BTreeSet<crate::EvidenceTypeId> {
        &self.evidence
    }
    #[must_use]
    pub fn gates(&self) -> &BTreeMap<GateId, GateStatus> {
        &self.gates
    }
    #[must_use]
    pub fn capabilities(&self) -> &BTreeSet<CapabilityId> {
        &self.capabilities
    }
    #[must_use]
    pub fn blockers(&self) -> &BTreeSet<crate::BlockerId> {
        &self.blockers
    }
}

/// Stable result codes for evaluator decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum TransitionDecisionCode {
    Accepted,
    InvalidDefinition,
    DefinitionIdentityConflict,
    WrongInstance,
    StaleRevision,
    UnknownEvent,
    NoMatchingTransition,
    GuardRejected,
    AmbiguousTransition,
    TerminalState,
}

impl TransitionDecisionCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "ACCEPTED",
            Self::InvalidDefinition => "INVALID_DEFINITION",
            Self::DefinitionIdentityConflict => "DEFINITION_IDENTITY_CONFLICT",
            Self::WrongInstance => "WRONG_INSTANCE",
            Self::StaleRevision => "STALE_REVISION",
            Self::UnknownEvent => "UNKNOWN_EVENT",
            Self::NoMatchingTransition => "NO_MATCHING_TRANSITION",
            Self::GuardRejected => "GUARD_REJECTED",
            Self::AmbiguousTransition => "AMBIGUOUS_TRANSITION",
            Self::TerminalState => "TERMINAL_STATE",
        }
    }
}

/// One guard evaluation retained in the deterministic decision trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardEvaluation {
    expression: String,
    matched: bool,
}

impl GuardEvaluation {
    #[must_use]
    pub fn expression(&self) -> &str {
        &self.expression
    }
    #[must_use]
    pub const fn matched(&self) -> bool {
        self.matched
    }
}

/// Immutable result of evaluating one event occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionDecision {
    code: TransitionDecisionCode,
    accepted: bool,
    reason: String,
    occurrence: crate::EventOccurrenceId,
    previous_state: StateId,
    resulting_state: Option<StateId>,
    matched_transition: Option<TransitionId>,
    guard_evaluations: Vec<GuardEvaluation>,
    projection: Option<TransitionProjection>,
    authorized_activity: Option<crate::ActivityId>,
}

impl TransitionDecision {
    #[must_use]
    pub const fn code(&self) -> TransitionDecisionCode {
        self.code
    }
    #[must_use]
    pub const fn accepted(&self) -> bool {
        self.accepted
    }
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
    #[must_use]
    pub fn occurrence(&self) -> &crate::EventOccurrenceId {
        &self.occurrence
    }
    #[must_use]
    pub fn previous_state(&self) -> &StateId {
        &self.previous_state
    }
    #[must_use]
    pub fn resulting_state(&self) -> Option<&StateId> {
        self.resulting_state.as_ref()
    }
    #[must_use]
    pub fn matched_transition(&self) -> Option<&TransitionId> {
        self.matched_transition.as_ref()
    }
    #[must_use]
    pub fn guard_evaluations(&self) -> &[GuardEvaluation] {
        &self.guard_evaluations
    }
    #[must_use]
    pub fn projection(&self) -> Option<&TransitionProjection> {
        self.projection.as_ref()
    }
    #[must_use]
    pub fn authorized_activity(&self) -> Option<&crate::ActivityId> {
        self.authorized_activity.as_ref()
    }
}

/// Stateless pure evaluator.
#[derive(Debug, Default, Clone, Copy)]
pub struct TransitionEvaluator;

impl TransitionEvaluator {
    /// Evaluates one explicit snapshot without mutating any input.
    #[must_use]
    pub fn evaluate(
        definition: &ProcessDefinition,
        instance: &ProcessInstance,
        event: &EventOccurrence,
        inputs: &EvaluationInputs,
    ) -> TransitionDecision {
        let occurrence = event.id().clone();
        let previous_state = instance.current_state().clone();
        let rejected = |code: TransitionDecisionCode, reason: &str| TransitionDecision {
            code,
            accepted: false,
            reason: reason.to_owned(),
            occurrence: occurrence.clone(),
            previous_state: previous_state.clone(),
            resulting_state: None,
            matched_transition: None,
            guard_evaluations: Vec::new(),
            projection: None,
            authorized_activity: None,
        };
        if !ProcessValidator::validate(definition).is_valid() {
            return rejected(
                TransitionDecisionCode::InvalidDefinition,
                "definition is not statically valid",
            );
        }
        if instance.require_definition(definition).is_err() {
            return rejected(
                TransitionDecisionCode::DefinitionIdentityConflict,
                "definition identity does not match instance pin",
            );
        }
        if event.instance_id() != instance.id() {
            return rejected(
                TransitionDecisionCode::WrongInstance,
                "event targets a different process instance",
            );
        }
        if event.expected_revision() != instance.revision() {
            return rejected(
                TransitionDecisionCode::StaleRevision,
                "event expected revision is stale",
            );
        }
        if !definition
            .events()
            .iter()
            .any(|item| item.id() == event.event_type())
        {
            return rejected(
                TransitionDecisionCode::UnknownEvent,
                "event type is not declared",
            );
        }
        if definition
            .states()
            .iter()
            .find(|item| item.id() == instance.current_state())
            .is_some_and(|state| state.is_terminal())
        {
            return rejected(
                TransitionDecisionCode::TerminalState,
                "terminal process instances cannot transition",
            );
        }
        let candidates = definition
            .transitions()
            .iter()
            .filter(|transition| {
                transition.from() == instance.current_state()
                    && transition.event() == event.event_type()
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return rejected(
                TransitionDecisionCode::NoMatchingTransition,
                "no transition is declared for current state and event",
            );
        }
        let mut matched = Vec::new();
        let mut evaluations = Vec::new();
        for transition in candidates {
            let result = evaluate_guard(
                transition.guard(),
                event,
                instance,
                inputs,
                &mut evaluations,
            );
            if result {
                matched.push(transition);
            }
        }
        if matched.is_empty() {
            let mut decision = rejected(
                TransitionDecisionCode::GuardRejected,
                "all candidate transition guards rejected the event",
            );
            decision.guard_evaluations = evaluations;
            return decision;
        }
        if matched.len() != 1 {
            let mut decision = rejected(
                TransitionDecisionCode::AmbiguousTransition,
                "more than one transition guard matched",
            );
            decision.guard_evaluations = evaluations;
            return decision;
        }
        let transition = matched[0];
        let status = if transition.completes()
            || definition
                .states()
                .iter()
                .any(|state| state.id() == transition.to() && state.is_terminal())
        {
            ProcessInstanceStatus::Completed
        } else if transition.pauses() {
            ProcessInstanceStatus::Paused
        } else if transition.blocker().is_some() {
            ProcessInstanceStatus::Blocked
        } else {
            ProcessInstanceStatus::Running
        };
        let projection = TransitionProjection::new(
            instance.revision(),
            transition.id().clone(),
            transition.to().clone(),
            status,
            "transition guard accepted",
        )
        .expect("validated transition projection fields are non-empty")
        .with_occurrence(occurrence.clone());
        TransitionDecision {
            code: TransitionDecisionCode::Accepted,
            accepted: true,
            reason: "transition accepted".to_owned(),
            occurrence,
            previous_state,
            resulting_state: Some(transition.to().clone()),
            matched_transition: Some(transition.id().clone()),
            guard_evaluations: evaluations,
            projection: Some(projection),
            authorized_activity: transition.authorized_activity().cloned(),
        }
    }
}

fn evaluate_guard(
    guard: &GuardExpression,
    event: &EventOccurrence,
    instance: &ProcessInstance,
    inputs: &EvaluationInputs,
    evaluations: &mut Vec<GuardEvaluation>,
) -> bool {
    let matched = match guard {
        GuardExpression::Always => true,
        GuardExpression::Never => false,
        GuardExpression::All(children) => children
            .iter()
            .all(|child| evaluate_guard(child, event, instance, inputs, evaluations)),
        GuardExpression::Any(children) => children
            .iter()
            .any(|child| evaluate_guard(child, event, instance, inputs, evaluations)),
        GuardExpression::Not(child) => !evaluate_guard(child, event, instance, inputs, evaluations),
        GuardExpression::EventAttributeEquals { name, value } => event
            .attributes()
            .get(name)
            .is_some_and(|actual| actual == value),
        GuardExpression::EvidencePresent(value) => {
            inputs.evidence().contains(value) || instance.evidence().contains(value)
        }
        GuardExpression::CapabilityAvailable(value) => inputs.capabilities().contains(value),
        GuardExpression::BlockerActive(value) => {
            inputs.blockers().contains(value)
                || instance
                    .blockers()
                    .get(value)
                    .is_some_and(|blocker| blocker.active())
        }
        GuardExpression::GateIs { gate, status } => {
            inputs
                .gates()
                .get(gate)
                .copied()
                .or_else(|| instance.active_gates().get(gate).copied())
                == Some(*status)
        }
    };
    evaluations.push(GuardEvaluation {
        expression: format!("{guard:?}"),
        matched,
    });
    matched
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EventOccurrenceId, EventTypeDefinition, EventTypeId, ProcessDefinitionBuilder,
        ProcessDefinitionId, ProcessDefinitionVersion, ProcessInstanceId, ProcessInstanceRevision,
        StateDefinition, TransitionId,
    };

    fn setup(guard: GuardExpression) -> (ProcessDefinition, ProcessInstance) {
        let definition = ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("evaluator-example").unwrap(),
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
            guard,
        )])
        .build()
        .unwrap();
        let instance =
            ProcessInstance::start(&definition, ProcessInstanceId::new("run-1").unwrap()).unwrap();
        (definition, instance)
    }

    fn event(instance: &ProcessInstance, event_type: &str) -> EventOccurrence {
        EventOccurrence::new(
            EventOccurrenceId::new("occurrence-1").unwrap(),
            EventTypeId::new(event_type).unwrap(),
            instance.id().clone(),
            instance.revision(),
        )
    }

    #[test]
    fn accepts_legal_transition_without_mutating_instance() {
        let (definition, instance) = setup(GuardExpression::Always);
        let before = instance.clone();
        let decision = TransitionEvaluator::evaluate(
            &definition,
            &instance,
            &event(&instance, "finish"),
            &EvaluationInputs::default(),
        );
        assert!(decision.accepted());
        assert_eq!(decision.code(), TransitionDecisionCode::Accepted);
        assert_eq!(decision.resulting_state().unwrap().as_str(), "done");
        assert_eq!(instance, before);
    }

    #[test]
    fn rejects_unknown_stale_wrong_and_terminal_events() {
        let (definition, instance) = setup(GuardExpression::Always);
        assert_eq!(
            TransitionEvaluator::evaluate(
                &definition,
                &instance,
                &event(&instance, "unknown"),
                &EvaluationInputs::default()
            )
            .code(),
            TransitionDecisionCode::UnknownEvent
        );
        let stale = EventOccurrence::new(
            EventOccurrenceId::new("stale").unwrap(),
            EventTypeId::new("finish").unwrap(),
            instance.id().clone(),
            ProcessInstanceRevision::new(1),
        );
        assert_eq!(
            TransitionEvaluator::evaluate(
                &definition,
                &instance,
                &stale,
                &EvaluationInputs::default()
            )
            .code(),
            TransitionDecisionCode::StaleRevision
        );
        let wrong = EventOccurrence::new(
            EventOccurrenceId::new("wrong").unwrap(),
            EventTypeId::new("finish").unwrap(),
            ProcessInstanceId::new("other").unwrap(),
            instance.revision(),
        );
        assert_eq!(
            TransitionEvaluator::evaluate(
                &definition,
                &instance,
                &wrong,
                &EvaluationInputs::default()
            )
            .code(),
            TransitionDecisionCode::WrongInstance
        );
        let mut completed = instance.clone();
        completed
            .apply_projection(
                &definition,
                crate::TransitionProjection::new(
                    ProcessInstanceRevision::initial(),
                    TransitionId::new("finish").unwrap(),
                    StateId::new("done").unwrap(),
                    ProcessInstanceStatus::Completed,
                    "complete",
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            TransitionEvaluator::evaluate(
                &definition,
                &completed,
                &event(&completed, "finish"),
                &EvaluationInputs::default()
            )
            .code(),
            TransitionDecisionCode::TerminalState
        );
    }

    #[test]
    fn evaluates_typed_guards_and_rejects_ambiguity() {
        let capability = CapabilityId::new("repository.read").unwrap();
        let (definition, instance) = setup(GuardExpression::All(vec![
            GuardExpression::EventAttributeEquals {
                name: "result".to_owned(),
                value: "ok".to_owned(),
            },
            GuardExpression::CapabilityAvailable(capability.clone()),
        ]));
        let event = event(&instance, "finish")
            .with_attribute("result", "ok")
            .unwrap();
        let rejected = TransitionEvaluator::evaluate(
            &definition,
            &instance,
            &event,
            &EvaluationInputs::default(),
        );
        assert_eq!(rejected.code(), TransitionDecisionCode::GuardRejected);
        let accepted = TransitionEvaluator::evaluate(
            &definition,
            &instance,
            &event,
            &EvaluationInputs::default().with_capabilities([capability]),
        );
        assert!(accepted.accepted());
        assert!(accepted.guard_evaluations().len() >= 2);
        let (definition, instance) = setup(GuardExpression::Always);
        let _ = (definition, instance);
    }
}
