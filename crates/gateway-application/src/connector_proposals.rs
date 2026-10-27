//! Verified EPIC-06 tool proposals bridged to policy-gated EPIC-07 calls.
//!
//! Proposals stay data until verified. A trusted binder resolves each proposal
//! against the current task/session and emits only a typed connector request;
//! `ConnectorApplicationPort` performs current CG-09 admission and dispatch.
//! A returned observation is not a goal-completion claim.

use crate::prompt_rendering::ToolDefinition;
use crate::{
    connector_results::ConnectorResult,
    connectors::{
        ConnectorApplicationPort, ConnectorFailure, ConnectorInventory, ConnectorObservation,
        ConnectorRequest, ConnectorTarget,
    },
    output_verification::VerifiedOutput,
    prompt_ir::{InvocationAdmissionPort, InvocationIr},
};
use gateway_domain::{
    NonEmptyText, PlanStepId, ResolvedReference, TaskId, TypedValue,
    output_verification::ToolCallProposal, prompt_ir::InvocationIrIdentity,
};
use serde_json::{Map, Value};

/// Trusted current-session binding. The implementation must revalidate the
/// invocation and bind step/execution identity to current admitted session
/// state. Model-selected capability and arguments are proposals only.
pub trait ConnectorProposalBindingPort {
    fn validate_current(&self, invocation: &InvocationIr) -> Result<(), ProposalError>;
    /// Final current-session check after binding/discovery callbacks. Custom
    /// binders must prove the exact bound coordinates, not merely task scope.
    fn validate_bound_current(
        &self,
        invocation: &InvocationIr,
        bound: &BoundConnectorProposal,
    ) -> Result<(), ProposalError>;
    fn bind(
        &self,
        invocation: &InvocationIr,
        proposal: &ToolCallProposal,
        ordinal: usize,
    ) -> Result<BoundConnectorProposal, ProposalError>;
}

/// Current host-owned session coordinates for one admitted model invocation.
pub struct ConnectorSessionBinding {
    pub target: ConnectorTarget,
    pub step: PlanStepId,
    pub execution: NonEmptyText,
}

/// Host boundary for deriving task/step/execution authority from current
/// session state. The model never supplies these values.
pub trait ConnectorSessionBindingPort {
    fn current_binding(
        &self,
        invocation: &InvocationIr,
    ) -> Result<ConnectorSessionBinding, ProposalError>;
}

/// Reusable trusted binder over the current InvocationIR admission, session
/// identity, and canonical connector inventory. Discovery only supplies a
/// capability mapping; the subsequent invoke still performs current CG-09
/// authorization and immediate pre-effect revalidation.
pub struct CanonicalConnectorProposalBinder<A, S, C> {
    admission: A,
    session: S,
    connectors: C,
}

impl<A, S, C> CanonicalConnectorProposalBinder<A, S, C> {
    pub fn new(admission: A, session: S, connectors: C) -> Self {
        Self {
            admission,
            session,
            connectors,
        }
    }
}

impl<A, S, C> ConnectorProposalBindingPort for CanonicalConnectorProposalBinder<A, S, C>
where
    A: InvocationAdmissionPort,
    S: ConnectorSessionBindingPort,
    C: ConnectorApplicationPort,
{
    fn validate_current(&self, invocation: &InvocationIr) -> Result<(), ProposalError> {
        crate::prompt_ir::validate_current(invocation, &self.admission)
            .map_err(|_| ProposalError::StaleInvocation)?;
        let binding = self.session.current_binding(invocation)?;
        if binding.target.scope != invocation.prepared().invocation().data().task.scope {
            return Err(ProposalError::StaleInvocation);
        }
        Ok(())
    }

    fn validate_bound_current(
        &self,
        invocation: &InvocationIr,
        bound: &BoundConnectorProposal,
    ) -> Result<(), ProposalError> {
        crate::prompt_ir::validate_current(invocation, &self.admission)
            .map_err(|_| ProposalError::StaleInvocation)?;
        let current = self.session.current_binding(invocation)?;
        if current.target != bound.request.target
            || current.step != bound.request.step
            || current.execution != bound.request.execution
        {
            return Err(ProposalError::StaleInvocation);
        }
        Ok(())
    }

    fn bind(
        &self,
        invocation: &InvocationIr,
        proposal: &ToolCallProposal,
        ordinal: usize,
    ) -> Result<BoundConnectorProposal, ProposalError> {
        self.validate_current(invocation)?;
        let binding = self.session.current_binding(invocation)?;
        let inventory = self
            .connectors
            .discover(&binding.target)
            .map_err(ProposalError::Connector)?;
        let tool = invocation
            .prompt()
            .tools()
            .get(&proposal.capability.id)
            .ok_or(ProposalError::InvalidProposal)?;
        bind_canonical_data(
            &invocation.identity(),
            &invocation.prepared().invocation().data().task,
            tool,
            proposal,
            ordinal,
            binding,
            &inventory,
        )
    }
}

fn bind_canonical_data(
    identity: &InvocationIrIdentity,
    task: &ResolvedReference<TaskId>,
    tool: &ToolDefinition,
    proposal: &ToolCallProposal,
    ordinal: usize,
    binding: ConnectorSessionBinding,
    inventory: &ConnectorInventory,
) -> Result<BoundConnectorProposal, ProposalError> {
    if inventory.target != binding.target || binding.target.scope != task.scope {
        return Err(ProposalError::StaleInvocation);
    }
    let mut matching = inventory
        .capabilities
        .iter()
        .filter(|entry| entry.capability == proposal.capability.id);
    let capability = matching.next().ok_or(ProposalError::InvalidProposal)?;
    if matching.next().is_some() {
        return Err(ProposalError::InvalidProposal);
    }
    let pin = tool
        .connector_inventory
        .as_ref()
        .ok_or(ProposalError::StaleInvocation)?;
    tool.validate_connector_binding()
        .map_err(|_| ProposalError::StaleInvocation)?;
    if proposal.capability != tool.binding
        || proposal.capability.scope != task.scope
        || !tool.input.accepts(&proposal.arguments)
    {
        return Err(ProposalError::InvalidProposal);
    }
    if pin.connector != binding.target.connector
        || pin.scope != binding.target.scope
        || pin.revision != inventory.revision
        || pin.argument_schema_digest != capability.argument_schema_digest
    {
        return Err(ProposalError::StaleInvocation);
    }
    Ok(BoundConnectorProposal {
        request: ConnectorRequest {
            target: inventory.target.clone(),
            capability: capability.capability.clone(),
            class: capability.class,
            inventory_revision: inventory.revision.clone(),
            arguments: typed_arguments(&proposal.arguments)?,
            task: task.id.clone(),
            step: binding.step,
            execution: binding.execution,
            correlation: proposal_correlation(identity, proposal, ordinal)?,
        },
    })
}

fn proposal_correlation(
    identity: &InvocationIrIdentity,
    proposal: &ToolCallProposal,
    ordinal: usize,
) -> Result<NonEmptyText, ProposalError> {
    // Equal argument payloads can intentionally describe distinct ordered
    // effects. The actual native identity and ordinal bind each instance.
    let bytes = serde_json::to_vec(&(
        "cg.connector-proposal-correlation/2.0",
        identity,
        ordinal,
        proposal,
    ))
    .map_err(|_| ProposalError::InvalidProposal)?;
    let digest = gateway_domain::context_content_digest(&bytes);
    NonEmptyText::new(format!("proposal-{}", digest.as_str()))
        .map_err(|_| ProposalError::InvalidProposal)
}

fn typed_arguments(
    value: &Value,
) -> Result<std::collections::BTreeMap<NonEmptyText, TypedValue>, ProposalError> {
    let Value::Object(object) = value else {
        return Err(ProposalError::InvalidProposal);
    };
    object
        .iter()
        .map(|(name, value)| {
            let name =
                NonEmptyText::new(name.clone()).map_err(|_| ProposalError::InvalidProposal)?;
            let value = match value {
                Value::Bool(v) => TypedValue::Boolean(*v),
                Value::Number(v) => {
                    TypedValue::Integer(v.as_i64().ok_or(ProposalError::InvalidProposal)?)
                }
                Value::String(v) => {
                    TypedValue::string(v.clone()).map_err(|_| ProposalError::InvalidProposal)?
                }
                _ => return Err(ProposalError::InvalidProposal),
            };
            Ok((name, value))
        })
        .collect()
}

pub struct BoundConnectorProposal {
    pub request: ConnectorRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalError {
    StaleInvocation,
    InvalidProposal,
    PolicyDenied,
    Connector(ConnectorFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchedProposal {
    pub ordinal: usize,
    /// Exact host-bound request that crossed ConnectorApplicationPort. It is
    /// retained for post-effect read correlation and immutable session audit.
    pub request: ConnectorRequest,
    pub observation: ConnectorObservation,
}

/// Successful observations are retained if a later proposal fails. `Complete`
/// means every proposed connector call returned a validated observation; it
/// never means the requested task goal is complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalBatch {
    pub invocation: gateway_domain::ContentDigest,
    pub all_calls_returned: bool,
    pub results: Vec<DispatchedProposal>,
    pub failure: Option<(usize, ProposalError)>,
}

pub struct ConnectorProposalDispatcher<B, C> {
    binder: B,
    connectors: C,
}

impl<B, C> ConnectorProposalDispatcher<B, C> {
    pub fn new(binder: B, connectors: C) -> Self {
        Self { binder, connectors }
    }
}

impl<B: ConnectorProposalBindingPort, C: ConnectorApplicationPort>
    ConnectorProposalDispatcher<B, C>
{
    /// Dispatch proposals only from a `VerifiedOutput` produced for this exact
    /// admitted IR. Calls run in proposal order and stop at the first failure.
    /// Each request crosses the connector application's current policy gate.
    pub fn dispatch(
        &self,
        invocation: &InvocationIr,
        verified: &VerifiedOutput,
    ) -> Result<ProposalBatch, ProposalError> {
        if verified.record().admitted_invocation != invocation.identity()
            || verified.output().data().invocation != invocation.identity()
        {
            return Err(ProposalError::StaleInvocation);
        }
        let proposals = &verified.output().data().tools;
        let mut batch = ProposalBatch {
            invocation: invocation.identity().invocation_digest,
            all_calls_returned: false,
            results: Vec::with_capacity(proposals.len()),
            failure: None,
        };
        for (ordinal, proposal) in proposals.iter().enumerate() {
            if let Err(error) = self.binder.validate_current(invocation) {
                batch.failure = Some((ordinal, error));
                return Ok(batch);
            }
            let bound = match self.binder.bind(invocation, proposal, ordinal) {
                Ok(bound) => bound,
                Err(error) => {
                    batch.failure = Some((ordinal, error));
                    return Ok(batch);
                }
            };
            if let Err(error) = validate_bound(invocation, proposal, &bound) {
                batch.failure = Some((ordinal, error));
                return Ok(batch);
            }
            // Discovery callbacks can revoke the originating invocation or
            // change its host coordinates. Recheck both before dispatch.
            if let Err(error) = self.binder.validate_bound_current(invocation, &bound) {
                batch.failure = Some((ordinal, error));
                return Ok(batch);
            }
            match self.connectors.invoke(&bound.request) {
                Ok(observation) => {
                    if ConnectorResult::validate_for_request(&bound.request, &observation).is_err()
                    {
                        let mutating = matches!(
                            bound.request.class,
                            crate::connectors::ConnectorOperationClass::Mutate
                                | crate::connectors::ConnectorOperationClass::Admin
                        );
                        batch.failure = Some((
                            ordinal,
                            ProposalError::Connector(ConnectorFailure {
                                code: crate::connectors::ConnectorFailureCode::StaleResult,
                                effect: if mutating {
                                    crate::connectors::ConnectorEffect::Unknown
                                } else {
                                    observation.metadata.effect
                                },
                                correlation: bound.request.correlation.clone(),
                            }),
                        ));
                        return Ok(batch);
                    }
                    batch.results.push(DispatchedProposal {
                        ordinal,
                        request: bound.request,
                        observation,
                    });
                }
                Err(error) => {
                    batch.failure = Some((ordinal, ProposalError::Connector(error)));
                    return Ok(batch);
                }
            }
        }
        batch.all_calls_returned = true;
        Ok(batch)
    }
}

fn validate_bound(
    invocation: &InvocationIr,
    proposal: &ToolCallProposal,
    bound: &BoundConnectorProposal,
) -> Result<(), ProposalError> {
    let request = &bound.request;
    let task = &invocation.prepared().invocation().data().task;
    if request.capability != proposal.capability.id
        || request.target.scope != task.scope
        || request.task != task.id
        || arguments_json(&request.arguments)? != proposal.arguments
        || proposal.capability.scope != task.scope
    {
        return Err(ProposalError::InvalidProposal);
    }
    Ok(())
}

fn arguments_json(
    arguments: &std::collections::BTreeMap<gateway_domain::NonEmptyText, TypedValue>,
) -> Result<Value, ProposalError> {
    let mut values = Map::new();
    for (name, value) in arguments {
        let value = match value {
            TypedValue::Boolean(v) => Value::Bool(*v),
            TypedValue::Integer(v) => Value::from(*v),
            TypedValue::String(v) => Value::String(v.as_str().to_owned()),
            TypedValue::Symbol(v) => Value::String(v.as_str().to_owned()),
            TypedValue::Decimal(_) | TypedValue::Set(_) => {
                return Err(ProposalError::InvalidProposal);
            }
        };
        values.insert(name.as_str().to_owned(), value);
    }
    Ok(Value::Object(values))
}

#[cfg(test)]
#[path = "connector_proposals/tests.rs"]
mod tests;
