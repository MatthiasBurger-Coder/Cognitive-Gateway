use std::path::PathBuf;

use gateway_domain::CapabilityId;
use gateway_process::{
    ApplicationError, ApplyEventResult, AuthorizationId, AuthorizationStatus, BlockerId,
    BlockerRuntimeState, CommitOutcome, EvaluationInputs, EventOccurrence, EventOccurrenceId,
    EventTypeId, EvidenceTypeId, GateId, GateStatus, InMemoryProcessStore, PauseReason,
    PolicyDecisionId, PolicyDecisionStatus, ProcessApplication, ProcessDefinition,
    ProcessDefinitionId, ProcessInstance, ProcessInstanceId, ProcessInstanceStatus,
    ProcessRegistry, ProcessSource, ProcessValidator, RetryOutcome, SemanticCompiler,
    TransitionDecisionCode,
};

const IMPLEMENTATION_SOURCE: &str =
    include_str!("../../../catalog/processes/implementation-lifecycle.feature");
const SIMPLE_SOURCE: &str = include_str!("../fixtures/strict-cognitive-gherkin/valid.feature");
const INVALID_SOURCE: &str =
    include_str!("../fixtures/strict-cognitive-gherkin/invalid-unknown-step.feature");

fn catalog() -> ProcessRegistry {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog/processes");
    ProcessRegistry::load(root).unwrap()
}

fn definition<'a>(registry: &'a ProcessRegistry, id: &str) -> &'a ProcessDefinition {
    let id = ProcessDefinitionId::new(id).unwrap();
    registry.resolve(&id, None).unwrap()
}

fn event(instance: &ProcessInstance, sequence: usize, event_type: &str) -> EventOccurrence {
    EventOccurrence::new(
        EventOccurrenceId::new(format!("integration-occurrence-{sequence}"))
            .expect("test occurrence identifier is valid"),
        EventTypeId::new(event_type).expect("test event identifier is valid"),
        instance.id().clone(),
        instance.revision(),
    )
}

fn evidence(value: &str) -> EvidenceTypeId {
    EvidenceTypeId::new(value).expect("test evidence identifier is valid")
}

fn gate(value: &str) -> GateId {
    GateId::new(value).expect("test gate identifier is valid")
}

fn apply(
    app: &ProcessApplication,
    store: &mut InMemoryProcessStore,
    definition: &ProcessDefinition,
    instance: &mut ProcessInstance,
    sequence: usize,
    event_type: &str,
    inputs: &EvaluationInputs,
) -> Result<ApplyEventResult, ApplicationError> {
    let occurrence = event(instance, sequence, event_type);
    let result = app.apply_event_atomically(store, definition, instance, &occurrence, inputs);
    if matches!(&result, Ok(ApplyEventResult::Committed { .. })) {
        *instance = store.instance(instance.id()).unwrap().clone();
    }
    result
}

fn assert_applied(
    app: &ProcessApplication,
    store: &mut InMemoryProcessStore,
    definition: &ProcessDefinition,
    instance: &mut ProcessInstance,
    sequence: usize,
    event_type: &str,
    inputs: EvaluationInputs,
) -> ApplyEventResult {
    let result = apply(
        app, store, definition, instance, sequence, event_type, &inputs,
    )
    .unwrap();
    assert!(matches!(result, ApplyEventResult::Committed { .. }));
    result
}

fn canonical_capabilities() -> [CapabilityId; 4] {
    [
        CapabilityId::new("architecture.dependency-analysis").unwrap(),
        CapabilityId::new("architecture.boundary-validation").unwrap(),
        CapabilityId::new("documentation.traceability-analysis").unwrap(),
        CapabilityId::new("quality.test-strategy-analysis").unwrap(),
    ]
}

#[test]
fn vertical_path_is_rust_only_and_traverses_catalog_to_atomic_completion() {
    let first_registry = catalog();
    let second_registry = catalog();
    assert_eq!(first_registry, second_registry);
    assert_eq!(first_registry.len(), 5);

    let app = ProcessApplication::new();
    let process = definition(&first_registry, "implementation-lifecycle");
    let compiled = app.compile_process_source(IMPLEMENTATION_SOURCE).unwrap();
    let compiled_again = app.compile_process_source(IMPLEMENTATION_SOURCE).unwrap();
    assert_eq!(compiled, compiled_again);
    assert_eq!(compiled.definition(), process);
    assert_eq!(
        compiled.definition().to_json().unwrap(),
        process.to_json().unwrap()
    );
    assert!(
        app.validate_process_definition_with_capabilities(process, &canonical_capabilities())
            .is_valid()
    );

    let mut instance = app
        .start_process(
            process,
            ProcessInstanceId::new("vertical-integration-run").unwrap(),
        )
        .unwrap();
    let pinned_digest = instance.definition_digest().clone();
    let mut store = InMemoryProcessStore::default();
    store.insert(instance.clone());

    assert_applied(
        &app,
        &mut store,
        process,
        &mut instance,
        1,
        "requirements.approved",
        EvaluationInputs::default().with_authorization(
            AuthorizationId::new("requirement-review").unwrap(),
            AuthorizationStatus::Allowed,
        ),
    );
    assert_applied(
        &app,
        &mut store,
        process,
        &mut instance,
        2,
        "readiness.passed",
        EvaluationInputs::default()
            .with_gate(gate("THREE_AMIGOS"), GateStatus::Passed)
            .with_policy_decision(
                PolicyDecisionId::new("implementation-policy").unwrap(),
                PolicyDecisionStatus::Allow,
            ),
    );
    assert_applied(
        &app,
        &mut store,
        process,
        &mut instance,
        3,
        "implementation.completed",
        EvaluationInputs::default(),
    );
    assert_applied(
        &app,
        &mut store,
        process,
        &mut instance,
        4,
        "verification.passed",
        EvaluationInputs::default().with_evidence([evidence("verification.report")]),
    );
    assert_applied(
        &app,
        &mut store,
        process,
        &mut instance,
        5,
        "architecture.approved",
        EvaluationInputs::default()
            .with_gate(gate("ARCHITECTURE_REVIEW"), GateStatus::Passed)
            .with_evidence([evidence("architecture.report")]),
    );
    assert_applied(
        &app,
        &mut store,
        process,
        &mut instance,
        6,
        "e2e.passed",
        EvaluationInputs::default()
            .with_gate(gate("E2E"), GateStatus::Passed)
            .with_evidence([evidence("e2e.report")]),
    );

    let before_completion = instance.clone();
    let completion = event(&before_completion, 7, "evidence.accepted");
    let completion_result = app
        .complete_process(
            &mut store,
            process,
            &before_completion,
            &completion,
            &EvaluationInputs::default().with_evidence([evidence("completion.record")]),
        )
        .unwrap();
    assert!(matches!(
        completion_result,
        ApplyEventResult::Committed { .. }
    ));
    instance = store.instance(instance.id()).unwrap().clone();

    assert_eq!(instance.current_state().as_str(), "COMPLETE");
    assert_eq!(instance.status(), ProcessInstanceStatus::Completed);
    assert_eq!(instance.definition_digest(), &pinned_digest);
    assert_eq!(instance.history().len(), 7);
    assert_eq!(store.instance(instance.id()), Some(&instance));
    assert_eq!(store.consumed_occurrences(instance.id()).unwrap().len(), 7);
}

#[test]
fn registry_identity_and_definition_pinning_fail_closed() {
    let duplicate = ProcessRegistry::from_sources([
        ProcessSource::new("one.feature", SIMPLE_SOURCE),
        ProcessSource::new("two.feature", SIMPLE_SOURCE),
    ])
    .unwrap_err();
    assert_eq!(duplicate.code(), "DUPLICATE_DEFINITION_ID_VERSION");

    let changed_source = SIMPLE_SOURCE.replace(
        "Given evidence verification.report",
        "Given evidence audit.record",
    );
    let original = SemanticCompiler::compile(SIMPLE_SOURCE).unwrap();
    let changed = SemanticCompiler::compile(&changed_source).unwrap();
    assert_ne!(
        original.definition().identity().digest(),
        changed.definition().identity().digest()
    );
    let changed_error = ProcessRegistry::from_sources([
        ProcessSource::new("canonical.feature", SIMPLE_SOURCE),
        ProcessSource::new("changed.feature", changed_source),
    ])
    .unwrap_err();
    assert_eq!(changed_error.code(), "DUPLICATE_DEFINITION_ID_VERSION");

    let app = ProcessApplication::new();
    let original_definition = original.definition();
    let changed_definition = changed.definition();
    let instance = app
        .start_process(
            original_definition,
            ProcessInstanceId::new("pinned-instance").unwrap(),
        )
        .unwrap();
    let unknown = EventOccurrence::new(
        EventOccurrenceId::new("unknown-event").unwrap(),
        EventTypeId::new("implementation.accepted").unwrap(),
        instance.id().clone(),
        instance.revision(),
    );
    let decision = app.evaluate_event(
        changed_definition,
        &instance,
        &unknown,
        &EvaluationInputs::default(),
    );
    assert_eq!(
        decision.code(),
        TransitionDecisionCode::DefinitionIdentityConflict
    );
}

#[test]
fn illegal_unknown_missing_evidence_and_failed_gate_events_are_rejected_without_mutation() {
    let app = ProcessApplication::new();
    let registry = catalog();
    let process = definition(&registry, "implementation-lifecycle");
    let mut instance = app
        .start_process(
            process,
            ProcessInstanceId::new("negative-path-run").unwrap(),
        )
        .unwrap();
    let mut store = InMemoryProcessStore::default();
    store.insert(instance.clone());

    let illegal = apply(
        &app,
        &mut store,
        process,
        &mut instance,
        1,
        "implementation.completed",
        &EvaluationInputs::default(),
    )
    .unwrap();
    assert_eq!(
        illegal.decision().code(),
        TransitionDecisionCode::NoMatchingTransition
    );
    assert_eq!(instance.revision().value(), 0);

    let unknown = apply(
        &app,
        &mut store,
        process,
        &mut instance,
        2,
        "unknown.event",
        &EvaluationInputs::default(),
    )
    .unwrap();
    assert_eq!(
        unknown.decision().code(),
        TransitionDecisionCode::UnknownEvent
    );

    assert_applied(
        &app,
        &mut store,
        process,
        &mut instance,
        3,
        "requirements.approved",
        EvaluationInputs::default().with_authorization(
            AuthorizationId::new("requirement-review").unwrap(),
            AuthorizationStatus::Allowed,
        ),
    );
    assert_applied(
        &app,
        &mut store,
        process,
        &mut instance,
        4,
        "readiness.passed",
        EvaluationInputs::default()
            .with_gate(gate("THREE_AMIGOS"), GateStatus::Passed)
            .with_policy_decision(
                PolicyDecisionId::new("implementation-policy").unwrap(),
                PolicyDecisionStatus::Allow,
            ),
    );
    assert_applied(
        &app,
        &mut store,
        process,
        &mut instance,
        5,
        "implementation.completed",
        EvaluationInputs::default(),
    );

    let missing_evidence = apply(
        &app,
        &mut store,
        process,
        &mut instance,
        6,
        "verification.passed",
        &EvaluationInputs::default(),
    )
    .unwrap();
    assert_eq!(
        missing_evidence.decision().code(),
        TransitionDecisionCode::WaitingForEvidence
    );
    assert_eq!(instance.current_state().as_str(), "VERIFY");

    assert_applied(
        &app,
        &mut store,
        process,
        &mut instance,
        7,
        "verification.passed",
        EvaluationInputs::default().with_evidence([evidence("verification.report")]),
    );
    let failed_gate = apply(
        &app,
        &mut store,
        process,
        &mut instance,
        8,
        "architecture.approved",
        &EvaluationInputs::default()
            .with_gate(gate("ARCHITECTURE_REVIEW"), GateStatus::Failed)
            .with_evidence([evidence("architecture.report")]),
    )
    .unwrap();
    assert_eq!(
        failed_gate.decision().code(),
        TransitionDecisionCode::GateFailed
    );
}

#[test]
fn duplicate_delivery_stale_revision_and_atomic_failure_preserve_authoritative_state() {
    let app = ProcessApplication::new();
    let source_result = app.compile_process_source(SIMPLE_SOURCE).unwrap();
    let process = source_result.definition();
    let mut instance = app
        .start_process(
            process,
            ProcessInstanceId::new("mutation-boundary-run").unwrap(),
        )
        .unwrap();
    let mut store = InMemoryProcessStore::default();
    store.insert(instance.clone());

    let original_snapshot = instance.clone();
    let accepted = event(&original_snapshot, 1, "implementation.accepted");
    let first = app
        .apply_event_atomically(
            &mut store,
            process,
            &original_snapshot,
            &accepted,
            &EvaluationInputs::default().with_gate(gate("THREE_AMIGOS"), GateStatus::Passed),
        )
        .unwrap();
    assert!(matches!(
        first.outcome(),
        Some(CommitOutcome::Applied { revision }) if revision.value() == 1
    ));
    instance = store.instance(instance.id()).unwrap().clone();
    let after_first = instance.clone();

    let duplicate = app
        .apply_event_atomically(
            &mut store,
            process,
            &original_snapshot,
            &accepted,
            &EvaluationInputs::default().with_gate(gate("THREE_AMIGOS"), GateStatus::Passed),
        )
        .unwrap();
    assert!(matches!(
        duplicate.outcome(),
        Some(CommitOutcome::Duplicate { revision }) if revision.value() == 1
    ));
    assert_eq!(store.instance(instance.id()), Some(&after_first));

    let stale_snapshot = after_first.clone();
    let current_event = event(&instance, 2, "implementation.completed");
    let before_failure = instance.clone();
    store.fail_next_commit();
    let atomic_error = app
        .apply_event_atomically(
            &mut store,
            process,
            &instance,
            &current_event,
            &EvaluationInputs::default(),
        )
        .unwrap_err();
    assert_eq!(atomic_error.code(), "ATOMIC_COMMIT_FAILED");
    assert_eq!(store.instance(instance.id()), Some(&before_failure));
    assert!(
        store
            .consumed_occurrences(instance.id())
            .unwrap()
            .iter()
            .all(|occurrence| occurrence != current_event.id())
    );

    app.apply_event_atomically(
        &mut store,
        process,
        &instance,
        &current_event,
        &EvaluationInputs::default(),
    )
    .unwrap();
    let stale_event = event(&stale_snapshot, 3, "implementation.completed");
    let stale_error = app
        .apply_event_atomically(
            &mut store,
            process,
            &stale_snapshot,
            &stale_event,
            &EvaluationInputs::default(),
        )
        .unwrap_err();
    assert_eq!(stale_error.code(), "STALE_REVISION");
}

#[test]
fn blocker_pause_retry_repair_authorization_and_terminal_guards_are_explicit() {
    let app = ProcessApplication::new();
    let registry = catalog();
    let verification = definition(&registry, "verification-quality-gate");
    let mut blocked_instance = app
        .start_process(verification, ProcessInstanceId::new("blocked-run").unwrap())
        .unwrap();
    app.record_blocker(
        verification,
        &mut blocked_instance,
        BlockerRuntimeState::new(
            BlockerId::new("verification-failed").unwrap(),
            "verification requires repair",
            true,
        )
        .unwrap(),
    )
    .unwrap();
    let blocked_event = event(&blocked_instance, 1, "verification.failed");
    let blocked = app.evaluate_event(
        verification,
        &blocked_instance,
        &blocked_event,
        &EvaluationInputs::default(),
    );
    assert_eq!(blocked.code(), TransitionDecisionCode::ActiveBlocker);

    let mut lifecycle_instance = blocked_instance.clone();
    app.pause_process(
        &mut lifecycle_instance,
        PauseReason::HumanReview,
        "review required",
    )
    .unwrap();
    assert_eq!(lifecycle_instance.status(), ProcessInstanceStatus::Paused);
    assert_eq!(
        app.resume_process(&mut lifecycle_instance, false)
            .unwrap_err()
            .code(),
        "WAITING_CONDITION_NOT_CLEARED"
    );
    app.resume_process(&mut lifecycle_instance, true).unwrap();
    assert_eq!(lifecycle_instance.status(), ProcessInstanceStatus::Running);
    assert_eq!(
        app.retry_process(
            &mut lifecycle_instance,
            gateway_process::ActivityId::new("repair-tests").unwrap(),
            2,
        )
        .unwrap(),
        RetryOutcome::Retried {
            attempt: 1,
            max_attempts: 2,
        }
    );
    assert_eq!(
        app.retry_process(
            &mut lifecycle_instance,
            gateway_process::ActivityId::new("repair-tests").unwrap(),
            2,
        )
        .unwrap(),
        RetryOutcome::Retried {
            attempt: 2,
            max_attempts: 2,
        }
    );
    assert_eq!(
        app.retry_process(
            &mut lifecycle_instance,
            gateway_process::ActivityId::new("repair-tests").unwrap(),
            2,
        )
        .unwrap(),
        RetryOutcome::Exhausted {
            attempts: 2,
            max_attempts: 2,
        }
    );

    let requirement = definition(&registry, "requirement-readiness");
    let mut requirement_instance = app
        .start_process(
            requirement,
            ProcessInstanceId::new("authorization-run").unwrap(),
        )
        .unwrap();
    let mut requirement_store = InMemoryProcessStore::default();
    requirement_store.insert(requirement_instance.clone());
    assert_applied(
        &app,
        &mut requirement_store,
        requirement,
        &mut requirement_instance,
        1,
        "request.submitted",
        EvaluationInputs::default(),
    );
    let waiting = apply(
        &app,
        &mut requirement_store,
        requirement,
        &mut requirement_instance,
        2,
        "readiness.approved",
        &EvaluationInputs::default()
            .with_gate(gate("requirement-review"), GateStatus::Passed)
            .with_evidence([evidence("requirement.record")]),
    )
    .unwrap();
    assert_eq!(
        waiting.decision().code(),
        TransitionDecisionCode::WaitingForAuthorization
    );
    let denied = apply(
        &app,
        &mut requirement_store,
        requirement,
        &mut requirement_instance,
        3,
        "readiness.approved",
        &EvaluationInputs::default()
            .with_gate(gate("requirement-review"), GateStatus::Passed)
            .with_evidence([evidence("requirement.record")])
            .with_authorization(
                AuthorizationId::new("requirement-owner").unwrap(),
                AuthorizationStatus::Denied,
            )
            .with_policy_decision(
                PolicyDecisionId::new("requirement-policy").unwrap(),
                PolicyDecisionStatus::Allow,
            ),
    )
    .unwrap();
    assert_eq!(
        denied.decision().code(),
        TransitionDecisionCode::AuthorizationDenied
    );

    let simple = app.compile_process_source(SIMPLE_SOURCE).unwrap();
    let mut terminal = app
        .start_process(
            simple.definition(),
            ProcessInstanceId::new("terminal-run").unwrap(),
        )
        .unwrap();
    let first_event = event(&terminal, 1, "implementation.accepted");
    let first = app.evaluate_event(
        simple.definition(),
        &terminal,
        &first_event,
        &EvaluationInputs::default().with_gate(gate("THREE_AMIGOS"), GateStatus::Passed),
    );
    terminal
        .apply_projection(simple.definition(), first.projection().unwrap().clone())
        .unwrap();
    let complete_event = event(&terminal, 2, "implementation.completed");
    let complete = app.evaluate_event(
        simple.definition(),
        &terminal,
        &complete_event,
        &EvaluationInputs::default(),
    );
    terminal
        .apply_projection(simple.definition(), complete.projection().unwrap().clone())
        .unwrap();
    let terminal_event = event(&terminal, 3, "implementation.completed");
    let terminal_decision = app.evaluate_event(
        simple.definition(),
        &terminal,
        &terminal_event,
        &EvaluationInputs::default(),
    );
    assert_eq!(
        terminal_decision.code(),
        TransitionDecisionCode::TerminalState
    );
}

#[test]
fn simulation_explanation_and_compilation_diagnostics_are_reproducible() {
    let app = ProcessApplication::new();
    let first = app.compile_process_source(SIMPLE_SOURCE).unwrap();
    let second = app.compile_process_source(SIMPLE_SOURCE).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        app.explain_compilation(&first).to_json().unwrap(),
        app.explain_compilation(&second).to_json().unwrap()
    );

    let mut instance = app
        .start_process(
            first.definition(),
            ProcessInstanceId::new("explanation-run").unwrap(),
        )
        .unwrap();
    let occurrence = event(&instance, 1, "implementation.accepted");
    let inputs = EvaluationInputs::default().with_gate(gate("THREE_AMIGOS"), GateStatus::Passed);
    let simulation = app.simulate_transition(first.definition(), &instance, &occurrence, &inputs);
    let direct = app.evaluate_event(first.definition(), &instance, &occurrence, &inputs);
    assert!(simulation.hypothetical());
    assert_eq!(simulation.decision(), &direct);
    assert_eq!(instance.revision().value(), 0);
    let explanation = app.explain_transition(first.definition(), &instance, &occurrence, &direct);
    assert_eq!(explanation.reason_code(), "ACCEPTED");
    assert!(
        explanation
            .to_json()
            .unwrap()
            .contains("implementation.accepted")
    );
    assert!(explanation.human_readable().contains("accepted"));

    instance
        .apply_projection(first.definition(), direct.projection().unwrap().clone())
        .unwrap();
    assert_eq!(instance.current_state().as_str(), "IMPLEMENT");

    let diagnostic = SemanticCompiler::compile(INVALID_SOURCE).unwrap_err();
    assert_eq!(diagnostic.diagnostics()[0].code(), "UNKNOWN_STATEMENT");
    assert!(diagnostic.diagnostics()[0].location().line() > 0);
}

#[test]
fn ambiguous_definition_is_rejected_by_static_validation() {
    let source = "@process(ambiguous)\n@process-version(1)\n@cg-language(1)\nFeature: Ambiguous\nRule: Process\nGiven state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: first\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\nScenario: second\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\n";
    let result = SemanticCompiler::compile(source).unwrap();
    let report = ProcessValidator::validate(result.definition());
    assert!(!report.is_valid());
    assert!(
        report
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "DUPLICATE_SEMANTIC_TRANSITION")
    );
}

#[test]
fn bounded_repair_loop_commits_declared_rework_transitions() {
    let app = ProcessApplication::new();
    let registry = catalog();
    let process = definition(&registry, "verification-quality-gate");
    let mut instance = app
        .start_process(
            process,
            ProcessInstanceId::new("repair-integration-run").unwrap(),
        )
        .unwrap();
    let mut store = InMemoryProcessStore::default();
    store.insert(instance.clone());

    assert_applied(
        &app,
        &mut store,
        process,
        &mut instance,
        1,
        "verification.failed",
        EvaluationInputs::default(),
    );
    assert_eq!(instance.current_state().as_str(), "REPAIR");
    assert_eq!(instance.status(), ProcessInstanceStatus::Blocked);
    assert_applied(
        &app,
        &mut store,
        process,
        &mut instance,
        2,
        "repair.completed",
        EvaluationInputs::default(),
    );
    assert_eq!(instance.current_state().as_str(), "VERIFY");
    assert_eq!(instance.status(), ProcessInstanceStatus::Running);
    assert_eq!(instance.history().len(), 2);
}
