//! Launcher-owned current admission and v2 mapping. All lifecycle lives in application.
mod capability_alias;
mod goal_verifier;
mod runtime_admission;
mod selector_association;
use crate::{
    codex_workspace::{LocalCodexHost, LocalWorkspaceResolver},
    cognitive_store::CognitiveStore,
    declarative_cli::host_mapping,
    session_store::PostgresSessionStore,
};
use gateway_application::{
    codex::*, connectors::ConnectorOperationClass,
    resolution_application::DeclarativeResolutionApplication, sessions::*,
};
use gateway_domain::{
    CapabilityClass, CapabilityDefinition, CapabilityId, Intent, PlanStepId, PolicyDefinition,
    PolicyId,
};
use gateway_policy::{
    Approval, PolicyAuthority, PolicyEngine, ProcessReadiness, StepFacts, StepPolicyInput,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::Read,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionAdmission {
    pub store_file: PathBuf,
    pub intent: Intent,
    #[serde(default)]
    pub basis: Option<ArtifactGoalBasis>,
    #[serde(default)]
    pub connector_goal: Option<ConnectorGoalAdmission>,
    pub execution: RequestedExecution,
    pub max_actions: u32,
    pub max_retries: u32,
    #[serde(default)]
    pub resource_limits: Option<SessionResourceLimits>,
    #[serde(default)]
    connector_runtime: Option<runtime_admission::RuntimeAdmission>,
    pub ttl_ms: u64,
    pub consent_required: bool,
    pub enabled: bool,
    pub issuers: Vec<PrincipalId>,
}

/// Trusted repository-owned registration facts. Wire strings are converted
/// into typed, bounded identities before a connector goal can enter a session.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConnectorGoalAdmission {
    pub plan: RecordRef,
    pub steps: Vec<String>,
    pub observations: Vec<ConnectorObservationAdmission>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConnectorObservationAdmission {
    pub condition: String,
    pub source: String,
    pub capability: String,
    pub operation_class: String,
}

fn registered_connector_goal(
    config: &SessionAdmission,
    scope: &gateway_domain::ContextScopeId,
) -> Result<SupportedConnectorGoal, SessionError> {
    let registered = config
        .connector_goal
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    let steps = registered
        .steps
        .iter()
        .map(|step| PlanStepId::new(step.clone()).map_err(|_| SessionError::InvalidInput))
        .collect::<Result<Vec<_>, _>>()?;
    let observations = registered
        .observations
        .iter()
        .map(|item| {
            let class = match item.operation_class.as_str() {
                "read" => ConnectorOperationClass::Read,
                "search" => ConnectorOperationClass::Search,
                "inspect" => ConnectorOperationClass::Inspect,
                _ => return Err(SessionError::UnsupportedCapability),
            };
            ConditionObservationMapping::new(
                gateway_domain::ConditionId::new(item.condition.clone())
                    .map_err(|_| SessionError::InvalidInput)?,
                gateway_domain::SourceId::new(item.source.clone())
                    .map_err(|_| SessionError::InvalidInput)?,
                CapabilityId::new(item.capability.clone())
                    .map_err(|_| SessionError::InvalidInput)?,
                class,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    SupportedConnectorGoal::from_host_registration(
        config.intent.clone(),
        scope.clone(),
        registered.plan.clone(),
        steps,
        observations,
    )
}

fn validate_registered_connector_plan(
    host: &LocalCodexHost,
    goal: &SupportedConnectorGoal,
) -> Result<(), SessionError> {
    let canonical = host
        .canonical
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    if canonical.plan["id"] != goal.plan().id().as_str()
        || canonical.plan["revision"] != goal.plan().revision().as_str()
        || canonical.plan["digest"] != goal.plan().digest()
    {
        return Err(SessionError::StaleRevision);
    }
    Ok(())
}

fn read(path: &str, limit: u64) -> Result<String, SessionError> {
    let file = std::fs::File::open(path).map_err(|_| SessionError::Unavailable)?;
    let mut text = String::new();
    file.take(limit + 1)
        .read_to_string(&mut text)
        .map_err(|_| SessionError::InvalidInput)?;
    if text.len() as u64 > limit {
        return Err(SessionError::LimitExceeded);
    }
    Ok(text)
}
fn store(host: &LocalCodexHost) -> Result<PostgresSessionStore, SessionError> {
    open_store(host).map(|(repository, _)| repository)
}

/// Exact locator input used to open a store, retained only in trusted memory.
/// It deliberately has no Debug, serialization, hash or plaintext accessor.
/// Equality proves an unchanged operator definition, not physical server identity.
#[derive(Clone)]
struct StoreDefinition(String);
impl StoreDefinition {
    fn matches(&self, current: &Self) -> bool {
        self.0 == current.0
    }
}

fn open_store(
    host: &LocalCodexHost,
) -> Result<(PostgresSessionStore, StoreDefinition), SessionError> {
    let definition = store_definition(host)?;
    let repository = PostgresSessionStore::new(CognitiveStore::connect(
        &definition.0,
        host.binding.canonical_scope.clone(),
    )?);
    Ok((repository, definition))
}
fn store_definition(host: &LocalCodexHost) -> Result<StoreDefinition, SessionError> {
    let config = host
        .sessions
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    if !config.store_file.is_absolute() {
        return Err(SessionError::InvalidInput);
    }
    let text = read(
        config
            .store_file
            .to_str()
            .ok_or(SessionError::InvalidInput)?,
        4096,
    )?;
    // Retain exactly the same owned input passed to connect; do not reopen the
    // locator later and attribute those different bytes to this connection.
    Ok(StoreDefinition(text.trim().to_owned()))
}
fn error(e: FacadeError) -> SessionError {
    match e {
        FacadeError::ScopeDenied => SessionError::ScopeDenied,
        FacadeError::StaleRevision => SessionError::StaleRevision,
        FacadeError::UnsupportedCapability => SessionError::UnsupportedCapability,
        FacadeError::PolicyDenied | FacadeError::SensitivityDenied => SessionError::AuthorityDenied,
        _ => SessionError::InvalidInput,
    }
}
pub struct LocalApplication {
    facade: CodexFacade<LocalCodexHost>,
    binding: ScopeBinding,
    admission: String,
    cwd: String,
    repository: String,
    enabled: bool,
}
impl LocalApplication {
    pub(crate) fn new(
        facade: CodexFacade<LocalCodexHost>,
        binding: ScopeBinding,
        admission: &str,
        cwd: &str,
        repository: &str,
        enabled: bool,
    ) -> Self {
        Self {
            facade,
            binding,
            admission: admission.into(),
            cwd: cwd.into(),
            repository: repository.into(),
            enabled,
        }
    }
    fn host(&self) -> LocalSessionHost {
        LocalSessionHost::new(Arc::new(LocalSessionAccess {
            binding: self.binding.clone(),
            admission: self.admission.clone(),
            cwd: self.cwd.clone(),
            repository: self.repository.clone(),
            store_connection: Default::default(),
        }))
    }
    fn execute_v2(&self, operation: &str, request: &Value) -> Result<Value, &'static str> {
        let decoded = boundary::decode(operation, request)?;
        if request["scope"] != self.binding.scope {
            return Err("CG_SCOPE_DENIED");
        }
        if !self.enabled {
            return Err("CG_UNSUPPORTED_CAPABILITY");
        }
        let host = self.host();
        let admitted = host.current().map_err(SessionError::code)?;
        let owner = host.owner().map_err(SessionError::code)?;
        let coordinator =
            SessionCoordinator::new(store(&admitted).map_err(SessionError::code)?, host);
        let snapshot = match decoded {
            boundary::Decoded::Command(command) => {
                if let Some(at) = command.mutation() {
                    let existing = coordinator
                        .details(&owner, &at.session)
                        .map_err(SessionError::code)?;
                    if existing.execution != boundary::execution(request)? {
                        return Err("CG_INVALID_INPUT");
                    }
                }
                coordinator
                    .execute(&owner, command)
                    .map_err(SessionError::code)?
            }
            boundary::Decoded::Inspect(query) => {
                let snapshot = coordinator
                    .inspect(&owner, query)
                    .map_err(SessionError::code)?;
                if coordinator
                    .details(&owner, &snapshot.session)
                    .map_err(SessionError::code)?
                    .execution
                    != boundary::execution(request)?
                {
                    return Err("CG_INVALID_INPUT");
                }
                snapshot
            }
        };
        let checkpoint = coordinator
            .details(&owner, &snapshot.session)
            .map_err(SessionError::code)?;
        let mut current = checkpoint.snapshot.clone();
        current.command_outcome = snapshot.command_outcome;
        Ok(boundary::response(request, current, &checkpoint))
    }
    pub fn operator(
        &self,
        session: &SessionId,
        issuer: &PrincipalId,
        decision: &str,
    ) -> Result<Value, SessionError> {
        let host = self.host();
        let admitted = host.current()?;
        let owner = host.owner()?;
        host.issuer(&owner, issuer)?;
        let repository = store(&admitted)?;
        if decision == "recover" {
            let c = SessionCoordinator::new(repository, host);
            return Ok(json!({"session":c.recover(&owner,session)?}));
        }
        let current = repository.load(&owner, session)?;
        if matches!(
            current.checkpoint.pending,
            Some(PendingInteraction::ConnectorConsent(_))
        ) || current.checkpoint.accepted_connector_consent.is_some()
        {
            let reference = repository.operator_decide_connector_consent(
                &owner,
                session,
                issuer,
                decision,
                host.now_ms()?,
            )?;
            return Ok(json!({"connector_consent_reference":reference}));
        }
        let reference = repository.issue(&owner, session, issuer, decision, host.now_ms()?)?;
        Ok(json!({"reference":reference}))
    }
}
impl CodexApplicationPort for LocalApplication {
    fn session_v2_enabled(&self) -> bool {
        self.enabled
    }
    fn execute(&self, operation: &str, request: &Value) -> Value {
        if request["schema_version"] == "2.0" {
            self.execute_v2(operation, request)
                .unwrap_or_else(boundary::failure)
        } else {
            self.facade.execute(operation, request)
        }
    }
    fn execute_with_context(
        &self,
        operation: &str,
        request: &Value,
        context: &RequestContext,
    ) -> Value {
        if request["schema_version"] != "2.0" {
            return self
                .facade
                .execute_with_context(operation, request, context);
        }
        if let Err(e) = context.check() {
            return boundary::failure(e.code());
        }
        // A transport timeout/EOF stops waiting; it never issues SessionCommand::Cancel.
        self.execute(operation, request)
    }
    fn read_resource(
        &self,
        scope: &Value,
        id: &str,
        revision: &str,
        digest: &str,
    ) -> Result<Value, FacadeError> {
        if let Ok(resource) = self.facade.read_resource(scope, id, revision, digest) {
            return Ok(resource);
        }
        if !self.enabled || scope != &self.binding.scope {
            return Err(FacadeError::ScopeDenied);
        }
        let host = self.host();
        let admitted = host.current().map_err(|_| FacadeError::ScopeDenied)?;
        let owner = host.owner().map_err(|_| FacadeError::ScopeDenied)?;
        let reference = RecordRef::new(
            RecordId::new(id).map_err(|_| FacadeError::ReferenceUnavailable)?,
            RecordId::new(revision).map_err(|_| FacadeError::ReferenceUnavailable)?,
            digest.into(),
        )
        .map_err(|_| FacadeError::ReferenceUnavailable)?;
        let repository = store(&admitted).map_err(|_| FacadeError::ReferenceUnavailable)?;
        if repository
            .record_kind(&owner, &reference)
            .map_err(|_| FacadeError::ReferenceUnavailable)?
            != RecordKind::Evidence
        {
            return Err(FacadeError::SensitivityDenied);
        }
        let bytes = repository
            .load_artifact(&owner, &reference)
            .map_err(|_| FacadeError::ReferenceUnavailable)?;
        let document: Value = serde_json::from_slice(&bytes).map_err(|_| FacadeError::Internal)?;
        let resource = json!({"schema_version":"2.0","scope":scope,"reference":reference,"contract":"cg.evidence","contract_version":"2.0","document":document});
        if !security::credential_free(&resource)
            || !contracts::valid(
                &resource,
                &boundary::artifact("resource.schema.json").unwrap(),
                &boundary::artifact("common.schema.json").unwrap(),
            )
        {
            return Err(FacadeError::SensitivityDenied);
        }
        Ok(resource)
    }
    fn read_with_context(
        &self,
        scope: &Value,
        id: &str,
        revision: &str,
        digest: &str,
        context: &RequestContext,
    ) -> Result<Value, FacadeError> {
        context.check()?;
        self.read_resource(scope, id, revision, digest)
    }
}
/// Shared current authority source. It deliberately owns neither a runtime nor
/// a coordinator: downstream adapters can share this Arc without owning a host.
pub(crate) struct LocalSessionAccess {
    binding: ScopeBinding,
    admission: String,
    cwd: String,
    repository: String,
    store_connection: std::sync::Mutex<Option<(StoreDefinition, CognitiveStore)>>,
}

struct LocalSessionHost {
    access: Arc<LocalSessionAccess>,
    connector_runtime: LocalConnectorRuntime,
}

/// Registration and pure retained-goal verification use trusted workspace
/// configuration. Preparation and execution remain unsupported until the
/// concrete model/connector factories are composed into this runtime.
struct LocalConnectorRuntime {
    access: Arc<LocalSessionAccess>,
}

impl ConnectorSessionRuntimePort for LocalConnectorRuntime {
    fn verify_pre_execution_goal(
        &self,
        checkpoint: &SessionCheckpoint,
        data: &PreExecutionGoalData,
    ) -> Result<Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>, SessionError> {
        runtime_admission::before_goal::verify(&self.access, checkpoint, data)
    }
    fn initial_assessment(
        &self,
        checkpoint: &SessionCheckpoint,
    ) -> Result<InitialAssessment, SessionError> {
        let goal = checkpoint
            .goal
            .as_connector()
            .ok_or(SessionError::UnsupportedCapability)?;
        Ok(InitialAssessment {
            goal: content_reference(
                "supported-goal",
                &serde_json::to_vec(checkpoint.goal.intent())
                    .map_err(|_| SessionError::InvalidInput)?,
            )?,
            // The plan is the only registered source basis available at this
            // boundary. Outcome remains UNKNOWN; this is never a CG-09 allow.
            authority: goal.plan().clone(),
            outcome: "UNKNOWN".into(),
        })
    }

    fn prepare_step(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
    ) -> Result<PreparedConnectorTurn, SessionError> {
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
        Err(SessionError::UnsupportedCapability)
    }

    fn verify_goal(
        &self,
        checkpoint: &SessionCheckpoint,
        step: &PlanStepId,
        dispatch: &DispatchId,
        readbacks: &[PersistedConnectorReadback],
    ) -> Result<Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>, SessionError>
    {
        goal_verifier::verify(&self.access, checkpoint, step, dispatch, readbacks)
    }
}

impl LocalSessionAccess {
    /// Reuse only a live connection to the freshly selected exact definition.
    /// No claim, consent or authority result is retained in this slot. A failed
    /// query is returned to the caller and is never replayed here.
    fn open_current_store(
        &self,
        host: &LocalCodexHost,
    ) -> Result<(PostgresSessionStore, StoreDefinition), SessionError> {
        let definition = store_definition(host)?;
        let mut slot = self
            .store_connection
            .lock()
            .map_err(|_| SessionError::StorageUnavailable)?;
        if let Some((selected, store)) = slot.as_ref()
            && selected.matches(&definition)
            && store.scope() == &host.binding.canonical_scope
            && !store.is_closed()?
        {
            return Ok((PostgresSessionStore::new(store.clone()), definition));
        }
        // Do not fall back to an old connection if selecting the new one fails.
        *slot = None;
        let store = CognitiveStore::connect(&definition.0, host.binding.canonical_scope.clone())?;
        *slot = Some((definition.clone(), store.clone()));
        Ok((PostgresSessionStore::new(store), definition))
    }
    pub(crate) fn current(&self) -> Result<LocalCodexHost, SessionError> {
        #[cfg(test)]
        let _profile = runtime_admission::profiling::span("access.full_current");
        let mut host = self.current_registration()?;
        let loaded = host
            .sessions
            .as_ref()
            .and_then(|config| config.connector_runtime.as_ref())
            .map(|runtime| runtime.load(&host))
            .transpose()?;
        if let Some(loaded) = loaded {
            host.sessions
                .as_mut()
                .and_then(|config| config.connector_runtime.as_mut())
                .ok_or(SessionError::AuthorityDenied)?
                .retain(loaded);
        }
        Ok(host)
    }

    /// Authenticate current resource registration only. This does not load or
    /// qualify producer/tool definitions and cannot grant any joined operation.
    /// Source, model and final-goal admission must continue using full current().
    fn current_registration(&self) -> Result<LocalCodexHost, SessionError> {
        #[cfg(test)]
        let _profile = runtime_admission::profiling::span("access.registration");
        let text = {
            #[cfg(test)]
            let _profile = runtime_admission::profiling::span("access.registration.read");
            read(&self.admission, 1_048_575)?
        };
        let resolver = {
            #[cfg(test)]
            let _profile = runtime_admission::profiling::span("access.registration.decode");
            LocalWorkspaceResolver::from_json(&text).map_err(error)?
        };
        #[cfg(test)]
        let _profile_host = runtime_admission::profiling::span("access.registration.host_validate");
        let binding = resolver
            .resolve(&WorkspaceReference {
                working_directory: self.cwd.clone(),
                repository: self.repository.clone(),
            })
            .map_err(error)?;
        if binding.scope != self.binding.scope
            || binding.canonical_scope != self.binding.canonical_scope
            || binding.session.principal != self.binding.session.principal
            || binding.session.session_id != self.binding.session.session_id
        {
            return Err(SessionError::ScopeDenied);
        }
        let host = resolver.host(&binding).map_err(error)?;
        if !host.sessions.as_ref().is_some_and(|s| s.enabled) {
            return Err(SessionError::AuthorityDenied);
        }
        if let Some(config) = host.sessions.as_ref()
            && let Some(runtime) = &config.connector_runtime
        {
            runtime.validate_registration(config, &host.binding.canonical_scope)?;
            validate_registered_connector_plan(
                &host,
                &registered_connector_goal(config, &host.binding.canonical_scope)?,
            )?;
        }
        Ok(host)
    }
    pub(crate) fn owner(&self) -> Result<OwnerBinding, SessionError> {
        let scope = &self.binding.scope;
        let token = |k: &str| scope[k].as_str().ok_or(SessionError::ScopeDenied);
        Ok(OwnerBinding {
            principal: PrincipalId::new(&self.binding.session.principal)?,
            workspace: WorkspaceId::new(token("workspace_id")?)?,
            project: ProjectId::new(token("project_id")?)?,
            binding: BindingId::new(token("binding_id")?)?,
            client_owner: ClientOwnerId::new(&self.binding.session.session_id)?,
        })
    }
    /// Authenticated read of the current durable session from the current
    /// operator-selected store. This lookup is not an operation or policy grant.
    pub(crate) fn current_claim(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
    ) -> Result<(LocalCodexHost, FencedSession), SessionError> {
        let (host, _, claim) = self.current_repository_claim(owner, session, None)?;
        Ok((host, claim))
    }
    fn current_resource_claim(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
    ) -> Result<(LocalCodexHost, FencedSession), SessionError> {
        if owner != &self.owner()? {
            return Err(SessionError::ScopeDenied);
        }
        let host = self.current_registration()?;
        let (repository, _) = self.open_current_store(&host)?;
        let claim = repository.load(owner, session)?;
        Ok((host, claim))
    }
    fn current_repository_claim(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
        deadline: Option<u64>,
    ) -> Result<(LocalCodexHost, PostgresSessionStore, FencedSession), SessionError> {
        let (host, repository, claim, _) =
            self.current_repository_claim_with_definition(owner, session, deadline)?;
        Ok((host, repository, claim))
    }
    fn current_repository_claim_with_definition(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
        deadline: Option<u64>,
    ) -> Result<
        (
            LocalCodexHost,
            PostgresSessionStore,
            FencedSession,
            StoreDefinition,
        ),
        SessionError,
    > {
        if owner != &self.owner()? {
            return Err(SessionError::ScopeDenied);
        }
        self.check_deadline(deadline)?;
        let host = self.current()?;
        self.check_deadline(deadline)?;
        let (repository, definition) = {
            #[cfg(test)]
            let _profile = runtime_admission::profiling::span("access.current_claim.open_store");
            self.open_current_store(&host)?
        };
        self.check_deadline(deadline)?;
        let claim = {
            #[cfg(test)]
            let _profile = runtime_admission::profiling::span("access.current_claim.pg_load");
            repository.load(owner, session)
        };
        self.check_deadline(deadline)?;
        let claim = claim?;
        Ok((host, repository, claim, definition))
    }
    fn check_deadline(&self, deadline: Option<u64>) -> Result<(), SessionError> {
        if let Some(until) = deadline
            && self.now_ms()? >= until
        {
            return Err(SessionError::LimitExceeded);
        }
        Ok(())
    }
    fn now_ms(&self) -> Result<u64, SessionError> {
        u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| SessionError::Unavailable)?
                .as_millis(),
        )
        .map_err(|_| SessionError::LimitExceeded)
    }
    fn disclose(&self, owner: &OwnerBinding, _: &SessionId) -> Result<(), SessionError> {
        if owner != &self.owner()? {
            return Err(SessionError::ScopeDenied);
        }
        self.current()?;
        Ok(())
    }
    pub(crate) fn resource_limits(
        &self,
        owner: &OwnerBinding,
        goal: &SupportedSessionGoal,
    ) -> Result<Option<SessionResourceLimits>, SessionError> {
        if owner != &self.owner()? {
            return Err(SessionError::ScopeDenied);
        }
        let host = self.current()?;
        Self::resource_limits_from_host(&host, goal)
    }

    /// Pure checks against the freshly authenticated host from this lookup.
    /// Callers must not retain this snapshot as authority for a later operation.
    fn resource_limits_from_host(
        host: &LocalCodexHost,
        goal: &SupportedSessionGoal,
    ) -> Result<Option<SessionResourceLimits>, SessionError> {
        let config = host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        if goal.intent() != &config.intent {
            return Err(SessionError::UnsupportedCapability);
        }
        let current_goal = match (&config.basis, &config.connector_goal) {
            (Some(basis), None) => SupportedSessionGoal::artifact(SupportedArtifactGoal::validate(
                config.intent.clone(),
                basis.clone(),
            )?),
            (None, Some(_)) => {
                let registered = registered_connector_goal(config, &host.binding.canonical_scope)?;
                validate_registered_connector_plan(host, &registered)?;
                SupportedSessionGoal::connector(registered)
            }
            _ => return Err(SessionError::InvalidInput),
        };
        if goal != &current_goal {
            return Err(SessionError::StaleRevision);
        }
        Ok(config.resource_limits.clone())
    }

    /// Complete resource admission using the host and claim from the SAME fresh
    /// current_repository_claim lookup. No authority result is cached or reused
    /// across guard callbacks; this avoids recursively reopening configuration
    /// and the database inside one admission check.
    fn validate_allocation_at(
        &self,
        allocation: &SessionResourceAllocation,
        host: &LocalCodexHost,
        current: &FencedSession,
    ) -> Result<(), SessionError> {
        if allocation.binding().owner != self.owner()? {
            return Err(SessionError::ScopeDenied);
        }
        let config = host
            .sessions
            .as_ref()
            .ok_or(SessionError::AuthorityDenied)?;
        let checkpoint = &current.checkpoint;
        if checkpoint.execution != config.execution
            || checkpoint.goal.intent() != &config.intent
            || checkpoint.budget.max_actions != config.max_actions
            || checkpoint.budget.max_retries != config.max_retries
        {
            return Err(SessionError::StaleRevision);
        }
        let limits = Self::resource_limits_from_host(host, &checkpoint.goal)?
            .ok_or(SessionError::UnsupportedCapability)?;
        if checkpoint
            .resource_ledger
            .as_ref()
            .map(|ledger| ledger.limits())
            != Some(&limits)
        {
            return Err(SessionError::StaleRevision);
        }
        validate_current_resource_claim(
            allocation,
            current,
            self.now_ms()?,
            std::time::Instant::now(),
        )
    }
}
impl LocalSessionHost {
    fn new(access: Arc<LocalSessionAccess>) -> Self {
        Self {
            connector_runtime: LocalConnectorRuntime {
                access: Arc::clone(&access),
            },
            access,
        }
    }
    fn current(&self) -> Result<LocalCodexHost, SessionError> {
        self.access.current()
    }
    fn owner(&self) -> Result<OwnerBinding, SessionError> {
        self.access.owner()
    }
}
impl InteractionAuthorityPort for LocalSessionHost {
    fn load_consent(
        &self,
        owner: &OwnerBinding,
        reference: &ConsentRecordRef,
    ) -> Result<ConsentStatus, SessionError> {
        if owner != &self.owner()? {
            return Err(SessionError::ScopeDenied);
        }
        let host = self.current()?;
        let status = store(&host)?.consent(owner, reference)?;
        if let ConsentStatus::Granted(grant) = &status {
            self.issuer(owner, grant.issuer())?;
        }
        Ok(status)
    }
    fn live_status(&self, grant: &VerifiedConsent) -> Result<ConsentStatus, SessionError> {
        self.load_consent(&grant.binding().owner, grant.record())
    }
    fn load_connector_consent(
        &self,
        owner: &OwnerBinding,
        reference: &ConnectorConsentRecordRef,
    ) -> Result<ConnectorConsentStatus, SessionError> {
        if owner != &self.owner()? {
            return Err(SessionError::ScopeDenied);
        }
        let host = self.current()?;
        let status = store(&host)?.connector_consent(owner, reference)?;
        if let ConnectorConsentStatus::Granted(grant) = &status {
            self.issuer(owner, &grant.binding().issuer)?;
        }
        Ok(status)
    }
    fn live_connector_consent(
        &self,
        grant: &VerifiedConnectorConsent,
    ) -> Result<ConnectorConsentStatus, SessionError> {
        self.load_connector_consent(&grant.binding().owner, grant.record())
    }
}

impl SessionResourceAuthorityPort for LocalSessionAccess {
    fn validate_allocation(
        &self,
        allocation: &SessionResourceAllocation,
    ) -> Result<(), SessionError> {
        // Authenticate current operator configuration before selecting the
        // store. Neither an allocation nor an old host snapshot selects it.
        let (host, current) = self
            .current_resource_claim(&allocation.binding().owner, &allocation.binding().session)?;
        self.validate_allocation_at(allocation, &host, &current)
    }
    // Joined invocation/connector/discovery admission deliberately retains the
    // default deny until trusted producer and target configuration is wired.
}

/// Validate a coordinator-minted child against current durable truth. Kept
/// separate from configuration/storage I/O so drift and expiry are exercised
/// against real coordinator-minted allocations without creating a test grant.
fn validate_current_resource_claim(
    allocation: &SessionResourceAllocation,
    current: &FencedSession,
    now_ms: u64,
    now: std::time::Instant,
) -> Result<(), SessionError> {
    let checkpoint = &current.checkpoint;
    let binding = allocation.binding();
    if binding.owner != checkpoint.owner || &binding.scope != checkpoint.goal.scope() {
        return Err(SessionError::ScopeDenied);
    }
    if binding.session != checkpoint.snapshot.session
        || binding.run != checkpoint.snapshot.run
        || allocation.claim_revision() != checkpoint.snapshot.revision
        || allocation.fence() != current.fence
    {
        return Err(SessionError::StaleRevision);
    }
    let prepared = checkpoint.snapshot.state == SessionState::Preparing
        && checkpoint.snapshot.dispatch_knowledge == DispatchKnowledge::Prepared;
    let executing = checkpoint.snapshot.state == SessionState::Dispatching
        && checkpoint.snapshot.dispatch_knowledge == DispatchKnowledge::Reserved;
    let valid_phase = match allocation.phase() {
        SessionResourcePhase::Preparation => {
            prepared && checkpoint.accepted_connector_consent.is_none()
        }
        SessionResourcePhase::ApprovedResume => {
            prepared && checkpoint.accepted_connector_consent.is_some()
        }
        SessionResourcePhase::Execution | SessionResourcePhase::AfterSnapshotRecovery => executing,
        SessionResourcePhase::GoalVerification => false,
    };
    if !valid_phase || checkpoint.snapshot.dispatch.as_ref() != Some(&binding.dispatch) {
        return Err(SessionError::InvalidState);
    }
    let registered_step = match checkpoint.goal.as_connector() {
        Some(goal) => goal.steps().contains(&binding.step),
        None => checkpoint
            .goal
            .as_artifact()
            .is_some_and(|goal| goal.basis().step == binding.step),
    };
    if !registered_step {
        return Err(SessionError::AuthorityDenied);
    }
    let ledger = checkpoint
        .resource_ledger
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    ledger.validate()?;
    let reservation = ledger
        .reservations()
        .iter()
        .find(|reservation| {
            reservation.id() == allocation.reservation_id() && reservation.binding() == binding
        })
        .ok_or(SessionError::AuthorityDenied)?;
    let plan = reservation
        .plan()
        .ok_or(SessionError::UnsupportedCapability)?;
    if allocation.cost_unit() != &ledger.limits().cost_unit
        || allocation.amounts().is_zero()
        || !allocation
            .amounts()
            .within(plan.amounts(allocation.phase()))
    {
        return Err(SessionError::AuthorityDenied);
    }
    let until = checkpoint
        .lease_until_ms
        .ok_or(SessionError::InvalidState)?
        .min(checkpoint.budget.deadline_ms);
    // The persisted deadline is wall-clock based; the original unforgeable
    // child also carries its mint-time monotonic ceiling. Check both, without
    // reconstructing/extending that ceiling when the clock or lease changes.
    if now_ms >= until || now >= allocation.deadline() {
        return Err(SessionError::LimitExceeded);
    }
    Ok(())
}
impl StructuredSessionHost for LocalSessionHost {
    fn now_ms(&self) -> Result<u64, SessionError> {
        self.access.now_ms()
    }
    fn disclose(&self, owner: &OwnerBinding, session: &SessionId) -> Result<(), SessionError> {
        self.access.disclose(owner, session)
    }
    fn issuer(&self, owner: &OwnerBinding, issuer: &PrincipalId) -> Result<(), SessionError> {
        self.disclose(owner, &SessionId::new("operator")?)?;
        if issuer != &owner.principal {
            return Err(SessionError::AuthorityDenied);
        }
        let host = self.current()?;
        if host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?
            .issuers
            .contains(issuer)
        {
            Ok(())
        } else {
            Err(SessionError::AuthorityDenied)
        }
    }
    fn connector_runtime(&self) -> Result<&dyn ConnectorSessionRuntimePort, SessionError> {
        Ok(&self.connector_runtime)
    }
    fn resource_limits(
        &self,
        owner: &OwnerBinding,
        goal: &SupportedSessionGoal,
    ) -> Result<Option<SessionResourceLimits>, SessionError> {
        self.access.resource_limits(owner, goal)
    }
    fn register(
        &self,
        owner: &OwnerBinding,
        intent: Intent,
        execution: RequestedExecution,
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
        self.disclose(owner, &SessionId::new("start")?)?;
        let host = self.current()?;
        let config = host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        if intent != config.intent || execution != config.execution {
            return Err(SessionError::UnsupportedCapability);
        }
        if config.ttl_ms == 0 || config.ttl_ms > 86_400_000 {
            return Err(SessionError::LimitExceeded);
        }
        let goal = match (&config.basis, &config.connector_goal) {
            (Some(basis), None) => SupportedSessionGoal::artifact(SupportedArtifactGoal::validate(
                intent,
                basis.clone(),
            )?),
            (None, Some(_)) => {
                let connector_goal =
                    registered_connector_goal(config, &host.binding.canonical_scope)?;
                validate_registered_connector_plan(&host, &connector_goal)?;
                SupportedSessionGoal::connector(connector_goal)
            }
            _ => return Err(SessionError::InvalidInput),
        };
        let budget = SessionBudget {
            actions: 0,
            retries: 0,
            max_actions: config.max_actions,
            max_retries: config.max_retries,
            deadline_ms: self
                .now_ms()?
                .checked_add(config.ttl_ms)
                .ok_or(SessionError::LimitExceeded)?,
        };
        budget.validate()?;
        Ok((goal, budget))
    }
    fn prepare(&self, checkpoint: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        self.disclose(&checkpoint.owner, &checkpoint.snapshot.session)?;
        let host = self.current()?;
        let config = host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        let artifact_goal = checkpoint
            .goal
            .as_artifact()
            .ok_or(SessionError::UnsupportedCapability)?;
        let config_basis = config
            .basis
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        if config_basis != artifact_goal.basis()
            || config.intent != *checkpoint.goal.intent()
            || config.execution != checkpoint.execution
        {
            return Err(SessionError::StaleRevision);
        }
        let canonical = host
            .canonical
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        let record = |reference: &RecordRef, contract: &str| -> Result<&Value, SessionError> {
            host.resources
                .iter()
                .find(|r| {
                    r["reference"]["id"] == reference.id().as_str()
                        && r["reference"]["revision"] == reference.revision().as_str()
                        && r["reference"]["digest"] == reference.digest()
                        && r["reference"]["contract"] == contract
                        && r["reference"]["contract_version"] == "1.0"
                })
                .filter(|r| {
                    !r["provenance"]
                        .as_array()
                        .is_none_or(|p| p.iter().any(|p| p["sensitivity"] == "SECRET"))
                })
                .map(|r| &r["document"])
                .ok_or(SessionError::Unavailable)
        };
        if canonical.plan["id"] != config_basis.plan.id().as_str()
            || canonical.plan["revision"] != config_basis.plan.revision().as_str()
            || canonical.plan["digest"] != config_basis.plan.digest()
        {
            return Err(SessionError::StaleRevision);
        }
        let (snapshot, rules) = host_mapping::capture(
            &canonical.catalog,
            &canonical.documents(&host.resources).map_err(error)?,
        )
        .map_err(error)?;
        if snapshot.scope != config_basis.scope
            || snapshot.scope != host.binding.canonical_scope
            || snapshot.operating_mode != checkpoint.execution.mode
            || snapshot.execution_profile != checkpoint.execution.profile
        {
            return Err(SessionError::ScopeDenied);
        }
        let resolved = DeclarativeResolutionApplication
            .resolve_plan(&snapshot, &rules)
            .map_err(|_| SessionError::InvalidInput)?;
        let mut projection = record(&config_basis.projection, "cg.context-projection")?.clone();
        let mut fragments = Vec::new();
        for source in &config_basis.sources {
            let fragment = record(source, "cg.context-fragment")?.clone();
            if fragment["id"] != source.id().as_str() {
                return Err(SessionError::InvalidInput);
            }
            fragments.push(fragment);
        }
        if !config_basis.sources.is_empty() {
            projection["fragments"] = json!(fragments);
            projection["selected"] = json!(
                checkpoint
                    .selected_source
                    .as_ref()
                    .map(|s| vec![s.id().as_str()])
                    .unwrap_or_default()
            );
        }
        let input =
            host_mapping::compile(resolved, canonical.policy.clone(), projection).map_err(error)?;
        if input.projection.mapping.step != config_basis.step {
            return Err(SessionError::StaleRevision);
        }
        let id = CapabilityId::new("context.artifact.persist")
            .map_err(|_| SessionError::InvalidInput)?;
        let capability = CapabilityDefinition::new(
            id.clone(),
            if config.consent_required {
                CapabilityClass::Mutate
            } else {
                CapabilityClass::Inspect
            },
        );
        let authority = PolicyAuthority {
            policies: vec![
                PolicyDefinition::new(
                    PolicyId::new("local-artifact-policy")
                        .map_err(|_| SessionError::InvalidInput)?,
                    "Trusted artifact persistence policy",
                    [id.clone()],
                )
                .map_err(|_| SessionError::InvalidInput)?,
            ],
            capabilities: [(id.clone(), capability.clone())].into(),
            ..Default::default()
        };
        let facts = StepFacts {
            authorizations: [(id.clone(), Approval::Granted)].into(),
            prerequisites_satisfied: true,
            ..Default::default()
        };
        let empty = std::collections::BTreeSet::new();
        let capabilities = [(id, capability)].into();
        let step = PlanStepId::new("artifact-persist").map_err(|_| SessionError::InvalidInput)?;
        let action_policy = PolicyEngine::evaluate(
            &authority,
            &StepPolicyInput {
                step: &step,
                capabilities: &capabilities,
                operating_mode: checkpoint.execution.mode,
                execution_profile: checkpoint.execution.profile,
                process: ProcessReadiness::NotApplicable,
                resolved: true,
                has_prerequisites: false,
                constraints: &empty,
                preconditions: &empty,
                facts: &facts,
            },
        );
        let authority_bytes=serde_json::to_vec(&json!({"policy":canonical.policy,"resolution":DeclarativeResolutionApplication.serialize_resolution(&input.resolved,Default::default()).map_err(|_|SessionError::InvalidInput)?,"artifact_policy":action_policy})).map_err(|_|SessionError::InvalidInput)?;
        Ok(PreparedTask {
            input,
            authority: content_reference("current-authority", &authority_bytes)?,
            action_policy,
        })
    }
}

#[cfg(test)]
mod connector_admission_tests {
    use super::*;

    pub(super) fn config_value(operation: &str) -> Value {
        let intent: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/declarative-cli/intent.json"
        ))
        .unwrap();
        json!({
            "store_file":"/var/lib/cg/session-db",
            "intent":intent,
            "connector_goal":{
                "plan":{"id":"connector-plan","revision":"1","digest":format!("sha256:{}", "a".repeat(64))},
                "steps":["connector-step"],
                "observations":[{"condition":"clean","source":"repository","capability":"repository.read","operation_class":operation}]
            },
            "execution":{"mode":"DEVELOPMENT","profile":"FULL_PATH"},
            "max_actions":2,"max_retries":1,"ttl_ms":60000,
            "consent_required":false,"enabled":true,"issuers":["operator"]
        })
    }

    fn config(operation: &str) -> SessionAdmission {
        serde_json::from_value(config_value(operation)).unwrap()
    }

    pub(super) fn workspace_fixture(
        sessions: Option<Value>,
        canonical_scope: &str,
        session_id: &str,
        database_connection: Option<&str>,
    ) -> (LocalSessionHost, LocalApplication, OwnerBinding, PathBuf) {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "cg-local-session-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let scope = json!({"workspace_id":"w","project_id":"p","binding_id":"b"});
        let mut mapping = json!({"repository":root,"scope":scope,
            "canonical_scope":canonical_scope,"principal":"operator",
            "session_id":session_id,"revision":"mapping-1","resources":[]});
        if let Some(mut sessions) = sessions {
            if let Some(connection) = database_connection {
                let connection_file = root.join("cognitive-database.txt");
                std::fs::write(&connection_file, connection).unwrap();
                sessions["store_file"] = json!(connection_file);
            }
            mapping["sessions"] = sessions;
        }
        let admission_json = json!({"schema_version":1,"mappings":[mapping]}).to_string();
        let admission_file = root.join("admission.json");
        std::fs::write(&admission_file, &admission_json).unwrap();
        let resolver = LocalWorkspaceResolver::from_json(&admission_json).unwrap();
        let path = root.to_string_lossy().into_owned();
        let binding = resolver
            .resolve(&WorkspaceReference {
                working_directory: path.clone(),
                repository: path.clone(),
            })
            .unwrap();
        let host = LocalSessionHost::new(Arc::new(LocalSessionAccess {
            binding: binding.clone(),
            admission: admission_file.to_string_lossy().into_owned(),
            cwd: path.clone(),
            repository: path.clone(),
            store_connection: Default::default(),
        }));
        let owner = host.owner().unwrap();
        let facade =
            CodexFacade::with_binding(binding.clone(), resolver.host(&binding).unwrap()).unwrap();
        let app = LocalApplication::new(
            facade,
            binding,
            &admission_file.to_string_lossy(),
            &path,
            &path,
            true,
        );
        (host, app, owner, root)
    }

    fn rebind_host(host: &mut LocalSessionHost, binding: ScopeBinding) {
        let old = &host.access;
        *host = LocalSessionHost::new(Arc::new(LocalSessionAccess {
            binding,
            admission: old.admission.clone(),
            cwd: old.cwd.clone(),
            repository: old.repository.clone(),
            store_connection: Default::default(),
        }));
    }

    fn workspace_host(sessions: Option<Value>) -> (LocalSessionHost, OwnerBinding, PathBuf) {
        let (host, _, owner, root) =
            workspace_fixture(sessions, "repository-scope", "local-session", None);
        (host, owner, root)
    }

    use crate::session_model_test_support as model_support;

    struct TestIrAdmission;
    impl gateway_domain::SemanticReferenceValidator for TestIrAdmission {
        fn validate(
            &self,
            _: &gateway_domain::SemanticTaskData,
            _: &gateway_domain::SemanticReferenceBinding,
        ) -> Result<(), gateway_domain::ValidationError> {
            Ok(())
        }
    }
    impl gateway_application::prompt_ir::InvocationAdmissionPort for TestIrAdmission {
        fn validate_current(
            &self,
            _: &gateway_application::prompt_ir::InvocationIr,
        ) -> Result<(), gateway_domain::prompt_ir::InvocationIrError> {
            Ok(())
        }
    }

    struct TestOutputHost;
    impl gateway_domain::SemanticReferenceValidator for TestOutputHost {
        fn validate(
            &self,
            _: &gateway_domain::SemanticTaskData,
            _: &gateway_domain::SemanticReferenceBinding,
        ) -> Result<(), gateway_domain::ValidationError> {
            Ok(())
        }
    }
    impl gateway_application::prompt_ir::InvocationAdmissionPort for TestOutputHost {
        fn validate_current(
            &self,
            _: &gateway_application::prompt_ir::InvocationIr,
        ) -> Result<(), gateway_domain::prompt_ir::InvocationIrError> {
            Ok(())
        }
    }
    impl gateway_application::output_verification::OutputVerificationPort for TestOutputHost {
        fn resolve_reference(
            &self,
            _: &gateway_application::prompt_ir::InvocationIr,
            _: &gateway_domain::output_verification::OutputReference,
        ) -> Result<(), gateway_domain::output_verification::VerificationError> {
            Ok(())
        }

        fn evaluate(
            &self,
            ir: &gateway_application::prompt_ir::InvocationIr,
            output: &gateway_application::output_verification::ParsedOutput,
        ) -> Result<
            gateway_application::output_verification::ConstraintReport,
            gateway_domain::output_verification::VerificationError,
        > {
            use gateway_domain::{
                semantic_output::CheckOutcome, semantic_task::SemanticVerificationCheck,
            };
            let checks = ir
                .prompt()
                .handoff()
                .task()
                .data()
                .verification_contract
                .checks
                .iter()
                .filter(|check| **check != SemanticVerificationCheck::OutputSchemaValid)
                .map(|check| (*check, CheckOutcome::Passed))
                .collect();
            Ok(gateway_application::output_verification::ConstraintReport {
                binding: output.binding(),
                policy: CheckOutcome::Passed,
                process: CheckOutcome::Passed,
                task_completeness: CheckOutcome::Passed,
                semantic_output: CheckOutcome::Passed,
                checks,
            })
        }
    }

    pub(super) fn prepared_connector_turn() -> PreparedConnectorTurn {
        use gateway_application::{
            output_parsing::parse_invocation_output, output_verification::verify_output,
            prompt_ir::admit_invocation_ir, prompt_rendering::ToolDefinition,
        };
        use gateway_domain::{
            NonEmptyText,
            semantic_output::{OutputField, OutputType},
        };

        let handoff = model_support::handoff();
        let prepared = model_support::prepared(&handoff, 100_000, true);
        let tools = prepared
            .invocation()
            .data()
            .permitted_capabilities
            .iter()
            .map(|(id, binding)| {
                (
                    id.clone(),
                    ToolDefinition {
                        binding: binding.clone(),
                        description: NonEmptyText::new("Read registered connector state").unwrap(),
                        input: OutputType::Object {
                            fields: std::collections::BTreeMap::from([(
                                "value".to_owned(),
                                OutputField {
                                    required: true,
                                    value_type: OutputType::String {},
                                },
                            )]),
                        },
                        connector_inventory: None,
                    },
                )
            })
            .collect();
        let invocation = admit_invocation_ir(
            gateway_domain::SchemaVersion::V1,
            handoff,
            prepared,
            tools,
            &TestIrAdmission,
        )
        .unwrap();
        let capability = invocation
            .prompt()
            .tools()
            .values()
            .next()
            .unwrap()
            .binding
            .clone();
        let wire = json!({
            "schema_version":"1.0",
            "invocation":invocation.identity(),
            "result":"entity",
            "references":[],
            "tools":[{"capability":capability,"arguments":{"value":"state"}}]
        });
        let parsed =
            parse_invocation_output(&invocation, &serde_json::to_vec(&wire).unwrap()).unwrap();
        let verified = verify_output(&invocation, parsed, &TestOutputHost, Vec::new()).unwrap();
        let scope = invocation.prepared().invocation().data().task.scope.clone();
        PreparedConnectorTurn::new(
            invocation,
            verified,
            ScopedConnectorSnapshotPayload::new(scope, br#"{"state":"before"}"#.to_vec()).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn trusted_connector_registration_is_typed_read_only_and_scoped() {
        let scope = gateway_domain::ContextScopeId::new("repository-scope").unwrap();
        let goal = registered_connector_goal(&config("read"), &scope).unwrap();
        assert_eq!(goal.scope(), &scope);
        assert_eq!(goal.steps(), &[PlanStepId::new("connector-step").unwrap()]);
        assert_eq!(goal.observations().len(), 1);
        assert_eq!(
            registered_connector_goal(&config("mutate"), &scope),
            Err(SessionError::UnsupportedCapability)
        );
    }

    #[test]
    fn connector_registration_accepts_only_the_supported_read_observation_classes() {
        let scope = gateway_domain::ContextScopeId::new("repository-scope").unwrap();
        for (class, expected) in [
            ("read", ConnectorOperationClass::Read),
            ("search", ConnectorOperationClass::Search),
            ("inspect", ConnectorOperationClass::Inspect),
        ] {
            let goal = registered_connector_goal(&config(class), &scope).unwrap();
            assert_eq!(goal.observations()[0].operation_class(), expected);
        }
        assert_eq!(
            registered_connector_goal(&config("admin"), &scope),
            Err(SessionError::UnsupportedCapability)
        );
    }

    #[test]
    fn connector_registration_rejects_missing_or_malformed_typed_identity() {
        let scope = gateway_domain::ContextScopeId::new("repository-scope").unwrap();
        let mut missing = config("read");
        missing.connector_goal = None;
        assert_eq!(
            registered_connector_goal(&missing, &scope),
            Err(SessionError::UnsupportedCapability)
        );

        let mut invalid_step = config("read");
        invalid_step.connector_goal.as_mut().unwrap().steps[0] = " ".into();
        assert_eq!(
            registered_connector_goal(&invalid_step, &scope),
            Err(SessionError::InvalidInput)
        );

        let mut invalid_condition = config("read");
        invalid_condition
            .connector_goal
            .as_mut()
            .unwrap()
            .observations[0]
            .condition = " ".into();
        assert_eq!(
            registered_connector_goal(&invalid_condition, &scope),
            Err(SessionError::InvalidInput)
        );

        let mut no_observations = config("read");
        no_observations
            .connector_goal
            .as_mut()
            .unwrap()
            .observations
            .clear();
        assert_eq!(
            registered_connector_goal(&no_observations, &scope),
            Err(SessionError::InvalidInput)
        );

        let mut invalid_source = config("read");
        invalid_source.connector_goal.as_mut().unwrap().observations[0].source = " ".into();
        assert_eq!(
            registered_connector_goal(&invalid_source, &scope),
            Err(SessionError::InvalidInput)
        );

        let mut invalid_capability = config("read");
        invalid_capability
            .connector_goal
            .as_mut()
            .unwrap()
            .observations[0]
            .capability = " ".into();
        assert_eq!(
            registered_connector_goal(&invalid_capability, &scope),
            Err(SessionError::InvalidInput)
        );
    }

    #[test]
    fn connector_registration_requires_the_exact_canonical_plan_pin() {
        let scope = gateway_domain::ContextScopeId::new("repository-scope").unwrap();
        let goal = registered_connector_goal(&config("read"), &scope).unwrap();
        let binding = ScopeBinding {
            scope: json!({}),
            canonical_scope: scope,
            session: SessionContext {
                principal: "operator".into(),
                session_id: "session".into(),
                connection_id: "connection".into(),
            },
            mapping_revision: "mapping-1".into(),
        };
        let canonical_plan = json!({
            "id":goal.plan().id().as_str(),
            "revision":goal.plan().revision().as_str(),
            "digest":goal.plan().digest(),
        });
        let canonical = || crate::codex_canonical::CanonicalAdmission {
            catalog: PathBuf::new(),
            plan: canonical_plan.clone(),
            rules: json!({}),
            process: json!({}),
            policy: json!({}),
            pinned_basis: None,
        };
        let mut host = LocalCodexHost {
            binding: binding.clone(),
            resources: Vec::new(),
            canonical: Some(canonical()),
            sessions: None,
        };
        assert_eq!(validate_registered_connector_plan(&host, &goal), Ok(()));

        for (field, wrong) in [
            ("id", json!("different-plan")),
            ("revision", json!("different-revision")),
            ("digest", json!("sha256:wrong")),
        ] {
            let mut admission = canonical();
            admission.plan[field] = wrong;
            host.canonical = Some(admission);
            assert_eq!(
                validate_registered_connector_plan(&host, &goal),
                Err(SessionError::StaleRevision),
                "mismatched {field} must be stale"
            );
        }
        host.canonical = None;
        assert_eq!(
            validate_registered_connector_plan(&host, &goal),
            Err(SessionError::UnsupportedCapability)
        );
    }

    #[test]
    fn local_resource_admission_is_optional_and_bound_to_the_current_registered_goal() {
        let mut admission = config_value("read");
        admission["intent"]["desired_state"]["conditions"][0]["id"] = json!("context-verified");
        admission["intent"]["desired_state"]["conditions"][0]["subject"] =
            json!("cg.context.projection.verified");
        admission["intent"]["desired_state"]["expression"]["value"] = json!("context-verified");
        admission.as_object_mut().unwrap().remove("connector_goal");
        let basis = ArtifactGoalBasis {
            scope: gateway_domain::ContextScopeId::new("repository-scope").unwrap(),
            plan: content_reference("resource-plan", b"plan").unwrap(),
            step: PlanStepId::new("resource-step").unwrap(),
            projection: content_reference("projection", b"projection").unwrap(),
            sources: Vec::new(),
        };
        admission["basis"] = serde_json::to_value(&basis).unwrap();
        let intent: Intent = serde_json::from_value(admission["intent"].clone()).unwrap();
        let goal = SupportedSessionGoal::artifact(
            SupportedArtifactGoal::validate(intent.clone(), basis.clone()).unwrap(),
        );
        let (host, owner, root) = workspace_host(Some(admission.clone()));
        assert_eq!(host.resource_limits(&owner, &goal), Ok(None));
        std::fs::remove_dir_all(root).unwrap();

        let limits = SessionResourceLimits {
            cost_unit: gateway_domain::ReferenceId::new("credits").unwrap(),
            amounts: SessionResourceAmounts {
                model_attempts: 2,
                input_tokens: 1_000,
                output_tokens: 100,
                cost_microunits: 10,
                ..Default::default()
            },
        };
        admission["resource_limits"] = serde_json::to_value(&limits).unwrap();
        let (host, owner, root) = workspace_host(Some(admission.clone()));
        assert_eq!(host.resource_limits(&owner, &goal), Ok(Some(limits)));
        let mut foreign = owner.clone();
        foreign.project = ProjectId::new("other-project").unwrap();
        assert_eq!(
            host.resource_limits(&foreign, &goal),
            Err(SessionError::ScopeDenied)
        );
        let mut stale_basis = basis;
        stale_basis.plan = content_reference("changed-resource-plan", b"changed").unwrap();
        let stale_goal = SupportedSessionGoal::artifact(
            SupportedArtifactGoal::validate(intent, stale_basis).unwrap(),
        );
        assert_eq!(
            host.resource_limits(&owner, &stale_goal),
            Err(SessionError::StaleRevision)
        );
        std::fs::remove_dir_all(root).unwrap();

        admission["resource_limits"]["unrecognized_authority"] = json!(true);
        assert!(serde_json::from_value::<SessionAdmission>(admission).is_err());
    }

    #[test]
    fn local_registration_checks_owner_enablement_intent_execution_and_ttl() {
        let (host, owner, root) = workspace_host(None);
        let absent_sessions = host.register(
            &owner,
            config("read").intent.clone(),
            config("read").execution,
        );
        assert!(
            matches!(absent_sessions, Err(SessionError::AuthorityDenied)),
            "unexpected result for absent session admission: {absent_sessions:?}"
        );
        let mut wrong_owner = owner.clone();
        wrong_owner.project = ProjectId::new("other-project").unwrap();
        assert!(matches!(
            host.register(
                &wrong_owner,
                config("read").intent.clone(),
                config("read").execution
            ),
            Err(SessionError::ScopeDenied)
        ));
        let mut wrong_scope_host = host;
        let mut changed_binding = wrong_scope_host.access.binding.clone();
        changed_binding.canonical_scope =
            gateway_domain::ContextScopeId::new("other-scope").unwrap();
        rebind_host(&mut wrong_scope_host, changed_binding);
        assert!(matches!(
            wrong_scope_host.register(
                &owner,
                config("read").intent.clone(),
                config("read").execution
            ),
            Err(SessionError::ScopeDenied)
        ));
        std::fs::remove_dir_all(root).unwrap();

        let mut disabled = config_value("read");
        disabled["enabled"] = json!(false);
        let (host, owner, root) = workspace_host(Some(disabled));
        assert!(matches!(
            host.register(
                &owner,
                config("read").intent.clone(),
                config("read").execution
            ),
            Err(SessionError::AuthorityDenied)
        ));
        std::fs::remove_dir_all(root).unwrap();

        let intent = config("read").intent;
        let production = RequestedExecution {
            mode: gateway_domain::OperatingMode::Hardening,
            profile: gateway_domain::ExecutionProfile::FullPath,
        };
        let mut other_intent: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/declarative-cli/intent.json"
        ))
        .unwrap();
        other_intent["id"] = json!("different-intent");
        let other_intent = serde_json::from_value(other_intent).unwrap();
        let (host, owner, root) = workspace_host(Some(config_value("read")));
        assert!(matches!(
            host.register(&owner, other_intent, config("read").execution),
            Err(SessionError::UnsupportedCapability)
        ));
        std::fs::remove_dir_all(root).unwrap();

        let (host, owner, root) = workspace_host(Some(config_value("read")));
        assert!(matches!(
            host.register(&owner, intent.clone(), production),
            Err(SessionError::UnsupportedCapability)
        ));
        for ttl in [0, 86_400_001] {
            let mut invalid_ttl = config_value("read");
            invalid_ttl["ttl_ms"] = json!(ttl);
            let (host, owner, root) = workspace_host(Some(invalid_ttl));
            assert!(matches!(
                host.register(&owner, intent.clone(), config("read").execution),
                Err(SessionError::LimitExceeded)
            ));
            std::fs::remove_dir_all(root).unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_connector_registration_rejects_mixed_goals_and_requires_canonical_plan() {
        let intent = config("read").intent;
        let scope = gateway_domain::ContextScopeId::new("repository-scope").unwrap();
        let basis = ArtifactGoalBasis {
            scope,
            plan: content_reference("artifact-plan", b"plan").unwrap(),
            step: PlanStepId::new("artifact-step").unwrap(),
            projection: content_reference("projection", b"projection").unwrap(),
            sources: Vec::new(),
        };

        let mut artifact_config = config_value("read");
        artifact_config["intent"]["desired_state"]["conditions"][0]["id"] =
            json!("context-verified");
        artifact_config["intent"]["desired_state"]["conditions"][0]["subject"] =
            json!("cg.context.projection.verified");
        artifact_config["intent"]["desired_state"]["expression"]["value"] =
            json!("context-verified");
        let artifact_intent: Intent =
            serde_json::from_value(artifact_config["intent"].clone()).unwrap();

        let mut mixed = artifact_config.clone();
        mixed["basis"] = serde_json::to_value(basis.clone()).unwrap();
        let (host, owner, root) = workspace_host(Some(mixed));
        let mixed_goal = host.register(&owner, artifact_intent.clone(), config("read").execution);
        assert!(
            matches!(mixed_goal, Err(SessionError::InvalidInput)),
            "unexpected result for mixed goal kinds: {mixed_goal:?}"
        );
        std::fs::remove_dir_all(root).unwrap();

        let mut artifact_only = artifact_config;
        artifact_only["basis"] = serde_json::to_value(basis).unwrap();
        artifact_only
            .as_object_mut()
            .unwrap()
            .remove("connector_goal");
        let (host, owner, root) = workspace_host(Some(artifact_only));
        let registered = host
            .register(&owner, artifact_intent, config("read").execution)
            .unwrap();
        assert!(registered.0.as_artifact().is_some());
        std::fs::remove_dir_all(root).unwrap();

        let (host, owner, root) = workspace_host(Some(config_value("read")));
        assert!(host.connector_runtime().is_ok());
        assert!(matches!(
            host.register(&owner, intent, config("read").execution),
            Err(SessionError::UnsupportedCapability)
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_connector_consent_reads_live_store_and_operator_routes_withdrawal() {
        let Ok(connection) = std::env::var("CG_COGNITIVE_TEST_DATABASE") else {
            return;
        };
        let scope = format!(
            "local-consent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let repository = crate::session_store::tests::repository_for_scope(&scope);
        let pending = crate::session_store::tests::create_pending_connector_session(
            &repository,
            "local-consent-session",
        );
        let session = pending.checkpoint.snapshot.session.clone();
        let reference = repository
            .operator_decide_connector_consent(
                &crate::session_store::tests::owner(),
                &session,
                &PrincipalId::new("operator").unwrap(),
                "approve",
                100,
            )
            .unwrap()
            .unwrap();

        let (host, app, owner, root) = workspace_fixture(
            Some(config_value("read")),
            &scope,
            "owner",
            Some(&connection),
        );
        assert!(matches!(
            host.load_connector_consent(&owner, &reference).unwrap(),
            ConnectorConsentStatus::Granted(_)
        ));
        let ConnectorConsentStatus::Granted(grant) =
            host.load_connector_consent(&owner, &reference).unwrap()
        else {
            panic!("expected the current connector grant")
        };
        assert!(matches!(
            host.live_connector_consent(&grant).unwrap(),
            ConnectorConsentStatus::Granted(_)
        ));
        let mut foreign = owner.clone();
        foreign.project = ProjectId::new("foreign-project").unwrap();
        assert_eq!(
            host.load_connector_consent(&foreign, &reference),
            Err(SessionError::ScopeDenied)
        );
        // A real connector receipt cannot be reinterpreted as an artifact
        // grant, and owner isolation applies before either store lookup.
        let artifact_reference = ConsentRecordRef(reference.0.clone());
        assert_eq!(
            host.load_consent(&foreign, &artifact_reference),
            Err(SessionError::ScopeDenied)
        );
        assert_eq!(
            host.load_consent(&owner, &artifact_reference),
            Err(SessionError::Unavailable)
        );
        let admitted = std::fs::read_to_string(&host.access.admission).unwrap();
        let mut revoked: Value = serde_json::from_str(&admitted).unwrap();
        revoked["mappings"][0]["sessions"]["issuers"] = json!([]);
        std::fs::write(&host.access.admission, revoked.to_string()).unwrap();
        assert_eq!(
            host.live_connector_consent(&grant),
            Err(SessionError::AuthorityDenied)
        );
        std::fs::write(&host.access.admission, admitted).unwrap();
        assert_eq!(
            host.issuer(&owner, &PrincipalId::new("unadmitted-issuer").unwrap()),
            Err(SessionError::AuthorityDenied)
        );

        let response = app
            .operator(&session, &PrincipalId::new("operator").unwrap(), "withdraw")
            .unwrap();
        assert!(response["connector_consent_reference"].is_null());
        assert_eq!(
            host.load_connector_consent(&owner, &reference).unwrap(),
            ConnectorConsentStatus::Withdrawn
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shared_local_access_keeps_current_authority_independent_of_host_lifetime() {
        let (host, owner, root) = workspace_host(Some(config_value("read")));
        let access = Arc::clone(&host.access);
        assert!(Arc::ptr_eq(&access, &host.access));
        drop(host);
        assert_eq!(access.owner(), Ok(owner));
        assert!(access.current().is_ok());
        let mut admission: Value =
            serde_json::from_str(&std::fs::read_to_string(&access.admission).unwrap()).unwrap();
        admission["mappings"][0]["sessions"]["enabled"] = json!(false);
        std::fs::write(&access.admission, admission.to_string()).unwrap();
        assert!(matches!(
            access.current(),
            Err(SessionError::AuthorityDenied)
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn current_local_authority_rejects_unreadable_oversized_and_missing_admission() {
        let (host, owner, root) = workspace_host(Some(config_value("read")));
        let original = std::fs::read(&host.access.admission).unwrap();
        std::fs::write(&host.access.admission, [0xff, 0xfe]).unwrap();
        assert!(matches!(host.current(), Err(SessionError::InvalidInput)));
        std::fs::write(&host.access.admission, vec![b' '; 1_048_576]).unwrap();
        assert!(matches!(host.current(), Err(SessionError::LimitExceeded)));
        std::fs::remove_file(&host.access.admission).unwrap();
        assert!(matches!(
            host.disclose(&owner, &SessionId::new("existing-session").unwrap()),
            Err(SessionError::Unavailable)
        ));
        std::fs::write(&host.access.admission, original).unwrap();
        assert!(host.current().is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_resource_claim_revalidates_live_identity_phase_and_reservation() {
        let scope = gateway_domain::ContextScopeId::new("resource-authority-scope").unwrap();
        crate::session_resource_test_support::with_preparation_resources(
            scope,
            SessionResourcePlan {
                preparation: SessionResourceAmounts {
                    model_attempts: 2,
                    input_tokens: 100,
                    ..Default::default()
                },
                ..Default::default()
            },
            |resources, _, authority| {
                let allocation = resources
                    .allocate(SessionResourceAmounts {
                        model_attempts: 1,
                        input_tokens: 40,
                        ..Default::default()
                    })
                    .unwrap();
                let current = authority.snapshot();
                let now = std::time::Instant::now();
                let check = |candidate: &FencedSession| {
                    validate_current_resource_claim(&allocation, candidate, 100, now)
                };
                // A proper child slice need not equal its entire phase ceiling.
                assert_eq!(check(&current), Ok(()));
                let mut changed = current.clone();
                changed.checkpoint.owner.project = ProjectId::new("other-project").unwrap();
                assert_eq!(check(&changed), Err(SessionError::ScopeDenied));
                let mut changed = current.clone();
                changed.checkpoint.snapshot.session = SessionId::new("other-session").unwrap();
                assert_eq!(check(&changed), Err(SessionError::StaleRevision));
                let mut changed = current.clone();
                changed.checkpoint.snapshot.run = RunId::new("other-run").unwrap();
                assert_eq!(check(&changed), Err(SessionError::StaleRevision));
                let mut changed = current.clone();
                changed.checkpoint.snapshot.revision =
                    Revision::new(allocation.claim_revision().value() + 1).unwrap();
                assert_eq!(check(&changed), Err(SessionError::StaleRevision));
                let mut changed = current.clone();
                changed.fence = FenceToken::new(allocation.fence().value() + 1).unwrap();
                assert_eq!(check(&changed), Err(SessionError::StaleRevision));
                for state in [
                    SessionState::Runnable,
                    SessionState::PendingConsent,
                    SessionState::Cancelled,
                    SessionState::Completed,
                    SessionState::Dispatching,
                ] {
                    let mut changed = current.clone();
                    changed.checkpoint.snapshot.state = state;
                    assert_eq!(check(&changed), Err(SessionError::InvalidState));
                }
                let mut changed = current.clone();
                changed.checkpoint.snapshot.dispatch_knowledge = DispatchKnowledge::Observed;
                assert_eq!(check(&changed), Err(SessionError::InvalidState));
                let mut changed = current.clone();
                changed.checkpoint.snapshot.dispatch =
                    Some(DispatchId::new("other-dispatch").unwrap());
                assert_eq!(check(&changed), Err(SessionError::InvalidState));
                let mut changed = current.clone();
                changed.checkpoint.resource_ledger = Some(
                    SessionResourceLedger::new(
                        current
                            .checkpoint
                            .resource_ledger
                            .as_ref()
                            .unwrap()
                            .limits()
                            .clone(),
                    )
                    .unwrap(),
                );
                assert_eq!(check(&changed), Err(SessionError::AuthorityDenied));
                let mut changed = current.clone();
                changed.checkpoint.resource_ledger = None;
                assert_eq!(check(&changed), Err(SessionError::UnsupportedCapability));
                // A real fence change observed from the underlying repository
                // immediately invalidates the previously minted child.
                authority.advance_fence();
                assert_eq!(
                    check(&authority.snapshot()),
                    Err(SessionError::StaleRevision)
                );
            },
        );
    }

    #[test]
    fn local_resource_authority_reads_current_postgres_claim_and_configuration() {
        let Ok(connection) = std::env::var("CG_COGNITIVE_TEST_DATABASE") else {
            return;
        };
        use std::cell::Cell;
        struct Probe<'a> {
            local: &'a LocalSessionHost,
            plan: SessionResourcePlan,
            called: &'a Cell<bool>,
        }
        impl InteractionAuthorityPort for Probe<'_> {
            fn load_consent(
                &self,
                owner: &OwnerBinding,
                reference: &ConsentRecordRef,
            ) -> Result<ConsentStatus, SessionError> {
                self.local.load_consent(owner, reference)
            }
            fn live_status(&self, grant: &VerifiedConsent) -> Result<ConsentStatus, SessionError> {
                self.local.live_status(grant)
            }
        }
        impl StructuredSessionHost for Probe<'_> {
            fn now_ms(&self) -> Result<u64, SessionError> {
                self.local.now_ms()
            }
            fn register(
                &self,
                owner: &OwnerBinding,
                intent: Intent,
                execution: RequestedExecution,
            ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
                self.local.register(owner, intent, execution)
            }
            fn prepare(&self, _: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
                Err(SessionError::UnsupportedCapability)
            }
            fn disclose(
                &self,
                owner: &OwnerBinding,
                session: &SessionId,
            ) -> Result<(), SessionError> {
                self.local.disclose(owner, session)
            }
            fn issuer(
                &self,
                owner: &OwnerBinding,
                issuer: &PrincipalId,
            ) -> Result<(), SessionError> {
                self.local.issuer(owner, issuer)
            }
            fn resource_limits(
                &self,
                owner: &OwnerBinding,
                goal: &SupportedSessionGoal,
            ) -> Result<Option<SessionResourceLimits>, SessionError> {
                self.local.resource_limits(owner, goal)
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
                checkpoint: &SessionCheckpoint,
            ) -> Result<InitialAssessment, SessionError> {
                self.local.connector_runtime.initial_assessment(checkpoint)
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
                checkpoint: &SessionCheckpoint,
                _: &PlanStepId,
                resources: &mut SessionPhaseResources,
                _: ConnectorPreparationContext<'_>,
            ) -> Result<ConnectorPreparation, SessionError> {
                let allocation = resources.allocate(SessionResourceAmounts {
                    model_attempts: 1,
                    ..Default::default()
                })?;
                let repository = store(&self.local.current()?)?;
                let current = repository.load(&checkpoint.owner, &checkpoint.snapshot.session)?;
                assert_eq!(&current.checkpoint, checkpoint);
                assert_eq!(self.local.access.validate_allocation(&allocation), Ok(()));
                let pooled_pid = || -> Result<i32, SessionError> {
                    self.local
                        .access
                        .store_connection
                        .lock()
                        .map_err(|_| SessionError::StorageUnavailable)?
                        .as_ref()
                        .ok_or(SessionError::StorageUnavailable)?
                        .1
                        .backend_pid()
                        .map_err(Into::into)
                };
                let first_pid = pooled_pid()?;
                assert_ne!(
                    repository.backend_pid()?,
                    first_pid,
                    "the existing writer must remain an independent connection"
                );
                for _ in 0..3 {
                    self.local.access.validate_allocation(&allocation)?;
                    assert_eq!(
                        pooled_pid()?,
                        first_pid,
                        "unchanged locator and scope reuse the connection, never its claim"
                    );
                }
                let original = std::fs::read_to_string(&self.local.access.admission).unwrap();
                let base: Value = serde_json::from_str(&original).unwrap();
                let write = |value: &Value| {
                    std::fs::write(&self.local.access.admission, value.to_string()).unwrap()
                };
                for (field, value) in [
                    ("max_actions", json!(3)),
                    ("max_retries", json!(2)),
                    (
                        "execution",
                        json!({"mode":"HARDENING","profile":"FULL_PATH"}),
                    ),
                ] {
                    let mut changed = base.clone();
                    changed["mappings"][0]["sessions"][field] = value;
                    write(&changed);
                    assert_eq!(
                        self.local.access.validate_allocation(&allocation),
                        Err(SessionError::StaleRevision)
                    );
                }
                // Changing the authenticated workspace or client owner cannot
                // redirect an existing allocation, even to a valid mapping.
                for (field, value) in [
                    (
                        "scope",
                        json!({"workspace_id":"w","project_id":"other-project","binding_id":"b"}),
                    ),
                    ("canonical_scope", json!("other-canonical-scope")),
                    ("principal", json!("other-operator")),
                    ("session_id", json!("other-client-owner")),
                ] {
                    let mut changed = base.clone();
                    changed["mappings"][0][field] = value;
                    // Keep admitted records individually valid so rejection
                    // proves the host binding check, not a malformed record.
                    if field == "scope" {
                        let scope = changed["mappings"][0]["scope"].clone();
                        for resource in changed["mappings"][0]["resources"].as_array_mut().unwrap()
                        {
                            resource["scope"] = scope.clone();
                        }
                    }
                    write(&changed);
                    assert_eq!(
                        self.local.access.validate_allocation(&allocation),
                        Err(SessionError::ScopeDenied)
                    );
                }
                let mut changed = base.clone();
                changed["mappings"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("sessions");
                write(&changed);
                assert_eq!(
                    self.local.access.validate_allocation(&allocation),
                    Err(SessionError::AuthorityDenied)
                );
                let mut changed = base.clone();
                changed["mappings"][0]["sessions"]
                    .as_object_mut()
                    .unwrap()
                    .remove("connector_goal");
                write(&changed);
                assert_eq!(
                    self.local.access.validate_allocation(&allocation),
                    Err(SessionError::InvalidInput)
                );
                let mut changed = base.clone();
                changed["mappings"][0]["sessions"]["connector_goal"]["plan"]["revision"] =
                    json!("changed-plan");
                write(&changed);
                assert_eq!(
                    self.local.access.validate_allocation(&allocation),
                    Err(SessionError::StaleRevision)
                );
                let mut changed = base.clone();
                changed["mappings"][0]["sessions"]["connector_goal"]["steps"] =
                    json!(["different-registered-step"]);
                write(&changed);
                assert_eq!(
                    self.local.access.validate_allocation(&allocation),
                    Err(SessionError::StaleRevision)
                );
                let mut changed = base.clone();
                changed["mappings"][0]["sessions"]["store_file"] =
                    json!("relative-database-credentials");
                write(&changed);
                assert_eq!(
                    self.local.access.validate_allocation(&allocation),
                    Err(SessionError::InvalidInput)
                );
                let mut changed = base.clone();
                changed["mappings"][0]["sessions"]["resource_limits"]["amounts"]["model_attempts"] =
                    json!(4);
                write(&changed);
                assert_eq!(
                    self.local.access.validate_allocation(&allocation),
                    Err(SessionError::StaleRevision)
                );
                let mut changed = base.clone();
                changed["mappings"][0]["sessions"]
                    .as_object_mut()
                    .unwrap()
                    .remove("resource_limits");
                write(&changed);
                assert_eq!(
                    self.local.access.validate_allocation(&allocation),
                    Err(SessionError::UnsupportedCapability)
                );
                let mut changed = base.clone();
                changed["mappings"][0]["sessions"]["enabled"] = json!(false);
                write(&changed);
                assert_eq!(
                    self.local.access.validate_allocation(&allocation),
                    Err(SessionError::AuthorityDenied)
                );
                write(&base);
                assert_eq!(self.local.access.validate_allocation(&allocation), Ok(()));
                let before_cancel_pid = pooled_pid()?;
                // Atomically stop the real persisted claim, advancing both
                // revision and fencing token. The old allocation stays inert.
                let mut stopped = current.checkpoint.clone();
                stopped.snapshot.revision = stopped.snapshot.revision.next()?;
                stopped.snapshot.state = SessionState::Cancelled;
                stopped.snapshot.dispatch_knowledge = DispatchKnowledge::Stopped;
                stopped.lease_until_ms = None;
                let advanced = repository.append(
                    &checkpoint.owner,
                    JournalAppend {
                        session: checkpoint.snapshot.session.clone(),
                        expected_revision: checkpoint.snapshot.revision,
                        fence: current.fence,
                        command: None,
                        next: stopped,
                        records: Vec::new(),
                    },
                )?;
                assert_ne!(advanced.fence, allocation.fence());
                assert_eq!(
                    self.local.access.validate_allocation(&allocation),
                    Err(SessionError::StaleRevision)
                );
                assert_eq!(
                    pooled_pid()?,
                    before_cancel_pid,
                    "the reused reader must observe the independent writer's committed fence"
                );
                self.called.set(true);
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
                unreachable!("probe cannot authorize execution")
            }
            fn verify_goal(
                &self,
                _: &SessionCheckpoint,
                _: &PlanStepId,
                _: &DispatchId,
                _: &[PersistedConnectorReadback],
            ) -> Result<
                Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>,
                SessionError,
            > {
                Err(SessionError::UnsupportedCapability)
            }
        }
        fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
            std::fs::create_dir_all(destination).unwrap();
            for entry in std::fs::read_dir(source).unwrap() {
                let entry = entry.unwrap();
                let target = destination.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy_tree(&entry.path(), &target);
                } else {
                    std::fs::copy(entry.path(), target).unwrap();
                }
            }
        }
        let scope = format!(
            "local-resource-authority-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let (mut local, _, owner, root) = workspace_fixture(
            Some(config_value("read")),
            &scope,
            "authority-client",
            Some(&connection),
        );
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        copy_tree(
            &fixtures.join("declarative-cli/catalog"),
            &root.join("catalog"),
        );
        let mut plan: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/declarative-v0.1/plan.json"
        ))
        .unwrap();
        plan["assessment"]["scope"] = json!(scope);
        let rules: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/declarative-cli/rules.json"
        ))
        .unwrap();
        let process: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/declarative-cli/process.json"
        ))
        .unwrap();
        let mut admission: Value =
            serde_json::from_str(&std::fs::read_to_string(&local.access.admission).unwrap())
                .unwrap();
        let mapping = &mut admission["mappings"][0];
        let mut canonical = json!({"catalog":root.join("catalog"),"policy":{}});
        let mut references = std::collections::BTreeMap::new();
        for (name, contract, document) in [
            ("plan", "cg.plan", plan.clone()),
            ("rules", "cg.composition-rules", rules),
            ("process", "cg.process-snapshot", process),
        ] {
            let digest = format!(
                "sha256:{}",
                gateway_domain::context_content_digest(document.to_string().as_bytes()).as_str()
            );
            let reference = json!({"id":name,"contract":contract,"contract_version":"1.0","revision":"1","digest":digest});
            let wire_scope = mapping["scope"].clone();
            mapping["resources"].as_array_mut().unwrap().push(json!({"schema_version":"1.0","scope":wire_scope,"reference":reference,"document":document,"provenance":[{"reference":reference,"source_id":name,"source_revision":"1","freshness":"current","sensitivity":"NORMAL","lineage":[]}]}));
            canonical[name] = reference.clone();
            references.insert(name, reference);
        }
        mapping["canonical"] = canonical;
        let config = &mut mapping["sessions"];
        config["intent"] = plan["assessment"]["document"]["intent"].clone();
        config["connector_goal"]["plan"] =
            json!({"id":"plan","revision":"1","digest":references["plan"]["digest"]});
        config["connector_goal"]["steps"] = json!(["step-condition.0.0", "step-condition.0.1"]);
        config["connector_goal"]["observations"] = json!([
            {"condition":"architecture","source":"repository","capability":"repository.read","operation_class":"read"},
            {"condition":"coverage","source":"repository","capability":"repository.read","operation_class":"read"}
        ]);
        let resource_plan = SessionResourcePlan {
            preparation: SessionResourceAmounts {
                model_attempts: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        config["resource_limits"] = serde_json::to_value(SessionResourceLimits {
            cost_unit: gateway_domain::ReferenceId::new("credits").unwrap(),
            amounts: resource_plan.total().unwrap(),
        })
        .unwrap();
        std::fs::write(&local.access.admission, admission.to_string()).unwrap();
        let resolver = LocalWorkspaceResolver::from_json(&admission.to_string()).unwrap();
        let new_binding = resolver
            .resolve(&WorkspaceReference {
                working_directory: local.access.cwd.clone(),
                repository: local.access.repository.clone(),
            })
            .unwrap();
        rebind_host(&mut local, new_binding);
        let repository = store(&local.current().unwrap()).unwrap();
        let called = Cell::new(false);
        let probe = Probe {
            local: &local,
            plan: resource_plan,
            called: &called,
        };
        let coordinator = SessionCoordinator::new(repository, probe);
        let intent = local.current().unwrap().sessions.unwrap().intent;
        let started = coordinator.execute(
            &owner,
            SessionCommand::Start {
                command: CommandId::new("authority-start").unwrap(),
                intent,
                execution: RequestedExecution {
                    mode: gateway_domain::OperatingMode::Development,
                    profile: gateway_domain::ExecutionProfile::FullPath,
                },
            },
        );
        let started = started.unwrap();
        assert!(
            coordinator
                .execute(
                    &owner,
                    SessionCommand::Continue {
                        at: Mutation {
                            session: started.session,
                            expected_revision: started.revision,
                            command: CommandId::new("authority-continue").unwrap()
                        }
                    }
                )
                .is_err()
        );
        assert!(called.get());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_resource_authority_does_not_promote_claims_to_owner_or_operation_grants() {
        let (host, _, root) = workspace_host(Some(config_value("read")));
        crate::session_resource_test_support::with_preparation_resources(
            gateway_domain::ContextScopeId::new("repository-scope").unwrap(),
            SessionResourcePlan {
                preparation: SessionResourceAmounts {
                    model_attempts: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
            |resources, _, _| {
                let allocation = resources
                    .allocate(SessionResourceAmounts {
                        model_attempts: 1,
                        ..Default::default()
                    })
                    .unwrap();
                // This real claim belongs to a different authenticated owner,
                // even though its canonical scope matches the local workspace.
                assert_eq!(
                    host.access.validate_allocation(&allocation),
                    Err(SessionError::ScopeDenied)
                );
                let target = gateway_application::connectors::ConnectorTarget {
                    connector: gateway_domain::NonEmptyText::new("configured-connector").unwrap(),
                    scope: allocation.binding().scope.clone(),
                };
                assert_eq!(
                    host.access
                        .validate_connector_discovery(&allocation, &target),
                    Err(SessionError::UnsupportedCapability)
                );
                let prepared = prepared_connector_turn();
                assert_eq!(
                    host.access
                        .validate_invocation_allocation(&allocation, prepared.invocation()),
                    Err(SessionError::UnsupportedCapability)
                );
            },
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_resource_claim_enforces_both_deadlines_and_execution_phase() {
        let prepared = prepared_connector_turn();
        let scope = prepared
            .invocation()
            .prepared()
            .invocation()
            .data()
            .task
            .scope
            .clone();
        crate::session_resource_test_support::with_execution_resources(
            scope,
            SessionResourcePlan {
                execution: SessionResourceAmounts {
                    connector_calls: 2,
                    ..Default::default()
                },
                ..Default::default()
            },
            prepared,
            |resources, _, authority| {
                let allocation = resources
                    .allocate(SessionResourceAmounts {
                        connector_calls: 1,
                        ..Default::default()
                    })
                    .unwrap();
                let current = authority.snapshot();
                let now = std::time::Instant::now();
                assert_eq!(
                    validate_current_resource_claim(&allocation, &current, 100, now),
                    Ok(())
                );
                let mut changed = current.clone();
                changed.checkpoint.lease_until_ms = Some(100);
                assert_eq!(
                    validate_current_resource_claim(&allocation, &changed, 100, now),
                    Err(SessionError::LimitExceeded)
                );
                let mut changed = current.clone();
                changed.checkpoint.budget.deadline_ms = 100;
                assert_eq!(
                    validate_current_resource_claim(&allocation, &changed, 100, now),
                    Err(SessionError::LimitExceeded)
                );
                let mut changed = current.clone();
                changed.checkpoint.lease_until_ms = None;
                assert_eq!(
                    validate_current_resource_claim(&allocation, &changed, 100, now),
                    Err(SessionError::InvalidState)
                );
                assert_eq!(
                    validate_current_resource_claim(
                        &allocation,
                        &current,
                        100,
                        allocation.deadline()
                    ),
                    Err(SessionError::LimitExceeded)
                );
                let mut changed = current.clone();
                changed.checkpoint.snapshot.dispatch_knowledge = DispatchKnowledge::Unknown;
                assert_eq!(
                    validate_current_resource_claim(&allocation, &changed, 100, now),
                    Err(SessionError::InvalidState)
                );
            },
        );
    }

    #[test]
    fn local_connector_runtime_reports_unknown_and_fails_closed_after_assessment() {
        let scope = gateway_domain::ContextScopeId::new("repository-scope").unwrap();
        let intent = config("read").intent;
        let plan = content_reference("connector-plan", b"plan").unwrap();
        let step = PlanStepId::new("connector-step").unwrap();
        let condition = intent.desired_state().conditions()[0].id().clone();
        let goal = SupportedSessionGoal::connector(
            SupportedConnectorGoal::from_host_registration(
                intent,
                scope,
                plan.clone(),
                vec![step.clone()],
                vec![
                    ConditionObservationMapping::new(
                        condition,
                        gateway_domain::SourceId::new("repository").unwrap(),
                        CapabilityId::new("repository.read").unwrap(),
                        ConnectorOperationClass::Read,
                    )
                    .unwrap(),
                ],
            )
            .unwrap(),
        );
        let session = SessionId::new("connector-session").unwrap();
        let checkpoint = SessionCheckpoint {
            owner: OwnerBinding {
                principal: PrincipalId::new("operator").unwrap(),
                workspace: WorkspaceId::new("workspace").unwrap(),
                project: ProjectId::new("project").unwrap(),
                binding: BindingId::new("binding").unwrap(),
                client_owner: ClientOwnerId::new("client-owner").unwrap(),
            },
            goal,
            execution: RequestedExecution {
                mode: gateway_domain::OperatingMode::Development,
                profile: gateway_domain::ExecutionProfile::FullPath,
            },
            snapshot: SessionSnapshot {
                session: session.clone(),
                run: RunId::new("run").unwrap(),
                revision: Revision::new(1).unwrap(),
                state: SessionState::Runnable,
                pending: None,
                dispatch: None,
                dispatch_knowledge: DispatchKnowledge::None,
                command_outcome: None,
                final_evidence: None,
            },
            budget: SessionBudget {
                actions: 0,
                retries: 0,
                max_actions: 1,
                max_retries: 0,
                deadline_ms: 1_000,
            },
            resource_ledger: None,
            pending: None,
            accepted_consent: None,
            accepted_connector_consent: None,
            authority_events: Vec::new(),
            selected_source: None,
            artifact: None,
            lease_until_ms: None,
            initial_assessment: None,
            connector_step_snapshots: Vec::new(),
            connector_results: Vec::new(),
            connector_observations: Vec::new(),
            pre_execution_goal: None,
        };
        let (host, _, root) = workspace_host(Some(config_value("read")));
        let runtime = &host.connector_runtime;
        let assessment = runtime.initial_assessment(&checkpoint).unwrap();
        assert_eq!(assessment.outcome, "UNKNOWN");
        assert_eq!(assessment.authority, plan);
        assert!(matches!(
            runtime.prepare_step(&checkpoint, &step),
            Err(SessionError::UnsupportedCapability)
        ));
        let prepared_turn = prepared_connector_turn();
        let dispatch = DispatchId::new("dispatch").unwrap();
        assert!(matches!(
            runtime.resource_plan(&checkpoint, &step),
            Err(SessionError::UnsupportedCapability)
        ));
        crate::session_resource_test_support::with_preparation_resources(
            checkpoint.goal.scope().clone(),
            SessionResourcePlan {
                preparation: SessionResourceAmounts {
                    model_attempts: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
            |resources, _, _| {
                assert!(matches!(
                    runtime.execute_step(&checkpoint, &step, resources, &dispatch, &prepared_turn),
                    Err(SessionError::UnsupportedCapability)
                ));
            },
        );
        assert!(matches!(
            runtime.verify_goal(
                &checkpoint,
                &step,
                &DispatchId::new("dispatch").unwrap(),
                &[]
            ),
            Err(SessionError::ScopeDenied)
        ));
        assert_eq!(checkpoint.snapshot.session, session);
        std::fs::remove_dir_all(root).unwrap();
    }
}
