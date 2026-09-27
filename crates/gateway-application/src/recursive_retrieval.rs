//! Bounded, advisory retrieval rounds. The host supplies evidence validation;
//! only CG-14 and the policy/process boundaries may authorize execution.
use crate::ports::outbound::KnowledgeRetrievalPort;
use gateway_domain::{
    AssessedFragment, BudgetUsage, EvidenceId, RetrievalBatch, RetrievalError,
    RetrievalExplanation, RetrievalPlan, RetrievalQuery, RetrievalResult, RetrievalRound,
    RetrievalStatus, StopCondition, SufficiencyAssessment, SufficiencyFinding,
    assess_sufficiency_with_threshold,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
};

/// Validation comes from the owning evidence boundary, never retrieved text.
pub trait RetrievalEvidencePort {
    fn validate(&self, result: &RetrievalResult) -> Result<EvidenceReview, RetrievalError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceReview {
    pub validated_evidence: BTreeSet<EvidenceId>,
    pub contaminated: bool,
}

/// The refiner can propose queries only. It cannot change scope, sources,
/// requirements or budgets of the existing plan.
pub trait QueryRefinementPort {
    fn refine(&self, trace: &RoundTrace) -> Result<BTreeSet<RetrievalQuery>, RetrievalError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecursiveStop {
    Sufficient,
    BudgetExhausted,
    NoProgress,
    RepeatedQuery,
    NoRefinement,
    AdapterFailure,
    EvidenceUnavailable,
    RefinementFailure,
    TerminalBatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundTrace {
    pub round: RetrievalRound,
    pub queries: BTreeSet<RetrievalQuery>,
    pub next_queries: Option<BTreeSet<RetrievalQuery>>,
    pub accepted: BTreeSet<gateway_domain::ReferenceId>,
    pub rejected: BTreeSet<gateway_domain::ReferenceId>,
    pub explanations: BTreeSet<RetrievalExplanation>,
    pub assessment: SufficiencyAssessment,
    pub usage: BudgetUsage,
    pub error: Option<RetrievalError>,
    pub stop: Option<RecursiveStop>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecursiveOutcome {
    pub results: Vec<RetrievalResult>,
    pub assessment: SufficiencyAssessment,
    pub usage: BudgetUsage,
    pub rounds: Vec<RoundTrace>,
    pub stop: RecursiveStop,
}

/// All usage is cumulative. An error after dispatch ends the run and preserves
/// the last reported usage; adapters must include timed/cancelled work in their
/// own measured response to provide exact accounting for such attempts.
pub fn retrieve_until_sufficient(
    plan: &RetrievalPlan,
    initial_usage: BudgetUsage,
    retrieval: &dyn KnowledgeRetrievalPort,
    evidence: &dyn RetrievalEvidencePort,
    refiner: Option<&dyn QueryRefinementPort>,
    max_no_progress: u64,
) -> Result<RecursiveOutcome, RetrievalError> {
    let mut active = plan.clone();
    let mut usage = initial_usage;
    usage.validate(&plan.request().input().budget)?;
    let mut retained = BTreeMap::<_, RetrievalResult>::new();
    let mut reviews = BTreeMap::<_, AssessedFragment>::new();
    let mut history = BTreeSet::new();
    let mut rounds = Vec::new();
    let mut no_progress = 0u64;
    let minimum_evidence = match plan.request().input().stop {
        Some(StopCondition::EvidenceSatisfied(count)) => count.get(),
        _ => 1,
    };
    let assess = |reviewed: &BTreeMap<gateway_domain::ReferenceId, AssessedFragment>, exhausted| {
        assess_sufficiency_with_threshold(
            &plan.request().input().required,
            &reviewed.values().cloned().collect::<Vec<_>>(),
            minimum_evidence,
            exhausted,
        )
    };
    let mut assessment = assess(&reviews, false);
    loop {
        let queries = active.request().input().queries.clone();
        let required_tokens = queries.iter().try_fold(0u64, |sum, query| {
            sum.checked_add(query.0.as_str().len() as u64)
                .ok_or(RetrievalError::ArithmeticOverflow)
        })?;
        let budget = &plan.request().input().budget;
        let available_context = budget
            .context
            .reservations()
            .get(&gateway_domain::ContextBudgetClass::Knowledge)
            .map_or(0, |value| value.0);
        let used_context = usage
            .context
            .get(&gateway_domain::ContextBudgetClass::Knowledge)
            .copied()
            .unwrap_or(0);
        let cannot_reserve = usage.rounds >= budget.rounds.0.get()
            || usage.results >= budget.results.0.get()
            || usage.elapsed_ms >= budget.latency.0.get()
            || (budget.cost.maximum > 0 && usage.cost >= budget.cost.maximum)
            || used_context >= available_context
            || usage
                .tokens
                .checked_add(required_tokens)
                .is_none_or(|next| next > budget.tokens.0);
        if cannot_reserve {
            assessment = assess(&reviews, true);
            return Ok(finish(
                retained,
                assessment,
                usage,
                rounds,
                RecursiveStop::BudgetExhausted,
            ));
        }
        history.insert(queries.clone());
        let round = RetrievalRound(
            NonZeroU64::new(
                usage
                    .rounds
                    .checked_add(1)
                    .ok_or(RetrievalError::ArithmeticOverflow)?,
            )
            .expect("increment is nonzero"),
        );
        let before = assessment.validated_evidence.clone();
        let batch: RetrievalBatch = match retrieval.retrieve_measured(&active, round, &usage) {
            Ok(batch) => batch,
            Err(failure) => {
                let valid_usage = failure.usage.rounds == round.0.get()
                    && failure.usage.results >= usage.results
                    && failure.usage.elapsed_ms >= usage.elapsed_ms
                    && failure.usage.cost >= usage.cost
                    && failure.usage.tokens >= usage.tokens
                    && failure.usage.cost_unit == usage.cost_unit;
                if valid_usage {
                    usage = failure.usage;
                } else {
                    usage.rounds = round.0.get();
                }
                rounds.push(RoundTrace {
                    round,
                    queries,
                    next_queries: None,
                    accepted: BTreeSet::new(),
                    rejected: BTreeSet::new(),
                    explanations: BTreeSet::new(),
                    assessment: assessment.clone(),
                    usage: usage.clone(),
                    error: Some(if valid_usage {
                        failure.error
                    } else {
                        RetrievalError::InvalidResult
                    }),
                    stop: Some(RecursiveStop::AdapterFailure),
                });
                return Ok(finish(
                    retained,
                    assessment,
                    usage,
                    rounds,
                    RecursiveStop::AdapterFailure,
                ));
            }
        };
        if batch.input().plan != *active.id()
            || batch.input().round != round
            || batch.input().scope != active.request().input().scope
            || batch.input().usage.rounds != round.0.get()
            || batch.input().usage.results < usage.results
            || batch.input().usage.tokens < usage.tokens
            || batch.input().usage.cost < usage.cost
            || batch.input().usage.elapsed_ms < usage.elapsed_ms
            || usage
                .results
                .checked_add(batch.input().results.len() as u64)
                .is_none_or(|minimum| batch.input().usage.results < minimum)
            || batch.input().usage.validate(budget).is_err()
        {
            usage.rounds = round.0.get();
            rounds.push(RoundTrace {
                round,
                queries,
                next_queries: None,
                accepted: BTreeSet::new(),
                rejected: BTreeSet::new(),
                explanations: batch.input().explanations.clone(),
                assessment: assessment.clone(),
                usage: usage.clone(),
                error: Some(RetrievalError::InvalidResult),
                stop: Some(RecursiveStop::AdapterFailure),
            });
            return Ok(finish(
                retained,
                assessment,
                usage,
                rounds,
                RecursiveStop::AdapterFailure,
            ));
        }
        usage = batch.input().usage.clone();
        for result in &batch.input().results {
            let review = match evidence.validate(result) {
                Ok(review) => review,
                Err(error) => {
                    rounds.push(RoundTrace {
                        round,
                        queries,
                        next_queries: None,
                        accepted: BTreeSet::new(),
                        rejected: BTreeSet::new(),
                        explanations: batch.input().explanations.clone(),
                        assessment: assessment.clone(),
                        usage: usage.clone(),
                        error: Some(error),
                        stop: Some(RecursiveStop::EvidenceUnavailable),
                    });
                    return Ok(finish(
                        retained,
                        assessment,
                        usage,
                        rounds,
                        RecursiveStop::EvidenceUnavailable,
                    ));
                }
            };
            let id = result.fragment.id.clone();
            let contaminated = retained.get(&id).is_some_and(|prior| prior != result);
            retained.entry(id.clone()).or_insert_with(|| result.clone());
            reviews
                .entry(id)
                .and_modify(|prior| {
                    prior.contaminated |= contaminated || review.contaminated;
                    prior
                        .validated_evidence
                        .extend(review.validated_evidence.iter().cloned());
                })
                .or_insert_with(|| AssessedFragment {
                    fragment: result.fragment.clone(),
                    validated_evidence: review.validated_evidence,
                    contaminated: review.contaminated || contaminated,
                });
        }
        assessment = assess(&reviews, false);
        let mut trace = RoundTrace {
            round,
            queries,
            next_queries: None,
            accepted: assessment.accepted.clone(),
            rejected: assessment.rejected.clone(),
            explanations: batch.input().explanations.clone(),
            assessment: assessment.clone(),
            usage: usage.clone(),
            error: None,
            stop: None,
        };
        if assessment
            .findings
            .contains(&SufficiencyFinding::Sufficient)
        {
            trace.stop = Some(RecursiveStop::Sufficient);
            rounds.push(trace);
            return Ok(finish(
                retained,
                assessment,
                usage,
                rounds,
                RecursiveStop::Sufficient,
            ));
        }
        if usage.rounds >= budget.rounds.0.get()
            || usage.results >= budget.results.0.get()
            || usage.elapsed_ms >= budget.latency.0.get()
            || (budget.cost.maximum > 0 && usage.cost >= budget.cost.maximum)
            || usage.tokens >= budget.tokens.0
        {
            assessment = assess(&reviews, true);
            trace.assessment = assessment.clone();
            trace.stop = Some(RecursiveStop::BudgetExhausted);
            rounds.push(trace);
            return Ok(finish(
                retained,
                assessment,
                usage,
                rounds,
                RecursiveStop::BudgetExhausted,
            ));
        }
        if matches!(
            batch.input().status,
            RetrievalStatus::Failed | RetrievalStatus::Complete
        ) || batch.input().reason == gateway_domain::RetrievalReason::BudgetReached
        {
            trace.stop = Some(RecursiveStop::TerminalBatch);
            rounds.push(trace);
            return Ok(finish(
                retained,
                assessment,
                usage,
                rounds,
                RecursiveStop::TerminalBatch,
            ));
        }
        no_progress = if assessment.validated_evidence == before {
            no_progress.saturating_add(1)
        } else {
            0
        };
        if no_progress > max_no_progress {
            trace.stop = Some(RecursiveStop::NoProgress);
            rounds.push(trace);
            return Ok(finish(
                retained,
                assessment,
                usage,
                rounds,
                RecursiveStop::NoProgress,
            ));
        }
        let Some(refiner) = refiner else {
            trace.stop = Some(RecursiveStop::NoRefinement);
            rounds.push(trace);
            return Ok(finish(
                retained,
                assessment,
                usage,
                rounds,
                RecursiveStop::NoRefinement,
            ));
        };
        let proposed = match refiner.refine(&trace) {
            Ok(proposed) if !proposed.is_empty() => proposed,
            Ok(_) => {
                trace.error = Some(RetrievalError::InvalidPlan);
                trace.stop = Some(RecursiveStop::RefinementFailure);
                rounds.push(trace);
                return Ok(finish(
                    retained,
                    assessment,
                    usage,
                    rounds,
                    RecursiveStop::RefinementFailure,
                ));
            }
            Err(error) => {
                trace.error = Some(error);
                trace.stop = Some(RecursiveStop::RefinementFailure);
                rounds.push(trace);
                return Ok(finish(
                    retained,
                    assessment,
                    usage,
                    rounds,
                    RecursiveStop::RefinementFailure,
                ));
            }
        };
        if history.contains(&proposed) {
            trace.next_queries = Some(proposed);
            trace.stop = Some(RecursiveStop::RepeatedQuery);
            rounds.push(trace);
            return Ok(finish(
                retained,
                assessment,
                usage,
                rounds,
                RecursiveStop::RepeatedQuery,
            ));
        }
        let proposed_bound = proposed
            .iter()
            .try_fold(0u64, |sum, query| {
                sum.checked_add(query.0.as_str().len() as u64)
            })
            .and_then(|bytes| usage.tokens.checked_add(bytes));
        if proposed_bound.is_none_or(|total| total > budget.tokens.0) {
            assessment = assess(&reviews, true);
            trace.assessment = assessment.clone();
            trace.next_queries = Some(proposed);
            trace.stop = Some(RecursiveStop::BudgetExhausted);
            rounds.push(trace);
            return Ok(finish(
                retained,
                assessment,
                usage,
                rounds,
                RecursiveStop::BudgetExhausted,
            ));
        }
        active = active.with_queries(proposed.clone())?;
        trace.next_queries = Some(proposed);
        rounds.push(trace);
    }
}

fn finish(
    retained: BTreeMap<gateway_domain::ReferenceId, RetrievalResult>,
    assessment: SufficiencyAssessment,
    usage: BudgetUsage,
    rounds: Vec<RoundTrace>,
    stop: RecursiveStop,
) -> RecursiveOutcome {
    RecursiveOutcome {
        results: retained.into_values().collect(),
        assessment,
        usage,
        rounds,
        stop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_domain::*;
    use std::{cell::Cell, num::NonZeroU64};

    fn text(value: &str) -> NonEmptyText {
        NonEmptyText::new(value).unwrap()
    }
    fn n(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }
    fn plan(rounds: u64, tokens: u64) -> RetrievalPlan {
        let input = RetrievalRequestInput {
            version: RetrievalVersion::V2,
            scope: ContextScopeId::new("scope").unwrap(),
            provenance: ProvenanceId::new("request").unwrap(),
            situation: None,
            step: None,
            purpose: RetrievalPurpose::TaskKnowledge,
            required: RequiredInformation {
                description: text("two independently validated links"),
                requirements: InformationRequirements::new(
                    FreshnessRequirement::Fresh,
                    None,
                    vec![
                        EvidenceId::new("one").unwrap(),
                        EvidenceId::new("two").unwrap(),
                    ],
                    vec![],
                )
                .unwrap(),
                accepted_trust: BTreeSet::from([TrustClass::RetrievedContent]),
                maximum_sensitivity: SensitivityClass::Public,
            },
            queries: BTreeSet::from([RetrievalQuery(text("first"))]),
            sources: vec![RetrievalSource {
                priority: 0,
                id: RetrievalSourceId::new("repo").unwrap(),
                kind: RetrievalSourceKind::Document,
                optional: false,
            }],
            strategies: vec![RetrievalStrategy {
                priority: 0,
                id: RetrievalStrategyId::new("lexical").unwrap(),
                kind: RetrievalStrategyKind::Lexical,
                optional: false,
            }],
            budget: RetrievalBudget {
                results: ResultBudget(n(4)),
                rounds: RoundBudget(n(rounds)),
                latency: LatencyBudget(n(100)),
                cost: CostBudget {
                    maximum: 0,
                    unit: text("unit"),
                },
                tokens: TokenBudget(tokens),
                context: ContextBudget::new(
                    TokenBudget(1000),
                    BTreeMap::from([(ContextBudgetClass::Knowledge, TokenBudget(1000))]),
                )
                .unwrap(),
            },
            stop: Some(StopCondition::BudgetExhausted),
        };
        let support = RetrievalSupport {
            sources: input
                .sources
                .iter()
                .map(|s| (s.id.clone(), s.kind))
                .collect(),
            strategies: input
                .strategies
                .iter()
                .map(|s| (s.id.clone(), s.kind))
                .collect(),
        };
        RetrievalPlan::new(
            RetrievalPlanId::new("plan").unwrap(),
            RetrievalRequest::new(input).unwrap(),
            &support,
        )
        .unwrap()
    }
    fn usage() -> BudgetUsage {
        BudgetUsage {
            results: 0,
            rounds: 0,
            elapsed_ms: 0,
            cost: 0,
            cost_unit: text("unit"),
            tokens: 0,
            context: BTreeMap::new(),
        }
    }
    struct Spy {
        calls: Cell<u64>,
        duplicate: bool,
    }
    impl KnowledgeRetrievalPort for Spy {
        fn retrieve(
            &self,
            plan: &RetrievalPlan,
            round: RetrievalRound,
            previous: &BudgetUsage,
        ) -> Result<RetrievalBatch, RetrievalError> {
            self.calls.set(self.calls.get() + 1);
            let number = if self.duplicate { 1 } else { round.0.get() };
            let id = if number == 1 { "one" } else { "two" };
            let fragment = RetrievedFragment {
                id: ReferenceId::new(format!("fragment-{id}")).unwrap(),
                scope: plan.request().input().scope.clone(),
                content: text("content"),
                provenance: Provenance::new(
                    ProvenanceId::new("origin").unwrap(),
                    SourceKind::Repository,
                    SourceId::new("repo").unwrap(),
                    "doc",
                )
                .unwrap(),
                snapshot: ContentDigest::new("a".repeat(64)).unwrap(),
                quality: QualityMetadata::new(
                    TrustClass::RetrievedContent,
                    SensitivityClass::Public,
                    Confidence::Unknown,
                    FreshnessStatus::Fresh,
                    Uncertainty::None,
                ),
                evidence: BTreeSet::from([EvidenceId::new(id).unwrap()]),
            };
            let result = RetrievalResult {
                fragment,
                source: RetrievalSourceId::new("repo").unwrap(),
                strategy: RetrievalStrategyId::new("lexical").unwrap(),
                score: 100,
            };
            let next = BudgetUsage {
                results: previous.results + 1,
                rounds: round.0.get(),
                elapsed_ms: previous.elapsed_ms + 1,
                cost: previous.cost,
                cost_unit: previous.cost_unit.clone(),
                tokens: previous.tokens + 10,
                context: BTreeMap::from([(
                    ContextBudgetClass::Knowledge,
                    previous.results * 7 + 7,
                )]),
            };
            let last = round.0.get() == plan.request().input().budget.rounds.0.get();
            RetrievalBatch::new(
                RetrievalBatchInput {
                    version: plan.version(),
                    plan: plan.id().clone(),
                    scope: plan.request().input().scope.clone(),
                    round,
                    status: if last {
                        RetrievalStatus::Complete
                    } else {
                        RetrievalStatus::Partial
                    },
                    reason: if last {
                        RetrievalReason::BudgetReached
                    } else {
                        RetrievalReason::MoreInformationNeeded
                    },
                    results: vec![result.clone()],
                    usage: next,
                    explanations: BTreeSet::from([RetrievalExplanation {
                        target: RetrievalExplanationTarget::Result(result.fragment.id.clone()),
                        selected: true,
                        reason: RetrievalReason::Relevant,
                        detail: text("matched query"),
                    }]),
                },
                plan,
            )
        }
    }
    struct Validator;
    impl RetrievalEvidencePort for Validator {
        fn validate(&self, result: &RetrievalResult) -> Result<EvidenceReview, RetrievalError> {
            Ok(EvidenceReview {
                validated_evidence: result.fragment.evidence.clone(),
                contaminated: false,
            })
        }
    }
    struct Refiner {
        repeated: bool,
    }
    impl QueryRefinementPort for Refiner {
        fn refine(&self, trace: &RoundTrace) -> Result<BTreeSet<RetrievalQuery>, RetrievalError> {
            Ok(if self.repeated {
                trace.queries.clone()
            } else {
                BTreeSet::from([RetrievalQuery(text(&format!(
                    "query-{}",
                    trace.round.0.get()
                )))])
            })
        }
    }
    #[test]
    fn two_rounds_satisfy_without_resetting_usage_or_scope() {
        let spy = Spy {
            calls: Cell::new(0),
            duplicate: false,
        };
        let outcome = retrieve_until_sufficient(
            &plan(3, 100),
            usage(),
            &spy,
            &Validator,
            Some(&Refiner { repeated: false }),
            1,
        )
        .unwrap();
        assert_eq!(outcome.stop, RecursiveStop::Sufficient);
        assert_eq!(spy.calls.get(), 2);
        assert_eq!(
            (
                outcome.usage.rounds,
                outcome.usage.results,
                outcome.usage.tokens
            ),
            (2, 2, 20)
        );
        assert_eq!(
            outcome.rounds[0]
                .next_queries
                .as_ref()
                .unwrap()
                .iter()
                .next()
                .unwrap()
                .0
                .as_str(),
            "query-1"
        );
        assert!(outcome.rounds[1].assessment.missing_evidence.is_empty());
    }
    #[test]
    fn duplicate_evidence_and_repeated_queries_cannot_claim_sufficiency() {
        let spy = Spy {
            calls: Cell::new(0),
            duplicate: true,
        };
        let outcome = retrieve_until_sufficient(
            &plan(3, 100),
            usage(),
            &spy,
            &Validator,
            Some(&Refiner { repeated: false }),
            0,
        )
        .unwrap();
        assert_eq!(outcome.stop, RecursiveStop::NoProgress);
        assert_eq!(spy.calls.get(), 2);
        assert_eq!(outcome.assessment.state, SufficiencyFinding::Partial);
        let spy = Spy {
            calls: Cell::new(0),
            duplicate: true,
        };
        let outcome = retrieve_until_sufficient(
            &plan(3, 100),
            usage(),
            &spy,
            &Validator,
            Some(&Refiner { repeated: true }),
            3,
        )
        .unwrap();
        assert_eq!(outcome.stop, RecursiveStop::RepeatedQuery);
        assert_eq!(spy.calls.get(), 1);
    }
    #[test]
    fn zero_and_overflowed_token_reservations_never_dispatch() {
        for tokens in [0, u64::MAX] {
            let spy = Spy {
                calls: Cell::new(0),
                duplicate: false,
            };
            let mut prior = usage();
            if tokens == u64::MAX {
                prior.tokens = u64::MAX;
            }
            let outcome = retrieve_until_sufficient(
                &plan(3, tokens),
                prior,
                &spy,
                &Validator,
                Some(&Refiner { repeated: false }),
                1,
            )
            .unwrap();
            assert_eq!(outcome.stop, RecursiveStop::BudgetExhausted);
            assert_eq!(spy.calls.get(), 0);
        }
        let spy = Spy {
            calls: Cell::new(0),
            duplicate: false,
        };
        let outcome = retrieve_until_sufficient(
            &plan(3, 16),
            usage(),
            &spy,
            &Validator,
            Some(&Refiner { repeated: false }),
            1,
        )
        .unwrap();
        assert_eq!(outcome.stop, RecursiveStop::BudgetExhausted);
        assert_eq!(spy.calls.get(), 1);
        assert_eq!(outcome.rounds[0].next_queries.as_ref().unwrap().len(), 1);
    }

    struct FailingPort(Cell<u64>);
    impl KnowledgeRetrievalPort for FailingPort {
        fn retrieve(
            &self,
            _: &RetrievalPlan,
            _: RetrievalRound,
            _: &BudgetUsage,
        ) -> Result<RetrievalBatch, RetrievalError> {
            self.0.set(self.0.get() + 1);
            Err(RetrievalError::ServiceUnavailable)
        }
    }
    struct InterruptedPort(RetrievalError);
    impl KnowledgeRetrievalPort for InterruptedPort {
        fn retrieve(
            &self,
            _: &RetrievalPlan,
            _: RetrievalRound,
            _: &BudgetUsage,
        ) -> Result<RetrievalBatch, RetrievalError> {
            Err(self.0)
        }
        fn retrieve_measured(
            &self,
            _: &RetrievalPlan,
            round: RetrievalRound,
            previous: &BudgetUsage,
        ) -> Result<RetrievalBatch, crate::ports::outbound::RetrievalAttemptFailure> {
            let mut usage = previous.clone();
            usage.rounds = round.0.get();
            usage.elapsed_ms += 17;
            usage.cost += 2;
            usage.tokens += 11;
            Err(crate::ports::outbound::RetrievalAttemptFailure {
                error: self.0,
                usage,
            })
        }
    }
    struct InvalidMeasurement;
    impl KnowledgeRetrievalPort for InvalidMeasurement {
        fn retrieve(
            &self,
            _: &RetrievalPlan,
            _: RetrievalRound,
            _: &BudgetUsage,
        ) -> Result<RetrievalBatch, RetrievalError> {
            Err(RetrievalError::Cancelled)
        }
        fn retrieve_measured(
            &self,
            _: &RetrievalPlan,
            _: RetrievalRound,
            previous: &BudgetUsage,
        ) -> Result<RetrievalBatch, crate::ports::outbound::RetrievalAttemptFailure> {
            Err(crate::ports::outbound::RetrievalAttemptFailure {
                error: RetrievalError::Cancelled,
                usage: previous.clone(),
            })
        }
    }
    struct FailingValidator;
    impl RetrievalEvidencePort for FailingValidator {
        fn validate(&self, _: &RetrievalResult) -> Result<EvidenceReview, RetrievalError> {
            Err(RetrievalError::ServiceUnavailable)
        }
    }
    struct Unverified;
    impl RetrievalEvidencePort for Unverified {
        fn validate(&self, _: &RetrievalResult) -> Result<EvidenceReview, RetrievalError> {
            Ok(EvidenceReview {
                validated_evidence: BTreeSet::new(),
                contaminated: false,
            })
        }
    }
    struct BadRefiner(bool);
    impl QueryRefinementPort for BadRefiner {
        fn refine(&self, _: &RoundTrace) -> Result<BTreeSet<RetrievalQuery>, RetrievalError> {
            if self.0 {
                Err(RetrievalError::InvalidPlan)
            } else {
                Ok(BTreeSet::new())
            }
        }
    }
    #[test]
    fn failures_consume_attempts_and_stop_without_a_second_dispatch() {
        let failed = FailingPort(Cell::new(0));
        let outcome = retrieve_until_sufficient(
            &plan(3, 100),
            usage(),
            &failed,
            &Validator,
            Some(&Refiner { repeated: false }),
            1,
        )
        .unwrap();
        assert_eq!(outcome.stop, RecursiveStop::AdapterFailure);
        assert_eq!(outcome.usage.rounds, 1);
        assert_eq!(
            outcome.rounds[0].error,
            Some(RetrievalError::ServiceUnavailable)
        );
        assert_eq!(failed.0.get(), 1);

        let spy = Spy {
            calls: Cell::new(0),
            duplicate: false,
        };
        let outcome = retrieve_until_sufficient(
            &plan(3, 100),
            usage(),
            &spy,
            &FailingValidator,
            Some(&Refiner { repeated: false }),
            1,
        )
        .unwrap();
        assert_eq!(outcome.stop, RecursiveStop::EvidenceUnavailable);
        assert_eq!(outcome.usage.rounds, 1);
        assert_eq!(spy.calls.get(), 1);
        for interruption in [RetrievalError::TimedOut, RetrievalError::Cancelled] {
            let outcome = retrieve_until_sufficient(
                &plan(3, 100),
                usage(),
                &InterruptedPort(interruption),
                &Validator,
                None,
                1,
            )
            .unwrap();
            assert_eq!(outcome.stop, RecursiveStop::AdapterFailure);
            assert_eq!(
                (
                    outcome.usage.rounds,
                    outcome.usage.elapsed_ms,
                    outcome.usage.cost,
                    outcome.usage.tokens
                ),
                (1, 17, 2, 11)
            );
            assert_eq!(outcome.rounds[0].error, Some(interruption));
        }
        let outcome = retrieve_until_sufficient(
            &plan(3, 100),
            usage(),
            &InvalidMeasurement,
            &Validator,
            None,
            1,
        )
        .unwrap();
        assert_eq!(outcome.stop, RecursiveStop::AdapterFailure);
        assert_eq!(outcome.usage.rounds, 1);
        assert_eq!(outcome.rounds[0].error, Some(RetrievalError::InvalidResult));
    }
    #[test]
    fn absent_or_invalid_refinement_and_terminal_budget_are_explicit() {
        let spy = Spy {
            calls: Cell::new(0),
            duplicate: false,
        };
        let outcome =
            retrieve_until_sufficient(&plan(3, 100), usage(), &spy, &Validator, None, 1).unwrap();
        assert_eq!(outcome.stop, RecursiveStop::NoRefinement);
        for bad in [true, false] {
            let outcome = retrieve_until_sufficient(
                &plan(3, 100),
                usage(),
                &spy,
                &Validator,
                Some(&BadRefiner(bad)),
                1,
            )
            .unwrap();
            assert_eq!(outcome.stop, RecursiveStop::RefinementFailure);
            assert_eq!(outcome.rounds[0].error, Some(RetrievalError::InvalidPlan));
        }
        let outcome = retrieve_until_sufficient(
            &plan(1, 100),
            usage(),
            &spy,
            &Unverified,
            Some(&Refiner { repeated: false }),
            1,
        )
        .unwrap();
        assert_eq!(outcome.stop, RecursiveStop::BudgetExhausted);
        assert!(
            outcome
                .assessment
                .findings
                .contains(&SufficiencyFinding::BudgetExhausted)
        );
    }
    #[test]
    fn unverified_links_cannot_advance_a_claim() {
        let spy = Spy {
            calls: Cell::new(0),
            duplicate: false,
        };
        let outcome = retrieve_until_sufficient(
            &plan(3, 100),
            usage(),
            &spy,
            &Unverified,
            Some(&Refiner { repeated: false }),
            0,
        )
        .unwrap();
        assert_eq!(outcome.stop, RecursiveStop::NoProgress);
        assert_eq!(outcome.assessment.state, SufficiencyFinding::Insufficient);
        assert_eq!(spy.calls.get(), 1);
    }
    struct TerminalPort {
        calls: Cell<u64>,
        wrong_plan: bool,
    }
    impl KnowledgeRetrievalPort for TerminalPort {
        fn retrieve(
            &self,
            plan: &RetrievalPlan,
            round: RetrievalRound,
            previous: &BudgetUsage,
        ) -> Result<RetrievalBatch, RetrievalError> {
            self.calls.set(self.calls.get() + 1);
            let alternate;
            let selected = if self.wrong_plan {
                let input = plan.request().input();
                let support = RetrievalSupport {
                    sources: input
                        .sources
                        .iter()
                        .map(|s| (s.id.clone(), s.kind))
                        .collect(),
                    strategies: input
                        .strategies
                        .iter()
                        .map(|s| (s.id.clone(), s.kind))
                        .collect(),
                };
                alternate = RetrievalPlan::new(
                    RetrievalPlanId::new("other").unwrap(),
                    plan.request().clone(),
                    &support,
                )
                .unwrap();
                &alternate
            } else {
                plan
            };
            let mut usage = previous.clone();
            usage.rounds = round.0.get();
            usage.tokens += 5;
            RetrievalBatch::new(
                RetrievalBatchInput {
                    version: selected.version(),
                    plan: selected.id().clone(),
                    scope: selected.request().input().scope.clone(),
                    round,
                    status: RetrievalStatus::Complete,
                    reason: RetrievalReason::NoMatches,
                    results: Vec::new(),
                    usage,
                    explanations: BTreeSet::new(),
                },
                selected,
            )
        }
    }
    #[test]
    fn terminal_and_misidentified_batches_never_start_another_round() {
        for wrong_plan in [false, true] {
            let port = TerminalPort {
                calls: Cell::new(0),
                wrong_plan,
            };
            let outcome = retrieve_until_sufficient(
                &plan(3, 100),
                usage(),
                &port,
                &Validator,
                Some(&Refiner { repeated: false }),
                3,
            )
            .unwrap();
            assert_eq!(port.calls.get(), 1);
            assert_eq!(outcome.usage.rounds, 1);
            assert_eq!(
                outcome.stop,
                if wrong_plan {
                    RecursiveStop::AdapterFailure
                } else {
                    RecursiveStop::TerminalBatch
                }
            );
            assert_eq!(
                outcome.rounds[0].error,
                wrong_plan.then_some(RetrievalError::InvalidResult)
            );
        }
    }
}
