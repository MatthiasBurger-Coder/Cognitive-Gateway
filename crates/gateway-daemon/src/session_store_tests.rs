//! PostgreSQL adapter failure/rollback tests, run by the disposable-host gate.
use super::*;
use gateway_application::connectors::{ConnectorOperationClass, ConnectorRequest, ConnectorTarget};
use gateway_domain::{
    ContentDigest, ContextScopeId, ExecutionProfile, OperatingMode, PlanStepId, PolicyId,
    ReferenceId, SchemaVersion, TaskId, prompt_ir::InvocationIrIdentity,
};
use std::{
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
pub(crate) fn owner() -> OwnerBinding {
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
    checkpoint_for_scope(repo.store.scope().clone(), id)
}
fn checkpoint_for_scope(scope: ContextScopeId, id: &str) -> SessionCheckpoint {
    let mut intent: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/declarative-cli/intent.json"
    ))
    .unwrap();
    intent["desired_state"]["conditions"][0]["id"] = serde_json::json!("context-verified");
    intent["desired_state"]["conditions"][0]["subject"] =
        serde_json::json!("cg.context.projection.verified");
    intent["desired_state"]["expression"]["value"] = serde_json::json!("context-verified");
    let goal = SupportedSessionGoal::artifact(
        SupportedArtifactGoal::validate(
            serde_json::from_value(intent).unwrap(),
            ArtifactGoalBasis {
                scope,
                plan: reference("plan"),
                step: PlanStepId::new("step").unwrap(),
                projection: reference("projection"),
                sources: vec![],
            },
        )
        .unwrap(),
    );
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
        resource_ledger: None,
        pending: None,
        accepted_consent: None,
        accepted_connector_consent: None,
        authority_events: vec![],
        selected_source: None,
        artifact: None,
        lease_until_ms: None,
        initial_assessment: Some(initial_assessment),
        connector_step_snapshots: vec![],
        connector_results: vec![],
        connector_observations: vec![],
        pre_execution_goal: None,
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

#[test]
fn fresh_session_reads_do_not_write_or_wait_for_writer_row_locks() {
    let Some(writer) = repository() else { return };
    let scope = writer.store.scope().clone();
    let reader = repository_for_scope(scope.as_str());
    let initial = checkpoint(&writer, "fresh-read");
    let owner = initial.owner.clone();
    let session = initial.snapshot.session.clone();
    assert_eq!(
        reader.load(&owner, &session),
        Err(SessionError::ScopeDenied)
    );
    assert!(
        reader
            .store
            .snapshot::<Journal>("task-sessions-v2")
            .unwrap()
            .is_none(),
        "a denied read must not initialize the journal"
    );
    let command = initial
        .snapshot
        .command_outcome
        .as_ref()
        .unwrap()
        .command
        .clone();
    let created = writer.create(&command, initial).unwrap();
    assert_eq!(reader.load(&owner, &session).unwrap(), created);

    // A reader sees one committed snapshot even while another connection owns
    // the write lock. A read transaction with FOR UPDATE would block here.
    let connection = std::env::var("CG_COGNITIVE_TEST_DATABASE").unwrap();
    let mut locker = postgres::Client::connect(&connection, postgres::NoTls).unwrap();
    let mut tx = locker.transaction().unwrap();
    tx.query_one(
        "SELECT revision FROM cg_cognitive_journals WHERE scope=$1 AND kind=$2 FOR UPDATE",
        &[&scope.as_str(), &"task-sessions-v2"],
    )
    .unwrap();
    assert_eq!(reader.load(&owner, &session).unwrap(), created);
    tx.rollback().unwrap();

    let advanced = writer.append(&owner, append(&created)).unwrap();
    assert_eq!(
        reader.load(&owner, &session).unwrap(),
        advanced,
        "a later read must observe the newly committed revision"
    );
    assert!(
        writer.append(&owner, append(&created)).is_err(),
        "fresh read optimization cannot weaken the write CAS"
    );
    assert_eq!(reader.load(&owner, &session).unwrap(), advanced);
    let mut foreign = owner.clone();
    foreign.client_owner = ClientOwnerId::new("foreign-reader").unwrap();
    assert_eq!(
        reader.load(&foreign, &session),
        Err(SessionError::ScopeDenied)
    );

    locker
        .execute(
            "UPDATE cg_cognitive_journals SET digest=$3 WHERE scope=$1 AND kind=$2",
            &[&scope.as_str(), &"task-sessions-v2", &"invalid-digest"],
        )
        .unwrap();
    assert_eq!(
        reader.load(&owner, &session),
        Err(SessionError::StorageUnavailable)
    );
}
pub(crate) fn repository_for_scope(scope: &str) -> PostgresSessionStore {
    let connection =
        std::env::var("CG_COGNITIVE_TEST_DATABASE").expect("run scripts/cognitive-test-host.py");
    PostgresSessionStore::new(
        CognitiveStore::connect(&connection, ContextScopeId::new(scope).unwrap()).unwrap(),
    )
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

fn connector_request(scope: &ContextScopeId, step: &PlanStepId) -> ConnectorRequest {
    ConnectorRequest {
        target: ConnectorTarget {
            connector: gateway_domain::NonEmptyText::new("connector-a").unwrap(),
            scope: scope.clone(),
        },
        capability: CapabilityId::new("connector.write").unwrap(),
        class: ConnectorOperationClass::Mutate,
        inventory_revision: ContentDigest::new("a".repeat(64)).unwrap(),
        arguments: Default::default(),
        task: TaskId::new("connector-task").unwrap(),
        step: step.clone(),
        execution: gateway_domain::NonEmptyText::new("execution-a").unwrap(),
        correlation: gateway_domain::NonEmptyText::new("correlation-a").unwrap(),
    }
}

fn connector_resume_material() -> ConnectorConsentResumeMaterial {
    let invocation = InvocationIrIdentity {
        schema_version: SchemaVersion::V1,
        invocation_id: ReferenceId::new("connector-invocation").unwrap(),
        invocation_digest: ContentDigest::new("c".repeat(64)).unwrap(),
        handoff_digest: ContentDigest::new("d".repeat(64)).unwrap(),
        tools_digest: ContentDigest::new("e".repeat(64)).unwrap(),
        ir_digest: ContentDigest::new("f".repeat(64)).unwrap(),
    };
    let pin = reference("connector-producer-pin");
    let raw_output = serde_json::to_vec(&serde_json::json!({
        "schema_version": SchemaVersion::V1,
        "invocation": invocation.clone(),
        "result": {},
        "references": [],
        "tools": [{"capability": {
            "id":"connector.write", "scope":"connector-consent-scope", "contract":"cg.capability",
            "contract_version":"1.0", "revision":"1", "digest":"a".repeat(64)
        }, "arguments":{"title":"original"}}]
    }))
    .unwrap();
    ConnectorConsentResumeMaterial::new(invocation.clone(), 0, raw_output, vec![pin.clone()])
        .unwrap()
}

fn connector_resume_binding() -> ConnectorConsentResumeBinding {
    let material = connector_resume_material();
    let bytes = serde_json::to_vec(&material).unwrap();
    ConnectorConsentResumeBinding {
        invocation: material.invocation.clone(),
        proposal_ordinal: material.proposal_ordinal,
        output_record: content_reference("connector-proposal-resume", &bytes).unwrap(),
        output_digest: material.output_digest,
        producer_pins: material.producer_pins.clone(),
    }
}

fn connector_pending_checkpoint(mut cp: SessionCheckpoint) -> SessionCheckpoint {
    let scope = cp.goal.scope().clone();
    let intent = cp.goal.intent().clone();
    let step = PlanStepId::new("connector-consent-step").unwrap();
    let condition = intent.desired_state().conditions()[0].id().clone();
    cp.goal = SupportedSessionGoal::connector(
        SupportedConnectorGoal::from_host_registration(
            intent,
            scope.clone(),
            reference("connector-consent-plan"),
            vec![step.clone()],
            vec![
                ConditionObservationMapping::new(
                    condition,
                    SourceId::new("connector-consent-source").unwrap(),
                    CapabilityId::new("connector.read").unwrap(),
                    ConnectorOperationClass::Read,
                )
                .unwrap(),
            ],
        )
        .unwrap(),
    );
    cp.snapshot.revision = Revision::new(2).unwrap();
    let pending = PendingRef {
        id: PendingId::new("connector-consent-pending").unwrap(),
        issued_revision: cp.snapshot.revision,
        expires_at_ms: 1_000,
    };
    let dispatch = DispatchId::new("connector-consent-dispatch").unwrap();
    let request = connector_request(&scope, &step);
    let binding = ConnectorConsentBinding::new(
        cp.owner.clone(),
        scope,
        cp.snapshot.session.clone(),
        cp.snapshot.run.clone(),
        cp.snapshot.revision,
        pending.id.clone(),
        step,
        dispatch.clone(),
        &request,
        connector_resume_binding(),
        PolicyId::new("connector-policy").unwrap(),
        ContentDigest::new("b".repeat(64)).unwrap(),
        pending.expires_at_ms,
        cp.owner.principal.clone(),
    )
    .unwrap();
    cp.snapshot.state = SessionState::PendingConsent;
    cp.snapshot.pending = Some(pending);
    cp.snapshot.dispatch = Some(dispatch);
    cp.snapshot.dispatch_knowledge = DispatchKnowledge::Prepared;
    cp.pending = Some(PendingInteraction::ConnectorConsent(Box::new(binding)));
    cp
}

#[test]
fn connector_consent_pending_roundtrips_and_rejects_identity_drift() {
    let scope = ContextScopeId::new("connector-consent-roundtrip").unwrap();
    let cp = connector_pending_checkpoint(checkpoint_for_scope(scope.clone(), "consent-wire"));
    cp.validate().unwrap();
    let restored = Checkpoint::from_typed(&cp).typed().unwrap();
    assert_eq!(restored, cp);

    let mut foreign_owner = cp.clone();
    foreign_owner.owner.project = ProjectId::new("other-project").unwrap();
    assert_eq!(
        foreign_owner.validate(),
        Err(SessionError::InvalidInteraction)
    );

    let mut wrong_pending = cp.clone();
    wrong_pending.snapshot.pending.as_mut().unwrap().id =
        PendingId::new("different-pending").unwrap();
    assert_eq!(
        wrong_pending.validate(),
        Err(SessionError::InvalidInteraction)
    );

    let mut wrong_dispatch = cp;
    wrong_dispatch.snapshot.dispatch = Some(DispatchId::new("different-dispatch").unwrap());
    assert_eq!(
        wrong_dispatch.validate(),
        Err(SessionError::InvalidInteraction)
    );
}

#[test]
fn connector_search_and_inspect_observation_classes_roundtrip_through_storage_wire() {
    let scope = ContextScopeId::new("connector-read-operation-roundtrip").unwrap();
    for class in [
        ConnectorOperationClass::Search,
        ConnectorOperationClass::Inspect,
    ] {
        let mut cp = checkpoint_for_scope(scope.clone(), &format!("connector-{class:?}"));
        let intent = cp.goal.intent().clone();
        let condition = intent.desired_state().conditions()[0].id().clone();
        cp.goal = SupportedSessionGoal::connector(
            SupportedConnectorGoal::from_host_registration(
                intent,
                scope.clone(),
                reference("connector-read-operation-plan"),
                vec![PlanStepId::new("connector-read-operation-step").unwrap()],
                vec![
                    ConditionObservationMapping::new(
                        condition,
                        SourceId::new("connector-read-operation-source").unwrap(),
                        CapabilityId::new("connector.read").unwrap(),
                        class,
                    )
                    .unwrap(),
                ],
            )
            .unwrap(),
        );
        let restored = Checkpoint::from_typed(&cp).typed().unwrap();
        assert_eq!(
            restored.goal.as_connector().unwrap().observations()[0].operation_class(),
            class
        );
    }
}

fn pending_connector_input_append(
    repo: &PostgresSessionStore,
    id: &str,
) -> (FencedSession, JournalAppend) {
    let pending = connector_pending_checkpoint(checkpoint(repo, id));
    let mut initial = pending.clone();
    initial.snapshot.revision = Revision::new(1).unwrap();
    initial.snapshot.state = SessionState::Runnable;
    initial.snapshot.pending = None;
    initial.snapshot.dispatch = None;
    initial.snapshot.dispatch_knowledge = DispatchKnowledge::None;
    initial.pending = None;
    let command = initial
        .snapshot
        .command_outcome
        .as_ref()
        .unwrap()
        .command
        .clone();
    repo.create(&command, initial.clone()).unwrap();
    let current = repo
        .load(&initial.owner, &initial.snapshot.session)
        .unwrap();
    let resume_record = match pending.pending.as_ref().unwrap() {
        PendingInteraction::ConnectorConsent(binding) => binding.resume.output_record.clone(),
        _ => unreachable!(),
    };
    let input_bytes = b"connector-producer-pin".to_vec();
    let input = RecordWrite {
        reference: content_reference("connector-producer-pin", &input_bytes).unwrap(),
        kind: RecordKind::ConnectorProducerInput,
        bytes: input_bytes,
    };
    let transition = JournalAppend {
        session: initial.snapshot.session.clone(),
        expected_revision: current.checkpoint.snapshot.revision,
        fence: current.fence,
        command: None,
        next: pending,
        records: vec![
            input,
            RecordWrite {
                reference: resume_record,
                kind: RecordKind::ConnectorProposal,
                bytes: serde_json::to_vec(&connector_resume_material()).unwrap(),
            },
        ],
    };
    (current, transition)
}
pub(crate) fn create_pending_connector_session(
    repo: &PostgresSessionStore,
    id: &str,
) -> FencedSession {
    let (initial, transition) = pending_connector_input_append(repo, id);
    repo.append(&initial.checkpoint.owner, transition).unwrap()
}

fn step_binding(checkpoint: &SessionCheckpoint, revision: u64) -> ConnectorStepBinding {
    ConnectorStepBinding {
        owner: checkpoint.owner.clone(),
        scope: checkpoint.goal.scope().clone(),
        session: checkpoint.snapshot.session.clone(),
        run: checkpoint.snapshot.run.clone(),
        revision: Revision::new(revision).unwrap(),
        task: TaskId::new("connector-task").unwrap(),
        step: PlanStepId::new("connector-step").unwrap(),
        dispatch: DispatchId::new("connector-dispatch").unwrap(),
        invocation: InvocationIrIdentity {
            schema_version: SchemaVersion::V1,
            invocation_id: ReferenceId::new("invocation").unwrap(),
            invocation_digest: ContentDigest::new("a".repeat(64)).unwrap(),
            handoff_digest: ContentDigest::new("b".repeat(64)).unwrap(),
            tools_digest: ContentDigest::new("c".repeat(64)).unwrap(),
            ir_digest: ContentDigest::new("d".repeat(64)).unwrap(),
        },
    }
}

#[test]
fn connector_step_append_accepts_maximum_bounded_record_shape() {
    let maximum = MAX_CONNECTOR_STEP_COMMIT_RECORDS;
    assert_eq!(
        maximum,
        2 + MAX_CONNECTOR_PROPOSALS_PER_STEP + MAX_CONNECTOR_GOAL_CONDITIONS
    );
    let records = (0..maximum)
        .map(|index| {
            let bytes = vec![index as u8];
            RecordWrite {
                reference: content_reference(&format!("bounded-{index}"), &bytes).unwrap(),
                kind: RecordKind::ConnectorObservation,
                bytes,
            }
        })
        .collect::<Vec<_>>();
    validate_append_record_limits(&records).unwrap();
}

#[test]
fn connector_step_append_rejects_record_count_and_payload_over_limits() {
    let count = MAX_CONNECTOR_STEP_COMMIT_RECORDS + 1;
    let records = (0..count)
        .map(|index| {
            let bytes = vec![index as u8];
            RecordWrite {
                reference: content_reference(&format!("too-many-{index}"), &bytes).unwrap(),
                kind: RecordKind::ConnectorObservation,
                bytes,
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        validate_append_record_limits(&records),
        Err(SessionError::LimitExceeded)
    );

    let bytes = vec![0; MAX_CONNECTOR_STEP_COMMIT_BYTES + 1];
    let records = vec![RecordWrite {
        reference: content_reference("too-large", &bytes).unwrap(),
        kind: RecordKind::ConnectorObservation,
        bytes,
    }];
    assert_eq!(
        validate_append_record_limits(&records),
        Err(SessionError::LimitExceeded)
    );
}

#[test]
fn journal_validates_scoped_connector_record_links_without_a_database() {
    let scope = ContextScopeId::new("connector-journal-scope").unwrap();
    let mut checkpoint = checkpoint_for_scope(scope.clone(), "connector-journal");
    let intent = checkpoint.goal.intent().clone();
    let condition = intent.desired_state().conditions()[0].id().clone();
    checkpoint.goal = SupportedSessionGoal::connector(
        SupportedConnectorGoal::from_host_registration(
            intent,
            scope.clone(),
            reference("connector-plan"),
            vec![PlanStepId::new("connector-step").unwrap()],
            vec![
                ConditionObservationMapping::new(
                    condition,
                    gateway_domain::SourceId::new("connector-source").unwrap(),
                    gateway_domain::CapabilityId::new("connector.read").unwrap(),
                    gateway_application::connectors::ConnectorOperationClass::Read,
                )
                .unwrap(),
            ],
        )
        .unwrap(),
    );
    checkpoint.snapshot.revision = Revision::new(3).unwrap();
    let before_bytes = br#"{"snapshot":"before"}"#.to_vec();
    let after_bytes = br#"{"snapshot":"after"}"#.to_vec();
    let observation_bytes = br#"{"observation":"separate"}"#.to_vec();
    let before_ref = content_reference("before-record", &before_bytes).unwrap();
    let after_ref = content_reference("after-record", &after_bytes).unwrap();
    let observation_ref = content_reference("observation-record", &observation_bytes).unwrap();
    let before_binding = step_binding(&checkpoint, 2);
    let after_binding = step_binding(&checkpoint, 3);
    checkpoint.connector_step_snapshots = vec![
        ConnectorStepSnapshotRef {
            binding: before_binding,
            phase: ConnectorSnapshotPhase::Before,
            record: before_ref.clone(),
        },
        ConnectorStepSnapshotRef {
            binding: after_binding.clone(),
            phase: ConnectorSnapshotPhase::After,
            record: after_ref.clone(),
        },
    ];
    let result_bytes = br#"{"ordered_batch":true}"#.to_vec();
    let result_ref = content_reference("proposal-batch", &result_bytes).unwrap();
    checkpoint.connector_results.push(ConnectorResultRef {
        binding: after_binding.clone(),
        record: result_ref.clone(),
        after_snapshot_state: ConnectorAfterSnapshotState::Captured,
    });
    checkpoint
        .connector_observations
        .push(ConnectorObservationRef {
            binding: after_binding,
            proposal_ordinal: Some(0),
            goal_condition: None,
            role: ConnectorObservationRole::PostEffectReadback,
            record: observation_ref.clone(),
        });
    let outcome = checkpoint.snapshot.command_outcome.clone().unwrap();
    let record = |reference, kind, bytes| Record {
        owner: checkpoint.owner.clone(),
        reference,
        kind,
        bytes,
    };
    let mut journal = Journal {
        schema_version: 2,
        sessions: vec![StoredSession {
            checkpoint: Checkpoint::from_typed(&checkpoint),
            fence: 1,
        }],
        commands: vec![Ledger {
            key: CommandKey {
                owner: checkpoint.owner.clone(),
                command: outcome.command.clone(),
            },
            outcome,
        }],
        records: vec![
            record(
                before_ref.clone(),
                RecordKind::ConnectorSnapshot,
                before_bytes,
            ),
            record(
                after_ref.clone(),
                RecordKind::ConnectorSnapshot,
                after_bytes,
            ),
            record(
                observation_ref.clone(),
                RecordKind::ConnectorObservation,
                observation_bytes,
            ),
            record(
                result_ref.clone(),
                RecordKind::ConnectorResult,
                result_bytes,
            ),
        ],
        grants: vec![],
        connector_grants: vec![],
    };
    journal.validate(&scope).unwrap();
    assert_eq!(journal.sessions[0].typed().unwrap().checkpoint, checkpoint);

    journal.records[2].kind = RecordKind::Evidence;
    assert_eq!(
        journal.validate(&scope),
        Err(SessionError::StorageUnavailable)
    );
}

#[test]
fn connector_snapshots_and_observations_commit_as_separate_immutable_records() {
    let Some(repo) = repository() else {
        return;
    };
    let mut connector_checkpoint = checkpoint(&repo, "connector-records");
    let intent = connector_checkpoint.goal.intent().clone();
    let condition = intent.desired_state().conditions()[0].id().clone();
    connector_checkpoint.goal = SupportedSessionGoal::connector(
        SupportedConnectorGoal::from_host_registration(
            intent,
            repo.store.scope().clone(),
            reference("connector-plan"),
            vec![PlanStepId::new("connector-step").unwrap()],
            vec![
                ConditionObservationMapping::new(
                    condition,
                    gateway_domain::SourceId::new("connector-source").unwrap(),
                    gateway_domain::CapabilityId::new("connector.read").unwrap(),
                    gateway_application::connectors::ConnectorOperationClass::Read,
                )
                .unwrap(),
            ],
        )
        .unwrap(),
    );
    let initial = repo
        .create(
            &CommandId::new("connector-records").unwrap(),
            connector_checkpoint,
        )
        .unwrap();

    let before_bytes = br#"{"kind":"before"}"#.to_vec();
    let before_ref = content_reference("connector-before", &before_bytes).unwrap();
    let mut reserve = append(&initial);
    let before_revision = reserve.next.snapshot.revision;
    reserve.next.snapshot.state = SessionState::Dispatching;
    reserve.next.snapshot.dispatch = Some(DispatchId::new("connector-dispatch").unwrap());
    reserve.next.snapshot.dispatch_knowledge = DispatchKnowledge::Reserved;
    reserve.next.lease_until_ms = Some(MAX_REVISION);
    reserve
        .next
        .connector_step_snapshots
        .push(ConnectorStepSnapshotRef {
            binding: step_binding(&reserve.next, before_revision.value()),
            phase: ConnectorSnapshotPhase::Before,
            record: before_ref.clone(),
        });
    reserve.records.push(RecordWrite {
        reference: before_ref.clone(),
        kind: RecordKind::ConnectorSnapshot,
        bytes: before_bytes.clone(),
    });
    let reserved = repo.append(&owner(), reserve).unwrap();

    let after_bytes = br#"{"kind":"after"}"#.to_vec();
    let observation_bytes = br#"{"kind":"connector_observation"}"#.to_vec();
    let after_ref = content_reference("connector-after", &after_bytes).unwrap();
    let observation_ref = content_reference("connector-observation", &observation_bytes).unwrap();
    let mut complete = append(&reserved);
    let after_revision = complete.next.snapshot.revision;
    let binding = step_binding(&complete.next, after_revision.value());
    complete
        .next
        .connector_step_snapshots
        .push(ConnectorStepSnapshotRef {
            binding: binding.clone(),
            phase: ConnectorSnapshotPhase::After,
            record: after_ref.clone(),
        });
    complete
        .next
        .connector_observations
        .push(ConnectorObservationRef {
            binding,
            proposal_ordinal: Some(0),
            goal_condition: None,
            role: ConnectorObservationRole::PostEffectReadback,
            record: observation_ref.clone(),
        });
    let batch_bytes = br#"{"ordered_batch":true}"#.to_vec();
    let batch_ref = content_reference("connector-batch", &batch_bytes).unwrap();
    complete.next.connector_results.push(ConnectorResultRef {
        binding: step_binding(&complete.next, after_revision.value()),
        record: batch_ref.clone(),
        after_snapshot_state: ConnectorAfterSnapshotState::Captured,
    });
    complete.records.extend([
        RecordWrite {
            reference: after_ref.clone(),
            kind: RecordKind::ConnectorSnapshot,
            bytes: after_bytes.clone(),
        },
        RecordWrite {
            reference: observation_ref.clone(),
            kind: RecordKind::ConnectorObservation,
            bytes: observation_bytes.clone(),
        },
        RecordWrite {
            reference: batch_ref.clone(),
            kind: RecordKind::ConnectorResult,
            bytes: batch_bytes,
        },
    ]);
    let committed = repo.append(&owner(), complete).unwrap();

    assert_eq!(
        repo.load_artifact(&owner(), &before_ref).unwrap(),
        before_bytes
    );
    assert_eq!(
        repo.load_artifact(&owner(), &after_ref).unwrap(),
        after_bytes
    );
    assert_eq!(
        repo.load_artifact(&owner(), &observation_ref).unwrap(),
        observation_bytes
    );
    assert_eq!(
        repo.record_kind(&owner(), &before_ref).unwrap(),
        RecordKind::ConnectorSnapshot
    );
    assert_eq!(
        repo.record_kind(&owner(), &observation_ref).unwrap(),
        RecordKind::ConnectorObservation
    );
    assert_eq!(
        repo.load(&owner(), &committed.checkpoint.snapshot.session)
            .unwrap(),
        committed
    );
}

#[test]
fn legacy_checkpoint_without_connector_record_fields_still_deserializes() {
    let legacy_checkpoint =
        checkpoint_for_scope(ContextScopeId::new("legacy-scope").unwrap(), "legacy");
    let mut legacy = serde_json::to_value(Checkpoint::from_typed(&legacy_checkpoint)).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("connector_step_snapshots");
    legacy
        .as_object_mut()
        .unwrap()
        .remove("connector_observations");
    legacy.as_object_mut().unwrap().remove("resource_ledger");
    let restored: Checkpoint = serde_json::from_value(legacy).unwrap();
    let typed = restored.typed().unwrap();
    assert!(typed.connector_step_snapshots.is_empty());
    assert!(typed.connector_observations.is_empty());
    assert!(typed.resource_ledger.is_none());
}

fn session_resource_limits() -> SessionResourceLimits {
    SessionResourceLimits {
        cost_unit: ReferenceId::new("credits").unwrap(),
        amounts: SessionResourceAmounts {
            model_attempts: 1,
            input_tokens: 100,
            output_tokens: 25,
            cost_microunits: 10,
            ..Default::default()
        },
    }
}

fn resource_connector_checkpoint(cp: SessionCheckpoint) -> SessionCheckpoint {
    let mut cp = connector_pending_checkpoint(cp);
    cp.snapshot.revision = Revision::new(1).unwrap();
    cp.snapshot.state = SessionState::Runnable;
    cp.snapshot.pending = None;
    cp.snapshot.dispatch = None;
    cp.snapshot.dispatch_knowledge = DispatchKnowledge::None;
    cp.pending = None;
    cp
}

fn reserve_preparation_resources(cp: &mut SessionCheckpoint) {
    let binding = ResourceReservationBinding {
        owner: cp.owner.clone(),
        scope: cp.goal.scope().clone(),
        session: cp.snapshot.session.clone(),
        run: cp.snapshot.run.clone(),
        revision: cp.snapshot.revision,
        step: cp.goal.as_connector().unwrap().steps()[0].clone(),
        dispatch: DispatchId::new("resource-preparation").unwrap(),
    };
    cp.snapshot.state = SessionState::Preparing;
    cp.snapshot.dispatch = Some(binding.dispatch.clone());
    cp.snapshot.dispatch_knowledge = DispatchKnowledge::Prepared;
    cp.lease_until_ms = Some(1_000);
    cp.resource_ledger
        .as_mut()
        .unwrap()
        .reserve_plan(
            binding,
            SessionResourcePlan {
                preparation: SessionResourceAmounts {
                    cost_microunits: 8,
                    ..session_resource_limits().amounts
                },
                approved_resume: SessionResourceAmounts {
                    cost_microunits: 2,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
}

#[test]
fn resource_ledger_storage_wire_preserves_consumption_and_rejects_corruption() {
    let mut cp = resource_connector_checkpoint(checkpoint_for_scope(
        ContextScopeId::new("resource-wire").unwrap(),
        "resources",
    ));
    cp.resource_ledger = Some(SessionResourceLedger::new(session_resource_limits()).unwrap());
    reserve_preparation_resources(&mut cp);
    cp.validate().unwrap();
    let wire = serde_json::to_value(Checkpoint::from_typed(&cp)).unwrap();
    let restored: Checkpoint = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(restored.typed().unwrap(), cp);

    let mut corrupted = wire;
    corrupted["resource_ledger"]["used"]["model_attempts"] = serde_json::json!(0);
    assert!(
        serde_json::from_value::<Checkpoint>(corrupted)
            .map_err(|_| SessionError::InvalidInput)
            .and_then(|checkpoint| checkpoint.typed())
            .is_err()
    );
}

#[test]
fn postgres_create_rejects_preconsumed_resource_history() {
    let Some(repo) = repository() else {
        return;
    };
    let mut initial = resource_connector_checkpoint(checkpoint(&repo, "forged-resource-start"));
    initial.resource_ledger = Some(SessionResourceLedger::new(session_resource_limits()).unwrap());
    let command = CommandId::new("forged-resource-start").unwrap();
    let mut forged = initial.clone();
    reserve_preparation_resources(&mut forged);
    assert_eq!(
        repo.create(&command, forged.clone()),
        Err(SessionError::InvalidState)
    );
    forged.snapshot.state = SessionState::Runnable;
    forged.snapshot.dispatch = None;
    forged.snapshot.dispatch_knowledge = DispatchKnowledge::None;
    forged.lease_until_ms = None;
    assert_eq!(
        repo.create(&command, forged),
        Err(SessionError::InvalidState)
    );
    let clean = repo.create(&command, initial).unwrap();
    assert!(
        clean
            .checkpoint
            .resource_ledger
            .as_ref()
            .unwrap()
            .used()
            .is_zero()
    );
}

/// Distinct processes reconstruct the PostgreSQL adapter; the database server
/// stays running. Missing telemetry conservatively retains the whole bound.
#[test]
fn postgres_resource_reservation_survives_process_restart_without_refill() {
    const PHASE: &str = "CG_SESSION_RESOURCE_RESTART_PHASE";
    const SCOPE: &str = "CG_SESSION_RESOURCE_RESTART_SCOPE";
    let Some(connection) = std::env::var_os("CG_COGNITIVE_TEST_DATABASE") else {
        return;
    };
    if let Ok(phase) = std::env::var(PHASE) {
        let scope = std::env::var(SCOPE).unwrap();
        let repo = repository_for_scope(&scope);
        let session = SessionId::new("resource-restart").unwrap();
        match phase.as_str() {
            "seed" => {
                let mut cp = resource_connector_checkpoint(checkpoint(&repo, session.as_str()));
                cp.resource_ledger =
                    Some(SessionResourceLedger::new(session_resource_limits()).unwrap());
                let current = repo
                    .create(&CommandId::new(session.as_str()).unwrap(), cp)
                    .unwrap();
                let mut transition = append(&current);
                reserve_preparation_resources(&mut transition.next);
                repo.append(&owner(), transition).unwrap();
            }
            "recover" => {
                let current = repo.load(&owner(), &session).unwrap();
                let ledger = current.checkpoint.resource_ledger.as_ref().unwrap();
                assert_eq!(ledger.used(), &session_resource_limits().amounts);
                assert_eq!(ledger.reservations().len(), 1);
                let plan = ledger.reservations()[0]
                    .plan()
                    .expect("v2 phase plan survived process restart");
                assert_eq!(plan.total().unwrap(), session_resource_limits().amounts);
                assert_eq!(plan.preparation.cost_microunits, 8);
                assert_eq!(plan.approved_resume.cost_microunits, 2);
                let mut transition = append(&current);
                transition.next.resource_ledger = None;
                assert_eq!(
                    repo.append(&owner(), transition),
                    Err(SessionError::InvalidState)
                );
                assert_eq!(repo.load(&owner(), &session).unwrap(), current);
                let mut exhausted = ledger.clone();
                let mut binding = ledger.reservations()[0].binding().clone();
                binding.dispatch = DispatchId::new("would-refill-after-restart").unwrap();
                binding.revision = binding.revision.next().unwrap();
                assert_eq!(
                    exhausted.reserve(
                        binding,
                        SessionResourceAmounts {
                            model_attempts: 1,
                            ..Default::default()
                        }
                    ),
                    Err(SessionError::LimitExceeded)
                );
                assert_eq!(exhausted, *ledger);
            }
            _ => panic!("unexpected resource restart phase"),
        }
        return;
    }
    let scope = format!(
        "resource-restart-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let executable = std::env::current_exe().unwrap();
    for phase in ["seed", "recover"] {
        let status = Command::new(&executable)
            .args(["--exact", "session_store::tests::postgres_resource_reservation_survives_process_restart_without_refill", "--nocapture"])
            .env(PHASE, phase)
            .env(SCOPE, &scope)
            .env("CG_COGNITIVE_TEST_DATABASE", &connection)
            .status().unwrap();
        assert!(
            status.success(),
            "resource restart child phase {phase} failed"
        );
    }
}

#[test]
fn connector_consent_decision_requires_a_matching_pending_binding() {
    let Some(repo) = repository() else {
        return;
    };
    let ordinary = checkpoint(&repo, "connector-consent-no-pending");
    let command = ordinary
        .snapshot
        .command_outcome
        .as_ref()
        .unwrap()
        .command
        .clone();
    repo.create(&command, ordinary.clone()).unwrap();
    assert_eq!(
        repo.operator_decide_connector_consent(
            &owner(),
            &ordinary.snapshot.session,
            &owner().principal,
            "approve",
            100,
        ),
        Err(SessionError::InvalidInteraction)
    );

    let pending = create_pending_connector_session(&repo, "connector-consent-wrong-issuer");
    repo.run(|journal| {
        let index = journal.index(&owner(), &pending.checkpoint.snapshot.session)?;
        let mut current = journal.sessions[index].typed()?.checkpoint;
        let Some(PendingInteraction::ConnectorConsent(binding)) = current.pending.as_mut() else {
            return Err(SessionError::InvalidInteraction);
        };
        binding.issuer = PrincipalId::new("different-issuer").unwrap();
        current.validate()?;
        journal.sessions[index].checkpoint = Checkpoint::from_typed(&current);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        repo.operator_decide_connector_consent(
            &owner(),
            &pending.checkpoint.snapshot.session,
            &owner().principal,
            "approve",
            100,
        ),
        Err(SessionError::AuthorityDenied)
    );
}

#[test]
fn connector_consent_ledger_is_owner_scoped_and_tracks_withdrawal() {
    let Some(repo) = repository() else {
        return;
    };
    let pending = create_pending_connector_session(&repo, "connector-approval");
    let session = pending.checkpoint.snapshot.session.clone();
    let revision = pending.checkpoint.snapshot.revision;
    let reference = repo
        .operator_decide_connector_consent(&owner(), &session, &owner().principal, "approve", 100)
        .unwrap()
        .unwrap();
    // A store decision does not itself resume the pending checkpoint.
    let still_pending = repo.load(&owner(), &session).unwrap();
    assert_eq!(still_pending.checkpoint.snapshot.revision, revision);
    assert_eq!(
        still_pending.checkpoint.snapshot.state,
        SessionState::PendingConsent
    );

    let ConnectorConsentStatus::Granted(grant) =
        repo.connector_consent(&owner(), &reference).unwrap()
    else {
        panic!("expected trusted connector grant")
    };
    let binding = grant.binding();
    assert_eq!(binding.session, session);
    assert_eq!(binding.issued_revision, revision);
    let request = connector_request(&binding.scope, &binding.step);
    grant
        .check_dispatch(
            &owner(),
            &binding.scope,
            &binding.session,
            &binding.run,
            binding.issued_revision,
            &binding.step,
            &binding.dispatch,
            &request,
            &binding.policy,
            &binding.policy_revision,
            101,
            false,
        )
        .unwrap();
    let mut drifted_request = request;
    drifted_request.execution = gateway_domain::NonEmptyText::new("other-execution").unwrap();
    assert_eq!(
        grant.check_dispatch(
            &owner(),
            &binding.scope,
            &binding.session,
            &binding.run,
            binding.issued_revision,
            &binding.step,
            &binding.dispatch,
            &drifted_request,
            &binding.policy,
            &binding.policy_revision,
            101,
            false,
        ),
        Err(SessionError::StaleRevision)
    );
    let mut foreign = owner();
    foreign.project = ProjectId::new("foreign-project").unwrap();
    assert_eq!(
        repo.connector_consent(&foreign, &reference),
        Err(SessionError::Unavailable)
    );
    repo.operator_decide_connector_consent(&owner(), &session, &owner().principal, "withdraw", 102)
        .unwrap();
    assert_eq!(
        repo.connector_consent(&owner(), &reference).unwrap(),
        ConnectorConsentStatus::Withdrawn
    );
}

#[test]
fn journal_validation_rejects_duplicate_connector_grant_record() {
    let Some(repo) = repository() else {
        return;
    };
    let pending = create_pending_connector_session(&repo, "connector-duplicate-journal-grant");
    repo.run(|journal| {
        let index = journal.index(&owner(), &pending.checkpoint.snapshot.session)?;
        let record = match journal.sessions[index].checkpoint.pending.as_ref().unwrap() {
            PendingWire::ConnectorConsent(binding) => binding.resume.output_record.clone(),
            _ => return Err(SessionError::InvalidInteraction),
        };
        let record_index = journal
            .records
            .iter()
            .position(|stored| stored.reference == record)
            .unwrap();
        let removed = journal.records.remove(record_index);
        assert_eq!(
            journal.validate(repo.store.scope()),
            Err(SessionError::StorageUnavailable)
        );
        journal.records.insert(record_index, removed);
        Ok(())
    })
    .unwrap();
    repo.operator_decide_connector_consent(
        &owner(),
        &pending.checkpoint.snapshot.session,
        &owner().principal,
        "approve",
        100,
    )
    .unwrap();
    repo.run(|journal| {
        let duplicate = journal.connector_grants.first().cloned().unwrap();
        journal.connector_grants.push(duplicate);
        assert_eq!(
            journal.validate(repo.store.scope()),
            Err(SessionError::StorageUnavailable)
        );
        journal.connector_grants.pop();
        Ok(())
    })
    .unwrap();
}

#[test]
fn connector_consent_expiry_denial_and_duplicate_decisions_are_typed() {
    let Some(expiry_repo) = repository() else {
        return;
    };

    let expired = create_pending_connector_session(&expiry_repo, "connector-expired-consent");
    assert_eq!(
        expiry_repo.operator_decide_connector_consent(
            &owner(),
            &expired.checkpoint.snapshot.session,
            &owner().principal,
            "approve",
            1_000,
        ),
        Err(SessionError::ExpiredInteraction)
    );
    assert_eq!(
        expiry_repo.operator_decide_connector_consent(
            &owner(),
            &expired.checkpoint.snapshot.session,
            &owner().principal,
            "withdraw",
            100,
        ),
        Err(SessionError::Unavailable)
    );

    let Some(denial_repo) = repository() else {
        return;
    };
    let denied = create_pending_connector_session(&denial_repo, "connector-denied-consent");
    let denied_binding = match denied.checkpoint.pending.as_ref().unwrap() {
        PendingInteraction::ConnectorConsent(binding) => binding.clone(),
        _ => unreachable!(),
    };
    assert_eq!(
        denial_repo.operator_decide_connector_consent(
            &owner(),
            &denied.checkpoint.snapshot.session,
            &owner().principal,
            "unknown",
            100,
        ),
        Err(SessionError::InvalidInput)
    );
    assert_eq!(
        denial_repo.operator_decide_connector_consent(
            &owner(),
            &denied.checkpoint.snapshot.session,
            &owner().principal,
            "deny",
            100,
        ),
        Ok(None)
    );
    let denied_current = denial_repo
        .load(&owner(), &denied.checkpoint.snapshot.session)
        .unwrap();
    assert_eq!(
        denied_current.checkpoint.snapshot.state,
        SessionState::Failed
    );
    assert_eq!(
        denied_current.checkpoint.snapshot.dispatch_knowledge,
        DispatchKnowledge::Stopped
    );
    let denied_reference = connector_consent_record_reference(&denied_binding).unwrap();
    assert_eq!(
        denial_repo
            .connector_consent(&owner(), &denied_reference)
            .unwrap(),
        ConnectorConsentStatus::Denied
    );

    let Some(duplicate_repo) = repository() else {
        return;
    };
    let approved = create_pending_connector_session(&duplicate_repo, "connector-duplicate-consent");
    duplicate_repo
        .operator_decide_connector_consent(
            &owner(),
            &approved.checkpoint.snapshot.session,
            &owner().principal,
            "approve",
            100,
        )
        .unwrap();
    assert_eq!(
        duplicate_repo.operator_decide_connector_consent(
            &owner(),
            &approved.checkpoint.snapshot.session,
            &owner().principal,
            "approve",
            101,
        ),
        Err(SessionError::Duplicate)
    );
}

struct RestartRuntime {
    dispatches: AtomicUsize,
}

impl ConnectorSessionRuntimePort for RestartRuntime {
    fn initial_assessment(
        &self,
        checkpoint: &SessionCheckpoint,
    ) -> Result<InitialAssessment, SessionError> {
        checkpoint
            .initial_assessment
            .clone()
            .ok_or(SessionError::InvalidState)
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
        self.dispatches.fetch_add(1, Ordering::SeqCst);
        Err(SessionError::UnsupportedCapability)
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

struct RestartHost {
    runtime: RestartRuntime,
}

impl InteractionAuthorityPort for RestartHost {
    fn load_consent(
        &self,
        _: &OwnerBinding,
        _: &ConsentRecordRef,
    ) -> Result<ConsentStatus, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }

    fn live_status(&self, _: &VerifiedConsent) -> Result<ConsentStatus, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}

impl ArtifactStoragePort for RestartHost {
    fn load_artifact(&self, _: &OwnerBinding, _: &RecordRef) -> Result<Vec<u8>, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}

impl StructuredSessionHost for RestartHost {
    fn now_ms(&self) -> Result<u64, SessionError> {
        Ok(100)
    }

    fn register(
        &self,
        _: &OwnerBinding,
        _: gateway_domain::Intent,
        _: RequestedExecution,
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }

    fn prepare(&self, _: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }

    fn connector_runtime(&self) -> Result<&dyn ConnectorSessionRuntimePort, SessionError> {
        Ok(&self.runtime)
    }

    fn disclose(&self, _: &OwnerBinding, _: &SessionId) -> Result<(), SessionError> {
        Ok(())
    }

    fn issuer(&self, _: &OwnerBinding, _: &PrincipalId) -> Result<(), SessionError> {
        Ok(())
    }
}

/// Uses two separately launched instances of this test process against the
/// disposable PostgreSQL host. This qualifies process restart only; the host
/// database server itself is not restarted here.
#[test]
fn postgres_connector_records_reload_after_process_restart_without_redispatch() {
    const PHASE: &str = "CG_SESSION_RESTART_QUALIFICATION_PHASE";
    const SCOPE: &str = "CG_SESSION_RESTART_QUALIFICATION_SCOPE";
    let Some(connection) = std::env::var_os("CG_COGNITIVE_TEST_DATABASE") else {
        return;
    };
    if let Ok(phase) = std::env::var(PHASE) {
        let scope = std::env::var(SCOPE).unwrap();
        let repo = repository_for_scope(&scope);
        match phase.as_str() {
            "seed" => {
                let pending = create_pending_connector_session(&repo, "restart-connector");
                let session = pending.checkpoint.snapshot.session.clone();
                let accepted = repo
                    .operator_decide_connector_consent(
                        &owner(),
                        &session,
                        &owner().principal,
                        "approve",
                        100,
                    )
                    .unwrap()
                    .unwrap();
                let binding = match pending.checkpoint.pending.as_ref().unwrap() {
                    PendingInteraction::ConnectorConsent(binding) => binding.as_ref().clone(),
                    _ => unreachable!(),
                };
                let current = repo.load(&owner(), &session).unwrap();
                let mut reserve = append(&current);
                let checkpoint = &mut reserve.next;
                checkpoint.snapshot.state = SessionState::Dispatching;
                checkpoint.snapshot.pending = None;
                checkpoint.snapshot.dispatch_knowledge = DispatchKnowledge::Reserved;
                checkpoint.pending = None;
                checkpoint.lease_until_ms = Some(30_000);
                checkpoint.accepted_connector_consent = Some(AcceptedConnectorConsent {
                    record: accepted,
                    binding: binding.clone(),
                });
                checkpoint.budget.reserve(100, false).unwrap();

                let snapshot_binding = ConnectorStepBinding {
                    owner: checkpoint.owner.clone(),
                    scope: binding.scope.clone(),
                    session: checkpoint.snapshot.session.clone(),
                    run: checkpoint.snapshot.run.clone(),
                    revision: checkpoint.snapshot.revision,
                    task: TaskId::new("connector-task").unwrap(),
                    step: binding.step.clone(),
                    dispatch: binding.dispatch.clone(),
                    invocation: binding.resume.invocation.clone(),
                };
                let snapshot_payload = ScopedConnectorSnapshotPayload::new(
                    binding.scope.clone(),
                    br#"{"snapshot":"before-restart"}"#.to_vec(),
                )
                .unwrap();
                let snapshot_bytes =
                    before_snapshot_record(&snapshot_binding, &snapshot_payload).unwrap();
                let snapshot_ref = content_reference("restart-before", &snapshot_bytes).unwrap();
                checkpoint
                    .connector_step_snapshots
                    .push(ConnectorStepSnapshotRef {
                        binding: snapshot_binding.clone(),
                        phase: ConnectorSnapshotPhase::Before,
                        record: snapshot_ref.clone(),
                    });
                reserve.records.push(RecordWrite {
                    reference: snapshot_ref,
                    kind: RecordKind::ConnectorSnapshot,
                    bytes: snapshot_bytes,
                });
                let reserved = repo.append(&owner(), reserve).unwrap();

                let mut uncertain = append(&reserved);
                uncertain.next.snapshot.state = SessionState::OutcomeUnknown;
                uncertain.next.snapshot.dispatch_knowledge = DispatchKnowledge::Unknown;
                uncertain.next.lease_until_ms = None;
                let mut after_binding = snapshot_binding;
                after_binding.revision = uncertain.next.snapshot.revision;
                let after_payload = ScopedConnectorSnapshotPayload::new(
                    binding.scope.clone(),
                    br#"{"snapshot":"after-restart-unknown"}"#.to_vec(),
                )
                .unwrap();
                let after_bytes = after_snapshot_record(&after_binding, &after_payload).unwrap();
                let after_ref = content_reference("restart-after", &after_bytes).unwrap();
                uncertain
                    .next
                    .connector_step_snapshots
                    .push(ConnectorStepSnapshotRef {
                        binding: after_binding.clone(),
                        phase: ConnectorSnapshotPhase::After,
                        record: after_ref.clone(),
                    });
                let result_bytes = serde_json::to_vec(&serde_json::json!({
                    "contract":"cg.connector-proposal-batch-result",
                    "contract_version":"1.0",
                    "binding":&after_binding,
                    "payload":{"effect":"unknown","after_snapshot_state":"captured"}
                }))
                .unwrap();
                let result_ref = content_reference("restart-result", &result_bytes).unwrap();
                uncertain.next.connector_results.push(ConnectorResultRef {
                    binding: after_binding.clone(),
                    record: result_ref.clone(),
                    after_snapshot_state: ConnectorAfterSnapshotState::Captured,
                });
                let observation_bytes = serde_json::to_vec(&serde_json::json!({
                    "contract":"cg.connector-observation",
                    "binding":&after_binding,
                    "payload":{"state":"independently-read"}
                }))
                .unwrap();
                let observation_ref =
                    content_reference("restart-observation", &observation_bytes).unwrap();
                uncertain
                    .next
                    .connector_observations
                    .push(ConnectorObservationRef {
                        binding: after_binding,
                        proposal_ordinal: Some(0),
                        goal_condition: None,
                        role: ConnectorObservationRole::PostEffectReadback,
                        record: observation_ref.clone(),
                    });
                uncertain.records.extend([
                    RecordWrite {
                        reference: after_ref,
                        kind: RecordKind::ConnectorSnapshot,
                        bytes: after_bytes,
                    },
                    RecordWrite {
                        reference: result_ref,
                        kind: RecordKind::ConnectorResult,
                        bytes: result_bytes,
                    },
                    RecordWrite {
                        reference: observation_ref,
                        kind: RecordKind::ConnectorObservation,
                        bytes: observation_bytes,
                    },
                ]);
                repo.append(&owner(), uncertain).unwrap();
            }
            "recover" => {
                let session = SessionId::new("restart-connector").unwrap();
                let current = repo.load(&owner(), &session).unwrap();
                assert_eq!(
                    current.checkpoint.snapshot.state,
                    SessionState::OutcomeUnknown
                );
                assert_eq!(
                    current.checkpoint.snapshot.dispatch_knowledge,
                    DispatchKnowledge::Unknown
                );
                assert_eq!(current.checkpoint.connector_step_snapshots.len(), 2);
                for snapshot in &current.checkpoint.connector_step_snapshots {
                    assert_eq!(
                        repo.record_kind(&owner(), &snapshot.record).unwrap(),
                        RecordKind::ConnectorSnapshot
                    );
                    let bytes = repo.load_artifact(&owner(), &snapshot.record).unwrap();
                    let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(
                        record["binding"]["owner"],
                        serde_json::to_value(owner()).unwrap()
                    );
                    assert_eq!(record["binding"]["scope"], scope);
                    assert_eq!(record["binding"]["session"], session.as_str());
                    assert_eq!(
                        record["binding"]["run"],
                        current.checkpoint.snapshot.run.as_str()
                    );
                    assert_eq!(
                        record["binding"]["revision"],
                        snapshot.binding.revision.value()
                    );
                    assert_eq!(record["binding"]["step"], snapshot.binding.step.as_str());
                    assert_eq!(
                        record["binding"]["dispatch"],
                        snapshot.binding.dispatch.as_str()
                    );
                }
                assert_eq!(current.checkpoint.connector_results.len(), 1);
                assert_eq!(
                    repo.record_kind(&owner(), &current.checkpoint.connector_results[0].record)
                        .unwrap(),
                    RecordKind::ConnectorResult
                );
                assert_eq!(current.checkpoint.connector_observations.len(), 1);
                assert_eq!(
                    repo.record_kind(
                        &owner(),
                        &current.checkpoint.connector_observations[0].record
                    )
                    .unwrap(),
                    RecordKind::ConnectorObservation
                );

                let pending = &current
                    .checkpoint
                    .accepted_connector_consent
                    .as_ref()
                    .unwrap()
                    .binding;
                let proposal = &pending.resume.output_record;
                assert_eq!(
                    repo.record_kind(&owner(), proposal).unwrap(),
                    RecordKind::ConnectorProposal
                );
                let proposal_bytes = repo.load_artifact(&owner(), proposal).unwrap();
                let material: ConnectorConsentResumeMaterial =
                    serde_json::from_slice(&proposal_bytes).unwrap();
                material.matches(&pending.resume).unwrap();
                let consent = &current
                    .checkpoint
                    .accepted_connector_consent
                    .as_ref()
                    .unwrap()
                    .record;
                let ConnectorConsentStatus::Granted(grant) =
                    repo.connector_consent(&owner(), consent).unwrap()
                else {
                    panic!("persisted consent grant missing after process restart")
                };
                assert_eq!(grant.binding(), pending);
                let mut foreign = owner();
                foreign.project = ProjectId::new("another-project").unwrap();
                assert_eq!(
                    repo.load(&foreign, &session),
                    Err(SessionError::ScopeDenied)
                );
                assert_eq!(
                    repo.load_artifact(&foreign, proposal),
                    Err(SessionError::Unavailable)
                );
                assert_eq!(
                    repo.connector_consent(&foreign, consent),
                    Err(SessionError::Unavailable)
                );

                let dispatches = RestartRuntime {
                    dispatches: AtomicUsize::new(0),
                };
                let reopened = repository_for_scope(repo.store.scope().as_str());
                let coordinator = SessionCoordinator::new(
                    reopened,
                    RestartHost {
                        runtime: dispatches,
                    },
                );
                let recovered = coordinator.recover(&owner(), &session).unwrap();
                assert_eq!(recovered.state, SessionState::OutcomeUnknown);
                assert_eq!(recovered.revision, current.checkpoint.snapshot.revision);
                assert_eq!(repo.load(&owner(), &session).unwrap(), current);
                assert_eq!(
                    coordinator.host.runtime.dispatches.load(Ordering::SeqCst),
                    0
                );
            }
            _ => panic!("unknown process-restart qualification phase"),
        }
        return;
    }

    let scope = format!(
        "process-restart-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let executable = std::env::current_exe().unwrap();
    for phase in ["seed", "recover"] {
        let status = Command::new(&executable)
            .args([
                "--exact",
                "session_store::tests::postgres_connector_records_reload_after_process_restart_without_redispatch",
                "--nocapture",
            ])
            .env(PHASE, phase)
            .env(SCOPE, &scope)
            .env("CG_COGNITIVE_TEST_DATABASE", &connection)
            .status()
            .unwrap();
        assert!(status.success(), "child process phase {phase} failed");
    }
}

#[test]
fn postgres_atomic_append_rejects_invalid_records_and_keeps_predecessor() {
    let Some(repo) = repository() else {
        return;
    };
    let cp = checkpoint(&repo, "atomic");
    let mut foreign = cp.clone();
    let mut basis = foreign.goal.as_artifact().unwrap().basis().clone();
    basis.scope = ContextScopeId::new("foreign").unwrap();
    foreign.goal = SupportedSessionGoal::artifact(
        SupportedArtifactGoal::validate(foreign.goal.intent().clone(), basis).unwrap(),
    );
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
                SessionError::Duplicate
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

#[test]
fn postgres_connector_producer_inputs_commit_with_pending_and_survive_accepted_consent() {
    let Some(repo) = repository() else { return };
    let (initial, transition) = pending_connector_input_append(&repo, "producer-input-positive");
    let input = transition.records[0].clone();
    let output = transition.records[1].clone();
    let committed = repo.append(&initial.checkpoint.owner, transition).unwrap();
    assert!(committed.checkpoint.artifact.is_none());
    assert!(committed.checkpoint.connector_step_snapshots.is_empty());
    let reader = repository_for_scope(repo.store.scope().as_str());
    assert_eq!(
        reader
            .load(&owner(), &committed.checkpoint.snapshot.session)
            .unwrap(),
        committed
    );
    assert_eq!(
        reader.record_kind(&owner(), &input.reference).unwrap(),
        RecordKind::ConnectorProducerInput
    );
    assert_eq!(
        reader.load_artifact(&owner(), &input.reference).unwrap(),
        input.bytes
    );
    assert_eq!(
        reader.record_kind(&owner(), &output.reference).unwrap(),
        RecordKind::ConnectorProposal
    );
    assert_eq!(
        content_reference(
            input.reference.id().as_str(),
            &reader.load_artifact(&owner(), &input.reference).unwrap()
        )
        .unwrap(),
        input.reference
    );
    let grant_ref = repo
        .operator_decide_connector_consent(
            &owner(),
            &committed.checkpoint.snapshot.session,
            &owner().principal,
            "approve",
            100,
        )
        .unwrap()
        .unwrap();
    let ConnectorConsentStatus::Granted(grant) =
        repo.connector_consent(&owner(), &grant_ref).unwrap()
    else {
        panic!("actual stored grant missing")
    };
    let mut accepted = append(&committed);
    accepted.next.accepted_connector_consent = Some(AcceptedConnectorConsent {
        record: grant_ref,
        binding: grant.binding().clone(),
    });
    accepted.next.pending = None;
    accepted.next.snapshot.pending = None;
    accepted.next.snapshot.state = SessionState::Runnable;
    let accepted = repo.append(&owner(), accepted).unwrap();
    assert_eq!(
        reader
            .load(&owner(), &accepted.checkpoint.snapshot.session)
            .unwrap(),
        accepted
    );
    assert_eq!(
        reader.load_artifact(&owner(), &input.reference).unwrap(),
        input.bytes
    );
    let mut foreign = owner();
    foreign.project = ProjectId::new("foreign-project").unwrap();
    assert!(reader.load_artifact(&foreign, &input.reference).is_err());
    assert!(reader.record_kind(&foreign, &input.reference).is_err());
}

#[test]
fn postgres_connector_producer_input_failures_roll_back_all_records_and_checkpoint() {
    let Some(repo) = repository() else { return };
    for fault in [
        "missing",
        "artifact",
        "snapshot",
        "proposal",
        "corrupt-hash",
        "orphan",
        "foreign-owner",
    ] {
        let (initial, mut transition) =
            pending_connector_input_append(&repo, &format!("producer-input-{fault}"));
        let expected_records = transition.records.clone();
        let mut append_owner = owner();
        match fault {
            "missing" => {
                transition.records.remove(0);
            }
            "artifact" => transition.records[0].kind = RecordKind::Artifact,
            "snapshot" => transition.records[0].kind = RecordKind::ConnectorSnapshot,
            "proposal" => transition.records[0].kind = RecordKind::ConnectorProposal,
            "corrupt-hash" => transition.records[0].bytes.push(b'!'),
            "orphan" => {
                let bytes = b"not referenced by the consent".to_vec();
                transition.records.push(RecordWrite {
                    reference: content_reference("orphan-producer-input", &bytes).unwrap(),
                    kind: RecordKind::ConnectorProducerInput,
                    bytes,
                });
            }
            _ => append_owner.client_owner = ClientOwnerId::new("foreign-client").unwrap(),
        }
        let attempted_records = transition.records.clone();
        assert!(repo.append(&append_owner, transition).is_err(), "{fault}");
        assert_eq!(
            repo.load(&owner(), &initial.checkpoint.snapshot.session)
                .unwrap(),
            initial,
            "{fault}"
        );
        for record in expected_records.into_iter().chain(attempted_records) {
            assert!(
                repo.load_artifact(&owner(), &record.reference).is_err(),
                "{fault}"
            );
            assert!(
                repo.load_artifact(&append_owner, &record.reference)
                    .is_err(),
                "{fault}"
            );
        }
    }
}

#[test]
fn postgres_connector_producer_input_links_reject_wrong_owner_kind_and_missing_stored_pin() {
    let Some(repo) = repository() else { return };
    let committed = create_pending_connector_session(&repo, "producer-input-integrity");
    let binding = match committed.checkpoint.pending.as_ref().unwrap() {
        PendingInteraction::ConnectorConsent(binding) => binding,
        _ => unreachable!(),
    };
    let input = binding.resume.producer_pins[0].clone();
    for fault in ["missing", "kind", "owner", "hash"] {
        repo.run(|journal| {
            let index = journal
                .records
                .iter()
                .position(|r| r.reference == input && r.owner == owner())
                .unwrap();
            let original = &journal.records[index];
            let stored = Record {
                owner: original.owner.clone(),
                reference: original.reference.clone(),
                kind: original.kind,
                bytes: original.bytes.clone(),
            };
            match fault {
                "missing" => {
                    journal.records.remove(index);
                }
                "kind" => journal.records[index].kind = RecordKind::Artifact,
                "owner" => {
                    journal.records[index].owner.project =
                        ProjectId::new("foreign-project").unwrap()
                }
                _ => journal.records[index].bytes.push(b'!'),
            }
            assert_eq!(
                journal.validate(repo.store.scope()),
                Err(SessionError::StorageUnavailable),
                "{fault}"
            );
            if fault == "missing" {
                journal.records.insert(index, stored);
            } else {
                journal.records[index] = stored;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(
            repo.load(&owner(), &committed.checkpoint.snapshot.session)
                .unwrap(),
            committed
        );
    }
}

#[path = "../../gateway-application/tests/support/pre_execution_goal.rs"]
mod observed_goal_fixture;

fn observed_goal_plan() -> SessionResourcePlan {
    SessionResourcePlan {
        preparation: SessionResourceAmounts {
            connector_calls: 2,
            retrieval_rounds: 2,
            retrieval_results: 2,
            retrieval_tokens: 4096,
            ..Default::default()
        },
        ..Default::default()
    }
}
struct ObservedGoalRuntime {
    rules: gateway_application::closed_loop::LoopRules,
    task: TaskId,
    execution: gateway_domain::NonEmptyText,
    context_digest: ContentDigest,
    unavailable_verifier: bool,
    preparations: AtomicUsize,
    effects: AtomicUsize,
}
impl ConnectorSessionRuntimePort for ObservedGoalRuntime {
    fn resource_plan(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
    ) -> Result<SessionResourcePlan, SessionError> {
        Ok(observed_goal_plan())
    }
    fn initial_assessment(
        &self,
        cp: &SessionCheckpoint,
    ) -> Result<InitialAssessment, SessionError> {
        cp.initial_assessment
            .clone()
            .ok_or(SessionError::InvalidState)
    }
    fn prepare_step(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
    ) -> Result<PreparedConnectorTurn, SessionError> {
        panic!("observed goal must not prepare an invocation")
    }
    fn prepare_authorized_step(
        &self,
        cp: &SessionCheckpoint,
        _: &PlanStepId,
        resources: &mut SessionPhaseResources,
        _: ConnectorPreparationContext<'_>,
    ) -> Result<ConnectorPreparation, SessionError> {
        self.preparations.fetch_add(1, Ordering::SeqCst);
        let allocation = resources.allocate(observed_goal_plan().preparation)?;
        assert_eq!(allocation.phase(), SessionResourcePhase::Preparation);
        let binding = PreExecutionGoalBinding {
            acquisition: allocation.binding().clone(),
            reservation: allocation.reservation_id().clone(),
            claim_revision: cp.snapshot.revision,
            task: self.task.clone(),
            execution: self.execution.clone(),
            context_digest: self.context_digest.clone(),
        };
        let goal = cp.goal.as_connector().unwrap();
        let data = observed_goal_fixture::data(goal, binding, true, 100_000);
        let candidate = observed_goal_fixture::candidate(data, goal, self.rules.clone())
            .ok_or(SessionError::InvalidState)?;
        Ok(ConnectorPreparation::GoalAlreadySatisfied(Box::new(
            candidate,
        )))
    }
    fn verify_pre_execution_goal(
        &self,
        cp: &SessionCheckpoint,
        data: &PreExecutionGoalData,
    ) -> Result<Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>, SessionError>
    {
        if self.unavailable_verifier {
            return Err(SessionError::StorageUnavailable);
        }
        if data.definition_pins() != observed_goal_fixture::definitions(cp.goal.scope()) {
            return Err(SessionError::StaleRevision);
        }
        gateway_application::closed_loop::ClosedLoop::verify_retained(
            data.execution()?,
            cp.goal.scope().clone(),
            cp.goal.intent().clone(),
            data.readset(),
            self.rules.clone(),
        )
        .map_err(|_| SessionError::InvalidState)
    }
    fn execute_step(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
        _: &mut SessionPhaseResources,
        _: &DispatchId,
        _: &PreparedConnectorTurn,
    ) -> Result<ConnectorSessionTurn, SessionError> {
        self.effects.fetch_add(1, Ordering::SeqCst);
        panic!("an already satisfied complete goal must not dispatch")
    }
    fn verify_goal(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
        _: &DispatchId,
        _: &[PersistedConnectorReadback],
    ) -> Result<Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>, SessionError>
    {
        panic!("invocation-free proof must use its owning verifier")
    }
}
struct ObservedGoalHost {
    runtime: ObservedGoalRuntime,
}
impl InteractionAuthorityPort for ObservedGoalHost {
    fn load_consent(
        &self,
        _: &OwnerBinding,
        _: &ConsentRecordRef,
    ) -> Result<ConsentStatus, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn live_status(&self, _: &VerifiedConsent) -> Result<ConsentStatus, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}
impl ArtifactStoragePort for ObservedGoalHost {
    fn load_artifact(&self, _: &OwnerBinding, _: &RecordRef) -> Result<Vec<u8>, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}
impl StructuredSessionHost for ObservedGoalHost {
    fn now_ms(&self) -> Result<u64, SessionError> {
        Ok(100_000)
    }
    fn resource_limits(
        &self,
        _: &OwnerBinding,
        _: &SupportedSessionGoal,
    ) -> Result<Option<SessionResourceLimits>, SessionError> {
        Ok(Some(SessionResourceLimits {
            cost_unit: ReferenceId::new("credits").unwrap(),
            amounts: observed_goal_plan().total()?,
        }))
    }
    fn register(
        &self,
        _: &OwnerBinding,
        _: gateway_domain::Intent,
        _: RequestedExecution,
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn prepare(&self, _: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        panic!("no model or artifact preparation is used")
    }
    fn connector_runtime(&self) -> Result<&dyn ConnectorSessionRuntimePort, SessionError> {
        Ok(&self.runtime)
    }
    fn disclose(&self, actual: &OwnerBinding, _: &SessionId) -> Result<(), SessionError> {
        if actual == &owner() {
            Ok(())
        } else {
            Err(SessionError::ScopeDenied)
        }
    }
    fn issuer(&self, _: &OwnerBinding, _: &PrincipalId) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}
// Actual native CLI planning, CG09 and CG10 supply the readonly source task,
// execution/context digest and LoopRules; the receipt is never constructed here.
fn observed_goal_initial(
    repo: &PostgresSessionStore,
    id: &str,
) -> (FencedSession, ObservedGoalHost) {
    let scope = repo.store.scope().clone();
    let registered = observed_goal_fixture::goal(scope.clone());
    let root = std::env::temp_dir().join(format!(
        "cg-pg-pre-goal-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let native = crate::declarative_cli::host_mapping::native_producer_fixture::real_plan(
        &root,
        &scope,
        registered.intent(),
    )
    .unwrap();
    let command = crate::declarative_cli::host_mapping::compile(
        native.resolved.clone(),
        native.policy.clone(),
        native.producer_projection.clone(),
    )
    .unwrap();
    let compiled = gateway_application::context_application::ContextApplication
        .compile_step(gateway_application::context_application::CompileStepInput {
            resolved: &command.resolved,
            authority: &command.authority,
            policy_context: &command.policy_context,
            catalog: &command.catalog,
            projection: &command.projection,
            candidates: &command.candidates,
            selected: &command.selected,
        })
        .unwrap();
    let context = compiled.to_json().unwrap();
    let execution =
        gateway_domain::NonEmptyText::new(compiled.context().execution_context().id().as_str())
            .unwrap();
    let goal = SupportedConnectorGoal::from_host_registration(
        registered.intent().clone(),
        scope.clone(),
        content_reference(
            "native-observed-goal-plan",
            &serde_json::to_vec(&native.plan).unwrap(),
        )
        .unwrap(),
        vec![native.producer_step.clone(), native.action_step.clone()],
        registered.observations().to_vec(),
    )
    .unwrap();
    let rules = gateway_application::closed_loop::LoopRules {
        capabilities: gateway_application::PlanningCapabilitySnapshot::new(
            native.resolved.snapshot.input().index.clone(),
            native.resolved.report.basis.registry_fingerprint.as_str(),
            gateway_domain::PlanningIrVersion::V1,
        )
        .unwrap(),
        requirements: gateway_domain::CapabilityRequirementRules::default(),
        planner: gateway_domain::PlannerRules::default(),
        max_iterations: 2,
        max_retries: 0,
    };
    let host = ObservedGoalHost {
        runtime: ObservedGoalRuntime {
            rules,
            task: native.task,
            execution,
            context_digest: gateway_domain::context_content_digest(context.as_bytes()),
            unavailable_verifier: true,
            preparations: AtomicUsize::new(0),
            effects: AtomicUsize::new(0),
        },
    };
    std::fs::remove_dir_all(root).unwrap();
    let mut cp = checkpoint(repo, id);
    cp.goal = SupportedSessionGoal::connector(goal);
    cp.execution.mode = native.resolved.snapshot.input().operating_mode;
    cp.execution.profile = native.resolved.snapshot.input().execution_profile;
    cp.initial_assessment.as_mut().unwrap().goal = content_reference(
        "supported-goal",
        &serde_json::to_vec(cp.goal.intent()).unwrap(),
    )
    .unwrap();
    cp.resource_ledger = Some(
        SessionResourceLedger::new(host.resource_limits(&owner(), &cp.goal).unwrap().unwrap())
            .unwrap(),
    );
    let initial = repo.create(&CommandId::new(id).unwrap(), cp).unwrap();
    (initial, host)
}
fn observed_goal_marker(
    repo: &PostgresSessionStore,
    id: &str,
) -> (FencedSession, ObservedGoalHost) {
    let (initial, host) = observed_goal_initial(repo, id);
    let coordinator =
        SessionCoordinator::new(repository_for_scope(repo.store.scope().as_str()), host);
    assert_eq!(
        coordinator.execute(
            &owner(),
            SessionCommand::Continue {
                at: Mutation {
                    session: initial.checkpoint.snapshot.session.clone(),
                    command: CommandId::new(format!("continue-{id}")).unwrap(),
                    expected_revision: initial.checkpoint.snapshot.revision
                }
            }
        ),
        Err(SessionError::StorageUnavailable)
    );
    let marker = repo
        .load(&owner(), &initial.checkpoint.snapshot.session)
        .unwrap();
    assert_eq!(marker.checkpoint.snapshot.state, SessionState::Runnable);
    assert!(marker.checkpoint.pre_execution_goal.is_some());
    assert_eq!(
        coordinator.host.runtime.preparations.load(Ordering::SeqCst),
        1
    );
    assert_eq!(coordinator.host.runtime.effects.load(Ordering::SeqCst), 0);
    assert!(marker.checkpoint.connector_step_snapshots.is_empty());
    assert!(marker.checkpoint.connector_results.is_empty());
    (marker, coordinator.host)
}

#[test]
fn postgres_pre_execution_goal_reopens_full_native_readset_and_rechecks_actual_age() {
    let Some(repo) = repository() else { return };
    let (marker, mut host) = observed_goal_marker(&repo, "pre-goal-roundtrip");
    let reader = repository_for_scope(repo.store.scope().as_str());
    let reopened = reader
        .load(&owner(), &marker.checkpoint.snapshot.session)
        .unwrap();
    assert_eq!(reopened, marker);
    let reference = reopened.checkpoint.pre_execution_goal.as_ref().unwrap();
    assert_eq!(
        reader.record_kind(&owner(), &reference.record).unwrap(),
        RecordKind::PreExecutionGoal
    );
    let data = load_pre_execution_goal(&reader, &reopened.checkpoint, 100_001).unwrap();
    assert_eq!(data.reads().len(), 2);
    assert_eq!(data.readset().digest(), &reference.readset_digest);
    assert!(
        load_pre_execution_goal(&reader, &reopened.checkpoint, reference.valid_until_ms).is_err()
    );
    host.runtime.unavailable_verifier = false;
    let proof = host
        .runtime
        .verify_pre_execution_goal(&reopened.checkpoint, &data)
        .unwrap()
        .unwrap();
    assert_eq!(proof.goal_digest(), &reference.goal_digest);
    assert_eq!(proof.readset_digest(), Some(&reference.readset_digest));
    let coordinator = SessionCoordinator::new(reader, host);
    let complete = coordinator
        .recover(&owner(), &marker.checkpoint.snapshot.session)
        .unwrap();
    assert_eq!(complete.state, SessionState::Completed);
    assert_eq!(
        coordinator.host.runtime.preparations.load(Ordering::SeqCst),
        1
    );
    assert_eq!(coordinator.host.runtime.effects.load(Ordering::SeqCst), 0);
}

#[test]
fn postgres_pre_execution_goal_storage_integrity_and_legacy_wire_remain_fail_closed() {
    let Some(repo) = repository() else { return };
    let (marker, _) = observed_goal_marker(&repo, "pre-goal-integrity");
    let reference = marker
        .checkpoint
        .pre_execution_goal
        .as_ref()
        .unwrap()
        .record
        .clone();
    for fault in ["missing", "kind", "owner", "hash"] {
        repo.run(|journal| {
            let index = journal
                .records
                .iter()
                .position(|r| r.reference == reference && r.owner == owner())
                .unwrap();
            let original = &journal.records[index];
            let saved = Record {
                owner: original.owner.clone(),
                reference: original.reference.clone(),
                kind: original.kind,
                bytes: original.bytes.clone(),
            };
            match fault {
                "missing" => {
                    journal.records.remove(index);
                }
                "kind" => journal.records[index].kind = RecordKind::Artifact,
                "owner" => {
                    journal.records[index].owner.project =
                        ProjectId::new("foreign-project").unwrap()
                }
                _ => journal.records[index].bytes.push(b'!'),
            }
            assert_eq!(
                journal.validate(repo.store.scope()),
                Err(SessionError::StorageUnavailable),
                "{fault}"
            );
            if fault == "missing" {
                journal.records.insert(index, saved);
            } else {
                journal.records[index] = saved;
            }
            Ok(())
        })
        .unwrap();
        assert!(load_pre_execution_goal(&repo, &marker.checkpoint, 100_001).is_ok());
    }
    let mut foreign = owner();
    foreign.project = ProjectId::new("foreign-project").unwrap();
    assert!(repo.load_artifact(&foreign, &reference).is_err());
    let legacy = checkpoint(&repo, "pre-goal-legacy");
    let mut wire = serde_json::to_value(Checkpoint::from_typed(&legacy)).unwrap();
    assert!(wire.get("pre_execution_goal").is_none());
    let restored: Checkpoint = serde_json::from_value(wire.clone()).unwrap();
    assert!(restored.typed().unwrap().pre_execution_goal.is_none());
    wire["pre_execution_goal"] = serde_json::Value::Null;
    let explicit: Checkpoint = serde_json::from_value(wire).unwrap();
    assert!(explicit.typed().unwrap().pre_execution_goal.is_none());
}

/// Fault injection receives the real coordinator-created transition after its
/// native candidate check; it cannot construct a candidate or goal reference.
struct PreGoalFaultRepository<'a> {
    store: &'a PostgresSessionStore,
    fault: &'static str,
    predecessor: std::cell::RefCell<Option<FencedSession>>,
    record: std::cell::RefCell<Option<RecordRef>>,
}
impl ArtifactStoragePort for PreGoalFaultRepository<'_> {
    fn load_artifact(&self, owner: &OwnerBinding, r: &RecordRef) -> Result<Vec<u8>, SessionError> {
        self.store.load_artifact(owner, r)
    }
}
impl SessionRepositoryPort for PreGoalFaultRepository<'_> {
    fn record_kind(&self, owner: &OwnerBinding, r: &RecordRef) -> Result<RecordKind, SessionError> {
        self.store.record_kind(owner, r)
    }
}
impl SessionJournalPort for PreGoalFaultRepository<'_> {
    fn load(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
    ) -> Result<FencedSession, SessionError> {
        self.store.load(owner, session)
    }
    fn command_outcome(&self, key: &CommandKey) -> Result<CommandOutcome, SessionError> {
        self.store.command_outcome(key)
    }
    fn create(
        &self,
        command: &CommandId,
        cp: SessionCheckpoint,
    ) -> Result<FencedSession, SessionError> {
        self.store.create(command, cp)
    }
    fn append(
        &self,
        owner: &OwnerBinding,
        mut transition: JournalAppend,
    ) -> Result<FencedSession, SessionError> {
        if let Some(reference) = &transition.next.pre_execution_goal {
            *self.predecessor.borrow_mut() = Some(self.store.load(owner, &transition.session)?);
            *self.record.borrow_mut() = Some(reference.record.clone());
            match self.fault {
                "missing" => transition.records.clear(),
                "kind" => transition.records[0].kind = RecordKind::Artifact,
                "hash" => transition.records[0].bytes.push(b'!'),
                _ => {
                    let mut foreign = owner.clone();
                    foreign.project = ProjectId::new("foreign-project").unwrap();
                    return self.store.append(&foreign, transition);
                }
            }
        }
        self.store.append(owner, transition)
    }
}
#[test]
fn postgres_pre_execution_goal_record_faults_roll_back_the_actual_native_transition() {
    let Some(repo) = repository() else { return };
    for fault in ["missing", "kind", "hash", "owner"] {
        let (initial, host) = observed_goal_initial(&repo, &format!("pre-goal-rollback-{fault}"));
        let faulting = PreGoalFaultRepository {
            store: &repo,
            fault,
            predecessor: Default::default(),
            record: Default::default(),
        };
        let coordinator = SessionCoordinator::new(faulting, host);
        assert!(
            coordinator
                .execute(
                    &owner(),
                    SessionCommand::Continue {
                        at: Mutation {
                            session: initial.checkpoint.snapshot.session.clone(),
                            command: CommandId::new(format!("pre-goal-fault-{fault}")).unwrap(),
                            expected_revision: initial.checkpoint.snapshot.revision
                        }
                    }
                )
                .is_err()
        );
        let predecessor = coordinator.repository.predecessor.borrow();
        let predecessor = predecessor
            .as_ref()
            .expect("actual candidate transition was attempted");
        assert_eq!(
            predecessor.checkpoint.snapshot.state,
            SessionState::Preparing
        );
        assert!(predecessor.checkpoint.pre_execution_goal.is_none());
        assert_eq!(
            repo.load(&owner(), &initial.checkpoint.snapshot.session)
                .unwrap(),
            *predecessor,
            "{fault}"
        );
        let reference = coordinator.repository.record.borrow();
        assert!(
            repo.load_artifact(&owner(), reference.as_ref().unwrap())
                .is_err(),
            "{fault}"
        );
        assert_eq!(coordinator.host.runtime.effects.load(Ordering::SeqCst), 0);
    }
}
