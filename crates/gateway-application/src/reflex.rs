//! CG-25 deterministic fast path. Only trusted host adapters supply live inputs.
use crate::{
    ScopedObservationBatch,
    closed_loop::{ExecutionRuntimePort, OutcomeStatus},
    context_application::{CompileStepInput, ContextApplication, ContextApplicationError},
    procedure_promotion::{PromotionApplication, PromotionAuthority, PromotionStore},
};
use gateway_domain::{
    learning::{FallbackBehavior, FingerprintSignal, LearnedProcedure, SituationFingerprint},
    procedure_promotion::*,
    *,
};
use gateway_registry::learned_procedures::LearnedProcedureRegistry;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReflexFailure {
    NovelSituation,
    AmbiguousMatch,
    Inactive,
    MissingObservation,
    MissingEvidence,
    StaleEvidence,
    ConflictingEvidence,
    Blocked,
    BindingMismatch,
    ProcessOrPolicyDenied,
    BudgetExhausted,
    ExecutionFailed,
    VerificationFailed,
    RegistryUnavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReflexDisposition {
    Success,
    FullCognitivePath,
    Stopped,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReflexTrace {
    pub stage: String,
    pub step: Option<usize>,
    pub failure: Option<ReflexFailure>,
    pub details: serde_json::Value,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReflexResult {
    pub procedure: Option<ProcedureVersion>,
    pub disposition: ReflexDisposition,
    pub failure: Option<ReflexFailure>,
    pub dispatched: u32,
    pub trace: Vec<ReflexTrace>,
}
impl ReflexResult {
    fn record(&mut self, stage: &str, step: Option<usize>, failure: Option<ReflexFailure>) {
        self.trace.push(ReflexTrace {
            stage: stage.into(),
            step,
            failure,
            details: serde_json::Value::Null,
        });
    }
    fn detail(&mut self, details: serde_json::Value) {
        if let Some(event) = self.trace.last_mut() {
            event.details = details;
        }
    }
    fn fail(&mut self, failure: ReflexFailure, fallback: FallbackBehavior) {
        self.failure = Some(failure);
        // Unproven applicability always returns to the cognitive path.
        self.disposition = if self.dispatched == 0 || fallback == FallbackBehavior::ReturnToPlanner
        {
            ReflexDisposition::FullCognitivePath
        } else {
            ReflexDisposition::Stopped
        };
        let step = self.trace.last().and_then(|event| event.step);
        self.record("FALLBACK", step, Some(failure));
        self.detail(serde_json::json!({"dispatched": self.dispatched, "fallback": fallback}));
    }
}

/// Host-owned bounds. Each dispatch consumes one iteration and one resource unit.
/// Retries consume the same cumulative budgets and require fresh compilation.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ReflexBudget {
    pub max_iterations: u32,
    pub max_retries: u32,
    pub max_resource_units: u32,
    pub max_elapsed_seconds: u64,
    pub max_evidence_age_seconds: u64,
}
/// An authenticated, complete live snapshot, never a model's claimed permission.
/// Fact identities and values are pinned to the current observations by the host.
pub struct ReflexSituation {
    pub fingerprint: SituationFingerprint,
    pub observations: ScopedObservationBatch,
    pub blockers: Vec<ReferenceId>,
}
/// The host runtime must enforce these limits during dispatch, including cancellation.
#[derive(Debug, Clone, Copy)]
pub struct ReflexDispatchLimits {
    pub deadline_unix_seconds: i64,
    pub resource_units: u32,
}
pub trait ReflexRuntime: ExecutionRuntimePort {
    fn execute_bounded(
        &mut self,
        execution: &ReferenceId,
        context: &crate::context_application::CompiledStep,
        limits: ReflexDispatchLimits,
    ) -> crate::closed_loop::ExecutionOutcome;
}
/// Driving adapter: obtain fresh observations and current Process/Policy inputs.
/// Keep this port and promotion authority inaccessible to models/workers.
pub trait ReflexInputs {
    fn situation(&mut self) -> Result<ReflexSituation, ReflexFailure>;
    fn prepare(&self, step: usize) -> Result<CompileStepInput<'_>, ReflexFailure>;
    /// Trusted monotonic Unix time; unknown/regressing time fails closed.
    fn now(&self) -> i64;
}

/// Exact equality intentionally rejects supersets and semantic near matches.
pub fn match_active<'a>(
    registry: &'a LearnedProcedureRegistry,
    fingerprint: &SituationFingerprint,
) -> Result<&'a LearnedProcedure, ReflexFailure> {
    let mut matches = registry.entries().filter(|entry| {
        entry.state() == PromotionState::Active
            && registry.active(entry.procedure().id())
                == Some(&ProcedureVersion::of(entry.procedure()))
            && entry.procedure().fingerprint() == fingerprint
    });
    let first = matches.next().ok_or(ReflexFailure::NovelSituation)?;
    if matches.next().is_some() {
        return Err(ReflexFailure::AmbiguousMatch);
    }
    Ok(first.procedure())
}

/// Source timestamps in this fast path must be explicit Unix seconds. Unknown
/// formats, future times and excessive age are insufficient evidence.
fn fresh(timestamp: Option<&SourceTimestamp>, now: i64, age: u64) -> bool {
    timestamp
        .and_then(|t| t.as_str().parse::<i64>().ok())
        .filter(|t| *t >= 0 && *t <= now)
        .is_some_and(|t| now.abs_diff(t) <= age)
}
fn normalized(batch: &ScopedObservationBatch) -> Result<CurrentState, ReflexFailure> {
    let mut input = NormalizationInput::new(batch.records().clone()).with_required_evidence(true);
    for (subject, metadata) in batch.quality_metadata() {
        input = input.with_quality_metadata(subject.clone(), metadata.clone());
    }
    normalize_current_state(
        ObservedStateId::new("reflex-state").expect("static id"),
        input,
    )
    .map_err(|_| ReflexFailure::ConflictingEvidence)
}
/// Presence, timestamp freshness and supporting lineage are all mandatory.
pub fn evidence_gate(
    procedure: &LearnedProcedure,
    batch: &ScopedObservationBatch,
    now: i64,
    age: u64,
    verification: bool,
) -> Result<(), ReflexFailure> {
    if now < 0 || batch.scope() != procedure.fingerprint().scope() {
        return Err(ReflexFailure::MissingEvidence);
    }
    let records = batch.records();
    if !verification {
        for id in procedure.required_observations() {
            let observation = records
                .observations()
                .iter()
                .find(|o| o.id() == id)
                .ok_or(ReflexFailure::MissingObservation)?;
            if !fresh(observation.occurred_at(), now, age) {
                return Err(ReflexFailure::StaleEvidence);
            }
        }
    }
    let required = if verification {
        procedure.verification_evidence()
    } else {
        procedure.required_evidence()
    };
    for id in required {
        let evidence = records
            .evidence()
            .iter()
            .find(|e| e.id() == id)
            .ok_or(ReflexFailure::MissingEvidence)?;
        if !fresh(evidence.occurred_at(), now, age) {
            return Err(ReflexFailure::StaleEvidence);
        }
        if evidence
            .links()
            .iter()
            .any(|link| link.relation() != EvidenceRelation::Supports)
        {
            return Err(ReflexFailure::ConflictingEvidence);
        }
    }
    for observation in records.observations() {
        if !fresh(observation.occurred_at(), now, age) {
            return Err(ReflexFailure::StaleEvidence);
        }
    }
    let state = normalized(batch)?;
    if state.entries().iter().any(|entry| {
        entry
            .metadata()
            .is_some_and(|metadata| metadata.freshness() != FreshnessStatus::Fresh)
    }) {
        return Err(ReflexFailure::StaleEvidence);
    }
    if state.entries().is_empty()
        || state
            .entries()
            .iter()
            .any(|e| e.status() != StateStatus::Known)
    {
        return Err(ReflexFailure::ConflictingEvidence);
    }
    // Every requested artifact must actually support a normalized known claim.
    if required.iter().any(|id| {
        !state
            .entries()
            .iter()
            .any(|entry| entry.lineage().evidence().contains(id))
    }) {
        return Err(ReflexFailure::MissingEvidence);
    }
    Ok(())
}

pub struct ReflexEngine<A, S> {
    promotion: PromotionApplication<A, S>,
}
impl<A: PromotionAuthority, S: PromotionStore> ReflexEngine<A, S> {
    pub fn new(promotion: PromotionApplication<A, S>) -> Self {
        Self { promotion }
    }
    /// No model, planner or raw capability dispatch occurs inside this service.
    /// Runtime adapters must enforce host cancellation/time/resource limits during
    /// a synchronous dispatch; elapsed limits are also checked before/after it.
    pub fn run(
        &mut self,
        execution: ReferenceId,
        cohort: ReferenceId,
        budget: ReflexBudget,
        inputs: &mut impl ReflexInputs,
        runtime: &mut impl ReflexRuntime,
    ) -> ReflexResult {
        let mut result = ReflexResult {
            procedure: None,
            disposition: ReflexDisposition::Success,
            failure: None,
            dispatched: 0,
            trace: vec![],
        };
        let started = inputs.now();
        let mut last_time = started;
        let mut retries = 0;
        let mut reserved = false;
        let mut outcome_evidence = None;
        let mut fallback = FallbackBehavior::ReturnToPlanner;
        let attempt = (|| -> Result<(), ReflexFailure> {
            let deadline = started
                .checked_add(
                    i64::try_from(budget.max_elapsed_seconds)
                        .map_err(|_| ReflexFailure::BudgetExhausted)?,
                )
                .ok_or(ReflexFailure::BudgetExhausted)?;
            let situation = inputs.situation()?;
            let (_, registry) = self
                .promotion
                .inspect()
                .map_err(|_| ReflexFailure::RegistryUnavailable)?;
            let procedure = match_active(&registry, &situation.fingerprint)?.clone();
            let version = ProcedureVersion::of(&procedure);
            result.procedure = Some(version.clone());
            fallback = procedure.fallback();
            result.record("MATCH_ACTIVE", None, None);
            result.detail(serde_json::json!({"fingerprint": procedure.fingerprint(), "procedure": version,
                "deadline": deadline, "max_iterations": budget.max_iterations, "max_retries": budget.max_retries,
                "max_resource_units": budget.max_resource_units}));
            for (index, declared) in procedure.steps().iter().enumerate() {
                loop {
                    let current = inputs.situation()?;
                    let now = inputs.now();
                    result.record("ATTEMPT", Some(index), None);
                    result.detail(serde_json::json!({"at": now, "iterations": result.dispatched, "retries": retries,
                        "source_snapshot": current.observations.ingestion_key().as_str(), "blockers": current.blockers,
                        "observations": procedure.required_observations(), "evidence": procedure.required_evidence()}));
                    check_budget(budget, started, last_time, now, result.dispatched, retries)?;
                    last_time = now;
                    if current.fingerprint != *procedure.fingerprint() {
                        return Err(ReflexFailure::NovelSituation);
                    }
                    if !current.blockers.is_empty() {
                        return Err(ReflexFailure::Blocked);
                    }
                    evidence_gate(
                        &procedure,
                        &current.observations,
                        now,
                        budget.max_evidence_age_seconds,
                        false,
                    )?;
                    let (_, registry) = self
                        .promotion
                        .inspect()
                        .map_err(|_| ReflexFailure::RegistryUnavailable)?;
                    registry
                        .eligible(
                            &version,
                            procedure.fingerprint().scope(),
                            &cohort,
                            ExecutionMode::Active,
                            now,
                        )
                        .map_err(|_| ReflexFailure::Inactive)?;
                    result.record("APPLICABILITY_AND_EVIDENCE", Some(index), None);
                    result.detail(serde_json::json!({"at": now, "source_snapshot": current.observations.ingestion_key().as_str(),
                        "observations": procedure.required_observations(), "evidence": procedure.required_evidence()}));
                    let input = inputs.prepare(index)?;
                    let snapshot = input.resolved.snapshot.input();
                    let mapping = &input.projection.mapping;
                    if snapshot.scope != *procedure.fingerprint().scope()
                        || snapshot.situation.records() != Some(current.observations.records())
                        || mapping.process.id().as_str() != declared.process().id().as_str()
                        || mapping.process.version().to_string()
                            != declared.process().version().to_string()
                        || mapping.process.digest().as_str() != declared.process().digest().as_str()
                        || input
                            .catalog
                            .workflow(&mapping.workflow)
                            .map(|w| w.policy_id())
                            != Some(declared.policy())
                        || procedure
                            .fingerprint()
                            .signals()
                            .iter()
                            .any(|signal| match signal {
                                FingerprintSignal::OperatingMode(mode) => {
                                    *mode != input.policy_context.operating_mode
                                }
                                FingerprintSignal::Fact(id) => !current
                                    .observations
                                    .records()
                                    .facts()
                                    .iter()
                                    .any(|f| f.id() == id),
                                FingerprintSignal::Capability(id) => {
                                    !input.authority.capabilities.contains_key(id)
                                }
                            })
                    {
                        return Err(ReflexFailure::BindingMismatch);
                    }
                    let step = snapshot
                        .plan
                        .steps()
                        .iter()
                        .find(|s| s.id() == &mapping.step)
                        .ok_or(ReflexFailure::BindingMismatch)?;
                    let requirements: Vec<_> = snapshot
                        .plan
                        .capability_requirements()
                        .iter()
                        .filter(|r| step.capability_requirements().contains(r.id()))
                        .collect();
                    if requirements.len() != 1
                        || requirements[0].capability() != declared.capability()
                    {
                        return Err(ReflexFailure::BindingMismatch);
                    }
                    if input.resolved.snapshot.process().is_none() {
                        return Err(ReflexFailure::ProcessOrPolicyDenied);
                    }
                    let state = normalized(&current.observations)?;
                    if state.entries().iter().any(|entry| {
                        !snapshot
                            .situation
                            .observed_state()
                            .entries()
                            .contains(entry)
                    }) {
                        return Err(ReflexFailure::BindingMismatch);
                    }
                    for prerequisite in step.prerequisites() {
                        condition_evidence(prerequisite, &snapshot.desired, &state)
                            .map_err(|_| ReflexFailure::NovelSituation)?;
                    }
                    let original_desired = &snapshot.desired;
                    let desired = DesiredState::new(
                        original_desired.id().clone(),
                        original_desired.conditions().to_vec(),
                        ConditionExpression::all(
                            std::iter::once(original_desired.expression().clone())
                                .chain(
                                    original_desired
                                        .acceptance_criteria()
                                        .iter()
                                        .map(|c| c.expression().clone()),
                                )
                                .chain(
                                    original_desired
                                        .constraints()
                                        .iter()
                                        .map(|c| c.expression().clone()),
                                )
                                .collect(),
                        )
                        .map_err(|_| ReflexFailure::BindingMismatch)?,
                        original_desired.constraints().to_vec(),
                        original_desired.acceptance_criteria().to_vec(),
                    )
                    .map_err(|_| ReflexFailure::BindingMismatch)?;
                    let conditions = [
                        Some(step.completion().clone()),
                        step.verification().cloned(),
                    ];
                    let compiled =
                        ContextApplication
                            .compile_step(input)
                            .map_err(|error| match error {
                                ContextApplicationError::NotAuthorized(_) => {
                                    ReflexFailure::ProcessOrPolicyDenied
                                }
                                _ => ReflexFailure::BindingMismatch,
                            })?;
                    result.record("PROCESS_POLICY_COMPILED", Some(index), None);
                    result.detail(serde_json::json!({"process": declared.process(), "capability": declared.capability(),
                        "policy": compiled.policy()}));
                    if !reserved {
                        self.promotion
                            .execute(
                                decision(&execution, "reserve"),
                                now,
                                PromotionCommand::ReserveExecution {
                                    procedure: version.clone(),
                                    execution: ExecutionRequest {
                                        id: execution.clone(),
                                        scope: procedure.fingerprint().scope().clone(),
                                        cohort: cohort.clone(),
                                        mode: ExecutionMode::Active,
                                    },
                                },
                            )
                            .map_err(|_| ReflexFailure::RegistryUnavailable)?;
                        reserved = true;
                        result.record("RESERVED", None, None);
                    }
                    let dispatch_started = inputs.now();
                    check_budget(
                        budget,
                        started,
                        last_time,
                        dispatch_started,
                        result.dispatched,
                        retries,
                    )?;
                    last_time = dispatch_started;
                    // Consume before dispatch; a failed attempt never refunds usage.
                    result.dispatched += 1;
                    let dispatch = decision(
                        &execution,
                        &format!("step-{index}-attempt-{}", result.dispatched),
                    );
                    let outcome = runtime.execute_bounded(
                        &dispatch,
                        &compiled,
                        ReflexDispatchLimits {
                            deadline_unix_seconds: deadline,
                            resource_units: 1,
                        },
                    );
                    result.record("DISPATCH", Some(index), None);
                    result.detail(serde_json::json!({"execution": dispatch, "at": dispatch_started, "iterations": result.dispatched,
                        "resource_units": result.dispatched, "retries": retries, "status": outcome.status}));
                    let now = inputs.now();
                    let clock_regressed = now < last_time;
                    if !clock_regressed {
                        last_time = now;
                    }
                    if clock_regressed
                        || now < 0
                        || now.abs_diff(started) >= budget.max_elapsed_seconds
                    {
                        return Err(ReflexFailure::BudgetExhausted);
                    }
                    last_time = now;
                    if outcome.execution != dispatch {
                        return Err(ReflexFailure::VerificationFailed);
                    }
                    match outcome.status {
                        OutcomeStatus::RetryableFailure => {
                            retries = retries
                                .checked_add(1)
                                .ok_or(ReflexFailure::BudgetExhausted)?;
                            result.record("RETRY", Some(index), None);
                            continue;
                        }
                        OutcomeStatus::Completed => {}
                        _ => return Err(ReflexFailure::ExecutionFailed),
                    }
                    let batch = outcome
                        .observations
                        .as_ref()
                        .ok_or(ReflexFailure::VerificationFailed)?;
                    evidence_gate(
                        &procedure,
                        batch,
                        now,
                        budget.max_evidence_age_seconds,
                        true,
                    )
                    .map_err(|_| ReflexFailure::VerificationFailed)?;
                    if batch
                        .records()
                        .observations()
                        .iter()
                        .any(|o| !fresh(o.occurred_at(), now, now.abs_diff(dispatch_started)))
                        || batch
                            .records()
                            .evidence()
                            .iter()
                            .filter(|e| procedure.verification_evidence().contains(e.id()))
                            .any(|e| !fresh(e.occurred_at(), now, now.abs_diff(dispatch_started)))
                    {
                        return Err(ReflexFailure::VerificationFailed);
                    }
                    // Verification cannot reuse the pre-dispatch snapshot.
                    if batch.ingestion_key() == current.observations.ingestion_key() {
                        return Err(ReflexFailure::VerificationFailed);
                    }
                    let state = normalized(batch)?;
                    let mut verification_support = std::collections::BTreeSet::new();
                    for condition in conditions.into_iter().flatten() {
                        verification_support
                            .extend(condition_evidence(&condition, &desired, &state)?);
                    }
                    if !procedure
                        .verification_evidence()
                        .iter()
                        .all(|id| verification_support.contains(id))
                    {
                        return Err(ReflexFailure::VerificationFailed);
                    }
                    if index + 1 == procedure.steps().len()
                        && compare_desired_state(&desired, &state, &ComparisonRules::default())
                            .map_err(|_| ReflexFailure::VerificationFailed)?
                            .outcome()
                            != ComparisonOutcome::Satisfied
                    {
                        return Err(ReflexFailure::VerificationFailed);
                    }
                    outcome_evidence = Some(decision(
                        &dispatch,
                        &format!("verified-{}", batch.ingestion_key().as_str()),
                    ));
                    result.record("VERIFIED", Some(index), None);
                    result.detail(serde_json::json!({"source_snapshot": batch.ingestion_key().as_str(), "at": now,
                        "evidence": verification_support, "verification": outcome_evidence}));
                    break;
                }
            }
            Ok(())
        })();
        if let Err(failure) = attempt {
            result.fail(failure, fallback);
            result.detail(serde_json::json!({"dispatched": result.dispatched, "retries": retries,
                "started_at": started, "observed_at": inputs.now(), "budget": budget, "fallback": fallback}));
        }
        if reserved {
            let outcome = match result.failure {
                None => RuntimeOutcome::Success,
                Some(ReflexFailure::VerificationFailed) => RuntimeOutcome::VerificationFailed,
                Some(ReflexFailure::ExecutionFailed | ReflexFailure::BudgetExhausted) => {
                    RuntimeOutcome::ExecutionFailed
                }
                _ => RuntimeOutcome::Refused,
            };
            if self
                .promotion
                .execute(
                    decision(&execution, "outcome"),
                    last_time,
                    PromotionCommand::RecordOutcome {
                        procedure: result.procedure.clone().expect("reserved procedure"),
                        execution_id: execution.clone(),
                        outcome,
                        evidence: outcome_evidence
                            .unwrap_or_else(|| decision(&execution, "failure-trace")),
                    },
                )
                .is_err()
            {
                result.fail(ReflexFailure::RegistryUnavailable, fallback);
            } else {
                result.record("OUTCOME_RECORDED", None, None);
            }
        }
        result
    }
}
fn decision(execution: &ReferenceId, suffix: &str) -> ReferenceId {
    use sha2::{Digest, Sha256};
    ReferenceId::new(format!(
        "reflex-{:x}",
        Sha256::digest(format!("{execution}/{suffix}"))
    ))
    .expect("digest identifier")
}
fn check_budget(
    budget: ReflexBudget,
    started: i64,
    last: i64,
    now: i64,
    iterations: u32,
    retries: u32,
) -> Result<(), ReflexFailure> {
    if started < 0
        || now < last
        || now.abs_diff(started) >= budget.max_elapsed_seconds
        || iterations >= budget.max_iterations
        || iterations >= budget.max_resource_units
        || retries > budget.max_retries
    {
        Err(ReflexFailure::BudgetExhausted)
    } else {
        Ok(())
    }
}

fn condition_evidence(
    condition: &PlanCondition,
    desired: &DesiredState,
    state: &CurrentState,
) -> Result<Vec<EvidenceId>, ReflexFailure> {
    match condition {
        PlanCondition::DesiredCondition(id) => {
            let comparison = compare_condition(desired, id, state, &ComparisonRules::default())
                .map_err(|_| ReflexFailure::VerificationFailed)?;
            if comparison.outcome() != ComparisonOutcome::Satisfied
                || comparison.trace().evidence().is_empty()
            {
                return Err(ReflexFailure::VerificationFailed);
            }
            Ok(comparison.trace().evidence().to_vec())
        }
        PlanCondition::Outcome(outcome) => {
            let entry = state
                .entries()
                .iter()
                .find(|entry| Some(entry.subject()) == outcome.subject())
                .filter(|entry| {
                    entry.status() == StateStatus::Known && !entry.lineage().evidence().is_empty()
                })
                .ok_or(ReflexFailure::VerificationFailed)?;
            if let Some(expected) = outcome.expected() {
                if entry.value() != Some(expected) {
                    return Err(ReflexFailure::VerificationFailed);
                }
            } else if !matches!(
                outcome.kind(),
                RequiredOutcomeKind::Observation
                    | RequiredOutcomeKind::EvidenceAcquisition
                    | RequiredOutcomeKind::ConflictResolution
            ) {
                return Err(ReflexFailure::VerificationFailed);
            }
            Ok(entry.lineage().evidence().to_vec())
        }
    }
}
