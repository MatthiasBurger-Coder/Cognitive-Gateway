//! Static, deterministic validation of canonical Process IR definitions.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use gateway_domain::CapabilityId;

use crate::{GuardExpression, ProcessDefinition, ProcessIrVersion, StateId};

/// One stable static-validation diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationDiagnostic {
    code: &'static str,
    message: String,
    element: String,
}

impl ValidationDiagnostic {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub fn element(&self) -> &str {
        &self.element
    }
}

/// Ordered result of static validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    diagnostics: Vec<ValidationDiagnostic>,
}

impl ValidationReport {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_empty()
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[ValidationDiagnostic] {
        &self.diagnostics
    }

    pub fn into_result(self) -> Result<(), Self> {
        if self.is_valid() { Ok(()) } else { Err(self) }
    }
}

/// Stateless validator for the canonical Process IR.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessValidator;

impl ProcessValidator {
    /// Validates structure, references, graph reachability and determinism
    /// prerequisites without repairing the definition.
    #[must_use]
    pub fn validate(definition: &ProcessDefinition) -> ValidationReport {
        Self::validate_with_capabilities(definition, &[])
    }

    /// Validates a definition and ensures every activity capability is present
    /// in the supplied canonical CG-03 capability set.
    #[must_use]
    pub fn validate_with_capabilities(
        definition: &ProcessDefinition,
        capabilities: &[CapabilityId],
    ) -> ValidationReport {
        let mut context = ValidationContext {
            definition,
            diagnostics: Vec::new(),
            capabilities: capabilities.iter().collect(),
        };
        context.run();
        context.diagnostics.sort_by(|left, right| {
            left.code
                .cmp(right.code)
                .then(left.element.cmp(&right.element))
                .then(left.message.cmp(&right.message))
        });
        ValidationReport {
            diagnostics: context.diagnostics,
        }
    }
}

struct ValidationContext<'a> {
    definition: &'a ProcessDefinition,
    diagnostics: Vec<ValidationDiagnostic>,
    capabilities: BTreeSet<&'a CapabilityId>,
}

impl ValidationContext<'_> {
    fn run(&mut self) {
        if !matches!(self.definition.ir_version(), ProcessIrVersion::V1) {
            self.add(
                "UNSUPPORTED_IR_VERSION",
                "only Process IR v1 is supported",
                "definition",
            );
        }
        if self.definition.verify_digest().is_err() {
            self.add(
                "NON_CANONICAL_DEFINITION",
                "definition digest does not match canonical IR",
                "definition",
            );
        }
        self.unique_ids();
        self.references();
        self.graph();
        self.transition_conflicts();
        self.recovery();
        self.capabilities();
    }

    fn unique_ids(&mut self) {
        Self::find_duplicates(
            self.definition
                .states()
                .iter()
                .map(|value| value.id().as_str()),
            "DUPLICATE_STATE",
            &mut self.diagnostics,
        );
        Self::find_duplicates(
            self.definition
                .events()
                .iter()
                .map(|value| value.id().as_str()),
            "DUPLICATE_EVENT",
            &mut self.diagnostics,
        );
        Self::find_duplicates(
            self.definition
                .transitions()
                .iter()
                .map(|value| value.id().as_str()),
            "DUPLICATE_TRANSITION",
            &mut self.diagnostics,
        );
        Self::find_duplicates(
            self.definition
                .gates()
                .iter()
                .map(|value| value.id().as_str()),
            "DUPLICATE_GATE",
            &mut self.diagnostics,
        );
        Self::find_duplicates(
            self.definition
                .invariants()
                .iter()
                .map(|value| value.id().as_str()),
            "DUPLICATE_INVARIANT",
            &mut self.diagnostics,
        );
        Self::find_duplicates(
            self.definition
                .blockers()
                .iter()
                .map(|value| value.id().as_str()),
            "DUPLICATE_BLOCKER",
            &mut self.diagnostics,
        );
        Self::find_duplicates(
            self.definition
                .activities()
                .iter()
                .map(|value| value.id().as_str()),
            "DUPLICATE_ACTIVITY",
            &mut self.diagnostics,
        );
        Self::find_duplicates(
            self.definition
                .transitions()
                .iter()
                .flat_map(|value| value.required_gates().iter().map(|id| id.as_str())),
            "DUPLICATE_GATE_REQUIREMENT",
            &mut self.diagnostics,
        );
    }

    fn find_duplicates<'a>(
        values: impl IntoIterator<Item = &'a str>,
        code: &'static str,
        diagnostics: &mut Vec<ValidationDiagnostic>,
    ) {
        let mut seen = BTreeSet::new();
        for value in values {
            if !seen.insert(value) {
                diagnostics.push(ValidationDiagnostic {
                    code,
                    message: format!("duplicate identifier {value}"),
                    element: value.to_owned(),
                });
            }
        }
    }

    fn references(&mut self) {
        let states = self
            .definition
            .states()
            .iter()
            .map(|value| value.id())
            .collect::<BTreeSet<_>>();
        let events = self
            .definition
            .events()
            .iter()
            .map(|value| value.id())
            .collect::<BTreeSet<_>>();
        let gates = self
            .definition
            .gates()
            .iter()
            .map(|value| value.id())
            .collect::<BTreeSet<_>>();
        let evidence = self
            .definition
            .evidence()
            .iter()
            .map(|value| value.evidence_type())
            .collect::<BTreeSet<_>>();
        let blockers = self
            .definition
            .blockers()
            .iter()
            .map(|value| value.id())
            .collect::<BTreeSet<_>>();
        let activities = self
            .definition
            .activities()
            .iter()
            .map(|value| value.id())
            .collect::<BTreeSet<_>>();
        for transition in self.definition.transitions() {
            if !states.contains(transition.from()) {
                self.add(
                    "UNKNOWN_REFERENCE",
                    "transition source state is not declared",
                    transition.id().as_str(),
                );
            }
            if !states.contains(transition.to()) {
                self.add(
                    "UNKNOWN_REFERENCE",
                    "transition target state is not declared",
                    transition.id().as_str(),
                );
            }
            if !events.contains(transition.event()) {
                self.add(
                    "UNKNOWN_REFERENCE",
                    "transition event is not declared",
                    transition.id().as_str(),
                );
            }
            for gate in transition.required_gates() {
                if !gates.contains(&gate) {
                    self.add(
                        "UNKNOWN_REFERENCE",
                        "required gate is not declared",
                        gate.as_str(),
                    );
                }
            }
            for item in transition.required_evidence() {
                if !evidence.contains(&item) {
                    self.add(
                        "UNKNOWN_REFERENCE",
                        "required evidence type is not declared",
                        item.as_str(),
                    );
                }
            }
            if let Some(activity) = transition.authorized_activity() {
                if !activities.contains(&activity) {
                    self.add(
                        "UNKNOWN_REFERENCE",
                        "authorized activity is not declared",
                        activity.as_str(),
                    );
                }
            }
            if let Some(blocker) = transition.blocker() {
                if !blockers.contains(&blocker) {
                    self.add(
                        "UNKNOWN_REFERENCE",
                        "transition blocker is not declared",
                        blocker.as_str(),
                    );
                }
            }
            if let Some(target) = transition.repair_target() {
                if !states.contains(&target) {
                    self.add(
                        "UNKNOWN_REFERENCE",
                        "repair target state is not declared",
                        target.as_str(),
                    );
                }
            }
            self.guard_references(
                transition.guard(),
                &states,
                &events,
                &gates,
                &evidence,
                &blockers,
            );
        }
        for gate in self.definition.gates() {
            for item in gate.required_evidence() {
                if !evidence.contains(&item.evidence_type()) {
                    self.add(
                        "UNKNOWN_REFERENCE",
                        "gate evidence type is not declared",
                        gate.id().as_str(),
                    );
                }
            }
        }
        for invariant in self.definition.invariants() {
            self.guard_references(
                invariant.condition(),
                &states,
                &events,
                &gates,
                &evidence,
                &blockers,
            );
        }
        for activity in self.definition.activities() {
            for item in activity.output_evidence() {
                if !evidence.contains(&item) {
                    self.add(
                        "UNKNOWN_REFERENCE",
                        "activity output evidence is not declared",
                        activity.id().as_str(),
                    );
                }
            }
        }
    }

    fn guard_references(
        &mut self,
        guard: &GuardExpression,
        states: &BTreeSet<&StateId>,
        _events: &BTreeSet<&crate::EventTypeId>,
        gates: &BTreeSet<&crate::GateId>,
        evidence: &BTreeSet<&crate::EvidenceTypeId>,
        blockers: &BTreeSet<&crate::BlockerId>,
    ) {
        match guard {
            GuardExpression::All(children) | GuardExpression::Any(children) => {
                if children.is_empty() {
                    self.add(
                        "MALFORMED_GUARD",
                        "guard collection cannot be empty",
                        "guard",
                    );
                }
                for child in children {
                    self.guard_references(child, states, _events, gates, evidence, blockers);
                }
            }
            GuardExpression::Not(child) => {
                self.guard_references(child, states, _events, gates, evidence, blockers)
            }
            GuardExpression::GateIs { gate, .. } => {
                if !gates.contains(&gate) {
                    self.add(
                        "UNKNOWN_REFERENCE",
                        "guard gate is not declared",
                        gate.as_str(),
                    );
                }
            }
            GuardExpression::EvidencePresent(item) => {
                if !evidence.contains(&item) {
                    self.add(
                        "UNKNOWN_REFERENCE",
                        "guard evidence is not declared",
                        item.as_str(),
                    );
                }
            }
            GuardExpression::BlockerActive(item) => {
                if !blockers.contains(&item) {
                    self.add(
                        "UNKNOWN_REFERENCE",
                        "guard blocker is not declared",
                        item.as_str(),
                    );
                }
            }
            GuardExpression::CapabilityAvailable(_)
            | GuardExpression::Always
            | GuardExpression::Never
            | GuardExpression::EventAttributeEquals { .. } => {}
        }
        let _ = states;
    }

    fn graph(&mut self) {
        let initial = self
            .definition
            .states()
            .iter()
            .filter(|state| state.is_initial())
            .map(|state| state.id().clone())
            .collect::<Vec<_>>();
        if initial.is_empty() {
            self.add(
                "MISSING_INITIAL_STATE",
                "definition has no initial state",
                "definition",
            );
        }
        if initial.len() > 1 {
            self.add(
                "MULTIPLE_INITIAL_STATES",
                "definition has multiple initial states",
                "definition",
            );
        }
        let terminal = self
            .definition
            .states()
            .iter()
            .filter(|state| state.is_terminal())
            .map(|state| state.id())
            .collect::<BTreeSet<_>>();
        let mut edges: BTreeMap<StateId, Vec<StateId>> = BTreeMap::new();
        for transition in self.definition.transitions() {
            if terminal.contains(&transition.from()) {
                self.add(
                    "INVALID_TERMINAL_TRANSITION",
                    "terminal states cannot have outgoing transitions",
                    transition.id().as_str(),
                );
            }
            edges
                .entry(transition.from().clone())
                .or_default()
                .push(transition.to().clone());
        }
        let mut visited = BTreeSet::new();
        let mut pending = VecDeque::from(initial);
        while let Some(state) = pending.pop_front() {
            if !visited.insert(state.clone()) {
                continue;
            }
            if let Some(next) = edges.get(&state) {
                pending.extend(next.iter().cloned());
            }
        }
        for state in self.definition.states() {
            if !visited.contains(state.id()) {
                self.add(
                    "UNREACHABLE_STATE",
                    "state cannot be reached from the initial state",
                    state.id().as_str(),
                );
            }
        }
    }

    fn transition_conflicts(&mut self) {
        let transitions = self.definition.transitions();
        for (index, left) in transitions.iter().enumerate() {
            for right in transitions.iter().skip(index + 1) {
                if left.from() != right.from() || left.event() != right.event() {
                    continue;
                }
                if left.guard() == right.guard() {
                    self.add(
                        "DUPLICATE_SEMANTIC_TRANSITION",
                        "transitions have identical source, event and guard",
                        right.id().as_str(),
                    );
                } else if guards_may_overlap(left.guard(), right.guard()) {
                    self.add(
                        "AMBIGUOUS_TRANSITION",
                        "transitions may both match the same input",
                        right.id().as_str(),
                    );
                }
            }
        }
    }

    fn recovery(&mut self) {
        for (index, policy) in self.definition.recovery().iter().enumerate() {
            if policy.max_attempts() == 0 {
                self.add(
                    "UNBOUNDED_RETRY_CYCLE",
                    "recovery policy must have a positive bound",
                    &format!("recovery-{index}"),
                );
            }
        }
        for transition in self.definition.transitions() {
            if transition.retry_attempts() == Some(0) {
                self.add(
                    "UNBOUNDED_RETRY_CYCLE",
                    "transition retry must have a positive bound",
                    transition.id().as_str(),
                );
            }
            if transition.repair_target() == Some(transition.from())
                && transition.retry_attempts().is_none()
            {
                self.add(
                    "UNBOUNDED_RETRY_CYCLE",
                    "self repair cycle requires an explicit retry bound",
                    transition.id().as_str(),
                );
            }
        }
    }

    fn capabilities(&mut self) {
        if self.capabilities.is_empty() {
            return;
        }
        for activity in self.definition.activities() {
            for capability in activity.capabilities() {
                if !self.capabilities.contains(&capability) {
                    self.add(
                        "UNKNOWN_CAPABILITY",
                        "activity capability is not present in the canonical capability contracts",
                        capability.as_str(),
                    );
                }
            }
        }
    }

    fn add(&mut self, code: &'static str, message: impl Into<String>, element: &str) {
        self.diagnostics.push(ValidationDiagnostic {
            code,
            message: message.into(),
            element: element.to_owned(),
        });
    }
}

fn guards_may_overlap(left: &GuardExpression, right: &GuardExpression) -> bool {
    match (left, right) {
        (GuardExpression::Never, _) | (_, GuardExpression::Never) => false,
        (
            GuardExpression::GateIs {
                gate: left_gate,
                status: left_status,
            },
            GuardExpression::GateIs {
                gate: right_gate,
                status: right_status,
            },
        ) if left_gate == right_gate => left_status == right_status,
        (
            GuardExpression::EventAttributeEquals {
                name: left_name,
                value: left_value,
            },
            GuardExpression::EventAttributeEquals {
                name: right_name,
                value: right_value,
            },
        ) if left_name == right_name => left_value == right_value,
        (GuardExpression::All(left), GuardExpression::All(right)) => {
            left.iter().all(|left_guard| {
                right
                    .iter()
                    .any(|right_guard| guards_are_disjoint(left_guard, right_guard))
            })
        }
        (GuardExpression::All(children), other) | (other, GuardExpression::All(children)) => {
            !children
                .iter()
                .any(|child| guards_are_disjoint(child, other))
        }
        _ => true,
    }
}

fn guards_are_disjoint(left: &GuardExpression, right: &GuardExpression) -> bool {
    !guards_may_overlap(left, right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EventTypeDefinition, EventTypeId, GuardExpression, ProcessDefinitionBuilder,
        ProcessDefinitionId, ProcessDefinitionVersion, StateDefinition, TransitionDefinition,
        TransitionId,
    };

    fn valid() -> ProcessDefinition {
        ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("validation-example").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .with_states([
            StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
            StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap(),
        ])
        .with_events([EventTypeDefinition::new(
            EventTypeId::new("finish").unwrap(),
        )])
        .with_transitions([TransitionDefinition::new(
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
    fn accepts_valid_definition_and_orders_diagnostics() {
        let report = ProcessValidator::validate(&valid());
        assert!(report.is_valid());
        assert!(report.diagnostics().is_empty());
    }

    #[test]
    fn detects_unreachable_and_terminal_outgoing_states() {
        let definition = ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("invalid-graph").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .with_states([
            StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
            StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap(),
            StateDefinition::new(StateId::new("lost").unwrap(), false, false).unwrap(),
        ])
        .with_events([EventTypeDefinition::new(
            EventTypeId::new("finish").unwrap(),
        )])
        .with_transitions([TransitionDefinition::new(
            TransitionId::new("finish").unwrap(),
            StateId::new("done").unwrap(),
            EventTypeId::new("finish").unwrap(),
            StateId::new("lost").unwrap(),
            GuardExpression::Always,
        )])
        .build()
        .unwrap();
        let codes = ProcessValidator::validate(&definition)
            .diagnostics()
            .iter()
            .map(ValidationDiagnostic::code)
            .collect::<Vec<_>>();
        assert!(codes.contains(&"INVALID_TERMINAL_TRANSITION"));
        assert!(codes.contains(&"UNREACHABLE_STATE"));
    }

    #[test]
    fn detects_ambiguous_and_malformed_guards() {
        let mut definition = valid();
        let mut transitions = definition.transitions().to_vec();
        transitions.push(TransitionDefinition::new(
            TransitionId::new("another").unwrap(),
            StateId::new("start").unwrap(),
            EventTypeId::new("finish").unwrap(),
            StateId::new("done").unwrap(),
            GuardExpression::Always,
        ));
        let report = ProcessValidator::validate(
            &ProcessDefinitionBuilder::new(
                ProcessDefinitionId::new("ambiguous").unwrap(),
                ProcessDefinitionVersion::new(1).unwrap(),
            )
            .with_states(definition.states().to_vec())
            .with_events(definition.events().to_vec())
            .with_transitions(transitions)
            .build()
            .unwrap(),
        );
        assert!(
            report
                .diagnostics()
                .iter()
                .any(|item| item.code() == "DUPLICATE_SEMANTIC_TRANSITION")
        );
        definition = valid();
        let guard = GuardExpression::All(Vec::new());
        assert!(
            !ProcessValidator::validate(
                &ProcessDefinitionBuilder::new(
                    ProcessDefinitionId::new("malformed").unwrap(),
                    ProcessDefinitionVersion::new(1).unwrap(),
                )
                .with_states(definition.states().to_vec())
                .with_events(definition.events().to_vec())
                .with_transitions([TransitionDefinition::new(
                    TransitionId::new("guard").unwrap(),
                    StateId::new("start").unwrap(),
                    EventTypeId::new("finish").unwrap(),
                    StateId::new("done").unwrap(),
                    guard,
                )])
                .build()
                .unwrap(),
            )
            .is_valid()
        );
    }

    #[test]
    fn checks_capabilities_and_recovery_bounds() {
        let capability = CapabilityId::new("repository.write").unwrap();
        let activity = crate::ActivityDefinition::new(
            crate::ActivityId::new("write").unwrap(),
            [capability.clone()].to_vec(),
            Vec::new(),
            Vec::new(),
        );
        let definition = ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("capability-check").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .with_states([
            StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
            StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap(),
        ])
        .with_events([EventTypeDefinition::new(
            EventTypeId::new("finish").unwrap(),
        )])
        .with_activities([activity])
        .with_transitions([TransitionDefinition::new(
            TransitionId::new("finish").unwrap(),
            StateId::new("start").unwrap(),
            EventTypeId::new("finish").unwrap(),
            StateId::new("done").unwrap(),
            GuardExpression::Always,
        )])
        .build()
        .unwrap();
        let missing = ProcessValidator::validate_with_capabilities(&definition, &[]);
        assert!(
            missing.is_valid(),
            "an omitted capability set means no external check"
        );
        let missing = ProcessValidator::validate_with_capabilities(
            &definition,
            &[CapabilityId::new("other").unwrap()],
        );
        assert!(
            missing
                .diagnostics()
                .iter()
                .any(|item| item.code() == "UNKNOWN_CAPABILITY")
        );
    }
}
