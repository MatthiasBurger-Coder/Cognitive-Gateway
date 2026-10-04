use gateway_application::{
    memory::{MemoryAction, MemoryApplication, MemoryChange, MemoryStore},
    model_releases::{
        ModelReleaseAuthority, ModelReleaseRegistry, ModelReleaseState, release_digest,
    },
    offline_learning::*,
};
use gateway_daemon::memory::InMemoryMemoryStore;
use gateway_domain::{
    Confidence, ContentDigest, ContextScopeId, EvidenceId, FreshnessStatus, NonEmptyText,
    ProvenanceId, QualityMetadata, ReferenceId, SensitivityClass, SufficiencyFinding, TrustClass,
    Uncertainty, UnixTimestamp,
    evaluation::{GoldenCase, METRICS, ReleasePolicy},
    memory::{ExperienceRecord, MemoryPayload},
    offline_learning::*,
};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
};
fn id(s: &str) -> ReferenceId {
    ReferenceId::new(s).unwrap()
}
fn scope() -> ContextScopeId {
    ContextScopeId::new("project-a").unwrap()
}
fn at(n: i64) -> UnixTimestamp {
    UnixTimestamp::new(n)
}
fn hash(n: u32) -> ContentDigest {
    ContentDigest::new(format!("{n:064x}")).unwrap()
}
fn model(n: u32) -> ModelVersion {
    ModelVersion {
        id: id("model"),
        version: n,
        artifact_digest: hash(n),
    }
}
fn decision(n: u32, time: i64) -> ModelReleaseDecision {
    ModelReleaseDecision {
        id: id(&format!("decision-{n}")),
        actor: ProvenanceId::new("operator").unwrap(),
        policy_decision: id("approval"),
        at: at(time),
    }
}
fn record(n: u32) -> ExperienceRecord {
    ExperienceRecord {
        schema_version: 1,
        id: id(&format!("memory-{n}")),
        scope: scope(),
        provenance: ProvenanceId::new(format!("source-{n}")).unwrap(),
        source_snapshot: id(&format!("trace-{n}")),
        source_version: NonEmptyText::new("revision-1").unwrap(),
        source_digest: hash(100 + n),
        created_at: at(10),
        observed_at: at(9),
        valid_from: at(10),
        expires_at: at(1000),
        max_age_seconds: 1000,
        quality: QualityMetadata::new(
            TrustClass::DerivedAssessment,
            SensitivityClass::Confidential,
            Confidence::score(0.9).unwrap(),
            FreshnessStatus::Fresh,
            Uncertainty::None,
        ),
        validation: Some(id("validated")),
        outcome: Some(NonEmptyText::new(if n == 1 { "FAILURE" } else { "SUCCESS" }).unwrap()),
        label_basis: Some(id("label")),
        payload: Some(MemoryPayload::Reference(id(&format!(
            "private-payload-{n}"
        )))),
    }
}
struct Evidence {
    signals: Vec<LearningSignal>,
    denied: Cell<bool>,
}
impl LearningEvidencePort for Evidence {
    fn verify(&self, signal: &LearningSignal) -> Result<bool, LearningError> {
        Ok(!self.denied.get() && self.signals.contains(signal))
    }
}
fn fixture() -> (
    MemoryApplication<InMemoryMemoryStore>,
    Evidence,
    Vec<DatasetRow>,
) {
    let mut memory = MemoryApplication::new(InMemoryMemoryStore::default());
    let mut rows = Vec::new();
    for n in 1..=4 {
        memory.admit(record(n), id("admit"), at(10)).unwrap();
        memory
            .curate(
                &scope(),
                &record(n).id,
                1,
                MemoryChange {
                    action: MemoryAction::Validate,
                    reason: id("review"),
                    at: at(11),
                    replacement: None,
                    successor: None,
                },
            )
            .unwrap();
        let signal = LearningSignal {
            schema_version: 1,
            id: id(&format!("signal-{n}")),
            memory: memory
                .eligibility_reference(&scope(), &record(n).id, at(20))
                .unwrap(),
            provenance: record(n).provenance,
            validation: id("validated"),
            label_basis: id("label"),
            evaluation: id("evaluation"),
            evidence: BTreeSet::from([EvidenceId::new(format!("evidence-{n}")).unwrap()]),
            observed_at: at(9),
            producer_model: model(1),
            example_digest: hash(if n == 4 { 1 } else { n }),
            leakage_group: id(&format!("episode-{n}")),
            measurements: BTreeMap::from([
                (SignalKind::Success, u64::from(n != 1 && n != 4)),
                (SignalKind::Failure, u64::from(n == 1 || n == 4)),
                (SignalKind::QualityGate, 0),
                (SignalKind::Retries, 2),
                (SignalKind::Repairs, 1),
                (SignalKind::LatencyMs, 12),
                (SignalKind::CostUnits, 3),
                (SignalKind::HumanCorrection, 1),
                (SignalKind::PolicyDenial, 1),
                (SignalKind::Rollback, 1),
                (SignalKind::Recurrence, 4),
            ]),
        };
        // The duplicate has matching validated outcome, independent provenance.
        if n == 4 {
            memory
                .curate(
                    &scope(),
                    &record(n).id,
                    2,
                    MemoryChange {
                        action: MemoryAction::Refresh,
                        reason: id("correct"),
                        at: at(12),
                        replacement: Some(ExperienceRecord {
                            outcome: Some(NonEmptyText::new("FAILURE").unwrap()),
                            source_snapshot: id("trace-4-corrected"),
                            ..record(n)
                        }),
                        successor: None,
                    },
                )
                .unwrap();
            memory
                .curate(
                    &scope(),
                    &record(n).id,
                    3,
                    MemoryChange {
                        action: MemoryAction::Validate,
                        reason: id("review"),
                        at: at(13),
                        replacement: None,
                        successor: None,
                    },
                )
                .unwrap();
        }
        let mut signal = signal;
        signal.memory = memory
            .eligibility_reference(&scope(), &record(n).id, at(20))
            .unwrap();
        rows.push(DatasetRow {
            signal,
            split: match n {
                2 => DatasetSplit::Validation,
                3 => DatasetSplit::Test,
                _ => DatasetSplit::Train,
            },
        });
    }
    let evidence = Evidence {
        signals: rows.iter().map(|r| r.signal.clone()).collect(),
        denied: Cell::new(false),
    };
    (memory, evidence, rows)
}
fn config() -> DatasetConfiguration {
    DatasetConfiguration {
        id: id("dataset"),
        version: 1,
        scope: scope(),
        source_revision: id("git-revision"),
        builder_version: id("builder-1"),
        excluded_examples: BTreeSet::new(),
        max_signals: 10,
    }
}
fn recipe() -> TrainingRecipe {
    TrainingRecipe {
        schema_version: 1,
        id: id("recipe"),
        version: 1,
        base_model: model(1),
        trainer_version: id("trainer-1"),
        evaluator_version: id("evaluator-1"),
        environment_digest: hash(99),
        seed: 42,
        parameters: BTreeMap::from([("epochs".into(), "2".into())]),
    }
}
fn policy() -> ReleasePolicy {
    ReleasePolicy {
        version: 1,
        baseline: id("baseline-1"),
        floors: METRICS.map(|m| (m, 900_000)).into(),
        baseline_scores: METRICS.map(|m| (m, 1_000_000)).into(),
        allowed_regression: 10_000,
    }
}
struct Worker {
    authorized: bool,
    fail: bool,
    wrong_binding: bool,
    calls: Cell<u32>,
    candidate: u32,
}
impl Worker {
    fn good(candidate: u32) -> Self {
        Self {
            authorized: true,
            fail: false,
            wrong_binding: false,
            calls: Cell::new(0),
            candidate,
        }
    }
}
impl OfflineAuthorizationPort for Worker {
    fn authorize(&self, _: &OfflineJob, _: UnixTimestamp) -> Result<bool, LearningError> {
        Ok(self.authorized)
    }
}
impl OfflineTrainingPort for Worker {
    fn train(
        &self,
        job: &OfflineJob,
        _: &TrainingRecipe,
        rows: &[DatasetRow],
    ) -> Result<TrainingRun, LearningError> {
        self.calls.set(self.calls.get() + 1);
        assert!(rows.iter().all(|r| r.split != DatasetSplit::Test));
        Ok(TrainingRun {
            schema_version: 1,
            job: job.id.clone(),
            scope: job.scope.clone(),
            dataset_digest: if self.wrong_binding {
                hash(999)
            } else {
                job.dataset_digest.clone()
            },
            recipe_digest: job.recipe_digest.clone(),
            candidate: model(self.candidate),
            evidence: id("training-evidence"),
            completed_at: at(21),
        })
    }
}
impl OfflineEvaluationPort for Worker {
    fn evaluate(
        &self,
        _: &TrainingRun,
        _: &TrainingRecipe,
        rows: &[DatasetRow],
    ) -> Result<OfflineEvaluationResult, LearningError> {
        self.calls.set(self.calls.get() + 1);
        assert!(rows.iter().all(|r| r.split == DatasetSplit::Test));
        let cases = rows
            .iter()
            .map(|r| GoldenCase {
                id: if self.wrong_binding {
                    id("wrong-case")
                } else {
                    r.signal.id.clone()
                },
                scope: scope(),
                relevant: BTreeSet::from([id("expected")]),
                returned: vec![id(if self.fail { "wrong" } else { "expected" })],
                expected_sufficiency: SufficiencyFinding::Sufficient,
                actual_sufficiency: SufficiencyFinding::Sufficient,
                expected_provenance: true,
                actual_provenance: true,
                expected_freshness: true,
                actual_freshness: true,
                expected_contamination_rejected: true,
                actual_contamination_rejected: true,
                token_budget: 10,
                tokens_used: 5,
                justified_tokens: 5,
                latency_ms: 10,
                cost_units: 1,
            })
            .collect();
        Ok(OfflineEvaluationResult {
            cases,
            evidence: id("evaluation-evidence"),
            evaluated_at: at(22),
        })
    }
}
struct Authority {
    allowed: bool,
    verified: bool,
}
impl ModelReleaseAuthority for Authority {
    fn authorize(&self, s: &ContextScopeId, _: &ModelReleaseEvent) -> Result<bool, LearningError> {
        Ok(self.allowed && *s == scope())
    }
    fn verify_canary(
        &self,
        _: &ModelReleaseManifest,
        _: &CanaryObservation,
    ) -> Result<bool, LearningError> {
        Ok(self.verified)
    }
}
fn authority() -> Authority {
    Authority {
        allowed: true,
        verified: true,
    }
}
fn canary() -> ModelCanary {
    ModelCanary {
        cohorts: BTreeSet::from([id("cohort")]),
        starts_at: at(30),
        ends_at: at(50),
        max_requests: 10,
        max_failures: 1,
        required_successes: 2,
    }
}
fn observation(n: u32, successes: u64, failures: u64) -> CanaryObservation {
    CanaryObservation {
        id: id(&format!("observation-{n}")),
        cohort: id("cohort"),
        observed_at: at(31),
        successes,
        failures,
        evidence: id(&format!("canary-evidence-{n}")),
    }
}
fn qualified(candidate: u32) -> QualifiedModel {
    let (memory, evidence, rows) = fixture();
    let dataset = assemble_dataset(&memory, &evidence, config(), rows, at(20)).unwrap();
    let worker = Worker::good(candidate);
    let run = train_offline(
        &memory,
        &evidence,
        &worker,
        &worker,
        &dataset,
        &recipe(),
        id("job"),
        at(20),
    )
    .unwrap();
    evaluate_offline(
        &memory,
        &evidence,
        &worker,
        &worker,
        &dataset,
        &recipe(),
        run,
        &policy(),
        at(21),
    )
    .unwrap()
}

#[test]
fn reproducible_reference_only_dataset_preserves_provenance_and_negatives() {
    let (memory, evidence, mut rows) = fixture();
    let a = assemble_dataset(&memory, &evidence, config(), rows.clone(), at(20)).unwrap();
    rows.reverse();
    let b = assemble_dataset(&memory, &evidence, config(), rows, at(20)).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.rows.len(), 3);
    assert_eq!(a.duplicates.len(), 1);
    assert_eq!(a.rows[0].signal.measurements[&SignalKind::Failure], 1);
    revalidate_dataset(&memory, &evidence, &a, at(21)).unwrap();
    let json = serde_json::to_string(&a).unwrap();
    assert!(!json.contains("private-payload"));
    assert!(!json.contains("outcome"));
    assert_eq!(serde_json::from_str::<DatasetManifest>(&json).unwrap(), a);
    for row in &a.rows {
        let value = serde_json::to_value(&row.signal).unwrap();
        let mut unknown = value.clone();
        unknown["permission"] = serde_json::json!("grant");
        assert!(serde_json::from_value::<LearningSignal>(unknown).is_err());
        assert_eq!(
            serde_json::from_value::<LearningSignal>(value).unwrap(),
            row.signal
        );
    }
    let mut changed = a.clone();
    changed.version += 1;
    assert_ne!(manifest_digest(&changed), a.digest);
    let mut changed = recipe();
    changed.seed += 1;
    assert_ne!(recipe_digest(&changed), recipe_digest(&recipe()));
}

#[test]
fn unverified_measurements_pending_sources_and_cross_project_inputs_fail_closed() {
    let (mut memory, evidence, rows) = fixture();
    let mut signal = rows[0].signal.clone();
    signal.measurements.insert(SignalKind::LatencyMs, 99);
    assert_eq!(
        admit_signal(&memory, &evidence, &scope(), &signal, at(20)),
        Err(LearningError::Unverified)
    );
    signal = rows[0].signal.clone();
    signal.measurements.insert(SignalKind::Success, 1);
    assert_eq!(
        admit_signal(&memory, &evidence, &scope(), &signal, at(20)),
        Err(LearningError::InvalidSignal)
    );
    signal = rows[0].signal.clone();
    signal.validation = id("fake-validation");
    assert_eq!(
        admit_signal(&memory, &evidence, &scope(), &signal, at(20)),
        Err(LearningError::InvalidSignal)
    );
    signal = rows[0].signal.clone();
    signal.schema_version = 2;
    assert_eq!(
        admit_signal(&memory, &evidence, &scope(), &signal, at(20)),
        Err(LearningError::InvalidSignal)
    );
    assert_eq!(
        admit_signal(
            &memory,
            &evidence,
            &ContextScopeId::new("project-b").unwrap(),
            &rows[0].signal,
            at(20)
        ),
        Err(LearningError::ScopeMismatch)
    );
    memory.admit(record(5), id("admit"), at(10)).unwrap();
    signal = rows[0].signal.clone();
    signal.memory.id = record(5).id;
    assert_eq!(
        admit_signal(&memory, &evidence, &scope(), &signal, at(20)),
        Err(LearningError::Revoked)
    );
    assert_eq!(
        admit_signal(&memory, &evidence, &scope(), &rows[0].signal, at(2000)),
        Err(LearningError::Revoked)
    );
}

#[test]
fn leakage_controls_cover_content_traces_episode_families_and_reserved_corpora() {
    let (memory, mut evidence, rows) = fixture();
    let mut bad = rows.clone();
    bad[3].split = DatasetSplit::Test;
    assert_eq!(
        assemble_dataset(&memory, &evidence, config(), bad, at(20)),
        Err(LearningError::Leakage)
    );
    let mut bad = rows.clone();
    bad[1].signal.leakage_group = bad[0].signal.leakage_group.clone();
    evidence.signals = bad.iter().map(|r| r.signal.clone()).collect();
    assert_eq!(
        assemble_dataset(&memory, &evidence, config(), bad, at(20)),
        Err(LearningError::Leakage)
    );
    evidence.signals = rows.iter().map(|r| r.signal.clone()).collect();
    let mut reserved = config();
    reserved.excluded_examples.insert(hash(1));
    assert_eq!(
        assemble_dataset(&memory, &evidence, reserved, rows.clone(), at(20)),
        Err(LearningError::Leakage)
    );
    let mut bad = rows.clone();
    bad.push(rows[0].clone());
    assert_eq!(
        assemble_dataset(&memory, &evidence, config(), bad, at(20)),
        Err(LearningError::DuplicateIdentity)
    );
    let mut bad = rows;
    bad.retain(|r| r.split != DatasetSplit::Test);
    assert_eq!(
        assemble_dataset(&memory, &evidence, config(), bad, at(20)),
        Err(LearningError::EmptySplit)
    );
}

#[test]
fn revocation_and_tampering_block_jobs_before_worker_invocation() {
    let (mut memory, evidence, rows) = fixture();
    let mut dataset = assemble_dataset(&memory, &evidence, config(), rows, at(20)).unwrap();
    let worker = Worker::good(2);
    dataset.rows[0]
        .signal
        .measurements
        .insert(SignalKind::Retries, 99);
    assert_eq!(
        train_offline(
            &memory,
            &evidence,
            &worker,
            &worker,
            &dataset,
            &recipe(),
            id("job"),
            at(20)
        ),
        Err(LearningError::InvalidManifest)
    );
    dataset.digest = manifest_digest(&dataset);
    assert_eq!(
        train_offline(
            &memory,
            &evidence,
            &worker,
            &worker,
            &dataset,
            &recipe(),
            id("job"),
            at(20)
        ),
        Err(LearningError::Unverified)
    );
    let (_, _, rows) = fixture();
    dataset = assemble_dataset(&memory, &evidence, config(), rows, at(20)).unwrap();
    // Even a deduplicated source is revalidated before consumption.
    memory
        .curate(
            &scope(),
            &id("memory-4"),
            4,
            MemoryChange {
                action: MemoryAction::Forget,
                reason: id("forget"),
                at: at(21),
                replacement: None,
                successor: None,
            },
        )
        .unwrap();
    assert_eq!(
        train_offline(
            &memory,
            &evidence,
            &worker,
            &worker,
            &dataset,
            &recipe(),
            id("job"),
            at(21)
        ),
        Err(LearningError::Revoked)
    );
    assert_eq!(worker.calls.get(), 0);
}

#[test]
fn offline_authorization_binding_and_objective_evaluation_are_required() {
    let (memory, evidence, rows) = fixture();
    let dataset = assemble_dataset(&memory, &evidence, config(), rows, at(20)).unwrap();
    let mut worker = Worker::good(2);
    worker.authorized = false;
    assert_eq!(
        train_offline(
            &memory,
            &evidence,
            &worker,
            &worker,
            &dataset,
            &recipe(),
            id("job"),
            at(20)
        ),
        Err(LearningError::Unauthorized)
    );
    assert_eq!(worker.calls.get(), 0);
    worker.authorized = true;
    worker.wrong_binding = true;
    assert_eq!(
        train_offline(
            &memory,
            &evidence,
            &worker,
            &worker,
            &dataset,
            &recipe(),
            id("job"),
            at(20)
        ),
        Err(LearningError::InvalidRun)
    );
    worker.wrong_binding = false;
    let run = train_offline(
        &memory,
        &evidence,
        &worker,
        &worker,
        &dataset,
        &recipe(),
        id("job"),
        at(20),
    )
    .unwrap();
    worker.fail = true;
    assert!(matches!(
        evaluate_offline(
            &memory,
            &evidence,
            &worker,
            &worker,
            &dataset,
            &recipe(),
            run.clone(),
            &policy(),
            at(21)
        ),
        Err(LearningError::Evaluation(_))
    ));
    worker.fail = false;
    worker.wrong_binding = true;
    assert!(matches!(
        evaluate_offline(
            &memory,
            &evidence,
            &worker,
            &worker,
            &dataset,
            &recipe(),
            run.clone(),
            &policy(),
            at(21)
        ),
        Err(LearningError::InvalidRun)
    ));
    worker.wrong_binding = false;
    let q = evaluate_offline(
        &memory,
        &evidence,
        &worker,
        &worker,
        &dataset,
        &recipe(),
        run,
        &policy(),
        at(21),
    )
    .unwrap();
    assert_eq!(q.evaluation().cases, 1);
    assert!(
        q.evaluation()
            .scores_millionths
            .values()
            .all(|v| *v == 1_000_000)
    );
}

#[test]
fn immutable_releases_canary_supersession_and_exact_rollback_keep_audit_history() {
    let mut registry = ModelReleaseRegistry::new(scope());
    let authority = authority();
    let first = registry
        .register(&authority, qualified(2), canary(), decision(1, 25))
        .unwrap();
    registry
        .start_canary(&authority, &first, decision(2, 30))
        .unwrap();
    assert_eq!(
        registry.activate(&authority, &first, decision(3, 31)),
        Err(LearningError::InvalidTransition)
    );
    registry
        .observe_canary(&authority, &first, observation(1, 2, 0), decision(3, 31))
        .unwrap();
    registry
        .activate(&authority, &first, decision(4, 32))
        .unwrap();
    let original = registry.manifest(&first).unwrap().clone();
    let second = registry
        .register(
            &authority,
            qualified(3),
            ModelCanary {
                starts_at: at(40),
                ends_at: at(60),
                ..canary()
            },
            decision(5, 35),
        )
        .unwrap();
    registry
        .start_canary(&authority, &second, decision(6, 40))
        .unwrap();
    registry
        .observe_canary(
            &authority,
            &second,
            CanaryObservation {
                observed_at: at(41),
                ..observation(2, 2, 0)
            },
            decision(7, 41),
        )
        .unwrap();
    registry
        .activate(&authority, &second, decision(8, 42))
        .unwrap();
    assert_eq!(registry.active(), Some(&second));
    assert_eq!(registry.state(&first), Some(ModelReleaseState::Superseded));
    registry
        .rollback(&authority, &second, decision(9, 43))
        .unwrap();
    assert_eq!(registry.active(), Some(&first));
    assert_eq!(registry.state(&second), Some(ModelReleaseState::RolledBack));
    assert_eq!(registry.manifest(&first), Some(&original));
    assert_eq!(release_digest(&original), original.digest);
    assert_eq!(
        registry.events().last().unwrap().restored,
        Some(first.clone())
    );
    assert_eq!(registry.events().len(), 9);
    assert_eq!(
        registry.register(
            &authority,
            qualified(3),
            ModelCanary {
                starts_at: at(50),
                ends_at: at(60),
                ..canary()
            },
            decision(10, 44)
        ),
        Err(LearningError::DuplicateIdentity)
    );
    let release_json = serde_json::to_string(registry.manifest(&second).unwrap()).unwrap();
    assert_eq!(
        serde_json::from_str::<ModelReleaseManifest>(&release_json).unwrap(),
        *registry.manifest(&second).unwrap()
    );
    if let Ok(path) = std::env::var("CG28_LEARNING_OUTPUT") {
        let (memory, evidence, rows) = fixture();
        let dataset = assemble_dataset(&memory, &evidence, config(), rows, at(20)).unwrap();
        std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({ "version":1,"fixture":"synthetic-offline-worker",
            "dataset":dataset,"recipe":recipe(),"releases":[registry.manifest(&first),registry.manifest(&second)],"events":registry.events(),"active":registry.active() })).unwrap()).unwrap();
    }
}

#[test]
fn denied_or_unverified_rollout_and_failed_canary_do_not_change_routing() {
    let mut registry = ModelReleaseRegistry::new(scope());
    let denied = Authority {
        allowed: false,
        verified: true,
    };
    assert_eq!(
        registry.register(&denied, qualified(2), canary(), decision(1, 25)),
        Err(LearningError::Unauthorized)
    );
    assert!(registry.events().is_empty());
    let authority = authority();
    let model = registry
        .register(&authority, qualified(2), canary(), decision(1, 25))
        .unwrap();
    registry
        .start_canary(&authority, &model, decision(2, 30))
        .unwrap();
    let unverified = Authority {
        allowed: true,
        verified: false,
    };
    assert_eq!(
        registry.observe_canary(&unverified, &model, observation(1, 2, 0), decision(3, 31)),
        Err(LearningError::Unverified)
    );
    assert_eq!(registry.events().len(), 2);
    registry
        .observe_canary(&authority, &model, observation(1, 2, 2), decision(3, 31))
        .unwrap();
    assert_eq!(
        registry.observe_canary(&authority, &model, observation(1, 2, 2), decision(4, 31)),
        Err(LearningError::DuplicateIdentity)
    );
    assert_eq!(
        registry.activate(&authority, &model, decision(4, 32)),
        Err(LearningError::InvalidTransition)
    );
    registry
        .rollback(&authority, &model, decision(4, 32))
        .unwrap();
    assert!(registry.active().is_none());
    assert_eq!(
        registry.activate(&authority, &model, decision(5, 33)),
        Err(LearningError::InvalidTransition)
    );
}

#[test]
fn upgrade_impact_is_scoped_and_requires_reembedding_reevaluation_and_recertification() {
    let kinds = [
        ModelArtifactKind::Embedding,
        ModelArtifactKind::VectorIndex,
        ModelArtifactKind::LearnedProcedure,
        ModelArtifactKind::SemanticMapping,
        ModelArtifactKind::PromptCache,
        ModelArtifactKind::EvaluationBaseline,
    ];
    let mut artifacts: Vec<_> = kinds
        .iter()
        .enumerate()
        .map(|(n, kind)| ModelDependentArtifact {
            id: id(&format!("artifact-{n}")),
            scope: scope(),
            kind: *kind,
            models: BTreeSet::from([model(1)]),
            digest: hash(50 + n as u32),
        })
        .collect();
    artifacts.push(ModelDependentArtifact {
        id: id("unaffected"),
        models: BTreeSet::from([model(5)]),
        ..artifacts[0].clone()
    });
    let impact = upgrade_impact(&scope(), &model(1), &model(2), &artifacts).unwrap();
    assert_eq!(impact.len(), 6);
    assert_eq!(impact[0].action, UpgradeAction::Reembed);
    assert_eq!(impact[2].action, UpgradeAction::Recertify);
    assert_eq!(impact[3].action, UpgradeAction::Reevaluate);
    artifacts[0].scope = ContextScopeId::new("other-project").unwrap();
    assert_eq!(
        upgrade_impact(&scope(), &model(1), &model(2), &artifacts),
        Err(LearningError::ScopeMismatch)
    );
}

#[test]
fn trace_and_source_content_cannot_be_partitioned_under_new_signal_ids() {
    let (mut memory, mut evidence, rows) = fixture();
    let mut copy = rows[0].clone();
    copy.signal.id = id("signal-other");
    copy.signal.example_digest = hash(999);
    copy.signal.leakage_group = id("another-episode");
    copy.split = DatasetSplit::Test;
    evidence.signals.push(copy.signal.clone());
    let mut bad = rows.clone();
    bad.push(copy.clone());
    assert_eq!(
        assemble_dataset(&memory, &evidence, config(), bad, at(20)),
        Err(LearningError::Leakage)
    );
    let mut source = record(6);
    source.source_digest = rows[0].signal.memory.source_digest.clone();
    memory.admit(source.clone(), id("admit"), at(10)).unwrap();
    memory
        .curate(
            &scope(),
            &source.id,
            1,
            MemoryChange {
                action: MemoryAction::Validate,
                reason: id("review"),
                at: at(11),
                replacement: None,
                successor: None,
            },
        )
        .unwrap();
    copy.signal.memory = memory
        .eligibility_reference(&scope(), &source.id, at(20))
        .unwrap();
    copy.signal.provenance = source.provenance;
    copy.signal.measurements.insert(SignalKind::Success, 1);
    copy.signal.measurements.insert(SignalKind::Failure, 0);
    evidence.signals.push(copy.signal.clone());
    let mut bad = rows;
    bad.push(copy);
    assert_eq!(
        assemble_dataset(&memory, &evidence, config(), bad, at(20)),
        Err(LearningError::Leakage)
    );
}

#[test]
fn manifest_structure_recipe_and_evaluation_revocation_are_rechecked() {
    let (memory, evidence, rows) = fixture();
    let dataset = assemble_dataset(&memory, &evidence, config(), rows, at(20)).unwrap();
    let mut changed = dataset.clone();
    changed.rows.reverse();
    changed.digest = manifest_digest(&changed);
    assert_eq!(
        revalidate_dataset(&memory, &evidence, &changed, at(21)),
        Err(LearningError::InvalidManifest)
    );
    assert_eq!(
        revalidate_dataset(&memory, &evidence, &dataset, at(19)),
        Err(LearningError::InvalidManifest)
    );
    let worker = Worker::good(2);
    let mut invalid_recipe = recipe();
    invalid_recipe.version = 0;
    assert_eq!(
        train_offline(
            &memory,
            &evidence,
            &worker,
            &worker,
            &dataset,
            &invalid_recipe,
            id("job"),
            at(20)
        ),
        Err(LearningError::InvalidManifest)
    );
    let run = train_offline(
        &memory,
        &evidence,
        &worker,
        &worker,
        &dataset,
        &recipe(),
        id("job"),
        at(20),
    )
    .unwrap();
    let mut invalid_run = run.clone();
    invalid_run.recipe_digest = hash(999);
    assert!(matches!(
        evaluate_offline(
            &memory,
            &evidence,
            &worker,
            &worker,
            &dataset,
            &recipe(),
            invalid_run,
            &policy(),
            at(21)
        ),
        Err(LearningError::InvalidRun)
    ));
    let denied = Worker {
        authorized: false,
        ..Worker::good(2)
    };
    assert!(matches!(
        evaluate_offline(
            &memory,
            &evidence,
            &denied,
            &denied,
            &dataset,
            &recipe(),
            run.clone(),
            &policy(),
            at(21)
        ),
        Err(LearningError::Unauthorized)
    ));
    assert_eq!(denied.calls.get(), 0);
    evidence.denied.set(true);
    assert!(matches!(
        evaluate_offline(
            &memory,
            &evidence,
            &worker,
            &worker,
            &dataset,
            &recipe(),
            run,
            &policy(),
            at(21)
        ),
        Err(LearningError::Unverified)
    ));
    // Learning admission and offline job calls do not mutate curation decisions.
    assert_eq!(
        memory
            .store()
            .decisions(&scope(), &id("memory-1"))
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn canary_bounds_checked_arithmetic_and_denied_transitions_preserve_state() {
    let mut registry = ModelReleaseRegistry::new(scope());
    let auth = authority();
    let invalid = ModelCanary {
        required_successes: 0,
        ..canary()
    };
    assert_eq!(
        registry.register(&auth, qualified(2), invalid, decision(1, 25)),
        Err(LearningError::InvalidRelease)
    );
    let mut other = ModelReleaseRegistry::new(ContextScopeId::new("other").unwrap());
    assert_eq!(
        other.register(&auth, qualified(2), canary(), decision(1, 25)),
        Err(LearningError::ScopeMismatch)
    );
    let model = registry
        .register(&auth, qualified(2), canary(), decision(1, 25))
        .unwrap();
    assert_eq!(
        registry.start_canary(&auth, &model, decision(2, 29)),
        Err(LearningError::InvalidTransition)
    );
    registry
        .start_canary(&auth, &model, decision(2, 30))
        .unwrap();
    for bad in [
        CanaryObservation {
            cohort: id("other"),
            ..observation(1, 2, 0)
        },
        CanaryObservation {
            observed_at: at(29),
            ..observation(1, 2, 0)
        },
        observation(1, 0, 0),
        observation(1, 11, 0),
    ] {
        assert_eq!(
            registry.observe_canary(&auth, &model, bad, decision(3, 31)),
            Err(LearningError::InvalidTransition)
        );
    }
    registry
        .observe_canary(&auth, &model, observation(1, 2, 0), decision(3, 31))
        .unwrap();
    assert_eq!(
        registry.observe_canary(&auth, &model, observation(2, u64::MAX, 0), decision(4, 31)),
        Err(LearningError::InvalidRun)
    );
    let denied = Authority {
        allowed: false,
        verified: true,
    };
    assert_eq!(
        registry.activate(&denied, &model, decision(4, 32)),
        Err(LearningError::Unauthorized)
    );
    assert!(registry.active().is_none());
    assert_eq!(
        registry.activate(&auth, &model, decision(3, 32)),
        Err(LearningError::DuplicateIdentity)
    );
    registry.activate(&auth, &model, decision(4, 32)).unwrap();
    assert_eq!(
        registry.rollback(&denied, &model, decision(5, 33)),
        Err(LearningError::Unauthorized)
    );
    assert_eq!(registry.active(), Some(&model));
    registry.rollback(&auth, &model, decision(5, 33)).unwrap();
    assert!(registry.active().is_none());
    assert_eq!(
        registry.rollback(&auth, &model, decision(6, 34)),
        Err(LearningError::InvalidTransition)
    );
}

#[test]
fn canary_rollback_retains_incumbent_and_upgrade_inventory_rejects_ambiguity() {
    let mut registry = ModelReleaseRegistry::new(scope());
    let auth = authority();
    let first = registry
        .register(&auth, qualified(2), canary(), decision(1, 25))
        .unwrap();
    registry
        .start_canary(&auth, &first, decision(2, 30))
        .unwrap();
    registry
        .observe_canary(&auth, &first, observation(1, 2, 0), decision(3, 31))
        .unwrap();
    registry.activate(&auth, &first, decision(4, 32)).unwrap();
    let second = registry
        .register(
            &auth,
            qualified(3),
            ModelCanary {
                starts_at: at(40),
                ends_at: at(60),
                ..canary()
            },
            decision(5, 35),
        )
        .unwrap();
    registry
        .start_canary(&auth, &second, decision(6, 40))
        .unwrap();
    registry.rollback(&auth, &second, decision(7, 41)).unwrap();
    assert_eq!(registry.active(), Some(&first));
    assert_eq!(registry.events().last().unwrap().restored, Some(first));
    assert_eq!(
        upgrade_impact(&scope(), &model(1), &model(1), &[]),
        Err(LearningError::InvalidManifest)
    );
    let artifact = ModelDependentArtifact {
        id: id("artifact"),
        scope: scope(),
        kind: ModelArtifactKind::Embedding,
        models: BTreeSet::from([model(1)]),
        digest: hash(10),
    };
    assert_eq!(
        upgrade_impact(
            &scope(),
            &model(1),
            &model(2),
            &[artifact.clone(), artifact]
        ),
        Err(LearningError::DuplicateIdentity)
    );
}

#[path = "support/epic03_learning.rs"]
mod complete;
