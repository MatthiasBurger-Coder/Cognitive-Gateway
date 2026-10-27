//! Canonical connector result boundary. Trusted acquisition metadata is supplied
//! by the collecting host, never decoded from provider quality/authority claims.
use crate::{ScopedObservationBatch, SourceSnapshot, connectors::*};
use gateway_context::{ContextFragment, FragmentMetadata};
use gateway_domain::*;
use std::collections::BTreeSet;

/// Correlation and handling information retained alongside existing CG-06 lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorResultMetadata {
    pub task: TaskId,
    pub step: PlanStepId,
    pub execution: NonEmptyText,
    pub object_reference: NonEmptyText,
    pub source_kind: SourceKind,
    pub source_digest: Option<ContentDigest>,
    pub observed_at: Option<UnixTimestamp>,
    /// Authenticated principal/endpoint reference, supplied by the host. No secret.
    pub authenticated_source: NonEmptyText,
    pub quality: QualityMetadata,
    pub effect: ConnectorEffect,
    pub partial: bool,
    /// Separate original effect request linked by a subsequent admitted read.
    pub verifies: Option<ConnectorRequest>,
    /// Content address of optional opaque archival data. Never ingested as evidence.
    pub non_authoritative_raw: Option<ReferenceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultError {
    Binding,
    Lineage,
    Quality,
    Basis,
}

/// A validated result; provider JSON has no representation here. Existing
/// observation/evidence records remain the only normalized content representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorResult(ConnectorObservation);

/// Content-free diagnostics; none grants authority or resolves an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultDiagnostic {
    AcknowledgementOnly,
    Stale,
    Conflict,
    Partial,
    UncertainEffect,
}

impl ConnectorResult {
    pub fn new(
        dispatch: &ConnectorDispatch,
        observation: ConnectorObservation,
    ) -> Result<Self, ResultError> {
        Self::validate(dispatch, &observation)?;
        Ok(Self(observation))
    }
    /// Revalidate public port records before ingestion, including custom adapters.
    pub fn validate(d: &ConnectorDispatch, o: &ConnectorObservation) -> Result<(), ResultError> {
        if o.acquired_at > d.deadline {
            return Err(ResultError::Binding);
        }
        Self::validate_for_request(&d.request, o)
    }
    /// Validate public-port result data against its exact request. The actual
    /// connector host still owns current policy admission and dispatch deadline;
    /// this method never constructs or substitutes an authorization grant.
    pub fn validate_for_request(
        r: &ConnectorRequest,
        o: &ConnectorObservation,
    ) -> Result<(), ResultError> {
        let m = &o.metadata;
        if o.target != r.target
            || o.inventory_revision != r.inventory_revision
            || o.correlation != r.correlation
            || m.task != r.task
            || m.step != r.step
            || m.execution != r.execution
            || m.observed_at.is_some_and(|t| t > o.acquired_at)
        {
            return Err(ResultError::Binding);
        }
        if m.source_kind == SourceKind::Model
            || m.quality.trust() != TrustClass::ObservedEvidence
            || (m.partial && m.quality.uncertainty() == Uncertainty::None)
        {
            return Err(ResultError::Quality);
        }
        let read = matches!(
            r.class,
            ConnectorOperationClass::Read
                | ConnectorOperationClass::Search
                | ConnectorOperationClass::Inspect
        );
        match o.basis {
            ConnectorObservationBasis::ProviderAcknowledgement => {
                if m.verifies.is_some() || m.effect == ConnectorEffect::NotDispatched {
                    return Err(ResultError::Basis);
                }
            }
            ConnectorObservationBasis::SourceRead => {
                if !read || m.verifies.is_some() || m.effect != ConnectorEffect::NoMutation {
                    return Err(ResultError::Basis);
                }
            }
            ConnectorObservationBasis::IndependentPostEffectRead => {
                let original = m.verifies.as_ref().ok_or(ResultError::Basis)?;
                if !read
                    || m.effect != ConnectorEffect::NoMutation
                    || original.target != r.target
                    || original.task != r.task
                    || original.correlation == r.correlation
                    || original.execution == r.execution
                    || !matches!(
                        original.class,
                        ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
                    )
                {
                    return Err(ResultError::Basis);
                }
            }
        }
        o.records.validate().map_err(|_| ResultError::Lineage)?;
        if o.records.observations().is_empty()
            || o.records.evidence().is_empty()
            || o.records.observations().len() > 256
            || o.records.provenances().len() > 256
            || o.records.facts().len() > 256
            || o.records.evidence().len() > 256
        {
            return Err(ResultError::Lineage);
        }
        // CG-06 checks dangling edges; this boundary additionally rejects cycles.
        let mut resolved = BTreeSet::new();
        loop {
            let before = resolved.len();
            for p in o.records.provenances() {
                if p.parent_provenance().iter().all(|id| resolved.contains(id)) {
                    resolved.insert(p.id().clone());
                }
            }
            if resolved.len() == o.records.provenances().len() {
                break;
            }
            if before == resolved.len() {
                return Err(ResultError::Lineage);
            }
        }
        let acquired = o.acquired_at.seconds().to_string();
        let direct: BTreeSet<_> = o
            .records
            .observations()
            .iter()
            .map(|v| v.provenance())
            .chain(o.records.evidence().iter().map(|v| v.provenance()))
            .collect();
        for p in o
            .records
            .provenances()
            .iter()
            .filter(|p| direct.contains(p.id()))
        {
            if p.source_id() != &o.source
                || p.source_kind() != m.source_kind
                || p.source_reference() != m.object_reference.as_str()
                || p.source_timestamp() != o.source_revision.as_ref()
                || p.acquired_at().map(SourceTimestamp::as_str) != Some(acquired.as_str())
                || p.producer() != Some(m.authenticated_source.as_str())
            {
                return Err(ResultError::Lineage);
            }
        }
        Ok(())
    }
    pub fn diagnostics(&self) -> Vec<ResultDiagnostic> {
        let o = &self.0;
        let mut result = vec![];
        if o.basis == ConnectorObservationBasis::ProviderAcknowledgement {
            result.push(ResultDiagnostic::AcknowledgementOnly);
        }
        if o.metadata.quality.freshness() == FreshnessStatus::Stale {
            result.push(ResultDiagnostic::Stale);
        }
        if o.metadata.quality.conflict() == ConflictStatus::Unresolved {
            result.push(ResultDiagnostic::Conflict);
        }
        if o.metadata.partial {
            result.push(ResultDiagnostic::Partial);
        }
        if o.metadata.effect == ConnectorEffect::Unknown {
            result.push(ResultDiagnostic::UncertainEffect);
        }
        result
    }
    pub fn observation(&self) -> &ConnectorObservation {
        &self.0
    }
    pub fn into_observation(self) -> ConnectorObservation {
        self.0
    }
    /// Acknowledgements cannot become observed state or context. Completion
    /// remains CG-14's decision even for an independent read-back.
    pub fn batch(&self) -> Result<ScopedObservationBatch, ResultError> {
        let o = &self.0;
        if o.basis == ConnectorObservationBasis::ProviderAcknowledgement {
            return Err(ResultError::Basis);
        }
        let snapshot = SourceSnapshot::new(
            o.source.clone(),
            o.metadata.source_kind,
            o.source_revision.clone(),
            Some(o.content_digest.clone()),
        )
        .map_err(|_| ResultError::Lineage)?;
        let mut batch =
            ScopedObservationBatch::new(o.target.scope.clone(), snapshot, o.records.clone())
                .map_err(|_| ResultError::Lineage)?;
        for observation in o.records.observations() {
            batch = batch
                .with_quality_metadata(observation.subject().clone(), vec![o.metadata.quality]);
        }
        Ok(batch)
    }
    /// Reference-only EPIC-02/CG-10 handoff. Disclosure and selection still apply.
    pub fn fragments(&self) -> Result<Vec<ContextFragment>, ResultError> {
        self.batch()?;
        let o = &self.0;
        o.records
            .evidence()
            .iter()
            .map(|e| {
                let p = o
                    .records
                    .provenances()
                    .iter()
                    .find(|p| p.id() == e.provenance())
                    .ok_or(ResultError::Lineage)?;
                let metadata = FragmentMetadata {
                    provenance: KnowledgeProvenance::new(
                        p.source_reference(),
                        p.source_timestamp().map(SourceTimestamp::as_str),
                    )
                    .map_err(|_| ResultError::Lineage)?,
                    evidence: BTreeSet::new(),
                    quality: o.metadata.quality,
                    rationale: NonEmptyText::new("Validated connector observation")
                        .map_err(|_| ResultError::Lineage)?,
                    validation: None,
                };
                ContextFragment::evidence(
                    ReferenceId::new(e.id().as_str()).map_err(|_| ResultError::Lineage)?,
                    e,
                    p,
                    metadata,
                    o.target.scope.clone(),
                    o.metadata.step.clone(),
                )
                .map_err(|_| ResultError::Lineage)
            })
            .collect()
    }
}
