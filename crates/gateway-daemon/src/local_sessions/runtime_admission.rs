//! Private operator sidecars. These records never enter the frozen inbound v1
//! resource namespace. Loading data does not admit a model or connector effect.
mod connector_source;
pub(super) mod before_goal;
mod consent_preparation;
mod current_producer_definitions;
mod historical_inventory;
mod model_documents;
mod model_factory;
mod phase_planning;
mod retained_model_producer;
#[cfg(test)]
pub(super) mod preparing_test_support;
#[cfg(test)]
pub(super) mod profiling;
mod source_context;
mod target_factory;

use super::*;
use crate::mcp_client::{
    Limits,
    mapping::{MappingDocument, OperationClass},
    transport::StdioConfig,
};
use gateway_application::connector_manifest::{
    CompatibilityDecision, ConnectorCompatibilityProfile, ConnectorManifest, ConnectorTransport,
    ConnectorVersion, InventoryKind,
};
use gateway_domain::{
    ContextScopeId, NonEmptyText, ReferenceId, ResolvedReference, SchemaVersion, SourceId,
    TypedValue,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Duration,
};

type Pin = ResolvedReference<ReferenceId>;
const DOCUMENT_BYTES: usize = 1_048_576;
const CATALOG_BYTES: usize = 8 * DOCUMENT_BYTES;
const DOCUMENTS: usize = 128;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, try_from = "RuntimeWire")]
pub(super) struct RuntimeAdmission(RuntimeWire, Option<Arc<LoadedRuntime>>);
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeWire {
    id: ReferenceId,
    revision: ReferenceId,
    policy: Pin,
    schema_version: SchemaVersion,
    scope: ContextScopeId,
    plan: RecordRef,
    goal: Pin,
    execution: RequestedExecution,
    connector: NonEmptyText,
    manifest: Pin,
    mapping: Pin,
    stdio: StdioAdmission,
    compatibility: CompatibilityAdmission,
    limits: LimitAdmission,
    credential_store: PathBuf,
    access_profile: NonEmptyText,
    documents: Vec<DocumentAdmission>,
    steps: Vec<StepAdmission>,
    snapshots: SnapshotAdmission,
    goal_reads: Vec<GoalReadAdmission>,
    verifier: Pin,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentAdmission {
    pin: Pin,
    path: PathBuf,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StdioAdmission {
    executable: PathBuf,
    arguments: Vec<String>,
    directory: PathBuf,
    environment: BTreeMap<String, String>,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityAdmission {
    gateway: ConnectorVersion,
    transports: BTreeSet<ConnectorTransport>,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitAdmission {
    frame_bytes: usize,
    inventory_bytes: usize,
    items: usize,
    pages: usize,
    events_per_request: usize,
    call_timeout_ms: u64,
    shutdown_ms: u64,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepAdmission {
    step: String,
    action_projection: Pin,
    resources: SessionResourcePlan,
    quotes: Pin,
    normalizations: BTreeMap<String, Pin>,
    producer: ProducerAdmission,
    post_effect_reads: Vec<ReadAdmission>,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
enum ProducerAdmission {
    Model(Box<ModelAdmission>),
    Structured { operations: Pin },
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelAdmission {
    association: Pin,
    producer_step: String,
    join: Pin,
    task: Pin,
    projection: Pin,
    context_analysis: Pin,
    fragments: Vec<Pin>,
    source_metadata: Vec<Pin>,
    output: Pin,
    verification: Pin,
    model: Pin,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadAdmission {
    mapping_reference: String,
    capability: CapabilityId,
    source: SourceId,
    arguments: BTreeMap<NonEmptyText, TypedValue>,
    normalization: Pin,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GoalReadAdmission {
    condition: String,
    read: ReadAdmission,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuoteDocument {
    schema_version: SchemaVersion,
    tariff: FixedTariffDocument,
    manifest: Pin,
    mapping: Pin,
    cost_unit: ReferenceId,
    discovery: SessionResourceAmounts,
    operations: Vec<OperationQuote>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
enum FixedTariffDocument {
    FixedLocalNoEmbedding {
        discovery_cost_microunits: u64,
        operation_cost_microunits: u64,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationQuote {
    mapping_reference: String,
    amounts: SessionResourceAmounts,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NormalizationDocument {
    schema_version: SchemaVersion,
    acquisition: AcquisitionDocument,
    source: SourceId,
    object_field: String,
    revision_field: Option<String>,
    digest_field: Option<String>,
    fields: BTreeMap<String, gateway_domain::SubjectPath>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcquisitionDocument {
    source_kind: gateway_domain::SourceKind,
    sensitivity: gateway_domain::SensitivityClass,
    authenticated_profile: NonEmptyText,
    semantics: ReadSemantics,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
enum ReadSemantics {
    DirectCurrentReadProfile {
        max_age_seconds: u64,
        time_basis: ReadTimeBasis,
    },
    ProviderAcknowledgementOnly,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ReadTimeBasis {
    RequestStartLowerBound,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProducerJoinDocument {
    schema_version: SchemaVersion,
    plan: RecordRef,
    producer_step: String,
    action_step: String,
    task: Pin,
    projection: Pin,
    action_projection: Pin,
    capabilities: Vec<ResolvedReference<CapabilityId>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifierDocument {
    schema_version: SchemaVersion,
    mode: VerifierMode,
    scope: ContextScopeId,
    goal: Pin,
    max_iterations: u32,
    max_retries: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum VerifierMode {
    RetainedObservations,
}
pub(super) struct VerifierSettings {
    pub(super) max_iterations: u32,
    pub(super) max_retries: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateUniverseDocument {
    schema_version: SchemaVersion,
    scope: ContextScopeId,
    source_contracts: Vec<StateSourceContract>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateSourceContract {
    source: SourceId,
    mapping_reference: String,
    export_revision: ReferenceId,
    subjects: Vec<gateway_domain::SubjectPath>,
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
enum SnapshotAdmission {
    AtomicCompleteRead {
        state_universe: Vec<SourceId>,
        provider_contract: Pin,
        read: ReadAdmission,
    },
    VersionedCompleteSet {
        state_universe: Vec<SourceId>,
        provider_contract: Pin,
        version_field: String,
        consistency: VersionConsistency,
        reads: Vec<ReadAdmission>,
    },
}
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum VersionConsistency {
    SameRevision,
}

fn invalid() -> SessionError {
    SessionError::InvalidInput
}
fn pin_contract(pin: &Pin, scope: &ContextScopeId, contract: &str) -> Result<(), SessionError> {
    if &pin.scope != scope {
        return Err(SessionError::ScopeDenied);
    }
    if pin.contract_version != SchemaVersion::V1 || pin.contract.as_str() != contract {
        return Err(invalid());
    }
    Ok(())
}
impl TryFrom<RuntimeWire> for RuntimeAdmission {
    type Error = SessionError;
    fn try_from(w: RuntimeWire) -> Result<Self, Self::Error> {
        if w.schema_version != SchemaVersion::V1
            || w.documents.is_empty()
            || w.documents.len() > DOCUMENTS
            || w.steps.is_empty()
            || w.steps.len() > MAX_CONNECTOR_GOAL_STEPS
            || w.goal_reads.is_empty()
            || w.goal_reads.len() > MAX_CONNECTOR_GOAL_CONDITIONS
            || !w.credential_store.is_absolute()
            || w.access_profile.as_str().len() > 128
        {
            return Err(invalid());
        }
        w.stdio.validate()?;
        w.limits.validate()?;
        if w.compatibility.transports != BTreeSet::from([ConnectorTransport::Stdio]) {
            return Err(invalid());
        }
        let mut ids = BTreeSet::new();
        for document in &w.documents {
            if !document.path.is_absolute()
                || !ids.insert(document.pin.id.clone())
                || document.pin.contract_version != SchemaVersion::V1
            {
                return Err(invalid());
            }
            if document.pin.scope != w.scope {
                return Err(SessionError::ScopeDenied);
            }
        }
        pin_contract(&w.goal, &w.scope, "cg.intent")?;
        pin_contract(&w.manifest, &w.scope, "cg.connector-manifest")?;
        pin_contract(&w.mapping, &w.scope, "cg.connector-mapping")?;
        pin_contract(&w.verifier, &w.scope, "cg.connector-verifier")?;
        let mut steps = BTreeSet::new();
        for step in &w.steps {
            PlanStepId::new(step.step.clone()).map_err(|_| invalid())?;
            if !steps.insert(&step.step)
                || step.post_effect_reads.len() > MAX_CONNECTOR_PROPOSALS_PER_STEP
            {
                return Err(invalid());
            }
            if step.normalizations.is_empty()
                || step.normalizations.len() > 128
                || step
                    .normalizations
                    .keys()
                    .any(|key| key.is_empty() || key.len() > 128)
            {
                return Err(SessionError::LimitExceeded);
            }
            step.resources.validate()?;
            if !step.resources.goal_verification.is_zero() {
                // External final reads belong to the execution allowance;
                // replayed goal verification itself is pure retained-data work.
                return Err(invalid());
            }
            if let ProducerAdmission::Model(model) = &step.producer {
                PlanStepId::new(model.producer_step.clone()).map_err(|_| invalid())?;
                if model.fragments.len() > 64 || model.source_metadata.len() > 64 {
                    return Err(SessionError::LimitExceeded);
                }
            }
        }
        let this = Self(w, None);
        for (pin, contract) in this.references() {
            pin_contract(pin, &this.0.scope, contract)?;
        }
        for read in this.reads() {
            if read.mapping_reference.is_empty()
                || read.mapping_reference.len() > 128
                || read.arguments.len() > 64
                || serde_json::to_vec(&read.arguments)
                    .map_err(|_| invalid())?
                    .len()
                    > 65_536
            {
                return Err(SessionError::LimitExceeded);
            }
        }
        Ok(this)
    }
}
impl StdioAdmission {
    fn validate(&self) -> Result<(), SessionError> {
        if !self.executable.is_absolute()
            || !self.directory.is_absolute()
            || self.arguments.len() > 128
            || self.environment.len() > 64
            || self.arguments.iter().any(|s| {
                s.len() > 8192 || s.contains('\0') || {
                    let key = s.trim_start_matches('-').split('=').next().unwrap_or("");
                    gateway_application::codex::security::credential_name(key)
                        || matches!(
                            key.to_ascii_uppercase().as_str(),
                            "TOKEN" | "AUTH" | "CREDENTIAL"
                        )
                }
            })
            || self.environment.iter().any(|(k, v)| {
                k.is_empty()
                    || k.len() > 128
                    || v.len() > 8192
                    || k.contains(['=', '\0'])
                    || v.contains('\0')
                    || gateway_application::codex::security::credential_name(k)
                    || [
                        "TOKEN",
                        "SECRET",
                        "AUTH",
                        "CREDENTIAL",
                        "API_KEY",
                        "PRIVATE_KEY",
                    ]
                    .iter()
                    .any(|name| k.to_ascii_uppercase().contains(name))
            })
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn build(&self) -> StdioConfig {
        StdioConfig {
            executable: self.executable.clone(),
            arguments: self.arguments.clone(),
            directory: self.directory.clone(),
            environment: self.environment.clone(),
        }
    }
}
impl LimitAdmission {
    fn validate(&self) -> Result<(), SessionError> {
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
            || self.shutdown_ms > 5_000
        {
            return Err(SessionError::LimitExceeded);
        }
        Ok(())
    }
    fn build(&self) -> Limits {
        Limits {
            frame_bytes: self.frame_bytes,
            inventory_bytes: self.inventory_bytes,
            items: self.items,
            pages: self.pages,
            events_per_request: self.events_per_request,
            call_timeout: Duration::from_millis(self.call_timeout_ms),
            shutdown: Duration::from_millis(self.shutdown_ms),
        }
    }
}

/// Owned canonical data from exactly the bytes checked by this loader. No path
/// is exposed to assembly and no successful pin check is followed by reopening.
pub(super) struct PinnedDocument {
    pin: Pin,
    canonical: Vec<u8>,
    value: Value,
}
pub(super) struct LoadedRuntime {
    resolved: Option<gateway_application::resolution_application::ResolvedPlan>,
    documents: BTreeMap<ReferenceId, PinnedDocument>,
    manifest: ConnectorManifest,
    mapping: MappingDocument,
    stdio: StdioConfig,
    limits: Limits,
}
impl LoadedRuntime {
    pub(super) fn resolved(
        &self,
    ) -> Result<&gateway_application::resolution_application::ResolvedPlan, SessionError> {
        self.resolved
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)
    }
    pub(super) fn document(&self, pin: &Pin) -> Result<&Value, SessionError> {
        let document = self
            .documents
            .get(&pin.id)
            .ok_or(SessionError::Unavailable)?;
        if document.pin != *pin {
            return Err(SessionError::StaleRevision);
        }
        Ok(&document.value)
    }
    pub(super) fn manifest(&self) -> &ConnectorManifest {
        &self.manifest
    }
    pub(super) fn mapping(&self) -> &MappingDocument {
        &self.mapping
    }
    pub(super) fn stdio(&self) -> &StdioConfig {
        &self.stdio
    }
    pub(super) fn limits(&self) -> &Limits {
        &self.limits
    }
}
// Full immutable pin identity; equal IDs alone never deduplicate graph edges.
type PinIdentity = (
    ReferenceId,
    ContextScopeId,
    ReferenceId,
    SchemaVersion,
    ReferenceId,
    gateway_domain::ContentDigest,
);
fn pin_identity(pin: &Pin) -> PinIdentity {
    (
        pin.id.clone(),
        pin.scope.clone(),
        pin.contract.clone(),
        pin.contract_version,
        pin.revision.clone(),
        pin.digest.clone(),
    )
}
fn reference_pin<I: AsRef<str>>(reference: &ResolvedReference<I>) -> Result<Pin, SessionError> {
    Ok(Pin {
        id: ReferenceId::new(reference.id.as_ref()).map_err(|_| invalid())?,
        scope: reference.scope.clone(),
        contract: reference.contract.clone(),
        contract_version: reference.contract_version,
        revision: reference.revision.clone(),
        digest: reference.digest.clone(),
    })
}

/// Supported edge owners only. Other records are retained opaque leaves: in
/// particular projection, context-analysis and source-metadata have no native
/// wire decoder here. This is bounded document discovery, not executable
/// producer admission or authoritative TaskTarget association validation.
fn document_references(pin: &Pin, value: &Value) -> Result<Vec<Pin>, SessionError> {
    use gateway_domain::semantic_output::{OutputContract, VerificationContract};
    if model_documents::ModelDocument::supports(pin.contract.as_str()) {
        // Existing opaque operator data remains passive. Only the declared
        // owning profile opens native model/context reference discovery.
        if matches!(
            pin.contract.as_str(),
            "cg.connector-model" | "cg.context-analysis" | "cg.context-source-metadata"
        ) {
            match value.get("profile") {
                None => return Ok(vec![]),
                Some(Value::String(profile)) if profile == "NATIVE_SESSION_MODEL_V1" => {}
                Some(_) => return Err(SessionError::UnsupportedCapability),
            }
        }
        return Ok(model_documents::ModelDocument::parse(
            pin,
            &serde_json::to_vec(value).map_err(|_| invalid())?,
        )?
        .references());
    }
    match pin.contract.as_str() {
        "cg.connector-selector-association" => {
            Ok(super::selector_association::SelectorAssociation::from_json(
                pin,
                &serde_json::to_vec(value).map_err(|_| invalid())?,
            )?
            .reference_bindings())
        }
        "cg.connector-object" => Ok(super::selector_association::ScopeObjectDocument::from_json(
            pin,
            &serde_json::to_vec(value).map_err(|_| invalid())?,
        )?
        .reference_bindings()),
        "cg.semantic-task" => {
            let task: gateway_domain::SemanticTaskIR =
                serde_json::from_value(value.clone()).map_err(|_| invalid())?;
            task.reference_bindings()
                .into_iter()
                .map(|binding| {
                    Ok(Pin {
                        id: ReferenceId::new(binding.id).map_err(|_| invalid())?,
                        scope: binding.scope,
                        contract: binding.contract,
                        contract_version: binding.contract_version,
                        revision: binding.revision,
                        digest: binding.digest,
                    })
                })
                .collect()
        }
        "cg.output-contract" => {
            let output: OutputContract =
                serde_json::from_value(value.clone()).map_err(|_| invalid())?;
            if output.schema_version != SchemaVersion::V1 {
                return Err(invalid());
            }
            Ok(vec![output.schema])
        }
        "cg.verification-contract" => {
            let verification: VerificationContract =
                serde_json::from_value(value.clone()).map_err(|_| invalid())?;
            if verification.schema_version != SchemaVersion::V1 {
                return Err(invalid());
            }
            Ok(verification
                .rules
                .into_iter()
                .flat_map(|rule| [rule.rule, rule.verifier])
                .collect())
        }
        "cg.connector-producer-join" => {
            let join: ProducerJoinDocument =
                serde_json::from_value(value.clone()).map_err(|_| invalid())?;
            if join.schema_version != SchemaVersion::V1 {
                return Err(invalid());
            }
            let mut edges = vec![join.task, join.projection, join.action_projection];
            for capability in join.capabilities {
                edges.push(reference_pin(&capability)?);
            }
            // The native registered plan RecordRef is validated by its current
            // canonical owner, not reinterpreted as an arbitrary sidecar pin.
            Ok(edges)
        }
        _ => Ok(vec![]),
    }
}

impl RuntimeAdmission {
    pub(super) fn retain(&mut self, documents: LoadedRuntime) {
        self.1 = Some(Arc::new(documents));
    }
    pub(super) fn loaded(&self) -> Result<&LoadedRuntime, SessionError> {
        self.1.as_deref().ok_or(SessionError::Unavailable)
    }
    pub(super) fn policy_reference(&self) -> &Pin {
        &self.0.policy
    }
    pub(super) fn reference(&self) -> Result<Pin, SessionError> {
        Ok(Pin {
            id: self.0.id.clone(),
            scope: self.0.scope.clone(),
            contract: ReferenceId::new("cg.connector-runtime").map_err(|_| invalid())?,
            contract_version: SchemaVersion::V1,
            revision: self.0.revision.clone(),
            digest: self.configuration_digest()?,
        })
    }
    pub(super) fn configuration_digest(
        &self,
    ) -> Result<gateway_domain::ContentDigest, SessionError> {
        let value = serde_json::to_value(&self.0).map_err(|_| invalid())?;
        Ok(gateway_domain::context_content_digest(
            &serde_json::to_vec(&value).map_err(|_| invalid())?,
        ))
    }
    pub(super) fn verifier(
        &self,
        config: &SessionAdmission,
    ) -> Result<VerifierSettings, SessionError> {
        self.verifier_document(self.loaded()?, config)
    }
    fn verifier_document(
        &self,
        loaded: &LoadedRuntime,
        config: &SessionAdmission,
    ) -> Result<VerifierSettings, SessionError> {
        let document: VerifierDocument =
            serde_json::from_value(loaded.document(&self.0.verifier)?.clone())
                .map_err(|_| invalid())?;
        let VerifierMode::RetainedObservations = document.mode;
        if document.schema_version != SchemaVersion::V1
            || document.scope != self.0.scope
            || document.goal != self.0.goal
        {
            return Err(SessionError::StaleRevision);
        }
        if document.max_iterations == 0
            || document.max_iterations > 64
            || document.max_iterations > config.max_actions
            || document.max_retries > 64
            || document.max_retries > config.max_retries
        {
            return Err(SessionError::LimitExceeded);
        }
        Ok(VerifierSettings {
            max_iterations: document.max_iterations,
            max_retries: document.max_retries,
        })
    }
    fn references(&self) -> Vec<(&Pin, &'static str)> {
        let w = &self.0;
        let mut pins = vec![
            (&w.policy, "cg.connector-canonical-policy"),
            (&w.manifest, "cg.connector-manifest"),
            (&w.mapping, "cg.connector-mapping"),
            (&w.verifier, "cg.connector-verifier"),
        ];
        for step in &w.steps {
            pins.push((&step.quotes, "cg.connector-quotes"));
            pins.extend(
                step.normalizations
                    .values()
                    .map(|pin| (pin, "cg.connector-normalization")),
            );
            pins.push((&step.action_projection, "cg.context-projection"));
            match &step.producer {
                ProducerAdmission::Structured { operations } => {
                    pins.push((operations, "cg.connector-operations"))
                }
                ProducerAdmission::Model(model) => {
                    let ModelAdmission {
                        association,
                        join,
                        task,
                        projection,
                        context_analysis,
                        fragments,
                        source_metadata,
                        output,
                        verification,
                        model,
                        ..
                    } = model.as_ref();
                    pins.push((association, "cg.connector-selector-association"));
                    pins.push((join, "cg.connector-producer-join"));
                    pins.extend([
                        (task, "cg.semantic-task"),
                        (projection, "cg.context-projection"),
                        (context_analysis, "cg.context-analysis"),
                        (output, "cg.output-contract"),
                        (verification, "cg.verification-contract"),
                        (model, "cg.connector-model"),
                    ]);
                    pins.extend(fragments.iter().map(|p| (p, "cg.context-fragment")));
                    pins.extend(
                        source_metadata
                            .iter()
                            .map(|p| (p, "cg.context-source-metadata")),
                    );
                }
            }
        }
        match &w.snapshots {
            SnapshotAdmission::AtomicCompleteRead {
                provider_contract, ..
            }
            | SnapshotAdmission::VersionedCompleteSet {
                provider_contract, ..
            } => pins.push((provider_contract, "cg.connector-state-universe")),
        }
        for read in self.reads() {
            pins.push((&read.normalization, "cg.connector-normalization"));
        }
        pins
    }
    fn reads(&self) -> Vec<&ReadAdmission> {
        let mut reads: Vec<_> = self
            .0
            .steps
            .iter()
            .flat_map(|s| &s.post_effect_reads)
            .collect();
        reads.extend(self.0.goal_reads.iter().map(|r| &r.read));
        match &self.0.snapshots {
            SnapshotAdmission::AtomicCompleteRead { read, .. } => reads.push(read),
            SnapshotAdmission::VersionedCompleteSet {
                reads: state_reads, ..
            } => reads.extend(state_reads),
        }
        reads
    }
    pub(super) fn validate_registration(
        &self,
        config: &SessionAdmission,
        scope: &ContextScopeId,
    ) -> Result<(), SessionError> {
        let w = &self.0;
        if &w.scope != scope {
            return Err(SessionError::ScopeDenied);
        }
        let goal = registered_connector_goal(config, scope)?;
        if config.basis.is_some()
            || w.plan != *goal.plan()
            || w.execution != config.execution
            || w.goal.id.as_str() != config.intent.id().as_str()
            || w.goal.digest
                != gateway_domain::context_content_digest(
                    &serde_json::to_vec(
                        &serde_json::to_value(&config.intent).map_err(|_| invalid())?,
                    )
                    .map_err(|_| invalid())?,
                )
            || w.steps
                .iter()
                .map(|s| s.step.as_str())
                .ne(goal.steps().iter().map(|s| s.as_str()))
            || w.steps.len() > config.max_actions as usize
        {
            return Err(SessionError::StaleRevision);
        }
        let limits = config
            .resource_limits
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        limits.validate()?;
        let mut total = SessionResourceAmounts::default();
        for step in &w.steps {
            total = total.checked_add(&step.resources.total()?)?;
        }
        if !total.within(&limits.amounts) {
            return Err(SessionError::LimitExceeded);
        }
        let mut conditions = BTreeSet::new();
        for read in &w.goal_reads {
            if !conditions.insert(&read.condition)
                || !goal.observations().iter().any(|m| {
                    m.condition().as_str() == read.condition
                        && m.source() == &read.read.source
                        && m.capability() == &read.read.capability
                })
            {
                return Err(invalid());
            }
        }
        if conditions.len() != goal.observations().len() {
            return Err(invalid());
        }
        Ok(())
    }
    /// Structured DTOs are retainable operator data, but cannot be promoted to
    /// the model-only PreparedConnectorTurn path by manufacturing an invocation.
    pub(super) fn require_model_producer(&self) -> Result<(), SessionError> {
        if self
            .0
            .steps
            .iter()
            .any(|s| matches!(s.producer, ProducerAdmission::Structured { .. }))
        {
            return Err(SessionError::UnsupportedCapability);
        }
        Ok(())
    }
    fn load_documents(
        &self,
        host: &LocalCodexHost,
    ) -> Result<BTreeMap<ReferenceId, PinnedDocument>, SessionError> {
        let config = host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        let scope = &host.binding.canonical_scope;
        let resources = &host.resources;
        self.validate_registration(config, scope)?;
        let mut documents = BTreeMap::new();
        let mut bytes = 0usize;
        for admitted in &self.0.documents {
            #[cfg(test)]
            let file_profile = profiling::span("runtime.documents.file_io");
            if !std::fs::metadata(&admitted.path)
                .map_err(|_| SessionError::Unavailable)?
                .is_file()
            {
                return Err(SessionError::Unavailable);
            }
            let raw = std::fs::File::open(&admitted.path).map_err(|_| SessionError::Unavailable)?;
            if !raw
                .metadata()
                .map_err(|_| SessionError::Unavailable)?
                .is_file()
            {
                return Err(SessionError::Unavailable);
            }
            let mut owned = Vec::new();
            raw.take((DOCUMENT_BYTES + 1) as u64)
                .read_to_end(&mut owned)
                .map_err(|_| invalid())?;
            if owned.len() > DOCUMENT_BYTES {
                return Err(SessionError::LimitExceeded);
            }
            bytes = bytes
                .checked_add(owned.len())
                .ok_or(SessionError::LimitExceeded)?;
            if bytes > CATALOG_BYTES {
                return Err(SessionError::LimitExceeded);
            }
            #[cfg(test)]
            drop(file_profile);
            #[cfg(test)]
            let decode_profile = profiling::span("runtime.documents.decode_credentials");
            let value = crate::local_mcp::decode::decode(&owned).map_err(|_| invalid())?;
            if !value.is_object() || !gateway_application::codex::security::credential_free(&value)
            {
                return Err(SessionError::AuthorityDenied);
            }
            #[cfg(test)]
            drop(decode_profile);
            #[cfg(test)]
            let _canonical = profiling::span("runtime.documents.canonical_digest");
            let canonical = serde_json::to_vec(&value).map_err(|_| invalid())?;
            if gateway_domain::context_content_digest(&canonical) != admitted.pin.digest {
                return Err(SessionError::StaleRevision);
            }
            documents.insert(
                admitted.pin.id.clone(),
                PinnedDocument {
                    pin: admitted.pin.clone(),
                    canonical,
                    value,
                },
            );
        }
        #[cfg(test)]
        let _closure = profiling::span("runtime.documents.typed_closure");
        let mut pending = VecDeque::new();
        let mut scheduled = BTreeSet::new();
        for (pin, _) in self.references() {
            if scheduled.insert(pin_identity(pin)) {
                pending.push_back(pin.clone());
            }
        }
        if scheduled.len() > DOCUMENTS {
            return Err(SessionError::LimitExceeded);
        }
        let mut reachable = BTreeSet::new();
        while let Some(pin) = pending.pop_front() {
            pin_contract(&pin, scope, pin.contract.as_str())?;
            if !documents.contains_key(&pin.id) {
                // No new contract is admitted into the frozen inbound namespace.
                if !matches!(
                    pin.contract.as_str(),
                    "cg.context-projection" | "cg.context-fragment"
                ) {
                    return Err(SessionError::Unavailable);
                }
                let resource = resources
                    .iter()
                    .find(|r| {
                        r["reference"]["id"] == pin.id.as_str()
                            && r["reference"]["contract"] == pin.contract.as_str()
                            && r["reference"]["contract_version"]
                                == pin.contract_version.to_string()
                            && r["reference"]["revision"] == pin.revision.as_str()
                            && r["reference"]["digest"] == format!("sha256:{}", pin.digest.as_str())
                    })
                    .ok_or(SessionError::StaleRevision)?;
                if resource["provenance"]
                    .as_array()
                    .is_some_and(|items| items.iter().any(|p| p["sensitivity"] == "SECRET"))
                {
                    return Err(SessionError::AuthorityDenied);
                }
                let value = resource["document"].clone();
                let canonical = serde_json::to_vec(&value).map_err(|_| invalid())?;
                if canonical.len() > DOCUMENT_BYTES {
                    return Err(SessionError::LimitExceeded);
                }
                bytes = bytes
                    .checked_add(canonical.len())
                    .ok_or(SessionError::LimitExceeded)?;
                if bytes > CATALOG_BYTES || documents.len() == DOCUMENTS {
                    return Err(SessionError::LimitExceeded);
                }
                if gateway_domain::context_content_digest(&canonical) != pin.digest {
                    return Err(SessionError::StaleRevision);
                }
                documents.insert(
                    pin.id.clone(),
                    PinnedDocument {
                        pin: pin.clone(),
                        canonical,
                        value,
                    },
                );
            }
            let document = documents.get(&pin.id).ok_or(SessionError::Unavailable)?;
            if document.pin != pin {
                return Err(SessionError::StaleRevision);
            }
            reachable.insert(pin_identity(&pin));
            for edge in document_references(&pin, &document.value)? {
                pin_contract(&edge, scope, edge.contract.as_str())?;
                if scheduled.insert(pin_identity(&edge)) {
                    if scheduled.len() > DOCUMENTS {
                        return Err(SessionError::LimitExceeded);
                    }
                    pending.push_back(edge);
                }
            }
        }
        if self
            .0
            .documents
            .iter()
            .any(|document| !reachable.contains(&pin_identity(&document.pin)))
        {
            return Err(invalid());
        }
        Ok(documents)
    }
    pub(super) fn load(&self, host: &LocalCodexHost) -> Result<LoadedRuntime, SessionError> {
        #[cfg(test)]
        let _total = profiling::span("runtime.load.total");
        let config = host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        let documents = {
            #[cfg(test)]
            let _documents = profiling::span("runtime.load.documents");
            self.load_documents(host)?
        };
        #[cfg(test)]
        let admission_profile = profiling::span("runtime.load.mapping_profiles");
        let manifest_doc = &documents
            .get(&self.0.manifest.id)
            .ok_or(SessionError::Unavailable)?
            .canonical;
        let mapping_doc = &documents
            .get(&self.0.mapping.id)
            .ok_or(SessionError::Unavailable)?
            .canonical;
        let manifest = ConnectorManifest::from_json(manifest_doc).map_err(|_| invalid())?;
        let mapping = MappingDocument::from_json(mapping_doc).map_err(|_| invalid())?;
        let compatibility = ConnectorCompatibilityProfile {
            gateway: self.0.compatibility.gateway,
            transports: self.0.compatibility.transports.clone(),
        };
        if manifest.connector_id() != self.0.connector.as_str()
            || manifest.definition().transport != ConnectorTransport::Stdio
            || !matches!(
                manifest.compatibility(&compatibility),
                CompatibilityDecision::Compatible | CompatibilityDecision::CompatibleDeprecated
            )
        {
            return Err(invalid());
        }
        for record in mapping.records() {
            if !manifest.definition().inventory.iter().any(|entry| {
                entry.kind == record.kind
                    && entry.name == record.name
                    && (entry.capability_mapping_ref.as_deref() == Some(&record.reference)
                        || entry.source_mapping_ref.as_deref() == Some(&record.reference))
            }) {
                return Err(invalid());
            }
        }
        for read in self.reads() {
            let record = mapping
                .records()
                .iter()
                .find(|r| {
                    r.reference == read.mapping_reference
                        && r.capability == read.capability
                        && r.source.as_ref() == Some(&read.source)
                })
                .ok_or(invalid())?;
            if !matches!(
                record.class,
                OperationClass::Read | OperationClass::Search | OperationClass::Inspect
            ) || (record.kind == InventoryKind::Resource && !read.arguments.is_empty())
            {
                return Err(invalid());
            }
        }
        // Structured records may be retained as data without native authority.
        // The executable Source requires an actual current canonical owner.
        if let Some(canonical) = &host.canonical
            && documents
                .get(&self.0.policy.id)
                .ok_or(SessionError::Unavailable)?
                .value
                != canonical.policy
        {
            return Err(SessionError::StaleRevision);
        }
        for step in &self.0.steps {
            if step.normalizations.len() != mapping.records().len() {
                return Err(invalid());
            }
            for record in mapping.records() {
                let pin = step
                    .normalizations
                    .get(&record.reference)
                    .ok_or(invalid())?;
                let profile: NormalizationDocument = serde_json::from_value(
                    documents
                        .get(&pin.id)
                        .ok_or(SessionError::Unavailable)?
                        .value
                        .clone(),
                )
                .map_err(|_| invalid())?;
                if profile.object_field.is_empty()
                    || profile.object_field.len() > 128
                    || profile.fields.is_empty()
                    || profile.fields.len() > 256
                    || profile.fields.keys().any(|name| {
                        name.is_empty() || name.len() > 128 || name == &profile.object_field
                    })
                    || profile.fields.values().collect::<BTreeSet<_>>().len()
                        != profile.fields.len()
                {
                    return Err(invalid());
                }
                if profile.schema_version != SchemaVersion::V1
                    || record.source.as_ref() != Some(&profile.source)
                    || profile.acquisition.authenticated_profile != self.0.access_profile
                    || profile.acquisition.source_kind == gateway_domain::SourceKind::Model
                {
                    return Err(invalid());
                }
                if let ReadSemantics::DirectCurrentReadProfile {
                    max_age_seconds,
                    time_basis: ReadTimeBasis::RequestStartLowerBound,
                } = &profile.acquisition.semantics
                    && (*max_age_seconds == 0 || *max_age_seconds > 3600)
                {
                    return Err(SessionError::LimitExceeded);
                }
            }
            let mut required = match &self.0.snapshots {
                SnapshotAdmission::AtomicCompleteRead { read, .. } => vec![read],
                SnapshotAdmission::VersionedCompleteSet { reads, .. } => reads.iter().collect(),
            };
            required.extend(self.0.goal_reads.iter().map(|read| &read.read));
            required.extend(step.post_effect_reads.iter());
            for read in required {
                if step.normalizations.get(&read.mapping_reference) != Some(&read.normalization) {
                    return Err(SessionError::StaleRevision);
                }
            }
        }
        #[cfg(test)]
        drop(admission_profile);
        // Resolve once from this fresh host capture. This owned data is never
        // reused across current-authority callbacks.
        let resolved = if self
            .0
            .steps
            .iter()
            .any(|step| matches!(step.producer, ProducerAdmission::Model(_)))
        {
            let canonical = host
                .canonical
                .as_ref()
                .ok_or(SessionError::UnsupportedCapability)?;
            let (snapshot, rules) = {
                #[cfg(test)]
                let _capture = profiling::span("runtime.load.canonical_capture");
                host_mapping::capture(
                    &canonical.catalog,
                    &canonical.documents(&host.resources).map_err(error)?,
                )
                .map_err(error)?
            };
            let resolved = {
                #[cfg(test)]
                let _resolve = profiling::span("runtime.load.canonical_resolve");
                DeclarativeResolutionApplication
                    .resolve_plan(&snapshot, &rules)
                    .map_err(|_| invalid())?
            };
            Some(resolved)
        } else {
            None
        };
        {
            #[cfg(test)]
            let _producer = profiling::span("runtime.load.producers");
            self.validate_producers(host, &documents, resolved.as_ref())?;
        }
        {
            #[cfg(test)]
            let _quotes = profiling::span("runtime.load.quotes");
            self.validate_quotes(config, &mapping, &documents)?;
        }
        {
            #[cfg(test)]
            let _snapshots = profiling::span("runtime.load.snapshots");
            self.validate_snapshots(&mapping, &documents)?;
        }
        let loaded = LoadedRuntime {
            resolved,
            documents,
            manifest,
            mapping,
            stdio: self.0.stdio.build(),
            limits: self.0.limits.build(),
        };
        {
            #[cfg(test)]
            let _verifier = profiling::span("runtime.load.verifier");
            self.verifier_document(&loaded, config)?;
        }
        Ok(loaded)
    }
    fn validate_snapshots(
        &self,
        mapping: &MappingDocument,
        documents: &BTreeMap<ReferenceId, PinnedDocument>,
    ) -> Result<(), SessionError> {
        let (universe, reads, version, provider) = match &self.0.snapshots {
            SnapshotAdmission::AtomicCompleteRead {
                state_universe,
                read,
                provider_contract,
                ..
            } => (state_universe, vec![read], None, provider_contract),
            SnapshotAdmission::VersionedCompleteSet {
                state_universe,
                reads,
                version_field,
                consistency: VersionConsistency::SameRevision,
                provider_contract,
                ..
            } => (
                state_universe,
                reads.iter().collect(),
                Some(version_field),
                provider_contract,
            ),
        };
        let expected: BTreeSet<_> = universe.iter().collect();
        let actual: BTreeSet<_> = reads.iter().map(|r| &r.source).collect();
        if universe.is_empty()
            || universe.len() > 64
            || expected.len() != universe.len()
            || reads.len() != universe.len()
            || actual != expected
        {
            return Err(invalid());
        }
        let contract: StateUniverseDocument = serde_json::from_value(
            documents
                .get(&provider.id)
                .ok_or(SessionError::Unavailable)?
                .value
                .clone(),
        )
        .map_err(|_| invalid())?;
        if contract.schema_version != SchemaVersion::V1
            || contract.scope != self.0.scope
            || contract.source_contracts.len() != reads.len()
        {
            return Err(invalid());
        }
        let mut seen = BTreeSet::new();
        for source in contract.source_contracts {
            let read = reads
                .iter()
                .find(|read| {
                    read.source == source.source
                        && read.mapping_reference == source.mapping_reference
                })
                .ok_or(invalid())?;
            let profile: NormalizationDocument = serde_json::from_value(
                documents
                    .get(&read.normalization.id)
                    .ok_or(SessionError::Unavailable)?
                    .value
                    .clone(),
            )
            .map_err(|_| invalid())?;
            let subjects: BTreeSet<_> = source.subjects.iter().collect();
            if source.export_revision.as_str().is_empty()
                || !seen.insert(source.source)
                || source.subjects.is_empty()
                || subjects.len() != source.subjects.len()
                || subjects != profile.fields.values().collect()
            {
                return Err(invalid());
            }
        }
        if let Some(version) = version {
            if version.is_empty() || version.len() > 128 {
                return Err(invalid());
            }
            for read in reads {
                let record = mapping
                    .records()
                    .iter()
                    .find(|r| r.reference == read.mapping_reference)
                    .ok_or(invalid())?;
                let profile = &documents
                    .get(&read.normalization.id)
                    .ok_or(SessionError::Unavailable)?
                    .value;
                let schema = record.result_schema.as_ref().ok_or(invalid())?;
                if record.kind != InventoryKind::Tool
                    || profile["revision_field"].as_str() != Some(version)
                    || schema["properties"][version]["type"] != "string"
                    || !schema["required"]
                        .as_array()
                        .is_some_and(|required| required.iter().any(|v| v == version))
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
    fn validate_producers(
        &self,
        host: &LocalCodexHost,
        documents: &BTreeMap<ReferenceId, PinnedDocument>,
        resolved: Option<&gateway_application::resolution_application::ResolvedPlan>,
    ) -> Result<(), SessionError> {
        for step in &self.0.steps {
            let action_doc = &documents
                .get(&step.action_projection.id)
                .ok_or(SessionError::Unavailable)?
                .value;
            let action_id = action_doc["id"].as_str().ok_or(invalid())?;
            gateway_domain::ExecutionContextId::new(action_id).map_err(|_| invalid())?;
            let task_id = action_doc["task"]["id"].as_str().ok_or(invalid())?;
            gateway_domain::TaskId::new(task_id).map_err(|_| invalid())?;
            if action_doc["step"] != step.step
                || action_doc["basis"]["scope"] != self.0.scope.as_str()
            {
                return Err(SessionError::StaleRevision);
            }
            let ProducerAdmission::Model(model) = &step.producer else {
                continue;
            };
            let ModelAdmission {
                producer_step,
                join,
                task,
                projection,
                ..
            } = model.as_ref();
            #[cfg(test)]
            let typed_profile = profiling::span("runtime.producer.typed_joins");
            let semantic: gateway_domain::SemanticTaskIR = serde_json::from_value(
                documents
                    .get(&task.id)
                    .ok_or(SessionError::Unavailable)?
                    .value
                    .clone(),
            )
            .map_err(|_| invalid())?;
            if task_id != semantic.data().id.as_str() {
                return Err(SessionError::StaleRevision);
            }
            let goal = &host
                .sessions
                .as_ref()
                .ok_or(SessionError::UnsupportedCapability)?
                .intent;
            if semantic.data().target.reference.scope != self.0.scope {
                return Err(SessionError::ScopeDenied);
            }
            if semantic.data().desired_state.as_ref() != Some(goal.desired_state()) {
                return Err(SessionError::StaleRevision);
            }
            let bound: ProducerJoinDocument = serde_json::from_value(
                documents
                    .get(&join.id)
                    .ok_or(SessionError::Unavailable)?
                    .value
                    .clone(),
            )
            .map_err(|_| invalid())?;
            if bound.schema_version != SchemaVersion::V1
                || bound.plan != self.0.plan
                || bound.producer_step != *producer_step
                || bound.action_step != step.step
                || bound.task != *task
                || bound.projection != *projection
                || bound.action_projection != step.action_projection
                || bound.capabilities.is_empty()
                || bound.capabilities.len() > MAX_CONNECTOR_PROPOSALS_PER_STEP
            {
                return Err(SessionError::StaleRevision);
            }
            let projection_doc = &documents
                .get(&projection.id)
                .ok_or(SessionError::Unavailable)?
                .value;
            if projection_doc["step"] != *producer_step
                || projection_doc["task"]["id"] != semantic.data().id.as_str()
                || projection_doc["basis"]["scope"] != self.0.scope.as_str()
            {
                return Err(SessionError::StaleRevision);
            }
            // Native resolution supplies both real plan steps. Preparation is
            // read-only; proposed action capabilities are not its effect grant.
            let canonical = host
                .canonical
                .as_ref()
                .ok_or(SessionError::UnsupportedCapability)?;
            let resolved = resolved.ok_or(SessionError::UnsupportedCapability)?;
            #[cfg(test)]
            drop(typed_profile);
            let (_, context, _) = {
                #[cfg(test)]
                let _policy = profiling::span("runtime.producer.policy");
                host_mapping::policy(resolved, canonical.policy.clone()).map_err(error)?
            };
            #[cfg(test)]
            let input_profile = profiling::span("runtime.producer.step_inputs");
            let inputs = gateway_application::policy_application::PolicyApplication
                .step_inputs(resolved, &context)
                .map_err(|_| invalid())?;
            let preparation = inputs
                .get(&PlanStepId::new(producer_step.clone()).map_err(|_| invalid())?)
                .ok_or_else(invalid)?;
            let action = inputs
                .get(&PlanStepId::new(step.step.clone()).map_err(|_| invalid())?)
                .ok_or_else(invalid)?;
            if !preparation.resolved
                || preparation.capabilities.is_empty()
                || preparation
                    .capabilities
                    .values()
                    .any(|c| c.class() != CapabilityClass::Inspect)
                || !action.resolved
            {
                return Err(SessionError::AuthorityDenied);
            }
            #[cfg(test)]
            drop(input_profile);
            #[cfg(test)]
            let _aliases = profiling::span("runtime.producer.capability_aliases");
            let mut capabilities = BTreeSet::new();
            for capability in &bound.capabilities {
                let definition = action
                    .capabilities
                    .get(&capability.id)
                    .ok_or(SessionError::StaleRevision)?;
                let id = ReferenceId::new(capability.id.as_str()).map_err(|_| invalid())?;
                let registered = documents.get(&id).ok_or(SessionError::Unavailable)?;
                super::capability_alias::validate(
                    &self.0.scope,
                    capability,
                    &registered.pin,
                    &registered.value,
                    definition,
                )?;
                if !capabilities.insert(capability.id.clone()) {
                    return Err(invalid());
                }
            }
            if capabilities != action.capabilities.keys().cloned().collect() {
                return Err(SessionError::StaleRevision);
            }
        }
        Ok(())
    }
    fn validate_quotes(
        &self,
        config: &SessionAdmission,
        mapping: &MappingDocument,
        documents: &BTreeMap<ReferenceId, PinnedDocument>,
    ) -> Result<(), SessionError> {
        let limits = config
            .resource_limits
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        let valid = |amounts: &SessionResourceAmounts, read: bool| {
            amounts.connector_calls == 1
                && (!read || amounts.retrieval_rounds > 0)
                && amounts.model_attempts == 0
                && amounts.input_tokens == 0
                && amounts.output_tokens == 0
                && amounts.output_bytes == 0
                && amounts.repair_attempts == 0
                && amounts.worker_attempts == 0
                && amounts.worker_compute_units == 0
                && amounts.within(&limits.amounts)
        };
        for step in &self.0.steps {
            let doc: QuoteDocument = serde_json::from_value(
                documents
                    .get(&step.quotes.id)
                    .ok_or(SessionError::Unavailable)?
                    .value
                    .clone(),
            )
            .map_err(|_| invalid())?;
            if doc.schema_version != SchemaVersion::V1
                || doc.manifest != self.0.manifest
                || doc.mapping != self.0.mapping
                || doc.cost_unit != limits.cost_unit
                || !valid(&doc.discovery, true)
                || doc.operations.is_empty()
                || doc.operations.len() > 128
            {
                return Err(invalid());
            }
            let mut seen = BTreeSet::new();
            for quote in doc.operations {
                let record = mapping
                    .records()
                    .iter()
                    .find(|r| r.reference == quote.mapping_reference)
                    .ok_or(invalid())?;
                let read = matches!(
                    record.class,
                    OperationClass::Read | OperationClass::Search | OperationClass::Inspect
                );
                if !seen.insert(quote.mapping_reference) || !valid(&quote.amounts, read) {
                    return Err(invalid());
                }
            }
        }
        for read in self.reads() {
            let doc: NormalizationDocument = serde_json::from_value(
                documents
                    .get(&read.normalization.id)
                    .ok_or(SessionError::Unavailable)?
                    .value
                    .clone(),
            )
            .map_err(|_| invalid())?;
            let record = mapping
                .records()
                .iter()
                .find(|r| r.reference == read.mapping_reference)
                .ok_or(invalid())?;
            if doc.schema_version != SchemaVersion::V1
                || doc.source != read.source
                || doc.object_field.is_empty()
                || doc.fields.is_empty()
                || doc.fields.len() > 256
                || doc
                    .fields
                    .keys()
                    .any(|k| k.is_empty() || k == &doc.object_field)
                || doc.fields.values().collect::<BTreeSet<_>>().len() != doc.fields.len()
                || (record.kind == InventoryKind::Resource
                    && (doc.fields.len() != 1
                        || doc.revision_field.is_some()
                        || doc.digest_field.is_some()))
            {
                return Err(invalid());
            }
        }
        for goal_read in &self.0.goal_reads {
            let condition = config
                .intent
                .desired_state()
                .conditions()
                .iter()
                .find(|c| c.id().as_str() == goal_read.condition)
                .ok_or(invalid())?;
            let profile: NormalizationDocument = serde_json::from_value(
                documents
                    .get(&goal_read.read.normalization.id)
                    .ok_or(SessionError::Unavailable)?
                    .value
                    .clone(),
            )
            .map_err(|_| invalid())?;
            let record = mapping
                .records()
                .iter()
                .find(|r| r.reference == goal_read.read.mapping_reference)
                .ok_or(invalid())?;
            if !profile
                .fields
                .values()
                .any(|subject| subject == condition.subject())
                || (record.kind == InventoryKind::Resource
                    && condition
                        .expected()
                        .is_some_and(|value| !matches!(value, TypedValue::String(_))))
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(in crate::local_sessions) mod tests {
    use super::*;
    use crate::local_sessions::connector_admission_tests::{config_value, workspace_fixture};
    pub(in crate::local_sessions) struct Fixture {
        pub(in crate::local_sessions) wire: Value,
        pub(in crate::local_sessions) root: PathBuf,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn pin(id: &str, contract: &str, scope: &str, value: &Value) -> Value {
        json!({"id":id,"scope":scope,"contract":contract,"contract_version":"1.0","revision":"1","digest":gateway_domain::context_content_digest(&serde_json::to_vec(value).unwrap())})
    }
    impl Fixture {
        pub(in crate::local_sessions) fn new() -> Self {
            Self::with_scope("runtime-sidecar-scope")
        }
        pub(in crate::local_sessions) fn with_scope(scope: &str) -> Self {
            let (host, _, _, root) =
                workspace_fixture(Some(config_value("read")), scope, "runtime-client", None);
            let mut wire: Value =
                serde_json::from_str(&std::fs::read_to_string(&host.access.admission).unwrap())
                    .unwrap();
            let config = &mut wire["mappings"][0]["sessions"];
            config["intent"]["desired_state"]["conditions"][0]["subject"] =
                json!("records.content");
            config["intent"]["desired_state"]["conditions"][0]["expected"] =
                json!({"kind":"STRING","value":"ready"});
            config["connector_goal"]["observations"][0]["source"] = json!("records.one");
            config["connector_goal"]["observations"][0]["capability"] =
                json!("records.resource.read");
            let amounts = SessionResourceAmounts {
                connector_calls: 1,
                retrieval_rounds: 1,
                retrieval_results: 1,
                retrieval_tokens: 100,
                ..Default::default()
            };
            config["resource_limits"] = serde_json::to_value(SessionResourceLimits {
                cost_unit: ReferenceId::new("credits").unwrap(),
                amounts: SessionResourceAmounts {
                    connector_calls: 10,
                    retrieval_rounds: 10,
                    retrieval_results: 10,
                    retrieval_tokens: 1000,
                    ..Default::default()
                },
            })
            .unwrap();
            let mut catalog = Vec::new();
            let mut record = |id: &str, contract: &str, value: Value| {
                let reference = pin(id, contract, scope, &value);
                let path = root.join(format!("{id}.json"));
                std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                catalog.push(json!({"pin":reference,"path":path}));
                reference
            };
            let policy = record(
                "canonical-policy",
                "cg.connector-canonical-policy",
                json!({}),
            );
            let mut manifest_document: Value = serde_json::from_str(include_str!(
                "../../../../tests/fixtures/connector-mapping-v1/manifest.json"
            ))
            .unwrap();
            manifest_document["manifest"]["inventory"][0]["source_mapping_ref"] =
                json!("mapping.records.read.v1");
            let manifest_document = ConnectorManifest::from_definition_json(
                &serde_json::to_vec(&manifest_document["manifest"]).unwrap(),
            )
            .unwrap();
            let manifest = record(
                "manifest",
                "cg.connector-manifest",
                serde_json::from_slice(&manifest_document.canonical_json()).unwrap(),
            );
            let mut mapping_document: Value = serde_json::from_str(include_str!(
                "../../../../tests/fixtures/connector-mapping-v1/mapping.json"
            ))
            .unwrap();
            mapping_document["records"][0]["source"] = json!("records.tool");
            let mapping = record("mapping", "cg.connector-mapping", mapping_document);
            let normalization = record(
                "normalization",
                "cg.connector-normalization",
                json!({"schema_version":"1.0","acquisition":{"source_kind":"TOOL","sensitivity":"NORMAL","authenticated_profile":"records.local","semantics":{"kind":"DIRECT_CURRENT_READ_PROFILE","max_age_seconds":60,"time_basis":"REQUEST_START_LOWER_BOUND"}},"source":"records.one","object_field":"object","revision_field":null,"digest_field":null,"fields":{"text":"records.content"}}),
            );
            let tool_normalization = record(
                "tool-normalization",
                "cg.connector-normalization",
                json!({"schema_version":"1.0","source":"records.tool","object_field":"object","revision_field":null,"digest_field":null,"fields":{"value":"records.content"},"acquisition":{"source_kind":"TOOL","sensitivity":"NORMAL","authenticated_profile":"records.local","semantics":{"kind":"PROVIDER_ACKNOWLEDGEMENT_ONLY"}}}),
            );
            let quotes = record(
                "quotes",
                "cg.connector-quotes",
                json!({"schema_version":"1.0","tariff":{"kind":"FIXED_LOCAL_NO_EMBEDDING","discovery_cost_microunits":0,"operation_cost_microunits":0},"manifest":manifest,"mapping":mapping,"cost_unit":"credits","discovery":amounts,"operations":[{"mapping_reference":"mapping.records.source.v1","amounts":amounts}]}),
            );
            let action_projection = record(
                "action-projection",
                "cg.context-projection",
                json!({"schema_version":1,"id":"action-execution-context","step":"connector-step","task":{"id":"task-inspect"},"basis":{"scope":scope}}),
            );
            let operations = record(
                "operations",
                "cg.connector-operations",
                json!({"schema_version":"1.0","operations":[{"mapping_reference":"mapping.records.source.v1","arguments":{}}]}),
            );
            let provider = record(
                "universe",
                "cg.connector-state-universe",
                json!({"schema_version":"1.0","scope":scope,"source_contracts":[{"source":"records.one","mapping_reference":"mapping.records.source.v1","export_revision":"registered-full-project-export-v1","subjects":["records.content"]}]}),
            );
            let read = json!({"mapping_reference":"mapping.records.source.v1","capability":"records.resource.read","source":"records.one","arguments":{},"normalization":normalization});
            let goal = pin(
                config["intent"]["id"].as_str().unwrap(),
                "cg.intent",
                scope,
                &serde_json::to_value(
                    serde_json::from_value::<Intent>(config["intent"].clone()).unwrap(),
                )
                .unwrap(),
            );
            let verifier = record(
                "verifier",
                "cg.connector-verifier",
                json!({"schema_version":"1.0","mode":"RETAINED_OBSERVATIONS","scope":scope,"goal":goal,"max_iterations":2,"max_retries":1}),
            );
            let resources = SessionResourcePlan {
                preparation: amounts,
                execution: amounts,
                after_snapshot_recovery: amounts,
                ..Default::default()
            };
            config["connector_runtime"] = json!({
                "schema_version":"1.0","id":"operator-runtime-records","revision":"operator-runtime-registration-4","policy":policy,"scope":scope,"plan":config["connector_goal"]["plan"],"goal":goal,"execution":config["execution"],
                "connector":"example.records","manifest":manifest,"mapping":mapping,
                "stdio":{"executable":"/usr/bin/python3","arguments":["-m","existing_mcp_server"],"directory":root,"environment":{}},
                "compatibility":{"gateway":{"major":0,"minor":1,"patch":0},"transports":["stdio"]},
                "limits":{"frame_bytes":1048576,"inventory_bytes":4194304,"items":1024,"pages":32,"events_per_request":128,"call_timeout_ms":30000,"shutdown_ms":100},
                "credential_store":root.join("operator-profiles.json"),"access_profile":"records.local",
                "documents":catalog,"steps":[{"step":"connector-step","action_projection":action_projection,"resources":resources,"quotes":quotes,"normalizations":{"mapping.records.read.v1":tool_normalization,"mapping.records.source.v1":normalization},"producer":{"kind":"STRUCTURED","operations":operations},"post_effect_reads":[]}],
                "snapshots":{"kind":"ATOMIC_COMPLETE_READ","state_universe":["records.one"],"provider_contract":provider,"read":read},
                "goal_reads":[{"condition":config["connector_goal"]["observations"][0]["condition"],"read":read}],"verifier":verifier
            });
            Self { wire, root }
        }
        pub(in crate::local_sessions) fn runtime(&mut self) -> &mut Value {
            &mut self.wire["mappings"][0]["sessions"]["connector_runtime"]
        }
        pub(in crate::local_sessions) fn host(&self) -> Result<LocalCodexHost, FacadeError> {
            let resolver = LocalWorkspaceResolver::from_json(&self.wire.to_string())?;
            let path = self.root.to_string_lossy().into_owned();
            let binding = resolver.resolve(&WorkspaceReference {
                working_directory: path.clone(),
                repository: path,
            })?;
            resolver.host(&binding)
        }
        fn load(&self) -> Result<LoadedRuntime, SessionError> {
            let host = self.host().map_err(error)?;
            let config = host.sessions.as_ref().ok_or(SessionError::Unavailable)?;
            config
                .connector_runtime
                .as_ref()
                .ok_or(SessionError::Unavailable)?
                .load(&host)
        }
        pub(in crate::local_sessions) fn rewrite(&mut self, id: &str, value: Value) {
            let path = self.runtime()["documents"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["pin"]["id"] == id)
                .unwrap()["path"]
                .as_str()
                .unwrap()
                .to_owned();
            std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
            let digest = json!(gateway_domain::context_content_digest(
                &serde_json::to_vec(&value).unwrap()
            ));
            fn update(value: &mut Value, id: &str, digest: &Value) {
                match value {
                    Value::Object(object) => {
                        if object.get("id").and_then(Value::as_str) == Some(id)
                            && object.contains_key("contract")
                        {
                            object.insert("digest".into(), digest.clone());
                        }
                        for value in object.values_mut() {
                            update(value, id, digest);
                        }
                    }
                    Value::Array(array) => {
                        for value in array {
                            update(value, id, digest);
                        }
                    }
                    _ => {}
                }
            }
            update(self.runtime(), id, &digest);
        }
        fn add(&mut self, id: &str, contract: &str, value: Value) -> Value {
            let scope = self.runtime()["scope"].as_str().unwrap().to_owned();
            let reference = pin(id, contract, &scope, &value);
            let path = self.root.join(format!("{id}.json"));
            std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            self.runtime()["documents"]
                .as_array_mut()
                .unwrap()
                .push(json!({"pin":reference,"path":path}));
            reference
        }
        pub(in crate::local_sessions) fn install_actual_native_model(
            &mut self,
        ) -> crate::declarative_cli::host_mapping::native_producer_fixture::NativeProducerFixture
        {
            self.install_native_model(false)
        }
        pub(in crate::local_sessions) fn install_actual_mixed_model(
            &mut self,
        ) -> crate::declarative_cli::host_mapping::native_producer_fixture::NativeProducerFixture
        {
            self.install_native_model(true)
        }
        fn install_native_model(
            &mut self,
            mixed: bool,
        ) -> crate::declarative_cli::host_mapping::native_producer_fixture::NativeProducerFixture
        {
            use crate::declarative_cli::host_mapping::native_producer_fixture::{
                real_mixed_plan, real_plan, two_condition_intent,
            };
            let scope = ContextScopeId::new(
                self.wire["mappings"][0]["canonical_scope"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            let intent = two_condition_intent();
            let native = if mixed {
                real_mixed_plan(&self.root, &scope, &intent)
            } else {
                real_plan(&self.root, &scope, &intent)
            }
            .unwrap();
            let wire_scope = self.wire["mappings"][0]["scope"].clone();
            let mut canonical = json!({"catalog":native.catalog,"policy":native.policy});
            let mut plan_reference = Value::Null;
            for (id, field, contract, document) in [
                ("native-plan", "plan", "cg.plan", native.plan.clone()),
                (
                    "native-rules",
                    "rules",
                    "cg.composition-rules",
                    native.rules.clone(),
                ),
                (
                    "native-process",
                    "process",
                    "cg.process-snapshot",
                    native.process.clone(),
                ),
            ] {
                let reference = json!({"id":id,"contract":contract,"contract_version":"1.0","revision":"operator-native-1","digest":format!("sha256:{}",gateway_domain::context_content_digest(&serde_json::to_vec(&document).unwrap()).as_str())});
                self.wire["mappings"][0]["resources"].as_array_mut().unwrap().push(json!({"schema_version":"1.0","scope":wire_scope,"reference":reference,"document":document,"provenance":[{"reference":reference,"source_id":id,"source_revision":"operator-native-1","freshness":"current","sensitivity":"NORMAL","lineage":[]}]}));
                canonical[field] = reference.clone();
                if field == "plan" {
                    plan_reference = json!({"id":id,"revision":reference["revision"],"digest":reference["digest"]});
                }
            }
            self.wire["mappings"][0]["canonical"] = canonical;
            self.rewrite("canonical-policy", native.policy.clone());
            let execution = json!({"mode":native.resolved.snapshot.input().operating_mode.to_string(),"profile":native.resolved.snapshot.input().execution_profile.to_string()});
            self.wire["mappings"][0]["sessions"]["intent"] = serde_json::to_value(&intent).unwrap();
            self.wire["mappings"][0]["sessions"]["execution"] = execution.clone();
            self.wire["mappings"][0]["sessions"]["connector_goal"]["plan"] = plan_reference.clone();
            self.wire["mappings"][0]["sessions"]["connector_goal"]["steps"] =
                json!([native.action_step.as_str()]);
            let capability = native
                .capabilities
                .iter()
                .find(|(_, definition)| definition.class() == CapabilityClass::Inspect)
                .unwrap()
                .0
                .as_str()
                .to_owned();
            assert_eq!(native.capabilities.len(), if mixed { 2 } else { 1 });
            self.wire["mappings"][0]["sessions"]["connector_goal"]["observations"] = json!(intent.desired_state().conditions().iter().map(|condition|json!({"condition":condition.id().as_str(),"source":"records.one","capability":capability,"operation_class":"read"})).collect::<Vec<_>>());
            self.runtime()["plan"] = plan_reference;
            self.runtime()["execution"] = execution;
            self.runtime()["steps"][0]["step"] = json!(native.action_step.as_str());
            self.runtime()["goal"] = pin(
                intent.id().as_str(),
                "cg.intent",
                scope.as_str(),
                &serde_json::to_value(&intent).unwrap(),
            );
            let goal = self.runtime()["goal"].clone();
            self.rewrite("verifier",json!({"schema_version":"1.0","mode":"RETAINED_OBSERVATIONS","scope":scope,"goal":goal,"max_iterations":2,"max_retries":1}));
            self.install_model_documents_with(|_, task| {
                task["id"] = json!(native.task.as_str());
                if mixed {
                    task["task_type"] = json!("MODIFY");
                }
            });
            self.rewrite("projection", native.producer_projection.clone());
            self.rewrite("action-projection", native.action_projection.clone());
            self.runtime()["steps"][0]["producer"]["producer_step"] =
                json!(native.producer_step.as_str());
            let mut aliases = Vec::new();
            self.runtime()["documents"]
                .as_array_mut()
                .unwrap()
                .retain(|d| d["pin"]["id"] != "records.resource.read");
            for (id, definition) in &native.capabilities {
                let mut alias = self.add(
                    id.as_str(),
                    "cg.connector-capability",
                    serde_json::to_value(definition).unwrap(),
                );
                alias["revision"] = json!("operator-capability-registration-7");
                self.runtime()["documents"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|doc| doc["pin"]["id"] == id.as_str())
                    .unwrap()["pin"] = alias.clone();
                aliases.push(alias);
            }
            let model = self.runtime()["steps"][0]["producer"].clone();
            let action_projection = self.runtime()["steps"][0]["action_projection"].clone();
            let plan = self.runtime()["plan"].clone();
            self.rewrite("join",json!({"schema_version":"1.0","plan":plan,"producer_step":native.producer_step.as_str(),"action_step":native.action_step.as_str(),"task":model["task"],"projection":model["projection"],"action_projection":action_projection,"capabilities":aliases}));
            let mut mapping: Value = serde_json::from_str(include_str!(
                "../../../../tests/fixtures/connector-mapping-v1/mapping.json"
            ))
            .unwrap();
            mapping["records"].as_array_mut().unwrap().truncate(1);
            mapping["records"][0]["capability"] = json!(capability);
            mapping["records"][0]["source"] = json!("records.one");
            mapping["records"][0]["result_schema"] = json!({"type":"object","properties":{"object":{"type":"string"},"id":{"type":"string"},"first":{"type":"boolean"},"second":{"type":"boolean"}},"required":["object","id","first","second"],"additionalProperties":false});
            if mixed {
                let capability = native
                    .capabilities
                    .iter()
                    .find(|(_, definition)| definition.class() == CapabilityClass::Mutate)
                    .unwrap()
                    .0;
                let mut mutation = mapping["records"][0].clone();
                mutation["capability"] = json!(capability);
                mutation["class"] = json!("MUTATE");
                mutation["side_effects"] = json!("mutation");
                mutation["idempotency"] = json!("non_idempotent");
                mutation["retry_safety"] = json!("never");
                mutation["consent"] = json!("required");
                mutation["source"] = json!("records.mutation");
                mutation["name"] = json!("records.update");
                mutation["reference"] = json!("mapping.records.update.v1");
                mapping["records"].as_array_mut().unwrap().push(mutation);
            }
            self.rewrite("mapping", mapping);
            let mut manifest: Value = serde_json::from_str(include_str!(
                "../../../../tests/fixtures/connector-mapping-v1/manifest.json"
            ))
            .unwrap();
            manifest["manifest"]["inventory"]
                .as_array_mut()
                .unwrap()
                .retain(|entry| entry["kind"] == "tool");
            manifest["manifest"]["inventory"][0]["source_mapping_ref"] =
                json!("mapping.records.read.v1");
            if mixed {
                let mut wire: Value = serde_json::from_str(include_str!(
                    "../../../../tests/fixtures/connector-mapping-v1/discovery.json"
                ))
                .unwrap();
                wire["tools"][0]["name"] = json!("records.update");
                let mut entry = manifest["manifest"]["inventory"][0].clone();
                entry["name"] = json!("records.update");
                entry["capability_mapping_ref"] = json!("mapping.records.update.v1");
                entry["source_mapping_ref"] = json!("mapping.records.update.v1");
                entry["schema_digest"] =
                    json!(crate::mcp_client::mapping::discovery_schema_digest(
                        InventoryKind::Tool,
                        &wire["tools"][0]
                    ));
                manifest["manifest"]["inventory"]
                    .as_array_mut()
                    .unwrap()
                    .push(entry);
            }
            let manifest = ConnectorManifest::from_definition_json(
                &serde_json::to_vec(&manifest["manifest"]).unwrap(),
            )
            .unwrap();
            self.rewrite(
                "manifest",
                serde_json::from_slice(&manifest.canonical_json()).unwrap(),
            );
            self.rewrite("normalization",json!({"schema_version":"1.0","acquisition":{"source_kind":"TOOL","sensitivity":"NORMAL","authenticated_profile":"records.local","semantics":{"kind":"DIRECT_CURRENT_READ_PROFILE","max_age_seconds":60,"time_basis":"REQUEST_START_LOWER_BOUND"}},"source":"records.one","object_field":"object","revision_field":null,"digest_field":null,"fields":{"first":"fixture.first","second":"fixture.second"}}));
            self.rewrite("universe",json!({"schema_version":"1.0","scope":scope,"source_contracts":[{"source":"records.one","mapping_reference":"mapping.records.read.v1","export_revision":"operator-native-full-export-1","subjects":["fixture.first","fixture.second"]}]}));
            let read = json!({"mapping_reference":"mapping.records.read.v1","capability":capability,"source":"records.one","arguments":{"query":{"kind":"STRING","value":"native-fixture"}},"normalization":self.runtime()["snapshots"]["read"]["normalization"]});
            self.runtime()["snapshots"]["read"] = read.clone();
            self.runtime()["goal_reads"] = json!(
                intent
                    .desired_state()
                    .conditions()
                    .iter()
                    .map(|condition| json!({"condition":condition.id().as_str(),"read":read}))
                    .collect::<Vec<_>>()
            );
            let quote = SessionResourceAmounts {
                connector_calls: 1,
                retrieval_rounds: 1,
                retrieval_results: 2,
                retrieval_tokens: 100,
                ..Default::default()
            };
            let mapping = self.runtime()["mapping"].clone();
            let manifest = self.runtime()["manifest"].clone();
            let mut operations =
                vec![json!({"mapping_reference":"mapping.records.read.v1","amounts":quote})];
            if mixed {
                operations
                    .push(json!({"mapping_reference":"mapping.records.update.v1","amounts":quote}));
            }
            self.rewrite("quotes",json!({"schema_version":"1.0","tariff":{"kind":"FIXED_LOCAL_NO_EMBEDDING","discovery_cost_microunits":0,"operation_cost_microunits":0},"manifest":manifest,"mapping":mapping,"cost_unit":"credits","discovery":quote,"operations":operations}));
            let resources = SessionResourcePlan {
                preparation: SessionResourceAmounts {
                    model_attempts: 1,
                    input_tokens: 4096,
                    output_tokens: 512,
                    output_bytes: 8192,
                    cost_microunits: 1,
                    ..quote
                },
                execution: quote,
                after_snapshot_recovery: quote,
                ..Default::default()
            };
            self.runtime()["steps"][0]["resources"] = serde_json::to_value(&resources).unwrap();
            self.wire["mappings"][0]["sessions"]["resource_limits"] =
                serde_json::to_value(SessionResourceLimits {
                    cost_unit: ReferenceId::new("credits").unwrap(),
                    amounts: resources.total().unwrap(),
                })
                .unwrap();
            let read_normalization = self.runtime()["snapshots"]["read"]["normalization"].clone();
            self.runtime()["steps"][0]["normalizations"] =
                json!({"mapping.records.read.v1":read_normalization});
            self.runtime()["documents"]
                .as_array_mut()
                .unwrap()
                .retain(|document| document["pin"]["id"] != "tool-normalization");
            if mixed {
                let profile=self.add("mutation-normalization","cg.connector-normalization",json!({"schema_version":"1.0","source":"records.mutation","object_field":"object","revision_field":null,"digest_field":null,"fields":{"first":"fixture.first","second":"fixture.second"},"acquisition":{"source_kind":"TOOL","sensitivity":"NORMAL","authenticated_profile":"records.local","semantics":{"kind":"PROVIDER_ACKNOWLEDGEMENT_ONLY"}}}));
                self.runtime()["steps"][0]["normalizations"]["mapping.records.update.v1"] = profile;
            }
            self.refresh_association();
            native
        }
        fn install_model_documents(&mut self) -> Value {
            self.install_model_documents_with(|_, _| {})
        }
        fn install_model_documents_with(
            &mut self,
            enrich: impl FnOnce(&mut Self, &mut Value),
        ) -> Value {
            let mut task: Value = serde_json::from_str(include_str!(
                "../../../../tests/fixtures/semantic-task-v1/minimal.json"
            ))
            .unwrap();
            let scope = self.runtime()["scope"].clone();
            let target = self.add(
                "artifact-1",
                "cg.connector-object",
                json!({"schema_version":"1.0","id":"artifact-1","revision":"1","scope":scope,"kind":"ARTIFACT","connector":"example.records","parent_target":null,"subjects":self.wire["mappings"][0]["sessions"]["intent"]["desired_state"]["conditions"].as_array().unwrap().iter().map(|condition|condition["subject"].clone()).collect::<Vec<_>>(),"coordinates":{"id":{"kind":"STRING","value":"native-fixture"}},"object_reference":"native-fixture"}),
            );
            let schema = self.add(
                "ArtifactInspection",
                "cg.output-schema",
                json!({"type":"string"}),
            );
            task["target"]["reference"] = target;
            task["output_contract"]["schema"] = schema.clone();
            task["desired_state"] =
                self.wire["mappings"][0]["sessions"]["intent"]["desired_state"].clone();
            enrich(self, &mut task);
            let task = serde_json::to_value(
                serde_json::from_value::<gateway_domain::SemanticTaskIR>(task).unwrap(),
            )
            .unwrap();
            let task_pin = self.add(
                task["id"].as_str().unwrap(),
                "cg.semantic-task",
                task.clone(),
            );
            let projection = self.add("projection","cg.context-projection",json!({"schema_version":1,"step":"producer-step","task":{"id":"task-inspect"},"basis":{"scope":"runtime-sidecar-scope"}}));
            let analysis = self.add(
                "analysis",
                "cg.context-analysis",
                json!({"schema_version":"1.0"}),
            );
            let output = self.add(
                "output",
                "cg.output-contract",
                json!({"schema_version":"1.0","schema":schema,"shape":{"type":"STRING"}}),
            );
            let rules = task["verification_contract"]["checks"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, check)| {
                    let rule = self.add(
                        &format!("rule-{i}"),
                        "cg.verification-rule",
                        json!({"schema_version":"1.0","check":check}),
                    );
                    let verifier = self.add(
                        &format!("verifier-{i}"),
                        "cg.verifier",
                        json!({"schema_version":"1.0","check":check}),
                    );
                    json!({"check":check,"rule":rule,"verifier":verifier})
                })
                .collect::<Vec<_>>();
            let verification = self.add(
                "verification",
                "cg.verification-contract",
                json!({"schema_version":"1.0","rules":rules}),
            );
            let model = self.add(
                "model",
                "cg.connector-model",
                json!({"schema_version":"1.0"}),
            );
            let plan = self.runtime()["plan"].clone();
            let definition = gateway_domain::CapabilityDefinition::new(
                CapabilityId::new("records.resource.read").unwrap(),
                CapabilityClass::Inspect,
            );
            let capability = self.add(
                "records.resource.read",
                "cg.connector-capability",
                serde_json::to_value(definition).unwrap(),
            );
            let action_projection = self.runtime()["steps"][0]["action_projection"].clone();
            let join = self.add("join","cg.connector-producer-join",json!({"schema_version":"1.0","plan":plan,"producer_step":"producer-step","action_step":"connector-step","task":task_pin,"projection":projection,"action_projection":action_projection,"capabilities":[capability]}));
            self.runtime()["documents"]
                .as_array_mut()
                .unwrap()
                .retain(|d| d["pin"]["id"] != "operations");
            self.runtime()["steps"][0]["producer"] = json!({"kind":"MODEL","producer_step":"producer-step","association":null,"join":join,"task":task_pin,"projection":projection,"context_analysis":analysis,"fragments":[],"source_metadata":[],"output":output,"verification":verification,"model":model});
            self.refresh_association();
            task
        }
        fn document_value(&mut self, id: &str) -> Value {
            let path = self.runtime()["documents"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["pin"]["id"] == id)
                .unwrap()["path"]
                .as_str()
                .unwrap()
                .to_owned();
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
        }
        pub(in crate::local_sessions) fn refresh_association(&mut self) {
            let model = self.runtime()["steps"][0]["producer"].clone();
            let task = self.document_value(model["task"]["id"].as_str().unwrap());
            let mapping_pin = self.runtime()["mapping"].clone();
            let mapping = self.document_value(mapping_pin["id"].as_str().unwrap());
            let record = if mapping["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|record| record["kind"] == "resource")
            {
                mapping["records"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|record| record["kind"] == "resource")
                    .unwrap()
                    .clone()
            } else {
                mapping["records"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|record| record["class"] == "READ")
                    .unwrap()
                    .clone()
            };
            let resource = record["kind"] == "resource";
            // Scope export is a separate host-only association, with its exact
            // registered universe pin and parent TaskTarget. No model grant.
            let mut object =
                self.document_value(task["target"]["reference"]["id"].as_str().unwrap());
            object["object_reference"] = if resource {
                json!("records://one")
            } else {
                json!("native-fixture")
            };
            self.rewrite(
                task["target"]["reference"]["id"].as_str().unwrap(),
                object.clone(),
            );
            let object_pin = self.runtime()["documents"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["pin"]["id"] == task["target"]["reference"]["id"])
                .unwrap()["pin"]
                .clone();
            let mut task = task;
            task["target"]["reference"] = object_pin.clone();
            self.rewrite(model["task"]["id"].as_str().unwrap(), task);
            let task_pin = self.runtime()["steps"][0]["producer"]["task"].clone();
            let mut join = self.document_value(model["join"]["id"].as_str().unwrap());
            join["task"] = task_pin.clone();
            self.rewrite(model["join"]["id"].as_str().unwrap(), join);
            object["id"] = json!("export-object");
            object["kind"] = json!("PROJECT");
            object["parent_target"] = object_pin.clone();
            let export_pin = if self.runtime()["documents"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["pin"]["id"] == "export-object")
            {
                self.rewrite("export-object", object);
                self.runtime()["documents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|d| d["pin"]["id"] == "export-object")
                    .unwrap()["pin"]
                    .clone()
            } else {
                self.add("export-object", "cg.connector-object", object)
            };
            let provider = self.runtime()["snapshots"]["provider_contract"].clone();
            let operation = |reference: &Value, purpose: &str, subject: Value, target: Value| json!({"mapping_reference":reference,"purpose":purpose,"profile":"EXACT_TASK_OPERANDS_V1","subject":subject,"object":target,"selector_arguments":if resource {json!({})} else {json!({"query":"id"})},"literal_arguments":{},"fixed_arguments":{},"proposal_data":[],"response":{"object_field":"object","identity_fields":if resource {json!({})} else {json!({"id":"id"})}}});
            let mut operations = vec![operation(
                &record["reference"],
                "FIXED_READ",
                json!({"kind":"SCOPE_EXPORT","provider_contract":provider}),
                export_pin,
            )];
            if !resource {
                for record in mapping["records"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|record| record["kind"] == "tool")
                {
                    operations.push(operation(
                        &record["reference"],
                        "PROPOSAL",
                        json!({"kind":"TASK_TARGET"}),
                        object_pin.clone(),
                    ));
                }
            }
            let association = json!({"schema_version":"1.0","scope":self.runtime()["scope"],"task":task_pin,"action_join":self.runtime()["steps"][0]["producer"]["join"],"action_projection":self.runtime()["steps"][0]["action_projection"],"read_projection":self.runtime()["steps"][0]["producer"]["projection"],"mapping":mapping_pin,"action_step":self.runtime()["steps"][0]["step"],"connector":self.runtime()["connector"],"operations":operations});
            let association = if self.runtime()["documents"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["pin"]["id"] == "association")
            {
                self.rewrite("association", association);
                self.runtime()["documents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|d| d["pin"]["id"] == "association")
                    .unwrap()["pin"]
                    .clone()
            } else {
                self.add(
                    "association",
                    "cg.connector-selector-association",
                    association,
                )
            };
            self.runtime()["steps"][0]["producer"]["association"] = association;
        }
    }
    #[test]
    fn actual_native_model_data_loads_current_plan_policy_roles_and_capability_alias() {
        let mut f = Fixture::new();
        let native = f.install_actual_native_model();
        let loaded = f
            .load()
            .expect("actual canonical native model data must load");
        assert_eq!(loaded.resolved().unwrap(), &native.resolved);
        let host = f.host().unwrap();
        host.sessions
            .as_ref()
            .unwrap()
            .connector_runtime
            .as_ref()
            .unwrap()
            .require_model_producer()
            .unwrap();
        assert_eq!(
            host.resources.len(),
            3,
            "only existing canonical resources enter v1; runtime sidecars stay private"
        );
        assert_eq!(loaded.mapping().records().len(), 1);
        assert_eq!(
            loaded.mapping().records()[0].capability,
            *native.capabilities.keys().next().unwrap()
        );
        assert_eq!(native.resolved.snapshot.input().plan.steps().len(), 2);
        assert_ne!(native.producer_step, native.action_step);
        assert_eq!(
            host.sessions.as_ref().unwrap().intent,
            crate::declarative_cli::host_mapping::native_producer_fixture::two_condition_intent()
        );
        let model = f.runtime()["steps"][0]["producer"].clone();
        let task: gateway_domain::SemanticTaskIR = serde_json::from_value(
            loaded
                .document(&serde_json::from_value(model["task"].clone()).unwrap())
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert_eq!(task.data().id, native.task);
        let association_pin: Pin = serde_json::from_value(model["association"].clone()).unwrap();
        let association = super::super::selector_association::SelectorAssociation::from_json(
            &association_pin,
            &serde_json::to_vec(loaded.document(&association_pin).unwrap()).unwrap(),
        )
        .unwrap();
        let task_pin: Pin = serde_json::from_value(model["task"].clone()).unwrap();
        assert_eq!(task_pin.id.as_str(), task.data().id.as_str());
        assert_eq!(task_pin.digest, task.content_digest().unwrap());
        assert!(association.reference_bindings().contains(&task_pin));
        assert!(
            association
                .reference_bindings()
                .contains(&serde_json::from_value(model["join"].clone()).unwrap())
        );
        let target = &task.data().target.reference;
        assert_eq!(target.contract.as_str(), "cg.connector-object");
        let target_value = loaded.document(target).unwrap();
        assert_eq!(target_value["id"], target.id.as_str());
        assert_eq!(target_value["revision"], target.revision.as_str());
        assert_eq!(
            target_value["kind"],
            serde_json::to_value(task.data().target.kind).unwrap()
        );
        let desired_subjects: BTreeSet<_> = task
            .data()
            .desired_state
            .as_ref()
            .unwrap()
            .conditions()
            .iter()
            .map(|condition| condition.subject().to_string())
            .collect();
        assert_eq!(
            target_value["subjects"]
                .as_array()
                .unwrap()
                .iter()
                .map(|subject| subject.as_str().unwrap().to_owned())
                .collect::<BTreeSet<_>>(),
            desired_subjects
        );
        let runtime = host
            .sessions
            .as_ref()
            .unwrap()
            .connector_runtime
            .as_ref()
            .unwrap();
        assert_eq!(
            loaded.document(runtime.policy_reference()).unwrap(),
            &native.policy
        );

        assert_eq!(
            task.data().desired_state.as_ref(),
            Some(host.sessions.as_ref().unwrap().intent.desired_state())
        );
        for (id, definition) in &native.capabilities {
            let alias: Pin = serde_json::from_value(
                f.runtime()["documents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|document| document["pin"]["id"] == id.as_str())
                    .unwrap()["pin"]
                    .clone(),
            )
            .unwrap();
            assert_eq!(alias.contract.as_str(), "cg.connector-capability");
            assert_eq!(
                alias.revision.as_str(),
                "operator-capability-registration-7"
            );
            assert_eq!(
                loaded.document(&alias).unwrap(),
                &serde_json::to_value(definition).unwrap()
            );
        }
        assert_eq!(f.runtime()["goal_reads"].as_array().unwrap().len(), 2);
        assert_eq!(
            f.runtime()["snapshots"]["read"]["capability"],
            native.capabilities.keys().next().unwrap().as_str()
        );
        // Data-load/actual native compilation evidence only: no model prepared
        // invocation, provider availability or connector execution is fabricated.
    }

    #[test]
    fn operator_runtime_reference_is_exact_body_identity_and_policy_is_current() {
        let mut f = Fixture::new();
        let host = f.host().unwrap();
        let runtime = host
            .sessions
            .as_ref()
            .unwrap()
            .connector_runtime
            .as_ref()
            .unwrap();
        let original = runtime.reference().unwrap();
        assert_eq!(original.id.as_str(), "operator-runtime-records");
        assert_eq!(
            original.revision.as_str(),
            "operator-runtime-registration-4"
        );
        assert_eq!(original.contract.as_str(), "cg.connector-runtime");
        assert_eq!(original.scope, host.binding.canonical_scope);
        assert_eq!(
            original.digest,
            gateway_domain::context_content_digest(
                &serde_json::to_vec(&serde_json::to_value(&runtime.0).unwrap()).unwrap()
            )
        );
        let mut retained = runtime.clone();
        retained.retain(runtime.load(&host).unwrap());
        assert_eq!(
            retained.reference().unwrap(),
            original,
            "owned loaded cache is not runtime wire identity"
        );
        f.runtime()["revision"] = json!("operator-runtime-registration-5");
        let next = f.host().unwrap();
        let revised = next
            .sessions
            .as_ref()
            .unwrap()
            .connector_runtime
            .as_ref()
            .unwrap()
            .reference()
            .unwrap();
        assert_ne!(revised, original);
        assert_ne!(revised.digest, original.digest);
        let mut f = Fixture::new();
        f.install_actual_native_model();
        assert!(f.load().is_ok());
        f.rewrite("canonical-policy", json!({"unrelated":"policy"}));
        assert_eq!(f.load().err(), Some(SessionError::StaleRevision));
        let mut f = Fixture::new();
        f.install_model_documents();
        f.runtime()["steps"][0]["producer"]
            .as_object_mut()
            .unwrap()
            .remove("association");
        assert!(f.host().is_err(), "MODEL association is mandatory");
    }
    #[test]
    fn actual_native_model_data_denies_alias_contract_revision_definition_and_current_basis_drift()
    {
        for case in [
            "alias-contract",
            "alias-revision",
            "current-definition",
            "canonical-policy",
            "native-producer-step",
        ] {
            let mut f = Fixture::new();
            let native = f.install_actual_native_model();
            assert!(
                f.load().is_ok(),
                "{case} starts with a truly loadable native basis"
            );
            let join_path = f.runtime()["documents"]
                .as_array()
                .unwrap()
                .iter()
                .find(|document| document["pin"]["id"] == "join")
                .unwrap()["path"]
                .as_str()
                .unwrap()
                .to_owned();
            let mut join: Value =
                serde_json::from_slice(&std::fs::read(join_path).unwrap()).unwrap();
            match case {
                "alias-contract" => {
                    join["capabilities"][0]["contract"] = json!("cg.unrelated-object")
                }
                "alias-revision" => {
                    join["capabilities"][0]["revision"] = json!("unregistered-revision")
                }
                "current-definition" => {
                    let id = native.capabilities.keys().next().unwrap().as_str();
                    let mut definition =
                        serde_json::to_value(native.capabilities.values().next().unwrap()).unwrap();
                    definition["class"] = json!("MUTATE");
                    f.rewrite(id, definition);
                    join["capabilities"][0] = f.runtime()["documents"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|document| document["pin"]["id"] == id)
                        .unwrap()["pin"]
                        .clone();
                }
                "canonical-policy" => {
                    f.wire["mappings"][0]["canonical"]["policy"]["basis"]["registry_fingerprint"] =
                        json!("f".repeat(64))
                }
                _ => {
                    let mut projection = native.producer_projection.clone();
                    projection["step"] = json!("foreign-step");
                    f.rewrite("projection", projection);
                    f.runtime()["steps"][0]["producer"]["producer_step"] = json!("foreign-step");
                    join["producer_step"] = json!("foreign-step");
                    join["projection"] = f.runtime()["steps"][0]["producer"]["projection"].clone();
                }
            }
            f.rewrite("join", join);
            f.refresh_association();
            assert_eq!(
                f.load().err(),
                Some(if case == "native-producer-step" {
                    SessionError::InvalidInput
                } else {
                    SessionError::StaleRevision
                }),
                "{case} cannot reuse actual current native admission"
            );
        }
    }
    #[test]
    fn resolver_loads_private_sidecars_and_retains_the_verified_document_snapshot() {
        let mut f = Fixture::new();
        let loaded = f.load().unwrap();
        assert_eq!(loaded.manifest().connector_id(), "example.records");
        assert_eq!(loaded.mapping().records().len(), 2);
        assert!(loaded.stdio().executable.is_absolute());
        assert_eq!(loaded.limits().pages, 32);
        let host = f.host().unwrap();
        assert!(
            host.resources.is_empty(),
            "private runtime contracts never enter frozen v1 resources"
        );
        let runtime = host
            .sessions
            .as_ref()
            .unwrap()
            .connector_runtime
            .as_ref()
            .unwrap();
        assert_eq!(
            runtime.require_model_producer(),
            Err(SessionError::UnsupportedCapability)
        );
        let original = loaded.document(&runtime.0.verifier).unwrap().clone();
        let mut retained = runtime.clone();
        retained.retain(loaded);
        let mut changed = original.clone();
        changed["max_iterations"] = json!(1);
        f.rewrite("verifier", changed);
        assert_eq!(
            retained
                .loaded()
                .unwrap()
                .document(&runtime.0.verifier)
                .unwrap(),
            &original
        );
        assert!(f.load().is_ok());
        let path = f.runtime()["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|document| document["pin"]["id"] == "manifest")
            .unwrap()["path"]
            .as_str()
            .unwrap()
            .to_owned();
        std::fs::write(path, b"{}").unwrap();
        assert_eq!(f.load().err(), Some(SessionError::StaleRevision));
    }
    #[test]
    fn resolver_refuses_unknown_runtime_fields_versions_paths_and_inline_material() {
        for mutate in [
            ("unknown", json!(true)),
            ("schema_version", json!("2.0")),
            ("credential_store", json!("relative-file")),
            ("credentials", json!({"password":"not-a-secret"})),
        ] {
            let mut f = Fixture::new();
            f.runtime()[mutate.0] = mutate.1;
            assert!(f.host().is_err());
        }
        let mut f = Fixture::new();
        f.runtime()["stdio"]["environment"]["API_KEY"] = json!("not-a-secret");
        assert!(f.host().is_err());
        let mut f = Fixture::new();
        f.runtime()["stdio"]["unregistered"] = json!(true);
        assert!(f.host().is_err());
        let mut f = Fixture::new();
        f.runtime()["stdio"]["arguments"] = json!(["--password", "not-a-secret"]);
        assert!(f.host().is_err());
        let mut f = Fixture::new();
        let duplicate = f.runtime()["documents"][0].clone();
        f.runtime()["documents"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(f.host().is_err());
        let mut f = Fixture::new();
        let external_reads = f.runtime()["steps"][0]["resources"]["execution"].clone();
        f.runtime()["steps"][0]["resources"]["goal_verification"] = external_reads;
        assert!(
            f.host().is_err(),
            "pure goal verification cannot acquire an external allowance"
        );
    }
    #[test]
    fn model_producer_keeps_separate_preparation_join_and_requires_real_native_basis() {
        let mut f = Fixture::new();
        let mut task = f.install_model_documents();
        let host = f.host().unwrap();
        let admission = host
            .sessions
            .as_ref()
            .unwrap()
            .connector_runtime
            .as_ref()
            .unwrap();
        assert_eq!(admission.require_model_producer(), Ok(()));
        let documents = admission.load_documents(&host).unwrap();
        assert!(documents.contains_key(&ReferenceId::new("artifact-1").unwrap()));
        assert!(documents.contains_key(&ReferenceId::new("ArtifactInspection").unwrap()));
        assert!(documents.contains_key(&ReferenceId::new("rule-0").unwrap()));
        assert!(documents.contains_key(&ReferenceId::new("verifier-0").unwrap()));
        assert!(documents.contains_key(&ReferenceId::new("records.resource.read").unwrap()));
        // Complete document discovery is data, not target association or an
        // executable producer: real current canonical steps remain mandatory.
        assert_eq!(f.load().err(), Some(SessionError::UnsupportedCapability));
        task["desired_state"] = Value::Null;
        f.rewrite("task-inspect", task);
        assert_eq!(f.load().err(), Some(SessionError::StaleRevision));
    }
    fn discovered_documents(
        f: &Fixture,
    ) -> Result<BTreeMap<ReferenceId, PinnedDocument>, SessionError> {
        let host = f.host().map_err(error)?;
        host.sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?
            .connector_runtime
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?
            .load_documents(&host)
    }

    #[test]
    fn runtime_closure_uses_owning_selector_and_object_parsers() {
        let mut f = Fixture::new();
        f.install_model_documents();
        assert!(discovered_documents(&f).is_ok());
        let mut association = f.document_value("association");
        association["permit"] = json!(true);
        f.rewrite("association", association);
        assert_eq!(
            discovered_documents(&f).err(),
            Some(SessionError::InvalidInput)
        );
        let mut f = Fixture::new();
        f.install_model_documents();
        let mut object = f.document_value("artifact-1");
        object["permit"] = json!(true);
        f.rewrite("artifact-1", object);
        f.refresh_association();
        assert_eq!(
            discovered_documents(&f).err(),
            Some(SessionError::InvalidInput)
        );
        let mut f = Fixture::new();
        f.install_model_documents();
        let mut object = f.document_value("export-object");
        object["parent_target"]["revision"] = json!("foreign-parent-revision");
        f.rewrite("export-object", object);
        let object_pin = f.runtime()["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["pin"]["id"] == "export-object")
            .unwrap()["pin"]
            .clone();
        let mut association = f.document_value("association");
        association["operations"][0]["object"] = object_pin;
        f.rewrite("association", association);
        assert_eq!(
            discovered_documents(&f).err(),
            Some(SessionError::StaleRevision),
            "typed parent edge is followed exactly"
        );
        let mut f = Fixture::new();
        f.install_model_documents();
        f.runtime()["documents"]
            .as_array_mut()
            .unwrap()
            .retain(|d| d["pin"]["id"] != "export-object");
        assert_eq!(
            discovered_documents(&f).err(),
            Some(SessionError::Unavailable)
        );
    }
    #[test]
    fn typed_reference_closure_reaches_nested_tasks_shared_schema_and_real_pinned_leaves() {
        let mut f = Fixture::new();
        f.install_model_documents_with(|fixture,task| {
            let target=fixture.add("nested-artifact","cg.artifact",json!({"id":"nested-artifact","content":"retained nested target"}));
            let mut nested=task.clone();nested["id"]=json!("nested-task");nested["target"]["reference"]=target;
            let nested=fixture.add("nested-task","cg.semantic-task",nested);
            let knowledge=fixture.add("knowledge","cg.knowledge",json!({"id":"knowledge","content":"retained explicit premise"}));
            task["inputs"]=json!([{"role":"nested","value":{"kind":"REFERENCE","value":nested}}]);
            task["assumptions"]=json!([{"id":"premise","premise":{"kind":"BOOLEAN","value":true},"basis":knowledge,"confidence":{"kind":"UNKNOWN"}}]);
        });
        let documents = discovered_documents(&f).unwrap();
        for id in [
            "nested-task",
            "nested-artifact",
            "knowledge",
            "ArtifactInspection",
            "rule-0",
            "verifier-0",
            "records.resource.read",
        ] {
            assert!(documents.contains_key(&ReferenceId::new(id).unwrap()));
        }
        assert_eq!(
            documents.len(),
            f.runtime()["documents"].as_array().unwrap().len()
        );
        let original = documents[&ReferenceId::new("nested-artifact").unwrap()]
            .value
            .clone();
        std::fs::write(f.root.join("nested-artifact.json"), b"{}").unwrap();
        assert_eq!(
            documents[&ReferenceId::new("nested-artifact").unwrap()].value,
            original
        );
        assert_eq!(
            discovered_documents(&f).err(),
            Some(SessionError::StaleRevision)
        );
    }
    #[test]
    fn typed_reference_closure_requires_every_pin_field_and_does_not_dedup_by_id() {
        for field in [
            "id",
            "scope",
            "contract",
            "contract_version",
            "revision",
            "digest",
            "missing",
        ] {
            let mut f = Fixture::new();
            f.install_model_documents();
            if field == "missing" {
                f.runtime()["documents"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|d| d["pin"]["id"] != "artifact-1");
            } else {
                let document = f.runtime()["documents"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|d| d["pin"]["id"] == "artifact-1")
                    .unwrap();
                document["pin"][field] = match field {
                    "id" => json!("different-id"),
                    "scope" => json!("different-scope"),
                    "contract" => json!("cg.other-contract"),
                    "contract_version" => json!("2.0"),
                    "revision" => json!("changed-revision"),
                    "digest" => json!("f".repeat(64)),
                    _ => unreachable!(),
                };
            }
            assert!(discovered_documents(&f).is_err(), "{field}");
        }
        let mut f = Fixture::new();
        f.install_model_documents_with(|_, task| {
            let mut alternate = task["target"]["reference"].clone();
            alternate["revision"] = json!("another-revision");
            task["inputs"] =
                json!([{"role":"alternate","value":{"kind":"REFERENCE","value":alternate}}]);
        });
        assert_eq!(
            discovered_documents(&f).err(),
            Some(SessionError::StaleRevision)
        );
    }
    #[test]
    fn typed_reference_closure_rejects_unused_and_unbounded_graphs_without_json_guessing() {
        let mut f = Fixture::new();
        f.install_model_documents_with(|fixture,task| {
            let unused=fixture.add("unused-leaf","cg.artifact",json!({"id":"unused-leaf"}));
            let leaf=fixture.add("opaque-leaf","cg.knowledge",json!({"looks_like_a_pin":unused}));
            task["assumptions"]=json!([{"id":"opaque-premise","premise":{"kind":"BOOLEAN","value":true},"basis":leaf,"confidence":{"kind":"UNKNOWN"}}]);
        });
        assert_eq!(
            discovered_documents(&f).err(),
            Some(SessionError::InvalidInput)
        );
        let mut f = Fixture::new();
        f.install_model_documents_with(|_,task| {
            task["inputs"]=Value::Array((0..=DOCUMENTS).map(|i| {
                let mut reference=task["target"]["reference"].clone();reference["id"]=json!(format!("expansion-{i}"));
                json!({"role":format!("input-{i}"),"value":{"kind":"REFERENCE","value":reference}})
            }).collect());
        });
        assert_eq!(
            discovered_documents(&f).err(),
            Some(SessionError::LimitExceeded)
        );
    }
    #[test]
    fn sidecar_loader_refuses_non_regular_files_before_reading() {
        let mut f = Fixture::new();
        let directory = f.root.clone();
        f.runtime()["documents"][0]["path"] = json!(directory);
        assert_eq!(f.load().err(), Some(SessionError::Unavailable));
        #[cfg(unix)]
        {
            let fifo = f.root.join("sidecar.fifo");
            assert!(
                std::process::Command::new("mkfifo")
                    .arg(&fifo)
                    .status()
                    .unwrap()
                    .success()
            );
            f.runtime()["documents"][0]["path"] = json!(fifo);
            assert_eq!(f.load().err(), Some(SessionError::Unavailable));
        }
    }
    #[test]
    fn typed_runtime_binds_native_scope_goal_plan_steps_and_resource_budget() {
        for field in ["scope", "goal", "plan", "steps", "budget", "condition"] {
            let mut f = Fixture::new();
            match field {
                "scope" => f.wire["mappings"][0]["canonical_scope"] = json!("different-scope"),
                "goal" => f.runtime()["goal"]["digest"] = json!("f".repeat(64)),
                "plan" => f.runtime()["plan"]["revision"] = json!("different-revision"),
                "steps" => f.runtime()["steps"][0]["step"] = json!("different-step"),
                "budget" => {
                    f.wire["mappings"][0]["sessions"]["resource_limits"]["amounts"]["connector_calls"] =
                        json!(1)
                }
                "condition" => {
                    f.runtime()["goal_reads"][0]["condition"] = json!("different-condition")
                }
                _ => unreachable!(),
            }
            assert!(f.load().is_err(), "native binding {field} must be rejected");
        }
    }
    #[test]
    fn retained_verifier_document_cannot_select_other_modes_goals_or_unbounded_work() {
        for (field, value) in [
            ("mode", json!("EXECUTE")),
            ("scope", json!("foreign-scope")),
            ("max_iterations", json!(0)),
            ("max_iterations", json!(65)),
            ("max_retries", json!(2)),
            ("permit", json!(true)),
        ] {
            let mut f = Fixture::new();
            let host = f.host().unwrap();
            let config = host.sessions.as_ref().unwrap();
            let loaded = config
                .connector_runtime
                .as_ref()
                .unwrap()
                .load(&host)
                .unwrap();
            let mut document = loaded
                .document(&config.connector_runtime.as_ref().unwrap().0.verifier)
                .unwrap()
                .clone();
            document[field] = value;
            f.rewrite("verifier", document);
            assert!(
                f.load().is_err(),
                "verifier field {field} cannot widen authority"
            );
        }
        let mut f = Fixture::new();
        let host = f.host().unwrap();
        let config = host.sessions.as_ref().unwrap();
        let loaded = config
            .connector_runtime
            .as_ref()
            .unwrap()
            .load(&host)
            .unwrap();
        let mut document = loaded
            .document(&config.connector_runtime.as_ref().unwrap().0.verifier)
            .unwrap()
            .clone();
        document["goal"]["digest"] = json!("f".repeat(64));
        f.rewrite("verifier", document);
        assert_eq!(f.load().err(), Some(SessionError::StaleRevision));
    }

    #[test]
    fn versioned_snapshot_requires_mandatory_structured_revision_and_declared_universe() {
        let mut f = Fixture::new();
        let mut mapping: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/connector-mapping-v1/mapping.json"
        ))
        .unwrap();
        mapping["records"][0]["source"] = json!("records.one");
        mapping["records"].as_array_mut().unwrap().truncate(1);
        mapping["records"][0]["result_schema"] = json!({"type":"object","properties":{"object":{"type":"string"},"revision":{"type":"string"},"value":{"type":"string"}},"required":["object","revision","value"],"additionalProperties":false});
        let mut manifest: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/connector-mapping-v1/manifest.json"
        ))
        .unwrap();
        manifest["manifest"]["inventory"]
            .as_array_mut()
            .unwrap()
            .retain(|entry| entry["kind"] == "tool");
        let manifest = ConnectorManifest::from_definition_json(
            &serde_json::to_vec(&manifest["manifest"]).unwrap(),
        )
        .unwrap();
        f.rewrite(
            "manifest",
            serde_json::from_slice(&manifest.canonical_json()).unwrap(),
        );
        f.rewrite("mapping", mapping.clone());
        f.rewrite("normalization",json!({"schema_version":"1.0","acquisition":{"source_kind":"TOOL","sensitivity":"NORMAL","authenticated_profile":"records.local","semantics":{"kind":"DIRECT_CURRENT_READ_PROFILE","max_age_seconds":60,"time_basis":"REQUEST_START_LOWER_BOUND"}},"source":"records.one","object_field":"object","revision_field":"revision","digest_field":null,"fields":{"value":"records.content"}}));
        let quote = SessionResourceAmounts {
            connector_calls: 1,
            retrieval_rounds: 1,
            retrieval_results: 1,
            retrieval_tokens: 100,
            ..Default::default()
        };
        let quoted_manifest = f.runtime()["manifest"].clone();
        let quoted_mapping = f.runtime()["mapping"].clone();
        f.rewrite("quotes",json!({"schema_version":"1.0","tariff":{"kind":"FIXED_LOCAL_NO_EMBEDDING","discovery_cost_microunits":0,"operation_cost_microunits":0},"manifest":quoted_manifest,"mapping":quoted_mapping,"cost_unit":"credits","discovery":quote,"operations":[{"mapping_reference":"mapping.records.read.v1","amounts":quote}]}));
        f.rewrite("universe",json!({"schema_version":"1.0","scope":"runtime-sidecar-scope","source_contracts":[{"source":"records.one","mapping_reference":"mapping.records.read.v1","export_revision":"registered-full-project-export-v1","subjects":["records.content"]}]}));
        let mut read = f.runtime()["snapshots"]["read"].clone();
        read["mapping_reference"] = json!("mapping.records.read.v1");
        read["capability"] = json!("records.read");
        f.runtime()["goal_reads"][0]["read"] = read.clone();
        f.wire["mappings"][0]["sessions"]["connector_goal"]["observations"][0]["capability"] =
            json!("records.read");
        let provider = f.runtime()["snapshots"]["provider_contract"].clone();
        f.runtime()["snapshots"] = json!({"kind":"VERSIONED_COMPLETE_SET","state_universe":["records.one"],"provider_contract":provider,"version_field":"revision","consistency":"SAME_REVISION","reads":[read]});
        let normalization = f.runtime()["goal_reads"][0]["read"]["normalization"].clone();
        f.runtime()["steps"][0]["normalizations"] =
            json!({"mapping.records.read.v1":normalization});
        f.runtime()["documents"]
            .as_array_mut()
            .unwrap()
            .retain(|document| document["pin"]["id"] != "tool-normalization");
        assert_eq!(f.load().err(), None);
        // An optional schema field cannot carry a mandatory cross-source
        // revision contract even if normalization names that field.
        mapping["records"][0]["result_schema"]["required"] = json!(["object", "value"]);
        f.rewrite("mapping", mapping);
        let quoted_manifest = f.runtime()["manifest"].clone();
        let quoted_mapping = f.runtime()["mapping"].clone();
        f.rewrite("quotes",json!({"schema_version":"1.0","tariff":{"kind":"FIXED_LOCAL_NO_EMBEDDING","discovery_cost_microunits":0,"operation_cost_microunits":0},"manifest":quoted_manifest,"mapping":quoted_mapping,"cost_unit":"credits","discovery":quote,"operations":[{"mapping_reference":"mapping.records.read.v1","amounts":quote}]}));
        assert_eq!(f.load().err(), Some(SessionError::InvalidInput));
    }

    #[test]
    fn target_profiles_require_exact_current_source_authentication_semantics_and_tariff() {
        let cases = [
            (
                "/acquisition/authenticated_profile",
                json!("foreign-profile"),
            ),
            ("/acquisition/source_kind", json!("MODEL")),
            (
                "/acquisition/semantics/kind",
                json!("UNQUALIFIED_PROVIDER_NOW"),
            ),
            (
                "/acquisition/semantics/time_basis",
                json!("RESPONSE_HOST_NOW"),
            ),
            ("/acquisition/semantics/max_age_seconds", json!(0)),
            ("/source", json!("other-source")),
        ];
        for (path, invalid_value) in cases {
            let mut f = Fixture::new();
            let loaded = f.load().unwrap();
            let id = ReferenceId::new("normalization").unwrap();
            let mut profile = loaded.documents.get(&id).unwrap().value.clone();
            *profile.pointer_mut(path).unwrap() = invalid_value;
            f.rewrite("normalization", profile);
            assert!(
                f.load().is_err(),
                "unsupported acquisition contract: {path}"
            );
        }
        let mut f = Fixture::new();
        let loaded = f.load().unwrap();
        let mut quote = loaded
            .documents
            .get(&ReferenceId::new("quotes").unwrap())
            .unwrap()
            .value
            .clone();
        quote.as_object_mut().unwrap().remove("tariff");
        f.rewrite("quotes", quote);
        assert_eq!(
            f.load().err(),
            Some(SessionError::InvalidInput),
            "zero numeric quote alone is no fixed billing contract"
        );
        let mut f = Fixture::new();
        f.runtime()["steps"][0]["normalizations"]
            .as_object_mut()
            .unwrap()
            .remove("mapping.records.read.v1");
        assert!(
            f.load().is_err(),
            "every actual mapping requires its own qualified normalization pin"
        );
    }

    #[test]
    fn pinned_content_must_satisfy_manifest_mapping_quote_and_snapshot_contracts() {
        for id in ["manifest", "mapping", "quotes", "normalization"] {
            let mut f = Fixture::new();
            f.rewrite(id, json!({"schema_version":"1.0","unexpected_field":true}));
            assert_eq!(f.load().err(), Some(SessionError::InvalidInput));
        }
        let mut f = Fixture::new();
        let host = f.host().unwrap();
        let runtime = host
            .sessions
            .as_ref()
            .unwrap()
            .connector_runtime
            .as_ref()
            .unwrap();
        let (read, provider) = match &runtime.0.snapshots {
            SnapshotAdmission::AtomicCompleteRead {
                read,
                provider_contract,
                ..
            } => (read.clone(), provider_contract.clone()),
            _ => unreachable!(),
        };
        // Resource reads have no revision envelope: a versioned multi-source
        // snapshot cannot be asserted by simply assigning a version-field name.
        let original_read = f.runtime()["snapshots"]["read"].clone();
        f.runtime()["snapshots"] = json!({"kind":"VERSIONED_COMPLETE_SET","state_universe":[read.source],"provider_contract":provider,"version_field":"revision","consistency":"SAME_REVISION","reads":[original_read]});
        assert_eq!(f.load().err(), Some(SessionError::InvalidInput));
        let mut f = Fixture::new();
        let path = f.runtime()["documents"][0]["path"]
            .as_str()
            .unwrap()
            .to_owned();
        std::fs::write(path, vec![b' '; DOCUMENT_BYTES + 1]).unwrap();
        assert_eq!(f.load().err(), Some(SessionError::LimitExceeded));
        let mut f = Fixture::new();
        f.rewrite("verifier", json!({"password":"not-a-secret"}));
        assert_eq!(f.load().err(), Some(SessionError::AuthorityDenied));
    }
}
