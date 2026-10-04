use super::*;

impl gateway_application::model_releases::ModelRecoveryAuthority for Authority {
    fn verify_qualification(&self, manifest: &ModelReleaseManifest) -> Result<bool, LearningError> {
        Ok(self.verified
            && manifest.training.evidence == id("training-evidence")
            && manifest.training.recipe_digest == recipe_digest(&manifest.recipe))
    }
}

#[test]
fn model_journal_replays_authority_and_rejects_tampering_or_revocation() {
    use gateway_application::model_releases::ModelReleaseJournal;
    let auth = authority();
    let mut registry = ModelReleaseRegistry::new(scope());
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
    registry
        .observe_canary(
            &auth,
            &second,
            CanaryObservation {
                observed_at: at(41),
                ..observation(2, 2, 0)
            },
            decision(7, 41),
        )
        .unwrap();
    registry.activate(&auth, &second, decision(8, 42)).unwrap();
    registry.rollback(&auth, &second, decision(9, 43)).unwrap();
    let journal = registry.journal();
    let recovered = ModelReleaseRegistry::recover(&journal, &auth).unwrap();
    assert_eq!(recovered.journal(), journal);
    assert_eq!(recovered.active(), Some(&first));
    for change in 0..10 {
        let mut bad = journal.clone();
        match change {
            0 => bad.manifests[0].digest = hash(999),
            1 => bad.manifests.push(bad.manifests[0].clone()),
            2 => bad.manifests[0].scope = ContextScopeId::new("other").unwrap(),
            3 => bad.manifests[0].predecessor = Some(model(99)),
            4 => bad.events[2].observation = None,
            5 => bad.events[8].restored = None,
            6 => bad.events[0].observation = Some(observation(99, 1, 0)),
            7 => bad.events = vec![],
            8 => bad.events = vec![bad.events[0].clone(); 4097],
            _ => bad.manifests = vec![bad.manifests[0].clone(); 257],
        }
        assert!(
            ModelReleaseRegistry::recover(&bad, &auth).is_err(),
            "change {change}"
        );
    }
    assert!(
        ModelReleaseRegistry::recover(
            &journal,
            &Authority {
                allowed: true,
                verified: false
            }
        )
        .is_err()
    );
    assert!(
        ModelReleaseRegistry::recover(
            &journal,
            &Authority {
                allowed: false,
                verified: true
            }
        )
        .is_err()
    );
    let empty = ModelReleaseJournal {
        scope: scope(),
        manifests: vec![],
        events: vec![],
    };
    assert!(
        ModelReleaseRegistry::recover(&empty, &auth)
            .unwrap()
            .active()
            .is_none()
    );
}

struct CpuFeatures {
    signals: Vec<LearningSignal>,
}
impl gateway_daemon::cpu_learning::LearningFeaturePort for CpuFeatures {
    fn features(&self, row: &DatasetRow) -> Result<BTreeMap<String, f64>, LearningError> {
        if !self.signals.contains(&row.signal) {
            return Err(LearningError::Unverified);
        }
        let n: u32 = row
            .signal
            .id
            .as_str()
            .trim_start_matches("signal-")
            .parse()
            .unwrap();
        Ok(BTreeMap::from([
            (
                "health".into(),
                if n % 2 == 0 {
                    1.0 + n as f64 / 1000.0
                } else {
                    -1.0 - n as f64 / 1000.0
                },
            ),
            ("noise".into(), f64::from(n % 3) / 1000.0),
        ]))
    }
}
fn cpu_dataset() -> (
    MemoryApplication<InMemoryMemoryStore>,
    Evidence,
    DatasetManifest,
) {
    let mut memory = MemoryApplication::new(InMemoryMemoryStore::default());
    let mut selections = vec![];
    for n in 1..=96 {
        let mut source = record(n);
        source.observed_at = at(i64::from(n) + 10);
        source.created_at = at(200);
        source.valid_from = at(200);
        source.outcome =
            Some(NonEmptyText::new(if n % 2 == 0 { "SUCCESS" } else { "FAILURE" }).unwrap());
        memory.admit(source.clone(), id("admit"), at(200)).unwrap();
        memory
            .curate(
                &scope(),
                &source.id,
                1,
                MemoryChange {
                    action: MemoryAction::Validate,
                    reason: id("review"),
                    at: at(201),
                    replacement: None,
                    successor: None,
                },
            )
            .unwrap();
        let signal = LearningSignal {
            schema_version: 1,
            id: id(&format!("signal-{n}")),
            memory: memory
                .eligibility_reference(&scope(), &source.id, at(202))
                .unwrap(),
            provenance: source.provenance,
            validation: id("validated"),
            label_basis: id("label"),
            evaluation: id("evaluation"),
            evidence: BTreeSet::from([EvidenceId::new(format!("evidence-{n}")).unwrap()]),
            observed_at: source.observed_at,
            producer_model: model(1),
            example_digest: hash(1000 + n),
            leakage_group: id(&format!("episode-{}", (n - 1) / 2)),
            measurements: BTreeMap::from([
                (SignalKind::Success, u64::from(n % 2 == 0)),
                (SignalKind::Failure, u64::from(n % 2 != 0)),
            ]),
        };
        selections.push(DatasetRow {
            signal,
            split: if n <= 48 {
                DatasetSplit::Train
            } else if n <= 72 {
                DatasetSplit::Validation
            } else {
                DatasetSplit::Test
            },
        });
    }
    let evidence = Evidence {
        signals: selections.iter().map(|r| r.signal.clone()).collect(),
        denied: Cell::new(false),
    };
    let dataset = assemble_dataset(
        &memory,
        &evidence,
        DatasetConfiguration {
            max_signals: 100,
            ..config()
        },
        selections,
        at(202),
    )
    .unwrap();
    (memory, evidence, dataset)
}
fn cpu_recipe() -> TrainingRecipe {
    let mut recipe = recipe();
    recipe.parameters=BTreeMap::from([
        ("ml_plan".into(),serde_json::json!({"feature_schema_version":"health-features-v1","features":["health","noise"],"top_k":[1,2],"temperatures":[0.5,1,2],
            "cv":"chronological","folds":3,"search":"grid","seed":42,"budget":6,"objective":"f1","stop_f1":null}).to_string()),
        ("ml_profile".into(),serde_json::json!({"task":"binary-classification","version":1,"floors":{"precision":1,"recall":1,"f1":1},"ceilings":{"false_positive_rate":0,"brier":0.01,"ece":0.1},"max_regression":0}).to_string())]);
    recipe
}
fn cpu_adapter(
    root: &std::path::Path,
    signals: Vec<LearningSignal>,
) -> gateway_daemon::cpu_learning::CpuOfflineAdapter<CpuFeatures> {
    gateway_daemon::cpu_learning::CpuOfflineAdapter {
        process: gateway_daemon::bounded_process::BoundedProcess {
            interpreter: "/usr/bin/python3".into(),
            script: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../services/offline-learning/pipeline.py")
                .canonicalize()
                .unwrap(),
            work_root: root.into(),
        },
        artifacts: root.into(),
        features: CpuFeatures { signals },
        clock: || at(203),
    }
}
fn test_dir(label: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "cg03-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    root
}
#[test]
fn real_cpu_adapter_uses_governed_sources_separate_training_and_held_out_test() {
    let root = test_dir("cpu");
    let (memory, evidence, dataset) = cpu_dataset();
    let adapter = cpu_adapter(&root, evidence.signals.clone());
    let recipe = cpu_recipe();
    let run = train_offline(
        &memory,
        &evidence,
        &Worker::good(2),
        &adapter,
        &dataset,
        &recipe,
        id("real-cpu-job"),
        at(202),
    )
    .unwrap();
    let repeat = train_offline(
        &memory,
        &evidence,
        &Worker::good(2),
        &adapter,
        &dataset,
        &recipe,
        id("real-cpu-job"),
        at(202),
    )
    .unwrap();
    assert_eq!(run, repeat);
    let qualified = evaluate_offline(
        &memory,
        &evidence,
        &Worker::good(2),
        &adapter,
        &dataset,
        &recipe,
        run.clone(),
        &policy(),
        at(203),
    )
    .unwrap();
    assert_eq!(qualified.evaluation().cases, 24);
    let repeated = evaluate_offline(
        &memory,
        &evidence,
        &Worker::good(2),
        &adapter,
        &dataset,
        &recipe,
        run.clone(),
        &policy(),
        at(203),
    )
    .unwrap();
    assert_eq!(qualified.evaluation(), repeated.evaluation());
    let training: Vec<_> = dataset
        .rows
        .iter()
        .filter(|r| r.split != DatasetSplit::Test)
        .cloned()
        .collect();
    let test: Vec<_> = dataset
        .rows
        .iter()
        .filter(|r| r.split == DatasetSplit::Test)
        .cloned()
        .collect();
    let job = OfflineJob {
        id: id("real-cpu-job"),
        scope: scope(),
        dataset_digest: dataset.digest.clone(),
        recipe_digest: recipe_digest(&recipe),
        base_model: recipe.base_model.clone(),
    };
    assert!(adapter.train(&job, &recipe, &dataset.rows).is_err());
    assert!(adapter.evaluate(&run, &recipe, &training).is_err());
    let mut wrong = run.clone();
    wrong.evidence = id("fabricated");
    assert!(adapter.evaluate(&wrong, &recipe, &test).is_err());
    struct NonfiniteFeatures;
    impl gateway_daemon::cpu_learning::LearningFeaturePort for NonfiniteFeatures {
        fn features(&self, _: &DatasetRow) -> Result<BTreeMap<String, f64>, LearningError> {
            Ok(BTreeMap::from([("health".into(), f64::NAN)]))
        }
    }
    let invalid_features = gateway_daemon::cpu_learning::CpuOfflineAdapter {
        process: adapter.process.clone(),
        artifacts: root.clone(),
        features: NonfiniteFeatures,
        clock: || at(203),
    };
    assert!(invalid_features.train(&job, &recipe, &training).is_err());
    let denied = cpu_adapter(&root, vec![]);
    assert!(denied.train(&job, &recipe, &training).is_err());
    let missing_store = cpu_adapter(&root, evidence.signals.clone());
    let missing_store = gateway_daemon::cpu_learning::CpuOfflineAdapter {
        artifacts: root.join("missing"),
        ..missing_store
    };
    assert_eq!(
        missing_store.train(&job, &recipe, &training),
        Err(LearningError::Storage)
    );
    let bad_script = root.join("bad-worker.py");
    for output in ["not-json", "{}"] {
        std::fs::write(&bad_script, format!("print({output:?})\n")).unwrap();
        let mut bad_adapter = cpu_adapter(&root, evidence.signals.clone());
        bad_adapter.process.script = bad_script.clone();
        assert_eq!(
            bad_adapter.train(&job, &recipe, &training),
            Err(LearningError::InvalidRun)
        );
    }
    let mut bad_recipe = recipe.clone();
    bad_recipe.parameters.clear();
    assert!(adapter.train(&job, &bad_recipe, &training).is_err());
    let mut strict = recipe.clone();
    strict.parameters.insert("ml_profile".into(),serde_json::json!({"task":"binary-classification","version":1,"floors":{"f1":1},"ceilings":{"brier":0},"max_regression":0}).to_string());
    // Changed recipe binding is rejected before attempting evaluation.
    assert!(adapter.evaluate(&run, &strict, &test).is_err());
    std::fs::write(
        root.join(run.candidate.artifact_digest.as_str()),
        b"corrupt",
    )
    .unwrap();
    assert!(adapter.artifact(&run.candidate).is_err());
    assert!(adapter.artifact(&model(99)).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

fn durable_connection() -> String {
    let raw = std::env::var("CG_COGNITIVE_TEST_DATABASE")
        .expect("run through scripts/cognitive-test-host.py");
    let mut client = postgres::Client::connect(&raw, postgres::NoTls).unwrap();
    let schema = format!(
        "cg03_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    client
        .batch_execute(&format!("CREATE SCHEMA {schema}"))
        .unwrap();
    format!("{raw} options='-c search_path={schema}'")
}
struct CpuAuthority {
    runs: Vec<TrainingRun>,
    artifacts: std::path::PathBuf,
    verified: Cell<bool>,
}
impl ModelReleaseAuthority for CpuAuthority {
    fn authorize(&self, s: &ContextScopeId, e: &ModelReleaseEvent) -> Result<bool, LearningError> {
        Ok(*s == scope()
            && e.decision.actor.as_str() == "operator"
            && e.decision.policy_decision == id("approval"))
    }
    fn verify_canary(
        &self,
        _: &ModelReleaseManifest,
        o: &CanaryObservation,
    ) -> Result<bool, LearningError> {
        Ok(self.verified.get() && o.evidence.as_str().starts_with("canary-evidence-"))
    }
}
impl gateway_application::model_releases::ModelRecoveryAuthority for CpuAuthority {
    fn verify_qualification(&self, m: &ModelReleaseManifest) -> Result<bool, LearningError> {
        use sha2::{Digest, Sha256};
        if !self.verified.get()
            || !self.runs.contains(&m.training)
            || m.training.recipe_digest != recipe_digest(&m.recipe)
        {
            return Ok(false);
        }
        let model = std::fs::read(
            self.artifacts
                .join(m.training.candidate.artifact_digest.as_str()),
        )
        .map_err(|_| LearningError::Storage)?;
        let evaluation = std::fs::read(
            self.artifacts
                .join(m.evaluation.evidence.as_str().trim_start_matches("sha256-")),
        )
        .map_err(|_| LearningError::Storage)?;
        let result: serde_json::Value =
            serde_json::from_slice(&evaluation).map_err(|_| LearningError::InvalidRun)?;
        Ok(
            format!("{:x}", Sha256::digest(model)) == m.training.candidate.artifact_digest.as_str()
                && format!("sha256-{:x}", Sha256::digest(evaluation))
                    == m.evaluation.evidence.as_str()
                && result["status"] == "PASS",
        )
    }
}
fn durable_models(connection: &str) -> gateway_daemon::durable_models::DurableModelReleases {
    gateway_daemon::durable_models::DurableModelReleases::new(
        gateway_daemon::cognitive_store::CognitiveStore::connect(connection, scope()).unwrap(),
    )
}
#[test]
#[ignore = "requires disposable PostgreSQL host; included in full qualification"]
fn durable_cpu_release_restart_inference_supersession_and_rollback() {
    use gateway_application::local_inference::{
        LocalInferenceError, LocalInferencePort, LocalInferenceRequest,
    };
    let root = test_dir("live-cpu");
    let connection = durable_connection();
    let (memory, evidence, dataset) = cpu_dataset();
    let adapter = cpu_adapter(&root, evidence.signals.clone());
    let recipe = cpu_recipe();
    let run = train_offline(
        &memory,
        &evidence,
        &Worker::good(2),
        &adapter,
        &dataset,
        &recipe,
        id("live-job-v2"),
        at(202),
    )
    .unwrap();
    let q = evaluate_offline(
        &memory,
        &evidence,
        &Worker::good(2),
        &adapter,
        &dataset,
        &recipe,
        run.clone(),
        &policy(),
        at(203),
    )
    .unwrap();
    let mut second_recipe = recipe.clone();
    second_recipe.base_model = run.candidate.clone();
    second_recipe.version = 2;
    second_recipe.parameters.insert(
        "prior_model".into(),
        serde_json::to_string(&run.candidate).unwrap(),
    );
    let second_run = train_offline(
        &memory,
        &evidence,
        &Worker::good(3),
        &adapter,
        &dataset,
        &second_recipe,
        id("live-job-v3"),
        at(202),
    )
    .unwrap();
    let second_q = evaluate_offline(
        &memory,
        &evidence,
        &Worker::good(3),
        &adapter,
        &dataset,
        &second_recipe,
        second_run.clone(),
        &policy(),
        at(203),
    )
    .unwrap();
    let auth = CpuAuthority {
        runs: vec![run.clone(), second_run.clone()],
        artifacts: root.clone(),
        verified: Cell::new(true),
    };
    let releases = durable_models(&connection);
    let bound = ModelCanary {
        starts_at: at(210),
        ends_at: at(230),
        ..canary()
    };
    let first = releases
        .apply(&auth, |r| {
            r.register(&auth, q, bound.clone(), decision(1, 204))
        })
        .unwrap();
    releases
        .apply(&auth, |r| r.start_canary(&auth, &first, decision(2, 210)))
        .unwrap();
    releases
        .apply(&auth, |r| {
            r.observe_canary(
                &auth,
                &first,
                CanaryObservation {
                    observed_at: at(211),
                    ..observation(1, 2, 0)
                },
                decision(3, 211),
            )
        })
        .unwrap();
    releases
        .apply(&auth, |r| r.activate(&auth, &first, decision(4, 212)))
        .unwrap();
    let request = LocalInferenceRequest {
        schema_version: "1.0".into(),
        role: "outcome-classifier".into(),
        input_contract: "health-features-v1".into(),
        output_contract: "binary-outcome-proposal-v1".into(),
        prompt: "{\"health\":1.075,\"noise\":0.001}".into(),
        output_schema: serde_json::json!({"type":"object"}),
    };
    {
        let inference = gateway_daemon::cpu_learning::CpuReleaseInference {
            releases: &releases,
            authority: &auth,
            artifacts: root.clone(),
            process: adapter.process.clone(),
        };
        let proposal = inference.infer(&request).unwrap();
        assert_eq!(proposal.proposal["label"], 1);
        assert_eq!(proposal.provenance.model_version, "2");
        let altered_script = root.join("altered-predictor.py");
        std::fs::write(&altered_script, b"print('changed runtime')").unwrap();
        let altered_inference = gateway_daemon::cpu_learning::CpuReleaseInference {
            releases: &releases,
            authority: &auth,
            artifacts: root.clone(),
            process: gateway_daemon::bounded_process::BoundedProcess {
                script: altered_script,
                ..adapter.process.clone()
            },
        };
        assert_eq!(
            altered_inference.infer(&request).unwrap_err(),
            LocalInferenceError::InvalidProposal
        );

        let mut wrong = request.clone();
        wrong.role = "training".into();
        assert_eq!(
            inference.infer(&wrong).unwrap_err(),
            LocalInferenceError::InvalidRequest
        );
        wrong = request.clone();
        wrong.prompt = "invalid-json".into();
        assert_eq!(
            inference.infer(&wrong).unwrap_err(),
            LocalInferenceError::InvalidRequest
        );
        wrong = request.clone();
        wrong.prompt = "{}".into();
        assert_eq!(
            inference.infer(&wrong).unwrap_err(),
            LocalInferenceError::Unavailable
        );
    }
    let second = releases
        .apply(&auth, |r| {
            r.register(
                &auth,
                second_q,
                ModelCanary {
                    starts_at: at(220),
                    ends_at: at(240),
                    ..bound
                },
                decision(5, 215),
            )
        })
        .unwrap();
    releases
        .apply(&auth, |r| r.start_canary(&auth, &second, decision(6, 220)))
        .unwrap();
    releases
        .apply(&auth, |r| {
            r.observe_canary(
                &auth,
                &second,
                CanaryObservation {
                    observed_at: at(221),
                    ..observation(2, 2, 0)
                },
                decision(7, 221),
            )
        })
        .unwrap();
    releases
        .apply(&auth, |r| r.activate(&auth, &second, decision(8, 222)))
        .unwrap();
    assert_eq!(releases.active(&auth).unwrap(), Some(second.clone()));
    drop(releases);
    // Actual database process restart, not just an in-memory reconstruction.
    let container = std::env::var("CG_COGNITIVE_TEST_CONTAINER")
        .expect("disposable container required for restart proof");
    {
        assert!(
            std::process::Command::new("docker")
                .args(["restart", "--timeout", "5", &container])
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        let mut ready = false;
        for _ in 0..60 {
            if std::process::Command::new("docker")
                .args(["exec", &container, "pg_isready", "-U", "cg", "-d", "cg"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
            {
                ready = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        assert!(ready);
    }
    let releases = durable_models(&connection);
    let inference = gateway_daemon::cpu_learning::CpuReleaseInference {
        releases: &releases,
        authority: &auth,
        artifacts: root.clone(),
        process: adapter.process.clone(),
    };
    assert_eq!(
        inference.infer(&request).unwrap().provenance.model_version,
        "3"
    );
    releases
        .apply(&auth, |r| r.rollback(&auth, &second, decision(9, 223)))
        .unwrap();
    assert_eq!(
        inference.infer(&request).unwrap().provenance.model_version,
        "2"
    );
    let journal = releases.apply(&auth, |r| Ok(r.journal())).unwrap();
    auth.verified.set(false);
    assert_eq!(
        inference.infer(&request).unwrap_err(),
        LocalInferenceError::Unavailable
    );
    auth.verified.set(true);
    if let Ok(path) = std::env::var("CG03_LIVE_OUTPUT") {
        let output_root = std::path::Path::new(&path).parent().unwrap();
        for manifest in &journal.manifests {
            let version = manifest.training.candidate.version;
            std::fs::copy(
                root.join(manifest.training.candidate.artifact_digest.as_str()),
                output_root.join(format!("epic03-cpu-model-v{version}.json")),
            )
            .unwrap();
            std::fs::copy(
                root.join(
                    manifest
                        .evaluation
                        .evidence
                        .as_str()
                        .trim_start_matches("sha256-"),
                ),
                output_root.join(format!("epic03-cpu-evaluation-v{version}.json")),
            )
            .unwrap();
        }

        let evaluation_path = root.join(
            journal
                .manifests
                .iter()
                .find(|m| m.training.candidate == first)
                .unwrap()
                .evaluation
                .evidence
                .as_str()
                .trim_start_matches("sha256-"),
        );
        let evaluation: serde_json::Value =
            serde_json::from_slice(&std::fs::read(evaluation_path).unwrap()).unwrap();
        std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({"schema_version":1,"status":"PASS","task":"binary-classification","dataset":dataset,
            "journal":journal,"metrics":evaluation["metrics"],"baseline":evaluation["baseline"],"prior_comparison":true,"real_cpu_training":true,
            "postgres_restart":true,"inference_versions":[2,3,2],"revoked_qualification_refused":true,"test_used_for_selection":false})).unwrap()).unwrap();
    }
    // Persistence rollback on a failed compound operation: even an earlier valid
    // mutation inside the closure cannot leak into subsequent inference.
    assert!(
        releases
            .apply(&auth, |r| {
                r.rollback(&auth, &first, decision(10, 224))?;
                Err::<(), _>(LearningError::Storage)
            })
            .is_err()
    );
    assert_eq!(releases.active(&auth).unwrap(), Some(first.clone()));
    releases
        .apply(&auth, |r| r.rollback(&auth, &first, decision(10, 224)))
        .unwrap();
    assert_eq!(
        inference.infer(&request).unwrap_err(),
        LocalInferenceError::Unavailable
    );
    let mut client = postgres::Client::connect(&connection, postgres::NoTls).unwrap();
    let mut bad = journal.clone();
    bad.scope = ContextScopeId::new("other").unwrap();
    let raw = serde_json::to_string(&bad).unwrap();
    use sha2::{Digest, Sha256};
    let checksum = format!("{:x}", Sha256::digest(raw.as_bytes()));
    client
        .execute(
            "UPDATE cg_cognitive_journals SET payload=$1,digest=$2",
            &[&raw, &checksum],
        )
        .unwrap();
    assert_eq!(releases.active(&auth), Err(LearningError::ScopeMismatch));
    client
        .execute("UPDATE cg_cognitive_journals SET digest='corrupt'", &[])
        .unwrap();
    assert_eq!(releases.active(&auth), Err(LearningError::Storage));
    let store =
        gateway_daemon::cognitive_store::CognitiveStore::connect(&connection, scope()).unwrap();
    let too_large: Result<(), gateway_daemon::cognitive_store::StoreError> =
        store.transact("test-size-limit", &String::new(), |value| {
            *value = "x".repeat(33_554_433);
            Ok(())
        });
    assert_eq!(
        too_large,
        Err(gateway_daemon::cognitive_store::StoreError::Limit)
    );
    assert_eq!(
        store
            .transact::<String, _, gateway_daemon::cognitive_store::StoreError>(
                "test-size-limit",
                &String::new(),
                |value| Ok(value.clone())
            )
            .unwrap(),
        ""
    );
    std::fs::remove_dir_all(root).unwrap();
}
