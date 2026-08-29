//! JSON wire contract for the complete CG-06 declarative situation model.
//!
//! Wire structs are deliberately separate from domain structs. Every
//! deserialization path reconstructs domain values through their validation
//! constructors, so JSON syntax alone never implies semantic acceptance.

use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{
    AssertionPolarity, Assessment, AssessmentConclusion, AssessmentKind, AssessmentOrigin,
    AssessmentRuleContract, AssessmentRuleVersion, AssessmentStatus, BasisReferences,
    ComparisonOperator, ConditionExpression, Confidence, ConflictStatus, ContentDigest,
    DeclarativeConstraint, DeclarativeContext, DeclarativeContextVersion, DesiredCondition,
    DesiredState, Evidence, EvidenceContent, EvidenceKind, EvidenceLink, EvidenceRelation, Fact,
    FreshnessPolicy, FreshnessStatus, Intent, NormalizationDiagnostic, NormalizationReasonCode,
    NormalizedClaim, NormalizedStateEntry, Observation, ObservationEvidenceSet, OriginalInput,
    Provenance, QualitativeLikelihood, QualityMetadata, ReasonCode, ReferenceId, Risk,
    RiskCategory, RiskLikelihood, RiskOrigin, RiskSeverity, RiskStatus, SensitivityClass,
    Situation, SituationDiagnostic, SituationDiagnosticCode, SituationReference, SourceId,
    SourceKind, SourceTimestamp, StateLineage, StateStatus, SubjectPath, TrustClass, TypedValue,
    Uncertainty, UnixTimestamp, ValidityInterval,
    declarative_context::ObservedState,
    identifiers::{
        AcceptanceCriterionId, AssessmentId, AssessmentRuleId, ConditionId, ContextCacheEntryId,
        ContextScopeId, DeclarativeContextId, DesiredStateId, EvidenceId, FactId, IntentId,
        ObservationId, ObservedStateId, ProvenanceId, RiskId, SituationId,
    },
    serialization::SerializationError,
    validation::ValidationError,
};

macro_rules! string_id_serde {
    ($($type:ty),+ $(,)?) => {
        $(
            impl Serialize for $type {
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where S: Serializer {
                    serializer.serialize_str(self.as_str())
                }
            }

            impl<'de> Deserialize<'de> for $type {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where D: Deserializer<'de> {
                    <$type>::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
                }
            }
        )+
    };
}

macro_rules! wire_serde {
    ($type:ty, $wire:ty) => {
        impl Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                <$wire>::from_domain(self).serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                <$wire>::deserialize(deserializer)?
                    .into_domain()
                    .map_err(D::Error::custom)
            }
        }
    };
}

string_id_serde!(
    AcceptanceCriterionId,
    AssessmentId,
    AssessmentRuleId,
    ConditionId,
    ContextCacheEntryId,
    ContextScopeId,
    DeclarativeContextId,
    DesiredStateId,
    EvidenceId,
    FactId,
    IntentId,
    ObservationId,
    ObservedStateId,
    ProvenanceId,
    ReferenceId,
    RiskId,
    SituationId,
    SourceId,
);

impl Serialize for SourceTimestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SourceTimestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SourceTimestamp::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl Serialize for ContentDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ContentDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ContentDigest::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl Serialize for ReasonCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ReasonCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ReasonCode::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl Serialize for AssessmentRuleVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for AssessmentRuleVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        AssessmentRuleVersion::from_str(&String::deserialize(deserializer)?)
            .map_err(D::Error::custom)
    }
}

impl Serialize for DeclarativeContextVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DeclarativeContextVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        DeclarativeContextVersion::from_str(&String::deserialize(deserializer)?)
            .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireDecimal {
    unscaled: String,
    scale: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum WireTypedValue {
    #[serde(rename = "BOOLEAN")]
    Boolean(bool),
    #[serde(rename = "INTEGER")]
    Integer(i64),
    #[serde(rename = "DECIMAL")]
    Decimal(WireDecimal),
    #[serde(rename = "STRING")]
    String(String),
    #[serde(rename = "SYMBOL")]
    Symbol(String),
    #[serde(rename = "SET")]
    Set(Vec<WireTypedValue>),
}

impl WireTypedValue {
    fn from_domain(value: &TypedValue) -> Self {
        match value {
            TypedValue::Boolean(value) => Self::Boolean(*value),
            TypedValue::Integer(value) => Self::Integer(*value),
            TypedValue::Decimal(value) => Self::Decimal(WireDecimal {
                unscaled: value.unscaled().to_string(),
                scale: value.scale(),
            }),
            TypedValue::String(value) => Self::String(value.as_str().to_owned()),
            TypedValue::Symbol(value) => Self::Symbol(value.as_str().to_owned()),
            TypedValue::Set(values) => Self::Set(values.iter().map(Self::from_domain).collect()),
        }
    }

    fn into_domain(self) -> Result<TypedValue, ValidationError> {
        match self {
            Self::Boolean(value) => Ok(TypedValue::Boolean(value)),
            Self::Integer(value) => Ok(TypedValue::Integer(value)),
            Self::Decimal(WireDecimal { unscaled, scale }) => {
                Ok(TypedValue::Decimal(crate::DecimalValue::new(
                    unscaled
                        .parse()
                        .map_err(|_| ValidationError::InvalidDeclarativeValue {
                            reason: "decimal unscaled value must be an i128",
                        })?,
                    scale,
                )?))
            }
            Self::String(value) => TypedValue::string(value),
            Self::Symbol(value) => TypedValue::symbol(value),
            Self::Set(values) => TypedValue::set(
                values
                    .into_iter()
                    .map(Self::into_domain)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        }
    }
}

wire_serde!(TypedValue, WireTypedValue);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum WireConditionExpression {
    #[serde(rename = "CONDITION")]
    Condition(ConditionId),
    #[serde(rename = "ALL")]
    All(Vec<WireConditionExpression>),
    #[serde(rename = "ANY")]
    Any(Vec<WireConditionExpression>),
    #[serde(rename = "NOT")]
    Not(Box<WireConditionExpression>),
}

impl WireConditionExpression {
    fn from_domain(value: &ConditionExpression) -> Self {
        match value {
            ConditionExpression::Condition(id) => Self::Condition(id.clone()),
            ConditionExpression::All(values) => {
                Self::All(values.iter().map(Self::from_domain).collect())
            }
            ConditionExpression::Any(values) => {
                Self::Any(values.iter().map(Self::from_domain).collect())
            }
            ConditionExpression::Not(value) => Self::Not(Box::new(Self::from_domain(value))),
        }
    }

    fn into_domain(self) -> Result<ConditionExpression, ValidationError> {
        match self {
            Self::Condition(id) => Ok(ConditionExpression::condition(id)),
            Self::All(values) => ConditionExpression::all(
                values
                    .into_iter()
                    .map(Self::into_domain)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Self::Any(values) => ConditionExpression::any(
                values
                    .into_iter()
                    .map(Self::into_domain)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Self::Not(value) => Ok(ConditionExpression::negate(value.into_domain()?)),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireDesiredCondition {
    id: ConditionId,
    subject: String,
    operator: String,
    expected: Option<WireTypedValue>,
}

impl WireDesiredCondition {
    fn from_domain(value: &DesiredCondition) -> Self {
        Self {
            id: value.id().clone(),
            subject: value.subject().to_string(),
            operator: value.operator().to_string(),
            expected: value.expected().map(WireTypedValue::from_domain),
        }
    }

    fn into_domain(self) -> Result<DesiredCondition, ValidationError> {
        DesiredCondition::new(
            self.id,
            SubjectPath::from_str(&self.subject)?,
            ComparisonOperator::from_str(&self.operator)?,
            self.expected.map(WireTypedValue::into_domain).transpose()?,
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireDeclarativeConstraint {
    id: crate::ConstraintId,
    expression: WireConditionExpression,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireAcceptanceCriterion {
    id: AcceptanceCriterionId,
    description: String,
    expression: WireConditionExpression,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum WireOriginalInput {
    #[serde(rename = "INLINE")]
    Inline(String),
    #[serde(rename = "REFERENCE")]
    Reference(ReferenceId),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireDesiredState {
    schema_version: DeclarativeContextVersion,
    id: DesiredStateId,
    conditions: Vec<WireDesiredCondition>,
    expression: WireConditionExpression,
    constraints: Vec<WireDeclarativeConstraint>,
    acceptance_criteria: Vec<WireAcceptanceCriterion>,
}

impl WireDesiredState {
    fn from_domain(value: &DesiredState) -> Self {
        Self {
            schema_version: value.version(),
            id: value.id().clone(),
            conditions: value
                .conditions()
                .iter()
                .map(WireDesiredCondition::from_domain)
                .collect(),
            expression: WireConditionExpression::from_domain(value.expression()),
            constraints: value
                .constraints()
                .iter()
                .map(|constraint| WireDeclarativeConstraint {
                    id: constraint.id().clone(),
                    expression: WireConditionExpression::from_domain(constraint.expression()),
                })
                .collect(),
            acceptance_criteria: value
                .acceptance_criteria()
                .iter()
                .map(|criterion| WireAcceptanceCriterion {
                    id: criterion.id().clone(),
                    description: criterion.description().to_owned(),
                    expression: WireConditionExpression::from_domain(criterion.expression()),
                })
                .collect(),
        }
    }

    fn into_domain(self) -> Result<DesiredState, ValidationError> {
        let conditions = self
            .conditions
            .into_iter()
            .map(WireDesiredCondition::into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        let constraints = self
            .constraints
            .into_iter()
            .map(|constraint| {
                Ok(DeclarativeConstraint::new(
                    constraint.id,
                    constraint.expression.into_domain()?,
                ))
            })
            .collect::<Result<Vec<_>, ValidationError>>()?;
        let criteria = self
            .acceptance_criteria
            .into_iter()
            .map(|criterion| {
                crate::AcceptanceCriterion::new(
                    criterion.id,
                    criterion.description,
                    criterion.expression.into_domain()?,
                )
            })
            .collect::<Result<Vec<_>, ValidationError>>()?;
        DesiredState::new_with_version(
            self.schema_version,
            self.id,
            conditions,
            self.expression.into_domain()?,
            constraints,
            criteria,
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireIntent {
    schema_version: DeclarativeContextVersion,
    id: IntentId,
    desired_state: WireDesiredState,
    original_input: Option<WireOriginalInput>,
}

impl WireIntent {
    fn from_domain(value: &Intent) -> Self {
        Self {
            schema_version: value.version(),
            id: value.id().clone(),
            desired_state: WireDesiredState::from_domain(value.desired_state()),
            original_input: value.original_input().map(|input| match input {
                OriginalInput::Inline(value) => {
                    WireOriginalInput::Inline(value.as_str().to_owned())
                }
                OriginalInput::Reference(id) => WireOriginalInput::Reference(id.clone()),
            }),
        }
    }

    fn into_domain(self) -> Result<Intent, ValidationError> {
        let intent = Intent::new(self.id, self.desired_state.into_domain()?);
        if self.schema_version != crate::declarative_context::DECLARATIVE_CONTEXT_IR_VERSION {
            return Err(ValidationError::UnsupportedSchemaVersion {
                expected: "1.0",
                actual: self.schema_version.to_string(),
            });
        }
        Ok(match self.original_input {
            None => intent,
            Some(WireOriginalInput::Inline(value)) => {
                intent.with_original_input(OriginalInput::inline(value)?)
            }
            Some(WireOriginalInput::Reference(id)) => {
                intent.with_original_input(OriginalInput::reference(id))
            }
        })
    }
}

fn parse_trust(value: &str) -> Result<TrustClass, ValidationError> {
    match value {
        "CANONICAL_REFERENCE" => Ok(TrustClass::CanonicalReference),
        "OBSERVED_EVIDENCE" => Ok(TrustClass::ObservedEvidence),
        "RETRIEVED_CONTENT" => Ok(TrustClass::RetrievedContent),
        "CALLER_INPUT" => Ok(TrustClass::CallerInput),
        "DERIVED_ASSESSMENT" => Ok(TrustClass::DerivedAssessment),
        "SYNTHETIC_DATA" => Ok(TrustClass::SyntheticData),
        "MIXED" => Ok(TrustClass::Mixed),
        value => Err(ValidationError::UnknownDomainValue {
            field: "trust_class",
            value: value.to_owned(),
        }),
    }
}

fn parse_sensitivity(value: &str) -> Result<SensitivityClass, ValidationError> {
    match value {
        "PUBLIC" => Ok(SensitivityClass::Public),
        "NORMAL" => Ok(SensitivityClass::Normal),
        "INTERNAL" => Ok(SensitivityClass::Internal),
        "CONFIDENTIAL" => Ok(SensitivityClass::Confidential),
        "SECRET" => Ok(SensitivityClass::Secret),
        value => Err(ValidationError::UnknownDomainValue {
            field: "sensitivity_class",
            value: value.to_owned(),
        }),
    }
}

fn parse_freshness(value: &str) -> Result<FreshnessStatus, ValidationError> {
    match value {
        "FRESH" => Ok(FreshnessStatus::Fresh),
        "STALE" => Ok(FreshnessStatus::Stale),
        "UNKNOWN" => Ok(FreshnessStatus::Unknown),
        value => Err(ValidationError::UnknownDomainValue {
            field: "freshness_status",
            value: value.to_owned(),
        }),
    }
}

fn parse_uncertainty(value: &str) -> Result<Uncertainty, ValidationError> {
    match value {
        "NONE" => Ok(Uncertainty::None),
        "INCOMPLETE" => Ok(Uncertainty::Incomplete),
        "PROBABILISTIC" => Ok(Uncertainty::Probabilistic),
        "UNKNOWN" => Ok(Uncertainty::Unknown),
        value => Err(ValidationError::UnknownDomainValue {
            field: "uncertainty",
            value: value.to_owned(),
        }),
    }
}

fn parse_conflict(value: &str) -> Result<ConflictStatus, ValidationError> {
    match value {
        "NONE" => Ok(ConflictStatus::None),
        "UNRESOLVED" => Ok(ConflictStatus::Unresolved),
        value => Err(ValidationError::UnknownDomainValue {
            field: "conflict_status",
            value: value.to_owned(),
        }),
    }
}

macro_rules! string_enum_serde {
    ($type:ty, $parser:path) => {
        impl Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                $parser(&String::deserialize(deserializer)?).map_err(D::Error::custom)
            }
        }
    };
}

string_enum_serde!(TrustClass, parse_trust);
string_enum_serde!(SensitivityClass, parse_sensitivity);
string_enum_serde!(FreshnessStatus, parse_freshness);
string_enum_serde!(Uncertainty, parse_uncertainty);
string_enum_serde!(ConflictStatus, parse_conflict);

macro_rules! fromstr_enum_serde {
    ($type:ty) => {
        impl Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                <$type>::from_str(&String::deserialize(deserializer)?).map_err(D::Error::custom)
            }
        }
    };
}

fromstr_enum_serde!(AssertionPolarity);
fromstr_enum_serde!(EvidenceKind);
fromstr_enum_serde!(SourceKind);

fn parse_evidence_relation(value: &str) -> Result<EvidenceRelation, ValidationError> {
    match value {
        "SUPPORTS" => Ok(EvidenceRelation::Supports),
        "CHALLENGES" => Ok(EvidenceRelation::Challenges),
        value => Err(ValidationError::UnknownDomainValue {
            field: "evidence_relation",
            value: value.to_owned(),
        }),
    }
}

string_enum_serde!(EvidenceRelation, parse_evidence_relation);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum WireConfidence {
    #[serde(rename = "UNKNOWN")]
    Unknown,
    #[serde(rename = "NOT_APPLICABLE")]
    NotApplicable,
    #[serde(rename = "SCORE")]
    Score(f64),
}

impl WireConfidence {
    fn from_domain(value: &Confidence) -> Self {
        match value {
            Confidence::Unknown => Self::Unknown,
            Confidence::NotApplicable => Self::NotApplicable,
            Confidence::Score(value) => Self::Score(value.as_fraction()),
        }
    }

    fn into_domain(self) -> Result<Confidence, ValidationError> {
        match self {
            Self::Unknown => Ok(Confidence::Unknown),
            Self::NotApplicable => Ok(Confidence::NotApplicable),
            Self::Score(value) => Confidence::score(value),
        }
    }
}

wire_serde!(Confidence, WireConfidence);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireQualityMetadata {
    trust: TrustClass,
    sensitivity: SensitivityClass,
    confidence: WireConfidence,
    freshness: FreshnessStatus,
    uncertainty: Uncertainty,
    conflict: ConflictStatus,
}

impl WireQualityMetadata {
    fn from_domain(value: &QualityMetadata) -> Self {
        Self {
            trust: value.trust(),
            sensitivity: value.sensitivity(),
            confidence: WireConfidence::from_domain(&value.confidence()),
            freshness: value.freshness(),
            uncertainty: value.uncertainty(),
            conflict: value.conflict(),
        }
    }

    fn into_domain(self) -> Result<QualityMetadata, ValidationError> {
        Ok(QualityMetadata::new(
            self.trust,
            self.sensitivity,
            self.confidence.into_domain()?,
            self.freshness,
            self.uncertainty,
        )
        .with_conflict(self.conflict))
    }
}

impl Serialize for UnixTimestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i64(self.seconds())
    }
}

impl<'de> Deserialize<'de> for UnixTimestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::new(i64::deserialize(deserializer)?))
    }
}

impl Serialize for FreshnessPolicy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(self.max_age_seconds())
    }
}

impl<'de> Deserialize<'de> for FreshnessPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::new(u64::deserialize(deserializer)?))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireValidityInterval {
    not_before: Option<UnixTimestamp>,
    not_after: Option<UnixTimestamp>,
}

impl WireValidityInterval {
    fn from_domain(value: &ValidityInterval) -> Self {
        Self {
            not_before: value.not_before(),
            not_after: value.not_after(),
        }
    }

    fn into_domain(self) -> Result<ValidityInterval, ValidationError> {
        ValidityInterval::new(self.not_before, self.not_after)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireProvenance {
    id: ProvenanceId,
    source_kind: SourceKind,
    source_id: SourceId,
    source_reference: String,
    producer: Option<String>,
    acquired_at: Option<SourceTimestamp>,
    source_timestamp: Option<SourceTimestamp>,
    parent_provenance: Vec<ProvenanceId>,
}

impl WireProvenance {
    fn from_domain(value: &Provenance) -> Self {
        Self {
            id: value.id().clone(),
            source_kind: value.source_kind(),
            source_id: value.source_id().clone(),
            source_reference: value.source_reference().to_owned(),
            producer: value.producer().map(str::to_owned),
            acquired_at: value.acquired_at().cloned(),
            source_timestamp: value.source_timestamp().cloned(),
            parent_provenance: value.parent_provenance().to_vec(),
        }
    }

    fn into_domain(self) -> Result<Provenance, ValidationError> {
        let mut value = Provenance::new(
            self.id,
            self.source_kind,
            self.source_id,
            self.source_reference,
        )?;
        if let Some(producer) = self.producer {
            value = value.with_producer(producer)?;
        }
        if let Some(acquired_at) = self.acquired_at {
            value = value.with_acquired_at(acquired_at);
        }
        if let Some(source_timestamp) = self.source_timestamp {
            value = value.with_source_timestamp(source_timestamp);
        }
        value.with_parent_provenance(self.parent_provenance)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireObservation {
    id: ObservationId,
    subject: String,
    value: WireTypedValue,
    provenance: ProvenanceId,
    occurred_at: Option<SourceTimestamp>,
}

impl WireObservation {
    fn from_domain(value: &Observation) -> Self {
        Self {
            id: value.id().clone(),
            subject: value.subject().to_string(),
            value: WireTypedValue::from_domain(value.value()),
            provenance: value.provenance().clone(),
            occurred_at: value.occurred_at().cloned(),
        }
    }

    fn into_domain(self) -> Result<Observation, ValidationError> {
        let observation = Observation::new(
            self.id,
            SubjectPath::from_str(&self.subject)?,
            self.value.into_domain()?,
            self.provenance,
        )?;
        Ok(match self.occurred_at {
            Some(value) => observation.with_occurred_at(value),
            None => observation,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireFact {
    id: FactId,
    subject: String,
    value: WireTypedValue,
    polarity: AssertionPolarity,
    observations: Vec<ObservationId>,
}

impl WireFact {
    fn from_domain(value: &Fact) -> Self {
        Self {
            id: value.id().clone(),
            subject: value.subject().to_string(),
            value: WireTypedValue::from_domain(value.value()),
            polarity: value.polarity(),
            observations: value.observations().to_vec(),
        }
    }

    fn into_domain(self) -> Result<Fact, ValidationError> {
        Fact::new(
            self.id,
            SubjectPath::from_str(&self.subject)?,
            self.value.into_domain()?,
            self.polarity,
            self.observations,
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum WireEvidenceContent {
    #[serde(rename = "INLINE")]
    Inline(String),
    #[serde(rename = "REFERENCE")]
    Reference {
        reference: ReferenceId,
        digest: Option<ContentDigest>,
    },
}

impl WireEvidenceContent {
    fn from_domain(value: &EvidenceContent) -> Self {
        match value {
            EvidenceContent::Inline(value) => Self::Inline(value.as_str().to_owned()),
            EvidenceContent::Reference { reference, digest } => Self::Reference {
                reference: reference.clone(),
                digest: digest.clone(),
            },
        }
    }

    fn into_domain(self) -> Result<EvidenceContent, ValidationError> {
        match self {
            Self::Inline(value) => EvidenceContent::inline(value),
            Self::Reference { reference, digest } => {
                Ok(EvidenceContent::reference(reference, digest))
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireEvidenceLink {
    fact: FactId,
    relation: EvidenceRelation,
}

impl WireEvidenceLink {
    fn from_domain(value: &EvidenceLink) -> Self {
        Self {
            fact: value.fact().clone(),
            relation: value.relation(),
        }
    }
}

impl From<WireEvidenceLink> for EvidenceLink {
    fn from(value: WireEvidenceLink) -> Self {
        Self::new(value.fact, value.relation)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireEvidence {
    id: EvidenceId,
    kind: EvidenceKind,
    summary: String,
    content: WireEvidenceContent,
    provenance: ProvenanceId,
    links: Vec<WireEvidenceLink>,
    occurred_at: Option<SourceTimestamp>,
}

impl WireEvidence {
    fn from_domain(value: &Evidence) -> Self {
        Self {
            id: value.id().clone(),
            kind: value.kind(),
            summary: value.summary().to_owned(),
            content: WireEvidenceContent::from_domain(value.content()),
            provenance: value.provenance().clone(),
            links: value
                .links()
                .iter()
                .map(WireEvidenceLink::from_domain)
                .collect(),
            occurred_at: value.occurred_at().cloned(),
        }
    }

    fn into_domain(self) -> Result<Evidence, ValidationError> {
        let evidence = Evidence::new(
            self.id,
            self.kind,
            self.summary,
            self.content.into_domain()?,
            self.provenance,
            self.links.into_iter().map(Into::into).collect(),
        )?;
        Ok(match self.occurred_at {
            Some(value) => evidence.with_occurred_at(value),
            None => evidence,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireObservationEvidenceSet {
    provenances: Vec<WireProvenance>,
    observations: Vec<WireObservation>,
    facts: Vec<WireFact>,
    evidence: Vec<WireEvidence>,
}

impl WireObservationEvidenceSet {
    fn from_domain(value: &ObservationEvidenceSet) -> Self {
        Self {
            provenances: value
                .provenances()
                .iter()
                .map(WireProvenance::from_domain)
                .collect(),
            observations: value
                .observations()
                .iter()
                .map(WireObservation::from_domain)
                .collect(),
            facts: value.facts().iter().map(WireFact::from_domain).collect(),
            evidence: value
                .evidence()
                .iter()
                .map(WireEvidence::from_domain)
                .collect(),
        }
    }

    fn into_domain(self) -> Result<ObservationEvidenceSet, ValidationError> {
        ObservationEvidenceSet::new(
            self.provenances
                .into_iter()
                .map(WireProvenance::into_domain)
                .collect::<Result<Vec<_>, _>>()?,
            self.observations
                .into_iter()
                .map(WireObservation::into_domain)
                .collect::<Result<Vec<_>, _>>()?,
            self.facts
                .into_iter()
                .map(WireFact::into_domain)
                .collect::<Result<Vec<_>, _>>()?,
            self.evidence
                .into_iter()
                .map(WireEvidence::into_domain)
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}

wire_serde!(Provenance, WireProvenance);
wire_serde!(Observation, WireObservation);
wire_serde!(Fact, WireFact);
wire_serde!(Evidence, WireEvidence);
wire_serde!(ObservationEvidenceSet, WireObservationEvidenceSet);

fn parse_state_status(value: &str) -> Result<StateStatus, ValidationError> {
    match value {
        "KNOWN" => Ok(StateStatus::Known),
        "UNKNOWN" => Ok(StateStatus::Unknown),
        "CONFLICTED" => Ok(StateStatus::Conflicted),
        "UNSUPPORTED" => Ok(StateStatus::Unsupported),
        value => Err(ValidationError::UnknownDomainValue {
            field: "state_status",
            value: value.to_owned(),
        }),
    }
}

fn parse_normalization_reason(value: &str) -> Result<NormalizationReasonCode, ValidationError> {
    match value {
        "UNKNOWN_STATE" => Ok(NormalizationReasonCode::UnknownState),
        "CONFLICTING_ASSERTIONS" => Ok(NormalizationReasonCode::ConflictingAssertions),
        "INCOMPATIBLE_VALUE_TYPES" => Ok(NormalizationReasonCode::IncompatibleValueTypes),
        "MISSING_EVIDENCE" => Ok(NormalizationReasonCode::MissingEvidence),
        value => Err(ValidationError::UnknownDomainValue {
            field: "normalization_reason_code",
            value: value.to_owned(),
        }),
    }
}

string_enum_serde!(StateStatus, parse_state_status);
string_enum_serde!(NormalizationReasonCode, parse_normalization_reason);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireNormalizedClaim {
    fact: FactId,
    value: WireTypedValue,
    polarity: AssertionPolarity,
    observations: Vec<ObservationId>,
    provenances: Vec<ProvenanceId>,
    supporting_evidence: Vec<EvidenceId>,
    challenging_evidence: Vec<EvidenceId>,
}

impl WireNormalizedClaim {
    fn from_domain(value: &NormalizedClaim) -> Self {
        Self {
            fact: value.fact().clone(),
            value: WireTypedValue::from_domain(value.value()),
            polarity: value.polarity(),
            observations: value.observations().to_vec(),
            provenances: value.provenances().to_vec(),
            supporting_evidence: value.supporting_evidence().to_vec(),
            challenging_evidence: value.challenging_evidence().to_vec(),
        }
    }

    fn into_domain(self) -> Result<NormalizedClaim, ValidationError> {
        NormalizedClaim::from_parts(
            self.fact,
            self.value.into_domain()?,
            self.polarity,
            self.observations,
            self.provenances,
            self.supporting_evidence,
            self.challenging_evidence,
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireStateLineage {
    facts: Vec<FactId>,
    observations: Vec<ObservationId>,
    evidence: Vec<EvidenceId>,
    provenances: Vec<ProvenanceId>,
}

impl WireStateLineage {
    fn from_domain(value: &StateLineage) -> Self {
        Self {
            facts: value.facts().to_vec(),
            observations: value.observations().to_vec(),
            evidence: value.evidence().to_vec(),
            provenances: value.provenances().to_vec(),
        }
    }

    fn into_domain(self) -> Result<StateLineage, ValidationError> {
        StateLineage::from_parts(
            self.facts,
            self.observations,
            self.evidence,
            self.provenances,
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireNormalizedStateEntry {
    subject: String,
    status: StateStatus,
    value: Option<WireTypedValue>,
    polarity: Option<AssertionPolarity>,
    claims: Vec<WireNormalizedClaim>,
    lineage: WireStateLineage,
    diagnostics: Vec<NormalizationReasonCode>,
    metadata: Option<WireQualityMetadata>,
}

impl WireNormalizedStateEntry {
    fn from_domain(value: &NormalizedStateEntry) -> Self {
        Self {
            subject: value.subject().to_string(),
            status: value.status(),
            value: value.value().map(WireTypedValue::from_domain),
            polarity: value.polarity(),
            claims: value
                .claims()
                .iter()
                .map(WireNormalizedClaim::from_domain)
                .collect(),
            lineage: WireStateLineage::from_domain(value.lineage()),
            diagnostics: value.diagnostics().to_vec(),
            metadata: value
                .metadata()
                .map(|metadata| WireQualityMetadata::from_domain(&metadata)),
        }
    }

    fn into_domain(self) -> Result<NormalizedStateEntry, ValidationError> {
        NormalizedStateEntry::from_parts(
            SubjectPath::from_str(&self.subject)?,
            self.status,
            self.value.map(WireTypedValue::into_domain).transpose()?,
            self.polarity,
            self.claims
                .into_iter()
                .map(WireNormalizedClaim::into_domain)
                .collect::<Result<Vec<_>, _>>()?,
            self.lineage.into_domain()?,
            self.diagnostics,
            self.metadata
                .map(WireQualityMetadata::into_domain)
                .transpose()?,
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireNormalizationDiagnostic {
    code: NormalizationReasonCode,
    subject: String,
    facts: Vec<FactId>,
}

impl WireNormalizationDiagnostic {
    fn from_domain(value: &NormalizationDiagnostic) -> Self {
        Self {
            code: value.code(),
            subject: value.subject().to_string(),
            facts: value.facts().to_vec(),
        }
    }

    fn into_domain(self) -> Result<NormalizationDiagnostic, ValidationError> {
        NormalizationDiagnostic::from_parts(
            self.code,
            SubjectPath::from_str(&self.subject)?,
            self.facts,
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireObservedState {
    schema_version: DeclarativeContextVersion,
    id: ObservedStateId,
    entries: Vec<WireNormalizedStateEntry>,
    diagnostics: Vec<WireNormalizationDiagnostic>,
}

impl WireObservedState {
    fn from_domain(value: &ObservedState) -> Self {
        Self {
            schema_version: value.version(),
            id: value.id().clone(),
            entries: value
                .entries()
                .iter()
                .map(WireNormalizedStateEntry::from_domain)
                .collect(),
            diagnostics: value
                .diagnostics()
                .iter()
                .map(WireNormalizationDiagnostic::from_domain)
                .collect(),
        }
    }

    fn into_domain(self) -> Result<ObservedState, ValidationError> {
        self.schema_version.ensure_supported()?;
        let mut entries = self
            .entries
            .into_iter()
            .map(WireNormalizedStateEntry::into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort_by(|left, right| left.subject().cmp(right.subject()));
        if entries
            .windows(2)
            .any(|pair| pair[0].subject() == pair[1].subject())
        {
            return Err(ValidationError::DuplicateRelationship {
                field: "observed_state.entries",
            });
        }
        let mut diagnostics = self
            .diagnostics
            .into_iter()
            .map(WireNormalizationDiagnostic::into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        diagnostics.sort();
        if diagnostics.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ValidationError::DuplicateRelationship {
                field: "observed_state.diagnostics",
            });
        }
        Ok(ObservedState::new_v1_with_entries(
            self.id,
            entries,
            diagnostics,
        ))
    }
}

wire_serde!(ObservedState, WireObservedState);

fn parse_assessment_kind(value: &str) -> Result<AssessmentKind, ValidationError> {
    match value {
        "QUALITY" => Ok(AssessmentKind::Quality),
        "ARCHITECTURE" => Ok(AssessmentKind::Architecture),
        "COVERAGE" => Ok(AssessmentKind::Coverage),
        "DEPENDENCY" => Ok(AssessmentKind::Dependency),
        "DATA_QUALITY" => Ok(AssessmentKind::DataQuality),
        "OPERATIONAL" => Ok(AssessmentKind::Operational),
        "SECURITY" => Ok(AssessmentKind::Security),
        value => Err(ValidationError::UnknownDomainValue {
            field: "assessment_kind",
            value: value.to_owned(),
        }),
    }
}

fn parse_assessment_conclusion(value: &str) -> Result<AssessmentConclusion, ValidationError> {
    match value {
        "POSITIVE" => Ok(AssessmentConclusion::Positive),
        "AT_RISK" => Ok(AssessmentConclusion::AtRisk),
        "NEGATIVE" => Ok(AssessmentConclusion::Negative),
        "UNKNOWN" => Ok(AssessmentConclusion::Unknown),
        value => Err(ValidationError::UnknownDomainValue {
            field: "assessment_conclusion",
            value: value.to_owned(),
        }),
    }
}

fn parse_assessment_status(value: &str) -> Result<AssessmentStatus, ValidationError> {
    match value {
        "DETERMINED" => Ok(AssessmentStatus::Determined),
        "UNRESOLVED" => Ok(AssessmentStatus::Unresolved),
        "PROPOSED" => Ok(AssessmentStatus::Proposed),
        value => Err(ValidationError::UnknownDomainValue {
            field: "assessment_status",
            value: value.to_owned(),
        }),
    }
}

fn parse_risk_category(value: &str) -> Result<RiskCategory, ValidationError> {
    match value {
        "QUALITY" => Ok(RiskCategory::Quality),
        "ARCHITECTURE" => Ok(RiskCategory::Architecture),
        "DEPENDENCY" => Ok(RiskCategory::Dependency),
        "SECURITY" => Ok(RiskCategory::Security),
        "OPERATIONAL" => Ok(RiskCategory::Operational),
        "DATA_QUALITY" => Ok(RiskCategory::DataQuality),
        value => Err(ValidationError::UnknownDomainValue {
            field: "risk_category",
            value: value.to_owned(),
        }),
    }
}

fn parse_risk_severity(value: &str) -> Result<RiskSeverity, ValidationError> {
    match value {
        "UNKNOWN" => Ok(RiskSeverity::Unknown),
        "INFORMATIONAL" => Ok(RiskSeverity::Informational),
        "LOW" => Ok(RiskSeverity::Low),
        "MEDIUM" => Ok(RiskSeverity::Medium),
        "HIGH" => Ok(RiskSeverity::High),
        "CRITICAL" => Ok(RiskSeverity::Critical),
        value => Err(ValidationError::UnknownDomainValue {
            field: "risk_severity",
            value: value.to_owned(),
        }),
    }
}

fn parse_risk_status(value: &str) -> Result<RiskStatus, ValidationError> {
    match value {
        "OPEN" => Ok(RiskStatus::Open),
        "UNRESOLVED" => Ok(RiskStatus::Unresolved),
        "UNKNOWN" => Ok(RiskStatus::Unknown),
        value => Err(ValidationError::UnknownDomainValue {
            field: "risk_status",
            value: value.to_owned(),
        }),
    }
}

fn parse_likelihood(value: &str) -> Result<QualitativeLikelihood, ValidationError> {
    match value {
        "RARE" => Ok(QualitativeLikelihood::Rare),
        "POSSIBLE" => Ok(QualitativeLikelihood::Possible),
        "LIKELY" => Ok(QualitativeLikelihood::Likely),
        value => Err(ValidationError::UnknownDomainValue {
            field: "qualitative_likelihood",
            value: value.to_owned(),
        }),
    }
}

fn parse_situation_diagnostic(value: &str) -> Result<SituationDiagnosticCode, ValidationError> {
    match value {
        "UNKNOWN_STATE" => Ok(SituationDiagnosticCode::UnknownState),
        "STATE_CONFLICT" => Ok(SituationDiagnosticCode::StateConflict),
        "UNSUPPORTED_STATE" => Ok(SituationDiagnosticCode::UnsupportedState),
        "UNRESOLVED_ASSESSMENT" => Ok(SituationDiagnosticCode::UnresolvedAssessment),
        "UNKNOWN_RISK" => Ok(SituationDiagnosticCode::UnknownRisk),
        "DATA_QUALITY" => Ok(SituationDiagnosticCode::DataQuality),
        value => Err(ValidationError::UnknownDomainValue {
            field: "situation_diagnostic_code",
            value: value.to_owned(),
        }),
    }
}

string_enum_serde!(AssessmentKind, parse_assessment_kind);
string_enum_serde!(AssessmentConclusion, parse_assessment_conclusion);
string_enum_serde!(AssessmentStatus, parse_assessment_status);
string_enum_serde!(RiskCategory, parse_risk_category);
string_enum_serde!(RiskSeverity, parse_risk_severity);
string_enum_serde!(RiskStatus, parse_risk_status);
string_enum_serde!(SituationDiagnosticCode, parse_situation_diagnostic);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireRuleContract {
    id: AssessmentRuleId,
    version: AssessmentRuleVersion,
    semantic_digest: Option<ContentDigest>,
}

impl WireRuleContract {
    fn from_domain(value: &AssessmentRuleContract) -> Self {
        Self {
            id: value.id().clone(),
            version: value.version(),
            semantic_digest: value.semantic_digest().cloned(),
        }
    }

    fn into_domain(self) -> Result<AssessmentRuleContract, ValidationError> {
        let value = AssessmentRuleContract::new(self.id, self.version)?;
        Ok(match self.semantic_digest {
            Some(digest) => value.with_semantic_digest(digest),
            None => value,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireBasisReferences {
    state_subjects: Vec<String>,
    facts: Vec<FactId>,
    evidence: Vec<EvidenceId>,
    provenances: Vec<ProvenanceId>,
    assessments: Vec<AssessmentId>,
}

impl WireBasisReferences {
    fn from_domain(value: &BasisReferences) -> Self {
        Self {
            state_subjects: value
                .state_subjects()
                .iter()
                .map(ToString::to_string)
                .collect(),
            facts: value.facts().to_vec(),
            evidence: value.evidence().to_vec(),
            provenances: value.provenances().to_vec(),
            assessments: value.assessments().to_vec(),
        }
    }

    fn into_domain(self) -> Result<BasisReferences, ValidationError> {
        BasisReferences::new(
            self.state_subjects
                .iter()
                .map(|value| SubjectPath::from_str(value))
                .collect::<Result<Vec<_>, _>>()?,
            self.facts,
            self.evidence,
            self.provenances,
            self.assessments,
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum WireAssessmentOrigin {
    #[serde(rename = "DETERMINISTIC")]
    Deterministic { rule: WireRuleContract },
    #[serde(rename = "EXTERNAL")]
    External {
        source_kind: SourceKind,
        provenance: ProvenanceId,
    },
}

impl WireAssessmentOrigin {
    fn from_domain(value: &AssessmentOrigin) -> Self {
        match value {
            AssessmentOrigin::Deterministic { rule } => Self::Deterministic {
                rule: WireRuleContract::from_domain(rule),
            },
            AssessmentOrigin::External {
                source_kind,
                provenance,
            } => Self::External {
                source_kind: *source_kind,
                provenance: provenance.clone(),
            },
        }
    }

    fn into_domain(self) -> Result<AssessmentOrigin, ValidationError> {
        match self {
            Self::Deterministic { rule } => Ok(AssessmentOrigin::Deterministic {
                rule: rule.into_domain()?,
            }),
            Self::External {
                source_kind,
                provenance,
            } => Ok(AssessmentOrigin::External {
                source_kind,
                provenance,
            }),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireAssessment {
    id: AssessmentId,
    kind: AssessmentKind,
    conclusion: AssessmentConclusion,
    status: AssessmentStatus,
    reason: ReasonCode,
    summary: String,
    basis: WireBasisReferences,
    origin: WireAssessmentOrigin,
    quality: WireQualityMetadata,
}

impl WireAssessment {
    fn from_domain(value: &Assessment) -> Self {
        Self {
            id: value.id().clone(),
            kind: value.kind(),
            conclusion: value.conclusion(),
            status: value.status(),
            reason: value.reason().clone(),
            summary: value.summary().to_owned(),
            basis: WireBasisReferences::from_domain(value.basis()),
            origin: WireAssessmentOrigin::from_domain(value.origin()),
            quality: WireQualityMetadata::from_domain(&value.quality()),
        }
    }

    fn into_domain(self) -> Result<Assessment, ValidationError> {
        Assessment::new(
            self.id,
            self.kind,
            self.conclusion,
            self.status,
            self.reason,
            self.summary,
            self.basis.into_domain()?,
            self.origin.into_domain()?,
            self.quality.into_domain()?,
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum WireRiskLikelihood {
    #[serde(rename = "UNKNOWN")]
    Unknown,
    #[serde(rename = "QUALITATIVE")]
    Qualitative(String),
    #[serde(rename = "EXPLICIT_PROBABILITY")]
    ExplicitProbability(f64),
}

impl WireRiskLikelihood {
    fn from_domain(value: RiskLikelihood) -> Self {
        match value {
            RiskLikelihood::Unknown => Self::Unknown,
            RiskLikelihood::Qualitative(value) => Self::Qualitative(value.as_str().to_owned()),
            RiskLikelihood::ExplicitProbability(value) => {
                Self::ExplicitProbability(value.as_fraction())
            }
        }
    }

    fn into_domain(self) -> Result<RiskLikelihood, ValidationError> {
        match self {
            Self::Unknown => Ok(RiskLikelihood::Unknown),
            Self::Qualitative(value) => Ok(RiskLikelihood::Qualitative(parse_likelihood(&value)?)),
            Self::ExplicitProbability(value) => RiskLikelihood::probability(value),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum WireRiskOrigin {
    #[serde(rename = "DETERMINISTIC")]
    Deterministic { rule: WireRuleContract },
    #[serde(rename = "ASSESSMENT_DERIVED")]
    AssessmentDerived,
    #[serde(rename = "EXTERNAL")]
    External {
        source_kind: SourceKind,
        provenance: ProvenanceId,
    },
}

impl WireRiskOrigin {
    fn from_domain(value: &RiskOrigin) -> Self {
        match value {
            RiskOrigin::Deterministic { rule } => Self::Deterministic {
                rule: WireRuleContract::from_domain(rule),
            },
            RiskOrigin::AssessmentDerived => Self::AssessmentDerived,
            RiskOrigin::External {
                source_kind,
                provenance,
            } => Self::External {
                source_kind: *source_kind,
                provenance: provenance.clone(),
            },
        }
    }

    fn into_domain(self) -> Result<RiskOrigin, ValidationError> {
        match self {
            Self::Deterministic { rule } => Ok(RiskOrigin::Deterministic {
                rule: rule.into_domain()?,
            }),
            Self::AssessmentDerived => Ok(RiskOrigin::AssessmentDerived),
            Self::External {
                source_kind,
                provenance,
            } => Ok(RiskOrigin::External {
                source_kind,
                provenance,
            }),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireRisk {
    id: RiskId,
    category: RiskCategory,
    severity: RiskSeverity,
    likelihood: WireRiskLikelihood,
    status: RiskStatus,
    reason: ReasonCode,
    summary: String,
    basis: WireBasisReferences,
    origin: WireRiskOrigin,
    quality: WireQualityMetadata,
}

impl WireRisk {
    fn from_domain(value: &Risk) -> Self {
        Self {
            id: value.id().clone(),
            category: value.category(),
            severity: value.severity(),
            likelihood: WireRiskLikelihood::from_domain(value.likelihood()),
            status: value.status(),
            reason: value.reason().clone(),
            summary: value.summary().to_owned(),
            basis: WireBasisReferences::from_domain(value.basis()),
            origin: WireRiskOrigin::from_domain(value.origin()),
            quality: WireQualityMetadata::from_domain(&value.quality()),
        }
    }

    fn into_domain(self) -> Result<Risk, ValidationError> {
        Risk::new(
            self.id,
            self.category,
            self.severity,
            self.likelihood.into_domain()?,
            self.status,
            self.reason,
            self.summary,
            self.basis.into_domain()?,
            self.origin.into_domain()?,
            self.quality.into_domain()?,
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireSituationDiagnostic {
    code: SituationDiagnosticCode,
    summary: String,
    basis: WireBasisReferences,
}

impl WireSituationDiagnostic {
    fn from_domain(value: &SituationDiagnostic) -> Self {
        Self {
            code: value.code(),
            summary: value.summary().to_owned(),
            basis: WireBasisReferences::from_domain(value.basis()),
        }
    }

    fn into_domain(self) -> Result<SituationDiagnostic, ValidationError> {
        SituationDiagnostic::new(self.code, self.summary, self.basis.into_domain()?)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum WireSituationReference {
    #[serde(rename = "EXTERNAL")]
    External {
        source: SourceId,
        reference: ReferenceId,
    },
    #[serde(rename = "RUNTIME")]
    Runtime {
        runtime: crate::ExecutionRuntimeId,
        reference: ReferenceId,
    },
}

impl WireSituationReference {
    fn from_domain(value: &SituationReference) -> Self {
        match value {
            SituationReference::External { source, reference } => Self::External {
                source: source.clone(),
                reference: reference.clone(),
            },
            SituationReference::Runtime { runtime, reference } => Self::Runtime {
                runtime: runtime.clone(),
                reference: reference.clone(),
            },
        }
    }

    fn into_domain(self) -> SituationReference {
        match self {
            Self::External { source, reference } => {
                SituationReference::External { source, reference }
            }
            Self::Runtime { runtime, reference } => {
                SituationReference::Runtime { runtime, reference }
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireSituation {
    schema_version: DeclarativeContextVersion,
    id: SituationId,
    observed_state_id: Option<ObservedStateId>,
    assessments: Vec<WireAssessment>,
    risks: Vec<WireRisk>,
    diagnostics: Vec<WireSituationDiagnostic>,
    references: Vec<WireSituationReference>,
}

impl WireSituation {
    fn from_domain(value: &Situation) -> Self {
        Self {
            schema_version: value.version(),
            id: value.id().clone(),
            observed_state_id: value.observed_state_id().cloned(),
            assessments: value
                .assessments()
                .iter()
                .map(WireAssessment::from_domain)
                .collect(),
            risks: value.risks().iter().map(WireRisk::from_domain).collect(),
            diagnostics: value
                .diagnostics()
                .iter()
                .map(WireSituationDiagnostic::from_domain)
                .collect(),
            references: value
                .references()
                .iter()
                .map(WireSituationReference::from_domain)
                .collect(),
        }
    }

    fn into_domain(self) -> Result<Situation, ValidationError> {
        self.schema_version.ensure_supported()?;
        let mut assessments = self
            .assessments
            .into_iter()
            .map(WireAssessment::into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        assessments.sort_by(|left, right| left.id().cmp(right.id()));
        ensure_unique_ids(&assessments, "assessment")?;
        let assessment_ids = assessments
            .iter()
            .map(|assessment| assessment.id().clone())
            .collect::<std::collections::BTreeSet<_>>();
        let mut risks = self
            .risks
            .into_iter()
            .map(WireRisk::into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        risks.sort_by(|left, right| left.id().cmp(right.id()));
        ensure_unique_ids(&risks, "risk")?;
        for risk in &risks {
            for assessment in risk.basis().assessments() {
                if !assessment_ids.contains(assessment) {
                    return Err(ValidationError::MissingDeclarativeIdentity {
                        kind: "assessment",
                        id: assessment.to_string(),
                    });
                }
            }
        }
        let mut diagnostics = self
            .diagnostics
            .into_iter()
            .map(WireSituationDiagnostic::into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        diagnostics.sort();
        if diagnostics.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ValidationError::DuplicateRelationship {
                field: "situation.diagnostics",
            });
        }
        let mut references = self
            .references
            .into_iter()
            .map(WireSituationReference::into_domain)
            .collect::<Vec<_>>();
        references.sort();
        if references.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ValidationError::DuplicateRelationship {
                field: "situation.references",
            });
        }
        match self.observed_state_id {
            Some(observed_state_id) => Ok(Situation::from_parts(
                self.schema_version,
                self.id,
                observed_state_id,
                assessments,
                risks,
                diagnostics,
                references,
            )),
            None if assessments.is_empty()
                && risks.is_empty()
                && diagnostics.is_empty()
                && references.is_empty() =>
            {
                Situation::new(self.schema_version, self.id)
            }
            None => Err(ValidationError::InvalidDeclarativeValue {
                reason: "serialized situation must reference an observed state snapshot",
            }),
        }
    }
}

fn ensure_unique_ids<T>(values: &[T], kind: &'static str) -> Result<(), ValidationError>
where
    T: HasId,
{
    for pair in values.windows(2) {
        if pair[0].id_text() == pair[1].id_text() {
            return Err(ValidationError::DuplicateDeclarativeIdentity {
                kind,
                id: pair[0].id_text().to_owned(),
            });
        }
    }
    Ok(())
}

trait HasId {
    fn id_text(&self) -> &str;
}

impl HasId for Assessment {
    fn id_text(&self) -> &str {
        self.id().as_str()
    }
}

impl HasId for Risk {
    fn id_text(&self) -> &str {
        self.id().as_str()
    }
}

wire_serde!(Assessment, WireAssessment);
wire_serde!(Risk, WireRisk);
wire_serde!(Situation, WireSituation);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireDeclarativeContext {
    schema_version: DeclarativeContextVersion,
    id: DeclarativeContextId,
}

impl WireDeclarativeContext {
    fn from_domain(value: &DeclarativeContext) -> Self {
        Self {
            schema_version: value.version(),
            id: value.id().clone(),
        }
    }

    fn into_domain(self) -> Result<DeclarativeContext, ValidationError> {
        DeclarativeContext::new(self.schema_version, self.id)
    }
}

wire_serde!(DeclarativeContext, WireDeclarativeContext);
wire_serde!(DesiredState, WireDesiredState);
wire_serde!(Intent, WireIntent);
wire_serde!(ConditionExpression, WireConditionExpression);

impl Serialize for QualityMetadata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireQualityMetadata::from_domain(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for QualityMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        WireQualityMetadata::deserialize(deserializer)?
            .into_domain()
            .map_err(D::Error::custom)
    }
}

impl Serialize for ValidityInterval {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireValidityInterval::from_domain(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ValidityInterval {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        WireValidityInterval::deserialize(deserializer)?
            .into_domain()
            .map_err(D::Error::custom)
    }
}

/// Complete provider-independent CG-06 document used for fixture and
/// regression round trips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarativeContextSituationDocument {
    context: DeclarativeContext,
    intent: Option<Intent>,
    records: Option<ObservationEvidenceSet>,
    observed_state: ObservedState,
    situation: Situation,
}

impl DeclarativeContextSituationDocument {
    /// Creates a complete document and checks cross-aggregate identity.
    pub fn new(
        context: DeclarativeContext,
        intent: Option<Intent>,
        records: Option<ObservationEvidenceSet>,
        observed_state: ObservedState,
        situation: Situation,
    ) -> Result<Self, ValidationError> {
        context.version().ensure_supported()?;
        observed_state.version().ensure_supported()?;
        situation.version().ensure_supported()?;
        if let Some(intent) = &intent {
            intent.version().ensure_supported()?;
        }
        if let Some(records) = &records {
            records.validate()?;
        }
        if situation.observed_state_id() != Some(observed_state.id()) {
            return Err(ValidationError::MissingDeclarativeIdentity {
                kind: "situation observed_state",
                id: observed_state.id().to_string(),
            });
        }
        Ok(Self {
            context,
            intent,
            records,
            observed_state,
            situation,
        })
    }

    /// Returns the declarative context aggregate.
    #[must_use]
    pub const fn context(&self) -> &DeclarativeContext {
        &self.context
    }

    /// Returns the optional structured intent.
    #[must_use]
    pub const fn intent(&self) -> Option<&Intent> {
        self.intent.as_ref()
    }

    /// Returns the optional complete observation/evidence lineage.
    #[must_use]
    pub const fn records(&self) -> Option<&ObservationEvidenceSet> {
        self.records.as_ref()
    }

    /// Returns the normalized observed-state snapshot.
    #[must_use]
    pub const fn observed_state(&self) -> &ObservedState {
        &self.observed_state
    }

    /// Returns the assembled situation snapshot.
    #[must_use]
    pub const fn situation(&self) -> &Situation {
        &self.situation
    }

    /// Serializes the complete document in canonical compact JSON.
    pub fn to_json(&self) -> Result<String, SerializationError> {
        serde_json::to_string(self).map_err(SerializationError::Json)
    }

    /// Serializes the complete document in human-readable deterministic JSON.
    pub fn to_json_pretty(&self) -> Result<String, SerializationError> {
        serde_json::to_string_pretty(self).map_err(SerializationError::Json)
    }

    /// Parses and validates a complete v1 document.
    pub fn from_json(value: &str) -> Result<Self, SerializationError> {
        let wire = serde_json::from_str::<WireDeclarativeContextSituationDocument>(value)
            .map_err(SerializationError::Json)?;
        wire.into_domain().map_err(SerializationError::Validation)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireDeclarativeContextSituationDocument {
    schema_version: DeclarativeContextVersion,
    context: WireDeclarativeContext,
    intent: Option<WireIntent>,
    records: Option<WireObservationEvidenceSet>,
    observed_state: WireObservedState,
    situation: WireSituation,
}

impl WireDeclarativeContextSituationDocument {
    fn from_domain(value: &DeclarativeContextSituationDocument) -> Self {
        Self {
            schema_version: crate::declarative_context::DECLARATIVE_CONTEXT_IR_VERSION,
            context: WireDeclarativeContext::from_domain(value.context()),
            intent: value.intent().map(WireIntent::from_domain),
            records: value.records().map(WireObservationEvidenceSet::from_domain),
            observed_state: WireObservedState::from_domain(value.observed_state()),
            situation: WireSituation::from_domain(value.situation()),
        }
    }

    fn into_domain(self) -> Result<DeclarativeContextSituationDocument, ValidationError> {
        self.schema_version.ensure_supported()?;
        DeclarativeContextSituationDocument::new(
            self.context.into_domain()?,
            self.intent.map(WireIntent::into_domain).transpose()?,
            self.records
                .map(WireObservationEvidenceSet::into_domain)
                .transpose()?,
            self.observed_state.into_domain()?,
            self.situation.into_domain()?,
        )
    }
}

impl Serialize for DeclarativeContextSituationDocument {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WireDeclarativeContextSituationDocument::from_domain(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DeclarativeContextSituationDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        WireDeclarativeContextSituationDocument::deserialize(deserializer)?
            .into_domain()
            .map_err(D::Error::custom)
    }
}

macro_rules! json_api {
    ($type:ty, $wire:ty) => {
        impl $type {
            /// Serializes this validated CG-06 value as compact JSON.
            pub fn to_json(&self) -> Result<String, SerializationError> {
                serde_json::to_string(self).map_err(SerializationError::Json)
            }

            /// Parses and validates this CG-06 value from JSON.
            pub fn from_json(value: &str) -> Result<Self, SerializationError> {
                let wire =
                    serde_json::from_str::<$wire>(value).map_err(SerializationError::Json)?;
                wire.into_domain().map_err(SerializationError::Validation)
            }
        }
    };
}

json_api!(DeclarativeContext, WireDeclarativeContext);
json_api!(DesiredState, WireDesiredState);
json_api!(Intent, WireIntent);
json_api!(ObservedState, WireObservedState);
json_api!(ObservationEvidenceSet, WireObservationEvidenceSet);
json_api!(Assessment, WireAssessment);
json_api!(Risk, WireRisk);
json_api!(Situation, WireSituation);

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::{
        AssessmentId, AssessmentKind, AssessmentOrigin, AssessmentRuleContract, AssessmentRuleId,
        AssessmentRuleVersion, AssessmentStatus, ConditionId, DeclarativeConstraint,
        DesiredStateId, EvidenceContent, EvidenceId, EvidenceKind, EvidenceLink, EvidenceRelation,
        FactId, FreshnessStatus, IntentId, NormalizationInput, NormalizationReasonCode,
        ObservationId, ObservedStateId, ProvenanceId, QualitativeLikelihood, RiskCategory, RiskId,
        RiskLikelihood, RiskOrigin, RiskSeverity, RiskStatus, SituationAssemblyInput, SituationId,
        SituationReference, SourceId, SourceKind, SourceTimestamp, TrustClass, Uncertainty,
        declarative_context::DECLARATIVE_CONTEXT_IR_VERSION,
        identifiers::{ContextScopeId, ExecutionRuntimeId, ReferenceId},
        intent::{AcceptanceCriterion, DecimalValue},
        normalize_current_state,
        observation::{Fact, Observation, ObservationEvidenceSet, Provenance},
        quality::{Confidence, SensitivityClass},
    };

    fn records() -> ObservationEvidenceSet {
        let provenance = Provenance::new(
            ProvenanceId::new("prov-1").unwrap(),
            SourceKind::Repository,
            SourceId::new("repo-1").unwrap(),
            "repository://snapshot",
        )
        .unwrap()
        .with_producer("fixture")
        .unwrap()
        .with_acquired_at(SourceTimestamp::new("2026-08-29T10:00:00Z").unwrap())
        .with_source_timestamp(SourceTimestamp::new("2026-08-29T09:59:00Z").unwrap())
        .with_parent_provenance(Vec::new())
        .unwrap();
        let observation = Observation::new(
            ObservationId::new("observation-1").unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            TypedValue::Decimal(DecimalValue::new(9200, 2).unwrap()),
            provenance.id().clone(),
        )
        .unwrap()
        .with_occurred_at(SourceTimestamp::new("2026-08-29T10:01:00Z").unwrap());
        let fact = Fact::new(
            FactId::new("fact-1").unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            TypedValue::Decimal(DecimalValue::new(9200, 2).unwrap()),
            AssertionPolarity::Affirmed,
            vec![observation.id().clone()],
        )
        .unwrap();
        let evidence = Evidence::new(
            EvidenceId::new("evidence-1").unwrap(),
            EvidenceKind::Artifact,
            "coverage artifact",
            EvidenceContent::reference(
                ReferenceId::new("artifact-coverage").unwrap(),
                Some(ContentDigest::new("a".repeat(64)).unwrap()),
            ),
            provenance.id().clone(),
            vec![EvidenceLink::new(
                fact.id().clone(),
                EvidenceRelation::Supports,
            )],
        )
        .unwrap();
        ObservationEvidenceSet::new(
            vec![provenance],
            vec![observation],
            vec![fact],
            vec![evidence],
        )
        .unwrap()
    }

    fn state_and_records() -> (ObservedState, ObservationEvidenceSet) {
        let records = records();
        let state = normalize_current_state(
            ObservedStateId::new("state-1").unwrap(),
            NormalizationInput::new(records.clone()).with_quality_metadata(
                SubjectPath::from_str("coverage.percent").unwrap(),
                vec![QualityMetadata::new(
                    TrustClass::ObservedEvidence,
                    SensitivityClass::Confidential,
                    Confidence::score(0.92).unwrap(),
                    FreshnessStatus::Fresh,
                    Uncertainty::None,
                )],
            ),
        )
        .unwrap();
        (state, records)
    }

    fn intent() -> Intent {
        let coverage = DesiredCondition::new(
            ConditionId::new("coverage-target").unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            ComparisonOperator::GreaterOrEqual,
            Some(TypedValue::Integer(95)),
        )
        .unwrap();
        let release = DesiredCondition::new(
            ConditionId::new("release-present").unwrap(),
            SubjectPath::from_str("release.version").unwrap(),
            ComparisonOperator::Present,
            None,
        )
        .unwrap();
        let expression = ConditionExpression::all(vec![
            ConditionExpression::condition(coverage.id().clone()),
            ConditionExpression::any(vec![
                ConditionExpression::negate(ConditionExpression::condition(release.id().clone())),
                ConditionExpression::condition(coverage.id().clone()),
            ])
            .unwrap(),
        ])
        .unwrap();
        let desired = DesiredState::new(
            DesiredStateId::new("desired-1").unwrap(),
            vec![coverage, release],
            expression,
            vec![DeclarativeConstraint::new(
                crate::ConstraintId::new("constraint-1").unwrap(),
                ConditionExpression::condition(ConditionId::new("coverage-target").unwrap()),
            )],
            vec![
                AcceptanceCriterion::new(
                    crate::AcceptanceCriterionId::new("criterion-1").unwrap(),
                    "coverage is acceptable",
                    ConditionExpression::condition(ConditionId::new("coverage-target").unwrap()),
                )
                .unwrap(),
            ],
        )
        .unwrap();
        Intent::new(IntentId::new("intent-1").unwrap(), desired)
            .with_original_input(OriginalInput::inline("raise coverage").unwrap())
    }

    fn situation() -> (Situation, ObservedState, ObservationEvidenceSet) {
        let (state, records) = state_and_records();
        let basis = BasisReferences::from_state_entry(&state.entries()[0]).unwrap();
        let quality = state.entries()[0].metadata().unwrap();
        let assessment = Assessment::new(
            AssessmentId::new("assessment-1").unwrap(),
            AssessmentKind::Coverage,
            AssessmentConclusion::AtRisk,
            AssessmentStatus::Determined,
            ReasonCode::new("COVERAGE_BELOW_TARGET").unwrap(),
            "coverage is below target",
            basis.clone(),
            AssessmentOrigin::Deterministic {
                rule: AssessmentRuleContract::new(
                    AssessmentRuleId::new("coverage-target-rule").unwrap(),
                    AssessmentRuleVersion::V1,
                )
                .unwrap(),
            },
            quality,
        )
        .unwrap();
        let risk = Risk::new(
            RiskId::new("risk-1").unwrap(),
            RiskCategory::Quality,
            RiskSeverity::High,
            RiskLikelihood::Qualitative(QualitativeLikelihood::Possible),
            RiskStatus::Open,
            ReasonCode::new("COVERAGE_RISK").unwrap(),
            "coverage target may remain unmet",
            BasisReferences::new(
                basis.state_subjects().to_vec(),
                basis.facts().to_vec(),
                basis.evidence().to_vec(),
                basis.provenances().to_vec(),
                vec![assessment.id().clone()],
            )
            .unwrap(),
            RiskOrigin::AssessmentDerived,
            quality,
        )
        .unwrap();
        let situation = SituationAssemblyInput::new(state.clone())
            .with_records(records.clone())
            .with_assessments(vec![assessment])
            .unwrap()
            .with_risks(vec![risk])
            .unwrap()
            .with_references(vec![SituationReference::External {
                source: SourceId::new("github").unwrap(),
                reference: ReferenceId::new("issue-1").unwrap(),
            }])
            .unwrap()
            .assemble(SituationId::new("situation-1").unwrap())
            .unwrap();
        (situation, state, records)
    }

    fn document() -> DeclarativeContextSituationDocument {
        let (situation, state, records) = situation();
        DeclarativeContextSituationDocument::new(
            DeclarativeContext::new(
                DECLARATIVE_CONTEXT_IR_VERSION,
                DeclarativeContextId::new("context-1").unwrap(),
            )
            .unwrap(),
            Some(intent()),
            Some(records),
            state,
            situation,
        )
        .unwrap()
    }

    #[test]
    fn complete_document_round_trips_canonically_and_preserves_sensitive_references() {
        let original = document();
        let json = original.to_json().unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["schema_version"], "1.0");
        assert_eq!(value["observed_state"]["entries"][0]["status"], "KNOWN");
        assert_eq!(
            value["records"]["evidence"][0]["content"]["kind"],
            "REFERENCE"
        );
        assert_eq!(
            value["situation"]["assessments"][0]["origin"]["kind"],
            "DETERMINISTIC"
        );
        assert_eq!(
            value["situation"]["risks"][0]["likelihood"]["kind"],
            "QUALITATIVE"
        );
        assert!(!json.contains("provider"));
        let restored = DeclarativeContextSituationDocument::from_json(&json).unwrap();
        assert_eq!(restored, original);
        assert_eq!(restored.to_json().unwrap(), json);
        assert_eq!(
            DeclarativeContextSituationDocument::from_json(&original.to_json_pretty().unwrap())
                .unwrap(),
            original
        );
    }

    #[test]
    fn direct_cg06_values_round_trip_without_type_coercion() {
        let values = vec![
            TypedValue::Boolean(true),
            TypedValue::Integer(-7),
            TypedValue::Decimal(DecimalValue::new(9200, 2).unwrap()),
            TypedValue::string("text").unwrap(),
            TypedValue::symbol("SYMBOL").unwrap(),
            TypedValue::set(vec![TypedValue::Integer(1), TypedValue::Integer(2)]).unwrap(),
        ];
        for value in values {
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(serde_json::from_str::<TypedValue>(&json).unwrap(), value);
        }
        let (situation, state, records) = situation();
        assert_eq!(
            ObservedState::from_json(&state.to_json().unwrap()).unwrap(),
            state
        );
        assert_eq!(
            ObservationEvidenceSet::from_json(&records.to_json().unwrap()).unwrap(),
            records
        );
        assert_eq!(
            Situation::from_json(&situation.to_json().unwrap()).unwrap(),
            situation
        );
        assert_eq!(
            Intent::from_json(&intent().to_json().unwrap()).unwrap(),
            intent()
        );
        let quality = QualityMetadata::new(
            TrustClass::Mixed,
            SensitivityClass::Secret,
            Confidence::NotApplicable,
            FreshnessStatus::Unknown,
            Uncertainty::Probabilistic,
        )
        .with_conflict(ConflictStatus::Unresolved);
        assert_eq!(
            serde_json::from_str::<QualityMetadata>(&serde_json::to_string(&quality).unwrap())
                .unwrap(),
            quality
        );
        let interval =
            ValidityInterval::new(Some(UnixTimestamp::new(-5)), Some(UnixTimestamp::new(5)))
                .unwrap();
        assert_eq!(
            serde_json::from_str::<ValidityInterval>(&serde_json::to_string(&interval).unwrap())
                .unwrap(),
            interval
        );
        assert_eq!(
            serde_json::from_str::<FreshnessPolicy>(
                &serde_json::to_string(&FreshnessPolicy::new(60)).unwrap()
            )
            .unwrap(),
            FreshnessPolicy::new(60)
        );
    }

    #[test]
    fn reordered_collections_canonicalize_to_the_same_document() {
        let original = document();
        let mut value: Value = serde_json::from_str(&original.to_json().unwrap()).unwrap();
        value["records"]["evidence"] = json!([]);
        value["records"]["evidence"] = json!([{
            "id": "evidence-1",
            "kind": "ARTIFACT",
            "summary": "coverage artifact",
            "content": {"kind": "REFERENCE", "value": {"reference": "artifact-coverage", "digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}},
            "provenance": "prov-1",
            "links": [{"fact": "fact-1", "relation": "SUPPORTS"}],
            "occurred_at": null
        }]);
        let restored = DeclarativeContextSituationDocument::from_json(&value.to_string()).unwrap();
        assert_eq!(restored.to_json().unwrap(), original.to_json().unwrap());
    }

    #[test]
    fn malformed_versions_enums_fields_and_references_fail_closed() {
        let original = document();
        let json = original.to_json().unwrap();
        let mut value: Value = serde_json::from_str(&json).unwrap();
        value["schema_version"] = json!("2.0");
        assert!(DeclarativeContextSituationDocument::from_json(&value.to_string()).is_err());
        value["schema_version"] = json!("1.0");
        value["situation"]["risks"][0]["likelihood"]["kind"] = json!("MADE_UP");
        assert!(DeclarativeContextSituationDocument::from_json(&value.to_string()).is_err());
        value["situation"]["risks"][0]["likelihood"]["kind"] = json!("QUALITATIVE");
        value["situation"]["risks"][0]["likelihood"]["value"] = json!("MADE_UP");
        assert!(DeclarativeContextSituationDocument::from_json(&value.to_string()).is_err());
        value["situation"]["risks"][0]["likelihood"]["value"] = json!("POSSIBLE");
        value["situation"]["risks"][0]["basis"]["assessments"] = json!(["missing-assessment"]);
        assert!(matches!(
            DeclarativeContextSituationDocument::from_json(&value.to_string()),
            Err(SerializationError::Validation(
                ValidationError::MissingDeclarativeIdentity { .. }
            ))
        ));
        value["situation"]["risks"][0]["basis"]["assessments"] = json!(["assessment-1"]);
        value["unknown_field"] = json!(true);
        assert!(DeclarativeContextSituationDocument::from_json(&value.to_string()).is_err());
    }

    #[test]
    fn invalid_typed_values_quality_intervals_lineage_and_duplicates_fail_closed() {
        let original = document();
        let mut value: Value = serde_json::from_str(&original.to_json().unwrap()).unwrap();
        value["intent"]["desired_state"]["conditions"][0]["expected"] = json!({
            "kind": "DECIMAL",
            "value": {"unscaled": "not-an-integer", "scale": 2}
        });
        assert!(DeclarativeContextSituationDocument::from_json(&value.to_string()).is_err());

        let mut interval = serde_json::to_value(
            ValidityInterval::new(Some(UnixTimestamp::new(1)), Some(UnixTimestamp::new(2)))
                .unwrap(),
        )
        .unwrap();
        interval["not_before"] = json!(3);
        interval["not_after"] = json!(2);
        assert!(serde_json::from_value::<ValidityInterval>(interval).is_err());
        assert!(
            serde_json::from_value::<Confidence>(json!({"kind": "SCORE", "value": 2.0})).is_err()
        );

        let mut duplicate: Value = serde_json::from_str(&original.to_json().unwrap()).unwrap();
        let entry = duplicate["observed_state"]["entries"][0].clone();
        duplicate["observed_state"]["entries"] = json!([entry.clone(), entry]);
        assert!(DeclarativeContextSituationDocument::from_json(&duplicate.to_string()).is_err());

        let mut lineage: Value = serde_json::from_str(&original.to_json().unwrap()).unwrap();
        lineage["observed_state"]["entries"][0]["lineage"]["facts"] = json!([]);
        assert!(DeclarativeContextSituationDocument::from_json(&lineage.to_string()).is_err());
    }

    #[test]
    fn base_situation_without_snapshot_reference_remains_a_valid_empty_boundary() {
        let situation = Situation::new_v1(SituationId::new("empty-situation").unwrap());
        let json = situation.to_json().unwrap();
        assert_eq!(Situation::from_json(&json).unwrap(), situation);
        assert!(
            Situation::from_json(&json.replace(
                "\"observed_state_id\":null",
                "\"observed_state_id\":null,\"risks\":[{}]"
            ))
            .is_err()
        );
        assert_eq!(ContextScopeId::new("scope").unwrap().as_str(), "scope");
    }

    #[test]
    fn wire_contract_covers_all_supported_variants_and_optional_paths() {
        for value in [
            "CANONICAL_REFERENCE",
            "OBSERVED_EVIDENCE",
            "RETRIEVED_CONTENT",
            "CALLER_INPUT",
            "DERIVED_ASSESSMENT",
            "SYNTHETIC_DATA",
            "MIXED",
        ] {
            assert!(parse_trust(value).is_ok());
        }
        assert!(parse_trust("INVALID").is_err());
        for value in ["PUBLIC", "NORMAL", "INTERNAL", "CONFIDENTIAL", "SECRET"] {
            assert!(parse_sensitivity(value).is_ok());
        }
        assert!(parse_sensitivity("INVALID").is_err());
        for value in ["FRESH", "STALE", "UNKNOWN"] {
            assert!(parse_freshness(value).is_ok());
        }
        assert!(parse_freshness("INVALID").is_err());
        for value in ["NONE", "INCOMPLETE", "PROBABILISTIC", "UNKNOWN"] {
            assert!(parse_uncertainty(value).is_ok());
        }
        assert!(parse_uncertainty("INVALID").is_err());
        for value in ["NONE", "UNRESOLVED"] {
            assert!(parse_conflict(value).is_ok());
        }
        assert!(parse_conflict("INVALID").is_err());
        for value in ["SUPPORTS", "CHALLENGES"] {
            assert!(parse_evidence_relation(value).is_ok());
        }
        assert!(parse_evidence_relation("INVALID").is_err());
        for value in ["KNOWN", "UNKNOWN", "CONFLICTED", "UNSUPPORTED"] {
            assert!(parse_state_status(value).is_ok());
        }
        assert!(parse_state_status("INVALID").is_err());
        for value in [
            "UNKNOWN_STATE",
            "CONFLICTING_ASSERTIONS",
            "INCOMPATIBLE_VALUE_TYPES",
            "MISSING_EVIDENCE",
        ] {
            assert!(parse_normalization_reason(value).is_ok());
        }
        assert!(parse_normalization_reason("INVALID").is_err());
        for value in [
            "QUALITY",
            "ARCHITECTURE",
            "COVERAGE",
            "DEPENDENCY",
            "DATA_QUALITY",
            "OPERATIONAL",
            "SECURITY",
        ] {
            assert!(parse_assessment_kind(value).is_ok());
        }
        assert!(parse_assessment_kind("INVALID").is_err());
        for value in ["POSITIVE", "AT_RISK", "NEGATIVE", "UNKNOWN"] {
            assert!(parse_assessment_conclusion(value).is_ok());
        }
        assert!(parse_assessment_conclusion("INVALID").is_err());
        for value in ["DETERMINED", "UNRESOLVED", "PROPOSED"] {
            assert!(parse_assessment_status(value).is_ok());
        }
        assert!(parse_assessment_status("INVALID").is_err());
        for value in [
            "QUALITY",
            "ARCHITECTURE",
            "DEPENDENCY",
            "SECURITY",
            "OPERATIONAL",
            "DATA_QUALITY",
        ] {
            assert!(parse_risk_category(value).is_ok());
        }
        assert!(parse_risk_category("INVALID").is_err());
        for value in [
            "UNKNOWN",
            "INFORMATIONAL",
            "LOW",
            "MEDIUM",
            "HIGH",
            "CRITICAL",
        ] {
            assert!(parse_risk_severity(value).is_ok());
        }
        assert!(parse_risk_severity("INVALID").is_err());
        for value in ["OPEN", "UNRESOLVED", "UNKNOWN"] {
            assert!(parse_risk_status(value).is_ok());
        }
        assert!(parse_risk_status("INVALID").is_err());
        for value in ["RARE", "POSSIBLE", "LIKELY"] {
            assert!(parse_likelihood(value).is_ok());
        }
        assert!(parse_likelihood("INVALID").is_err());
        for value in [
            "UNKNOWN_STATE",
            "STATE_CONFLICT",
            "UNSUPPORTED_STATE",
            "UNRESOLVED_ASSESSMENT",
            "UNKNOWN_RISK",
            "DATA_QUALITY",
        ] {
            assert!(parse_situation_diagnostic(value).is_ok());
        }
        assert!(parse_situation_diagnostic("INVALID").is_err());

        assert!(
            WireTypedValue::Decimal(WireDecimal {
                unscaled: "not-an-integer".to_owned(),
                scale: 2,
            })
            .into_domain()
            .is_err()
        );

        let source_records = records();
        let mut provenance = WireProvenance::from_domain(&source_records.provenances()[0]);
        provenance.producer = None;
        provenance.acquired_at = None;
        provenance.source_timestamp = None;
        assert!(provenance.into_domain().is_ok());
        let mut observation = WireObservation::from_domain(&source_records.observations()[0]);
        observation.occurred_at = None;
        assert!(observation.into_domain().is_ok());
        let mut evidence = WireEvidence::from_domain(&source_records.evidence()[0]);
        evidence.occurred_at = None;
        assert!(evidence.into_domain().is_ok());
        assert!(
            WireEvidenceContent::Inline("inline evidence".to_owned())
                .into_domain()
                .is_ok()
        );
        assert!(
            WireEvidenceContent::from_domain(&EvidenceContent::inline("inline").unwrap())
                .into_domain()
                .is_ok()
        );

        for confidence in [
            Confidence::Unknown,
            Confidence::NotApplicable,
            Confidence::score(0.5).unwrap(),
        ] {
            assert!(
                WireConfidence::from_domain(&confidence)
                    .into_domain()
                    .is_ok()
            );
        }

        let intent_value = intent();
        let mut no_input = WireIntent::from_domain(&intent_value);
        no_input.original_input = None;
        assert!(no_input.into_domain().is_ok());
        let mut reference_input = WireIntent::from_domain(&intent_value);
        reference_input.original_input = Some(WireOriginalInput::Reference(
            ReferenceId::new("intent-input").unwrap(),
        ));
        assert!(reference_input.into_domain().is_ok());
        let mut future_intent = WireIntent::from_domain(&intent_value);
        future_intent.schema_version = DeclarativeContextVersion::new(2, 0).unwrap();
        assert!(future_intent.into_domain().is_err());

        let rule = AssessmentRuleContract::new(
            AssessmentRuleId::new("rule-with-digest").unwrap(),
            AssessmentRuleVersion::V1,
        )
        .unwrap()
        .with_semantic_digest(ContentDigest::new("b".repeat(64)).unwrap());
        assert!(WireRuleContract::from_domain(&rule).into_domain().is_ok());

        let (base_situation, state, _) = situation();
        let basis = BasisReferences::from_state_entry(&state.entries()[0]).unwrap();
        let external_assessment = Assessment::new(
            AssessmentId::new("assessment-external").unwrap(),
            AssessmentKind::Quality,
            AssessmentConclusion::Positive,
            AssessmentStatus::Proposed,
            ReasonCode::new("EXTERNAL_ASSESSMENT").unwrap(),
            "external assessment",
            basis.clone(),
            AssessmentOrigin::External {
                source_kind: SourceKind::Repository,
                provenance: ProvenanceId::new("prov-1").unwrap(),
            },
            state.entries()[0].metadata().unwrap(),
        )
        .unwrap();
        assert!(
            WireAssessment::from_domain(&external_assessment)
                .into_domain()
                .is_ok()
        );

        for likelihood in [
            RiskLikelihood::Unknown,
            RiskLikelihood::Qualitative(QualitativeLikelihood::Likely),
            RiskLikelihood::probability(0.75).unwrap(),
        ] {
            assert!(
                WireRiskLikelihood::from_domain(likelihood)
                    .into_domain()
                    .is_ok()
            );
        }
        let deterministic_origin = RiskOrigin::Deterministic {
            rule: AssessmentRuleContract::new(
                AssessmentRuleId::new("risk-rule").unwrap(),
                AssessmentRuleVersion::V1,
            )
            .unwrap(),
        };
        let external_origin = RiskOrigin::External {
            source_kind: SourceKind::Repository,
            provenance: ProvenanceId::new("prov-1").unwrap(),
        };
        assert!(
            WireRiskOrigin::from_domain(&deterministic_origin)
                .into_domain()
                .is_ok()
        );
        assert!(
            WireRiskOrigin::from_domain(&external_origin)
                .into_domain()
                .is_ok()
        );
        let risk = Risk::new(
            RiskId::new("risk-external").unwrap(),
            RiskCategory::Operational,
            RiskSeverity::Critical,
            RiskLikelihood::Unknown,
            RiskStatus::Unknown,
            ReasonCode::new("EXTERNAL_RISK").unwrap(),
            "external risk",
            basis,
            external_origin,
            state.entries()[0].metadata().unwrap(),
        )
        .unwrap();
        assert!(WireRisk::from_domain(&risk).into_domain().is_ok());

        let diagnostic = SituationDiagnostic::new(
            SituationDiagnosticCode::DataQuality,
            "quality requires review",
            BasisReferences::new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
                .unwrap(),
        )
        .unwrap();
        assert!(
            WireSituationDiagnostic::from_domain(&diagnostic)
                .into_domain()
                .is_ok()
        );
        let runtime_reference = SituationReference::Runtime {
            runtime: ExecutionRuntimeId::new("runtime-1").unwrap(),
            reference: ReferenceId::new("run-1").unwrap(),
        };
        assert_eq!(
            WireSituationReference::from_domain(&runtime_reference).into_domain(),
            runtime_reference
        );

        let normalization_diagnostic = NormalizationDiagnostic::from_parts(
            NormalizationReasonCode::MissingEvidence,
            SubjectPath::from_str("coverage.percent").unwrap(),
            vec![FactId::new("fact-1").unwrap()],
        )
        .unwrap();
        assert!(
            WireNormalizationDiagnostic::from_domain(&normalization_diagnostic)
                .into_domain()
                .is_ok()
        );
        let mut observed_wire = WireObservedState::from_domain(&state);
        let diagnostic_wire = WireNormalizationDiagnostic::from_domain(&normalization_diagnostic);
        observed_wire.diagnostics = vec![diagnostic_wire.clone(), diagnostic_wire];
        assert!(observed_wire.into_domain().is_err());

        let empty = Situation::new_v1(SituationId::new("empty-wire").unwrap());
        let mut duplicate_diagnostics = WireSituation::from_domain(&empty);
        let situation_diagnostic = WireSituationDiagnostic::from_domain(&diagnostic);
        duplicate_diagnostics.diagnostics =
            vec![situation_diagnostic.clone(), situation_diagnostic];
        assert!(duplicate_diagnostics.into_domain().is_err());
        let mut duplicate_references = WireSituation::from_domain(&empty);
        let external_reference = WireSituationReference::External {
            source: SourceId::new("source").unwrap(),
            reference: ReferenceId::new("reference").unwrap(),
        };
        duplicate_references.references = vec![external_reference.clone(), external_reference];
        assert!(duplicate_references.into_domain().is_err());
        let missing_snapshot = WireSituation {
            schema_version: DECLARATIVE_CONTEXT_IR_VERSION,
            id: SituationId::new("missing-snapshot").unwrap(),
            observed_state_id: None,
            assessments: Vec::new(),
            risks: Vec::new(),
            diagnostics: Vec::new(),
            references: vec![WireSituationReference::External {
                source: SourceId::new("source").unwrap(),
                reference: ReferenceId::new("reference").unwrap(),
            }],
        };
        assert!(missing_snapshot.into_domain().is_err());

        let original_document = document();
        let (context, intent_value, records_value, state_value, _) = (
            original_document.context().clone(),
            original_document.intent().cloned(),
            original_document.records().cloned(),
            original_document.observed_state().clone(),
            original_document.situation().clone(),
        );
        assert!(
            DeclarativeContextSituationDocument::new(
                context,
                intent_value,
                records_value,
                state_value,
                Situation::new_v1(SituationId::new("different-situation").unwrap()),
            )
            .is_err()
        );
        let json = original_document.to_json().unwrap();
        assert_eq!(
            serde_json::from_str::<DeclarativeContextSituationDocument>(&json).unwrap(),
            original_document
        );
        assert_eq!(base_situation.id(), original_document.situation().id());

        let input = NormalizationInput::new(source_records);
        assert!(input.records().facts().len() == 1);
        assert!(input.unknown_subjects().is_empty());
        assert!(!input.requires_evidence());
        assert!(input.quality_metadata().is_empty());

        let fact_id = FactId::new("normalized-fact").unwrap();
        let claim = NormalizedClaim::from_parts(
            fact_id.clone(),
            TypedValue::Integer(1),
            AssertionPolarity::Affirmed,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        let empty_lineage =
            StateLineage::from_parts(Vec::new(), Vec::new(), Vec::new(), Vec::new()).unwrap();
        assert!(
            NormalizedClaim::from_parts(
                fact_id.clone(),
                TypedValue::Integer(1),
                AssertionPolarity::Affirmed,
                Vec::new(),
                Vec::new(),
                vec![EvidenceId::new("duplicate-evidence").unwrap(); 2],
                Vec::new(),
            )
            .is_err()
        );
        assert!(
            NormalizedClaim::from_parts(
                fact_id.clone(),
                TypedValue::Integer(1),
                AssertionPolarity::Affirmed,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                vec![EvidenceId::new("duplicate-evidence").unwrap(); 2],
            )
            .is_err()
        );
        let duplicate_claim_lineage =
            StateLineage::from_parts(vec![fact_id.clone()], Vec::new(), Vec::new(), Vec::new())
                .unwrap();
        assert!(
            NormalizedStateEntry::from_parts(
                SubjectPath::from_str("duplicate.claim").unwrap(),
                StateStatus::Known,
                Some(TypedValue::Integer(1)),
                Some(AssertionPolarity::Affirmed),
                vec![claim.clone(), claim.clone()],
                duplicate_claim_lineage.clone(),
                Vec::new(),
                None,
            )
            .is_err()
        );
        assert!(
            NormalizedStateEntry::from_parts(
                SubjectPath::from_str("duplicate.diagnostic").unwrap(),
                StateStatus::Unknown,
                None,
                None,
                Vec::new(),
                empty_lineage.clone(),
                vec![
                    NormalizationReasonCode::UnknownState,
                    NormalizationReasonCode::UnknownState,
                ],
                None,
            )
            .is_err()
        );
        assert!(
            NormalizedStateEntry::from_parts(
                SubjectPath::from_str("invalid.known").unwrap(),
                StateStatus::Known,
                None,
                None,
                Vec::new(),
                empty_lineage.clone(),
                Vec::new(),
                None,
            )
            .is_err()
        );
        assert!(
            NormalizedStateEntry::from_parts(
                SubjectPath::from_str("invalid.unknown").unwrap(),
                StateStatus::Unknown,
                None,
                None,
                vec![claim],
                duplicate_claim_lineage,
                Vec::new(),
                None,
            )
            .is_err()
        );
        assert!(
            StateLineage::from_parts(
                vec![FactId::new("duplicate-lineage").unwrap(); 2],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .is_err()
        );
        assert!(
            NormalizationDiagnostic::from_parts(
                NormalizationReasonCode::MissingEvidence,
                SubjectPath::from_str("duplicate.diagnostic").unwrap(),
                vec![FactId::new("duplicate-fact").unwrap(); 2],
            )
            .is_err()
        );
    }
}
