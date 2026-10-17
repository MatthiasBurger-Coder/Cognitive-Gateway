//! Contract preparation only. #263 owns full CG-10 admission/rendering; this
//! service consumes trusted captured bindings and never authorizes model output.
use gateway_domain::model_invocation::{
    EffectiveGenerationRecord, ModelAvailability, ModelCapability, ModelContractError,
    ModelInvocation, ModelProvider, ModelRuntimeStatus, effective_generation,
    validate_model_binding, validate_model_eligibility,
};
use gateway_domain::{ContentDigest, ReferenceId, ResolvedReference};
use serde::Serialize;

/// Only successful strict negotiation can construct this adapter input.
#[derive(Debug, Clone, Serialize)]
pub struct PreparedModelInvocation {
    invocation: ModelInvocation,
    capability: ModelCapability,
    provider: ModelProvider,
    effective_generation: EffectiveGenerationRecord,
}
impl PreparedModelInvocation {
    pub fn invocation(&self) -> &ModelInvocation {
        &self.invocation
    }
    pub fn capability(&self) -> &ModelCapability {
        &self.capability
    }
    pub fn provider(&self) -> &ModelProvider {
        &self.provider
    }
    pub fn effective_generation(&self) -> &EffectiveGenerationRecord {
        &self.effective_generation
    }
}

pub fn prepare_model_invocation(
    invocation: ModelInvocation,
    capability: ModelCapability,
    provider: ModelProvider,
    runtime: &ModelRuntimeStatus,
) -> Result<PreparedModelInvocation, ModelContractError> {
    validate_model_eligibility(&invocation, &capability, &provider)?;
    validate_model_binding(&runtime.provider)?;
    validate_model_binding(&runtime.observation)?;
    if runtime.provider != provider.data().identity {
        return Err(ModelContractError::InvalidBinding);
    }
    if runtime.availability != ModelAvailability::Available {
        return Err(ModelContractError::ModelUnavailable);
    }
    let effective_generation = effective_generation(&invocation.data().generation, &provider)?;
    Ok(PreparedModelInvocation {
        invocation,
        capability,
        provider,
        effective_generation,
    })
}

/// Unverified output reference. A transport success does not establish validity,
/// grounding, permission or external-effect completion.
#[derive(Debug, Clone, Serialize)]
pub struct ModelProposal {
    pub invocation_digest: ContentDigest,
    pub raw_output: ResolvedReference<ReferenceId>,
}
/// Replaceable outer adapter. Implementations MUST map every effective control
/// or return MODEL_CAPABILITY_MISMATCH, and retain the effective record for audit.
/// No tool execution is authorized here. Prompt IR/transport implementations are
/// separate downstream slices; this port deliberately contains no SDK payload.
pub trait ModelInvocationPort {
    fn invoke(
        &self,
        request: &PreparedModelInvocation,
    ) -> Result<ModelProposal, ModelContractError>;
}
