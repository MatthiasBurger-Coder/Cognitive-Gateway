use gateway_domain::cognitive_routing::{PrivacyBoundary, TaskClass};
use gateway_domain::{model_invocation::*, semantic_output::*, *};
use std::collections::{BTreeMap, BTreeSet};
pub fn pin(id: &str) -> ResolvedReference<ReferenceId> {
    ResolvedReference {
        id: ReferenceId::new(id).unwrap(),
        scope: ContextScopeId::new("project-x").unwrap(),
        contract: ReferenceId::new(id).unwrap(),
        contract_version: SchemaVersion::V1,
        revision: ReferenceId::new("7").unwrap(),
        digest: ContentDigest::new("a".repeat(64)).unwrap(),
    }
}
pub fn profile() -> GenerationProfile {
    serde_json::from_str(include_str!("profile.json")).unwrap()
}
pub fn provider_data() -> ModelProviderData {
    use GenerationControl::*;
    ModelProviderData {
        schema_version: SchemaVersion::V1,
        identity: pin("provider"),
        adapter: pin("adapter"),
        locality: PrivacyBoundary::OnDevice,
        maximum_sensitivity: SensitivityClass::Confidential,
        controls: BTreeMap::from([
            (Temperature, GenerationDomain::Scalar { min: 0.0, max: 2.0 }),
            (
                TopP,
                GenerationDomain::Scalar {
                    min: 0.01,
                    max: 1.0,
                },
            ),
            (TopK, GenerationDomain::Integer { min: 1, max: 100 }),
            (
                StopConditions,
                GenerationDomain::Stops {
                    max_count: 4,
                    max_bytes: 100,
                },
            ),
            (
                Seed,
                GenerationDomain::Integer {
                    min: 0,
                    max: u64::MAX,
                },
            ),
        ]),
        defaults: BTreeMap::from([(Temperature, GenerationValue::Scalar(1.0))]),
        unexposed_defaults: BTreeSet::from([TopP, TopK, StopConditions, Seed]),
        provenance: pin("provider-declaration"),
    }
}
pub fn provider() -> ModelProvider {
    ModelProvider::try_from(provider_data()).unwrap()
}
pub fn task() -> SemanticTaskIR {
    serde_json::from_str(include_str!("../semantic-task-v1/minimal.json")).unwrap()
}
pub fn output() -> OutputContract {
    OutputContract {
        schema_version: SchemaVersion::V1,
        schema: task().data().output_contract.schema.clone(),
        shape: OutputType::String {},
    }
}
pub fn capability_data() -> ModelCapabilityData {
    ModelCapabilityData {
        schema_version: SchemaVersion::V1,
        identity: pin("capability"),
        model: pin("model"),
        provider: pin("provider"),
        task_classes: BTreeSet::from([TaskClass::Extraction]),
        input_contracts: BTreeSet::from([ReferenceId::new("prompt-ir-v1").unwrap()]),
        output_contracts: BTreeMap::from([(output().schema.id.clone(), output().schema)]),
        context_limit: 4096,
        max_output_tokens: 1024,
        structured_output_modes: BTreeSet::from([
            StructuredOutputMode::JsonSchema,
            StructuredOutputMode::ToolProposal,
        ]),
        tool_calling: true,
        reasoning_strategies: BTreeSet::from([ReferenceId::new("bounded").unwrap()]),
        provenance: pin("qualification"),
    }
}
pub fn capability() -> ModelCapability {
    ModelCapability::try_from(capability_data()).unwrap()
}
pub fn invocation_data() -> ModelInvocationData {
    let task = task();
    ModelInvocationData {
        schema_version: SchemaVersion::V1,
        invocation_id: ReferenceId::new("invocation-1").unwrap(),
        task: ResolvedReference {
            id: task.data().id.clone(),
            scope: task.data().target.reference.scope.clone(),
            contract: ReferenceId::new("semantic-task-ir").unwrap(),
            contract_version: SchemaVersion::V1,
            revision: ReferenceId::new("7").unwrap(),
            digest: task.content_digest().unwrap(),
        },
        compiled_context: pin("compiled-context"),
        capability: pin("capability"),
        task_class: TaskClass::Extraction,
        input_contract: ReferenceId::new("prompt-ir-v1").unwrap(),
        output: output(),
        strategy: ReferenceId::new("bounded").unwrap(),
        permitted_capabilities: BTreeMap::new(),
        generation: profile(),
        input_tokens: 1000,
        timeout_ms: 10000,
        max_attempts: 2,
        sensitivity: SensitivityClass::Internal,
        permitted_localities: BTreeSet::from([PrivacyBoundary::OnDevice]),
        authority: pin("policy-process-admission"),
        provenance: pin("trace"),
    }
}
pub fn admit(data: ModelInvocationData) -> Result<ModelInvocation, ModelContractError> {
    ModelInvocation::new(
        data,
        ModelInvocationBasis {
            task: &task(),
            output: &output(),
            compiled_context: &pin("compiled-context"),
            permitted_capabilities: &BTreeMap::new(),
            sensitivity: SensitivityClass::Internal,
            permitted_localities: &BTreeSet::from([PrivacyBoundary::OnDevice]),
            authority: &pin("policy-process-admission"),
        },
    )
}
pub fn runtime() -> ModelRuntimeStatus {
    ModelRuntimeStatus {
        provider: pin("provider"),
        availability: ModelAvailability::Available,
        observation: pin("health"),
    }
}
