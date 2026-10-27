//! Owning operator documents for native model preparation. Inspectable data,
//! never Process/Policy, semantic-reference, availability or spending grants.
use super::{Pin, PinnedDocument, SessionError};
use crate::model_adapters::{SessionToolResponseMode, SessionToolWireMode};
use gateway_domain::{
    cognitive_routing::{PrivacyBoundary, TaskClass},
    model_invocation::*,
    *,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;

const BYTES: usize = super::DOCUMENT_BYTES;
const NODES: usize = 128;
const DEPTH: usize = 32;
const ENTRIES: usize = 64;
const MODEL: &str = "cg.connector-model";
const PROVIDER: &str = "cg.connector-model-provider";
const CAPABILITY: &str = "cg.connector-model-capability";
const GENERATION: &str = "cg.connector-model-generation";
const STATUS: &str = "cg.connector-model-status";
const TARGET: &str = "cg.connector-model-target";
const ADAPTER: &str = "cg.connector-model-adapter";
const TOKENIZER: &str = "cg.connector-model-tokenizer";
const PROVENANCE: &str = "cg.connector-model-provenance";
const ANALYSIS: &str = "cg.context-analysis";
const SOURCE: &str = "cg.context-source-metadata";
fn invalid() -> SessionError {
    SessionError::InvalidInput
}
fn decode<T: DeserializeOwned>(value: Value) -> Result<T, SessionError> {
    serde_json::from_value(value).map_err(|_| invalid())
}
fn v1(version: SchemaVersion) -> Result<(), SessionError> {
    if version == SchemaVersion::V1 {
        Ok(())
    } else {
        Err(SessionError::UnsupportedCapability)
    }
}
fn role(pin: &Pin, contract: &str) -> Result<(), SessionError> {
    if pin.contract.as_str() != contract {
        return Err(SessionError::UnsupportedCapability);
    }
    validate_model_binding(pin).map_err(|_| invalid())
}
fn reserved(value: &Value, keys: &[&str]) -> Result<(), SessionError> {
    if !value.is_object() || keys.iter().any(|key| value.get(key).is_some()) {
        Err(invalid())
    } else {
        Ok(())
    }
}
fn inject(mut value: Value, key: &str, data: impl serde::Serialize) -> Result<Value, SessionError> {
    value.as_object_mut().ok_or_else(invalid)?.insert(
        key.into(),
        serde_json::to_value(data).map_err(|_| invalid())?,
    );
    Ok(value)
}
fn pinned_value(pin: &Pin, bytes: &[u8]) -> Result<Value, SessionError> {
    if bytes.len() > BYTES {
        return Err(SessionError::LimitExceeded);
    }
    validate_model_binding(pin).map_err(|_| invalid())?;
    let value = crate::local_mcp::decode::decode(bytes).map_err(|_| invalid())?;
    if !gateway_application::codex::security::credential_free(&value) {
        return Err(SessionError::AuthorityDenied);
    }
    let canonical = serde_json::to_vec(&value).map_err(|_| invalid())?;
    if context_content_digest(&canonical) != pin.digest {
        return Err(SessionError::StaleRevision);
    }
    Ok(value)
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum NativeProfile {
    NativeSessionModelV1,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelProducerDocument {
    profile: NativeProfile,
    schema_version: SchemaVersion,
    pub(super) task: Pin,
    pub(super) invocation_task: ResolvedReference<TaskId>,
    pub(super) provider: Pin,
    pub(super) capability: Pin,
    pub(super) generation: Pin,
    pub(super) status: Pin,
    pub(super) analysis: Pin,
    pub(super) sources: Vec<Pin>,
    pub(super) task_class: TaskClass,
    pub(super) input_contract: ReferenceId,
    pub(super) strategy: ReferenceId,
    pub(super) sensitivity: SensitivityClass,
    pub(super) permitted_localities: BTreeSet<PrivacyBoundary>,
    pub(super) input_tokens: u32,
    pub(super) timeout_ms: u32,
    pub(super) max_attempts: u16,
}
impl ModelProducerDocument {
    pub(super) fn validate_task_alias(
        &self,
        actual: &Pin,
        task: &SemanticTaskIR,
    ) -> Result<(), SessionError> {
        role(actual, "cg.semantic-task")?;
        let alias = &self.invocation_task;
        validate_model_binding(alias).map_err(|_| invalid())?;
        if &self.task != actual
            || actual.id.as_str() != task.data().id.as_str()
            || actual.scope != task.data().target.reference.scope
            || actual.digest != task.content_digest().map_err(|_| invalid())?
            || alias.contract.as_str() != "semantic-task-ir"
            || alias.contract_version != actual.contract_version
            || alias.id != task.data().id
            || alias.scope != actual.scope
            || alias.revision != actual.revision
            || alias.digest != actual.digest
        {
            return Err(SessionError::StaleRevision);
        }
        Ok(())
    }
    fn check(&self) -> Result<(), SessionError> {
        match self.profile {
            NativeProfile::NativeSessionModelV1 => {}
        }
        v1(self.schema_version)?;
        for (pin, kind) in [
            (&self.task, "cg.semantic-task"),
            (&self.provider, PROVIDER),
            (&self.capability, CAPABILITY),
            (&self.generation, GENERATION),
            (&self.status, STATUS),
            (&self.analysis, ANALYSIS),
        ] {
            role(pin, kind)?;
        }
        let alias = &self.invocation_task;
        validate_model_binding(alias).map_err(|_| invalid())?;
        if alias.contract.as_str() != "semantic-task-ir"
            || alias.id.as_str() != self.task.id.as_str()
            || alias.scope != self.task.scope
            || alias.contract_version != self.task.contract_version
            || alias.revision != self.task.revision
            || alias.digest != self.task.digest
        {
            return Err(SessionError::StaleRevision);
        }
        if self.sources.len() > ENTRIES
            || self.sources.iter().any(|p| role(p, SOURCE).is_err())
            || self
                .sources
                .iter()
                .map(|p| &p.id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.sources.len()
            || self.permitted_localities.is_empty()
            || self.permitted_localities.len() > 4
            || self.input_tokens == 0
            || self.timeout_ms == 0
            || self.timeout_ms > 86_400_000
            || self.max_attempts == 0
            || self.max_attempts > 32
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct DeclaredStatus {
    pub(super) provider: Pin,
    pub(super) availability: ModelAvailability,
    pub(super) observation: Pin,
    pub(super) valid_from_ms: u64,
    pub(super) valid_until_ms: u64,
}
/// This declaration is deliberately NOT a ModelRuntimeStatus. A later factory
/// must authenticate the current status owner and exact source bytes, check the
/// validity window against its real clock, and only then build native status.
impl DeclaredStatus {
    pub(super) fn in_window(&self, now_ms: u64) -> bool {
        self.valid_from_ms <= now_ms && now_ms < self.valid_until_ms
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusWire {
    schema_version: SchemaVersion,
    provider: Pin,
    availability: ModelAvailability,
    valid_from_ms: u64,
    valid_until_ms: u64,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TargetDefinition {
    schema_version: SchemaVersion,
    pub(super) runtime_id: String,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum RendererProfile {
    ChatCompletionsSessionToolsRuntimeV2,
    ChatCompletionsSessionToolsPortableRuntimeV4,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum TokenizerMethod {
    Utf8WireBytesPlus256V1,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdapterDefinition {
    schema_version: SchemaVersion,
    pub(super) renderer: RendererProfile,
    pub(super) wire_mode: SessionToolWireMode,
    pub(super) response_mode: SessionToolResponseMode,
    pub(super) tokenizer: Pin,
    pub(super) endpoint: String,
    /// Credential selector only; secret resolution belongs to the current host.
    pub(super) credential_profile: Option<ReferenceId>,
    pub(super) credential_source: Option<ModelCredentialSource>,
    pub(super) pricing: PricingDeclaration,
}
/// Explicit secret locator. Only the variable name enters pinned data; no
/// credential value or fingerprint is retained by this definition owner.
#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub(super) enum ModelCredentialSource {
    EnvironmentBearerV1 { variable: String },
}
impl ModelCredentialSource {
    fn check(&self) -> Result<(), SessionError> {
        match self {
            Self::EnvironmentBearerV1 { variable } => {
                if variable.is_empty()
                    || variable.len() > 128
                    || !variable.bytes().enumerate().all(|(i, b)| {
                        b == b'_' || b.is_ascii_alphabetic() || i > 0 && b.is_ascii_digit()
                    })
                {
                    return Err(invalid());
                }
                Ok(())
            }
        }
    }
    /// Called anew by the actual attempt owner, not once per preparation/run.
    pub(super) fn resolve(&self) -> Result<String, SessionError> {
        self.check()?;
        let Self::EnvironmentBearerV1 { variable } = self;
        let secret = std::env::var(variable).map_err(|_| SessionError::Unavailable)?;
        if secret.is_empty() || secret.len() > 8192 || secret.chars().any(char::is_control) {
            return Err(SessionError::Unavailable);
        }
        Ok(secret)
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PricingDeclaration {
    pub(super) cost_unit: ReferenceId,
    pub(super) fixed_per_attempt_microunits: u64,
    pub(super) input_microunits_per_unit: u64,
    pub(super) output_microunits_per_unit: u64,
}
impl PricingDeclaration {
    /// Arithmetic only; the host must authenticate/qualify the actual tariff.
    pub(super) fn maximum(
        &self,
        input: u64,
        output: u64,
        attempts: u64,
    ) -> Result<u64, SessionError> {
        input
            .checked_mul(self.input_microunits_per_unit)
            .and_then(|i| {
                output
                    .checked_mul(self.output_microunits_per_unit)
                    .and_then(|o| i.checked_add(o))
            })
            .and_then(|n| n.checked_add(self.fixed_per_attempt_microunits))
            .and_then(|n| n.checked_mul(attempts))
            .ok_or(SessionError::LimitExceeded)
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenizerDefinition {
    schema_version: SchemaVersion,
    method: TokenizerMethod,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceDefinition {
    schema_version: SchemaVersion,
    declaration: ProvenanceKind,
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ProvenanceKind {
    OperatorDeclarationV1,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum BudgetClass {
    AuthorityReserved,
    TaskReserved,
    OutputContractReserved,
    Evidence,
    Knowledge,
    Memory,
    RuntimeState,
    SafetyMargin,
}
impl BudgetClass {
    fn native(&self) -> ContextBudgetClass {
        match self {
            Self::AuthorityReserved => ContextBudgetClass::AuthorityReserved,
            Self::TaskReserved => ContextBudgetClass::TaskReserved,
            Self::OutputContractReserved => ContextBudgetClass::OutputContractReserved,
            Self::Evidence => ContextBudgetClass::Evidence,
            Self::Knowledge => ContextBudgetClass::Knowledge,
            Self::Memory => ContextBudgetClass::Memory,
            Self::RuntimeState => ContextBudgetClass::RuntimeState,
            Self::SafetyMargin => ContextBudgetClass::SafetyMargin,
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RetrievalBudgetSpec {
    results: u64,
    rounds: u64,
    latency_ms: u64,
    cost: u64,
    cost_unit: NonEmptyText,
    tokens: u64,
    context_total: u64,
    context_reservations: Vec<ClassReservation>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassReservation {
    class: BudgetClass,
    units: u64,
}
impl RetrievalBudgetSpec {
    pub(super) fn native(&self) -> Result<RetrievalBudget, SessionError> {
        if self.context_reservations.len() > 8 {
            return Err(SessionError::LimitExceeded);
        }
        let mut reservations = BTreeMap::new();
        for entry in &self.context_reservations {
            if reservations
                .insert(entry.class.native(), TokenBudget(entry.units))
                .is_some()
            {
                return Err(invalid());
            }
        }
        Ok(RetrievalBudget {
            results: ResultBudget(NonZeroU64::new(self.results).ok_or_else(invalid)?),
            rounds: RoundBudget(NonZeroU64::new(self.rounds).ok_or_else(invalid)?),
            latency: LatencyBudget(NonZeroU64::new(self.latency_ms).ok_or_else(invalid)?),
            cost: CostBudget {
                maximum: self.cost,
                unit: self.cost_unit.clone(),
            },
            tokens: TokenBudget(self.tokens),
            context: ContextBudget::new(TokenBudget(self.context_total), reservations)
                .map_err(|_| invalid())?,
        })
    }
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum SourceClass {
    CurrentInput,
    TaskState,
    SituationState,
    CatalogAuthority,
    Repository,
    Rag,
    StructuredData,
    Memory,
    Examples,
    ToolsMcp,
    RuntimeState,
    Clarification,
}
impl SourceClass {
    pub(super) fn native(&self) -> ContextSourceClass {
        match self {
            Self::CurrentInput => ContextSourceClass::CurrentInput,
            Self::TaskState => ContextSourceClass::TaskState,
            Self::SituationState => ContextSourceClass::SituationState,
            Self::CatalogAuthority => ContextSourceClass::CatalogAuthority,
            Self::Repository => ContextSourceClass::Repository,
            Self::Rag => ContextSourceClass::Rag,
            Self::StructuredData => ContextSourceClass::StructuredData,
            Self::Memory => ContextSourceClass::Memory,
            Self::Examples => ContextSourceClass::Examples,
            Self::ToolsMcp => ContextSourceClass::ToolsMcp,
            Self::RuntimeState => ContextSourceClass::RuntimeState,
            Self::Clarification => ContextSourceClass::Clarification,
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum NeedClass {
    SystemState,
    DomainKnowledge,
    Evidence,
    StructuredData,
    Memory,
    RuntimeState,
}
impl NeedClass {
    fn native(&self) -> ContextClass {
        match self {
            Self::SystemState => ContextClass::SystemState,
            Self::DomainKnowledge => ContextClass::DomainKnowledge,
            Self::Evidence => ContextClass::Evidence,
            Self::StructuredData => ContextClass::StructuredData,
            Self::Memory => ContextClass::Memory,
            Self::RuntimeState => ContextClass::RuntimeState,
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdditionalNeedSpec {
    id: ReferenceId,
    class: NeedClass,
    purpose: NonEmptyText,
    mandatory: bool,
    permitted_sources: Vec<SourceClass>,
    description: NonEmptyText,
    maximum_sensitivity: SensitivityClass,
    accepted_trust: BTreeSet<TrustClass>,
    fresh: bool,
    evidence: Vec<EvidenceId>,
    provenances: Vec<ProvenanceId>,
    budget: RetrievalBudgetSpec,
    priority: u32,
}
impl AdditionalNeedSpec {
    pub(super) fn native(&self) -> Result<ContextRequirement, SessionError> {
        if self.permitted_sources.is_empty()
            || self.permitted_sources.len() > 12
            || self.evidence.len() > ENTRIES
            || self.provenances.len() > ENTRIES
            || self.accepted_trust.is_empty()
        {
            return Err(invalid());
        }
        let sources: Vec<_> = self
            .permitted_sources
            .iter()
            .map(SourceClass::native)
            .collect();
        if sources.iter().collect::<BTreeSet<_>>().len() != sources.len() {
            return Err(invalid());
        }
        Ok(ContextRequirement {
            id: self.id.clone(),
            class: self.class.native(),
            purpose: self.purpose.clone(),
            necessity: if self.mandatory {
                ContextNecessity::Mandatory
            } else {
                ContextNecessity::Optional
            },
            permitted_sources: sources,
            information: RequiredInformation {
                description: self.description.clone(),
                requirements: InformationRequirements::new(
                    if self.fresh {
                        FreshnessRequirement::Fresh
                    } else {
                        FreshnessRequirement::NotSpecified
                    },
                    None,
                    self.evidence.clone(),
                    self.provenances.clone(),
                )
                .map_err(|_| invalid())?,
                accepted_trust: self.accepted_trust.clone(),
                maximum_sensitivity: self.maximum_sensitivity,
            },
            budget: self.budget.native()?,
            stop: StopCondition::BudgetExhausted,
            priority: self.priority,
        })
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ContextAnalysisDocument {
    profile: NativeProfile,
    schema_version: SchemaVersion,
    pub(super) budget_per_requirement: RetrievalBudgetSpec,
    pub(super) total_tokens: u64,
    pub(super) maximum_sensitivity: SensitivityClass,
    pub(super) additional: Vec<AdditionalNeedSpec>,
}
impl ContextAnalysisDocument {
    pub(super) fn additional(&self) -> Result<Vec<ContextRequirement>, SessionError> {
        self.additional
            .iter()
            .map(AdditionalNeedSpec::native)
            .collect()
    }
}
#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub(super) enum SatisfactionBinding {
    BeforeRead {
        target: Pin,
        mapping_reference: String,
        source: SourceId,
    },
    Catalog {
        projection: Pin,
    },
    Task {
        task: Pin,
    },
    OutputContract {
        output: Pin,
        verification: Pin,
    },
    Fragment {
        fragment: Pin,
    },
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceBindingDocument {
    profile: NativeProfile,
    schema_version: SchemaVersion,
    pub(super) class: SourceClass,
    pub(super) covers: BTreeSet<ReferenceId>,
    pub(super) satisfaction: SatisfactionBinding,
}

pub(super) enum ModelDocument {
    Model(Box<ModelProducerDocument>),
    Provider(ModelProviderData),
    Capability(ModelCapabilityData),
    Generation(GenerationProfileData),
    Status(DeclaredStatus),
    Target(TargetDefinition),
    Adapter(AdapterDefinition),
    Tokenizer(TokenizerMethod),
    Provenance,
    Analysis(ContextAnalysisDocument),
    Source(SourceBindingDocument),
}
impl ModelDocument {
    pub(super) fn supports(contract: &str) -> bool {
        matches!(
            contract,
            MODEL
                | PROVIDER
                | CAPABILITY
                | GENERATION
                | STATUS
                | TARGET
                | ADAPTER
                | TOKENIZER
                | PROVENANCE
                | ANALYSIS
                | SOURCE
        )
    }
    pub(super) fn parse(pin: &Pin, bytes: &[u8]) -> Result<Self, SessionError> {
        let value = pinned_value(pin, bytes)?;
        Ok(match pin.contract.as_str() {
            MODEL => {
                let data: ModelProducerDocument = decode(value)?;
                data.check()?;
                Self::Model(Box::new(data))
            }
            PROVIDER => {
                reserved(&value, &["identity"])?;
                let data: ModelProviderData = decode(inject(value, "identity", pin)?)?;
                v1(data.schema_version)?;
                role(&data.adapter, ADAPTER)?;
                role(&data.provenance, PROVENANCE)?;
                if data.controls.len() > ENTRIES || data.defaults.len() > ENTRIES {
                    return Err(invalid());
                }
                Self::Provider(data)
            }
            CAPABILITY => {
                reserved(&value, &["identity"])?;
                let data: ModelCapabilityData = decode(inject(value, "identity", pin)?)?;
                v1(data.schema_version)?;
                role(&data.model, TARGET)?;
                role(&data.provider, PROVIDER)?;
                role(&data.provenance, PROVENANCE)?;
                if data.output_contracts.len() > ENTRIES
                    || data.input_contracts.len() > ENTRIES
                    || data.task_classes.len() > ENTRIES
                    || data.reasoning_strategies.len() > ENTRIES
                {
                    return Err(invalid());
                }
                for pin in data.output_contracts.values() {
                    role(pin, "cg.output-schema")?;
                }
                Self::Capability(data)
            }
            GENERATION => {
                reserved(&value, &["identity", "profile_id", "revision"])?;
                let value = inject(
                    inject(value, "profile_id", &pin.id)?,
                    "revision",
                    &pin.revision,
                )?;
                let data: GenerationProfileData = decode(value)?;
                v1(data.schema_version)?;
                Self::Generation(data)
            }
            STATUS => {
                let data: StatusWire = decode(value)?;
                v1(data.schema_version)?;
                role(&data.provider, PROVIDER)?;
                if data.valid_from_ms >= data.valid_until_ms
                    || data.valid_until_ms - data.valid_from_ms > 86_400_000
                {
                    return Err(invalid());
                }
                Self::Status(DeclaredStatus {
                    provider: data.provider,
                    availability: data.availability,
                    observation: pin.clone(),
                    valid_from_ms: data.valid_from_ms,
                    valid_until_ms: data.valid_until_ms,
                })
            }
            TARGET => {
                let data: TargetDefinition = decode(value)?;
                v1(data.schema_version)?;
                if data.runtime_id.is_empty()
                    || data.runtime_id.len() > 256
                    || !data
                        .runtime_id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-/:".contains(&b))
                {
                    return Err(invalid());
                }
                Self::Target(data)
            }
            ADAPTER => {
                let data: AdapterDefinition = decode(value)?;
                v1(data.schema_version)?;
                role(&data.tokenizer, TOKENIZER)?;
                if data.credential_profile.is_some() {
                    return Err(SessionError::UnsupportedCapability);
                }
                if let Some(locator) = &data.credential_source {
                    locator.check()?;
                }
                if !matches!(
                    (data.renderer, data.wire_mode),
                    (
                        RendererProfile::ChatCompletionsSessionToolsRuntimeV2,
                        SessionToolWireMode::CanonicalV1
                    ) | (
                        RendererProfile::ChatCompletionsSessionToolsPortableRuntimeV4,
                        SessionToolWireMode::PortableV1
                    )
                ) || data.endpoint.len() > 2048
                    || data.response_mode != SessionToolResponseMode::ToolProposalTextV1
                {
                    return Err(invalid());
                }
                Self::Adapter(data)
            }
            TOKENIZER => {
                let data: TokenizerDefinition = decode(value)?;
                v1(data.schema_version)?;
                Self::Tokenizer(data.method)
            }
            PROVENANCE => {
                let data: ProvenanceDefinition = decode(value)?;
                v1(data.schema_version)?;
                match data.declaration {
                    ProvenanceKind::OperatorDeclarationV1 => Self::Provenance,
                }
            }
            ANALYSIS => {
                let data: ContextAnalysisDocument = decode(value)?;
                match data.profile {
                    NativeProfile::NativeSessionModelV1 => {}
                }
                v1(data.schema_version)?;
                if data.total_tokens == 0
                    || data.additional.len() > ENTRIES
                    || data
                        .additional
                        .iter()
                        .map(|r| &r.id)
                        .collect::<BTreeSet<_>>()
                        .len()
                        != data.additional.len()
                {
                    return Err(invalid());
                }
                data.budget_per_requirement.native()?;
                data.additional()?;
                Self::Analysis(data)
            }
            SOURCE => {
                let data: SourceBindingDocument = decode(value)?;
                match data.profile {
                    NativeProfile::NativeSessionModelV1 => {}
                }
                v1(data.schema_version)?;
                if data.covers.is_empty() || data.covers.len() > ENTRIES {
                    return Err(invalid());
                }
                match &data.satisfaction {
                    SatisfactionBinding::BeforeRead {
                        target,
                        mapping_reference,
                        ..
                    } => {
                        role(target, "cg.connector-object")?;
                        if !matches!(data.class, SourceClass::ToolsMcp)
                            || mapping_reference.is_empty()
                            || mapping_reference.len() > 256
                            || mapping_reference.chars().any(char::is_control)
                        {
                            return Err(invalid());
                        }
                    }
                    SatisfactionBinding::Catalog { projection } => {
                        role(projection, "cg.context-projection")?
                    }
                    SatisfactionBinding::Task { task } => role(task, "cg.semantic-task")?,
                    SatisfactionBinding::OutputContract {
                        output,
                        verification,
                    } => {
                        role(output, "cg.output-contract")?;
                        role(verification, "cg.verification-contract")?;
                    }
                    SatisfactionBinding::Fragment { fragment } => {
                        role(fragment, "cg.context-fragment")?
                    }
                };
                Self::Source(data)
            }
            _ => return Err(SessionError::UnsupportedCapability),
        })
    }
    pub(super) fn references(&self) -> Vec<Pin> {
        match self {
            Self::Model(d) => {
                let mut p = vec![
                    d.task.clone(),
                    d.provider.clone(),
                    d.capability.clone(),
                    d.generation.clone(),
                    d.status.clone(),
                    d.analysis.clone(),
                ];
                p.extend(d.sources.clone());
                p
            }
            Self::Provider(d) => vec![d.adapter.clone(), d.provenance.clone()],
            Self::Capability(d) => {
                let mut p = vec![d.model.clone(), d.provider.clone(), d.provenance.clone()];
                p.extend(d.output_contracts.values().cloned());
                p
            }
            Self::Status(d) => vec![d.provider.clone()],
            Self::Adapter(d) => vec![d.tokenizer.clone()],
            Self::Source(d) => match &d.satisfaction {
                SatisfactionBinding::BeforeRead { target, .. } => vec![target.clone()],
                SatisfactionBinding::Catalog { projection } => vec![projection.clone()],
                SatisfactionBinding::Task { task } => vec![task.clone()],
                SatisfactionBinding::OutputContract {
                    output,
                    verification,
                } => vec![output.clone(), verification.clone()],
                SatisfactionBinding::Fragment { fragment } => vec![fragment.clone()],
            },
            _ => vec![],
        }
    }
}

/// Only constructed after every exact captured reference and cycle is checked.
/// This result remains declarations: it performs no current authority checks.
pub(super) struct NativeModelDefinitions {
    pub(super) model: ModelProducerDocument,
    pub(super) provider: ModelProvider,
    pub(super) capability: ModelCapability,
    pub(super) generation: GenerationProfile,
    pub(super) status: DeclaredStatus,
    pub(super) target: TargetDefinition,
    pub(super) adapter: AdapterDefinition,
    pub(super) tokenizer: TokenizerMethod,
    pub(super) analysis: ContextAnalysisDocument,
    pub(super) sources: Vec<SourceBindingDocument>,
}
pub(super) fn read_model(
    root: &Pin,
    catalog: &BTreeMap<ReferenceId, PinnedDocument>,
) -> Result<NativeModelDefinitions, SessionError> {
    role(root, MODEL)?;
    if catalog.len() > super::DOCUMENTS
        || catalog
            .values()
            .try_fold(0usize, |sum, document| {
                sum.checked_add(document.canonical.len())
            })
            .is_none_or(|bytes| bytes > super::CATALOG_BYTES)
    {
        return Err(SessionError::LimitExceeded);
    }
    let mut nodes = BTreeMap::new();
    let mut active = BTreeSet::new();
    let mut done = BTreeSet::new();
    fn visit(
        pin: &Pin,
        scope: &ContextScopeId,
        catalog: &BTreeMap<ReferenceId, PinnedDocument>,
        nodes: &mut BTreeMap<ReferenceId, ModelDocument>,
        active: &mut BTreeSet<ReferenceId>,
        done: &mut BTreeSet<ReferenceId>,
        depth: usize,
    ) -> Result<(), SessionError> {
        if depth > DEPTH {
            return Err(SessionError::LimitExceeded);
        }
        if &pin.scope != scope {
            return Err(SessionError::ScopeDenied);
        }
        let actual = catalog.get(&pin.id).ok_or(SessionError::Unavailable)?;
        if &actual.pin != pin {
            return Err(SessionError::StaleRevision);
        }
        if active.contains(&pin.id) {
            return Err(invalid());
        }
        if done.contains(&pin.id) {
            return Ok(());
        }
        if done.len() + active.len() >= NODES {
            return Err(SessionError::LimitExceeded);
        }
        let value = pinned_value(pin, &actual.canonical)?;
        active.insert(pin.id.clone());
        let edges = if ModelDocument::supports(pin.contract.as_str()) {
            let node = ModelDocument::parse(pin, &actual.canonical)?;
            let edges = node.references();
            nodes.insert(pin.id.clone(), node);
            edges
        } else {
            if !matches!(
                pin.contract.as_str(),
                "cg.semantic-task"
                    | "cg.output-schema"
                    | "cg.output-contract"
                    | "cg.verification-contract"
                    | "cg.context-fragment"
                    | "cg.context-projection"
                    | "cg.connector-object"
                    | "cg.connector-producer-join"
            ) {
                return Err(SessionError::UnsupportedCapability);
            }
            super::document_references(pin, &value)?
        };
        if edges.len() > NODES {
            return Err(SessionError::LimitExceeded);
        }
        for edge in edges {
            visit(&edge, scope, catalog, nodes, active, done, depth + 1)?;
        }
        active.remove(&pin.id);
        done.insert(pin.id.clone());
        Ok(())
    }
    visit(
        root,
        &root.scope,
        catalog,
        &mut nodes,
        &mut active,
        &mut done,
        0,
    )?;
    let Some(ModelDocument::Model(model)) = nodes.get(&root.id) else {
        return Err(invalid());
    };
    let model = (**model).clone();
    let task_doc = catalog.get(&model.task.id).ok_or_else(invalid)?;
    let task: SemanticTaskIR = decode(pinned_value(&model.task, &task_doc.canonical)?)?;
    model.validate_task_alias(&model.task, &task)?;
    let Some(ModelDocument::Provider(provider)) = nodes.get(&model.provider.id) else {
        return Err(invalid());
    };
    let provider = ModelProvider::try_from(provider.clone()).map_err(|_| invalid())?;
    let Some(ModelDocument::Capability(capability)) = nodes.get(&model.capability.id) else {
        return Err(invalid());
    };
    let capability = ModelCapability::try_from(capability.clone()).map_err(|_| invalid())?;
    let Some(ModelDocument::Generation(generation)) = nodes.get(&model.generation.id) else {
        return Err(invalid());
    };
    let generation = GenerationProfile::try_from(generation.clone()).map_err(|_| invalid())?;
    let Some(ModelDocument::Status(status)) = nodes.get(&model.status.id) else {
        return Err(invalid());
    };
    let Some(ModelDocument::Target(target)) = nodes.get(&capability.data().model.id) else {
        return Err(invalid());
    };
    let Some(ModelDocument::Adapter(adapter)) = nodes.get(&provider.data().adapter.id) else {
        return Err(invalid());
    };
    let Some(ModelDocument::Tokenizer(tokenizer)) = nodes.get(&adapter.tokenizer.id) else {
        return Err(invalid());
    };
    let Some(ModelDocument::Analysis(analysis)) = nodes.get(&model.analysis.id) else {
        return Err(invalid());
    };
    if capability.data().provider != provider.data().identity
        || status.provider != provider.data().identity
        || !capability.data().tool_calling
        || !capability
            .data()
            .structured_output_modes
            .contains(&StructuredOutputMode::ToolProposal)
        || generation.data().structured_output_mode != StructuredOutputMode::ToolProposal
        || !capability.data().task_classes.contains(&model.task_class)
        || !capability
            .data()
            .input_contracts
            .contains(&model.input_contract)
        || !capability
            .data()
            .reasoning_strategies
            .contains(&model.strategy)
        || model
            .input_tokens
            .checked_add(generation.data().max_output_tokens)
            .is_none_or(|total| total > capability.data().context_limit)
        || generation.data().max_output_tokens > capability.data().max_output_tokens
        || capability
            .data()
            .output_contracts
            .get(&task.data().output_contract.schema.id)
            != Some(&task.data().output_contract.schema)
    {
        return Err(invalid());
    }
    effective_generation(&generation, &provider).map_err(|_| invalid())?;
    // Reuse the actual HTTP adapter's pure configuration validator; constructing
    // it performs no provider call and establishes no credential/policy grant.
    crate::model_adapters::SessionToolHttpAdapter::new(
        crate::model_adapters::AdapterKind::ChatCompletionsV1,
        adapter.endpoint.clone(),
        None,
        None,
    )
    .map_err(|_| invalid())?
    .with_wire_mode(adapter.wire_mode)
    .with_response_mode(adapter.response_mode);
    let sources = model
        .sources
        .iter()
        .map(|pin| match nodes.get(&pin.id) {
            Some(ModelDocument::Source(data)) => Ok(data.clone()),
            _ => Err(invalid()),
        })
        .collect::<Result<_, _>>()?;
    Ok(NativeModelDefinitions {
        model,
        provider,
        capability,
        generation,
        status: status.clone(),
        target: target.clone(),
        adapter: adapter.clone(),
        tokenizer: *tokenizer,
        analysis: analysis.clone(),
        sources,
    })
}

#[cfg(test)]
#[path = "model_documents/tests.rs"]
mod tests;
