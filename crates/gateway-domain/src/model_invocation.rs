//! EPIC-06.01: immutable, provider-neutral model contracts. Validation establishes
//! compatibility only; trusted Process/Policy admission remains with the host.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::cognitive_routing::{PrivacyBoundary, TaskClass};
use crate::{
    CapabilityId, ContentDigest, ReferenceId, ResolvedReference, SchemaVersion, SemanticTaskIR,
    SensitivityClass, TaskId, context_content_digest, semantic_output::OutputContract,
};

pub const MODEL_CONTRACT_VERSION: SchemaVersion = SchemaVersion::V1;
type Pin = ResolvedReference<ReferenceId>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ModelContractError {
    IncompatibleVersion,
    InvalidBinding,
    InvalidLimits,
    InvalidGenerationParameter,
    ModelCapabilityMismatch,
    ContextLimitExceeded,
    AuthorityMismatch,
    ModelUnavailable,
}
impl fmt::Display for ModelContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_value(self)
                .expect("error enum")
                .as_str()
                .expect("string enum")
        )
    }
}
impl std::error::Error for ModelContractError {}
use ModelContractError as E;

fn version(v: SchemaVersion) -> Result<(), E> {
    if v != MODEL_CONTRACT_VERSION {
        return Err(E::IncompatibleVersion);
    }
    Ok(())
}
pub fn validate_model_binding<I>(pin: &ResolvedReference<I>) -> Result<(), E> {
    version(pin.contract_version)?;
    if pin.revision.as_str() == "latest"
        || pin.digest.as_str().len() != 64
        || !pin
            .digest
            .as_str()
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(E::InvalidBinding);
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GenerationControl {
    Temperature,
    TopP,
    TopK,
    StopConditions,
    Seed,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum GenerationValue {
    Scalar(f64),
    Integer(u64),
    Stops(BTreeSet<String>),
}
impl GenerationValue {
    fn validate(&mut self, control: GenerationControl) -> Result<(), E> {
        use GenerationControl as C;
        let valid = match (control, &mut *self) {
            (C::Temperature, Self::Scalar(v)) => v.is_finite() && *v >= 0.0,
            (C::TopP, Self::Scalar(v)) => v.is_finite() && *v > 0.0 && *v <= 1.0,
            (C::TopK, Self::Integer(v)) => *v > 0,
            (C::Seed, Self::Integer(_)) => true,
            (C::StopConditions, Self::Stops(v)) => {
                !v.is_empty()
                    && v.len() <= 128
                    && v.iter().all(|s| !s.is_empty() && s.len() <= 4096)
            }
            _ => false,
        };
        if !valid {
            return Err(E::InvalidGenerationParameter);
        }
        if let Self::Scalar(v) = self {
            if *v == 0.0 {
                *v = 0.0;
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum GenerationDomain {
    Scalar { min: f64, max: f64 },
    Integer { min: u64, max: u64 },
    Stops { max_count: u16, max_bytes: u32 },
}
impl GenerationDomain {
    fn validate(&self, control: GenerationControl) -> Result<(), E> {
        use GenerationControl as C;
        let valid = match (control, self) {
            (C::Temperature | C::TopP, Self::Scalar { min, max }) => {
                min.is_finite()
                    && max.is_finite()
                    && min <= max
                    && *min >= 0.0
                    && (control != C::TopP || (*min > 0.0 && *max <= 1.0))
            }
            (C::TopK | C::Seed, Self::Integer { min, max }) => {
                min <= max && (control != C::TopK || *min > 0)
            }
            (
                C::StopConditions,
                Self::Stops {
                    max_count,
                    max_bytes,
                },
            ) => (1..=128).contains(max_count) && (1..=4096).contains(max_bytes),
            _ => false,
        };
        if !valid {
            return Err(E::InvalidLimits);
        }
        Ok(())
    }
    fn accepts(&self, value: &GenerationValue) -> bool {
        match (self, value) {
            (Self::Scalar { min, max }, GenerationValue::Scalar(v)) => v >= min && v <= max,
            (Self::Integer { min, max }, GenerationValue::Integer(v)) => v >= min && v <= max,
            (
                Self::Stops {
                    max_count,
                    max_bytes,
                },
                GenerationValue::Stops(v),
            ) => v.len() <= *max_count as usize && v.iter().all(|s| s.len() <= *max_bytes as usize),
            _ => false,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StructuredOutputMode {
    Text,
    Json,
    JsonSchema,
    ToolProposal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReproducibilityClass {
    Stochastic,
    SeedHint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationProfileData {
    pub schema_version: SchemaVersion,
    pub profile_id: ReferenceId,
    pub revision: ReferenceId,
    pub controls: BTreeMap<GenerationControl, GenerationValue>,
    pub max_output_tokens: u32,
    pub structured_output_mode: StructuredOutputMode,
    pub reproducibility_class: ReproducibilityClass,
}
impl TryFrom<GenerationProfileData> for GenerationProfile {
    type Error = E;
    fn try_from(mut data: GenerationProfileData) -> Result<Self, E> {
        version(data.schema_version)?;
        if data.revision.as_str() == "latest" {
            return Err(E::InvalidBinding);
        }
        if data.max_output_tokens == 0 {
            return Err(E::InvalidLimits);
        }
        for (control, value) in &mut data.controls {
            value.validate(*control)?;
        }
        if data.controls.contains_key(&GenerationControl::Seed)
            != (data.reproducibility_class == ReproducibilityClass::SeedHint)
        {
            return Err(E::InvalidGenerationParameter);
        }
        Ok(Self(data))
    }
}

macro_rules! immutable_contract {
    ($name:ident, $data:ident, $wire:literal) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(try_from = $wire, into = $wire)]
        pub struct $name($data);
        impl From<$name> for $data {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl $name {
            pub fn data(&self) -> &$data {
                &self.0
            }
            /// Deterministic for equivalent validated inputs; no provider defaults are inferred.
            pub fn content_digest(&self) -> ContentDigest {
                context_content_digest(
                    &serde_json::to_vec(self).expect("validated finite contract"),
                )
            }
        }
    };
}
immutable_contract!(
    GenerationProfile,
    GenerationProfileData,
    "GenerationProfileData"
);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderData {
    pub schema_version: SchemaVersion,
    pub identity: Pin,
    pub adapter: Pin,
    pub locality: PrivacyBoundary,
    pub maximum_sensitivity: SensitivityClass,
    pub controls: BTreeMap<GenerationControl, GenerationDomain>,
    /// Known, material defaults; all other supported controls must be listed as unexposed.
    pub defaults: BTreeMap<GenerationControl, GenerationValue>,
    pub unexposed_defaults: BTreeSet<GenerationControl>,
    pub provenance: Pin,
}
impl TryFrom<ModelProviderData> for ModelProvider {
    type Error = E;
    fn try_from(mut data: ModelProviderData) -> Result<Self, E> {
        version(data.schema_version)?;
        for pin in [&data.identity, &data.adapter, &data.provenance] {
            validate_model_binding(pin)?;
        }
        for (control, domain) in &data.controls {
            domain.validate(*control)?;
        }
        for (control, value) in &mut data.defaults {
            value.validate(*control)?;
            if !data
                .controls
                .get(control)
                .is_some_and(|domain| domain.accepts(value))
            {
                return Err(E::ModelCapabilityMismatch);
            }
        }
        let known: BTreeSet<_> = data.defaults.keys().copied().collect();
        if !known.is_disjoint(&data.unexposed_defaults)
            || known
                .union(&data.unexposed_defaults)
                .copied()
                .collect::<BTreeSet<_>>()
                != data.controls.keys().copied().collect()
        {
            return Err(E::InvalidGenerationParameter);
        }
        Ok(Self(data))
    }
}
immutable_contract!(ModelProvider, ModelProviderData, "ModelProviderData");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilityData {
    pub schema_version: SchemaVersion,
    pub identity: Pin,
    pub model: Pin,
    pub provider: Pin,
    pub task_classes: BTreeSet<TaskClass>,
    pub input_contracts: BTreeSet<ReferenceId>,
    pub output_contracts: BTreeMap<ReferenceId, Pin>,
    pub context_limit: u32,
    pub max_output_tokens: u32,
    pub structured_output_modes: BTreeSet<StructuredOutputMode>,
    pub tool_calling: bool,
    pub reasoning_strategies: BTreeSet<ReferenceId>,
    pub provenance: Pin,
}
impl TryFrom<ModelCapabilityData> for ModelCapability {
    type Error = E;
    fn try_from(data: ModelCapabilityData) -> Result<Self, E> {
        version(data.schema_version)?;
        for pin in [
            &data.identity,
            &data.model,
            &data.provider,
            &data.provenance,
        ] {
            validate_model_binding(pin)?;
        }
        for (id, pin) in &data.output_contracts {
            validate_model_binding(pin)?;
            if id != &pin.id {
                return Err(E::InvalidBinding);
            }
        }
        if data.context_limit == 0
            || data.max_output_tokens == 0
            || data.max_output_tokens > data.context_limit
            || data.task_classes.is_empty()
            || data.input_contracts.is_empty()
            || data.output_contracts.is_empty()
            || data.structured_output_modes.is_empty()
            || data.reasoning_strategies.is_empty()
        {
            return Err(E::InvalidLimits);
        }
        Ok(Self(data))
    }
}
immutable_contract!(ModelCapability, ModelCapabilityData, "ModelCapabilityData");

/// Mutable health observations are deliberately outside semantic capability identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ModelAvailability {
    Available,
    Unavailable,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRuntimeStatus {
    pub provider: Pin,
    pub availability: ModelAvailability,
    pub observation: Pin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelInvocationData {
    pub schema_version: SchemaVersion,
    pub invocation_id: ReferenceId,
    pub task: ResolvedReference<TaskId>,
    pub compiled_context: Pin,
    pub capability: Pin,
    pub task_class: TaskClass,
    pub input_contract: ReferenceId,
    pub output: OutputContract,
    pub strategy: ReferenceId,
    pub permitted_capabilities: BTreeMap<CapabilityId, ResolvedReference<CapabilityId>>,
    pub generation: GenerationProfile,
    /// Conservative pre-render input reservation; exact tokenizer accounting belongs to #264.
    pub input_tokens: u32,
    pub timeout_ms: u32,
    pub max_attempts: u16,
    pub sensitivity: SensitivityClass,
    pub permitted_localities: BTreeSet<PrivacyBoundary>,
    pub authority: Pin,
    pub provenance: Pin,
}
/// Captured trusted-host bindings. These are not caller/model self-authorization.
pub struct ModelInvocationBasis<'a> {
    pub task: &'a SemanticTaskIR,
    pub output: &'a OutputContract,
    pub compiled_context: &'a Pin,
    pub permitted_capabilities: &'a BTreeMap<CapabilityId, ResolvedReference<CapabilityId>>,
    pub sensitivity: SensitivityClass,
    pub permitted_localities: &'a BTreeSet<PrivacyBoundary>,
    pub authority: &'a Pin,
}
/// Output-only admitted contract: deserializing raw input cannot create it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ModelInvocation(ModelInvocationData);
impl ModelInvocation {
    pub fn new(data: ModelInvocationData, basis: ModelInvocationBasis<'_>) -> Result<Self, E> {
        version(data.schema_version)?;
        version(data.output.schema_version)?;
        validate_model_binding(&data.task)?;
        for pin in [
            &data.compiled_context,
            &data.capability,
            &data.output.schema,
            &data.authority,
            &data.provenance,
        ] {
            validate_model_binding(pin)?;
        }
        let scope = &basis.task.data().target.reference.scope;
        if data.task.id != basis.task.data().id
            || data.task.digest != basis.task.content_digest().map_err(|_| E::InvalidBinding)?
            || &data.task.scope != scope
            || data.task.contract.as_str() != "semantic-task-ir"
            || &data.compiled_context != basis.compiled_context
            || data.output != *basis.output
            || data.output.schema != basis.task.data().output_contract.schema
            || &data.output.schema.scope != scope
            || &data.compiled_context.scope != scope
        {
            return Err(E::InvalidBinding);
        }
        if &data.authority != basis.authority
            || &data.authority.scope != scope
            || data.sensitivity != basis.sensitivity
            || data.permitted_localities.is_empty()
            || !data
                .permitted_localities
                .is_subset(basis.permitted_localities)
        {
            return Err(E::AuthorityMismatch);
        }
        for (id, pin) in &data.permitted_capabilities {
            validate_model_binding(pin)?;
            if id != &pin.id
                || &pin.scope != scope
                || basis.permitted_capabilities.get(id) != Some(pin)
            {
                return Err(E::AuthorityMismatch);
            }
        }
        if data.timeout_ms == 0 || data.max_attempts == 0 || data.input_tokens == 0 {
            return Err(E::InvalidLimits);
        }
        Ok(Self(data))
    }
    pub fn data(&self) -> &ModelInvocationData {
        &self.0
    }
    pub fn content_digest(&self) -> ContentDigest {
        context_content_digest(&serde_json::to_vec(self).expect("validated invocation"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GenerationParameterOrigin {
    RequestedProfile,
    AdapterDefault,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EffectiveGenerationValue {
    pub value: GenerationValue,
    pub origin: GenerationParameterOrigin,
    pub provenance: ContentDigest,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EffectiveGenerationRecord {
    pub schema_version: SchemaVersion,
    pub requested_profile_digest: ContentDigest,
    pub provider_declaration_digest: ContentDigest,
    pub controls: BTreeMap<GenerationControl, EffectiveGenerationValue>,
    pub unexposed_defaults: BTreeSet<GenerationControl>,
    pub max_output_tokens: u32,
    pub structured_output_mode: StructuredOutputMode,
    pub reproducibility_class: ReproducibilityClass,
}
/// Strict negotiation: no implicit degradation. The adapter must execute these
/// effective controls or return a typed mismatch. Seeds never promise exact replay.
pub fn effective_generation(
    profile: &GenerationProfile,
    provider: &ModelProvider,
) -> Result<EffectiveGenerationRecord, E> {
    let requested_profile_digest = profile.content_digest();
    let provider_declaration_digest = provider.content_digest();
    let mut controls: BTreeMap<_, _> = provider
        .0
        .defaults
        .iter()
        .map(|(k, v)| {
            (
                *k,
                EffectiveGenerationValue {
                    value: v.clone(),
                    origin: GenerationParameterOrigin::AdapterDefault,
                    provenance: provider_declaration_digest.clone(),
                },
            )
        })
        .collect();
    for (control, value) in &profile.0.controls {
        if !provider
            .0
            .controls
            .get(control)
            .is_some_and(|domain| domain.accepts(value))
        {
            return Err(E::ModelCapabilityMismatch);
        }
        controls.insert(
            *control,
            EffectiveGenerationValue {
                value: value.clone(),
                origin: GenerationParameterOrigin::RequestedProfile,
                provenance: requested_profile_digest.clone(),
            },
        );
    }
    let unexposed_defaults = provider
        .0
        .unexposed_defaults
        .iter()
        .filter(|c| !controls.contains_key(c))
        .copied()
        .collect();
    Ok(EffectiveGenerationRecord {
        schema_version: MODEL_CONTRACT_VERSION,
        requested_profile_digest,
        provider_declaration_digest,
        controls,
        unexposed_defaults,
        max_output_tokens: profile.0.max_output_tokens,
        structured_output_mode: profile.0.structured_output_mode,
        reproducibility_class: profile.0.reproducibility_class,
    })
}

/// Semantic compatibility is independent of runtime availability; never route here.
pub fn validate_model_eligibility(
    invocation: &ModelInvocation,
    capability: &ModelCapability,
    provider: &ModelProvider,
) -> Result<(), E> {
    let d = invocation.data();
    let c = capability.data();
    let p = provider.data();
    if d.capability != c.identity
        || c.provider != p.identity
        || !c.task_classes.contains(&d.task_class)
        || !c.input_contracts.contains(&d.input_contract)
        || c.output_contracts.get(&d.output.schema.id) != Some(&d.output.schema)
        || !c.reasoning_strategies.contains(&d.strategy)
        || !c
            .structured_output_modes
            .contains(&d.generation.0.structured_output_mode)
        || ((!d.permitted_capabilities.is_empty()
            || d.generation.0.structured_output_mode == StructuredOutputMode::ToolProposal)
            && !c.tool_calling)
        || d.sensitivity > p.maximum_sensitivity
        || !d.permitted_localities.contains(&p.locality)
    {
        return Err(E::ModelCapabilityMismatch);
    }
    if d.generation.0.max_output_tokens > c.max_output_tokens
        || u64::from(d.input_tokens) + u64::from(d.generation.0.max_output_tokens)
            > u64::from(c.context_limit)
    {
        return Err(E::ContextLimitExceeded);
    }
    effective_generation(&d.generation, provider)?;
    Ok(())
}
