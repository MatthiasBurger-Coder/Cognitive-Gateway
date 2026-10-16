//! EPIC-05.12: typed migration boundaries, with no parser or provider dependency.
use gateway_domain::*;

use crate::{
    DeclarativePlanningApplication, PlanningApplicationError, PlanningCapabilitySnapshot,
    resolution_application::{
        DeclarativeResolutionApplication, ResolutionApplicationError, ResolvedPlan,
    },
    resolution_composition::CompositionRules,
    resolution_snapshot::ResolutionSnapshotInput,
};

/// The trusted host authenticates the complete source document against exact
/// reference pins and target association. It also checks the task's mandatory
/// semantic manifest (including output/evidence requirements). Shape alone is
/// insufficient: the capability snapshot must match the pinned declarations.
/// insufficient. No default implementation grants admission or policy authority.
pub trait SemanticPlanningHost: SemanticReferenceValidator {
    fn validate_source(
        &self,
        task: &SemanticTaskIR,
        source: &DeclarativeContextSituationDocument,
        capabilities: &PlanningCapabilitySnapshot,
    ) -> Result<(), ValidationError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticPlanningError {
    InvalidSemantics(ValidationError),
    SourceMismatch,
    MissingDesiredState,
    UnmappedCapability(CapabilityId),
    Planning(PlanningApplicationError),
    PlanningBlocked,
    ResolutionMismatch,
    Resolution(ResolutionApplicationError),
}

/// Map structured CG-06 inputs into compilation data. This is structural
/// migration, not admission: callers must subsequently use `plan_semantic_task`.
/// Explicit semantics are never overwritten when they conflict with CG-06.
pub fn map_cg06_inputs(
    mut data: SemanticTaskData,
    source: &DeclarativeContextSituationDocument,
    context: ResolvedReference<DeclarativeContextId>,
    state: ResolvedReference<ObservedStateId>,
) -> Result<SemanticTaskIR, SemanticPlanningError> {
    if context.id != *source.context().id()
        || state.id != *source.observed_state().id()
        || data.current_state.as_ref().is_some_and(|s| s != &state)
        || data
            .context_refs
            .iter()
            .any(|c| c.id == context.id && c != &context)
    {
        return Err(SemanticPlanningError::SourceMismatch);
    }
    if let Some(intent) = source.intent() {
        if data
            .desired_state
            .as_ref()
            .is_some_and(|d| d != intent.desired_state())
        {
            return Err(SemanticPlanningError::SourceMismatch);
        }
        data.desired_state = Some(intent.desired_state().clone());
    }
    if !data.context_refs.contains(&context) {
        data.context_refs.push(context);
    }
    data.current_state = Some(state);
    SemanticTaskIR::new(data).map_err(SemanticPlanningError::InvalidSemantics)
}

#[derive(Debug, Clone, Default)]
pub struct SemanticPlanningRules {
    pub comparison: ComparisonRules,
    pub delta: DeltaDerivationRules,
    pub capabilities: CapabilityRequirementRules,
    pub planner: PlannerRules,
}

/// Immutable envelope over existing contracts, not a replacement Plan model.
/// Keep this envelope at downstream handoffs: Plan alone cannot represent output,
/// verification, evidence, policy references, assumptions or process pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticPlanHandoff {
    task: SemanticTaskIR,
    source: DeclarativeContextSituationDocument,
    delta: Delta,
    plan: Plan,
    capabilities: PlanningCapabilitySnapshot,
}

impl SemanticPlanHandoff {
    pub fn task(&self) -> &SemanticTaskIR {
        &self.task
    }
    pub fn source(&self) -> &DeclarativeContextSituationDocument {
        &self.source
    }
    pub fn delta(&self) -> &Delta {
        &self.delta
    }
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// Revalidate at the next boundary, then use CG-08's unchanged contracts.
    /// Process selection/applicability remains CG-08 owned; a task process pin
    /// must be explicitly reflected in its existing selection rules.
    pub fn resolve(
        &self,
        host: &impl SemanticPlanningHost,
        input: &ResolutionSnapshotInput,
        rules: &CompositionRules,
    ) -> Result<ResolvedPlan, SemanticPlanningError> {
        validate_source(&self.task, &self.source, &self.capabilities, host)?;
        if input.plan != self.plan
            || input.delta != self.delta
            || Some(&input.desired) != self.task.data().desired_state.as_ref()
            || input.situation != self.source
            || input.scope != self.task.data().target.reference.scope
            || PlanningCapabilitySnapshot::new(
                input.index.clone(),
                self.capabilities.identity(),
                self.capabilities.version(),
            )
            .map_err(SemanticPlanningError::Planning)?
                != self.capabilities
        {
            return Err(SemanticPlanningError::ResolutionMismatch);
        }
        if let Some(process) = &self.task.data().process {
            let matches = rules
                .processes
                .required_definition
                .as_ref()
                .is_some_and(|p| {
                    p.id().as_str() == process.id().as_str()
                        && p.version().value() == process.version()
                        && p.digest().as_str() == process.digest().as_str()
                });
            if !matches {
                return Err(SemanticPlanningError::ResolutionMismatch);
            }
        }
        DeclarativeResolutionApplication
            .resolve_plan(input, rules)
            .map_err(SemanticPlanningError::Resolution)
    }
}

fn validate_source(
    task: &SemanticTaskIR,
    source: &DeclarativeContextSituationDocument,
    capabilities: &PlanningCapabilitySnapshot,
    host: &impl SemanticPlanningHost,
) -> Result<(), SemanticPlanningError> {
    let data = task.data();
    if data.current_state.as_ref().map(|s| &s.id) != Some(source.observed_state().id())
        || !data
            .context_refs
            .iter()
            .any(|c| &c.id == source.context().id())
        || source
            .intent()
            .is_some_and(|i| Some(i.desired_state()) != data.desired_state.as_ref())
    {
        return Err(SemanticPlanningError::SourceMismatch);
    }
    task.validate_references(host)
        .map_err(SemanticPlanningError::InvalidSemantics)?;
    host.validate_source(task, source, capabilities)
        .map_err(SemanticPlanningError::InvalidSemantics)
}

pub fn plan_semantic_task(
    task: &SemanticTaskIR,
    source: &DeclarativeContextSituationDocument,
    host: &impl SemanticPlanningHost,
    delta_id: DeltaId,
    capabilities: &PlanningCapabilitySnapshot,
    rules: &SemanticPlanningRules,
) -> Result<SemanticPlanHandoff, SemanticPlanningError> {
    validate_source(task, source, capabilities, host)?;
    let desired = task
        .data()
        .desired_state
        .as_ref()
        .ok_or(SemanticPlanningError::MissingDesiredState)?;
    let app = DeclarativePlanningApplication;
    let delta = app
        .derive_delta(
            delta_id,
            desired,
            source.observed_state(),
            Some(source.situation()),
            &rules.comparison,
            &rules.delta,
        )
        .map_err(SemanticPlanningError::Planning)?;
    let requirements = app
        .derive_capability_requirements(desired, delta.delta(), capabilities, &rules.capabilities)
        .map_err(SemanticPlanningError::Planning)?;
    // Declarations cannot manufacture Delta lineage or be silently dropped.
    for declared in &task.data().capability_requirements {
        if !requirements
            .requirements()
            .iter()
            .any(|r| r.capability() == &declared.id)
        {
            return Err(SemanticPlanningError::UnmappedCapability(
                declared.id.clone(),
            ));
        }
    }
    let result = app
        .build_plan(desired, delta.delta(), &requirements, &rules.planner)
        .map_err(SemanticPlanningError::Planning)?;
    if !result.is_execution_ready() {
        return Err(SemanticPlanningError::PlanningBlocked);
    }
    let plan = result
        .plan()
        .ok_or(SemanticPlanningError::PlanningBlocked)?;
    Ok(SemanticPlanHandoff {
        task: task.clone(),
        source: source.clone(),
        delta: delta.delta().clone(),
        plan: plan.clone(),
        capabilities: capabilities.clone(),
    })
}
