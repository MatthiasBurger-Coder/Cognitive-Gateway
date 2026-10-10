//! Launcher-owned current admission and v2 mapping. All lifecycle lives in application.
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
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionAdmission {
    pub store_file: PathBuf,
    pub intent: Intent,
    pub basis: ArtifactGoalBasis,
    pub execution: RequestedExecution,
    pub max_actions: u32,
    pub max_retries: u32,
    pub ttl_ms: u64,
    pub consent_required: bool,
    pub enabled: bool,
    pub issuers: Vec<PrincipalId>,
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
    // Local database credentials are read only by this trusted storage adapter.
    Ok(PostgresSessionStore::new(CognitiveStore::connect(
        text.trim(),
        host.binding.canonical_scope.clone(),
    )?))
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
        LocalSessionHost {
            binding: self.binding.clone(),
            admission: self.admission.clone(),
            cwd: self.cwd.clone(),
            repository: self.repository.clone(),
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
struct LocalSessionHost {
    binding: ScopeBinding,
    admission: String,
    cwd: String,
    repository: String,
}
impl LocalSessionHost {
    fn current(&self) -> Result<LocalCodexHost, SessionError> {
        let resolver =
            LocalWorkspaceResolver::from_json(&read(&self.admission, 1_048_575)?).map_err(error)?;
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
        let status = store(&host)?.consent(owner, reference)?;
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
        let goal = SupportedArtifactGoal::validate(intent, config.basis.clone())?;
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
        if config.basis != *checkpoint.goal.basis()
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
        if canonical.plan["id"] != config.basis.plan.id().as_str()
            || canonical.plan["revision"] != config.basis.plan.revision().as_str()
            || canonical.plan["digest"] != config.basis.plan.digest()
        {
            return Err(SessionError::StaleRevision);
        }
        let (snapshot, rules) = host_mapping::capture(
            &canonical.catalog,
            &canonical.documents(&host.resources).map_err(error)?,
        )
        .map_err(error)?;
        if snapshot.scope != config.basis.scope
            || snapshot.scope != host.binding.canonical_scope
            || snapshot.operating_mode != checkpoint.execution.mode
            || snapshot.execution_profile != checkpoint.execution.profile
        {
            return Err(SessionError::ScopeDenied);
        }
        let resolved = DeclarativeResolutionApplication
            .resolve_plan(&snapshot, &rules)
            .map_err(|_| SessionError::InvalidInput)?;
        let mut projection = record(&config.basis.projection, "cg.context-projection")?.clone();
        let mut fragments = Vec::new();
        for source in &config.basis.sources {
            let fragment = record(source, "cg.context-fragment")?.clone();
            if fragment["id"] != source.id().as_str() {
                return Err(SessionError::InvalidInput);
            }
            fragments.push(fragment);
        }
        if !config.basis.sources.is_empty() {
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
        if input.projection.mapping.step != config.basis.step {
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
