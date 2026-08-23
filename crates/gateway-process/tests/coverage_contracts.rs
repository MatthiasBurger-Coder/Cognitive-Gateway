use std::str::FromStr;

use gateway_domain::CapabilityId;
use gateway_process::*;

fn simple_definition(id: &str, guard: GuardExpression) -> ProcessDefinition {
    let mut builder = ProcessDefinitionBuilder::new(
        ProcessDefinitionId::new(id).unwrap(),
        ProcessDefinitionVersion::new(1).unwrap(),
    )
    .with_states([
        StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
        StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap(),
    ])
    .with_events([EventTypeDefinition::new(
        EventTypeId::new("finish").unwrap(),
    )]);

    match &guard {
        GuardExpression::EvidencePresent(evidence) => {
            builder = builder.with_evidence([EvidenceRequirement::new(evidence.clone(), true)]);
        }
        GuardExpression::BlockerActive(blocker) => {
            builder =
                builder.with_blockers([
                    BlockerDefinition::new(blocker.clone(), "waiting", true).unwrap()
                ]);
        }
        GuardExpression::GateIs { gate, .. } => {
            builder = builder.with_gates([GateDefinition::new(gate.clone(), Vec::new())]);
        }
        _ => {}
    }

    builder
        .with_transitions([TransitionDefinition::new(
            TransitionId::new("finish").unwrap(),
            StateId::new("start").unwrap(),
            EventTypeId::new("finish").unwrap(),
            StateId::new("done").unwrap(),
            guard,
        )])
        .build()
        .unwrap()
}

fn event(instance: &ProcessInstance) -> EventOccurrence {
    EventOccurrence::new(
        EventOccurrenceId::new("occurrence-1").unwrap(),
        EventTypeId::new("finish").unwrap(),
        instance.id().clone(),
        instance.revision(),
    )
}

fn strict_source(body: &str) -> String {
    format!(
        "@process(coverage-case)\n@process-version(1)\n@cg-language(1)\nFeature: Coverage\nRule: Process\n{body}\n"
    )
}

fn assert_compilation_error(source: &str, code: &str) {
    let error = SemanticCompiler::compile(source).unwrap_err();
    assert!(
        error
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == code),
        "expected {code}, got {:?}",
        error.diagnostics()
    );
}

#[test]
fn identifiers_and_error_contracts_are_strongly_typed() {
    let identifier = ProcessDefinitionId::new("alpha.beta").unwrap();
    assert_eq!(identifier.as_str(), "alpha.beta");
    assert_eq!(identifier.as_ref(), "alpha.beta");
    assert_eq!(identifier.to_string(), "alpha.beta");
    assert_eq!(identifier.clone().into_inner(), "alpha.beta");
    assert_eq!(
        ProcessDefinitionId::from_str("alpha.beta").unwrap(),
        identifier
    );

    let invalid = [
        "",
        ".leading",
        "trailing.",
        "two..dots",
        "has space",
        "has/slash",
    ];
    for value in invalid {
        assert_eq!(
            ProcessDefinitionId::new(value).unwrap_err().code(),
            ValidationCode::InvalidIdentifier
        );
    }
    assert_eq!(
        ProcessDefinitionId::new("a".repeat(129))
            .unwrap_err()
            .code(),
        ValidationCode::InvalidIdentifier
    );

    let version = ProcessDefinitionVersion::new(3).unwrap();
    assert_eq!(version.value(), 3);
    assert_eq!(version.to_string(), "3");
    assert_eq!(u32::from(version), 3);
    assert_eq!(ProcessDefinitionVersion::try_from(3).unwrap(), version);
    assert_eq!(
        ProcessDefinitionVersion::new(0).unwrap_err().code(),
        ValidationCode::InvalidVersion
    );

    let digest = ProcessDefinitionDigest::new("A".repeat(64)).unwrap();
    assert_eq!(digest.as_str(), "a".repeat(64));
    assert_eq!(digest.to_string(), "a".repeat(64));
    for value in ["", "f", &"f".repeat(63), &"f".repeat(65), &"g".repeat(64)] {
        assert_eq!(
            ProcessDefinitionDigest::new(value).unwrap_err().code(),
            ValidationCode::InvalidDigest
        );
    }
    assert!(
        serde_json::from_str::<ProcessDefinitionId>("\"bad value\"")
            .unwrap_err()
            .to_string()
            .contains(ValidationCode::InvalidIdentifier.as_str())
    );
    assert!(
        serde_json::from_str::<ProcessDefinitionDigest>("\"short\"")
            .unwrap_err()
            .to_string()
            .contains("digest")
    );

    assert_eq!(ProcessInstanceRevision::initial().value(), 0);
    assert_eq!(ProcessInstanceRevision::new(7).next().unwrap().value(), 8);
    assert_eq!(
        ProcessInstanceRevision::new(u64::MAX)
            .next()
            .unwrap_err()
            .code(),
        ValidationCode::InvalidDefinition
    );
    for code in [
        ValidationCode::InvalidIdentifier,
        ValidationCode::InvalidVersion,
        ValidationCode::InvalidDigest,
        ValidationCode::UnsupportedIrVersion,
        ValidationCode::DuplicateIdentifier,
        ValidationCode::MissingInitialState,
        ValidationCode::MultipleInitialStates,
        ValidationCode::EmptyDefinition,
        ValidationCode::InvalidDefinition,
        ValidationCode::InvalidReference,
        ValidationCode::NonCanonicalDefinition,
    ] {
        assert!(!code.as_str().is_empty());
    }
}

#[test]
fn source_frontend_exposes_locations_keywords_and_structure() {
    let source = "# comment\n@process(example)\n@process-version(1)\n@cg-language(1)\n@marker\nFeature: Example\n  Rule: Process\n    Given state START\n    When event finish\n    Then result\n    And another\n    But final\n    Scenario: scenario\n      Given state START\n      When event finish\n      Then result\n      | key | value |\n";
    let document = SourceDocument::parse(source).unwrap();
    assert_eq!(document.process_id(), "example");
    assert_eq!(document.process_version(), "1");
    assert_eq!(document.language_version(), "1");
    assert_eq!(document.feature_name(), "Example");
    assert_eq!(document.feature_location().line(), 6);
    assert!(document.feature_location().column() > 0);
    assert_eq!(document.tags().len(), 4);
    assert_eq!(document.tags()[0].name(), "process");
    assert_eq!(document.tags()[0].value(), Some("example"));
    assert_eq!(document.tags()[3].name(), "marker");
    assert_eq!(document.tags()[3].value(), None);
    assert_eq!(document.rules().len(), 1);
    let rule = &document.rules()[0];
    assert_eq!(rule.name(), "Process");
    assert!(rule.location().line() > 0);
    assert_eq!(rule.declarations().len(), 5);
    assert_eq!(rule.scenarios().len(), 1);
    let scenario = &rule.scenarios()[0];
    assert_eq!(scenario.name(), "scenario");
    assert_eq!(scenario.steps().len(), 3);
    assert_eq!(scenario.steps()[0].keyword(), SourceStepKeyword::Given);
    assert_eq!(scenario.steps()[0].text(), "state START");
    assert!(scenario.steps()[0].location().line() > 0);
    assert_eq!(scenario.steps()[2].table()[0].cells(), ["key", "value"]);
    assert!(scenario.steps()[2].table()[0].location().line() > 0);
    for (keyword, expected) in [
        (SourceStepKeyword::Given, "Given"),
        (SourceStepKeyword::When, "When"),
        (SourceStepKeyword::Then, "Then"),
        (SourceStepKeyword::And, "And"),
        (SourceStepKeyword::But, "But"),
    ] {
        assert_eq!(keyword.as_str(), expected);
    }
}

#[test]
fn source_frontend_rejects_invalid_structures_without_coercion() {
    let cases = [
        ("Feature: x\nFeature: y", "DUPLICATE_FEATURE"),
        ("Rule: Process", "UNSUPPORTED_RULE"),
        ("Feature: x\nScenario: x", "SCENARIO_OUTSIDE_RULE"),
        (
            "Feature: x\nRule: Process\nGiven x",
            "MISSING_OR_DUPLICATE_TAG",
        ),
        (
            "Feature: x\nRule: Process\nScenario: x\nGiven x",
            "MISSING_OR_DUPLICATE_TAG",
        ),
        (
            "Feature: x\nRule: Process\nGiven x\n| one |",
            "TABLE_OUTSIDE_STEP",
        ),
        (
            "@process(x)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\n| one |",
            "TABLE_OUTSIDE_STEP",
        ),
        (
            "@process(x)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nScenario: x\n| one |",
            "TABLE_OUTSIDE_STEP",
        ),
        (
            "@process(x)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nScenario: x\nGiven value\n| |",
            "INVALID_TABLE",
        ),
        (
            "@process()\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process",
            "INVALID_TAG",
        ),
        (
            "@process(x)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nBackground:",
            "UNSUPPORTED_STRUCTURE",
        ),
        (
            "@process(x)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nScenario Outline: x",
            "UNSUPPORTED_STRUCTURE",
        ),
        (
            "@process(x)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nScenario: x\nGiven",
            "MISSING_TEXT",
        ),
        (
            "@process(x)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nScenario: x\nunknown",
            "INVALID_STEP",
        ),
    ];
    for (source, code) in cases {
        assert_eq!(
            SourceDocument::parse(source).unwrap_err().code(),
            code,
            "{source}"
        );
    }
    assert_eq!(
        SourceDocument::parse("").unwrap_err().code(),
        "MISSING_FEATURE"
    );
    assert_eq!(
        SourceDocument::parse("Feature: x").unwrap_err().code(),
        "MISSING_RULE"
    );
    assert_eq!(
        SourceDocument::parse("Feature: x\nRule: Other")
            .unwrap_err()
            .code(),
        "UNSUPPORTED_RULE"
    );
    assert_eq!(
        SourceDocument::parse("Feature:").unwrap_err().code(),
        "MISSING_TEXT"
    );
    assert_eq!(
        SourceDocument::parse("Feature: x\nRule: Process\nScenario: x")
            .unwrap_err()
            .code(),
        "MISSING_OR_DUPLICATE_TAG"
    );
    assert_eq!(
        SourceDocument::parse("@bad(tag\nFeature: x")
            .unwrap_err()
            .code(),
        "INVALID_TAG"
    );
    assert_eq!(
        SourceDocument::parse("@bad!\nFeature: x")
            .unwrap_err()
            .code(),
        "INVALID_TAG"
    );
}

#[test]
fn compiler_compiles_the_complete_three_amigos_contract_shape() {
    let source = "@process(complete-contract)\n@process-version(1)\n@cg-language(1)\nFeature: Complete contract\nRule: Process\nGiven state START is initial\nGiven state WORK\nGiven state DONE is terminal\nGiven event start\nGiven event finish\nGiven gate review\nGiven evidence report\nGiven activity implement\nGiven activity implement requires capability repository.write\nGiven activity implement produces evidence report\nGiven activity implement constrained by mode=hardened\nGiven invariant ready requires gate review passed\nGiven blocker waiting reason needs review resolvable\nGiven retry start max 2 repair WORK\nScenario: start\nGiven process state START\nGiven gate review is open\nGiven evidence report is present\nGiven blocker waiting is active\nGiven capability repository.write is available\nGiven authorization human-review is allowed\nAnd policy decision release is allow\nWhen event start occurs\nThen transition to state WORK\nThen require gate review\nThen require evidence report\nThen authorize activity implement\nThen require capability repository.write\nThen block process with waiting\nThen pause process\nThen retry activity max 2\nThen repair through state WORK\nScenario: finish\nGiven process state WORK\nGiven gate review is waiting-for-evidence\nGiven authorization human-review is waiting\nAnd policy decision release is waiting\nWhen event finish occurs\nThen transition to state DONE\nThen complete process\n";
    let result = SemanticCompiler::compile(source).unwrap();
    assert_eq!(
        result.definition().identity().id().as_str(),
        "complete-contract"
    );
    assert_eq!(result.definition().states().len(), 3);
    assert_eq!(result.definition().events().len(), 2);
    assert_eq!(result.definition().gates()[0].id().as_str(), "review");
    assert_eq!(
        result.definition().evidence()[0].evidence_type().as_str(),
        "report"
    );
    assert_eq!(
        result.definition().activities()[0].capabilities()[0].as_str(),
        "repository.write"
    );
    assert_eq!(
        result.definition().activities()[0].output_evidence()[0].as_str(),
        "report"
    );
    assert_eq!(
        result.definition().activities()[0].constraints()[0].name(),
        "mode"
    );
    assert_eq!(
        result.definition().invariants()[0].reason(),
        "invariant requires gate review passed"
    );
    assert!(result.definition().blockers()[0].resolvable());
    assert_eq!(result.definition().recovery()[0].max_attempts(), 2);
    assert_eq!(result.definition().transitions().len(), 2);
    assert!(result.definition().transitions()[0].pauses());
    assert_eq!(
        result.definition().transitions()[0].retry_attempts(),
        Some(2)
    );
    assert_eq!(
        result.definition().transitions()[0]
            .repair_target()
            .unwrap()
            .as_str(),
        "WORK"
    );
    assert!(
        result
            .trace()
            .iter()
            .any(|entry| entry.construct() == "blocker declaration")
    );
    assert!(
        result
            .trace()
            .iter()
            .all(|entry| entry.location().line() > 0)
    );
    assert!(result.definition().verify_digest().is_ok());
}

#[test]
fn evaluator_inputs_and_guard_variants_are_deterministic() {
    let evidence = EvidenceTypeId::new("report").unwrap();
    let gate = GateId::new("review").unwrap();
    let capability = CapabilityId::new("repository.write").unwrap();
    let blocker = BlockerId::new("wait").unwrap();
    let auth = AuthorizationId::new("human-review").unwrap();
    let policy = PolicyDecisionId::new("release").unwrap();
    let inputs = EvaluationInputs::default()
        .with_evidence([evidence.clone()])
        .with_evidence_status(
            EvidenceTypeId::new("invalid").unwrap(),
            EvidenceStatus::Invalid,
        )
        .with_gate(gate.clone(), GateStatus::Passed)
        .with_capabilities([capability.clone()])
        .with_blockers([blocker.clone()])
        .with_authorization(auth.clone(), AuthorizationStatus::Allowed)
        .with_policy_decision(policy.clone(), PolicyDecisionStatus::Allow);
    assert!(inputs.evidence().contains(&evidence));
    assert_eq!(inputs.evidence_status()[&evidence], EvidenceStatus::Present);
    assert_eq!(inputs.gates()[&gate], GateStatus::Passed);
    assert!(inputs.capabilities().contains(&capability));
    assert!(inputs.blockers().contains(&blocker));
    assert_eq!(
        inputs.policy().authorizations()[&auth],
        AuthorizationStatus::Allowed
    );
    assert_eq!(
        inputs.policy().decisions()[&policy],
        PolicyDecisionStatus::Allow
    );

    let guards = [
        GuardExpression::Never,
        GuardExpression::Any(vec![GuardExpression::Never, GuardExpression::Always]),
        GuardExpression::Not(Box::new(GuardExpression::Never)),
        GuardExpression::EventAttributeEquals {
            name: "result".into(),
            value: "ok".into(),
        },
        GuardExpression::EvidencePresent(evidence.clone()),
        GuardExpression::CapabilityAvailable(capability.clone()),
        GuardExpression::BlockerActive(blocker.clone()),
        GuardExpression::GateIs {
            gate: gate.clone(),
            status: GateStatus::Passed,
        },
        GuardExpression::AuthorizationIs {
            authorization: auth.clone(),
            status: AuthorizationStatus::Allowed,
        },
        GuardExpression::PolicyDecisionIs {
            policy: policy.clone(),
            status: PolicyDecisionStatus::Allow,
        },
    ];
    for (index, guard) in guards.into_iter().enumerate() {
        let definition = simple_definition(&format!("guard-{index}"), guard);
        let instance = ProcessInstance::start(
            &definition,
            ProcessInstanceId::new(format!("run-{index}")).unwrap(),
        )
        .unwrap();
        let event = event(&instance).with_attribute("result", "ok").unwrap();
        let decision = TransitionEvaluator::evaluate(&definition, &instance, &event, &inputs);
        assert!(!decision.guard_evaluations().is_empty());
        assert_eq!(decision.occurrence(), event.id());
        assert_eq!(decision.previous_state(), instance.current_state());
        assert!(decision.resulting_state().is_some() || !decision.accepted());
        let _ = decision.code().as_str();
        let _ = decision.reason();
        let _ = decision.matched_transition();
        let _ = decision.projection();
        let _ = decision.authorized_activity();
        let _ = decision.constraint_evaluations();
        let _ = decision.authorized_activity_definition();
        for evaluation in decision.guard_evaluations() {
            assert!(!evaluation.expression().is_empty());
            let _ = evaluation.matched();
        }
    }
    for code in [
        TransitionDecisionCode::Accepted,
        TransitionDecisionCode::InvalidDefinition,
        TransitionDecisionCode::DefinitionIdentityConflict,
        TransitionDecisionCode::WrongInstance,
        TransitionDecisionCode::StaleRevision,
        TransitionDecisionCode::UnknownEvent,
        TransitionDecisionCode::NoMatchingTransition,
        TransitionDecisionCode::GuardRejected,
        TransitionDecisionCode::AmbiguousTransition,
        TransitionDecisionCode::TerminalState,
        TransitionDecisionCode::WaitingForEvidence,
        TransitionDecisionCode::WaitingForAuthorization,
        TransitionDecisionCode::GateFailed,
        TransitionDecisionCode::EvidenceInvalid,
        TransitionDecisionCode::ActiveBlocker,
        TransitionDecisionCode::InvariantViolation,
        TransitionDecisionCode::AuthorizationDenied,
    ] {
        assert!(!code.as_str().is_empty());
    }
}

#[test]
fn ir_builder_and_runtime_models_cover_all_typed_accessors() {
    let state_start = StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap();
    let state_done = StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap();
    assert_eq!(state_start.id().as_str(), "start");
    assert!(state_start.is_initial());
    assert!(!state_start.is_terminal());
    assert!(StateDefinition::new(StateId::new("bad").unwrap(), true, true).is_err());

    let evidence = EvidenceRequirement::new(EvidenceTypeId::new("report").unwrap(), true);
    assert!(evidence.required());
    assert_eq!(evidence.evidence_type().as_str(), "report");
    let gate = GateDefinition::new(GateId::new("review").unwrap(), vec![evidence.clone()]);
    assert_eq!(gate.id().as_str(), "review");
    assert_eq!(gate.required_evidence().len(), 1);
    let invariant = InvariantDefinition::new(
        BlockerId::new("safe").unwrap(),
        GuardExpression::Always,
        "safe",
    )
    .unwrap();
    assert_eq!(invariant.id().as_str(), "safe");
    assert_eq!(invariant.reason(), "safe");
    assert!(
        InvariantDefinition::new(BlockerId::new("bad").unwrap(), GuardExpression::Always, " ")
            .is_err()
    );
    let blocker =
        BlockerDefinition::new(BlockerId::new("wait").unwrap(), "needs review", true).unwrap();
    assert_eq!(blocker.id().as_str(), "wait");
    assert_eq!(blocker.reason(), "needs review");
    assert!(blocker.resolvable());
    assert!(BlockerDefinition::new(BlockerId::new("bad").unwrap(), "", false).is_err());
    let constraint = ActivityConstraint::new("mode", "full").unwrap();
    assert_eq!(constraint.name(), "mode");
    assert_eq!(constraint.value(), "full");
    assert!(ActivityConstraint::new("", "full").is_err());
    let activity = ActivityDefinition::new(
        ActivityId::new("work").unwrap(),
        vec![CapabilityId::new("repository.write").unwrap()],
        vec![EvidenceTypeId::new("report").unwrap()],
        vec![constraint],
    );
    assert_eq!(activity.id().as_str(), "work");
    assert_eq!(activity.capabilities().len(), 1);
    assert_eq!(activity.output_evidence().len(), 1);
    assert_eq!(activity.constraints().len(), 1);
    let recovery = RecoveryPolicy::new(2, Some(StateId::new("start").unwrap())).unwrap();
    assert_eq!(recovery.max_attempts(), 2);
    assert_eq!(recovery.repair_target().unwrap().as_str(), "start");
    assert!(RecoveryPolicy::new(0, None).is_err());
    let extension = ExecutionGraphExtension::new("execution-graph", false).unwrap();
    assert_eq!(extension.kind(), "execution-graph");
    assert!(!extension.supported());
    assert!(ExecutionGraphExtension::new("", false).is_err());

    let transition = TransitionDefinition::new(
        TransitionId::new("finish").unwrap(),
        StateId::new("start").unwrap(),
        EventTypeId::new("finish").unwrap(),
        StateId::new("done").unwrap(),
        GuardExpression::Always,
    )
    .as_automatic()
    .with_required_gates(vec![GateId::new("review").unwrap()])
    .with_required_evidence(vec![EvidenceTypeId::new("report").unwrap()])
    .with_authorized_activity(ActivityId::new("work").unwrap())
    .with_blocker(BlockerId::new("wait").unwrap())
    .as_pausing()
    .as_completing()
    .with_retry(2)
    .with_repair_target(StateId::new("start").unwrap());
    assert!(transition.is_automatic());
    assert_eq!(transition.id().as_str(), "finish");
    assert_eq!(transition.from().as_str(), "start");
    assert_eq!(transition.event().as_str(), "finish");
    assert_eq!(transition.to().as_str(), "done");
    assert!(matches!(transition.guard(), GuardExpression::Always));
    assert_eq!(transition.required_gates().len(), 1);
    assert_eq!(transition.required_evidence().len(), 1);
    assert_eq!(transition.authorized_activity().unwrap().as_str(), "work");
    assert_eq!(transition.blocker().unwrap().as_str(), "wait");
    assert!(transition.pauses());
    assert!(transition.completes());
    assert_eq!(transition.retry_attempts(), Some(2));
    assert_eq!(transition.repair_target().unwrap().as_str(), "start");

    let definition = ProcessDefinitionBuilder::new(
        ProcessDefinitionId::new("rich").unwrap(),
        ProcessDefinitionVersion::new(1).unwrap(),
    )
    .with_states([state_done, state_start])
    .with_events([EventTypeDefinition::new(
        EventTypeId::new("finish").unwrap(),
    )])
    .with_transitions([transition])
    .with_gates([gate])
    .with_evidence([evidence])
    .with_invariants([invariant])
    .with_blockers([blocker])
    .with_activities([activity])
    .with_recovery([recovery])
    .with_extensions([extension])
    .build()
    .unwrap();
    assert_eq!(definition.ir_version(), ProcessIrVersion::V1);
    assert_eq!(definition.identity().id().as_str(), "rich");
    assert_eq!(definition.identity().version().value(), 1);
    assert_eq!(definition.identity().digest().as_str().len(), 64);
    assert_eq!(definition.states().len(), 2);
    assert_eq!(definition.events().len(), 1);
    assert_eq!(definition.transitions().len(), 1);
    assert_eq!(definition.gates().len(), 1);
    assert_eq!(definition.evidence().len(), 1);
    assert_eq!(definition.invariants().len(), 1);
    assert_eq!(definition.blockers().len(), 1);
    assert_eq!(definition.activities().len(), 1);
    assert_eq!(definition.recovery().len(), 1);
    assert_eq!(definition.extensions().len(), 1);
    assert_eq!(definition.initial_state().id().as_str(), "start");
    let json = definition.to_json().unwrap();
    assert_eq!(ProcessDefinition::from_json(&json).unwrap(), definition);
    assert!(ProcessDefinition::from_json("not-json").is_err());
    assert!(
        ProcessValidator::validate(&definition)
            .diagnostics()
            .iter()
            .all(|diagnostic| {
                !diagnostic.code().is_empty()
                    && !diagnostic.message().is_empty()
                    && !diagnostic.element().is_empty()
            })
    );
}

#[test]
fn instance_lifecycle_and_application_projections_are_observable() {
    let definition = simple_definition("instance-contract", GuardExpression::Always);
    let app = ProcessApplication::new();
    let mut instance = app
        .start_process(&definition, ProcessInstanceId::new("run-1").unwrap())
        .unwrap();
    assert_eq!(instance.definition_id().as_str(), "instance-contract");
    assert_eq!(instance.definition_version().value(), 1);
    assert_eq!(instance.definition_digest().as_str().len(), 64);
    assert_eq!(instance.status(), ProcessInstanceStatus::Running);
    assert!(instance.previous_state().is_none());
    assert!(instance.active_gates().is_empty());
    assert!(instance.blockers().is_empty());
    assert!(instance.evidence().is_empty());
    assert!(instance.retry_attempts().is_empty());
    assert!(instance.context_references().is_empty());
    assert!(instance.history().is_empty());
    assert!(instance.waiting_condition().is_none());

    let blocker =
        BlockerRuntimeState::new(BlockerId::new("wait").unwrap(), "review", true).unwrap();
    assert_eq!(blocker.id().as_str(), "wait");
    assert_eq!(blocker.reason(), "review");
    assert!(blocker.active());
    assert!(blocker.resolvable());
    let mut unresolved =
        BlockerRuntimeState::new(BlockerId::new("fixed").unwrap(), "fixed", false).unwrap();
    assert_eq!(
        unresolved.resolve().unwrap_err().code(),
        "BLOCKER_NOT_RESOLVABLE"
    );
    assert_eq!(
        app.record_blocker(&definition, &mut instance, blocker)
            .unwrap_err()
            .code(),
        "UNKNOWN_BLOCKER"
    );
    assert_eq!(
        app.record_evidence(
            &definition,
            &mut instance,
            EvidenceTypeId::new("report").unwrap()
        )
        .unwrap_err()
        .code(),
        "UNKNOWN_EVIDENCE"
    );
    assert_eq!(
        instance.add_context_reference(" ").unwrap_err().code(),
        "INVALID_CONTEXT_REFERENCE"
    );
    assert_eq!(
        instance.add_context_reference("input:snapshot").unwrap(),
        ()
    );
    assert_eq!(
        instance.context_references().iter().next().unwrap(),
        "input:snapshot"
    );
    assert_eq!(
        instance.to_json().unwrap(),
        ProcessInstance::from_json(&instance.to_json().unwrap())
            .unwrap()
            .to_json()
            .unwrap()
    );
    assert_eq!(
        ProcessInstance::from_json("not-json").unwrap_err().code(),
        "SERIALIZATION_ERROR"
    );

    let occurrence = event(&instance);
    let decision = app.evaluate_event(
        &definition,
        &instance,
        &occurrence,
        &EvaluationInputs::default(),
    );
    let projection = decision.projection().unwrap();
    assert_eq!(projection.expected_revision().value(), 0);
    assert_eq!(projection.transition().as_str(), "finish");
    assert_eq!(projection.target_state().as_str(), "done");
    assert_eq!(projection.occurrence().unwrap(), occurrence.id());
    let mut store = InMemoryProcessStore::default();
    store.insert(instance.clone());
    let result = app
        .apply_event_atomically(
            &mut store,
            &definition,
            &instance,
            &occurrence,
            &EvaluationInputs::default(),
        )
        .unwrap();
    assert_eq!(result.decision().code(), TransitionDecisionCode::Accepted);
    assert!(matches!(
        result.outcome(),
        Some(CommitOutcome::Applied { .. })
    ));
    assert!(result.decision().resulting_state().is_some());
    let committed = store.instance(instance.id()).unwrap();
    assert_eq!(committed.history().len(), 1);
    let history = &committed.history()[0];
    assert_eq!(history.revision().value(), 1);
    assert_eq!(history.transition().as_str(), "finish");
    assert_eq!(history.from().as_str(), "start");
    assert_eq!(history.to().as_str(), "done");
    assert_eq!(history.reason(), "transition guard accepted");
    assert_eq!(history.occurrence().unwrap(), occurrence.id());
}

#[test]
fn compiler_covers_typed_declaration_error_paths() {
    let declarations = r#"Given state START is initial
Given state DONE is terminal
Given event finish
Given gate review
Given evidence report
Given activity work
Given activity work requires capability repository.write
Given activity work produces evidence report
Given activity work constrained by mode=hardened
Given invariant stop requires gate review passed
Given blocker waiting reason review resolvable
Given retry finish max 2 repair DONE
Given state START
Given state START is terminal
Given event finish
Given gate review
Given evidence report
Given activity work
Given activity work requires capability repository.write
Given activity work requires capability bad/capability
Given activity work produces evidence bad/evidence
Given activity work constrained by =
Given blocker waiting reason review
Given blocker empty reason
Given blocker bad/blocker reason review
Given blocker waiting reason review resolvable
Given invariant bad/invariant requires gate review passed
Given invariant stop requires gate bad/gate passed
Given retry missing max 2
Given retry finish max nope
Given retry finish max 0
Given retry finish max 2 repair bad/state
Given unsupported declaration"#;
    let source = strict_source(declarations);
    let error = SemanticCompiler::compile(&source).unwrap_err();
    for code in [
        "DUPLICATE_DECLARATION",
        "CONFLICTING_DECLARATION",
        "INVALID_IDENTIFIER",
        "INVALID_CONSTRAINT",
        "INVALID_DECLARATION",
        "UNKNOWN_REFERENCE",
        "INVALID_RETRY_POLICY",
        "UNKNOWN_DECLARATION",
    ] {
        assert!(
            error
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code() == code),
            "missing {code} in {:?}",
            error.diagnostics()
        );
    }

    for (body, code) in [
        ("Given state bad/id", "INVALID_IDENTIFIER"),
        ("Given event bad/event", "INVALID_IDENTIFIER"),
        ("Given gate bad/gate", "INVALID_IDENTIFIER"),
        ("Given evidence bad/evidence", "INVALID_IDENTIFIER"),
        ("Given activity bad/activity", "INVALID_IDENTIFIER"),
        ("Given blocker waiting", "INVALID_DECLARATION"),
        (
            "Given state START is initial\nScenario: run\nGiven process state START\nThen transition to state DONE",
            "MISSING_EVENT",
        ),
    ] {
        assert_compilation_error(&strict_source(body), code);
    }

    for (source, code) in [
        (
            "@process(bad/id)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process",
            "INVALID_PROCESS_ID",
        ),
        (
            "@process(valid)\n@process-version(nope)\n@cg-language(1)\nFeature: x\nRule: Process",
            "INVALID_PROCESS_VERSION",
        ),
        (
            "@process(valid)\n@process-version(0)\n@cg-language(1)\nFeature: x\nRule: Process",
            "INVALID_PROCESS_VERSION",
        ),
        (
            "@process(valid)\n@process-version(1)\n@cg-language(2)\nFeature: x\nRule: Process",
            "UNSUPPORTED_LANGUAGE_VERSION",
        ),
        (
            "@process(valid)\n@process-version(1)\n@cg-language(1)\n@unknown(tag)\nFeature: x\nRule: Process",
            "UNKNOWN_TAG",
        ),
        (
            "@process(valid)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nGiven state START is initial\nGiven event finish\nScenario: run\nGiven process state START\nGiven event finish occurs\nWhen event finish occurs\nThen transition to state START",
            "UNKNOWN_STATEMENT",
        ),
        (
            "@process(valid)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nGiven state START is initial\nGiven event finish\nGiven activity bad/activity requires capability repository.write\nGiven activity bad/activity produces evidence report\nGiven activity bad/activity constrained by mode=hardened\nScenario: run\nGiven process state START\nWhen event finish occurs\nThen transition to state START",
            "INVALID_IDENTIFIER",
        ),
        (
            "@process(valid)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nGiven state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: run\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\nThen require gate review",
            "UNKNOWN_REFERENCE",
        ),
        (
            "@process(valid)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nGiven state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: run\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\nThen require evidence report",
            "UNKNOWN_REFERENCE",
        ),
        (
            "@process(valid)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nGiven state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: run\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\nThen authorize activity work",
            "UNKNOWN_REFERENCE",
        ),
        (
            "@process(valid)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nGiven state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: run\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\nThen block process with waiting",
            "UNKNOWN_REFERENCE",
        ),
        (
            "@process(valid)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nGiven state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: run\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\nThen repair through state MISSING",
            "UNKNOWN_REFERENCE",
        ),
    ] {
        assert_compilation_error(source, code);
    }
}

#[test]
fn compiler_covers_all_guard_statuses_and_scenario_fail_closed_paths() {
    let valid = r#"Given state START is initial
Given state WORK
Given state DONE is terminal
Given event finish
Given gate review
Given evidence report
Given activity work
Given blocker waiting reason review resolvable
Scenario: gate-open
Given process state START
Given gate review is open
When event finish occurs
Then transition to state WORK
Scenario: gate-passed
Given process state START
Given gate review is passed
When event finish occurs
Then transition to state WORK
Scenario: gate-failed
Given process state START
Given gate review is failed
When event finish occurs
Then transition to state WORK
Scenario: gate-blocked
Given process state START
Given gate review is blocked
When event finish occurs
Then transition to state WORK
Scenario: gate-evidence
Given process state START
Given gate review is waiting-for-evidence
When event finish occurs
Then transition to state WORK
Scenario: gate-authorization
Given process state START
Given gate review is waiting-for-authorization
When event finish occurs
Then transition to state WORK
Scenario: evidence
Given process state START
Given evidence report is present
When event finish occurs
Then transition to state WORK
Scenario: blocker
Given process state START
Given blocker waiting is active
When event finish occurs
Then transition to state WORK
Scenario: capability
Given process state START
Given capability repository.write is available
When event finish occurs
Then transition to state WORK
Scenario: authorization-allowed
Given process state START
Given authorization human-review is allowed
When event finish occurs
Then transition to state WORK
Scenario: authorization-denied
Given process state START
Given authorization human-review is denied
When event finish occurs
Then transition to state WORK
Scenario: authorization-waiting
Given process state START
Given authorization human-review is waiting
When event finish occurs
Then transition to state WORK
Scenario: policy-allow
Given process state START
Given policy decision release is allow
When event finish occurs
Then transition to state WORK
Scenario: policy-deny
Given process state START
Given policy decision release is deny
When event finish occurs
Then transition to state WORK
Scenario: policy-waiting
Given process state START
Given policy decision release is waiting
When event finish occurs
Then transition to state WORK
Scenario: complete
Given process state WORK
When event finish occurs
Then transition to state DONE
Then complete process"#;
    let result = SemanticCompiler::compile(&strict_source(valid)).unwrap();
    assert!(result.definition().transitions().len() >= 16);

    for (body, code) in [
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state bad/state\nWhen event finish occurs\nThen transition to state DONE",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nGiven gate bad/gate is unknown\nWhen event finish occurs\nThen transition to state DONE",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nGiven gate review is unknown\nWhen event finish occurs\nThen transition to state DONE",
            "INVALID_GATE_STATUS",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nGiven evidence bad/evidence is present\nWhen event finish occurs\nThen transition to state DONE",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nGiven blocker bad/blocker is active\nWhen event finish occurs\nThen transition to state DONE",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nGiven capability bad/capability is available\nWhen event finish occurs\nThen transition to state DONE",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nGiven authorization bad/auth is allowed\nWhen event finish occurs\nThen transition to state DONE",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nGiven authorization human-review is unknown\nWhen event finish occurs\nThen transition to state DONE",
            "INVALID_AUTHORIZATION_STATUS",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nGiven policy decision bad/policy is allow\nWhen event finish occurs\nThen transition to state DONE",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nGiven policy decision release is unknown\nWhen event finish occurs\nThen transition to state DONE",
            "INVALID_POLICY_STATUS",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nWhen event bad/event occurs\nThen transition to state DONE",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nWhen event finish occurs\nThen transition to state bad/state",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\nThen require gate bad/gate",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\nThen require evidence bad/evidence",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\nThen authorize activity bad/activity",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\nThen block process with bad/blocker",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\nThen retry activity max 0",
            "INVALID_RETRY_POLICY",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\nThen repair through state bad/state",
            "INVALID_IDENTIFIER",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nWhen event finish occurs\nThen transition to state DONE",
            "MISSING_PROCESS_STATE",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nThen transition to state DONE",
            "MISSING_EVENT",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nWhen event finish occurs",
            "MISSING_TARGET_STATE",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state UNKNOWN\nWhen event missing occurs\nThen transition to state OTHER",
            "UNKNOWN_REFERENCE",
        ),
        (
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: bad\nGiven process state START\nGiven arbitrary statement\nWhen event finish occurs\nThen transition to state DONE",
            "UNKNOWN_STATEMENT",
        ),
    ] {
        assert_compilation_error(&strict_source(body), code);
    }
}

#[test]
fn public_boundaries_cover_errors_constraints_and_runtime_statuses() {
    for status in [
        AuthorizationStatus::Allowed,
        AuthorizationStatus::Denied,
        AuthorizationStatus::Waiting,
    ] {
        assert!(!status.as_str().is_empty());
    }
    for status in [
        PolicyDecisionStatus::Allow,
        PolicyDecisionStatus::Deny,
        PolicyDecisionStatus::Waiting,
    ] {
        assert!(!status.as_str().is_empty());
    }
    for status in [
        EvidenceStatus::Missing,
        EvidenceStatus::Present,
        EvidenceStatus::Invalid,
        EvidenceStatus::Failed,
    ] {
        assert!(!status.as_str().is_empty());
    }
    let evidence_id = EvidenceTypeId::new("report").unwrap();
    let evidence_reference = EvidenceReference::new(
        evidence_id.clone(),
        EvidenceStatus::Present,
        "verification.log",
    )
    .unwrap();
    assert_eq!(evidence_reference.evidence_type(), &evidence_id);
    assert_eq!(evidence_reference.status(), EvidenceStatus::Present);
    assert_eq!(evidence_reference.provenance(), "verification.log");
    assert_eq!(
        EvidenceReference::new(evidence_id.clone(), EvidenceStatus::Missing, " ")
            .unwrap_err()
            .code(),
        "INVALID_EVIDENCE"
    );
    let policy = PolicyInput::default()
        .with_authorization(
            AuthorizationId::new("review").unwrap(),
            AuthorizationStatus::Waiting,
        )
        .with_policy_decision(
            PolicyDecisionId::new("release").unwrap(),
            PolicyDecisionStatus::Deny,
        );
    assert_eq!(policy.authorizations().len(), 1);
    assert_eq!(policy.decisions().len(), 1);

    let capability = CapabilityId::new("repository.write").unwrap();
    let activity_id = ActivityId::new("implement").unwrap();
    let blocker_id = BlockerId::new("waiting").unwrap();
    let activity = ActivityDefinition::new(
        activity_id.clone(),
        vec![capability.clone()],
        vec![evidence_id.clone()],
        vec![ActivityConstraint::new("mode", "hardened").unwrap()],
    );
    let definition = ProcessDefinitionBuilder::new(
        ProcessDefinitionId::new("application-rich").unwrap(),
        ProcessDefinitionVersion::new(1).unwrap(),
    )
    .with_states([
        StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
        StateDefinition::new(StateId::new("middle").unwrap(), false, false).unwrap(),
        StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap(),
    ])
    .with_events([
        EventTypeDefinition::new(EventTypeId::new("finish").unwrap()),
        EventTypeDefinition::new(EventTypeId::new("complete").unwrap()),
    ])
    .with_evidence([EvidenceRequirement::new(evidence_id.clone(), true)])
    .with_blockers([BlockerDefinition::new(blocker_id.clone(), "review required", true).unwrap()])
    .with_activities([activity])
    .with_transitions([
        TransitionDefinition::new(
            TransitionId::new("to-middle").unwrap(),
            StateId::new("start").unwrap(),
            EventTypeId::new("finish").unwrap(),
            StateId::new("middle").unwrap(),
            GuardExpression::Never,
        )
        .with_authorized_activity(activity_id.clone()),
        TransitionDefinition::new(
            TransitionId::new("to-done").unwrap(),
            StateId::new("start").unwrap(),
            EventTypeId::new("finish").unwrap(),
            StateId::new("done").unwrap(),
            GuardExpression::Always,
        )
        .with_authorized_activity(activity_id.clone()),
        TransitionDefinition::new(
            TransitionId::new("complete").unwrap(),
            StateId::new("middle").unwrap(),
            EventTypeId::new("complete").unwrap(),
            StateId::new("done").unwrap(),
            GuardExpression::Always,
        )
        .as_completing(),
    ])
    .build()
    .unwrap();
    let app = ProcessApplication::new();
    let mut instance = app
        .start_process(&definition, ProcessInstanceId::new("rich-run").unwrap())
        .unwrap();
    assert_eq!(
        app.validate_process_definition(&definition).into_result(),
        Ok(())
    );
    let inspection = app.inspect_process(&definition, &instance).unwrap();
    assert_eq!(inspection.definition_id().as_str(), "application-rich");
    assert_eq!(inspection.definition_version().value(), 1);
    assert_eq!(inspection.definition_digest().as_str().len(), 64);
    assert_eq!(inspection.instance().id(), instance.id());
    assert_eq!(inspection.authorized_activities().len(), 1);
    let projected = &inspection.authorized_activities()[0];
    assert_eq!(projected.id(), &activity_id);
    assert_eq!(projected.capabilities(), &[capability]);
    assert_eq!(
        projected.output_evidence(),
        std::slice::from_ref(&evidence_id)
    );
    assert_eq!(projected.constraints().len(), 1);
    assert!(inspection.to_json().unwrap().contains("application-rich"));

    let wrong_definition = simple_definition("different-definition", GuardExpression::Always);
    assert_eq!(
        app.inspect_process(&wrong_definition, &instance)
            .unwrap_err()
            .code(),
        "DEFINITION_IDENTITY_CONFLICT"
    );
    assert!(
        app.get_process_definition(
            &ProcessRegistry::default(),
            &ProcessDefinitionId::new("missing").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap()
        )
        .is_none()
    );
    assert!(
        app.resolve_process_definition(
            &ProcessRegistry::default(),
            &ProcessDefinitionId::new("missing").unwrap(),
            None
        )
        .is_none()
    );
    let registry = ProcessRegistry::from_sources([ProcessSource::new(
        "rich.feature",
        include_str!("../fixtures/strict-cognitive-gherkin/valid.feature"),
    )])
    .unwrap();
    let summaries = app.list_process_definitions(&registry);
    assert_eq!(summaries[0].source_path(), "rich.feature");
    assert_eq!(summaries[0].id().as_str(), "canonical-issue-lifecycle");
    assert_eq!(summaries[0].version().value(), 1);
    assert_eq!(summaries[0].digest().as_str().len(), 64);

    let occurrence = EventOccurrence::new(
        EventOccurrenceId::new("rich-occurrence").unwrap(),
        EventTypeId::new("finish").unwrap(),
        instance.id().clone(),
        instance.revision(),
    );
    let accepted = app.evaluate_event(
        &definition,
        &instance,
        &occurrence,
        &EvaluationInputs::default(),
    );
    let mut store = InMemoryProcessStore::default();
    store.insert(instance.clone());
    assert_eq!(
        app.commit_transition(
            &mut store,
            &definition,
            &EventOccurrence::new(
                EventOccurrenceId::new("different-occurrence").unwrap(),
                EventTypeId::new("finish").unwrap(),
                instance.id().clone(),
                instance.revision(),
            ),
            &accepted,
        )
        .unwrap_err()
        .code(),
        "OCCURRENCE_IDENTITY_CONFLICT"
    );
    let unknown_event = EventOccurrence::new(
        EventOccurrenceId::new("unknown-event").unwrap(),
        EventTypeId::new("unknown").unwrap(),
        instance.id().clone(),
        instance.revision(),
    );
    let rejected = app.evaluate_event(
        &definition,
        &instance,
        &unknown_event,
        &EvaluationInputs::default(),
    );
    assert_eq!(
        app.commit_transition(&mut store, &definition, &unknown_event, &rejected)
            .unwrap_err()
            .code(),
        "UNKNOWN_EVENT"
    );
    let other_definition = ProcessDefinitionBuilder::new(
        ProcessDefinitionId::new("other-definition").unwrap(),
        ProcessDefinitionVersion::new(1).unwrap(),
    )
    .with_states([
        StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
        StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap(),
    ])
    .with_events([EventTypeDefinition::new(
        EventTypeId::new("finish").unwrap(),
    )])
    .with_transitions([TransitionDefinition::new(
        TransitionId::new("other-transition").unwrap(),
        StateId::new("start").unwrap(),
        EventTypeId::new("finish").unwrap(),
        StateId::new("done").unwrap(),
        GuardExpression::Always,
    )])
    .build()
    .unwrap();
    assert_eq!(
        app.commit_transition(&mut store, &other_definition, &occurrence, &accepted)
            .unwrap_err()
            .code(),
        "UNKNOWN_TRANSITION"
    );
    let conflicting_event = EventOccurrence::new(
        occurrence.id().clone(),
        EventTypeId::new("complete").unwrap(),
        instance.id().clone(),
        instance.revision(),
    );
    assert_eq!(
        app.commit_transition(&mut store, &definition, &conflicting_event, &accepted)
            .unwrap_err()
            .code(),
        "EVENT_TYPE_CONFLICT"
    );

    app.record_evidence(&definition, &mut instance, evidence_id.clone())
        .unwrap();
    let blocker = BlockerRuntimeState::new(blocker_id.clone(), "review", true).unwrap();
    app.record_blocker(&definition, &mut instance, blocker)
        .unwrap();
    app.resolve_blocker(&mut instance, &blocker_id).unwrap();
    assert!(!instance.blockers()[&blocker_id].active());
    assert_eq!(
        app.resolve_blocker(&mut instance, &BlockerId::new("missing").unwrap())
            .unwrap_err()
            .code(),
        "UNKNOWN_BLOCKER"
    );
    assert_eq!(
        app.pause_process(&mut instance, PauseReason::HumanReview, " ")
            .unwrap_err()
            .code(),
        "INVALID_WAITING_CONDITION"
    );
    assert_eq!(
        app.resume_process(&mut instance, true).unwrap_err().code(),
        "NOT_PAUSED"
    );
    assert_eq!(
        app.retry_process(&mut instance, ActivityId::new("implement").unwrap(), 0)
            .unwrap_err()
            .code(),
        "UNBOUNDED_RETRY"
    );

    let non_completion = ProcessDefinitionBuilder::new(
        ProcessDefinitionId::new("non-completion").unwrap(),
        ProcessDefinitionVersion::new(1).unwrap(),
    )
    .with_states([
        StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
        StateDefinition::new(StateId::new("middle").unwrap(), false, false).unwrap(),
        StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap(),
    ])
    .with_events([
        EventTypeDefinition::new(EventTypeId::new("advance").unwrap()),
        EventTypeDefinition::new(EventTypeId::new("finish").unwrap()),
    ])
    .with_transitions([
        TransitionDefinition::new(
            TransitionId::new("advance").unwrap(),
            StateId::new("start").unwrap(),
            EventTypeId::new("advance").unwrap(),
            StateId::new("middle").unwrap(),
            GuardExpression::Always,
        ),
        TransitionDefinition::new(
            TransitionId::new("finish").unwrap(),
            StateId::new("middle").unwrap(),
            EventTypeId::new("finish").unwrap(),
            StateId::new("done").unwrap(),
            GuardExpression::Always,
        ),
    ])
    .build()
    .unwrap();
    let non_completion_instance = app
        .start_process(
            &non_completion,
            ProcessInstanceId::new("non-completion-run").unwrap(),
        )
        .unwrap();
    let non_completion_event = EventOccurrence::new(
        EventOccurrenceId::new("advance-occurrence").unwrap(),
        EventTypeId::new("advance").unwrap(),
        non_completion_instance.id().clone(),
        non_completion_instance.revision(),
    );
    assert_eq!(
        app.complete_process(
            &mut store,
            &non_completion,
            &non_completion_instance,
            &non_completion_event,
            &EvaluationInputs::default(),
        )
        .unwrap_err()
        .code(),
        "NOT_COMPLETION_TRANSITION"
    );
}

#[test]
fn validator_reports_reference_graph_conflict_recovery_and_capability_diagnostics() {
    let start = StateId::new("start").unwrap();
    let done = StateId::new("done").unwrap();
    let orphan = StateId::new("orphan").unwrap();
    let event_id = EventTypeId::new("finish").unwrap();
    let missing_gate = GateId::new("missing-gate").unwrap();
    let missing_evidence = EvidenceTypeId::new("missing-evidence").unwrap();
    let missing_blocker = BlockerId::new("missing-blocker").unwrap();
    let missing_activity = ActivityId::new("missing-activity").unwrap();
    let capability = CapabilityId::new("repository.write").unwrap();
    let transition = TransitionDefinition::new(
        TransitionId::new("to-done").unwrap(),
        start.clone(),
        event_id.clone(),
        done.clone(),
        GuardExpression::GateIs {
            gate: missing_gate.clone(),
            status: GateStatus::Passed,
        },
    )
    .with_required_gates(vec![missing_gate.clone(), missing_gate.clone()])
    .with_required_evidence(vec![missing_evidence.clone()])
    .with_authorized_activity(missing_activity)
    .with_blocker(missing_blocker.clone())
    .with_retry(0)
    .with_repair_target(StateId::new("missing-state").unwrap());
    let duplicate = TransitionDefinition::new(
        TransitionId::new("duplicate").unwrap(),
        start.clone(),
        event_id.clone(),
        done.clone(),
        GuardExpression::Always,
    )
    .with_repair_target(start.clone());
    let duplicate_semantic = TransitionDefinition::new(
        TransitionId::new("duplicate-semantic").unwrap(),
        start.clone(),
        event_id.clone(),
        done.clone(),
        GuardExpression::Always,
    );
    let terminal_outgoing = TransitionDefinition::new(
        TransitionId::new("terminal-outgoing").unwrap(),
        done.clone(),
        event_id.clone(),
        start.clone(),
        GuardExpression::Always,
    );
    let definition = ProcessDefinitionBuilder::new(
        ProcessDefinitionId::new("validator-diagnostics").unwrap(),
        ProcessDefinitionVersion::new(1).unwrap(),
    )
    .with_states([
        StateDefinition::new(start, true, false).unwrap(),
        StateDefinition::new(done, false, true).unwrap(),
        StateDefinition::new(orphan, false, false).unwrap(),
    ])
    .with_events([EventTypeDefinition::new(event_id)])
    .with_gates([GateDefinition::new(
        GateId::new("declared-gate").unwrap(),
        vec![EvidenceRequirement::new(missing_evidence.clone(), true)],
    )])
    .with_invariants([
        InvariantDefinition::new(
            BlockerId::new("invariant-gate").unwrap(),
            GuardExpression::GateIs {
                gate: missing_gate.clone(),
                status: GateStatus::Passed,
            },
            "gate must pass",
        )
        .unwrap(),
        InvariantDefinition::new(
            BlockerId::new("invariant-blocker").unwrap(),
            GuardExpression::BlockerActive(missing_blocker.clone()),
            "blocker must be clear",
        )
        .unwrap(),
    ])
    .with_activities([ActivityDefinition::new(
        ActivityId::new("work").unwrap(),
        vec![capability.clone()],
        vec![missing_evidence.clone()],
        Vec::new(),
    )])
    .with_transitions([transition, duplicate, duplicate_semantic, terminal_outgoing])
    .build()
    .unwrap();
    let report = ProcessValidator::validate_with_capabilities(
        &definition,
        &[CapabilityId::new("other.capability").unwrap()],
    );
    assert!(!report.is_valid());
    let codes = report
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect::<Vec<_>>();
    for code in [
        "UNKNOWN_REFERENCE",
        "DUPLICATE_GATE_REQUIREMENT",
        "UNREACHABLE_STATE",
        "INVALID_TERMINAL_TRANSITION",
        "DUPLICATE_SEMANTIC_TRANSITION",
        "AMBIGUOUS_TRANSITION",
        "UNBOUNDED_RETRY_CYCLE",
        "UNKNOWN_CAPABILITY",
    ] {
        assert!(codes.contains(&code), "missing {code}: {codes:?}");
    }
    assert!(report.clone().into_result().is_err());
    assert!(report.diagnostics().iter().all(|diagnostic| {
        !diagnostic.message().is_empty() && !diagnostic.element().is_empty()
    }));
}

#[test]
fn evaluator_covers_constraints_authorization_invariants_and_status_projections() {
    let review = GateId::new("review").unwrap();
    let report = EvidenceTypeId::new("report").unwrap();
    let waiting = BlockerId::new("waiting").unwrap();
    let authorization = AuthorizationId::new("human-review").unwrap();
    let policy = PolicyDecisionId::new("release").unwrap();
    let make_definition = |id: &str,
                           guard: GuardExpression,
                           requires_gate: bool,
                           requires_evidence: bool,
                           gate_prerequisite: bool,
                           invariant: Option<GuardExpression>,
                           mode: u8| {
        let mut transition = TransitionDefinition::new(
            TransitionId::new("advance").unwrap(),
            StateId::new("start").unwrap(),
            EventTypeId::new("finish").unwrap(),
            StateId::new("middle").unwrap(),
            guard,
        );
        if requires_gate {
            transition = transition.with_required_gates(vec![review.clone()]);
        }
        if requires_evidence {
            transition = transition.with_required_evidence(vec![report.clone()]);
        }
        transition = match mode {
            1 => transition.as_pausing(),
            2 => transition.with_blocker(waiting.clone()),
            _ => transition,
        };
        let mut builder = ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new(id).unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .with_states([
            StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
            StateDefinition::new(StateId::new("middle").unwrap(), false, false).unwrap(),
            StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap(),
        ])
        .with_events([
            EventTypeDefinition::new(EventTypeId::new("finish").unwrap()),
            EventTypeDefinition::new(EventTypeId::new("complete").unwrap()),
        ])
        .with_gates([GateDefinition::new(
            review.clone(),
            if gate_prerequisite {
                vec![EvidenceRequirement::new(report.clone(), true)]
            } else {
                Vec::new()
            },
        )])
        .with_evidence([EvidenceRequirement::new(report.clone(), true)])
        .with_blockers([BlockerDefinition::new(waiting.clone(), "waiting", true).unwrap()])
        .with_transitions([
            transition,
            TransitionDefinition::new(
                TransitionId::new("complete").unwrap(),
                StateId::new("middle").unwrap(),
                EventTypeId::new("complete").unwrap(),
                StateId::new("done").unwrap(),
                GuardExpression::Always,
            ),
        ]);
        if let Some(condition) = invariant {
            builder = builder.with_invariants([InvariantDefinition::new(
                BlockerId::new("invariant").unwrap(),
                condition,
                "invariant condition",
            )
            .unwrap()]);
        }
        builder.build().unwrap()
    };
    let start_instance = |definition: &ProcessDefinition, id: &str| {
        ProcessInstance::start(definition, ProcessInstanceId::new(id).unwrap()).unwrap()
    };
    let evaluate =
        |definition: &ProcessDefinition, instance: &ProcessInstance, inputs: EvaluationInputs| {
            let occurrence = EventOccurrence::new(
                EventOccurrenceId::new("constraint-occurrence").unwrap(),
                EventTypeId::new("finish").unwrap(),
                instance.id().clone(),
                instance.revision(),
            );
            TransitionEvaluator::evaluate(definition, instance, &occurrence, &inputs)
        };

    for (status, code) in [
        (GateStatus::Open, TransitionDecisionCode::WaitingForEvidence),
        (
            GateStatus::WaitingForEvidence,
            TransitionDecisionCode::WaitingForEvidence,
        ),
        (
            GateStatus::WaitingForAuthorization,
            TransitionDecisionCode::WaitingForAuthorization,
        ),
        (GateStatus::Failed, TransitionDecisionCode::GateFailed),
        (GateStatus::Blocked, TransitionDecisionCode::GateFailed),
    ] {
        let definition = make_definition(
            &format!("gate-{:?}", status),
            GuardExpression::Always,
            true,
            false,
            false,
            None,
            0,
        );
        let instance = start_instance(&definition, "gate-run");
        assert_eq!(
            evaluate(
                &definition,
                &instance,
                EvaluationInputs::default().with_gate(review.clone(), status),
            )
            .code(),
            code
        );
    }
    let passed_gate = make_definition(
        "gate-passed",
        GuardExpression::Always,
        true,
        false,
        false,
        None,
        0,
    );
    let passed_instance = start_instance(&passed_gate, "passed-gate-run");
    assert_eq!(
        evaluate(
            &passed_gate,
            &passed_instance,
            EvaluationInputs::default().with_gate(review.clone(), GateStatus::Passed),
        )
        .code(),
        TransitionDecisionCode::Accepted
    );

    for (status, code) in [
        (
            EvidenceStatus::Missing,
            TransitionDecisionCode::WaitingForEvidence,
        ),
        (
            EvidenceStatus::Invalid,
            TransitionDecisionCode::EvidenceInvalid,
        ),
        (
            EvidenceStatus::Failed,
            TransitionDecisionCode::EvidenceInvalid,
        ),
    ] {
        let definition = make_definition(
            &format!("evidence-{:?}", status),
            GuardExpression::Always,
            true,
            true,
            false,
            None,
            0,
        );
        let instance = start_instance(&definition, "evidence-run");
        assert_eq!(
            evaluate(
                &definition,
                &instance,
                EvaluationInputs::default()
                    .with_gate(review.clone(), GateStatus::Passed)
                    .with_evidence_status(report.clone(), status),
            )
            .code(),
            code
        );
    }
    let present_evidence = make_definition(
        "evidence-present",
        GuardExpression::Always,
        true,
        true,
        false,
        None,
        0,
    );
    let present_instance = start_instance(&present_evidence, "present-evidence-run");
    assert_eq!(
        evaluate(
            &present_evidence,
            &present_instance,
            EvaluationInputs::default()
                .with_gate(review.clone(), GateStatus::Passed)
                .with_evidence([report.clone()]),
        )
        .code(),
        TransitionDecisionCode::Accepted
    );

    let gate_evidence = make_definition(
        "gate-evidence",
        GuardExpression::Always,
        false,
        false,
        true,
        None,
        0,
    );
    let gate_evidence_instance = start_instance(&gate_evidence, "gate-evidence-run");
    assert_eq!(
        evaluate(
            &gate_evidence,
            &gate_evidence_instance,
            EvaluationInputs::default(),
        )
        .code(),
        TransitionDecisionCode::WaitingForEvidence
    );
    assert_eq!(
        evaluate(
            &gate_evidence,
            &gate_evidence_instance,
            EvaluationInputs::default().with_evidence([report.clone()]),
        )
        .code(),
        TransitionDecisionCode::Accepted
    );

    let blocker_definition = make_definition(
        "active-blocker",
        GuardExpression::Always,
        false,
        false,
        false,
        None,
        0,
    );
    let mut blocker_instance = start_instance(&blocker_definition, "blocker-run");
    blocker_instance
        .record_blocker(BlockerRuntimeState::new(waiting.clone(), "waiting", true).unwrap());
    assert_eq!(
        evaluate(
            &blocker_definition,
            &blocker_instance,
            EvaluationInputs::default(),
        )
        .code(),
        TransitionDecisionCode::ActiveBlocker
    );

    for (condition, code) in [
        (GuardExpression::Always, TransitionDecisionCode::Accepted),
        (
            GuardExpression::Never,
            TransitionDecisionCode::InvariantViolation,
        ),
    ] {
        let definition = make_definition(
            "invariant-case",
            GuardExpression::Always,
            false,
            false,
            false,
            Some(condition),
            0,
        );
        let instance = start_instance(&definition, "invariant-run");
        assert_eq!(
            evaluate(&definition, &instance, EvaluationInputs::default()).code(),
            code
        );
    }

    for (input, code) in [
        (
            EvaluationInputs::default(),
            TransitionDecisionCode::WaitingForAuthorization,
        ),
        (
            EvaluationInputs::default()
                .with_authorization(authorization.clone(), AuthorizationStatus::Waiting),
            TransitionDecisionCode::WaitingForAuthorization,
        ),
        (
            EvaluationInputs::default()
                .with_authorization(authorization.clone(), AuthorizationStatus::Denied),
            TransitionDecisionCode::AuthorizationDenied,
        ),
    ] {
        let definition = make_definition(
            &format!("authorization-{:?}", code),
            GuardExpression::AuthorizationIs {
                authorization: authorization.clone(),
                status: AuthorizationStatus::Allowed,
            },
            false,
            false,
            false,
            None,
            0,
        );
        let instance = start_instance(&definition, "authorization-run");
        assert_eq!(evaluate(&definition, &instance, input).code(), code);
    }
    let policy_definition = make_definition(
        "policy-denied",
        GuardExpression::PolicyDecisionIs {
            policy: policy.clone(),
            status: PolicyDecisionStatus::Allow,
        },
        false,
        false,
        false,
        None,
        0,
    );
    let policy_instance = start_instance(&policy_definition, "policy-run");
    assert_eq!(
        evaluate(
            &policy_definition,
            &policy_instance,
            EvaluationInputs::default()
                .with_policy_decision(policy.clone(), PolicyDecisionStatus::Deny,),
        )
        .code(),
        TransitionDecisionCode::AuthorizationDenied
    );
    assert_eq!(
        evaluate(
            &policy_definition,
            &policy_instance,
            EvaluationInputs::default()
                .with_policy_decision(policy.clone(), PolicyDecisionStatus::Waiting,),
        )
        .code(),
        TransitionDecisionCode::WaitingForAuthorization
    );

    let all_guard = make_definition(
        "all-auth",
        GuardExpression::All(vec![
            GuardExpression::AuthorizationIs {
                authorization: authorization.clone(),
                status: AuthorizationStatus::Allowed,
            },
            GuardExpression::PolicyDecisionIs {
                policy: policy.clone(),
                status: PolicyDecisionStatus::Allow,
            },
        ]),
        false,
        false,
        false,
        None,
        0,
    );
    let all_instance = start_instance(&all_guard, "all-auth-run");
    assert_eq!(
        evaluate(&all_guard, &all_instance, EvaluationInputs::default()).code(),
        TransitionDecisionCode::WaitingForAuthorization
    );
    let any_guard = make_definition(
        "any-auth",
        GuardExpression::Any(vec![
            GuardExpression::AuthorizationIs {
                authorization: authorization.clone(),
                status: AuthorizationStatus::Allowed,
            },
            GuardExpression::PolicyDecisionIs {
                policy: policy.clone(),
                status: PolicyDecisionStatus::Allow,
            },
        ]),
        false,
        false,
        false,
        None,
        0,
    );
    let any_instance = start_instance(&any_guard, "any-auth-run");
    assert_eq!(
        evaluate(&any_guard, &any_instance, EvaluationInputs::default()).code(),
        TransitionDecisionCode::WaitingForAuthorization
    );
    let not_guard = make_definition(
        "not-auth",
        GuardExpression::Not(Box::new(GuardExpression::AuthorizationIs {
            authorization: authorization.clone(),
            status: AuthorizationStatus::Allowed,
        })),
        false,
        false,
        false,
        None,
        0,
    );
    let not_instance = start_instance(&not_guard, "not-auth-run");
    assert_eq!(
        evaluate(&not_guard, &not_instance, EvaluationInputs::default()).code(),
        TransitionDecisionCode::WaitingForAuthorization
    );

    let paused = make_definition(
        "paused-status",
        GuardExpression::Always,
        false,
        false,
        false,
        None,
        1,
    );
    let paused_instance = start_instance(&paused, "paused-run");
    assert_eq!(
        evaluate(&paused, &paused_instance, EvaluationInputs::default()).code(),
        TransitionDecisionCode::Accepted
    );
    assert_eq!(
        evaluate(&paused, &paused_instance, EvaluationInputs::default())
            .projection()
            .unwrap()
            .target_state()
            .as_str(),
        "middle"
    );
    let blocked = make_definition(
        "blocked-status",
        GuardExpression::Always,
        false,
        false,
        false,
        None,
        2,
    );
    let blocked_instance = start_instance(&blocked, "blocked-run");
    assert_eq!(
        evaluate(&blocked, &blocked_instance, EvaluationInputs::default()).code(),
        TransitionDecisionCode::Accepted
    );
}

#[test]
fn public_error_registry_and_ir_boundaries_are_exercised() {
    let source = ProcessSource::new(
        "catalog/coverage.feature",
        strict_source(
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: finish\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE",
        ),
    );
    assert_eq!(source.path(), "catalog/coverage.feature");
    assert!(source.content().contains("@process(coverage-case)"));

    let registry = ProcessRegistry::from_sources([source]).unwrap();
    assert!(!registry.is_empty());
    assert_eq!(registry.len(), 1);
    assert_eq!(
        registry.entries()[0].source_path(),
        "catalog/coverage.feature"
    );
    assert_eq!(
        registry.entries()[0].definition().identity().id().as_str(),
        "coverage-case"
    );
    let id = ProcessDefinitionId::new("coverage-case").unwrap();
    let version = ProcessDefinitionVersion::new(1).unwrap();
    assert!(registry.get(&id, version).is_some());
    assert!(registry.resolve(&id, Some(version)).is_some());
    assert!(registry.resolve(&id, None).is_some());
    assert!(
        registry
            .get(&id, ProcessDefinitionVersion::new(2).unwrap())
            .is_none()
    );
    assert!(
        registry
            .get(&ProcessDefinitionId::new("missing").unwrap(), version)
            .is_none()
    );
    assert_eq!(registry.definitions().count(), 1);

    let invalid_definition = strict_source(
        "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: first\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE\nScenario: second\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE",
    );
    let registry_error =
        ProcessRegistry::from_sources([ProcessSource::new("invalid.feature", invalid_definition)])
            .unwrap_err();
    assert_eq!(registry_error.code(), "INVALID_DEFINITION");
    assert_eq!(registry_error.source_path(), Some("invalid.feature"));
    assert!(!registry_error.message().is_empty());
    assert!(registry_error.to_string().contains("[invalid.feature]"));

    let app = ProcessApplication::new();
    let compilation_error = app
        .compile_process_source("Feature: incomplete")
        .unwrap_err();
    assert_eq!(compilation_error.code(), "COMPILATION_ERROR");
    assert!(!compilation_error.message().is_empty());
    assert!(compilation_error.to_string().contains("COMPILATION_ERROR"));
    let registry_application_error: ApplicationError =
        ProcessRegistry::from_sources([ProcessSource::new("bad.feature", "Feature: incomplete")])
            .unwrap_err()
            .into();
    assert_eq!(registry_application_error.code(), "COMPILATION_ERROR");
    assert!(!registry_application_error.message().is_empty());

    let compilation = app
        .compile_process_source(&strict_source(
            "Given state START is initial\nGiven state DONE is terminal\nGiven event finish\nScenario: finish\nGiven process state START\nWhen event finish occurs\nThen transition to state DONE",
        ))
        .unwrap();
    let explanation = app.explain_compilation(&compilation);
    assert_eq!(explanation.definition_id().as_str(), "coverage-case");
    assert_eq!(explanation.definition_version().value(), 1);
    assert_eq!(explanation.definition_digest().as_str().len(), 64);
    assert!(!explanation.trace().is_empty());
    assert_eq!(explanation.trace().len(), compilation.trace().len());
    let trace = &explanation.trace()[0];
    assert!(trace.line() > 0);
    assert!(trace.column() > 0);
    assert!(!trace.construct().is_empty());
    assert!(!trace.target().is_empty());
    assert!(
        explanation
            .human_readable()
            .contains("compiled coverage-case")
    );
    assert!(explanation.to_json().unwrap().contains("coverage-case"));

    let instance = app
        .start_process(
            compilation.definition(),
            ProcessInstanceId::new("coverage-app-run").unwrap(),
        )
        .unwrap();
    let rejected_event = EventOccurrence::new(
        EventOccurrenceId::new("rejected-occurrence").unwrap(),
        EventTypeId::new("unknown").unwrap(),
        instance.id().clone(),
        instance.revision(),
    );
    let rejected_decision = app.evaluate_event(
        compilation.definition(),
        &instance,
        &rejected_event,
        &EvaluationInputs::default(),
    );
    let runtime = app.explain_transition(
        compilation.definition(),
        &instance,
        &rejected_event,
        &rejected_decision,
    );
    assert!(runtime.human_readable().starts_with("rejected"));
    assert_eq!(runtime.reason_code(), rejected_decision.code().as_str());
    assert!(runtime.authorized_activity().is_none());
    assert!(runtime.to_json().unwrap().contains("rejected-occurrence"));

    let mut store = InMemoryProcessStore::default();
    store.insert(instance.clone());
    let result = app
        .complete_process(
            &mut store,
            compilation.definition(),
            &instance,
            &rejected_event,
            &EvaluationInputs::default(),
        )
        .unwrap();
    assert!(result.outcome().is_none());
    assert_eq!(
        result.decision().code(),
        TransitionDecisionCode::UnknownEvent
    );

    let invalid_start = ProcessDefinitionBuilder::new(
        ProcessDefinitionId::new("invalid-start").unwrap(),
        ProcessDefinitionVersion::new(1).unwrap(),
    )
    .with_states([
        StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
        StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap(),
        StateDefinition::new(StateId::new("orphan").unwrap(), false, false).unwrap(),
    ])
    .with_events([EventTypeDefinition::new(
        EventTypeId::new("finish").unwrap(),
    )])
    .with_transitions([TransitionDefinition::new(
        TransitionId::new("finish").unwrap(),
        StateId::new("start").unwrap(),
        EventTypeId::new("finish").unwrap(),
        StateId::new("done").unwrap(),
        GuardExpression::Always,
    )])
    .build()
    .unwrap();
    let app_error = app
        .start_process(
            &invalid_start,
            ProcessInstanceId::new("invalid-run").unwrap(),
        )
        .unwrap_err();
    assert_eq!(app_error.code(), "DEFINITION_NOT_VALIDATED");
    assert!(!app_error.message().is_empty());
    assert!(app_error.to_string().contains("DEFINITION_NOT_VALIDATED"));

    let blocker_error =
        BlockerRuntimeState::new(BlockerId::new("invalid-blocker").unwrap(), " ", false)
            .unwrap_err();
    assert_eq!(blocker_error.code(), "INVALID_BLOCKER");
    assert!(!blocker_error.message().is_empty());
    assert!(blocker_error.to_string().contains("INVALID_BLOCKER"));
    let projection_error = TransitionProjection::new(
        ProcessInstanceRevision::initial(),
        TransitionId::new("finish").unwrap(),
        StateId::new("done").unwrap(),
        ProcessInstanceStatus::Running,
        " ",
    )
    .unwrap_err();
    assert_eq!(projection_error.code(), "INVALID_TRANSITION");
    assert!(!projection_error.message().is_empty());
    assert!(projection_error.to_string().contains("INVALID_TRANSITION"));
    let valid_instance = app
        .start_process(
            compilation.definition(),
            ProcessInstanceId::new("projection-run").unwrap(),
        )
        .unwrap();
    let unknown_projection = TransitionProjection::new(
        valid_instance.revision(),
        TransitionId::new("missing").unwrap(),
        StateId::new("done").unwrap(),
        ProcessInstanceStatus::Running,
        "unknown",
    )
    .unwrap();
    assert_eq!(
        valid_instance
            .clone()
            .apply_projection(compilation.definition(), unknown_projection)
            .unwrap_err()
            .code(),
        "UNKNOWN_TRANSITION"
    );
    let illegal_projection = TransitionProjection::new(
        valid_instance.revision(),
        compilation.definition().transitions()[0].id().clone(),
        valid_instance.current_state().clone(),
        ProcessInstanceStatus::Running,
        "illegal",
    )
    .unwrap();
    assert_eq!(
        valid_instance
            .clone()
            .apply_projection(compilation.definition(), illegal_projection)
            .unwrap_err()
            .code(),
        "ILLEGAL_STATE_PROJECTION"
    );

    for (source, code) in [
        (
            "@process(x)\n@process-version(1)\n@cg-language(1)\nFeature: x\nWhen event finish",
            "STEP_OUTSIDE_RULE",
        ),
        (
            "@process(x)\n@process-version(1)\n@cg-language(1)\nFeature: x\n| one |",
            "TABLE_OUTSIDE_RULE",
        ),
        (
            "@process\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process",
            "INVALID_TAG",
        ),
    ] {
        assert_eq!(SourceDocument::parse(source).unwrap_err().code(), code);
    }

    let tampered = compilation.definition().to_json().unwrap().replace(
        compilation.definition().identity().digest().as_str(),
        &"0".repeat(64),
    );
    assert_eq!(
        ProcessDefinition::from_json(&tampered).unwrap_err().code(),
        ValidationCode::NonCanonicalDefinition
    );

    let mut builder = ProcessDefinitionBuilder::new(
        ProcessDefinitionId::new("ir-errors").unwrap(),
        ProcessDefinitionVersion::new(1).unwrap(),
    );
    assert_eq!(
        builder.clone().build().unwrap_err().code(),
        ValidationCode::EmptyDefinition
    );
    builder = builder
        .with_states([
            StateDefinition::new(StateId::new("a").unwrap(), false, false).unwrap(),
            StateDefinition::new(StateId::new("b").unwrap(), false, true).unwrap(),
        ])
        .with_events([EventTypeDefinition::new(EventTypeId::new("go").unwrap())])
        .with_transitions([TransitionDefinition::new(
            TransitionId::new("go").unwrap(),
            StateId::new("a").unwrap(),
            EventTypeId::new("go").unwrap(),
            StateId::new("b").unwrap(),
            GuardExpression::Always,
        )]);
    assert_eq!(
        builder.build().unwrap_err().code(),
        ValidationCode::MissingInitialState
    );

    let multiple_initial = ProcessDefinitionBuilder::new(
        ProcessDefinitionId::new("multiple-initial").unwrap(),
        ProcessDefinitionVersion::new(1).unwrap(),
    )
    .with_states([
        StateDefinition::new(StateId::new("a").unwrap(), true, false).unwrap(),
        StateDefinition::new(StateId::new("b").unwrap(), true, false).unwrap(),
    ])
    .with_events([EventTypeDefinition::new(EventTypeId::new("go").unwrap())])
    .with_transitions([TransitionDefinition::new(
        TransitionId::new("go").unwrap(),
        StateId::new("a").unwrap(),
        EventTypeId::new("go").unwrap(),
        StateId::new("b").unwrap(),
        GuardExpression::Always,
    )]);
    assert_eq!(
        multiple_initial.build().unwrap_err().code(),
        ValidationCode::MultipleInitialStates
    );
    assert_eq!(ProcessIrVersion::V1.to_string(), "V1");
}
