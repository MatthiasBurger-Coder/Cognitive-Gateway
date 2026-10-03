use gateway_application::{
    experience_patterns::{PatternError, PatternLimits, VerifiedExecution, inspect_patterns},
    memory::{MemoryAction, MemoryApplication, MemoryChange, MemoryStore},
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
