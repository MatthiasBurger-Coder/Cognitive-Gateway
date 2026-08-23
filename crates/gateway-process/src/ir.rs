use std::{collections::BTreeSet, fmt};

use gateway_domain::CapabilityId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    ActivityId, BlockerId, EventTypeId, EvidenceTypeId, GateId, ProcessDefinitionDigest,
    ProcessDefinitionId, ProcessDefinitionVersion, ProcessError, StateId, TransitionId,
    ValidationCode,
};

/// Only the known canonical Process IR version is accepted by v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ProcessIrVersion {
    V1,
}

/// Stable identity tuple for a definition, including its content digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionIdentity {
    id: ProcessDefinitionId,
    version: ProcessDefinitionVersion,
    digest: ProcessDefinitionDigest,
}

impl DefinitionIdentity {
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

/// Declarative typed guard tree. It cannot contain executable callbacks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardExpression {
    Always,
    Never,
    All(Vec<Self>),
    Any(Vec<Self>),
    Not(Box<Self>),
    EventAttributeEquals {
        name: String,
        value: String,
    },
    EvidencePresent(EvidenceTypeId),
    CapabilityAvailable(gateway_domain::CapabilityId),
    BlockerActive(BlockerId),
    AuthorizationIs {
        authorization: crate::AuthorizationId,
        status: crate::AuthorizationStatus,
    },
    PolicyDecisionIs {
        policy: crate::PolicyDecisionId,
        status: crate::PolicyDecisionStatus,
    },
    GateIs {
        gate: GateId,
        status: GateStatus,
    },
}

/// The finite gate statuses available in Process IR v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GateStatus {
    Open,
    Passed,
    Failed,
    Blocked,
    WaitingForEvidence,
    WaitingForAuthorization,
}

/// A process state with explicit initial and terminal semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateDefinition {
    id: StateId,
    initial: bool,
    terminal: bool,
}

impl StateDefinition {
    pub fn new(id: StateId, initial: bool, terminal: bool) -> Result<Self, ProcessError> {
        if initial && terminal {
            return Err(ProcessError::new(
                ValidationCode::InvalidDefinition,
                "initial state cannot be terminal",
            ));
        }
        Ok(Self {
            id,
            initial,
            terminal,
        })
    }
    #[must_use]
    pub fn id(&self) -> &StateId {
        &self.id
    }
    #[must_use]
    pub const fn is_initial(&self) -> bool {
        self.initial
    }
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        self.terminal
    }
}

/// A declared event type, separate from runtime event occurrences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventTypeDefinition {
    id: EventTypeId,
}

impl EventTypeDefinition {
    #[must_use]
    pub const fn new(id: EventTypeId) -> Self {
        Self { id }
    }
    #[must_use]
    pub fn id(&self) -> &EventTypeId {
        &self.id
    }
}

/// A transition selected by state, event and a typed guard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionDefinition {
    id: TransitionId,
    from: StateId,
    event: EventTypeId,
    to: StateId,
    guard: GuardExpression,
    automatic: bool,
    required_gates: Vec<GateId>,
    required_evidence: Vec<EvidenceTypeId>,
    authorized_activity: Option<ActivityId>,
    blocker: Option<BlockerId>,
    pauses: bool,
    completes: bool,
    retry_attempts: Option<u32>,
    repair_target: Option<StateId>,
}

impl TransitionDefinition {
    pub fn new(
        id: TransitionId,
        from: StateId,
        event: EventTypeId,
        to: StateId,
        guard: GuardExpression,
    ) -> Self {
        Self {
            id,
            from,
            event,
            to,
            guard,
            automatic: false,
            required_gates: Vec::new(),
            required_evidence: Vec::new(),
            authorized_activity: None,
            blocker: None,
            pauses: false,
            completes: false,
            retry_attempts: None,
            repair_target: None,
        }
    }
    #[must_use]
    pub fn id(&self) -> &TransitionId {
        &self.id
    }
    #[must_use]
    pub fn from(&self) -> &StateId {
        &self.from
    }
    #[must_use]
    pub fn event(&self) -> &EventTypeId {
        &self.event
    }
    #[must_use]
    pub fn to(&self) -> &StateId {
        &self.to
    }
    #[must_use]
    pub fn guard(&self) -> &GuardExpression {
        &self.guard
    }
    #[must_use]
    pub const fn is_automatic(&self) -> bool {
        self.automatic
    }
    #[must_use]
    pub fn as_automatic(mut self) -> Self {
        self.automatic = true;
        self
    }
    #[must_use]
    pub fn with_required_gates(mut self, mut values: Vec<GateId>) -> Self {
        values.sort();
        self.required_gates = values;
        self
    }
    #[must_use]
    pub fn with_required_evidence(mut self, mut values: Vec<EvidenceTypeId>) -> Self {
        values.sort();
        self.required_evidence = values;
        self
    }
    #[must_use]
    pub fn with_authorized_activity(mut self, value: ActivityId) -> Self {
        self.authorized_activity = Some(value);
        self
    }
    #[must_use]
    pub fn with_blocker(mut self, value: BlockerId) -> Self {
        self.blocker = Some(value);
        self
    }
    #[must_use]
    pub fn as_pausing(mut self) -> Self {
        self.pauses = true;
        self
    }
    #[must_use]
    pub fn as_completing(mut self) -> Self {
        self.completes = true;
        self
    }
    #[must_use]
    pub fn with_retry(mut self, max_attempts: u32) -> Self {
        self.retry_attempts = Some(max_attempts);
        self
    }
    #[must_use]
    pub fn with_repair_target(mut self, target: StateId) -> Self {
        self.repair_target = Some(target);
        self
    }
    #[must_use]
    pub fn required_gates(&self) -> &[GateId] {
        &self.required_gates
    }
    #[must_use]
    pub fn required_evidence(&self) -> &[EvidenceTypeId] {
        &self.required_evidence
    }
    #[must_use]
    pub fn authorized_activity(&self) -> Option<&ActivityId> {
        self.authorized_activity.as_ref()
    }
    #[must_use]
    pub fn blocker(&self) -> Option<&BlockerId> {
        self.blocker.as_ref()
    }
    #[must_use]
    pub const fn pauses(&self) -> bool {
        self.pauses
    }
    #[must_use]
    pub const fn completes(&self) -> bool {
        self.completes
    }
    #[must_use]
    pub const fn retry_attempts(&self) -> Option<u32> {
        self.retry_attempts
    }
    #[must_use]
    pub fn repair_target(&self) -> Option<&StateId> {
        self.repair_target.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRequirement {
    evidence_type: EvidenceTypeId,
    required: bool,
}

impl EvidenceRequirement {
    #[must_use]
    pub const fn new(evidence_type: EvidenceTypeId, required: bool) -> Self {
        Self {
            evidence_type,
            required,
        }
    }
    #[must_use]
    pub fn evidence_type(&self) -> &EvidenceTypeId {
        &self.evidence_type
    }
    #[must_use]
    pub const fn required(&self) -> bool {
        self.required
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateDefinition {
    id: GateId,
    required_evidence: Vec<EvidenceRequirement>,
}

impl GateDefinition {
    pub fn new(id: GateId, mut required_evidence: Vec<EvidenceRequirement>) -> Self {
        required_evidence.sort_by(|left, right| left.evidence_type.cmp(&right.evidence_type));
        Self {
            id,
            required_evidence,
        }
    }
    #[must_use]
    pub fn id(&self) -> &GateId {
        &self.id
    }
    #[must_use]
    pub fn required_evidence(&self) -> &[EvidenceRequirement] {
        &self.required_evidence
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvariantDefinition {
    id: BlockerId,
    condition: GuardExpression,
    reason: String,
}

impl InvariantDefinition {
    pub fn new(
        id: BlockerId,
        condition: GuardExpression,
        reason: impl Into<String>,
    ) -> Result<Self, ProcessError> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(ProcessError::new(
                ValidationCode::InvalidDefinition,
                "invariant reason cannot be empty",
            ));
        }
        Ok(Self {
            id,
            condition,
            reason,
        })
    }
    #[must_use]
    pub fn id(&self) -> &BlockerId {
        &self.id
    }
    #[must_use]
    pub fn condition(&self) -> &GuardExpression {
        &self.condition
    }
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockerDefinition {
    id: BlockerId,
    reason: String,
    resolvable: bool,
}

impl BlockerDefinition {
    pub fn new(
        id: BlockerId,
        reason: impl Into<String>,
        resolvable: bool,
    ) -> Result<Self, ProcessError> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(ProcessError::new(
                ValidationCode::InvalidDefinition,
                "blocker reason cannot be empty",
            ));
        }
        Ok(Self {
            id,
            reason,
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
    pub const fn resolvable(&self) -> bool {
        self.resolvable
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityConstraint {
    name: String,
    value: String,
}

impl ActivityConstraint {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Result<Self, ProcessError> {
        let name = name.into();
        let value = value.into();
        if name.trim().is_empty() || value.trim().is_empty() {
            return Err(ProcessError::new(
                ValidationCode::InvalidDefinition,
                "activity constraint fields cannot be empty",
            ));
        }
        Ok(Self { name, value })
    }
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityDefinition {
    id: ActivityId,
    capabilities: Vec<CapabilityId>,
    output_evidence: Vec<EvidenceTypeId>,
    constraints: Vec<ActivityConstraint>,
}

impl ActivityDefinition {
    pub fn new(
        id: ActivityId,
        mut capabilities: Vec<CapabilityId>,
        mut output_evidence: Vec<EvidenceTypeId>,
        mut constraints: Vec<ActivityConstraint>,
    ) -> Self {
        capabilities.sort();
        output_evidence.sort();
        constraints.sort_by(|a, b| a.name.cmp(&b.name).then(a.value.cmp(&b.value)));
        Self {
            id,
            capabilities,
            output_evidence,
            constraints,
        }
    }
    #[must_use]
    pub fn id(&self) -> &ActivityId {
        &self.id
    }
    #[must_use]
    pub fn capabilities(&self) -> &[CapabilityId] {
        &self.capabilities
    }
    #[must_use]
    pub fn output_evidence(&self) -> &[EvidenceTypeId] {
        &self.output_evidence
    }
    #[must_use]
    pub fn constraints(&self) -> &[ActivityConstraint] {
        &self.constraints
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryPolicy {
    max_attempts: u32,
    repair_target: Option<StateId>,
}

impl RecoveryPolicy {
    pub fn new(max_attempts: u32, repair_target: Option<StateId>) -> Result<Self, ProcessError> {
        if max_attempts == 0 {
            return Err(ProcessError::new(
                ValidationCode::InvalidDefinition,
                "recovery max_attempts must be positive",
            ));
        }
        Ok(Self {
            max_attempts,
            repair_target,
        })
    }
    #[must_use]
    pub const fn max_attempts(&self) -> u32 {
        self.max_attempts
    }
    #[must_use]
    pub fn repair_target(&self) -> Option<&StateId> {
        self.repair_target.as_ref()
    }
}

/// Explicit extension declaration for scheduling/graph semantics deferred from IR v1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionGraphExtension {
    kind: String,
    supported: bool,
}

impl ExecutionGraphExtension {
    pub fn new(kind: impl Into<String>, supported: bool) -> Result<Self, ProcessError> {
        let kind = kind.into();
        if kind.trim().is_empty() {
            return Err(ProcessError::new(
                ValidationCode::InvalidDefinition,
                "extension kind cannot be empty",
            ));
        }
        Ok(Self { kind, supported })
    }
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }
    #[must_use]
    pub const fn supported(&self) -> bool {
        self.supported
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessDefinition {
    ir_version: ProcessIrVersion,
    identity: DefinitionIdentity,
    states: Vec<StateDefinition>,
    events: Vec<EventTypeDefinition>,
    transitions: Vec<TransitionDefinition>,
    gates: Vec<GateDefinition>,
    evidence: Vec<EvidenceRequirement>,
    invariants: Vec<InvariantDefinition>,
    blockers: Vec<BlockerDefinition>,
    activities: Vec<ActivityDefinition>,
    recovery: Vec<RecoveryPolicy>,
    extensions: Vec<ExecutionGraphExtension>,
}

/// Builder for the canonical Process IR v1 definition.
#[derive(Debug, Clone)]
pub struct ProcessDefinitionBuilder {
    id: ProcessDefinitionId,
    version: ProcessDefinitionVersion,
    states: Vec<StateDefinition>,
    events: Vec<EventTypeDefinition>,
    transitions: Vec<TransitionDefinition>,
    gates: Vec<GateDefinition>,
    evidence: Vec<EvidenceRequirement>,
    invariants: Vec<InvariantDefinition>,
    blockers: Vec<BlockerDefinition>,
    activities: Vec<ActivityDefinition>,
    recovery: Vec<RecoveryPolicy>,
    extensions: Vec<ExecutionGraphExtension>,
}

macro_rules! builder_method {
    ($name:ident, $field:ident, $type:ty) => {
        pub fn $name(mut self, values: impl IntoIterator<Item = $type>) -> Self {
            self.$field = values.into_iter().collect();
            self
        }
    };
}

impl ProcessDefinitionBuilder {
    #[must_use]
    pub fn new(id: ProcessDefinitionId, version: ProcessDefinitionVersion) -> Self {
        Self {
            id,
            version,
            states: Vec::new(),
            events: Vec::new(),
            transitions: Vec::new(),
            gates: Vec::new(),
            evidence: Vec::new(),
            invariants: Vec::new(),
            blockers: Vec::new(),
            activities: Vec::new(),
            recovery: Vec::new(),
            extensions: Vec::new(),
        }
    }
    builder_method!(with_states, states, StateDefinition);
    builder_method!(with_events, events, EventTypeDefinition);
    builder_method!(with_transitions, transitions, TransitionDefinition);
    builder_method!(with_gates, gates, GateDefinition);
    builder_method!(with_evidence, evidence, EvidenceRequirement);
    builder_method!(with_invariants, invariants, InvariantDefinition);
    builder_method!(with_blockers, blockers, BlockerDefinition);
    builder_method!(with_activities, activities, ActivityDefinition);
    builder_method!(with_recovery, recovery, RecoveryPolicy);
    builder_method!(with_extensions, extensions, ExecutionGraphExtension);

    pub fn build(self) -> Result<ProcessDefinition, ProcessError> {
        let mut definition = ProcessDefinition {
            ir_version: ProcessIrVersion::V1,
            identity: DefinitionIdentity {
                id: self.id,
                version: self.version,
                digest: ProcessDefinitionDigest::new("0".repeat(64))?,
            },
            states: self.states,
            events: self.events,
            transitions: self.transitions,
            gates: self.gates,
            evidence: self.evidence,
            invariants: self.invariants,
            blockers: self.blockers,
            activities: self.activities,
            recovery: self.recovery,
            extensions: self.extensions,
        };
        definition.normalize();
        definition.validate_structure()?;
        let digest = definition.calculate_digest()?;
        definition.identity.digest = digest;
        Ok(definition)
    }
}

impl ProcessDefinition {
    fn normalize(&mut self) {
        self.states.sort_by(|a, b| a.id.cmp(&b.id));
        self.events.sort_by(|a, b| a.id.cmp(&b.id));
        self.transitions.sort_by(|a, b| a.id.cmp(&b.id));
        self.gates.sort_by(|a, b| a.id.cmp(&b.id));
        self.evidence
            .sort_by(|a, b| a.evidence_type.cmp(&b.evidence_type));
        self.invariants.sort_by(|a, b| a.id.cmp(&b.id));
        self.blockers.sort_by(|a, b| a.id.cmp(&b.id));
        self.activities.sort_by(|a, b| a.id.cmp(&b.id));
        self.extensions.sort_by(|a, b| a.kind.cmp(&b.kind));
    }

    fn validate_unique<T, F>(
        items: &[T],
        mut key: F,
        kind: &'static str,
    ) -> Result<(), ProcessError>
    where
        F: FnMut(&T) -> &str,
    {
        let mut seen = BTreeSet::new();
        for item in items {
            if !seen.insert(key(item)) {
                return Err(ProcessError::new(
                    ValidationCode::DuplicateIdentifier,
                    format!("duplicate {kind} identifier"),
                ));
            }
        }
        Ok(())
    }

    fn validate_structure(&self) -> Result<(), ProcessError> {
        if self.states.is_empty() || self.events.is_empty() || self.transitions.is_empty() {
            return Err(ProcessError::new(
                ValidationCode::EmptyDefinition,
                "a process needs states, events and transitions",
            ));
        }
        let initial_count = self.states.iter().filter(|state| state.initial).count();
        if initial_count == 0 {
            return Err(ProcessError::new(
                ValidationCode::MissingInitialState,
                "exactly one initial state is required",
            ));
        }
        if initial_count > 1 {
            return Err(ProcessError::new(
                ValidationCode::MultipleInitialStates,
                "exactly one initial state is required",
            ));
        }
        Self::validate_unique(&self.states, |v| v.id.as_str(), "state")?;
        Self::validate_unique(&self.events, |v| v.id.as_str(), "event")?;
        Self::validate_unique(&self.transitions, |v| v.id.as_str(), "transition")?;
        Self::validate_unique(&self.gates, |v| v.id.as_str(), "gate")?;
        Self::validate_unique(&self.invariants, |v| v.id.as_str(), "invariant")?;
        Self::validate_unique(&self.blockers, |v| v.id.as_str(), "blocker")?;
        Self::validate_unique(&self.activities, |v| v.id.as_str(), "activity")?;
        let states = self
            .states
            .iter()
            .map(|v| v.id.clone())
            .collect::<BTreeSet<_>>();
        let events = self
            .events
            .iter()
            .map(|v| v.id.clone())
            .collect::<BTreeSet<_>>();
        for transition in &self.transitions {
            if !states.contains(&transition.from)
                || !states.contains(&transition.to)
                || !events.contains(&transition.event)
            {
                return Err(ProcessError::new(
                    ValidationCode::InvalidReference,
                    format!(
                        "transition {} references an undeclared symbol",
                        transition.id
                    ),
                ));
            }
        }
        for policy in &self.recovery {
            if let Some(target) = &policy.repair_target {
                if !states.contains(target) {
                    return Err(ProcessError::new(
                        ValidationCode::InvalidReference,
                        format!("recovery target {target} is unknown"),
                    ));
                }
            }
        }
        Ok(())
    }

    fn canonical_value(&self) -> CanonicalDefinition<'_> {
        CanonicalDefinition {
            ir_version: self.ir_version,
            id: &self.identity.id,
            version: self.identity.version,
            states: &self.states,
            events: &self.events,
            transitions: &self.transitions,
            gates: &self.gates,
            evidence: &self.evidence,
            invariants: &self.invariants,
            blockers: &self.blockers,
            activities: &self.activities,
            recovery: &self.recovery,
            extensions: &self.extensions,
        }
    }

    fn calculate_digest(&self) -> Result<ProcessDefinitionDigest, ProcessError> {
        let bytes = serde_json::to_vec(&self.canonical_value()).map_err(|error| {
            ProcessError::new(ValidationCode::NonCanonicalDefinition, error.to_string())
        })?;
        let digest = Sha256::digest(bytes);
        ProcessDefinitionDigest::new(format!("{digest:x}"))
    }

    /// Recomputes the digest and rejects tampered serialized definitions.
    pub fn verify_digest(&self) -> Result<(), ProcessError> {
        if self.calculate_digest()?.as_str() == self.identity.digest.as_str() {
            Ok(())
        } else {
            Err(ProcessError::new(
                ValidationCode::NonCanonicalDefinition,
                "definition digest does not match canonical content",
            ))
        }
    }

    /// Serializes the definition to deterministic compact JSON.
    pub fn to_json(&self) -> Result<String, ProcessError> {
        self.verify_digest()?;
        serde_json::to_string(self).map_err(|error| {
            ProcessError::new(ValidationCode::NonCanonicalDefinition, error.to_string())
        })
    }

    /// Deserializes, validates and verifies a canonical definition.
    pub fn from_json(value: &str) -> Result<Self, ProcessError> {
        let mut definition: Self = serde_json::from_str(value).map_err(|error| {
            ProcessError::new(ValidationCode::NonCanonicalDefinition, error.to_string())
        })?;
        if definition.ir_version != ProcessIrVersion::V1 {
            return Err(ProcessError::new(
                ValidationCode::UnsupportedIrVersion,
                "unsupported Process IR version",
            ));
        }
        definition.normalize();
        definition.validate_structure()?;
        definition.verify_digest()?;
        Ok(definition)
    }

    #[must_use]
    pub fn ir_version(&self) -> ProcessIrVersion {
        self.ir_version
    }
    #[must_use]
    pub fn identity(&self) -> &DefinitionIdentity {
        &self.identity
    }
    #[must_use]
    pub fn states(&self) -> &[StateDefinition] {
        &self.states
    }
    #[must_use]
    pub fn events(&self) -> &[EventTypeDefinition] {
        &self.events
    }
    #[must_use]
    pub fn transitions(&self) -> &[TransitionDefinition] {
        &self.transitions
    }
    #[must_use]
    pub fn gates(&self) -> &[GateDefinition] {
        &self.gates
    }
    #[must_use]
    pub fn evidence(&self) -> &[EvidenceRequirement] {
        &self.evidence
    }
    #[must_use]
    pub fn invariants(&self) -> &[InvariantDefinition] {
        &self.invariants
    }
    #[must_use]
    pub fn blockers(&self) -> &[BlockerDefinition] {
        &self.blockers
    }
    #[must_use]
    pub fn activities(&self) -> &[ActivityDefinition] {
        &self.activities
    }
    #[must_use]
    pub fn recovery(&self) -> &[RecoveryPolicy] {
        &self.recovery
    }
    #[must_use]
    pub fn extensions(&self) -> &[ExecutionGraphExtension] {
        &self.extensions
    }
    #[must_use]
    pub fn initial_state(&self) -> &StateDefinition {
        self.states
            .iter()
            .find(|state| state.initial)
            .expect("validated definition has an initial state")
    }
}

#[derive(Serialize)]
struct CanonicalDefinition<'a> {
    ir_version: ProcessIrVersion,
    id: &'a ProcessDefinitionId,
    version: ProcessDefinitionVersion,
    states: &'a [StateDefinition],
    events: &'a [EventTypeDefinition],
    transitions: &'a [TransitionDefinition],
    gates: &'a [GateDefinition],
    evidence: &'a [EvidenceRequirement],
    invariants: &'a [InvariantDefinition],
    blockers: &'a [BlockerDefinition],
    activities: &'a [ActivityDefinition],
    recovery: &'a [RecoveryPolicy],
    extensions: &'a [ExecutionGraphExtension],
}

impl fmt::Display for ProcessIrVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EventOccurrenceId, ProcessInstanceId, ProcessInstanceRevision};

    fn definition() -> ProcessDefinition {
        let start = StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap();
        let done = StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap();
        ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("example").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .with_states([start, done])
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
    fn identity_digest_is_stable_and_round_trips() {
        let first = definition();
        let second = definition();
        assert_eq!(first.identity().digest(), second.identity().digest());
        assert_eq!(
            first,
            ProcessDefinition::from_json(&first.to_json().unwrap()).unwrap()
        );
        assert_eq!(first.to_json().unwrap(), second.to_json().unwrap());
    }

    #[test]
    fn construction_rejects_invalid_structure_and_references() {
        let missing = ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("bad").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .build()
        .unwrap_err();
        assert_eq!(missing.code(), ValidationCode::EmptyDefinition);
        let state = StateDefinition::new(StateId::new("same").unwrap(), true, false).unwrap();
        let duplicate_state =
            StateDefinition::new(StateId::new("same").unwrap(), false, false).unwrap();
        let duplicate = ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("bad").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .with_states([state, duplicate_state])
        .with_events([EventTypeDefinition::new(EventTypeId::new("event").unwrap())])
        .with_transitions([TransitionDefinition::new(
            TransitionId::new("t").unwrap(),
            StateId::new("same").unwrap(),
            EventTypeId::new("event").unwrap(),
            StateId::new("same").unwrap(),
            GuardExpression::Always,
        )])
        .build()
        .unwrap_err();
        assert_eq!(duplicate.code(), ValidationCode::DuplicateIdentifier);
        assert!(StateDefinition::new(StateId::new("start").unwrap(), true, true).is_err());
        assert!(ProcessDefinitionVersion::new(0).is_err());
        assert!(ProcessDefinitionDigest::new("bad").is_err());
    }

    #[test]
    fn typed_runtime_primitives_are_distinct_and_monotonic() {
        let occurrence = EventOccurrenceId::new("occurrence-1").unwrap();
        let instance = ProcessInstanceId::new("instance-1").unwrap();
        assert_eq!(occurrence.as_str(), "occurrence-1");
        assert_eq!(instance.as_str(), "instance-1");
        assert_eq!(
            ProcessInstanceRevision::initial().next().unwrap().value(),
            1
        );
    }

    #[test]
    fn canonicalization_orders_collections() {
        let start = StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap();
        let done = StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap();
        let build = |states| {
            ProcessDefinitionBuilder::new(
                ProcessDefinitionId::new("order").unwrap(),
                ProcessDefinitionVersion::new(1).unwrap(),
            )
            .with_states(states)
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
        };
        assert_eq!(
            build([start.clone(), done.clone()]).identity().digest(),
            build([done, start]).identity().digest()
        );
    }
}
