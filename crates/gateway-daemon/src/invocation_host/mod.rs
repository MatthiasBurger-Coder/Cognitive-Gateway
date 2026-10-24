//! Operator-owned invocation composition. This is a capability host, not a task
//! session coordinator: CG-14/EPIC-08 retain planning and lifecycle ownership.
mod assembly;
mod journal;

use crate::model_adapters::{AdapterKind, AdapterRenderer, HttpModelAdapter, RuntimeModelBinding};
use gateway_application::{
    codex::RequestContext,
    grounding::GroundingEvidencePort,
    invocation_qualification::{replay_output, trace_output},
    invocation_resilience::{InvocationAttemptResult, InvocationRun},
    model_invocation::PreparedModelInvocation,
    output_verification::{ConstraintReport, OutputVerificationPort, ParsedOutput, verify_output},
    prompt_ir::{InvocationAdmissionPort, InvocationIr, admit_invocation_ir, render_invocation_ir},
    prompt_rendering::{TokenCountRequest, TokenizerPort},
};
use gateway_context::PlannedContextHandoff;
use gateway_domain::{
    grounding::*, model_invocation::*, output_verification::*, prompt_ir::*, prompt_rendering::*,
    semantic_output::*, *,
};
use gateway_policy::{
    Approval, PolicyAuthority, PolicyDecision, PolicyEngine, ProcessReadiness, StepFacts,
    StepPolicyInput,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, &'static str>;
fn input<T, E>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|_| "CG_INVALID_INPUT")
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
fn read(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    input(std::fs::File::open(path).map(|f| f.take(1_048_577)))?
        .read_to_end(&mut bytes)
        .map_err(|_| "CG_UNAVAILABLE")?;
    if bytes.len() > 1_048_576 {
        return Err("CG_LIMIT_EXCEEDED");
    }
    Ok(bytes)
}

/// Explicit selection of a built-in, versioned instruction catalog role.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RegisteredRole {
    FileRequestExtractionV1,
}

/// Trusted operator configuration; never accepted from model/client requests.
/// The operator protects this file, source/oracle files and journal directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostAdmission {
    pub schema_version: SchemaVersion,
    pub registered_role: Option<RegisteredRole>,
    pub principal: ReferenceId,
    pub scope: ContextScopeId,
    pub budget_group: ReferenceId,
    pub enabled: bool,
    pub expires_at_ms: u64,
    pub source_file: PathBuf,
    pub oracle_file: PathBuf,
    pub oracle_digest: ContentDigest,
    pub journal_directory: PathBuf,
    pub task: SemanticTaskIR,
    pub projection: ExecutionContextIR,
    pub output: OutputContract,
    pub provider: ModelProvider,
    pub capability: ModelCapability,
    pub runtime_status: ModelRuntimeStatus,
    pub generation: GenerationProfile,
    pub tokenizer_id: ReferenceId,
    pub tokenizer_version: ReferenceId,
    pub adapter: String,
    pub endpoint: String,
    pub runtime_model: Option<String>,
    pub credential_env: Option<String>,
    pub sensitivity: SensitivityClass,
    pub limits: gateway_domain::invocation_resilience::InvocationRunLimits,
    pub cost_per_attempt_microunits: u64,
    pub deterministic_sufficient: bool,
}

pub struct InvocationHost {
    path: PathBuf,
    digest: ContentDigest,
    admission: HostAdmission,
    source: Vec<u8>,
    expected: Value,
    handoff: PlannedContextHandoff,
    prepared: PreparedModelInvocation,
}
impl InvocationHost {
    pub fn load(path: &Path, principal: &str, invocation: &str) -> Result<Self> {
        let bytes = read(path)?;
        let a: HostAdmission = input(serde_json::from_slice(&bytes))?;
        if a.schema_version != SchemaVersion::V1
            || a.principal.as_str() != principal
            || a.scope != a.task.data().target.reference.scope
            || !a.enabled
            || now() >= a.expires_at_ms
        {
            return Err("CG_AUTHORITY_DENIED");
        }
        input(a.limits.validate())?;
        for path in [&a.source_file, &a.oracle_file, &a.journal_directory] {
            if !path.is_absolute() {
                return Err("CG_INVALID_INPUT");
            }
        }
        if a.sensitivity > SensitivityClass::Internal
            || !matches!(
                a.task.data().task_type,
                SemanticTaskType::Inspect | SemanticTaskType::Analyze
            )
            || a.task.data().process.is_some()
            || !a.task.data().constraints.is_empty()
            || !a.projection.approved_capability_ids().is_empty()
            || !a.projection.constraints().is_empty()
            || a.projection.state().workflow() != WorkflowState::Running
            || a.projection.state().blocker() != BlockerState::Clear
            || a.output.schema != a.task.data().output_contract.schema
        {
            return Err("CG_UNSUPPORTED_CAPABILITY");
        }
        // Schema identity authenticates its actual definition, not a label.
        if a.output.schema.digest
            != context_content_digest(&input(serde_json::to_vec(&a.output.shape))?)
        {
            return Err("CG_STALE_REVISION");
        }
        let source = read(&a.source_file)?;
        let oracle = read(&a.oracle_file)?;
        if context_content_digest(&source) != a.task.data().target.reference.digest
            || context_content_digest(&oracle) != a.oracle_digest
        {
            return Err("CG_STALE_REVISION");
        }
        let expected: Value = input(serde_json::from_slice(&oracle))?;
        if !a.output.shape.accepts(&expected) {
            return Err("CG_INVALID_INPUT");
        }
        validate_registered_role(&a, &expected)?;
        let (handoff, prepared) = assembly::assemble(&a, &source, invocation)?;
        let host = Self {
            path: path.into(),
            digest: context_content_digest(&bytes),
            admission: a,
            source,
            expected,
            handoff,
            prepared,
        };
        host.policy()?;
        host.handoff
            .task()
            .validate_references(&host)
            .map_err(|_| "CG_REFERENCE_UNAVAILABLE")?;
        Ok(host)
    }
    fn policy(&self) -> Result<()> {
        let a = &self.admission;
        let id = input(CapabilityId::new("model.invoke"))?;
        let capability = CapabilityDefinition::new(id.clone(), CapabilityClass::Inspect);
        let capabilities = BTreeMap::from([(id.clone(), capability)]);
        let authority = PolicyAuthority {
            policies: vec![input(PolicyDefinition::new(
                a.projection.policy_id().clone(),
                "Operator-admitted bounded inference",
                [id.clone()],
            ))?],
            capabilities: capabilities.clone(),
            ..Default::default()
        };
        let facts = StepFacts {
            authorizations: [(
                id,
                if a.enabled {
                    Approval::Granted
                } else {
                    Approval::Denied
                },
            )]
            .into(),
            prerequisites_satisfied: true,
            ..Default::default()
        };
        let empty = BTreeSet::new();
        let report = PolicyEngine::evaluate(
            &authority,
            &StepPolicyInput {
                step: &input(PlanStepId::new("model-invocation"))?,
                capabilities: &capabilities,
                operating_mode: a.projection.operating_mode(),
                execution_profile: a.projection.execution_profile(),
                process: ProcessReadiness::NotApplicable,
                resolved: true,
                has_prerequisites: false,
                constraints: &empty,
                preconditions: &empty,
                facts: &facts,
            },
        );
        if report.decision != PolicyDecision::Allow {
            return Err("CG_POLICY_DENIED");
        }
        Ok(())
    }
    fn current(&self) -> Result<()> {
        if !self.admission.enabled
            || now() >= self.admission.expires_at_ms
            || context_content_digest(&read(&self.path)?) != self.digest
        {
            return Err("CG_AUTHORITY_DENIED");
        }
        if read(&self.admission.source_file)? != self.source
            || context_content_digest(&read(&self.admission.oracle_file)?)
                != self.admission.oracle_digest
        {
            return Err("CG_STALE_REVISION");
        }
        self.policy()
    }
    pub fn admitted_invocation(&self) -> Result<InvocationIr> {
        admit_invocation_ir(
            SchemaVersion::V1,
            self.handoff.clone(),
            self.prepared.clone(),
            BTreeMap::new(),
            self,
        )
        .map_err(|_| "CG_AUTHORITY_DENIED")
    }
    pub fn execute(&self, operation: &str) -> Result<Value> {
        let started = Instant::now();
        self.current()?;
        if operation == "cancel" {
            return journal::cancel(self);
        }
        let mut journal = journal::Journal::open(self)?;
        if operation == "inspect" {
            return Ok(journal.summary());
        }
        let ir = self.admitted_invocation()?;
        let invocation = ir.prepared().invocation().data().invocation_id.as_str();
        if operation == "replay" {
            let saved = journal.record(invocation)?;
            let raw: Vec<u8> = input(serde_json::from_value(saved["raw"].clone()))?;
            let binding = input(serde_json::from_value(saved["trace"]["replay"].clone()))?;
            return input(serde_json::to_value(replay_output(
                &ir, &binding, &raw, self,
            )));
        }
        if operation != "invoke" {
            return Err("CG_INVALID_INPUT");
        }
        journal.unused(invocation)?;
        if self.admission.deterministic_sufficient {
            let receipt = json!({"status":"VERIFIED", "mode":"deterministic", "result":self.expected, "model_calls":0});
            journal.finish(invocation, receipt.clone())?;
            return Ok(receipt);
        }
        let kind = self.kind()?;
        let binding = self
            .admission
            .runtime_model
            .as_ref()
            .map(|runtime_id| RuntimeModelBinding {
                canonical: self.admission.capability.data().model.clone(),
                runtime_id: runtime_id.clone(),
            });
        let renderer = input(if self.admission.registered_role.is_some() {
            AdapterRenderer::for_registered_role(kind, self.prepared.clone(), binding.clone())
        } else {
            AdapterRenderer::for_host(kind, self.prepared.clone(), binding.clone())
        })?;
        let (renderer_id, template_version) =
            gateway_application::prompt_rendering::PromptRendererPort::identity(&renderer);
        let profile = TokenizerProfile {
            schema_version: SchemaVersion::V1,
            model: self.admission.capability.data().model.clone(),
            generation_profile: self.admission.generation.content_digest(),
            tokenizer: self.admission.tokenizer_id.clone(),
            tokenizer_version: self.admission.tokenizer_version.clone(),
            renderer: renderer_id,
            template_version,
        };
        let rendered = render_invocation_ir(&ir, self, profile, &renderer, self)
            .map_err(|_| "CG_RENDERING_FAILED")?;
        let credential = self
            .admission
            .credential_env
            .as_ref()
            .map(|name| std::env::var(name).map_err(|_| "CG_CREDENTIAL_UNAVAILABLE"))
            .transpose()?;
        let adapter = input(if self.admission.registered_role.is_some() {
            HttpModelAdapter::for_registered_role(
                kind,
                self.admission.endpoint.clone(),
                credential,
                binding,
            )
        } else {
            HttpModelAdapter::for_host(kind, self.admission.endpoint.clone(), credential, binding)
        })?;
        let limits = journal.remaining()?;
        let context = RequestContext::new(
            invocation.into(),
            Instant::now() + Duration::from_millis(journal.remaining_ms()?),
        );
        let mut run = input(InvocationRun::new(ir.clone(), limits, context.clone()))?;
        let mut result = Err("CG_RETRY_EXHAUSTED");
        // CG-14 owns task lifecycle. This explicit operator call performs only a
        // bounded generation/repair chain and never dispatches tools or actions.
        for _ in 0..self.admission.limits.max_attempts {
            let port = journal.transport(&adapter, invocation);
            match run.dispatch(
                &ir,
                &rendered,
                gateway_application::closed_loop::LoopDecision::Continue,
                self.admission.cost_per_attempt_microunits,
                self,
                &port,
            ) {
                Ok(InvocationAttemptResult::Parsed(parsed)) => {
                    let parsed_record = input(serde_json::to_value(parsed.data()))?;
                    let verified = verify_output(&ir, *parsed, self, vec![]);
                    result = match verified {
                        Ok(verified) => Ok(
                            json!({"status":"VERIFIED", "result":verified.output().data().result,"verification":verified.record()}),
                        ),
                        Err(record) => Ok(json!({"status":"REJECTED", "verification":record})),
                    };
                    if let Ok(record) = &mut result {
                        record["parsed"] = parsed_record;
                    }
                    break;
                }
                Ok(InvocationAttemptResult::Malformed(_))
                | Ok(InvocationAttemptResult::TransientFailure) => (),
                Err(_) => {
                    result = Err("CG_INVOCATION_STOPPED");
                    break;
                }
            }
        }
        let mut record = result.unwrap_or_else(|code| json!({"status":"FAILED","diagnostic":code}));
        if self.admission.registered_role.is_some() {
            record["instruction_profile"] = input(serde_json::to_value(
                crate::prompt_renderers::file_request_instruction_profile(&self.admission.scope),
            ))?;
        }
        record["elapsed_ms"] = json!(started.elapsed().as_millis());
        record["provider_resources"] =
            json!({"status":"NOT_REPORTED", "cpu_ms":null, "ram_bytes":null});
        record["usage"] = input(serde_json::to_value(run.records()))?;
        record["adapter"] = input(serde_json::to_value(adapter.observation()))?;
        if let Some(raw) = journal.raw() {
            record["trace"] = input(serde_json::to_value(input(trace_output(
                &ir, &rendered, &raw,
            ))?))?;
            record["raw"] = json!(raw);
        }
        let mut retained = record.clone();
        retained["ir"] = input(serde_json::to_value(&ir))?;
        retained["rendered"] = input(serde_json::to_value(&rendered))?;
        journal.finish(invocation, retained)?;
        record["budget"] = journal.summary();
        // Raw bytes are retained in the operator-controlled journal, not stdout.
        record.as_object_mut().expect("record object").remove("raw");
        Ok(record)
    }
    fn kind(&self) -> Result<AdapterKind> {
        match self.admission.adapter.as_str() {
            "local" => Ok(AdapterKind::LocalServiceV1),
            "chat-completions" => Ok(AdapterKind::ChatCompletionsV1),
            "chat-completions-legacy" => Ok(AdapterKind::ChatCompletionsLegacyV1),
            _ => Err("CG_UNSUPPORTED_CAPABILITY"),
        }
    }
}

impl SemanticReferenceValidator for InvocationHost {
    fn validate(
        &self,
        _: &SemanticTaskData,
        b: &SemanticReferenceBinding,
    ) -> std::result::Result<(), ValidationError> {
        let a = &self.admission;
        let pins = [&a.task.data().target.reference, &a.output.schema];
        if self.current().is_ok()
            && pins.iter().any(|p| {
                p.id.as_str() == b.id
                    && p.scope == b.scope
                    && p.contract == b.contract
                    && p.contract_version == b.contract_version
                    && p.revision == b.revision
                    && p.digest == b.digest
            })
        {
            Ok(())
        } else {
            Err(ValidationError::InvalidStateCombination {
                reason: "unavailable current host reference",
            })
        }
    }
}
impl InvocationAdmissionPort for InvocationHost {
    fn validate_current(&self, ir: &InvocationIr) -> std::result::Result<(), InvocationIrError> {
        self.current().map_err(|_| InvocationIrError::StaleInput)?;
        if ir.prompt().handoff().to_value() != self.handoff.to_value()
            || serde_json::to_value(ir.prepared()).ok() != serde_json::to_value(&self.prepared).ok()
            || !ir.prompt().tools().is_empty()
        {
            return Err(InvocationIrError::InvalidBinding);
        }
        Ok(())
    }
}
impl TokenizerPort for InvocationHost {
    fn count(
        &self,
        r: &TokenCountRequest<'_>,
    ) -> std::result::Result<TokenCountResult, RenderDiagnostic> {
        if r.profile.model != self.admission.capability.data().model
            || r.profile.tokenizer != self.admission.tokenizer_id
            || r.profile.tokenizer_version != self.admission.tokenizer_version
        {
            return Err(RenderDiagnostic::InvalidAccounting);
        }
        // Explicitly qualified UTF-8 byte upper bound, not an exact tokenizer.
        // Include the complete wire body, plus conservative per-message framing.
        let section = |key: &str| {
            serde_json::to_vec(&r.sections.handoff()[key]).map_or(0, |v| v.len() as u64)
        };
        let mut usage = TokenUsage {
            instruction_tokens: section("instructions"),
            task_tokens: section("task"),
            context_tokens: section("context"),
            evidence_tokens: section("evidence"),
            tool_schema_tokens: serde_json::to_vec(r.sections.output_schema())
                .map_or(0, |v| v.len() as u64)
                + serde_json::to_vec(r.sections.tools()).map_or(0, |v| v.len() as u64),
            framing_tokens: 0,
        };
        usage.framing_tokens =
            (r.payload.len() as u64 + 256).saturating_sub(usage.total().unwrap_or(u64::MAX));
        Ok(TokenCountResult {
            profile: r.profile.clone(),
            payload_digest: r.payload_digest.clone(),
            mode: CountMode::Estimated,
            provenance: assembly::pin(
                &self.admission.scope,
                "utf8-wire-byte-upper-bound",
                "1",
                b"UTF8 byte fallback plus 256 chat framing tokens/v1",
            ),
            usage,
        })
    }
}
impl OutputVerificationPort for InvocationHost {
    fn grounding(&self) -> Option<&dyn GroundingEvidencePort> {
        Some(self)
    }
    fn resolve_reference(
        &self,
        _: &InvocationIr,
        _: &OutputReference,
    ) -> std::result::Result<(), VerificationError> {
        Err(VerificationError::UnsupportedReference)
    }
    fn evaluate(
        &self,
        ir: &InvocationIr,
        output: &ParsedOutput,
    ) -> std::result::Result<ConstraintReport, VerificationError> {
        self.current()
            .map_err(|_| VerificationError::StaleContext)?;
        let outcome = if output.data().result == self.expected && output.data().tools.is_empty() {
            CheckOutcome::Passed
        } else {
            CheckOutcome::Failed
        };
        Ok(ConstraintReport {
            binding: output.binding(),
            policy: CheckOutcome::Passed,
            process: CheckOutcome::Passed,
            task_completeness: outcome,
            semantic_output: outcome,
            checks: ir
                .prompt()
                .handoff()
                .task()
                .data()
                .verification_contract
                .checks
                .iter()
                .filter(|c| **c != SemanticVerificationCheck::OutputSchemaValid)
                .map(|c| (*c, outcome))
                .collect(),
        })
    }
}
impl GroundingEvidencePort for InvocationHost {
    fn manifest(
        &self,
        _: &InvocationIr,
        output: &ParsedOutput,
    ) -> std::result::Result<GroundingManifest, GroundingError> {
        assembly::manifest(self, output).map_err(|_| GroundingError::UnsupportedClaim)
    }
    fn resolve(
        &self,
        _: &InvocationIr,
        citation: &ResolvedReference<EvidenceId>,
    ) -> std::result::Result<GroundingEvidence, GroundingError> {
        self.current().map_err(|_| GroundingError::StaleEvidence)?;
        assembly::evidence(self, citation).map_err(|_| GroundingError::InvalidCitation)
    }
}

fn validate_registered_role(a: &HostAdmission, expected: &Value) -> Result<()> {
    if a.registered_role.is_none() {
        return Ok(());
    }
    if !crate::prompt_renderers::supports_file_request_definition(&a.task, &a.projection, &a.output)
    {
        return Err("CG_UNSUPPORTED_CAPABILITY");
    }
    let action = expected["action"].as_str().ok_or("CG_INVALID_INPUT")?;
    let target = expected["target"].as_str().ok_or("CG_INVALID_INPUT")?;
    if !matches!(action, "read" | "write" | "unknown")
        || expected["ambiguous"].as_bool() != Some(action == "unknown" || target.is_empty())
    {
        return Err("CG_INVALID_INPUT");
    }
    Ok(())
}
