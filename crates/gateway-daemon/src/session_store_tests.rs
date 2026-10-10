//! PostgreSQL adapter failure/rollback tests, run by the disposable-host gate.
use super::*;
use gateway_domain::{ContextScopeId, ExecutionProfile, OperatingMode, PlanStepId};
use std::time::{SystemTime, UNIX_EPOCH};
fn owner() -> OwnerBinding {
    OwnerBinding {
        principal: PrincipalId::new("operator").unwrap(),
        workspace: WorkspaceId::new("w").unwrap(),
        project: ProjectId::new("p").unwrap(),
        binding: BindingId::new("b").unwrap(),
        client_owner: ClientOwnerId::new("owner").unwrap(),
    }
}
fn reference(id: &str) -> RecordRef {
    content_reference(id, id.as_bytes()).unwrap()
}
fn checkpoint(repo: &PostgresSessionStore, id: &str) -> SessionCheckpoint {
    let mut intent: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/declarative-cli/intent.json"
    ))
    .unwrap();
    intent["desired_state"]["conditions"][0]["id"] = serde_json::json!("context-verified");
    intent["desired_state"]["conditions"][0]["subject"] =
        serde_json::json!("cg.context.projection.verified");
    intent["desired_state"]["expression"]["value"] = serde_json::json!("context-verified");
    let goal = SupportedArtifactGoal::validate(
        serde_json::from_value(intent).unwrap(),
        ArtifactGoalBasis {
            scope: repo.store.scope().clone(),
            plan: reference("plan"),
            step: PlanStepId::new("step").unwrap(),
            projection: reference("projection"),
            sources: vec![],
        },
    )
    .unwrap();
    let session = SessionId::new(id).unwrap();
    let initial_assessment = InitialAssessment {
        goal: content_reference(
            "supported-goal",
            &serde_json::to_vec(goal.intent()).unwrap(),
        )
        .unwrap(),
        authority: reference("authority"),
        outcome: "UNKNOWN".into(),
    };
    SessionCheckpoint {
        owner: owner(),
        goal,
        execution: RequestedExecution {
            mode: OperatingMode::Development,
            profile: ExecutionProfile::FullPath,
        },
        snapshot: SessionSnapshot {
            session: session.clone(),
            run: RunId::new(format!("run-{id}")).unwrap(),
            revision: Revision::new(1).unwrap(),
            state: SessionState::Runnable,
            pending: None,
            dispatch: None,
            dispatch_knowledge: DispatchKnowledge::None,
            command_outcome: Some(CommandOutcome {
                command: CommandId::new(id).unwrap(),
                session,
                revision: Revision::new(1).unwrap(),
            }),
            final_evidence: None,
        },
        budget: SessionBudget {
            actions: 0,
            retries: 0,
            max_actions: 3,
            max_retries: 1,
            deadline_ms: MAX_REVISION,
        },
        pending: None,
        accepted_consent: None,
        authority_events: vec![],
        selected_source: None,
        artifact: None,
        lease_until_ms: None,
        initial_assessment: Some(initial_assessment),
    }
}
fn repository() -> Option<PostgresSessionStore> {
    let connection = std::env::var("CG_COGNITIVE_TEST_DATABASE").ok()?;
    let scope = ContextScopeId::new(format!(
        "session-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
    .unwrap();
    Some(PostgresSessionStore::new(
        CognitiveStore::connect(&connection, scope).unwrap(),
    ))
}
fn append(current: &FencedSession) -> JournalAppend {
    let mut next = current.checkpoint.clone();
    next.snapshot.revision = next.snapshot.revision.next().unwrap();
    JournalAppend {
        session: next.snapshot.session.clone(),
        expected_revision: current.checkpoint.snapshot.revision,
        fence: current.fence,
        command: None,
        next,
        records: vec![],
    }
}
fn pause(current: &FencedSession) -> JournalAppend {
    let mut transition = append(current);
    let cp = &mut transition.next;
    let pending = PendingRef {
        id: PendingId::new(format!("pending-{}", cp.snapshot.revision.value())).unwrap(),
        issued_revision: cp.snapshot.revision,
        expires_at_ms: MAX_REVISION,
    };
    let binding = ActionBinding {
        owner: cp.owner.clone(),
        session: cp.snapshot.session.clone(),
        run: cp.snapshot.run.clone(),
        pending: pending.id.clone(),
        issued_revision: pending.issued_revision,
        dispatch: DispatchId::new("compile").unwrap(),
        step: RecordId::new("step").unwrap(),
        action: reference("action"),
        arguments: reference("arguments"),
        artifact_basis: reference("basis"),
        authority: reference("authority"),
        expires_at_ms: pending.expires_at_ms,
    };
    cp.snapshot.state = SessionState::PendingConsent;
    cp.snapshot.pending = Some(pending);
    cp.snapshot.dispatch = Some(binding.dispatch.clone());
    cp.snapshot.dispatch_knowledge = DispatchKnowledge::Prepared;
    cp.pending = Some(PendingInteraction::Consent(Box::new(binding)));
    transition
}

#[test]
fn postgres_atomic_append_rejects_invalid_records_and_keeps_predecessor() {
    let Some(repo) = repository() else {
        return;
    };
    let cp = checkpoint(&repo, "atomic");
    let mut foreign = cp.clone();
    let mut basis = foreign.goal.basis().clone();
    basis.scope = ContextScopeId::new("foreign").unwrap();
    foreign.goal = SupportedArtifactGoal::validate(foreign.goal.intent().clone(), basis).unwrap();
    assert_eq!(
        repo.create(&CommandId::new("foreign").unwrap(), foreign),
        Err(SessionError::ScopeDenied)
    );
    let initial = repo
        .create(&CommandId::new("atomic").unwrap(), cp.clone())
        .unwrap();
    assert_eq!(
        repo.create(&CommandId::new("atomic").unwrap(), cp.clone()),
        Err(SessionError::Duplicate)
    );
    for fault in 0..7 {
        let mut transition = append(&initial);
        let bytes = b"{}".to_vec();
        let mut record = RecordWrite {
            reference: content_reference("compile", &bytes).unwrap(),
            kind: RecordKind::Artifact,
            bytes,
        };
        transition.next.artifact = Some(record.reference.clone());
        transition.next.snapshot.dispatch = Some(DispatchId::new("compile").unwrap());
        transition.next.snapshot.dispatch_knowledge = DispatchKnowledge::Stopped;
        let expected = match fault {
            0 => {
                transition.fence = FenceToken::new(2).unwrap();
                SessionError::StaleRevision
            }
            1 => {
                transition.next.owner.principal = PrincipalId::new("foreign").unwrap();
                SessionError::ScopeDenied
            }
            2 => {
                record.bytes.push(b' ');
                SessionError::InvalidInput
            }
            3 => {
                transition.records = vec![record.clone(), record.clone(), record.clone()];
                SessionError::LimitExceeded
            }
            4 => {
                transition.next.artifact = None;
                SessionError::InvalidState
            }
            5 => {
                record.bytes = vec![0; 1_048_577];
                SessionError::InvalidInput
            }
            _ => {
                transition.next.budget.actions = 4;
                SessionError::LimitExceeded
            }
        };
        if transition.records.is_empty() {
            transition.records.push(record);
        }
        assert_eq!(repo.append(&owner(), transition), Err(expected));
        assert_eq!(repo.load(&owner(), &cp.snapshot.session).unwrap(), initial);
    }
    let bytes = b"{}".to_vec();
    let reference = content_reference("compile", &bytes).unwrap();
    let mut transition = append(&initial);
    transition.next.artifact = Some(reference.clone());
    transition.next.snapshot.dispatch = Some(DispatchId::new("compile").unwrap());
    transition.next.snapshot.dispatch_knowledge = DispatchKnowledge::Stopped;
    transition.records.push(RecordWrite {
        reference: reference.clone(),
        kind: RecordKind::Artifact,
        bytes: bytes.clone(),
    });
    let committed = repo.append(&owner(), transition).unwrap();
    assert_eq!(repo.load_artifact(&owner(), &reference).unwrap(), bytes);
    assert_eq!(
        repo.record_kind(&owner(), &reference).unwrap(),
        RecordKind::Artifact
    );
    let mut duplicate = append(&committed);
    duplicate.records.push(RecordWrite {
        reference: reference.clone(),
        kind: RecordKind::Artifact,
        bytes,
    });
    assert_eq!(
        repo.append(&owner(), duplicate),
        Err(SessionError::Duplicate)
    );
    assert_eq!(
        repo.load(&owner(), &cp.snapshot.session).unwrap(),
        committed
    );
    assert_eq!(
        repo.load_artifact(&owner(), &super::content_reference("absent", b"?").unwrap()),
        Err(SessionError::Unavailable)
    );
    let mut invalid = checkpoint(&repo, "invalid");
    invalid.snapshot.revision = Revision::new(2).unwrap();
    assert_eq!(
        repo.create(&CommandId::new("invalid").unwrap(), invalid),
        Err(SessionError::InvalidState)
    );
}
#[test]
fn postgres_trusted_grants_preserve_denial_withdrawal_and_consumed_history() {
    for decision in ["deny", "withdraw", "consume-withdraw"] {
        let Some(repo) = repository() else {
            return;
        };
        let cp = checkpoint(&repo, decision);
        let current = repo.create(&CommandId::new(decision).unwrap(), cp).unwrap();
        let pending = repo.append(&owner(), pause(&current)).unwrap();
        let mut foreign = owner();
        foreign.client_owner = ClientOwnerId::new("foreign").unwrap();
        assert_eq!(
            repo.load(&foreign, &pending.checkpoint.snapshot.session),
            Err(SessionError::ScopeDenied)
        );
        assert_eq!(
            repo.issue(
                &owner(),
                &pending.checkpoint.snapshot.session,
                &owner().principal,
                "bad",
                100
            ),
            Err(SessionError::InvalidInput)
        );
        assert_eq!(
            repo.issue(
                &owner(),
                &pending.checkpoint.snapshot.session,
                &owner().principal,
                "approve",
                MAX_REVISION
            ),
            Err(SessionError::ExpiredInteraction)
        );
        let reference = repo
            .issue(
                &owner(),
                &pending.checkpoint.snapshot.session,
                &owner().principal,
                if decision == "deny" {
                    "deny"
                } else {
                    "approve"
                },
                100,
            )
            .unwrap();
        if decision != "deny" {
            let reference = ConsentRecordRef(reference.unwrap());
            assert_eq!(
                repo.issue(
                    &owner(),
                    &pending.checkpoint.snapshot.session,
                    &owner().principal,
                    "approve",
                    100
                ),
                Err(SessionError::Duplicate)
            );
            let ConsentStatus::Granted(grant) = repo.consent(&owner(), &reference).unwrap() else {
                panic!("trusted grant")
            };
            assert_eq!(
                repo.consent(&foreign, &reference),
                Err(SessionError::Unavailable)
            );
            if decision == "consume-withdraw" {
                let mut transition = append(&pending);
                transition.next.accepted_consent = Some(*grant);
                transition.next.pending = None;
                transition.next.snapshot.pending = None;
                transition.next.snapshot.state = SessionState::Runnable;
                repo.append(&owner(), transition).unwrap();
            }
            repo.issue(
                &owner(),
                &pending.checkpoint.snapshot.session,
                &owner().principal,
                "withdraw",
                100,
            )
            .unwrap();
            assert_eq!(
                repo.consent(&owner(), &reference).unwrap(),
                ConsentStatus::Withdrawn
            );
        } else {
            let denied = repo
                .run(|journal| Ok(ConsentRecordRef(journal.grants[0].reference.clone())))
                .unwrap();
            assert_eq!(
                repo.consent(&owner(), &denied).unwrap(),
                ConsentStatus::Denied
            );
        }
        let stopped = repo
            .load(&owner(), &pending.checkpoint.snapshot.session)
            .unwrap();
        assert_eq!(stopped.checkpoint.snapshot.state, SessionState::Failed);
        assert_eq!(stopped.checkpoint.authority_events.len(), 1);
        assert_eq!(
            repo.issue(
                &owner(),
                &pending.checkpoint.snapshot.session,
                &owner().principal,
                "approve",
                100
            ),
            Err(SessionError::InvalidState)
        );
    }
}
#[test]
fn corrupted_journal_versions_links_and_digest_fail_closed() {
    let Some(repo) = repository() else {
        return;
    };
    let cp = checkpoint(&repo, "corrupt");
    repo.create(&CommandId::new("corrupt").unwrap(), cp)
        .unwrap();
    for fault in 0..9 {
        let original = repo
            .run(|journal| serde_json::to_value(&*journal).map_err(|_| SessionError::InvalidInput))
            .unwrap();
        let mut wire = original.clone();
        match fault {
            0 => wire["schema_version"] = serde_json::json!(9),
            1 => wire["sessions"][0]["fence"] = serde_json::json!(0),
            2 => wire["sessions"][0]["checkpoint"]["snapshot"]["revision"] = serde_json::json!(0),
            3 => {
                wire["sessions"][0]["checkpoint"]["selected_source"] =
                    serde_json::to_value(reference("unknown")).unwrap()
            }
            4 => wire["commands"] = serde_json::json!([]),
            5 => {
                let row = wire["sessions"][0].clone();
                wire["sessions"].as_array_mut().unwrap().push(row);
            }
            6 => {
                wire["sessions"][0]["checkpoint"]["artifact"] =
                    serde_json::to_value(reference("missing")).unwrap()
            }
            7 => {
                let cmd = wire["commands"][0].clone();
                wire["commands"].as_array_mut().unwrap().push(cmd);
            }
            _ => wire["sessions"][0]["checkpoint"]["basis"]["scope"] = serde_json::json!("foreign"),
        }
        // Write through the raw transaction owner to represent retained damaged data.
        repo.store
            .transact::<serde_json::Value, _, SessionError>(
                "task-sessions-v2",
                &serde_json::Value::Null,
                |stored| {
                    *stored = wire;
                    Ok(())
                },
            )
            .unwrap();
        assert!(
            repo.load(&owner(), &SessionId::new("corrupt").unwrap())
                .is_err()
        );
        repo.store
            .transact::<serde_json::Value, _, SessionError>(
                "task-sessions-v2",
                &serde_json::Value::Null,
                |stored| {
                    *stored = original;
                    Ok(())
                },
            )
            .unwrap();
    }
    assert_eq!(
        SessionError::from(StoreError::CommitUnknown),
        SessionError::OutcomeUnknown
    );
    assert_eq!(
        SessionError::from(StoreError::Limit),
        SessionError::LimitExceeded
    );
    assert!(matches!(
        CognitiveStore::connect("invalid connection", ContextScopeId::new("p").unwrap()),
        Err(StoreError::Storage)
    ));
}

#[test]
fn postgres_commit_ambiguity_and_checksum_damage_are_explicit() {
    let Some(repo) = repository() else {
        return;
    };
    let scope = repo.store.scope().as_str().to_string();
    let cp = checkpoint(&repo, "ambiguous");
    repo.create(&CommandId::new("ambiguous").unwrap(), cp)
        .unwrap();
    let connection = std::env::var("CG_COGNITIVE_TEST_DATABASE").unwrap();
    let mut database = postgres::Client::connect(&connection, postgres::NoTls).unwrap();
    let name = format!(
        "cg_failure_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    database.batch_execute(&format!("CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.scope='{scope}' THEN RAISE EXCEPTION 'qualification deferred commit failure'; END IF; RETURN NEW; END $$; CREATE CONSTRAINT TRIGGER {name} AFTER UPDATE ON cg_cognitive_journals DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION {name}();")).unwrap();
    let current = repo
        .load(&owner(), &SessionId::new("ambiguous").unwrap())
        .unwrap();
    let result = repo.append(&owner(), append(&current));
    database
        .batch_execute(&format!(
            "DROP TRIGGER {name} ON cg_cognitive_journals; DROP FUNCTION {name}();"
        ))
        .unwrap();
    assert_eq!(result, Err(SessionError::OutcomeUnknown));
    assert_eq!(
        repo.load(&owner(), &SessionId::new("ambiguous").unwrap())
            .unwrap(),
        current
    );
    database.execute("UPDATE cg_cognitive_journals SET digest='damaged' WHERE scope=$1 AND kind='task-sessions-v2'",&[&scope]).unwrap();
    assert_eq!(
        repo.load(&owner(), &SessionId::new("ambiguous").unwrap()),
        Err(SessionError::StorageUnavailable)
    );
}

#[test]
fn terminal_revocation_updates_authority_audit_without_revising_the_task() {
    let Some(repo) = repository() else {
        return;
    };
    let cp = checkpoint(&repo, "terminal-audit");
    let initial = repo
        .create(&CommandId::new("terminal-audit").unwrap(), cp)
        .unwrap();
    let pending = repo.append(&owner(), pause(&initial)).unwrap();
    let reference = ConsentRecordRef(
        repo.issue(
            &owner(),
            &pending.checkpoint.snapshot.session,
            &owner().principal,
            "approve",
            100,
        )
        .unwrap()
        .unwrap(),
    );
    let mut transition = append(&pending);
    transition.next.snapshot.state = SessionState::Cancelled;
    transition.next.snapshot.dispatch_knowledge = DispatchKnowledge::Stopped;
    transition.next.snapshot.pending = None;
    transition.next.pending = None;
    let terminal = repo.append(&owner(), transition).unwrap();
    repo.issue(
        &owner(),
        &terminal.checkpoint.snapshot.session,
        &owner().principal,
        "withdraw",
        MAX_REVISION,
    )
    .unwrap();
    assert_eq!(
        repo.consent(&owner(), &reference).unwrap(),
        ConsentStatus::Withdrawn
    );
    assert_eq!(
        repo.load(&owner(), &terminal.checkpoint.snapshot.session)
            .unwrap(),
        terminal
    );
    assert_eq!(
        repo.issue(
            &owner(),
            &terminal.checkpoint.snapshot.session,
            &owner().principal,
            "withdraw",
            100
        ),
        Err(SessionError::Unavailable)
    );
    let mut invalid = checkpoint(&repo, "no-assessment");
    invalid.initial_assessment = None;
    assert_eq!(
        repo.create(&CommandId::new("no-assessment").unwrap(), invalid),
        Err(SessionError::InvalidState)
    );
}
