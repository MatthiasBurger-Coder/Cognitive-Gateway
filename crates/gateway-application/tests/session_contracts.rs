//! #272 prerequisite gate: real shared admission, never a ProjectionHost.
use gateway_application::sessions::*;
use gateway_domain::{ContextScopeId, ExecutionProfile, Intent, OperatingMode, PlanStepId};
use serde_json::{Value, json};

fn owner() -> OwnerBinding {
    OwnerBinding {
        principal: PrincipalId::new("operator").unwrap(),
        workspace: WorkspaceId::new("workspace").unwrap(),
        project: ProjectId::new("project").unwrap(),
        binding: BindingId::new("binding").unwrap(),
        client_owner: ClientOwnerId::new("owner").unwrap(),
    }
}
fn reference(id: &str) -> RecordRef {
    RecordRef::new(
        RecordId::new(id).unwrap(),
        RecordId::new("1").unwrap(),
        format!("sha256:{}", "a".repeat(64)),
    )
    .unwrap()
}
fn revision(n: u64) -> Revision {
    Revision::new(n).unwrap()
}
fn at() -> Mutation {
    Mutation {
        session: SessionId::new("task").unwrap(),
        command: CommandId::new("command").unwrap(),
        expected_revision: revision(3),
    }
}
fn pending() -> PendingRef {
    PendingRef {
        id: PendingId::new("question").unwrap(),
        issued_revision: revision(3),
        expires_at_ms: 100,
    }
}
fn action() -> ActionBinding {
    ActionBinding {
        owner: owner(),
        session: at().session,
        run: RunId::new("run").unwrap(),
        pending: pending().id,
        issued_revision: revision(3),
        dispatch: DispatchId::new("dispatch").unwrap(),
        step: RecordId::new("step").unwrap(),
        action: reference("compile"),
        arguments: reference("arguments"),
        artifact_basis: reference("basis"),
        authority: reference("authority"),
        expires_at_ms: 100,
    }
}
fn intent_wire() -> Value {
    let mut intent: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/declarative-cli/intent.json"
    ))
    .unwrap();
    intent["desired_state"]["conditions"][0]["id"] = json!("context-verified");
    intent["desired_state"]["conditions"][0]["subject"] = json!("cg.context.projection.verified");
    intent["desired_state"]["expression"]["value"] = json!("context-verified");
    intent
}
fn intent() -> Intent {
    serde_json::from_value(intent_wire()).unwrap()
}
fn basis() -> ArtifactGoalBasis {
    ArtifactGoalBasis {
        scope: ContextScopeId::new("project").unwrap(),
        plan: reference("plan"),
        step: PlanStepId::new("step").unwrap(),
        projection: reference("projection"),
        sources: vec![reference("source")],
    }
}
fn snapshot(state: SessionState) -> SessionSnapshot {
    SessionSnapshot {
        session: at().session,
        run: RunId::new("run").unwrap(),
        revision: revision(3),
        state,
        pending: None,
        dispatch: None,
        dispatch_knowledge: DispatchKnowledge::None,
        command_outcome: None,
        final_evidence: None,
    }
}

#[test]
fn identities_and_checkpoint_decode_are_bounded_and_distinct() {
    macro_rules! check {
        ($($ty:ident),+) => {$(
            for valid in ["A", "abc._:-90", &"a".repeat(128)] {
                let id = $ty::new(valid).unwrap();
                assert_eq!(id.as_str(), valid);
                assert_eq!(serde_json::from_value::<$ty>(json!(valid)).unwrap(), id);
            }
            for invalid in ["", ".first", "-first", "has space", "slash/path", "é", "\n", &"a".repeat(129)] {
                assert_eq!($ty::new(invalid), Err(SessionError::InvalidInput));
                assert!(serde_json::from_value::<$ty>(json!(invalid)).is_err());
            }
            assert!(serde_json::from_value::<$ty>(json!(123)).is_err());
        )+};
    }
    check!(
        PrincipalId,
        WorkspaceId,
        ProjectId,
        BindingId,
        ClientOwnerId,
        SessionId,
        RunId,
        CommandId,
        DispatchId,
        PendingId,
        RecordId
    );
    for n in [0, 1, MAX_REVISION] {
        let r = revision(n);
        assert_eq!(r.value(), n);
        assert_eq!(serde_json::from_value::<Revision>(json!(n)).unwrap(), r);
    }
    assert_eq!(
        revision(MAX_REVISION).next(),
        Err(SessionError::LimitExceeded)
    );
    assert_eq!(revision(0).next().unwrap(), revision(1));
    assert_eq!(Revision::new(u64::MAX), Err(SessionError::LimitExceeded));
    for value in [json!(-1), json!(1.5), json!(u64::MAX), json!("1")] {
        assert!(serde_json::from_value::<Revision>(value).is_err());
    }
    let reference = reference("artifact");
    assert_eq!(reference.id().as_str(), "artifact");
    assert_eq!(reference.revision().as_str(), "1");
    assert_eq!(reference.digest(), format!("sha256:{}", "a".repeat(64)));
    let wire = serde_json::to_value(&reference).unwrap();
    assert_eq!(
        serde_json::from_value::<RecordRef>(wire.clone()).unwrap(),
        reference
    );
    for digest in [
        "bad".into(),
        format!("sha256:{}", "A".repeat(64)),
        format!("sha256:{}", "g".repeat(64)),
        format!("sha512:{}", "a".repeat(64)),
    ] {
        assert!(
            RecordRef::new(
                RecordId::new("a").unwrap(),
                RecordId::new("1").unwrap(),
                digest.clone()
            )
            .is_err()
        );
        let mut bad = wire.clone();
        bad["digest"] = json!(digest);
        assert!(serde_json::from_value::<RecordRef>(bad).is_err());
    }
    let mut bad = wire;
    bad["grant"] = json!(true);
    assert!(serde_json::from_value::<RecordRef>(bad).is_err());
    let owner = serde_json::to_value(owner()).unwrap();
    assert!(serde_json::from_value::<OwnerBinding>(owner.clone()).is_ok());
    let mut bad = owner;
    bad["connection"] = json!("transport");
    assert!(serde_json::from_value::<OwnerBinding>(bad).is_err());
    let wire = serde_json::to_value(basis()).unwrap();
    assert_eq!(
        serde_json::from_value::<ArtifactGoalBasis>(wire.clone()).unwrap(),
        basis()
    );
    let mut bad = wire.clone();
    bad["step"] = json!("-invalid");
    assert!(serde_json::from_value::<ArtifactGoalBasis>(bad).is_err());
    let mut bad = wire;
    bad["step"] = json!("a".repeat(129));
    assert!(serde_json::from_value::<ArtifactGoalBasis>(bad).is_err());
}

#[test]
fn supported_goal_preserves_domain_intent_and_refuses_unrelated_success_conditions() {
    let goal = SupportedArtifactGoal::validate(intent(), basis()).unwrap();
    assert_eq!(goal.intent(), &intent());
    assert_eq!(goal.basis(), &basis());
    let unrelated: Intent = serde_json::from_str(include_str!(
        "../../../tests/fixtures/declarative-cli/intent.json"
    ))
    .unwrap();
    assert_eq!(
        SupportedArtifactGoal::validate(unrelated, basis()),
        Err(SessionError::UnsupportedCapability)
    );
    for subject in ["cg.context.other.verified", "architecture.clean"] {
        let mut wire = intent_wire();
        wire["desired_state"]["conditions"][0]["subject"] = json!(subject);
        assert!(
            SupportedArtifactGoal::validate(serde_json::from_value(wire).unwrap(), basis())
                .is_err()
        );
    }
    let mut wire = intent_wire();
    wire["desired_state"]["conditions"][0]["expected"]["value"] = json!(false);
    assert!(
        SupportedArtifactGoal::validate(serde_json::from_value(wire).unwrap(), basis()).is_err()
    );
    let mut duplicated = basis();
    duplicated.sources.push(reference("source"));
    assert!(SupportedArtifactGoal::validate(intent(), duplicated).is_err());
    let mut too_many = basis();
    too_many.sources = (0..257)
        .map(|n| reference(&format!("source-{n}")))
        .collect();
    assert!(SupportedArtifactGoal::validate(intent(), too_many).is_err());
}

#[test]
fn every_mutation_has_one_command_and_the_expected_lifecycle_gate() {
    let owner = owner();
    let session = at().session;
    let pending = pending();
    let commands = [
        SessionCommand::Clarify {
            at: at(),
            pending: pending.id.clone(),
            answer: ClarificationAnswer::SelectSource(reference("source")),
        },
        SessionCommand::Approve {
            at: at(),
            pending: pending.id.clone(),
            consent: ConsentRecordRef(reference("consent")),
        },
        SessionCommand::Continue { at: at() },
        SessionCommand::Cancel { at: at() },
    ];
    let states = [
        SessionState::Runnable,
        SessionState::Dispatching,
        SessionState::PendingClarification,
        SessionState::PendingConsent,
        SessionState::Cancelling,
        SessionState::OutcomeUnknown,
        SessionState::Completed,
        SessionState::Failed,
        SessionState::Cancelled,
    ];
    for state in states {
        assert_eq!(
            state.terminal(),
            matches!(
                state,
                SessionState::Completed | SessionState::Failed | SessionState::Cancelled
            )
        );
        let admission = SessionAdmission {
            owner: &owner,
            session: &session,
            revision: revision(3),
            state,
            pending: Some(&pending),
        };
        for (i, command) in commands.iter().enumerate() {
            assert_eq!(command.command_id(), &at().command);
            assert_eq!(command.mutation(), Some(&at()));
            let allowed = match i {
                0 => state == SessionState::PendingClarification,
                1 => state == SessionState::PendingConsent,
                2 => state == SessionState::Runnable,
                _ => matches!(
                    state,
                    SessionState::Runnable
                        | SessionState::Dispatching
                        | SessionState::PendingClarification
                        | SessionState::PendingConsent
                ),
            };
            let result = admission.check(&owner, command, false, 99);
            assert_eq!(result.is_ok(), allowed, "{state:?} {command:?}");
            if state == SessionState::OutcomeUnknown {
                assert_eq!(result, Err(SessionError::OutcomeUnknown));
            }
            assert_eq!(
                admission.check(&owner, command, true, 99),
                Err(SessionError::Duplicate)
            );
        }
    }
    let start = SessionCommand::Start {
        command: at().command,
        intent: intent(),
        execution: RequestedExecution {
            mode: OperatingMode::Development,
            profile: ExecutionProfile::FullPath,
        },
    };
    assert_eq!(start.command_id().as_str(), "command");
    assert!(start.mutation().is_none());
    let admission = SessionAdmission {
        owner: &owner,
        session: &session,
        revision: revision(3),
        state: SessionState::Runnable,
        pending: None,
    };
    assert_eq!(
        admission.check(&owner, &start, false, 99),
        Err(SessionError::InvalidState)
    );
    for field in [
        "principal",
        "workspace",
        "project",
        "binding",
        "client_owner",
    ] {
        let mut wire = serde_json::to_value(&owner).unwrap();
        wire[field] = json!("foreign");
        let foreign = serde_json::from_value(wire).unwrap();
        assert_eq!(
            admission.check(&foreign, &commands[2], true, 99),
            Err(SessionError::ScopeDenied)
        );
    }
    let mut wrong = at();
    wrong.session = SessionId::new("other").unwrap();
    assert_eq!(
        admission.check(&owner, &SessionCommand::Cancel { at: wrong }, true, 99),
        Err(SessionError::Unavailable)
    );
    let mut stale = at();
    stale.expected_revision = revision(2);
    assert_eq!(
        admission.check(&owner, &SessionCommand::Continue { at: stale }, false, 99),
        Err(SessionError::StaleRevision)
    );
    let max = SessionAdmission {
        revision: revision(MAX_REVISION),
        ..admission
    };
    let mut overflowing = at();
    overflowing.expected_revision = revision(MAX_REVISION);
    assert_eq!(
        max.check(
            &owner,
            &SessionCommand::Continue { at: overflowing },
            false,
            99
        ),
        Err(SessionError::LimitExceeded)
    );
    let question = SessionAdmission {
        state: SessionState::PendingClarification,
        ..admission
    };
    assert_eq!(
        question.check(&owner, &commands[0], false, 99),
        Err(SessionError::InvalidInteraction)
    );
}

#[test]
fn clarification_checks_exact_identity_revision_expiry_and_admitted_answer() {
    let question = SourceQuestion {
        pending: pending(),
        basis: reference("basis"),
        alternatives: vec![reference("source")],
    };
    let answer = ClarificationAnswer::SelectSource(reference("source"));
    assert_eq!(
        question
            .validate_answer(&pending().id, revision(3), 99, &answer)
            .unwrap(),
        reference("source")
    );
    assert_eq!(
        question.validate_answer(&PendingId::new("other").unwrap(), revision(3), 99, &answer),
        Err(SessionError::InvalidInteraction)
    );
    assert_eq!(
        question.validate_answer(&pending().id, revision(2), 99, &answer),
        Err(SessionError::InvalidInteraction)
    );
    assert_eq!(
        question.validate_answer(&pending().id, revision(3), 100, &answer),
        Err(SessionError::ExpiredInteraction)
    );
    assert_eq!(
        question.validate_answer(
            &pending().id,
            revision(3),
            99,
            &ClarificationAnswer::SelectSource(reference("other"))
        ),
        Err(SessionError::InvalidInput)
    );
    let encoded = serde_json::to_value(&question).unwrap();
    assert_eq!(
        serde_json::from_value::<SourceQuestion>(encoded).unwrap(),
        question
    );
    for invalid in [
        PendingRef {
            issued_revision: revision(0),
            ..pending()
        },
        PendingRef {
            expires_at_ms: 0,
            ..pending()
        },
        PendingRef {
            expires_at_ms: MAX_REVISION + 1,
            ..pending()
        },
    ] {
        assert_eq!(
            invalid.check(&invalid.id, invalid.issued_revision, 1),
            Err(SessionError::InvalidInteraction)
        );
    }
}

#[test]
fn consent_binds_every_authority_input_and_survives_only_its_own_approval_revision() {
    let binding = action();
    let record = ConsentRecordRef(reference("consent"));
    let grant = VerifiedConsent::from_trusted_record(
        record.clone(),
        binding.clone(),
        PrincipalId::new("issuer").unwrap(),
    );
    assert_eq!(grant.record(), &record);
    assert_eq!(grant.binding(), &binding);
    assert_eq!(grant.issuer().as_str(), "issuer");
    assert!(
        grant
            .check_approval(&binding, revision(3), 99, false)
            .is_ok()
    );
    assert!(
        grant
            .check_dispatch(&binding, revision(4), 99, false)
            .is_ok()
    );
    assert_eq!(
        grant.check_approval(&binding, revision(4), 99, false),
        Err(SessionError::InvalidInteraction)
    );
    assert_eq!(
        grant.check_dispatch(&binding, revision(3), 99, false),
        Err(SessionError::StaleRevision)
    );
    assert_eq!(
        grant.check_dispatch(&binding, revision(5), 99, false),
        Err(SessionError::StaleRevision)
    );
    assert_eq!(
        grant.check_dispatch(&binding, revision(4), 100, false),
        Err(SessionError::ExpiredInteraction)
    );
    assert_eq!(
        grant.check_dispatch(&binding, revision(4), 99, true),
        Err(SessionError::AuthorityDenied)
    );
    for field in [
        "owner",
        "session",
        "run",
        "pending",
        "issued_revision",
        "dispatch",
        "step",
        "action",
        "arguments",
        "artifact_basis",
        "authority",
        "expires_at_ms",
    ] {
        let mut wire = serde_json::to_value(&binding).unwrap();
        match field {
            "owner" => wire[field]["principal"] = json!("other"),
            "issued_revision" | "expires_at_ms" => wire[field] = json!(5),
            "action" | "arguments" | "artifact_basis" | "authority" => {
                wire[field]["digest"] = json!(format!("sha256:{}", "b".repeat(64)))
            }
            _ => wire[field] = json!("other"),
        }
        let changed = serde_json::from_value(wire).unwrap();
        assert_eq!(
            grant.check_dispatch(&changed, revision(4), 99, false),
            Err(SessionError::InvalidInteraction),
            "{field}"
        );
    }
    let mut overflow = binding;
    overflow.issued_revision = revision(MAX_REVISION);
    let grant = VerifiedConsent::from_trusted_record(
        record,
        overflow.clone(),
        PrincipalId::new("issuer").unwrap(),
    );
    assert_eq!(
        grant.check_dispatch(&overflow, revision(MAX_REVISION), 99, false),
        Err(SessionError::LimitExceeded)
    );
}

#[test]
fn cumulative_budgets_cannot_reset_on_decode_and_rejections_do_not_consume_usage() {
    let mut budget = SessionBudget {
        actions: 0,
        retries: 0,
        max_actions: 2,
        max_retries: 1,
        deadline_ms: 100,
    };
    budget.reserve(99, false).unwrap();
    budget.reserve(99, true).unwrap();
    assert_eq!((budget.actions, budget.retries), (2, 1));
    let restored: SessionBudget =
        serde_json::from_value(serde_json::to_value(&budget).unwrap()).unwrap();
    assert_eq!(restored, budget);
    assert_eq!(budget.reserve(99, false), Err(SessionError::LimitExceeded));
    assert_eq!(budget, restored);
    let mut retry = SessionBudget {
        actions: 0,
        max_actions: 3,
        ..restored.clone()
    };
    let before = retry.clone();
    assert!(retry.reserve(99, true).is_err());
    assert_eq!(retry, before);
    let mut expired = SessionBudget {
        actions: 0,
        retries: 0,
        ..restored.clone()
    };
    let before = expired.clone();
    assert!(expired.reserve(100, false).is_err());
    assert_eq!(expired, before);
    for invalid in [
        SessionBudget {
            max_actions: 0,
            ..restored.clone()
        },
        SessionBudget {
            max_actions: 10001,
            ..restored.clone()
        },
        SessionBudget {
            max_retries: 10001,
            ..restored.clone()
        },
        SessionBudget {
            actions: 3,
            ..restored.clone()
        },
        SessionBudget {
            retries: 2,
            ..restored.clone()
        },
        SessionBudget {
            deadline_ms: 0,
            ..restored
        },
    ] {
        let mut invalid = invalid;
        let before = invalid.clone();
        assert!(invalid.validate().is_err());
        assert!(invalid.reserve(1, false).is_err());
        assert_eq!(invalid, before);
    }
}

#[test]
fn projections_refuse_fabricated_terminal_success_and_concealed_uncertainty() {
    let runnable = snapshot(SessionState::Runnable);
    runnable.validate().unwrap();
    let mut invalid = runnable.clone();
    invalid.revision = revision(0);
    assert!(invalid.validate().is_err());
    let mut invalid = runnable.clone();
    invalid.final_evidence = Some(reference("evidence"));
    assert!(invalid.validate().is_err());
    let mut invalid = runnable.clone();
    invalid.pending = Some(pending());
    assert!(invalid.validate().is_err());
    let mut invalid = runnable.clone();
    invalid.dispatch = Some(DispatchId::new("dispatch").unwrap());
    assert!(invalid.validate().is_err());
    let mut pending_state = snapshot(SessionState::PendingConsent);
    assert!(pending_state.validate().is_err());
    pending_state.pending = Some(pending());
    pending_state.validate().unwrap();
    pending_state.pending.as_mut().unwrap().issued_revision = revision(2);
    assert!(pending_state.validate().is_err());
    for state in [
        SessionState::Completed,
        SessionState::Failed,
        SessionState::Cancelled,
    ] {
        let mut terminal = snapshot(state);
        if state == SessionState::Completed {
            assert!(terminal.validate().is_err());
            terminal.final_evidence = Some(reference("evidence"));
        }
        terminal.validate().unwrap();
        terminal.dispatch = Some(DispatchId::new("dispatch").unwrap());
        for knowledge in [DispatchKnowledge::Reserved, DispatchKnowledge::Unknown] {
            terminal.dispatch_knowledge = knowledge;
            assert_eq!(terminal.validate(), Err(SessionError::OutcomeUnknown));
        }
        for knowledge in [DispatchKnowledge::Verified, DispatchKnowledge::Stopped] {
            terminal.dispatch_knowledge = knowledge;
            terminal.validate().unwrap();
        }
    }
    for state in [
        SessionState::Dispatching,
        SessionState::Cancelling,
        SessionState::OutcomeUnknown,
        SessionState::Runnable,
        SessionState::PendingClarification,
        SessionState::PendingConsent,
    ] {
        let mut value = snapshot(state);
        if matches!(
            state,
            SessionState::PendingClarification | SessionState::PendingConsent
        ) {
            value.pending = Some(pending());
        }
        value.dispatch = Some(DispatchId::new("dispatch").unwrap());
        value.dispatch_knowledge = DispatchKnowledge::Verified;
        assert_eq!(
            value.validate().is_ok(),
            matches!(
                state,
                SessionState::Runnable
                    | SessionState::PendingClarification
                    | SessionState::PendingConsent
            )
        );
        value.dispatch_knowledge = DispatchKnowledge::Unknown;
        assert_eq!(
            value.validate().is_ok(),
            matches!(
                state,
                SessionState::Dispatching | SessionState::Cancelling | SessionState::OutcomeUnknown
            )
        );
    }
    let mut value = runnable.clone();
    value.command_outcome = Some(CommandOutcome {
        command: at().command,
        session: value.session.clone(),
        revision: revision(1),
    });
    value.validate().unwrap();
    for (session, rev) in [
        (value.session.clone(), 0),
        (value.session.clone(), 4),
        (SessionId::new("other").unwrap(), 1),
    ] {
        value.command_outcome.as_mut().unwrap().session = session;
        value.command_outcome.as_mut().unwrap().revision = revision(rev);
        assert!(value.validate().is_err());
    }
}

#[test]
fn diagnostic_mapping_is_sanitized_stable_and_unique() {
    let errors = [
        SessionError::InvalidInput,
        SessionError::UnsupportedCapability,
        SessionError::ScopeDenied,
        SessionError::Unavailable,
        SessionError::Duplicate,
        SessionError::StaleRevision,
        SessionError::InvalidState,
        SessionError::InvalidInteraction,
        SessionError::ExpiredInteraction,
        SessionError::ConsentRequired,
        SessionError::AuthorityDenied,
        SessionError::LimitExceeded,
        SessionError::OutcomeUnknown,
        SessionError::StorageUnavailable,
    ];
    let mut unique = std::collections::BTreeSet::new();
    for error in errors {
        assert!(unique.insert(error.code()));
        assert!(error.code().starts_with("CG_"));
        assert_eq!(error.to_string(), error.code());
    }
}

fn checkpoint() -> SessionCheckpoint {
    SessionCheckpoint {
        owner: owner(),
        goal: SupportedArtifactGoal::validate(intent(), basis()).unwrap(),
        execution: RequestedExecution {
            mode: OperatingMode::Development,
            profile: ExecutionProfile::FullPath,
        },
        snapshot: snapshot(SessionState::Runnable),
        budget: SessionBudget {
            actions: 1,
            retries: 0,
            max_actions: 3,
            max_retries: 1,
            deadline_ms: 100,
        },
        pending: None,
        accepted_consent: None,
        authority_events: vec![],
    }
}

#[test]
fn checkpoints_preserve_full_pending_identity_and_trusted_authority_history() {
    let base = checkpoint();
    base.validate().unwrap();
    assert_eq!(FenceToken::new(0), Err(SessionError::InvalidInput));
    assert_eq!(FenceToken::new(u64::MAX), Err(SessionError::InvalidInput));
    assert_eq!(FenceToken::new(1).unwrap().value(), 1);
    let mut bad = base.clone();
    bad.budget.actions = 100;
    assert!(bad.validate().is_err());
    let mut bad = base.clone();
    bad.snapshot.revision = revision(0);
    assert!(bad.validate().is_err());
    let mut question = base.clone();
    question.snapshot.state = SessionState::PendingClarification;
    question.snapshot.pending = Some(pending());
    let source = SourceQuestion {
        pending: pending(),
        basis: reference("projection"),
        alternatives: vec![reference("source")],
    };
    assert!(question.validate().is_err());
    question.pending = Some(PendingInteraction::Source(source.clone()));
    question.validate().unwrap();
    for field in ["pending", "empty", "many", "basis", "foreign", "duplicate"] {
        let mut bad = question.clone();
        let Some(PendingInteraction::Source(ref mut q)) = bad.pending else {
            unreachable!()
        };
        match field {
            "pending" => q.pending.id = PendingId::new("other").unwrap(),
            "empty" => q.alternatives.clear(),
            "many" => q.alternatives = vec![reference("source"); 257],
            "basis" => q.basis = reference("other"),
            "foreign" => q.alternatives = vec![reference("other")],
            _ => q.alternatives.push(reference("source")),
        }
        assert_eq!(
            bad.validate(),
            Err(SessionError::InvalidInteraction),
            "{field}"
        );
    }
    let mut mismatch = base.clone();
    mismatch.pending = question.pending.clone();
    assert!(mismatch.validate().is_err());
    let mut consent = base.clone();
    consent.snapshot.state = SessionState::PendingConsent;
    consent.snapshot.pending = Some(pending());
    consent.snapshot.dispatch = Some(action().dispatch);
    consent.snapshot.dispatch_knowledge = DispatchKnowledge::Prepared;
    consent.pending = Some(PendingInteraction::Consent(Box::new(action())));
    consent.validate().unwrap();
    for field in [
        "owner",
        "session",
        "run",
        "pending",
        "issued_revision",
        "expires_at_ms",
        "dispatch",
    ] {
        let mut wire = serde_json::to_value(action()).unwrap();
        match field {
            "owner" => wire[field]["principal"] = json!("other"),
            "issued_revision" | "expires_at_ms" => wire[field] = json!(5),
            _ => wire[field] = json!("other"),
        }
        let mut bad = consent.clone();
        bad.pending = Some(PendingInteraction::Consent(Box::new(
            serde_json::from_value(wire).unwrap(),
        )));
        assert_eq!(
            bad.validate(),
            Err(SessionError::InvalidInteraction),
            "{field}"
        );
    }
    let mut accepted = base.clone();
    accepted.snapshot.revision = revision(4);
    accepted.snapshot.dispatch = Some(action().dispatch);
    accepted.snapshot.dispatch_knowledge = DispatchKnowledge::Prepared;
    accepted.accepted_consent = Some(VerifiedConsent::from_trusted_record(
        ConsentRecordRef(reference("consent")),
        action(),
        PrincipalId::new("issuer").unwrap(),
    ));
    accepted.validate().unwrap();
    for field in ["owner", "session", "run", "dispatch", "issued_revision"] {
        let mut wire = serde_json::to_value(action()).unwrap();
        match field {
            "owner" => wire[field]["principal"] = json!("other"),
            "issued_revision" => wire[field] = json!(4),
            _ => wire[field] = json!("other"),
        }
        let mut bad = accepted.clone();
        bad.accepted_consent = Some(VerifiedConsent::from_trusted_record(
            ConsentRecordRef(reference("consent")),
            serde_json::from_value(wire).unwrap(),
            PrincipalId::new("issuer").unwrap(),
        ));
        assert_eq!(
            bad.validate(),
            Err(SessionError::InvalidInteraction),
            "{field}"
        );
    }
    let events = [
        AuthorityEvent::Denied {
            pending: pending().id,
            revision: revision(3),
        },
        AuthorityEvent::Withdrawn {
            record: ConsentRecordRef(reference("consent")),
            revision: revision(3),
        },
    ];
    for event in events {
        let mut historical = base.clone();
        historical.authority_events.push(event.clone());
        historical.validate().unwrap();
        for rev in [0, 4] {
            let mut bad = historical.clone();
            match &mut bad.authority_events[0] {
                AuthorityEvent::Denied { revision, .. }
                | AuthorityEvent::Withdrawn { revision, .. } => *revision = crate::revision(rev),
            }
            assert_eq!(bad.validate(), Err(SessionError::InvalidState));
        }
        historical.authority_events = vec![event; 4097];
        assert_eq!(historical.validate(), Err(SessionError::LimitExceeded));
    }
    let mut terminal = snapshot(SessionState::Cancelled);
    terminal.dispatch = Some(action().dispatch);
    terminal.dispatch_knowledge = DispatchKnowledge::Prepared;
    assert_eq!(terminal.validate(), Err(SessionError::InvalidState));
}

fn append(current: &FencedSession, command: Option<CommandId>) -> JournalAppend {
    let mut next = current.checkpoint.clone();
    next.snapshot.revision = current.checkpoint.snapshot.revision.next().unwrap();
    if let Some(id) = &command {
        next.snapshot.command_outcome = Some(CommandOutcome {
            command: id.clone(),
            session: next.snapshot.session.clone(),
            revision: next.snapshot.revision,
        });
    }
    JournalAppend {
        session: current.checkpoint.snapshot.session.clone(),
        expected_revision: current.checkpoint.snapshot.revision,
        fence: current.fence,
        command,
        next,
    }
}

#[test]
fn conditional_append_refuses_fencing_scope_goal_and_budget_drift() {
    let current = FencedSession {
        checkpoint: checkpoint(),
        fence: FenceToken::new(1).unwrap(),
    };
    let next = append(&current, Some(at().command));
    next.validate_against(&current).unwrap();
    append(&current, None).validate_against(&current).unwrap();
    for field in [
        "owner",
        "fence",
        "session",
        "expected_revision",
        "goal",
        "execution",
        "next_session",
        "run",
        "revision",
        "max_actions",
        "max_retries",
        "deadline",
        "actions",
        "command",
        "command_revision",
        "missing_outcome",
        "autonomous_outcome",
    ] {
        let mut bad = append(&current, Some(at().command));
        match field {
            "owner" => bad.next.owner.principal = PrincipalId::new("other").unwrap(),
            "fence" => bad.fence = FenceToken::new(2).unwrap(),
            "session" => bad.session = SessionId::new("other").unwrap(),
            "expected_revision" => bad.expected_revision = revision(2),
            "goal" => {
                let mut wire = intent_wire();
                wire["id"] = json!("other");
                bad.next.goal =
                    SupportedArtifactGoal::validate(serde_json::from_value(wire).unwrap(), basis())
                        .unwrap();
            }
            "execution" => bad.next.execution.mode = OperatingMode::Hardening,
            "next_session" => {
                bad.next.snapshot.session = SessionId::new("other").unwrap();
                bad.next.snapshot.command_outcome.as_mut().unwrap().session =
                    bad.next.snapshot.session.clone();
            }
            "run" => bad.next.snapshot.run = RunId::new("other").unwrap(),
            "revision" => {
                bad.next.snapshot.revision = revision(5);
                bad.next.snapshot.command_outcome.as_mut().unwrap().revision = revision(5);
            }
            "max_actions" => bad.next.budget.max_actions = 4,
            "max_retries" => bad.next.budget.max_retries = 2,
            "deadline" => bad.next.budget.deadline_ms = 200,
            "actions" => bad.next.budget.actions = 0,
            "command" => bad.command = Some(CommandId::new("other").unwrap()),
            "command_revision" => {
                bad.next.snapshot.command_outcome.as_mut().unwrap().revision = revision(3)
            }
            "missing_outcome" => bad.next.snapshot.command_outcome = None,
            _ => bad.command = None,
        }
        assert!(bad.validate_against(&current).is_err(), "{field}");
        assert_eq!(current.checkpoint, checkpoint());
    }
    let mut budget_used = current.clone();
    budget_used.checkpoint.budget.retries = 1;
    let mut rollback = append(&budget_used, None);
    rollback.next.budget.retries = 0;
    assert_eq!(
        rollback.validate_against(&budget_used),
        Err(SessionError::InvalidState)
    );
    let mut history = current.clone();
    history
        .checkpoint
        .authority_events
        .push(AuthorityEvent::Denied {
            pending: pending().id,
            revision: revision(3),
        });
    let mut erased = append(&history, None);
    erased.next.authority_events.clear();
    assert_eq!(
        erased.validate_against(&history),
        Err(SessionError::InvalidState)
    );
    let mut bad_current = current.clone();
    bad_current.checkpoint.budget.max_actions = 0;
    assert!(next.validate_against(&bad_current).is_err());
    let mut bad_next = append(&current, None);
    bad_next.next.budget.max_actions = 0;
    assert!(bad_next.validate_against(&current).is_err());
    let mut terminal = current.clone();
    terminal.checkpoint.snapshot.state = SessionState::Cancelled;
    let next = append(&terminal, None);
    assert_eq!(
        next.validate_against(&terminal),
        Err(SessionError::InvalidState)
    );
}
