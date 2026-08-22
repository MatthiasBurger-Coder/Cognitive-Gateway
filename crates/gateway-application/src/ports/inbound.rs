use gateway_domain::{execution_context::ExecutionContext, task::TaskDescriptor};

pub trait ResolveExecutionContext {
    type Error;

    fn resolve(&self, task: &TaskDescriptor) -> Result<ExecutionContext, Self::Error>;
}
