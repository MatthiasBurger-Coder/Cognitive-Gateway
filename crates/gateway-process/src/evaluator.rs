//! Pure deterministic transition evaluation over explicit process snapshots.

use std::collections::{BTreeMap, BTreeSet};

use gateway_domain::CapabilityId;

use crate::{
    AuthorizationStatus, ConstraintEvaluation, EventOccurrence, EvidenceStatus, GateId, GateStatus,
    GuardExpression, PolicyDecisionStatus, PolicyInput, ProcessDefinition, ProcessInstance,
    ProcessInstanceStatus, ProcessValidator, StateId, TransitionId, TransitionProjection,
};

/// Explicit inputs available to typed guard evaluation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvaluationInputs {
    evidence: BTreeSet<crate::EvidenceTypeId>,
    evidence_status: BTreeMap<crate::EvidenceTypeId, EvidenceStatus>,
    gates: BTreeMap<GateId, GateStatus>,
    capabilities: BTreeSet<CapabilityId>,
    blockers: BTreeSet<crate::BlockerId>,
    policy: PolicyInput,
}

impl EvaluationInputs {
    #[must_use]
    pub fn with_evidence(
        mut self,
        values: impl IntoIterator<Item = crate::EvidenceTypeId>,
    ) -> Self {
        for value in values {
            self.evidence_status
                .insert(value.clone(), EvidenceStatus::Present);
            self.evidence.insert(value);
        }
        self
    }
    #[must_use]
    pub fn with_evidence_status(
        mut self,
        value: crate::EvidenceTypeId,
        status: EvidenceStatus,
    ) -> Self {
        if status == EvidenceStatus::Present {
            self.evidence.insert(value.clone());
        }
        self.evidence_status.insert(value, status);
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
    pub fn with_authorization(
        mut self,
        id: crate::AuthorizationId,
        status: AuthorizationStatus,
    ) -> Self {
        self.policy = self.policy.with_authorization(id, status);
        self
    }
    #[must_use]
    pub fn with_policy_decision(
        mut self,
        id: crate::PolicyDecisionId,
        status: PolicyDecisionStatus,
    ) -> Self {
        self.policy = self.policy.with_policy_decision(id, status);
        self
    }
    #[must_use]
    pub fn evidence(&self) -> &BTreeSet<crate::EvidenceTypeId> {
        &self.evidence
    }
    #[must_use]
    pub fn evidence_status(&self) -> &BTreeMap<crate::EvidenceTypeId, EvidenceStatus> {
        &self.evidence_status
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
    #[must_use]
    pub fn policy(&self) -> &PolicyInput {
        &self.policy
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
    WaitingForEvidence,
    WaitingForAuthorization,
    GateFailed,
    EvidenceInvalid,
    ActiveBlocker,
    InvariantViolation,
    AuthorizationDenied,
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
            Self::WaitingForEvidence => "WAITING_FOR_EVIDENCE",
            Self::WaitingForAuthorization => "WAITING_FOR_AUTHORIZATION",
            Self::GateFailed => "GATE_FAILED",
            Self::EvidenceInvalid => "EVIDENCE_INVALID",
            Self::ActiveBlocker => "ACTIVE_BLOCKER",
            Self::InvariantViolation => "INVARIANT_VIOLATION",
            Self::AuthorizationDenied => "AUTHORIZATION_DENIED",
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
    constraint_evaluations: Vec<ConstraintEvaluation>,
    authorized_activity_definition: Option<crate::AuthorizedActivity>,
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
    #[must_use]
    pub fn constraint_evaluations(&self) -> &[ConstraintEvaluation] {
        &self.constraint_evaluations
    }
    #[must_use]
    pub fn authorized_activity_definition(&self) -> Option<&crate::AuthorizedActivity> {
        self.authorized_activity_definition.as_ref()
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
            constraint_evaluations: Vec::new(),
            authorized_activity_definition: None,
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
        let mut constraint_evaluations = Vec::new();
        for transition in candidates {
            if let Some((code, reason)) = evaluate_constraints(
                definition,
                instance,
                transition,
                inputs,
                &mut constraint_evaluations,
            ) {
                let mut decision = rejected(code, &reason);
                decision.constraint_evaluations = constraint_evaluations;
                return decision;
            }
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
            decision.constraint_evaluations = constraint_evaluations;
            return decision;
        }
        if matched.len() != 1 {
            let mut decision = rejected(
                TransitionDecisionCode::AmbiguousTransition,
                "more than one transition guard matched",
            );
            decision.guard_evaluations = evaluations;
            decision.constraint_evaluations = constraint_evaluations;
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
            constraint_evaluations,
            authorized_activity_definition: transition.authorized_activity().and_then(|activity| {
                definition
                    .activities()
                    .iter()
                    .find(|item| item.id() == activity)
                    .map(crate::AuthorizedActivity::from_definition)
            }),
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
            inputs
                .evidence_status()
                .get(value)
                .copied()
                .unwrap_or_else(|| {
                    if inputs.evidence().contains(value) || instance.evidence().contains(value) {
                        EvidenceStatus::Present
                    } else {
                        EvidenceStatus::Missing
                    }
                })
                == EvidenceStatus::Present
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
        GuardExpression::AuthorizationIs {
            authorization,
            status,
        } => inputs.policy().authorizations().get(authorization).copied() == Some(*status),
        GuardExpression::PolicyDecisionIs { policy, status } => {
            inputs.policy().decisions().get(policy).copied() == Some(*status)
        }
    };
    evaluations.push(GuardEvaluation {
        expression: format!("{guard:?}"),
        matched,
    });
    matched
}

fn evidence_status(
    value: &crate::EvidenceTypeId,
    instance: &ProcessInstance,
    inputs: &EvaluationInputs,
) -> EvidenceStatus {
    inputs
        .evidence_status()
        .get(value)
        .copied()
        .unwrap_or_else(|| {
            if inputs.evidence().contains(value) || instance.evidence().contains(value) {
                EvidenceStatus::Present
            } else {
                EvidenceStatus::Missing
            }
        })
}

fn evaluate_constraints(
    definition: &ProcessDefinition,
    instance: &ProcessInstance,
    transition: &crate::TransitionDefinition,
    inputs: &EvaluationInputs,
    trace: &mut Vec<ConstraintEvaluation>,
) -> Option<(TransitionDecisionCode, String)> {
    for gate_id in transition.required_gates() {
        let status = inputs
            .gates()
            .get(gate_id)
            .copied()
            .or_else(|| instance.active_gates().get(gate_id).copied())
            .unwrap_or(GateStatus::Open);
        trace.push(ConstraintEvaluation::new(
            "GATE",
            gate_id.as_str(),
            &format!("{status:?}"),
            "required transition gate",
        ));
        match status {
            GateStatus::Passed => {}
            GateStatus::WaitingForEvidence => {
                return Some((
                    TransitionDecisionCode::WaitingForEvidence,
                    "gate is waiting for evidence".to_owned(),
                ));
            }
            GateStatus::WaitingForAuthorization => {
                return Some((
                    TransitionDecisionCode::WaitingForAuthorization,
                    "gate is waiting for authorization".to_owned(),
                ));
            }
            GateStatus::Open => {
                return Some((
                    TransitionDecisionCode::WaitingForEvidence,
                    "required gate is not passed".to_owned(),
                ));
            }
            GateStatus::Failed | GateStatus::Blocked => {
                return Some((
                    TransitionDecisionCode::GateFailed,
                    "required gate failed or is blocked".to_owned(),
                ));
            }
        }
    }
    for evidence in transition.required_evidence() {
        let status = evidence_status(evidence, instance, inputs);
        trace.push(ConstraintEvaluation::new(
            "EVIDENCE",
            evidence.as_str(),
            status.as_str(),
            "required transition evidence",
        ));
        match status {
            EvidenceStatus::Present => {}
            EvidenceStatus::Missing => {
                return Some((
                    TransitionDecisionCode::WaitingForEvidence,
                    "required evidence is missing".to_owned(),
                ));
            }
            EvidenceStatus::Invalid | EvidenceStatus::Failed => {
                return Some((
                    TransitionDecisionCode::EvidenceInvalid,
                    "required evidence is invalid or failed".to_owned(),
                ));
            }
        }
    }
    for gate in definition.gates() {
        for evidence in gate.required_evidence() {
            let status = evidence_status(evidence.evidence_type(), instance, inputs);
            trace.push(ConstraintEvaluation::new(
                "GATE_EVIDENCE",
                gate.id().as_str(),
                status.as_str(),
                "gate evidence prerequisite",
            ));
            if status != EvidenceStatus::Present {
                return Some((
                    TransitionDecisionCode::WaitingForEvidence,
                    "gate evidence prerequisite is not present".to_owned(),
                ));
            }
        }
    }
    if let Some(blocker) = instance.blockers().values().find(|value| value.active()) {
        trace.push(ConstraintEvaluation::new(
            "BLOCKER",
            blocker.id().as_str(),
            "ACTIVE",
            blocker.reason(),
        ));
        return Some((
            TransitionDecisionCode::ActiveBlocker,
            "an active blocker prevents progression".to_owned(),
        ));
    }
    for invariant in definition.invariants() {
        let check_event = EventOccurrence::new(
            crate::EventOccurrenceId::new("invariant-check").expect("static identifier is valid"),
            transition.event().clone(),
            instance.id().clone(),
            instance.revision(),
        );
        let mut guard_trace = Vec::new();
        if !evaluate_guard(
            invariant.condition(),
            &check_event,
            instance,
            inputs,
            &mut guard_trace,
        ) {
            trace.push(ConstraintEvaluation::new(
                "INVARIANT",
                invariant.id().as_str(),
                "VIOLATED",
                invariant.reason(),
            ));
            return Some((
                TransitionDecisionCode::InvariantViolation,
                invariant.reason().to_owned(),
            ));
        }
        trace.push(ConstraintEvaluation::new(
            "INVARIANT",
            invariant.id().as_str(),
            "PASSED",
            invariant.reason(),
        ));
    }
    if let Some(result) = authorization_constraint(transition.guard(), inputs) {
        trace.push(ConstraintEvaluation::new(
            "AUTHORIZATION",
            result.1.as_str(),
            result.0,
            "typed authorization or policy input",
        ));
        return Some((
            match result.0 {
                "DENIED" => TransitionDecisionCode::AuthorizationDenied,
                _ => TransitionDecisionCode::WaitingForAuthorization,
            },
            match result.0 {
                "DENIED" => "authorization was denied".to_owned(),
                _ => "authorization input is missing or waiting".to_owned(),
            },
        ));
    }
    None
}

fn authorization_constraint(
    guard: &GuardExpression,
    inputs: &EvaluationInputs,
) -> Option<(&'static str, String)> {
    match guard {
        GuardExpression::AuthorizationIs {
            authorization,
            status: AuthorizationStatus::Allowed,
        } => match inputs.policy().authorizations().get(authorization) {
            Some(AuthorizationStatus::Allowed) => None,
            Some(AuthorizationStatus::Denied) => Some(("DENIED", authorization.to_string())),
            Some(AuthorizationStatus::Waiting) | None => {
                Some(("WAITING", authorization.to_string()))
            }
        },
        GuardExpression::PolicyDecisionIs {
            policy,
            status: PolicyDecisionStatus::Allow,
        } => match inputs.policy().decisions().get(policy) {
            Some(PolicyDecisionStatus::Allow) => None,
            Some(PolicyDecisionStatus::Deny) => Some(("DENIED", policy.to_string())),
            Some(PolicyDecisionStatus::Waiting) | None => Some(("WAITING", policy.to_string())),
        },
        GuardExpression::All(children) => children
            .iter()
            .filter_map(|child| authorization_constraint(child, inputs))
            .min_by_key(|(status, _)| *status == "WAITING"),
        GuardExpression::Any(children) => {
            let dependencies = children
                .iter()
                .filter_map(|child| authorization_constraint(child, inputs))
                .collect::<Vec<_>>();
            if dependencies.is_empty() {
                None
            } else if dependencies.iter().any(|(status, _)| *status == "WAITING") {
                dependencies
                    .into_iter()
                    .find(|(status, _)| *status == "WAITING")
            } else {
                dependencies.into_iter().next()
            }
        }
        GuardExpression::Not(child) => authorization_constraint(child, inputs),
        _ => None,
    }
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

    #[test]
    fn evaluates_gates_evidence_and_blockers_as_first_class_constraints() {
        let definition = ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("constraints-example").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .with_states([
            StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
            StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap(),
        ])
        .with_events([EventTypeDefinition::new(
            EventTypeId::new("finish").unwrap(),
        )])
        .with_gates([crate::GateDefinition::new(
            GateId::new("review").unwrap(),
            Vec::new(),
        )])
        .with_evidence([crate::EvidenceRequirement::new(
            crate::EvidenceTypeId::new("report").unwrap(),
            true,
        )])
        .with_transitions([crate::TransitionDefinition::new(
            TransitionId::new("finish").unwrap(),
            StateId::new("start").unwrap(),
            EventTypeId::new("finish").unwrap(),
            StateId::new("done").unwrap(),
            GuardExpression::Always,
        )
        .with_required_gates(vec![GateId::new("review").unwrap()])
        .with_required_evidence(vec![crate::EvidenceTypeId::new("report").unwrap()])])
        .build()
        .unwrap();
        let instance =
            ProcessInstance::start(&definition, ProcessInstanceId::new("run-1").unwrap()).unwrap();
        let occurrence = event(&instance, "finish");
        assert_eq!(
            TransitionEvaluator::evaluate(
                &definition,
                &instance,
                &occurrence,
                &EvaluationInputs::default()
            )
            .code(),
            TransitionDecisionCode::WaitingForEvidence
        );
        let failed = EvaluationInputs::default()
            .with_gate(GateId::new("review").unwrap(), GateStatus::Failed);
        assert_eq!(
            TransitionEvaluator::evaluate(&definition, &instance, &occurrence, &failed).code(),
            TransitionDecisionCode::GateFailed
        );
        let invalid = EvaluationInputs::default()
            .with_gate(GateId::new("review").unwrap(), GateStatus::Passed)
            .with_evidence_status(
                crate::EvidenceTypeId::new("report").unwrap(),
                EvidenceStatus::Invalid,
            );
        assert_eq!(
            TransitionEvaluator::evaluate(&definition, &instance, &occurrence, &invalid).code(),
            TransitionDecisionCode::EvidenceInvalid
        );
        let complete = EvaluationInputs::default()
            .with_gate(GateId::new("review").unwrap(), GateStatus::Passed)
            .with_evidence([crate::EvidenceTypeId::new("report").unwrap()]);
        assert!(
            TransitionEvaluator::evaluate(&definition, &instance, &occurrence, &complete)
                .accepted()
        );
        let mut blocked = instance;
        blocked.record_blocker(
            crate::BlockerRuntimeState::new(
                crate::BlockerId::new("incident").unwrap(),
                "incident",
                true,
            )
            .unwrap(),
        );
        assert_eq!(
            TransitionEvaluator::evaluate(
                &definition,
                &blocked,
                &event(&blocked, "finish"),
                &complete
            )
            .code(),
            TransitionDecisionCode::ActiveBlocker
        );
    }

    #[test]
    fn authorization_is_fail_closed_and_projects_capability_first_activity() {
        let authorization = crate::AuthorizationId::new("human-review").unwrap();
        let policy = crate::PolicyDecisionId::new("release-check").unwrap();
        let activity_id = crate::ActivityId::new("ship").unwrap();
        let capability = CapabilityId::new("repository.write").unwrap();
        let evidence = crate::EvidenceTypeId::new("release-report").unwrap();
        let constraint = crate::ActivityConstraint::new("branch", "protected").unwrap();
        let definition = ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("authorization-example").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .with_states([
            StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
            StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap(),
        ])
        .with_events([EventTypeDefinition::new(
            EventTypeId::new("finish").unwrap(),
        )])
        .with_evidence([crate::EvidenceRequirement::new(evidence.clone(), true)])
        .with_activities([crate::ActivityDefinition::new(
            activity_id.clone(),
            vec![capability.clone()],
            vec![evidence.clone()],
            vec![constraint.clone()],
        )])
        .with_transitions([crate::TransitionDefinition::new(
            TransitionId::new("finish").unwrap(),
            StateId::new("start").unwrap(),
            EventTypeId::new("finish").unwrap(),
            StateId::new("done").unwrap(),
            GuardExpression::All(vec![
                GuardExpression::AuthorizationIs {
                    authorization: authorization.clone(),
                    status: AuthorizationStatus::Allowed,
                },
                GuardExpression::PolicyDecisionIs {
                    policy: policy.clone(),
                    status: PolicyDecisionStatus::Allow,
                },
            ]),
        )
        .with_authorized_activity(activity_id)])
        .build()
        .unwrap();
        let instance =
            ProcessInstance::start(&definition, ProcessInstanceId::new("run-1").unwrap()).unwrap();
        let occurrence = event(&instance, "finish");

        let missing = TransitionEvaluator::evaluate(
            &definition,
            &instance,
            &occurrence,
            &EvaluationInputs::default(),
        );
        assert_eq!(
            missing.code(),
            TransitionDecisionCode::WaitingForAuthorization
        );
        assert!(
            missing
                .constraint_evaluations()
                .iter()
                .any(|item| item.kind() == "AUTHORIZATION" && item.status() == "WAITING")
        );

        let denied = TransitionEvaluator::evaluate(
            &definition,
            &instance,
            &occurrence,
            &EvaluationInputs::default()
                .with_authorization(authorization.clone(), AuthorizationStatus::Denied)
                .with_policy_decision(policy.clone(), PolicyDecisionStatus::Allow),
        );
        assert_eq!(denied.code(), TransitionDecisionCode::AuthorizationDenied);

        let accepted = TransitionEvaluator::evaluate(
            &definition,
            &instance,
            &occurrence,
            &EvaluationInputs::default()
                .with_authorization(authorization, AuthorizationStatus::Allowed)
                .with_policy_decision(policy, PolicyDecisionStatus::Allow),
        );
        assert_eq!(accepted.code(), TransitionDecisionCode::Accepted);
        let projected = accepted.authorized_activity_definition().unwrap();
        assert_eq!(projected.id().as_str(), "ship");
        assert_eq!(projected.capabilities(), &[capability]);
        assert_eq!(projected.output_evidence(), &[evidence]);
        assert_eq!(projected.constraints(), &[constraint]);
    }
}
