use super::*;
use gateway_application::{interpretation_adapters::*, local_inference::*};
use serde_json::{Value, json};

impl SemanticProposalHost for Host {
    fn validate_meaning(&self, task: &SemanticTaskIR, _: &CompletenessResult) -> bool {
        task.data().goal.description.as_str() == "Report the artifact properties"
    }
    fn validate_permissions(&self, task: &SemanticTaskIR, _: &CompletenessResult) -> bool {
        task.data().policy_refs.is_empty()
    }
}
fn policy() -> InterpretationPolicy {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/interpretation-adapters-v1/policy.json"
    ))
    .unwrap()
}
fn response(source: SignalSource) -> LocalInferenceProposal {
    LocalInferenceProposal {
        schema_version: "1.0".into(),
        kind: "proposal".into(),
        model_id: "fixture".into(),
        artifact_digest: format!("sha256:{}", "a".repeat(64)),
        proposal: json!({"source":source,"score":0.8,"embedding_space":if source == SignalSource::Embedding {Some("space-v1")} else {None},"task":serde_json::from_str::<Value>(&task().to_canonical_json().unwrap()).unwrap()}),
        metrics: json!({}),
        provenance: LocalInferenceProvenance {
            model_version: "v1".into(),
            runtime: "fixture".into(),
            runtime_version: "v1".into(),
            runtime_configuration: json!({}),
            prompt_version: "v1".into(),
            template_digest: format!("sha256:{}", "b".repeat(64)),
            system_digest: format!("sha256:{}", "c".repeat(64)),
            input_contract: "interpretation-gaps/1.0".into(),
            output_contract: "semantic-proposal/1.0".into(),
        },
    }
}
struct Model(
    Result<LocalInferenceProposal, LocalInferenceError>,
    Cell<usize>,
);
impl LocalInferencePort for Model {
    fn infer(
        &self,
        request: &LocalInferenceRequest,
    ) -> Result<LocalInferenceProposal, LocalInferenceError> {
        assert_eq!(request.output_contract, "semantic-proposal/1.0");
        assert!(request.output_schema["properties"]["task"].is_object());
        self.1.set(self.1.get() + 1);
        self.0.clone()
    }
}
fn run(
    response: Result<LocalInferenceProposal, LocalInferenceError>,
    source: SignalSource,
    host: &Host,
) -> AssistedInterpretation {
    let model = Model(response, Cell::new(0));
    let adapter = LocalSemanticAdapter(&model);
    let mut adapters = InterpretationAdapters::default();
    match source {
        SignalSource::Classifier => adapters.classifier = Some(&adapter),
        SignalSource::Embedding => adapters.embedding = Some(&adapter),
        SignalSource::Slm => adapters.slm = Some(&adapter),
    }
    let lookup = Lookup::default();
    let context = InterpretationContext::admit(data(vec![]), host, time(100)).unwrap();
    let v = validator(host, &lookup);
    let expected = v
        .validate(&context, &revision_id(), &[field("target", &[])], time(100))
        .unwrap();
    let result = v
        .assist(
            &context,
            &revision_id(),
            &[field("target", &[])],
            time(100),
            &policy(),
            &adapters,
        )
        .unwrap();
    assert_eq!(
        expected.canonical_bytes(),
        result.deterministic.canonical_bytes()
    );
    assert_eq!(model.1.get(), 1);
    result
}

#[test]
fn full_semantic_proposals_preserve_provenance_and_clarification() {
    for source in [
        SignalSource::Classifier,
        SignalSource::Embedding,
        SignalSource::Slm,
    ] {
        let result = run(Ok(response(source)), source, &Host::default());
        assert_eq!(result.attempts, vec![(source, AttemptStatus::Validated)]);
        let retained = result.proposal.unwrap();
        assert_eq!(retained.provenance.model_version, "v1");
        assert_eq!(retained.model_id, "fixture");
        assert!(!result.deterministic.is_complete());
    }
}

#[test]
fn failure_fixtures_keep_deterministic_fallback() {
    for error in [
        LocalInferenceError::Unavailable,
        LocalInferenceError::InvalidRequest,
        LocalInferenceError::InvalidProposal,
    ] {
        let status = if error == LocalInferenceError::Unavailable {
            AttemptStatus::Unavailable
        } else {
            AttemptStatus::Invalid
        };
        let result = run(Err(error), SignalSource::Slm, &Host::default());
        assert_eq!(result.attempts[0].1, status);
        assert!(result.proposal.is_none());
    }
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/interpretation-adapters-v1/failures.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let mut p = response(SignalSource::Slm);
        let mut value = serde_json::to_value(&p).unwrap();
        *value
            .pointer_mut(case["pointer"].as_str().unwrap())
            .unwrap() = case["value"].clone();
        p = serde_json::from_value(value).unwrap();
        let result = run(Ok(p), SignalSource::Slm, &Host::default());
        assert!(result.proposal.is_none(), "{}", case["name"]);
    }
    let mut p = response(SignalSource::Embedding);
    p.proposal["embedding_space"] = json!("different-space");
    assert!(
        run(Ok(p), SignalSource::Embedding, &Host::default())
            .proposal
            .is_none()
    );
    let host = Host {
        rejected: Some(id("missing-entity")),
        ..Host::default()
    };
    assert_eq!(
        run(Ok(response(SignalSource::Slm)), SignalSource::Slm, &host).attempts[0].1,
        AttemptStatus::Rejected
    );

    // A real, structurally valid policy reference is still not permission.
    let mut p = response(SignalSource::Slm);
    let reference = p.proposal["task"]["output_contract"]["schema"].clone();
    p.proposal["task"]["policy_refs"] = json!([reference]);
    assert!(SemanticTaskIR::from_json(&p.proposal["task"].to_string()).is_ok());
    assert_eq!(
        run(Ok(p), SignalSource::Slm, &Host::default()).attempts[0].1,
        AttemptStatus::Rejected
    );
}

#[test]
fn changed_basis_during_success_or_failure_never_returns_stale_proposals() {
    struct Changing<'a>(&'a Host, bool);
    impl InterpretationAdapter for Changing<'_> {
        fn propose(
            &self,
            _: &LocalInferenceRequest,
        ) -> Result<LocalInferenceProposal, LocalInferenceError> {
            self.0.changed.set(true);
            if self.1 {
                Ok(response(SignalSource::Slm))
            } else {
                Err(LocalInferenceError::Unavailable)
            }
        }
    }
    for success in [true, false] {
        let host = Host::default();
        let lookup = Lookup::default();
        let context = InterpretationContext::admit(data(vec![]), &host, time(100)).unwrap();
        let adapter = Changing(&host, success);
        let adapters = InterpretationAdapters {
            slm: Some(&adapter),
            ..Default::default()
        };
        assert!(
            validator(&host, &lookup)
                .assist(
                    &context,
                    &revision_id(),
                    &[field("target", &[])],
                    time(100),
                    &policy(),
                    &adapters
                )
                .is_err()
        );
    }
}

#[test]
fn deterministic_first_and_bounded_escalation() {
    let host = Host::default();
    let lookup = Lookup::default();
    let v = validator(&host, &lookup);
    let unavailable = Model(Err(LocalInferenceError::Unavailable), Cell::new(0));
    let available = Model(Ok(response(SignalSource::Slm)), Cell::new(0));
    let first = LocalSemanticAdapter(&unavailable);
    let last = LocalSemanticAdapter(&available);
    let adapters = InterpretationAdapters {
        classifier: Some(&first),
        embedding: Some(&first),
        slm: Some(&last),
    };
    let context = InterpretationContext::admit(
        data(vec![fact("f", "a", InterpretationTier::CurrentInput)]),
        &host,
        time(100),
    )
    .unwrap();
    let mut invalid = policy();
    invalid.version = "2.0".into();
    assert!(
        v.assist(
            &context,
            &revision_id(),
            &[field("target", &[])],
            time(100),
            &invalid,
            &adapters
        )
        .unwrap()
        .deterministic
        .is_complete()
    );
    assert_eq!(unavailable.1.get(), 0);
    let context = InterpretationContext::admit(data(vec![]), &host, time(100)).unwrap();
    assert!(
        v.assist(
            &context,
            &revision_id(),
            &[field("target", &[])],
            time(100),
            &invalid,
            &adapters
        )
        .is_err()
    );
    let result = v
        .assist(
            &context,
            &revision_id(),
            &[field("target", &[])],
            time(100),
            &policy(),
            &adapters,
        )
        .unwrap();
    assert_eq!(result.attempts.len(), 3);
    assert_eq!(unavailable.1.get(), 2);
    assert_eq!(available.1.get(), 1);
    assert!(
        v.assist(
            &context,
            &revision_id(),
            &[field("target", &[])],
            time(100),
            &policy(),
            &InterpretationAdapters::default()
        )
        .unwrap()
        .proposal
        .is_none()
    );
    assert!(
        v.assist(
            &context,
            &revision_id(),
            &[field("target", &[])],
            time(200),
            &policy(),
            &adapters
        )
        .is_err()
    );
}

#[test]
fn policy_is_versioned_model_scoped_and_ranking_only() {
    let p = policy();
    assert!(p.validate());
    assert_eq!(
        serde_json::from_str::<InterpretationPolicy>(&serde_json::to_string(&p).unwrap())
            .unwrap()
            .version,
        "1.0"
    );
    let mut duplicate = p.clone();
    duplicate.thresholds.push(p.thresholds[0].clone());
    assert!(!duplicate.validate());
    for (pointer, value) in [
        ("/version", json!("2.0")),
        ("/thresholds", json!([])),
        ("/thresholds/0/model_id", json!("")),
        ("/thresholds/0/model_version", json!("")),
        ("/thresholds/0/artifact_digest", json!("bad")),
        ("/thresholds/0/fixture_dataset", json!("")),
        ("/thresholds/0/minimum_rank", json!(1.1)),
        ("/thresholds/0/embedding_space", json!("wrong")),
        ("/thresholds/1/embedding_space", Value::Null),
    ] {
        let mut value_json = serde_json::to_value(&p).unwrap();
        *value_json.pointer_mut(pointer).unwrap() = value;
        assert!(
            !serde_json::from_value::<InterpretationPolicy>(value_json)
                .unwrap()
                .validate()
        );
    }
}
