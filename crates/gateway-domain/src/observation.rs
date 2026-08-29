//! Explicit observation, assertion, evidence and provenance contracts.
//!
//! These records describe what an external input said and how it was
//! obtained. They are deliberately separate from authorization, capability
//! selection and process-gate decisions.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use crate::{
    identifiers::{EvidenceId, FactId, ObservationId, ProvenanceId, ReferenceId, SourceId},
    intent::{SubjectPath, TypedValue},
    validation::{NonEmptyText, ValidationError},
};

/// Provider-independent source classes for externally derived records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum SourceKind {
    Caller,
    Repository,
    Git,
    GitHub,
    Ci,
    Tool,
    Runtime,
    Retrieval,
    Model,
    Synthetic,
}

impl SourceKind {
    /// Returns the stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Caller => "CALLER",
            Self::Repository => "REPOSITORY",
            Self::Git => "GIT",
            Self::GitHub => "GITHUB",
            Self::Ci => "CI",
            Self::Tool => "TOOL",
            Self::Runtime => "RUNTIME",
            Self::Retrieval => "RETRIEVAL",
            Self::Model => "MODEL",
            Self::Synthetic => "SYNTHETIC",
        }
    }
}

impl fmt::Display for SourceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for SourceKind {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "CALLER" => Ok(Self::Caller),
            "REPOSITORY" => Ok(Self::Repository),
            "GIT" => Ok(Self::Git),
            "GITHUB" => Ok(Self::GitHub),
            "CI" => Ok(Self::Ci),
            "TOOL" => Ok(Self::Tool),
            "RUNTIME" => Ok(Self::Runtime),
            "RETRIEVAL" => Ok(Self::Retrieval),
            "MODEL" => Ok(Self::Model),
            "SYNTHETIC" => Ok(Self::Synthetic),
            value => Err(ValidationError::UnknownDomainValue {
                field: "source_kind",
                value: value.to_owned(),
            }),
        }
    }
}

/// An opaque, source-supplied timestamp retained without local-time coercion.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SourceTimestamp(NonEmptyText);

impl SourceTimestamp {
    /// Retains a non-empty timestamp supplied by the source.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        Ok(Self(NonEmptyText::new_for_field(
            value,
            "source_timestamp",
        )?))
    }

    /// Returns the source timestamp exactly as supplied.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for SourceTimestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// An optional stable hexadecimal content digest for an external artifact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ContentDigest(NonEmptyText);

impl ContentDigest {
    /// Creates a canonical 256-bit hexadecimal digest.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        if value.len() != 64 || !value.chars().all(|character| character.is_ascii_hexdigit()) {
            return Err(ValidationError::InvalidDeclarativeValue {
                reason: "content digest must be exactly 64 hexadecimal characters",
            });
        }
        Ok(Self(NonEmptyText::new_for_field(value, "content_digest")?))
    }

    /// Returns the digest text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for ContentDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Source lineage and acquisition metadata for one declarative record.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Provenance {
    id: ProvenanceId,
    source_kind: SourceKind,
    source_id: SourceId,
    source_reference: NonEmptyText,
    producer: Option<NonEmptyText>,
    acquired_at: Option<SourceTimestamp>,
    source_timestamp: Option<SourceTimestamp>,
    parent_provenance: Vec<ProvenanceId>,
}

impl Provenance {
    /// Creates source lineage without assigning trust or authority.
    pub fn new(
        id: ProvenanceId,
        source_kind: SourceKind,
        source_id: SourceId,
        source_reference: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            id,
            source_kind,
            source_id,
            source_reference: NonEmptyText::new_for_field(source_reference, "source_reference")?,
            producer: None,
            acquired_at: None,
            source_timestamp: None,
            parent_provenance: Vec::new(),
        })
    }

    /// Adds optional producer metadata.
    pub fn with_producer(mut self, producer: impl Into<String>) -> Result<Self, ValidationError> {
        self.producer = Some(NonEmptyText::new_for_field(producer, "producer")?);
        Ok(self)
    }

    /// Adds the acquisition time supplied by the collecting boundary.
    #[must_use]
    pub fn with_acquired_at(mut self, acquired_at: SourceTimestamp) -> Self {
        self.acquired_at = Some(acquired_at);
        self
    }

    /// Adds the timestamp supplied by the source.
    #[must_use]
    pub fn with_source_timestamp(mut self, source_timestamp: SourceTimestamp) -> Self {
        self.source_timestamp = Some(source_timestamp);
        self
    }

    /// Adds sorted parent lineage references and rejects duplicates/self-lineage.
    pub fn with_parent_provenance(
        mut self,
        mut parent_provenance: Vec<ProvenanceId>,
    ) -> Result<Self, ValidationError> {
        parent_provenance.sort();
        if parent_provenance.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ValidationError::DuplicateRelationship {
                field: "parent_provenance",
            });
        }
        if parent_provenance.iter().any(|parent| parent == &self.id) {
            return Err(ValidationError::SelfReference {
                field: "parent_provenance",
            });
        }
        self.parent_provenance = parent_provenance;
        Ok(self)
    }

    /// Returns the provenance identity.
    #[must_use]
    pub fn id(&self) -> &ProvenanceId {
        &self.id
    }

    /// Returns the provider-independent source class.
    #[must_use]
    pub const fn source_kind(&self) -> SourceKind {
        self.source_kind
    }

    /// Returns the typed source identity.
    #[must_use]
    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    /// Returns the opaque source reference.
    #[must_use]
    pub fn source_reference(&self) -> &str {
        self.source_reference.as_str()
    }

    /// Returns optional producer metadata.
    #[must_use]
    pub fn producer(&self) -> Option<&str> {
        self.producer.as_ref().map(NonEmptyText::as_str)
    }

    /// Returns the acquisition timestamp, if supplied.
    #[must_use]
    pub fn acquired_at(&self) -> Option<&SourceTimestamp> {
        self.acquired_at.as_ref()
    }

    /// Returns the source timestamp, if supplied.
    #[must_use]
    pub fn source_timestamp(&self) -> Option<&SourceTimestamp> {
        self.source_timestamp.as_ref()
    }

    /// Returns parent lineage references in canonical order.
    #[must_use]
    pub fn parent_provenance(&self) -> &[ProvenanceId] {
        &self.parent_provenance
    }
}

/// One reported or measured input from a source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Observation {
    id: ObservationId,
    subject: SubjectPath,
    value: TypedValue,
    provenance: ProvenanceId,
    occurred_at: Option<SourceTimestamp>,
}

impl Observation {
    /// Creates one typed observation linked to source provenance.
    pub fn new(
        id: ObservationId,
        subject: SubjectPath,
        value: TypedValue,
        provenance: ProvenanceId,
    ) -> Result<Self, ValidationError> {
        value.validate()?;
        Ok(Self {
            id,
            subject,
            value,
            provenance,
            occurred_at: None,
        })
    }

    /// Adds the source occurrence time.
    #[must_use]
    pub fn with_occurred_at(mut self, occurred_at: SourceTimestamp) -> Self {
        self.occurred_at = Some(occurred_at);
        self
    }

    /// Returns the observation identity.
    #[must_use]
    pub fn id(&self) -> &ObservationId {
        &self.id
    }

    /// Returns the observed subject/property path.
    #[must_use]
    pub fn subject(&self) -> &SubjectPath {
        &self.subject
    }

    /// Returns the explicit observed value.
    #[must_use]
    pub fn value(&self) -> &TypedValue {
        &self.value
    }

    /// Returns the required provenance identity.
    #[must_use]
    pub fn provenance(&self) -> &ProvenanceId {
        &self.provenance
    }

    /// Returns the optional source occurrence timestamp.
    #[must_use]
    pub fn occurred_at(&self) -> Option<&SourceTimestamp> {
        self.occurred_at.as_ref()
    }

    /// Returns the deterministic identity used for semantic deduplication.
    #[must_use]
    pub fn deduplication_key(&self) -> String {
        format!(
            "OBSERVATION|subject={}|value={}|provenance={}|occurred_at={}",
            self.subject,
            self.value,
            self.provenance,
            self.occurred_at
                .as_ref()
                .map_or("", SourceTimestamp::as_str)
        )
    }
}

/// Whether a normalized fact is asserted or explicitly negated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum AssertionPolarity {
    Affirmed,
    Negated,
}

impl AssertionPolarity {
    /// Returns the stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Affirmed => "AFFIRMED",
            Self::Negated => "NEGATED",
        }
    }
}

impl fmt::Display for AssertionPolarity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for AssertionPolarity {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "AFFIRMED" => Ok(Self::Affirmed),
            "NEGATED" => Ok(Self::Negated),
            value => Err(ValidationError::UnknownDomainValue {
                field: "assertion_polarity",
                value: value.to_owned(),
            }),
        }
    }
}

/// A normalized assertion derived from one or more observations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Fact {
    id: FactId,
    subject: SubjectPath,
    value: TypedValue,
    polarity: AssertionPolarity,
    observations: Vec<ObservationId>,
}

impl Fact {
    /// Creates a normalized fact with explicit observation lineage.
    pub fn new(
        id: FactId,
        subject: SubjectPath,
        value: TypedValue,
        polarity: AssertionPolarity,
        mut observations: Vec<ObservationId>,
    ) -> Result<Self, ValidationError> {
        value.validate()?;
        if observations.is_empty() {
            return Err(ValidationError::EmptyRelationship {
                field: "fact_observations",
            });
        }
        observations.sort();
        if observations.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ValidationError::DuplicateRelationship {
                field: "fact_observations",
            });
        }
        Ok(Self {
            id,
            subject,
            value,
            polarity,
            observations,
        })
    }

    /// Returns the fact identity.
    #[must_use]
    pub fn id(&self) -> &FactId {
        &self.id
    }

    /// Returns the normalized subject/property path.
    #[must_use]
    pub fn subject(&self) -> &SubjectPath {
        &self.subject
    }

    /// Returns the normalized typed value.
    #[must_use]
    pub fn value(&self) -> &TypedValue {
        &self.value
    }

    /// Returns the assertion polarity.
    #[must_use]
    pub const fn polarity(&self) -> AssertionPolarity {
        self.polarity
    }

    /// Returns source observations in canonical identity order.
    #[must_use]
    pub fn observations(&self) -> &[ObservationId] {
        &self.observations
    }

    /// Returns the deterministic semantic identity of this normalized claim.
    #[must_use]
    pub fn deduplication_key(&self) -> String {
        format!(
            "FACT|subject={}|value={}|polarity={}|observations={}",
            self.subject,
            self.value,
            self.polarity,
            self.observations
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

/// Classification of evidence without provider-specific authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum EvidenceKind {
    CallerStatement,
    Report,
    Measurement,
    Artifact,
    TestResult,
    Document,
    RetrievalResult,
    ToolOutput,
    ModelOutput,
    SyntheticFixture,
}

impl EvidenceKind {
    /// Returns the stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CallerStatement => "CALLER_STATEMENT",
            Self::Report => "REPORT",
            Self::Measurement => "MEASUREMENT",
            Self::Artifact => "ARTIFACT",
            Self::TestResult => "TEST_RESULT",
            Self::Document => "DOCUMENT",
            Self::RetrievalResult => "RETRIEVAL_RESULT",
            Self::ToolOutput => "TOOL_OUTPUT",
            Self::ModelOutput => "MODEL_OUTPUT",
            Self::SyntheticFixture => "SYNTHETIC_FIXTURE",
        }
    }
}

impl fmt::Display for EvidenceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for EvidenceKind {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "CALLER_STATEMENT" => Ok(Self::CallerStatement),
            "REPORT" => Ok(Self::Report),
            "MEASUREMENT" => Ok(Self::Measurement),
            "ARTIFACT" => Ok(Self::Artifact),
            "TEST_RESULT" => Ok(Self::TestResult),
            "DOCUMENT" => Ok(Self::Document),
            "RETRIEVAL_RESULT" => Ok(Self::RetrievalResult),
            "TOOL_OUTPUT" => Ok(Self::ToolOutput),
            "MODEL_OUTPUT" => Ok(Self::ModelOutput),
            "SYNTHETIC_FIXTURE" => Ok(Self::SyntheticFixture),
            value => Err(ValidationError::UnknownDomainValue {
                field: "evidence_kind",
                value: value.to_owned(),
            }),
        }
    }
}

/// Inline evidence summary or a reference to an external artifact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum EvidenceContent {
    Inline(NonEmptyText),
    Reference {
        reference: ReferenceId,
        digest: Option<ContentDigest>,
    },
}

impl EvidenceContent {
    /// Creates bounded inline evidence content.
    pub fn inline(value: impl Into<String>) -> Result<Self, ValidationError> {
        Ok(Self::Inline(NonEmptyText::new_for_field(
            value,
            "evidence_content",
        )?))
    }

    /// Creates an external content reference with optional integrity metadata.
    #[must_use]
    pub const fn reference(reference: ReferenceId, digest: Option<ContentDigest>) -> Self {
        Self::Reference { reference, digest }
    }

    /// Returns the inline content, if present.
    #[must_use]
    pub fn inline_content(&self) -> Option<&str> {
        match self {
            Self::Inline(value) => Some(value.as_str()),
            Self::Reference { .. } => None,
        }
    }

    /// Returns the external reference, if present.
    #[must_use]
    pub const fn reference_data(&self) -> Option<(&ReferenceId, Option<&ContentDigest>)> {
        match self {
            Self::Inline(_) => None,
            Self::Reference { reference, digest } => Some((reference, digest.as_ref())),
        }
    }
}

/// Whether evidence supports or challenges a normalized fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum EvidenceRelation {
    Supports,
    Challenges,
}

impl EvidenceRelation {
    /// Returns the stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supports => "SUPPORTS",
            Self::Challenges => "CHALLENGES",
        }
    }
}

impl fmt::Display for EvidenceRelation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One typed support/challenge edge from evidence to a fact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct EvidenceLink {
    fact: FactId,
    relation: EvidenceRelation,
}

impl EvidenceLink {
    /// Creates one explicit evidence-to-fact relationship.
    #[must_use]
    pub const fn new(fact: FactId, relation: EvidenceRelation) -> Self {
        Self { fact, relation }
    }

    /// Returns the linked fact identity.
    #[must_use]
    pub const fn fact(&self) -> &FactId {
        &self.fact
    }

    /// Returns the relationship polarity.
    #[must_use]
    pub const fn relation(&self) -> EvidenceRelation {
        self.relation
    }
}

/// Evidence supporting or challenging one or more normalized facts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Evidence {
    id: EvidenceId,
    kind: EvidenceKind,
    summary: NonEmptyText,
    content: EvidenceContent,
    provenance: ProvenanceId,
    links: Vec<EvidenceLink>,
    occurred_at: Option<SourceTimestamp>,
}

impl Evidence {
    /// Creates evidence with explicit provenance and fact relationships.
    pub fn new(
        id: EvidenceId,
        kind: EvidenceKind,
        summary: impl Into<String>,
        content: EvidenceContent,
        provenance: ProvenanceId,
        mut links: Vec<EvidenceLink>,
    ) -> Result<Self, ValidationError> {
        if links.is_empty() {
            return Err(ValidationError::EmptyRelationship {
                field: "evidence_links",
            });
        }
        links.sort();
        if links.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ValidationError::DuplicateRelationship {
                field: "evidence_links",
            });
        }
        Ok(Self {
            id,
            kind,
            summary: NonEmptyText::new_for_field(summary, "evidence_summary")?,
            content,
            provenance,
            links,
            occurred_at: None,
        })
    }

    /// Adds an optional evidence occurrence timestamp.
    #[must_use]
    pub fn with_occurred_at(mut self, occurred_at: SourceTimestamp) -> Self {
        self.occurred_at = Some(occurred_at);
        self
    }

    /// Returns the evidence identity.
    #[must_use]
    pub fn id(&self) -> &EvidenceId {
        &self.id
    }

    /// Returns the evidence classification.
    #[must_use]
    pub const fn kind(&self) -> EvidenceKind {
        self.kind
    }

    /// Returns the bounded evidence summary.
    #[must_use]
    pub fn summary(&self) -> &str {
        self.summary.as_str()
    }

    /// Returns the inline/reference content descriptor.
    #[must_use]
    pub const fn content(&self) -> &EvidenceContent {
        &self.content
    }

    /// Returns the evidence provenance identity.
    #[must_use]
    pub fn provenance(&self) -> &ProvenanceId {
        &self.provenance
    }

    /// Returns support/challenge links in canonical order.
    #[must_use]
    pub fn links(&self) -> &[EvidenceLink] {
        &self.links
    }

    /// Returns the optional evidence occurrence timestamp.
    #[must_use]
    pub fn occurred_at(&self) -> Option<&SourceTimestamp> {
        self.occurred_at.as_ref()
    }

    /// Returns the deterministic semantic identity used for deduplication.
    #[must_use]
    pub fn deduplication_key(&self) -> String {
        let links = self
            .links
            .iter()
            .map(|link| format!("{}:{}", link.fact, link.relation))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "EVIDENCE|kind={}|summary={}|content={:?}|provenance={}|links={}|occurred_at={}",
            self.kind,
            self.summary(),
            self.content,
            self.provenance,
            links,
            self.occurred_at
                .as_ref()
                .map_or("", SourceTimestamp::as_str)
        )
    }
}

/// Validated collection boundary for an observation-to-evidence lineage graph.
///
/// The collection is the point at which references can be checked together:
/// individual records remain small and composable, while accepted facts
/// cannot dangle from missing observations or provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationEvidenceSet {
    provenances: Vec<Provenance>,
    observations: Vec<Observation>,
    facts: Vec<Fact>,
    evidence: Vec<Evidence>,
}

impl ObservationEvidenceSet {
    /// Sorts and validates one complete lineage collection.
    pub fn new(
        mut provenances: Vec<Provenance>,
        mut observations: Vec<Observation>,
        mut facts: Vec<Fact>,
        mut evidence: Vec<Evidence>,
    ) -> Result<Self, ValidationError> {
        provenances.sort_by(|left, right| left.id.cmp(&right.id));
        observations.sort_by(|left, right| left.id.cmp(&right.id));
        facts.sort_by(|left, right| left.id.cmp(&right.id));
        evidence.sort_by(|left, right| left.id.cmp(&right.id));
        ensure_unique_ids(&provenances, "provenance", |value| value.id.to_string())?;
        ensure_unique_ids(&observations, "observation", |value| value.id.to_string())?;
        ensure_unique_ids(&facts, "fact", |value| value.id.to_string())?;
        ensure_unique_ids(&evidence, "evidence", |value| value.id.to_string())?;
        deduplicate_by_key(&mut observations, Observation::deduplication_key);
        deduplicate_by_key(&mut facts, Fact::deduplication_key);
        deduplicate_by_key(&mut evidence, Evidence::deduplication_key);
        let set = Self {
            provenances,
            observations,
            facts,
            evidence,
        };
        set.validate()?;
        Ok(set)
    }

    /// Rechecks collection identities, semantic uniqueness and all references.
    pub fn validate(&self) -> Result<(), ValidationError> {
        ensure_unique_ids(&self.provenances, "provenance", |value| {
            value.id.to_string()
        })?;
        ensure_unique_ids(&self.observations, "observation", |value| {
            value.id.to_string()
        })?;
        ensure_unique_ids(&self.facts, "fact", |value| value.id.to_string())?;
        ensure_unique_ids(&self.evidence, "evidence", |value| value.id.to_string())?;
        ensure_unique_keys(
            &self.observations,
            "observation",
            Observation::deduplication_key,
        )?;
        ensure_unique_keys(&self.facts, "fact", Fact::deduplication_key)?;
        ensure_unique_keys(&self.evidence, "evidence", Evidence::deduplication_key)?;

        let provenance_ids = self
            .provenances
            .iter()
            .map(|value| value.id.clone())
            .collect::<BTreeSet<_>>();
        for provenance in &self.provenances {
            for parent in &provenance.parent_provenance {
                ensure_reference(
                    provenance_ids.contains(parent),
                    "provenance",
                    parent.to_string(),
                )?;
            }
        }

        let observation_ids = self
            .observations
            .iter()
            .map(|value| value.id.clone())
            .collect::<BTreeSet<_>>();
        for observation in &self.observations {
            ensure_reference(
                provenance_ids.contains(&observation.provenance),
                "provenance",
                observation.provenance.to_string(),
            )?;
        }

        let fact_ids = self
            .facts
            .iter()
            .map(|value| value.id.clone())
            .collect::<BTreeSet<_>>();
        for fact in &self.facts {
            for observation in &fact.observations {
                ensure_reference(
                    observation_ids.contains(observation),
                    "observation",
                    observation.to_string(),
                )?;
            }
        }

        for item in &self.evidence {
            ensure_reference(
                provenance_ids.contains(&item.provenance),
                "provenance",
                item.provenance.to_string(),
            )?;
            for link in &item.links {
                ensure_reference(fact_ids.contains(&link.fact), "fact", link.fact.to_string())?;
            }
        }
        Ok(())
    }

    /// Returns all provenances in canonical identity order.
    #[must_use]
    pub fn provenances(&self) -> &[Provenance] {
        &self.provenances
    }

    /// Returns all observations in canonical identity order.
    #[must_use]
    pub fn observations(&self) -> &[Observation] {
        &self.observations
    }

    /// Returns all facts in canonical identity order.
    #[must_use]
    pub fn facts(&self) -> &[Fact] {
        &self.facts
    }

    /// Returns all evidence in canonical identity order.
    #[must_use]
    pub fn evidence(&self) -> &[Evidence] {
        &self.evidence
    }

    /// Returns facts with at least one support and one challenge edge.
    #[must_use]
    pub fn conflicting_fact_ids(&self) -> Vec<FactId> {
        let mut relations = BTreeMap::<FactId, (bool, bool)>::new();
        for item in &self.evidence {
            for link in &item.links {
                let entry = relations.entry(link.fact.clone()).or_default();
                match link.relation {
                    EvidenceRelation::Supports => entry.0 = true,
                    EvidenceRelation::Challenges => entry.1 = true,
                }
            }
        }
        relations
            .into_iter()
            .filter_map(|(fact, (supports, challenges))| (supports && challenges).then_some(fact))
            .collect()
    }
}

fn deduplicate_by_key<T, F>(values: &mut Vec<T>, key: F)
where
    F: Fn(&T) -> String,
{
    let mut seen = BTreeSet::new();
    values.retain(|value| seen.insert(key(value)));
}

fn ensure_unique_ids<T, F>(values: &[T], kind: &'static str, id: F) -> Result<(), ValidationError>
where
    F: Fn(&T) -> String,
{
    for pair in values.windows(2) {
        let left = id(&pair[0]);
        let right = id(&pair[1]);
        if left == right {
            return Err(ValidationError::DuplicateDeclarativeIdentity { kind, id: left });
        }
    }
    Ok(())
}

fn ensure_unique_keys<T, F>(values: &[T], kind: &'static str, key: F) -> Result<(), ValidationError>
where
    F: Fn(&T) -> String,
{
    let mut seen = BTreeSet::new();
    for value in values {
        let key = key(value);
        if !seen.insert(key.clone()) {
            return Err(ValidationError::DuplicateDeclarativeIdentity { kind, id: key });
        }
    }
    Ok(())
}

fn ensure_reference(present: bool, kind: &'static str, id: String) -> Result<(), ValidationError> {
    if present {
        Ok(())
    } else {
        Err(ValidationError::MissingDeclarativeIdentity { kind, id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identifiers::{EvidenceId, FactId, ObservationId};

    fn provenance(id: &str, kind: SourceKind) -> Provenance {
        Provenance::new(
            ProvenanceId::new(id).unwrap(),
            kind,
            SourceId::new(format!("source-{id}")).unwrap(),
            format!("adapter://{id}"),
        )
        .unwrap()
    }

    fn observation(id: &str, provenance: &str) -> Observation {
        Observation::new(
            ObservationId::new(id).unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            TypedValue::Integer(92),
            ProvenanceId::new(provenance).unwrap(),
        )
        .unwrap()
    }

    fn fact(id: &str, observation: &str) -> Fact {
        Fact::new(
            FactId::new(id).unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            TypedValue::Integer(92),
            AssertionPolarity::Affirmed,
            vec![ObservationId::new(observation).unwrap()],
        )
        .unwrap()
    }

    #[test]
    fn source_kinds_timestamps_and_digests_are_strict() {
        let kinds = [
            SourceKind::Caller,
            SourceKind::Repository,
            SourceKind::Git,
            SourceKind::GitHub,
            SourceKind::Ci,
            SourceKind::Tool,
            SourceKind::Runtime,
            SourceKind::Retrieval,
            SourceKind::Model,
            SourceKind::Synthetic,
        ];
        for kind in kinds {
            assert_eq!(SourceKind::from_str(kind.as_str()).unwrap(), kind);
            assert_eq!(kind.to_string(), kind.as_str());
        }
        assert!(SourceKind::from_str("MODEL_OUTPUT").is_err());
        assert!(SourceTimestamp::new("2026-08-29T12:00:00Z").is_ok());
        assert!(SourceTimestamp::new(" ").is_err());
        assert!(ContentDigest::new("a".repeat(64)).is_ok());
        assert!(ContentDigest::new("g".repeat(64)).is_err());
        assert!(ContentDigest::new("short").is_err());
    }

    #[test]
    fn provenance_keeps_source_metadata_and_rejects_bad_lineage() {
        let id = ProvenanceId::new("prov-1").unwrap();
        let parent = ProvenanceId::new("prov-0").unwrap();
        let timestamp = SourceTimestamp::new("2026-08-29T12:00:00Z").unwrap();
        let value = Provenance::new(
            id.clone(),
            SourceKind::Tool,
            SourceId::new("coverage-tool").unwrap(),
            "tool://coverage",
        )
        .unwrap()
        .with_producer("coverage-cli")
        .unwrap()
        .with_acquired_at(timestamp.clone())
        .with_source_timestamp(timestamp.clone())
        .with_parent_provenance(vec![parent.clone()])
        .unwrap();
        assert_eq!(value.id(), &id);
        assert_eq!(value.source_kind(), SourceKind::Tool);
        assert_eq!(value.source_id().as_str(), "coverage-tool");
        assert_eq!(value.source_reference(), "tool://coverage");
        assert_eq!(value.producer(), Some("coverage-cli"));
        assert_eq!(value.acquired_at(), Some(&timestamp));
        assert_eq!(value.source_timestamp(), Some(&timestamp));
        assert_eq!(value.parent_provenance(), std::slice::from_ref(&parent));
        assert!(
            Provenance::new(
                ProvenanceId::new("prov-2").unwrap(),
                SourceKind::Caller,
                SourceId::new("caller").unwrap(),
                "caller://input",
            )
            .unwrap()
            .with_producer(" ")
            .is_err()
        );
        assert!(
            Provenance::new(
                id.clone(),
                SourceKind::Caller,
                SourceId::new("caller").unwrap(),
                "caller://input",
            )
            .unwrap()
            .with_parent_provenance(vec![id.clone(), id])
            .is_err()
        );
        assert!(
            Provenance::new(
                ProvenanceId::new("prov-3").unwrap(),
                SourceKind::Caller,
                SourceId::new("caller").unwrap(),
                "caller://input",
            )
            .unwrap()
            .with_parent_provenance(vec![parent.clone(), parent])
            .is_err()
        );
    }

    #[test]
    fn observations_and_facts_keep_typed_lineage() {
        let timestamp = SourceTimestamp::new("2026-08-29T12:00:00Z").unwrap();
        let observation =
            observation("observation-1", "prov-1").with_occurred_at(timestamp.clone());
        assert_eq!(observation.id().as_str(), "observation-1");
        assert_eq!(observation.subject().to_string(), "coverage.percent");
        assert_eq!(observation.value(), &TypedValue::Integer(92));
        assert_eq!(observation.provenance().as_str(), "prov-1");
        assert_eq!(observation.occurred_at(), Some(&timestamp));
        assert!(!observation.deduplication_key().is_empty());

        let fact = fact("fact-1", "observation-1");
        assert_eq!(fact.id().as_str(), "fact-1");
        assert_eq!(fact.subject().to_string(), "coverage.percent");
        assert_eq!(fact.value(), &TypedValue::Integer(92));
        assert_eq!(fact.polarity(), AssertionPolarity::Affirmed);
        assert_eq!(fact.observations()[0].as_str(), "observation-1");
        assert!(!fact.deduplication_key().is_empty());
        assert_eq!(
            AssertionPolarity::from_str("NEGATED").unwrap(),
            AssertionPolarity::Negated
        );
        assert_eq!(AssertionPolarity::Affirmed.to_string(), "AFFIRMED");
        assert!(AssertionPolarity::from_str("affirmed").is_err());
        assert!(
            Fact::new(
                FactId::new("empty").unwrap(),
                SubjectPath::from_str("coverage.percent").unwrap(),
                TypedValue::Integer(92),
                AssertionPolarity::Negated,
                vec![],
            )
            .is_err()
        );
        assert!(
            Fact::new(
                FactId::new("duplicate").unwrap(),
                SubjectPath::from_str("coverage.percent").unwrap(),
                TypedValue::Integer(92),
                AssertionPolarity::Affirmed,
                vec![
                    ObservationId::new("observation-1").unwrap(),
                    ObservationId::new("observation-1").unwrap(),
                ],
            )
            .is_err()
        );
    }

    #[test]
    fn evidence_content_kinds_links_and_relations_are_explicit() {
        let reference = ReferenceId::new("coverage-report").unwrap();
        let digest = ContentDigest::new("b".repeat(64)).unwrap();
        let inline = EvidenceContent::inline("coverage report").unwrap();
        assert_eq!(inline.inline_content(), Some("coverage report"));
        assert!(inline.reference_data().is_none());
        let external = EvidenceContent::reference(reference.clone(), Some(digest.clone()));
        assert!(external.inline_content().is_none());
        let (actual_reference, actual_digest) = external.reference_data().unwrap();
        assert_eq!(actual_reference, &reference);
        assert_eq!(actual_digest, Some(&digest));

        let kinds = [
            EvidenceKind::CallerStatement,
            EvidenceKind::Report,
            EvidenceKind::Measurement,
            EvidenceKind::Artifact,
            EvidenceKind::TestResult,
            EvidenceKind::Document,
            EvidenceKind::RetrievalResult,
            EvidenceKind::ToolOutput,
            EvidenceKind::ModelOutput,
            EvidenceKind::SyntheticFixture,
        ];
        for kind in kinds {
            assert_eq!(EvidenceKind::from_str(kind.as_str()).unwrap(), kind);
            assert_eq!(kind.to_string(), kind.as_str());
        }
        assert!(EvidenceKind::from_str("FACT").is_err());
        let fact_id = FactId::new("fact-1").unwrap();
        let link = EvidenceLink::new(fact_id.clone(), EvidenceRelation::Supports);
        assert_eq!(link.fact(), &fact_id);
        assert_eq!(link.relation(), EvidenceRelation::Supports);
        assert_eq!(EvidenceRelation::Challenges.to_string(), "CHALLENGES");
        let evidence = Evidence::new(
            EvidenceId::new("evidence-1").unwrap(),
            EvidenceKind::Report,
            "coverage report",
            external,
            ProvenanceId::new("prov-1").unwrap(),
            vec![link],
        )
        .unwrap()
        .with_occurred_at(SourceTimestamp::new("2026-08-29T12:00:00Z").unwrap());
        assert_eq!(evidence.id().as_str(), "evidence-1");
        assert_eq!(evidence.kind(), EvidenceKind::Report);
        assert_eq!(evidence.summary(), "coverage report");
        assert_eq!(evidence.provenance().as_str(), "prov-1");
        assert_eq!(evidence.links().len(), 1);
        assert!(evidence.occurred_at().is_some());
        assert!(!evidence.deduplication_key().is_empty());
        assert!(EvidenceContent::inline(" ").is_err());
        assert!(
            Evidence::new(
                EvidenceId::new("empty").unwrap(),
                EvidenceKind::Artifact,
                "artifact",
                EvidenceContent::inline("bytes").unwrap(),
                ProvenanceId::new("prov-1").unwrap(),
                vec![],
            )
            .is_err()
        );
        let duplicate_link = EvidenceLink::new(fact_id, EvidenceRelation::Supports);
        assert!(
            Evidence::new(
                EvidenceId::new("duplicate").unwrap(),
                EvidenceKind::Report,
                "report",
                EvidenceContent::inline("bytes").unwrap(),
                ProvenanceId::new("prov-1").unwrap(),
                vec![duplicate_link.clone(), duplicate_link],
            )
            .is_err()
        );
    }

    #[test]
    fn complete_lineage_set_is_sorted_and_reports_conflicting_evidence() {
        let source = provenance("prov-1", SourceKind::Tool);
        let second_source = provenance("prov-2", SourceKind::Model);
        let first_observation = observation("observation-1", "prov-1");
        let second_observation = observation("observation-2", "prov-2");
        let first_fact = fact("fact-1", "observation-1");
        let second_fact = Fact::new(
            FactId::new("fact-2").unwrap(),
            SubjectPath::from_str("architecture.violation").unwrap(),
            TypedValue::Boolean(false),
            AssertionPolarity::Negated,
            vec![ObservationId::new("observation-2").unwrap()],
        )
        .unwrap();
        let support = Evidence::new(
            EvidenceId::new("evidence-support").unwrap(),
            EvidenceKind::Report,
            "report supports",
            EvidenceContent::inline("92 percent").unwrap(),
            ProvenanceId::new("prov-1").unwrap(),
            vec![
                EvidenceLink::new(first_fact.id().clone(), EvidenceRelation::Supports),
                EvidenceLink::new(second_fact.id().clone(), EvidenceRelation::Supports),
            ],
        )
        .unwrap();
        let challenge = Evidence::new(
            EvidenceId::new("evidence-challenge").unwrap(),
            EvidenceKind::ModelOutput,
            "model challenges",
            EvidenceContent::reference(ReferenceId::new("model-output").unwrap(), None),
            ProvenanceId::new("prov-2").unwrap(),
            vec![EvidenceLink::new(
                first_fact.id().clone(),
                EvidenceRelation::Challenges,
            )],
        )
        .unwrap();
        let set = ObservationEvidenceSet::new(
            vec![second_source, source],
            vec![second_observation, first_observation],
            vec![second_fact, first_fact],
            vec![challenge, support],
        )
        .unwrap();
        assert_eq!(set.provenances()[0].id().as_str(), "prov-1");
        assert_eq!(set.observations()[0].id().as_str(), "observation-1");
        assert_eq!(set.facts()[0].id().as_str(), "fact-1");
        assert_eq!(set.evidence()[0].id().as_str(), "evidence-challenge");
        assert_eq!(
            set.conflicting_fact_ids(),
            vec![FactId::new("fact-1").unwrap()]
        );
        assert!(set.validate().is_ok());
    }

    #[test]
    fn lineage_set_rejects_duplicates_and_dangling_references() {
        let source = provenance("prov-1", SourceKind::Tool);
        let duplicate_source = provenance("prov-1", SourceKind::Caller);
        assert!(matches!(
            ObservationEvidenceSet::new(
                vec![source.clone(), duplicate_source],
                vec![],
                vec![],
                vec![],
            ),
            Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "provenance",
                ..
            })
        ));

        let duplicate_observation = observation("observation-1", "prov-1");
        assert!(matches!(
            ObservationEvidenceSet::new(
                vec![source.clone()],
                vec![duplicate_observation.clone(), duplicate_observation],
                vec![],
                vec![],
            ),
            Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "observation",
                ..
            })
        ));

        let duplicate_fact = fact("fact-1", "observation-1");
        assert!(matches!(
            ObservationEvidenceSet::new(
                vec![source.clone()],
                vec![observation("observation-1", "prov-1")],
                vec![duplicate_fact.clone(), duplicate_fact],
                vec![],
            ),
            Err(ValidationError::DuplicateDeclarativeIdentity { kind: "fact", .. })
        ));

        let duplicate_evidence = Evidence::new(
            EvidenceId::new("evidence-1").unwrap(),
            EvidenceKind::Report,
            "same",
            EvidenceContent::inline("same").unwrap(),
            ProvenanceId::new("prov-1").unwrap(),
            vec![EvidenceLink::new(
                FactId::new("fact-1").unwrap(),
                EvidenceRelation::Supports,
            )],
        )
        .unwrap();
        assert!(matches!(
            ObservationEvidenceSet::new(
                vec![source.clone()],
                vec![observation("observation-1", "prov-1")],
                vec![fact("fact-1", "observation-1")],
                vec![duplicate_evidence.clone(), duplicate_evidence],
            ),
            Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "evidence",
                ..
            })
        ));

        assert!(matches!(
            ObservationEvidenceSet::new(
                vec![],
                vec![observation("observation-1", "missing")],
                vec![],
                vec![],
            ),
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "provenance",
                ..
            })
        ));
        assert!(matches!(
            ObservationEvidenceSet::new(
                vec![source.clone()],
                vec![],
                vec![fact("fact-1", "missing-observation")],
                vec![],
            ),
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "observation",
                ..
            })
        ));
        let missing_fact_evidence = Evidence::new(
            EvidenceId::new("missing-fact-evidence").unwrap(),
            EvidenceKind::Report,
            "report",
            EvidenceContent::inline("report").unwrap(),
            ProvenanceId::new("prov-1").unwrap(),
            vec![EvidenceLink::new(
                FactId::new("missing-fact").unwrap(),
                EvidenceRelation::Supports,
            )],
        )
        .unwrap();
        assert!(matches!(
            ObservationEvidenceSet::new(vec![source], vec![], vec![], vec![missing_fact_evidence],),
            Err(ValidationError::MissingDeclarativeIdentity { kind: "fact", .. })
        ));
    }

    #[test]
    fn lineage_deduplication_detects_semantically_equal_distinct_ids() {
        let source = provenance("prov-1", SourceKind::Tool);
        let first = observation("observation-1", "prov-1");
        let second_duplicate = observation("observation-2", "prov-1");
        let observations = ObservationEvidenceSet::new(
            vec![source.clone()],
            vec![first, second_duplicate],
            vec![],
            vec![],
        )
        .unwrap();
        assert_eq!(observations.observations().len(), 1);

        let first_fact = fact("fact-1", "observation-1");
        let second_fact = fact("fact-2", "observation-1");
        let facts = ObservationEvidenceSet::new(
            vec![source.clone()],
            vec![observation("observation-1", "prov-1")],
            vec![first_fact, second_fact],
            vec![],
        )
        .unwrap();
        assert_eq!(facts.facts().len(), 1);

        let first_evidence = Evidence::new(
            EvidenceId::new("evidence-1").unwrap(),
            EvidenceKind::Report,
            "same",
            EvidenceContent::inline("same").unwrap(),
            ProvenanceId::new("prov-1").unwrap(),
            vec![EvidenceLink::new(
                FactId::new("fact-1").unwrap(),
                EvidenceRelation::Supports,
            )],
        )
        .unwrap();
        let second_evidence = Evidence::new(
            EvidenceId::new("evidence-2").unwrap(),
            EvidenceKind::Report,
            "same",
            EvidenceContent::inline("same").unwrap(),
            ProvenanceId::new("prov-1").unwrap(),
            vec![EvidenceLink::new(
                FactId::new("fact-1").unwrap(),
                EvidenceRelation::Supports,
            )],
        )
        .unwrap();
        let evidence = ObservationEvidenceSet::new(
            vec![source],
            vec![observation("observation-1", "prov-1")],
            vec![fact("fact-1", "observation-1")],
            vec![first_evidence, second_evidence],
        )
        .unwrap();
        assert_eq!(evidence.evidence().len(), 1);
    }
}
