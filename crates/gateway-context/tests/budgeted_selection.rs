use gateway_context::budgeted::*;
use gateway_context::{ContextFragment, FragmentKind, FragmentMetadata};
use gateway_domain::*;
use std::collections::{BTreeMap, BTreeSet};

fn id(value: &str) -> ReferenceId {
    ReferenceId::new(value).unwrap()
}
fn target() -> NonEmptyText {
    NonEmptyText::new("runtime:model:v1").unwrap()
}
fn exact(tokens: u64) -> TokenEstimate {
    TokenEstimate {
        estimator: TokenEstimatorId::new("estimator").unwrap(),
        version: TokenEstimatorVersion::new("v1").unwrap(),
        count: TokenCount::Exact {
            tokens,
            target: target(),
        },
    }
}
fn estimated(tokens: u64, upper: Option<u64>) -> TokenEstimate {
    TokenEstimate {
        count: TokenCount::Estimated {
            tokens,
            upper_bound: upper,
            semantics: NonEmptyText::new("conservative").unwrap(),
        },
        ..exact(0)
    }
}
fn fragment(name: &str, source: &str, content: &str, kind: FragmentKind) -> ContextFragment {
    let trust = match kind {
        FragmentKind::Evidence => TrustClass::ObservedEvidence,
        FragmentKind::Memory => TrustClass::DerivedAssessment,
        _ => TrustClass::RetrievedContent,
    };
    ContextFragment::external(
        id(name),
        kind,
        content,
        FragmentMetadata {
            provenance: KnowledgeProvenance::new(source, Some("rev")).unwrap(),
            evidence: BTreeSet::from([id("evidence")]),
            quality: QualityMetadata::new(
                trust,
                SensitivityClass::Confidential,
                Confidence::Unknown,
                FreshnessStatus::Stale,
                Uncertainty::Probabilistic,
            ),
            rationale: NonEmptyText::new("needed").unwrap(),
            validation: Some(id("validated")),
        },
        ContextScopeId::new("scope").unwrap(),
        PlanStepId::new("step").unwrap(),
    )
    .unwrap()
}
fn ranked(name: &str, source: &str, content: &str, tokens: u64, score: u32) -> RankedFragment {
    RankedFragment {
        fragment: fragment(name, source, content, FragmentKind::Knowledge),
        score,
        mandatory: false,
        estimate: exact(tokens),
    }
}
fn budget(knowledge: u64, evidence: u64) -> ContextBudget {
    ContextBudget::new(
        TokenBudget(1000),
        BTreeMap::from([
            (ContextBudgetClass::AuthorityReserved, TokenBudget(10)),
            (ContextBudgetClass::TaskReserved, TokenBudget(10)),
            (ContextBudgetClass::OutputContractReserved, TokenBudget(10)),
            (ContextBudgetClass::RuntimeState, TokenBudget(10)),
            (ContextBudgetClass::SafetyMargin, TokenBudget(5)),
            (ContextBudgetClass::Knowledge, TokenBudget(knowledge)),
            (ContextBudgetClass::Evidence, TokenBudget(evidence)),
        ]),
    )
    .unwrap()
}
fn sections() -> BTreeMap<ContextBudgetClass, TokenEstimate> {
    BTreeMap::from([
        (ContextBudgetClass::AuthorityReserved, exact(4)),
        (ContextBudgetClass::TaskReserved, exact(3)),
        (ContextBudgetClass::OutputContractReserved, exact(2)),
        (ContextBudgetClass::RuntimeState, exact(1)),
    ])
}
fn run(
    items: &[RankedFragment],
    compacted: &[CompactedCandidate],
    required: &[&str],
    budget: &ContextBudget,
) -> Result<BudgetedSelection, SelectionError> {
    select_context(
        budget,
        &target(),
        &sections(),
        items,
        compacted,
        &required.iter().map(|s| id(s)).collect(),
    )
}
#[test]
fn deterministic_ties_budget_and_source_provenance() {
    let a = ranked("a", "source-a", "same", 3, 10);
    let b = ranked("b", "source-b", "same", 3, 10);
    let c = ranked("c", "source-a", "same", 3, 10);
    let d = ranked("d", "source-d", "other", 3, 9);
    let first = run(
        &[d.clone(), c.clone(), b.clone(), a.clone()],
        &[],
        &[],
        &budget(6, 0),
    )
    .unwrap();
    let second = run(&[a, b, c, d], &[], &[], &budget(6, 0)).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.selected, BTreeSet::from([id("a"), id("b")]));
    assert_eq!(
        first
            .decisions
            .iter()
            .find(|d| d.id == id("c"))
            .unwrap()
            .reason,
        SelectionReason::Redundant
    );
    assert_eq!(
        first
            .decisions
            .iter()
            .find(|d| d.id == id("d"))
            .unwrap()
            .reason,
        SelectionReason::BudgetExceeded
    );
    assert_eq!(first.usage[&ContextBudgetClass::SafetyMargin], 5);
}
#[test]
fn mandatory_evidence_and_fixed_sections_fail_explicitly() {
    let evidence = RankedFragment {
        fragment: fragment("e", "sensor", "claim", FragmentKind::Evidence),
        score: 0,
        mandatory: false,
        estimate: exact(4),
    };
    assert_eq!(
        run(std::slice::from_ref(&evidence), &[], &["e"], &budget(0, 3)),
        Err(SelectionError::MandatoryOverBudget(
            ContextBudgetClass::Evidence
        ))
    );
    assert_eq!(
        run(&[], &[], &["missing"], &budget(0, 0)),
        Err(SelectionError::MissingMandatory(id("missing")))
    );
    let mut bad = sections();
    bad.insert(ContextBudgetClass::AuthorityReserved, exact(11));
    assert_eq!(
        select_context(&budget(0, 0), &target(), &bad, &[], &[], &BTreeSet::new()),
        Err(SelectionError::MandatoryOverBudget(
            ContextBudgetClass::AuthorityReserved
        ))
    );
    assert!(run(&[evidence], &[], &["e"], &budget(0, 4)).is_ok());
}
#[test]
fn unknown_and_unbounded_estimates_never_become_exact() {
    let mut item = ranked("a", "source", "text", 1, 1);
    item.estimate = estimated(1, None);
    assert_eq!(
        run(&[item.clone()], &[], &[], &budget(10, 0)),
        Err(SelectionError::InvalidEstimate(id("a")))
    );
    item.estimate = estimated(1, Some(4));
    assert_eq!(
        run(&[item.clone()], &[], &[], &budget(3, 0))
            .unwrap()
            .selected
            .len(),
        0
    );
    assert_eq!(
        run(&[item], &[], &[], &budget(4, 0)).unwrap().usage[&ContextBudgetClass::Knowledge],
        4
    );
    let mut bad = sections();
    bad.insert(
        ContextBudgetClass::TaskReserved,
        TokenEstimate {
            count: TokenCount::Unknown {
                reason: NonEmptyText::new("offline").unwrap(),
            },
            ..exact(0)
        },
    );
    assert_eq!(
        select_context(&budget(4, 0), &target(), &bad, &[], &[], &BTreeSet::new()),
        Err(SelectionError::InvalidSectionEstimate(
            ContextBudgetClass::TaskReserved
        ))
    );
}
#[test]
fn compaction_preserves_lineage_and_rejects_metadata_relabeling() {
    let a = ranked("a", "source", "long-a", 8, 10);
    let b = ranked("b", "source", "long-b", 8, 9);
    let summary = CompactedCandidate {
        fragment: fragment("summary", "source", "brief", FragmentKind::Knowledge),
        sources: BTreeSet::from([id("a"), id("b")]),
        estimate: exact(3),
    };
    let result = run(
        &[a.clone(), b.clone()],
        std::slice::from_ref(&summary),
        &[],
        &budget(3, 0),
    )
    .unwrap();
    assert_eq!(result.selected, BTreeSet::from([id("summary")]));
    assert_eq!(
        result.lineage[&id("summary")],
        BTreeSet::from([id("a"), id("b")])
    );
    let mut hostile = summary;
    hostile.fragment = fragment("summary", "other", "brief", FragmentKind::Knowledge);
    assert_eq!(
        run(&[a, b], &[hostile], &[], &budget(3, 0)),
        Err(SelectionError::InvalidCompaction(id("summary")))
    );
}

#[test]
fn duplicates_invalid_compaction_and_target_mismatch_fail() {
    let a = ranked("a", "source", "text", 4, 1);
    let mut changed = a.clone();
    changed.score = 2;
    assert_eq!(
        run(&[a.clone(), changed], &[], &[], &budget(10, 0)),
        Err(SelectionError::DuplicateId(id("a")))
    );
    let bad = CompactedCandidate {
        fragment: fragment("summary", "source", "brief", FragmentKind::Knowledge),
        sources: BTreeSet::from([id("a")]),
        estimate: exact(1),
    };
    assert_eq!(
        run(
            std::slice::from_ref(&a),
            &[bad.clone(), bad.clone()],
            &[],
            &budget(10, 0)
        ),
        Err(SelectionError::InvalidCompaction(id("summary")))
    );
    assert_eq!(
        run(
            std::slice::from_ref(&a),
            &[CompactedCandidate {
                sources: BTreeSet::from([id("missing")]),
                ..bad.clone()
            }],
            &[],
            &budget(10, 0)
        ),
        Err(SelectionError::InvalidCompaction(id("summary")))
    );
    let mut mandatory = a.clone();
    mandatory.mandatory = true;
    assert_eq!(
        run(&[mandatory], &[bad], &[], &budget(10, 0)),
        Err(SelectionError::InvalidCompaction(id("summary")))
    );
    let mut wrong_target = a;
    wrong_target.estimate.count = TokenCount::Exact {
        tokens: 4,
        target: NonEmptyText::new("other-runtime").unwrap(),
    };
    assert_eq!(
        run(&[wrong_target], &[], &[], &budget(10, 0)),
        Err(SelectionError::InvalidEstimate(id("a")))
    );
}
#[test]
fn checked_arithmetic_rejects_overflow_and_zero_capacity_excludes_optional() {
    let input = RankedFragment {
        fragment: ContextFragment::external(
            id("input"),
            FragmentKind::UserInput,
            "text",
            FragmentMetadata {
                provenance: KnowledgeProvenance::new("caller", None::<String>).unwrap(),
                evidence: BTreeSet::new(),
                quality: QualityMetadata::new(
                    TrustClass::CallerInput,
                    SensitivityClass::Normal,
                    Confidence::Unknown,
                    FreshnessStatus::Unknown,
                    Uncertainty::None,
                ),
                rationale: NonEmptyText::new("needed").unwrap(),
                validation: None,
            },
            ContextScopeId::new("scope").unwrap(),
            PlanStepId::new("step").unwrap(),
        )
        .unwrap(),
        score: 1,
        mandatory: false,
        estimate: exact(u64::MAX),
    };
    let max_budget = ContextBudget::new(
        TokenBudget(u64::MAX),
        BTreeMap::from([(ContextBudgetClass::TaskReserved, TokenBudget(u64::MAX))]),
    )
    .unwrap();
    let section = BTreeMap::from([
        (ContextBudgetClass::AuthorityReserved, exact(0)),
        (ContextBudgetClass::TaskReserved, exact(1)),
        (ContextBudgetClass::OutputContractReserved, exact(0)),
    ]);
    assert_eq!(
        select_context(
            &max_budget,
            &target(),
            &section,
            &[input],
            &[],
            &BTreeSet::new()
        ),
        Err(SelectionError::ArithmeticOverflow)
    );
    let ordinary = ranked("a", "source", "text", 1, 1);
    assert!(
        run(&[ordinary], &[], &[], &budget(0, 0))
            .unwrap()
            .selected
            .is_empty()
    );
}
