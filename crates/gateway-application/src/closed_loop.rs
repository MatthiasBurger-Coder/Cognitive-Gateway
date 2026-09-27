//! CG-14 bounded orchestration. Runtime claims never substitute for CG-06 evidence.
use crate::{
    DeclarativePlanningApplication, PlanningApplicationError, PlanningCapabilitySnapshot,
    ScopedObservationBatch,
    context_application::{
        CompileStepInput, CompiledStep, ContextApplication, ContextApplicationError,
    },
};
use gateway_domain::*;
use gateway_policy::PolicyDecision;
use serde::Serialize;

/// Immutable planning configuration captured for the lifetime of a run.
#[derive(Debug, Clone)]
pub struct LoopRules {
    pub capabilities: PlanningCapabilitySnapshot,
    pub requirements: CapabilityRequirementRules,
    pub planner: PlannerRules,
    pub max_iterations: u32,
    /// Total attempts without observable goal progress, across all replans.
    pub max_retries: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LoopDecision {
    Continue,
    Replan,
    Pause,
    Success,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LoopReason {
    InitialPlan,
    AssumptionsPreserved,
    ChangedSituation,
    GoalSatisfied,
    MissingEvidence,
    PlanningBlocked,
    AuthorizationUnavailable,
    PolicyDenied,
    HardFailure,
    ExplicitBlocker,
    IterationBudget,
    RetryBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OutcomeStatus {
    Completed,
    RetryableFailure,
    HardFailure,
    Blocked,
}

/// Result of one dispatch. `observations` is a complete replacement snapshot;
/// omitted subjects become unknown. Adapters must not silently mix old reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionOutcome {
    pub execution: ReferenceId,
    pub status: OutcomeStatus,
    pub observations: Option<ScopedObservationBatch>,
}

/// Replaceable outbound adapter. An uncertain transport result must return a
/// blocked outcome, not automatically repeat an operation with side effects.
pub trait ExecutionRuntimePort {
    fn execute(&mut self, execution: &ReferenceId, context: &CompiledStep) -> ExecutionOutcome;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopError {
    Domain(ValidationError),
    Planning(PlanningApplicationError),
    ScopeMismatch,
    NotReady,
    StaleExecution,
    StaleResolution,
    WrongStep,
    Compilation(ContextApplicationError),
}
impl From<ValidationError> for LoopError {
    fn from(value: ValidationError) -> Self {
        Self::Domain(value)
    }
}
impl From<PlanningApplicationError> for LoopError {
    fn from(value: PlanningApplicationError) -> Self {
        Self::Planning(value)
    }
}

/// One evidence-backed assessment. Each revision owns its full lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopAssessment {
    pub document: DeclarativeContextSituationDocument,
    pub delta: Delta,
    pub plan: Option<Plan>,
    pub comparison: ComparisonResult,
    pub diagnostics: Vec<PlannerDiagnostic>,
    pub source: crate::SourceSnapshot,
    pub ingestion_key: crate::IngestionKey,
}

/// Private state prevents callers from clearing budgets or replacing history.
#[derive(Debug)]
pub struct ClosedLoop {
    run_id: ReferenceId,
    scope: ContextScopeId,
    original_intent: Intent,
    intent: Intent,
    rules: LoopRules,
    assessment: LoopAssessment,
    iterations: u32,
    retries: u32,
    revision: u64,
    decision: LoopDecision,
    pending: Option<ReferenceId>,
    audit: Vec<serde_json::Value>,
}

impl ClosedLoop {
    /// Advisory retrieval cannot resume a paused run or authorize a step.
    /// Missing information pauses this run until CG-06 observations and current
    /// policy/process inputs are supplied through their existing boundaries.
    pub fn apply_retrieval_assessment(
        &mut self,
        assessment: &gateway_domain::SufficiencyAssessment,
    ) -> Result<LoopDecision, LoopError> {
        if self.pending.is_some()
            || matches!(self.decision, LoopDecision::Success | LoopDecision::Stopped)
        {
            return Err(LoopError::NotReady);
        }
        self.audit.push(serde_json::json!({"event":"RETRIEVAL_ASSESSMENT",
            "state":format!("{:?}", assessment.state),
            "missing_evidence":assessment.missing_evidence.iter().map(EvidenceId::as_str).collect::<Vec<_>>() }));
        if !assessment
            .findings
            .contains(&SufficiencyFinding::Sufficient)
        {
            self.record(LoopReason::MissingEvidence);
        }
        Ok(self.decision)
    }
    pub fn start(
        run_id: ReferenceId,
        scope: ContextScopeId,
        intent: Intent,
        initial: ScopedObservationBatch,
        rules: LoopRules,
    ) -> Result<Self, LoopError> {
        // Acceptance criteria and invariants participate in the same deterministic Delta.
        let desired = intent.desired_state();
        let expression = ConditionExpression::all(
            std::iter::once(desired.expression().clone())
                .chain(
                    desired
                        .acceptance_criteria()
                        .iter()
                        .map(|c| c.expression().clone()),
                )
                .chain(desired.constraints().iter().map(|c| c.expression().clone()))
                .collect(),
        )?;
        let effective = DesiredState::new(
            desired.id().clone(),
            desired.conditions().to_vec(),
            expression,
            desired.constraints().to_vec(),
            desired.acceptance_criteria().to_vec(),
        )?;
        let mut execution_intent = Intent::new(intent.id().clone(), effective);
        if let Some(original) = intent.original_input() {
            execution_intent = execution_intent.with_original_input(original.clone());
        }
        let assessment = assess(&run_id, &scope, &execution_intent, &initial, &rules, 0)?;
        let mut run = Self {
            run_id,
            scope,
            original_intent: intent,
            intent: execution_intent,
            rules,
            assessment,
            iterations: 0,
            retries: 0,
            revision: 0,
            decision: LoopDecision::Replan,
            pending: None,
            audit: vec![],
        };
        run.choose(LoopReason::InitialPlan);
        Ok(run)
    }

    pub fn assessment(&self) -> &LoopAssessment {
        &self.assessment
    }
    pub fn decision(&self) -> LoopDecision {
        self.decision
    }
    pub fn iterations(&self) -> u32 {
        self.iterations
    }
    pub fn retries(&self) -> u32 {
        self.retries
    }
    pub fn pending_execution(&self) -> Option<&ReferenceId> {
        self.pending.as_ref()
    }
    pub fn audit(&self) -> &[serde_json::Value] {
        &self.audit
    }
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(
            &serde_json::json!({"schema_version":1, "run_id":self.run_id, "scope":self.scope,
            "intent":self.original_intent, "execution_intent": self.intent,
            "max_iterations":self.rules.max_iterations, "max_retries":self.rules.max_retries,
            "pending_execution":self.pending,
            "iterations":self.iterations, "retries":self.retries, "decision":self.decision,
            "capability_snapshot": self.rules.capabilities.identity(),
            "planner_version": self.rules.planner.version().to_string(), "audit":self.audit}),
        )
    }

    /// Resolve against `assessment()` anew and supply current policy/process
    /// inputs on every attempt. Old approvals cannot survive reassessment.
    pub fn execute(
        &mut self,
        input: CompileStepInput<'_>,
        runtime: &mut impl ExecutionRuntimePort,
    ) -> Result<LoopDecision, LoopError> {
        if self.pending.is_some()
            || !matches!(self.decision, LoopDecision::Continue | LoopDecision::Replan)
        {
            return Err(LoopError::NotReady);
        }
        let snapshot = input.resolved.snapshot.input();
        if snapshot.scope != self.scope
            || Some(&snapshot.plan) != self.assessment.plan.as_ref()
            || snapshot.delta != self.assessment.delta
            || snapshot.situation != self.assessment.document
            || snapshot.desired != *self.intent.desired_state()
        {
            return Err(LoopError::StaleResolution);
        }
        let next = snapshot
            .plan
            .topological_steps()?
            .into_iter()
            .find(|s| s.kind() != PlanStepKind::NoOp);
        if next.map(PlanStep::id) != Some(&input.projection.mapping.step) {
            return Err(LoopError::WrongStep);
        }
        let compiled = match ContextApplication.compile_step(input) {
            Ok(compiled) => compiled,
            Err(error) => {
                let reason =
                    if error == ContextApplicationError::NotAuthorized(PolicyDecision::Deny) {
                        LoopReason::PolicyDenied
                    } else {
                        LoopReason::AuthorizationUnavailable
                    };
                self.audit.push(
                    serde_json::json!({"event":"DISPATCH_REJECTED", "revision":self.revision,
                    "plan":snapshot.plan.id().as_str(), "error":format!("{error:?}")}),
                );
                self.record(reason);
                return Err(LoopError::Compilation(error));
            }
        };
        self.iterations += 1;
        let execution = ReferenceId::new(format!("{}-execution-{}", self.run_id, self.iterations))?;
        self.pending = Some(execution.clone());
        self.audit.push(serde_json::json!({"event":"EXECUTION", "execution":execution,
            "revision":self.revision, "context":serde_json::from_str::<serde_json::Value>(&compiled.to_json().expect("compiled context serializes")).expect("compiled JSON")}));
        let outcome = runtime.execute(&execution, &compiled);
        self.ingest(outcome)
    }

    /// Invalid/mismatched results leave the dispatch pending. The caller may
    /// deliver a corrected correlated outcome; dispatch is never repeated here.
    pub fn ingest(&mut self, outcome: ExecutionOutcome) -> Result<LoopDecision, LoopError> {
        if self.pending.as_ref() != Some(&outcome.execution) {
            return Err(LoopError::StaleExecution);
        }
        let next = outcome
            .observations
            .as_ref()
            .map(|batch| {
                assess(
                    &self.run_id,
                    &self.scope,
                    &self.intent,
                    batch,
                    &self.rules,
                    self.revision + 1,
                )
            })
            .transpose()?;
        let mut reason = LoopReason::MissingEvidence;
        if let Some(next) = next {
            let progress = next.delta.actionable_items().len()
                < self.assessment.delta.actionable_items().len();
            if !progress {
                self.retries = self.retries.saturating_add(1);
            }
            reason = if assumptions_preserved(&self.assessment, &next) {
                LoopReason::AssumptionsPreserved
            } else {
                LoopReason::ChangedSituation
            };
            self.assessment = next;
            self.revision += 1;
        }
        self.pending = None;
        self.audit.push(serde_json::json!({"event":"OUTCOME", "execution":outcome.execution,
            "status":outcome.status, "new_revision": self.revision,
            "source":outcome.observations.as_ref().map(|b| serde_json::json!({"source":b.snapshot().source_id(), "ingestion_key":b.ingestion_key().as_str()}))}));
        match outcome.status {
            OutcomeStatus::HardFailure => self.record(LoopReason::HardFailure),
            OutcomeStatus::Blocked => self.record(LoopReason::ExplicitBlocker),
            _ if reason == LoopReason::MissingEvidence => self.record(reason),
            _ => self.choose(reason),
        }
        Ok(self.decision)
    }

    /// Explicit refresh after a pause, from the external evidence boundary.
    /// This does not reset attempt or retry budgets and cannot revive a terminal run.
    pub fn refresh(
        &mut self,
        observations: ScopedObservationBatch,
    ) -> Result<LoopDecision, LoopError> {
        if self.decision != LoopDecision::Pause || self.pending.is_some() {
            return Err(LoopError::NotReady);
        }
        let next = assess(
            &self.run_id,
            &self.scope,
            &self.intent,
            &observations,
            &self.rules,
            self.revision + 1,
        )?;
        self.assessment = next;
        self.revision += 1;
        self.choose(LoopReason::ChangedSituation);
        Ok(self.decision)
    }

    pub fn stop(&mut self) -> Result<(), LoopError> {
        if self.pending.is_some()
            || matches!(self.decision, LoopDecision::Success | LoopDecision::Stopped)
        {
            return Err(LoopError::NotReady);
        }
        self.record(LoopReason::ExplicitBlocker);
        Ok(())
    }

    fn choose(&mut self, reason: LoopReason) {
        let reason = if self.assessment.comparison.outcome() == ComparisonOutcome::Satisfied {
            LoopReason::GoalSatisfied
        } else if self.iterations >= self.rules.max_iterations {
            LoopReason::IterationBudget
        } else if self.retries > self.rules.max_retries {
            LoopReason::RetryBudget
        } else if self.assessment.plan.is_none() {
            LoopReason::PlanningBlocked
        } else {
            reason
        };
        self.record(reason);
    }

    fn record(&mut self, reason: LoopReason) {
        self.decision = match reason {
            LoopReason::GoalSatisfied => LoopDecision::Success,
            LoopReason::HardFailure
            | LoopReason::ExplicitBlocker
            | LoopReason::PolicyDenied
            | LoopReason::IterationBudget
            | LoopReason::RetryBudget => LoopDecision::Stopped,
            LoopReason::MissingEvidence
            | LoopReason::PlanningBlocked
            | LoopReason::AuthorizationUnavailable => LoopDecision::Pause,
            LoopReason::AssumptionsPreserved => LoopDecision::Continue,
            LoopReason::InitialPlan | LoopReason::ChangedSituation => LoopDecision::Replan,
        };
        self.audit.push(serde_json::json!({"event":"DECISION", "revision":self.revision,
            "decision":self.decision, "reason":reason, "iterations":self.iterations, "retries":self.retries,
            "assessment": {"document":serde_json::from_str::<serde_json::Value>(&self.assessment.document.to_json().expect("valid document serializes")).expect("document JSON"),
                "delta":self.assessment.delta, "plan":self.assessment.plan, "goal_outcome":self.assessment.comparison.outcome().as_str(),
                "source": self.assessment.source.source_id(), "ingestion_key":self.assessment.ingestion_key.as_str(),
                "diagnostics":self.assessment.diagnostics.iter().map(|d| serde_json::json!({"code":d.code().as_str(), "delta_item":d.delta_item().map(DeltaItemId::as_str), "blocking":d.is_blocking(), "rationale":d.rationale()})).collect::<Vec<_>>()}}));
    }
}

fn assess(
    run_id: &ReferenceId,
    scope: &ContextScopeId,
    intent: &Intent,
    batch: &ScopedObservationBatch,
    rules: &LoopRules,
    revision: u64,
) -> Result<LoopAssessment, LoopError> {
    if batch.scope() != scope {
        return Err(LoopError::ScopeMismatch);
    }
    let mut normalization = NormalizationInput::new(batch.records().clone())
        .with_required_evidence(true)
        .with_unknown_subjects(
            intent
                .desired_state()
                .conditions()
                .iter()
                .map(|c| c.subject().clone()),
        )?;
    for (subject, metadata) in batch.quality_metadata() {
        normalization = normalization.with_quality_metadata(subject.clone(), metadata.clone());
    }
    let state = normalize_current_state(
        ObservedStateId::new(format!("{run_id}-state-{revision}"))?,
        normalization,
    )?;
    let situation = SituationAssemblyInput::new(state.clone())
        .with_records(batch.records().clone())
        .assemble(SituationId::new(format!("{run_id}-situation-{revision}"))?)?;
    let app = DeclarativePlanningApplication;
    let derivation = app.derive_delta(
        DeltaId::new(format!("{run_id}-delta-{revision}"))?,
        intent.desired_state(),
        &state,
        Some(&situation),
        &ComparisonRules::default().requiring_fresh_evidence(true),
        &DeltaDerivationRules::default(),
    )?;
    let requirements = app.derive_capability_requirements(
        intent.desired_state(),
        derivation.delta(),
        &rules.capabilities,
        &rules.requirements,
    )?;
    let planner = app.build_plan(
        intent.desired_state(),
        derivation.delta(),
        &requirements,
        &rules.planner,
    )?;
    Ok(LoopAssessment {
        document: DeclarativeContextSituationDocument::new(
            DeclarativeContext::new_v1(DeclarativeContextId::new(format!(
                "{run_id}-context-{revision}"
            ))?),
            Some(intent.clone()),
            Some(batch.records().clone()),
            state,
            situation,
        )?,
        delta: derivation.delta().clone(),
        comparison: derivation.comparison().clone(),
        plan: planner.plan().cloned(),
        diagnostics: planner.diagnostics().to_vec(),
        source: batch.snapshot().clone(),
        ingestion_key: batch.ingestion_key(),
    })
}

fn assumptions_preserved(previous: &LoopAssessment, next: &LoopAssessment) -> bool {
    let (Some(old), Some(new)) = (&previous.plan, &next.plan) else {
        return false;
    };
    // Compare the complete remaining step contracts, including dependencies,
    // prerequisites and verification. Snapshot identities are deliberately new.
    new.steps().iter().all(|step| old.steps().contains(step))
        && next.delta.actionable_items().iter().all(|item| {
            previous
                .delta
                .items()
                .iter()
                .any(|old| old.condition() == item.condition() && old.kind() == item.kind())
                && item.basis().state_subjects().iter().all(|subject| {
                    previous
                        .document
                        .observed_state()
                        .entries()
                        .iter()
                        .find(|e| e.subject() == subject)
                        == next
                            .document
                            .observed_state()
                            .entries()
                            .iter()
                            .find(|e| e.subject() == subject)
                })
        })
}
