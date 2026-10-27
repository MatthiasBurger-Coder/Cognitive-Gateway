//! Actual native definitions, PostgreSQL claims and coordinator-minted children.
use super::*;
use crate::goal_observation_test_harness as allocation_harness;
use crate::{
    codex_workspace::LocalWorkspaceResolver,
    local_sessions::{LocalSessionAccess, LocalSessionHost},
    mcp_client::session_budget::SessionConnectorRun,
};
use gateway_application::codex::{WorkspaceReference, WorkspaceResolver};
use std::{cell::RefCell, path::PathBuf, process::Command};

type InventoryHook = dyn FnOnce(
    &Arc<GoalSource>,
    &SessionConnectorRun,
    &ConnectorInventory,
) -> Result<(), SessionError>;
thread_local! {
    static INVENTORY_HOOK: RefCell<Option<Box<InventoryHook>>> = const { RefCell::new(None) };
}
pub(in crate::local_sessions::observed_goal) fn after_inventory(
    source: &Arc<GoalSource>,
    run: &SessionConnectorRun,
    inventory: &ConnectorInventory,
) -> Result<(), SessionError> {
    let hook = INVENTORY_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook(source, run, inventory)?;
    }
    Ok(())
}
pub(in crate::local_sessions) struct Fixture {
    root: PathBuf,
    access: Arc<LocalSessionAccess>,
}
impl Fixture {
    pub(in crate::local_sessions) fn new() -> Self {
        Self::configured(true)
    }
    fn single_source() -> Self {
        Self::configured(false)
    }
    fn configured(multiple: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cg403-boundary-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let support = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support");
        let code = "import sys,json;from pathlib import Path;sys.path.insert(0,sys.argv[1]);import observed_goal_session as f;launch,request,intent,root=f.configure(Path(sys.argv[2]),sys.argv[3]=='true');print('BOUNDARY_SETUP='+json.dumps(launch))";
        let bin = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_owned();
        let output = Command::new("python3")
            .arg("-c")
            .arg(code)
            .arg(support)
            .arg(&root)
            .arg(if multiple { "true" } else { "false" })
            .env("CG_QUALIFICATION_BIN_DIR", bin)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "native setup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        let launch: Vec<String> = serde_json::from_str(
            text.lines()
                .find_map(|l| l.strip_prefix("BOUNDARY_SETUP="))
                .expect("actual native setup marker"),
        )
        .unwrap();
        let peer_path = root.join("peer.py");
        let peer = std::fs::read_to_string(&peer_path).unwrap();
        let emit = " print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':v}),flush=True)";
        assert!(peer.contains(emit));
        std::fs::write(&peer_path, peer.replace(emit, &format!(" with (root/'response-times.jsonl').open('a') as f:f.write(json.dumps({{'method':m,'at_ms':int(time.time()*1000)}})+'\\n')\n{emit}"))).unwrap();
        let flag = |name: &str| launch[launch.iter().position(|v| v == name).unwrap() + 1].clone();
        let admission = flag("--admission");
        let cwd = flag("--cwd");
        let repository = flag("--repository");
        let resolver =
            LocalWorkspaceResolver::from_json(&std::fs::read_to_string(&admission).unwrap())
                .unwrap();
        let binding = resolver
            .resolve(&WorkspaceReference {
                working_directory: cwd.clone(),
                repository: repository.clone(),
            })
            .unwrap();
        Self {
            root,
            access: Arc::new(LocalSessionAccess {
                binding,
                admission,
                cwd,
                repository,
                selected_store: Mutex::new(None),
            }),
        }
    }
    pub(in crate::local_sessions) fn access(&self) -> Arc<LocalSessionAccess> {
        self.access.clone()
    }
    pub(in crate::local_sessions) fn host(&self) -> LocalSessionHost {
        LocalSessionHost {
            access: self.access.clone(),
            goal_runtime: super::super::ObservedGoalRuntime::new(self.access.clone()),
        }
    }
    pub(in crate::local_sessions) fn config(&self) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(&self.access.admission).unwrap()).unwrap()
    }
    pub(in crate::local_sessions) fn write_config(&self, value: &serde_json::Value) {
        std::fs::write(&self.access.admission, serde_json::to_vec(value).unwrap()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
type ObserveHook =
    dyn FnOnce(&SessionCheckpoint, &mut SessionPhaseResources) -> Result<(), SessionError>;
struct NativeHost {
    local: LocalSessionHost,
    observe: RefCell<Option<Box<ObserveHook>>>,
}
impl InteractionAuthorityPort for NativeHost {
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
impl StructuredSessionHost for NativeHost {
    fn now_ms(&self) -> Result<u64, SessionError> {
        self.local.now_ms()
    }
    fn register(
        &self,
        o: &OwnerBinding,
        i: Intent,
        e: RequestedExecution,
    ) -> Result<(SupportedArtifactGoal, SessionBudget), SessionError> {
        self.local.register(o, i, e)
    }
    fn register_goal(
        &self,
        o: &OwnerBinding,
        i: Intent,
        e: RequestedExecution,
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
        self.local.register_goal(o, i, e)
    }
    fn prepare(&self, c: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        self.local.prepare(c)
    }
    fn disclose(&self, o: &OwnerBinding, s: &SessionId) -> Result<(), SessionError> {
        self.local.disclose(o, s)
    }
    fn issuer(&self, o: &OwnerBinding, p: &PrincipalId) -> Result<(), SessionError> {
        self.local.issuer(o, p)
    }
    fn goal_observation_runtime(&self) -> Option<&dyn ReadonlyGoalObservationRuntimePort> {
        Some(self)
    }
}
impl ReadonlyGoalObservationRuntimePort for NativeHost {
    fn resource_limits(
        &self,
        c: &SessionCheckpoint,
    ) -> Result<SessionResourceLimits, SessionError> {
        self.local.goal_runtime.resource_limits(c)
    }
    fn preparation_budget(
        &self,
        c: &SessionCheckpoint,
    ) -> Result<SessionResourceAmounts, SessionError> {
        self.local.goal_runtime.preparation_budget(c)
    }
    fn observe_goal(
        &self,
        c: &SessionCheckpoint,
        r: &mut SessionPhaseResources,
    ) -> Result<Option<PreExecutionGoalCandidate>, SessionError> {
        if let Some(hook) = self.observe.borrow_mut().take() {
            hook(c, r)?;
        }
        self.local.goal_runtime.observe_goal(c, r)
    }
    fn verify_retained_goal(
        &self,
        _: &SessionCheckpoint,
        _: &PreExecutionGoalData,
    ) -> Result<Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>, SessionError>
    {
        // A real marker is committed. This controlled outage stops completion;
        // the production verifier below still reads and verifies actual PG data.
        Err(SessionError::StorageUnavailable)
    }
}
fn request(source: &GoalSource, inventory: &ConnectorInventory) -> ConnectorRequest {
    let route = &source.defs.normalizations[0];
    ConnectorRequest {
        target: source.target(),
        capability: route.read.capability.clone(),
        class: ConnectorOperationClass::Read,
        inventory_revision: inventory.revision.clone(),
        arguments: route.read.arguments.clone(),
        task: source.defs.task.data().id.clone(),
        step: source.defs.native.step.clone(),
        execution: source.execution().unwrap(),
        correlation: NonEmptyText::new("boundary-native-read").unwrap(),
    }
}
fn wire_calls(root: &std::path::Path) -> usize {
    std::fs::read_to_string(root.join("wire.jsonl"))
        .unwrap()
        .lines()
        .filter(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["method"] == "tools/call")
        .count()
}

#[test]
fn genuine_native_claim_and_source_reject_foreign_current_bindings_before_reads() {
    if std::env::var_os("CG_COGNITIVE_TEST_DATABASE").is_none() {
        return;
    }
    let fixture = Fixture::new();
    // This component profile allows 20s per RPC, retaining the coordinator's
    // original 30s claim and absolute TTL. Installed entry tests retain 10s.
    let mut profile = fixture.config();
    profile["mappings"][0]["sessions"]["observed_goal_runtime"]["limits"]["call_timeout_ms"] =
        serde_json::json!(20_000);
    fixture.write_config(&profile);
    let access = fixture.access();
    let root = fixture.root.clone();
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let completed = observed.clone();
    INVENTORY_HOOK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move |source, _run, inventory| {
            assert_eq!(wire_calls(&root), 0, "discovery is not a goal read");
            let valid = request(source, inventory);
            source.route(&valid, None)?;
            for field in ["target", "task", "step", "execution", "arguments", "source"] {
                let mut changed = valid.clone();
                match field {
                    "target" => changed.target.connector = NonEmptyText::new("foreign").unwrap(),
                    "task" => changed.task = TaskId::new("foreign").unwrap(),
                    "step" => changed.step = PlanStepId::new("foreign").unwrap(),
                    "execution" => changed.execution = NonEmptyText::new("foreign").unwrap(),
                    "arguments" => {
                        changed.arguments.clear();
                    }
                    _ => {}
                }
                let s = (field == "source").then(|| SourceId::new("foreign").unwrap());
                assert_eq!(
                    source.route(&changed, s.as_ref()).err(),
                    Some(SessionError::ScopeDenied),
                    "{field}"
                );
                assert_eq!(
                    source.validate_connector_allocation(
                        source.view.allocation(),
                        &changed,
                        s.as_ref()
                    ),
                    Err(SessionError::ScopeDenied)
                );
            }
            let empty = GoalSource::new(
                source.claim.clone(),
                source.defs.clone(),
                source.checkpoint.clone(),
                source.view.clone(),
            );
            assert_eq!(
                empty.route(&valid, None).err(),
                Some(SessionError::InvalidState)
            );
            let foreign = ConnectorTarget {
                connector: NonEmptyText::new("foreign").unwrap(),
                scope: source.target().scope,
            };
            assert_eq!(
                source.validate_connector_discovery(source.view.allocation(), &foreign),
                Err(SessionError::ScopeDenied)
            );
            assert_eq!(
                source.admit_target(&foreign).unwrap_err().code,
                ConnectorFailureCode::ScopeMismatch
            );
            let failure = PolicySource(source.clone())
                .authorize(
                    &ConnectorRequest {
                        task: TaskId::new("foreign").unwrap(),
                        ..valid.clone()
                    },
                    None,
                )
                .err()
                .expect("foreign task must be denied");
            assert_eq!(failure.code, ConnectorFailureCode::ScopeMismatch);
            assert_eq!(failure.effect, ConnectorEffect::NotDispatched);
            let response = ScalarResponseAdmission::validate(
                source.as_ref(),
                &valid,
                Some(&SourceId::new("foreign").unwrap()),
                &ProtocolResult::Tool {
                    content: vec![],
                    structured: None,
                    is_error: false,
                },
            )
            .unwrap_err();
            assert_eq!(response.effect, ConnectorEffect::NoMutation);
            assert_eq!(
                wire_calls(&root),
                0,
                "all substitutions denied before tool read"
            );
            completed.store(true, std::sync::atomic::Ordering::SeqCst);
            Err(SessionError::UnsupportedCapability)
        }))
    });
    let host = NativeHost {
        local: fixture.host(),
        observe: RefCell::new(Some(Box::new(move |cp, _| {
            let mut foreign = cp.clone();
            foreign.owner.principal = PrincipalId::new("foreign").unwrap();
            assert_eq!(
                GoalClaim::capture(access.clone(), &foreign).err(),
                Some(SessionError::ScopeDenied)
            );
            assert_eq!(
                super::super::metadata(&access, &foreign).err(),
                Some(SessionError::ScopeDenied)
            );
            let mut expired = cp.clone();
            expired.budget.deadline_ms = 1;
            assert_eq!(
                super::super::metadata(&access, &expired).err(),
                Some(SessionError::ExpiredInteraction)
            );
            let mut stale = cp.clone();
            stale.snapshot.revision = stale.snapshot.revision.next()?;
            assert_eq!(
                GoalClaim::capture(access.clone(), &stale).err(),
                Some(SessionError::StaleRevision)
            );
            let claim = GoalClaim::capture(access.clone(), cp)?;
            let foreign_amounts = SessionResourceAmounts {
                connector_calls: 1,
                ..Default::default()
            };
            // An actual second coordinator owns this child. Even a live,
            // otherwise valid allowance cannot cross the durable PG claim.
            allocation_harness::with_preparation_resources(
                ContextScopeId::new("foreign-coordinator-project").unwrap(),
                ReferenceId::new("credits").unwrap(),
                foreign_amounts,
                |_, resources| {
                    let foreign = resources.allocate(foreign_amounts)?;
                    assert_eq!(
                        claim.validate_allocation(&foreign),
                        Err(SessionError::AuthorityDenied)
                    );
                    Ok(())
                },
            );
            let host = access.current()?;
            let store = &host.sessions.as_ref().unwrap().store_file;
            let original_store = std::fs::read(store).unwrap();
            std::fs::write(store, b" ").unwrap();
            let empty_store = GoalClaim::capture(access.clone(), cp).err();
            std::fs::write(store, &original_store).unwrap();
            assert_eq!(empty_store, Some(SessionError::Unavailable));
            let changed_store = format!(
                "{} application_name=changed-selector",
                String::from_utf8(original_store.clone()).unwrap().trim()
            );
            std::fs::write(store, changed_store).unwrap();
            let rotated_store = claim.current().err();
            std::fs::write(store, &original_store).unwrap();
            assert_eq!(rotated_store, Some(SessionError::StaleRevision));
            let original_admission = std::fs::read(&access.admission).unwrap();
            let mut admission: serde_json::Value =
                serde_json::from_slice(&original_admission).unwrap();
            admission["mappings"][0]["sessions"]["store_file"] =
                serde_json::json!("relative-store");
            std::fs::write(&access.admission, serde_json::to_vec(&admission).unwrap()).unwrap();
            let relative_store = GoalClaim::capture(access.clone(), cp).err();
            std::fs::write(&access.admission, &original_admission).unwrap();
            assert_eq!(relative_store, Some(SessionError::InvalidInput));
            admission = serde_json::from_slice(&original_admission).unwrap();
            admission["mappings"][0]["sessions"]["resource_limits"]["amounts"]["connector_calls"] =
                serde_json::json!(500);
            std::fs::write(&access.admission, serde_json::to_vec(&admission).unwrap()).unwrap();
            let changed_limits = claim.current().err();
            std::fs::write(&access.admission, &original_admission).unwrap();
            assert_eq!(changed_limits, Some(SessionError::StaleRevision));
            claim.current()?;
            let mut wrong_limits = cp.clone();
            let limits = wrong_limits.resource_ledger.as_ref().unwrap().limits();
            let mut changed = limits.clone();
            changed.amounts.connector_calls += 1;
            wrong_limits.resource_ledger = Some(SessionResourceLedger::new(changed)?);
            assert_eq!(
                super::super::metadata(&access, &wrong_limits).err(),
                Some(SessionError::StaleRevision)
            );
            Ok(())
        }))),
    };
    let owner = host.local.owner().unwrap();
    let current = host.local.current().unwrap();
    let config = current.sessions.as_ref().unwrap();
    let intent = config.intent.clone();
    let execution = config.execution;
    let repository = host.local.access.selected_repository(&current).unwrap();
    let coordinator = SessionCoordinator::new(repository.clone(), host);
    let start = coordinator
        .execute(
            &owner,
            SessionCommand::Start {
                command: CommandId::new("boundary-start").unwrap(),
                intent,
                execution,
            },
        )
        .unwrap();
    let runnable = repository.load(&owner, &start.session).unwrap().checkpoint;
    assert_eq!(
        GoalClaim::capture(fixture.access.clone(), &runnable).err(),
        Some(SessionError::InvalidState)
    );
    let started_at = std::time::Instant::now();
    let started_wall = now_ms().unwrap();
    let result = coordinator.execute(
        &owner,
        SessionCommand::Continue {
            at: Mutation {
                session: start.session.clone(),
                command: CommandId::new("boundary-observe").unwrap(),
                expected_revision: start.revision,
            },
        },
    );
    let methods = std::fs::read_to_string(fixture.root.join("wire.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["method"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        result,
        Err(SessionError::UnsupportedCapability),
        "elapsed={:?}, hook={}, wire_methods={methods:?}, started_wall={started_wall}, replies={}",
        started_at.elapsed(),
        observed.load(std::sync::atomic::Ordering::SeqCst),
        std::fs::read_to_string(fixture.root.join("response-times.jsonl")).unwrap_or_default()
    );
    assert!(
        observed.load(std::sync::atomic::Ordering::SeqCst),
        "actual source hook must finish"
    );
    let cp = repository.load(&owner, &start.session).unwrap().checkpoint;
    assert_eq!(cp.snapshot.state, SessionState::Dispatching);
    assert!(cp.pre_execution_goal.is_none());
    assert_eq!(wire_calls(&fixture.root), 0);
    let stopped = coordinator
        .execute(
            &owner,
            SessionCommand::Cancel {
                at: Mutation {
                    session: start.session,
                    command: CommandId::new("boundary-cancel").unwrap(),
                    expected_revision: cp.snapshot.revision,
                },
            },
        )
        .unwrap();
    assert_eq!(stopped.state, SessionState::Cancelled);
}

#[test]
fn genuine_retained_verifier_reloads_owner_pg_records_and_rejects_supplied_or_current_drift() {
    if std::env::var_os("CG_COGNITIVE_TEST_DATABASE").is_none() {
        return;
    }
    // One atomic source export supplies BOTH unchanged original conditions.
    // Multi-object routing is qualified separately, including installed tests.
    let fixture = Fixture::single_source();
    // Separate genuine claim: negative source exercise never consumes its time.
    let mut profile = fixture.config();
    profile["mappings"][0]["sessions"]["observed_goal_runtime"]["limits"]["call_timeout_ms"] =
        serde_json::json!(20_000);
    fixture.write_config(&profile);
    let original_config = fixture.config();
    let host = NativeHost {
        local: fixture.host(),
        observe: RefCell::new(None),
    };
    let owner = host.local.owner().unwrap();
    let current = host.local.current().unwrap();
    let config = current.sessions.as_ref().unwrap();
    let intent = config.intent.clone();
    let execution = config.execution;
    let repository = host.local.access.selected_repository(&current).unwrap();
    let coordinator = SessionCoordinator::new(repository.clone(), host);
    let start = coordinator
        .execute(
            &owner,
            SessionCommand::Start {
                command: CommandId::new("retained-boundary-start").unwrap(),
                intent,
                execution,
            },
        )
        .unwrap();
    let result = coordinator.execute(
        &owner,
        SessionCommand::Continue {
            at: Mutation {
                session: start.session.clone(),
                command: CommandId::new("retained-boundary-observe").unwrap(),
                expected_revision: start.revision,
            },
        },
    );
    assert_eq!(result, Err(SessionError::StorageUnavailable));
    let cp = repository.load(&owner, &start.session).unwrap().checkpoint;
    assert_eq!(cp.snapshot.state, SessionState::Runnable);
    let data = load_pre_execution_goal(&repository, &cp, now_ms().unwrap()).unwrap();
    assert_eq!(cp.goal.intent().desired_state().conditions().len(), 2);
    assert_eq!(data.reads().len(), 2);
    assert!(
        super::super::verify::retained(&fixture.access, &cp, &data)
            .unwrap()
            .is_some()
    );
    let (_, defs) = super::super::metadata(&fixture.access, &cp).unwrap();
    let requests = data
        .reads()
        .iter()
        .map(|read| {
            let declaration = defs
                .runtime
                .goal_reads
                .iter()
                .find(|d| d.condition == read.condition.as_str())
                .unwrap();
            ConnectorRequest {
                target: ConnectorTarget {
                    connector: defs.runtime.connector.clone(),
                    scope: defs.runtime.scope.clone(),
                },
                capability: declaration.read.capability.clone(),
                class: ConnectorOperationClass::Read,
                inventory_revision: read.observation.inventory_revision.clone(),
                arguments: declaration.read.arguments.clone(),
                task: defs.task.data().id.clone(),
                step: defs.native.step.clone(),
                execution: data.binding().execution.clone(),
                correlation: NonEmptyText::new(&read.read_request.correlation).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    let inputs = data
        .reads()
        .iter()
        .zip(&requests)
        .map(|(read, request)| PreExecutionGoalReadInput {
            condition: &read.condition,
            read_request: request,
            observation: &read.observation,
            batch: &read.batch,
            max_age_seconds: read.max_age_seconds,
        })
        .collect::<Vec<_>>();
    let mut foreign_binding = data.binding().clone();
    foreign_binding.context_digest = context_content_digest(b"untrusted-supplied-context");
    let supplied = PreExecutionGoalData::new(
        cp.goal.as_connector().unwrap(),
        foreign_binding,
        data.definition_pins().to_vec(),
        &inputs,
        now_ms().unwrap(),
    )
    .unwrap();
    assert_eq!(
        super::super::verify::retained(&fixture.access, &cp, &supplied).err(),
        Some(SessionError::InvalidInteraction)
    );
    let mut stale = cp.clone();
    stale.snapshot.revision = stale.snapshot.revision.next().unwrap();
    assert_eq!(
        super::super::verify::retained(&fixture.access, &stale, &data).err(),
        Some(SessionError::StaleRevision)
    );
    let mut changed = original_config.clone();
    changed["mappings"][0]["sessions"]["consent_required"] = serde_json::json!(true);
    fixture.write_config(&changed);
    assert_eq!(
        super::super::verify::retained(&fixture.access, &cp, &data).err(),
        Some(SessionError::StaleRevision)
    );
    fixture.write_config(&original_config);
    assert_eq!(
        wire_calls(&fixture.root),
        1,
        "replay and denial checks never reread sources"
    );
}

#[test]
fn source_failures_keep_content_free_codes_and_actual_effect_stage() {
    let correlation = NonEmptyText::new("actual-correlation").unwrap();
    for (error, code) in [
        (
            SessionError::ScopeDenied,
            ConnectorFailureCode::ScopeMismatch,
        ),
        (
            SessionError::LimitExceeded,
            ConnectorFailureCode::BudgetExhausted,
        ),
        (
            SessionError::StorageUnavailable,
            ConnectorFailureCode::Unavailable,
        ),
        (SessionError::Unavailable, ConnectorFailureCode::Unavailable),
        (
            SessionError::OutcomeUnknown,
            ConnectorFailureCode::MutationUncertain,
        ),
        (
            SessionError::AuthorityDenied,
            ConnectorFailureCode::PermissionDenied,
        ),
    ] {
        let early = failure(error, &correlation);
        assert_eq!(early.code, code);
        assert_eq!(early.effect, ConnectorEffect::NotDispatched);
        assert_eq!(early.correlation, correlation);
        let late = response_failure(error, &correlation);
        assert_eq!(late.code, code);
        assert_eq!(late.effect, ConnectorEffect::NoMutation);
        assert_eq!(late.correlation, correlation);
    }
}
