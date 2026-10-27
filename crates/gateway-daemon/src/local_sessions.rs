//! Launcher-owned current admission and v2 mapping. All lifecycle lives in application.
#[cfg(test)]
#[path = "local_sessions/before_inspection_tests.rs"]
mod before_inspection_tests;
#[cfg(test)]
#[path = "local_sessions/boundary_tests.rs"]
mod boundary_tests;
mod definition_json;
mod observed_goal;
use crate::{
    codex_workspace::{LocalCodexHost, LocalWorkspaceResolver},
    cognitive_store::CognitiveStore,
    declarative_cli::host_mapping,
    session_store::PostgresSessionStore,
};
use gateway_application::{
    codex::*, resolution_application::DeclarativeResolutionApplication, sessions::*,
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
    sync::{Arc, Mutex},
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
    #[serde(default)]
    pub resource_limits: Option<SessionResourceLimits>,
    #[serde(default)]
    observed_goal_runtime: Option<observed_goal::definitions::ReadonlyRuntimeAdmission>,
    pub execution: RequestedExecution,
    pub max_actions: u32,
    pub max_retries: u32,
    pub ttl_ms: u64,
    pub consent_required: bool,
    pub enabled: bool,
    pub issuers: Vec<PrincipalId>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConnectorGoalAdmission {
    plan: RecordRef,
    steps: Vec<String>,
    observations: Vec<ConnectorObservationAdmission>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectorObservationAdmission {
    condition: String,
    source: String,
    capability: String,
    operation_class: String,
}

fn registered_connector_goal(
    config: &SessionAdmission,
    scope: &gateway_domain::ContextScopeId,
) -> Result<SupportedConnectorGoal, SessionError> {
    let goal = config
        .connector_goal
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    let steps = goal
        .steps
        .iter()
        .map(|step| PlanStepId::new(step).map_err(|_| SessionError::InvalidInput))
        .collect::<Result<Vec<_>, _>>()?;
    let observations = goal
        .observations
        .iter()
        .map(|mapping| {
            let class = match mapping.operation_class.as_str() {
                "read" => gateway_application::connectors::ConnectorOperationClass::Read,
                "search" => gateway_application::connectors::ConnectorOperationClass::Search,
                "inspect" => gateway_application::connectors::ConnectorOperationClass::Inspect,
                _ => return Err(SessionError::UnsupportedCapability),
            };
            ConditionObservationMapping::new(
                gateway_domain::ConditionId::new(&mapping.condition)
                    .map_err(|_| SessionError::InvalidInput)?,
                gateway_domain::SourceId::new(&mapping.source)
                    .map_err(|_| SessionError::InvalidInput)?,
                CapabilityId::new(&mapping.capability).map_err(|_| SessionError::InvalidInput)?,
                class,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    SupportedConnectorGoal::from_host_registration(
        config.intent.clone(),
        scope.clone(),
        goal.plan.clone(),
        steps,
        observations,
    )
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
        let access = Arc::new(LocalSessionAccess {
            binding: self.binding.clone(),
            admission: self.admission.clone(),
            cwd: self.cwd.clone(),
            repository: self.repository.clone(),
            selected_store: Mutex::new(None),
        });
        LocalSessionHost {
            goal_runtime: observed_goal::ObservedGoalRuntime::new(Arc::clone(&access)),
            access,
        }
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
        let coordinator = SessionCoordinator::new(
            host.access
                .selected_repository(&admitted)
                .map_err(SessionError::code)?,
            host,
        );
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
        let repository = host.access.selected_repository(&admitted)?;
        if decision == "inspect-before" {
            let current = repository.load(&owner, session)?;
            let reference = current
                .checkpoint
                .prepared_before
                .as_ref()
                .ok_or(SessionError::Unavailable)?;
            let historical = load_prepared_before_historical(&repository, &current.checkpoint)?;
            let snapshot: Value = serde_json::from_slice(
                &historical
                    .encode()
                    .map_err(|_| SessionError::InvalidInput)?,
            )
            .map_err(|_| SessionError::InvalidInput)?;
            // Original capture DATA remains readable after freshness expiry.
            // This query neither issues consent nor establishes current usage.
            let result = json!({"contract":"cg.complete-before-inspection","contract_version":"1.0",
                "historical":true,"before":reference,"snapshot":snapshot});
            let frame = json!({"schema_version":"2.0","status":"ok","result":&result});
            if !security::credential_free(&frame) {
                return Err(SessionError::AuthorityDenied);
            }
            if serde_json::to_vec(&frame)
                .map_err(|_| SessionError::InvalidInput)?
                .len()
                >= crate::local_mcp::MAX_FRAME_BYTES
            {
                return Err(SessionError::LimitExceeded);
            }
            host.issuer(&owner, issuer)?;
            host.access.selected_repository(&host.current()?)?;
            return Ok(result);
        }
        if decision == "recover" {
            let c = SessionCoordinator::new(repository, host);
            return Ok(json!({"session":c.recover(&owner,session)?}));
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
        let repository = host
            .access
            .selected_repository(&admitted)
            .map_err(|_| FacadeError::ReferenceUnavailable)?;
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
struct LocalSessionHost {
    access: Arc<LocalSessionAccess>,
    goal_runtime: observed_goal::ObservedGoalRuntime,
}
impl LocalSessionHost {
    fn current(&self) -> Result<LocalCodexHost, SessionError> {
        self.access.current()
    }
    fn owner(&self) -> Result<OwnerBinding, SessionError> {
        self.access.owner()
    }
}
/// One shared definition of current launcher identity for the session host and
/// its connector adapters. Each access still resolves fresh admission; this is
/// not a cache of permissions, session records or observations.
struct LocalSessionAccess {
    binding: ScopeBinding,
    admission: String,
    cwd: String,
    repository: String,
    selected_store: Mutex<Option<SelectedSessionStore>>,
}
struct SelectedSessionStore {
    path: PathBuf,
    connection: String,
    scope: gateway_domain::ContextScopeId,
    repository: PostgresSessionStore,
}
impl LocalSessionAccess {
    /// Pin the selected database for this one command and share its connection,
    /// never its rows or authority decisions. A selector change fails closed.
    fn selected_repository(
        &self,
        host: &LocalCodexHost,
    ) -> Result<PostgresSessionStore, SessionError> {
        let config = host
            .sessions
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        if !config.store_file.is_absolute() {
            return Err(SessionError::InvalidInput);
        }
        if host.binding.canonical_scope != self.binding.canonical_scope {
            return Err(SessionError::ScopeDenied);
        }
        let connection = read(
            config
                .store_file
                .to_str()
                .ok_or(SessionError::InvalidInput)?,
            4096,
        )?;
        let connection = connection.trim();
        if connection.is_empty() {
            return Err(SessionError::StorageUnavailable);
        }
        let mut slot = self
            .selected_store
            .lock()
            .map_err(|_| SessionError::StorageUnavailable)?;
        if let Some(selected) = slot.as_ref() {
            if selected.path != config.store_file
                || selected.connection != connection
                || selected.scope != host.binding.canonical_scope
            {
                return Err(SessionError::StaleRevision);
            }
            return Ok(selected.repository.clone());
        }
        let repository = PostgresSessionStore::new(CognitiveStore::connect(
            connection,
            host.binding.canonical_scope.clone(),
        )?);
        *slot = Some(SelectedSessionStore {
            path: config.store_file.clone(),
            connection: connection.to_owned(),
            scope: host.binding.canonical_scope.clone(),
            repository: repository.clone(),
        });
        Ok(repository)
    }
    fn current(&self) -> Result<LocalCodexHost, SessionError> {
        let definition = read(&self.admission, 1_048_575)?;
        definition_json::parse(&definition)?;
        let resolver = LocalWorkspaceResolver::from_json(&definition).map_err(error)?;
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
        Ok(host)
    }
    fn owner(&self) -> Result<OwnerBinding, SessionError> {
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
        let status = self
            .access
            .selected_repository(&host)?
            .consent(owner, reference)?;
        if let ConsentStatus::Granted(grant) = &status {
            self.issuer(owner, grant.issuer())?;
        }
        Ok(status)
    }
    fn live_status(&self, grant: &VerifiedConsent) -> Result<ConsentStatus, SessionError> {
        self.load_consent(&grant.binding().owner, grant.record())
    }
}
impl StructuredSessionHost for LocalSessionHost {
    fn goal_observation_runtime(&self) -> Option<&dyn ReadonlyGoalObservationRuntimePort> {
        Some(&self.goal_runtime)
    }
    fn register_goal(
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
        match (
            &config.basis,
            &config.connector_goal,
            &config.observed_goal_runtime,
        ) {
            (Some(_), None, None) => self
                .register(owner, intent, execution)
                .map(|(goal, budget)| (goal.into(), budget)),
            (None, Some(_), Some(runtime)) => {
                if config.intent != intent || config.execution != execution {
                    return Err(SessionError::UnsupportedCapability);
                }
                if config.ttl_ms == 0 || config.ttl_ms > 86_400_000 {
                    return Err(SessionError::LimitExceeded);
                }
                config
                    .resource_limits
                    .as_ref()
                    .ok_or(SessionError::UnsupportedCapability)?
                    .validate()?;
                let goal = registered_connector_goal(config, &host.binding.canonical_scope)?;
                runtime.capture(&host, &goal, execution)?;
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
                Ok((SupportedSessionGoal::connector(goal), budget))
            }
            _ => Err(SessionError::InvalidInput),
        }
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
    fn register(
        &self,
        owner: &OwnerBinding,
        intent: Intent,
        execution: RequestedExecution,
    ) -> Result<(SupportedArtifactGoal, SessionBudget), SessionError> {
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
        let goal = SupportedArtifactGoal::validate(
            intent,
            config
                .basis
                .clone()
                .ok_or(SessionError::UnsupportedCapability)?,
        )?;
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
        let basis = config
            .basis
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        if basis != checkpoint.goal.artifact_basis()?
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
        if canonical.plan["id"] != basis.plan.id().as_str()
            || canonical.plan["revision"] != basis.plan.revision().as_str()
            || canonical.plan["digest"] != basis.plan.digest()
        {
            return Err(SessionError::StaleRevision);
        }
        let (snapshot, rules) = host_mapping::capture(
            &canonical.catalog,
            &canonical.documents(&host.resources).map_err(error)?,
        )
        .map_err(error)?;
        if snapshot.scope != basis.scope
            || snapshot.scope != host.binding.canonical_scope
            || snapshot.operating_mode != checkpoint.execution.mode
            || snapshot.execution_profile != checkpoint.execution.profile
        {
            return Err(SessionError::ScopeDenied);
        }
        let resolved = DeclarativeResolutionApplication
            .resolve_plan(&snapshot, &rules)
            .map_err(|_| SessionError::InvalidInput)?;
        let mut projection = record(&basis.projection, "cg.context-projection")?.clone();
        let mut fragments = Vec::new();
        for source in &basis.sources {
            let fragment = record(source, "cg.context-fragment")?.clone();
            if fragment["id"] != source.id().as_str() {
                return Err(SessionError::InvalidInput);
            }
            fragments.push(fragment);
        }
        if !basis.sources.is_empty() {
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
        if input.projection.mapping.step != basis.step {
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
