//! Operator-owned read-only definitions. Captured data never grants a claim or
//! connector operation; the installed source still joins current live authority.
use super::world_definitions::{AtomicBeforeAdmission, AtomicBeforeDefinitions};
use crate::{
    codex_workspace::LocalCodexHost,
    declarative_cli::host_mapping,
    mcp_client::{
        Limits,
        mapping::{MappingDocument, OperationClass},
        transport::StdioConfig,
    },
};
use gateway_application::connector_manifest::{
    CompatibilityDecision, ConnectorCompatibilityProfile, ConnectorManifest, ConnectorTransport,
    ConnectorVersion,
};
use gateway_application::{
    codex::CompileCommand,
    context_application::{CompileStepInput, CompiledStep, ContextApplication},
    policy_application::{PolicyApplication, ResolvedStepPolicyInput},
    resolution_application::DeclarativeResolutionApplication,
    sessions::*,
};
use gateway_domain::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub(in crate::local_sessions) type Pin = ResolvedReference<ReferenceId>;
const MAX_DOCUMENT: usize = 1_048_576;
const MAX_TOTAL: usize = 8 * MAX_DOCUMENT;
const MAX_DOCUMENTS: usize = 128;
fn invalid() -> SessionError {
    SessionError::InvalidInput
}
fn canonical(value: &impl Serialize) -> Result<Vec<u8>, SessionError> {
    serde_json::to_vec(&serde_json::to_value(value).map_err(|_| invalid())?).map_err(|_| invalid())
}
fn hash(value: &impl Serialize) -> Result<ContentDigest, SessionError> {
    Ok(context_content_digest(&canonical(value)?))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct ReadonlyRuntimeAdmission {
    pub schema_version: SchemaVersion,
    pub id: ReferenceId,
    pub revision: ReferenceId,
    pub scope: ContextScopeId,
    pub plan: RecordRef,
    pub goal: Pin,
    pub execution: RequestedExecution,
    pub connector: NonEmptyText,
    pub policy: Pin,
    pub manifest: Pin,
    pub mapping: Pin,
    pub association: Pin,
    pub task: Pin,
    pub projection: Pin,
    pub step: String,
    pub quotes: Pin,
    pub verifier: Pin,
    pub stdio: StdioAdmission,
    pub compatibility: CompatibilityAdmission,
    pub limits: LimitAdmission,
    pub credential_store: PathBuf,
    pub access_profile: NonEmptyText,
    pub documents: Vec<DocumentAdmission>,
    pub goal_reads: Vec<GoalReadAdmission>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete_before: Option<AtomicBeforeAdmission>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct DocumentAdmission {
    pub pin: Pin,
    pub path: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct StdioAdmission {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub directory: PathBuf,
    pub environment: BTreeMap<String, String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct CompatibilityAdmission {
    pub gateway: ConnectorVersion,
    pub transports: BTreeSet<ConnectorTransport>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct LimitAdmission {
    pub frame_bytes: usize,
    pub inventory_bytes: usize,
    pub items: usize,
    pub pages: usize,
    pub events_per_request: usize,
    pub call_timeout_ms: u64,
    pub shutdown_ms: u64,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct ReadAdmission {
    pub mapping_reference: String,
    pub capability: CapabilityId,
    pub source: SourceId,
    pub arguments: BTreeMap<NonEmptyText, TypedValue>,
    pub normalization: Pin,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct GoalReadAdmission {
    pub condition: String,
    pub read: ReadAdmission,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct NormalizationDocument {
    pub schema_version: SchemaVersion,
    pub acquisition: AcquisitionDocument,
    pub source: SourceId,
    pub object_field: String,
    pub revision_field: Option<String>,
    pub digest_field: Option<String>,
    pub fields: BTreeMap<String, SubjectPath>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct AcquisitionDocument {
    pub source_kind: SourceKind,
    pub sensitivity: SensitivityClass,
    pub authenticated_profile: NonEmptyText,
    pub semantics: ReadSemantics,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub(in crate::local_sessions) enum ReadSemantics {
    DirectCurrentReadProfile {
        max_age_seconds: u64,
        time_basis: ReadTimeBasis,
    },
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(in crate::local_sessions) enum ReadTimeBasis {
    RequestStartLowerBound,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct QuoteDocument {
    pub schema_version: SchemaVersion,
    pub tariff: FixedTariffDocument,
    pub manifest: Pin,
    pub mapping: Pin,
    pub cost_unit: ReferenceId,
    pub discovery: SessionResourceAmounts,
    pub operations: Vec<OperationQuote>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub(in crate::local_sessions) enum FixedTariffDocument {
    FixedLocalNoEmbedding {
        discovery_cost_microunits: u64,
        operation_cost_microunits: u64,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct OperationQuote {
    pub mapping_reference: String,
    pub amounts: SessionResourceAmounts,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct VerifierDocument {
    pub schema_version: SchemaVersion,
    pub mode: VerifierMode,
    pub scope: ContextScopeId,
    pub goal: Pin,
    pub max_iterations: u32,
    pub max_retries: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(in crate::local_sessions) enum VerifierMode {
    RetainedObservations,
}

pub(in crate::local_sessions) struct ReadNormalization {
    pub read: ReadAdmission,
    pub document: NormalizationDocument,
}

/// Data assembled through actual native owners, not an authorization witness.
/// Reuse requires the caller's current claim checks and fresh fingerprint joins.
pub(in crate::local_sessions) struct ReadonlyDefinitions {
    pub runtime: ReadonlyRuntimeAdmission,
    pub command: CompileCommand,
    pub compiled: CompiledStep,
    pub native: ResolvedStepPolicyInput,
    pub task: SemanticTaskIR,
    pub manifest: ConnectorManifest,
    pub mapping: MappingDocument,
    pub normalizations: Vec<ReadNormalization>,
    pub quotes: QuoteDocument,
    pub verifier: VerifierDocument,
    pub documents: BTreeMap<ReferenceId, (Pin, Value)>,
    pub definition_pins: Vec<Pin>,
    pub fingerprint: ContentDigest,
    pub before: Option<AtomicBeforeDefinitions>,
}

impl NormalizationDocument {
    pub fn profile(
        &self,
    ) -> Result<crate::mcp_client::normalization::NormalizationProfile, SessionError> {
        let profile = crate::mcp_client::normalization::NormalizationProfile {
            source: self.source.clone(),
            source_kind: self.acquisition.source_kind,
            object_field: self.object_field.clone(),
            revision_field: self.revision_field.clone(),
            digest_field: self.digest_field.clone(),
            fields: self.fields.clone(),
        };
        profile.validate().map_err(|_| invalid())?;
        Ok(profile)
    }
}

impl ReadonlyDefinitions {
    /// Check fresh owner metadata before reusing immutable native compilation
    /// data. This performs no current-session/connector authorization itself.
    pub fn validate_current_metadata(&self, host: &LocalCodexHost) -> Result<(), SessionError> {
        let current = host
            .canonical
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        if host.binding.canonical_scope != self.runtime.scope
            || current.pinned_basis.as_ref() != Some(&self.command.resolved.report.basis)
            || self.runtime.current_fingerprint(host)? != self.fingerprint
        {
            return Err(SessionError::StaleRevision);
        }
        Ok(())
    }
    pub fn document(&self, pin: &Pin) -> Result<&Value, SessionError> {
        let (actual, value) = self
            .documents
            .get(&pin.id)
            .ok_or(SessionError::Unavailable)?;
        if actual != pin {
            return Err(SessionError::StaleRevision);
        }
        Ok(value)
    }
}

impl LimitAdmission {
    pub fn build(&self) -> Result<Limits, SessionError> {
        if self.frame_bytes == 0
            || self.frame_bytes > 16_777_216
            || self.inventory_bytes == 0
            || self.inventory_bytes > 67_108_864
            || self.items == 0
            || self.items > 65_536
            || self.pages == 0
            || self.pages > 1024
            || self.events_per_request == 0
            || self.events_per_request > 4096
            || self.call_timeout_ms == 0
            || self.call_timeout_ms > 3_600_000
            || self.shutdown_ms == 0
            || self.shutdown_ms > 5000
        {
            return Err(invalid());
        }
        Ok(Limits {
            frame_bytes: self.frame_bytes,
            inventory_bytes: self.inventory_bytes,
            items: self.items,
            pages: self.pages,
            events_per_request: self.events_per_request,
            call_timeout: Duration::from_millis(self.call_timeout_ms),
            shutdown: Duration::from_millis(self.shutdown_ms),
        })
    }
}
impl StdioAdmission {
    pub fn build(&self) -> Result<StdioConfig, SessionError> {
        if !self.executable.is_absolute()
            || !self.directory.is_absolute()
            || self.arguments.len() > 32
            || self.arguments.iter().any(|a| a.len() > 4096)
            || !self.environment.is_empty()
        {
            return Err(invalid());
        }
        Ok(StdioConfig {
            executable: self.executable.clone(),
            arguments: self.arguments.clone(),
            directory: self.directory.clone(),
            environment: self.environment.clone(),
        })
    }
}
impl CompatibilityAdmission {
    pub fn build(&self) -> ConnectorCompatibilityProfile {
        ConnectorCompatibilityProfile {
            gateway: self.gateway,
            transports: self.transports.clone(),
        }
    }
}
impl ReadonlyRuntimeAdmission {
    pub fn reference(&self) -> Result<Pin, SessionError> {
        Ok(Pin {
            id: self.id.clone(),
            scope: self.scope.clone(),
            contract: ReferenceId::new("cg.connector-observed-goal-runtime")
                .map_err(|_| invalid())?,
            contract_version: SchemaVersion::V1,
            revision: self.revision.clone(),
            digest: hash(self)?,
        })
    }
    pub fn validate_registration(
        &self,
        goal: &SupportedConnectorGoal,
        execution: RequestedExecution,
    ) -> Result<(), SessionError> {
        if self.schema_version != SchemaVersion::V1
            || &self.scope != goal.scope()
            || &self.plan != goal.plan()
            || self.execution != execution
            || self.step.is_empty()
            || goal
                .steps()
                .first()
                .is_none_or(|step| step.as_str() != self.step)
            || self.documents.is_empty()
            || self.documents.len() > MAX_DOCUMENTS
            || self.goal_reads.is_empty()
            || self.goal_reads.len() > MAX_CONNECTOR_GOAL_CONDITIONS
            || !self.credential_store.is_absolute()
        {
            return Err(invalid());
        }
        self.limits.build()?;
        self.stdio.build()?;
        let mut roots = vec![
            (&self.goal, "cg.connector-goal"),
            (&self.policy, "cg.connector-canonical-policy"),
            (&self.manifest, "cg.connector-manifest"),
            (&self.mapping, "cg.connector-mapping"),
            (&self.association, "cg.connector-read-association"),
            (&self.task, "cg.semantic-task"),
            (&self.projection, "cg.context-projection"),
            (&self.quotes, "cg.connector-quotes"),
            (&self.verifier, "cg.connector-verifier"),
        ];
        roots.extend(
            self.goal_reads
                .iter()
                .map(|r| (&r.read.normalization, "cg.connector-normalization")),
        );
        if let Some(world) = &self.complete_before {
            roots.extend(world.roots());
            if world.read.mapping_reference.is_empty()
                || world.read.mapping_reference.len() > 128
                || world.read.arguments.len() > 256
                || canonical(&world.read.arguments)?.len() > 262_144
            {
                return Err(invalid());
            }
        }
        for (pin, contract) in roots {
            validate_pin(pin, &self.scope)?;
            if pin.contract.as_str() != contract {
                return Err(invalid());
            }
        }
        let mut ids = BTreeSet::from([self.id.clone()]);
        for doc in &self.documents {
            validate_pin(&doc.pin, &self.scope)?;
            if !doc.path.is_absolute() || !ids.insert(doc.pin.id.clone()) {
                return Err(invalid());
            }
        }
        let mut conditions = BTreeSet::new();
        for route in &self.goal_reads {
            if route.condition.is_empty()
                || route.read.mapping_reference.is_empty()
                || route.read.mapping_reference.len() > 128
                || route.read.arguments.len() > 256
                || !conditions.insert(&route.condition)
            {
                return Err(invalid());
            }
        }
        if self.goal_reads.len() != goal.observations().len() {
            return Err(invalid());
        }
        for observed in goal.observations() {
            let r = self
                .goal_reads
                .iter()
                .find(|r| r.condition == observed.condition().as_str())
                .ok_or_else(invalid)?;
            if &r.read.source != observed.source() || &r.read.capability != observed.capability() {
                return Err(SessionError::StaleRevision);
            }
        }
        Ok(())
    }
    /// Fresh bounded metadata only. No resolver, policy evaluation, provider,
    /// credentials or session mutation is performed by this fingerprint check.
    pub fn current_fingerprint(
        &self,
        host: &LocalCodexHost,
    ) -> Result<ContentDigest, SessionError> {
        Ok(self.load_metadata(host)?.fingerprint)
    }
    pub fn capture(
        &self,
        host: &LocalCodexHost,
        goal: &SupportedConnectorGoal,
        execution: RequestedExecution,
    ) -> Result<ReadonlyDefinitions, SessionError> {
        self.validate_registration(goal, execution)?;
        if host.binding.canonical_scope != self.scope {
            return Err(SessionError::ScopeDenied);
        }
        let metadata = self.load_metadata(host)?;
        let canonical_host = host
            .canonical
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        if canonical_host.plan["id"] != self.plan.id().as_str()
            || canonical_host.plan["revision"] != self.plan.revision().as_str()
            || canonical_host.plan["digest"] != self.plan.digest()
            || self.document(&metadata.documents, &self.policy)? != &canonical_host.policy
        {
            return Err(SessionError::StaleRevision);
        }
        let declared_goal =
            Intent::from_json(&self.document(&metadata.documents, &self.goal)?.to_string())
                .map_err(|_| invalid())?;
        if &declared_goal != goal.intent() {
            return Err(SessionError::StaleRevision);
        }
        let (snapshot, rules) = host_mapping::capture(
            &canonical_host.catalog,
            &canonical_host
                .documents(&host.resources)
                .map_err(super::super::error)?,
        )
        .map_err(super::super::error)?;
        let resolved = DeclarativeResolutionApplication
            .resolve_plan(&snapshot, &rules)
            .map_err(|_| invalid())?;
        if canonical_host.pinned_basis.as_ref() != Some(&resolved.report.basis)
            || snapshot.scope != self.scope
            || snapshot.operating_mode != execution.mode
            || snapshot.execution_profile != execution.profile
            || &snapshot.desired != goal.intent().desired_state()
        {
            return Err(SessionError::StaleRevision);
        }
        let command = host_mapping::compile(
            resolved,
            canonical_host.policy.clone(),
            self.document(&metadata.documents, &self.projection)?
                .clone(),
        )
        .map_err(super::super::error)?;
        let task =
            SemanticTaskIR::from_json(&self.document(&metadata.documents, &self.task)?.to_string())
                .map_err(|_| invalid())?;
        if task.data().id.as_str() != self.task.id.as_str()
            || task.data().target.reference.scope != self.scope
            || task.data().desired_state.as_ref() != Some(goal.intent().desired_state())
            || command.projection.task.id() != &task.data().id
            || command.projection.mapping.step.as_str() != self.step
        {
            return Err(SessionError::StaleRevision);
        }
        let references = References {
            scope: &self.scope,
            documents: &metadata.documents,
        };
        task.validate_references(&references)
            .map_err(|_| SessionError::AuthorityDenied)?;
        let native = PolicyApplication
            .step_input(
                &command.resolved,
                &command.policy_context,
                &command.projection.mapping.step,
            )
            .map_err(|_| SessionError::AuthorityDenied)?;
        // This installed route qualifies an actual Inspect-only native step.
        // Mixed mutation closures remain unsupported; no step or facts are rewritten.
        if !native.resolved
            || native.capabilities.is_empty()
            || native
                .capabilities
                .values()
                .any(|c| c.class() != CapabilityClass::Inspect)
        {
            return Err(SessionError::AuthorityDenied);
        }
        let compiled = ContextApplication
            .compile_step(CompileStepInput {
                resolved: &command.resolved,
                authority: &command.authority,
                policy_context: &command.policy_context,
                catalog: &command.catalog,
                projection: &command.projection,
                candidates: &command.candidates,
                selected: &command.selected,
            })
            .map_err(|_| SessionError::AuthorityDenied)?;
        let manifest = ConnectorManifest::from_definition_json(&canonical(
            self.document(&metadata.documents, &self.manifest)?,
        )?)
        .map_err(|_| invalid())?;
        if manifest.definition().transport != ConnectorTransport::Stdio
            || manifest.connector_id() != self.connector.as_str()
            || !matches!(
                manifest.compatibility(&self.compatibility.build()),
                CompatibilityDecision::Compatible | CompatibilityDecision::CompatibleDeprecated
            )
        {
            return Err(invalid());
        }
        let mapping = MappingDocument::from_json(&canonical(
            self.document(&metadata.documents, &self.mapping)?,
        )?)
        .map_err(|_| invalid())?;
        if mapping.records().is_empty() || mapping.records().len() > 256 {
            return Err(invalid());
        }
        for record in mapping.records() {
            if !matches!(
                record.class,
                OperationClass::Read | OperationClass::Search | OperationClass::Inspect
            ) || !native.capabilities.contains_key(&record.capability)
                || !self
                    .goal_reads
                    .iter()
                    .any(|route| route.read.mapping_reference == record.reference)
                    && !self
                        .complete_before
                        .as_ref()
                        .is_some_and(|world| world.read.mapping_reference == record.reference)
            {
                return Err(SessionError::AuthorityDenied);
            }
        }
        let mut normalizations: Vec<ReadNormalization> = Vec::new();
        for route in &self.goal_reads {
            let record = mapping
                .records()
                .iter()
                .find(|r| r.reference == route.read.mapping_reference)
                .ok_or_else(invalid)?;
            let observed = goal
                .observations()
                .iter()
                .find(|o| o.condition().as_str() == route.condition)
                .ok_or_else(invalid)?;
            if record.capability != route.read.capability
                || record.source.as_ref() != Some(&route.read.source)
                || record.class.operation_class() != observed.operation_class()
            {
                return Err(SessionError::StaleRevision);
            }
            let position =
                if let Some(position) = normalization_index(&normalizations, &route.read)? {
                    position
                } else {
                    let doc: NormalizationDocument = serde_json::from_value(
                        self.document(&metadata.documents, &route.read.normalization)?
                            .clone(),
                    )
                    .map_err(|_| invalid())?;
                    let ReadSemantics::DirectCurrentReadProfile {
                        max_age_seconds, ..
                    } = doc.acquisition.semantics;
                    if doc.schema_version != SchemaVersion::V1
                        || record.source.as_ref() != Some(&doc.source)
                        || doc.acquisition.source_kind != SourceKind::Tool
                        || doc.acquisition.authenticated_profile != self.access_profile
                        || max_age_seconds == 0
                        || max_age_seconds > 3600
                        || doc.fields.is_empty()
                        || doc.fields.len() > 256
                        || doc.object_field.is_empty()
                    {
                        return Err(invalid());
                    }
                    doc.profile()?;
                    normalizations.push(ReadNormalization {
                        read: route.read.clone(),
                        document: doc,
                    });
                    normalizations.len() - 1
                };
            let normalizer = &normalizations[position].document;
            let condition = goal
                .intent()
                .desired_state()
                .conditions()
                .iter()
                .find(|c| c.id().as_str() == route.condition)
                .ok_or_else(invalid)?;
            if !normalizer
                .fields
                .values()
                .any(|subject| subject == condition.subject())
            {
                return Err(SessionError::StaleRevision);
            }
        }
        let quotes: QuoteDocument =
            serde_json::from_value(self.document(&metadata.documents, &self.quotes)?.clone())
                .map_err(|_| invalid())?;
        if quotes.schema_version != SchemaVersion::V1
            || quotes.manifest != self.manifest
            || quotes.mapping != self.mapping
            || quotes.operations.len() != mapping.records().len()
        {
            return Err(invalid());
        }
        quotes.discovery.validate_readonly()?;
        let mut quote_refs = BTreeSet::new();
        for q in &quotes.operations {
            q.amounts.validate_readonly()?;
            if !quote_refs.insert(&q.mapping_reference)
                || !mapping
                    .records()
                    .iter()
                    .any(|r| r.reference == q.mapping_reference)
            {
                return Err(invalid());
            }
        }
        let config = host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        if config
            .resource_limits
            .as_ref()
            .is_none_or(|limits| limits.cost_unit != quotes.cost_unit)
        {
            return Err(SessionError::StaleRevision);
        }
        let verifier: VerifierDocument =
            serde_json::from_value(self.document(&metadata.documents, &self.verifier)?.clone())
                .map_err(|_| invalid())?;
        if verifier.schema_version != SchemaVersion::V1
            || verifier.scope != self.scope
            || verifier.goal != self.goal
            || verifier.max_iterations == 0
            || verifier.max_iterations > 64
            || verifier.max_iterations > config.max_actions
            || verifier.max_retries > 64
            || verifier.max_retries > config.max_retries
        {
            return Err(invalid());
        }
        if self.current_fingerprint(host)? != metadata.fingerprint {
            return Err(SessionError::StaleRevision);
        }
        let mut definition_pins = self
            .documents
            .iter()
            .map(|d| d.pin.clone())
            .collect::<Vec<_>>();
        definition_pins.push(self.reference()?);
        definition_pins.sort_by(|a, b| a.id.cmp(&b.id));
        let mut captured = ReadonlyDefinitions {
            runtime: self.clone(),
            command,
            compiled,
            native,
            task,
            manifest,
            mapping,
            normalizations,
            quotes,
            verifier,
            definition_pins,
            documents: metadata.documents,
            fingerprint: metadata.fingerprint,
            before: None,
        };
        if let Some(world) = &self.complete_before {
            let complete = world.capture(&captured)?;
            let record = captured
                .mapping
                .records()
                .iter()
                .find(|r| r.reference == complete.read.mapping_reference)
                .ok_or_else(invalid)?;
            if record.capability != complete.read.capability
                || record.source.as_ref() != Some(&complete.read.source)
                || record.class.operation_class() != complete.profile.anchor_route.class
            {
                return Err(SessionError::StaleRevision);
            }
            if normalization_index(&captured.normalizations, &complete.read)?.is_none() {
                captured.normalizations.push(ReadNormalization {
                    read: complete.read.clone(),
                    document: complete.normalization.clone(),
                });
            }
            captured.before = Some(complete);
        }
        Ok(captured)
    }
    fn document<'a>(
        &self,
        docs: &'a BTreeMap<ReferenceId, (Pin, Value)>,
        pin: &Pin,
    ) -> Result<&'a Value, SessionError> {
        let (actual, value) = docs.get(&pin.id).ok_or(SessionError::Unavailable)?;
        if actual != pin {
            return Err(SessionError::StaleRevision);
        }
        Ok(value)
    }
    fn load_metadata(&self, host: &LocalCodexHost) -> Result<Metadata, SessionError> {
        let config = host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        let current = config
            .observed_goal_runtime
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        if current.reference()? != self.reference()? {
            return Err(SessionError::StaleRevision);
        }
        let registered_goal = config.connector_goal.as_ref().map(|g| json!({"plan":g.plan,"steps":g.steps,"observations":g.observations.iter().map(|o|json!({"condition":o.condition,"source":o.source,"capability":o.capability,"operation_class":o.operation_class})).collect::<Vec<_>>()}));
        let session_config = json!({"store_file":config.store_file,"intent":config.intent,"basis":config.basis,"connector_goal":registered_goal,"resource_limits":config.resource_limits,"execution":config.execution,"max_actions":config.max_actions,"max_retries":config.max_retries,"ttl_ms":config.ttl_ms,"consent_required":config.consent_required,"enabled":config.enabled,"issuers":config.issuers});
        let mut total = 0usize;
        let mut documents = BTreeMap::new();
        for declaration in &self.documents {
            let bytes = read_regular(&declaration.path, MAX_DOCUMENT)?;
            total = total
                .checked_add(bytes.len())
                .ok_or(SessionError::LimitExceeded)?;
            if total > MAX_TOTAL {
                return Err(SessionError::LimitExceeded);
            }
            let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
            let value = super::super::definition_json::parse(text)?;
            if hash(&value)? != declaration.pin.digest {
                return Err(SessionError::StaleRevision);
            }
            if documents
                .insert(declaration.pin.id.clone(), (declaration.pin.clone(), value))
                .is_some()
            {
                return Err(invalid());
            }
        }
        // Resolve every root using full pin equality before producing metadata.
        for pin in [
            &self.goal,
            &self.policy,
            &self.manifest,
            &self.mapping,
            &self.association,
            &self.task,
            &self.projection,
            &self.quotes,
            &self.verifier,
        ]
        .into_iter()
        .chain(
            self.goal_reads
                .iter()
                .map(|route| &route.read.normalization),
        ) {
            self.document(&documents, pin)?;
        }
        if let Some(world) = &self.complete_before {
            for (pin, _) in world.roots() {
                self.document(&documents, pin)?;
            }
        }
        let canonical_host = host
            .canonical
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        let mut catalog = Vec::new();
        let mut catalog_bytes = 0;
        catalog_manifest(
            &canonical_host.catalog,
            &canonical_host.catalog,
            0,
            &mut catalog,
            &mut catalog_bytes,
        )?;
        catalog.sort_by(|a, b| a.0.cmp(&b.0));
        let values = documents
            .iter()
            .map(|(id, (pin, _))| (id, pin))
            .collect::<Vec<_>>();
        let fingerprint = hash(
            &json!({"contract":"cg.connector-observed-goal-definitions/1.0","runtime":self,"session_config":session_config,"binding":{"scope":host.binding.scope,"canonical_scope":host.binding.canonical_scope,"principal":host.binding.session.principal,"session_id":host.binding.session.session_id,"connection_id":host.binding.session.connection_id,"mapping_revision":host.binding.mapping_revision},"canonical":{"catalog":canonical_host.catalog,"plan":canonical_host.plan,"rules":canonical_host.rules,"process":canonical_host.process,"policy":canonical_host.policy},"resources":host.resources,"catalog_manifest":catalog,"documents":values}),
        )?;
        Ok(Metadata {
            documents,
            fingerprint,
        })
    }
}
fn normalization_index(
    entries: &[ReadNormalization],
    read: &ReadAdmission,
) -> Result<Option<usize>, SessionError> {
    let matching = entries.iter().position(|entry| {
        entry.read.capability == read.capability
            && entry.read.source == read.source
            && entry.read.arguments == read.arguments
    });
    if let Some(index) = matching
        && entries[index].read != *read
    {
        return Err(SessionError::InvalidInteraction);
    }
    Ok(matching)
}

struct Metadata {
    documents: BTreeMap<ReferenceId, (Pin, Value)>,
    fingerprint: ContentDigest,
}
fn validate_pin(pin: &Pin, scope: &ContextScopeId) -> Result<(), SessionError> {
    if &pin.scope != scope || pin.contract_version != SchemaVersion::V1 {
        return Err(SessionError::ScopeDenied);
    }
    Ok(())
}
fn read_regular(path: &Path, limit: usize) -> Result<Vec<u8>, SessionError> {
    if !path.is_absolute()
        || !std::fs::metadata(path)
            .map_err(|_| SessionError::Unavailable)?
            .is_file()
    {
        return Err(invalid());
    }
    let file = std::fs::File::open(path).map_err(|_| SessionError::Unavailable)?;
    if !file
        .metadata()
        .map_err(|_| SessionError::Unavailable)?
        .is_file()
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SessionError::Unavailable)?;
    if bytes.len() > limit {
        return Err(SessionError::LimitExceeded);
    }
    Ok(bytes)
}
fn catalog_manifest(
    root: &Path,
    path: &Path,
    depth: usize,
    output: &mut Vec<(PathBuf, ContentDigest)>,
    total: &mut usize,
) -> Result<(), SessionError> {
    if depth > 16 || output.len() > 512 {
        return Err(SessionError::LimitExceeded);
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| SessionError::Unavailable)?;
    if metadata.file_type().is_symlink() {
        return Err(invalid());
    }
    if metadata.is_dir() {
        output.push((
            path.strip_prefix(root)
                .map_err(|_| invalid())?
                .to_path_buf(),
            context_content_digest(b"catalog-directory/1.0"),
        ));
        for entry in std::fs::read_dir(path).map_err(|_| SessionError::Unavailable)? {
            catalog_manifest(
                root,
                &entry.map_err(|_| SessionError::Unavailable)?.path(),
                depth + 1,
                output,
                total,
            )?;
        }
    } else if metadata.is_file() {
        let bytes = read_regular(path, MAX_DOCUMENT)?;
        *total = total
            .checked_add(bytes.len())
            .ok_or(SessionError::LimitExceeded)?;
        if *total > MAX_TOTAL {
            return Err(SessionError::LimitExceeded);
        }
        output.push((
            path.strip_prefix(root)
                .map_err(|_| invalid())?
                .to_path_buf(),
            context_content_digest(&bytes),
        ));
    } else {
        return Err(invalid());
    }
    Ok(())
}
// This stage authenticates exact current reference records. The installed
// source additionally admits task/object/selector association through the
// owning read-association contract after actual bounded discovery.
struct References<'a> {
    scope: &'a ContextScopeId,
    documents: &'a BTreeMap<ReferenceId, (Pin, Value)>,
}
impl SemanticReferenceValidator for References<'_> {
    fn validate(
        &self,
        task: &SemanticTaskData,
        binding: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        let actual = self
            .documents
            .get(&ReferenceId::new(&binding.id)?)
            .map(|(p, _)| p);
        if task.target.reference.scope != *self.scope
            || actual.is_none_or(|pin| {
                pin.id.as_str() != binding.id
                    || pin.scope != binding.scope
                    || pin.contract != binding.contract
                    || pin.contract_version != binding.contract_version
                    || pin.revision != binding.revision
                    || pin.digest != binding.digest
            })
        {
            return Err(ValidationError::InvalidStateCombination {
                reason: "readonly task reference is not current",
            });
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "definitions_tests.rs"]
mod tests;
