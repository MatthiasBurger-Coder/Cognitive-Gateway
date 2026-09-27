use gateway_application::reasoning_strategy::{
    ReasoningAdapter, StrategyAttempt, StrategyAttemptFailure, StrategyDecision,
    StrategyDispatchError, StrategyHandoff, select_strategy,
};
use gateway_domain::{
    EvidenceId, ExecutionContextId, ReasoningCapability as Capability,
    ReasoningStrategy as Strategy, ReasoningStrategyContract, ReferenceId, StrategyBudget,
    StrategyError, StrategyFallback, StrategyUsage, SufficiencyAssessment, SufficiencyFinding,
    VerificationExpectation,
};
use std::collections::{BTreeMap, BTreeSet};

struct DirectOnly;
impl ReasoningAdapter for DirectOnly {
    fn supported_strategies(&self) -> BTreeSet<Strategy> {
        BTreeSet::from([Strategy::Direct])
    }
    fn capabilities(&self) -> BTreeSet<Capability> {
        BTreeSet::new()
    }
    fn attempt(&self, _: StrategyHandoff<'_>) -> Result<StrategyAttempt, StrategyAttemptFailure> {
        panic!("unsupported strategy must not dispatch")
    }
}

fn evidence(findings: &[SufficiencyFinding]) -> SufficiencyAssessment {
    SufficiencyAssessment {
        state: findings[0],
        findings: findings.iter().copied().collect(),
        accepted: BTreeSet::new(),
        rejected: BTreeSet::new(),
        rejection_reasons: BTreeMap::new(),
        validated_evidence: BTreeSet::new(),
        missing_evidence: BTreeSet::new(),
        missing_provenance: BTreeSet::new(),
        missing_evidence_count: 0,
    }
}

fn multi(
    fallback: Option<StrategyFallback>,
    verification: VerificationExpectation,
) -> ReasoningStrategyContract {
    ReasoningStrategyContract::new(
        Strategy::MultiPass,
        BTreeSet::from([Capability::MultiplePasses]),
        "application/json".into(),
        verification,
        StrategyBudget {
            iterations: 2,
            retrieval_rounds: 0,
            cost: 0,
            cost_unit: "microcredits".into(),
            latency_ms: 10,
            tokens: 20,
        },
        fallback,
        "1.0",
    )
    .unwrap()
}

#[test]
fn direct_only_adapter_requires_explicit_fallback() {
    let sufficient = evidence(&[SufficiencyFinding::Sufficient]);
    assert_eq!(
        select_strategy(
            &DirectOnly,
            &multi(None, VerificationExpectation::None),
            &sufficient
        ),
        Err(StrategyDispatchError::Contract(
            StrategyError::UnsupportedStrategy
        ))
    );
    let contract = multi(
        Some(StrategyFallback {
            strategy: Strategy::Direct,
            reason: "single attempt acceptable".into(),
        }),
        VerificationExpectation::None,
    );
    let selection = select_strategy(&DirectOnly, &contract, &sufficient).unwrap();
    assert_eq!(selection.selected, Strategy::Direct);
    assert_eq!(
        selection.fallback_reason.as_deref(),
        Some("single attempt acceptable")
    );
}

#[test]
fn evidence_failures_stay_visible_even_with_declared_fallback() {
    let contract = multi(
        Some(StrategyFallback {
            strategy: Strategy::Direct,
            reason: "single attempt acceptable".into(),
        }),
        VerificationExpectation::EvidenceBacked,
    );
    for finding in [
        SufficiencyFinding::Insufficient,
        SufficiencyFinding::Conflicting,
        SufficiencyFinding::Stale,
        SufficiencyFinding::Contaminated,
        SufficiencyFinding::BudgetExhausted,
    ] {
        let assessment = evidence(&[SufficiencyFinding::Sufficient, finding]);
        assert_eq!(
            select_strategy(&DirectOnly, &contract, &assessment),
            Err(StrategyDispatchError::Contract(
                StrategyError::InsufficientEvidence
            ))
        );
    }
}

#[test]
fn public_trace_contains_only_decisions_references_and_usage() {
    let contract = multi(
        Some(StrategyFallback {
            strategy: Strategy::Direct,
            reason: "single attempt acceptable".into(),
        }),
        VerificationExpectation::None,
    );
    let selection = select_strategy(
        &DirectOnly,
        &contract,
        &evidence(&[SufficiencyFinding::Sufficient]),
    )
    .unwrap();
    let decision = StrategyDecision {
        selection,
        context_id: ExecutionContextId::new("context-1").unwrap(),
        output_reference: ReferenceId::new("output-1").unwrap(),
        provenance_references: BTreeSet::from([ReferenceId::new("evidence-1").unwrap()]),
        validated_evidence: BTreeSet::from([EvidenceId::new("evidence-1").unwrap()]),
        evidence_findings: BTreeSet::from([SufficiencyFinding::Sufficient]),
        cumulative_usage: StrategyUsage {
            iterations: 1,
            ..StrategyUsage::default()
        },
    };
    let value: serde_json::Value = serde_json::from_str(&decision.to_json().unwrap()).unwrap();
    assert_eq!(value["selected"], "DIRECT");
    assert_eq!(value["context_id"], "context-1");
    assert_eq!(value["validated_evidence"][0], "evidence-1");
    assert_eq!(value["fallback_reason"], "single attempt acceptable");
    assert_eq!(value["provenance_references"][0], "evidence-1");
    assert!(value.get("reasoning").is_none());
}
