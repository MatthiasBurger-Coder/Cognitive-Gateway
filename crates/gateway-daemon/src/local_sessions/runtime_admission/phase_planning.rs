//! Pure phase accounting from the caller's freshly authenticated loaded runtime.
//! These are ceilings, never spending cursors, provider availability or grants.
use super::{
    model_documents::{NativeModelDefinitions, PricingDeclaration, read_model},
    *,
};
use gateway_domain::invocation_resilience::InvocationRunLimits;

pub(super) struct PreparationSlices {
    model: SessionResourceAmounts,
    connector: SessionResourceAmounts,
}
impl PreparationSlices {
    pub(super) fn model(&self) -> &SessionResourceAmounts {
        &self.model
    }
    pub(super) fn connector(&self) -> &SessionResourceAmounts {
        &self.connector
    }
}
/// One exact operator plan plus its disjoint preparation children. The caller
/// allocates each child once from the existing coordinator phase cursor. No
/// resource reservation is created here and unused ceilings are not refunded.
pub(super) struct NativePhasePlan {
    resources: SessionResourcePlan,
    preparation: PreparationSlices,
}
impl NativePhasePlan {
    pub(super) fn capture(
        runtime: &RuntimeAdmission,
        step: &PlanStepId,
        cost_unit: &ReferenceId,
    ) -> Result<Self, SessionError> {
        let loaded = runtime.loaded()?;
        let step = runtime
            .0
            .steps
            .iter()
            .find(|s| s.step == step.as_str())
            .ok_or(SessionError::AuthorityDenied)?;
        let ProducerAdmission::Model(model) = &step.producer else {
            return Err(SessionError::UnsupportedCapability);
        };
        let definitions = read_model(&model.model, &loaded.documents)?;
        let quote: QuoteDocument = serde_json::from_value(loaded.document(&step.quotes)?.clone())
            .map_err(|_| invalid())?;
        if quote.schema_version != SchemaVersion::V1
            || quote.manifest != runtime.0.manifest
            || quote.mapping != runtime.0.mapping
            || &quote.cost_unit != cost_unit
            || &definitions.adapter.pricing.cost_unit != cost_unit
        {
            return Err(SessionError::StaleRevision);
        }
        let FixedTariffDocument::FixedLocalNoEmbedding {
            discovery_cost_microunits,
            operation_cost_microunits,
        } = quote.tariff;
        let tariff = ConnectorTariff {
            discovery: discovery_cost_microunits,
            operation: operation_cost_microunits,
        };
        let resources = step.resources.clone();
        let preparation = split(
            &resources,
            tariff,
            ModelBudget::from_definitions(&definitions),
        )?;
        Ok(Self {
            resources,
            preparation,
        })
    }
    pub(super) fn resources(&self) -> &SessionResourcePlan {
        &self.resources
    }
    pub(super) fn preparation(&self) -> &PreparationSlices {
        &self.preparation
    }
}
#[derive(Clone, Copy)]
struct ConnectorTariff {
    discovery: u64,
    operation: u64,
}
impl ConnectorTariff {
    /// The owning composer opens one target and discovers once per phase; the
    /// remaining logical calls use that target. Discovery failure abandons the
    /// slice, rather than secretly adding another discovery/reconnect allowance.
    fn maximum(self, calls: u64) -> Result<u64, SessionError> {
        let operations = calls.checked_sub(1).ok_or(SessionError::LimitExceeded)?;
        self.operation
            .checked_mul(operations)
            .and_then(|n| n.checked_add(self.discovery))
            .ok_or(SessionError::LimitExceeded)
    }
}
struct ModelBudget {
    attempts: u16,
    input: u32,
    output: u32,
    timeout_ms: u32,
    pricing: PricingDeclaration,
}
impl ModelBudget {
    fn from_definitions(d: &NativeModelDefinitions) -> Self {
        Self {
            attempts: d.model.max_attempts,
            input: d.model.input_tokens,
            output: d.generation.data().max_output_tokens,
            timeout_ms: d.model.timeout_ms,
            pricing: d.adapter.pricing.clone(),
        }
    }
}
fn split(
    resources: &SessionResourcePlan,
    tariff: ConnectorTariff,
    budget: ModelBudget,
) -> Result<PreparationSlices, SessionError> {
    // The native owner enforces all model-free phase invariants and checked
    // totals. This adapter only narrows unsupported worker/repair operations.
    resources.validate()?;
    for phase in [
        SessionResourcePhase::Preparation,
        SessionResourcePhase::ApprovedResume,
        SessionResourcePhase::Execution,
        SessionResourcePhase::AfterSnapshotRecovery,
        SessionResourcePhase::GoalVerification,
    ] {
        let a = resources.amounts(phase);
        if a.repair_attempts != 0 || a.worker_attempts != 0 || a.worker_compute_units != 0 {
            return Err(SessionError::UnsupportedCapability);
        }
    }
    if !resources.goal_verification.is_zero() {
        return Err(SessionError::UnsupportedCapability);
    }
    for a in [
        &resources.approved_resume,
        &resources.execution,
        &resources.after_snapshot_recovery,
    ] {
        if !a.is_zero() && a.cost_microunits < tariff.maximum(a.connector_calls)? {
            return Err(SessionError::LimitExceeded);
        }
    }
    let a = &resources.preparation;
    let connector = SessionResourceAmounts {
        connector_calls: a.connector_calls,
        retrieval_rounds: a.retrieval_rounds,
        retrieval_results: a.retrieval_results,
        retrieval_tokens: a.retrieval_tokens,
        cost_microunits: tariff.maximum(a.connector_calls)?,
        ..Default::default()
    };
    let model = a.checked_sub(&connector)?;
    let limits = InvocationRunLimits {
        schema_version: SchemaVersion::V1,
        max_attempts: budget.attempts,
        timeout_ms: budget.timeout_ms,
        invocation_input_tokens: u64::from(budget.input),
        invocation_output_tokens: u64::from(budget.output),
        run_input_tokens: model.input_tokens,
        run_output_tokens: model.output_tokens,
        cost_microunits: model.cost_microunits,
        output_bytes: model.output_bytes,
        reconciliation_tolerance: 0,
    };
    limits.validate().map_err(|_| SessionError::LimitExceeded)?;
    // Attempts and run token totals are independent ceilings. A later attempt
    // can legitimately be denied before I/O by the existing cumulative run.
    if u64::from(budget.attempts) > model.model_attempts
        || u64::from(budget.input) > model.input_tokens
        || u64::from(budget.output) > model.output_tokens
        || budget.pricing.maximum(
            u64::from(budget.input),
            u64::from(budget.output),
            u64::from(budget.attempts),
        )? > model.cost_microunits
    {
        return Err(SessionError::LimitExceeded);
    }
    if model.checked_add(&connector)? != *a {
        return Err(SessionError::InvalidInput);
    }
    Ok(PreparationSlices { model, connector })
}
#[cfg(test)]
mod tests;
