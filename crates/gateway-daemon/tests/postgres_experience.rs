use gateway_application::{
    experience_patterns::{
        ExperienceIngestionPort, PatternError, PatternLimits, VerifiedExecution, inspect_patterns,
    },
    memory::{MemoryAction, MemoryApplication, MemoryChange, MemoryError, MemoryStore},
};
use gateway_daemon::{
    postgres_experience::{PostgresExperienceError, PostgresExperienceStore, RetentionPolicy},
    postgres_memory::PostgresMemoryStore,
};
use gateway_domain::{
    Confidence, ContentDigest, ContextScopeId, EvidenceId, FactId, FreshnessStatus, NonEmptyText,
    ProvenanceId, QualityMetadata, ReferenceId, SensitivityClass, TrustClass, Uncertainty,
    UnixTimestamp,
    learning::FingerprintSignal,
    memory::{CurationState, ExperienceRecord, MEMORY_SCHEMA_VERSION, MemoryPayload},
};

fn id(value: &str) -> ReferenceId {
    ReferenceId::new(value).unwrap()
}
fn scope(value: &str) -> ContextScopeId {
    ContextScopeId::new(value).unwrap()
}
fn at() -> UnixTimestamp {
    UnixTimestamp::new(20)
}

fn config() -> postgres::Config {
    let path = std::env::var("CG_POSTGRES_ENV_FILE").unwrap_or_else(|_| {
        format!(
            "{}/.config/cognitive-gateway/postgres.env",
            std::env::var("HOME").unwrap()
        )
    });
    let contents = std::fs::read_to_string(path).unwrap();
    let setting = |key: &str| {
        contents
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .map(str::to_owned)
    };
    let mut config = postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(setting("CG_POSTGRES_PORT").map_or(55432, |s| s.parse().unwrap()))
        .user("cognitive_gateway")
        .dbname("cognitive_gateway")
        .password(setting("CG_POSTGRES_PASSWORD").expect("database password"));
    config
}

fn record(project: &str, name: &str, outcome: &str) -> ExperienceRecord {
    ExperienceRecord {
        schema_version: MEMORY_SCHEMA_VERSION,
        id: id(name),
        scope: scope(project),
        provenance: ProvenanceId::new(format!("source-{name}")).unwrap(),
        source_snapshot: id(&format!("snapshot-{name}")),
        source_version: NonEmptyText::new("commit-1").unwrap(),
        source_digest: ContentDigest::new("a".repeat(64)).unwrap(),
        created_at: UnixTimestamp::new(10),
        observed_at: UnixTimestamp::new(9),
        valid_from: UnixTimestamp::new(10),
        expires_at: UnixTimestamp::new(100),
        max_age_seconds: 90,
        quality: QualityMetadata::new(
            TrustClass::DerivedAssessment,
            SensitivityClass::Internal,
            Confidence::score(0.9).unwrap(),
            FreshnessStatus::Fresh,
            Uncertainty::None,
        ),
        validation: Some(id("validation-1")),
        outcome: Some(NonEmptyText::new(outcome).unwrap()),
        label_basis: Some(id("label-1")),
        payload: Some(MemoryPayload::Reference(id(&format!("payload-{name}")))),
    }
}

fn validated<S: MemoryStore>(
    memory: &mut MemoryApplication<S>,
    project: &str,
    name: &str,
    outcome: &str,
) {
    memory
        .admit(record(project, name, outcome), id("admit"), at())
        .unwrap();
    memory
        .curate(
            &scope(project),
            &id(name),
            1,
            MemoryChange {
                action: MemoryAction::Validate,
                reason: id("validate"),
                at: at(),
                replacement: None,
                successor: None,
            },
        )
        .unwrap();
}

fn execution(name: &str) -> VerifiedExecution {
    VerifiedExecution {
        memory_id: id(name),
        runtime: id("linux"),
        trace: id(&format!("snapshot-{name}")),
        evaluation: id(&format!("evaluation-{name}")),
        validation: id("validation-1"),
        label_basis: id("label-1"),
        evidence: vec![EvidenceId::new(format!("evidence-{name}")).unwrap()],
        signals: vec![FingerprintSignal::Fact(FactId::new("fact-a").unwrap())],
        semantic_hint: None,
    }
}

#[test]
#[ignore = "requires the PostgreSQL Compose service; run scripts/test-postgres.sh"]
fn postgres_round_trip_revalidates_and_preserves_negative_evidence() {
    let config = config();
    let connection_string = format!(
        "host=127.0.0.1 port={} user=cognitive_gateway dbname=cognitive_gateway password={}",
        config.get_ports()[0],
        String::from_utf8_lossy(config.get_password().unwrap())
    );
    let alternate_memory = PostgresMemoryStore::connect(&connection_string).unwrap();
    let alternate_experience =
        PostgresExperienceStore::connect(&connection_string, RetentionPolicy::default()).unwrap();
    assert!(
        alternate_memory
            .list(&scope("cg22-empty-scope"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        alternate_experience
            .count(&scope("cg22-empty-scope"))
            .unwrap(),
        0
    );
    assert!(matches!(
        PostgresExperienceStore::connect_config(
            &config,
            RetentionPolicy {
                max_rows_per_scope: 0,
                ..RetentionPolicy::default()
            }
        ),
        Err(PostgresExperienceError::InvalidRetention)
    ));
    let project = format!(
        "cg22-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let project = scope(&project);
    let mut memory = MemoryApplication::new(PostgresMemoryStore::connect_config(&config).unwrap());
    for (name, outcome) in [("a", "SUCCESS"), ("b", "SUCCESS"), ("c", "FAILURE")] {
        validated(&mut memory, project.as_str(), name, outcome);
    }
    let store = PostgresExperienceStore::connect_config(
        &config,
        RetentionPolicy {
            max_rows_per_scope: 3,
            ..RetentionPolicy::default()
        },
    )
    .unwrap();
    for name in ["b", "a", "c"] {
        let receipt = store
            .record_verified(&memory, &project, at(), execution(name))
            .unwrap();
        assert!(receipt.inserted);
        assert_eq!(receipt.pruned_rows, 0);
    }
    assert!(
        !store
            .record_verified(&memory, &project, at(), execution("a"))
            .unwrap()
            .inserted
    );
    assert_eq!(store.count(&project).unwrap(), 3);
    assert!(matches!(
        store.list_verified(
            &project,
            PatternLimits {
                max_inputs: 1,
                ..PatternLimits::default()
            }
        ),
        Err(PatternError::TooManyInputs)
    ));
    assert!(matches!(
        store.list_verified(
            &project,
            PatternLimits {
                max_inputs: usize::MAX,
                ..PatternLimits::default()
            }
        ),
        Err(PatternError::InvalidLimits)
    ));
    let mut stale_store = PostgresMemoryStore::connect_config(&config).unwrap();
    let current_a = memory.store().get(&project, &id("a")).unwrap().unwrap();
    let decision_a = memory.store().decisions(&project, &id("a")).unwrap()[1].clone();
    assert!(matches!(
        stale_store.commit(Some(1), current_a, decision_a),
        Err(MemoryError::RevisionConflict)
    ));
    let mut tamper = config.connect(postgres::NoTls).unwrap();
    let original = tamper
        .query_one(
            "SELECT execution_json, eligibility_json, trace FROM cg_verified_executions WHERE scope = $1 AND memory_id = 'b'",
            &[&project.as_str()],
        )
        .unwrap();
    for (column, original_value) in [
        ("execution_json", original.get::<_, String>(0)),
        ("eligibility_json", original.get::<_, String>(1)),
        ("trace", original.get::<_, String>(2)),
    ] {
        let corrupted = if column == "trace" {
            "wrong-trace"
        } else {
            "{}"
        };
        tamper
            .execute(
                &format!("UPDATE cg_verified_executions SET {column} = $1 WHERE scope = $2 AND memory_id = 'b'"),
                &[&corrupted, &project.as_str()],
            )
            .unwrap();
        assert!(matches!(
            store.list_verified(&project, PatternLimits::default()),
            Err(PatternError::Storage)
        ));
        tamper
            .execute(
                &format!("UPDATE cg_verified_executions SET {column} = $1 WHERE scope = $2 AND memory_id = 'b'"),
                &[&original_value, &project.as_str()],
            )
            .unwrap();
    }
    validated(&mut memory, project.as_str(), "d", "SUCCESS");
    assert!(matches!(
        store.record_verified(&memory, &project, at(), execution("d")),
        Err(PostgresExperienceError::Capacity)
    ));
    assert_eq!(store.count(&project).unwrap(), 3);
    let mut changed = execution("a");
    changed.runtime = id("other-runtime");
    assert!(matches!(
        store.record_verified(&memory, &project, at(), changed),
        Err(PostgresExperienceError::Conflict)
    ));
    drop(store);
    drop(memory);

    let mut memory = MemoryApplication::new(PostgresMemoryStore::connect_config(&config).unwrap());
    let reopened =
        PostgresExperienceStore::connect_config(&config, RetentionPolicy::default()).unwrap();
    let report =
        inspect_patterns(&memory, &reopened, &project, at(), PatternLimits::default()).unwrap();
    assert_eq!(report.metrics.candidate_count, 1);
    assert_eq!(report.findings[0].observed_failure_count, 1);
    assert_eq!(
        report.findings[0]
            .candidate
            .as_ref()
            .unwrap()
            .experience()
            .len(),
        2
    );
    assert_eq!(reopened.count(&project).unwrap(), 3);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cg"))
        .args([
            "patterns",
            "--scope",
            project.as_str(),
            "--at",
            "20",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "CLI database inspection failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli_report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(cli_report["metrics"]["candidate_count"], 1);
    assert_eq!(cli_report["findings"][0]["observed_failure_count"], 1);
    let cli = env!("CARGO_BIN_EXE_cg");
    let current = std::process::Command::new(cli)
        .args(["patterns", "--scope", project.as_str(), "--json"])
        .output()
        .unwrap();
    assert!(current.status.success());
    let invalid_scope = std::process::Command::new(cli)
        .args(["patterns", "--scope", "bad scope", "--json"])
        .output()
        .unwrap();
    assert_eq!(invalid_scope.status.code(), Some(3));
    let invalid_time = std::process::Command::new(cli)
        .args([
            "patterns",
            "--scope",
            project.as_str(),
            "--at",
            "invalid",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(invalid_time.status.code(), Some(3));
    let test_config = std::env::temp_dir().join(format!("{project}-postgres.env"));
    for (contents, code) in [
        ("CG_POSTGRES_PORT=55432\n", "DATABASE_CONFIG"),
        (
            "CG_POSTGRES_PASSWORD=test-only\nCG_POSTGRES_PORT=invalid\n",
            "DATABASE_CONFIG",
        ),
        ("CG_POSTGRES_PASSWORD=wrong-password\n", "DATABASE_ERROR"),
    ] {
        std::fs::write(&test_config, contents).unwrap();
        let failed = std::process::Command::new(cli)
            .args(["patterns", "--scope", project.as_str(), "--json"])
            .env("CG_POSTGRES_ENV_FILE", &test_config)
            .output()
            .unwrap();
        assert!(!failed.status.success());
        let response: serde_json::Value = serde_json::from_slice(&failed.stdout).unwrap();
        assert_eq!(response["error"]["code"], code);
    }
    std::fs::remove_file(test_config).unwrap();

    memory
        .curate(
            &project,
            &id("a"),
            2,
            MemoryChange {
                action: MemoryAction::Invalidate,
                reason: id("revoke"),
                at: at(),
                replacement: None,
                successor: None,
            },
        )
        .unwrap();
    let report =
        inspect_patterns(&memory, &reopened, &project, at(), PatternLimits::default()).unwrap();
    assert_eq!(report.metrics.rejected_count, 1);
    assert_eq!(report.metrics.candidate_count, 0);
    assert_eq!(report.findings[0].failures.len(), 1);
    assert!(matches!(
        reopened.record_verified(&memory, &project, at(), execution("a")),
        Err(PostgresExperienceError::Ineligible)
            | Err(PostgresExperienceError::Pattern(PatternError::Memory(_)))
    ));

    let retaining = PostgresExperienceStore::connect_config(
        &config,
        RetentionPolicy {
            max_rows_per_scope: 3,
            max_age_seconds: 1,
        },
    )
    .unwrap();
    let receipt = retaining
        .record_verified(&memory, &project, UnixTimestamp::new(22), execution("d"))
        .unwrap();
    assert_eq!(receipt.pruned_rows, 3);
    assert_eq!(retaining.count(&project).unwrap(), 1);

    memory
        .admit(record(project.as_str(), "e", "SUCCESS"), id("admit"), at())
        .unwrap();
    memory
        .curate(
            &project,
            &id("e"),
            1,
            MemoryChange {
                action: MemoryAction::Reject,
                reason: id("reject"),
                at: at(),
                replacement: None,
                successor: None,
            },
        )
        .unwrap();
    assert_eq!(
        memory
            .store()
            .get(&project, &id("e"))
            .unwrap()
            .unwrap()
            .state,
        CurationState::Rejected
    );
    memory
        .curate(
            &project,
            &id("b"),
            2,
            MemoryChange {
                action: MemoryAction::Supersede,
                reason: id("supersede"),
                at: at(),
                replacement: None,
                successor: Some(id("d")),
            },
        )
        .unwrap();
    assert_eq!(
        memory
            .store()
            .get(&project, &id("b"))
            .unwrap()
            .unwrap()
            .state,
        CurationState::Superseded
    );

    memory
        .curate(
            &project,
            &id("a"),
            3,
            MemoryChange {
                action: MemoryAction::Forget,
                reason: id("forget"),
                at: at(),
                replacement: None,
                successor: None,
            },
        )
        .unwrap();
    drop(memory);
    let memory = MemoryApplication::new(PostgresMemoryStore::connect_config(&config).unwrap());
    let forgotten = memory.store().get(&project, &id("a")).unwrap().unwrap();
    assert_eq!(forgotten.state, CurationState::Forgotten);
    assert!(forgotten.record.payload.is_none());
    let decisions = memory.store().decisions(&project, &id("a")).unwrap();
    assert_eq!(decisions.len(), 4);
    assert_eq!(decisions[3].action, MemoryAction::Forget);

    let mut client = config.connect(postgres::NoTls).unwrap();
    let stored_record: String = client
        .query_one(
            "SELECT record_json FROM cg_memory_entries WHERE scope = $1 AND id = 'a'",
            &[&project.as_str()],
        )
        .unwrap()
        .get(0);
    assert!(!stored_record.contains("payload-a"));
    client
        .execute(
            "DELETE FROM cg_verified_executions WHERE scope = $1",
            &[&project.as_str()],
        )
        .unwrap();
    client
        .execute(
            "DELETE FROM cg_memory_decisions WHERE scope = $1",
            &[&project.as_str()],
        )
        .unwrap();
    client
        .execute(
            "DELETE FROM cg_memory_entries WHERE scope = $1",
            &[&project.as_str()],
        )
        .unwrap();
}
