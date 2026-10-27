//! Actual readonly goal capture and pure, persisted-data verification. No model,
//! mutation, consent grant or invocation identity is created on this route.
use super::{
    current_producer_definitions::CurrentProducerDefinitions,
    target_factory::{ScopedTarget, TargetRead},
    *,
};
use gateway_application::{
    PlanningCapabilitySnapshot,
    closed_loop::{ClosedLoop, LoopRules, VerifiedOutcomeReceipt},
};
use gateway_domain::{
    CapabilityRequirementRules, ConditionId, ContentDigest, PlannerRules, PlanningIrVersion,
};

fn invalid() -> SessionError {
    SessionError::InvalidInteraction
}
fn canonical(value: impl serde::Serialize) -> Result<Vec<u8>, SessionError> {
    serde_json::to_vec(&serde_json::to_value(value).map_err(|_| invalid())?).map_err(|_| invalid())
}
fn pins(
    runtime: &RuntimeAdmission,
    canonical_documents: &[(Pin, Value)],
) -> Result<Vec<Pin>, SessionError> {
    let mut pins = vec![runtime.reference()?];
    pins.extend(runtime.loaded()?.documents.values().map(|d| d.pin.clone()));
    pins.extend(canonical_documents.iter().map(|(p, _)| p.clone()));
    let mut keyed = pins
        .into_iter()
        .map(|p| Ok((serde_json::to_vec(&p).map_err(|_| invalid())?, p)))
        .collect::<Result<Vec<_>, SessionError>>()?;
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.dedup_by(|a, b| a.0 == b.0);
    Ok(keyed.into_iter().map(|(_, p)| p).collect())
}
fn rules(
    runtime: &RuntimeAdmission,
    resolved: &gateway_application::resolution_application::ResolvedPlan,
) -> Result<LoopRules, SessionError> {
    // Runtime.load already authenticated and validated this exact verifier body.
    let verifier: VerifierDocument =
        serde_json::from_value(runtime.loaded()?.document(&runtime.0.verifier)?.clone())
            .map_err(|_| invalid())?;
    if verifier.schema_version != SchemaVersion::V1
        || verifier.scope != runtime.0.scope
        || verifier.goal != runtime.0.goal
        || verifier.max_iterations == 0
    {
        return Err(invalid());
    }
    let capabilities = PlanningCapabilitySnapshot::new(
        resolved.snapshot.input().index.clone(),
        resolved.report.basis.registry_fingerprint.as_str(),
        PlanningIrVersion::V1,
    )
    .map_err(|_| invalid())?;
    Ok(LoopRules {
        capabilities,
        requirements: CapabilityRequirementRules::default(),
        planner: PlannerRules::default(),
        max_iterations: verifier.max_iterations,
        max_retries: verifier.max_retries,
    })
}
/// Read results remain owned and available for exact route reuse within this
/// preparation. They are data, never an authority cache for another callback.
pub(super) struct CapturedGoalReads {
    data: PreExecutionGoalData,
    reads: Vec<(ReadAdmission, TargetRead)>,
}
impl CapturedGoalReads {
    pub(super) fn data(&self) -> &PreExecutionGoalData {
        &self.data
    }
    pub(super) fn reads(&self) -> &[(ReadAdmission, TargetRead)] {
        &self.reads
    }
    pub(super) fn into_candidate(
        self,
        proof: &VerifiedOutcomeReceipt,
    ) -> Result<PreExecutionGoalCandidate, SessionError> {
        PreExecutionGoalCandidate::new(self.data, proof)
    }
}
/// Discovery belongs to the same genuine connector child and must already have
/// completed. Repeated goal conditions reuse an actual identical read, including
/// mapping, source, arguments, normalization and capability; no provider I/O is
/// repeated merely because one source proves several original goal conditions.
pub(super) fn capture(
    target: &mut ScopedTarget,
    checkpoint: &SessionCheckpoint,
) -> Result<(CapturedGoalReads, Option<VerifiedOutcomeReceipt>), SessionError> {
    if checkpoint.snapshot.state != SessionState::Preparing
        || checkpoint.accepted_connector_consent.is_some()
    {
        return Err(SessionError::InvalidState);
    }
    let view = target.context_source().model_inputs_current()?;
    if view.checkpoint() != checkpoint {
        return Err(SessionError::StaleRevision);
    }
    let goal = checkpoint
        .goal
        .as_connector()
        .ok_or(SessionError::UnsupportedCapability)?;
    let runtime = view.runtime().clone();
    let binding = view.pre_execution_goal_binding()?;
    let definitions = pins(&runtime, view.canonical_documents())?;
    let loop_rules = rules(&runtime, view.resolved())?;
    drop(view);
    let mut reads: Vec<(ReadAdmission, TargetRead)> = Vec::new();
    let mut positions = BTreeMap::new();
    let mut conditions = Vec::new();
    for declared in &runtime.0.goal_reads {
        let condition = ConditionId::new(&declared.condition).map_err(|_| invalid())?;
        let key = canonical(&declared.read)?;
        let index = if let Some(index) = positions.get(&key) {
            *index
        } else {
            let read = target.goal_read(&condition)?;
            let index = reads.len();
            reads.push((declared.read.clone(), read));
            positions.insert(key, index);
            index
        };
        conditions.push((condition, index));
    }
    let mut batches = Vec::new();
    let mut ages = Vec::new();
    for (declared, read) in &reads {
        let doc: NormalizationDocument =
            serde_json::from_value(runtime.loaded()?.document(&declared.normalization)?.clone())
                .map_err(|_| invalid())?;
        let ReadSemantics::DirectCurrentReadProfile {
            max_age_seconds, ..
        } = doc.acquisition.semantics
        else {
            return Err(SessionError::UnsupportedCapability);
        };
        batches.push(read.batch()?);
        ages.push(max_age_seconds);
    }
    let inputs = conditions
        .iter()
        .map(|(condition, index)| PreExecutionGoalReadInput {
            condition,
            read_request: reads[*index].1.request(),
            observation: reads[*index].1.observation(),
            batch: &batches[*index],
            max_age_seconds: ages[*index],
        })
        .collect::<Vec<_>>();
    let access = target.context_source().model_access();
    let data = PreExecutionGoalData::new(goal, binding, definitions, &inputs, access.now_ms()?)?;
    let proof = ClosedLoop::verify_retained(
        data.execution()?,
        goal.scope().clone(),
        goal.intent().clone(),
        data.readset(),
        loop_rules,
    )
    .map_err(|_| invalid())?;
    target
        .context_source()
        .validate_context_checkpoint(checkpoint)?;
    // Re-evaluate age after native computation; never renew observed timestamps.
    let final_now = access.now_ms()?;
    if data.reads().iter().any(|read| {
        read.observation.metadata.observed_at.is_none_or(|t| {
            t.seconds() < 0
                || final_now
                    >= (t.seconds() as u64)
                        .saturating_add(read.max_age_seconds)
                        .saturating_mul(1000)
        })
    }) {
        return Err(SessionError::ExpiredInteraction);
    }
    Ok((CapturedGoalReads { data, reads }, proof))
}

/// Reload from the actual currently selected owner-scoped journal. A caller's
/// supplied RAM data cannot attest persistence or current source definitions.
pub(in crate::local_sessions) fn verify(
    access: &LocalSessionAccess,
    checkpoint: &SessionCheckpoint,
    supplied: &PreExecutionGoalData,
) -> Result<Option<VerifiedOutcomeReceipt>, SessionError> {
    let (host, repository, current, definition) = access.current_repository_claim_with_definition(
        &checkpoint.owner,
        &checkpoint.snapshot.session,
        Some(checkpoint.budget.deadline_ms),
    )?;
    if current.checkpoint != *checkpoint || checkpoint.pre_execution_goal.is_none() {
        return Err(SessionError::StaleRevision);
    }
    let actual = load_pre_execution_goal(&repository, checkpoint, access.now_ms()?)?;
    if actual.binding() != supplied.binding()
        || actual.reads() != supplied.reads()
        || actual.definition_pins() != supplied.definition_pins()
        || actual.readset().digest() != supplied.readset().digest()
    {
        return Err(invalid());
    }
    let definitions = CurrentProducerDefinitions::capture(
        access,
        checkpoint,
        &actual.binding().acquisition.step,
    )?;
    let runtime = definitions.runtime();
    if pins(runtime, definitions.canonical_documents())? != actual.definition_pins() {
        return Err(SessionError::StaleRevision);
    }
    let context = definitions.producer_context();
    let execution = context.context().execution_context();
    if execution.task().id() != &actual.binding().task
        || execution.id().as_str() != actual.binding().execution.as_str()
        || gateway_domain::context_content_digest(
            context.to_json().map_err(|_| invalid())?.as_bytes(),
        ) != actual.binding().context_digest
    {
        return Err(SessionError::StaleRevision);
    }
    for read in actual.reads() {
        let declared = runtime
            .0
            .goal_reads
            .iter()
            .find(|r| r.condition == read.condition.as_str())
            .ok_or_else(invalid)?;
        let mapping = runtime
            .loaded()?
            .mapping
            .records()
            .iter()
            .find(|r| r.reference == declared.read.mapping_reference)
            .ok_or_else(invalid)?;
        let request = gateway_application::connectors::ConnectorRequest {
            target: gateway_application::connectors::ConnectorTarget {
                connector: runtime.0.connector.clone(),
                scope: runtime.0.scope.clone(),
            },
            capability: declared.read.capability.clone(),
            class: mapping.class.operation_class(),
            inventory_revision: ContentDigest::new(&read.read_request.inventory_revision)
                .map_err(|_| invalid())?,
            arguments: declared.read.arguments.clone(),
            task: execution.task().id().clone(),
            step: actual.binding().acquisition.step.clone(),
            execution: actual.binding().execution.clone(),
            correlation: NonEmptyText::new(&read.read_request.correlation)
                .map_err(|_| invalid())?,
        };
        let normalization: NormalizationDocument = serde_json::from_value(
            runtime
                .loaded()?
                .document(&declared.read.normalization)?
                .clone(),
        )
        .map_err(|_| invalid())?;
        if ConnectorRequestIdentity::from_request(&request)? != read.read_request
            || declared.read.source != read.observation.source
            || !matches!(normalization.acquisition.semantics,ReadSemantics::DirectCurrentReadProfile {max_age_seconds,..} if max_age_seconds==read.max_age_seconds)
        {
            return Err(SessionError::StaleRevision);
        }
    }
    let goal = checkpoint.goal.as_connector().ok_or_else(invalid)?;
    let outcome = ClosedLoop::verify_retained(
        actual.execution()?,
        goal.scope().clone(),
        goal.intent().clone(),
        actual.readset(),
        rules(runtime, definitions.resolved())?,
    )
    .map_err(|_| invalid())?;
    let (_, _, latest, latest_definition) = access.current_repository_claim_with_definition(
        &checkpoint.owner,
        &checkpoint.snapshot.session,
        Some(checkpoint.budget.deadline_ms),
    )?;
    if latest != current
        || !definition.matches(&latest_definition)
        || repository.load(&checkpoint.owner, &checkpoint.snapshot.session)? != current
    {
        return Err(SessionError::StaleRevision);
    }
    let latest_definitions = CurrentProducerDefinitions::capture(
        access,
        checkpoint,
        &actual.binding().acquisition.step,
    )?;
    if pins(
        latest_definitions.runtime(),
        latest_definitions.canonical_documents(),
    )? != actual.definition_pins()
        || latest_definitions
            .producer_context()
            .to_json()
            .map_err(|_| invalid())?
            != context.to_json().map_err(|_| invalid())?
    {
        return Err(SessionError::StaleRevision);
    }
    // The final native data reload includes the actual current age check.
    load_pre_execution_goal(&repository, checkpoint, access.now_ms()?)?;
    drop(host);
    Ok(outcome)
}

#[cfg(test)]
mod tests;
