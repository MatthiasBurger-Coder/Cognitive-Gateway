//! Provider-neutral invocation boundary for trusted connector sessions.
//!
//! This is intentionally separate from the standalone `InvocationHost`, whose
//! result-only oracle behavior must not acquire connector capabilities. A
//! session host supplies the already-admitted task handoff, prepared model
//! invocation and exact current-inventory tool definitions. Provider calls are
//! converted to the canonical proposal envelope and remain unverified until
//! the existing application `verify_output` gate accepts them.

use gateway_application::{
    codex::RequestContext,
    invocation_resilience::{
        BoundedInvocationPort, BoundedInvocationRequest, InvocationAttemptResponse,
        InvocationAttemptResult, InvocationRun,
    },
    model_invocation::PreparedModelInvocation,
    output_verification::{
        ConstraintReport, OutputVerificationPort, ParsedOutput, VerifiedOutput, verify_output,
    },
    prompt_ir::{InvocationIr, admit_invocation_ir, render_invocation_ir},
    prompt_rendering::{PromptRendererPort, TokenizerPort, ToolDefinition},
    sessions::{
        SessionError, SessionResourceAllocation, SessionResourceAmounts,
        SessionResourceAuthorityPort, SessionResourcePhase,
    },
};
use gateway_context::PlannedContextHandoff;
use gateway_domain::{
    CapabilityId, SchemaVersion, context_content_digest, invocation_resilience::*,
    output_verification::*, prompt_ir::InvocationIrError, prompt_rendering::TokenizerProfile,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const MAX_SESSION_TOOL_CALLS: usize = 32;

/// An invocation which was admitted from current trusted session state. It can
/// only be created by supplying host-owned producer artifacts and an exact set
/// of connector inventory-pinned tool definitions.
#[derive(Debug)]
pub struct SessionToolInvocation {
    invocation: InvocationIr,
}

pub struct SessionToolExecution<'a> {
    pub host: &'a dyn OutputVerificationPort,
    pub profile: TokenizerProfile,
    pub renderer: &'a dyn PromptRendererPort,
    pub tokenizer: &'a dyn TokenizerPort,
    pub limits: InvocationRunLimits,
    pub context: RequestContext,
    pub reserved_cost_microunits: u64,
    pub transport: &'a dyn BoundedInvocationPort,
}

/// One model allocation from one persisted preparation claim. The allocation
/// cannot be cloned or decoded, and every attempt shares this same run.
pub struct SessionToolRun {
    run: InvocationRun,
    allocation: SessionResourceAllocation,
    conservative_usage: SessionResourceAmounts,
    output_token_cap: u64,
    cost_cap: u64,
}

impl SessionToolRun {
    pub fn new(
        original: InvocationIr,
        allocation: SessionResourceAllocation,
        limits: InvocationRunLimits,
        mut context: RequestContext,
        cost_unit: &gateway_domain::ReferenceId,
    ) -> Result<Self, SessionToolInvocationError> {
        use InvocationRunDiagnostic as D;
        let amounts = allocation.amounts();
        if allocation.phase() != SessionResourcePhase::Preparation
            || allocation.cost_unit() != cost_unit
            || allocation.binding().scope != original.prepared().invocation().data().task.scope
            || amounts.connector_calls != 0
            || amounts.retrieval_rounds != 0
            || amounts.retrieval_results != 0
            || amounts.retrieval_tokens != 0
            || amounts.repair_attempts != 0
            || amounts.worker_attempts != 0
            || amounts.worker_compute_units != 0
        {
            return Err(SessionToolInvocationError::Run(D::InvalidBinding));
        }
        let denial = if u64::from(limits.max_attempts) > amounts.model_attempts {
            Some(D::AttemptBudgetExceeded)
        } else if limits.run_input_tokens > amounts.input_tokens
            || limits.invocation_input_tokens > amounts.input_tokens
        {
            Some(D::InputTokenBudgetExceeded)
        } else if limits.run_output_tokens > amounts.output_tokens
            || limits.invocation_output_tokens > amounts.output_tokens
        {
            Some(D::OutputTokenBudgetExceeded)
        } else if limits.cost_microunits > amounts.cost_microunits {
            Some(D::CostBudgetExceeded)
        } else if limits.output_bytes > amounts.output_bytes {
            Some(D::OutputBudgetExceeded)
        } else {
            None
        };
        if let Some(error) = denial {
            return Err(SessionToolInvocationError::Run(error));
        }
        context.deadline = context.deadline.min(allocation.deadline());
        let output_token_cap = limits.run_output_tokens;
        let cost_cap = limits.cost_microunits;
        Ok(Self {
            run: InvocationRun::new(original, limits, context)
                .map_err(SessionToolInvocationError::Run)?,
            allocation,
            conservative_usage: SessionResourceAmounts::default(),
            output_token_cap,
            cost_cap,
        })
    }

    pub fn usage(&self) -> &InvocationRunUsage {
        self.run.usage()
    }
    pub fn records(&self) -> &[AttemptUsageRecord] {
        self.run.records()
    }
    /// Smaller provider reports do not replenish the admitted output/cost slice.
    pub fn conservative_usage(&self) -> &SessionResourceAmounts {
        &self.conservative_usage
    }
    pub fn reservation_id(&self) -> &gateway_domain::ContentDigest {
        self.allocation.reservation_id()
    }
    /// Validate the retained run's actual private child around a pure binding
    /// operation. This grants no transport call and exposes no allocation.
    pub(crate) fn validate_current(
        &self,
        authority: &dyn SessionResourceAuthorityPort,
        ir: &InvocationIr,
    ) -> Result<(), SessionToolInvocationError> {
        self.run
            .check_current()
            .map_err(SessionToolInvocationError::Run)?;
        authority
            .validate_allocation(&self.allocation)
            .map_err(SessionToolInvocationError::Resource)?;
        authority
            .validate_invocation_allocation(&self.allocation, ir)
            .map_err(SessionToolInvocationError::Resource)?;
        self.run
            .check_current()
            .map_err(SessionToolInvocationError::Run)
    }
}

pub struct SessionToolAttemptExecution<'a> {
    pub host: &'a dyn OutputVerificationPort,
    pub authority: &'a dyn SessionResourceAuthorityPort,
    pub profile: TokenizerProfile,
    pub renderer: &'a dyn PromptRendererPort,
    pub tokenizer: &'a dyn TokenizerPort,
    pub reserved_cost_microunits: u64,
    pub cost_unit: &'a gateway_domain::ReferenceId,
    pub transport: &'a dyn BoundedInvocationPort,
    pub decision: gateway_application::closed_loop::LoopDecision,
}

struct ClaimAdmission<'a> {
    host: &'a dyn OutputVerificationPort,
    authority: &'a dyn SessionResourceAuthorityPort,
    allocation: &'a SessionResourceAllocation,
    count: &'a gateway_domain::prompt_rendering::TokenCountResult,
    payload: &'a str,
}

impl gateway_domain::semantic_task::SemanticReferenceValidator for ClaimAdmission<'_> {
    fn validate(
        &self,
        task: &gateway_domain::SemanticTaskData,
        binding: &gateway_domain::SemanticReferenceBinding,
    ) -> Result<(), gateway_domain::ValidationError> {
        self.host.validate(task, binding)
    }
}
impl gateway_application::prompt_ir::InvocationAdmissionPort for ClaimAdmission<'_> {
    fn validate_current(&self, ir: &InvocationIr) -> Result<(), InvocationIrError> {
        self.host.validate_current(ir)?;
        self.authority
            .validate_allocation(self.allocation)
            .map_err(|_| InvocationIrError::InvalidBinding)?;
        self.authority
            .validate_invocation_allocation(self.allocation, ir)
            .map_err(|_| InvocationIrError::InvalidBinding)?;
        if self.count.mode == gateway_domain::prompt_rendering::CountMode::Estimated {
            self.authority
                .validate_token_bound(self.allocation, self.count, self.payload)
                .map_err(|_| InvocationIrError::InvalidBinding)?;
        }
        Ok(())
    }
}

impl SessionToolInvocation {
    pub fn admit(
        handoff: PlannedContextHandoff,
        prepared: PreparedModelInvocation,
        tools: BTreeMap<CapabilityId, ToolDefinition>,
        host: &dyn OutputVerificationPort,
    ) -> std::result::Result<Self, InvocationIrError> {
        validate_connector_tools(&prepared, &tools)?;
        let invocation = admit_invocation_ir(SchemaVersion::V1, handoff, prepared, tools, host)?;
        host.validate_current(&invocation)?;
        Ok(Self { invocation })
    }

    pub fn invocation(&self) -> &InvocationIr {
        &self.invocation
    }

    /// Attempt the current admitted route using the same durable preparation
    /// allocation and mutable invocation ledger across retry/fallback.
    pub fn invoke_in(
        &self,
        run: &mut SessionToolRun,
        execution: SessionToolAttemptExecution<'_>,
    ) -> Result<VerifiedOutput, SessionToolInvocationError> {
        use InvocationRunDiagnostic as D;
        let SessionToolAttemptExecution {
            host,
            authority,
            profile,
            renderer,
            tokenizer,
            reserved_cost_microunits,
            cost_unit,
            transport,
            decision,
        } = execution;
        if let Some(error) = run.run.terminal() {
            return Err(SessionToolInvocationError::Run(error));
        }
        run.run
            .check_current()
            .map_err(SessionToolInvocationError::Run)?;
        authority
            .validate_allocation(&run.allocation)
            .map_err(SessionToolInvocationError::Resource)?;
        authority
            .validate_invocation_allocation(&run.allocation, &self.invocation)
            .map_err(SessionToolInvocationError::Resource)?;
        if cost_unit != run.allocation.cost_unit() {
            return Err(SessionToolInvocationError::Run(D::InvalidBinding));
        }
        let rendered = render_invocation_ir(&self.invocation, host, profile, renderer, tokenizer)
            .map_err(SessionToolInvocationError::Rendering)?;
        let accounting = rendered.rendered().accounting();
        // Estimated counts require the trusted source to qualify the actual
        // method, profile, provenance and payload bound for this allocation.
        if accounting.count.mode != gateway_domain::prompt_rendering::CountMode::Exact {
            authority
                .validate_token_bound(
                    &run.allocation,
                    &accounting.count,
                    rendered.rendered().payload(),
                )
                .map_err(SessionToolInvocationError::Resource)?;
        }
        let output = run
            .conservative_usage
            .output_tokens
            .checked_add(accounting.reserved_output_tokens)
            .filter(|total| *total <= run.output_token_cap)
            .ok_or(SessionToolInvocationError::Run(
                D::OutputTokenBudgetExceeded,
            ))?;
        let cost = run
            .conservative_usage
            .cost_microunits
            .checked_add(reserved_cost_microunits)
            .filter(|total| *total <= run.cost_cap)
            .ok_or(SessionToolInvocationError::Run(D::CostBudgetExceeded))?;
        let before = run.run.usage().attempts;
        let admission = ClaimAdmission {
            host,
            authority,
            allocation: &run.allocation,
            count: &accounting.count,
            payload: rendered.rendered().payload(),
        };
        let result = run.run.dispatch(
            &self.invocation,
            &rendered,
            decision,
            reserved_cost_microunits,
            &admission,
            transport,
        );
        if run.run.usage().attempts != before {
            run.conservative_usage.model_attempts = u64::from(run.run.usage().attempts);
            run.conservative_usage.input_tokens = run.run.usage().input_tokens;
            run.conservative_usage.output_tokens = output;
            run.conservative_usage.cost_microunits = cost;
            run.conservative_usage.output_bytes = run.run.usage().output_bytes;
        }
        let result = result.map_err(SessionToolInvocationError::Run)?;
        authority
            .validate_allocation(&run.allocation)
            .map_err(SessionToolInvocationError::Resource)?;
        match result {
            InvocationAttemptResult::Parsed(output) => {
                let verified = verify_output(&self.invocation, *output, host, vec![])
                    .map_err(SessionToolInvocationError::Rejected)?;
                authority
                    .validate_allocation(&run.allocation)
                    .map_err(SessionToolInvocationError::Resource)?;
                authority
                    .validate_invocation_allocation(&run.allocation, &self.invocation)
                    .map_err(SessionToolInvocationError::Resource)?;
                run.run
                    .check_current()
                    .map_err(SessionToolInvocationError::Run)?;
                Ok(verified)
            }
            InvocationAttemptResult::Malformed(error) => {
                Err(SessionToolInvocationError::Malformed(error))
            }
            InvocationAttemptResult::TransientFailure => {
                Err(SessionToolInvocationError::ProviderUnavailable)
            }
        }
    }

    /// Render, account and dispatch one bounded model attempt, then run the
    /// existing application verification gate. No result is returned as
    /// dispatchable unless the exact current IR yields `VerifiedOutput`.
    pub fn invoke(
        &self,
        execution: SessionToolExecution<'_>,
    ) -> Result<VerifiedOutput, SessionToolInvocationError> {
        let SessionToolExecution {
            host,
            profile,
            renderer,
            tokenizer,
            limits,
            context,
            reserved_cost_microunits,
            transport,
        } = execution;
        let rendered = render_invocation_ir(&self.invocation, host, profile, renderer, tokenizer)
            .map_err(SessionToolInvocationError::Rendering)?;
        let mut run = InvocationRun::new(self.invocation.clone(), limits, context)
            .map_err(SessionToolInvocationError::Run)?;
        match run
            .dispatch(
                &self.invocation,
                &rendered,
                gateway_application::closed_loop::LoopDecision::Continue,
                reserved_cost_microunits,
                host,
                transport,
            )
            .map_err(SessionToolInvocationError::Run)?
        {
            InvocationAttemptResult::Parsed(output) => {
                verify_output(&self.invocation, *output, host, vec![])
                    .map_err(SessionToolInvocationError::Rejected)
            }
            InvocationAttemptResult::Malformed(error) => {
                Err(SessionToolInvocationError::Malformed(error))
            }
            InvocationAttemptResult::TransientFailure => {
                Err(SessionToolInvocationError::ProviderUnavailable)
            }
        }
    }
}

fn validate_connector_tools(
    prepared: &PreparedModelInvocation,
    tools: &BTreeMap<CapabilityId, ToolDefinition>,
) -> Result<(), InvocationIrError> {
    use InvocationIrError as E;
    let task = &prepared.invocation().data().task;
    let admitted = &prepared.invocation().data().permitted_capabilities;
    if tools.is_empty() || tools.len() != admitted.len() {
        return Err(E::InvalidBinding);
    }
    let mut inventory: Option<(&str, &gateway_domain::ContentDigest)> = None;
    for (id, tool) in tools {
        let pin = tool.connector_inventory.as_ref().ok_or(E::InvalidBinding)?;
        if id != &tool.binding.id
            || id.as_str() != tool.binding.id.as_str()
            || tool.binding.scope != task.scope
            || pin.scope != task.scope
            || admitted.get(id) != Some(&tool.binding)
        {
            return Err(E::InvalidBinding);
        }
        let schema_digest = context_content_digest(
            &serde_json::to_vec(&tool.input.json_schema()).map_err(|_| E::InvalidBinding)?,
        );
        if schema_digest != pin.argument_schema_digest {
            return Err(E::InvalidBinding);
        }
        if let Some((connector, revision)) = inventory {
            if connector != pin.connector.as_str() || revision != &pin.revision {
                return Err(E::InvalidBinding);
            }
        } else {
            inventory = Some((pin.connector.as_str(), &pin.revision));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFinish {
    Complete,
    Refused,
    Incomplete,
}

/// Native provider tool call before canonical capability binding. `name` must
/// exactly equal the admitted `CapabilityId`; no fuzzy aliases are accepted.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderToolCall {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderToolCompletion {
    pub model_id: String,
    pub finish: ProviderFinish,
    pub result: Value,
    pub calls: Vec<ProviderToolCall>,
    pub usage: Option<ProviderTokenUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderTokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderToolFailure {
    Transient,
    Hard,
}

/// Provider transport seam for a session-only tool invocation. An eventual
/// provider adapter must honor deadline/cancellation and output bounds in the
/// request. It receives no connector runtime or dispatch authority.
pub trait SessionToolProviderPort: Send + Sync {
    fn complete(
        &self,
        request: BoundedInvocationRequest<'_>,
    ) -> std::result::Result<ProviderToolCompletion, ProviderToolFailure>;
}

/// Session state that must still be current for this exact admitted invocation.
/// Production composition binds this to the owning session/step/run/inventory
/// journal; the provider adapter has no way to manufacture or refresh it.
pub trait SessionToolAuthorityPort: Send + Sync {
    fn validate_semantic_reference(
        &self,
        task: &gateway_domain::SemanticTaskData,
        binding: &gateway_domain::SemanticReferenceBinding,
    ) -> Result<(), gateway_domain::ValidationError>;
    fn validate_session_invocation(&self, ir: &InvocationIr) -> Result<(), InvocationIrError>;
    fn resolve_reference(
        &self,
        ir: &InvocationIr,
        reference: &OutputReference,
    ) -> Result<(), VerificationError>;
    fn evaluate_output(
        &self,
        ir: &InvocationIr,
        output: &ParsedOutput,
    ) -> Result<ConstraintReport, VerificationError>;
}

pub trait SessionToolSemanticReferencePort: Send + Sync {
    fn validate_reference(
        &self,
        task: &gateway_domain::SemanticTaskData,
        binding: &gateway_domain::SemanticReferenceBinding,
    ) -> Result<(), gateway_domain::ValidationError>;
}

impl SessionToolSemanticReferencePort for NoSemanticReferencePort {
    fn validate_reference(
        &self,
        _: &gateway_domain::SemanticTaskData,
        _: &gateway_domain::SemanticReferenceBinding,
    ) -> Result<(), gateway_domain::ValidationError> {
        Err(gateway_domain::ValidationError::InvalidStateCombination {
            reason: "semantic reference authority unavailable",
        })
    }
}

/// Composite app verification boundary for session tools. The authority port
/// must bind every call to the current session owner, project, run, revision,
/// step and dispatch, and re-read exact connector inventory pins.
pub struct SessionToolVerificationHost<A, R = NoSemanticReferencePort> {
    authority: Arc<A>,
    references: R,
}

pub struct NoSemanticReferencePort;

impl<A> SessionToolVerificationHost<A, NoSemanticReferencePort> {
    pub fn new(authority: A) -> Self {
        Self {
            authority: Arc::new(authority),
            references: NoSemanticReferencePort,
        }
    }

    pub fn with_references<R>(authority: A, references: R) -> SessionToolVerificationHost<A, R> {
        SessionToolVerificationHost {
            authority: Arc::new(authority),
            references,
        }
    }
}

impl<A: SessionToolAuthorityPort, R: SessionToolSemanticReferencePort>
    gateway_domain::semantic_task::SemanticReferenceValidator
    for SessionToolVerificationHost<A, R>
{
    fn validate(
        &self,
        task: &gateway_domain::SemanticTaskData,
        binding: &gateway_domain::SemanticReferenceBinding,
    ) -> Result<(), gateway_domain::ValidationError> {
        self.references.validate_reference(task, binding)
    }
}

impl<A: SessionToolAuthorityPort, R: SessionToolSemanticReferencePort>
    gateway_application::prompt_ir::InvocationAdmissionPort for SessionToolVerificationHost<A, R>
{
    fn validate_current(&self, ir: &InvocationIr) -> Result<(), InvocationIrError> {
        validate_invocation_tool_pins(ir)?;
        self.authority.validate_session_invocation(ir)
    }
}

impl<A: SessionToolAuthorityPort, R: SessionToolSemanticReferencePort> OutputVerificationPort
    for SessionToolVerificationHost<A, R>
{
    fn resolve_reference(
        &self,
        ir: &InvocationIr,
        reference: &OutputReference,
    ) -> Result<(), VerificationError> {
        self.authority.resolve_reference(ir, reference)
    }

    fn evaluate(
        &self,
        ir: &InvocationIr,
        output: &ParsedOutput,
    ) -> Result<ConstraintReport, VerificationError> {
        self.authority.evaluate_output(ir, output)
    }
}

/// Converts a typed provider completion to strict `OutputProposalData` bytes.
/// The adapter never executes the returned tools.
pub struct SessionToolProviderAdapter<P> {
    provider: P,
}

impl<P> SessionToolProviderAdapter<P> {
    pub fn new(provider: P) -> Self {
        Self { provider }
    }
}

impl<P: SessionToolProviderPort> BoundedInvocationPort for SessionToolProviderAdapter<P> {
    fn invoke(&self, request: BoundedInvocationRequest<'_>) -> InvocationAttemptResponse {
        let failure = |failure| InvocationAttemptResponse {
            output: Err(failure),
            usage: None,
        };
        if request.context.check().is_err()
            || request.ir.identity() != request.binding.invocation
            || request.rendered.identity() != &request.ir.identity()
        {
            return failure(InvocationAttemptFailure::Hard);
        }
        if validate_invocation_tool_pins(request.ir).is_err() {
            return failure(InvocationAttemptFailure::Hard);
        }
        let completion = match self.provider.complete(BoundedInvocationRequest {
            ir: request.ir,
            rendered: request.rendered,
            binding: request.binding,
            context: request.context,
            max_output_bytes: request.max_output_bytes,
            reserved_cost_microunits: request.reserved_cost_microunits,
        }) {
            Ok(completion) => completion,
            Err(ProviderToolFailure::Transient) => {
                return failure(InvocationAttemptFailure::Transient);
            }
            Err(ProviderToolFailure::Hard) => return failure(InvocationAttemptFailure::Hard),
        };
        if request.context.check().is_err() {
            return failure(InvocationAttemptFailure::Transient);
        }
        let expected_model = request.ir.prepared().capability().data().model.id.as_str();
        if completion.model_id != expected_model
            || completion.finish != ProviderFinish::Complete
            || completion.calls.len() > MAX_SESSION_TOOL_CALLS
        {
            return failure(InvocationAttemptFailure::Hard);
        }
        let tools = request.ir.prompt().tools();
        let mut proposals = Vec::with_capacity(completion.calls.len());
        for call in completion.calls {
            let Some(tool) = tools
                .values()
                .find(|tool| tool.binding.id.as_str() == call.name)
            else {
                return failure(InvocationAttemptFailure::Hard);
            };
            proposals.push(json!({
                "capability": tool.binding,
                "arguments": call.arguments,
            }));
        }
        let envelope = json!({
            "schema_version": SchemaVersion::V1,
            "invocation": request.ir.identity(),
            "result": completion.result,
            "references": [],
            "tools": proposals,
        });
        let raw = match serde_json::to_vec(&envelope) {
            Ok(raw) if raw.len() <= request.max_output_bytes => raw,
            _ => return failure(InvocationAttemptFailure::Hard),
        };
        let usage = completion.usage.map(|usage| ProviderUsage {
            binding: request.binding.clone(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            // Cost is an admission-owned upper bound; the provider cannot set
            // or lower the amount charged to this attempt.
            cost_microunits: request.reserved_cost_microunits,
        });
        InvocationAttemptResponse {
            output: Ok(raw),
            usage,
        }
    }
}

pub(crate) fn validate_invocation_tool_pins(ir: &InvocationIr) -> Result<(), InvocationIrError> {
    use InvocationIrError as E;
    let tools = ir.prompt().tools();
    let task_scope = &ir.prepared().invocation().data().task.scope;
    if tools.is_empty()
        || tools.len()
            != ir
                .prepared()
                .invocation()
                .data()
                .permitted_capabilities
                .len()
    {
        return Err(E::InvalidBinding);
    }
    let mut inventory: Option<(&str, &gateway_domain::ContentDigest)> = None;
    for (id, tool) in tools {
        let pin = tool.connector_inventory.as_ref().ok_or(E::InvalidBinding)?;
        if id != &tool.binding.id
            || tool.binding.scope != *task_scope
            || pin.scope != *task_scope
            || ir
                .prepared()
                .invocation()
                .data()
                .permitted_capabilities
                .get(id)
                != Some(&tool.binding)
        {
            return Err(E::InvalidBinding);
        }
        let schema_digest = context_content_digest(
            &serde_json::to_vec(&tool.input.json_schema()).map_err(|_| E::InvalidBinding)?,
        );
        if schema_digest != pin.argument_schema_digest {
            return Err(E::InvalidBinding);
        }
        if let Some((connector, revision)) = inventory {
            if connector != pin.connector.as_str() || revision != &pin.revision {
                return Err(E::InvalidBinding);
            }
        } else {
            inventory = Some((pin.connector.as_str(), &pin.revision));
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum SessionToolInvocationError {
    Resource(SessionError),
    Admission(InvocationIrError),
    Rendering(gateway_application::prompt_ir::InvocationPreparationError),
    Run(InvocationRunDiagnostic),
    ProviderUnavailable,
    Malformed(VerificationError),
    Rejected(Box<VerificationRecord>),
}

impl From<InvocationIrError> for SessionToolInvocationError {
    fn from(error: InvocationIrError) -> Self {
        Self::Admission(error)
    }
}
