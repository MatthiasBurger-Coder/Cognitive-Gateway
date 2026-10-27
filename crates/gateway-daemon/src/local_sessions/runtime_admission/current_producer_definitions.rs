//! Current native producer definitions are data, without an allocation or grant.
//! Both the live source and retained-consent preflight share this native compiler.
use super::connector_source::{
    canonical_owner_documents, compile, projection_command, workflow_policy,
};
use super::*;
use gateway_application::{
    codex::CompileCommand,
    context_application::CompiledStep,
    policy_application::{PolicyApplication, ResolvedStepPolicyInput},
};
use gateway_domain::*;

pub(super) struct NativeProducerDefinitions {
    pub(super) runtime: RuntimeAdmission,
    pub(super) canonical_policy: Value,
    pub(super) action: CompileCommand,
    pub(super) action_input: ResolvedStepPolicyInput,
    pub(super) producer_input: ResolvedStepPolicyInput,
    pub(super) producer_context: CompiledStep,
    pub(super) producer_projection: Value,
    pub(super) canonical_documents: Vec<(Pin, Value)>,
    pub(super) canonical_pins: Vec<Pin>,
    pub(super) rule_fingerprint: String,
    pub(super) policy: PolicyId,
    pub(super) basis_json: Value,
    pub(super) policy_revision: ContentDigest,
}
fn invalid() -> SessionError {
    SessionError::InvalidInput
}

/// Fresh definitions for historical-consent preflight. This type owns no
/// resource allocation, live consent, invocation or callable transport. Pending
/// consent has no preparation lease; only the real session budget applies here.
pub(super) struct CurrentProducerDefinitions {
    checkpoint: SessionCheckpoint,
    step: PlanStepId,
    native: NativeProducerDefinitions,
}
impl CurrentProducerDefinitions {
    pub(super) fn capture(
        access: &LocalSessionAccess,
        checkpoint: &SessionCheckpoint,
        step: &PlanStepId,
    ) -> Result<Self, SessionError> {
        let (host, repository, claim, definition) = access
            .current_repository_claim_with_definition(
                &checkpoint.owner,
                &checkpoint.snapshot.session,
                Some(checkpoint.budget.deadline_ms),
            )?;
        validate_checkpoint(&host, &claim, checkpoint, step)?;
        let native = assemble(&host, checkpoint, step, &|| {
            access.check_deadline(Some(checkpoint.budget.deadline_ms))
        })?;
        // Authenticate again after native assembly, including configuration and
        // both the original and newly selected repository. There is no fallback
        // to the first capture when anything changes during reconstruction.
        let (latest_host, _, latest, latest_definition) = access
            .current_repository_claim_with_definition(
                &checkpoint.owner,
                &checkpoint.snapshot.session,
                Some(checkpoint.budget.deadline_ms),
            )?;
        if latest != claim
            || !definition.matches(&latest_definition)
            || repository.load(&checkpoint.owner, &checkpoint.snapshot.session)? != claim
        {
            return Err(SessionError::StaleRevision);
        }
        validate_checkpoint(&latest_host, &latest, checkpoint, step)?;
        let current_runtime = latest_host
            .sessions
            .as_ref()
            .and_then(|config| config.connector_runtime.as_ref())
            .ok_or(SessionError::UnsupportedCapability)?;
        let canonical = host
            .canonical
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        let latest_canonical = latest_host
            .canonical
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        let config = host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        let latest_config = latest_host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        if current_runtime.reference()? != native.runtime.reference()?
            || config.consent_required != latest_config.consent_required
            || config.issuers != latest_config.issuers
            || config.ttl_ms != latest_config.ttl_ms
            || canonical.catalog != latest_canonical.catalog
            || canonical.plan != latest_canonical.plan
            || canonical.rules != latest_canonical.rules
            || canonical.process != latest_canonical.process
            || canonical.policy != latest_canonical.policy
            || canonical.pinned_basis != latest_canonical.pinned_basis
            || canonical_owner_documents(&latest_host)? != native.canonical_documents
        {
            return Err(SessionError::StaleRevision);
        }
        access.check_deadline(Some(checkpoint.budget.deadline_ms))?;
        Ok(Self {
            checkpoint: checkpoint.clone(),
            step: step.clone(),
            native,
        })
    }
    pub(super) fn checkpoint(&self) -> &SessionCheckpoint {
        &self.checkpoint
    }
    pub(super) fn step(&self) -> &PlanStepId {
        &self.step
    }
    pub(super) fn runtime(&self) -> &RuntimeAdmission {
        &self.native.runtime
    }
    pub(super) fn canonical_documents(&self) -> &[(Pin, Value)] {
        &self.native.canonical_documents
    }
    pub(super) fn producer_context(&self) -> &CompiledStep {
        &self.native.producer_context
    }
    pub(super) fn producer_projection(&self) -> &Value {
        &self.native.producer_projection
    }
    pub(super) fn resolved(&self) -> &gateway_application::resolution_application::ResolvedPlan {
        &self.native.action.resolved
    }
    pub(super) fn canonical_policy(&self) -> &Value {
        &self.native.canonical_policy
    }
    pub(super) fn action_input(&self) -> &ResolvedStepPolicyInput {
        &self.native.action_input
    }
    pub(super) fn policy(&self) -> &PolicyId {
        &self.native.policy
    }
    pub(super) fn policy_revision(&self) -> &ContentDigest {
        &self.native.policy_revision
    }
}

fn validate_checkpoint(
    host: &LocalCodexHost,
    claim: &FencedSession,
    expected: &SessionCheckpoint,
    step: &PlanStepId,
) -> Result<(), SessionError> {
    if &claim.checkpoint != expected {
        return Err(SessionError::StaleRevision);
    }
    if !matches!(
        expected.snapshot.state,
        SessionState::Preparing | SessionState::PendingConsent | SessionState::Runnable
    ) {
        return Err(SessionError::InvalidState);
    }
    let config = host
        .sessions
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    let goal = expected
        .goal
        .as_connector()
        .ok_or(SessionError::UnsupportedCapability)?;
    if goal.scope() != &host.binding.canonical_scope
        || !goal.steps().contains(step)
        || expected.execution != config.execution
        || expected.budget.max_actions != config.max_actions
        || expected.budget.max_retries != config.max_retries
    {
        return Err(SessionError::StaleRevision);
    }
    let limits = LocalSessionAccess::resource_limits_from_host(host, &expected.goal)?
        .ok_or(SessionError::UnsupportedCapability)?;
    if expected
        .resource_ledger
        .as_ref()
        .map(|ledger| ledger.limits())
        != Some(&limits)
    {
        return Err(SessionError::StaleRevision);
    }
    Ok(())
}

/// Pure assembly over the caller's one actual loaded host. The guard only
/// checks caller-owned deadlines. Consent facts are cleared by the native
/// projection owner; the action is not compiled and cannot authorize effects.
pub(super) fn assemble(
    host: &LocalCodexHost,
    checkpoint: &SessionCheckpoint,
    step_id: &PlanStepId,
    guard: &dyn Fn() -> Result<(), SessionError>,
) -> Result<NativeProducerDefinitions, SessionError> {
    guard()?;
    let runtime = host
        .sessions
        .as_ref()
        .and_then(|config| config.connector_runtime.as_ref())
        .ok_or(SessionError::UnsupportedCapability)?;
    let loaded = runtime.loaded()?;
    let step = runtime
        .0
        .steps
        .iter()
        .find(|step| step.step == step_id.as_str())
        .ok_or(SessionError::AuthorityDenied)?;
    let ProducerAdmission::Model(model) = &step.producer else {
        return Err(SessionError::UnsupportedCapability);
    };
    let canonical_documents = canonical_owner_documents(host)?;
    let canonical_pins = canonical_documents
        .iter()
        .map(|(pin, _)| pin.clone())
        .collect::<Vec<_>>();
    let action = projection_command(host, loaded.document(&step.action_projection)?.clone())?;
    guard()?;
    let producer_projection = loaded.document(&model.projection)?.clone();
    let producer = projection_command(host, producer_projection.clone())?;
    guard()?;
    if &action.resolved.snapshot.input().desired != checkpoint.goal.intent().desired_state()
        || &producer.resolved.snapshot.input().desired != checkpoint.goal.intent().desired_state()
    {
        return Err(SessionError::StaleRevision);
    }
    if action.resolved != producer.resolved
        || action.policy_context != producer.policy_context
        || action.projection.mapping.step != *step_id
        || producer.projection.mapping.step.as_str() != model.producer_step
        || action.projection.task.id() != producer.projection.task.id()
    {
        return Err(SessionError::StaleRevision);
    }
    let rule_fingerprint = DeclarativeResolutionApplication
        .inspect_resolution(&action.resolved)
        .map_err(|_| invalid())?
        .rule_fingerprint;
    // Both projections use the same full native resolution and policy facts.
    // Assemble the whole map once in this callback; these remain plain inputs.
    let inputs = PolicyApplication
        .step_inputs(&action.resolved, &action.policy_context)
        .map_err(|_| invalid())?;
    let action_input = inputs.get(step_id).cloned().ok_or_else(invalid)?;
    let producer_input = inputs
        .get(&producer.projection.mapping.step)
        .cloned()
        .ok_or_else(invalid)?;
    if action_input.operating_mode != checkpoint.execution.mode
        || action_input.execution_profile != checkpoint.execution.profile
        || producer_input.operating_mode != checkpoint.execution.mode
        || producer_input.execution_profile != checkpoint.execution.profile
    {
        return Err(SessionError::StaleRevision);
    }
    if producer_input.capabilities.is_empty()
        || producer_input
            .capabilities
            .values()
            .any(|cap| cap.class() != CapabilityClass::Inspect)
    {
        return Err(SessionError::AuthorityDenied);
    }
    guard()?;
    let producer_context = compile(&producer).map_err(|_| SessionError::AuthorityDenied)?;
    guard()?;
    let policy = workflow_policy(&action)?;
    let basis_json = host_mapping::policy(
        &action.resolved,
        host.canonical
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?
            .policy
            .clone(),
    )
    .map_err(error)?
    .2;
    let policy_identity = json!({
        "domain":"cg.connector-current-policy/1.0",
        "runtime":runtime.reference()?,
        "canonical_policy":runtime.policy_reference(),
        "basis":basis_json,
        "canonical_records":canonical_pins,
        "catalog":host.canonical.as_ref().ok_or(SessionError::UnsupportedCapability)?.catalog,
        "native_rule_fingerprint":rule_fingerprint,
        "step":step_id.as_str(),
        "action_projection":step.action_projection,
        "read_projection":model.projection,
        "workflow_policy":policy.as_str(),
        "consent_required":host.sessions.as_ref().ok_or(SessionError::UnsupportedCapability)?.consent_required
    });
    let policy_revision = gateway_domain::context_content_digest(
        &serde_json::to_vec(&policy_identity).map_err(|_| invalid())?,
    );
    guard()?;
    Ok(NativeProducerDefinitions {
        runtime: runtime.clone(),
        canonical_policy: host
            .canonical
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?
            .policy
            .clone(),
        action,
        action_input,
        producer_input,
        producer_context,
        producer_projection,
        canonical_documents,
        canonical_pins,
        rule_fingerprint,
        policy,
        basis_json,
        policy_revision,
    })
}

#[cfg(test)]
mod tests;
