use crate::{execution_profile::ExecutionProfile, operating_mode::OperatingMode, task::TaskDescriptor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionContext {
    pub task: TaskDescriptor,
    pub operating_mode: OperatingMode,
    pub execution_profile: ExecutionProfile,
}
