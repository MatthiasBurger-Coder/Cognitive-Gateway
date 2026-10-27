//! #407 component capture through real native compilation and coordinator
//! allocations; PostgreSQL and cg-local are real. No MCP #408 is simulated.
use super::*;
use crate::{complete_snapshot_fixture as snapshot, prepared_before_fixture};
use gateway_application::sessions::scoped_snapshots::*;
use gateway_domain::*;
use std::{cell::Cell, process::Command};

struct BeforeHost {
    local: LocalSessionHost,
    now: Cell<u64>,
    captures: Cell<usize>,
    kind: SnapshotProfileKind,
    canary: bool,
    foreign_runtime: Cell<bool>,
}
impl InteractionAuthorityPort for BeforeHost {
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
impl StructuredSessionHost for BeforeHost {
    fn now_ms(&self) -> Result<u64, SessionError> {
        Ok(self.now.get())
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
impl ReadonlyGoalObservationRuntimePort for BeforeHost {
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
    fn prepare_readonly(
        &self,
        cp: &SessionCheckpoint,
        phase: &mut SessionPhaseResources,
    ) -> Result<Option<ReadonlyPreparation>, SessionError> {
        self.captures.set(self.captures.get() + 1);
        let current = self.local.current()?;
        let config = current.sessions.as_ref().ok_or(SessionError::Unavailable)?;
        let runtime = config
            .observed_goal_runtime
            .as_ref()
            .ok_or(SessionError::Unavailable)?;
        let native = runtime.capture(&current, cp.goal.as_connector().unwrap(), cp.execution)?;
        let allocation = phase.allocate(*phase.remaining())?;
        let mut fixture = if self.kind == SnapshotProfileKind::AtomicCompleteExport {
            snapshot::Fixture::atomic_in_scope(allocation.binding().scope.clone())
        } else {
            snapshot::Fixture::in_scope(self.kind, allocation.binding().scope.clone())
        };
        // Persist reordered members and an explicit shorter freshness contract.
        // Their content-owner digests must accompany the genuine native DATA.
        fixture.universe.objects.reverse();
        fixture.repin_universe();
        fixture.profile.max_age_seconds = 45;
        fixture.repin_profile();
        if self.canary {
            // Valid direct native DATA, not a malformed envelope. The historical
            // disclosure guard must deny the whole response without rewriting it.
            let old = &fixture.acquisitions[0];
            let native = snapshot::native_read_in_scope(
                allocation.binding().scope.clone(),
                old.request.correlation.as_str(),
                old.observation.metadata.object_reference.as_str(),
                old.observation.source_revision.as_ref().unwrap().as_str(),
                old.observation.acquired_at.seconds(),
                old.request.arguments.clone(),
                old.observation
                    .records
                    .observations()
                    .iter()
                    .map(|o| {
                        (
                            o.subject().clone(),
                            if o.subject().to_string() == "state.one.bytes" {
                                TypedValue::String(
                                    NonEmptyText::new("Bearer SNAPSHOT_CANARY_407_SECRET").unwrap(),
                                )
                            } else {
                                o.value().clone()
                            },
                        )
                    })
                    .collect(),
            );
            fixture.replace_acquisition(
                0,
                SnapshotAcquisition {
                    request: native.request,
                    observation: native.observation,
                    batch: native.batch,
                },
            );
        }
        let runtime_pin = if self.foreign_runtime.get() {
            snapshot::pin(
                "foreign-native-runtime",
                "cg.connector-observed-goal-runtime",
                &serde_json::from_str::<Value>(&native.compiled.to_json().unwrap()).unwrap(),
            )
        } else {
            runtime.reference()?
        };
        let captured = prepared_before_fixture::capture(
            &mut fixture,
            &allocation,
            &native.compiled,
            runtime_pin,
            self.now.get(),
        );
        Ok(Some(ReadonlyPreparation::BeforeCaptured(Box::new(
            captured,
        ))))
    }
}
fn enabled() -> bool {
    std::env::var_os("CG_COGNITIVE_TEST_DATABASE").is_some()
}
fn clock() -> u64 {
    100_000
} // Explicit component clock; historical inspection uses real host NOW.
fn setup(
    kind: SnapshotProfileKind,
    canary: bool,
) -> (
    observed_goal::NativeBoundaryFixture,
    SessionCoordinator<PostgresSessionStore, BeforeHost>,
    OwnerBinding,
    SessionSnapshot,
) {
    let fixture = observed_goal::NativeBoundaryFixture::new();
    let local = fixture.host();
    let owner = local.owner().unwrap();
    let current = local.current().unwrap();
    let config = current.sessions.as_ref().unwrap();
    let intent = config.intent.clone();
    let execution = config.execution;
    let repo = local.access.selected_repository(&current).unwrap();
    let coordinator = SessionCoordinator::new(
        repo,
        BeforeHost {
            local,
            now: Cell::new(clock()),
            captures: Cell::new(0),
            kind,
            canary,
            foreign_runtime: Cell::new(false),
        },
    );
    let start = coordinator
        .execute(
            &owner,
            SessionCommand::Start {
                command: CommandId::new("before-start").unwrap(),
                intent,
                execution,
            },
        )
        .unwrap();
    (fixture, coordinator, owner, start)
}
fn capture(
    c: &SessionCoordinator<PostgresSessionStore, BeforeHost>,
    owner: &OwnerBinding,
    start: &SessionSnapshot,
) -> SessionCheckpoint {
    c.execute(
        owner,
        SessionCommand::Continue {
            at: Mutation {
                session: start.session.clone(),
                command: CommandId::new("before-capture").unwrap(),
                expected_revision: start.revision,
            },
        },
    )
    .unwrap();
    c.repository.load(owner, &start.session).unwrap().checkpoint
}
fn inspect(
    f: &observed_goal::NativeBoundaryFixture,
    session: &SessionId,
    issuer: &str,
) -> (bool, Value) {
    let access = f.access();
    let path = std::path::Path::new(&access.admission)
        .parent()
        .unwrap()
        .join("inspect-before-request.json");
    std::fs::write(
        &path,
        serde_json::to_vec(
            &json!({"session_id":session,"issuer":issuer,"decision":"inspect-before"}),
        )
        .unwrap(),
    )
    .unwrap();
    let bin = std::env::var_os("CG_QUALIFICATION_BIN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_owned()
        })
        .join("cg-local");
    let mut cmd = Command::new(bin);
    cmd.env_clear().env("PATH", "/usr/bin:/bin");
    // Retain only the coverage runtime's output locator when instrumented.
    // Provider credentials and the rest of the launch environment stay absent.
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        cmd.env("LLVM_PROFILE_FILE", profile);
    }
    for (flag, value) in [
        ("--client-name", "codex".to_owned()),
        ("--client-version", "1.0".to_owned()),
        ("--principal", access.binding.session.principal.to_string()),
        (
            "--workspace",
            access.binding.scope["workspace_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        ),
        (
            "--project",
            access.binding.scope["project_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        ),
        (
            "--binding",
            access.binding.scope["binding_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        ),
        ("--admission", access.admission.clone()),
        ("--cwd", access.cwd.clone()),
        ("--repository", access.repository.clone()),
        ("--session", access.binding.session.session_id.to_string()),
        ("--operation", "session.authority".to_owned()),
        ("--request", path.to_str().unwrap().to_owned()),
    ] {
        cmd.arg(flag).arg(value);
    }
    let out = cmd.output().unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains("SNAPSHOT_CANARY_407_SECRET"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("SNAPSHOT_CANARY_407_SECRET"));
    (
        out.status.success(),
        serde_json::from_slice(&out.stdout).expect("fixed JSON operator response"),
    )
}

#[test]
fn actual_pg_preserves_all_three_native_profiles_and_historical_lifecycle() {
    if !enabled() {
        return;
    }
    for kind in [
        SnapshotProfileKind::AtomicCompleteExport,
        SnapshotProfileKind::SnapshotScopedReads,
        SnapshotProfileKind::BracketedNonReusedAnchor,
    ] {
        let (_fixture, c, owner, start) = setup(kind, false);
        let cp = capture(&c, &owner, &start);
        assert_eq!(cp.snapshot.state, SessionState::Runnable);
        assert_eq!(cp.snapshot.dispatch_knowledge, DispatchKnowledge::Prepared);
        assert!(cp.pre_execution_goal.is_none());
        assert!(cp.snapshot.final_evidence.is_none());
        let before = cp.prepared_before.as_ref().unwrap();
        let default_scope = match kind {
            SnapshotProfileKind::AtomicCompleteExport => snapshot::Fixture::atomic(),
            SnapshotProfileKind::SnapshotScopedReads => snapshot::Fixture::versioned(),
            SnapshotProfileKind::BracketedNonReusedAnchor => snapshot::Fixture::bracketed(),
        }
        .universe
        .pin
        .scope;
        assert_ne!(
            before.binding.capture.scope, default_scope,
            "actual native project scope must replace the shared sample scope"
        );
        assert_eq!(
            before.binding.capture.revision.next().unwrap(),
            before.committed_revision
        );
        assert_eq!(before.committed_revision, cp.snapshot.revision);
        assert_eq!(before.validated_at_ms, clock());
        let record = c.repository.load_artifact(&owner, &before.record).unwrap();
        assert!(!record.is_empty());
        let initial = load_prepared_before_current(&c.repository, &cp, clock()).unwrap();
        assert_eq!(initial.profile().kind, kind);
        assert!(load_prepared_before_current(&c.repository, &cp, clock() + 1_000_000).is_err());
        assert_eq!(
            load_prepared_before_historical(&c.repository, &cp)
                .unwrap()
                .encode()
                .unwrap(),
            initial.encode().unwrap()
        );
        c.host.now.set(clock() + 1_000_000);
        assert_eq!(c.recover(&owner, &start.session).unwrap(), cp.snapshot);
        assert_eq!(c.host.captures.get(), 1);
        let duplicate = c.execute(
            &owner,
            SessionCommand::Continue {
                at: Mutation {
                    session: start.session.clone(),
                    command: CommandId::new("before-capture").unwrap(),
                    expected_revision: start.revision,
                },
            },
        );
        assert_eq!(duplicate, Err(SessionError::Duplicate));
        assert_eq!(
            c.execute(
                &owner,
                SessionCommand::Continue {
                    at: Mutation {
                        session: start.session.clone(),
                        command: CommandId::new("before-model-uninstalled").unwrap(),
                        expected_revision: cp.snapshot.revision
                    }
                }
            ),
            Err(SessionError::UnsupportedCapability)
        );
        assert_eq!(
            c.repository
                .load(&owner, &start.session)
                .unwrap()
                .checkpoint,
            cp
        );
        assert_eq!(c.host.captures.get(), 1);
        let cancelled = c
            .execute(
                &owner,
                SessionCommand::Cancel {
                    at: Mutation {
                        session: start.session.clone(),
                        command: CommandId::new("before-cancel").unwrap(),
                        expected_revision: cp.snapshot.revision,
                    },
                },
            )
            .unwrap();
        assert_eq!(cancelled.state, SessionState::Cancelled);
        let retained = c
            .repository
            .load(&owner, &start.session)
            .unwrap()
            .checkpoint;
        assert_eq!(retained.prepared_before, cp.prepared_before);
        assert_eq!(retained.resource_ledger, cp.resource_ledger);
        assert_eq!(
            load_prepared_before_historical(&c.repository, &retained)
                .unwrap()
                .encode()
                .unwrap(),
            initial.encode().unwrap()
        );
        assert_eq!(c.host.captures.get(), 1);
    }
}

#[test]
fn actual_operator_inspects_original_expired_data_without_issuing_or_mutating() {
    if !enabled() {
        return;
    }
    let (f, c, owner, start) = setup(SnapshotProfileKind::AtomicCompleteExport, false);
    let cp = capture(&c, &owner, &start);
    let issuer = f.config()["mappings"][0]["sessions"]["issuers"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let journal_before = journal_bytes(&f);
    let (ok, response) = inspect(&f, &start.session, &issuer);
    assert_eq!(journal_bytes(&f), journal_before);
    assert!(ok, "operator failed: {response}");
    assert_eq!(
        response["result"]["contract"],
        "cg.complete-before-inspection"
    );
    assert_eq!(response["result"]["contract_version"], "1.0");
    assert_eq!(response["result"]["historical"], true);
    assert_eq!(
        response["result"]["before"],
        serde_json::to_value(cp.prepared_before.as_ref().unwrap()).unwrap()
    );
    assert_eq!(
        response["result"]["snapshot"],
        serde_json::from_slice::<Value>(
            &load_prepared_before_historical(&c.repository, &cp)
                .unwrap()
                .encode()
                .unwrap()
        )
        .unwrap()
    );
    assert_eq!(
        c.repository
            .load(&owner, &start.session)
            .unwrap()
            .checkpoint,
        cp
    );
    // Historical disclosure uses the saved authenticated envelope; it must
    // not compile/replay a current provider runtime or require #408 acquisition.
    let registered = f.config();
    let mut historical_only = registered.clone();
    historical_only["mappings"][0]["sessions"]["observed_goal_runtime"] = Value::Null;
    f.write_config(&historical_only);
    let (still_readable, retained) = inspect(&f, &start.session, &issuer);
    f.write_config(&registered);
    assert!(
        still_readable,
        "historical runtime-free inspection: {retained}"
    );
    assert_eq!(retained, response);
    assert_eq!(journal_bytes(&f), journal_before);
    let (ok, response) = inspect(&f, &start.session, "foreign-issuer");
    assert!(!ok);
    assert_ne!(response["status"], "ok");
    assert_eq!(
        c.repository
            .load(&owner, &start.session)
            .unwrap()
            .checkpoint,
        cp
    );
}

#[test]
fn credential_like_native_data_is_retained_but_whole_operator_disclosure_is_denied() {
    if !enabled() {
        return;
    }
    let (f, c, owner, start) = setup(SnapshotProfileKind::AtomicCompleteExport, true);
    let cp = capture(&c, &owner, &start);
    let before = cp.prepared_before.as_ref().unwrap();
    let original = c.repository.load_artifact(&owner, &before.record).unwrap();
    assert!(String::from_utf8_lossy(&original).contains("SNAPSHOT_CANARY_407_SECRET"));
    let issuer = f.config()["mappings"][0]["sessions"]["issuers"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let journal_before = journal_bytes(&f);
    let (ok, response) = inspect(&f, &start.session, &issuer);
    assert_eq!(journal_bytes(&f), journal_before);
    assert!(!ok);
    assert_eq!(response["status"], "error");
    assert_eq!(
        c.repository
            .load(&owner, &start.session)
            .unwrap()
            .checkpoint,
        cp
    );
    assert_eq!(
        c.repository.load_artifact(&owner, &before.record).unwrap(),
        original
    );
}

fn journal_bytes(f: &observed_goal::NativeBoundaryFixture) -> Vec<u8> {
    let mut database = postgres::Client::connect(
        &std::env::var("CG_COGNITIVE_TEST_DATABASE").unwrap(),
        postgres::NoTls,
    )
    .unwrap();
    let scope = f.access().binding.canonical_scope.to_string();
    let row = database
        .query_one(
            "SELECT payload FROM cg_cognitive_journals WHERE scope=$1 AND kind='task-sessions-v2'",
            &[&scope],
        )
        .unwrap();
    let payload: String = row.get(0);
    payload.into_bytes()
}
fn replace_journal(f: &observed_goal::NativeBoundaryFixture, bytes: &[u8]) {
    use sha2::{Digest, Sha256};
    let mut database = postgres::Client::connect(
        &std::env::var("CG_COGNITIVE_TEST_DATABASE").unwrap(),
        postgres::NoTls,
    )
    .unwrap();
    let scope = f.access().binding.canonical_scope.to_string();
    let payload = std::str::from_utf8(bytes).unwrap();
    let digest = format!("{:x}", Sha256::digest(bytes));
    assert_eq!(database.execute("UPDATE cg_cognitive_journals SET payload=$2,digest=$3 WHERE scope=$1 AND kind='task-sessions-v2'",&[&scope,&payload,&digest]).unwrap(),1);
}

#[test]
fn actual_pg_refuses_orphan_kind_owner_and_payload_corruption_without_repairing_history() {
    if !enabled() {
        return;
    }
    let (f, c, owner, start) = setup(SnapshotProfileKind::AtomicCompleteExport, false);
    let cp = capture(&c, &owner, &start);
    let before = cp.prepared_before.as_ref().unwrap();
    assert_eq!(
        c.repository.record_kind(&owner, &before.record).unwrap(),
        RecordKind::CompleteBefore
    );
    let original = journal_bytes(&f);
    let body: Value = serde_json::from_slice(&original).unwrap();
    let index = body["records"]
        .as_array()
        .unwrap()
        .iter()
        .position(|r| r["kind"] == "complete_before")
        .unwrap();
    for variant in ["kind", "owner", "bytes", "orphan", "metadata"] {
        let mut damaged = body.clone();
        match variant {
            "kind" => damaged["records"][index]["kind"] = json!("artifact"),
            "owner" => damaged["records"][index]["owner"]["project"] = json!("foreign-project"),
            "bytes" => damaged["records"][index]["bytes"][0] = json!(0),
            "orphan" => damaged["sessions"][0]["checkpoint"]["prepared_before"] = Value::Null,
            "metadata" => {
                damaged["sessions"][0]["checkpoint"]["prepared_before"]["validated_at_ms"] =
                    json!(clock() + 1)
            }
            _ => unreachable!(),
        }
        // Rehash the outer journal: this exercises native linkage/content
        // authentication rather than merely its outer SHA256 checksum.
        replace_journal(&f, &serde_json::to_vec(&damaged).unwrap());
        let outcome = c.repository.load(&owner, &start.session);
        let unchanged = journal_bytes(&f);
        replace_journal(&f, &original);
        assert_eq!(
            outcome.err(),
            Some(SessionError::StorageUnavailable),
            "{variant}"
        );
        assert_eq!(unchanged, serde_json::to_vec(&damaged).unwrap());
        assert_eq!(
            c.repository
                .load(&owner, &start.session)
                .unwrap()
                .checkpoint,
            cp
        );
    }
    // A duplicate/orphan CompleteBefore cannot be appended to an otherwise
    // legitimate next revision, and cannot leave a partially changed checkpoint.
    let current = c.repository.load(&owner, &start.session).unwrap();
    let mut next = cp.clone();
    next.snapshot.revision = cp.snapshot.revision.next().unwrap();
    let transition = JournalAppend {
        session: start.session.clone(),
        expected_revision: cp.snapshot.revision,
        fence: current.fence,
        command: None,
        next,
        records: vec![RecordWrite {
            reference: before.record.clone(),
            kind: RecordKind::CompleteBefore,
            bytes: c.repository.load_artifact(&owner, &before.record).unwrap(),
        }],
    };
    assert!(c.repository.append(&owner, transition).is_err());
    assert_eq!(journal_bytes(&f), original);
    assert_eq!(c.repository.load(&owner, &start.session).unwrap(), current);
}

#[test]
fn postgres_commit_failure_cannot_publish_a_before_reference_or_orphan_record() {
    if !enabled() {
        return;
    }
    let (f, c, owner, start) = setup(SnapshotProfileKind::AtomicCompleteExport, false);
    let scope = f.access().binding.canonical_scope.to_string();
    let suffix = scope
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>();
    let function = format!("bf_{}", &suffix[suffix.len().saturating_sub(36)..]);
    let trigger = format!("bt_{}", &suffix[suffix.len().saturating_sub(36)..]);
    assert!(scope.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    let mut db = postgres::Client::connect(
        &std::env::var("CG_COGNITIVE_TEST_DATABASE").unwrap(),
        postgres::NoTls,
    )
    .unwrap();
    db.batch_execute(&format!("CREATE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.scope='{scope}' AND NEW.kind='task-sessions-v2' AND strpos(NEW.payload,'\"prepared_before\"')>0 THEN RAISE EXCEPTION 'controlled complete-before commit outage'; END IF; RETURN NEW; END $$; CREATE TRIGGER {trigger} BEFORE UPDATE ON cg_cognitive_journals FOR EACH ROW EXECUTE FUNCTION {function}();")).unwrap();
    let outcome = c.execute(
        &owner,
        SessionCommand::Continue {
            at: Mutation {
                session: start.session.clone(),
                command: CommandId::new("before-failed-commit").unwrap(),
                expected_revision: start.revision,
            },
        },
    );
    db.batch_execute(&format!(
        "DROP TRIGGER {trigger} ON cg_cognitive_journals; DROP FUNCTION {function}();"
    ))
    .unwrap();
    assert_eq!(outcome, Err(SessionError::StorageUnavailable));
    let predecessor = c
        .repository
        .load(&owner, &start.session)
        .unwrap()
        .checkpoint;
    assert_eq!(predecessor.snapshot.state, SessionState::Dispatching);
    assert!(predecessor.prepared_before.is_none());
    assert_eq!(c.host.captures.get(), 1);
    let journal: Value = serde_json::from_slice(&journal_bytes(&f)).unwrap();
    assert!(
        !journal["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["kind"] == "complete_before")
    );
}

#[test]
fn genuine_coordinator_refuses_a_foreign_runtime_pin_before_postgres_publication() {
    if !enabled() {
        return;
    }
    let (f, c, owner, start) = setup(SnapshotProfileKind::AtomicCompleteExport, false);
    c.host.foreign_runtime.set(true);
    assert_eq!(
        c.execute(
            &owner,
            SessionCommand::Continue {
                at: Mutation {
                    session: start.session.clone(),
                    command: CommandId::new("foreign-runtime").unwrap(),
                    expected_revision: start.revision
                }
            }
        ),
        Err(SessionError::InvalidInteraction)
    );
    let cp = c
        .repository
        .load(&owner, &start.session)
        .unwrap()
        .checkpoint;
    assert!(cp.prepared_before.is_none());
    assert_eq!(c.host.captures.get(), 1);
    let journal: Value = serde_json::from_slice(&journal_bytes(&f)).unwrap();
    assert!(
        !journal["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["kind"] == "complete_before")
    );
}

#[test]
fn default_project_native_records_do_not_authenticate_another_capture_scope() {
    let mut actual =
        snapshot::Fixture::atomic_in_scope(ContextScopeId::new("another-native-project").unwrap());
    let old = &actual.acquisitions[0];
    let foreign = snapshot::native_read(
        old.request.correlation.as_str(),
        old.observation.metadata.object_reference.as_str(),
        old.observation.source_revision.as_ref().unwrap().as_str(),
        old.observation.acquired_at.seconds(),
        old.request.arguments.clone(),
        old.observation
            .records
            .observations()
            .iter()
            .map(|o| (o.subject().clone(), o.value().clone()))
            .collect(),
    );
    actual.replace_acquisition(
        0,
        SnapshotAcquisition {
            request: foreign.request,
            observation: foreign.observation,
            batch: foreign.batch,
        },
    );
    assert!(
        actual.validate().is_err(),
        "coherent native DATA from another project cannot replace the actual scoped acquisition"
    );
}
