#![forbid(unsafe_code)]

use gateway_domain::{execution_context::ExecutionContext, execution_profile::ExecutionProfile, operating_mode::OperatingMode, task::TaskDescriptor};

#[derive(Debug, Default)]
pub struct ContextCompiler;

impl ContextCompiler {
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
