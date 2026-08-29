//! CG-06.06 deterministic situation assembly and explainability.
//!
//! Assessments and risks are derived interpretations.  They retain their
//! explicit basis and quality metadata, while the underlying normalized facts
//! remain owned by [`crate::normalization::ObservedState`] and are never
//! replaced by a derived conclusion.

use std::{collections::BTreeSet, fmt, str::FromStr};

use crate::{
    declarative_context::{DeclarativeContextVersion, ObservedState, Situation},
    identifiers::{
        AssessmentId, AssessmentRuleId, EvidenceId, ExecutionRuntimeId, FactId, ProvenanceId,
        ReferenceId, RiskId, SituationId, SourceId,
    },
    normalization::{NormalizedStateEntry, StateStatus},
    observation::{ObservationEvidenceSet, SourceKind},
    quality::{ConflictStatus, FreshnessStatus, QualityMetadata, SensitivityClass, Uncertainty},
    task::TaskConfidence,
    validation::{NonEmptyText, ValidationError, validate_identifier},
    version::SchemaVersion,
};

/// The currently supported deterministic assessment-rule contract version.
pub const ASSESSMENT_RULE_VERSION: AssessmentRuleVersion = AssessmentRuleVersion::V1;

/// A version of a deterministic assessment rule contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct AssessmentRuleVersion(SchemaVersion);

impl AssessmentRuleVersion {
    /// The first supported rule contract version.
    pub const V1: Self = Self(SchemaVersion::V1);

    /// Creates a syntactically valid rule version.
    pub fn new(major: u16, minor: u16) -> Result<Self, ValidationError> {
        SchemaVersion::new(major, minor).map(Self)
    }

    /// Returns the major component.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.0.major()
    }

    /// Returns the minor component.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.0.minor()
    }

    /// Rejects rule versions that this implementation does not understand.
    pub fn ensure_supported(self) -> Result<(), ValidationError> {
        if self == Self::V1 {
            Ok(())
        } else {
            Err(ValidationError::UnsupportedSchemaVersion {
                expected: "1.0",
                actual: self.to_string(),
            })
        }
    }
}

impl fmt::Display for AssessmentRuleVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for AssessmentRuleVersion {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        SchemaVersion::from_str(value).map(Self)
    }
}

/// The stable identity and compatibility contract for one deterministic rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct AssessmentRuleContract {
    id: AssessmentRuleId,
    version: AssessmentRuleVersion,
    semantic_digest: Option<crate::observation::ContentDigest>,
}

impl AssessmentRuleContract {
    /// Creates a supported rule contract without a data-driven semantic digest.
    pub fn new(
        id: AssessmentRuleId,
        version: AssessmentRuleVersion,
    ) -> Result<Self, ValidationError> {
        version.ensure_supported()?;
        Ok(Self {
            id,
            version,
            semantic_digest: None,
        })
    }

    /// Adds the digest of a data-driven rule definition.
    #[must_use]
    pub fn with_semantic_digest(
        mut self,
        semantic_digest: crate::observation::ContentDigest,
    ) -> Self {
        self.semantic_digest = Some(semantic_digest);
        self
    }

    /// Returns the stable rule identity.
    #[must_use]
    pub fn id(&self) -> &AssessmentRuleId {
        &self.id
    }

    /// Returns the compatibility version.
    #[must_use]
    pub const fn version(&self) -> AssessmentRuleVersion {
        self.version
    }

    /// Returns the optional rule-definition digest.
    #[must_use]
    pub fn semantic_digest(&self) -> Option<&crate::observation::ContentDigest> {
        self.semantic_digest.as_ref()
    }
}

/// A validated stable machine-readable reason code.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ReasonCode(String);

/// Alias for callers that distinguish assessment reason terminology.
pub type AssessmentReasonCode = ReasonCode;

/// Alias for callers that distinguish risk reason terminology.
pub type RiskReasonCode = ReasonCode;

impl ReasonCode {
    /// Creates a reason code without normalizing its spelling.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        Ok(Self(validate_identifier(value)?))
    }

    /// Returns the stable machine-readable code.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ReasonCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ReasonCode {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

/// Canonical references used as the basis of a derived result.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct BasisReferences {
    state_subjects: Vec<crate::intent::SubjectPath>,
    facts: Vec<FactId>,
    evidence: Vec<EvidenceId>,
    provenances: Vec<ProvenanceId>,
    assessments: Vec<AssessmentId>,
}

/// Alias emphasizing an assessment's explicit basis.
pub type AssessmentBasis = BasisReferences;

/// Alias emphasizing a risk's explicit basis.
pub type RiskBasis = BasisReferences;

impl BasisReferences {
    /// Creates canonical basis references.  An empty basis is only useful for
    /// a standalone diagnostic; assessments and risks reject it.
    pub fn new(
        mut state_subjects: Vec<crate::intent::SubjectPath>,
        mut facts: Vec<FactId>,
        mut evidence: Vec<EvidenceId>,
        mut provenances: Vec<ProvenanceId>,
        mut assessments: Vec<AssessmentId>,
    ) -> Result<Self, ValidationError> {
        sort_unique(&mut state_subjects, "basis.state_subjects")?;
        sort_unique(&mut facts, "basis.facts")?;
        sort_unique(&mut evidence, "basis.evidence")?;
        sort_unique(&mut provenances, "basis.provenances")?;
        sort_unique(&mut assessments, "basis.assessments")?;
        Ok(Self {
            state_subjects,
            facts,
            evidence,
            provenances,
            assessments,
        })
    }

    /// Creates a basis from one normalized state entry's complete lineage.
    pub fn from_state_entry(entry: &NormalizedStateEntry) -> Result<Self, ValidationError> {
        Self::new(
            vec![entry.subject().clone()],
            entry.lineage().facts().to_vec(),
            entry.lineage().evidence().to_vec(),
            entry.lineage().provenances().to_vec(),
            Vec::new(),
        )
    }

    /// Returns whether no input reference was supplied.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.state_subjects.is_empty()
            && self.facts.is_empty()
            && self.evidence.is_empty()
            && self.provenances.is_empty()
            && self.assessments.is_empty()
    }

    /// Returns normalized state subjects.
    #[must_use]
    pub fn state_subjects(&self) -> &[crate::intent::SubjectPath] {
        &self.state_subjects
    }

    /// Returns normalized fact identities.
    #[must_use]
    pub fn facts(&self) -> &[FactId] {
        &self.facts
    }

    /// Returns evidence identities.
    #[must_use]
    pub fn evidence(&self) -> &[EvidenceId] {
        &self.evidence
    }

    /// Returns provenance identities.
    #[must_use]
    pub fn provenances(&self) -> &[ProvenanceId] {
        &self.provenances
    }

    /// Returns assessment identities.
    #[must_use]
    pub fn assessments(&self) -> &[AssessmentId] {
        &self.assessments
    }
}

/// The broad semantic category of an assessment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum AssessmentKind {
    Quality,
    Architecture,
    Coverage,
    Dependency,
    DataQuality,
    Operational,
    Security,
}

impl AssessmentKind {
    /// Returns the stable machine-readable category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Quality => "QUALITY",
            Self::Architecture => "ARCHITECTURE",
            Self::Coverage => "COVERAGE",
            Self::Dependency => "DEPENDENCY",
            Self::DataQuality => "DATA_QUALITY",
            Self::Operational => "OPERATIONAL",
            Self::Security => "SECURITY",
        }
    }
}

/// The deterministic conclusion of an assessment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum AssessmentConclusion {
    Positive,
    AtRisk,
    Negative,
    Unknown,
}

impl AssessmentConclusion {
    /// Returns the stable machine-readable conclusion.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Positive => "POSITIVE",
            Self::AtRisk => "AT_RISK",
            Self::Negative => "NEGATIVE",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Lifecycle-neutral status of an assessment result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum AssessmentStatus {
    Determined,
    Unresolved,
    Proposed,
}

impl AssessmentStatus {
    /// Returns the stable machine-readable status.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Determined => "DETERMINED",
            Self::Unresolved => "UNRESOLVED",
            Self::Proposed => "PROPOSED",
        }
    }
}

/// How a derived assessment entered the situation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum AssessmentOrigin {
    Deterministic {
        rule: AssessmentRuleContract,
    },
    External {
        source_kind: SourceKind,
        provenance: ProvenanceId,
    },
}

impl AssessmentOrigin {
    /// Returns the deterministic rule, if this is a rule-derived result.
    #[must_use]
    pub fn rule(&self) -> Option<&AssessmentRuleContract> {
        match self {
            Self::Deterministic { rule } => Some(rule),
            Self::External { .. } => None,
        }
    }

    /// Returns the external source class, if applicable.
    #[must_use]
    pub const fn source_kind(&self) -> Option<SourceKind> {
        match self {
            Self::Deterministic { .. } => None,
            Self::External { source_kind, .. } => Some(*source_kind),
        }
    }

    /// Returns the external provenance, if applicable.
    #[must_use]
    pub fn provenance(&self) -> Option<&ProvenanceId> {
        match self {
            Self::Deterministic { .. } => None,
            Self::External { provenance, .. } => Some(provenance),
        }
    }
}

/// A derived interpretation that never replaces its observed basis.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Assessment {
    id: AssessmentId,
    kind: AssessmentKind,
    conclusion: AssessmentConclusion,
    status: AssessmentStatus,
    reason: AssessmentReasonCode,
    summary: NonEmptyText,
    basis: AssessmentBasis,
    origin: AssessmentOrigin,
    quality: QualityMetadata,
}

impl Assessment {
    /// Creates a validated derived assessment.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: AssessmentId,
        kind: AssessmentKind,
        conclusion: AssessmentConclusion,
        status: AssessmentStatus,
        reason: AssessmentReasonCode,
        summary: impl Into<String>,
        basis: AssessmentBasis,
        origin: AssessmentOrigin,
        quality: QualityMetadata,
    ) -> Result<Self, ValidationError> {
        if basis.is_empty() {
            return Err(ValidationError::InvalidDeclarativeValue {
                reason: "assessment must retain at least one basis reference",
            });
        }
        Ok(Self {
            id,
            kind,
            conclusion,
            status,
            reason,
            summary: NonEmptyText::new_for_field(summary, "assessment_summary")?,
            basis,
            origin,
            quality,
        })
    }

    /// Returns the assessment identity.
    #[must_use]
    pub fn id(&self) -> &AssessmentId {
        &self.id
    }

    /// Returns the assessment category.
    #[must_use]
    pub const fn kind(&self) -> AssessmentKind {
        self.kind
    }

    /// Returns the derived conclusion.
    #[must_use]
    pub const fn conclusion(&self) -> AssessmentConclusion {
        self.conclusion
    }

    /// Returns the lifecycle-neutral result status.
    #[must_use]
    pub const fn status(&self) -> AssessmentStatus {
        self.status
    }

    /// Returns the stable reason code.
    #[must_use]
    pub fn reason(&self) -> &AssessmentReasonCode {
        &self.reason
    }

    /// Returns the human-readable result summary.
    #[must_use]
    pub fn summary(&self) -> &str {
        self.summary.as_str()
    }

    /// Returns explicit state/fact/evidence/provenance basis references.
    #[must_use]
    pub const fn basis(&self) -> &AssessmentBasis {
        &self.basis
    }

    /// Returns the rule or external source classification.
    #[must_use]
    pub const fn origin(&self) -> &AssessmentOrigin {
        &self.origin
    }

    /// Returns propagated quality metadata.
    #[must_use]
    pub const fn quality(&self) -> QualityMetadata {
        self.quality
    }
}

/// A risk's semantic category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum RiskCategory {
    Quality,
    Architecture,
    Dependency,
    Security,
    Operational,
    DataQuality,
}

impl RiskCategory {
    /// Returns the stable machine-readable category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Quality => "QUALITY",
            Self::Architecture => "ARCHITECTURE",
            Self::Dependency => "DEPENDENCY",
            Self::Security => "SECURITY",
            Self::Operational => "OPERATIONAL",
            Self::DataQuality => "DATA_QUALITY",
        }
    }
}

/// Qualitative likelihood that avoids invented numeric precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum QualitativeLikelihood {
    Rare,
    Possible,
    Likely,
}

impl QualitativeLikelihood {
    /// Returns the stable machine-readable likelihood.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rare => "RARE",
            Self::Possible => "POSSIBLE",
            Self::Likely => "LIKELY",
        }
    }
}

/// Explicit risk likelihood.  Probability is accepted only when supplied as
/// a bounded value by the caller or a documented deterministic rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum RiskLikelihood {
    Unknown,
    Qualitative(QualitativeLikelihood),
    ExplicitProbability(TaskConfidence),
}

impl RiskLikelihood {
    /// Creates an explicit bounded probability supplied by the caller.
    pub fn probability(value: f64) -> Result<Self, ValidationError> {
        Ok(Self::ExplicitProbability(TaskConfidence::new(value)?))
    }

    /// Returns the stable machine-readable likelihood category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Qualitative(QualitativeLikelihood::Rare) => "RARE",
            Self::Qualitative(QualitativeLikelihood::Possible) => "POSSIBLE",
            Self::Qualitative(QualitativeLikelihood::Likely) => "LIKELY",
            Self::ExplicitProbability(_) => "EXPLICIT_PROBABILITY",
        }
    }

    /// Returns the explicit probability, if one was supplied.
    #[must_use]
    pub const fn probability_value(self) -> Option<TaskConfidence> {
        match self {
            Self::ExplicitProbability(value) => Some(value),
            Self::Unknown | Self::Qualitative(_) => None,
        }
    }
}

/// Risk severity without implying an authorization or process decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum RiskSeverity {
    Unknown,
    Informational,
    Low,
    Medium,
    High,
    Critical,
}

impl RiskSeverity {
    /// Returns the stable machine-readable severity.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Informational => "INFORMATIONAL",
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
            Self::Critical => "CRITICAL",
        }
    }
}

/// Lifecycle-neutral risk state exposed in the situation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum RiskStatus {
    Open,
    Unresolved,
    Unknown,
}

impl RiskStatus {
    /// Returns the stable machine-readable status.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Unresolved => "UNRESOLVED",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// How a risk was derived or proposed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum RiskOrigin {
    Deterministic {
        rule: AssessmentRuleContract,
    },
    AssessmentDerived,
    External {
        source_kind: SourceKind,
        provenance: ProvenanceId,
    },
}

/// A derived risk with explicit likelihood semantics and basis lineage.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Risk {
    id: RiskId,
    category: RiskCategory,
    severity: RiskSeverity,
    likelihood: RiskLikelihood,
    status: RiskStatus,
    reason: RiskReasonCode,
    summary: NonEmptyText,
    basis: RiskBasis,
    origin: RiskOrigin,
    quality: QualityMetadata,
}

impl Risk {
    /// Creates a validated risk.  No numeric probability is inferred.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: RiskId,
        category: RiskCategory,
        severity: RiskSeverity,
        likelihood: RiskLikelihood,
        status: RiskStatus,
        reason: RiskReasonCode,
        summary: impl Into<String>,
        basis: RiskBasis,
        origin: RiskOrigin,
        quality: QualityMetadata,
    ) -> Result<Self, ValidationError> {
        if basis.is_empty() {
            return Err(ValidationError::InvalidDeclarativeValue {
                reason: "risk must retain at least one basis reference",
            });
        }
        Ok(Self {
            id,
            category,
            severity,
            likelihood,
            status,
            reason,
            summary: NonEmptyText::new_for_field(summary, "risk_summary")?,
            basis,
            origin,
            quality,
        })
    }

    /// Returns the risk identity.
    #[must_use]
    pub fn id(&self) -> &RiskId {
        &self.id
    }

    /// Returns the risk category.
    #[must_use]
    pub const fn category(&self) -> RiskCategory {
        self.category
    }

    /// Returns severity.
    #[must_use]
    pub const fn severity(&self) -> RiskSeverity {
        self.severity
    }

    /// Returns the explicit likelihood.
    #[must_use]
    pub const fn likelihood(&self) -> RiskLikelihood {
        self.likelihood
    }

    /// Returns lifecycle-neutral risk status.
    #[must_use]
    pub const fn status(&self) -> RiskStatus {
        self.status
    }

    /// Returns the stable reason code.
    #[must_use]
    pub fn reason(&self) -> &RiskReasonCode {
        &self.reason
    }

    /// Returns the human-readable risk summary.
    #[must_use]
    pub fn summary(&self) -> &str {
        self.summary.as_str()
    }

    /// Returns explicit basis references.
    #[must_use]
    pub const fn basis(&self) -> &RiskBasis {
        &self.basis
    }

    /// Returns risk derivation origin.
    #[must_use]
    pub const fn origin(&self) -> &RiskOrigin {
        &self.origin
    }

    /// Returns propagated quality metadata.
    #[must_use]
    pub const fn quality(&self) -> QualityMetadata {
        self.quality
    }
}

/// Stable diagnostic categories retained in the situation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum SituationDiagnosticCode {
    UnknownState,
    StateConflict,
    UnsupportedState,
    UnresolvedAssessment,
    UnknownRisk,
    DataQuality,
}

impl SituationDiagnosticCode {
    /// Returns the stable machine-readable diagnostic code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownState => "UNKNOWN_STATE",
            Self::StateConflict => "STATE_CONFLICT",
            Self::UnsupportedState => "UNSUPPORTED_STATE",
            Self::UnresolvedAssessment => "UNRESOLVED_ASSESSMENT",
            Self::UnknownRisk => "UNKNOWN_RISK",
            Self::DataQuality => "DATA_QUALITY",
        }
    }
}

/// An unresolved conflict, question or data-quality condition.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SituationDiagnostic {
    code: SituationDiagnosticCode,
    summary: NonEmptyText,
    basis: BasisReferences,
}

impl SituationDiagnostic {
    /// Creates a retained diagnostic; diagnostics may be basis-free when the
    /// question itself is the only available information.
    pub fn new(
        code: SituationDiagnosticCode,
        summary: impl Into<String>,
        basis: BasisReferences,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            code,
            summary: NonEmptyText::new_for_field(summary, "situation_diagnostic")?,
            basis,
        })
    }

    /// Returns the machine-readable code.
    #[must_use]
    pub const fn code(&self) -> SituationDiagnosticCode {
        self.code
    }

    /// Returns the human-readable diagnostic summary.
    #[must_use]
    pub fn summary(&self) -> &str {
        self.summary.as_str()
    }

    /// Returns diagnostic basis references.
    #[must_use]
    pub const fn basis(&self) -> &BasisReferences {
        &self.basis
    }
}

/// A reference to external or runtime state without embedding provider data.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum SituationReference {
    External {
        source: SourceId,
        reference: ReferenceId,
    },
    Runtime {
        runtime: ExecutionRuntimeId,
        reference: ReferenceId,
    },
}

/// One human-readable projection derived from the same result and basis.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ExplainabilityTrace {
    item: ExplainabilityItem,
    reason: ReasonCode,
    summary: String,
    basis: BasisReferences,
}

/// The derived item described by an explainability trace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ExplainabilityItem {
    Assessment(AssessmentId),
    Risk(RiskId),
}

impl ExplainabilityTrace {
    pub(crate) fn for_assessment(assessment: &Assessment) -> Self {
        let summary = format!(
            "assessment {} ({}) is {} [{}]: {}",
            assessment.id(),
            assessment.kind().as_str(),
            assessment.conclusion().as_str(),
            assessment.status().as_str(),
            assessment.summary()
        );
        Self {
            item: ExplainabilityItem::Assessment(assessment.id().clone()),
            reason: assessment.reason().clone(),
            summary,
            basis: assessment.basis().clone(),
        }
    }

    pub(crate) fn for_risk(risk: &Risk) -> Self {
        let summary = format!(
            "risk {} ({}) is {} severity with {} likelihood [{}]: {}",
            risk.id(),
            risk.category().as_str(),
            risk.severity().as_str(),
            risk.likelihood().as_str(),
            risk.status().as_str(),
            risk.summary()
        );
        Self {
            item: ExplainabilityItem::Risk(risk.id().clone()),
            reason: risk.reason().clone(),
            summary,
            basis: risk.basis().clone(),
        }
    }

    /// Returns the described assessment or risk identity.
    #[must_use]
    pub const fn item(&self) -> &ExplainabilityItem {
        &self.item
    }

    /// Returns the machine-readable reason.
    #[must_use]
    pub fn reason(&self) -> &ReasonCode {
        &self.reason
    }

    /// Returns the human-readable explanation.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Returns the exact basis used by the explained result.
    #[must_use]
    pub const fn basis(&self) -> &BasisReferences {
        &self.basis
    }
}

/// Explicit inputs to deterministic situation assembly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SituationAssemblyInput {
    observed_state: ObservedState,
    records: Option<ObservationEvidenceSet>,
    assessments: Vec<Assessment>,
    risks: Vec<Risk>,
    diagnostics: Vec<SituationDiagnostic>,
    references: Vec<SituationReference>,
}

impl SituationAssemblyInput {
    /// Starts an assembly from a normalized observed-state snapshot.
    #[must_use]
    pub fn new(observed_state: ObservedState) -> Self {
        Self {
            observed_state,
            records: None,
            assessments: Vec::new(),
            risks: Vec::new(),
            diagnostics: Vec::new(),
            references: Vec::new(),
        }
    }

    /// Adds the explicit evidence/provenance graph used by external proposals.
    #[must_use]
    pub fn with_records(mut self, records: ObservationEvidenceSet) -> Self {
        self.records = Some(records);
        self
    }

    /// Adds canonical assessment results.
    pub fn with_assessments(
        mut self,
        mut assessments: Vec<Assessment>,
    ) -> Result<Self, ValidationError> {
        assessments.sort_by(|left, right| left.id.cmp(&right.id));
        ensure_unique_ids(&assessments, "assessment")?;
        self.assessments = assessments;
        Ok(self)
    }

    /// Adds canonical risk results.
    pub fn with_risks(mut self, mut risks: Vec<Risk>) -> Result<Self, ValidationError> {
        risks.sort_by(|left, right| left.id.cmp(&right.id));
        ensure_unique_ids(&risks, "risk")?;
        self.risks = risks;
        Ok(self)
    }

    /// Adds unresolved or data-quality diagnostics.
    pub fn with_diagnostics(
        mut self,
        mut diagnostics: Vec<SituationDiagnostic>,
    ) -> Result<Self, ValidationError> {
        diagnostics.sort();
        if diagnostics.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ValidationError::DuplicateRelationship {
                field: "situation.diagnostics",
            });
        }
        self.diagnostics = diagnostics;
        Ok(self)
    }

    /// Adds canonical external/runtime references.
    pub fn with_references(
        mut self,
        mut references: Vec<SituationReference>,
    ) -> Result<Self, ValidationError> {
        references.sort();
        if references.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ValidationError::DuplicateRelationship {
                field: "situation.references",
            });
        }
        self.references = references;
        Ok(self)
    }

    /// Returns the normalized input snapshot.
    #[must_use]
    pub const fn observed_state(&self) -> &ObservedState {
        &self.observed_state
    }

    /// Assembles a deterministic versioned situation.
    pub fn assemble(self, id: SituationId) -> Result<Situation, ValidationError> {
        assemble_situation(id, self)
    }
}

/// Assembles a situation without reading ambient time or provider state.
pub fn assemble_situation(
    id: SituationId,
    input: SituationAssemblyInput,
) -> Result<Situation, ValidationError> {
    input.observed_state.version().ensure_supported()?;
    if let Some(records) = &input.records {
        records.validate()?;
    }

    let assessment_ids = input
        .assessments
        .iter()
        .map(|assessment| assessment.id().clone())
        .collect::<BTreeSet<_>>();
    let mut known_subjects = BTreeSet::new();
    let mut known_facts = BTreeSet::new();
    let mut known_evidence = BTreeSet::new();
    let mut known_provenances = BTreeSet::new();
    for entry in input.observed_state.entries() {
        known_subjects.insert(entry.subject().clone());
        known_facts.extend(entry.lineage().facts().iter().cloned());
        known_evidence.extend(entry.lineage().evidence().iter().cloned());
        known_provenances.extend(entry.lineage().provenances().iter().cloned());
    }
    if let Some(records) = &input.records {
        known_facts.extend(records.facts().iter().map(|fact| fact.id().clone()));
        known_evidence.extend(
            records
                .evidence()
                .iter()
                .map(|evidence| evidence.id().clone()),
        );
        known_provenances.extend(
            records
                .provenances()
                .iter()
                .map(|provenance| provenance.id().clone()),
        );
    }

    for assessment in &input.assessments {
        validate_basis(
            assessment.basis(),
            &known_subjects,
            &known_facts,
            &known_evidence,
            &known_provenances,
            &BTreeSet::new(),
        )?;
        if !assessment.basis().assessments().is_empty() {
            return Err(ValidationError::InvalidDeclarativeValue {
                reason: "assessment basis must not reference derived assessments",
            });
        }
        if let AssessmentOrigin::External { provenance, .. } = assessment.origin() {
            ensure_known(
                known_provenances.contains(provenance),
                "provenance",
                provenance.to_string(),
            )?;
            if !assessment.basis().provenances().contains(provenance) {
                return Err(ValidationError::MissingDeclarativeIdentity {
                    kind: "assessment provenance basis",
                    id: provenance.to_string(),
                });
            }
        }
        ensure_quality_not_downgraded(assessment, &input.observed_state)?;
    }

    for risk in &input.risks {
        validate_basis(
            risk.basis(),
            &known_subjects,
            &known_facts,
            &known_evidence,
            &known_provenances,
            &assessment_ids,
        )?;
        if let RiskOrigin::External { provenance, .. } = risk.origin() {
            ensure_known(
                known_provenances.contains(provenance),
                "provenance",
                provenance.to_string(),
            )?;
            if !risk.basis().provenances().contains(provenance) {
                return Err(ValidationError::MissingDeclarativeIdentity {
                    kind: "risk provenance basis",
                    id: provenance.to_string(),
                });
            }
        }
        ensure_risk_quality(risk, &input.observed_state, &input.assessments)?;
    }

    let mut diagnostics = input.diagnostics;
    diagnostics.extend(derived_state_diagnostics(&input.observed_state)?);
    diagnostics.sort();
    diagnostics.dedup();

    let assessments = input.assessments;
    let risks = input.risks;
    let references = input.references;
    Ok(Situation::from_parts(
        DeclarativeContextVersion::V1,
        id,
        input.observed_state.id().clone(),
        assessments,
        risks,
        diagnostics,
        references,
    ))
}

fn validate_basis(
    basis: &BasisReferences,
    subjects: &BTreeSet<crate::intent::SubjectPath>,
    facts: &BTreeSet<FactId>,
    evidence: &BTreeSet<EvidenceId>,
    provenances: &BTreeSet<ProvenanceId>,
    assessments: &BTreeSet<AssessmentId>,
) -> Result<(), ValidationError> {
    if basis.is_empty() {
        return Err(ValidationError::InvalidDeclarativeValue {
            reason: "derived result must retain at least one basis reference",
        });
    }
    for subject in basis.state_subjects() {
        ensure_known(
            subjects.contains(subject),
            "state_subject",
            subject.to_string(),
        )?;
    }
    for fact in basis.facts() {
        ensure_known(facts.contains(fact), "fact", fact.to_string())?;
    }
    for evidence_id in basis.evidence() {
        ensure_known(
            evidence.contains(evidence_id),
            "evidence",
            evidence_id.to_string(),
        )?;
    }
    for provenance in basis.provenances() {
        ensure_known(
            provenances.contains(provenance),
            "provenance",
            provenance.to_string(),
        )?;
    }
    for assessment in basis.assessments() {
        ensure_known(
            assessments.contains(assessment),
            "assessment",
            assessment.to_string(),
        )?;
    }
    Ok(())
}

fn ensure_quality_not_downgraded(
    assessment: &Assessment,
    observed_state: &ObservedState,
) -> Result<(), ValidationError> {
    let source_sensitivity = observed_state
        .entries()
        .iter()
        .filter(|entry| {
            assessment
                .basis()
                .state_subjects()
                .contains(entry.subject())
        })
        .filter_map(NormalizedStateEntry::metadata)
        .map(QualityMetadata::sensitivity)
        .fold(SensitivityClass::Public, SensitivityClass::strongest);
    if assessment.quality().sensitivity() < source_sensitivity {
        return Err(ValidationError::InvalidDeclarativeValue {
            reason: "assessment sensitivity must not downgrade its state basis",
        });
    }
    Ok(())
}

fn ensure_risk_quality(
    risk: &Risk,
    observed_state: &ObservedState,
    assessments: &[Assessment],
) -> Result<(), ValidationError> {
    let mut source_quality = Vec::new();
    for entry in observed_state.entries() {
        if risk.basis().state_subjects().contains(entry.subject()) {
            if let Some(metadata) = entry.metadata() {
                source_quality.push(metadata);
            }
        }
    }
    for assessment_id in risk.basis().assessments() {
        if let Some(assessment) = assessments.iter().find(|item| item.id() == assessment_id) {
            source_quality.push(assessment.quality());
        }
    }
    let Some(expected) = QualityMetadata::merge(&source_quality) else {
        return Ok(());
    };
    let actual = risk.quality();
    if actual.sensitivity() < expected.sensitivity()
        || (expected.freshness() == FreshnessStatus::Stale
            && actual.freshness() != FreshnessStatus::Stale)
        || (expected.uncertainty() == Uncertainty::Unknown
            && actual.uncertainty() != Uncertainty::Unknown)
        || (expected.uncertainty() == Uncertainty::Incomplete
            && matches!(
                actual.uncertainty(),
                Uncertainty::None | Uncertainty::Probabilistic
            ))
        || (expected.conflict() == ConflictStatus::Unresolved
            && actual.conflict() != ConflictStatus::Unresolved)
    {
        return Err(ValidationError::InvalidDeclarativeValue {
            reason: "risk quality must preserve conservative basis metadata",
        });
    }
    Ok(())
}

fn derived_state_diagnostics(
    observed_state: &ObservedState,
) -> Result<Vec<SituationDiagnostic>, ValidationError> {
    let mut diagnostics = Vec::new();
    for entry in observed_state.entries() {
        let (code, summary) = match entry.status() {
            StateStatus::Known => continue,
            StateStatus::Unknown => (
                SituationDiagnosticCode::UnknownState,
                format!("state for {} is unknown", entry.subject()),
            ),
            StateStatus::Conflicted => (
                SituationDiagnosticCode::StateConflict,
                format!(
                    "state for {} contains unresolved conflicting claims",
                    entry.subject()
                ),
            ),
            StateStatus::Unsupported => (
                SituationDiagnosticCode::UnsupportedState,
                format!(
                    "state for {} lacks required support evidence",
                    entry.subject()
                ),
            ),
        };
        diagnostics.push(SituationDiagnostic::new(
            code,
            summary,
            BasisReferences::from_state_entry(entry)?,
        )?);
    }
    Ok(diagnostics)
}

fn sort_unique<T: Ord>(values: &mut [T], field: &'static str) -> Result<(), ValidationError> {
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ValidationError::DuplicateRelationship { field });
    }
    Ok(())
}

fn ensure_unique_ids<T>(values: &[T], kind: &'static str) -> Result<(), ValidationError>
where
    T: HasTypedId,
{
    for pair in values.windows(2) {
        if pair[0].typed_id() == pair[1].typed_id() {
            return Err(ValidationError::DuplicateDeclarativeIdentity {
                kind,
                id: pair[0].typed_id().to_owned(),
            });
        }
    }
    Ok(())
}

trait HasTypedId {
    fn typed_id(&self) -> &str;
}

impl HasTypedId for Assessment {
    fn typed_id(&self) -> &str {
        self.id().as_str()
    }
}

impl HasTypedId for Risk {
    fn typed_id(&self) -> &str {
        self.id().as_str()
    }
}

fn ensure_known(known: bool, kind: &'static str, id: String) -> Result<(), ValidationError> {
    if known {
        Ok(())
    } else {
        Err(ValidationError::MissingDeclarativeIdentity { kind, id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        declarative_context::ObservedState,
        identifiers::{
            AssessmentId, AssessmentRuleId, EvidenceId, FactId, ObservationId, ObservedStateId,
            ProvenanceId, RiskId, SituationId, SourceId,
        },
        intent::{SubjectPath, TypedValue},
        normalization::{NormalizationInput, normalize_current_state},
        observation::{
            AssertionPolarity, Evidence, EvidenceContent, EvidenceKind, EvidenceLink,
            EvidenceRelation, Fact, Observation, Provenance,
        },
        quality::{Confidence, FreshnessStatus, TrustClass},
    };

    fn quality() -> QualityMetadata {
        QualityMetadata::new(
            TrustClass::ObservedEvidence,
            SensitivityClass::Internal,
            Confidence::score(0.92).unwrap(),
            FreshnessStatus::Fresh,
            Uncertainty::None,
        )
    }

    fn state_with_quality(status: &str, value: i64) -> ObservedState {
        let provenance = Provenance::new(
            ProvenanceId::new("prov-1").unwrap(),
            SourceKind::Tool,
            SourceId::new("tool-1").unwrap(),
            "tool://coverage",
        )
        .unwrap();
        let observation = Observation::new(
            ObservationId::new("observation-1").unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            TypedValue::Integer(value),
            provenance.id().clone(),
        )
        .unwrap();
        let fact = Fact::new(
            FactId::new("fact-1").unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            TypedValue::Integer(value),
            AssertionPolarity::Affirmed,
            vec![observation.id().clone()],
        )
        .unwrap();
        let evidence = Evidence::new(
            EvidenceId::new("evidence-1").unwrap(),
            EvidenceKind::Report,
            "coverage report",
            EvidenceContent::inline("coverage report").unwrap(),
            provenance.id().clone(),
            vec![EvidenceLink::new(
                fact.id().clone(),
                EvidenceRelation::Supports,
            )],
        )
        .unwrap();
        normalize_current_state(
            ObservedStateId::new(status).unwrap(),
            NormalizationInput::new(
                ObservationEvidenceSet::new(
                    vec![provenance],
                    vec![observation],
                    vec![fact],
                    vec![evidence],
                )
                .unwrap(),
            )
            .with_quality_metadata(
                SubjectPath::from_str("coverage.percent").unwrap(),
                vec![quality()],
            ),
        )
        .unwrap()
    }

    fn assessment(state: &ObservedState) -> Assessment {
        let basis = BasisReferences::from_state_entry(&state.entries()[0]).unwrap();
        Assessment::new(
            AssessmentId::new("assessment-coverage").unwrap(),
            AssessmentKind::Coverage,
            AssessmentConclusion::AtRisk,
            AssessmentStatus::Determined,
            ReasonCode::new("COVERAGE_BELOW_TARGET").unwrap(),
            "coverage is below the target",
            basis,
            AssessmentOrigin::Deterministic {
                rule: AssessmentRuleContract::new(
                    AssessmentRuleId::new("coverage-target").unwrap(),
                    AssessmentRuleVersion::V1,
                )
                .unwrap(),
            },
            quality(),
        )
        .unwrap()
    }

    #[test]
    fn rule_contracts_are_versioned_and_fail_closed() {
        assert_eq!(AssessmentRuleVersion::V1.to_string(), "1.0");
        assert!(
            AssessmentRuleVersion::new(2, 0)
                .unwrap()
                .ensure_supported()
                .is_err()
        );
        assert!(
            AssessmentRuleContract::new(
                AssessmentRuleId::new("rule").unwrap(),
                AssessmentRuleVersion::new(2, 0).unwrap(),
            )
            .is_err()
        );
        assert_eq!(ReasonCode::new("REASON_1").unwrap().as_str(), "REASON_1");
        assert!(ReasonCode::new("bad reason").is_err());
        assert_eq!(
            ReasonCode::from_str("ROUND_TRIP").unwrap().to_string(),
            "ROUND_TRIP"
        );
    }

    #[test]
    fn assessment_and_risk_retain_distinct_basis_and_quality() {
        let state = state_with_quality("state-1", 92);
        let assessment = assessment(&state);
        assert_eq!(assessment.basis().facts()[0].as_str(), "fact-1");
        assert_eq!(
            assessment.origin().rule().unwrap().version(),
            AssessmentRuleVersion::V1
        );
        assert_eq!(
            assessment.quality().sensitivity(),
            SensitivityClass::Internal
        );

        let risk = Risk::new(
            RiskId::new("risk-coverage").unwrap(),
            RiskCategory::Quality,
            RiskSeverity::High,
            RiskLikelihood::Unknown,
            RiskStatus::Open,
            ReasonCode::new("COVERAGE_RISK").unwrap(),
            "quality target may remain unmet",
            BasisReferences::new(
                vec![SubjectPath::from_str("coverage.percent").unwrap()],
                vec![FactId::new("fact-1").unwrap()],
                vec![EvidenceId::new("evidence-1").unwrap()],
                vec![ProvenanceId::new("prov-1").unwrap()],
                vec![assessment.id().clone()],
            )
            .unwrap(),
            RiskOrigin::AssessmentDerived,
            quality(),
        )
        .unwrap();
        assert_eq!(risk.likelihood(), RiskLikelihood::Unknown);
        assert!(risk.likelihood().probability_value().is_none());
    }

    #[test]
    fn assembly_is_order_independent_and_explains_derived_results() {
        let state = state_with_quality("state-1", 92);
        let assessment = assessment(&state);
        let risk = Risk::new(
            RiskId::new("risk-coverage").unwrap(),
            RiskCategory::Quality,
            RiskSeverity::High,
            RiskLikelihood::Qualitative(QualitativeLikelihood::Possible),
            RiskStatus::Open,
            ReasonCode::new("COVERAGE_RISK").unwrap(),
            "quality target may remain unmet",
            BasisReferences::new(
                vec![SubjectPath::from_str("coverage.percent").unwrap()],
                vec![FactId::new("fact-1").unwrap()],
                vec![EvidenceId::new("evidence-1").unwrap()],
                vec![ProvenanceId::new("prov-1").unwrap()],
                vec![assessment.id().clone()],
            )
            .unwrap(),
            RiskOrigin::AssessmentDerived,
            quality(),
        )
        .unwrap();
        let situation = SituationAssemblyInput::new(state)
            .with_risks(vec![risk.clone()])
            .unwrap()
            .with_assessments(vec![assessment.clone()])
            .unwrap()
            .assemble(SituationId::new("situation-1").unwrap())
            .unwrap();
        assert_eq!(situation.observed_state_id().unwrap().as_str(), "state-1");
        assert_eq!(situation.assessments()[0], assessment);
        assert_eq!(situation.risks()[0], risk);
        let trace = &situation.explainability()[0];
        assert_eq!(trace.reason().as_str(), "COVERAGE_BELOW_TARGET");
        assert!(trace.summary().contains("coverage"));
    }

    #[test]
    fn unresolved_state_and_explicit_diagnostics_remain_visible() {
        let state = state_with_quality("state-1", 92);
        let basis = BasisReferences::from_state_entry(&state.entries()[0]).unwrap();
        let diagnostic = SituationDiagnostic::new(
            SituationDiagnosticCode::DataQuality,
            "review the source quality",
            basis,
        )
        .unwrap();
        let situation = SituationAssemblyInput::new(state)
            .with_diagnostics(vec![diagnostic])
            .unwrap()
            .assemble(SituationId::new("situation-1").unwrap())
            .unwrap();
        assert_eq!(situation.diagnostics().len(), 1);
        assert_eq!(
            situation.diagnostics()[0].code(),
            SituationDiagnosticCode::DataQuality
        );
    }

    #[test]
    fn dangling_basis_and_quality_downgrades_fail_closed() {
        let state = state_with_quality("state-1", 92);
        let mut bad_basis = BasisReferences::from_state_entry(&state.entries()[0]).unwrap();
        bad_basis.facts = vec![FactId::new("missing-fact").unwrap()];
        let bad = Assessment::new(
            AssessmentId::new("assessment-bad").unwrap(),
            AssessmentKind::Quality,
            AssessmentConclusion::Unknown,
            AssessmentStatus::Unresolved,
            ReasonCode::new("UNKNOWN").unwrap(),
            "unknown",
            bad_basis,
            AssessmentOrigin::Deterministic {
                rule: AssessmentRuleContract::new(
                    AssessmentRuleId::new("rule").unwrap(),
                    AssessmentRuleVersion::V1,
                )
                .unwrap(),
            },
            quality(),
        )
        .unwrap();
        assert!(
            SituationAssemblyInput::new(state.clone())
                .with_assessments(vec![bad])
                .unwrap()
                .assemble(SituationId::new("situation-1").unwrap())
                .is_err()
        );

        let lower = QualityMetadata::new(
            TrustClass::ObservedEvidence,
            SensitivityClass::Public,
            Confidence::Unknown,
            FreshnessStatus::Fresh,
            Uncertainty::None,
        );
        let basis = BasisReferences::from_state_entry(&state.entries()[0]).unwrap();
        let bad_quality = Assessment::new(
            AssessmentId::new("assessment-lower").unwrap(),
            AssessmentKind::Quality,
            AssessmentConclusion::AtRisk,
            AssessmentStatus::Determined,
            ReasonCode::new("LOWER").unwrap(),
            "lower handling",
            basis,
            AssessmentOrigin::Deterministic {
                rule: AssessmentRuleContract::new(
                    AssessmentRuleId::new("rule").unwrap(),
                    AssessmentRuleVersion::V1,
                )
                .unwrap(),
            },
            lower,
        )
        .unwrap();
        assert!(
            SituationAssemblyInput::new(state)
                .with_assessments(vec![bad_quality])
                .unwrap()
                .assemble(SituationId::new("situation-1").unwrap())
                .is_err()
        );
    }

    #[test]
    fn model_proposals_need_provenance_and_keep_their_source_class() {
        let state = state_with_quality("state-1", 92);
        let basis = BasisReferences::from_state_entry(&state.entries()[0]).unwrap();
        let proposal = Assessment::new(
            AssessmentId::new("assessment-model").unwrap(),
            AssessmentKind::Architecture,
            AssessmentConclusion::Unknown,
            AssessmentStatus::Proposed,
            ReasonCode::new("MODEL_PROPOSAL").unwrap(),
            "model proposed a review",
            basis,
            AssessmentOrigin::External {
                source_kind: SourceKind::Model,
                provenance: ProvenanceId::new("prov-1").unwrap(),
            },
            quality(),
        )
        .unwrap();
        let situation = SituationAssemblyInput::new(state)
            .with_assessments(vec![proposal])
            .unwrap()
            .assemble(SituationId::new("situation-model").unwrap())
            .unwrap();
        assert_eq!(
            situation.assessments()[0].origin().source_kind(),
            Some(SourceKind::Model)
        );
        assert_eq!(
            situation.assessments()[0].status(),
            AssessmentStatus::Proposed
        );
    }

    #[test]
    fn risk_likelihood_and_quality_boundaries_are_explicit() {
        assert_eq!(
            RiskLikelihood::probability(0.5).unwrap().as_str(),
            "EXPLICIT_PROBABILITY"
        );
        assert!(RiskLikelihood::probability(1.2).is_err());
        let stale = QualityMetadata::new(
            TrustClass::ObservedEvidence,
            SensitivityClass::Public,
            Confidence::Unknown,
            FreshnessStatus::Stale,
            Uncertainty::None,
        );
        let state = state_with_quality("state-1", 92);
        let assessment = assessment(&state);
        let basis = BasisReferences::from_state_entry(&state.entries()[0]).unwrap();
        let risk = Risk::new(
            RiskId::new("risk-bad-quality").unwrap(),
            RiskCategory::Quality,
            RiskSeverity::High,
            RiskLikelihood::Unknown,
            RiskStatus::Open,
            ReasonCode::new("STALE_BASIS").unwrap(),
            "bad propagation",
            BasisReferences::new(
                basis.state_subjects().to_vec(),
                basis.facts().to_vec(),
                basis.evidence().to_vec(),
                basis.provenances().to_vec(),
                vec![assessment.id().clone()],
            )
            .unwrap(),
            RiskOrigin::AssessmentDerived,
            stale,
        )
        .unwrap();
        assert!(
            SituationAssemblyInput::new(state)
                .with_assessments(vec![assessment])
                .unwrap()
                .with_risks(vec![risk])
                .unwrap()
                .assemble(SituationId::new("situation-risk").unwrap())
                .is_err()
        );
    }

    #[test]
    fn all_codes_accessors_and_reference_variants_are_stable() {
        assert_eq!(AssessmentKind::Quality.as_str(), "QUALITY");
        assert_eq!(AssessmentKind::Architecture.as_str(), "ARCHITECTURE");
        assert_eq!(AssessmentKind::Coverage.as_str(), "COVERAGE");
        assert_eq!(AssessmentKind::Dependency.as_str(), "DEPENDENCY");
        assert_eq!(AssessmentKind::DataQuality.as_str(), "DATA_QUALITY");
        assert_eq!(AssessmentKind::Operational.as_str(), "OPERATIONAL");
        assert_eq!(AssessmentKind::Security.as_str(), "SECURITY");
        assert_eq!(AssessmentConclusion::Positive.as_str(), "POSITIVE");
        assert_eq!(AssessmentConclusion::AtRisk.as_str(), "AT_RISK");
        assert_eq!(AssessmentConclusion::Negative.as_str(), "NEGATIVE");
        assert_eq!(AssessmentConclusion::Unknown.as_str(), "UNKNOWN");
        assert_eq!(AssessmentStatus::Determined.as_str(), "DETERMINED");
        assert_eq!(AssessmentStatus::Unresolved.as_str(), "UNRESOLVED");
        assert_eq!(AssessmentStatus::Proposed.as_str(), "PROPOSED");
        assert_eq!(RiskCategory::Quality.as_str(), "QUALITY");
        assert_eq!(RiskCategory::Architecture.as_str(), "ARCHITECTURE");
        assert_eq!(RiskCategory::Dependency.as_str(), "DEPENDENCY");
        assert_eq!(RiskCategory::Security.as_str(), "SECURITY");
        assert_eq!(RiskCategory::Operational.as_str(), "OPERATIONAL");
        assert_eq!(RiskCategory::DataQuality.as_str(), "DATA_QUALITY");
        assert_eq!(QualitativeLikelihood::Rare.as_str(), "RARE");
        assert_eq!(QualitativeLikelihood::Possible.as_str(), "POSSIBLE");
        assert_eq!(QualitativeLikelihood::Likely.as_str(), "LIKELY");
        assert_eq!(RiskLikelihood::Unknown.as_str(), "UNKNOWN");
        assert_eq!(
            RiskLikelihood::Qualitative(QualitativeLikelihood::Likely).as_str(),
            "LIKELY"
        );
        assert_eq!(RiskSeverity::Unknown.as_str(), "UNKNOWN");
        assert_eq!(RiskSeverity::Informational.as_str(), "INFORMATIONAL");
        assert_eq!(RiskSeverity::Low.as_str(), "LOW");
        assert_eq!(RiskSeverity::Medium.as_str(), "MEDIUM");
        assert_eq!(RiskSeverity::High.as_str(), "HIGH");
        assert_eq!(RiskSeverity::Critical.as_str(), "CRITICAL");
        assert_eq!(RiskStatus::Open.as_str(), "OPEN");
        assert_eq!(RiskStatus::Unresolved.as_str(), "UNRESOLVED");
        assert_eq!(RiskStatus::Unknown.as_str(), "UNKNOWN");
        assert_eq!(
            SituationDiagnosticCode::UnknownState.as_str(),
            "UNKNOWN_STATE"
        );
        assert_eq!(
            SituationDiagnosticCode::StateConflict.as_str(),
            "STATE_CONFLICT"
        );
        assert_eq!(
            SituationDiagnosticCode::UnsupportedState.as_str(),
            "UNSUPPORTED_STATE"
        );
        assert_eq!(
            SituationDiagnosticCode::UnresolvedAssessment.as_str(),
            "UNRESOLVED_ASSESSMENT"
        );
        assert_eq!(
            SituationDiagnosticCode::UnknownRisk.as_str(),
            "UNKNOWN_RISK"
        );
        assert_eq!(
            SituationDiagnosticCode::DataQuality.as_str(),
            "DATA_QUALITY"
        );

        let digest = crate::observation::ContentDigest::new("a".repeat(64)).unwrap();
        let rule = AssessmentRuleContract::new(
            AssessmentRuleId::new("rule-1").unwrap(),
            AssessmentRuleVersion::V1,
        )
        .unwrap()
        .with_semantic_digest(digest.clone());
        assert_eq!(rule.id().as_str(), "rule-1");
        assert_eq!(rule.version().major(), 1);
        assert_eq!(rule.version().minor(), 0);
        assert_eq!(rule.semantic_digest(), Some(&digest));
        assert_eq!(
            AssessmentRuleVersion::from_str("1.0").unwrap(),
            AssessmentRuleVersion::V1
        );

        let subject = SubjectPath::from_str("coverage.percent").unwrap();
        let basis = BasisReferences::new(
            vec![subject.clone()],
            vec![FactId::new("fact-1").unwrap()],
            vec![EvidenceId::new("evidence-1").unwrap()],
            vec![ProvenanceId::new("prov-1").unwrap()],
            vec![AssessmentId::new("assessment-1").unwrap()],
        )
        .unwrap();
        assert!(!basis.is_empty());
        assert_eq!(basis.state_subjects(), &[subject]);
        assert_eq!(basis.facts()[0].as_str(), "fact-1");
        assert_eq!(basis.evidence()[0].as_str(), "evidence-1");
        assert_eq!(basis.provenances()[0].as_str(), "prov-1");
        assert_eq!(basis.assessments()[0].as_str(), "assessment-1");

        let external_origin = AssessmentOrigin::External {
            source_kind: SourceKind::Model,
            provenance: ProvenanceId::new("prov-1").unwrap(),
        };
        assert_eq!(external_origin.rule(), None);
        assert_eq!(external_origin.source_kind(), Some(SourceKind::Model));
        assert_eq!(external_origin.provenance().unwrap().as_str(), "prov-1");
        let deterministic_origin = AssessmentOrigin::Deterministic { rule };
        assert!(deterministic_origin.rule().is_some());
        assert_eq!(deterministic_origin.source_kind(), None);
        assert_eq!(deterministic_origin.provenance(), None);
        assert_eq!(
            RiskLikelihood::Qualitative(QualitativeLikelihood::Rare).probability_value(),
            None
        );
        let explicit_probability =
            RiskLikelihood::ExplicitProbability(TaskConfidence::new(0.5).unwrap());
        assert!(explicit_probability.probability_value().is_some());

        let assessment = Assessment::new(
            AssessmentId::new("assessment-1").unwrap(),
            AssessmentKind::Quality,
            AssessmentConclusion::Positive,
            AssessmentStatus::Determined,
            ReasonCode::new("QUALITY_OK").unwrap(),
            "quality is within target",
            BasisReferences::new(
                vec![SubjectPath::from_str("coverage.percent").unwrap()],
                vec![FactId::new("fact-1").unwrap()],
                vec![EvidenceId::new("evidence-1").unwrap()],
                vec![ProvenanceId::new("prov-1").unwrap()],
                Vec::new(),
            )
            .unwrap(),
            external_origin,
            quality(),
        )
        .unwrap();
        assert_eq!(assessment.id().as_str(), "assessment-1");
        assert_eq!(assessment.kind(), AssessmentKind::Quality);
        assert_eq!(assessment.conclusion(), AssessmentConclusion::Positive);
        assert_eq!(assessment.status(), AssessmentStatus::Determined);
        assert_eq!(assessment.reason().as_str(), "QUALITY_OK");
        assert_eq!(assessment.summary(), "quality is within target");
        assert_eq!(assessment.basis().facts().len(), 1);
        assert_eq!(assessment.origin().source_kind(), Some(SourceKind::Model));
        assert_eq!(assessment.quality(), quality());
        assert!(
            Assessment::new(
                AssessmentId::new("assessment-empty").unwrap(),
                AssessmentKind::Quality,
                AssessmentConclusion::Unknown,
                AssessmentStatus::Unresolved,
                ReasonCode::new("EMPTY").unwrap(),
                "empty basis",
                BasisReferences::new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
                    .unwrap(),
                AssessmentOrigin::Deterministic {
                    rule: AssessmentRuleContract::new(
                        AssessmentRuleId::new("rule-empty").unwrap(),
                        AssessmentRuleVersion::V1,
                    )
                    .unwrap(),
                },
                quality(),
            )
            .is_err()
        );
        assert!(
            Risk::new(
                RiskId::new("risk-empty").unwrap(),
                RiskCategory::Quality,
                RiskSeverity::Unknown,
                RiskLikelihood::Unknown,
                RiskStatus::Unknown,
                ReasonCode::new("EMPTY").unwrap(),
                "empty basis",
                BasisReferences::new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
                    .unwrap(),
                RiskOrigin::AssessmentDerived,
                quality(),
            )
            .is_err()
        );

        let reference_external = SituationReference::External {
            source: SourceId::new("github").unwrap(),
            reference: ReferenceId::new("issue-1").unwrap(),
        };
        let reference_runtime = SituationReference::Runtime {
            runtime: ExecutionRuntimeId::new("runtime-1").unwrap(),
            reference: ReferenceId::new("run-1").unwrap(),
        };
        let state = state_with_quality("state-reference", 92);
        let input = SituationAssemblyInput::new(state.clone());
        assert_eq!(input.observed_state().id(), state.id());
        let situation = input
            .with_records(ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap())
            .with_assessments(Vec::new())
            .unwrap()
            .with_risks(Vec::new())
            .unwrap()
            .with_references(vec![reference_runtime.clone(), reference_external.clone()])
            .unwrap()
            .assemble(SituationId::new("situation-reference").unwrap())
            .unwrap();
        assert_eq!(
            situation.references(),
            &[reference_external, reference_runtime]
        );
        assert_eq!(situation.observed_state_id(), Some(state.id()));
        assert!(situation.assessments().is_empty());
        assert!(situation.risks().is_empty());
        assert!(situation.explainability().is_empty());
    }

    #[test]
    fn input_collection_duplicates_and_unknown_state_diagnostics_fail_closed() {
        let state = state_with_quality("state-duplicates", 92);
        let assessment = assessment(&state);
        assert!(
            SituationAssemblyInput::new(state.clone())
                .with_assessments(vec![assessment.clone(), assessment.clone()])
                .is_err()
        );
        let risk = Risk::new(
            RiskId::new("risk-duplicate").unwrap(),
            RiskCategory::Quality,
            RiskSeverity::Low,
            RiskLikelihood::Unknown,
            RiskStatus::Unknown,
            ReasonCode::new("UNKNOWN_RISK").unwrap(),
            "risk is unknown",
            BasisReferences::from_state_entry(&state.entries()[0]).unwrap(),
            RiskOrigin::Deterministic {
                rule: AssessmentRuleContract::new(
                    AssessmentRuleId::new("risk-rule").unwrap(),
                    AssessmentRuleVersion::V1,
                )
                .unwrap(),
            },
            quality(),
        )
        .unwrap();
        assert!(
            SituationAssemblyInput::new(state.clone())
                .with_risks(vec![risk.clone(), risk])
                .is_err()
        );
        let diagnostic = SituationDiagnostic::new(
            SituationDiagnosticCode::UnknownRisk,
            "risk is not classifiable",
            BasisReferences::new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(diagnostic.code(), SituationDiagnosticCode::UnknownRisk);
        assert_eq!(diagnostic.summary(), "risk is not classifiable");
        assert!(diagnostic.basis().is_empty());
        assert!(
            SituationAssemblyInput::new(state.clone())
                .with_diagnostics(vec![diagnostic.clone(), diagnostic])
                .is_err()
        );
        assert!(
            SituationAssemblyInput::new(state.clone())
                .with_references(vec![
                    SituationReference::External {
                        source: SourceId::new("github").unwrap(),
                        reference: ReferenceId::new("issue-1").unwrap(),
                    },
                    SituationReference::External {
                        source: SourceId::new("github").unwrap(),
                        reference: ReferenceId::new("issue-1").unwrap(),
                    },
                ])
                .is_err()
        );

        let unknown = normalize_current_state(
            ObservedStateId::new("state-unknown-situation").unwrap(),
            NormalizationInput::new(
                ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap(),
            )
            .with_unknown_subjects(vec![
                SubjectPath::from_str("architecture.violation").unwrap(),
            ])
            .unwrap(),
        )
        .unwrap();
        let situation = SituationAssemblyInput::new(unknown)
            .assemble(SituationId::new("situation-unknown").unwrap())
            .unwrap();
        assert_eq!(
            situation.diagnostics()[0].code(),
            SituationDiagnosticCode::UnknownState
        );
        assert!(situation.diagnostics()[0].summary().contains("unknown"));
    }

    #[test]
    fn explainability_trace_exposes_assessment_and_risk_items() {
        let state = state_with_quality("state-traces", 92);
        let assessment = assessment(&state);
        let risk = Risk::new(
            RiskId::new("risk-trace").unwrap(),
            RiskCategory::Operational,
            RiskSeverity::Critical,
            RiskLikelihood::ExplicitProbability(TaskConfidence::new(0.75).unwrap()),
            RiskStatus::Unresolved,
            ReasonCode::new("UNRESOLVED_RISK").unwrap(),
            "risk needs review",
            BasisReferences::new(
                vec![SubjectPath::from_str("coverage.percent").unwrap()],
                vec![FactId::new("fact-1").unwrap()],
                vec![EvidenceId::new("evidence-1").unwrap()],
                vec![ProvenanceId::new("prov-1").unwrap()],
                vec![assessment.id().clone()],
            )
            .unwrap(),
            RiskOrigin::External {
                source_kind: SourceKind::Model,
                provenance: ProvenanceId::new("prov-1").unwrap(),
            },
            quality(),
        )
        .unwrap();
        let assessment_trace = ExplainabilityTrace::for_assessment(&assessment);
        let risk_trace = ExplainabilityTrace::for_risk(&risk);
        assert_eq!(
            assessment_trace.item(),
            &ExplainabilityItem::Assessment(assessment.id().clone())
        );
        assert_eq!(
            risk_trace.item(),
            &ExplainabilityItem::Risk(risk.id().clone())
        );
        assert_eq!(risk_trace.reason().as_str(), "UNRESOLVED_RISK");
        assert!(risk_trace.summary().contains("CRITICAL"));
        assert_eq!(risk_trace.basis().assessments(), risk.basis().assessments());
        assert!(matches!(risk.origin(), RiskOrigin::External { .. }));
    }
}
