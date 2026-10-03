//! Model-independent learning contracts. None of these values grants execution authority.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::memory::MemoryEligibilityReference;
use crate::{
    CapabilityId, ContentDigest, ContextScopeId, EvidenceId, FactId, ObservationId, OperatingMode,
    PolicyId, ProvenanceId, ReferenceId, SerializationError, ValidationError,
};

pub const LEARNING_SCHEMA_VERSION: u16 = 1;

fn invalid(reason: &'static str) -> ValidationError {
    ValidationError::InvalidDeclarativeValue { reason }
}

fn nonempty_unique<T: Ord>(values: &[T], field: &'static str) -> Result<(), ValidationError> {
    if values.is_empty() {
        return Err(ValidationError::EmptyRelationship { field });
    }
    if values.iter().collect::<BTreeSet<_>>().len() != values.len() {
        return Err(ValidationError::DuplicateRelationship { field });
    }
    Ok(())
}

fn version(value: u16) -> Result<(), ValidationError> {
    if value == LEARNING_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(ValidationError::UnsupportedSchemaVersion {
            expected: "1",
            actual: value.to_string(),
        })
    }
}

/// A structural selector; lexical or embedding similarity alone cannot satisfy it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FingerprintSignal {
    OperatingMode(OperatingMode),
    Capability(CapabilityId),
    Fact(FactId),
}

/// The typed situation shape to which a candidate or procedure may apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SituationFingerprint {
    scope: ContextScopeId,
    signals: Vec<FingerprintSignal>,
}

impl SituationFingerprint {
    pub fn new(
        scope: ContextScopeId,
        mut signals: Vec<FingerprintSignal>,
    ) -> Result<Self, ValidationError> {
        nonempty_unique(&signals, "fingerprint.signals")?;
        signals.sort();
        let fingerprint = Self { scope, signals };
        fingerprint.validate()?;
        Ok(fingerprint)
    }

    pub fn scope(&self) -> &ContextScopeId {
        &self.scope
    }
    pub fn signals(&self) -> &[FingerprintSignal] {
        &self.signals
    }

    fn validate(&self) -> Result<(), ValidationError> {
        nonempty_unique(&self.signals, "fingerprint.signals")?;
        if self
            .signals
            .iter()
            .filter(|signal| matches!(signal, FingerprintSignal::OperatingMode(_)))
            .count()
            > 1
        {
            return Err(invalid(
                "fingerprint cannot declare conflicting operating modes",
            ));
        }
        if !self
            .signals
            .iter()
            .any(|signal| !matches!(signal, FingerprintSignal::OperatingMode(_)))
        {
            return Err(invalid("fingerprint requires a fact or capability signal"));
        }
        if !self.signals.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(invalid("fingerprint signals must be in canonical order"));
        }
        Ok(())
    }
}

/// A reference to a registry process definition; resolution is external to this contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessReference {
    id: ReferenceId,
    version: u32,
    digest: ContentDigest,
}

impl ProcessReference {
    pub fn new(
        id: ReferenceId,
        version: u32,
        digest: ContentDigest,
    ) -> Result<Self, ValidationError> {
        if version == 0 {
            return Err(invalid("process version must be positive"));
        }
        if id.as_str().starts_with('.') || id.as_str().ends_with('.') || id.as_str().contains("..")
        {
            return Err(invalid("invalid process definition identifier"));
        }
        if digest
            .as_str()
            .bytes()
            .any(|byte| byte.is_ascii_uppercase())
        {
            return Err(invalid("process digest must use lowercase hexadecimal"));
        }
        Ok(Self {
            id,
            version,
            digest,
        })
    }
    pub fn id(&self) -> &ReferenceId {
        &self.id
    }
    pub fn version(&self) -> u32 {
        self.version
    }
    pub fn digest(&self) -> &ContentDigest {
        &self.digest
    }
}

/// A validated historical input. It is evidence for learning, never permission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceBasis {
    memory: MemoryEligibilityReference,
    provenance: ProvenanceId,
    evaluation: ReferenceId,
}

impl ExperienceBasis {
    pub fn new(
        memory: MemoryEligibilityReference,
        provenance: ProvenanceId,
        evaluation: ReferenceId,
    ) -> Result<Self, ValidationError> {
        if memory.schema_version != crate::memory::MEMORY_SCHEMA_VERSION
            || memory.revision == 0
            || memory.eligibility_version == 0
        {
            return Err(invalid("invalid memory eligibility reference"));
        }
        Ok(Self {
            memory,
            provenance,
            evaluation,
        })
    }
    pub fn memory(&self) -> &MemoryEligibilityReference {
        &self.memory
    }
    pub fn provenance(&self) -> &ProvenanceId {
        &self.provenance
    }
    pub fn evaluation(&self) -> &ReferenceId {
        &self.evaluation
    }
}

/// Inspectable hypothesis derived from eligible experience; it has no steps or authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternCandidate {
    schema_version: u16,
    id: ReferenceId,
    fingerprint: SituationFingerprint,
    experience: Vec<ExperienceBasis>,
}

impl PatternCandidate {
    pub fn new(
        id: ReferenceId,
        fingerprint: SituationFingerprint,
        mut experience: Vec<ExperienceBasis>,
    ) -> Result<Self, ValidationError> {
        fingerprint.validate()?;
        nonempty_unique(
            &experience.iter().map(|x| &x.memory.id).collect::<Vec<_>>(),
            "pattern.experience",
        )?;
        for basis in &experience {
            ExperienceBasis::new(
                basis.memory.clone(),
                basis.provenance.clone(),
                basis.evaluation.clone(),
            )?;
        }
        if experience
            .iter()
            .any(|x| x.memory.scope != fingerprint.scope)
        {
            return Err(invalid("experience scope must match fingerprint scope"));
        }
        experience.sort_by(|a, b| a.memory.id.cmp(&b.memory.id));
        Ok(Self {
            schema_version: LEARNING_SCHEMA_VERSION,
            id,
            fingerprint,
            experience,
        })
    }
    pub fn id(&self) -> &ReferenceId {
        &self.id
    }
    pub fn fingerprint(&self) -> &SituationFingerprint {
        &self.fingerprint
    }
    pub fn experience(&self) -> &[ExperienceBasis] {
        &self.experience
    }
    pub fn to_json(&self) -> Result<String, SerializationError> {
        Ok(serde_json::to_string(self)?)
    }
    pub fn from_json(json: &str) -> Result<Self, SerializationError> {
        let parsed: Self = serde_json::from_str(json)?;
        version(parsed.schema_version)?;
        let canonical = Self::new(
            parsed.id.clone(),
            parsed.fingerprint.clone(),
            parsed.experience.clone(),
        )?;
        if parsed != canonical {
            return Err(invalid("noncanonical pattern candidate").into());
        }
        Ok(parsed)
    }
}

/// A procedure refers to existing process, capability and policy identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcedureStep {
    process: ProcessReference,
    capability: CapabilityId,
    policy: PolicyId,
}

impl ProcedureStep {
    pub fn new(process: ProcessReference, capability: CapabilityId, policy: PolicyId) -> Self {
        Self {
            process,
            capability,
            policy,
        }
    }
    pub fn process(&self) -> &ProcessReference {
        &self.process
    }
    pub fn capability(&self) -> &CapabilityId {
        &self.capability
    }
    pub fn policy(&self) -> &PolicyId {
        &self.policy
    }
}

/// Explicit action when applicability or verification fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FallbackBehavior {
    Stop,
    ReturnToPlanner,
}

/// Immutable, digestible content for one procedure version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcedureContent {
    schema_version: u16,
    id: ReferenceId,
    version: u32,
    source_candidate: ReferenceId,
    fingerprint: SituationFingerprint,
    experience: Vec<ExperienceBasis>,
    steps: Vec<ProcedureStep>,
    required_observations: Vec<ObservationId>,
    required_evidence: Vec<EvidenceId>,
    verification_evidence: Vec<EvidenceId>,
    fallback: FallbackBehavior,
}

impl ProcedureContent {
    fn validate(&self) -> Result<(), ValidationError> {
        version(self.schema_version)?;
        if self.version == 0 {
            return Err(invalid("procedure version must be positive"));
        }
        self.fingerprint.validate()?;
        nonempty_unique(
            &self
                .experience
                .iter()
                .map(|x| &x.memory.id)
                .collect::<Vec<_>>(),
            "procedure.experience",
        )?;
        for basis in &self.experience {
            ExperienceBasis::new(
                basis.memory.clone(),
                basis.provenance.clone(),
                basis.evaluation.clone(),
            )?;
        }
        if self
            .experience
            .iter()
            .any(|x| x.memory.scope != self.fingerprint.scope)
        {
            return Err(invalid("experience scope must match procedure scope"));
        }
        if !self
            .experience
            .windows(2)
            .all(|pair| pair[0].memory.id < pair[1].memory.id)
        {
            return Err(invalid("experience references must be in canonical order"));
        }
        if self.steps.is_empty() {
            return Err(ValidationError::EmptyRelationship {
                field: "procedure.steps",
            });
        }
        for step in &self.steps {
            ProcessReference::new(
                step.process.id.clone(),
                step.process.version,
                step.process.digest.clone(),
            )?;
        }
        nonempty_unique(&self.required_observations, "required_observations")?;
        nonempty_unique(&self.required_evidence, "required_evidence")?;
        nonempty_unique(&self.verification_evidence, "verification_evidence")?;
        if !self.required_observations.windows(2).all(|p| p[0] < p[1])
            || !self.required_evidence.windows(2).all(|p| p[0] < p[1])
            || !self.verification_evidence.windows(2).all(|p| p[0] < p[1])
        {
            return Err(invalid("requirement references must be in canonical order"));
        }
        Ok(())
    }
}

/// The digest covers the canonical JSON of every content field, including identity and version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearnedProcedure {
    #[serde(flatten)]
    content: ProcedureContent,
    digest: ContentDigest,
}

impl LearnedProcedure {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ReferenceId,
        version: u32,
        candidate: &PatternCandidate,
        steps: Vec<ProcedureStep>,
        mut required_observations: Vec<ObservationId>,
        mut required_evidence: Vec<EvidenceId>,
        mut verification_evidence: Vec<EvidenceId>,
        fallback: FallbackBehavior,
    ) -> Result<Self, ValidationError> {
        required_observations.sort();
        required_evidence.sort();
        verification_evidence.sort();
        let content = ProcedureContent {
            schema_version: LEARNING_SCHEMA_VERSION,
            id,
            version,
            source_candidate: candidate.id.clone(),
            fingerprint: candidate.fingerprint.clone(),
            experience: candidate.experience.clone(),
            steps,
            required_observations,
            required_evidence,
            verification_evidence,
            fallback,
        };
        content.validate()?;
        let digest = digest_content(&content);
        Ok(Self { content, digest })
    }
    pub fn id(&self) -> &ReferenceId {
        &self.content.id
    }
    pub fn version(&self) -> u32 {
        self.content.version
    }
    pub fn digest(&self) -> &ContentDigest {
        &self.digest
    }
    pub fn fingerprint(&self) -> &SituationFingerprint {
        &self.content.fingerprint
    }
    pub fn source_candidate(&self) -> &ReferenceId {
        &self.content.source_candidate
    }
    pub fn experience(&self) -> &[ExperienceBasis] {
        &self.content.experience
    }
    pub fn steps(&self) -> &[ProcedureStep] {
        &self.content.steps
    }
    pub fn required_observations(&self) -> &[ObservationId] {
        &self.content.required_observations
    }
    pub fn required_evidence(&self) -> &[EvidenceId] {
        &self.content.required_evidence
    }
    pub fn verification_evidence(&self) -> &[EvidenceId] {
        &self.content.verification_evidence
    }
    pub fn fallback(&self) -> FallbackBehavior {
        self.content.fallback
    }
    pub fn to_json(&self) -> Result<String, SerializationError> {
        Ok(serde_json::to_string(self)?)
    }
    pub fn from_json(json: &str) -> Result<Self, SerializationError> {
        let parsed: Self = serde_json::from_str(json)?;
        parsed.content.validate()?;
        if parsed.digest != digest_content(&parsed.content) {
            return Err(invalid("procedure digest mismatch").into());
        }
        Ok(parsed)
    }
}

fn digest_content(content: &ProcedureContent) -> ContentDigest {
    let bytes = serde_json::to_vec(content).expect("validated procedure content serializes");
    ContentDigest::new(format!("{:x}", Sha256::digest(bytes))).expect("SHA-256 is a valid digest")
}

/// Lifecycle is separate from immutable procedure content and never grants policy authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcedureState {
    Draft,
    Evaluated,
    Approved,
    Active,
    Suspended,
    Retired,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcedureTransition {
    pub procedure_id: ReferenceId,
    pub procedure_version: u32,
    pub from: ProcedureState,
    pub to: ProcedureState,
    pub decision: ReferenceId,
    pub actor: ProvenanceId,
    pub at: i64,
}

impl ProcedureTransition {
    pub fn new(
        procedure: &LearnedProcedure,
        from: ProcedureState,
        to: ProcedureState,
        decision: ReferenceId,
        actor: ProvenanceId,
        at: i64,
    ) -> Result<Self, ValidationError> {
        use ProcedureState::*;
        if !matches!(
            (from, to),
            (Draft, Evaluated | Rejected)
                | (Evaluated, Approved | Rejected)
                | (Approved, Active | Retired)
                | (Active, Suspended | Retired)
                | (Suspended, Active | Retired)
        ) {
            return Err(ValidationError::InvalidStateTransition {
                state: "procedure",
                from: format!("{from:?}"),
                to: format!("{to:?}"),
            });
        }
        Ok(Self {
            procedure_id: procedure.id().clone(),
            procedure_version: procedure.version(),
            from,
            to,
            decision,
            actor,
            at,
        })
    }
}

/// Append-only projection of decisions for one immutable procedure version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcedureLifecycle {
    procedure_digest: ContentDigest,
    procedure_id: ReferenceId,
    procedure_version: u32,
    state: ProcedureState,
    history: Vec<ProcedureTransition>,
}

impl ProcedureLifecycle {
    pub fn new(procedure: &LearnedProcedure) -> Self {
        Self {
            procedure_digest: procedure.digest().clone(),
            procedure_id: procedure.id().clone(),
            procedure_version: procedure.version(),
            state: ProcedureState::Draft,
            history: Vec::new(),
        }
    }
    pub fn state(&self) -> ProcedureState {
        self.state
    }
    pub fn history(&self) -> &[ProcedureTransition] {
        &self.history
    }
    /// Advancing a draft requires a reproducible, passing evaluation for its exact content.
    pub fn apply_evaluated(
        &mut self,
        event: ProcedureTransition,
        bundle: &crate::procedure_evaluation::EvaluationBundle,
    ) -> Result<(), ValidationError> {
        bundle.proves(&bundle.procedure)?;
        if event.from != ProcedureState::Draft
            || event.to != ProcedureState::Evaluated
            || event.decision.as_str() != bundle.digest.as_str()
            || self.procedure_digest != *bundle.procedure.digest()
        {
            return Err(invalid(
                "evaluation transition must reference exact evidence bundle",
            ));
        }
        self.apply_checked(event)
    }
    pub fn apply(&mut self, event: ProcedureTransition) -> Result<(), ValidationError> {
        if event.to == ProcedureState::Evaluated {
            return Err(invalid("explicit successful evaluation evidence required"));
        }
        self.apply_checked(event)
    }
    fn apply_checked(&mut self, event: ProcedureTransition) -> Result<(), ValidationError> {
        if event.procedure_id != self.procedure_id
            || event.procedure_version != self.procedure_version
            || event.from != self.state
        {
            return Err(invalid(
                "procedure transition does not match current projection",
            ));
        }
        if self
            .history
            .iter()
            .any(|previous| previous.decision == event.decision)
        {
            return Err(ValidationError::DuplicateRelationship {
                field: "procedure.decision",
            });
        }
        if self
            .history
            .last()
            .is_some_and(|previous| event.at < previous.at)
        {
            return Err(invalid("procedure decision time must be monotonic"));
        }
        use ProcedureState::*;
        if !matches!(
            (event.from, event.to),
            (Draft, Evaluated | Rejected)
                | (Evaluated, Approved | Rejected)
                | (Approved, Active | Retired)
                | (Active, Suspended | Retired)
                | (Suspended, Active | Retired)
        ) {
            return Err(ValidationError::InvalidStateTransition {
                state: "procedure",
                from: format!("{:?}", event.from),
                to: format!("{:?}", event.to),
            });
        }
        self.state = event.to;
        self.history.push(event);
        Ok(())
    }
}
