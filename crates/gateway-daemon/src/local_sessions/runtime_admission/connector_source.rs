//! Current native policy/claim source. No transport, price, model or spending
//! cursor is owned here; data capture does not grant connector execution.
use super::*;
use crate::{
    local_sessions::selector_association::{
        AssociationContext, BoundAssociation, FixedReadJoin, ScopeObjectDocument,
        SelectorAssociation,
    },
    mcp_client::{
        ProtocolResult, mapping::MappedInventory, session_budget::ReadonlyAllocationView,
    },
    session_store::{ConnectorPolicyAuditContext, ConnectorPolicyAuditPins},
};
use gateway_application::{
    codex::CompileCommand,
    connector_policy::{ConnectorPolicyEvaluation, ConnectorPolicySnapshot, ConnectorPolicySource},
    connectors::{
        ConnectorDispatch, ConnectorEffect, ConnectorFailure, ConnectorFailureCode,
        ConnectorInventory, ConnectorRequest, ConnectorTarget,
    },
    context_application::{
        CompileStepInput, CompiledStep, ContextApplication, ContextApplicationError,
    },
    policy_application::ResolvedStepPolicyInput,
    prompt_rendering::ToolDefinition,
    resolution::ResolutionBasis,
};
use gateway_domain::{ContentDigest, UnixTimestamp};
use std::sync::OnceLock;

struct AllocationIdentity {
    binding: ResourceReservationBinding,
    reservation: ContentDigest,
    revision: Revision,
    fence: FenceToken,
    phase: SessionResourcePhase,
    amounts: SessionResourceAmounts,
    cost_unit: ReferenceId,
    deadline: std::time::Instant,
}
impl AllocationIdentity {
    fn capture(a: &SessionResourceAllocation) -> Self {
        Self {
            binding: a.binding().clone(),
            reservation: a.reservation_id().clone(),
            revision: a.claim_revision(),
            fence: a.fence(),
            phase: a.phase(),
            amounts: *a.amounts(),
            cost_unit: a.cost_unit().clone(),
            deadline: a.deadline(),
        }
    }
    fn check(&self, a: &SessionResourceAllocation) -> Result<(), SessionError> {
        if self.binding != *a.binding()
            || self.reservation != *a.reservation_id()
            || self.revision != a.claim_revision()
            || self.fence != a.fence()
            || self.phase != a.phase()
            || self.amounts != *a.amounts()
            || self.cost_unit != *a.cost_unit()
            || self.deadline != a.deadline()
        {
            return Err(SessionError::StaleRevision);
        }
        Ok(())
    }
}
struct NativeContexts {
    checkpoint: SessionCheckpoint,
    runtime: RuntimeAdmission,
    canonical_policy: Value,
    producer_projection: Value,
    action_execution: NonEmptyText,
    resolved: gateway_application::resolution_application::ResolvedPlan,
    basis: ResolutionBasis,
    basis_json: Value,
    canonical_pins: Vec<Pin>,
    canonical_documents: Vec<(Pin, Value)>,
    rule_fingerprint: String,
    authority: PolicyAuthority,
    action: ResolvedStepPolicyInput,
    producer: ResolvedStepPolicyInput,
    producer_context: CompiledStep,
    action_context: Option<CompiledStep>,
    policy: PolicyId,
    policy_revision: ContentDigest,
    receipt: Option<NonEmptyText>,
    deadline: UnixTimestamp,
    deadline_ms: u64,
    consent_required: bool,
}
/// One owning result of one complete fresh source lookup. Data accessors do
/// not authorize model or connector I/O, and the view is neither cloneable nor
/// serializable. Callers must obtain another fresh view for another guard.
pub(super) struct ModelInputsCurrent {
    source_identity: Arc<()>,
    inventory: ConnectorInventory,
    native: NativeContexts,
    binding: ResourceReservationBinding,
    reservation: ContentDigest,
    claim_revision: Revision,
    proposal_tools: BTreeMap<CapabilityId, ToolDefinition>,
}
impl ModelInputsCurrent {
    /// Historical acquisition coordinates only; no child grant is exposed.
    pub(super) fn pre_execution_goal_binding(
        &self,
    ) -> Result<PreExecutionGoalBinding, SessionError> {
        let context = self.producer_context();
        let execution = context.context().execution_context();
        Ok(PreExecutionGoalBinding {
            acquisition: self.binding.clone(),
            reservation: self.reservation.clone(),
            claim_revision: self.claim_revision,
            task: execution.task().id().clone(),
            execution: NonEmptyText::new(execution.id().as_str()).map_err(|_| invalid())?,
            context_digest: gateway_domain::context_content_digest(
                context.to_json().map_err(|_| invalid())?.as_bytes(),
            ),
        })
    }
    pub(super) fn inventory(&self) -> &ConnectorInventory {
        &self.inventory
    }
    pub(super) fn action_execution(&self) -> &NonEmptyText {
        &self.native.action_execution
    }
    pub(super) fn canonical_pins(&self) -> &[Pin] {
        &self.native.canonical_pins
    }
    pub(super) fn canonical_documents(&self) -> &[(Pin, Value)] {
        &self.native.canonical_documents
    }
    pub(super) fn checkpoint(&self) -> &SessionCheckpoint {
        &self.native.checkpoint
    }
    pub(super) fn binding(&self) -> &ResourceReservationBinding {
        &self.binding
    }
    pub(super) fn runtime(&self) -> &RuntimeAdmission {
        &self.native.runtime
    }
    pub(super) fn resolved(&self) -> &gateway_application::resolution_application::ResolvedPlan {
        &self.native.resolved
    }
    pub(super) fn producer_context(&self) -> &CompiledStep {
        &self.native.producer_context
    }
    pub(super) fn proposal_tools(&self) -> &BTreeMap<CapabilityId, ToolDefinition> {
        &self.proposal_tools
    }
    pub(super) fn canonical_policy(&self) -> &Value {
        &self.native.canonical_policy
    }
    pub(super) fn producer_projection(&self) -> &Value {
        &self.native.producer_projection
    }
}

/// I/O capture runs BEFORE SessionConnectorRun::with_factory. The callback in
/// that constructor is intentionally pure and only attaches the sealed view.
pub(super) struct ConnectorSourceBasis {
    access: Arc<LocalSessionAccess>,
    allocation: AllocationIdentity,
    expected: FencedSession,
    repository: PostgresSessionStore,
    store_definition: StoreDefinition,
    runtime: RuntimeAdmission,
    canonical: crate::codex_canonical::CanonicalAdmission,
    native: NativeContexts,
    audit: ConnectorPolicyAuditContext,
    action_request: Option<ConnectorRequest>,
}
pub(super) struct LocalConnectorSource {
    identity: Arc<()>,
    basis: ConnectorSourceBasis,
    allocation: ReadonlyAllocationView,
    inventory: OnceLock<InventoryBinding>,
    #[cfg(test)]
    timings: SourceTimings,
}
#[cfg(test)]
#[derive(Default)]
struct SourceTimings {
    calls: std::sync::atomic::AtomicU64,
    total: std::sync::atomic::AtomicU64,
    fresh: std::sync::atomic::AtomicU64,
    native: std::sync::atomic::AtomicU64,
    claims: std::sync::atomic::AtomicU64,
}
#[cfg(test)]
struct ProfileTimer<'a>(&'a std::sync::atomic::AtomicU64, std::time::Instant);
#[cfg(test)]
impl Drop for ProfileTimer<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(
            self.1.elapsed().as_micros().min(u64::MAX as u128) as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}
struct InventoryBinding {
    tools: BTreeMap<CapabilityId, ToolDefinition>,
    inventory: MappedInventory,
    association: BoundAssociation,
}
/// Consumed exactly once by the trusted target factory after its actual,
/// budgeted discovery. It cannot build an unchecked inventory.
pub(super) struct InventoryBinder {
    source: Arc<LocalConnectorSource>,
}

fn denied(request: &ConnectorRequest, error: SessionError) -> ConnectorFailure {
    ConnectorFailure {
        code: match error {
            SessionError::ScopeDenied => ConnectorFailureCode::ScopeMismatch,
            SessionError::LimitExceeded | SessionError::ExpiredInteraction => {
                ConnectorFailureCode::BudgetExhausted
            }
            SessionError::Unavailable | SessionError::StorageUnavailable => {
                ConnectorFailureCode::Unavailable
            }
            _ => ConnectorFailureCode::PermissionDenied,
        },
        effect: ConnectorEffect::NotDispatched,
        correlation: request.correlation.clone(),
    }
}
fn check_source_deadline(
    access: &LocalSessionAccess,
    allocation: &SessionResourceAllocation,
    until_ms: u64,
) -> Result<(), SessionError> {
    access.check_deadline(Some(until_ms))?;
    if std::time::Instant::now() >= allocation.deadline() {
        return Err(SessionError::LimitExceeded);
    }
    Ok(())
}
pub(super) fn compile(command: &CompileCommand) -> Result<CompiledStep, ContextApplicationError> {
    ContextApplication.compile_step(CompileStepInput {
        resolved: &command.resolved,
        authority: &command.authority,
        policy_context: &command.policy_context,
        catalog: &command.catalog,
        projection: &command.projection,
        candidates: &command.candidates,
        selected: &command.selected,
    })
}
#[cfg(test)]
fn canonical_document_pins(host: &LocalCodexHost) -> Result<Vec<Pin>, SessionError> {
    canonical_owner_documents(host)
        .map(|documents| documents.into_iter().map(|(pin, _)| pin).collect())
}
pub(super) fn canonical_owner_documents(
    host: &LocalCodexHost,
) -> Result<Vec<(Pin, Value)>, SessionError> {
    let canonical = host
        .canonical
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    let documents = canonical.documents(&host.resources).map_err(error)?;
    [
        (&canonical.plan, "cg.plan"),
        (&canonical.rules, "cg.composition-rules"),
        (&canonical.process, "cg.process-snapshot"),
    ]
    .into_iter()
    .zip(documents)
    .map(|((reference, contract), document)| {
        let mut value = reference.clone();
        let digest = value["digest"]
            .as_str()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .ok_or(invalid())?
            .to_owned();
        value["digest"] = json!(digest);
        value["scope"] = json!(host.binding.canonical_scope);
        let pin: Pin = serde_json::from_value(value).map_err(|_| invalid())?;
        if pin.contract.as_str() != contract || pin.contract_version != SchemaVersion::V1 {
            return Err(SessionError::UnsupportedCapability);
        }
        if gateway_domain::context_content_digest(
            &serde_json::to_vec(&document).map_err(|_| invalid())?,
        ) != pin.digest
        {
            return Err(SessionError::StaleRevision);
        }
        Ok((pin, document))
    })
    .collect()
}
pub(super) fn projection_command(
    host: &LocalCodexHost,
    document: Value,
) -> Result<CompileCommand, SessionError> {
    let canonical = host
        .canonical
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    let resolved = host
        .sessions
        .as_ref()
        .and_then(|config| config.connector_runtime.as_ref())
        .ok_or(SessionError::UnsupportedCapability)?
        .loaded()?
        .resolved()?
        .clone();
    let mut command =
        host_mapping::compile(resolved, canonical.policy.clone(), document).map_err(error)?;
    // Operator facts do not carry a request-scoped connector consent grant.
    for facts in command.policy_context.steps.values_mut() {
        facts.consents.clear();
    }
    Ok(command)
}
pub(super) fn workflow_policy(command: &CompileCommand) -> Result<PolicyId, SessionError> {
    let workflow = command
        .catalog
        .workflow(&command.projection.mapping.workflow)
        .ok_or(SessionError::StaleRevision)?;
    let policy = command
        .authority
        .policies
        .iter()
        .find(|policy| {
            policy.id() == workflow.policy_id()
                && command.catalog.policy(policy.id()) == Some(*policy)
        })
        .ok_or(SessionError::AuthorityDenied)?;
    Ok(policy.id().clone())
}
fn step<'a>(
    runtime: &'a RuntimeAdmission,
    allocation: &SessionResourceAllocation,
) -> Result<&'a StepAdmission, SessionError> {
    runtime
        .0
        .steps
        .iter()
        .find(|step| step.step == allocation.binding().step.as_str())
        .ok_or(SessionError::AuthorityDenied)
}
fn native_contexts(
    access: &LocalSessionAccess,
    host: &LocalCodexHost,
    repository: &PostgresSessionStore,
    current: &FencedSession,
    allocation: &SessionResourceAllocation,
    action_request: Option<&ConnectorRequest>,
) -> Result<NativeContexts, SessionError> {
    let until = current
        .checkpoint
        .lease_until_ms
        .ok_or(SessionError::InvalidState)?
        .min(current.checkpoint.budget.deadline_ms);
    let guard = |until_ms| {
        // Pure assembly has no connector/model/audit callbacks. Both callers
        // bracket the whole operation with the exact current durable claim.
        check_source_deadline(access, allocation, until_ms)
    };
    guard(until)?;
    let super::current_producer_definitions::NativeProducerDefinitions {
        runtime,
        canonical_policy,
        mut action,
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
    } = super::current_producer_definitions::assemble(
        host,
        &current.checkpoint,
        &allocation.binding().step,
        &|| guard(until),
    )?;
    let mut receipt = None;
    let mut effective_until = until;
    if let Some(request) = action_request
        && (request.step != allocation.binding().step
            || request.target.scope != allocation.binding().scope
            || request.execution.as_str() != action.projection.id.as_str()
            || &request.task != action.projection.task.id())
    {
        return Err(SessionError::ScopeDenied);
    }
    if let Some(accepted) = &current.checkpoint.accepted_connector_consent {
        access.check_deadline(Some(current.checkpoint.budget.deadline_ms))?;
        let status = repository.connector_consent(&current.checkpoint.owner, &accepted.record)?;
        access.check_deadline(Some(current.checkpoint.budget.deadline_ms))?;
        let ConnectorConsentStatus::Granted(grant) = status else {
            return Err(SessionError::AuthorityDenied);
        };
        let config = host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        let binding = grant.binding();
        if binding != &accepted.binding
            || binding.issuer != current.checkpoint.owner.principal
            || !config.issuers.contains(&binding.issuer)
            || binding.owner != allocation.binding().owner
            || binding.scope != allocation.binding().scope
            || binding.session != allocation.binding().session
            || binding.run != allocation.binding().run
            || binding.step != allocation.binding().step
            || binding.dispatch != allocation.binding().dispatch
            || binding.policy != policy
            || binding.policy_revision != policy_revision
            || access.now_ms()? >= binding.expires_at_ms
        {
            return Err(SessionError::AuthorityDenied);
        }
        effective_until = effective_until.min(binding.expires_at_ms);
        // Reading retained state needs no mutation grant. Only the actual
        // request can turn its exact accepted consent into ActionContext facts.
        if let Some(request) = action_request {
            grant.check_dispatch(
                &current.checkpoint.owner,
                &allocation.binding().scope,
                &allocation.binding().session,
                &allocation.binding().run,
                binding.issued_revision,
                &allocation.binding().step,
                &allocation.binding().dispatch,
                request,
                &policy,
                &policy_revision,
                access.now_ms()?,
                false,
            )?;
            action
                .policy_context
                .steps
                .get_mut(&allocation.binding().step)
                .ok_or(SessionError::AuthorityDenied)?
                .consents
                .insert(request.capability.clone(), Approval::Granted);
            receipt = Some(
                NonEmptyText::new(format!(
                    "{}@{}#{}",
                    accepted.record.0.id().as_str(),
                    accepted.record.0.revision().as_str(),
                    accepted.record.0.digest()
                ))
                .map_err(|_| invalid())?,
            );
        }
    }
    guard(effective_until)?;
    let action_context = if action_request.is_some() {
        match compile(&action) {
            Ok(context) => Some(context),
            Err(ContextApplicationError::NotAuthorized(_)) => None,
            Err(_) => return Err(SessionError::AuthorityDenied),
        }
    } else {
        None
    };
    if producer_context.context().execution_context().id() == &action.projection.id {
        return Err(SessionError::StaleRevision);
    }
    let mut action_input = action_input;
    action_input.facts = action
        .policy_context
        .steps
        .get(&allocation.binding().step)
        .ok_or(SessionError::AuthorityDenied)?
        .clone();
    guard(effective_until)?;
    Ok(NativeContexts {
        checkpoint: current.checkpoint.clone(),
        runtime: runtime.clone(),
        canonical_policy,
        producer_projection,
        action_execution: NonEmptyText::new(action.projection.id.as_str())
            .map_err(|_| invalid())?,
        resolved: action.resolved.clone(),
        basis: action.resolved.report.basis.clone(),
        basis_json,
        canonical_pins,
        canonical_documents,
        rule_fingerprint,
        authority: action.authority,
        action: action_input,
        producer: producer_input,
        producer_context,
        action_context,
        policy,
        policy_revision,
        receipt,
        consent_required: host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?
            .consent_required,
        deadline_ms: effective_until,
        deadline: UnixTimestamp::new(
            i64::try_from(effective_until / 1000).map_err(|_| SessionError::LimitExceeded)?,
        ),
    })
}
impl ConnectorSourceBasis {
    /// Actual native compilation results, not planned ids or a dispatch grant.
    pub(super) fn producer_context(&self) -> &CompiledStep {
        &self.native.producer_context
    }
    pub(super) fn action_context(&self) -> Option<&CompiledStep> {
        self.native.action_context.as_ref()
    }
    pub(super) fn runtime(&self) -> &RuntimeAdmission {
        &self.runtime
    }
    pub(super) fn capture(
        access: Arc<LocalSessionAccess>,
        allocation: &SessionResourceAllocation,
        action_request: Option<&ConnectorRequest>,
    ) -> Result<Self, SessionError> {
        if std::time::Instant::now() >= allocation.deadline() {
            return Err(SessionError::LimitExceeded);
        }
        let (host, repository, current, store_definition) = access
            .current_repository_claim_with_definition(
                &allocation.binding().owner,
                &allocation.binding().session,
                None,
            )?;
        access.validate_allocation_at(allocation, &host, &current)?;
        let runtime = host
            .sessions
            .as_ref()
            .and_then(|config| config.connector_runtime.as_ref())
            .ok_or(SessionError::UnsupportedCapability)?
            .clone();
        let native = native_contexts(
            &access,
            &host,
            &repository,
            &current,
            allocation,
            action_request,
        )?;
        let mut pins = runtime
            .references()
            .into_iter()
            .map(|(pin, _)| pin.clone())
            .collect::<Vec<_>>();
        // All loaded transitive owner records are evidence, not permission.
        pins.extend(
            runtime
                .loaded()?
                .documents
                .values()
                .map(|document| document.pin.clone()),
        );
        pins.retain(|pin| {
            pin != runtime.policy_reference()
                && pin != &runtime.0.manifest
                && pin != &runtime.0.mapping
        });
        pins.extend(native.canonical_pins.iter().cloned());
        let mut unique = BTreeSet::new();
        pins.retain(|pin| unique.insert(pin_identity(pin)));
        let audit = ConnectorPolicyAuditContext::from_allocation(
            allocation,
            native.basis.clone(),
            ConnectorPolicyAuditPins {
                canonical_policy: runtime.policy_reference().clone(),
                runtime: runtime.reference()?,
                manifest: runtime.0.manifest.clone(),
                mapping: runtime.0.mapping.clone(),
                producer: pins,
            },
        )?;
        check_source_deadline(&access, allocation, native.deadline_ms)?;
        if repository.load(&allocation.binding().owner, &allocation.binding().session)? != current {
            return Err(SessionError::StaleRevision);
        }
        check_source_deadline(&access, allocation, native.deadline_ms)?;
        Ok(Self {
            access,
            allocation: AllocationIdentity::capture(allocation),
            expected: current,
            repository,
            store_definition,
            canonical: host
                .canonical
                .as_ref()
                .ok_or(SessionError::UnsupportedCapability)?
                .clone(),
            runtime,
            native,
            audit,
            action_request: action_request.cloned(),
        })
    }
    /// Pure attach: no config, store, provider or native catalog lookup here.
    pub(super) fn attach(
        self,
        allocation: ReadonlyAllocationView,
    ) -> Result<(Arc<LocalConnectorSource>, InventoryBinder), SessionError> {
        self.allocation.check(allocation.allocation())?;
        let source = Arc::new(LocalConnectorSource {
            identity: Arc::new(()),
            basis: self,
            allocation,
            inventory: OnceLock::new(),
            #[cfg(test)]
            timings: SourceTimings::default(),
        });
        Ok((Arc::clone(&source), InventoryBinder { source }))
    }
}
impl LocalConnectorSource {
    /// Share the same authenticated owner/store selector, never a grant or
    /// an independently supplied model authority.
    pub(super) fn model_access(&self) -> Arc<LocalSessionAccess> {
        Arc::clone(&self.basis.access)
    }
    pub(super) fn producer_context(&self) -> &CompiledStep {
        self.basis.producer_context()
    }
    pub(super) fn action_context(&self) -> Option<&CompiledStep> {
        self.basis.action_context()
    }
    pub(super) fn runtime(&self) -> &RuntimeAdmission {
        self.basis.runtime()
    }
    #[cfg(test)]
    pub(super) fn diagnostic_timings(&self) -> (u64, u64, u64, u64, u64) {
        use std::sync::atomic::Ordering::Relaxed;
        (
            self.timings.calls.load(Relaxed),
            self.timings.total.load(Relaxed) / 1000,
            self.timings.fresh.load(Relaxed) / 1000,
            self.timings.native.load(Relaxed) / 1000,
            self.timings.claims.load(Relaxed) / 1000,
        )
    }
    /// Fresh source policy/claim revalidation only; never validates a different
    /// model allocation or grants a model invocation.
    pub(super) fn validate_context_checkpoint(
        &self,
        checkpoint: &SessionCheckpoint,
    ) -> Result<(), SessionError> {
        if checkpoint != &self.basis.expected.checkpoint {
            return Err(SessionError::StaleRevision);
        }
        self.validate_producer_current()
    }
    pub(super) fn context_resource_binding(&self) -> &ResourceReservationBinding {
        self.allocation.allocation().binding()
    }
    /// Current, checked tool metadata restricted to explicit proposal
    /// associations. Fixed-read dependencies are not exposed as proposal tools.
    pub(super) fn proposal_tools_current(
        &self,
    ) -> Result<BTreeMap<CapabilityId, ToolDefinition>, SessionError> {
        self.current()?;
        self.proposal_tools_bound()
    }
    fn proposal_tools_bound(&self) -> Result<BTreeMap<CapabilityId, ToolDefinition>, SessionError> {
        let inventory = self.inventory.get().ok_or(SessionError::InvalidState)?;
        inventory
            .association
            .proposal_capabilities()
            .into_iter()
            .map(|capability| {
                let tool = inventory
                    .tools
                    .get(&capability)
                    .ok_or(SessionError::UnsupportedCapability)?
                    .clone();
                Ok((capability, tool))
            })
            .collect()
    }
    pub(super) fn model_inputs_current(&self) -> Result<ModelInputsCurrent, SessionError> {
        let native = self.current()?;
        let proposal_tools = self.proposal_tools_bound()?;
        Ok(ModelInputsCurrent {
            source_identity: Arc::clone(&self.identity),
            inventory: self
                .inventory
                .get()
                .ok_or(SessionError::InvalidState)?
                .inventory
                .inventory()
                .clone(),
            native,
            binding: self.allocation.allocation().binding().clone(),
            reservation: self.allocation.allocation().reservation_id().clone(),
            claim_revision: self.allocation.allocation().claim_revision(),
            proposal_tools,
        })
    }
    /// Owned native planning data from the same freshly validated authority
    /// capture; never a CLI record alias or a cached authority grant.
    pub(super) fn resolved_plan_current(
        &self,
    ) -> Result<gateway_application::resolution_application::ResolvedPlan, SessionError> {
        self.current().map(|native| native.resolved)
    }
    pub(super) fn validate_producer_current(&self) -> Result<(), SessionError> {
        self.current().map(|_| ())
    }
    fn current(&self) -> Result<NativeContexts, SessionError> {
        #[cfg(test)]
        self.timings
            .calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(test)]
        let _total = ProfileTimer(&self.timings.total, std::time::Instant::now());
        #[cfg(test)]
        let _profile_total = profiling::span("source.current.total");
        let b = &self.basis;
        let allocation = self.allocation.allocation();
        b.allocation.check(allocation)?;
        check_source_deadline(&b.access, allocation, b.native.deadline_ms)?;
        let lookup = {
            #[cfg(test)]
            let _profile_fresh = profiling::span("source.current.fresh_lookup");
            #[cfg(test)]
            let _fresh = ProfileTimer(&self.timings.fresh, std::time::Instant::now());
            b.access.current_repository_claim_with_definition(
                &allocation.binding().owner,
                &allocation.binding().session,
                Some(b.expected.checkpoint.budget.deadline_ms),
            )
        };
        let (host, current_repository, current, current_definition) = lookup?;
        b.access
            .validate_allocation_at(allocation, &host, &current)?;
        if !current_definition.matches(&b.store_definition) || current != b.expected {
            return Err(SessionError::StaleRevision);
        }
        let original = {
            #[cfg(test)]
            let _profile_original = profiling::span("source.current.original_claim");
            #[cfg(test)]
            let _claims = ProfileTimer(&self.timings.claims, std::time::Instant::now());
            b.repository
                .load(&allocation.binding().owner, &allocation.binding().session)
        }?;
        if original != b.expected {
            return Err(SessionError::StaleRevision);
        }
        let runtime = host
            .sessions
            .as_ref()
            .and_then(|config| config.connector_runtime.as_ref())
            .ok_or(SessionError::UnsupportedCapability)?;
        if runtime.reference()? != b.runtime.reference()? {
            return Err(SessionError::StaleRevision);
        }
        let canonical = host
            .canonical
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        if canonical.catalog != b.canonical.catalog
            || canonical.plan != b.canonical.plan
            || canonical.rules != b.canonical.rules
            || canonical.process != b.canonical.process
            || canonical.policy != b.canonical.policy
            || canonical.pinned_basis != b.canonical.pinned_basis
        {
            return Err(SessionError::StaleRevision);
        }
        let native = {
            #[cfg(test)]
            let _profile_native = profiling::span("source.current.native_assembly");
            #[cfg(test)]
            let _native = ProfileTimer(&self.timings.native, std::time::Instant::now());
            native_contexts(
                &b.access,
                &host,
                &current_repository,
                &current,
                allocation,
                b.action_request.as_ref(),
            )
        }?;
        let context_bytes = |context: &CompiledStep| context.to_json().map_err(|_| invalid());
        if context_bytes(&native.producer_context)? != context_bytes(&b.native.producer_context)?
            || native
                .action_context
                .as_ref()
                .map(&context_bytes)
                .transpose()?
                != b.native
                    .action_context
                    .as_ref()
                    .map(&context_bytes)
                    .transpose()?
        {
            return Err(SessionError::StaleRevision);
        }
        if native.basis != b.native.basis
            || native.canonical_pins != b.native.canonical_pins
            || native.rule_fingerprint != b.native.rule_fingerprint
            || native.consent_required != b.native.consent_required
            || native.basis_json != b.native.basis_json
            || native.policy_revision != b.native.policy_revision
            || native.policy != b.native.policy
            || native.receipt != b.native.receipt
            || native.deadline_ms != b.native.deadline_ms
            || native.deadline != b.native.deadline
            || native.producer_context.context().execution_context().id()
                != b.native.producer_context.context().execution_context().id()
        {
            return Err(SessionError::StaleRevision);
        }
        check_source_deadline(&b.access, allocation, native.deadline_ms)?;
        let final_claim = {
            #[cfg(test)]
            let _profile_final = profiling::span("source.current.final_claim");
            #[cfg(test)]
            let _claims = ProfileTimer(&self.timings.claims, std::time::Instant::now());
            b.repository
                .load(&allocation.binding().owner, &allocation.binding().session)
        }?;
        if final_claim != b.expected {
            return Err(SessionError::StaleRevision);
        }
        check_source_deadline(&b.access, allocation, native.deadline_ms)?;
        Ok(native)
    }
    fn matched<'a>(
        &'a self,
        request: &'a ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<(super::super::selector_association::MatchedRequest<'a>, bool), SessionError> {
        let inventory = self.inventory.get().ok_or(SessionError::AuthorityDenied)?;
        let record = inventory
            .inventory
            .entries()
            .iter()
            .find(|entry| entry.mapping.capability == request.capability)
            .ok_or(SessionError::AuthorityDenied)?;
        if source.is_none()
            && let Ok(matched) = inventory
                .association
                .validate_proposal(&record.mapping.reference, request)
        {
            return Ok((matched, true));
        }
        let read = self
            .basis
            .runtime
            .reads()
            .into_iter()
            .find(|read| {
                read.mapping_reference == record.mapping.reference
                    && read.capability == request.capability
                    && read.arguments == request.arguments
            })
            .ok_or(SessionError::AuthorityDenied)?;
        if !matches!(
            record.mapping.class.operation_class(),
            ConnectorOperationClass::Read
                | ConnectorOperationClass::Search
                | ConnectorOperationClass::Inspect
        ) || record.mapping.source.as_ref() != Some(&read.source)
        {
            return Err(SessionError::AuthorityDenied);
        }
        let provider = match &self.basis.runtime.0.snapshots {
            SnapshotAdmission::AtomicCompleteRead {
                provider_contract, ..
            }
            | SnapshotAdmission::VersionedCompleteSet {
                provider_contract, ..
            } => provider_contract,
        };
        let exact_export = FixedReadJoin {
            mapping_reference: &record.mapping.reference,
            source,
            arguments: &read.arguments,
            scope_export: Some(provider),
        };
        // One wire-identical read can serve several host-owned read roles.
        // Its explicit operator association, never list order, owns the join.
        inventory
            .association
            .validate_fixed_read(request, exact_export)
            .or_else(|_| {
                inventory.association.validate_fixed_read(
                    request,
                    FixedReadJoin {
                        mapping_reference: &record.mapping.reference,
                        source,
                        arguments: &read.arguments,
                        scope_export: None,
                    },
                )
            })
            .map(|matched| (matched, false))
    }
    /// Check the snapshot factory's explicit registered export route. A normal
    /// CG09 allow alone never proves complete-universe snapshot membership.
    pub(super) fn validate_scope_export_read(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
        expected_provider: &Pin,
    ) -> Result<(), SessionError> {
        self.snapshot(request, source)?;
        let (provider, reads) = match &self.basis.runtime.0.snapshots {
            SnapshotAdmission::AtomicCompleteRead {
                provider_contract,
                read,
                ..
            } => (provider_contract, vec![read]),
            SnapshotAdmission::VersionedCompleteSet {
                provider_contract,
                reads,
                ..
            } => (provider_contract, reads.iter().collect()),
        };
        if provider != expected_provider {
            return Err(SessionError::StaleRevision);
        }
        let inventory = self.inventory.get().ok_or(SessionError::AuthorityDenied)?;
        let read = reads
            .into_iter()
            .find(|read| {
                read.capability == request.capability && read.arguments == request.arguments
            })
            .ok_or(SessionError::AuthorityDenied)?;
        inventory
            .association
            .validate_fixed_read(
                request,
                FixedReadJoin {
                    mapping_reference: &read.mapping_reference,
                    source,
                    arguments: &read.arguments,
                    scope_export: Some(provider),
                },
            )
            .map(|_| ())
    }
    fn snapshot(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, SessionError> {
        self.policy_snapshot(request, source, true)
    }
    fn policy_snapshot(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
        require_action_context: bool,
    ) -> Result<ConnectorPolicySnapshot, SessionError> {
        let current = self.current()?;
        self.policy_snapshot_in_current(request, source, require_action_context, &current)
    }
    fn policy_snapshot_in_current(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
        require_action_context: bool,
        current: &NativeContexts,
    ) -> Result<ConnectorPolicySnapshot, SessionError> {
        let (_, proposal) = self.matched(request, source)?;
        if !current
            .action
            .capabilities
            .contains_key(&request.capability)
            || (!proposal
                && !current
                    .producer
                    .capabilities
                    .contains_key(&request.capability))
        {
            return Err(SessionError::AuthorityDenied);
        }
        if proposal && require_action_context {
            if self.allocation.allocation().phase() != SessionResourcePhase::Execution {
                return Err(SessionError::AuthorityDenied);
            }
            let compiled = current
                .action_context
                .as_ref()
                .ok_or(SessionError::AuthorityDenied)?;
            if request.execution.as_str() != compiled.context().execution_context().id().as_str()
                || compiled.context().execution_context().policy_id() != &current.policy
            {
                return Err(SessionError::StaleRevision);
            }
            if self.basis.action_request.as_ref() != Some(request) {
                return Err(SessionError::AuthorityDenied);
            }
        } else if !proposal
            && request.execution.as_str()
                != current
                    .producer_context
                    .context()
                    .execution_context()
                    .id()
                    .as_str()
        {
            return Err(SessionError::StaleRevision);
        }
        Ok(ConnectorPolicySnapshot {
            step: current.action.step.clone(),
            resolved: current.action.resolved,
            has_prerequisites: current.action.has_prerequisites,
            constraints: current.action.constraints.clone(),
            preconditions: current.action.preconditions.clone(),
            authority: current.authority.clone(),
            facts: current.action.facts.clone(),
            operating_mode: current.action.operating_mode,
            execution_profile: current.action.execution_profile,
            process: current.action.process.clone(),
            consent_required: current.consent_required,
            administrative_capabilities: BTreeSet::new(),
            policy: current.policy.clone(),
            policy_revision: current.policy_revision.clone(),
            consent_receipt: current.receipt.clone(),
            budget_reservation: NonEmptyText::new(
                self.allocation.allocation().reservation_id().as_str(),
            )
            .map_err(|_| invalid())?,
            deadline: current.deadline,
        })
    }
    /// Data-only policy preview inside one guarded batch. The caller must
    /// retain the surrounding NativeRun current guards; this is no dispatch.
    pub(super) fn preview_in_view(
        &self,
        view: &ModelInputsCurrent,
        request: &ConnectorRequest,
    ) -> Result<SourcePolicyPreview, ConnectorFailure> {
        let check = || -> Result<ConnectorPolicySnapshot, SessionError> {
            if !Arc::ptr_eq(&view.source_identity, &self.identity)
                || view.binding != *self.allocation.allocation().binding()
                || view.runtime().reference()? != self.basis.runtime.reference()?
                || view.checkpoint() != &self.basis.expected.checkpoint
            {
                return Err(SessionError::StaleRevision);
            }
            let (_, proposal) = self.matched(request, None)?;
            if !proposal {
                return Err(SessionError::AuthorityDenied);
            }
            self.policy_snapshot_in_current(request, None, false, &view.native)
        };
        let snapshot = check().map_err(|error| denied(request, error))?;
        let report =
            gateway_application::connector_policy::preview_connector_policy(request, &snapshot)?;
        Ok(SourcePolicyPreview {
            policy: snapshot.policy,
            policy_revision: snapshot.policy_revision,
            deadline: snapshot.deadline,
            report,
        })
    }
    pub(super) fn preview(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<SourcePolicyPreview, ConnectorFailure> {
        let (_, proposal) = self
            .matched(request, source)
            .map_err(|error| denied(request, error))?;
        if !proposal {
            return Err(denied(request, SessionError::AuthorityDenied));
        }
        let snapshot = self
            .policy_snapshot(request, source, false)
            .map_err(|error| denied(request, error))?;
        let report =
            gateway_application::connector_policy::preview_connector_policy(request, &snapshot)?;
        Ok(SourcePolicyPreview {
            policy: snapshot.policy,
            policy_revision: snapshot.policy_revision,
            deadline: snapshot.deadline,
            report,
        })
    }
    pub(super) fn validate_response(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
        response: &ProtocolResult,
    ) -> Result<(), ConnectorFailure> {
        self.matched(request, source)
            .map_err(|error| {
                let mut failure = denied(request, error);
                failure.effect = if matches!(
                    request.class,
                    ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
                ) {
                    ConnectorEffect::Unknown
                } else {
                    ConnectorEffect::NoMutation
                };
                failure
            })?
            .0
            .validate_response(response)
    }
}
impl InventoryBinder {
    pub(super) fn bind(
        self,
        inventory: MappedInventory,
        tools: BTreeMap<CapabilityId, ToolDefinition>,
    ) -> Result<Arc<LocalConnectorSource>, SessionError> {
        self.source.current()?;
        let b = &self.source.basis;
        let allocation = self.source.allocation.allocation();
        let step = step(&b.runtime, allocation)?;
        let ProducerAdmission::Model(model) = &step.producer else {
            return Err(SessionError::UnsupportedCapability);
        };
        let loaded = b.runtime.loaded()?;
        let task: gateway_domain::SemanticTaskIR =
            serde_json::from_value(loaded.document(&model.task)?.clone()).map_err(|_| invalid())?;
        let join: ProducerJoinDocument =
            serde_json::from_value(loaded.document(&model.join)?.clone()).map_err(|_| invalid())?;
        for (capability, tool) in &tools {
            let definition = b
                .native
                .action
                .capabilities
                .get(capability)
                .ok_or(SessionError::AuthorityDenied)?;
            let proposed = join
                .capabilities
                .iter()
                .find(|pin| &pin.id == capability)
                .ok_or(SessionError::AuthorityDenied)?;
            if &tool.binding != proposed {
                return Err(SessionError::StaleRevision);
            }
            let registered = reference_pin(proposed)?;
            super::super::capability_alias::validate(
                &allocation.binding().scope,
                proposed,
                &registered,
                loaded.document(&registered)?,
                definition,
            )?;
        }
        let objects = loaded
            .documents
            .values()
            .filter(|document| document.pin.contract.as_str() == "cg.connector-object")
            .map(|document| ScopeObjectDocument::from_json(&document.pin, &document.canonical))
            .collect::<Result<Vec<_>, _>>()?;
        let selector = SelectorAssociation::from_json(
            &model.association,
            &serde_json::to_vec(loaded.document(&model.association)?).map_err(|_| invalid())?,
        )?;
        let action_execution = NonEmptyText::new(
            loaded.document(&step.action_projection)?["id"]
                .as_str()
                .ok_or(invalid())?,
        )
        .map_err(|_| invalid())?;
        let read_execution = NonEmptyText::new(
            b.native
                .producer_context
                .context()
                .execution_context()
                .id()
                .as_str(),
        )
        .map_err(|_| invalid())?;
        let provider = match &b.runtime.0.snapshots {
            SnapshotAdmission::AtomicCompleteRead {
                provider_contract, ..
            }
            | SnapshotAdmission::VersionedCompleteSet {
                provider_contract, ..
            } => provider_contract,
        };
        let association = selector.admit(AssociationContext {
            task_pin: &model.task,
            task: &task,
            action_join: &model.join,
            action_projection: &step.action_projection,
            read_projection: &model.projection,
            read_execution: &read_execution,
            action_step: &allocation.binding().step,
            execution: &action_execution,
            mapping_pin: &b.runtime.0.mapping,
            mapping_document: loaded.document(&b.runtime.0.mapping)?,
            inventory: &inventory,
            tools: &tools,
            objects: &objects,
            scope_export: Some(provider),
        })?;
        self.source
            .inventory
            .set(InventoryBinding {
                tools,
                inventory,
                association,
            })
            .map_err(|_| SessionError::InvalidState)?;
        Ok(self.source)
    }
}
/// A current report used to request exact consent, never a dispatch token.
pub(super) struct SourcePolicyPreview {
    pub(super) policy: PolicyId,
    pub(super) policy_revision: ContentDigest,
    pub(super) deadline: UnixTimestamp,
    pub(super) report: gateway_policy::StepPolicyReport,
}
#[derive(Clone)]
pub(super) struct ConnectorPolicyHandle(Arc<LocalConnectorSource>);
impl LocalConnectorSource {
    pub(super) fn policy_handle(self: &Arc<Self>) -> ConnectorPolicyHandle {
        ConnectorPolicyHandle(Arc::clone(self))
    }
}
impl crate::mcp_client::scalar_quotes::ScalarResponseAdmission for ConnectorPolicyHandle {
    fn validate(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
        result: &ProtocolResult,
    ) -> Result<(), ConnectorFailure> {
        self.0.validate_response(request, source, result)
    }
}
impl ConnectorPolicySource for ConnectorPolicyHandle {
    fn admit_target(&self, target: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        self.0.admit_target(target)
    }
    fn authorize(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.0.authorize(request, source)
    }
    fn revalidate(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.0.revalidate(dispatch, source)
    }
    fn record_decision(
        &self,
        evaluation: &ConnectorPolicyEvaluation<'_>,
    ) -> Result<(), ConnectorFailure> {
        self.0.record_decision(evaluation)
    }
}
impl ConnectorPolicySource for LocalConnectorSource {
    fn admit_target(&self, target: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        let correlation = NonEmptyText::new("connector-source-target").expect("static identifier");
        let failure = |error| ConnectorFailure {
            code: if error == SessionError::ScopeDenied {
                ConnectorFailureCode::ScopeMismatch
            } else {
                ConnectorFailureCode::PermissionDenied
            },
            effect: ConnectorEffect::NotDispatched,
            correlation: correlation.clone(),
        };
        self.current().map_err(failure)?;
        if target.connector != self.basis.runtime.0.connector
            || target.scope != self.allocation.allocation().binding().scope
        {
            return Err(failure(SessionError::ScopeDenied));
        }
        Ok(())
    }
    fn authorize(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.snapshot(request, source)
            .map_err(|error| denied(request, error))
    }
    fn revalidate(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.snapshot(&dispatch.request, source)
            .map_err(|error| denied(&dispatch.request, error))
    }
    fn record_decision(
        &self,
        evaluation: &ConnectorPolicyEvaluation<'_>,
    ) -> Result<(), ConnectorFailure> {
        // Audit binds the already-evaluated immutable capture. No current
        // reload may relabel old facts with alleged new provenance.
        let snapshot = evaluation.snapshot();
        let native = &self.basis.native;
        if snapshot.step != native.action.step
            || snapshot.policy != native.policy
            || snapshot.policy_revision != native.policy_revision
            || snapshot.facts != native.action.facts
            || snapshot.constraints != native.action.constraints
            || snapshot.preconditions != native.action.preconditions
            || snapshot.resolved != native.action.resolved
            || snapshot.has_prerequisites != native.action.has_prerequisites
            || snapshot.operating_mode != native.action.operating_mode
            || snapshot.execution_profile != native.action.execution_profile
            || snapshot.process != native.action.process
            || snapshot.authority.policies != native.authority.policies
            || snapshot.authority.capabilities != native.authority.capabilities
            || snapshot.authority.constraints != native.authority.constraints
            || snapshot.authority.required_evidence != native.authority.required_evidence
            || snapshot.consent_receipt != native.receipt
            || snapshot.deadline != native.deadline
            || snapshot.consent_required != native.consent_required
            || !snapshot.administrative_capabilities.is_empty()
            || snapshot.budget_reservation.as_str()
                != self.allocation.allocation().reservation_id().as_str()
        {
            return Err(denied(evaluation.request(), SessionError::StaleRevision));
        }
        self.matched(evaluation.request(), evaluation.source())
            .map_err(|error| denied(evaluation.request(), error))?;
        self.basis
            .repository
            .record_connector_policy_decision(
                &self.allocation.allocation().binding().owner,
                &self.basis.audit,
                evaluation,
            )
            .map(|_| ())
            .map_err(|error| denied(evaluation.request(), error))
    }
}
// Both joined methods below check the captured exact allocation and call
// current()/snapshot(), including the full freshly authenticated source basis.
impl crate::mcp_client::session_budget::QualifiedConnectorAuthority for LocalConnectorSource {}

impl SessionResourceAuthorityPort for LocalConnectorSource {
    fn validate_allocation(
        &self,
        allocation: &SessionResourceAllocation,
    ) -> Result<(), SessionError> {
        self.basis.allocation.check(allocation)?;
        self.current().map(|_| ())
    }
    fn validate_connector_allocation(
        &self,
        allocation: &SessionResourceAllocation,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<(), SessionError> {
        self.basis.allocation.check(allocation)?;
        self.snapshot(request, source).map(|_| ())
    }
    fn validate_connector_discovery(
        &self,
        allocation: &SessionResourceAllocation,
        target: &ConnectorTarget,
    ) -> Result<(), SessionError> {
        self.basis.allocation.check(allocation)?;
        self.current()?;
        if target.connector != self.basis.runtime.0.connector
            || target.scope != allocation.binding().scope
        {
            return Err(SessionError::ScopeDenied);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp_client::{
        CallContext, ProtocolInventory, mapping::DiscoveryMapper,
        session_budget::SessionConnectorRun,
    };
    use gateway_application::{
        connector_policy::Cg09ConnectorAdmission, connectors::ConnectorAdmissionPort,
    };
    use std::cell::Cell;

    struct Probe<'a> {
        local: LocalSessionHost,
        plan: SessionResourcePlan,
        called: &'a Cell<bool>,
        callback:
            &'a dyn Fn(&LocalSessionHost, SessionResourceAllocation) -> Result<(), SessionError>,
    }
    impl InteractionAuthorityPort for Probe<'_> {
        fn load_consent(
            &self,
            o: &OwnerBinding,
            r: &ConsentRecordRef,
        ) -> Result<ConsentStatus, SessionError> {
            self.local.load_consent(o, r)
        }
        fn live_status(&self, g: &VerifiedConsent) -> Result<ConsentStatus, SessionError> {
            self.local.live_status(g)
        }
    }
    impl StructuredSessionHost for Probe<'_> {
        fn now_ms(&self) -> Result<u64, SessionError> {
            self.local.now_ms()
        }
        fn register(
            &self,
            o: &OwnerBinding,
            i: Intent,
            e: RequestedExecution,
        ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
            self.local.register(o, i, e)
        }
        fn prepare(&self, _: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
            Err(SessionError::UnsupportedCapability)
        }
        fn disclose(&self, o: &OwnerBinding, s: &SessionId) -> Result<(), SessionError> {
            self.local.disclose(o, s)
        }
        fn issuer(&self, o: &OwnerBinding, i: &PrincipalId) -> Result<(), SessionError> {
            self.local.issuer(o, i)
        }
        fn resource_limits(
            &self,
            o: &OwnerBinding,
            g: &SupportedSessionGoal,
        ) -> Result<Option<SessionResourceLimits>, SessionError> {
            self.local.resource_limits(o, g)
        }
        fn connector_runtime(&self) -> Result<&dyn ConnectorSessionRuntimePort, SessionError> {
            Ok(self)
        }
    }
    impl ConnectorSessionRuntimePort for Probe<'_> {
        fn resource_plan(
            &self,
            _: &SessionCheckpoint,
            _: &PlanStepId,
        ) -> Result<SessionResourcePlan, SessionError> {
            Ok(self.plan.clone())
        }
        fn initial_assessment(
            &self,
            c: &SessionCheckpoint,
        ) -> Result<InitialAssessment, SessionError> {
            self.local.connector_runtime.initial_assessment(c)
        }
        fn prepare_step(
            &self,
            _: &SessionCheckpoint,
            _: &PlanStepId,
        ) -> Result<PreparedConnectorTurn, SessionError> {
            Err(SessionError::UnsupportedCapability)
        }
        fn prepare_authorized_step(
            &self,
            _: &SessionCheckpoint,
            _: &PlanStepId,
            r: &mut SessionPhaseResources,
            _: ConnectorPreparationContext<'_>,
        ) -> Result<ConnectorPreparation, SessionError> {
            self.called.set(true);
            let allocation = r.allocate(SessionResourceAmounts {
                connector_calls: 1,
                retrieval_rounds: 1,
                retrieval_results: 2,
                retrieval_tokens: 100,
                ..Default::default()
            })?;
            (self.callback)(&self.local, allocation)
                .expect("the complete native-source qualification callback must succeed before the deliberate unsupported-turn boundary");
            Err(SessionError::UnsupportedCapability)
        }
        fn execute_step(
            &self,
            _: &SessionCheckpoint,
            _: &PlanStepId,
            _: &mut SessionPhaseResources,
            _: &DispatchId,
            _: &PreparedConnectorTurn,
        ) -> Result<ConnectorSessionTurn, SessionError> {
            unreachable!("source probe dispatches nothing")
        }
        fn verify_goal(
            &self,
            _: &SessionCheckpoint,
            _: &PlanStepId,
            _: &DispatchId,
            _: &[PersistedConnectorReadback],
        ) -> Result<Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>, SessionError>
        {
            Err(SessionError::UnsupportedCapability)
        }
    }
    fn journal(connection: &str, scope: &ContextScopeId) -> Option<String> {
        let mut db = postgres::Client::connect(connection, postgres::NoTls).unwrap();
        db.query_opt("SELECT payload FROM cg_cognitive_journals WHERE scope=$1 AND kind='connector-policy-decisions-v1'",&[&scope.as_str()]).unwrap().map(|row|row.get(0))
    }
    #[test]
    fn canonical_owner_pins_cover_same_actual_document_bytes_and_contract_roles() {
        let mut fixture = super::super::tests::Fixture::new();
        fixture.install_actual_native_model();
        let mut host = fixture.host().unwrap();
        let pins = canonical_document_pins(&host).unwrap();
        assert_eq!(
            pins.iter()
                .map(|pin| pin.contract.as_str())
                .collect::<Vec<_>>(),
            vec!["cg.plan", "cg.composition-rules", "cg.process-snapshot"]
        );
        let original = host.resources.clone();
        let rules = host
            .resources
            .iter_mut()
            .find(|record| record["reference"]["id"] == "native-rules")
            .unwrap();
        rules["document"]["resolution"]["semantics"]["unused.native.guard"] = json!("ALWAYS");
        let canonical = host.canonical.as_ref().unwrap();
        assert!(
            host_mapping::capture(
                &canonical.catalog,
                &canonical.documents(&host.resources).unwrap()
            )
            .is_ok(),
            "changed body is still valid actual native configuration; old owner digest is the defect"
        );
        assert!(matches!(
            canonical_document_pins(&host),
            Err(SessionError::StaleRevision)
        ));
        host.resources = original;
        host.canonical.as_mut().unwrap().rules["contract"] = json!("cg.plan");
        let rules = host
            .resources
            .iter_mut()
            .find(|record| record["reference"]["id"] == "native-rules")
            .unwrap();
        rules["reference"]["contract"] = json!("cg.plan");
        assert!(
            matches!(
                canonical_document_pins(&host),
                Err(SessionError::UnsupportedCapability)
            ),
            "matching bytes cannot disguise the actual native rules owner as a Plan contract"
        );
    }
    #[test]
    fn precise_effective_expiry_denies_a_still_live_monotonic_allocation() {
        let f = super::super::tests::Fixture::new();
        let host = f.host().unwrap();
        let path = f.root.to_string_lossy().into_owned();
        let access = LocalSessionAccess {
            binding: host.binding.clone(),
            admission: f.root.join("admission.json").to_string_lossy().into_owned(),
            cwd: path.clone(),
            repository: path,
            store_connection: Default::default(),
        };
        crate::session_resource_test_support::with_preparation_resources(
            host.binding.canonical_scope.clone(),
            SessionResourcePlan {
                preparation: SessionResourceAmounts {
                    connector_calls: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
            |resources, _, _| {
                let allocation = resources
                    .allocate(SessionResourceAmounts {
                        connector_calls: 1,
                        ..Default::default()
                    })
                    .unwrap();
                let now = access.now_ms().unwrap();
                assert!(allocation.deadline() > std::time::Instant::now());
                assert!(check_source_deadline(&access, &allocation, now + 60_000).is_ok());
                assert_eq!(
                    check_source_deadline(&access, &allocation, now.saturating_sub(1)),
                    Err(SessionError::LimitExceeded),
                    "expiry must deny independently of the longer still-live resource allowance"
                );
            },
        );
        // This proves the precise expiry barrier only; no accepted connector
        // consent or mutating ActionContext is manufactured by this fixture.
    }

    #[test]
    fn real_postgres_claim_native_contexts_sealed_inventory_policy_and_audit_liveness() {
        run_native_source(false, None, false)
    }
    #[test]
    fn real_mixed_native_action_retains_read_context_without_consent_or_action_grant() {
        run_native_source(true, None, false)
    }
    #[test]
    fn actual_native_execution_must_match_the_initial_authenticated_checkpoint_mode() {
        run_native_source(
            false,
            Some(RequestedExecution {
                mode: gateway_domain::OperatingMode::Hardening,
                profile: gateway_domain::ExecutionProfile::FullPath,
            }),
            false,
        )
    }
    #[test]
    fn actual_native_execution_must_match_the_initial_authenticated_checkpoint_profile() {
        run_native_source(
            false,
            Some(RequestedExecution {
                mode: gateway_domain::OperatingMode::Development,
                profile: gateway_domain::ExecutionProfile::NormalPath,
            }),
            false,
        )
    }
    #[test]
    fn actual_native_plan_must_match_the_complete_initial_checkpoint_goal() {
        run_native_source(false, None, true)
    }
    fn run_native_source(
        mixed: bool,
        execution_mismatch: Option<RequestedExecution>,
        goal_mismatch: bool,
    ) {
        let Ok(connection) = std::env::var("CG_COGNITIVE_TEST_DATABASE") else {
            return;
        };
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scope = format!("native-source-{unique}");
        let mut f = super::super::tests::Fixture::with_scope(&scope);
        let native = if mixed {
            f.install_actual_mixed_model()
        } else {
            f.install_actual_native_model()
        };
        if goal_mismatch {
            let mut intent = f.wire["mappings"][0]["sessions"]["intent"].clone();
            intent["desired_state"]["conditions"][0]["expected"]["value"] = json!(false);
            let intent: Intent = serde_json::from_value(intent).unwrap();
            f.wire["mappings"][0]["sessions"]["intent"] = json!(intent);
            let mut goal = f.runtime()["goal"].clone();
            goal["digest"] = json!(gateway_domain::context_content_digest(
                &serde_json::to_vec(&serde_json::to_value(&intent).unwrap()).unwrap()
            ));
            f.runtime()["goal"] = goal.clone();
            f.rewrite("verifier",json!({"schema_version":"1.0","mode":"RETAINED_OBSERVATIONS","scope":scope,"goal":goal,"max_iterations":2,"max_retries":1}));
            let task_pin = f.runtime()["steps"][0]["producer"]["task"].clone();
            let mut task: Value = serde_json::from_slice(
                &std::fs::read(
                    f.root
                        .join(format!("{}.json", task_pin["id"].as_str().unwrap())),
                )
                .unwrap(),
            )
            .unwrap();
            task["desired_state"] = json!(intent.desired_state());
            f.rewrite(task_pin["id"].as_str().unwrap(), task);
            f.refresh_association();
        }
        if let Some(execution) = execution_mismatch {
            f.wire["mappings"][0]["sessions"]["execution"] = json!(execution);
            f.runtime()["execution"] = json!(execution);
        }
        let same_read = f.runtime()["snapshots"]["read"].clone();
        f.runtime()["steps"][0]["post_effect_reads"] = json!([same_read]);
        let database = f.root.join("database.txt");
        std::fs::write(&database, &connection).unwrap();
        f.wire["mappings"][0]["sessions"]["store_file"] = json!(database);
        let admission_file = f.root.join("admission.json");
        std::fs::write(&admission_file, f.wire.to_string()).unwrap();
        let host = f.host().unwrap();
        let path = f.root.to_string_lossy().into_owned();
        let local = LocalSessionHost::new(Arc::new(LocalSessionAccess {
            binding: host.binding.clone(),
            admission: admission_file.to_string_lossy().into_owned(),
            cwd: path.clone(),
            repository: path,
            store_connection: Default::default(),
        }));
        let owner = local.owner().unwrap();
        let config = host.sessions.as_ref().unwrap();
        let resource_plan: SessionResourcePlan =
            serde_json::from_value(f.runtime()["steps"][0]["resources"].clone()).unwrap();
        let callback = |local: &LocalSessionHost,
                        allocation: SessionResourceAllocation|
         -> Result<(), SessionError> {
            if goal_mismatch {
                let (_, claim) = local
                    .access
                    .current_claim(&allocation.binding().owner, &allocation.binding().session)?;
                assert_ne!(
                    claim.checkpoint.goal.intent().desired_state(),
                    &native.resolved.snapshot.input().desired,
                    "initial authenticated task+goal differs only in actual expected values from valid native Plan"
                );
                assert!(
                    matches!(
                        ConnectorSourceBasis::capture(Arc::clone(&local.access), &allocation, None),
                        Err(SessionError::StaleRevision)
                    ),
                    "valid native GoalA projection cannot authorize actual GoalB, even with identical subjects and valid task/registration pins"
                );
                return Ok(());
            }
            if let Some(execution) = execution_mismatch {
                assert_eq!(
                    local
                        .access
                        .current_claim(&allocation.binding().owner, &allocation.binding().session)?
                        .1
                        .checkpoint
                        .execution,
                    execution
                );
                assert!(
                    matches!(
                        ConnectorSourceBasis::capture(Arc::clone(&local.access), &allocation, None),
                        Err(SessionError::StaleRevision)
                    ),
                    "actual native execution must equal initially authenticated checkpoint execution before any context becomes usable"
                );
                return Ok(());
            }
            let basis =
                ConnectorSourceBasis::capture(Arc::clone(&local.access), &allocation, None)?;
            assert!(
                basis.action_context().is_none(),
                "before action admission only real producer context exists"
            );
            let loaded = basis.runtime().loaded()?;
            let target = ConnectorTarget {
                connector: basis.runtime.0.connector.clone(),
                scope: allocation.binding().scope.clone(),
            };
            let discovery: Value = serde_json::from_slice(include_bytes!(
                "../../../../../tests/fixtures/connector-mapping-v1/discovery.json"
            ))
            .unwrap();
            let wire = ProtocolInventory {
                tools: loaded
                    .mapping()
                    .records()
                    .iter()
                    .filter(|record| record.kind == InventoryKind::Tool)
                    .map(|record| {
                        let mut tool = discovery["tools"][0].clone();
                        tool["name"] = json!(record.name);
                        tool
                    })
                    .collect(),
                resources: vec![],
                templates: vec![],
                prompts: vec![],
            };
            // Offline schema fixture through the REAL mapper; no live provider
            // or model result is claimed by this control-plane source test.
            let mapper = DiscoveryMapper::new(
                target.clone(),
                loaded.manifest().clone(),
                loaded.mapping().clone(),
            )
            .unwrap();
            let inventory = mapper.snapshot(&wire).unwrap();
            let cap = native
                .capabilities
                .iter()
                .find(|(_, definition)| definition.class() == CapabilityClass::Inspect)
                .unwrap()
                .0
                .clone();
            let step = step(&basis.runtime, &allocation)?;
            let ProducerAdmission::Model(model) = &step.producer else {
                unreachable!()
            };
            let join: ProducerJoinDocument =
                serde_json::from_value(loaded.document(&model.join)?.clone()).unwrap();
            let tools = loaded
                .mapping()
                .records()
                .iter()
                .filter(|record| record.kind == InventoryKind::Tool)
                .map(|record| {
                    let schema = mapper
                        .tool_schema(&wire, &record.capability, &inventory.inventory().revision)
                        .unwrap();
                    let alias = join
                        .capabilities
                        .iter()
                        .find(|alias| alias.id == record.capability)
                        .unwrap()
                        .clone();
                    let tool = ToolDefinition::from_connector_schema(
                        alias,
                        NonEmptyText::new("Native source test").unwrap(),
                        target.clone(),
                        schema.inventory_revision.clone(),
                        schema.argument_schema_digest.clone(),
                        &schema.argument_schema,
                    )
                    .unwrap();
                    (record.capability.clone(), tool)
                })
                .collect::<BTreeMap<_, _>>();
            let action_request = ConnectorRequest {
                target: target.clone(),
                capability: cap,
                class: ConnectorOperationClass::Read,
                inventory_revision: inventory.inventory().revision.clone(),
                arguments: BTreeMap::from([(
                    NonEmptyText::new("query").unwrap(),
                    TypedValue::string("native-fixture").unwrap(),
                )]),
                task: native.task.clone(),
                step: native.action_step.clone(),
                execution: NonEmptyText::new("native-action-context").unwrap(),
                correlation: NonEmptyText::new("native-source-action").unwrap(),
            };
            let action_basis = ConnectorSourceBasis::capture(
                Arc::clone(&local.access),
                &allocation,
                Some(&action_request),
            )?;
            assert!(
                action_basis.native.action_context.is_some() != mixed,
                "native ActionContext exists only when actual whole-action policy allows; mutation consent is never fabricated"
            );
            let mut captured = None;
            let context = CallContext::new(
                NonEmptyText::new("native-source-test").unwrap(),
                allocation.deadline(),
            );
            let result = SessionConnectorRun::with_factory(
                allocation,
                Arc::clone(&local.access) as Arc<dyn SessionResourceAuthorityPort>,
                context,
                |view| {
                    let (source, binder) = basis.attach(view.clone())?;
                    let (_, action_binder) = action_basis.attach(view)?;
                    captured = Some((source, binder, action_binder));
                    Err(SessionError::UnsupportedCapability)
                },
            );
            assert!(
                matches!(result, Err(SessionError::UnsupportedCapability)),
                "test deliberately constructs no transport/run"
            );
            let (source, binder, action_binder) = captured.unwrap();
            assert!(
                source.authorize(&action_request, None).is_err(),
                "unsealed inventory denies every action"
            );
            let source = binder.bind(inventory.clone(), tools.clone())?;
            if !mixed {
                let profile_before = profiling::snapshot();
                source.current()?;
                eprintln!(
                    "content-free single current source callback (count,microseconds): {:?}",
                    profiling::delta(&profile_before)
                );
            }
            let calls_before_view = source.diagnostic_timings().0;
            let view = source.model_inputs_current()?;
            assert_eq!(
                source.diagnostic_timings().0 - calls_before_view,
                1,
                "one model view performs exactly one complete fresh source lookup"
            );
            assert_eq!(view.checkpoint(), &source.basis.expected.checkpoint);
            assert_eq!(view.binding(), source.context_resource_binding());
            assert_eq!(view.runtime().reference()?, source.runtime().reference()?);
            assert_eq!(view.resolved(), &native.resolved);
            assert_eq!(view.canonical_policy(), &native.policy);
            assert_eq!(
                view.producer_projection()["id"],
                view.producer_context()
                    .context()
                    .execution_context()
                    .id()
                    .as_str()
            );
            let proposal_tools = source.proposal_tools_current()?;
            assert_eq!(view.proposal_tools(), &proposal_tools);

            assert!(
                proposal_tools
                    .values()
                    .all(|tool| tools.get(&tool.binding.id) == Some(tool))
            );
            assert_eq!(
                proposal_tools.len(),
                if mixed { 2 } else { 1 },
                "actual fixture explicitly registers both tool proposals in the mixed profile"
            );
            assert_eq!(
                source
                    .producer_context()
                    .context()
                    .execution_context()
                    .id()
                    .as_str(),
                "native-producer-context"
            );
            assert!(source.action_context().is_none());
            assert_eq!(
                source.runtime().reference()?,
                source.basis.runtime.reference()?
            );

            let action_source = action_binder.bind(inventory, tools)?;
            assert_eq!(
                view.inventory(),
                source.inventory.get().unwrap().inventory.inventory()
            );
            assert_eq!(view.action_execution(), &action_request.execution);
            assert_eq!(
                view.canonical_documents().len(),
                view.canonical_pins().len()
            );
            for ((pin, body), alias) in view.canonical_documents().iter().zip(view.canonical_pins())
            {
                assert_eq!(pin, alias);
                assert_eq!(
                    gateway_domain::context_content_digest(&serde_json::to_vec(body).unwrap()),
                    pin.digest
                );
            }
            let calls_before_preview = source.diagnostic_timings().0;
            let journal_before_preview = journal(&connection, &target.scope);
            let view_preview = source.preview_in_view(&view, &action_request).unwrap();
            assert_eq!(
                source.diagnostic_timings().0,
                calls_before_preview,
                "same-view preview must not perform another source lookup"
            );
            assert_eq!(
                journal(&connection, &target.scope),
                journal_before_preview,
                "same-view preview must not audit or dispatch"
            );
            assert!(
                action_source
                    .preview_in_view(&view, &action_request)
                    .is_err(),
                "an otherwise equivalent distinct source cannot consume this source's view"
            );
            let mut foreign_preview = action_request.clone();
            foreign_preview.arguments.insert(
                NonEmptyText::new("query").unwrap(),
                TypedValue::string("foreign-object").unwrap(),
            );
            assert!(source.preview_in_view(&view, &foreign_preview).is_err());

            assert!(
                source.authorize(&action_request, None).is_err(),
                "planned action id cannot substitute for absent actual compiled ActionContext"
            );
            let preview = source.preview(&action_request, None).unwrap();
            assert_eq!(view_preview.report, preview.report);
            assert_eq!(view_preview.policy_revision, preview.policy_revision);
            assert_eq!(preview.policy, source.basis.native.policy);
            assert_eq!(preview.policy_revision, source.basis.native.policy_revision);
            assert_eq!(preview.deadline, source.basis.native.deadline);
            assert_eq!(
                preview.report.decision,
                gateway_policy::PolicyDecision::Allow
            );
            assert!(
                journal(&connection, &target.scope).is_none(),
                "pure preview does not record admission or dispatch"
            );
            let mut read = action_request.clone();
            read.execution = NonEmptyText::new("native-producer-context").unwrap();
            read.correlation = NonEmptyText::new("native-source-fixed-read").unwrap();
            let provider = match &source.basis.runtime.0.snapshots {
                SnapshotAdmission::AtomicCompleteRead {
                    provider_contract, ..
                }
                | SnapshotAdmission::VersionedCompleteSet {
                    provider_contract, ..
                } => provider_contract,
            };
            assert!(
                source
                    .validate_scope_export_read(&read, None, provider)
                    .is_ok(),
                "same exact fixed read remains a qualified export when also used as post-effect read"
            );
            // The explicit full-join marker cannot turn any other genuine
            // coordinator allocation into this source's allocation.
            crate::session_resource_test_support::with_preparation_resources(
                target.scope.clone(),
                SessionResourcePlan {
                    preparation: SessionResourceAmounts {
                        connector_calls: 1,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                |resources, _, _| {
                    let foreign = resources
                        .allocate(SessionResourceAmounts {
                            connector_calls: 1,
                            ..Default::default()
                        })
                        .unwrap();
                    assert!(source.validate_allocation(&foreign).is_err());
                    assert!(
                        source
                            .validate_connector_discovery(&foreign, &target)
                            .is_err()
                    );
                    assert!(
                        source
                            .validate_connector_allocation(&foreign, &read, None)
                            .is_err()
                    );
                },
            );
            let admission = Cg09ConnectorAdmission::new(source.policy_handle());
            let before = journal(&connection, &target.scope);
            let dispatch = admission.authorize(&read, None).unwrap();
            if mixed {
                assert!(
                    source
                        .basis
                        .native
                        .producer
                        .capabilities
                        .values()
                        .all(|definition| definition.class() == CapabilityClass::Inspect)
                );
                assert_eq!(source.basis.native.action.capabilities.len(), 2);
                let mut mutation = action_request.clone();
                mutation.capability = native
                    .capabilities
                    .iter()
                    .find(|(_, definition)| definition.class() == CapabilityClass::Mutate)
                    .unwrap()
                    .0
                    .clone();
                mutation.class = ConnectorOperationClass::Mutate;
                mutation.correlation = NonEmptyText::new("native-source-mutation").unwrap();
                let preview = source.preview(&mutation, None).unwrap();
                let calls_before = source.diagnostic_timings().0;
                let bounded_preview = source.preview_in_view(&view, &mutation).unwrap();
                assert_eq!(source.diagnostic_timings().0, calls_before);
                assert_eq!(
                    bounded_preview.report, preview.report,
                    "same-view real mutation preview retains RequireConsent without an ActionContext or grant"
                );
                assert_eq!(
                    preview.report.decision,
                    gateway_policy::PolicyDecision::RequireConsent
                );
                assert!(
                    source.authorize(&mutation, None).is_err(),
                    "actual mutating ActionContext is absent before exact accepted consent"
                );
                assert!(action_source.authorize(&mutation, None).is_err());
                assert!(
                    admission.validate_current_dispatch(&dispatch, None).is_ok(),
                    "fixed read has true producer context and true action read capability while mutation remains denied"
                );
                return Ok(());
            }
            let recorded = journal(&connection, &target.scope);
            assert_ne!(
                recorded, before,
                "normal CG09 admission persists real journal evidence"
            );
            for _ in 0..3 {
                admission
                    .validate_current_dispatch(&dispatch, None)
                    .unwrap();
            }
            assert_eq!(
                journal(&connection, &target.scope),
                recorded,
                "read-only liveness does not grow audit journal"
            );
            admission.revalidate(&dispatch, None).unwrap();
            assert_ne!(journal(&connection, &target.scope), recorded);
            assert!(
                Cg09ConnectorAdmission::new(action_source.policy_handle())
                    .authorize(&action_request, None)
                    .is_err(),
                "a real compiled readonly ActionContext is still no execution-phase reservation grant"
            );
            let mut foreign = read.clone();
            foreign.arguments.insert(
                NonEmptyText::new("query").unwrap(),
                TypedValue::string("foreign-object").unwrap(),
            );
            assert!(admission.authorize(&foreign, None).is_err());
            let response = ProtocolResult::Tool {
                content: vec![],
                structured: Some(
                    json!({"object":"native-fixture","id":"native-fixture","first":true,"second":true}),
                ),
                is_error: false,
            };
            assert!(source.validate_response(&read, None, &response).is_ok());
            let original_config = std::fs::read(&admission_file).unwrap();
            // Resource-only authority authenticates the current durable claim;
            // it deliberately does not promote a sidecar into an effect grant.
            // Joined Source authority must still freshly check every sidecar.
            let config_wire: Value = serde_json::from_slice(&original_config).unwrap();
            let policy_path =
                config_wire["mappings"][0]["sessions"]["connector_runtime"]["documents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|entry| entry["pin"]["id"] == "canonical-policy")
                    .unwrap()["path"]
                    .as_str()
                    .unwrap();
            let original_policy = std::fs::read(policy_path).unwrap();
            let allocation = source.allocation.allocation();
            local.access.validate_allocation(allocation)?;
            source.validate_connector_discovery(allocation, &target)?;
            source.validate_connector_allocation(allocation, &read, None)?;
            std::fs::write(policy_path, b"{\"unexpected_policy_body\":true}").unwrap();
            assert!(
                local.access.validate_allocation(allocation).is_ok(),
                "resource-only claim check is independent of executable sidecar loading"
            );
            assert!(
                source.current().is_err(),
                "full current policy capture rejects actual sidecar digest drift"
            );
            assert!(
                source.model_inputs_current().is_err(),
                "another model view must freshly reject sidecar drift rather than reuse the previous view"
            );
            assert!(
                source
                    .validate_connector_discovery(allocation, &target)
                    .is_err()
            );
            assert!(
                source
                    .validate_connector_allocation(allocation, &read, None)
                    .is_err()
            );
            let drift_failure = admission.authorize(&read, None).unwrap_err();
            assert_eq!(
                drift_failure.effect,
                ConnectorEffect::NotDispatched,
                "sidecar drift is denied before connector I/O"
            );
            std::fs::write(policy_path, &original_policy).unwrap();
            local.access.validate_allocation(allocation)?;
            source.validate_connector_discovery(allocation, &target)?;
            source.validate_connector_allocation(allocation, &read, None)?;
            let mut changed: Value = serde_json::from_slice(&original_config).unwrap();
            changed["mappings"][0]["sessions"]["connector_runtime"]["revision"] =
                json!("operator-revoked-registration");
            std::fs::write(&admission_file, changed.to_string()).unwrap();
            assert!(
                source.revalidate(&dispatch, None).is_err(),
                "runtime registration drift revokes current source"
            );
            std::fs::write(&admission_file, &original_config).unwrap();
            let mut changed: Value = serde_json::from_slice(&original_config).unwrap();
            changed["mappings"][0]["canonical"]["rules"]["revision"] =
                json!("operator-rules-registration-next");
            let resource = changed["mappings"][0]["resources"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|record| record["reference"]["id"] == "native-rules")
                .unwrap();
            resource["reference"]["revision"] = json!("operator-rules-registration-next");
            for provenance in resource["provenance"].as_array_mut().unwrap() {
                provenance["reference"]["revision"] = json!("operator-rules-registration-next");
            }
            std::fs::write(&admission_file, changed.to_string()).unwrap();
            assert!(
                matches!(source.current(), Err(SessionError::StaleRevision)),
                "same canonical rules bytes with a new actual owner revision cannot relabel the captured policy basis"
            );
            std::fs::write(&admission_file, &original_config).unwrap();
            let mut changed: Value = serde_json::from_slice(&original_config).unwrap();
            changed["mappings"][0]["sessions"]["consent_required"] = json!(true);
            std::fs::write(&admission_file, changed.to_string()).unwrap();
            assert!(
                matches!(source.current(), Err(SessionError::StaleRevision)),
                "session consent requirement is frozen with source facts"
            );
            std::fs::write(&admission_file, &original_config).unwrap();
            let original_connection = std::fs::read(&database).unwrap();
            std::fs::write(&database, b"host=127.0.0.1 port=1 connect_timeout=1").unwrap();
            assert!(
                source.revalidate(&dispatch, None).is_err(),
                "same-path current store content is authenticated; captured DB cannot substitute"
            );
            std::fs::write(&database, &original_connection).unwrap();
            let text = std::str::from_utf8(&original_connection).unwrap().trim();
            let switched = if text.starts_with("postgres://") || text.starts_with("postgresql://") {
                format!(
                    "{text}{}application_name=native-source-choice-switch",
                    if text.contains('?') { '&' } else { '?' }
                )
            } else {
                format!("{text} application_name=native-source-choice-switch")
            };
            std::fs::write(&database, switched.as_bytes()).unwrap();
            let (_, _, same_claim) = source.basis.access.current_repository_claim(
                &source.basis.expected.checkpoint.owner,
                &source.basis.expected.checkpoint.snapshot.session,
                None,
            )?;
            assert!(
                same_claim == source.basis.expected,
                "new valid locator still selects the identical actual checkpoint"
            );
            assert!(
                source.basis.repository.load(
                    &source.basis.expected.checkpoint.owner,
                    &source.basis.expected.checkpoint.snapshot.session
                )? == source.basis.expected,
                "original repository claim also remains valid"
            );
            assert!(
                matches!(source.current(), Err(SessionError::StaleRevision)),
                "a changed same-path store definition is denied even when it connects to the same DB and identical claim"
            );
            std::fs::write(&database, &original_connection).unwrap();
            assert!(
                source.current().is_ok(),
                "restoring the exact operator definition restores this still-current read-only source"
            );
            let repository = &source.basis.repository;
            let current = &source.basis.expected;
            let mut cancelled = current.checkpoint.clone();
            cancelled.snapshot.revision = cancelled.snapshot.revision.next()?;
            cancelled.snapshot.state = SessionState::Cancelled;
            cancelled.snapshot.dispatch_knowledge = DispatchKnowledge::Stopped;
            cancelled.lease_until_ms = None;
            repository.append(
                &current.checkpoint.owner,
                JournalAppend {
                    session: current.checkpoint.snapshot.session.clone(),
                    expected_revision: current.checkpoint.snapshot.revision,
                    fence: current.fence,
                    command: None,
                    next: cancelled,
                    records: vec![],
                },
            )?;
            assert!(
                admission
                    .validate_current_dispatch(&dispatch, None)
                    .is_err(),
                "real concurrent cancel invalidates captured source"
            );
            Ok(())
        };
        let called = Cell::new(false);
        let probe = Probe {
            local,
            plan: resource_plan,
            called: &called,
            callback: &callback,
        };
        let store = store(&probe.local.current().unwrap()).unwrap();
        let coordinator = SessionCoordinator::new(store, probe);
        let started = coordinator
            .execute(
                &owner,
                SessionCommand::Start {
                    command: CommandId::new(format!("native-source-start-{unique}")).unwrap(),
                    intent: config.intent.clone(),
                    execution: config.execution,
                },
            )
            .unwrap();
        assert!(
            coordinator
                .execute(
                    &owner,
                    SessionCommand::Continue {
                        at: Mutation {
                            session: started.session,
                            expected_revision: started.revision,
                            command: CommandId::new(format!("native-source-continue-{unique}"))
                                .unwrap()
                        }
                    }
                )
                .is_err()
        );
        assert!(
            called.get(),
            "the real coordinator must mint and invoke the current phase claim"
        );
    }
}
