//! Bounded parsing and repair proposals; no execution authority or provider SDK.
use crate::{output_verification::*, prompt_ir::InvocationIr};
use gateway_domain::{
    SchemaVersion, context_content_digest,
    output_parsing::{OutputCompletion, OutputRepairProposal, RepairError, validate_repair_budget},
    output_verification::*,
};

/// Complete JSON is still provisional: current reference/policy/evidence checks
/// belong to verify_output. Validate the exact admitted schema and inventory here.
pub fn parse_invocation_output(
    ir: &InvocationIr,
    raw: &[u8],
) -> Result<ParsedOutput, VerificationError> {
    let output = parse_output(raw)?;
    validate_output_proposal(
        &ir.identity(),
        &ir.prepared().invocation().data().output,
        output.data(),
    )?;
    validate_tools(ir, output.data())?;
    Ok(output)
}

pub(crate) fn validate_tools(
    ir: &InvocationIr,
    data: &OutputProposalData,
) -> Result<(), VerificationError> {
    for tool in &data.tools {
        let Some(definition) = ir.prompt().tools().get(&tool.capability.id) else {
            return Err(VerificationError::InvalidToolProposal);
        };
        if definition.binding != tool.capability || !definition.input.accepts(&tool.arguments) {
            return Err(VerificationError::InvalidToolProposal);
        }
    }
    Ok(())
}

/// Own one stream per invocation attempt. Fragments expose no parsed/tool value.
/// Overflow poisons the stream even if a transport ignores the push error.
pub struct OutputStream<'a> {
    ir: &'a InvocationIr,
    bytes: Vec<u8>,
    overflow: bool,
}
impl<'a> OutputStream<'a> {
    pub fn new(ir: &'a InvocationIr) -> Self {
        Self {
            ir,
            bytes: Vec::new(),
            overflow: false,
        }
    }
    pub fn push(&mut self, fragment: &[u8]) -> Result<(), VerificationError> {
        if self.overflow || fragment.len() > MAX_OUTPUT_BYTES - self.bytes.len() {
            self.overflow = true;
            self.bytes.clear();
            return Err(VerificationError::OutputLimitExceeded);
        }
        self.bytes.extend_from_slice(fragment);
        Ok(())
    }
    /// Transport must explicitly report successful completion, including for a
    /// syntactically complete prefix. Consuming self prevents reuse after finish.
    pub fn finish(self, completion: OutputCompletion) -> Result<ParsedOutput, VerificationError> {
        if self.overflow {
            return Err(VerificationError::OutputLimitExceeded);
        }
        if completion != OutputCompletion::Complete {
            return Err(VerificationError::OutputParseFailed);
        }
        parse_invocation_output(self.ir, &self.bytes)
    }
}

/// Mandatory trusted session accounting. Reserve the whole repair invocation's
/// token/time/cost budget using the original immutable IR, charging the same
/// aggregate session as the initial invocation. Missing/refused admission denies.
/// This port reserves budget only; it must never dispatch a model or tool.
pub trait OutputRepairBudgetPort {
    fn reserve(
        &mut self,
        ir: &InvocationIr,
        attempt: u8,
        max_output_bytes: usize,
    ) -> Result<(), RepairError>;
}

/// One controller per original invocation, retained across failures. Local
/// ceilings supplement mandatory aggregate accounting; reconstructing this
/// controller cannot reset the trusted host's session budget.
pub struct OutputRepairSession<'a> {
    ir: &'a InvocationIr,
    attempts: u8,
    max_attempts: u8,
    max_output_bytes: usize,
}
impl<'a> OutputRepairSession<'a> {
    pub fn new(
        ir: &'a InvocationIr,
        max_attempts: u8,
        max_output_bytes: usize,
    ) -> Result<Self, RepairError> {
        validate_repair_budget(max_attempts, max_output_bytes)?;
        Ok(Self {
            ir,
            attempts: 0,
            max_attempts,
            max_output_bytes,
        })
    }
    /// Recompute the failure from exact bytes; callers cannot nominate policy or
    /// evidence failures for schema repair. Raw model text is never in the proposal.
    pub fn propose(
        &mut self,
        raw: &[u8],
        budget: &mut dyn OutputRepairBudgetPort,
    ) -> Result<OutputRepairProposal, RepairError> {
        let diagnostic = match parse_invocation_output(self.ir, raw) {
            Err(
                e @ (VerificationError::OutputParseFailed
                | VerificationError::OutputSchemaInvalid
                | VerificationError::InvalidToolProposal),
            ) => e,
            _ => return Err(RepairError::RepairNotPermitted),
        };
        if self.attempts >= self.max_attempts {
            return Err(RepairError::BudgetExhausted);
        }
        self.attempts += 1;
        budget.reserve(self.ir, self.attempts, self.max_output_bytes)?;
        Ok(OutputRepairProposal {
            schema_version: SchemaVersion::V1,
            failed_output: VerificationBinding {
                invocation: self.ir.identity(),
                output_digest: context_content_digest(raw),
            },
            diagnostic,
            attempt: self.attempts,
            max_output_bytes: self.max_output_bytes,
        })
    }
}
