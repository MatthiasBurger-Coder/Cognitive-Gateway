use super::*;
use crate::local_sessions::runtime_admission::{
    model_factory::{
        NativeModelProducerPlan,
        tests::{PEER, http_provider, install},
    },
    preparing_test_support::NativeConsentPreparingProbe,
    target_factory::PreparedTarget,
    tests::Fixture,
};
use crate::local_sessions::*;
use std::cell::{Cell, RefCell};

pub(in crate::local_sessions::runtime_admission) struct CommittedNativePending {
    pub(in crate::local_sessions::runtime_admission) keep: Fixture,
    pub(in crate::local_sessions::runtime_admission) access: Arc<LocalSessionAccess>,
    pub(in crate::local_sessions::runtime_admission) repository:
        crate::session_store::PostgresSessionStore,
    pub(in crate::local_sessions::runtime_admission) checkpoint: SessionCheckpoint,
    pub(in crate::local_sessions::runtime_admission) binding: Box<ConnectorConsentBinding>,
    pub(in crate::local_sessions::runtime_admission) material: ConnectorConsentResumeMaterial,
    pub(in crate::local_sessions::runtime_admission) request: ConnectorRequest,
}
pub(in crate::local_sessions::runtime_admission) fn committed_native_pending()
-> Option<CommittedNativePending> {
    let connection = std::env::var("CG_COGNITIVE_TEST_DATABASE").ok()?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut f = Fixture::with_scope(&format!("native-consent-e2e-{nonce}"));
    let native = f.install_actual_mixed_model();
    let provider = http_provider();
    install(&mut f, provider.endpoint(), None, &native);
    let peer = f.root.join("model-peer.py");
    std::fs::write(&peer, PEER).unwrap();
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
        model_attempts: 1,
        input_tokens: 131072,
        output_tokens: 4096,
        output_bytes: 65536,
        cost_microunits: 0,
        ..Default::default()
    };
    f.runtime()["steps"][0]["resources"] = json!(plan);
    f.wire["mappings"][0]["sessions"]["resource_limits"] = json!(SessionResourceLimits {
        cost_unit: ReferenceId::new("credits").unwrap(),
        amounts: plan.total().unwrap()
    });
    let database = f.root.join("database.txt");
    std::fs::write(&database, connection).unwrap();
    f.wire["mappings"][0]["sessions"]["store_file"] = json!(database);
    let admission = f.root.join("admission.json");
    std::fs::write(&admission, f.wire.to_string()).unwrap();
    let host = f.host().unwrap();
    let config = host.sessions.as_ref().unwrap();
    let cwd = f.root.to_string_lossy().into_owned();
    let local = LocalSessionHost::new(Arc::new(LocalSessionAccess {
        binding: host.binding.clone(),
        admission: admission.to_string_lossy().into_owned(),
        cwd: cwd.clone(),
        repository: cwd,
        store_connection: Default::default(),
    }));
    let owner = local.owner().unwrap();

    let access = Arc::clone(&local.access);
    let completed = Cell::new(false);
    let original = RefCell::new(None);
    let callback = |local: &LocalSessionHost,
                    cp: &SessionCheckpoint,
                    resources: &mut SessionPhaseResources,
                    context: ConnectorPreparationContext<'_>|
     -> Result<ConnectorPreparation, SessionError> {
        let started = std::time::Instant::now();
        let connector = resources.allocate(SessionResourceAmounts {
            connector_calls: 2,
            retrieval_rounds: 2,
            retrieval_results: 12,
            retrieval_tokens: 900_000,
            ..Default::default()
        })?;
        let model = resources.allocate(SessionResourceAmounts {
            model_attempts: 1,
            input_tokens: 131072,
            output_tokens: 4096,
            output_bytes: 65536,
            ..Default::default()
        })?;
        let capture = PreparedTarget::capture(Arc::clone(&local.access), &connector, None)?;
        let ctx = crate::mcp_client::CallContext::new(
            NonEmptyText::new("native-consent-source").unwrap(),
            connector.deadline(),
        );
        let mut target = capture.build(connector, ctx, Arc::new(()))?;
        stage("discovery", &started, target.discover())?;
        let producer = stage(
            "capture",
            &started,
            NativeModelProducerPlan::capture(&target, cp, &model),
        )?;
        let tools = target.context_source().proposal_tools_current()?;
        let mutate = native
            .capabilities
            .iter()
            .find(|(_, definition)| definition.class() == CapabilityClass::Mutate)
            .unwrap()
            .0;
        provider.propose(tools.get(mutate).unwrap());
        let acquired = stage("acquire", &started, producer.acquire(&mut target))?;
        let mut run = stage(
            "start",
            &started,
            acquired.start(
                &target,
                model,
                RequestContext::new(
                    "native-consent-model".into(),
                    std::time::Instant::now() + std::time::Duration::from_secs(30),
                ),
            ),
        )?;
        let turn = stage(
            "attempt",
            &started,
            run.attempt(
                &target,
                gateway_application::closed_loop::LoopDecision::Continue,
            ),
        )?;
        let (preparation, requests) = stage(
            "consent bridge",
            &started,
            prepare_bound(&run, &target, &turn, context),
        )?;
        *original.borrow_mut() = Some((
            turn.invocation().identity(),
            turn.verified_output().output().raw_bytes().to_vec(),
            requests[0].clone(),
        ));
        assert!(matches!(
            preparation,
            ConnectorPreparation::ConsentRequired { .. }
        ));
        assert!(target.context_source().action_context().is_none());
        assert_eq!(run.usage().model_attempts, 1);
        assert_eq!(target.usage()?.connector_calls, 2);
        eprintln!(
            "native actual pending-consent preparation elapsed_ms={}",
            started.elapsed().as_millis()
        );
        Ok(preparation)
    };
    let probe = NativeConsentPreparingProbe {
        local,
        plan,
        completed: &completed,
        callback: &callback,
    };
    let repository = store(&probe.local.current().unwrap()).unwrap();
    let operator_store = store(&probe.local.current().unwrap()).unwrap();
    let coordinator = SessionCoordinator::new(repository, probe);
    let started = coordinator
        .execute(
            &owner,
            SessionCommand::Start {
                command: CommandId::new(format!("native-consent-start-{nonce}")).unwrap(),
                intent: config.intent.clone(),
                execution: config.execution,
            },
        )
        .unwrap();
    let pending = coordinator
        .execute(
            &owner,
            SessionCommand::Continue {
                at: Mutation {
                    session: started.session,
                    expected_revision: started.revision,
                    command: CommandId::new(format!("native-consent-continue-{nonce}")).unwrap(),
                },
            },
        )
        .unwrap();
    assert!(completed.get());
    assert_eq!(pending.state, SessionState::PendingConsent);
    assert_eq!(pending.dispatch_knowledge, DispatchKnowledge::Prepared);
    let current = operator_store.load(&owner, &pending.session).unwrap();
    let Some(PendingInteraction::ConnectorConsent(binding)) = &current.checkpoint.pending else {
        panic!("actual coordinator must persist native connector consent");
    };
    let bytes = operator_store
        .load_artifact(&owner, &binding.resume.output_record)
        .unwrap();
    let material: ConnectorConsentResumeMaterial = serde_json::from_slice(&bytes).unwrap();
    material.matches(&binding.resume).unwrap();
    let saved = original.borrow();
    let (identity, raw, request) = saved.as_ref().unwrap();
    assert_eq!(&material.invocation, identity);
    assert_eq!(&material.raw_output, raw);
    assert_eq!(material.proposal_ordinal, 0);
    assert_eq!(
        binding.request,
        ConnectorRequestIdentity::from_request(request).unwrap()
    );
    let mut roles = BTreeSet::new();
    for pin in &material.producer_pins {
        let data = operator_store.load_artifact(&owner, pin).unwrap();
        assert_eq!(content_reference(pin.id().as_str(), &data).unwrap(), *pin);
        let value: Value = serde_json::from_slice(&data).unwrap();
        if let Some(role) = value["role"].as_str() {
            roles.insert(role.to_owned());
        }
    }
    for role in [
        "runtime",
        "original-invocation",
        "native-resolution",
        "registered-runtime-document",
        "canonical-owner-document",
        "generation-evidence",
        "historical-inventory",
    ] {
        assert!(roles.contains(role), "retained producer role missing");
    }
    let binding = binding.clone();
    let request = request.clone();
    drop(saved);
    provider.finish(1);
    Some(CommittedNativePending {
        keep: f,
        access,
        repository: operator_store,
        checkpoint: current.checkpoint,
        binding,
        material,
        request,
    })
}

#[test]
fn actual_native_turn_becomes_coordinator_pending_consent_and_real_operator_grant() {
    let Some(fixture) = committed_native_pending() else {
        return;
    };
    assert!(fixture.keep.root.exists());
    assert_eq!(
        fixture.material.invocation,
        fixture.binding.resume.invocation
    );
    fixture.access.current().unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let record = fixture
        .repository
        .operator_decide_connector_consent(
            &fixture.checkpoint.owner,
            &fixture.checkpoint.snapshot.session,
            &fixture.checkpoint.owner.principal,
            "approve",
            now,
        )
        .unwrap()
        .unwrap();
    let ConnectorConsentStatus::Granted(grant) = fixture
        .repository
        .connector_consent(&fixture.checkpoint.owner, &record)
        .unwrap()
    else {
        panic!("actual operator decision must mint native grant");
    };
    grant
        .check_dispatch(
            &fixture.checkpoint.owner,
            &fixture.binding.scope,
            &fixture.binding.session,
            &fixture.binding.run,
            fixture.binding.issued_revision,
            &fixture.binding.step,
            &fixture.binding.dispatch,
            &fixture.request,
            &fixture.binding.policy,
            &fixture.binding.policy_revision,
            now,
            false,
        )
        .unwrap();
    assert!(
        grant
            .check_dispatch(
                &fixture.checkpoint.owner,
                &fixture.binding.scope,
                &fixture.binding.session,
                &fixture.binding.run,
                fixture.binding.issued_revision,
                &fixture.binding.step,
                &fixture.binding.dispatch,
                &fixture.request,
                &fixture.binding.policy,
                &fixture.binding.policy_revision,
                fixture.binding.expires_at_ms,
                false
            )
            .is_err()
    );
    fixture
        .repository
        .operator_decide_connector_consent(
            &fixture.checkpoint.owner,
            &fixture.checkpoint.snapshot.session,
            &fixture.checkpoint.owner.principal,
            "withdraw",
            now,
        )
        .unwrap();
    assert!(matches!(
        fixture
            .repository
            .connector_consent(&fixture.checkpoint.owner, &record)
            .unwrap(),
        ConnectorConsentStatus::Withdrawn
    ));
}

fn stage<T>(
    label: &str,
    time: &std::time::Instant,
    result: Result<T, SessionError>,
) -> Result<T, SessionError> {
    eprintln!(
        "native consent stage={label}; elapsed_ms={}; error={:?}",
        time.elapsed().as_millis(),
        result.as_ref().err()
    );
    result
}
