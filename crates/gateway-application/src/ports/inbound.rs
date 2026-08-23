use gateway_domain::execution_context::ExecutionContext;

use crate::context::ExecutionRequest;

pub trait ResolveExecutionContext {
    type Error;

    fn resolve(&self, request: &ExecutionRequest<'_>) -> Result<ExecutionContext, Self::Error>;
}
