use super::*;

#[test]
fn native_registration_preserves_read_classes_and_refuses_incomplete_or_mutating_routes() {
    let intent: Intent = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/declarative-cli/intent.json"
    ))
    .unwrap();
    let observations = intent
        .desired_state()
        .conditions()
        .iter()
        .map(|condition| json!({"condition":condition.id(),"source":"registered.source","capability":"registered.read","operation_class":"read"}))
        .collect::<Vec<_>>();
    let wire = json!({"store_file":"/private/operator-store","intent":intent,
        "connector_goal":{"plan":content_reference("plan",b"registered plan data").unwrap(),"steps":["step-condition.0"],"observations":observations},
        "execution":{"mode":"DEVELOPMENT","profile":"FULL_PATH"},"max_actions":2,"max_retries":0,"ttl_ms":300000,
        "consent_required":false,"enabled":true,"issuers":["operator"]});
    let scope = gateway_domain::ContextScopeId::new("registered-scope").unwrap();
    for class in ["read", "search", "inspect"] {
        let mut valid = wire.clone();
        for route in valid["connector_goal"]["observations"]
            .as_array_mut()
            .unwrap()
        {
            route["operation_class"] = json!(class);
        }
        let config: SessionAdmission = serde_json::from_value(valid).unwrap();
        let goal = registered_connector_goal(&config, &scope).unwrap();
        assert_eq!(goal.intent(), &intent);
        assert_eq!(goal.observations().len(), observations.len());
    }
    for case in [
        "absent",
        "mutate",
        "condition",
        "source",
        "capability",
        "step",
    ] {
        let mut invalid = wire.clone();
        match case {
            "absent" => invalid["connector_goal"] = Value::Null,
            "mutate" => {
                invalid["connector_goal"]["observations"][0]["operation_class"] = json!("mutate")
            }
            "step" => invalid["connector_goal"]["steps"][0] = json!(""),
            field => invalid["connector_goal"]["observations"][0][field] = json!(""),
        }
        let config: SessionAdmission = serde_json::from_value(invalid).unwrap();
        assert!(
            registered_connector_goal(&config, &scope).is_err(),
            "{case}"
        );
    }
    let mut unknown = wire;
    unknown["grant"] = json!(true);
    assert!(serde_json::from_value::<SessionAdmission>(unknown).is_err());
}

#[test]
fn adapter_error_mapping_preserves_authority_scope_and_revision_categories() {
    for (input, expected) in [
        (FacadeError::ScopeDenied, SessionError::ScopeDenied),
        (FacadeError::StaleRevision, SessionError::StaleRevision),
        (
            FacadeError::UnsupportedCapability,
            SessionError::UnsupportedCapability,
        ),
        (FacadeError::PolicyDenied, SessionError::AuthorityDenied),
        (
            FacadeError::SensitivityDenied,
            SessionError::AuthorityDenied,
        ),
        (FacadeError::InvalidInput, SessionError::InvalidInput),
        (FacadeError::Internal, SessionError::InvalidInput),
    ] {
        assert_eq!(error(input), expected);
    }
}

#[test]
fn current_native_host_rejects_changed_identity_and_foreign_operator_owner() {
    if std::env::var_os("CG_COGNITIVE_TEST_DATABASE").is_none() {
        return;
    }
    let fixture = observed_goal::NativeBoundaryFixture::new();
    let access = fixture.access();
    let host = fixture.host();
    let owner = access.owner().unwrap();
    let mut foreign = owner.clone();
    foreign.project = ProjectId::new("foreign-project").unwrap();
    let session = SessionId::new("foreign-session").unwrap();
    let reference = ConsentRecordRef(content_reference("not-a-grant", b"not-a-grant").unwrap());
    assert_eq!(
        host.disclose(&foreign, &session),
        Err(SessionError::ScopeDenied)
    );
    assert!(matches!(
        host.load_consent(&foreign, &reference),
        Err(SessionError::ScopeDenied)
    ));
    assert_eq!(
        host.issuer(&foreign, &owner.principal),
        Err(SessionError::ScopeDenied)
    );
    assert_eq!(
        host.issuer(&owner, &PrincipalId::new("foreign-issuer").unwrap()),
        Err(SessionError::AuthorityDenied)
    );
    let original = fixture.config();
    for field in ["principal", "session_id", "canonical_scope"] {
        let mut changed = original.clone();
        changed["mappings"][0][field] = json!("foreign-binding");
        fixture.write_config(&changed);
        assert!(
            matches!(access.current(), Err(SessionError::ScopeDenied)),
            "{field}"
        );
    }
    let mut disabled = original.clone();
    disabled["mappings"][0]["sessions"]["enabled"] = json!(false);
    fixture.write_config(&disabled);
    assert!(matches!(
        access.current(),
        Err(SessionError::AuthorityDenied)
    ));
    fixture.write_config(&original);
    assert!(access.current().is_ok());
    let raw = serde_json::to_string(&original).unwrap();
    std::fs::write(
        &access.admission,
        raw.replacen('{', "{\"schema_version\":1,", 1),
    )
    .unwrap();
    assert!(matches!(access.current(), Err(SessionError::InvalidInput)));
    std::fs::write(&access.admission, [0xff]).unwrap();
    assert!(matches!(access.current(), Err(SessionError::InvalidInput)));
    std::fs::write(&access.admission, vec![b' '; 1_048_576]).unwrap();
    assert!(matches!(access.current(), Err(SessionError::LimitExceeded)));
}

#[test]
fn selected_database_connection_cannot_redirect_or_change_scope_within_a_command() {
    if std::env::var_os("CG_COGNITIVE_TEST_DATABASE").is_none() {
        return;
    }
    let fixture = observed_goal::NativeBoundaryFixture::new();
    let access = fixture.access();
    let host = access.current().unwrap();
    access.selected_repository(&host).unwrap();
    let mut wrong_scope = access.current().unwrap();
    wrong_scope.binding.canonical_scope =
        gateway_domain::ContextScopeId::new("foreign-scope").unwrap();
    assert_eq!(
        access.selected_repository(&wrong_scope).err(),
        Some(SessionError::ScopeDenied)
    );
    let original = fixture.config();
    let store = PathBuf::from(
        original["mappings"][0]["sessions"]["store_file"]
            .as_str()
            .unwrap(),
    );
    let mut relative = original.clone();
    relative["mappings"][0]["sessions"]["store_file"] = json!("relative-store");
    fixture.write_config(&relative);
    assert_eq!(
        access.selected_repository(&access.current().unwrap()).err(),
        Some(SessionError::InvalidInput)
    );
    let alternate = store.with_file_name("alternate-private-store");
    std::fs::copy(&store, &alternate).unwrap();
    let mut redirect = original.clone();
    redirect["mappings"][0]["sessions"]["store_file"] = json!(alternate);
    fixture.write_config(&redirect);
    assert_eq!(
        access.selected_repository(&access.current().unwrap()).err(),
        Some(SessionError::StaleRevision)
    );
    fixture.write_config(&original);
    let connection = std::fs::read_to_string(&store).unwrap();
    std::fs::write(
        &store,
        format!("{} application_name=cg403-changed", connection.trim()),
    )
    .unwrap();
    assert_eq!(
        access.selected_repository(&access.current().unwrap()).err(),
        Some(SessionError::StaleRevision)
    );
    std::fs::write(&store, " \n").unwrap();
    assert_eq!(
        access.selected_repository(&access.current().unwrap()).err(),
        Some(SessionError::StorageUnavailable)
    );
    std::fs::write(&store, connection).unwrap();
    assert!(
        access
            .selected_repository(&access.current().unwrap())
            .is_ok()
    );
}
