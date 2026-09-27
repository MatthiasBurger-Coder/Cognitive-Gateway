//! Token bounded selection before the existing authorized CG-10 compilation.
use crate::{
    context_application::{
        CompileStepInput, CompiledStep, ContextApplication, ContextApplicationError,
    },
    ports::outbound::{ContextTokenEstimateRequest, TokenEstimatorPort},
};
use gateway_context::ContextDisclosurePolicy;
use gateway_context::budgeted::{
    BudgetedSelection, CompactedCandidate, RankedFragment, SelectionError, fragment_budget_class,
    select_context_with_exclusions,
};
use gateway_domain::{
    ContextBudget, ContextBudgetClass, NonEmptyText, RetrievalError, TokenCount, TokenEstimate,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetedCompileError {
    Compilation(ContextApplicationError),
    Estimator(RetrievalError),
    Selection(SelectionError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetedCompiledStep {
    pub step: CompiledStep,
    pub selection: BudgetedSelection,
    pub section_estimates: BTreeMap<ContextBudgetClass, TokenEstimate>,
    pub total_estimate: TokenEstimate,
}

/// Host-authenticated selection decisions; excluded IDs cannot be compacted.
pub struct ContextSelectionPolicy<'a> {
    pub required: &'a BTreeSet<gateway_domain::ReferenceId>,
    pub excluded: &'a BTreeSet<gateway_domain::ReferenceId>,
}
fn estimate_json(estimate: &TokenEstimate) -> serde_json::Value {
    let count = match &estimate.count {
        TokenCount::Exact { tokens, target } => {
            serde_json::json!({"kind": "exact", "tokens": tokens, "target": target.as_str()})
        }
        TokenCount::Estimated {
            tokens,
            upper_bound,
            semantics,
        } => {
            serde_json::json!({"kind": "estimated", "tokens": tokens, "upper_bound": upper_bound, "semantics": semantics.as_str()})
        }
        TokenCount::Unknown { reason } => {
            serde_json::json!({"kind": "unknown", "reason": reason.as_str()})
        }
    };
    serde_json::json!({"estimator": estimate.estimator.as_str(), "version": estimate.version.as_str(), "count": count})
}
impl BudgetedCompiledStep {
    /// Carries the trace with the semantic envelope; compacted source IDs never
    /// disappear from the handoff even when their original bytes were omitted.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        self.serialize(None)
    }
    pub fn to_json_with_policy(
        &self,
        policy: ContextDisclosurePolicy,
    ) -> Result<String, serde_json::Error> {
        self.serialize(Some(policy))
    }
    fn serialize(
        &self,
        policy: Option<ContextDisclosurePolicy>,
    ) -> Result<String, serde_json::Error> {
        let step = match policy {
            Some(policy) => self.step.to_json_with_policy(policy)?,
            None => self.step.to_json()?,
        };
        let mut value: serde_json::Value = serde_json::from_str(&step)?;
        value["context_selection"] = serde_json::json!({
            "usage": self.selection.usage.iter().map(|(class, tokens)|
                (format!("{class:?}"), *tokens)).collect::<BTreeMap<_, _>>(),
            "decisions": self.selection.decisions.iter().map(|decision|
                serde_json::json!({"id": decision.id.as_str(), "reason": decision.reason.as_str()})).collect::<Vec<_>>(),
            "lineage": self.selection.lineage.iter().map(|(id, sources)|
                (id.as_str(), sources.iter().map(gateway_domain::ReferenceId::as_str).collect::<Vec<_>>())).collect::<BTreeMap<_, _>>(),
            "estimates": self.selection.estimates.iter().map(|(id, estimate)|
                (id.as_str(), estimate_json(estimate))).collect::<BTreeMap<_, _>>(),
            "section_estimates": self.section_estimates.iter().map(|(class, estimate)|
                (format!("{class:?}"), estimate_json(estimate))).collect::<BTreeMap<_, _>>(),
            "total_estimate": estimate_json(&self.total_estimate),
        });
        if policy.is_some_and(|policy| !policy.include_external_content) {
            for field in ["estimates", "section_estimates", "total_estimate"] {
                value["context_selection"][field] =
                    serde_json::json!({"representation":"redacted"});
            }
        }
        serde_json::to_string(&value)
    }
}

/// The estimator counts the semantic JSON sections for the chosen target. A
/// renderer must independently enforce its own final provider prompt limit.
pub fn compile_budgeted_step(
    input: CompileStepInput<'_>,
    budget: &ContextBudget,
    target: &NonEmptyText,
    ranked: &[RankedFragment],
    compacted: &[CompactedCandidate],
    required: &BTreeSet<gateway_domain::ReferenceId>,
    estimator: &dyn TokenEstimatorPort,
) -> Result<BudgetedCompiledStep, BudgetedCompileError> {
    compile_budgeted_step_with_exclusions(
        input,
        budget,
        target,
        ranked,
        compacted,
        ContextSelectionPolicy {
            required,
            excluded: &BTreeSet::new(),
        },
        estimator,
    )
}

/// Applies an authenticated quarantine set before context compilation.
pub fn compile_budgeted_step_with_exclusions(
    input: CompileStepInput<'_>,
    budget: &ContextBudget,
    target: &NonEmptyText,
    ranked: &[RankedFragment],
    compacted: &[CompactedCandidate],
    policy: ContextSelectionPolicy<'_>,
    estimator: &dyn TokenEstimatorPort,
) -> Result<BudgetedCompiledStep, BudgetedCompileError> {
    let empty_candidates = [];
    let empty_selected = BTreeSet::new();
    let baseline = ContextApplication
        .compile_step(CompileStepInput {
            resolved: input.resolved,
            authority: input.authority,
            policy_context: input.policy_context,
            catalog: input.catalog,
            projection: input.projection,
            candidates: &empty_candidates,
            selected: &empty_selected,
        })
        .map_err(BudgetedCompileError::Compilation)?;
    let value: serde_json::Value = serde_json::from_str(
        &baseline
            .to_json()
            .expect("validated semantic envelope serializes"),
    )
    .expect("validated semantic envelope serializes as JSON");
    let scope = &input.projection.mapping.basis.scope;
    let mut sections = BTreeMap::new();
    let sections_to_measure = [
        (
            ContextBudgetClass::AuthorityReserved,
            serde_json::json!({"schema_version": value["schema_version"], "scope": value["scope"], "step": value["step"],
                "stable": value["stable"], "basis": value["basis"], "policy": value["gateway"]["policy"],
                "constraints": value["gateway"]["constraints"], "provenance": value["gateway"]["provenance"],
                "execution_context": value["execution_context"], "dynamic": []}),
        ),
        (
            ContextBudgetClass::TaskReserved,
            serde_json::json!({"task": value["gateway"]["task"], "user_input": value["user_input"]}),
        ),
        (
            ContextBudgetClass::OutputContractReserved,
            value["gateway"]["output_contract"].clone(),
        ),
        (
            ContextBudgetClass::RuntimeState,
            value["gateway"]["runtime_state"].clone(),
        ),
    ];
    for (class, section) in sections_to_measure {
        let content = serde_json::to_string(&section).expect("JSON value serializes");
        let estimate = estimator
            .estimate_context(&ContextTokenEstimateRequest {
                scope,
                target,
                content: &content,
            })
            .map_err(BudgetedCompileError::Estimator)?;
        sections.insert(class, estimate);
    }
    let selection = select_context_with_exclusions(
        budget,
        target,
        &sections,
        ranked,
        compacted,
        policy.required,
        policy.excluded,
    )
    .map_err(BudgetedCompileError::Selection)?;
    let step = ContextApplication
        .compile_step(CompileStepInput {
            resolved: input.resolved,
            authority: input.authority,
            policy_context: input.policy_context,
            catalog: input.catalog,
            projection: input.projection,
            candidates: &selection.fragments,
            selected: &selection.selected,
        })
        .map_err(BudgetedCompileError::Compilation)?;
    for id in policy.required {
        if !step
            .context()
            .fragments()
            .iter()
            .any(|fragment| fragment.id() == id)
        {
            return Err(BudgetedCompileError::Selection(
                SelectionError::MissingMandatory(id.clone()),
            ));
        }
    }
    let mut selection = selection;
    let final_json = step
        .to_json()
        .expect("validated semantic envelope serializes");
    let final_value: serde_json::Value =
        serde_json::from_str(&final_json).expect("validated semantic envelope serializes as JSON");
    for fragment in step.context().fragments() {
        let wire = final_value["dynamic"]
            .as_array()
            .expect("dynamic array")
            .iter()
            .find(|item| item["id"] == fragment.id().as_str())
            .expect("compiled fragment has wire representation");
        let content = serde_json::to_string(wire).expect("JSON value serializes");
        let estimate = estimator
            .estimate_context(&ContextTokenEstimateRequest {
                scope,
                target,
                content: &content,
            })
            .map_err(BudgetedCompileError::Estimator)?;
        let final_bound = match &estimate.count {
            TokenCount::Exact {
                tokens,
                target: actual,
            } if actual == target => *tokens,
            TokenCount::Estimated {
                tokens,
                upper_bound: Some(upper),
                ..
            } if upper >= tokens => *upper,
            _ => {
                return Err(BudgetedCompileError::Selection(
                    SelectionError::InvalidEstimate(fragment.id().clone()),
                ));
            }
        };
        let previous = selection
            .estimates
            .get(fragment.id())
            .expect("selected fragment has estimate")
            .budget_bound()
            .expect("selection validated the estimate");
        if final_bound > previous {
            let class = fragment_budget_class(fragment.kind());
            let count = selection
                .usage
                .get(&class)
                .copied()
                .unwrap_or(0)
                .checked_add(final_bound - previous)
                .ok_or(BudgetedCompileError::Selection(
                    SelectionError::ArithmeticOverflow,
                ))?;
            selection.usage.insert(class, count);
            selection.estimates.insert(fragment.id().clone(), estimate);
        }
    }
    if budget.validate_usage(&selection.usage).is_err() {
        let class = selection
            .usage
            .iter()
            .find(|(class, used)| **used > budget.reservations().get(class).map_or(0, |cap| cap.0))
            .map_or(ContextBudgetClass::AuthorityReserved, |(class, _)| *class);
        return Err(BudgetedCompileError::Selection(
            SelectionError::MandatoryOverBudget(class),
        ));
    }
    let total_estimate = estimator
        .estimate_context(&ContextTokenEstimateRequest {
            scope,
            target,
            content: &final_json,
        })
        .map_err(BudgetedCompileError::Estimator)?;
    let total_bound = match &total_estimate.count {
        TokenCount::Exact {
            tokens,
            target: actual,
        } if actual == target => *tokens,
        TokenCount::Estimated {
            tokens,
            upper_bound: Some(upper),
            ..
        } if upper >= tokens => *upper,
        _ => {
            return Err(BudgetedCompileError::Selection(
                SelectionError::TotalOverBudget,
            ));
        }
    };
    let safety = budget
        .reservations()
        .get(&ContextBudgetClass::SafetyMargin)
        .map_or(0, |v| v.0);
    if total_bound > budget.total().0 - safety {
        return Err(BudgetedCompileError::Selection(
            SelectionError::TotalOverBudget,
        ));
    }
    Ok(BudgetedCompiledStep {
        step,
        selection,
        section_estimates: sections,
        total_estimate,
    })
}
