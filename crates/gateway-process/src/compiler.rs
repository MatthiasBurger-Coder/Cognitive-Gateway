//! Deterministic semantic compilation from normalized Strict Cognitive Gherkin
//! into the canonical Process IR.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use gateway_domain::CapabilityId;

use crate::{
    ActivityConstraint, ActivityDefinition, ActivityId, BlockerDefinition, BlockerId,
    EventTypeDefinition, EventTypeId, EvidenceRequirement, EvidenceTypeId, ExecutionGraphExtension,
    GateDefinition, GateId, GateStatus, GuardExpression, InvariantDefinition, ProcessDefinition,
    ProcessDefinitionBuilder, ProcessDefinitionId, ProcessDefinitionVersion, RecoveryPolicy,
    SourceDocument, SourceLocation, SourceRule, SourceScenario, SourceStep, SourceStepKeyword,
    StateDefinition, StateId, TransitionDefinition, TransitionId,
};

/// A source-to-IR trace entry retained for explainability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilationTraceEntry {
    location: SourceLocation,
    construct: String,
    target: String,
}

impl CompilationTraceEntry {
    #[must_use]
    pub const fn location(&self) -> SourceLocation {
        self.location
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

/// One stable source diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilationDiagnostic {
    code: &'static str,
    message: String,
    location: SourceLocation,
}

impl CompilationDiagnostic {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
    #[must_use]
    pub const fn location(&self) -> SourceLocation {
        self.location
    }
}

/// Compilation failure containing deterministic diagnostics in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilationError {
    diagnostics: Vec<CompilationDiagnostic>,
}

impl CompilationError {
    #[must_use]
    pub fn diagnostics(&self) -> &[CompilationDiagnostic] {
        &self.diagnostics
    }
}

impl fmt::Display for CompilationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(first) = self.diagnostics.first() {
            write!(formatter, "{}: {}", first.code, first.message)
        } else {
            formatter.write_str("process compilation failed")
        }
    }
}

impl Error for CompilationError {}

/// Successful compiler output, including the validated candidate and trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilationResult {
    definition: ProcessDefinition,
    trace: Vec<CompilationTraceEntry>,
}

impl CompilationResult {
    #[must_use]
    pub fn definition(&self) -> &ProcessDefinition {
        &self.definition
    }
    #[must_use]
    pub fn trace(&self) -> &[CompilationTraceEntry] {
        &self.trace
    }
}

/// Stateless compiler for Strict Cognitive Gherkin v1.
#[derive(Debug, Default, Clone, Copy)]
pub struct SemanticCompiler;

impl SemanticCompiler {
    /// Parses and semantically compiles one source document.
    pub fn compile(source: &str) -> Result<CompilationResult, CompilationError> {
        let document = SourceDocument::parse(source).map_err(|error| CompilationError {
            diagnostics: vec![CompilationDiagnostic {
                code: error.code(),
                message: error.message().to_owned(),
                location: error.location(),
            }],
        })?;
        Self::compile_document(&document)
    }

    /// Compiles an already normalized frontend document.
    pub fn compile_document(
        document: &SourceDocument,
    ) -> Result<CompilationResult, CompilationError> {
        let context = CompilerContext::new(document);
        context.compile()
    }
}

struct CompilerContext<'a> {
    document: &'a SourceDocument,
    diagnostics: Vec<CompilationDiagnostic>,
    trace: Vec<CompilationTraceEntry>,
    states: BTreeMap<StateId, StateFlags>,
    events: BTreeSet<EventTypeId>,
    gates: BTreeSet<GateId>,
    evidence: BTreeSet<EvidenceTypeId>,
    blockers: BTreeMap<BlockerId, BlockerDefinition>,
    invariants: Vec<InvariantDefinition>,
    activities: BTreeMap<ActivityId, ActivityDraft>,
    recovery: Vec<RecoveryPolicy>,
    transitions: Vec<TransitionDefinition>,
    transition_number: usize,
}

#[derive(Clone, Copy)]
struct StateFlags {
    initial: bool,
    terminal: bool,
}

#[derive(Default)]
struct ActivityDraft {
    capabilities: Vec<CapabilityId>,
    evidence: Vec<EvidenceTypeId>,
    constraints: Vec<ActivityConstraint>,
}

impl<'a> CompilerContext<'a> {
    fn new(document: &'a SourceDocument) -> Self {
        Self {
            document,
            diagnostics: Vec::new(),
            trace: Vec::new(),
            states: BTreeMap::new(),
            events: BTreeSet::new(),
            gates: BTreeSet::new(),
            evidence: BTreeSet::new(),
            blockers: BTreeMap::new(),
            invariants: Vec::new(),
            activities: BTreeMap::new(),
            recovery: Vec::new(),
            transitions: Vec::new(),
            transition_number: 0,
        }
    }

    fn compile(mut self) -> Result<CompilationResult, CompilationError> {
        self.check_tags();
        let process_id = match ProcessDefinitionId::new(self.document.process_id()) {
            Ok(value) => value,
            Err(error) => {
                self.error(
                    "INVALID_PROCESS_ID",
                    error.to_string(),
                    self.document.feature_location(),
                );
                ProcessDefinitionId::new("invalid-process").expect("static fallback is valid")
            }
        };
        let version = match self.document.process_version().parse::<u32>() {
            Ok(value) => match ProcessDefinitionVersion::new(value) {
                Ok(version) => version,
                Err(error) => {
                    self.error(
                        "INVALID_PROCESS_VERSION",
                        error.to_string(),
                        self.document.feature_location(),
                    );
                    ProcessDefinitionVersion::new(1).expect("static fallback is valid")
                }
            },
            Err(error) => {
                self.error(
                    "INVALID_PROCESS_VERSION",
                    error.to_string(),
                    self.document.feature_location(),
                );
                ProcessDefinitionVersion::new(1).expect("static fallback is valid")
            }
        };
        if self.document.language_version() != "1" {
            self.error(
                "UNSUPPORTED_LANGUAGE_VERSION",
                "only Strict Cognitive Gherkin version 1 is supported",
                self.document.feature_location(),
            );
        }
        self.compile_declarations(&self.document.rules()[0]);
        self.compile_scenarios(&self.document.rules()[0]);
        if self.diagnostics.is_empty() {
            let states = self
                .states
                .iter()
                .map(|(id, flags)| {
                    StateDefinition::new(id.clone(), flags.initial, flags.terminal)
                        .expect("flags were checked")
                })
                .collect::<Vec<_>>();
            let events = self
                .events
                .iter()
                .cloned()
                .map(EventTypeDefinition::new)
                .collect::<Vec<_>>();
            let gates = self
                .gates
                .iter()
                .cloned()
                .map(|id| GateDefinition::new(id, Vec::new()))
                .collect::<Vec<_>>();
            let evidence = self
                .evidence
                .iter()
                .cloned()
                .map(|id| EvidenceRequirement::new(id, true))
                .collect::<Vec<_>>();
            let blockers = self.blockers.into_values().collect::<Vec<_>>();
            let activities = self
                .activities
                .into_iter()
                .map(|(id, draft)| {
                    ActivityDefinition::new(
                        id,
                        draft.capabilities,
                        draft.evidence,
                        draft.constraints,
                    )
                })
                .collect::<Vec<_>>();
            let definition = ProcessDefinitionBuilder::new(process_id, version)
                .with_states(states)
                .with_events(events)
                .with_transitions(self.transitions)
                .with_gates(gates)
                .with_evidence(evidence)
                .with_invariants(self.invariants)
                .with_blockers(blockers)
                .with_activities(activities)
                .with_recovery(self.recovery)
                .with_extensions([ExecutionGraphExtension::new("execution-graph", false)
                    .expect("static extension is valid")])
                .build()
                .map_err(|error| CompilationError {
                    diagnostics: vec![CompilationDiagnostic {
                        code: "INVALID_PROCESS_IR",
                        message: error.to_string(),
                        location: self.document.feature_location(),
                    }],
                })?;
            return Ok(CompilationResult {
                definition,
                trace: self.trace,
            });
        }
        Err(CompilationError {
            diagnostics: self.diagnostics,
        })
    }

    fn check_tags(&mut self) {
        for tag in self.document.tags() {
            if !matches!(tag.name(), "process" | "process-version" | "cg-language") {
                self.error(
                    "UNKNOWN_TAG",
                    format!("tag @{} is not part of the v1 vocabulary", tag.name()),
                    tag.location(),
                );
            }
        }
    }

    fn compile_declarations(&mut self, rule: &SourceRule) {
        for step in rule.declarations() {
            self.compile_declaration(step);
        }
    }

    fn compile_declaration(&mut self, step: &SourceStep) {
        let tokens = step.text().split_whitespace().collect::<Vec<_>>();
        if tokens.is_empty() {
            self.error(
                "EMPTY_DECLARATION",
                "declaration cannot be empty",
                step.location(),
            );
            return;
        }
        match tokens.as_slice() {
            ["state", id] => self.declare_state(id, false, false, step),
            ["state", id, "is", "initial"] => self.declare_state(id, true, false, step),
            ["state", id, "is", "terminal"] => self.declare_state(id, false, true, step),
            ["event", id] => self.declare_event(id, step),
            ["gate", id] => self.declare_gate(id, step),
            ["evidence", id] => self.declare_evidence(id, step),
            ["activity", id] => self.declare_activity(id, step),
            ["activity", id, "requires", "capability", capability] => {
                self.activity_capability(id, capability, step)
            }
            ["activity", id, "produces", "evidence", evidence] => {
                self.activity_evidence(id, evidence, step)
            }
            ["activity", id, "constrained", "by", constraint] => {
                self.activity_constraint(id, constraint, step)
            }
            ["invariant", id, "requires", "gate", gate, "passed"] => {
                self.declare_invariant(id, gate, step)
            }
            ["retry", event, "max", attempts] => self.declare_retry(event, attempts, None, step),
            ["retry", event, "max", attempts, "repair", state] => {
                self.declare_retry(event, attempts, Some(state), step)
            }
            _ if tokens.first() == Some(&"blocker") => self.declare_blocker(step),
            _ => self.error(
                "UNKNOWN_DECLARATION",
                format!("unrecognized declaration: {}", step.text()),
                step.location(),
            ),
        }
    }

    fn declare_state(&mut self, raw: &str, initial: bool, terminal: bool, step: &SourceStep) {
        let Ok(id) = StateId::new(raw) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid state identifier {raw:?}"),
                step.location(),
            );
            return;
        };
        if let Some(previous) = self.states.get(&id) {
            if previous.initial != initial || previous.terminal != terminal {
                self.error(
                    "CONFLICTING_DECLARATION",
                    format!("state {id} has conflicting declarations"),
                    step.location(),
                );
            } else {
                self.error(
                    "DUPLICATE_DECLARATION",
                    format!("state {id} is declared more than once"),
                    step.location(),
                );
            }
            return;
        }
        self.states
            .insert(id.clone(), StateFlags { initial, terminal });
        self.trace(step, "state declaration", id.to_string());
    }

    fn declare_event(&mut self, raw: &str, step: &SourceStep) {
        let Ok(id) = EventTypeId::new(raw) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid event identifier {raw:?}"),
                step.location(),
            );
            return;
        };
        if !self.events.insert(id.clone()) {
            self.error(
                "DUPLICATE_DECLARATION",
                format!("event {id} is declared more than once"),
                step.location(),
            );
            return;
        }
        self.trace(step, "event declaration", id.to_string());
    }

    fn declare_gate(&mut self, raw: &str, step: &SourceStep) {
        let Ok(id) = GateId::new(raw) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid gate identifier {raw:?}"),
                step.location(),
            );
            return;
        };
        if !self.gates.insert(id.clone()) {
            self.error(
                "DUPLICATE_DECLARATION",
                format!("gate {id} is declared more than once"),
                step.location(),
            );
            return;
        }
        self.trace(step, "gate declaration", id.to_string());
    }

    fn declare_evidence(&mut self, raw: &str, step: &SourceStep) {
        let Ok(id) = EvidenceTypeId::new(raw) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid evidence identifier {raw:?}"),
                step.location(),
            );
            return;
        };
        if !self.evidence.insert(id.clone()) {
            self.error(
                "DUPLICATE_DECLARATION",
                format!("evidence {id} is declared more than once"),
                step.location(),
            );
            return;
        }
        self.trace(step, "evidence declaration", id.to_string());
    }

    fn declare_activity(&mut self, raw: &str, step: &SourceStep) {
        let Ok(id) = ActivityId::new(raw) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid activity identifier {raw:?}"),
                step.location(),
            );
            return;
        };
        if self.activities.contains_key(&id) {
            self.error(
                "DUPLICATE_DECLARATION",
                format!("activity {id} is declared more than once"),
                step.location(),
            );
            return;
        }
        self.activities.insert(id.clone(), ActivityDraft::default());
        self.trace(step, "activity declaration", id.to_string());
    }

    fn activity_capability(&mut self, activity: &str, capability: &str, step: &SourceStep) {
        let Ok(activity_id) = ActivityId::new(activity) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid activity identifier {activity:?}"),
                step.location(),
            );
            return;
        };
        let Ok(capability_id) = CapabilityId::new(capability) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid capability identifier {capability:?}"),
                step.location(),
            );
            return;
        };
        let draft = self.activities.entry(activity_id).or_default();
        if draft.capabilities.contains(&capability_id) {
            self.error(
                "DUPLICATE_DECLARATION",
                format!("capability {capability} is repeated for activity {activity}"),
                step.location(),
            );
            return;
        }
        draft.capabilities.push(capability_id.clone());
        self.trace(
            step,
            "activity capability",
            format!("{activity}:{capability_id}"),
        );
    }

    fn activity_evidence(&mut self, activity: &str, evidence: &str, step: &SourceStep) {
        let Ok(activity_id) = ActivityId::new(activity) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid activity identifier {activity:?}"),
                step.location(),
            );
            return;
        };
        let Ok(evidence_id) = EvidenceTypeId::new(evidence) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid evidence identifier {evidence:?}"),
                step.location(),
            );
            return;
        };
        let draft = self.activities.entry(activity_id).or_default();
        draft.evidence.push(evidence_id);
    }

    fn activity_constraint(&mut self, activity: &str, raw: &str, step: &SourceStep) {
        let Ok(activity_id) = ActivityId::new(activity) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid activity identifier {activity:?}"),
                step.location(),
            );
            return;
        };
        let Some((name, value)) = raw.split_once('=') else {
            self.error(
                "INVALID_CONSTRAINT",
                "activity constraint must be name=value",
                step.location(),
            );
            return;
        };
        let Ok(constraint) = ActivityConstraint::new(name, value) else {
            self.error(
                "INVALID_CONSTRAINT",
                "activity constraint fields cannot be empty",
                step.location(),
            );
            return;
        };
        let draft = self.activities.entry(activity_id).or_default();
        draft.constraints.push(constraint);
    }

    fn declare_blocker(&mut self, step: &SourceStep) {
        let text = step.text();
        let Some(rest) = text.strip_prefix("blocker ") else {
            self.error(
                "UNKNOWN_DECLARATION",
                "invalid blocker declaration",
                step.location(),
            );
            return;
        };
        let Some((id, reason)) = rest.split_once(" reason ") else {
            self.error(
                "INVALID_DECLARATION",
                "blocker requires reason",
                step.location(),
            );
            return;
        };
        let (reason, resolvable) = reason
            .strip_suffix(" resolvable")
            .map_or((reason, false), |value| (value, true));
        let Ok(id) = BlockerId::new(id) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid blocker identifier {id:?}"),
                step.location(),
            );
            return;
        };
        match BlockerDefinition::new(id.clone(), reason, resolvable) {
            Ok(definition) => {
                if self.blockers.insert(id.clone(), definition).is_some() {
                    self.error(
                        "DUPLICATE_DECLARATION",
                        format!("blocker {id} is declared more than once"),
                        step.location(),
                    );
                } else {
                    self.trace(step, "blocker declaration", id.to_string());
                }
            }
            Err(error) => self.error("INVALID_DECLARATION", error.to_string(), step.location()),
        }
    }

    fn declare_invariant(&mut self, raw: &str, gate: &str, step: &SourceStep) {
        let Ok(id) = BlockerId::new(raw) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid invariant identifier {raw:?}"),
                step.location(),
            );
            return;
        };
        let Ok(gate_id) = GateId::new(gate) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid gate identifier {gate:?}"),
                step.location(),
            );
            return;
        };
        match InvariantDefinition::new(
            id,
            GuardExpression::GateIs {
                gate: gate_id,
                status: GateStatus::Passed,
            },
            format!("invariant requires gate {gate} passed"),
        ) {
            Ok(value) => self.invariants.push(value),
            Err(error) => self.error("INVALID_DECLARATION", error.to_string(), step.location()),
        }
    }

    fn declare_retry(
        &mut self,
        event: &str,
        attempts: &str,
        repair: Option<&str>,
        step: &SourceStep,
    ) {
        if !self.events.iter().any(|value| value.as_str() == event) {
            self.error(
                "UNKNOWN_REFERENCE",
                format!("event {event} is not declared"),
                step.location(),
            );
            return;
        }
        let Ok(attempts) = attempts.parse::<u32>() else {
            self.error(
                "INVALID_RETRY_POLICY",
                "retry max must be a positive integer",
                step.location(),
            );
            return;
        };
        let repair = match repair {
            Some(value) => match StateId::new(value) {
                Ok(id) => Some(id),
                Err(_) => {
                    self.error(
                        "INVALID_IDENTIFIER",
                        format!("invalid repair state {value:?}"),
                        step.location(),
                    );
                    return;
                }
            },
            None => None,
        };
        match RecoveryPolicy::new(attempts, repair) {
            Ok(policy) => self.recovery.push(policy),
            Err(error) => self.error("INVALID_RETRY_POLICY", error.to_string(), step.location()),
        }
    }

    fn compile_scenarios(&mut self, rule: &SourceRule) {
        for scenario in rule.scenarios() {
            self.compile_scenario(scenario);
        }
    }

    fn compile_scenario(&mut self, scenario: &SourceScenario) {
        let mut state = None;
        let mut event = None;
        let mut target = None;
        let mut guards = Vec::new();
        let mut required_gates = Vec::new();
        let mut required_evidence = Vec::new();
        let mut activity = None;
        let mut blocker = None;
        let mut pauses = false;
        let mut completes = false;
        let mut retry = None;
        let mut repair_target = None;
        for step in scenario.steps() {
            let text = step.text();
            let tokens = text.split_whitespace().collect::<Vec<_>>();
            match tokens.as_slice() {
                ["process", "state", value] => self.parse_state_guard(value, step, &mut state),
                ["gate", gate, "is", status] => {
                    self.parse_gate_guard(gate, status, step, &mut guards)
                }
                ["evidence", evidence, "is", "present"] => {
                    self.parse_evidence_guard(evidence, step, &mut guards)
                }
                ["blocker", value, "is", "active"] => {
                    self.parse_blocker_guard(value, step, &mut guards)
                }
                ["capability", value, "is", "available"] => {
                    self.parse_capability_guard(value, step, &mut guards)
                }
                ["event", value, "occurs"]
                    if matches!(
                        step.keyword(),
                        SourceStepKeyword::When | SourceStepKeyword::And | SourceStepKeyword::But
                    ) =>
                {
                    self.parse_event(value, step, &mut event)
                }
                ["transition", "to", "state", value] => self.parse_target(value, step, &mut target),
                ["require", "gate", value] => {
                    self.require_gate(value, step, &mut required_gates, &mut guards)
                }
                ["require", "evidence", value] => {
                    self.require_evidence(value, step, &mut required_evidence, &mut guards)
                }
                ["authorize", "activity", value] => {
                    self.authorize_activity(value, step, &mut activity)
                }
                ["require", "capability", value] => {
                    self.parse_capability_guard(value, step, &mut guards)
                }
                ["block", "process", "with", value] => {
                    self.parse_blocker_result(value, step, &mut blocker)
                }
                ["pause", "process"] => pauses = true,
                ["complete", "process"] => completes = true,
                ["retry", "activity", "max", value] => retry = self.parse_retry_result(value, step),
                ["repair", "through", "state", value] => {
                    self.parse_repair_target(value, step, &mut repair_target)
                }
                _ => self.error(
                    "UNKNOWN_STATEMENT",
                    format!("unrecognized semantic statement: {text}"),
                    step.location(),
                ),
            }
        }
        let Some(from) = state else {
            self.error(
                "MISSING_PROCESS_STATE",
                format!("scenario {} needs process state", scenario.name()),
                scenario.location(),
            );
            return;
        };
        let Some(event) = event else {
            self.error(
                "MISSING_EVENT",
                format!("scenario {} needs an event", scenario.name()),
                scenario.location(),
            );
            return;
        };
        let Some(to) = target else {
            self.error(
                "MISSING_TARGET_STATE",
                format!("scenario {} needs a transition target", scenario.name()),
                scenario.location(),
            );
            return;
        };
        if !self.states.contains_key(&from) {
            self.error(
                "UNKNOWN_REFERENCE",
                format!("state {from} is not declared"),
                scenario.location(),
            );
        }
        if !self.events.contains(&event) {
            self.error(
                "UNKNOWN_REFERENCE",
                format!("event {event} is not declared"),
                scenario.location(),
            );
        }
        if !self.states.contains_key(&to) {
            self.error(
                "UNKNOWN_REFERENCE",
                format!("state {to} is not declared"),
                scenario.location(),
            );
        }
        let guard = match guards.len() {
            0 => GuardExpression::Always,
            1 => guards.remove(0),
            _ => GuardExpression::All(guards),
        };
        self.transition_number += 1;
        let id = TransitionId::new(format!("transition-{}", self.transition_number))
            .expect("static prefix is valid");
        let mut transition = TransitionDefinition::new(id, from, event, to, guard)
            .with_required_gates(required_gates)
            .with_required_evidence(required_evidence);
        if let Some(activity) = activity {
            transition = transition.with_authorized_activity(activity);
        }
        if let Some(blocker) = blocker {
            transition = transition.with_blocker(blocker);
        }
        if pauses {
            transition = transition.as_pausing();
        }
        if completes {
            transition = transition.as_completing();
        }
        if let Some(retry) = retry {
            transition = transition.with_retry(retry);
        }
        if let Some(repair) = repair_target {
            transition = transition.with_repair_target(repair);
        }
        self.transitions.push(transition);
    }

    fn parse_state_guard(&mut self, raw: &str, step: &SourceStep, state: &mut Option<StateId>) {
        match StateId::new(raw) {
            Ok(value) => *state = Some(value),
            Err(_) => self.error(
                "INVALID_IDENTIFIER",
                format!("invalid state identifier {raw:?}"),
                step.location(),
            ),
        }
    }
    fn parse_gate_guard(
        &mut self,
        gate: &str,
        status: &str,
        step: &SourceStep,
        guards: &mut Vec<GuardExpression>,
    ) {
        let Ok(gate) = GateId::new(gate) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid gate identifier {gate:?}"),
                step.location(),
            );
            return;
        };
        let status = match gate_status(status) {
            Some(value) => value,
            None => {
                self.error(
                    "INVALID_GATE_STATUS",
                    format!("unknown gate status {status:?}"),
                    step.location(),
                );
                return;
            }
        };
        guards.push(GuardExpression::GateIs { gate, status });
    }
    fn parse_evidence_guard(
        &mut self,
        evidence: &str,
        step: &SourceStep,
        guards: &mut Vec<GuardExpression>,
    ) {
        match EvidenceTypeId::new(evidence) {
            Ok(value) => guards.push(GuardExpression::EvidencePresent(value)),
            Err(_) => self.error(
                "INVALID_IDENTIFIER",
                format!("invalid evidence identifier {evidence:?}"),
                step.location(),
            ),
        }
    }
    fn parse_blocker_guard(
        &mut self,
        blocker: &str,
        step: &SourceStep,
        guards: &mut Vec<GuardExpression>,
    ) {
        match BlockerId::new(blocker) {
            Ok(value) => guards.push(GuardExpression::BlockerActive(value)),
            Err(_) => self.error(
                "INVALID_IDENTIFIER",
                format!("invalid blocker identifier {blocker:?}"),
                step.location(),
            ),
        }
    }
    fn parse_capability_guard(
        &mut self,
        capability: &str,
        step: &SourceStep,
        guards: &mut Vec<GuardExpression>,
    ) {
        match CapabilityId::new(capability) {
            Ok(value) => guards.push(GuardExpression::CapabilityAvailable(value)),
            Err(_) => self.error(
                "INVALID_IDENTIFIER",
                format!("invalid capability identifier {capability:?}"),
                step.location(),
            ),
        }
    }
    fn parse_event(&mut self, raw: &str, step: &SourceStep, event: &mut Option<EventTypeId>) {
        match EventTypeId::new(raw) {
            Ok(value) => *event = Some(value),
            Err(_) => self.error(
                "INVALID_IDENTIFIER",
                format!("invalid event identifier {raw:?}"),
                step.location(),
            ),
        }
    }
    fn parse_target(&mut self, raw: &str, step: &SourceStep, target: &mut Option<StateId>) {
        match StateId::new(raw) {
            Ok(value) => *target = Some(value),
            Err(_) => self.error(
                "INVALID_IDENTIFIER",
                format!("invalid state identifier {raw:?}"),
                step.location(),
            ),
        }
    }
    fn require_gate(
        &mut self,
        raw: &str,
        step: &SourceStep,
        required: &mut Vec<GateId>,
        guards: &mut Vec<GuardExpression>,
    ) {
        let Ok(value) = GateId::new(raw) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid gate identifier {raw:?}"),
                step.location(),
            );
            return;
        };
        if !self.gates.contains(&value) {
            self.error(
                "UNKNOWN_REFERENCE",
                format!("gate {value} is not declared"),
                step.location(),
            );
        }
        required.push(value.clone());
        guards.push(GuardExpression::GateIs {
            gate: value,
            status: GateStatus::Passed,
        });
    }
    fn require_evidence(
        &mut self,
        raw: &str,
        step: &SourceStep,
        required: &mut Vec<EvidenceTypeId>,
        guards: &mut Vec<GuardExpression>,
    ) {
        let Ok(value) = EvidenceTypeId::new(raw) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid evidence identifier {raw:?}"),
                step.location(),
            );
            return;
        };
        if !self.evidence.contains(&value) {
            self.error(
                "UNKNOWN_REFERENCE",
                format!("evidence {value} is not declared"),
                step.location(),
            );
        }
        required.push(value.clone());
        guards.push(GuardExpression::EvidencePresent(value));
    }
    fn authorize_activity(
        &mut self,
        raw: &str,
        step: &SourceStep,
        activity: &mut Option<ActivityId>,
    ) {
        let Ok(value) = ActivityId::new(raw) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid activity identifier {raw:?}"),
                step.location(),
            );
            return;
        };
        if !self.activities.contains_key(&value) {
            self.error(
                "UNKNOWN_REFERENCE",
                format!("activity {value} is not declared"),
                step.location(),
            );
        }
        *activity = Some(value);
    }
    fn parse_blocker_result(
        &mut self,
        raw: &str,
        step: &SourceStep,
        blocker: &mut Option<BlockerId>,
    ) {
        let Ok(value) = BlockerId::new(raw) else {
            self.error(
                "INVALID_IDENTIFIER",
                format!("invalid blocker identifier {raw:?}"),
                step.location(),
            );
            return;
        };
        if !self.blockers.contains_key(&value) {
            self.error(
                "UNKNOWN_REFERENCE",
                format!("blocker {value} is not declared"),
                step.location(),
            );
        }
        *blocker = Some(value);
    }
    fn parse_retry_result(&mut self, raw: &str, step: &SourceStep) -> Option<u32> {
        match raw.parse::<u32>() {
            Ok(value) if value > 0 => Some(value),
            _ => {
                self.error(
                    "INVALID_RETRY_POLICY",
                    "retry max must be a positive integer",
                    step.location(),
                );
                None
            }
        }
    }
    fn parse_repair_target(&mut self, raw: &str, step: &SourceStep, target: &mut Option<StateId>) {
        match StateId::new(raw) {
            Ok(value) => {
                if !self.states.contains_key(&value) {
                    self.error(
                        "UNKNOWN_REFERENCE",
                        format!("repair state {value} is not declared"),
                        step.location(),
                    );
                }
                *target = Some(value);
            }
            Err(_) => self.error(
                "INVALID_IDENTIFIER",
                format!("invalid state identifier {raw:?}"),
                step.location(),
            ),
        }
    }

    fn trace(&mut self, step: &SourceStep, construct: &str, target: String) {
        self.trace.push(CompilationTraceEntry {
            location: step.location(),
            construct: construct.to_owned(),
            target,
        });
    }
    fn error(&mut self, code: &'static str, message: impl Into<String>, location: SourceLocation) {
        self.diagnostics.push(CompilationDiagnostic {
            code,
            message: message.into(),
            location,
        });
    }
}

fn gate_status(value: &str) -> Option<GateStatus> {
    match value {
        "open" => Some(GateStatus::Open),
        "passed" => Some(GateStatus::Passed),
        "failed" => Some(GateStatus::Failed),
        "blocked" => Some(GateStatus::Blocked),
        "waiting-for-evidence" => Some(GateStatus::WaitingForEvidence),
        "waiting-for-authorization" => Some(GateStatus::WaitingForAuthorization),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = include_str!("../fixtures/strict-cognitive-gherkin/valid.feature");

    #[test]
    fn compiles_valid_source_to_ir_and_trace() {
        let result = SemanticCompiler::compile(VALID).unwrap();
        assert_eq!(
            result.definition().identity().id().as_str(),
            "canonical-issue-lifecycle"
        );
        assert_eq!(result.definition().transitions().len(), 2);
        assert!(!result.trace().is_empty());
    }

    #[test]
    fn rejects_unknown_semantics_versions_and_references() {
        let unknown =
            include_str!("../fixtures/strict-cognitive-gherkin/invalid-unknown-step.feature");
        assert_eq!(
            SemanticCompiler::compile(unknown)
                .unwrap_err()
                .diagnostics()[0]
                .code(),
            "UNKNOWN_STATEMENT"
        );
        let version = include_str!("../fixtures/strict-cognitive-gherkin/invalid-version.feature");
        assert_eq!(
            SemanticCompiler::compile(version)
                .unwrap_err()
                .diagnostics()[0]
                .code(),
            "UNSUPPORTED_LANGUAGE_VERSION"
        );
        let reference =
            include_str!("../fixtures/strict-cognitive-gherkin/invalid-reference.feature");
        assert_eq!(
            SemanticCompiler::compile(reference)
                .unwrap_err()
                .diagnostics()[0]
                .code(),
            "UNKNOWN_REFERENCE"
        );
    }

    #[test]
    fn compiles_typed_guards_and_activity_contracts() {
        let source = "@process(example)\n@process-version(1)\n@cg-language(1)\nFeature: Example\nRule: Process\nGiven state START is initial\nGiven state DONE is terminal\nGiven event finish\nGiven gate review\nGiven evidence report\nGiven activity ship requires capability repository.write\nGiven blocker blocked reason needs review resolvable\nScenario: finish\nGiven process state START\nGiven gate review is passed\nGiven evidence report is present\nWhen event finish occurs\nThen require gate review\nThen require evidence report\nThen authorize activity ship\nThen transition to state DONE\nThen complete process\n";
        let result = SemanticCompiler::compile(source).unwrap();
        let transition = &result.definition().transitions()[0];
        assert_eq!(transition.required_gates().len(), 1);
        assert_eq!(transition.required_evidence().len(), 1);
        assert_eq!(transition.authorized_activity().unwrap().as_str(), "ship");
        assert!(transition.completes());
    }

    #[test]
    fn compiler_output_is_deterministic_for_identical_source() {
        let first = SemanticCompiler::compile(VALID).unwrap();
        let second = SemanticCompiler::compile(VALID).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.definition().to_json().unwrap(),
            second.definition().to_json().unwrap()
        );
    }
}
