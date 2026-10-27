use super::*;
use crate::local_sessions::runtime_admission::tests::Fixture;

#[test]
fn actual_current_definitions_need_no_allocation_or_preparation_lease() {
    let Ok(connection) = std::env::var("CG_COGNITIVE_TEST_DATABASE") else {
        return;
    };
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut fixture = Fixture::with_scope(&format!("current-definitions-{nonce}"));
    let native = fixture.install_actual_mixed_model();
    let database = fixture.root.join("database");
    std::fs::write(&database, connection).unwrap();
    fixture.wire["mappings"][0]["sessions"]["store_file"] = json!(database);
    let admission = fixture.root.join("admission.json");
    std::fs::write(&admission, fixture.wire.to_string()).unwrap();
    let host = fixture.host().unwrap();
    let config = host.sessions.as_ref().unwrap();
    let path = fixture.root.to_string_lossy().into_owned();
    let access = Arc::new(LocalSessionAccess {
        binding: host.binding.clone(),
        admission: admission.to_string_lossy().into_owned(),
        cwd: path.clone(),
        repository: path,
        store_connection: Default::default(),
    });
    let local = LocalSessionHost::new(Arc::clone(&access));
    let owner = local.owner().unwrap();
    let repository = store(&host).unwrap();
    let coordinator = SessionCoordinator::new(repository, local);
    let started = coordinator
        .execute(
            &owner,
            SessionCommand::Start {
                command: CommandId::new(format!("current-definition-start-{nonce}")).unwrap(),
                intent: config.intent.clone(),
                execution: config.execution,
            },
        )
        .unwrap();
    let checkpoint = coordinator.details(&owner, &started.session).unwrap();
    assert_eq!(checkpoint.snapshot.state, SessionState::Runnable);
    assert!(checkpoint.lease_until_ms.is_none());
    assert!(
        checkpoint
            .resource_ledger
            .as_ref()
            .unwrap()
            .reservations()
            .is_empty()
    );
    let step = checkpoint.goal.as_connector().unwrap().steps()[0].clone();
    let current = CurrentProducerDefinitions::capture(&access, &checkpoint, &step).unwrap();
    assert_eq!(current.checkpoint(), &checkpoint);
    assert_eq!(current.step(), &step);
    assert_eq!(&current.native.action.resolved, &native.resolved);
    assert!(
        current
            .native
            .action
            .policy_context
            .steps
            .values()
            .all(|facts| facts.consents.is_empty())
    );
    assert!(!current.producer_context().to_json().unwrap().is_empty());
    for (pin, body) in current.canonical_documents() {
        assert_eq!(
            pin.digest,
            context_content_digest(&serde_json::to_vec(body).unwrap())
        );
    }
    assert_eq!(
        coordinator.details(&owner, &started.session).unwrap(),
        checkpoint,
        "data capture cannot append, allocate or alter the actual session"
    );
    let original_consent_required =
        fixture.wire["mappings"][0]["sessions"]["consent_required"].clone();
    fixture.wire["mappings"][0]["sessions"]["consent_required"] =
        json!(!original_consent_required.as_bool().unwrap());
    std::fs::write(&admission, fixture.wire.to_string()).unwrap();
    let changed = CurrentProducerDefinitions::capture(&access, &checkpoint, &step).unwrap();
    assert_ne!(
        changed.policy_revision(),
        current.policy_revision(),
        "a later lookup must evaluate the current actual consent configuration"
    );
    fixture.wire["mappings"][0]["sessions"]["consent_required"] = original_consent_required;
    std::fs::write(&admission, fixture.wire.to_string()).unwrap();
    assert_eq!(
        CurrentProducerDefinitions::capture(&access, &checkpoint, &step)
            .unwrap()
            .policy_revision(),
        current.policy_revision()
    );
    let mut stale = checkpoint.clone();
    stale.snapshot.revision = stale.snapshot.revision.next().unwrap();
    assert!(matches!(
        CurrentProducerDefinitions::capture(&access, &stale, &step),
        Err(SessionError::StaleRevision)
    ));
    assert!(
        CurrentProducerDefinitions::capture(
            &access,
            &checkpoint,
            &PlanStepId::new("foreign-step").unwrap()
        )
        .is_err()
    );
    let mut foreign = checkpoint.clone();
    foreign.owner.principal = PrincipalId::new("foreign-owner").unwrap();
    assert!(matches!(
        CurrentProducerDefinitions::capture(&access, &foreign, &step),
        Err(SessionError::ScopeDenied)
    ));
    let cancelled = coordinator
        .execute(
            &owner,
            SessionCommand::Cancel {
                at: Mutation {
                    session: started.session.clone(),
                    expected_revision: started.revision,
                    command: CommandId::new(format!("current-definition-cancel-{nonce}")).unwrap(),
                },
            },
        )
        .unwrap();
    assert_eq!(cancelled.state, SessionState::Cancelled);
    assert!(matches!(
        CurrentProducerDefinitions::capture(&access, &checkpoint, &step),
        Err(SessionError::StaleRevision)
    ));
    let final_checkpoint = coordinator.details(&owner, &started.session).unwrap();
    assert!(matches!(
        CurrentProducerDefinitions::capture(&access, &final_checkpoint, &step),
        Err(SessionError::InvalidState)
    ));
}
