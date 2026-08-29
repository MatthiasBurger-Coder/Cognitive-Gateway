//! Deterministic normalization from explicit lineage records to current state.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    declarative_context::ObservedState,
    identifiers::{EvidenceId, FactId, ObservationId, ObservedStateId, ProvenanceId},
    intent::{SubjectPath, TypedValue},
    observation::{
        AssertionPolarity, Evidence, EvidenceRelation, Fact, Observation, ObservationEvidenceSet,
    },
    quality::{ConflictStatus, QualityMetadata},
    validation::ValidationError,
};

/// Stable reason codes emitted by current-state normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum NormalizationReasonCode {
    UnknownState,
    ConflictingAssertions,
    IncompatibleValueTypes,
    MissingEvidence,
}

impl NormalizationReasonCode {
    /// Returns the stable machine-readable reason code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownState => "UNKNOWN_STATE",
            Self::ConflictingAssertions => "CONFLICTING_ASSERTIONS",
            Self::IncompatibleValueTypes => "INCOMPATIBLE_VALUE_TYPES",
            Self::MissingEvidence => "MISSING_EVIDENCE",
        }
    }
}

/// The four non-coercive states exposed by a current-state snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum StateStatus {
    Known,
    Unknown,
    Conflicted,
    Unsupported,
}

impl StateStatus {
    /// Returns the stable machine-readable status.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Known => "KNOWN",
            Self::Unknown => "UNKNOWN",
            Self::Conflicted => "CONFLICTED",
            Self::Unsupported => "UNSUPPORTED",
        }
    }
}

/// One normalized claim retained as part of state explainability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct NormalizedClaim {
    fact: FactId,
    value: TypedValue,
    polarity: AssertionPolarity,
    observations: Vec<ObservationId>,
    provenances: Vec<ProvenanceId>,
    supporting_evidence: Vec<EvidenceId>,
    challenging_evidence: Vec<EvidenceId>,
}

impl NormalizedClaim {
    pub(crate) fn from_parts(
        fact: FactId,
        value: TypedValue,
        polarity: AssertionPolarity,
        mut observations: Vec<ObservationId>,
        mut provenances: Vec<ProvenanceId>,
        mut supporting_evidence: Vec<EvidenceId>,
        mut challenging_evidence: Vec<EvidenceId>,
    ) -> Result<Self, ValidationError> {
        value.validate()?;
        sort_unique(&mut observations, "normalized_claim.observations")?;
        sort_unique(&mut provenances, "normalized_claim.provenances")?;
        sort_unique(
            &mut supporting_evidence,
            "normalized_claim.supporting_evidence",
        )?;
        sort_unique(
            &mut challenging_evidence,
            "normalized_claim.challenging_evidence",
        )?;
        Ok(Self {
            fact,
            value,
            polarity,
            observations,
            provenances,
            supporting_evidence,
            challenging_evidence,
        })
    }

    /// Returns the source fact identity.
    #[must_use]
    pub fn fact(&self) -> &FactId {
        &self.fact
    }

    /// Returns the explicitly typed asserted value.
    #[must_use]
    pub fn value(&self) -> &TypedValue {
        &self.value
    }

    /// Returns whether the value is affirmed or negated.
    #[must_use]
    pub const fn polarity(&self) -> AssertionPolarity {
        self.polarity
    }

    /// Returns source observations in canonical order.
    #[must_use]
    pub fn observations(&self) -> &[ObservationId] {
        &self.observations
    }

    /// Returns provenance identities in canonical order.
    #[must_use]
    pub fn provenances(&self) -> &[ProvenanceId] {
        &self.provenances
    }

    /// Returns evidence that explicitly supports this fact.
    #[must_use]
    pub fn supporting_evidence(&self) -> &[EvidenceId] {
        &self.supporting_evidence
    }

    /// Returns evidence that explicitly challenges this fact.
    #[must_use]
    pub fn challenging_evidence(&self) -> &[EvidenceId] {
        &self.challenging_evidence
    }
}

/// Aggregate lineage for one normalized state entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct StateLineage {
    facts: Vec<FactId>,
    observations: Vec<ObservationId>,
    evidence: Vec<EvidenceId>,
    provenances: Vec<ProvenanceId>,
}

impl StateLineage {
    pub(crate) fn from_parts(
        mut facts: Vec<FactId>,
        mut observations: Vec<ObservationId>,
        mut evidence: Vec<EvidenceId>,
        mut provenances: Vec<ProvenanceId>,
    ) -> Result<Self, ValidationError> {
        sort_unique(&mut facts, "normalized_state.lineage.facts")?;
        sort_unique(&mut observations, "normalized_state.lineage.observations")?;
        sort_unique(&mut evidence, "normalized_state.lineage.evidence")?;
        sort_unique(&mut provenances, "normalized_state.lineage.provenances")?;
        Ok(Self {
            facts,
            observations,
            evidence,
            provenances,
        })
    }

    fn from_claims(claims: &[NormalizedClaim]) -> Self {
        let mut facts = BTreeSet::new();
        let mut observations = BTreeSet::new();
        let mut evidence = BTreeSet::new();
        let mut provenances = BTreeSet::new();
        for claim in claims {
            facts.insert(claim.fact.clone());
            observations.extend(claim.observations.iter().cloned());
            evidence.extend(claim.supporting_evidence.iter().cloned());
            evidence.extend(claim.challenging_evidence.iter().cloned());
            provenances.extend(claim.provenances.iter().cloned());
        }
        Self {
            facts: facts.into_iter().collect(),
            observations: observations.into_iter().collect(),
            evidence: evidence.into_iter().collect(),
            provenances: provenances.into_iter().collect(),
        }
    }

    /// Returns source facts in canonical order.
    #[must_use]
    pub fn facts(&self) -> &[FactId] {
        &self.facts
    }

    /// Returns source observations in canonical order.
    #[must_use]
    pub fn observations(&self) -> &[ObservationId] {
        &self.observations
    }

    /// Returns source evidence in canonical order.
    #[must_use]
    pub fn evidence(&self) -> &[EvidenceId] {
        &self.evidence
    }

    /// Returns source provenance in canonical order.
    #[must_use]
    pub fn provenances(&self) -> &[ProvenanceId] {
        &self.provenances
    }
}

/// One deterministic current-state entry; only KNOWN entries expose a value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct NormalizedStateEntry {
    subject: SubjectPath,
    status: StateStatus,
    value: Option<TypedValue>,
    polarity: Option<AssertionPolarity>,
    claims: Vec<NormalizedClaim>,
    lineage: StateLineage,
    diagnostics: Vec<NormalizationReasonCode>,
    metadata: Option<QualityMetadata>,
}

impl NormalizedStateEntry {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts(
        subject: SubjectPath,
        status: StateStatus,
        value: Option<TypedValue>,
        polarity: Option<AssertionPolarity>,
        mut claims: Vec<NormalizedClaim>,
        lineage: StateLineage,
        mut diagnostics: Vec<NormalizationReasonCode>,
        metadata: Option<QualityMetadata>,
    ) -> Result<Self, ValidationError> {
        claims.sort_by(|left, right| left.fact.cmp(&right.fact));
        if claims.windows(2).any(|pair| pair[0].fact == pair[1].fact) {
            return Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "normalized_claim",
                id: claims[1].fact.to_string(),
            });
        }
        diagnostics.sort();
        if diagnostics.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ValidationError::DuplicateRelationship {
                field: "normalized_state.diagnostics",
            });
        }
        let expected_lineage = StateLineage::from_claims(&claims);
        if lineage != expected_lineage {
            return Err(ValidationError::InvalidDeclarativeValue {
                reason: "normalized state lineage must match its claims",
            });
        }
        let is_known = status == StateStatus::Known;
        if is_known != (value.is_some() && polarity.is_some()) {
            return Err(ValidationError::InvalidStateCombination {
                reason: "known state must have value and polarity while other states must not",
            });
        }
        if status == StateStatus::Unknown && !claims.is_empty() {
            return Err(ValidationError::InvalidStateCombination {
                reason: "unknown state must not contain claims",
            });
        }
        Ok(Self {
            subject,
            status,
            value,
            polarity,
            claims,
            lineage,
            diagnostics,
            metadata,
        })
    }

    /// Returns the normalized subject/property path.
    #[must_use]
    pub fn subject(&self) -> &SubjectPath {
        &self.subject
    }

    /// Returns the explicit state status.
    #[must_use]
    pub const fn status(&self) -> StateStatus {
        self.status
    }

    /// Returns a value only for a supported, non-conflicted known state.
    #[must_use]
    pub fn value(&self) -> Option<&TypedValue> {
        self.value.as_ref()
    }

    /// Returns the known value's assertion polarity, if applicable.
    #[must_use]
    pub const fn polarity(&self) -> Option<AssertionPolarity> {
        self.polarity
    }

    /// Returns all normalized claims retained for this subject.
    #[must_use]
    pub fn claims(&self) -> &[NormalizedClaim] {
        &self.claims
    }

    /// Returns complete source lineage for this state entry.
    #[must_use]
    pub const fn lineage(&self) -> &StateLineage {
        &self.lineage
    }

    /// Returns stable diagnostics attached to this entry.
    #[must_use]
    pub fn diagnostics(&self) -> &[NormalizationReasonCode] {
        &self.diagnostics
    }

    /// Returns explicit propagated quality metadata, if supplied.
    #[must_use]
    pub const fn metadata(&self) -> Option<QualityMetadata> {
        self.metadata
    }
}

/// Input options for pure current-state normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizationInput {
    records: ObservationEvidenceSet,
    unknown_subjects: Vec<SubjectPath>,
    require_evidence: bool,
    quality_metadata: BTreeMap<SubjectPath, Vec<QualityMetadata>>,
}

impl NormalizationInput {
    /// Starts normalization from a validated observation/evidence set.
    #[must_use]
    pub fn new(records: ObservationEvidenceSet) -> Self {
        Self {
            records,
            unknown_subjects: Vec::new(),
            require_evidence: false,
            quality_metadata: BTreeMap::new(),
        }
    }

    /// Adds explicit subjects for which no observation is available.
    pub fn with_unknown_subjects<I>(mut self, subjects: I) -> Result<Self, ValidationError>
    where
        I: IntoIterator<Item = SubjectPath>,
    {
        let mut subjects = subjects.into_iter().collect::<Vec<_>>();
        subjects.sort();
        if subjects.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ValidationError::DuplicateRelationship {
                field: "unknown_subjects",
            });
        }
        self.unknown_subjects = subjects;
        Ok(self)
    }

    /// Makes explicit support evidence mandatory for every asserted fact.
    #[must_use]
    pub const fn with_required_evidence(mut self, required: bool) -> Self {
        self.require_evidence = required;
        self
    }

    /// Supplies explicit quality metadata for one normalized subject.
    #[must_use]
    pub fn with_quality_metadata(
        mut self,
        subject: SubjectPath,
        mut metadata: Vec<QualityMetadata>,
    ) -> Self {
        metadata.sort();
        self.quality_metadata.insert(subject, metadata);
        self
    }

    /// Returns the validated source records.
    #[must_use]
    pub const fn records(&self) -> &ObservationEvidenceSet {
        &self.records
    }

    /// Returns explicit unknown subjects.
    #[must_use]
    pub fn unknown_subjects(&self) -> &[SubjectPath] {
        &self.unknown_subjects
    }

    /// Returns whether support evidence is required.
    #[must_use]
    pub const fn requires_evidence(&self) -> bool {
        self.require_evidence
    }

    /// Returns explicit quality metadata grouped by subject.
    #[must_use]
    pub const fn quality_metadata(&self) -> &BTreeMap<SubjectPath, Vec<QualityMetadata>> {
        &self.quality_metadata
    }
}

/// Purely normalizes explicit records into a versioned observed-state snapshot.
pub fn normalize_current_state(
    id: ObservedStateId,
    input: NormalizationInput,
) -> Result<ObservedState, ValidationError> {
    input.records.validate()?;
    let NormalizationInput {
        records,
        unknown_subjects,
        require_evidence,
        quality_metadata,
    } = input;
    let mut observations = BTreeMap::<ObservationId, &Observation>::new();
    for observation in records.observations() {
        observations.insert(observation.id().clone(), observation);
    }
    let mut evidence_by_fact = BTreeMap::<FactId, (Vec<&Evidence>, Vec<&Evidence>)>::new();
    for evidence in records.evidence() {
        for link in evidence.links() {
            let relations = evidence_by_fact.entry(link.fact().clone()).or_default();
            match link.relation() {
                EvidenceRelation::Supports => relations.0.push(evidence),
                EvidenceRelation::Challenges => relations.1.push(evidence),
            }
        }
    }

    let mut facts_by_subject = BTreeMap::<SubjectPath, Vec<&Fact>>::new();
    for fact in records.facts() {
        facts_by_subject
            .entry(fact.subject().clone())
            .or_default()
            .push(fact);
    }
    let mut subjects = facts_by_subject.keys().cloned().collect::<BTreeSet<_>>();
    subjects.extend(unknown_subjects);
    let conflicting_facts = records
        .conflicting_fact_ids()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut entries = Vec::new();
    let mut diagnostics = Vec::new();

    for subject in subjects {
        let facts = facts_by_subject.remove(&subject).unwrap_or_default();
        let mut metadata = quality_metadata
            .get(&subject)
            .and_then(|values| QualityMetadata::merge(values));
        if facts.is_empty() {
            let diagnostic = NormalizationDiagnostic::new(
                NormalizationReasonCode::UnknownState,
                subject.clone(),
                Vec::new(),
            );
            diagnostics.push(diagnostic);
            entries.push(NormalizedStateEntry {
                subject,
                status: StateStatus::Unknown,
                value: None,
                polarity: None,
                claims: Vec::new(),
                lineage: StateLineage::from_claims(&[]),
                diagnostics: vec![NormalizationReasonCode::UnknownState],
                metadata,
            });
            continue;
        }

        let claims = facts
            .into_iter()
            .map(|fact| normalized_claim(fact, &observations, &evidence_by_fact))
            .collect::<Result<Vec<_>, _>>()?;
        let missing_evidence = require_evidence
            && claims
                .iter()
                .any(|claim| claim.supporting_evidence.is_empty());
        let incompatible_types = claims
            .windows(2)
            .any(|pair| pair[0].value.kind() != pair[1].value.kind());
        let different_assertions = claims
            .windows(2)
            .any(|pair| pair[0].value != pair[1].value || pair[0].polarity != pair[1].polarity);
        let evidence_conflict = claims
            .iter()
            .any(|claim| conflicting_facts.contains(&claim.fact));
        let metadata_conflict =
            metadata.is_some_and(|value| value.conflict() == ConflictStatus::Unresolved);
        let mut entry_diagnostics = Vec::new();
        let status = if missing_evidence {
            entry_diagnostics.push(NormalizationReasonCode::MissingEvidence);
            StateStatus::Unsupported
        } else if incompatible_types {
            entry_diagnostics.push(NormalizationReasonCode::IncompatibleValueTypes);
            entry_diagnostics.push(NormalizationReasonCode::ConflictingAssertions);
            StateStatus::Conflicted
        } else if different_assertions || evidence_conflict || metadata_conflict {
            entry_diagnostics.push(NormalizationReasonCode::ConflictingAssertions);
            StateStatus::Conflicted
        } else {
            StateStatus::Known
        };
        if status == StateStatus::Conflicted {
            metadata = metadata.map(|value| value.with_conflict(ConflictStatus::Unresolved));
        }
        if !entry_diagnostics.is_empty() {
            diagnostics.push(NormalizationDiagnostic::new(
                entry_diagnostics[0],
                subject.clone(),
                claims.iter().map(|claim| claim.fact.clone()).collect(),
            ));
        }
        let (value, polarity) = if status == StateStatus::Known {
            (Some(claims[0].value.clone()), Some(claims[0].polarity))
        } else {
            (None, None)
        };
        entries.push(NormalizedStateEntry {
            subject,
            status,
            value,
            polarity,
            lineage: StateLineage::from_claims(&claims),
            claims,
            diagnostics: entry_diagnostics,
            metadata,
        });
    }

    Ok(ObservedState::new_v1_with_entries(id, entries, diagnostics))
}

/// A stable normalization diagnostic tied to a subject and source facts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct NormalizationDiagnostic {
    code: NormalizationReasonCode,
    subject: SubjectPath,
    facts: Vec<FactId>,
}

impl NormalizationDiagnostic {
    pub(crate) fn from_parts(
        code: NormalizationReasonCode,
        subject: SubjectPath,
        mut facts: Vec<FactId>,
    ) -> Result<Self, ValidationError> {
        facts.sort();
        if facts.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ValidationError::DuplicateRelationship {
                field: "normalization_diagnostic.facts",
            });
        }
        Ok(Self {
            code,
            subject,
            facts,
        })
    }

    fn new(code: NormalizationReasonCode, subject: SubjectPath, mut facts: Vec<FactId>) -> Self {
        facts.sort();
        facts.dedup();
        Self {
            code,
            subject,
            facts,
        }
    }

    /// Returns the stable diagnostic code.
    #[must_use]
    pub const fn code(&self) -> NormalizationReasonCode {
        self.code
    }

    /// Returns the affected subject.
    #[must_use]
    pub const fn subject(&self) -> &SubjectPath {
        &self.subject
    }

    /// Returns affected source fact identities.
    #[must_use]
    pub fn facts(&self) -> &[FactId] {
        &self.facts
    }
}

fn normalized_claim(
    fact: &Fact,
    observations: &BTreeMap<ObservationId, &Observation>,
    evidence_by_fact: &BTreeMap<FactId, (Vec<&Evidence>, Vec<&Evidence>)>,
) -> Result<NormalizedClaim, ValidationError> {
    let mut provenances = BTreeSet::new();
    for observation_id in fact.observations() {
        let observation = observations.get(observation_id).ok_or_else(|| {
            ValidationError::MissingDeclarativeIdentity {
                kind: "observation",
                id: observation_id.to_string(),
            }
        })?;
        provenances.insert(observation.provenance().clone());
    }
    let (supporting, challenging) = evidence_by_fact
        .get(fact.id())
        .map_or((Vec::new(), Vec::new()), |(supporting, challenging)| {
            (supporting.clone(), challenging.clone())
        });
    let mut supporting_ids = supporting
        .iter()
        .map(|evidence| {
            provenances.insert(evidence.provenance().clone());
            evidence.id().clone()
        })
        .collect::<Vec<_>>();
    let mut challenging_ids = challenging
        .iter()
        .map(|evidence| {
            provenances.insert(evidence.provenance().clone());
            evidence.id().clone()
        })
        .collect::<Vec<_>>();
    supporting_ids.sort();
    challenging_ids.sort();
    Ok(NormalizedClaim {
        fact: fact.id().clone(),
        value: fact.value().clone(),
        polarity: fact.polarity(),
        observations: fact.observations().to_vec(),
        provenances: provenances.into_iter().collect(),
        supporting_evidence: supporting_ids,
        challenging_evidence: challenging_ids,
    })
}

fn sort_unique<T: Ord>(values: &mut [T], field: &'static str) -> Result<(), ValidationError> {
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ValidationError::DuplicateRelationship { field });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{
        identifiers::{EvidenceId, FactId, ObservationId, ProvenanceId, SourceId},
        observation::{EvidenceContent, EvidenceKind, EvidenceLink, Provenance, SourceKind},
        quality::{Confidence, FreshnessStatus, SensitivityClass, TrustClass, Uncertainty},
    };

    fn provenance(id: &str) -> Provenance {
        Provenance::new(
            ProvenanceId::new(id).unwrap(),
            SourceKind::Tool,
            SourceId::new(format!("source-{id}")).unwrap(),
            format!("tool://{id}"),
        )
        .unwrap()
    }

    fn fact(id: &str, observation: &str, subject: &str, value: TypedValue) -> Fact {
        Fact::new(
            FactId::new(id).unwrap(),
            SubjectPath::from_str(subject).unwrap(),
            value,
            AssertionPolarity::Affirmed,
            vec![ObservationId::new(observation).unwrap()],
        )
        .unwrap()
    }

    fn records(
        facts: Vec<Fact>,
        observations: Vec<Observation>,
        evidence: Vec<Evidence>,
    ) -> ObservationEvidenceSet {
        ObservationEvidenceSet::new(vec![provenance("prov-1")], observations, facts, evidence)
            .unwrap()
    }

    #[test]
    fn status_and_reason_codes_are_stable() {
        assert_eq!(StateStatus::Known.as_str(), "KNOWN");
        assert_eq!(StateStatus::Unknown.as_str(), "UNKNOWN");
        assert_eq!(StateStatus::Conflicted.as_str(), "CONFLICTED");
        assert_eq!(StateStatus::Unsupported.as_str(), "UNSUPPORTED");
        assert_eq!(
            NormalizationReasonCode::IncompatibleValueTypes.as_str(),
            "INCOMPATIBLE_VALUE_TYPES"
        );
        assert_eq!(
            NormalizationReasonCode::ConflictingAssertions.as_str(),
            "CONFLICTING_ASSERTIONS"
        );
        assert_eq!(
            NormalizationReasonCode::UnknownState.as_str(),
            "UNKNOWN_STATE"
        );
        assert_eq!(
            NormalizationReasonCode::MissingEvidence.as_str(),
            "MISSING_EVIDENCE"
        );
    }

    #[test]
    fn known_state_preserves_typed_value_and_complete_lineage() {
        let observation = Observation::new(
            ObservationId::new("observation-1").unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            TypedValue::Integer(92),
            ProvenanceId::new("prov-1").unwrap(),
        )
        .unwrap();
        let source_fact = fact(
            "fact-1",
            "observation-1",
            "coverage.percent",
            TypedValue::Integer(92),
        );
        let evidence = Evidence::new(
            EvidenceId::new("evidence-1").unwrap(),
            EvidenceKind::Report,
            "coverage report",
            EvidenceContent::inline("92 percent").unwrap(),
            ProvenanceId::new("prov-1").unwrap(),
            vec![EvidenceLink::new(
                source_fact.id().clone(),
                EvidenceRelation::Supports,
            )],
        )
        .unwrap();
        let subject = SubjectPath::from_str("coverage.percent").unwrap();
        let first_metadata = QualityMetadata::new(
            TrustClass::ObservedEvidence,
            SensitivityClass::Internal,
            Confidence::score(0.98).unwrap(),
            FreshnessStatus::Fresh,
            Uncertainty::Probabilistic,
        );
        let stronger_metadata = QualityMetadata::new(
            TrustClass::RetrievedContent,
            SensitivityClass::Secret,
            Confidence::Unknown,
            FreshnessStatus::Stale,
            Uncertainty::Incomplete,
        );
        let snapshot = normalize_current_state(
            ObservedStateId::new("state-1").unwrap(),
            NormalizationInput::new(records(
                vec![source_fact],
                vec![observation],
                vec![evidence],
            ))
            .with_quality_metadata(subject, vec![first_metadata, stronger_metadata]),
        )
        .unwrap();
        assert_eq!(snapshot.entries().len(), 1);
        let entry = &snapshot.entries()[0];
        assert_eq!(entry.status(), StateStatus::Known);
        assert_eq!(entry.subject().to_string(), "coverage.percent");
        assert_eq!(
            entry.metadata().unwrap().sensitivity(),
            SensitivityClass::Secret
        );
        assert_eq!(entry.metadata().unwrap().trust(), TrustClass::Mixed);
        assert_eq!(
            entry.metadata().unwrap().freshness(),
            FreshnessStatus::Stale
        );
        assert_eq!(entry.value(), Some(&TypedValue::Integer(92)));
        assert_eq!(entry.polarity(), Some(AssertionPolarity::Affirmed));
        assert_eq!(entry.claims().len(), 1);
        let claim = &entry.claims()[0];
        assert_eq!(claim.fact().as_str(), "fact-1");
        assert_eq!(claim.value(), &TypedValue::Integer(92));
        assert_eq!(claim.observations()[0].as_str(), "observation-1");
        assert_eq!(claim.provenances()[0].as_str(), "prov-1");
        assert_eq!(claim.supporting_evidence()[0].as_str(), "evidence-1");
        assert!(claim.challenging_evidence().is_empty());
        assert_eq!(entry.lineage().facts().len(), 1);
        assert_eq!(entry.lineage().observations().len(), 1);
        assert_eq!(entry.lineage().evidence().len(), 1);
        assert_eq!(entry.lineage().provenances().len(), 1);
        assert!(snapshot.diagnostics().is_empty());
    }

    #[test]
    fn unknown_state_is_explicit_and_duplicate_unknowns_fail_closed() {
        let input = NormalizationInput::new(records(vec![], vec![], vec![]));
        let subject = SubjectPath::from_str("architecture.violation").unwrap();
        let input = input.with_unknown_subjects(vec![subject.clone()]).unwrap();
        assert_eq!(input.unknown_subjects(), std::slice::from_ref(&subject));
        assert!(!input.requires_evidence());
        assert_eq!(input.records().observations().len(), 0);
        let snapshot =
            normalize_current_state(ObservedStateId::new("state-unknown").unwrap(), input).unwrap();
        assert_eq!(snapshot.entries()[0].status(), StateStatus::Unknown);
        assert!(snapshot.entries()[0].value().is_none());
        assert_eq!(
            snapshot.diagnostics()[0].code(),
            NormalizationReasonCode::UnknownState
        );
        let duplicate = NormalizationInput::new(records(vec![], vec![], vec![]))
            .with_unknown_subjects(vec![subject.clone(), subject]);
        assert!(duplicate.is_err());
    }

    #[test]
    fn conflicts_are_preserved_without_input_order_or_type_coercion() {
        let first_observation = Observation::new(
            ObservationId::new("observation-1").unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            TypedValue::Integer(92),
            ProvenanceId::new("prov-1").unwrap(),
        )
        .unwrap();
        let second_observation = Observation::new(
            ObservationId::new("observation-2").unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            TypedValue::string("92").unwrap(),
            ProvenanceId::new("prov-1").unwrap(),
        )
        .unwrap();
        let snapshot = normalize_current_state(
            ObservedStateId::new("state-conflict").unwrap(),
            NormalizationInput::new(records(
                vec![
                    fact(
                        "fact-2",
                        "observation-2",
                        "coverage.percent",
                        TypedValue::string("92").unwrap(),
                    ),
                    fact(
                        "fact-1",
                        "observation-1",
                        "coverage.percent",
                        TypedValue::Integer(92),
                    ),
                ],
                vec![second_observation, first_observation],
                vec![],
            )),
        )
        .unwrap();
        let entry = &snapshot.entries()[0];
        assert_eq!(entry.status(), StateStatus::Conflicted);
        assert!(entry.value().is_none());
        assert_eq!(entry.claims().len(), 2);
        assert_eq!(
            entry.diagnostics(),
            &[
                NormalizationReasonCode::IncompatibleValueTypes,
                NormalizationReasonCode::ConflictingAssertions
            ]
        );
    }

    #[test]
    fn missing_support_is_unsupported_only_when_explicitly_required() {
        let observation = Observation::new(
            ObservationId::new("observation-1").unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            TypedValue::Integer(92),
            ProvenanceId::new("prov-1").unwrap(),
        )
        .unwrap();
        let source_fact = fact(
            "fact-1",
            "observation-1",
            "coverage.percent",
            TypedValue::Integer(92),
        );
        let optional = normalize_current_state(
            ObservedStateId::new("state-optional").unwrap(),
            NormalizationInput::new(records(
                vec![source_fact.clone()],
                vec![observation.clone()],
                vec![],
            )),
        )
        .unwrap();
        assert_eq!(optional.entries()[0].status(), StateStatus::Known);
        let required = normalize_current_state(
            ObservedStateId::new("state-required").unwrap(),
            NormalizationInput::new(records(vec![source_fact], vec![observation], vec![]))
                .with_required_evidence(true),
        )
        .unwrap();
        assert_eq!(required.entries()[0].status(), StateStatus::Unsupported);
        assert_eq!(
            required.entries()[0].diagnostics(),
            &[NormalizationReasonCode::MissingEvidence]
        );
    }

    #[test]
    fn evidence_conflict_and_explicit_negation_remain_visible() {
        let observation = Observation::new(
            ObservationId::new("observation-1").unwrap(),
            SubjectPath::from_str("architecture.violation").unwrap(),
            TypedValue::Boolean(false),
            ProvenanceId::new("prov-1").unwrap(),
        )
        .unwrap();
        let source_fact = Fact::new(
            FactId::new("fact-1").unwrap(),
            SubjectPath::from_str("architecture.violation").unwrap(),
            TypedValue::Boolean(false),
            AssertionPolarity::Negated,
            vec![ObservationId::new("observation-1").unwrap()],
        )
        .unwrap();
        let support = Evidence::new(
            EvidenceId::new("evidence-support").unwrap(),
            EvidenceKind::Report,
            "report",
            EvidenceContent::inline("no violation").unwrap(),
            ProvenanceId::new("prov-1").unwrap(),
            vec![EvidenceLink::new(
                source_fact.id().clone(),
                EvidenceRelation::Supports,
            )],
        )
        .unwrap();
        let challenge = Evidence::new(
            EvidenceId::new("evidence-challenge").unwrap(),
            EvidenceKind::ModelOutput,
            "model challenge",
            EvidenceContent::inline("review needed").unwrap(),
            ProvenanceId::new("prov-1").unwrap(),
            vec![EvidenceLink::new(
                source_fact.id().clone(),
                EvidenceRelation::Challenges,
            )],
        )
        .unwrap();
        let snapshot = normalize_current_state(
            ObservedStateId::new("state-evidence-conflict").unwrap(),
            NormalizationInput::new(records(
                vec![source_fact],
                vec![observation],
                vec![support, challenge],
            )),
        )
        .unwrap();
        let entry = &snapshot.entries()[0];
        assert_eq!(entry.status(), StateStatus::Conflicted);
        assert_eq!(entry.claims()[0].polarity(), AssertionPolarity::Negated);
        assert_eq!(entry.claims()[0].supporting_evidence().len(), 1);
        assert_eq!(entry.claims()[0].challenging_evidence().len(), 1);
    }

    #[test]
    fn unresolved_quality_conflict_marks_an_entry_conflicted() {
        let observation = Observation::new(
            ObservationId::new("observation-1").unwrap(),
            SubjectPath::from_str("coverage.percent").unwrap(),
            TypedValue::Integer(92),
            ProvenanceId::new("prov-1").unwrap(),
        )
        .unwrap();
        let source_fact = fact(
            "fact-1",
            "observation-1",
            "coverage.percent",
            TypedValue::Integer(92),
        );
        let subject = SubjectPath::from_str("coverage.percent").unwrap();
        let quality = QualityMetadata::new(
            TrustClass::DerivedAssessment,
            SensitivityClass::Normal,
            Confidence::Unknown,
            FreshnessStatus::Unknown,
            Uncertainty::Unknown,
        )
        .with_conflict(ConflictStatus::Unresolved);
        let snapshot = normalize_current_state(
            ObservedStateId::new("state-quality-conflict").unwrap(),
            NormalizationInput::new(records(vec![source_fact], vec![observation], vec![]))
                .with_quality_metadata(subject, vec![quality]),
        )
        .unwrap();
        assert_eq!(snapshot.entries()[0].status(), StateStatus::Conflicted);
        assert_eq!(
            snapshot.entries()[0].metadata().unwrap().conflict(),
            ConflictStatus::Unresolved
        );
    }

    #[test]
    fn defensive_claim_normalization_rejects_missing_observations() {
        let source_fact = fact(
            "fact-1",
            "missing-observation",
            "coverage.percent",
            TypedValue::Integer(92),
        );
        let observations = BTreeMap::new();
        let evidence = BTreeMap::new();
        assert!(matches!(
            normalized_claim(&source_fact, &observations, &evidence),
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "observation",
                ..
            })
        ));
    }
}
