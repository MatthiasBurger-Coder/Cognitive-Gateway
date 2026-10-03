use super::*;
use gateway_application::{
    ScopedObservationBatch, SourceSnapshot, closed_loop::*, procedure_promotion::*, reflex::*,
};
use gateway_domain::{learning::*, procedure_evaluation::*, procedure_promotion::*};
#[path = "../../../../tests/support/procedure_promotion.rs"]
mod promotion;

fn batch(value: bool, time: &str, marker: &str, evidence_present: bool) -> ScopedObservationBatch {
    let provenance = Provenance::new(
        ProvenanceId::new("source").unwrap(),
        SourceKind::Tool,
        SourceId::new("tool").unwrap(),
        "trusted tool result",
    )
    .unwrap();
    let observation = Observation::new(
        ObservationId::new("observation").unwrap(),
        SubjectPath::new(if value {
            ["quality", "passed"]
        } else {
            ["repository", "available"]
        })
        .unwrap(),
        TypedValue::Boolean(value),
        provenance.id().clone(),
    )
    .unwrap()
    .with_occurred_at(SourceTimestamp::new(time).unwrap());
    let fact = Fact::new(
        FactId::new("fact").unwrap(),
        observation.subject().clone(),
        observation.value().clone(),
        AssertionPolarity::Affirmed,
        vec![observation.id().clone()],
    )
    .unwrap();
    let evidence = Evidence::new(
        EvidenceId::new("evidence").unwrap(),
        EvidenceKind::TestResult,
        "result",
        EvidenceContent::inline("checked").unwrap(),
        provenance.id().clone(),
        vec![EvidenceLink::new(
            fact.id().clone(),
            EvidenceRelation::Supports,
        )],
    )
    .unwrap()
    .with_occurred_at(SourceTimestamp::new(time).unwrap());
    ScopedObservationBatch::new(
        ContextScopeId::new("project-a").unwrap(),
        SourceSnapshot::new(
            SourceId::new("tool").unwrap(),
            SourceKind::Tool,
            Some(SourceTimestamp::new(marker).unwrap()),
            None,
        )
        .unwrap(),
        ObservationEvidenceSet::new(
            vec![provenance],
            vec![observation],
            vec![fact],
            if evidence_present {
                vec![evidence]
            } else {
                vec![]
            },
        )
        .unwrap(),
    )
    .unwrap()
}
fn fixture() -> Fixture {
    let mut input = input();
    let records = batch(false, "20", "before", true).records().clone();
    let state = normalize_current_state(
        ObservedStateId::new("current").unwrap(),
        NormalizationInput::new(records.clone())
            .with_required_evidence(true)
            .with_unknown_subjects([SubjectPath::new(["quality", "passed"]).unwrap()])
            .unwrap(),
    )
    .unwrap();
    let situation = SituationAssemblyInput::new(state.clone())
        .with_records(records.clone())
        .assemble(SituationId::new("situation").unwrap())
        .unwrap();
    input.delta = gateway_application::DeclarativePlanningApplication
        .derive_delta(
            DeltaId::new("delta").unwrap(),
            &input.desired,
            &state,
            Some(&situation),
            &ComparisonRules::default(),
            &DeltaDerivationRules::default(),
        )
        .unwrap()
        .delta()
        .clone();
    let requirements = input
        .delta
        .items()
        .iter()
        .map(|item| {
            CapabilityRequirement::new(
                CapabilityRequirementId::new("requirement").unwrap(),
                CapabilityId::new("architecture.dependency-analysis").unwrap(),
                RequirementCardinality::Mandatory,
                item.id().clone(),
                "bounded reflex",
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    input.plan = plan(
        &input.desired,
        &input.delta,
        &requirements,
        &PlannerRules::default(),
    )
    .unwrap()
    .plan()
    .unwrap()
    .clone();
    input.situation = DeclarativeContextSituationDocument::new(
        DeclarativeContext::new_v1(DeclarativeContextId::new("context").unwrap()),
        None,
        Some(records),
        state,
        situation,
    )
    .unwrap();
    Fixture::from_input(input)
}
fn procedure(f: &Fixture) -> LearnedProcedure {
    let original = promotion::procedure(1);
    let fingerprint = SituationFingerprint::new(
        ContextScopeId::new("project-a").unwrap(),
        vec![
            FingerprintSignal::OperatingMode(OperatingMode::Hardening),
            FingerprintSignal::Fact(FactId::new("fact").unwrap()),
        ],
    )
    .unwrap();
    let experience = original
        .experience()
        .iter()
        .map(|e| {
            let mut memory = e.memory().clone();
            memory.scope = fingerprint.scope().clone();
            ExperienceBasis::new(memory, e.provenance().clone(), e.evaluation().clone()).unwrap()
        })
        .collect();
    let candidate =
        PatternCandidate::new(promotion::id("candidate"), fingerprint, experience).unwrap();
    let process = &f.projection.mapping.process;
    LearnedProcedure::new(
        promotion::id("reflex"),
        1,
        &candidate,
        vec![ProcedureStep::new(
            ProcessReference::new(
                promotion::id(process.id().as_str()),
                process.version().to_string().parse().unwrap(),
                ContentDigest::new(process.digest().as_str()).unwrap(),
            )
            .unwrap(),
            CapabilityId::new("architecture.dependency-analysis").unwrap(),
            PolicyId::new("fixture-policy").unwrap(),
        )],
        vec![ObservationId::new("observation").unwrap()],
        vec![EvidenceId::new("evidence").unwrap()],
        vec![EvidenceId::new("evidence").unwrap()],
        FallbackBehavior::ReturnToPlanner,
    )
    .unwrap()
}
fn history(p: &LearnedProcedure, active: bool) -> PromotionJournal {
    let version = ProcedureVersion::of(p);
    let snapshot = ReplaySnapshot {
        id: promotion::id("snapshot"),
        at: 20,
        scope: p.fingerprint().scope().clone(),
        signals: p.fingerprint().signals().iter().cloned().collect(),
        observations: p
            .required_observations()
            .iter()
            .map(|id| (id.clone(), InputStatus::Present))
            .collect(),
        evidence: p
            .required_evidence()
            .iter()
            .map(|id| (id.clone(), InputStatus::Present))
            .collect(),
        verification: p
            .verification_evidence()
            .iter()
            .map(|id| (id.clone(), InputStatus::Present))
            .collect(),
        experience: p
            .experience()
            .iter()
            .map(|e| (e.memory().id.clone(), InputStatus::Present))
            .collect(),
        steps: p
            .steps()
            .iter()
            .map(|s| StepSnapshot {
                process: s.process().clone(),
                capability: s.capability().clone(),
                policy: s.policy().clone(),
                capability_available: true,
                process_allowed: true,
                policy_allowed: true,
                process_trace: "current process".into(),
                policy_trace: "current policy".into(),
                execution: InputStatus::Present,
            })
            .collect(),
    };
    let positive = ReplayCase::new(
        promotion::id("positive"),
        CaseKind::HistoricalSuccess,
        ReplayOutcome::Success,
        snapshot,
    );
    let mut cases = counterfactuals(p, &positive).unwrap();
    let mut failure = positive.snapshot.clone();
    failure.steps[0].execution = InputStatus::Failed;
    cases.push(ReplayCase::new(
        promotion::id("historical-failure"),
        CaseKind::HistoricalFailure,
        ReplayOutcome::ExecutionFailed,
        failure,
    ));
    cases.push(positive);
    let bundle = EvaluationBundle::evaluate(
        p,
        EvaluationDataset {
            schema_version: 1,
            id: promotion::id("dataset"),
            version: 1,
            cases,
        },
        promotion::id("runtime"),
    )
    .unwrap();
    assert!(bundle.report.passed, "{:?}", bundle.report);
    let digest = bundle.digest.clone();
    let commands = vec![
        PromotionCommand::Discover {
            procedure: Box::new(p.clone()),
            discovery_evidence: promotion::id("source"),
        },
        PromotionCommand::Advance {
            procedure: version.clone(),
            from: PromotionState::Discovered,
            to: PromotionState::Candidate,
            evidence: promotion::id("source"),
        },
        PromotionCommand::Advance {
            procedure: version.clone(),
            from: PromotionState::Candidate,
            to: PromotionState::Validated,
            evidence: promotion::id("source"),
        },
        PromotionCommand::Evaluate {
            procedure: version.clone(),
            bundle: Box::new(bundle),
        },
        PromotionCommand::Approve {
            procedure: version.clone(),
            evaluation_digest: digest,
        },
        PromotionCommand::StartCanary {
            procedure: version.clone(),
            boundary: CanaryBoundary {
                scope: p.fingerprint().scope().clone(),
                cohorts: [promotion::id("pilot")].into(),
                starts_at: 20,
                ends_at: 100,
                max_executions: 2,
                max_failures: 0,
                required_successes: 1,
            },
        },
        PromotionCommand::ReserveExecution {
            procedure: version.clone(),
            execution: ExecutionRequest {
                id: promotion::id("trial"),
                scope: p.fingerprint().scope().clone(),
                cohort: promotion::id("pilot"),
                mode: ExecutionMode::Canary,
            },
        },
        PromotionCommand::RecordOutcome {
            procedure: version.clone(),
            execution_id: promotion::id("trial"),
            outcome: RuntimeOutcome::Success,
            evidence: promotion::id("verified"),
        },
    ];
    let mut journal = PromotionJournal::default();
    for command in commands {
        journal
            .events
            .push(promotion::event(journal.events.len(), 20, command));
    }
    if active {
        journal.events.push(promotion::event(
            journal.events.len(),
            20,
            PromotionCommand::Activate { procedure: version },
        ));
    }
    gateway_registry::learned_procedures::LearnedProcedureRegistry::from_journal(&journal).unwrap();
    journal
}
struct Authority;
impl PromotionAuthority for Authority {
    fn authorize(&self, _: &PromotionCommand) -> Option<AuthorizedPromotion> {
        Some(AuthorizedPromotion {
            actor: ProvenanceId::new("host").unwrap(),
            policy_decision: promotion::id("policy"),
            role: PromotionRole::Runtime,
        })
    }
}
struct Store(PromotionJournal);
impl PromotionStore for Store {
    fn load(&self) -> Result<PromotionJournal, PromotionError> {
        Ok(self.0.clone())
    }
    fn append(&mut self, revision: usize, event: PromotionEvent) -> Result<(), PromotionError> {
        if self.0.events.len() != revision {
            return Err(PromotionError::Conflict);
        }
        self.0.events.push(event);
        Ok(())
    }
}
struct Inputs {
    f: Fixture,
    p: LearnedProcedure,
    batch: ScopedObservationBatch,
    blocked: bool,
    selected: BTreeSet<ReferenceId>,
    clock: std::rc::Rc<std::cell::Cell<i64>>,
}
impl ReflexInputs for Inputs {
    fn situation(&mut self) -> Result<ReflexSituation, ReflexFailure> {
        Ok(ReflexSituation {
            fingerprint: self.p.fingerprint().clone(),
            observations: self.batch.clone(),
            blockers: if self.blocked {
                vec![promotion::id("blocker")]
            } else {
                vec![]
            },
        })
    }
    fn prepare(&self, _: usize) -> Result<CompileStepInput<'_>, ReflexFailure> {
        Ok(CompileStepInput {
            resolved: &self.f.resolved,
            authority: &self.f.authority,
            policy_context: &self.f.policy,
            catalog: &self.f.catalog,
            projection: &self.f.projection,
            candidates: &[],
            selected: &self.selected,
        })
    }
    fn now(&self) -> i64 {
        self.clock.get()
    }
}
struct Runtime {
    calls: usize,
    status: OutcomeStatus,
    output: ScopedObservationBatch,
    clock: std::rc::Rc<std::cell::Cell<i64>>,
    elapsed: i64,
}
impl ExecutionRuntimePort for Runtime {
    fn execute(&mut self, id: &ReferenceId, _: &CompiledStep) -> ExecutionOutcome {
        self.calls += 1;
        self.clock.set(self.clock.get() + self.elapsed);
        ExecutionOutcome {
            execution: id.clone(),
            status: self.status,
            observations: Some(self.output.clone()),
        }
    }
}
impl ReflexRuntime for Runtime {
    fn execute_bounded(
        &mut self,
        id: &ReferenceId,
        compiled: &CompiledStep,
        limits: ReflexDispatchLimits,
    ) -> ExecutionOutcome {
        assert_eq!(limits.deadline_unix_seconds, 30);
        assert_eq!(limits.resource_units, 1);
        self.execute(id, compiled)
    }
}
fn setup(active: bool) -> (ReflexEngine<Authority, Store>, Inputs, Runtime) {
    let f = fixture();
    let p = procedure(&f);
    let clock = std::rc::Rc::new(std::cell::Cell::new(20));
    (
        ReflexEngine::new(PromotionApplication::new(
            Authority,
            Store(history(&p, active)),
        )),
        Inputs {
            f,
            p,
            batch: batch(false, "20", "before", true),
            blocked: false,
            selected: BTreeSet::new(),
            clock: clock.clone(),
        },
        Runtime {
            calls: 0,
            status: OutcomeStatus::Completed,
            output: batch(true, "20", "after", true),
            clock,
            elapsed: 0,
        },
    )
}
fn budget() -> ReflexBudget {
    ReflexBudget {
        max_iterations: 3,
        max_retries: 1,
        max_resource_units: 3,
        max_elapsed_seconds: 10,
        max_evidence_age_seconds: 5,
    }
}
#[test]
fn reflex_executes_verified_active_procedure_without_model() {
    let (mut engine, mut inputs, mut runtime) = setup(true);
    let result = engine.run(
        promotion::id("execution"),
        promotion::id("pilot"),
        budget(),
        &mut inputs,
        &mut runtime,
    );
    assert_eq!(result.failure, None, "{result:?}");
    assert_eq!(result.disposition, ReflexDisposition::Success);
    assert_eq!(runtime.calls, 1);
    assert_eq!(result.trace.last().unwrap().stage, "OUTCOME_RECORDED");
    if let Ok(path) = std::env::var("CG25_REFLEX_OUTPUT") {
        std::fs::write(path, serde_json::to_string_pretty(&result).unwrap()).unwrap();
    }
    let again = engine.run(
        promotion::id("execution"),
        promotion::id("pilot"),
        budget(),
        &mut inputs,
        &mut runtime,
    );
    assert_eq!(again.failure, Some(ReflexFailure::RegistryUnavailable));
    assert_eq!(runtime.calls, 1);
}
#[test]
fn reflex_false_positive_regressions_never_dispatch() {
    for case in 0..7 {
        let (mut engine, mut inputs, mut runtime) = setup(case != 0);
        let expected = match case {
            0 => ReflexFailure::NovelSituation,
            1 => {
                inputs.batch = batch(false, "1", "before", true);
                ReflexFailure::StaleEvidence
            }
            2 => {
                inputs.batch = batch(false, "20", "before", false);
                ReflexFailure::MissingEvidence
            }
            3 => {
                inputs.blocked = true;
                ReflexFailure::Blocked
            }
            4 => {
                inputs
                    .f
                    .policy
                    .steps
                    .values_mut()
                    .for_each(|f| f.authorizations.clear());
                ReflexFailure::ProcessOrPolicyDenied
            }
            5 => {
                inputs.f.projection.mapping.workflow = WorkflowId::new("wrong-workflow").unwrap();
                ReflexFailure::BindingMismatch
            }
            _ => {
                inputs.batch = batch(false, "21", "future", true);
                ReflexFailure::StaleEvidence
            }
        };
        let result = engine.run(
            promotion::id("execution"),
            promotion::id("pilot"),
            budget(),
            &mut inputs,
            &mut runtime,
        );
        assert_eq!(result.failure, Some(expected), "case {case}: {result:?}");
        assert_eq!(runtime.calls, 0);
        assert_eq!(result.disposition, ReflexDisposition::FullCognitivePath);
    }
}
#[test]
fn reflex_verification_and_retry_budgets_fail_closed() {
    for case in 0..7 {
        let (mut engine, mut inputs, mut runtime) = setup(true);
        let mut limits = budget();
        let expected = match case {
            0 => {
                runtime.output = batch(false, "20", "after", true);
                ReflexFailure::VerificationFailed
            }
            1 => {
                runtime.output = inputs.batch.clone();
                ReflexFailure::VerificationFailed
            }
            2 => {
                runtime.status = OutcomeStatus::RetryableFailure;
                ReflexFailure::BudgetExhausted
            }
            3 => {
                limits.max_resource_units = 0;
                ReflexFailure::BudgetExhausted
            }
            4 => {
                runtime.output = batch(true, "19", "after", true);
                ReflexFailure::VerificationFailed
            }
            5 => {
                runtime.output = batch(true, "20", "after", false);
                ReflexFailure::VerificationFailed
            }
            _ => {
                runtime.status = OutcomeStatus::HardFailure;
                ReflexFailure::ExecutionFailed
            }
        };
        let result = engine.run(
            promotion::id("execution"),
            promotion::id("pilot"),
            limits,
            &mut inputs,
            &mut runtime,
        );
        assert_eq!(result.failure, Some(expected), "{result:?}");
        assert!(runtime.calls <= 2);
    }
}

#[test]
fn reflex_exact_match_rejects_novel_and_ambiguous_shapes() {
    let (_, inputs, _) = setup(true);
    let registry = gateway_registry::learned_procedures::LearnedProcedureRegistry::from_journal(
        &history(&inputs.p, true),
    )
    .unwrap();
    let mut signals = inputs.p.fingerprint().signals().to_vec();
    signals.push(FingerprintSignal::Fact(FactId::new("novel").unwrap()));
    let near = SituationFingerprint::new(inputs.p.fingerprint().scope().clone(), signals).unwrap();
    assert_eq!(
        match_active(&registry, &near),
        Err(ReflexFailure::NovelSituation)
    );
    let other_scope = SituationFingerprint::new(
        ContextScopeId::new("other").unwrap(),
        inputs.p.fingerprint().signals().to_vec(),
    )
    .unwrap();
    assert_eq!(
        match_active(&registry, &other_scope),
        Err(ReflexFailure::NovelSituation)
    );
    let candidate = PatternCandidate::new(
        promotion::id("other-candidate"),
        inputs.p.fingerprint().clone(),
        inputs.p.experience().to_vec(),
    )
    .unwrap();
    let second = LearnedProcedure::new(
        promotion::id("second"),
        1,
        &candidate,
        inputs.p.steps().to_vec(),
        inputs.p.required_observations().to_vec(),
        inputs.p.required_evidence().to_vec(),
        inputs.p.verification_evidence().to_vec(),
        FallbackBehavior::Stop,
    )
    .unwrap();
    let mut journal = history(&inputs.p, true);
    for mut event in history(&second, true).events {
        match &mut event.command {
            PromotionCommand::ReserveExecution { execution, .. } => {
                execution.id = promotion::id("other-trial")
            }
            PromotionCommand::RecordOutcome { execution_id, .. } => {
                *execution_id = promotion::id("other-trial")
            }
            _ => {}
        }
        event.metadata.id = promotion::id(&format!("decision-{}", journal.events.len()));
        journal.events.push(event);
    }
    let registry =
        gateway_registry::learned_procedures::LearnedProcedureRegistry::from_journal(&journal)
            .unwrap();
    assert_eq!(
        match_active(&registry, inputs.p.fingerprint()),
        Err(ReflexFailure::AmbiguousMatch)
    );
}
#[test]
fn reflex_missing_observation_and_conflicts_reject_evidence() {
    let (_, inputs, _) = setup(true);
    let candidate = PatternCandidate::new(
        promotion::id("candidate"),
        inputs.p.fingerprint().clone(),
        inputs.p.experience().to_vec(),
    )
    .unwrap();
    let p = LearnedProcedure::new(
        promotion::id("missing-observation"),
        1,
        &candidate,
        inputs.p.steps().to_vec(),
        vec![ObservationId::new("missing").unwrap()],
        inputs.p.required_evidence().to_vec(),
        inputs.p.verification_evidence().to_vec(),
        FallbackBehavior::Stop,
    )
    .unwrap();
    assert_eq!(
        evidence_gate(&p, &inputs.batch, 20, 5, false),
        Err(ReflexFailure::MissingObservation)
    );
    let records = inputs.batch.records();
    let extra = Observation::new(
        ObservationId::new("conflicting").unwrap(),
        records.observations()[0].subject().clone(),
        TypedValue::Boolean(true),
        records.provenances()[0].id().clone(),
    )
    .unwrap()
    .with_occurred_at(SourceTimestamp::new("20").unwrap());
    let fact = Fact::new(
        FactId::new("conflicting").unwrap(),
        extra.subject().clone(),
        extra.value().clone(),
        AssertionPolarity::Affirmed,
        vec![extra.id().clone()],
    )
    .unwrap();
    let mut observations = records.observations().to_vec();
    observations.push(extra);
    let mut facts = records.facts().to_vec();
    facts.push(fact);
    let records = ObservationEvidenceSet::new(
        records.provenances().to_vec(),
        observations,
        facts,
        records.evidence().to_vec(),
    )
    .unwrap();
    let batch = ScopedObservationBatch::new(
        inputs.batch.scope().clone(),
        inputs.batch.snapshot().clone(),
        records,
    )
    .unwrap();
    assert_eq!(
        evidence_gate(&inputs.p, &batch, 20, 5, false),
        Err(ReflexFailure::ConflictingEvidence)
    );
}
#[test]
fn reflex_time_and_iteration_limits_are_closed() {
    for case in 0..5 {
        let (mut engine, mut inputs, mut runtime) = setup(true);
        let mut limits = budget();
        match case {
            0 => limits.max_iterations = 0,
            1 => limits.max_elapsed_seconds = 0,
            2 => runtime.elapsed = 10,
            3 => runtime.elapsed = -1,
            _ => {
                runtime.status = OutcomeStatus::RetryableFailure;
                limits.max_iterations = 1;
            }
        }
        let result = engine.run(
            promotion::id("execution"),
            promotion::id("pilot"),
            limits,
            &mut inputs,
            &mut runtime,
        );
        assert_eq!(result.failure, Some(ReflexFailure::BudgetExhausted));
        assert_eq!(runtime.calls, usize::from(case >= 2));
    }
}

#[test]
fn reflex_honors_stop_after_dispatch_and_explicit_stale_quality() {
    let (_, mut inputs, mut runtime) = setup(true);
    let candidate = PatternCandidate::new(
        promotion::id("candidate"),
        inputs.p.fingerprint().clone(),
        inputs.p.experience().to_vec(),
    )
    .unwrap();
    inputs.p = LearnedProcedure::new(
        inputs.p.id().clone(),
        1,
        &candidate,
        inputs.p.steps().to_vec(),
        inputs.p.required_observations().to_vec(),
        inputs.p.required_evidence().to_vec(),
        inputs.p.verification_evidence().to_vec(),
        FallbackBehavior::Stop,
    )
    .unwrap();
    let mut engine = ReflexEngine::new(PromotionApplication::new(
        Authority,
        Store(history(&inputs.p, true)),
    ));
    runtime.status = OutcomeStatus::HardFailure;
    let result = engine.run(
        promotion::id("execution"),
        promotion::id("pilot"),
        budget(),
        &mut inputs,
        &mut runtime,
    );
    assert_eq!(result.disposition, ReflexDisposition::Stopped);
    assert_eq!(result.failure, Some(ReflexFailure::ExecutionFailed));
    let quality = QualityMetadata::new(
        TrustClass::ObservedEvidence,
        SensitivityClass::Normal,
        Confidence::Unknown,
        FreshnessStatus::Stale,
        Uncertainty::None,
    );
    let subject = inputs.batch.records().observations()[0].subject().clone();
    let batch = inputs
        .batch
        .clone()
        .with_quality_metadata(subject, vec![quality]);
    assert_eq!(
        evidence_gate(&inputs.p, &batch, 20, 5, false),
        Err(ReflexFailure::StaleEvidence)
    );
}
