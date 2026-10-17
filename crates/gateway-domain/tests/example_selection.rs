#[path = "../../../tests/fixtures/example-selection-v1/support.rs"]
mod support;
use gateway_domain::memory::CurationState;
use gateway_domain::*;
use std::collections::BTreeSet;
use support::*;

#[test]
fn requirement_activation_bindings_and_policy_validation() {
    let r = requirements(context_requirement());
    let t = task();
    assert_eq!(
        ExampleRequirement::from_context(&r, &t, &id("absent"), policy()).unwrap(),
        None
    );
    let mut c = context_requirement();
    c.necessity = ContextNecessity::Conditional {
        condition: id("condition"),
        active: false,
    };
    assert!(
        ExampleRequirement::from_context(&requirements(c), &t, &id("examples"), policy())
            .unwrap()
            .is_none()
    );
    let mut data = t.data().clone();
    data.goal.description.push_str("changed");
    let changed = SemanticTaskIR::new(data).unwrap();
    assert_eq!(
        ExampleRequirement::from_context(&r, &changed, &id("examples"), policy()),
        Err(ContextPlanningError::BindingMismatch)
    );
    for mode in 0..5 {
        let mut p = policy();
        match mode {
            0 => p.sources.clear(),
            1 => p.minimum_relevance = 1001,
            2 => {
                let mut c = context_requirement();
                c.permitted_sources = vec![ContextSourceClass::Repository];
                assert!(
                    ExampleRequirement::from_context(&requirements(c), &t, &id("examples"), p)
                        .is_err()
                );
                continue;
            }
            3 => {
                let mut v = pin(PolicyId::new("reuse").unwrap());
                v.scope = ContextScopeId::new("other").unwrap();
                p.reuse.insert(v.scope.clone(), v);
            }
            _ => p.architecture.as_mut().unwrap().contract_version = SchemaVersion::V2,
        }
        assert!(ExampleRequirement::from_context(&r, &t, &id("examples"), p).is_err());
    }
    assert_eq!(
        ExampleSourceClass::Curated.context_class(),
        ContextSourceClass::Examples
    );
    assert_eq!(
        ExampleSourceClass::PromotedProcedure.context_class(),
        ContextSourceClass::Examples
    );
    assert_eq!(
        ExampleSourceClass::Retrieval.context_class(),
        ContextSourceClass::Rag
    );
    assert_eq!(
        ExampleSourceClass::GovernedExperience.context_class(),
        ContextSourceClass::Memory
    );
    assert_eq!(requirement().requirements(), &r.binding());
    assert_eq!(requirement().context().id, id("examples"));
    assert_eq!(requirement().policy(), &policy());
    assert_eq!(requirement().to_value()["schema_version"], "1.0");
}
#[test]
fn eligibility_denies_invalid_unvalidated_unsuccessful_stale_untrusted_and_sensitive_data() {
    let r = requirement();
    let base = candidate("a");
    assert_eq!(example_rejection(&r, &base, &BTreeSet::new()), None);
    for mode in 0..21 {
        let mut c = base.clone();
        let reason = match mode {
            0 => {
                c.pin.contract_version = SchemaVersion::V2;
                ExampleReason::InvalidBinding
            }
            1 => {
                c.pin.digest = context_content_digest(b"tamper");
                ExampleReason::InvalidBinding
            }
            2 => {
                c.input.scope = ContextScopeId::new("other").unwrap();
                seal(&mut c);
                ExampleReason::InvalidBinding
            }
            3 => {
                c.input.snapshot = context_content_digest(b"tamper");
                seal(&mut c);
                ExampleReason::InvalidBinding
            }
            4 => {
                c.validation = CurationState::Pending;
                ExampleReason::Unvalidated
            }
            5 => {
                c.validation_ref = None;
                ExampleReason::Unvalidated
            }
            6 => {
                c.validated_digest = Some(context_content_digest(b"old"));
                ExampleReason::Unvalidated
            }
            7 => {
                c.successful = false;
                seal(&mut c);
                ExampleReason::Unsuccessful
            }
            8 => {
                c.compatibility.task = SemanticTaskType::Create;
                seal(&mut c);
                ExampleReason::Incompatible
            }
            9 => {
                c.compatibility.target = TargetKind::Service;
                seal(&mut c);
                ExampleReason::Incompatible
            }
            10 => {
                c.compatibility.output.revision = id("old");
                seal(&mut c);
                ExampleReason::Incompatible
            }
            11 => {
                c.compatibility.architecture = None;
                seal(&mut c);
                ExampleReason::Incompatible
            }
            12 => {
                c.input.quality = QualityMetadata::new(
                    TrustClass::RetrievedContent,
                    SensitivityClass::Normal,
                    Confidence::Unknown,
                    FreshnessStatus::Stale,
                    Uncertainty::None,
                );
                seal(&mut c);
                ExampleReason::Stale
            }
            13 => {
                c.input.quality = QualityMetadata::new(
                    TrustClass::CanonicalReference,
                    SensitivityClass::Normal,
                    Confidence::Unknown,
                    FreshnessStatus::Fresh,
                    Uncertainty::None,
                );
                seal(&mut c);
                ExampleReason::Untrusted
            }
            14 => {
                c.input.quality = QualityMetadata::new(
                    TrustClass::RetrievedContent,
                    SensitivityClass::Normal,
                    Confidence::Unknown,
                    FreshnessStatus::Fresh,
                    Uncertainty::Probabilistic,
                );
                seal(&mut c);
                ExampleReason::Untrusted
            }
            15 => {
                c.input.quality = c.input.quality.with_conflict(ConflictStatus::Unresolved);
                seal(&mut c);
                ExampleReason::Untrusted
            }
            16 => {
                c.expected_output.as_mut().unwrap().quality = QualityMetadata::new(
                    TrustClass::RetrievedContent,
                    SensitivityClass::Secret,
                    Confidence::Unknown,
                    FreshnessStatus::Fresh,
                    Uncertainty::None,
                );
                seal(&mut c);
                ExampleReason::SensitivityRejected
            }
            17 => {
                c.compatibility.tags.clear();
                seal(&mut c);
                ExampleReason::Irrelevant
            }
            18 => {
                c.tokens.count = TokenCount::Unknown {
                    reason: text("unavailable"),
                };
                ExampleReason::InvalidTokens
            }
            19 => {
                c.tokens = estimate(0);
                ExampleReason::InvalidTokens
            }
            _ => {
                c.pin.contract = id("authority");
                ExampleReason::InvalidBinding
            }
        };
        assert_eq!(
            example_rejection(&r, &c, &BTreeSet::new()),
            Some(reason),
            "mode {mode}"
        );
    }
    for state in [
        CurationState::Rejected,
        CurationState::Invalidated,
        CurationState::Superseded,
        CurationState::Forgotten,
    ] {
        let mut c = base.clone();
        c.validation = state;
        assert_eq!(
            example_rejection(&r, &c, &BTreeSet::new()),
            Some(ExampleReason::Unvalidated)
        );
    }
    let mut p = policy();
    p.sources = BTreeSet::from([ExampleSourceClass::Curated]);
    let r = ExampleRequirement::from_context(
        &requirements(context_requirement()),
        &task(),
        &id("examples"),
        p,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        example_rejection(&r, &base, &BTreeSet::new()),
        Some(ExampleReason::NotPermitted)
    );
}
#[test]
fn cross_scope_requires_pinned_grant_and_quarantine_covers_original_fragments() {
    let mut c = candidate("a");
    let other = ContextScopeId::new("other").unwrap();
    c.pin.scope = other.clone();
    c.input.scope = other.clone();
    c.expected_output.as_mut().unwrap().scope = other.clone();
    seal(&mut c);
    assert_eq!(
        example_rejection(&requirement(), &c, &BTreeSet::new()),
        Some(ExampleReason::CrossScope)
    );
    let mut p = policy();
    p.reuse.insert(other, pin(PolicyId::new("reuse").unwrap()));
    let r = ExampleRequirement::from_context(
        &requirements(context_requirement()),
        &task(),
        &id("examples"),
        p,
    )
    .unwrap()
    .unwrap();
    assert_eq!(example_rejection(&r, &c, &BTreeSet::new()), None);
    for excluded in [
        c.pin.id.clone(),
        c.input.id.clone(),
        c.expected_output.as_ref().unwrap().id.clone(),
    ] {
        assert_eq!(
            example_rejection(&r, &c, &BTreeSet::from([excluded])),
            Some(ExampleReason::Contaminated)
        );
    }
}
#[test]
fn evidence_sensitivity_and_relevance_constraints_apply_to_both_halves() {
    let mut ctx = context_requirement();
    ctx.information.requirements = InformationRequirements::new(
        FreshnessRequirement::Fresh,
        Some(SensitivityClass::Internal),
        vec![EvidenceId::new("proof").unwrap()],
        vec![ProvenanceId::new("lineage-a-input").unwrap()],
    )
    .unwrap();
    let r = ExampleRequirement::from_context(
        &requirements(ctx.clone()),
        &task(),
        &id("examples"),
        policy(),
    )
    .unwrap()
    .unwrap();
    let mut c = candidate("a");
    assert_eq!(
        example_rejection(&r, &c, &BTreeSet::new()),
        Some(ExampleReason::SensitivityRejected)
    );
    for f in [&mut c.input, c.expected_output.as_mut().unwrap()] {
        f.quality = QualityMetadata::new(
            TrustClass::RetrievedContent,
            SensitivityClass::Internal,
            Confidence::Unknown,
            FreshnessStatus::Fresh,
            Uncertainty::None,
        );
    }
    seal(&mut c);
    assert_eq!(
        example_rejection(&r, &c, &BTreeSet::new()),
        Some(ExampleReason::EvidenceMissing)
    );
    c.input.evidence.insert(EvidenceId::new("proof").unwrap());
    seal(&mut c);
    assert_eq!(example_rejection(&r, &c, &BTreeSet::new()), None);
    ctx.stop = StopCondition::EvidenceSatisfied(2.try_into().unwrap());
    let r =
        ExampleRequirement::from_context(&requirements(ctx), &task(), &id("examples"), policy())
            .unwrap()
            .unwrap();
    assert_eq!(
        example_rejection(&r, &c, &BTreeSet::new()),
        Some(ExampleReason::EvidenceMissing)
    );
    let mut p = policy();
    p.tags.clear();
    let r = ExampleRequirement::from_context(
        &requirements(context_requirement()),
        &task(),
        &id("examples"),
        p,
    )
    .unwrap()
    .unwrap();
    assert_eq!(example_relevance(&r, &c), 1000);
}
#[test]
fn duplicate_detection_and_validated_bounded_set_are_lossless() {
    let a = candidate("a");
    let mut b = candidate("b");
    assert_eq!(example_duplicate(&a, &b), Some(ExampleReason::Duplicate));
    change_pair(
        &mut b,
        "Inspect this Rust module!",
        "Return a typed report with tests.",
    );
    assert_eq!(
        example_duplicate(&a, &b),
        Some(ExampleReason::NearDuplicate)
    );
    change_pair(
        &mut b,
        "Compare latency percentiles",
        "Report measured histogram",
    );
    assert_eq!(example_duplicate(&a, &b), None);
    let s = set(a.clone());
    let v = s.to_value();
    assert_eq!(v["selected"][0], a.to_value());
    assert_eq!(v["usage"]["tokens"], 40);
    assert_eq!(s.usage().results, 1);
    assert_eq!(
        s.binding().digest,
        context_content_digest(v.to_string().as_bytes())
    );
    let src = s.source().unwrap();
    assert_eq!(src.tokens, 40);
    assert_eq!(src.quality, quality());
    assert!(matches!(
        src.present,
        Some(ContextSatisfaction::Examples(_))
    ));
    assert_eq!(s.requirement(), &requirement());
    assert_eq!(s.selected(), std::slice::from_ref(&a));
    assert_eq!(s.decisions().len(), 1);
    assert!(ExampleSet::new(id("s"), requirement(), vec![a.clone()], vec![]).is_err());
    assert!(
        ExampleSet::new(
            id("s"),
            requirement(),
            vec![],
            vec![decision(&a, ExampleReason::Selected)]
        )
        .is_err()
    );
    let mut bad = a.clone();
    bad.validation = CurationState::Pending;
    assert!(
        ExampleSet::new(
            id("s"),
            requirement(),
            vec![bad.clone()],
            vec![decision(&bad, ExampleReason::Selected)]
        )
        .is_err()
    );
    let mut dup = a.clone();
    dup.pin.id = id("b");
    assert!(
        ExampleSet::new(
            id("s"),
            requirement(),
            vec![a.clone(), dup.clone()],
            vec![
                decision(&a, ExampleReason::Selected),
                decision(&dup, ExampleReason::Selected)
            ]
        )
        .is_err()
    );
    let empty = ExampleSet::new(id("s"), requirement(), vec![], vec![]).unwrap();
    assert!(empty.source().is_none());
    let mut costly = a.clone();
    costly.tokens = estimate(201);
    assert_eq!(
        ExampleSet::new(
            id("s"),
            requirement(),
            vec![costly.clone()],
            vec![decision(&costly, ExampleReason::Selected)]
        ),
        Err(ContextPlanningError::InfeasibleBudget)
    );
    let mut ctx = context_requirement();
    ctx.budget.results = ResultBudget(1.try_into().unwrap());
    let r =
        ExampleRequirement::from_context(&requirements(ctx), &task(), &id("examples"), policy())
            .unwrap()
            .unwrap();
    assert_eq!(
        ExampleSet::new(
            id("s"),
            r,
            vec![a.clone(), b.clone()],
            vec![
                decision(&a, ExampleReason::Selected),
                decision(&b, ExampleReason::Selected)
            ]
        ),
        Err(ContextPlanningError::InfeasibleBudget)
    );
}

#[test]
fn captured_sources_and_synthetic_examples_need_validation_and_keep_token_semantics() {
    for source in [
        ExampleSourceClass::Curated,
        ExampleSourceClass::Repository,
        ExampleSourceClass::Retrieval,
        ExampleSourceClass::GovernedExperience,
        ExampleSourceClass::PromotedProcedure,
    ] {
        let mut c = candidate("source");
        c.source_class = source;
        c.expected_output = None;
        c.input.quality = QualityMetadata::new(
            TrustClass::SyntheticData,
            SensitivityClass::Normal,
            Confidence::Unknown,
            FreshnessStatus::Fresh,
            Uncertainty::None,
        );
        c.tokens.count = TokenCount::Exact {
            tokens: 80,
            target: text("reference-tokenizer"),
        };
        seal(&mut c);
        assert_eq!(
            example_rejection(&requirement(), &c, &BTreeSet::new()),
            None
        );
        let s = set(c.clone());
        assert_eq!(
            s.to_value()["selected"][0]["token_estimate"]["count"]["kind"],
            "EXACT"
        );
        assert_eq!(s.usage().tokens, 80);
        assert_eq!(
            s.source().unwrap().quality.trust(),
            TrustClass::SyntheticData
        );
        c.validation = CurationState::Pending;
        assert_eq!(
            example_rejection(&requirement(), &c, &BTreeSet::new()),
            Some(ExampleReason::Unvalidated)
        );
    }
    let mut c = candidate("unknown");
    c.tokens.count = TokenCount::Unknown {
        reason: text("offline"),
    };
    assert_eq!(c.to_value()["token_estimate"]["count"]["kind"], "UNKNOWN");
    assert!(c.to_value()["token_estimate"]["bound"].is_null());
}
