//! Bounded dispatch across retries, repair and pre-admitted fallback. One mutable
//! session is owned by the CG-14 host for the entire run; it is not deserializable.
use crate::{
    closed_loop::LoopDecision,
    codex::{FacadeError, RequestContext},
    output_parsing::parse_invocation_output,
    output_verification::ParsedOutput,
    prompt_ir::{InvocationAdmissionPort, InvocationIr, RenderedInvocationIr, validate_current},
};
use InvocationRunDiagnostic as D;
use gateway_domain::{
    invocation_resilience::*, output_parsing::MAX_OUTPUT_BYTES,
    output_verification::VerificationError, prompt_rendering::CountMode,
};
use std::time::{Duration, Instant};

/// One outer provider call. The adapter MUST honor the deadline/cancellation,
/// token reserve and byte ceiling; it has no tool-dispatch authority. Cost is a
/// trusted host upper bound for this exact route, in the run's common currency.
pub struct BoundedInvocationRequest<'a> {
    pub ir: &'a InvocationIr,
    pub rendered: &'a RenderedInvocationIr,
    pub binding: &'a AttemptBinding,
    pub context: &'a RequestContext,
    pub max_output_bytes: usize,
    pub reserved_cost_microunits: u64,
}
pub struct InvocationAttemptResponse {
    pub output: Result<Vec<u8>, InvocationAttemptFailure>,
    pub usage: Option<ProviderUsage>,
}
pub trait BoundedInvocationPort {
    fn invoke(&self, request: BoundedInvocationRequest<'_>) -> InvocationAttemptResponse;
}
#[derive(Debug)]
pub enum InvocationAttemptResult {
    /// Still requires schema/reference/constraint/grounding verification. No tool grant.
    Parsed(Box<ParsedOutput>),
    Malformed(VerificationError),
    TransientFailure,
}

pub struct InvocationRun {
    limits: InvocationRunLimits,
    original: InvocationIr,
    context: RequestContext,
    usage: InvocationRunUsage,
    records: Vec<AttemptUsageRecord>,
    terminal: Option<D>,
}
impl InvocationRun {
    /// Called once by the trusted lifecycle host, never once per retry/provider.
    /// Captures the earlier of the existing request deadline and the original
    /// invocation/run timeout. Cloning the external token still cancels this run.
    pub fn new(
        original: InvocationIr,
        limits: InvocationRunLimits,
        mut context: RequestContext,
    ) -> Result<Self, D> {
        limits.validate()?;
        let d = original.prepared().invocation().data();
        context.deadline = context.deadline.min(
            Instant::now() + Duration::from_millis(u64::from(limits.timeout_ms.min(d.timeout_ms))),
        );
        Ok(Self {
            limits,
            original,
            context,
            usage: InvocationRunUsage::default(),
            records: Vec::new(),
            terminal: None,
        })
    }
    pub fn usage(&self) -> &InvocationRunUsage {
        &self.usage
    }
    pub fn records(&self) -> &[AttemptUsageRecord] {
        &self.records
    }
    pub fn terminal(&self) -> Option<D> {
        self.terminal
    }
    fn check(&self) -> Result<(), D> {
        if let Some(error) = self.terminal {
            return Err(error);
        }
        self.context.check().map_err(|e| match e {
            FacadeError::Cancelled => D::Cancelled,
            _ => D::DeadlineExceeded,
        })
    }
    fn bind(&self, ir: &InvocationIr, rendered: &RenderedInvocationIr) -> Result<(), D> {
        if rendered.identity() != &ir.identity() {
            return Err(D::InvalidBinding);
        }
        // Match CG-26's frozen fallback basis: capability is the sole variation.
        // This pins consent/authority, scope, sensitivity, generation, tools and
        // original budgets. Current admission is still mandatory for each call.
        let mut proposed = ir.prepared().invocation().data().clone();
        proposed.capability = self
            .original
            .prepared()
            .invocation()
            .data()
            .capability
            .clone();
        if &proposed != self.original.prepared().invocation().data()
            || ir.identity().handoff_digest != self.original.identity().handoff_digest
            || ir.prompt().bindings() != self.original.prompt().bindings()
            || ir.prompt().tools() != self.original.prompt().tools()
            || ir.prepared().provider().data().locality
                != self.original.prepared().provider().data().locality
        {
            return Err(D::AuthorityMismatch);
        }
        Ok(())
    }
    /// The existing CG-14 decision is consumed without creating a second planner.
    /// Every call (including repair/fallback) charges this same ledger. A denied
    /// admission consumes no attempt; a dispatched failure retains reservations.
    pub fn dispatch(
        &mut self,
        ir: &InvocationIr,
        rendered: &RenderedInvocationIr,
        decision: LoopDecision,
        reserved_cost_microunits: u64,
        host: &dyn InvocationAdmissionPort,
        transport: &dyn BoundedInvocationPort,
    ) -> Result<InvocationAttemptResult, D> {
        self.check()?;
        if !matches!(decision, LoopDecision::Continue | LoopDecision::Replan) {
            return Err(D::LifecycleBlocked);
        }
        self.bind(ir, rendered)?;
        validate_current(ir, host).map_err(|_| D::AuthorityMismatch)?;
        self.check()?;
        if self.usage.attempts
            >= self
                .limits
                .max_attempts
                .min(self.original.prepared().invocation().data().max_attempts)
        {
            return Err(D::AttemptBudgetExceeded);
        }
        let a = rendered.rendered().accounting();
        let input = a.total_input_tokens;
        let output = a.reserved_output_tokens;
        if input == 0 || a.count.usage.total() != Some(input) {
            return Err(D::TokenCountUnavailable);
        }
        if input
            .checked_add(output)
            .is_none_or(|n| n > a.context_limit)
        {
            return Err(D::ContextWindowExceeded);
        }
        if input > self.limits.invocation_input_tokens
            || input
                .checked_add(self.usage.input_tokens)
                .is_none_or(|n| n > self.limits.run_input_tokens)
        {
            return Err(D::InputTokenBudgetExceeded);
        }
        if output > self.limits.invocation_output_tokens
            || output
                .checked_add(self.usage.output_tokens)
                .is_none_or(|n| n > self.limits.run_output_tokens)
        {
            return Err(D::OutputTokenBudgetExceeded);
        }
        if reserved_cost_microunits
            .checked_add(self.usage.cost_microunits)
            .is_none_or(|n| n > self.limits.cost_microunits)
        {
            return Err(D::CostBudgetExceeded);
        }
        let available_bytes = self
            .limits
            .output_bytes
            .saturating_sub(self.usage.output_bytes);
        if available_bytes == 0 {
            return Err(D::OutputBudgetExceeded);
        }
        let max_output_bytes = available_bytes.min(MAX_OUTPUT_BYTES as u64) as usize;
        self.usage.attempts += 1;
        self.usage.input_tokens += input;
        self.usage.output_tokens += output;
        self.usage.cost_microunits += reserved_cost_microunits;
        let binding = AttemptBinding {
            schema_version: gateway_domain::SchemaVersion::V1,
            attempt: self.usage.attempts,
            invocation: ir.identity(),
            profile: a.count.profile.clone(),
            payload_digest: a.count.payload_digest.clone(),
        };
        self.records.push(AttemptUsageRecord {
            binding: binding.clone(),
            local_input_tokens: input,
            local_provenance: match a.count.mode {
                CountMode::Exact => UsageProvenance::LocalExact,
                CountMode::Estimated => UsageProvenance::LocalEstimated,
            },
            reserved_output_tokens: output,
            reserved_cost_microunits,
            reported: None,
            reported_provenance: None,
            diagnostics: Vec::new(),
        });
        // Recheck after all preparation and reservation, immediately at dispatch.
        // No await or host callback occurs between this check and the port call.
        self.check()?;
        let response = transport.invoke(BoundedInvocationRequest {
            ir,
            rendered,
            binding: &binding,
            context: &self.context,
            max_output_bytes,
            reserved_cost_microunits,
        });
        let mut error = self.reconcile(response.usage, a.context_limit);
        let result = match response.output {
            Ok(raw) => {
                self.usage.output_bytes = self.usage.output_bytes.saturating_add(raw.len() as u64);
                if raw.len() > max_output_bytes {
                    error = Some(D::OutputBudgetExceeded);
                }
                match parse_invocation_output(ir, &raw) {
                    Ok(parsed) => InvocationAttemptResult::Parsed(Box::new(parsed)),
                    Err(e) => InvocationAttemptResult::Malformed(e),
                }
            }
            Err(InvocationAttemptFailure::Transient) => {
                // Missing output/usage is not free: retain the entire byte reserve.
                self.usage.output_bytes = self
                    .usage
                    .output_bytes
                    .saturating_add(max_output_bytes as u64);
                InvocationAttemptResult::TransientFailure
            }
            Err(failure) => {
                error = Some(match failure {
                    InvocationAttemptFailure::UncertainSideEffect => D::UncertainSideEffect,
                    _ => D::HardFailure,
                });
                InvocationAttemptResult::TransientFailure
            }
        };
        // Account completed work even if cancellation/deadline won while in flight.
        error = error.or_else(|| self.check().err());
        if let Some(e) = error {
            self.terminal = Some(e);
            self.records
                .last_mut()
                .expect("attempt")
                .diagnostics
                .push(e);
            return Err(e);
        }
        Ok(result)
    }
    fn reconcile(&mut self, reported: Option<ProviderUsage>, context_limit: u64) -> Option<D> {
        let record = self.records.last_mut().expect("reserved attempt");
        let report = reported?;
        let valid = report.binding == record.binding;
        record.reported = Some(report.clone());
        record.reported_provenance = Some(UsageProvenance::ProviderReported);
        if !valid {
            return Some(D::InvalidBinding);
        }
        if report.input_tokens.abs_diff(record.local_input_tokens)
            > self.limits.reconciliation_tolerance
        {
            record.diagnostics.push(D::UsageReconciliationMismatch);
        }
        // Never refund locally counted input; replace only output/cost reserves
        // with actual reported consumption. Missing usage keeps full reserves.
        let input_sum = self.usage.input_tokens.checked_add(
            report
                .input_tokens
                .saturating_sub(record.local_input_tokens),
        );
        self.usage.input_tokens = input_sum.unwrap_or(u64::MAX);
        self.usage.output_tokens = (self.usage.output_tokens - record.reserved_output_tokens)
            .saturating_add(report.output_tokens);
        self.usage.cost_microunits = (self.usage.cost_microunits - record.reserved_cost_microunits)
            .saturating_add(report.cost_microunits);
        if input_sum.is_none()
            || report.input_tokens > self.limits.invocation_input_tokens
            || self.usage.input_tokens > self.limits.run_input_tokens
        {
            Some(D::InputTokenBudgetExceeded)
        } else if report.output_tokens > record.reserved_output_tokens
            || self.usage.output_tokens > self.limits.run_output_tokens
        {
            Some(D::OutputTokenBudgetExceeded)
        } else if report.cost_microunits > record.reserved_cost_microunits
            || self.usage.cost_microunits > self.limits.cost_microunits
        {
            Some(D::CostBudgetExceeded)
        } else if report
            .input_tokens
            .checked_add(report.output_tokens)
            .is_none_or(|n| n > context_limit)
        {
            Some(D::ContextWindowExceeded)
        } else {
            None
        }
    }
}
