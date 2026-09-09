#![forbid(unsafe_code)]

use gateway_domain::{
    execution_context::ExecutionContext, execution_context::ExecutionContextIR,
    execution_context_v2::ExecutionContextIRV2, execution_profile::ExecutionProfile,
    operating_mode::OperatingMode, task::TaskDescriptor,
};

#[derive(Debug, Default)]
pub struct ContextCompiler;

/// Version-aware CG-10 handoff. This boundary does not execute or authorize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextHandoff {
    V1(ExecutionContextIR),
    V2(ExecutionContextIRV2),
}

impl ContextCompiler {
    /// Accepts a validated versioned handoff without downgrading v2 shapes.
    pub fn inspect_handoff(handoff: ContextHandoff) -> Result<ContextHandoff, String> {
        match &handoff {
            ContextHandoff::V1(context) => context.validate().map_err(|e| e.to_string())?,
            ContextHandoff::V2(context) => context.validate().map_err(|e| e.to_string())?,
        }
        Ok(handoff)
    }

    #[must_use]
    pub fn compile(
        task: TaskDescriptor,
        operating_mode: OperatingMode,
        execution_profile: ExecutionProfile,
    ) -> ExecutionContext {
        ExecutionContext {
            task,
            operating_mode,
            execution_profile,
        }
    }
}
