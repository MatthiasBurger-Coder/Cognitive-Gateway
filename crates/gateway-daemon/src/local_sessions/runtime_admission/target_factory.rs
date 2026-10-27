//! Source-bound local scalar target. Pricing, freshness and read roles are
//! explicit trusted contracts; transport success never proves goal completion.
use super::connector_source::{ConnectorSourceBasis, InventoryBinder, LocalConnectorSource};
use super::*;
use crate::mcp_client::{
    self, CallContext, HostConfig, Observer,
    credential_file::CredentialFileResolver,
    normalization::{
        Acquisition, AcquisitionPort, CanonicalResultNormalizer, NormalizationProfile,
        QualifiedAcquisitionPort,
    },
    scalar_quotes::{
        FixedLocalTariff, QualifiedScalarConfig, QualifiedScalarMetadata, qualified_scalar_host,
    },
    session_budget::{SessionConnectorComponents, SessionConnectorRun},
    transport::CredentialedStdioFactory,
};
use gateway_application::{
    connector_policy::Cg09ConnectorAdmission, connector_results::ConnectorResult, connectors::*,
    prompt_rendering::ToolDefinition, sessions::connector_request_digest,
};
use gateway_domain::{
    Confidence, ConflictStatus, FreshnessPolicy, NonEmptyText, Uncertainty, UnixTimestamp,
};
use std::sync::Mutex;

const ROLE_HINTS: usize = 512;
#[derive(Clone)]
enum ReadRole {
    SourceRead,
    IndependentPostEffectRead(Box<ConnectorRequest>),
    ProposalReturn,
}
#[derive(Clone)]
struct RoleHint {
    request: ConnectorRequest,
    source: Option<SourceId>,
    role: ReadRole,
    started_at: UnixTimestamp,
}
#[derive(Default)]
struct RoleHints(Mutex<BTreeMap<gateway_domain::ContentDigest, RoleHint>>);
impl RoleHints {
    fn insert(&self, hint: RoleHint) -> Result<(), SessionError> {
        let key = connector_request_digest(&hint.request)?;
        let mut hints = self.0.lock().map_err(|_| SessionError::AuthorityDenied)?;
        if hints.len() >= ROLE_HINTS || hints.contains_key(&key) {
            return Err(SessionError::LimitExceeded);
        }
        hints.insert(key, hint);
        Ok(())
    }
}
struct DirectCollector {
    profile: AcquisitionDocument,
    wire_source: Option<SourceId>,
    hints: Arc<RoleHints>,
}
fn clock() -> Result<UnixTimestamp, mcp_client::Error> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| mcp_client::Error::InvalidState)?
        .as_secs();
    Ok(UnixTimestamp::new(
        i64::try_from(seconds).map_err(|_| mcp_client::Error::Limit)?,
    ))
}
impl AcquisitionPort for DirectCollector {
    fn collect(&self, dispatch: &ConnectorDispatch) -> Result<Acquisition, mcp_client::Error> {
        let key = connector_request_digest(&dispatch.request)
            .map_err(|_| mcp_client::Error::Configuration)?;
        let hint = self
            .hints
            .0
            .lock()
            .map_err(|_| mcp_client::Error::InvalidState)?
            .get(&key)
            .cloned()
            .ok_or(mcp_client::Error::InvalidState)?;
        if hint.request != dispatch.request || hint.source != self.wire_source {
            return Err(mcp_client::Error::Configuration);
        }
        let now = clock()?;
        if hint.started_at > now {
            return Err(mcp_client::Error::InvalidState);
        }
        let read = matches!(
            dispatch.request.class,
            ConnectorOperationClass::Read
                | ConnectorOperationClass::Search
                | ConnectorOperationClass::Inspect
        );
        let (observed_at, max_age, basis, effect, verifies, qualified) = match &hint.role {
            ReadRole::ProposalReturn => (
                None,
                FreshnessPolicy::new(0),
                ConnectorObservationBasis::ProviderAcknowledgement,
                if read {
                    ConnectorEffect::NoMutation
                } else {
                    ConnectorEffect::Acknowledged
                },
                None,
                false,
            ),
            ReadRole::SourceRead | ReadRole::IndependentPostEffectRead(_) => {
                let ReadSemantics::DirectCurrentReadProfile {
                    max_age_seconds,
                    time_basis: ReadTimeBasis::RequestStartLowerBound,
                } = self.profile.semantics
                else {
                    return Err(mcp_client::Error::Configuration);
                };
                if !read {
                    return Err(mcp_client::Error::Configuration);
                }
                let (basis, verifies) = match &hint.role {
                    ReadRole::IndependentPostEffectRead(original) => {
                        if original.target != dispatch.request.target
                            || original.task != dispatch.request.task
                            || original.step != dispatch.request.step
                            || original.execution == dispatch.request.execution
                            || original.correlation == dispatch.request.correlation
                            || !matches!(
                                original.class,
                                ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
                            )
                        {
                            return Err(mcp_client::Error::Configuration);
                        }
                        (
                            ConnectorObservationBasis::IndependentPostEffectRead,
                            Some(original.as_ref().clone()),
                        )
                    }
                    _ => (ConnectorObservationBasis::SourceRead, None),
                };
                (
                    Some(hint.started_at),
                    FreshnessPolicy::new(max_age_seconds),
                    basis,
                    ConnectorEffect::NoMutation,
                    verifies,
                    true,
                )
            }
        };
        Ok(Acquisition {
            acquired_at: now,
            observed_at,
            evaluated_at: now,
            max_age,
            authenticated_source: self.profile.authenticated_profile.clone(),
            sensitivity: self.profile.sensitivity,
            confidence: if qualified {
                Confidence::NotApplicable
            } else {
                Confidence::Unknown
            },
            uncertainty: if qualified {
                Uncertainty::None
            } else {
                Uncertainty::Unknown
            },
            // No conflicting host report is present; acknowledgement quality
            // remains explicitly Unknown and cannot become goal evidence.
            conflict: ConflictStatus::None,
            partial: false,
            basis,
            effect,
            verifies,
            non_authoritative_raw: None,
        })
    }
}
// This concrete collector reads only the bounded immutable role table and the
// host clock. No metadata field is copied from provider/model JSON or secrets.
impl QualifiedAcquisitionPort for DirectCollector {}

pub(super) struct PreparedTarget {
    access: Arc<LocalSessionAccess>,
    basis: ConnectorSourceBasis,
    step: PlanStepId,
    dispatch: DispatchId,
    action_request: Option<ConnectorRequest>,
}
pub(super) struct ScopedTarget {
    run: SessionConnectorRun,
    source: Arc<LocalConnectorSource>,
    metadata: QualifiedScalarMetadata,
    binder: Option<InventoryBinder>,
    summary: Option<ConnectorInventory>,
    hints: Arc<RoleHints>,
    step: PlanStepId,
    dispatch: DispatchId,
    ordinal: u64,
    action_request: Option<ConnectorRequest>,
    proposal_used: bool,
    mutation_observation: Option<ConnectorObservation>,
}
impl PreparedTarget {
    pub(super) fn capture(
        access: Arc<LocalSessionAccess>,
        allocation: &SessionResourceAllocation,
        action_request: Option<&ConnectorRequest>,
    ) -> Result<Self, SessionError> {
        Ok(Self {
            basis: ConnectorSourceBasis::capture(Arc::clone(&access), allocation, action_request)?,
            access,
            step: allocation.binding().step.clone(),
            dispatch: allocation.binding().dispatch.clone(),
            action_request: action_request.cloned(),
        })
    }
    pub(super) fn build(
        self,
        allocation: SessionResourceAllocation,
        context: CallContext,
        observer: Arc<dyn Observer>,
    ) -> Result<ScopedTarget, SessionError> {
        let runtime = self.basis.runtime().clone();
        let step = runtime
            .0
            .steps
            .iter()
            .find(|step| step.step == self.step.as_str())
            .ok_or(SessionError::AuthorityDenied)?;
        let loaded = runtime.loaded()?;
        let quote: QuoteDocument = serde_json::from_value(loaded.document(&step.quotes)?.clone())
            .map_err(|_| invalid())?;
        let FixedTariffDocument::FixedLocalNoEmbedding {
            discovery_cost_microunits,
            operation_cost_microunits,
        } = quote.tariff;
        let hints = Arc::new(RoleHints::default());
        let mut normalizers = BTreeMap::new();
        for record in loaded.mapping().records() {
            let pin = step
                .normalizations
                .get(&record.reference)
                .ok_or(SessionError::UnsupportedCapability)?;
            let doc: NormalizationDocument =
                serde_json::from_value(loaded.document(pin)?.clone()).map_err(|_| invalid())?;
            if doc.acquisition.authenticated_profile != runtime.0.access_profile
                || record.source.as_ref() != Some(&doc.source)
            {
                return Err(SessionError::AuthorityDenied);
            }
            if record.kind == InventoryKind::Tool {
                let schema = record
                    .result_schema
                    .as_ref()
                    .ok_or(SessionError::UnsupportedCapability)?;
                let required = |field: &str| {
                    schema["required"]
                        .as_array()
                        .is_some_and(|names| names.iter().any(|name| name == field))
                };
                if !required(&doc.object_field)
                    || schema["properties"][&doc.object_field]["type"] != "string"
                    || doc.fields.keys().any(|field| {
                        !required(field)
                            || !matches!(
                                schema["properties"][field]["type"].as_str(),
                                Some("boolean" | "integer" | "string")
                            )
                    })
                {
                    return Err(SessionError::UnsupportedCapability);
                }
            }
            let normalizer = CanonicalResultNormalizer::new_qualified(
                NormalizationProfile {
                    source: doc.source,
                    source_kind: doc.acquisition.source_kind,
                    object_field: doc.object_field,
                    revision_field: doc.revision_field,
                    digest_field: doc.digest_field,
                    fields: doc.fields,
                },
                Box::new(DirectCollector {
                    profile: doc.acquisition,
                    wire_source: if record.kind == InventoryKind::Resource {
                        record.source.clone()
                    } else {
                        None
                    },
                    hints: Arc::clone(&hints),
                }),
            )
            .map_err(|_| invalid())?;
            normalizers.insert(record.capability.clone(), normalizer);
        }
        let target = ConnectorTarget {
            connector: runtime.0.connector.clone(),
            scope: runtime.0.scope.clone(),
        };
        let host = HostConfig {
            target,
            manifest: loaded.manifest().clone(),
            profile: ConnectorCompatibilityProfile {
                gateway: runtime.0.compatibility.gateway,
                transports: runtime.0.compatibility.transports.clone(),
            },
            limits: runtime.0.limits.build(),
        };
        let resolver = Arc::new(
            CredentialFileResolver::new(runtime.0.credential_store.clone())
                .map_err(|_| SessionError::AuthorityDenied)?,
        );
        let transport = Box::new(CredentialedStdioFactory::new(
            runtime.0.stdio.build(),
            resolver,
            CredentialProfileRef::new(runtime.0.access_profile.clone()),
            runtime.0.connector.as_str(),
            loaded.manifest().server_id(),
        ));
        let mapping = loaded.mapping().clone();
        let mut attached = None;
        let run = SessionConnectorRun::with_factory(allocation, self.access, context, |view| {
            let (source, binder) = self.basis.attach(view.clone())?;
            let handle = source.policy_handle();
            let bundle = qualified_scalar_host(
                QualifiedScalarConfig {
                    allocation: view,
                    host,
                    document: mapping,
                    normalizers,
                    tariff: FixedLocalTariff::operator_fixed_no_embedding(
                        quote.cost_unit,
                        discovery_cost_microunits,
                        operation_cost_microunits,
                    ),
                },
                transport,
                Arc::new(Cg09ConnectorAdmission::new(handle.clone())),
                Arc::new(handle),
                observer,
            )
            .map_err(|_| SessionError::UnsupportedCapability)?;
            attached = Some((Arc::clone(&source), binder, bundle.metadata()));
            Ok(SessionConnectorComponents::from_qualified_joined(
                bundle, source,
            ))
        })?;
        let (source, binder, metadata) = attached.ok_or(SessionError::InvalidState)?;
        Ok(ScopedTarget {
            run,
            source,
            metadata,
            binder: Some(binder),
            summary: None,
            hints,
            step: self.step,
            dispatch: self.dispatch,
            ordinal: 0,
            action_request: self.action_request,
            proposal_used: false,
            mutation_observation: None,
        })
    }
}
fn session_error(error: ConnectorFailure) -> SessionError {
    if matches!(
        error.effect,
        ConnectorEffect::Unknown | ConnectorEffect::Acknowledged
    ) {
        return SessionError::OutcomeUnknown;
    }
    match error.code {
        ConnectorFailureCode::BudgetExhausted => SessionError::LimitExceeded,
        ConnectorFailureCode::ScopeMismatch => SessionError::ScopeDenied,
        ConnectorFailureCode::MutationUncertain => SessionError::OutcomeUnknown,
        _ => SessionError::AuthorityDenied,
    }
}
impl ScopedTarget {
    pub(super) fn context_source(&self) -> &LocalConnectorSource {
        &self.source
    }
    pub(super) fn discover(&mut self) -> Result<&ConnectorInventory, SessionError> {
        if self.summary.is_some() {
            return Err(SessionError::InvalidState);
        }
        let target = ConnectorTarget {
            connector: self.source.runtime().0.connector.clone(),
            scope: self.source.runtime().0.scope.clone(),
        };
        let summary = self.run.discover(&target).map_err(session_error)?;
        let actual = self
            .metadata
            .snapshot_for(&summary)
            .map_err(|_| SessionError::OutcomeUnknown)?;
        let runtime = self.source.runtime();
        let step = runtime
            .0
            .steps
            .iter()
            .find(|step| step.step == self.step.as_str())
            .ok_or(SessionError::InvalidState)?;
        let ProducerAdmission::Model(model) = &step.producer else {
            return Err(SessionError::UnsupportedCapability);
        };
        let join: ProducerJoinDocument =
            serde_json::from_value(runtime.loaded()?.document(&model.join)?.clone())
                .map_err(|_| invalid())?;
        let mut tools = BTreeMap::new();
        for entry in actual
            .entries()
            .iter()
            .filter(|entry| entry.mapping.kind == InventoryKind::Tool)
        {
            let schema = self
                .metadata
                .tool_schema(
                    &summary.target,
                    &entry.mapping.capability,
                    &summary.revision,
                )
                .map_err(|_| SessionError::StaleRevision)?;
            let alias = join
                .capabilities
                .iter()
                .find(|alias| alias.id == entry.mapping.capability)
                .ok_or(SessionError::AuthorityDenied)?;
            let tool = ToolDefinition::from_connector_schema(
                alias.clone(),
                NonEmptyText::new("Admitted connector operation").map_err(|_| invalid())?,
                summary.target.clone(),
                summary.revision.clone(),
                schema.argument_schema_digest,
                &schema.argument_schema,
            )
            .map_err(|_| invalid())?;
            tools.insert(entry.mapping.capability.clone(), tool);
        }
        self.binder
            .take()
            .ok_or(SessionError::InvalidState)?
            .bind(actual, tools)?;
        self.summary = Some(summary);
        self.summary.as_ref().ok_or(SessionError::InvalidState)
    }
    fn execute_read(
        &mut self,
        read: ReadAdmission,
        role: ReadRole,
        export: bool,
    ) -> Result<TargetRead, SessionError> {
        let mapping = read.mapping_reference.as_str();
        let summary = self.summary.as_ref().ok_or(SessionError::InvalidState)?;
        let document = self.source.runtime().loaded()?.mapping();
        let record = document
            .records()
            .iter()
            .find(|record| record.reference == mapping)
            .ok_or(SessionError::UnsupportedCapability)?;
        if !matches!(
            record.class.operation_class(),
            ConnectorOperationClass::Read
                | ConnectorOperationClass::Search
                | ConnectorOperationClass::Inspect
        ) {
            return Err(SessionError::AuthorityDenied);
        }
        let source = if record.kind == InventoryKind::Resource {
            Some(read.source.clone())
        } else {
            None
        };
        self.ordinal = self
            .ordinal
            .checked_add(1)
            .ok_or(SessionError::LimitExceeded)?;
        let request = ConnectorRequest {
            target: summary.target.clone(),
            capability: read.capability,
            class: record.class.operation_class(),
            inventory_revision: summary.revision.clone(),
            arguments: read.arguments,
            task: self
                .source
                .producer_context()
                .context()
                .execution_context()
                .task()
                .id()
                .clone(),
            step: self.step.clone(),
            execution: NonEmptyText::new(
                self.source
                    .producer_context()
                    .context()
                    .execution_context()
                    .id()
                    .as_str(),
            )
            .map_err(|_| invalid())?,
            correlation: NonEmptyText::new(format!(
                "{}-fixed-read-{}",
                self.dispatch.as_str(),
                self.ordinal
            ))
            .map_err(|_| invalid())?,
        };
        if export {
            let provider = match &self.source.runtime().0.snapshots {
                SnapshotAdmission::AtomicCompleteRead {
                    provider_contract, ..
                }
                | SnapshotAdmission::VersionedCompleteSet {
                    provider_contract, ..
                } => provider_contract,
            };
            self.source
                .validate_scope_export_read(&request, source.as_ref(), provider)?;
        }
        let acquisition: NormalizationDocument = serde_json::from_value(
            self.source
                .runtime()
                .loaded()?
                .document(
                    self.source
                        .runtime()
                        .0
                        .steps
                        .iter()
                        .find(|step| step.step == self.step.as_str())
                        .and_then(|step| step.normalizations.get(mapping))
                        .ok_or(SessionError::UnsupportedCapability)?,
                )?
                .clone(),
        )
        .map_err(|_| invalid())?;
        if !matches!(
            acquisition.acquisition.semantics,
            ReadSemantics::DirectCurrentReadProfile { .. }
        ) {
            return Err(SessionError::UnsupportedCapability);
        }
        self.hints.insert(RoleHint {
            request: request.clone(),
            source: source.clone(),
            role,
            started_at: clock().map_err(|_| SessionError::Unavailable)?,
        })?;
        let observation = if let Some(source) = &source {
            self.run.read_resource(&request, source)
        } else {
            self.run.invoke(&request)
        }
        .map_err(session_error)?;
        ConnectorResult::validate_for_request(&request, &observation)
            .map_err(|_| SessionError::OutcomeUnknown)?;
        Ok(TargetRead {
            request,
            observation,
        })
    }
    pub(super) fn source_read(&mut self, mapping: &str) -> Result<TargetRead, SessionError> {
        let reads = match &self.source.runtime().0.snapshots {
            SnapshotAdmission::AtomicCompleteRead { read, .. } => vec![read],
            SnapshotAdmission::VersionedCompleteSet { reads, .. } => reads.iter().collect(),
        };
        let read = reads
            .into_iter()
            .find(|read| read.mapping_reference == mapping)
            .cloned()
            .ok_or(SessionError::UnsupportedCapability)?;
        self.execute_read(read, ReadRole::SourceRead, true)
    }
    pub(super) fn goal_read(
        &mut self,
        condition: &gateway_domain::ConditionId,
    ) -> Result<TargetRead, SessionError> {
        let read = self
            .source
            .runtime()
            .0
            .goal_reads
            .iter()
            .find(|read| read.condition == condition.as_str())
            .map(|read| read.read.clone())
            .ok_or(SessionError::UnsupportedCapability)?;
        self.execute_read(read, ReadRole::SourceRead, false)
    }
    pub(super) fn independent_read(&mut self, mapping: &str) -> Result<TargetRead, SessionError> {
        let original = self
            .action_request
            .as_ref()
            .filter(|request| {
                matches!(
                    request.class,
                    ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
                )
            })
            .ok_or(SessionError::AuthorityDenied)?
            .clone();
        let dispatched = self
            .mutation_observation
            .as_ref()
            .ok_or(SessionError::InvalidState)?;
        ConnectorResult::validate_for_request(&original, dispatched)
            .map_err(|_| SessionError::InvalidInteraction)?;
        if dispatched.metadata.effect != ConnectorEffect::Acknowledged {
            return Err(SessionError::InvalidState);
        }
        let read = self
            .source
            .runtime()
            .0
            .steps
            .iter()
            .find(|step| step.step == self.step.as_str())
            .and_then(|step| {
                step.post_effect_reads
                    .iter()
                    .find(|read| read.mapping_reference == mapping)
            })
            .cloned()
            .ok_or(SessionError::UnsupportedCapability)?;
        self.execute_read(
            read,
            ReadRole::IndependentPostEffectRead(Box::new(original)),
            false,
        )
    }
    pub(super) fn invoke_proposal(&mut self) -> Result<ConnectorObservation, SessionError> {
        if self.proposal_used {
            return Err(SessionError::InvalidState);
        }
        let request = self
            .action_request
            .as_ref()
            .ok_or(SessionError::AuthorityDenied)?
            .clone();
        if self.summary.as_ref().is_none_or(|summary| {
            summary.revision != request.inventory_revision || summary.target != request.target
        }) {
            return Err(SessionError::StaleRevision);
        }
        self.proposal_used = true;
        self.hints.insert(RoleHint {
            request: request.clone(),
            source: None,
            role: ReadRole::ProposalReturn,
            started_at: clock().map_err(|_| SessionError::Unavailable)?,
        })?;
        let observation = self.run.invoke(&request).map_err(session_error)?;
        if matches!(
            request.class,
            ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
        ) {
            ConnectorResult::validate_for_request(&request, &observation)
                .map_err(|_| SessionError::OutcomeUnknown)?;
            self.mutation_observation = Some(observation.clone());
        }
        Ok(observation)
    }
    pub(super) fn usage(&self) -> Result<SessionResourceAmounts, SessionError> {
        self.run.usage()
    }
}
pub(super) struct TargetRead {
    request: ConnectorRequest,
    observation: ConnectorObservation,
}
impl TargetRead {
    pub(super) fn request(&self) -> &ConnectorRequest {
        &self.request
    }
    pub(super) fn observation(&self) -> &ConnectorObservation {
        &self.observation
    }
    pub(super) fn batch(
        &self,
    ) -> Result<gateway_application::ScopedObservationBatch, SessionError> {
        ConnectorResult::validate_for_request(&self.request, &self.observation)
            .map_err(|_| SessionError::InvalidInteraction)?;
        if self.observation.basis == ConnectorObservationBasis::ProviderAcknowledgement {
            return Err(SessionError::AuthorityDenied);
        }
        let observation = &self.observation;
        let snapshot = gateway_application::SourceSnapshot::new(
            observation.source.clone(),
            observation.metadata.source_kind,
            observation.source_revision.clone(),
            Some(observation.content_digest.clone()),
        )
        .map_err(|_| invalid())?;
        let mut batch = gateway_application::ScopedObservationBatch::new(
            observation.target.scope.clone(),
            snapshot,
            observation.records.clone(),
        )
        .map_err(|_| invalid())?;
        for item in observation.records.observations() {
            batch = batch
                .with_quality_metadata(item.subject().clone(), vec![observation.metadata.quality]);
        }
        Ok(batch)
    }
}

#[cfg(test)]
mod tests;
