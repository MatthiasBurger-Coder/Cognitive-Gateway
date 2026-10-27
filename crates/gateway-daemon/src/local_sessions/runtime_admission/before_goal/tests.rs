use super::*;
use crate::local_sessions::runtime_admission::{
    model_factory::tests::{PEER, http_provider, install},
    preparing_test_support::NativeConsentPreparingProbe,
    target_factory::PreparedTarget,
    tests::Fixture,
};
use crate::local_sessions::*;
use std::cell::Cell;

#[test]
fn actual_mcp_goal_read_completes_via_postgres_without_model_or_mutation() {
    let Ok(connection) = std::env::var("CG_COGNITIVE_TEST_DATABASE") else {
        return;
    };
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut f = Fixture::with_scope(&format!("native-no-model-goal-{nonce}"));
    let native = f.install_actual_mixed_model();
    let provider = http_provider();
    let missing = format!("CG_ABSENT_GOAL_MODEL_CREDENTIAL_{nonce}");
    install(&mut f, provider.endpoint(), Some(&missing), &native);
    let peer = f.root.join("read-only-peer.py");
    std::fs::write(&peer, PEER.replace("'second':False", "'second':True")).unwrap();
    f.runtime()["stdio"]["executable"] = json!("/usr/bin/python3");
    f.runtime()["stdio"]["arguments"] = json!([peer]);
    let profiles = f.root.join("profiles.json");
    std::fs::write(&profiles,json!({"schema_version":1,"profiles":{"records.local":{"connector":"example.records","server":"example.records.local","tools":["records.read","records.update"],"generation":1,"environment":{}}}}).to_string()).unwrap();
    f.runtime()["credential_store"] = json!(profiles);
    let mut plan: SessionResourcePlan =
        serde_json::from_value(f.runtime()["steps"][0]["resources"].clone()).unwrap();
    plan.preparation = SessionResourceAmounts {
        connector_calls: 2,
        retrieval_rounds: 2,
        retrieval_results: 12,
        retrieval_tokens: 900_000,
        ..Default::default()
    };
    f.runtime()["steps"][0]["resources"] = json!(plan);
    f.wire["mappings"][0]["sessions"]["resource_limits"] = json!(SessionResourceLimits {
        cost_unit: ReferenceId::new("credits").unwrap(),
        amounts: plan.total().unwrap()
    });
    let database = f.root.join("database");
    std::fs::write(&database, connection).unwrap();
    f.wire["mappings"][0]["sessions"]["store_file"] = json!(database);
    let admission = f.root.join("admission.json");
    std::fs::write(&admission, f.wire.to_string()).unwrap();
    let host = f.host().unwrap();
    let config = host.sessions.as_ref().unwrap();
    let path = f.root.to_string_lossy().into_owned();
    let local = LocalSessionHost::new(Arc::new(LocalSessionAccess {
        binding: host.binding.clone(),
        admission: admission.to_string_lossy().into_owned(),
        cwd: path.clone(),
        repository: path,
        store_connection: Default::default(),
    }));
    let owner = local.owner().unwrap();
    let completed = Cell::new(false);
    let callback = |local: &LocalSessionHost,
                    cp: &SessionCheckpoint,
                    resources: &mut SessionPhaseResources,
                    _: ConnectorPreparationContext<'_>|
     -> Result<ConnectorPreparation, SessionError> {
        let started = std::time::Instant::now();
        assert_eq!(resources.phase(), SessionResourcePhase::Preparation);
        assert_eq!(plan.preparation.model_attempts, 0);
        let connector = resources.allocate(plan.preparation)?;
        let prepared = PreparedTarget::capture(Arc::clone(&local.access), &connector, None)?;
        let context = crate::mcp_client::CallContext::new(
            NonEmptyText::new("pre-goal-native-read").unwrap(),
            connector.deadline(),
        );
        let mut target = prepared.build(connector, context, Arc::new(()))?;
        target.discover()?;
        let (captured, proof) = capture(&mut target, cp)?;
        assert_eq!(
            captured.reads().len(),
            1,
            "identical declared source reads are reused for both original goal conditions"
        );
        assert_eq!(captured.data().reads().len(), 2);
        let proof =
            proof.expect("the complete original two-condition goal must hold in actual MCP data");
        assert_eq!(
            proof.goal_digest(),
            &ClosedLoop::goal_digest(cp.goal.intent())
        );
        assert_eq!(target.usage()?.connector_calls, 2);
        assert_eq!(target.usage()?.model_attempts, 0);
        assert!(target.context_source().action_context().is_none());
        eprintln!(
            "actual MCP pre-execution goal capture elapsed_ms={}",
            started.elapsed().as_millis()
        );
        Ok(ConnectorPreparation::GoalAlreadySatisfied(Box::new(
            captured.into_candidate(&proof)?,
        )))
    };
    let probe = NativeConsentPreparingProbe {
        local,
        plan: plan.clone(),
        completed: &completed,
        callback: &callback,
    };
    let repository = store(&probe.local.current().unwrap()).unwrap();
    let reader = store(&probe.local.current().unwrap()).unwrap();
    let coordinator = SessionCoordinator::new(repository, probe);
    let started = coordinator
        .execute(
            &owner,
            SessionCommand::Start {
                command: CommandId::new(format!("pre-goal-start-{nonce}")).unwrap(),
                intent: config.intent.clone(),
                execution: config.execution,
            },
        )
        .unwrap();
    let done = coordinator
        .execute(
            &owner,
            SessionCommand::Continue {
                at: Mutation {
                    session: started.session.clone(),
                    expected_revision: started.revision,
                    command: CommandId::new(format!("pre-goal-continue-{nonce}")).unwrap(),
                },
            },
        )
        .unwrap();
    assert!(completed.get());
    assert_eq!(done.state, SessionState::Completed);
    assert_eq!(done.dispatch_knowledge, DispatchKnowledge::Verified);
    let persisted = reader.load(&owner, &done.session).unwrap().checkpoint;
    assert!(persisted.pending.is_none());
    assert!(persisted.accepted_connector_consent.is_none());
    assert!(persisted.connector_step_snapshots.is_empty());
    assert!(persisted.connector_results.is_empty());
    assert!(persisted.connector_observations.is_empty());
    let manifest = persisted.pre_execution_goal.as_ref().unwrap();
    assert_eq!(
        reader.record_kind(&owner, &manifest.record).unwrap(),
        RecordKind::PreExecutionGoal
    );
    let data = load_pre_execution_goal(
        &reader,
        &persisted,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
    )
    .unwrap();
    assert_eq!(data.reads().len(), 2);
    let evidence = reader
        .load_artifact(&owner, done.final_evidence.as_ref().unwrap())
        .unwrap();
    let evidence: Value = serde_json::from_slice(&evidence).unwrap();
    assert_eq!(
        evidence["contract"],
        "cg.connector-pre-execution-goal-verification"
    );
    assert_eq!(evidence["purpose"], "PRE_EXECUTION_GOAL");
    assert!(evidence.get("invocation").is_none());
    assert_eq!(coordinator.recover(&owner, &done.session).unwrap(), done);
    provider.finish(0);
}
