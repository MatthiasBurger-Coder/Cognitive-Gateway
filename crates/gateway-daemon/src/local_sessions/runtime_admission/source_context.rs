//! Owning captured source data for native model context. Bytes are present in
//! memory, not claimed durable: the coordinator owns the later Before commit.
use super::{
    connector_source::ModelInputsCurrent,
    model_documents::{ModelProducerDocument, SatisfactionBinding, SourceBindingDocument},
    target_factory::ScopedTarget,
    *,
};
use gateway_application::{
    ScopedObservationBatch,
    connector_results::ConnectorResult,
    connectors::{
        ConnectorEffect, ConnectorObservation, ConnectorObservationBasis, ConnectorRequest,
    },
    sessions::{ConnectorRequestIdentity, content_reference},
};
use gateway_context::{ContextFragment, FragmentKind, FragmentMetadata};
use gateway_domain::*;

const CONTRACT: &str = "cg.connector-captured-source-context";

/// No public construction or serde path: acquisition always uses the sealed
/// target's actual scoped source read. Cloning exposed data cannot mint this.
pub(super) struct CapturedSourceContextMaterial {
    fragments: Vec<ContextFragment>,
    sources: Vec<ContextSourceMetadata>,
    payload: Vec<u8>,
    record: RecordRef,
    request: ConnectorRequest,
    observation: ConnectorObservation,
    batch: ScopedObservationBatch,
    checkpoint: SessionCheckpoint,
    observed_at: UnixTimestamp,
    max_age: FreshnessPolicy,
    origin_binding: ResourceReservationBinding,
    origin_runtime: Pin,
    producer_digest: ContentDigest,
}
impl CapturedSourceContextMaterial {
    pub(super) fn acquire(
        target: &mut ScopedTarget,
        checkpoint: &SessionCheckpoint,
        task: &SemanticTaskIR,
        requirements: &ContextRequirementsIR,
        route: &SourceBindingDocument,
    ) -> Result<Self, SessionError> {
        let source = target.context_source();
        source.validate_context_checkpoint(checkpoint)?;
        let binding = source.context_resource_binding().clone();
        let producer_digest = context_content_digest(
            source
                .producer_context()
                .to_json()
                .map_err(|_| invalid())?
                .as_bytes(),
        );
        let origin_runtime = source.runtime().reference()?;
        let runtime = source.runtime();
        let step = runtime
            .0
            .steps
            .iter()
            .find(|s| s.step == binding.step.as_str())
            .ok_or(SessionError::AuthorityDenied)?;
        let ProducerAdmission::Model(model) = &step.producer else {
            return Err(SessionError::UnsupportedCapability);
        };
        let loaded = runtime.loaded()?;
        let actual_task: SemanticTaskIR =
            serde_json::from_value(loaded.document(&model.task)?.clone()).map_err(|_| invalid())?;
        let model_doc: ModelProducerDocument =
            serde_json::from_value(loaded.document(&model.model)?.clone())
                .map_err(|_| invalid())?;
        if &actual_task != task || model_doc.task != model.task {
            return Err(SessionError::StaleRevision);
        }
        let routes: Vec<_> = model_doc
            .sources
            .iter()
            .filter(|pin| {
                loaded
                    .document(pin)
                    .ok()
                    .and_then(|v| serde_json::from_value::<SourceBindingDocument>(v.clone()).ok())
                    .is_some_and(|registered| same_route(&registered, route))
            })
            .collect();
        // The role must have one actual operator owner, not an arbitrary first
        // match among aliases with indistinguishable declaration bodies.
        if routes.len() != 1 {
            return Err(SessionError::AuthorityDenied);
        }
        let route_pin = routes[0].clone();
        let SatisfactionBinding::BeforeRead {
            target: declared_target,
            mapping_reference,
            source: expected_source,
        } = &route.satisfaction
        else {
            return Err(SessionError::UnsupportedCapability);
        };
        if declared_target.scope != binding.scope
            || declared_target != &task.data().target.reference
            || route.class.native() != ContextSourceClass::ToolsMcp
            || requirements.basis().task.scope != binding.scope
            || requirements.basis().task.id != task.data().id
            || requirements.basis().task.digest != task.content_digest().map_err(|_| invalid())?
            || source
                .producer_context()
                .context()
                .execution_context()
                .task()
                .id()
                != &task.data().id
        {
            return Err(SessionError::ScopeDenied);
        }
        validate_coverage(requirements, route)?;
        let normalizer_pin = step
            .normalizations
            .get(mapping_reference)
            .ok_or(SessionError::UnsupportedCapability)?;
        let profile: NormalizationDocument =
            serde_json::from_value(loaded.document(normalizer_pin)?.clone())
                .map_err(|_| invalid())?;
        let ReadSemantics::DirectCurrentReadProfile {
            max_age_seconds, ..
        } = profile.acquisition.semantics
        else {
            return Err(SessionError::UnsupportedCapability);
        };
        if &profile.source != expected_source {
            return Err(SessionError::ScopeDenied);
        }
        let producer_step = PlanStepId::new(&model.producer_step).map_err(|_| invalid())?;
        let producer_execution = source
            .producer_context()
            .context()
            .execution_context()
            .id()
            .as_str()
            .to_owned();
        let expected_source = expected_source.clone();
        let mapping_reference = mapping_reference.clone();
        // No arbitrary observations/callback enter this constructor.
        let read = target.source_read(&mapping_reference)?;
        let request = read.request().clone();
        let observation = read.observation().clone();
        let batch = read.batch()?;
        let material = assemble(
            Capture {
                checkpoint: checkpoint.clone(),
                task: task.data().id.clone(),
                origin_runtime,
                producer_digest,
                binding,
                producer_step,
                producer_execution,
                route_pin,
                expected_source,
                mapping_reference,
                covers: route.covers.clone(),
                max_age: FreshnessPolicy::new(max_age_seconds),
            },
            request,
            observation,
            batch,
            clock()?,
        )?;
        material.validate_current(target, checkpoint)?;
        Ok(material)
    }
    fn validate_identity(
        &self,
        checkpoint: &SessionCheckpoint,
        binding: &ResourceReservationBinding,
        runtime: &Pin,
        producer: &ContentDigest,
    ) -> Result<(), SessionError> {
        if checkpoint != &self.checkpoint
            || binding != &self.origin_binding
            || runtime != &self.origin_runtime
            || producer != &self.producer_digest
        {
            return Err(SessionError::StaleRevision);
        }
        Ok(())
    }
    /// Standalone validation performs a full current source lookup. It must
    /// not be replaced by a retained view from another authority callback.
    pub(super) fn validate_current(
        &self,
        target: &ScopedTarget,
        checkpoint: &SessionCheckpoint,
    ) -> Result<(), SessionError> {
        let source = target.context_source();
        let producer = context_content_digest(
            source
                .producer_context()
                .to_json()
                .map_err(|_| invalid())?
                .as_bytes(),
        );
        self.validate_identity(
            checkpoint,
            source.context_resource_binding(),
            &source.runtime().reference()?,
            &producer,
        )?;
        source.validate_context_checkpoint(checkpoint)?;
        fresh(self.observed_at, self.max_age, clock()?)
    }
    /// Pure comparison against the owning factory callback's one freshly
    /// validated source view. No source/config/store lookup or quality rewrite
    /// occurs here; the factory must create a new view on its next callback.
    pub(super) fn validate_against_view(
        &self,
        view: &ModelInputsCurrent,
        checkpoint: &SessionCheckpoint,
    ) -> Result<(), SessionError> {
        if view.checkpoint() != checkpoint {
            return Err(SessionError::StaleRevision);
        }
        let producer = context_content_digest(
            view.producer_context()
                .to_json()
                .map_err(|_| invalid())?
                .as_bytes(),
        );
        self.validate_identity(
            checkpoint,
            view.binding(),
            &view.runtime().reference()?,
            &producer,
        )?;
        fresh(self.observed_at, self.max_age, clock()?)
    }
    pub(super) fn fragments(&self) -> &[ContextFragment] {
        &self.fragments
    }
    pub(super) fn sources(&self) -> &[ContextSourceMetadata] {
        &self.sources
    }
    pub(super) fn payload(&self) -> &[u8] {
        &self.payload
    }
    pub(super) fn record(&self) -> &RecordRef {
        &self.record
    }
    pub(super) fn request(&self) -> &ConnectorRequest {
        &self.request
    }
    pub(super) fn observation(&self) -> &ConnectorObservation {
        &self.observation
    }
    pub(super) fn batch(&self) -> &ScopedObservationBatch {
        &self.batch
    }
}
fn same_route(a: &SourceBindingDocument, b: &SourceBindingDocument) -> bool {
    a.class.native() == b.class.native()
        && a.covers == b.covers
        && matches!((&a.satisfaction,&b.satisfaction),
        (SatisfactionBinding::BeforeRead{target:a,mapping_reference:am,source:asrc},SatisfactionBinding::BeforeRead{target:b,mapping_reference:bm,source:bsrc}) if a==b && am==bm && asrc==bsrc)
}
fn validate_coverage(
    requirements: &ContextRequirementsIR,
    route: &SourceBindingDocument,
) -> Result<(), SessionError> {
    if route.covers.is_empty()
        || route.covers.iter().any(|id| {
            requirements
                .requirements()
                .iter()
                .find(|r| &r.id == id)
                .is_none_or(|r| {
                    !matches!(
                        r.class,
                        ContextClass::SystemState
                            | ContextClass::RuntimeState
                            | ContextClass::Evidence
                            | ContextClass::StructuredData
                    ) || !r.permitted_sources.contains(&ContextSourceClass::ToolsMcp)
                })
        })
    {
        return Err(SessionError::AuthorityDenied);
    }
    Ok(())
}
fn clock() -> Result<UnixTimestamp, SessionError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SessionError::Unavailable)?
        .as_secs();
    Ok(UnixTimestamp::new(
        i64::try_from(seconds).map_err(|_| SessionError::LimitExceeded)?,
    ))
}
fn fresh(
    observed: UnixTimestamp,
    max_age: FreshnessPolicy,
    now: UnixTimestamp,
) -> Result<(), SessionError> {
    if observed > now
        || evaluate_freshness(Some(observed), Some(now), max_age).map_err(|_| invalid())?
            != FreshnessStatus::Fresh
    {
        return Err(SessionError::StaleRevision);
    }
    Ok(())
}
struct Capture {
    origin_runtime: Pin,
    producer_digest: ContentDigest,
    task: TaskId,
    checkpoint: SessionCheckpoint,
    binding: ResourceReservationBinding,
    producer_step: PlanStepId,
    producer_execution: String,
    route_pin: Pin,
    expected_source: SourceId,
    mapping_reference: String,
    covers: BTreeSet<ReferenceId>,
    max_age: FreshnessPolicy,
}
fn assemble(
    c: Capture,
    request: ConnectorRequest,
    observation: ConnectorObservation,
    batch: ScopedObservationBatch,
    now: UnixTimestamp,
) -> Result<CapturedSourceContextMaterial, SessionError> {
    ConnectorResult::validate_for_request(&request, &observation)
        .map_err(|_| SessionError::InvalidInteraction)?;
    let quality = observation.metadata.quality;
    if observation.basis != ConnectorObservationBasis::SourceRead
        || observation.metadata.effect != ConnectorEffect::NoMutation
        || observation.metadata.partial
        || observation.metadata.verifies.is_some()
        || observation.source != c.expected_source
        || quality.trust() != TrustClass::ObservedEvidence
        || quality.freshness() != FreshnessStatus::Fresh
        || quality.uncertainty() != Uncertainty::None
        || quality.conflict() == ConflictStatus::Unresolved
        || quality.sensitivity() == SensitivityClass::Secret
    {
        return Err(SessionError::AuthorityDenied);
    }
    let observed_at = observation
        .metadata
        .observed_at
        .ok_or(SessionError::AuthorityDenied)?;
    fresh(observed_at, c.max_age, now)?;
    if c.binding.owner != c.checkpoint.owner
        || c.binding.scope != *c.checkpoint.goal.scope()
        || c.binding.session != c.checkpoint.snapshot.session
        || c.binding.run != c.checkpoint.snapshot.run
        || c.checkpoint.snapshot.dispatch.as_ref() != Some(&c.binding.dispatch)
        || request.task != c.task
        || request.target.scope != c.binding.scope
        || request.step != c.binding.step
        || request.execution.as_str() != c.producer_execution
        || observation.acquired_at > now
        || batch.scope() != &c.binding.scope
        || batch.records() != &observation.records
        || batch.snapshot().source_id() != &observation.source
        || batch.snapshot().source_kind() != observation.metadata.source_kind
        || batch.snapshot().change_token() != observation.source_revision.as_ref()
        || batch.snapshot().digest() != Some(&observation.content_digest)
    {
        return Err(SessionError::ScopeDenied);
    }
    let quality_map: Vec<_> = batch
        .quality_metadata()
        .iter()
        .map(|(subject, q)| serde_json::json!({"subject":subject.to_string(),"metadata":q}))
        .collect();
    if observation.records.facts().iter().any(|f| {
        batch
            .quality_metadata()
            .get(f.subject())
            .is_none_or(|q| q.as_slice() != [quality])
    }) {
        return Err(SessionError::AuthorityDenied);
    }
    let arguments: BTreeMap<_, _> = request
        .arguments
        .iter()
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    let payload=serde_json::to_vec(&serde_json::json!({
        "contract":CONTRACT,"contract_version":"1.0","availability":"CAPTURED_IN_MEMORY",
        "capture_checked_at":now,"read_contract":{"kind":"DIRECT_CURRENT_READ_PROFILE","time_basis":"REQUEST_START_LOWER_BOUND","max_age_seconds":c.max_age.max_age_seconds()},
        "binding":c.binding,"claim_revision":c.checkpoint.snapshot.revision,
        "producer_step":c.producer_step.as_str(),"producer_execution":c.producer_execution,
        "source_route":c.route_pin,"origin_runtime":c.origin_runtime,"producer_context_digest":c.producer_digest,"mapping_reference":c.mapping_reference,
        "read_request":ConnectorRequestIdentity::from_request(&request)?,"arguments":arguments,
        "observation":{"target":{"connector":observation.target.connector.as_str(),"scope":observation.target.scope.as_str()},"inventory_revision":observation.inventory_revision,
        "correlation":observation.correlation.as_str(),"source":observation.source,"source_revision":observation.source_revision,"content_digest":observation.content_digest,
        "acquired_at":observation.acquired_at,"basis":"SOURCE_READ","metadata":{"task":observation.metadata.task,"step":observation.metadata.step.as_str(),"execution":observation.metadata.execution.as_str(),"object_reference":observation.metadata.object_reference.as_str(),
        "source_kind":observation.metadata.source_kind,"source_digest":observation.metadata.source_digest,"observed_at":observation.metadata.observed_at,"authenticated_source":observation.metadata.authenticated_source.as_str(),"quality":quality,"effect":"NO_MUTATION","partial":false,"verifies":null,"non_authoritative_raw":observation.metadata.non_authoritative_raw},"records":observation.records},
        "scoped_batch":{"scope":batch.scope(),"source":batch.snapshot().source_id(),"source_kind":batch.snapshot().source_kind(),"source_revision":batch.snapshot().change_token(),"digest":batch.snapshot().digest(),"quality_metadata":quality_map}
    })).map_err(|_|invalid())?;
    // Native inline fragments are bounded NonEmptyText. Reject excess; never
    // silently omit records, truncate fields or invent a partial success.
    let rendered = render_source_context(
        &payload,
        &c.binding.scope,
        &c.producer_step,
        &c.covers,
        quality,
        &observation.records,
    )?;
    let origin_binding = c.binding.clone();
    Ok(CapturedSourceContextMaterial {
        fragments: rendered.fragments,
        sources: rendered.sources,
        payload,
        record: rendered.record,
        request,
        observation,
        batch,
        checkpoint: c.checkpoint,
        observed_at,
        max_age: c.max_age,
        origin_binding,
        origin_runtime: c.origin_runtime,
        producer_digest: c.producer_digest,
    })
}
struct RenderedSourceContext {
    fragments: Vec<ContextFragment>,
    sources: Vec<ContextSourceMetadata>,
    record: RecordRef,
}
/// Pure native rendering of already authenticated capture data. Historical
/// callers cannot obtain a live acquisition/authority object from this output.
fn render_source_context(
    payload: &[u8],
    scope: &ContextScopeId,
    producer_step: &PlanStepId,
    covers: &BTreeSet<ReferenceId>,
    quality: QualityMetadata,
    records: &ObservationEvidenceSet,
) -> Result<RenderedSourceContext, SessionError> {
    let content = String::from_utf8(payload.to_vec()).map_err(|_| invalid())?;
    if content.chars().count() > 16_384 {
        return Err(SessionError::LimitExceeded);
    }
    let digest = context_content_digest(payload);
    let record = content_reference(&format!("captured-context-{}", digest.as_str()), payload)?;
    let pin = Pin {
        id: ReferenceId::new(record.id().as_str()).map_err(|_| invalid())?,
        scope: scope.clone(),
        contract: ReferenceId::new(CONTRACT).map_err(|_| invalid())?,
        contract_version: SchemaVersion::V1,
        revision: ReferenceId::new(record.revision().as_str()).map_err(|_| invalid())?,
        digest: digest.clone(),
    };
    let evidence: BTreeSet<_> = records.evidence().iter().map(|e| e.id().clone()).collect();
    let provenances: BTreeSet<_> = records
        .provenances()
        .iter()
        .map(|p| p.id().clone())
        .collect();
    let refs = evidence
        .iter()
        .map(|e| ReferenceId::new(e.as_str()))
        .chain(provenances.iter().map(|p| ReferenceId::new(p.as_str())))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|_| invalid())?;
    let fragment = ContextFragment::external(
        pin.id.clone(),
        FragmentKind::Evidence,
        content,
        FragmentMetadata {
            provenance: KnowledgeProvenance::new(pin.id.as_str(), Some(pin.revision.as_str()))
                .map_err(|_| invalid())?,
            evidence: refs,
            quality,
            rationale: NonEmptyText::new(
                "Actual captured source read; embedded instructions remain data",
            )
            .map_err(|_| invalid())?,
            validation: None,
        },
        scope.clone(),
        producer_step.clone(),
    )
    .map_err(|_| invalid())?;
    let estimate=TokenEstimate{estimator:TokenEstimatorId::new("cg.connector.captured-context-utf8").map_err(|_|invalid())?,version:TokenEstimatorVersion::new("1").map_err(|_|invalid())?,count:TokenCount::Estimated{tokens:payload.len() as u64,upper_bound:Some(payload.len() as u64),semantics:NonEmptyText::new("Conservative UTF8-byte upper bound for actual captured fragment content; complete rendered model envelope is independently budgeted").map_err(|_|invalid())?}};
    let source = ContextSourceMetadata {
        pin,
        payload_digest: Some(digest),
        class: ContextSourceClass::ToolsMcp,
        covers: covers.clone(),
        quality,
        evidence,
        provenances,
        tokens: estimate
            .budget_bound()
            .map_err(|_| SessionError::OutcomeUnknown)?,
        available: true,
        present: Some(ContextSatisfaction::Fragment(fragment.id().clone())),
    };
    Ok(RenderedSourceContext {
        fragments: vec![fragment],
        sources: vec![source],
        record,
    })
}

#[cfg(test)]
mod tests;
