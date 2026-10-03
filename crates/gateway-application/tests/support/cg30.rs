//! Integrated, provider-free v0.3 acceptance and measured qualification evidence.
use super::*;
use gateway_application::{experience_patterns::*, memory::*};
use gateway_domain::memory::{ExperienceRecord, MEMORY_SCHEMA_VERSION, MemoryPayload};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Instant;

// Exercise the existing replaceable outer adapters without adding a production dependency.
#[path = "../../../gateway-daemon/src/experience_patterns.rs"]
mod experience_adapter;
#[path = "../../../gateway-daemon/src/memory.rs"]
mod memory_adapter;
use super::super::routing_fixtures as routing_fixture;

fn digest(value: &impl serde::Serialize) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).unwrap()))
}
fn detected_procedure() -> (LearnedProcedure, PatternReport) {
    let template = procedure(&fixture());
    let scope = template.fingerprint().scope().clone();
    let mut memory = MemoryApplication::new(memory_adapter::InMemoryMemoryStore::default());
    let mut source = experience_adapter::InMemoryVerifiedExecutionSource::default();
    for (name, outcome) in [
        ("success-a", "SUCCESS"),
        ("success-b", "SUCCESS"),
        ("failure", "FAILURE"),
    ] {
        let record = ExperienceRecord {
            schema_version: MEMORY_SCHEMA_VERSION,
            id: promotion::id(name),
            scope: scope.clone(),
            provenance: ProvenanceId::new(format!("source-{name}")).unwrap(),
            source_snapshot: promotion::id(&format!("trace-{name}")),
            source_version: NonEmptyText::new("fixture-v1").unwrap(),
            source_digest: ContentDigest::new(digest(&name)).unwrap(),
            created_at: UnixTimestamp::new(10),
            observed_at: UnixTimestamp::new(9),
            valid_from: UnixTimestamp::new(10),
            expires_at: UnixTimestamp::new(100),
            max_age_seconds: 90,
            quality: QualityMetadata::new(
                TrustClass::DerivedAssessment,
                SensitivityClass::Internal,
                Confidence::score(1.0).unwrap(),
                FreshnessStatus::Fresh,
                Uncertainty::None,
            ),
            validation: Some(promotion::id("validated-trace")),
            outcome: Some(NonEmptyText::new(outcome).unwrap()),
            label_basis: Some(promotion::id("verified-goal")),
            payload: Some(MemoryPayload::Reference(promotion::id(&format!(
                "payload-{name}"
            )))),
        };
        source.insert(
            scope.clone(),
            VerifiedExecution {
                memory_id: record.id.clone(),
                runtime: promotion::id("fixture-runtime-v1"),
                trace: record.source_snapshot.clone(),
                evaluation: promotion::id("goal-evaluation-v1"),
                validation: record.validation.clone().unwrap(),
                label_basis: record.label_basis.clone().unwrap(),
                evidence: vec![EvidenceId::new(format!("evidence-{name}")).unwrap()],
                signals: template.fingerprint().signals().to_vec(),
                semantic_hint: None,
            },
        );
        memory
            .admit(record, promotion::id("admit"), UnixTimestamp::new(20))
            .unwrap();
        memory
            .curate(
                &scope,
                &promotion::id(name),
                1,
                MemoryChange {
                    action: MemoryAction::Validate,
                    reason: promotion::id("validate"),
                    at: UnixTimestamp::new(20),
                    replacement: None,
                    successor: None,
                },
            )
            .unwrap();
    }
    let report = inspect_patterns(
        &memory,
        &source,
        &scope,
        UnixTimestamp::new(20),
        PatternLimits::default(),
    )
    .unwrap();
    assert_eq!(report.metrics.candidate_count, 1);
    assert_eq!(report.findings[0].failures.len(), 1);
    let candidate = report.findings[0].candidate.as_ref().unwrap();
    let p = LearnedProcedure::new(
        template.id().clone(),
        1,
        candidate,
        template.steps().to_vec(),
        template.required_observations().to_vec(),
        template.required_evidence().to_vec(),
        template.verification_evidence().to_vec(),
        template.fallback(),
    )
    .unwrap();
    assert_eq!(p.source_candidate(), candidate.id());
    (p, report)
}

struct Governor;
impl PromotionAuthority for Governor {
    fn authorize(&self, _: &PromotionCommand) -> Option<AuthorizedPromotion> {
        Some(AuthorizedPromotion {
            actor: ProvenanceId::new("qualification-host").unwrap(),
            policy_decision: promotion::id("current-governance"),
            role: PromotionRole::Governor,
        })
    }
}
fn apply(app: &mut PromotionApplication<Governor, Store>, command: PromotionCommand) {
    let n = app.inspect().unwrap().0.events.len();
    app.execute(promotion::id(&format!("qualification-{n}")), 20, command)
        .unwrap();
}
// Use the real compiler and bounded runtime for canary; only a fresh, correlated,
// evidence-supported desired-state comparison can supply the successful outcome.
fn qualify_canary(
    app: &mut PromotionApplication<Governor, Store>,
    p: &LearnedProcedure,
    trial: &str,
) {
    let version = ProcedureVersion::of(p);
    let commands = history(p, false).events;
    for event in commands.into_iter().take(6) {
        apply(app, event.command);
    }
    apply(
        app,
        PromotionCommand::ReserveExecution {
            procedure: version.clone(),
            execution: ExecutionRequest {
                id: promotion::id(trial),
                scope: p.fingerprint().scope().clone(),
                cohort: promotion::id("pilot"),
                mode: ExecutionMode::Canary,
            },
        },
    );
    assert!(
        app.execute(
            promotion::id("pending-activation"),
            20,
            PromotionCommand::Activate {
                procedure: version.clone()
            }
        )
        .is_err()
    );
    let (_, inputs, mut runtime) = setup(true);
    let compiled = ContextApplication
        .compile_step(inputs.prepare(0).unwrap())
        .unwrap();
    let output = runtime.execute_bounded(
        &promotion::id(trial),
        &compiled,
        ReflexDispatchLimits {
            deadline_unix_seconds: 30,
            resource_units: 1,
        },
    );
    assert_eq!(output.execution, promotion::id(trial));
    assert_eq!(output.status, OutcomeStatus::Completed);
    let observed = output.observations.unwrap();
    assert_ne!(observed.ingestion_key(), inputs.batch.ingestion_key());
    evidence_gate(p, &observed, 20, 0, true).unwrap();
    let state = normalize_current_state(
        ObservedStateId::new("canary-state").unwrap(),
        NormalizationInput::new(observed.records().clone()).with_required_evidence(true),
    )
    .unwrap();
    assert_eq!(
        compare_desired_state(
            &inputs.f.resolved.snapshot.input().desired,
            &state,
            &ComparisonRules::default()
        )
        .unwrap()
        .outcome(),
        ComparisonOutcome::Satisfied
    );
    apply(
        app,
        PromotionCommand::RecordOutcome {
            procedure: version,
            execution_id: promotion::id(trial),
            outcome: RuntimeOutcome::Success,
            evidence: promotion::id(&format!("verified-{trial}")),
        },
    );
}
fn run_journal(
    journal: PromotionJournal,
    p: LearnedProcedure,
    name: &str,
) -> (ReflexResult, PromotionJournal) {
    let (_, mut inputs, mut runtime) = setup(true);
    inputs.p = p;
    let shared = std::rc::Rc::new(std::cell::RefCell::new(journal));
    let mut engine = ReflexEngine::new(PromotionApplication::new(
        Authority,
        FaultStore {
            journal: shared.clone(),
            fail_outcome: false,
            conflict: false,
        },
    ));
    let result = engine.run(
        promotion::id(name),
        promotion::id("pilot"),
        budget(),
        &mut inputs,
        &mut runtime,
    );
    assert_eq!(result.disposition, ReflexDisposition::Success, "{result:?}");
    assert_eq!(runtime.calls, 1);
    assert_provenance(&result);
    let retained = shared.borrow().clone();
    (result, retained)
}
fn assert_provenance(result: &ReflexResult) {
    assert!(result.procedure.is_some());
    for (stage, fields) in [
        ("MATCH_ACTIVE", vec!["fingerprint", "procedure"]),
        (
            "APPLICABILITY_AND_EVIDENCE",
            vec!["source_snapshot", "observations", "evidence"],
        ),
        (
            "PROCESS_POLICY_COMPILED",
            vec!["process", "capability", "policy"],
        ),
        (
            "DISPATCH",
            vec!["execution", "iterations", "resource_units"],
        ),
        (
            "VERIFIED",
            vec!["source_snapshot", "evidence", "verification"],
        ),
    ] {
        let event = result.trace.iter().find(|e| e.stage == stage).unwrap();
        for field in fields {
            assert!(
                !event.details[field].is_null(),
                "{stage}.{field}: {event:?}"
            );
        }
    }
    assert_eq!(result.trace.last().unwrap().stage, "OUTCOME_RECORDED");
}

#[test]
fn cognitive_runtime_release_qualification() {
    let policy: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/epic03-v0.3/policy.json"
    ))
    .unwrap();
    let started = Instant::now();
    let (p, patterns) = detected_procedure();
    let mut app = PromotionApplication::new(Governor, Store(PromotionJournal::default()));
    qualify_canary(&mut app, &p, "trial-v1");
    let version = ProcedureVersion::of(&p);
    apply(
        &mut app,
        PromotionCommand::Activate {
            procedure: version.clone(),
        },
    );
    let (active, retained) = run_journal(app.inspect().unwrap().0, p.clone(), "active-v1");
    app = PromotionApplication::new(Governor, Store(retained));
    let candidate = PatternCandidate::new(
        p.source_candidate().clone(),
        p.fingerprint().clone(),
        p.experience().to_vec(),
    )
    .unwrap();
    let next = LearnedProcedure::new(
        p.id().clone(),
        2,
        &candidate,
        p.steps().to_vec(),
        p.required_observations().to_vec(),
        p.required_evidence().to_vec(),
        p.verification_evidence().to_vec(),
        p.fallback(),
    )
    .unwrap();
    qualify_canary(&mut app, &next, "trial-v2");
    let successor = ProcedureVersion::of(&next);
    apply(
        &mut app,
        PromotionCommand::Supersede {
            procedure: successor.clone(),
            previous: version.clone(),
        },
    );
    let (upgraded, retained) = run_journal(app.inspect().unwrap().0, next.clone(), "active-v2");
    app = PromotionApplication::new(Governor, Store(retained));
    apply(
        &mut app,
        PromotionCommand::Rollback {
            procedure: successor,
            restore: Some(version.clone()),
        },
    );
    let (restored, retained) = run_journal(app.inspect().unwrap().0, p.clone(), "restored-v1");
    app = PromotionApplication::new(Governor, Store(retained));
    assert_eq!(active.procedure, restored.procedure);
    assert_ne!(active.procedure, upgraded.procedure);
    let lifecycle = app.inspect().unwrap().0;
    // Recompute every retained evaluation and prove order independence/tamper rejection.
    for event in &lifecycle.events {
        if let PromotionCommand::Evaluate { bundle, .. } = &event.command {
            bundle.validate().unwrap();
            let mut dataset = bundle.dataset.clone();
            dataset.cases.reverse();
            assert_eq!(
                **bundle,
                EvaluationBundle::evaluate(
                    &bundle.procedure,
                    dataset,
                    bundle.report.manifest.runtime_version.clone()
                )
                .unwrap()
            );
            let mut altered = (**bundle).clone();
            altered.report.passed = false;
            assert!(altered.validate().is_err());
        }
    }
    let lifecycle_elapsed = started.elapsed().as_nanos();
    let classification = classification_benchmark();
    let routing = routing_benchmark();
    let failures = failure_injection();
    assert!(
        classification["false_positive"].as_u64().unwrap()
            <= policy["classification"]["max_false_positive"]
                .as_u64()
                .unwrap()
    );
    assert!(
        classification["false_negative"].as_u64().unwrap()
            <= policy["classification"]["max_false_negative"]
                .as_u64()
                .unwrap()
    );
    assert_eq!(routing["correct"], routing["total"]);
    let evidence = json!({"schema_version": 1, "suite": "CG-30-v1", "policy": policy,
        "fixture_only": true, "configuration": {"reflex_budget": budget(), "pattern_limits": PatternLimits::default()}, "patterns": patterns, "lifecycle": lifecycle,
        "reflex_proofs": [active, upgraded, restored], "classification": classification,
        "routing": routing, "routing_execution": routed_execution_proofs(), "failures": failures,
        "performance": {"lifecycle_wall_ns": lifecycle_elapsed, "model_calls": 0,
            "cost": 0, "cost_unit": "external-provider-calls", "memory_bytes": null,
            "reflex_latency": latency_summary(&classification),
            "note": "Host wall time; fixture dispatch counts are resource units, not hardware utilization."}});
    if let Ok(path) = std::env::var("CG30_QUALIFICATION_OUTPUT") {
        std::fs::write(
            path,
            serde_json::to_string_pretty(
                &json!({"payload_sha256": digest(&evidence), "evidence": evidence}),
            )
            .unwrap(),
        )
        .unwrap();
    }
}

fn conflicting(inputs: &Inputs) -> ScopedObservationBatch {
    let records = inputs.batch.records();
    let extra = Observation::new(
        ObservationId::new("conflict").unwrap(),
        records.observations()[0].subject().clone(),
        TypedValue::Boolean(true),
        records.provenances()[0].id().clone(),
    )
    .unwrap()
    .with_occurred_at(SourceTimestamp::new("20").unwrap());
    let fact = Fact::new(
        FactId::new("conflict").unwrap(),
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
    ScopedObservationBatch::new(
        inputs.batch.scope().clone(),
        inputs.batch.snapshot().clone(),
        ObservationEvidenceSet::new(
            records.provenances().to_vec(),
            observations,
            facts,
            records.evidence().to_vec(),
        )
        .unwrap(),
    )
    .unwrap()
}
fn ambiguous_journal(p: &LearnedProcedure) -> PromotionJournal {
    let candidate = PatternCandidate::new(
        p.source_candidate().clone(),
        p.fingerprint().clone(),
        p.experience().to_vec(),
    )
    .unwrap();
    let other = LearnedProcedure::new(
        promotion::id("competing"),
        1,
        &candidate,
        p.steps().to_vec(),
        p.required_observations().to_vec(),
        p.required_evidence().to_vec(),
        p.verification_evidence().to_vec(),
        p.fallback(),
    )
    .unwrap();
    let mut journal = history(p, true);
    for mut event in history(&other, true).events {
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
    journal
}
fn classification_benchmark() -> Value {
    let names = [
        "exact",
        "inactive",
        "novel-superset",
        "different-scope",
        "ambiguous",
        "missing-evidence",
        "stale-evidence",
        "conflicting-evidence",
        "policy-bypass",
        "process-bypass",
        "blocker",
        "future-evidence",
    ];
    let mut rows = vec![];
    let (mut tp, mut tn, mut fp, mut fn_count) = (0, 0, 0, 0);
    for (index, name) in names.into_iter().enumerate() {
        let (mut engine, mut inputs, mut runtime) = setup(index != 1);
        let expected = match index {
            0 => None,
            1 => Some(ReflexFailure::NovelSituation),
            2 | 3 => {
                let signals = if index == 2 {
                    let mut s = inputs.p.fingerprint().signals().to_vec();
                    s.push(FingerprintSignal::Fact(FactId::new("novel").unwrap()));
                    s
                } else {
                    inputs.p.fingerprint().signals().to_vec()
                };
                let fingerprint = SituationFingerprint::new(
                    ContextScopeId::new(if index == 3 { "other" } else { "project-a" }).unwrap(),
                    signals,
                )
                .unwrap();
                let candidate = PatternCandidate::new(
                    promotion::id("novel-candidate"),
                    fingerprint,
                    inputs
                        .p
                        .experience()
                        .iter()
                        .map(|basis| {
                            let mut memory = basis.memory().clone();
                            if index == 3 {
                                memory.scope = ContextScopeId::new("other").unwrap();
                            }
                            ExperienceBasis::new(
                                memory,
                                basis.provenance().clone(),
                                basis.evaluation().clone(),
                            )
                            .unwrap()
                        })
                        .collect(),
                )
                .unwrap();
                inputs.p = LearnedProcedure::new(
                    promotion::id("novel-input"),
                    1,
                    &candidate,
                    inputs.p.steps().to_vec(),
                    inputs.p.required_observations().to_vec(),
                    inputs.p.required_evidence().to_vec(),
                    inputs.p.verification_evidence().to_vec(),
                    inputs.p.fallback(),
                )
                .unwrap();
                Some(ReflexFailure::NovelSituation)
            }
            4 => {
                engine = ReflexEngine::new(PromotionApplication::new(
                    Authority,
                    Store(ambiguous_journal(&inputs.p)),
                ));
                Some(ReflexFailure::AmbiguousMatch)
            }
            5 => {
                inputs.batch = batch(false, "20", "before", false);
                Some(ReflexFailure::MissingEvidence)
            }
            6 => {
                inputs.batch = batch(false, "1", "before", true);
                Some(ReflexFailure::StaleEvidence)
            }
            7 => {
                inputs.batch = conflicting(&inputs);
                Some(ReflexFailure::ConflictingEvidence)
            }
            8 => {
                inputs
                    .f
                    .policy
                    .steps
                    .values_mut()
                    .for_each(|f| f.authorizations.clear());
                Some(ReflexFailure::ProcessOrPolicyDenied)
            }
            9 => {
                inputs.f.projection.mapping.workflow = WorkflowId::new("bypass").unwrap();
                Some(ReflexFailure::BindingMismatch)
            }
            10 => {
                inputs.blocked = true;
                Some(ReflexFailure::Blocked)
            }
            _ => {
                inputs.batch = batch(false, "21", "before", true);
                Some(ReflexFailure::StaleEvidence)
            }
        };
        let start = Instant::now();
        let result = engine.run(
            promotion::id(name),
            promotion::id("pilot"),
            budget(),
            &mut inputs,
            &mut runtime,
        );
        let elapsed = start.elapsed().as_nanos();
        let activated = runtime.calls > 0;
        match (index == 0, activated) {
            (true, true) => tp += 1,
            (false, false) => tn += 1,
            (false, true) => fp += 1,
            (true, false) => fn_count += 1,
        }
        assert_eq!(result.failure, expected, "{name}: {result:?}");
        if index == 0 {
            assert_provenance(&result);
        } else {
            assert_eq!(result.disposition, ReflexDisposition::FullCognitivePath);
            assert_eq!(runtime.calls, 0);
        }
        rows.push(json!({"id": name, "expected_activation": index == 0, "runtime_calls": runtime.calls, "wall_ns": elapsed, "result": result}));
    }
    json!({"true_positive": tp, "true_negative": tn, "false_positive": fp, "false_negative": fn_count,
        "false_positive_denominator": tn + fp, "false_negative_denominator": tp + fn_count,
        "false_positive_rate": fp as f64 / (tn + fp) as f64, "false_negative_rate": fn_count as f64 / (tp + fn_count) as f64,
        "cases": rows})
}

fn routing_benchmark() -> Value {
    use gateway_application::cognitive_routing::*;
    use gateway_domain::cognitive_routing::*;
    let mut cases = vec![];
    for index in 0..10 {
        let mut req = routing_fixture::request();
        req.deterministic_sufficient = false;
        req.reflex_applicable = false;
        let mut local = routing_fixture::candidate("local", CognitiveRoute::LocalSlm);
        let strong = routing_fixture::candidate("strong", CognitiveRoute::StrongLlm);
        let (name, reason) = match index {
            0 => ("local-compatible", None),
            1 => {
                local.available = false;
                ("model-unavailable", Some(RouteRejection::Unavailable))
            }
            2 => {
                local.qualified = false;
                ("unqualified-upgrade", Some(RouteRejection::Unqualified))
            }
            3 => {
                local.cost = req.max_cost + 1;
                ("cost", Some(RouteRejection::Cost))
            }
            4 => {
                local.latency_ms = req.max_latency_ms + 1;
                ("latency", Some(RouteRejection::Latency))
            }
            5 => {
                local.hardware = HardwareRequirement::Gpu;
                ("hardware", Some(RouteRejection::Hardware))
            }
            6 => {
                local.input_contracts = ["other".into()].into();
                ("input-contract", Some(RouteRejection::InputContract))
            }
            7 => {
                local.output_contracts = vec![json!("other")];
                ("output-contract", Some(RouteRejection::OutputContract))
            }
            8 => {
                local.available = false;
                req.max_privacy = PrivacyBoundary::OnDevice;
                ("private-outage", Some(RouteRejection::Unavailable))
            }
            _ => {
                local.tasks = [TaskClass::Ranking].into();
                ("task-class", Some(RouteRejection::TaskClass))
            }
        };
        let snapshot = routing_fixture::snapshot(vec![strong, local]);
        let start = Instant::now();
        let explanation = select_route(&req, &snapshot).unwrap();
        let elapsed = start.elapsed().as_nanos();
        let expected = if index == 0 {
            Some("local")
        } else if index == 8 {
            None
        } else {
            Some("strong")
        };
        assert_eq!(
            explanation.selected.as_ref().map(|c| c.id.as_str()),
            expected
        );
        if let Some(reason) = reason {
            assert!(
                explanation
                    .alternatives
                    .iter()
                    .find(|a| a.candidate.id == "local")
                    .unwrap()
                    .reasons
                    .contains(&reason)
            );
        }
        if let Some(c) = &explanation.selected {
            assert!(
                c.qualified
                    && c.available
                    && c.privacy <= req.max_privacy
                    && c.cost <= req.max_cost
                    && c.latency_ms <= req.max_latency_ms
            );
        }
        let mut reversed = snapshot.clone();
        reversed.candidates.reverse();
        assert_eq!(explanation, select_route(&req, &reversed).unwrap());
        cases.push(json!({"id": name, "snapshot": snapshot, "expected": expected, "explanation": explanation, "wall_ns": elapsed}));
    }
    json!({"correct": cases.len(), "total": cases.len(), "constraint_violations": 0, "cases": cases})
}

#[derive(Clone)]
struct FaultStore {
    journal: std::rc::Rc<std::cell::RefCell<PromotionJournal>>,
    fail_outcome: bool,
    conflict: bool,
}
impl PromotionStore for FaultStore {
    fn load(&self) -> Result<PromotionJournal, PromotionError> {
        Ok(self.journal.borrow().clone())
    }
    fn append(&mut self, revision: usize, event: PromotionEvent) -> Result<(), PromotionError> {
        if self.conflict || self.journal.borrow().events.len() != revision {
            return Err(PromotionError::Conflict);
        }
        if self.fail_outcome && matches!(event.command, PromotionCommand::RecordOutcome { .. }) {
            return Err(PromotionError::Store("injected outcome failure".into()));
        }
        self.journal.borrow_mut().events.push(event);
        Ok(())
    }
}
fn failure_injection() -> Value {
    let mut rows = vec![];
    for case in 0..5 {
        let (_, mut inputs, mut runtime) = setup(true);
        let initial = history(&inputs.p, true);
        let journal = std::rc::Rc::new(std::cell::RefCell::new(initial.clone()));
        let mut engine = ReflexEngine::new(PromotionApplication::new(
            Authority,
            FaultStore {
                journal: journal.clone(),
                fail_outcome: case == 3,
                conflict: case == 4,
            },
        ));
        let (name, expected, calls) = match case {
            0 => {
                runtime.status = OutcomeStatus::HardFailure;
                ("worker-hard-failure", ReflexFailure::ExecutionFailed, 1)
            }
            1 => {
                runtime.status = OutcomeStatus::RetryableFailure;
                ("worker-retry-exhaustion", ReflexFailure::BudgetExhausted, 2)
            }
            2 => {
                runtime.output = inputs.batch.clone();
                ("reused-verification", ReflexFailure::VerificationFailed, 1)
            }
            3 => (
                "outcome-store-failure",
                ReflexFailure::RegistryUnavailable,
                1,
            ),
            _ => (
                "reservation-conflict",
                ReflexFailure::RegistryUnavailable,
                0,
            ),
        };
        let result = engine.run(
            promotion::id(name),
            promotion::id("pilot"),
            budget(),
            &mut inputs,
            &mut runtime,
        );
        assert_eq!(result.failure, Some(expected));
        assert_eq!(runtime.calls, calls);
        assert_ne!(result.disposition, ReflexDisposition::Success);
        let retained = journal.borrow().clone();
        gateway_registry::learned_procedures::LearnedProcedureRegistry::from_journal(&retained)
            .unwrap();
        let appended = &retained.events[initial.events.len()..];
        assert_eq!(
            appended
                .iter()
                .filter(|e| matches!(e.command, PromotionCommand::ReserveExecution { .. }))
                .count(),
            usize::from(case != 4)
        );
        assert_eq!(
            appended
                .iter()
                .filter(|e| matches!(e.command, PromotionCommand::RecordOutcome { .. }))
                .count(),
            usize::from(case < 3)
        );
        let again = engine.run(
            promotion::id(name),
            promotion::id("pilot"),
            budget(),
            &mut inputs,
            &mut runtime,
        );
        assert_eq!(again.failure, Some(ReflexFailure::RegistryUnavailable));
        assert_eq!(runtime.calls, calls);
        rows.push(json!({"id": name, "runtime_calls": calls, "result": result, "journal": retained, "duplicate": again}));
    }
    json!({"cases": rows})
}

fn routed_execution_proofs() -> Vec<Value> {
    use gateway_application::cognitive_routing::*;
    use gateway_domain::cognitive_routing::*;
    struct Runtime {
        fixture: Fixture,
        calls: std::cell::Cell<usize>,
        deny_fallback: bool,
        invalid_identity: bool,
    }
    impl CognitiveRouteRuntime for Runtime {
        fn prepare(
            &self,
            _: &CognitiveRouteRequest,
            _: &RouteCandidate,
        ) -> Result<CompiledStep, ContextApplicationError> {
            if self.deny_fallback && self.calls.get() > 0 {
                let mut denied = Fixture::new();
                denied
                    .policy
                    .steps
                    .values_mut()
                    .for_each(|f| f.authorizations.clear());
                return denied.compile();
            }
            self.fixture.compile()
        }
        fn attempt(&self, handoff: RouteHandoff<'_>) -> RouteAttemptReport {
            let first = self.calls.get() == 0;
            self.calls.set(self.calls.get() + 1);
            RouteAttemptReport {
                model: if self.invalid_identity {
                    None
                } else {
                    handoff.candidate.model.clone()
                },
                outcome: if first {
                    RouteAttemptOutcome::ModelFailure
                } else {
                    RouteAttemptOutcome::Success
                },
                output_reference: (!first).then(|| promotion::id("governed-proposal")),
                cost_unit: handoff.request.cost_unit.clone(),
                cost: 0,
                latency_ms: 0,
            }
        }
    }
    let mut rows = vec![];
    for case in 0..4 {
        let runtime = Runtime {
            fixture: Fixture::new(),
            calls: std::cell::Cell::new(0),
            deny_fallback: case == 1,
            invalid_identity: case == 3,
        };
        let mut request = routing_fixture::request();
        request.deterministic_sufficient = false;
        request.reflex_applicable = false;
        request.output_contract = runtime.fixture.compile().unwrap().output_contract().clone();
        let mut candidates = vec![
            routing_fixture::candidate("local", CognitiveRoute::LocalSlm),
            routing_fixture::candidate("replacement", CognitiveRoute::SpecializedLocal),
        ];
        for candidate in &mut candidates {
            candidate.output_contracts = vec![request.output_contract.clone()];
        }
        if case == 2 {
            for candidate in &mut candidates {
                candidate.available = false;
            }
        }
        let snapshot = routing_fixture::snapshot(candidates);
        let registry =
            gateway_registry::model_capabilities::ModelCapabilityRegistry::new(snapshot.clone())
                .unwrap();
        let telemetry = route_and_execute(&request, &registry, &runtime).unwrap();
        let (name, disposition, calls) = match case {
            0 => ("model-failure-fallback", RoutingDisposition::Success, 2),
            1 => (
                "policy-revoked-before-fallback",
                RoutingDisposition::ProcessOrPolicyDenied,
                1,
            ),
            2 => (
                "all-models-unavailable",
                RoutingDisposition::NoCompatibleRoute,
                0,
            ),
            _ => (
                "model-identity-mismatch",
                RoutingDisposition::InvalidReport,
                1,
            ),
        };
        assert_eq!(telemetry.disposition, disposition);
        assert_eq!(runtime.calls.get(), calls);
        assert_eq!(telemetry.execution_provenance.len(), calls);
        assert_eq!(telemetry.consumed_cost, calls as u64);
        assert_eq!(telemetry.consumed_latency_ms, calls as u64 * 10);
        rows.push(json!({"id": name, "snapshot": snapshot, "runtime_calls": calls, "telemetry": telemetry}));
    }
    rows
}

fn latency_summary(classification: &Value) -> Value {
    let mut samples = classification["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["wall_ns"].as_u64().unwrap())
        .collect::<Vec<_>>();
    samples.sort();
    json!({"samples": samples.len(), "unit": "ns", "min": samples[0],
        "median": samples[(samples.len() - 1) / 2], "p95": samples[(95 * samples.len()).div_ceil(100) - 1],
        "max": samples[samples.len() - 1], "basis": "single-run host wall time including governance, fixture dispatch and refusal; not a production SLA"})
}
