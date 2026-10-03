use gateway_domain::{
    ProvenanceId, ReferenceId,
    learning::{LearnedProcedure, ProcedureLifecycle, ProcedureState, ProcedureTransition},
    procedure_evaluation::*,
};

fn procedure() -> LearnedProcedure {
    LearnedProcedure::from_json(include_str!(
        "../../../tests/fixtures/procedure-evaluation-v1/procedure.json"
    ))
    .unwrap()
}
fn dataset() -> EvaluationDataset {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/procedure-evaluation-v1/historical.json"
    ))
    .unwrap()
}
fn full() -> EvaluationBundle {
    let p = procedure();
    let mut d = dataset();
    d.cases.extend(counterfactuals(&p, &d.cases[0]).unwrap());
    EvaluationBundle::evaluate(&p, d, ReferenceId::new("runtime-1").unwrap()).unwrap()
}
#[test]
fn golden_replay_and_counterfactuals_prove_every_required_outcome() {
    let bundle = full();
    bundle.validate().unwrap();
    assert!(bundle.report.passed);
    assert!(bundle.report.missing_coverage.is_empty());
    for result in &bundle.report.results {
        assert!(result.passed);
        assert!(!result.critical_false_positive);
        if !matches!(
            result.outcome,
            ReplayOutcome::Success
                | ReplayOutcome::ExecutionFailed
                | ReplayOutcome::VerificationFailed
        ) {
            assert!(!result.activated);
            assert_eq!(result.completed_steps, 0);
        }
    }
    let mut reordered = bundle.dataset.clone();
    reordered.cases.reverse();
    assert_eq!(
        bundle,
        EvaluationBundle::evaluate(
            &procedure(),
            reordered,
            ReferenceId::new("runtime-1").unwrap()
        )
        .unwrap()
    );
    if let Ok(path) = std::env::var("CG23_EVALUATION_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&bundle).unwrap()).unwrap();
    }
}
#[test]
fn failed_incomplete_or_altered_evidence_cannot_advance() {
    let p = procedure();
    let bundle = full();
    let event = |decision| {
        ProcedureTransition::new(
            &p,
            ProcedureState::Draft,
            ProcedureState::Evaluated,
            decision,
            ProvenanceId::new("reviewer").unwrap(),
            10,
        )
        .unwrap()
    };
    let mut lifecycle = ProcedureLifecycle::new(&p);
    assert!(
        lifecycle
            .apply(event(ReferenceId::new("unproven").unwrap()))
            .is_err()
    );
    assert!(
        lifecycle
            .apply_evaluated(event(ReferenceId::new("unbound").unwrap()), &bundle)
            .is_err()
    );
    let incomplete =
        EvaluationBundle::evaluate(&p, dataset(), ReferenceId::new("runtime-1").unwrap()).unwrap();
    assert!(!incomplete.report.passed);
    assert!(incomplete.proves(&p).is_err());
    let mut changed = bundle.clone();
    changed.report.passed = false;
    assert!(changed.validate().is_err());
    let mut changed = bundle.clone();
    changed.dataset.cases[0].snapshot.at += 1;
    assert!(changed.validate().is_err());
    let mut changed = bundle.clone();
    changed.report.manifest.evaluator_version += 1;
    assert!(changed.validate().is_err());
    let mut changed = bundle.clone();
    changed.procedure = LearnedProcedure::new(
        p.id().clone(),
        2,
        &gateway_domain::learning::PatternCandidate::new(
            p.source_candidate().clone(),
            p.fingerprint().clone(),
            p.experience().to_vec(),
        )
        .unwrap(),
        p.steps().to_vec(),
        p.required_observations().to_vec(),
        p.required_evidence().to_vec(),
        p.verification_evidence().to_vec(),
        p.fallback(),
    )
    .unwrap();
    assert!(changed.validate().is_err());
    assert!(bundle.proves(&changed.procedure).is_err());
    lifecycle
        .apply_evaluated(
            event(ReferenceId::new(bundle.digest.as_str()).unwrap()),
            &bundle,
        )
        .unwrap();
    assert_eq!(lifecycle.state(), ProcedureState::Evaluated);
    let approval = ProcedureTransition::new(
        &p,
        ProcedureState::Evaluated,
        ProcedureState::Approved,
        ReferenceId::new("approval").unwrap(),
        ProvenanceId::new("reviewer").unwrap(),
        11,
    )
    .unwrap();
    lifecycle.apply(approval).unwrap();
}
#[test]
fn false_positive_activation_is_critical_and_wrong_refusal_does_not_pass() {
    let mut d = full().dataset;
    let positive = d
        .cases
        .iter()
        .find(|c| c.kind == CaseKind::HistoricalSuccess)
        .unwrap()
        .clone();
    d.cases.push(ReplayCase::new(
        ReferenceId::new("unsafe-positive").unwrap(),
        CaseKind::HistoricalFailure,
        ReplayOutcome::NotApplicable,
        positive.snapshot.clone(),
    ));
    let b = EvaluationBundle::evaluate(&procedure(), d, ReferenceId::new("runtime-1").unwrap())
        .unwrap();
    assert!(!b.report.passed);
    assert!(b.report.results.iter().any(|r| r.critical_false_positive));
    let mut d = full().dataset;
    let case = d
        .cases
        .iter_mut()
        .find(|c| c.kind == CaseKind::PolicyDenial)
        .unwrap();
    case.snapshot.evidence.clear();
    *case = ReplayCase::new(
        case.id.clone(),
        case.kind,
        case.expected,
        case.snapshot.clone(),
    );
    assert!(
        !EvaluationBundle::evaluate(&procedure(), d, ReferenceId::new("runtime-1").unwrap())
            .unwrap()
            .report
            .passed
    );
}
#[test]
fn invalid_datasets_and_baselines_fail_closed() {
    let p = procedure();
    let mut d = dataset();
    assert!(counterfactuals(&p, &d.cases[1]).is_err());
    d.cases[0].snapshot.evidence.clear();
    assert!(d.validate().is_err());
    assert!(counterfactuals(&p, &d.cases[0]).is_err());
    let mut d = dataset();
    d.cases.push(d.cases[0].clone());
    assert!(d.validate().is_err());
    let mut d = dataset();
    d.schema_version = 2;
    assert!(d.validate().is_err());
    let mut d = dataset();
    d.version = 0;
    assert!(d.validate().is_err());
    let mut d = dataset();
    d.cases.clear();
    assert!(d.validate().is_err());
    let mut d = dataset();
    d.cases[0].expected = ReplayOutcome::ProcessDenied;
    assert!(d.validate().is_err());
}

#[test]
fn later_step_denials_and_every_verification_status_fail_closed() {
    let original = procedure();
    let candidate = gateway_domain::learning::PatternCandidate::new(
        original.source_candidate().clone(),
        original.fingerprint().clone(),
        original.experience().to_vec(),
    )
    .unwrap();
    let p = LearnedProcedure::new(
        original.id().clone(),
        2,
        &candidate,
        vec![original.steps()[0].clone(); 2],
        original.required_observations().to_vec(),
        original.required_evidence().to_vec(),
        original.verification_evidence().to_vec(),
        gateway_domain::learning::FallbackBehavior::ReturnToPlanner,
    )
    .unwrap();
    let mut d = dataset();
    for case in &mut d.cases {
        let mut s = case.snapshot.clone();
        s.steps.push(s.steps[0].clone());
        *case = ReplayCase::new(case.id.clone(), case.kind, case.expected, s);
    }
    let positives = d.cases[0].clone();
    d.cases.extend(counterfactuals(&p, &positives).unwrap());
    let b =
        EvaluationBundle::evaluate(&p, d.clone(), ReferenceId::new("runtime-1").unwrap()).unwrap();
    assert!(b.report.passed);
    for result in b
        .report
        .results
        .iter()
        .filter(|r| r.id.as_str().ends_with("process-1") || r.id.as_str().ends_with("policy-1"))
    {
        assert!(!result.activated);
        assert_eq!(result.completed_steps, 0);
        assert_eq!(
            result.fallback,
            Some(gateway_domain::learning::FallbackBehavior::ReturnToPlanner)
        );
    }
    for status in [
        InputStatus::Missing,
        InputStatus::Stale,
        InputStatus::Conflicting,
        InputStatus::Failed,
    ] {
        let mut s = positives.snapshot.clone();
        s.verification
            .insert(p.verification_evidence()[0].clone(), status);
        let case = ReplayCase::new(
            ReferenceId::new("verification-case").unwrap(),
            CaseKind::VerificationFailure,
            ReplayOutcome::VerificationFailed,
            s,
        );
        let mut data = d.clone();
        data.cases.push(case);
        let b =
            EvaluationBundle::evaluate(&p, data, ReferenceId::new("runtime-1").unwrap()).unwrap();
        let result = b
            .report
            .results
            .iter()
            .find(|r| r.id.as_str() == "verification-case")
            .unwrap();
        assert!(result.passed && result.activated);
        assert_eq!(result.completed_steps, 2);
    }
    let encoded = serde_json::to_string(&b).unwrap();
    let decoded: EvaluationBundle = serde_json::from_str(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, b);
    d.cases[0].snapshot.steps.pop();
    let case = d.cases[0].clone();
    d.cases[0] = ReplayCase::new(case.id, case.kind, case.expected, case.snapshot);
    assert!(
        !EvaluationBundle::evaluate(&p, d, ReferenceId::new("runtime-1").unwrap())
            .unwrap()
            .report
            .passed
    );
}

#[test]
fn conflicting_operating_modes_cannot_match_or_validate() {
    use gateway_domain::{
        OperatingMode,
        learning::{FingerprintSignal, SituationFingerprint},
    };
    let p = procedure();
    let mut signals = p.fingerprint().signals().to_vec();
    signals.push(FingerprintSignal::OperatingMode(OperatingMode::Hardening));
    assert!(SituationFingerprint::new(p.fingerprint().scope().clone(), signals).is_err());
    let mut d = full().dataset;
    let mut s = dataset().cases[0].snapshot.clone();
    s.signals
        .insert(FingerprintSignal::OperatingMode(OperatingMode::Hardening));
    d.cases.push(ReplayCase::new(
        ReferenceId::new("conflicting-mode").unwrap(),
        CaseKind::NearMatch,
        ReplayOutcome::NotApplicable,
        s,
    ));
    assert!(
        EvaluationBundle::evaluate(&p, d, ReferenceId::new("runtime-1").unwrap())
            .unwrap()
            .report
            .passed
    );
}
