use gateway_application::{connectors::*, sessions::*};
use gateway_domain::*;
use std::collections::BTreeMap;

fn text(value: &str) -> NonEmptyText {
    NonEmptyText::new(value).unwrap()
}
fn digest(value: char) -> ContentDigest {
    ContentDigest::new(value.to_string().repeat(64)).unwrap()
}
fn owner() -> OwnerBinding {
    OwnerBinding {
        principal: PrincipalId::new("principal-a").unwrap(),
        workspace: WorkspaceId::new("workspace-a").unwrap(),
        project: ProjectId::new("project-a").unwrap(),
        binding: BindingId::new("binding-a").unwrap(),
        client_owner: ClientOwnerId::new("client-a").unwrap(),
    }
}
fn request() -> ConnectorRequest {
    ConnectorRequest {
        target: ConnectorTarget {
            connector: text("connector-a"),
            scope: ContextScopeId::new("scope-a").unwrap(),
        },
        capability: CapabilityId::new("documents.write").unwrap(),
        class: ConnectorOperationClass::Mutate,
        inventory_revision: digest('a'),
        arguments: BTreeMap::from([(text("title"), TypedValue::string("original").unwrap())]),
        task: TaskId::new("task-a").unwrap(),
        step: PlanStepId::new("step-a").unwrap(),
        execution: text("execution-a"),
        correlation: text("correlation-a"),
    }
}
fn resume_material() -> ConnectorConsentResumeMaterial {
    let invocation = gateway_domain::prompt_ir::InvocationIrIdentity {
        schema_version: SchemaVersion::V1,
        invocation_id: ReferenceId::new("invocation-a").unwrap(),
        invocation_digest: digest('c'),
        handoff_digest: digest('d'),
        tools_digest: digest('e'),
        ir_digest: digest('f'),
    };
    let pin = content_reference("producer-pin", b"trusted-producer-pin").unwrap();
    let raw_output = serde_json::to_vec(&serde_json::json!({
        "schema_version": SchemaVersion::V1,
        "invocation": invocation.clone(),
        "result": {},
        "references": [],
        "tools": [{"capability": {
            "id":"documents.write", "scope":"scope-a", "contract":"cg.capability",
            "contract_version":"1.0", "revision":"1", "digest":"a".repeat(64)
        }, "arguments":{"title":"original"}}]
    }))
    .unwrap();
    ConnectorConsentResumeMaterial::new(invocation.clone(), 0, raw_output, vec![pin.clone()])
        .unwrap()
}
fn resume_binding() -> ConnectorConsentResumeBinding {
    let material = resume_material();
    let invocation = material.invocation.clone();
    let pin = material.producer_pins[0].clone();
    let record_bytes = serde_json::to_vec(&material).unwrap();
    ConnectorConsentResumeBinding {
        invocation,
        proposal_ordinal: 0,
        output_record: content_reference("connector-proposal-resume", &record_bytes).unwrap(),
        output_digest: material.output_digest,
        producer_pins: vec![pin],
    }
}
fn binding(request: &ConnectorRequest) -> ConnectorConsentBinding {
    ConnectorConsentBinding::new(
        owner(),
        ContextScopeId::new("scope-a").unwrap(),
        SessionId::new("session-a").unwrap(),
        RunId::new("run-a").unwrap(),
        Revision::new(4).unwrap(),
        PendingId::new("pending-a").unwrap(),
        PlanStepId::new("step-a").unwrap(),
        DispatchId::new("dispatch-a").unwrap(),
        request,
        resume_binding(),
        PolicyId::new("policy-a").unwrap(),
        digest('b'),
        10_000,
        PrincipalId::new("issuer-a").unwrap(),
    )
    .unwrap()
}
fn record_ref() -> ConnectorConsentRecordRef {
    let bytes = b"connector-consent-record";
    ConnectorConsentRecordRef(content_reference("connector-consent-test", bytes).unwrap())
}

#[test]
fn consent_binds_exact_request_and_every_session_policy_identity() {
    let request = request();
    let binding = binding(&request);
    binding.validate().unwrap();
    binding
        .matches_current(
            &owner(),
            &ContextScopeId::new("scope-a").unwrap(),
            &SessionId::new("session-a").unwrap(),
            &RunId::new("run-a").unwrap(),
            Revision::new(4).unwrap(),
            &PlanStepId::new("step-a").unwrap(),
            &DispatchId::new("dispatch-a").unwrap(),
            &request,
            &PolicyId::new("policy-a").unwrap(),
            &digest('b'),
        )
        .unwrap();

    let mut wrong_owner = owner();
    wrong_owner.project = ProjectId::new("other-project").unwrap();
    assert!(
        binding
            .matches_current(
                &wrong_owner,
                &ContextScopeId::new("scope-a").unwrap(),
                &SessionId::new("session-a").unwrap(),
                &RunId::new("run-a").unwrap(),
                Revision::new(4).unwrap(),
                &PlanStepId::new("step-a").unwrap(),
                &DispatchId::new("dispatch-a").unwrap(),
                &request,
                &PolicyId::new("policy-a").unwrap(),
                &digest('b'),
            )
            .is_err()
    );

    let mut changed_request = request.clone();
    changed_request.arguments.insert(
        text("title"),
        TypedValue::string("changed arguments").unwrap(),
    );
    assert!(
        binding
            .matches_current(
                &owner(),
                &ContextScopeId::new("scope-a").unwrap(),
                &SessionId::new("session-a").unwrap(),
                &RunId::new("run-a").unwrap(),
                Revision::new(4).unwrap(),
                &PlanStepId::new("step-a").unwrap(),
                &DispatchId::new("dispatch-a").unwrap(),
                &changed_request,
                &PolicyId::new("policy-a").unwrap(),
                &digest('b'),
            )
            .is_err()
    );

    let mut changed_inventory = request.clone();
    changed_inventory.inventory_revision = digest('c');
    assert!(
        binding
            .matches_current(
                &owner(),
                &ContextScopeId::new("scope-a").unwrap(),
                &SessionId::new("session-a").unwrap(),
                &RunId::new("run-a").unwrap(),
                Revision::new(4).unwrap(),
                &PlanStepId::new("step-a").unwrap(),
                &DispatchId::new("dispatch-a").unwrap(),
                &changed_inventory,
                &PolicyId::new("policy-a").unwrap(),
                &digest('b'),
            )
            .is_err()
    );

    let mut drifted_requests = Vec::new();
    let mut changed = request.clone();
    changed.target.connector = text("connector-b");
    drifted_requests.push(changed);
    let mut changed = request.clone();
    changed.target.scope = ContextScopeId::new("scope-b").unwrap();
    drifted_requests.push(changed);
    let mut changed = request.clone();
    changed.capability = CapabilityId::new("documents.delete").unwrap();
    drifted_requests.push(changed);
    let mut changed = request.clone();
    changed.class = ConnectorOperationClass::Read;
    drifted_requests.push(changed);
    let mut changed = request.clone();
    changed.task = TaskId::new("task-b").unwrap();
    drifted_requests.push(changed);
    let mut changed = request.clone();
    changed.execution = text("execution-b");
    drifted_requests.push(changed);
    let mut changed = request.clone();
    changed.correlation = text("correlation-b");
    drifted_requests.push(changed);
    for changed in drifted_requests {
        assert!(
            binding
                .matches_current(
                    &owner(),
                    &ContextScopeId::new("scope-a").unwrap(),
                    &SessionId::new("session-a").unwrap(),
                    &RunId::new("run-a").unwrap(),
                    Revision::new(4).unwrap(),
                    &PlanStepId::new("step-a").unwrap(),
                    &DispatchId::new("dispatch-a").unwrap(),
                    &changed,
                    &PolicyId::new("policy-a").unwrap(),
                    &digest('b'),
                )
                .is_err()
        );
    }

    for (session, run, revision, step, dispatch, policy, policy_revision) in [
        (
            "other-session",
            "run-a",
            4,
            "step-a",
            "dispatch-a",
            "policy-a",
            'b',
        ),
        (
            "session-a",
            "other-run",
            4,
            "step-a",
            "dispatch-a",
            "policy-a",
            'b',
        ),
        (
            "session-a",
            "run-a",
            5,
            "step-a",
            "dispatch-a",
            "policy-a",
            'b',
        ),
        (
            "session-a",
            "run-a",
            4,
            "other-step",
            "dispatch-a",
            "policy-a",
            'b',
        ),
        (
            "session-a",
            "run-a",
            4,
            "step-a",
            "other-dispatch",
            "policy-a",
            'b',
        ),
        (
            "session-a",
            "run-a",
            4,
            "step-a",
            "dispatch-a",
            "other-policy",
            'b',
        ),
        (
            "session-a",
            "run-a",
            4,
            "step-a",
            "dispatch-a",
            "policy-a",
            'c',
        ),
    ] {
        assert!(
            binding
                .matches_current(
                    &owner(),
                    &ContextScopeId::new("scope-a").unwrap(),
                    &SessionId::new(session).unwrap(),
                    &RunId::new(run).unwrap(),
                    Revision::new(revision).unwrap(),
                    &PlanStepId::new(step).unwrap(),
                    &DispatchId::new(dispatch).unwrap(),
                    &request,
                    &PolicyId::new(policy).unwrap(),
                    &digest(policy_revision),
                )
                .is_err()
        );
    }
}

#[test]
fn verified_connector_consent_denies_expiry_revocation_and_identity_drift() {
    let request = request();
    let verified =
        VerifiedConnectorConsent::from_trusted_record(record_ref(), binding(&request)).unwrap();
    let args = (
        &owner(),
        &ContextScopeId::new("scope-a").unwrap(),
        &SessionId::new("session-a").unwrap(),
        &RunId::new("run-a").unwrap(),
        Revision::new(4).unwrap(),
        &PlanStepId::new("step-a").unwrap(),
        &DispatchId::new("dispatch-a").unwrap(),
        &request,
        &PolicyId::new("policy-a").unwrap(),
        &digest('b'),
    );
    assert!(
        verified
            .check_dispatch(
                args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7, args.8, args.9,
                9_999, false,
            )
            .is_ok()
    );
    assert_eq!(
        verified
            .check_dispatch(
                args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7, args.8, args.9,
                10_000, false,
            )
            .unwrap_err(),
        SessionError::ExpiredInteraction
    );
    assert_eq!(
        verified
            .check_dispatch(
                args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7, args.8, args.9,
                9_999, true,
            )
            .unwrap_err(),
        SessionError::AuthorityDenied
    );
}

#[test]
fn consent_binding_wire_is_strict_and_consistent() {
    let binding = binding(&request());
    let material = resume_material();
    material.matches(&binding.resume).unwrap();

    let mut drifted_resume = binding.resume.clone();
    drifted_resume.output_digest = digest('a');
    assert_eq!(
        material.matches(&drifted_resume),
        Err(SessionError::StaleRevision)
    );

    let mut changed_invocation = material.invocation.clone();
    changed_invocation.invocation_digest = digest('a');
    let changed_output = serde_json::to_vec(&serde_json::json!({
        "schema_version": SchemaVersion::V1,
        "invocation": changed_invocation,
        "result": {},
        "references": [],
        "tools": [{"capability": {
            "id":"documents.write", "scope":"scope-a", "contract":"cg.capability",
            "contract_version":"1.0", "revision":"1", "digest":"a".repeat(64)
        }, "arguments":{"title":"original"}}]
    }))
    .unwrap();
    assert!(matches!(
        ConnectorConsentResumeMaterial::new(
            material.invocation.clone(),
            0,
            changed_output,
            material.producer_pins.clone(),
        ),
        Err(SessionError::StaleRevision)
    ));

    let json = serde_json::to_value(&binding).unwrap();
    let decoded: ConnectorConsentBinding = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(decoded, binding);

    let mut wrong_scope = json.clone();
    wrong_scope["scope"] = serde_json::json!("scope-other");
    assert!(serde_json::from_value::<ConnectorConsentBinding>(wrong_scope).is_err());

    let mut unknown = json;
    unknown["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ConnectorConsentBinding>(unknown).is_err());
}

#[test]
fn resume_material_validation_rejects_invalid_bounds_credentials_pins_and_digests() {
    let material = resume_material();

    let mut duplicate_resume = resume_binding();
    duplicate_resume
        .producer_pins
        .push(duplicate_resume.producer_pins[0].clone());
    assert_eq!(
        duplicate_resume.validate(),
        Err(SessionError::InvalidInteraction)
    );

    assert_eq!(
        ConnectorConsentResumeMaterial::new(
            material.invocation.clone(),
            0,
            Vec::new(),
            material.producer_pins.clone(),
        ),
        Err(SessionError::LimitExceeded)
    );
    assert_eq!(
        ConnectorConsentResumeMaterial::new(
            material.invocation.clone(),
            0,
            br#"{"secret":"password=not-for-persistence"}"#.to_vec(),
            material.producer_pins.clone(),
        ),
        Err(SessionError::AuthorityDenied)
    );

    let mut invalid = material.clone();
    invalid.proposal_ordinal = u32::MAX;
    assert_eq!(invalid.validate(), Err(SessionError::InvalidInteraction));

    let mut invalid = material.clone();
    invalid.raw_output.clear();
    assert_eq!(invalid.validate(), Err(SessionError::LimitExceeded));

    let mut invalid = material.clone();
    invalid.producer_pins.clear();
    assert_eq!(invalid.validate(), Err(SessionError::InvalidInteraction));

    let mut invalid = material.clone();
    invalid.producer_pins.push(invalid.producer_pins[0].clone());
    assert_eq!(invalid.validate(), Err(SessionError::InvalidInteraction));

    let mut invalid = material.clone();
    invalid.raw_output = br#"{"secret":"password=not-for-persistence"}"#.to_vec();
    invalid.output_digest = gateway_domain::context_content_digest(&invalid.raw_output);
    assert_eq!(invalid.validate(), Err(SessionError::AuthorityDenied));

    let mut invalid = material;
    invalid.output_digest = digest('a');
    assert_eq!(invalid.validate(), Err(SessionError::InvalidInteraction));
}

struct LegacyAuthority;
impl InteractionAuthorityPort for LegacyAuthority {
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

#[test]
fn connector_consent_authority_defaults_fail_closed_for_legacy_adapters() {
    let authority = LegacyAuthority;
    let grant =
        VerifiedConnectorConsent::from_trusted_record(record_ref(), binding(&request())).unwrap();
    assert_eq!(
        authority.load_connector_consent(&owner(), &record_ref()),
        Err(SessionError::UnsupportedCapability)
    );
    assert_eq!(
        authority.live_connector_consent(&grant),
        Err(SessionError::UnsupportedCapability)
    );
}
