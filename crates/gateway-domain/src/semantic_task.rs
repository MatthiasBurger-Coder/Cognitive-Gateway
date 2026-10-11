//! SemanticTaskIR v1: resolved provider-neutral task meaning, not execution authority.
//!
//! JSON admission validates structure. `validate_references` additionally checks
//! captured reference bindings through an authoritative, caller-supplied resolver.

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use sha2::{Digest, Sha256};

use crate::{
    CapabilityId, Confidence, Constraint, ContentDigest, ContextScopeId, DeclarativeContextId,
    DesiredCondition, DesiredState, EvidenceId, NonEmptyText, ObservationId, ObservedStateId,
    PolicyId, ReferenceId, SchemaVersion, SerializationError, TaskId, TypedValue, ValidationError,
    learning::ProcessReference,
};

pub const SEMANTIC_TASK_IR_VERSION: SchemaVersion = SchemaVersion::V1;

/// A binding to one object in one captured scope/revision. There are no candidate
/// sets or unresolved variants. The referenced contract owns content validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedReference<I> {
    pub id: I,
    pub scope: ContextScopeId,
    pub contract: ReferenceId,
    pub contract_version: SchemaVersion,
    pub revision: ReferenceId,
    pub digest: ContentDigest,
}

/// Finite baseline inventory. Adding a task kind requires a versioned contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SemanticTaskType {
    Analyze,
    AnalyzePerformance,
    Inspect,
    Create,
    Modify,
    Verify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TargetKind {
    Service,
    Artifact,
    Project,
    Entity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticTarget {
    pub kind: TargetKind,
    pub reference: ResolvedReference<ReferenceId>,
}

/// An outcome identity and its human explanation, never a process instruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticGoal {
    pub outcome: ReferenceId,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum SemanticInputValue {
    Literal(TypedValue),
    Reference(ResolvedReference<ReferenceId>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticTaskInput {
    pub role: ReferenceId,
    pub value: SemanticInputValue,
}

/// Unverified premise, retained separately from all mandatory operands. Detailed
/// epistemic lifecycle and attachment contracts remain EPIC-05.05 work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticAssumption {
    pub id: ReferenceId,
    pub premise: TypedValue,
    pub basis: ResolvedReference<ReferenceId>,
    pub confidence: Confidence,
}

/// Pin a provider-neutral output schema; inline schemas and result validation
/// are owned by EPIC-05.06. No provider rendering options are carried here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticOutputContract {
    pub schema: ResolvedReference<ReferenceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SemanticVerificationCheck {
    OutputSchemaValid,
    EvidenceRequired,
    AllClaimsSupported,
    NoUnresolvedReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticVerificationContract {
    pub checks: Vec<SemanticVerificationCheck>,
}

/// Construction/wire input. An arbitrary instance is not a validated IR.
/// Every collection is required on the wire, even when empty. Only state,
/// desired_state and process may be omitted or null.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticTaskData {
    #[serde(deserialize_with = "deserialize_task_version")]
    pub schema_version: SchemaVersion,
    pub id: TaskId,
    pub task_type: SemanticTaskType,
    pub target: SemanticTarget,
    pub goal: SemanticGoal,
    pub inputs: Vec<SemanticTaskInput>,
    pub context_refs: Vec<ResolvedReference<DeclarativeContextId>>,
    pub current_state: Option<ResolvedReference<ObservedStateId>>,
    pub desired_state: Option<DesiredState>,
    pub observations: Vec<ResolvedReference<ObservationId>>,
    pub history: Vec<ResolvedReference<ReferenceId>>,
    // These are declared abstract needs, not CG-07 Delta-derived requirements.
    pub capability_requirements: Vec<ResolvedReference<CapabilityId>>,
    pub constraints: Vec<Constraint>,
    pub policy_refs: Vec<ResolvedReference<PolicyId>>,
    pub evidence_refs: Vec<ResolvedReference<EvidenceId>>,
    pub assumptions: Vec<SemanticAssumption>,
    pub output_contract: SemanticOutputContract,
    pub verification_contract: SemanticVerificationContract,
    pub process: Option<ProcessReference>,
}

/// Immutable validated task shape. Reference existence and target association
/// must still be checked at handoff; shape validation never authorizes execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SemanticTaskData", into = "SemanticTaskData")]
pub struct SemanticTaskIR(SemanticTaskData);

fn deserialize_task_version<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<SchemaVersion, D::Error> {
    let text = String::deserialize(deserializer)?;
    let version: SchemaVersion = text.parse().map_err(D::Error::custom)?;
    if version.to_string() != text {
        return Err(D::Error::custom(
            "task schema version must use canonical MAJOR.MINOR spelling",
        ));
    }
    Ok(version)
}

fn invalid(reason: &'static str) -> ValidationError {
    ValidationError::InvalidStateCombination { reason }
}

fn canonical_set<T, K: Ord>(
    values: &mut [T],
    key: impl Fn(&T) -> K,
) -> Result<(), ValidationError> {
    values.sort_by_key(&key);
    if values.windows(2).any(|pair| key(&pair[0]) == key(&pair[1])) {
        return Err(invalid("duplicate semantic collection identity"));
    }
    Ok(())
}

fn canonical_value(value: &mut TypedValue) -> Result<(), ValidationError> {
    value.validate()?;
    if let TypedValue::Set(values) = value {
        canonical_set(values, |v| v.clone())?;
    }
    Ok(())
}

/// Projection passed to the external reference validator, preserving the exact
/// contract, scope, revision and digest. No resolution is performed by this IR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticReferenceBinding {
    pub id: String,
    pub scope: ContextScopeId,
    pub contract: ReferenceId,
    pub contract_version: SchemaVersion,
    pub revision: ReferenceId,
    pub digest: ContentDigest,
}

/// The trusted resolver validates existence, uniqueness, kind, source contract,
/// digest and revision against the captured basis. It also checks association
/// with the task target (including desired-state predicates). It must not fetch
/// a replacement for a stale or missing binding or confer policy authority.
pub trait SemanticReferenceValidator {
    fn validate(
        &self,
        task: &SemanticTaskData,
        binding: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError>;
}

impl<I: AsRef<str>> ResolvedReference<I> {
    fn binding(&self) -> SemanticReferenceBinding {
        SemanticReferenceBinding {
            id: self.id.as_ref().to_owned(),
            scope: self.scope.clone(),
            contract: self.contract.clone(),
            contract_version: self.contract_version,
            revision: self.revision.clone(),
            digest: self.digest.clone(),
        }
    }
}

impl SemanticTaskData {
    fn bindings(&self) -> Vec<SemanticReferenceBinding> {
        let mut bindings = vec![
            self.target.reference.binding(),
            self.output_contract.schema.binding(),
        ];
        macro_rules! collect {
            ($($field:ident),+ $(,)?) => {$(
                bindings.extend(self.$field.iter().map(ResolvedReference::binding));
            )+};
        }
        collect!(
            context_refs,
            current_state,
            observations,
            history,
            capability_requirements,
            policy_refs,
            evidence_refs
        );
        if let Some(process) = &self.process {
            bindings.push(SemanticReferenceBinding {
                id: process.id().as_str().to_owned(),
                scope: self.target.reference.scope.clone(),
                contract: ReferenceId::new("cg.process-definition").expect("static identifier"),
                contract_version: SchemaVersion::V1,
                revision: ReferenceId::new(process.version().to_string())
                    .expect("numeric identifier"),
                digest: process.digest().clone(),
            });
        }
        for input in &self.inputs {
            if let SemanticInputValue::Reference(reference) = &input.value {
                bindings.push(reference.binding());
            }
        }
        bindings.extend(self.assumptions.iter().map(|value| value.basis.binding()));
        bindings
    }
}

impl TryFrom<SemanticTaskData> for SemanticTaskIR {
    type Error = ValidationError;

    fn try_from(mut data: SemanticTaskData) -> Result<Self, Self::Error> {
        if data.schema_version != SEMANTIC_TASK_IR_VERSION {
            return Err(ValidationError::UnsupportedSchemaVersion {
                expected: "1.0",
                actual: data.schema_version.to_string(),
            });
        }
        NonEmptyText::new(data.goal.description.clone())?;
        for binding in data.bindings() {
            if binding.scope != data.target.reference.scope {
                return Err(invalid("semantic references must share the target scope"));
            }
            let digest = binding.digest.as_str();
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(invalid(
                    "semantic references require a lowercase SHA-256 digest",
                ));
            }
        }
        for input in &mut data.inputs {
            if let SemanticInputValue::Literal(value) = &mut input.value {
                canonical_value(value)?;
            }
        }
        for assumption in &mut data.assumptions {
            canonical_value(&mut assumption.premise)?;
        }
        canonical_set(&mut data.inputs, |v| v.role.clone())?;
        canonical_set(&mut data.constraints, |v| v.id().clone())?;
        canonical_set(&mut data.assumptions, |v| v.id.clone())?;
        macro_rules! sort_refs {
            ($($field:ident),+ $(,)?) => {$(
                canonical_set(&mut data.$field, |v| v.id.clone())?;
            )+};
        }
        sort_refs!(
            context_refs,
            observations,
            history,
            capability_requirements,
            policy_refs,
            evidence_refs
        );
        canonical_set(&mut data.verification_contract.checks, |v| *v)?;
        let checks = &data.verification_contract.checks;
        if !checks.contains(&SemanticVerificationCheck::OutputSchemaValid)
            || !checks.contains(&SemanticVerificationCheck::NoUnresolvedReference)
        {
            return Err(invalid("verification requires schema and reference checks"));
        }
        if checks.contains(&SemanticVerificationCheck::AllClaimsSupported)
            && !checks.contains(&SemanticVerificationCheck::EvidenceRequired)
        {
            return Err(invalid("claim support requires evidence verification"));
        }
        if let Some(desired) = &data.desired_state {
            let mut conditions = Vec::new();
            for condition in desired.conditions() {
                let mut expected = condition.expected().cloned();
                if let Some(value) = &mut expected {
                    canonical_value(value)?;
                }
                conditions.push(DesiredCondition::new(
                    condition.id().clone(),
                    condition.subject().clone(),
                    condition.operator(),
                    expected,
                )?);
            }
            data.desired_state = Some(DesiredState::new_with_version(
                desired.version(),
                desired.id().clone(),
                conditions,
                desired.expression().clone(),
                desired.constraints().to_vec(),
                desired.acceptance_criteria().to_vec(),
            )?);
        }
        if let Some(process) = &data.process {
            ProcessReference::new(
                process.id().clone(),
                process.version(),
                process.digest().clone(),
            )?;
        }
        Ok(Self(data))
    }
}

impl From<SemanticTaskIR> for SemanticTaskData {
    fn from(value: SemanticTaskIR) -> Self {
        value.0
    }
}

impl SemanticTaskIR {
    pub fn new(data: SemanticTaskData) -> Result<Self, ValidationError> {
        data.try_into()
    }

    pub fn data(&self) -> &SemanticTaskData {
        &self.0
    }

    pub fn from_json(json: &str) -> Result<Self, SerializationError> {
        // Direct serde parsing retains duplicate-key rejection in strict structs.
        let data: SemanticTaskData = serde_json::from_str(json)?;
        Ok(Self::new(data)?)
    }

    /// Compact UTF-8 JSON, object keys lexically ordered recursively. Arrays
    /// retain the canonical domain order; no whitespace or trailing newline.
    pub fn to_canonical_json(&self) -> Result<String, SerializationError> {
        Ok(serde_json::to_string(&serde_json::to_value(&self.0)?)?)
    }

    /// TaskId is caller identity; this digest is exact semantic revision identity.
    pub fn content_digest(&self) -> Result<ContentDigest, SerializationError> {
        let bytes = self.to_canonical_json()?;
        Ok(ContentDigest::new(format!(
            "{:x}",
            Sha256::digest(bytes.as_bytes())
        ))?)
    }

    pub fn validate_references(
        &self,
        validator: &impl SemanticReferenceValidator,
    ) -> Result<(), ValidationError> {
        for binding in self.0.bindings() {
            validator.validate(&self.0, &binding)?;
        }
        Ok(())
    }
}
