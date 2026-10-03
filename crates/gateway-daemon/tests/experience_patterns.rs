use gateway_application::{
    experience_patterns::{PatternError, PatternLimits, VerifiedExecution, inspect_patterns},
    memory::{MemoryAction, MemoryApplication, MemoryChange},
};
use gateway_daemon::{
    experience_patterns::InMemoryVerifiedExecutionSource, memory::InMemoryMemoryStore,
};
use gateway_domain::{
    CapabilityId, Confidence, ContentDigest, ContextScopeId, EvidenceId, FactId, FreshnessStatus,
    NonEmptyText, ProvenanceId, QualityMetadata, ReferenceId, SensitivityClass, TrustClass,
    Uncertainty, UnixTimestamp,
    learning::FingerprintSignal,
    memory::{ExperienceRecord, MEMORY_SCHEMA_VERSION, MemoryPayload},
};

fn id(s: &str) -> ReferenceId {
    ReferenceId::new(s).unwrap()
}
fn scope(s: &str) -> ContextScopeId {
    ContextScopeId::new(s).unwrap()
}
fn at() -> UnixTimestamp {
    UnixTimestamp::new(20)
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

fn execution(name: &str, runtime: &str, facts: &[&str], hint: Option<&str>) -> VerifiedExecution {
    VerifiedExecution {
        memory_id: id(name),
        runtime: id(runtime),
        trace: id(&format!("snapshot-{name}")),
        evaluation: id(&format!("evaluation-{name}")),
        validation: id("validation-1"),
        label_basis: id("label-1"),
        evidence: vec![EvidenceId::new(format!("evidence-{name}")).unwrap()],
        signals: facts
            .iter()
            .map(|fact| FingerprintSignal::Fact(FactId::new(*fact).unwrap()))
            .collect(),
        semantic_hint: hint.map(str::to_owned),
    }
}

fn validated(
    app: &mut MemoryApplication<InMemoryMemoryStore>,
    project: &str,
    name: &str,
    outcome: &str,
) {
    app.admit(record(project, name, outcome), id("admit"), at())
        .unwrap();
    app.curate(
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

#[test]
fn repeated_conflicting_and_near_matches_are_reproducible() {
    let mut memory = MemoryApplication::new(InMemoryMemoryStore::default());
    let mut source = InMemoryVerifiedExecutionSource::default();
    for (name, outcome, facts, hint) in [
        ("a", "SUCCESS", vec!["fact-a"], Some("  Runtime 42  ")),
        ("b", "SUCCESS", vec!["fact-a"], None),
        ("c", "FAILURE", vec!["fact-a"], None),
        ("d", "FAILURE", vec!["fact-a", "fact-b"], Some("runtime 42")),
    ] {
        validated(&mut memory, "project-a", name, outcome);
        source.insert(scope("project-a"), execution(name, "linux", &facts, hint));
    }
    let report = inspect_patterns(
        &memory,
        &source,
        &scope("project-a"),
        at(),
        PatternLimits::default(),
    )
    .unwrap();
    assert_eq!(report.metrics.input_count, 4);
    assert_eq!(report.metrics.candidate_count, 1);
    assert_eq!(report.findings.len(), 2);
    let repeated = report
        .findings
        .iter()
        .find(|f| f.successes.len() == 2)
        .unwrap();
    assert_eq!(repeated.failures.len(), 1);
    assert_eq!(repeated.near_matches, vec![id("d")]);
    assert_eq!(repeated.semantic_nominations, vec![id("d")]);
    let candidate = repeated.candidate.as_ref().unwrap();
    assert_eq!(candidate.experience().len(), 2);
    assert_eq!(candidate.experience()[0].memory().id, id("a"));
    assert!(
        report
            .findings
            .iter()
            .find(|f| f.failures.iter().any(|x| x.basis.memory().id == id("d")))
            .unwrap()
            .candidate
            .is_none()
    );

    let mut reordered = InMemoryVerifiedExecutionSource::default();
    for name in ["d", "c", "b", "a"] {
        let (facts, hint) = if name == "d" {
            (vec!["fact-b", "fact-a"], Some("runtime 42"))
        } else if name == "a" {
            (vec!["fact-a"], Some("Runtime 42"))
        } else {
            (vec!["fact-a"], None)
        };
        reordered.insert(scope("project-a"), execution(name, "linux", &facts, hint));
    }
    assert_eq!(
        report,
        inspect_patterns(
            &memory,
            &reordered,
            &scope("project-a"),
            at(),
            PatternLimits::default()
        )
        .unwrap()
    );
}

#[test]
fn sparse_semantic_only_and_runtime_boundaries_cannot_create_candidates() {
    let mut memory = MemoryApplication::new(InMemoryMemoryStore::default());
    let mut source = InMemoryVerifiedExecutionSource::default();
    for (name, runtime, fact) in [
        ("a", "linux", "fact-a"),
        ("b", "windows", "fact-a"),
        ("c", "linux", "fact-b"),
    ] {
        validated(&mut memory, "project-a", name, "SUCCESS");
        source.insert(
            scope("project-a"),
            execution(name, runtime, &[fact], Some("same noisy text")),
        );
    }
    validated(&mut memory, "project-b", "other", "SUCCESS");
    source.insert(
        scope("project-b"),
        execution("other", "linux", &["fact-a"], None),
    );
    let report = inspect_patterns(
        &memory,
        &source,
        &scope("project-a"),
        at(),
        PatternLimits::default(),
    )
    .unwrap();
    assert_eq!(report.metrics.candidate_count, 0);
    assert_eq!(report.metrics.group_count, 3);
    assert!(report.findings.iter().all(|f| f.candidate.is_none()));
    assert!(
        report
            .findings
            .iter()
            .any(|f| !f.semantic_nominations.is_empty())
    );
    assert!(report.findings.iter().all(|f| f.near_matches.is_empty()));
    assert_eq!(
        report
            .findings
            .iter()
            .map(|f| f.cross_runtime_matches.len())
            .sum::<usize>(),
        2
    );
}

#[test]
fn unvalidated_revoked_and_mismatched_executions_do_not_enter_pipeline() {
    let mut memory = MemoryApplication::new(InMemoryMemoryStore::default());
    let mut source = InMemoryVerifiedExecutionSource::default();
    memory
        .admit(record("project-a", "pending", "SUCCESS"), id("admit"), at())
        .unwrap();
    source.insert(
        scope("project-a"),
        execution("pending", "linux", &["fact-a"], None),
    );
    let report = inspect_patterns(
        &memory,
        &source,
        &scope("project-a"),
        at(),
        PatternLimits::default(),
    )
    .unwrap();
    assert_eq!(report.metrics.rejected_count, 1);
    assert!(report.findings.is_empty());

    validated(&mut memory, "project-a", "valid", "SUCCESS");
    let mut bad = execution("valid", "linux", &["fact-a"], None);
    bad.validation = id("wrong-validation");
    source.insert(scope("project-a"), bad);
    assert_eq!(
        inspect_patterns(
            &memory,
            &source,
            &scope("project-a"),
            at(),
            PatternLimits::default()
        ),
        Err(PatternError::InvalidExecution)
    );

    let mut valid_source = InMemoryVerifiedExecutionSource::default();
    valid_source.insert(
        scope("project-a"),
        execution("valid", "linux", &["fact-a"], None),
    );
    memory
        .curate(
            &scope("project-a"),
            &id("valid"),
            2,
            MemoryChange {
                action: MemoryAction::Invalidate,
                reason: id("invalidated"),
                at: at(),
                replacement: None,
                successor: None,
            },
        )
        .unwrap();
    let report = inspect_patterns(
        &memory,
        &valid_source,
        &scope("project-a"),
        at(),
        PatternLimits::default(),
    )
    .unwrap();
    assert_eq!(report.metrics.rejected_count, 1);
    assert!(report.findings.is_empty());
}

#[test]
fn limits_bound_input_groups_and_sampling() {
    let mut memory = MemoryApplication::new(InMemoryMemoryStore::default());
    let mut source = InMemoryVerifiedExecutionSource::default();
    for name in ["a", "b", "c"] {
        validated(&mut memory, "project-a", name, "SUCCESS");
        source.insert(
            scope("project-a"),
            execution(name, "linux", &["fact-a"], None),
        );
    }
    let limits = PatternLimits {
        max_inputs: 2,
        ..PatternLimits::default()
    };
    assert_eq!(
        inspect_patterns(&memory, &source, &scope("project-a"), at(), limits),
        Err(PatternError::TooManyInputs)
    );
    let limits = PatternLimits {
        max_per_group: 2,
        ..PatternLimits::default()
    };
    let report = inspect_patterns(&memory, &source, &scope("project-a"), at(), limits).unwrap();
    assert_eq!(report.metrics.sampled_out_count, 1);
    assert_eq!(report.metrics.retained_count, 2);
    assert_eq!(report.metrics.candidate_count, 1);
    let limits = PatternLimits {
        min_successes: 1,
        ..PatternLimits::default()
    };
    assert_eq!(
        inspect_patterns(&memory, &source, &scope("project-a"), at(), limits),
        Err(PatternError::InvalidLimits)
    );
}

#[test]
fn bounded_sampling_keeps_negative_evidence_and_counts_full_group() {
    let mut memory = MemoryApplication::new(InMemoryMemoryStore::default());
    let mut source = InMemoryVerifiedExecutionSource::default();
    for (name, outcome) in [("a", "SUCCESS"), ("b", "SUCCESS"), ("c", "FAILURE")] {
        validated(&mut memory, "project-a", name, outcome);
        source.insert(
            scope("project-a"),
            execution(name, "linux", &["fact-a"], None),
        );
    }
    let report = inspect_patterns(
        &memory,
        &source,
        &scope("project-a"),
        at(),
        PatternLimits {
            max_per_group: 2,
            ..PatternLimits::default()
        },
    )
    .unwrap();
    let finding = &report.findings[0];
    assert_eq!(
        (
            finding.observed_success_count,
            finding.observed_failure_count
        ),
        (2, 1)
    );
    assert_eq!((finding.successes.len(), finding.failures.len()), (1, 1));
    assert!(finding.candidate.is_none());
    assert_eq!(report.metrics.sampled_out_count, 1);
}

#[test]
fn capability_signal_is_structural_and_duplicate_signals_fail() {
    let mut memory = MemoryApplication::new(InMemoryMemoryStore::default());
    validated(&mut memory, "project-a", "a", "SUCCESS");
    let mut source = InMemoryVerifiedExecutionSource::default();
    let mut value = execution("a", "linux", &[], None);
    value.signals = vec![FingerprintSignal::Capability(
        CapabilityId::new("capability-a").unwrap(),
    )];
    source.insert(scope("project-a"), value.clone());
    assert_eq!(
        inspect_patterns(
            &memory,
            &source,
            &scope("project-a"),
            at(),
            PatternLimits::default()
        )
        .unwrap()
        .findings
        .len(),
        1
    );
    let mut duplicate = InMemoryVerifiedExecutionSource::default();
    value.signals.push(value.signals[0].clone());
    duplicate.insert(scope("project-a"), value);
    assert_eq!(
        inspect_patterns(
            &memory,
            &duplicate,
            &scope("project-a"),
            at(),
            PatternLimits::default()
        ),
        Err(PatternError::InvalidFingerprint)
    );
}

#[test]
fn cli_inspects_generated_report_without_mutation() {
    let memory = MemoryApplication::new(InMemoryMemoryStore::default());
    let source = InMemoryVerifiedExecutionSource::default();
    let report = inspect_patterns(
        &memory,
        &source,
        &scope("project-a"),
        at(),
        PatternLimits::default(),
    )
    .unwrap();
    let input = serde_json::to_string(&report).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cg"))
        .args(["patterns", "--report", &input, "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["metrics"]["input_count"], 0);
    assert_eq!(parsed["scope"], "project-a");
}
