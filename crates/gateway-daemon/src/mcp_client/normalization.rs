//! Provider-neutral scalar normalization profile for the admitted MCP mapper.
use super::{
    Error, ProtocolResult,
    mapping::{ConnectorResultNormalizer, MappingRecord},
    schema,
};
use gateway_application::{connector_results::*, connectors::*};
use gateway_domain::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MAX_CANONICAL_INPUT_BYTES: usize = 262_144;

/// Only an admitted scalar normalizer can construct this output proof.
pub(super) struct ScalarBounds {
    source: SourceId,
    subjects: BTreeSet<SubjectPath>,
}
impl ScalarBounds {
    pub(super) fn source(&self) -> &SourceId {
        &self.source
    }
    pub(super) fn subjects(&self) -> &BTreeSet<SubjectPath> {
        &self.subjects
    }
    pub(super) fn value_bytes(&self) -> u64 {
        // Native TypedValue scalar serde adds {kind,value}, at most 32 bytes
        // per field, to the same JSON scalar represented in the bounded input.
        MAX_CANONICAL_INPUT_BYTES as u64 + 32 * self.subjects.len() as u64
    }
}

/// Immutable CG-owned projection; names never come from server instructions.
#[derive(Debug, Clone)]
pub struct NormalizationProfile {
    pub source: SourceId,
    pub source_kind: SourceKind,
    pub object_field: String,
    pub revision_field: Option<String>,
    pub digest_field: Option<String>,
    pub fields: BTreeMap<String, SubjectPath>,
}
/// Metadata from an admitted collector, separate from the provider payload.
#[derive(Debug, Clone)]
pub struct Acquisition {
    pub acquired_at: UnixTimestamp,
    pub observed_at: Option<UnixTimestamp>,
    pub evaluated_at: UnixTimestamp,
    pub max_age: FreshnessPolicy,
    pub authenticated_source: NonEmptyText,
    pub sensitivity: SensitivityClass,
    /// Explicit collector assessment under its authenticated source contract.
    /// Unassessed sources must retain Unknown; completeness alone is no proof.
    pub confidence: Confidence,
    pub uncertainty: Uncertainty,
    pub conflict: ConflictStatus,
    pub partial: bool,
    pub basis: ConnectorObservationBasis,
    pub effect: ConnectorEffect,
    pub verifies: Option<ConnectorRequest>,
    pub non_authoritative_raw: Option<ReferenceId>,
}
/// Host-owned authentication/time/handling authority. No default implementation
/// and no provider JSON is passed to this port. #229 owns credential resolution.
pub trait AcquisitionPort: Send + Sync {
    fn collect(&self, dispatch: &ConnectorDispatch) -> Result<Acquisition, Error>;
}
/// Trusted collector contract for the qualified direct acquisition path.
/// Implementations perform bounded read-only host metadata lookups only: no
/// connector/model/embedding I/O, no secret material in returned metadata.
/// Implementing this trait is a source-owner assertion, never inferred from stdio.
pub trait QualifiedAcquisitionPort: AcquisitionPort {}

pub struct CanonicalResultNormalizer {
    profile: NormalizationProfile,
    acquisition: Box<dyn AcquisitionPort>,
    qualified: bool,
}
impl CanonicalResultNormalizer {
    pub fn new(
        profile: NormalizationProfile,
        acquisition: Box<dyn AcquisitionPort>,
    ) -> Result<Self, Error> {
        if profile.fields.is_empty()
            || profile.fields.len() > 256
            || profile
                .fields
                .keys()
                .any(|k| k.is_empty() || k == &profile.object_field)
            || profile.object_field.is_empty()
            || profile
                .fields
                .values()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != profile.fields.len()
        {
            return Err(Error::Configuration);
        }
        Ok(Self {
            profile,
            acquisition,
            qualified: false,
        })
    }
    pub fn new_qualified(
        profile: NormalizationProfile,
        acquisition: Box<dyn QualifiedAcquisitionPort>,
    ) -> Result<Self, Error> {
        let mut value = Self::new(profile, acquisition)?;
        value.qualified = true;
        Ok(value)
    }
    pub(super) fn resource_bounds(&self) -> Result<ScalarBounds, Error> {
        if !self.qualified {
            return Err(Error::Unsupported);
        }
        Ok(ScalarBounds {
            source: self.profile.source.clone(),
            subjects: self.profile.fields.values().cloned().collect(),
        })
    }
    pub fn canonicalize(
        &self,
        d: &ConnectorDispatch,
        source: Option<&SourceId>,
        mapping: &MappingRecord,
        result: ProtocolResult,
    ) -> Result<ConnectorResult, Error> {
        let p = &self.profile;
        if mapping.source.as_ref() != Some(&p.source)
            || source.is_some_and(|s| s != &p.source)
            || mapping.capability != d.request.capability
            || mapping.class.operation_class() != d.request.class
        {
            return Err(Error::Schema);
        }
        let value = match result {
            ProtocolResult::Tool {
                structured: Some(v),
                is_error: false,
                ..
            } if mapping.kind == gateway_application::connector_manifest::InventoryKind::Tool => {
                schema::accepts(mapping.result_schema.as_ref().ok_or(Error::Schema)?, &v)?;
                v
            }
            // Text resources are content, never parsed as an authoritative JSON envelope.
            ProtocolResult::Resource { contents }
                if mapping.kind
                    == gateway_application::connector_manifest::InventoryKind::Resource =>
            {
                if contents.len() != 1 {
                    return Err(Error::Schema);
                }
                let c = &contents[0];
                let uri = c.get("uri").and_then(|v| v.as_str()).ok_or(Error::Schema)?;
                let text = c
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or(Error::Schema)?;
                if p.fields.len() != 1 || p.revision_field.is_some() || p.digest_field.is_some() {
                    return Err(Error::Schema);
                }
                serde_json::json!({p.object_field.clone(): uri, p.fields.keys().next().ok_or(Error::Schema)?.clone(): text})
            }
            _ => return Err(Error::Schema),
        };
        let bytes = serde_json::to_vec(&value).map_err(|_| Error::Schema)?;
        if bytes.len() > MAX_CANONICAL_INPUT_BYTES {
            return Err(Error::Limit);
        }
        let string = |key: &str| value.get(key).and_then(|v| v.as_str()).ok_or(Error::Schema);
        let object_reference =
            NonEmptyText::new(string(&p.object_field)?).map_err(|_| Error::Schema)?;
        let source_revision = p
            .revision_field
            .as_ref()
            .and_then(|key| value.get(key))
            .map(|v| {
                SourceTimestamp::new(v.as_str().ok_or(Error::Schema)?).map_err(|_| Error::Schema)
            })
            .transpose()?;
        let source_digest = p
            .digest_field
            .as_ref()
            .and_then(|key| value.get(key))
            .map(|v| {
                ContentDigest::new(v.as_str().ok_or(Error::Schema)?).map_err(|_| Error::Schema)
            })
            .transpose()?;
        let digest = ContentDigest::new(format!("{:x}", Sha256::digest(&bytes)))
            .map_err(|_| Error::Schema)?;
        let a = self.acquisition.collect(d)?;
        if a.acquired_at > a.evaluated_at || a.observed_at.is_some_and(|t| t > a.acquired_at) {
            return Err(Error::Schema);
        }
        let freshness = evaluate_freshness(a.observed_at, Some(a.evaluated_at), a.max_age)
            .map_err(|_| Error::Schema)?;
        let quality = QualityMetadata::new(
            TrustClass::ObservedEvidence,
            a.sensitivity,
            a.confidence,
            freshness,
            if a.partial {
                Uncertainty::Incomplete
            } else {
                a.uncertainty
            },
        )
        .with_conflict(a.conflict);
        // Identity binds the entire admitted request as well as content. It is
        // deterministic across replay and cannot collide across scopes/dispatches.
        let r = &d.request;
        let arguments: BTreeMap<_, _> = r
            .arguments
            .iter()
            .map(|(k, v)| (k.as_str(), v.to_string()))
            .collect();
        let identity_bytes = serde_json::to_vec(&serde_json::json!({
            "result_version":"1", "connector":r.target.connector.as_str(), "scope":r.target.scope.as_str(),
            "capability":r.capability.as_str(), "class":mapping.class, "inventory":r.inventory_revision.as_str(),
            "arguments":arguments, "task":r.task.as_str(), "step":r.step.as_str(), "execution":r.execution.as_str(),
            "correlation":r.correlation.as_str(), "object":object_reference.as_str(), "content_digest":digest.as_str(),
            "acquired_at":a.acquired_at.seconds(), "source":p.source.as_str(), "fields":p.fields.iter().map(|(k,v)| (k.as_str(),v.to_string())).collect::<BTreeMap<_,_>>()
        })).map_err(|_| Error::Schema)?;
        let identity = format!("{:x}", Sha256::digest(&identity_bytes));
        let pid =
            ProvenanceId::new(format!("connector.p.{identity}")).map_err(|_| Error::Schema)?;
        let mut provenance = Provenance::new(
            pid.clone(),
            p.source_kind,
            p.source.clone(),
            object_reference.as_str(),
        )
        .and_then(|v| v.with_producer(a.authenticated_source.as_str()))
        .map_err(|_| Error::Schema)?
        .with_acquired_at(
            SourceTimestamp::new(a.acquired_at.seconds().to_string()).map_err(|_| Error::Schema)?,
        );
        if let Some(revision) = &source_revision {
            provenance = provenance.with_source_timestamp(revision.clone());
        }
        let mut observations = vec![];
        let mut facts = vec![];
        let mut evidence = vec![];
        for (index, (field, subject)) in p.fields.iter().enumerate() {
            let v = value.get(field).ok_or(Error::Schema)?;
            let typed = if let Some(v) = v.as_str() {
                TypedValue::String(NonEmptyText::new(v).map_err(|_| Error::Schema)?)
            } else if let Some(v) = v.as_bool() {
                TypedValue::Boolean(v)
            } else if let Some(v) = v.as_i64() {
                TypedValue::Integer(v)
            } else {
                return Err(Error::Schema);
            };
            let oid = ObservationId::new(format!("connector.o.{identity}.{index}"))
                .map_err(|_| Error::Schema)?;
            let fid = FactId::new(format!("connector.f.{identity}.{index}"))
                .map_err(|_| Error::Schema)?;
            observations.push(
                Observation::new(oid.clone(), subject.clone(), typed.clone(), pid.clone())
                    .map_err(|_| Error::Schema)?,
            );
            facts.push(
                Fact::new(
                    fid.clone(),
                    subject.clone(),
                    typed,
                    AssertionPolarity::Affirmed,
                    vec![oid],
                )
                .map_err(|_| Error::Schema)?,
            );
            evidence.push(
                Evidence::new(
                    EvidenceId::new(format!("connector.e.{identity}.{index}"))
                        .map_err(|_| Error::Schema)?,
                    EvidenceKind::ToolOutput,
                    "Connector reported value",
                    EvidenceContent::inline(v.to_string()).map_err(|_| Error::Schema)?,
                    pid.clone(),
                    vec![EvidenceLink::new(fid, EvidenceRelation::Supports)],
                )
                .map_err(|_| Error::Schema)?,
            );
        }
        let records = ObservationEvidenceSet::new(vec![provenance], observations, facts, evidence)
            .map_err(|_| Error::Schema)?;
        ConnectorResult::new(
            d,
            ConnectorObservation {
                target: d.request.target.clone(),
                inventory_revision: d.request.inventory_revision.clone(),
                correlation: d.request.correlation.clone(),
                source: p.source.clone(),
                source_revision,
                content_digest: digest,
                acquired_at: a.acquired_at,
                records,
                basis: a.basis,
                metadata: ConnectorResultMetadata {
                    task: d.request.task.clone(),
                    step: d.request.step.clone(),
                    execution: d.request.execution.clone(),
                    object_reference,
                    source_kind: p.source_kind,
                    source_digest,
                    observed_at: a.observed_at,
                    authenticated_source: a.authenticated_source,
                    quality,
                    effect: a.effect,
                    partial: a.partial,
                    verifies: a.verifies,
                    non_authoritative_raw: a.non_authoritative_raw,
                },
            },
        )
        .map_err(|_| Error::Schema)
    }
}
impl ConnectorResultNormalizer for CanonicalResultNormalizer {
    fn normalize(
        &self,
        d: &ConnectorDispatch,
        source: Option<&SourceId>,
        mapping: &MappingRecord,
        result: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        self.canonicalize(d, source, mapping, result)
            .map(ConnectorResult::into_observation)
    }
}

#[cfg(test)]
#[path = "normalization_tests.rs"]
mod tests;
