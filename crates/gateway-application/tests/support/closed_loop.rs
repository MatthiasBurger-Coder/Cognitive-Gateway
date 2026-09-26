use super::*;
use gateway_application::{
    PlanningCapabilitySnapshot, ScopedObservationBatch, SourceSnapshot, closed_loop::*,
};

fn scope() -> ContextScopeId {
    ContextScopeId::new("project-a").unwrap()
}
fn subject() -> SubjectPath {
    SubjectPath::new(["quality", "passed"]).unwrap()
}
fn batch(value: Option<bool>, evidence: bool, fresh: bool) -> ScopedObservationBatch {
    let records = if let Some(value) = value {
        let provenance = Provenance::new(
            ProvenanceId::new("tool").unwrap(),
            SourceKind::Tool,
            SourceId::new("tool").unwrap(),
            "fixture://report",
        )
        .unwrap();
        let observation = Observation::new(
            ObservationId::new("observation").unwrap(),
            subject(),
            TypedValue::Boolean(value),
            provenance.id().clone(),
        )
        .unwrap();
        let fact = Fact::new(
            FactId::new("fact").unwrap(),
            subject(),
            TypedValue::Boolean(value),
            AssertionPolarity::Affirmed,
            vec![observation.id().clone()],
        )
        .unwrap();
        let reports = if evidence {
            vec![
                Evidence::new(
                    EvidenceId::new("report").unwrap(),
                    EvidenceKind::Report,
                    "test report",
                    EvidenceContent::inline("captured tool output").unwrap(),
                    provenance.id().clone(),
                    vec![EvidenceLink::new(
                        fact.id().clone(),
                        EvidenceRelation::Supports,
                    )],
                )
                .unwrap(),
            ]
        } else {
            vec![]
        };
        ObservationEvidenceSet::new(vec![provenance], vec![observation], vec![fact], reports)
            .unwrap()
    } else {
        ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap()
    };
    let batch = ScopedObservationBatch::new(
        scope(),
        SourceSnapshot::new(
            SourceId::new("tool").unwrap(),
            SourceKind::Tool,
            None,
            Some(ContentDigest::new("a".repeat(64)).unwrap()),
        )
        .unwrap(),
        records,
    )
    .unwrap();
    if fresh {
        batch.with_quality_metadata(
            subject(),
            vec![QualityMetadata::new(
                TrustClass::ObservedEvidence,
                SensitivityClass::Public,
                Confidence::Unknown,
                FreshnessStatus::Fresh,
                Uncertainty::None,
            )],
        )
    } else {
        batch
    }
}
fn loop_rules(iterations: u32, retries: u32) -> LoopRules {
    LoopRules {
        capabilities: PlanningCapabilitySnapshot::new(
            input().index,
            "fixture",
            PlanningIrVersion::V1,
        )
        .unwrap(),
        requirements: CapabilityRequirementRules::default()
            .with_observation(CapabilityId::new("architecture.dependency-analysis").unwrap())
            .with_evidence_acquisition(
                CapabilityId::new("architecture.dependency-analysis").unwrap(),
            ),
        planner: PlannerRules::default(),
        max_iterations: iterations,
        max_retries: retries,
    }
}
fn start(iterations: u32, retries: u32) -> ClosedLoop {
    ClosedLoop::start(
        ReferenceId::new("run-1").unwrap(),
        scope(),
        Intent::new(IntentId::new("intent").unwrap(), input().desired)
            .with_original_input(OriginalInput::inline("Ensure quality passes").unwrap()),
        batch(None, false, false),
        loop_rules(iterations, retries),
    )
    .unwrap()
}
fn fixture_for(run: &ClosedLoop) -> Fixture {
    let mut input = input();
    let assessment = run.assessment();
    input.plan = assessment.plan.clone().unwrap();
    input.delta = assessment.delta.clone();
    input.desired = assessment
        .document
        .intent()
        .unwrap()
        .desired_state()
        .clone();
    input.situation = assessment.document.clone();
    Fixture::from_input(input)
}
struct Runtime {
    status: OutcomeStatus,
    batch: Option<ScopedObservationBatch>,
    calls: usize,
    wrong_id: bool,
}
impl ExecutionRuntimePort for Runtime {
    fn execute(&mut self, execution: &ReferenceId, _: &CompiledStep) -> ExecutionOutcome {
        self.calls += 1;
        ExecutionOutcome {
            execution: if self.wrong_id {
                ReferenceId::new("wrong").unwrap()
            } else {
                execution.clone()
            },
            status: self.status,
            observations: self.batch.clone(),
        }
    }
}
fn runtime(batch: Option<ScopedObservationBatch>) -> Runtime {
    Runtime {
        status: OutcomeStatus::Completed,
        batch,
        calls: 0,
        wrong_id: false,
    }
}
fn execute(
    run: &mut ClosedLoop,
    fixture: &Fixture,
    runtime: &mut Runtime,
) -> Result<LoopDecision, LoopError> {
    run.execute(
        CompileStepInput {
            resolved: &fixture.resolved,
            authority: &fixture.authority,
            policy_context: &fixture.policy,
            catalog: &fixture.catalog,
            projection: &fixture.projection,
            candidates: &[],
            selected: &BTreeSet::new(),
        },
        runtime,
    )
}

#[test]
fn observes_success_and_retains_complete_deterministic_audit() {
    let mut run = start(4, 1);
    assert_eq!(run.decision(), LoopDecision::Replan);
    assert!(run.pending_execution().is_none());
    let fixture = fixture_for(&run);
    let registry = fixture.resolved.snapshot.input().registry.clone();
    let mut runtime = runtime(Some(batch(Some(true), true, true)));
    assert_eq!(
        execute(&mut run, &fixture, &mut runtime),
        Ok(LoopDecision::Success)
    );
    assert_eq!(run.iterations(), 1);
    assert_eq!(runtime.calls, 1);
    assert_eq!(fixture.resolved.snapshot.input().registry, registry);
    assert!(run.assessment().delta.is_noop());
    let json: serde_json::Value = serde_json::from_str(&run.to_json().unwrap()).unwrap();
    assert_eq!(json["audit"][1]["event"], "EXECUTION");
    assert_eq!(json["audit"][2]["event"], "OUTCOME");
    assert_eq!(json["audit"][3]["reason"], "GOAL_SATISFIED");
    assert_eq!(json["audit"][3]["assessment"]["goal_outcome"], "SATISFIED");
    let mut replay = start(4, 1);
    execute(&mut replay, &fixture, &mut runtime).unwrap();
    assert_eq!(run.to_json().unwrap(), replay.to_json().unwrap());
    assert_eq!(
        execute(&mut run, &fixture, &mut runtime),
        Err(LoopError::NotReady)
    );
    assert_eq!(
        run.refresh(batch(None, false, false)),
        Err(LoopError::NotReady)
    );
    assert_eq!(run.stop(), Err(LoopError::NotReady));
}

#[test]
fn unchanged_plan_requires_fresh_authority_and_stops_at_retry_budget() {
    let mut run = start(5, 1);
    let first = fixture_for(&run);
    let mut runtime = runtime(Some(batch(None, false, false)));
    runtime.status = OutcomeStatus::RetryableFailure;
    assert_eq!(
        execute(&mut run, &first, &mut runtime),
        Ok(LoopDecision::Continue)
    );
    assert_eq!(run.retries(), 1);
    assert_eq!(
        execute(&mut run, &first, &mut runtime),
        Err(LoopError::StaleResolution)
    );
    assert_eq!(runtime.calls, 1);
    let second = fixture_for(&run);
    assert_eq!(
        execute(&mut run, &second, &mut runtime),
        Ok(LoopDecision::Stopped)
    );
    assert_eq!(run.audit().last().unwrap()["reason"], "RETRY_BUDGET");
    assert_eq!(run.retries(), 2);
}

#[test]
fn missing_evidence_pauses_and_refresh_preserves_iteration_budget() {
    let mut run = start(1, 10);
    let fixture = fixture_for(&run);
    let mut runtime = runtime(None);
    assert_eq!(
        execute(&mut run, &fixture, &mut runtime),
        Ok(LoopDecision::Pause)
    );
    assert_eq!(run.audit().last().unwrap()["reason"], "MISSING_EVIDENCE");
    assert_eq!(
        execute(&mut run, &fixture, &mut runtime),
        Err(LoopError::NotReady)
    );
    assert_eq!(
        run.refresh(batch(None, false, false)),
        Ok(LoopDecision::Stopped)
    );
    assert_eq!(run.audit().last().unwrap()["reason"], "ITERATION_BUDGET");
    assert_eq!(runtime.calls, 1);
    let zero = start(0, 10);
    assert_eq!(zero.decision(), LoopDecision::Stopped);
}

#[test]
fn evidence_changes_replan_and_claims_without_fresh_support_never_succeed() {
    for observations in [
        batch(Some(true), false, true),
        batch(Some(true), true, false),
    ] {
        let mut run = start(4, 3);
        let fixture = fixture_for(&run);
        let mut runtime = runtime(Some(observations));
        assert_eq!(
            execute(&mut run, &fixture, &mut runtime),
            Ok(LoopDecision::Replan)
        );
        assert_ne!(
            run.assessment().comparison.outcome(),
            ComparisonOutcome::Satisfied
        );
        assert_eq!(run.audit().last().unwrap()["reason"], "CHANGED_SITUATION");
    }
    let mut run = start(4, 3);
    let fixture = fixture_for(&run);
    let mut runtime = runtime(Some(batch(Some(false), true, true)));
    // This fixture deliberately has no mutation capability: replanning fails closed.
    assert_eq!(
        execute(&mut run, &fixture, &mut runtime),
        Ok(LoopDecision::Pause)
    );
    assert_eq!(run.audit().last().unwrap()["reason"], "PLANNING_BLOCKED");
    assert!(run.assessment().plan.is_none());
    assert_eq!(
        run.refresh(batch(Some(true), true, true)),
        Ok(LoopDecision::Success)
    );
}

#[test]
fn hard_failures_and_blockers_override_even_satisfied_goals() {
    for (status, reason) in [
        (OutcomeStatus::HardFailure, "HARD_FAILURE"),
        (OutcomeStatus::Blocked, "EXPLICIT_BLOCKER"),
    ] {
        let mut run = start(3, 3);
        let fixture = fixture_for(&run);
        let mut runtime = runtime(Some(batch(Some(true), true, true)));
        runtime.status = status;
        assert_eq!(
            execute(&mut run, &fixture, &mut runtime),
            Ok(LoopDecision::Stopped)
        );
        assert_eq!(run.audit().last().unwrap()["reason"], reason);
        assert_eq!(
            run.assessment().comparison.outcome(),
            ComparisonOutcome::Satisfied
        );
    }
    let mut run = start(3, 3);
    run.stop().unwrap();
    assert_eq!(run.decision(), LoopDecision::Stopped);
}

#[test]
fn invalid_scope_and_correlations_leave_pending_attempt_unrepeated() {
    let mut run = start(3, 3);
    let fixture = fixture_for(&run);
    let mut runtime = runtime(Some(batch(Some(true), true, true)));
    runtime.wrong_id = true;
    assert_eq!(
        execute(&mut run, &fixture, &mut runtime),
        Err(LoopError::StaleExecution)
    );
    let id = run.pending_execution().unwrap().clone();
    assert_eq!(
        execute(&mut run, &fixture, &mut runtime),
        Err(LoopError::NotReady)
    );
    assert_eq!(run.stop(), Err(LoopError::NotReady));
    let original = batch(None, false, false);
    let wrong_scope = ScopedObservationBatch::new(
        ContextScopeId::new("other").unwrap(),
        original.snapshot().clone(),
        original.records().clone(),
    )
    .unwrap();
    assert_eq!(
        run.ingest(ExecutionOutcome {
            execution: id.clone(),
            status: OutcomeStatus::Completed,
            observations: Some(wrong_scope.clone())
        }),
        Err(LoopError::ScopeMismatch)
    );
    assert_eq!(
        ClosedLoop::start(
            ReferenceId::new("run-1").unwrap(),
            scope(),
            Intent::new(IntentId::new("intent").unwrap(), input().desired),
            wrong_scope,
            loop_rules(1, 1)
        )
        .unwrap_err(),
        LoopError::ScopeMismatch
    );
    let outcome = ExecutionOutcome {
        execution: id,
        status: OutcomeStatus::Completed,
        observations: runtime.batch,
    };
    assert_eq!(run.ingest(outcome.clone()), Ok(LoopDecision::Success));
    assert_eq!(run.ingest(outcome), Err(LoopError::StaleExecution));
    assert_eq!(runtime.calls, 1);
}

#[test]
fn policy_denials_stop_and_missing_authority_pauses_before_runtime() {
    for denied in [false, true] {
        let mut run = start(3, 3);
        let mut fixture = fixture_for(&run);
        for facts in fixture.policy.steps.values_mut() {
            if denied {
                facts
                    .authorizations
                    .values_mut()
                    .for_each(|value| *value = Approval::Denied);
            } else {
                facts.authorizations.clear();
            }
        }
        let mut runtime = runtime(None);
        assert!(matches!(
            execute(&mut run, &fixture, &mut runtime),
            Err(LoopError::Compilation(_))
        ));
        assert_eq!(runtime.calls, 0);
        assert_eq!(run.iterations(), 0);
        assert_eq!(
            run.decision(),
            if denied {
                LoopDecision::Stopped
            } else {
                LoopDecision::Pause
            }
        );
        if !denied {
            assert_eq!(
                run.refresh(batch(None, false, false)),
                Ok(LoopDecision::Replan)
            );
            let fixture = fixture_for(&run);
            runtime.batch = Some(batch(Some(true), true, true));
            assert_eq!(
                execute(&mut run, &fixture, &mut runtime),
                Ok(LoopDecision::Success)
            );
        }
    }
}

#[test]
fn stale_projection_wrong_step_and_process_pause_cannot_dispatch() {
    let mut run = start(3, 3);
    let mut fixture = fixture_for(&run);
    let mut runtime = runtime(None);
    fixture.projection.mapping.step = PlanStepId::new("other").unwrap();
    assert_eq!(
        execute(&mut run, &fixture, &mut runtime),
        Err(LoopError::WrongStep)
    );
    let mut fixture = fixture_for(&run);
    fixture.projection.state_basis.situation = SituationId::new("stale").unwrap();
    assert_eq!(
        execute(&mut run, &fixture, &mut runtime),
        Err(LoopError::Compilation(
            ContextApplicationError::StaleMapping
        ))
    );
    assert_eq!(run.decision(), LoopDecision::Pause);
    assert_eq!(runtime.calls, 0);
}

#[test]
fn acceptance_criteria_are_checked_even_when_primary_goal_is_satisfied() {
    let desired = input().desired;
    let missing = DesiredCondition::new(
        ConditionId::new("acceptance").unwrap(),
        SubjectPath::new(["tests", "passed"]).unwrap(),
        ComparisonOperator::Equals,
        Some(TypedValue::Boolean(true)),
    )
    .unwrap();
    let desired = DesiredState::new(
        desired.id().clone(),
        vec![desired.conditions()[0].clone(), missing.clone()],
        desired.expression().clone(),
        vec![],
        vec![
            AcceptanceCriterion::new(
                AcceptanceCriterionId::new("tests").unwrap(),
                "tests must pass",
                ConditionExpression::condition(missing.id().clone()),
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let run = ClosedLoop::start(
        ReferenceId::new("run-1").unwrap(),
        scope(),
        Intent::new(IntentId::new("intent").unwrap(), desired),
        batch(Some(true), true, true),
        loop_rules(3, 3),
    )
    .unwrap();
    assert_ne!(run.decision(), LoopDecision::Success);
    assert_eq!(run.assessment().delta.actionable_items().len(), 1);
    let satisfied = ClosedLoop::start(
        ReferenceId::new("run-1").unwrap(),
        scope(),
        Intent::new(IntentId::new("intent").unwrap(), input().desired),
        batch(Some(true), true, true),
        loop_rules(0, 0),
    )
    .unwrap();
    assert_eq!(satisfied.decision(), LoopDecision::Success);
}

#[test]
fn successive_observations_finish_only_the_remaining_steps() {
    let original = input().desired;
    let second = DesiredCondition::new(
        ConditionId::new("second").unwrap(),
        SubjectPath::new(["second", "passed"]).unwrap(),
        ComparisonOperator::Equals,
        Some(TypedValue::Boolean(true)),
    )
    .unwrap();
    let desired = DesiredState::new(
        original.id().clone(),
        vec![original.conditions()[0].clone(), second.clone()],
        ConditionExpression::all(vec![
            original.expression().clone(),
            ConditionExpression::condition(second.id().clone()),
        ])
        .unwrap(),
        vec![],
        vec![],
    )
    .unwrap();
    let mut run = ClosedLoop::start(
        ReferenceId::new("multi-step").unwrap(),
        scope(),
        Intent::new(IntentId::new("intent").unwrap(), desired),
        batch(None, false, false),
        loop_rules(4, 2),
    )
    .unwrap();
    assert_eq!(run.assessment().plan.as_ref().unwrap().steps().len(), 2);
    let first = fixture_for(&run);
    let mut runtime = runtime(Some(batch(Some(true), true, true)));
    assert_eq!(
        execute(&mut run, &first, &mut runtime),
        Ok(LoopDecision::Continue)
    );
    assert_eq!(run.retries(), 0);
    assert_eq!(run.assessment().delta.actionable_items().len(), 1);
    let next = fixture_for(&run);
    assert_ne!(next.projection.mapping.step, first.projection.mapping.step);
    let first_batch = batch(Some(true), true, true);
    let records = first_batch.records();
    let provenance = records.provenances()[0].clone();
    let observation = Observation::new(
        ObservationId::new("second-observation").unwrap(),
        second.subject().clone(),
        TypedValue::Boolean(true),
        provenance.id().clone(),
    )
    .unwrap();
    let fact = Fact::new(
        FactId::new("second-fact").unwrap(),
        second.subject().clone(),
        TypedValue::Boolean(true),
        AssertionPolarity::Affirmed,
        vec![observation.id().clone()],
    )
    .unwrap();
    let evidence = Evidence::new(
        EvidenceId::new("second-report").unwrap(),
        EvidenceKind::Report,
        "second report",
        EvidenceContent::inline("passed").unwrap(),
        provenance.id().clone(),
        vec![EvidenceLink::new(
            fact.id().clone(),
            EvidenceRelation::Supports,
        )],
    )
    .unwrap();
    let records = ObservationEvidenceSet::new(
        vec![provenance],
        vec![records.observations()[0].clone(), observation],
        vec![records.facts()[0].clone(), fact],
        vec![records.evidence()[0].clone(), evidence],
    )
    .unwrap();
    let both = ScopedObservationBatch::new(scope(), first_batch.snapshot().clone(), records)
        .unwrap()
        .with_quality_metadata(
            subject(),
            first_batch.quality_metadata()[&subject()].clone(),
        )
        .with_quality_metadata(
            second.subject().clone(),
            first_batch.quality_metadata()[&subject()].clone(),
        );
    runtime.batch = Some(both);
    assert_eq!(
        execute(&mut run, &next, &mut runtime),
        Ok(LoopDecision::Success)
    );
    assert_eq!(run.iterations(), 2);
    assert_eq!(run.retries(), 0);
}

#[test]
fn paused_authoritative_process_prevents_runtime_invocation() {
    use gateway_application::{DeclarativeSituationApplication, ProcessSnapshotInput};
    use gateway_process::{LifecycleController, PauseReason};
    let mut run = start(3, 3);
    let fixture = fixture_for(&run);
    let mut snapshot = fixture.resolved.snapshot.input().clone();
    LifecycleController::pause(
        snapshot.instance.as_mut().unwrap(),
        PauseReason::HumanReview,
        "review",
    )
    .unwrap();
    snapshot.expected_revision = Some(snapshot.instance.as_ref().unwrap().revision());
    snapshot.situation_process = Some(
        DeclarativeSituationApplication::new()
            .process_reference(ProcessSnapshotInput::new(
                snapshot.processes.definitions().next().unwrap(),
                snapshot.instance.as_ref().unwrap(),
            ))
            .unwrap(),
    );
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(&snapshot, &rules())
        .unwrap();
    let mut fixture = fixture;
    fixture.projection.mapping.basis = resolved.report.basis.clone();
    fixture.projection.state_basis = resolved.report.basis.clone();
    fixture.policy.basis = resolved.report.basis.clone();
    fixture.resolved = resolved;
    let mut runtime = runtime(None);
    assert_eq!(
        execute(&mut run, &fixture, &mut runtime),
        Err(LoopError::Compilation(
            ContextApplicationError::NotAuthorized(PolicyDecision::Deny)
        ))
    );
    assert_eq!(runtime.calls, 0);
    assert_eq!(run.decision(), LoopDecision::Stopped);
}

#[test]
fn constraints_and_changed_evidence_remain_goal_requirements() {
    let desired = input().desired;
    let constraint = DesiredCondition::new(
        ConditionId::new("constraint").unwrap(),
        SubjectPath::new(["boundary", "valid"]).unwrap(),
        ComparisonOperator::Equals,
        Some(TypedValue::Boolean(true)),
    )
    .unwrap();
    let desired = DesiredState::new(
        desired.id().clone(),
        vec![desired.conditions()[0].clone(), constraint.clone()],
        desired.expression().clone(),
        vec![DeclarativeConstraint::new(
            ConstraintId::new("boundary").unwrap(),
            ConditionExpression::condition(constraint.id().clone()),
        )],
        vec![],
    )
    .unwrap();
    let run = ClosedLoop::start(
        ReferenceId::new("constrained").unwrap(),
        scope(),
        Intent::new(IntentId::new("intent").unwrap(), desired),
        batch(Some(true), true, true),
        loop_rules(3, 3),
    )
    .unwrap();
    assert_ne!(run.decision(), LoopDecision::Success);
    assert_eq!(run.assessment().delta.actionable_items().len(), 1);

    // Same semantic gap, different underlying evidence: continuation is invalid.
    let mut run = ClosedLoop::start(
        ReferenceId::new("evidence-change").unwrap(),
        scope(),
        Intent::new(IntentId::new("intent").unwrap(), input().desired),
        batch(Some(true), false, true),
        loop_rules(3, 3),
    )
    .unwrap();
    let fixture = fixture_for(&run);
    let mut runtime = runtime(Some(batch(Some(false), false, true)));
    assert_eq!(
        execute(&mut run, &fixture, &mut runtime),
        Ok(LoopDecision::Replan)
    );
    assert_eq!(run.audit().last().unwrap()["reason"], "CHANGED_SITUATION");
}

#[test]
fn mutation_requires_separate_consent_on_every_attempt() {
    let mut template = input();
    fn mutation_contract(document: String) -> String {
        let mut value: serde_json::Value = serde_json::from_str(&document).unwrap();
        for capability in value["provided_capabilities"].as_array_mut().unwrap() {
            if capability["id"] == "architecture.dependency-analysis" {
                capability["class"] = "MUTATE".into();
                capability["constraints"] = serde_json::json!([]);
            }
        }
        value.to_string()
    }
    template.registry = gateway_registry::Registry::from_documents(
        template.registry.agents().iter().map(|a| {
            AgentDefinitionDocument::from_json(&mutation_contract(a.to_json().unwrap())).unwrap()
        }),
        template.registry.skills().iter().map(|s| {
            SkillDefinitionDocument::from_json(&mutation_contract(s.to_json().unwrap())).unwrap()
        }),
    )
    .unwrap();
    template.index = template.registry.capability_index().unwrap();
    let mut rules = loop_rules(3, 3);
    rules.capabilities = PlanningCapabilitySnapshot::new(
        template.index.clone(),
        "mutation-fixture",
        PlanningIrVersion::V1,
    )
    .unwrap();
    rules.requirements = CapabilityRequirementRules::default()
        .with_domain_change(CapabilityId::new("architecture.dependency-analysis").unwrap());
    let mut run = ClosedLoop::start(
        ReferenceId::new("mutation").unwrap(),
        scope(),
        Intent::new(IntentId::new("intent").unwrap(), template.desired.clone()),
        batch(Some(false), true, true),
        rules,
    )
    .unwrap();
    let prepare = |run: &ClosedLoop| {
        let mut snapshot = template.clone();
        snapshot.plan = run.assessment().plan.clone().unwrap();
        snapshot.delta = run.assessment().delta.clone();
        snapshot.situation = run.assessment().document.clone();
        snapshot.desired = snapshot.situation.intent().unwrap().desired_state().clone();
        Fixture::from_input(snapshot)
    };
    let fixture = prepare(&run);
    let mut runtime = runtime(Some(batch(Some(true), true, true)));
    assert_eq!(
        execute(&mut run, &fixture, &mut runtime),
        Err(LoopError::Compilation(
            ContextApplicationError::NotAuthorized(PolicyDecision::RequireConsent)
        ))
    );
    assert_eq!(runtime.calls, 0);
    run.refresh(batch(Some(false), true, true)).unwrap();
    let mut fixture = prepare(&run);
    for facts in fixture.policy.steps.values_mut() {
        facts.consents = facts.authorizations.clone();
    }
    assert_eq!(
        execute(&mut run, &fixture, &mut runtime),
        Ok(LoopDecision::Success)
    );
    assert_eq!(runtime.calls, 1);
}
