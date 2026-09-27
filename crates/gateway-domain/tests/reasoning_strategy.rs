use gateway_domain::{
    ReasoningCapability as Capability, ReasoningStrategy as Strategy, ReasoningStrategyContract,
    StrategyBudget, StrategyError, StrategyFallback, StrategyUsage, VerificationExpectation,
};
use std::collections::BTreeSet;

fn contract(strategy: Strategy) -> ReasoningStrategyContract {
    let capabilities = strategy.required_capability().into_iter().collect();
    ReasoningStrategyContract::new(
        strategy,
        capabilities,
        "application/json".into(),
        if strategy == Strategy::Verify {
            VerificationExpectation::EvidenceBacked
        } else {
            VerificationExpectation::None
        },
        StrategyBudget {
            iterations: 3,
            retrieval_rounds: 2,
            cost: 8,
            cost_unit: "microcredits".into(),
            latency_ms: 100,
            tokens: 200,
        },
        None,
        "1.0",
    )
    .unwrap()
}

#[test]
fn strategy_wire_round_trips_and_rejects_unknown_or_invalid_values() {
    for strategy in [
        Strategy::Direct,
        Strategy::RetrievalAssisted,
        Strategy::PlanExecute,
        Strategy::Verify,
        Strategy::MultiPass,
    ] {
        let value = serde_json::to_string(&contract(strategy)).unwrap();
        assert_eq!(
            serde_json::from_str::<ReasoningStrategyContract>(&value).unwrap(),
            contract(strategy)
        );
        assert_eq!(
            serde_json::to_string(
                &serde_json::from_str::<ReasoningStrategyContract>(&value).unwrap()
            )
            .unwrap(),
            value
        );
    }
    let value = serde_json::to_string(&contract(Strategy::Direct)).unwrap();
    for changed in [
        value.replace("\"DIRECT\"", "\"UNKNOWN\""),
        value.replace("\"1.0\"", "\"2.0\""),
        value.replace("\"iterations\":3", "\"iterations\":0"),
        value.replace(
            "\"output_contract\":\"application/json\"",
            "\"output_contract\":\" \"",
        ),
    ] {
        assert!(
            serde_json::from_str::<ReasoningStrategyContract>(&changed).is_err(),
            "{changed}"
        );
    }
}

#[test]
fn intrinsic_capabilities_and_budget_shape_are_checked() {
    let mut value = serde_json::to_value(contract(Strategy::RetrievalAssisted)).unwrap();
    value["required_capabilities"] = serde_json::json!([]);
    assert!(serde_json::from_value::<ReasoningStrategyContract>(value).is_err());
    let mut value = serde_json::to_value(contract(Strategy::RetrievalAssisted)).unwrap();
    value["budget"]["retrieval_rounds"] = serde_json::json!(0);
    assert!(serde_json::from_value::<ReasoningStrategyContract>(value).is_err());
    let mut value = serde_json::to_value(contract(Strategy::Direct)).unwrap();
    value["budget"]["cost_unit"] = serde_json::json!("bad unit");
    assert!(serde_json::from_value::<ReasoningStrategyContract>(value).is_err());
    let mut value = serde_json::to_value(contract(Strategy::Verify)).unwrap();
    value["verification"] = serde_json::json!("NONE");
    assert!(serde_json::from_value::<ReasoningStrategyContract>(value).is_err());
}

#[test]
fn unsupported_strategy_needs_declared_compatible_fallback() {
    let direct = BTreeSet::from([Strategy::Direct]);
    assert_eq!(
        contract(Strategy::MultiPass).select(&direct, &BTreeSet::new()),
        Err(StrategyError::UnsupportedStrategy)
    );
    let allowed = ReasoningStrategyContract::new(
        Strategy::MultiPass,
        BTreeSet::from([Capability::MultiplePasses]),
        "application/json".into(),
        VerificationExpectation::None,
        contract(Strategy::Direct).budget().clone(),
        Some(StrategyFallback {
            strategy: Strategy::Direct,
            reason: "single bounded attempt accepted".into(),
        }),
        "1.0",
    )
    .unwrap();
    let selected = allowed.select(&direct, &BTreeSet::new()).unwrap();
    assert_eq!(selected.requested, Strategy::MultiPass);
    assert_eq!(selected.selected, Strategy::Direct);
    assert_eq!(
        selected.fallback_reason.as_deref(),
        Some("single bounded attempt accepted")
    );
    let needs_retrieval = ReasoningStrategyContract::new(
        Strategy::MultiPass,
        BTreeSet::from([Capability::MultiplePasses, Capability::Retrieval]),
        "application/json".into(),
        VerificationExpectation::None,
        allowed.budget().clone(),
        Some(StrategyFallback {
            strategy: Strategy::Direct,
            reason: "declared".into(),
        }),
        "1.0",
    )
    .unwrap();
    assert_eq!(
        needs_retrieval.select(&direct, &BTreeSet::new()),
        Err(StrategyError::IncompatibleFallback)
    );
}

#[test]
fn aggregate_usage_cannot_reset_or_overflow() {
    let budget = contract(Strategy::MultiPass).budget().clone();
    let prior = StrategyUsage {
        iterations: 2,
        retrieval_rounds: 1,
        cost: 7,
        latency_ms: 90,
        tokens: 190,
    };
    let delta = StrategyUsage {
        iterations: 1,
        retrieval_rounds: 1,
        cost: 1,
        latency_ms: 10,
        tokens: 10,
    };
    assert_eq!(prior.checked_add(&delta, &budget).unwrap().iterations, 3);
    assert_eq!(
        prior.checked_add(
            &StrategyUsage {
                iterations: 2,
                ..delta.clone()
            },
            &budget
        ),
        Err(StrategyError::BudgetExceeded)
    );
    assert_eq!(
        prior.checked_add(
            &StrategyUsage {
                tokens: u64::MAX,
                ..delta
            },
            &budget
        ),
        Err(StrategyError::ArithmeticOverflow)
    );
}

#[test]
fn getters_and_primary_selection_preserve_declared_contract() {
    let contract = contract(Strategy::PlanExecute);
    assert_eq!(contract.strategy(), Strategy::PlanExecute);
    assert_eq!(contract.verification(), VerificationExpectation::None);
    assert_eq!(contract.output_contract(), "application/json");
    assert_eq!(
        contract.required_capabilities(),
        &BTreeSet::from([Capability::Planning])
    );
    let selection = contract
        .select(
            &BTreeSet::from([Strategy::PlanExecute]),
            &BTreeSet::from([Capability::Planning]),
        )
        .unwrap();
    assert_eq!(selection.selected, Strategy::PlanExecute);
    assert_eq!(selection.fallback_reason, None);
}

#[test]
fn fallback_shape_and_semantics_are_validated() {
    let base = contract(Strategy::Direct);
    for fallback in [
        StrategyFallback {
            strategy: Strategy::Direct,
            reason: "same".into(),
        },
        StrategyFallback {
            strategy: Strategy::Verify,
            reason: " ".into(),
        },
    ] {
        assert_eq!(
            ReasoningStrategyContract::new(
                Strategy::Direct,
                BTreeSet::new(),
                "application/json".into(),
                VerificationExpectation::None,
                base.budget().clone(),
                Some(fallback),
                "1.0"
            ),
            Err(StrategyError::InvalidFallback)
        );
    }
    let verify = ReasoningStrategyContract::new(
        Strategy::Direct,
        BTreeSet::new(),
        "application/json".into(),
        VerificationExpectation::None,
        base.budget().clone(),
        Some(StrategyFallback {
            strategy: Strategy::Verify,
            reason: "declared".into(),
        }),
        "1.0",
    )
    .unwrap();
    assert_eq!(
        verify.select(
            &BTreeSet::from([Strategy::Verify]),
            &BTreeSet::from([Capability::Verification])
        ),
        Err(StrategyError::IncompatibleFallback)
    );
    let mut no_rounds = base.budget().clone();
    no_rounds.retrieval_rounds = 0;
    let retrieval = ReasoningStrategyContract::new(
        Strategy::Direct,
        BTreeSet::new(),
        "application/json".into(),
        VerificationExpectation::None,
        no_rounds,
        Some(StrategyFallback {
            strategy: Strategy::RetrievalAssisted,
            reason: "declared".into(),
        }),
        "1.0",
    )
    .unwrap();
    assert_eq!(
        retrieval.select(
            &BTreeSet::from([Strategy::RetrievalAssisted]),
            &BTreeSet::from([Capability::Retrieval])
        ),
        Err(StrategyError::IncompatibleFallback)
    );
}

#[test]
fn every_usage_dimension_detects_overflow() {
    let budget = StrategyBudget {
        iterations: u64::MAX,
        retrieval_rounds: u64::MAX,
        cost: u64::MAX,
        cost_unit: "microcredits".into(),
        latency_ms: u64::MAX,
        tokens: u64::MAX,
    };
    let prior = StrategyUsage {
        iterations: u64::MAX,
        retrieval_rounds: u64::MAX,
        cost: u64::MAX,
        latency_ms: u64::MAX,
        tokens: u64::MAX,
    };
    for delta in [
        StrategyUsage {
            iterations: 1,
            ..StrategyUsage::default()
        },
        StrategyUsage {
            retrieval_rounds: 1,
            ..StrategyUsage::default()
        },
        StrategyUsage {
            cost: 1,
            ..StrategyUsage::default()
        },
        StrategyUsage {
            latency_ms: 1,
            ..StrategyUsage::default()
        },
        StrategyUsage {
            tokens: 1,
            ..StrategyUsage::default()
        },
    ] {
        assert_eq!(
            prior.checked_add(&delta, &budget),
            Err(StrategyError::ArithmeticOverflow)
        );
    }
}
