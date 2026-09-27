use gateway_domain::evaluation::*;
use gateway_domain::{ContextScopeId, ReferenceId, SufficiencyFinding};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn id(value: &str) -> ReferenceId {
    ReferenceId::new(value).unwrap()
}
fn finding(value: &str) -> SufficiencyFinding {
    match value {
        "SUFFICIENT" => SufficiencyFinding::Sufficient,
        "PARTIAL" => SufficiencyFinding::Partial,
        "INSUFFICIENT" => SufficiencyFinding::Insufficient,
        "CONFLICTING" => SufficiencyFinding::Conflicting,
        "STALE" => SufficiencyFinding::Stale,
        "UNTRUSTED" => SufficiencyFinding::Untrusted,
        "CONTAMINATED" => SufficiencyFinding::Contaminated,
        "BUDGET_EXHAUSTED" => SufficiencyFinding::BudgetExhausted,
        _ => panic!("unknown finding"),
    }
}
fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap()
}
fn number(value: &Value, key: &str) -> u64 {
    value[key].as_u64().unwrap()
}
fn fixture() -> (EvaluationManifest, Vec<GoldenCase>, ReleasePolicy) {
    let data: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/epic02-v0.2/golden.json"
    ))
    .unwrap();
    let scope = ContextScopeId::new(string(&data, "scope")).unwrap();
    let manifest = EvaluationManifest {
        version: number(&data, "version") as u16,
        dataset: id(string(&data, "dataset")),
        scope: scope.clone(),
        source_digest: string(&data, "source_digest").into(),
        index_version: string(&data, "index_version").into(),
        embedding_version: string(&data, "embedding_version").into(),
        model_version: string(&data, "model_version").into(),
        estimator_version: string(&data, "estimator_version").into(),
        strategy_version: string(&data, "strategy_version").into(),
        evaluator_version: string(&data, "evaluator_version").into(),
        baseline: id(string(&data, "baseline")),
    };
    let cases = data["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| GoldenCase {
            id: id(string(value, "id")),
            scope: scope.clone(),
            relevant: value["relevant"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| id(v.as_str().unwrap()))
                .collect(),
            returned: value["returned"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| id(v.as_str().unwrap()))
                .collect(),
            expected_sufficiency: finding(string(value, "expected")),
            actual_sufficiency: finding(string(value, "actual")),
            expected_provenance: value["provenance"].as_bool().unwrap(),
            actual_provenance: true,
            expected_freshness: value["freshness"].as_bool().unwrap(),
            actual_freshness: true,
            expected_contamination_rejected: value["contamination_rejected"].as_bool().unwrap(),
            actual_contamination_rejected: true,
            token_budget: number(value, "token_budget"),
            tokens_used: number(value, "tokens_used"),
            justified_tokens: number(value, "justified_tokens"),
            latency_ms: number(value, "latency_ms"),
            cost_units: number(value, "cost_units"),
        })
        .collect();
    let scores = |key: &str| -> BTreeMap<&'static str, u32> {
        METRICS
            .into_iter()
            .map(|metric| (metric, data[key][metric].as_u64().unwrap() as u32))
            .collect()
    };
    let policy = ReleasePolicy {
        version: EVALUATION_VERSION,
        baseline: manifest.baseline.clone(),
        floors: scores("floors"),
        baseline_scores: scores("baseline_scores"),
        allowed_regression: number(&data, "allowed_regression") as u32,
    };
    (manifest, cases, policy)
}

#[test]
fn versioned_golden_replay_and_release_policy() {
    let (manifest, cases, policy) = fixture();
    let report = evaluate(manifest.clone(), &cases).unwrap();
    assert_eq!(report.cases, 9);
    assert_eq!(report.metrics["precision"].millionths(), Some(600_000));
    assert_eq!(report.metrics["recall"].millionths(), Some(500_000));
    assert_eq!(
        report.metrics["token_efficiency"].millionths(),
        Some(740_740)
    );
    assert_eq!(report.latency_ms, 27);
    assert_eq!(report.cost_units, 2);
    policy.qualify(&report).unwrap();
    if let Ok(path) = std::env::var("CG20_EVALUATION_OUTPUT") {
        let metrics: BTreeMap<_, _> = report
            .metrics
            .iter()
            .map(|(key, value)| {
                (
                    *key,
                    serde_json::json!({"numerator":value.numerator,
                "denominator":value.denominator,"millionths":value.millionths()}),
                )
            })
            .collect();
        let output = serde_json::json!({"version": EVALUATION_VERSION,
            "dataset":report.manifest.dataset.as_str(),"scope":report.manifest.scope.as_str(),
            "baseline":report.manifest.baseline.as_str(),"evaluator_version":report.manifest.evaluator_version,
            "source_digest":report.manifest.source_digest,"index_version":report.manifest.index_version,
            "embedding_version":report.manifest.embedding_version,"model_version":report.manifest.model_version,
            "estimator_version":report.manifest.estimator_version,"strategy_version":report.manifest.strategy_version,
            "cases":report.cases,"latency_ms":report.latency_ms,"cost_units":report.cost_units,
            "metrics":metrics,"qualification":"PASS"});
        std::fs::write(path, serde_json::to_vec_pretty(&output).unwrap()).unwrap();
    }
    let mut reordered = cases.clone();
    reordered.reverse();
    assert_eq!(evaluate(manifest, &reordered).unwrap(), report);
}

#[test]
fn invalid_inputs_and_regressions_fail_closed() {
    let (manifest, cases, policy) = fixture();
    assert_eq!(
        evaluate(manifest.clone(), &[]),
        Err(EvaluationError::EmptyDataset)
    );
    let mut invalid = manifest.clone();
    invalid.version = 99;
    assert_eq!(
        evaluate(invalid, &cases),
        Err(EvaluationError::InvalidManifest)
    );
    let mut invalid = manifest.clone();
    invalid.model_version.clear();
    assert_eq!(
        evaluate(invalid, &cases),
        Err(EvaluationError::InvalidManifest)
    );
    let mut invalid = manifest.clone();
    invalid.source_digest = "not-a-digest".into();
    assert_eq!(
        evaluate(invalid, &cases),
        Err(EvaluationError::InvalidManifest)
    );
    let mut duplicated = cases.clone();
    duplicated.push(cases[0].clone());
    assert_eq!(
        evaluate(manifest.clone(), &duplicated),
        Err(EvaluationError::DuplicateCase)
    );
    let mut bad = cases.clone();
    bad[0].scope = ContextScopeId::new("other-project").unwrap();
    assert_eq!(
        evaluate(manifest.clone(), &bad),
        Err(EvaluationError::ScopeMismatch)
    );
    let mut bad = cases.clone();
    bad[0].returned.push(id("lexical"));
    assert_eq!(
        evaluate(manifest.clone(), &bad),
        Err(EvaluationError::DuplicateResult)
    );
    let mut bad = cases.clone();
    bad[0].justified_tokens = bad[0].tokens_used + 1;
    assert_eq!(
        evaluate(manifest.clone(), &bad),
        Err(EvaluationError::InvalidManifest)
    );
    let mut bad = cases.clone();
    bad[1].latency_ms = u64::MAX;
    assert_eq!(
        evaluate(manifest.clone(), &bad),
        Err(EvaluationError::ArithmeticOverflow)
    );
    let report = evaluate(manifest.clone(), &cases).unwrap();
    let mut bad_policy = policy.clone();
    bad_policy.floors.remove("precision");
    assert_eq!(
        bad_policy.qualify(&report),
        Err(EvaluationError::MissingMetric)
    );
    bad_policy = policy.clone();
    bad_policy.floors.insert("precision", 700_000);
    assert_eq!(
        bad_policy.qualify(&report),
        Err(EvaluationError::BelowThreshold("precision"))
    );
    bad_policy = policy.clone();
    bad_policy.baseline_scores.insert("precision", 700_000);
    assert_eq!(
        bad_policy.qualify(&report),
        Err(EvaluationError::BaselineRegression("precision"))
    );
    bad_policy = policy.clone();
    bad_policy.version = 2;
    assert_eq!(
        bad_policy.qualify(&report),
        Err(EvaluationError::InvalidManifest)
    );
    let mut no_relevance = cases.clone();
    for case in &mut no_relevance {
        case.relevant = BTreeSet::new();
    }
    let report = evaluate(manifest, &no_relevance).unwrap();
    assert_eq!(report.metrics["precision"].millionths(), None);
    assert_eq!(policy.qualify(&report), Err(EvaluationError::MissingMetric));
    assert_eq!(
        Metric {
            numerator: 2,
            denominator: 1
        }
        .millionths(),
        None
    );
}
