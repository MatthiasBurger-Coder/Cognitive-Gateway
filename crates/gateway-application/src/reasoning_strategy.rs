//! Typed EPIC-06 handoff for an already authorized, bounded semantic step.
use crate::context_budgeting::BudgetedCompiledStep;
use gateway_domain::{
    EvidenceId, ExecutionContextId, ReasoningCapability, ReasoningStrategy,
    ReasoningStrategyContract, ReferenceId, StrategyError, StrategySelection, StrategyUsage,
    SufficiencyAssessment, SufficiencyFinding,
};
use std::collections::BTreeSet;

/// Adapter support is a declared capability matrix, not a permission grant.
pub trait ReasoningAdapter {
    fn supported_strategies(&self) -> BTreeSet<ReasoningStrategy>;
    fn capabilities(&self) -> BTreeSet<ReasoningCapability>;
    fn attempt(
        &self,
        handoff: StrategyHandoff<'_>,
    ) -> Result<StrategyAttempt, StrategyAttemptFailure>;
}

/// Carries the current CG-09/CG-10 compiled step and CG-20B bounded context.
/// The strategy is advisory and cannot modify this authorization or context.
pub struct StrategyHandoff<'a> {
    pub step: &'a BudgetedCompiledStep,
    pub contract: &'a ReasoningStrategyContract,
    pub selection: &'a StrategySelection,
    pub evidence: &'a SufficiencyAssessment,
    pub prior_usage: &'a StrategyUsage,
}

/// Public, referenced result. Verification remains with the evidence/process loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyAttempt {
    pub output_reference: ReferenceId,
    pub provenance_references: BTreeSet<ReferenceId>,
    pub cost_unit: String,
    /// Additional measured usage. The application reserves one iteration first.
    pub usage: StrategyUsage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyAttemptFailure {
    pub reason: String,
    pub cost_unit: String,
    pub usage: StrategyUsage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrategyDispatchError {
    Contract(StrategyError),
    Adapter(String),
    InvalidReport,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyDecision {
    pub selection: StrategySelection,
    pub context_id: ExecutionContextId,
    pub output_reference: ReferenceId,
    pub provenance_references: BTreeSet<ReferenceId>,
    pub validated_evidence: BTreeSet<EvidenceId>,
    pub evidence_findings: BTreeSet<SufficiencyFinding>,
    pub cumulative_usage: StrategyUsage,
}

impl StrategyDecision {
    /// Stable public trace containing references and findings, never prompt or thought text.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&serde_json::json!({
            "version": gateway_domain::REASONING_STRATEGY_VERSION,
            "requested": self.selection.requested,
            "selected": self.selection.selected,
            "fallback_reason": self.selection.fallback_reason,
            "context_id": self.context_id.as_str(),
            "output_reference": self.output_reference.as_str(),
            "provenance_references": self.provenance_references.iter()
                .map(ReferenceId::as_str).collect::<Vec<_>>(),
            "validated_evidence": self.validated_evidence.iter()
                .map(EvidenceId::as_str).collect::<Vec<_>>(),
            "evidence_findings": self.evidence_findings.iter()
                .map(|finding| finding.as_str()).collect::<Vec<_>>(),
            "cumulative_usage": self.cumulative_usage,
        }))
    }
}

/// Pure admission check. It does not dispatch or reserve an iteration.
pub fn select_strategy(
    adapter: &dyn ReasoningAdapter,
    contract: &ReasoningStrategyContract,
    evidence: &SufficiencyAssessment,
) -> Result<StrategySelection, StrategyDispatchError> {
    let selection = contract
        .select(&adapter.supported_strategies(), &adapter.capabilities())
        .map_err(StrategyDispatchError::Contract)?;
    if contract.verification() == gateway_domain::VerificationExpectation::EvidenceBacked
        && (!evidence.findings.contains(&SufficiencyFinding::Sufficient)
            || evidence.findings.iter().any(|finding| {
                matches!(
                    finding,
                    SufficiencyFinding::Conflicting
                        | SufficiencyFinding::Partial
                        | SufficiencyFinding::Insufficient
                        | SufficiencyFinding::Stale
                        | SufficiencyFinding::Untrusted
                        | SufficiencyFinding::Contaminated
                        | SufficiencyFinding::BudgetExhausted
                )
            }))
    {
        return Err(StrategyDispatchError::Contract(
            StrategyError::InsufficientEvidence,
        ));
    }
    Ok(selection)
}

/// Owns cumulative usage across attempts and fallback. A failed call still
/// consumes its reserved iteration and reported measurements.
pub struct StrategySession {
    contract: ReasoningStrategyContract,
    usage: StrategyUsage,
    terminal: bool,
}

impl StrategySession {
    pub fn new(contract: ReasoningStrategyContract) -> Self {
        Self {
            contract,
            usage: StrategyUsage::default(),
            terminal: false,
        }
    }

    pub fn usage(&self) -> &StrategyUsage {
        &self.usage
    }

    pub fn attempt(
        &mut self,
        adapter: &dyn ReasoningAdapter,
        step: &BudgetedCompiledStep,
        evidence: &SufficiencyAssessment,
    ) -> Result<StrategyDecision, StrategyDispatchError> {
        if self.terminal {
            return Err(StrategyDispatchError::Terminal);
        }
        let result = dispatch_strategy(adapter, step, &self.contract, evidence, &mut self.usage);
        if matches!(
            result,
            Err(StrategyDispatchError::InvalidReport)
                | Err(StrategyDispatchError::Contract(
                    StrategyError::BudgetExceeded | StrategyError::ArithmeticOverflow
                ))
        ) {
            self.terminal = true;
        }
        result
    }
}

fn dispatch_strategy(
    adapter: &dyn ReasoningAdapter,
    step: &BudgetedCompiledStep,
    contract: &ReasoningStrategyContract,
    evidence: &SufficiencyAssessment,
    usage: &mut StrategyUsage,
) -> Result<StrategyDecision, StrategyDispatchError> {
    let selection = select_strategy(adapter, contract, evidence)?;
    usage
        .validate(contract.budget())
        .map_err(StrategyDispatchError::Contract)?;
    *usage = usage
        .checked_add(
            &StrategyUsage {
                iterations: 1,
                ..StrategyUsage::default()
            },
            contract.budget(),
        )
        .map_err(StrategyDispatchError::Contract)?;
    let handoff = StrategyHandoff {
        step,
        contract,
        selection: &selection,
        evidence,
        prior_usage: usage,
    };
    let attempt = match adapter.attempt(handoff) {
        Ok(attempt) => attempt,
        Err(failure) => {
            if failure.cost_unit != contract.budget().cost_unit {
                return Err(StrategyDispatchError::InvalidReport);
            }
            *usage = usage
                .checked_add(&failure.usage, contract.budget())
                .map_err(StrategyDispatchError::Contract)?;
            return Err(StrategyDispatchError::Adapter(failure.reason));
        }
    };
    if attempt.cost_unit != contract.budget().cost_unit {
        return Err(StrategyDispatchError::InvalidReport);
    }
    *usage = usage
        .checked_add(&attempt.usage, contract.budget())
        .map_err(StrategyDispatchError::Contract)?;
    if attempt.usage.iterations != 0 {
        return Err(StrategyDispatchError::InvalidReport);
    }
    Ok(StrategyDecision {
        selection,
        context_id: step.step.context().execution_context().id().clone(),
        output_reference: attempt.output_reference,
        provenance_references: attempt.provenance_references,
        validated_evidence: evidence.validated_evidence.clone(),
        evidence_findings: evidence.findings.clone(),
        cumulative_usage: usage.clone(),
    })
}
