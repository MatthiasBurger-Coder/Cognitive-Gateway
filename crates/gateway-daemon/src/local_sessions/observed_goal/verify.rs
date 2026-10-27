//! Pure replay over actual owner-scoped stored reads and current definitions.
use super::{LocalSessionAccess, claim::now_ms, metadata, target};
use gateway_application::{
    closed_loop::{ClosedLoop, VerifiedOutcomeReceipt},
    connectors::{ConnectorRequest, ConnectorTarget},
    sessions::*,
};
use gateway_domain::NonEmptyText;

pub(super) fn retained(
    access: &LocalSessionAccess,
    cp: &SessionCheckpoint,
    supplied: &PreExecutionGoalData,
) -> Result<Option<VerifiedOutcomeReceipt>, SessionError> {
    let (host, defs) = metadata(access, cp)?;
    let repository = access.selected_repository(&host)?;
    let current = repository.load(&cp.owner, &cp.snapshot.session)?;
    if current.checkpoint != *cp || cp.snapshot.state != SessionState::Runnable {
        return Err(SessionError::StaleRevision);
    }
    let actual = load_pre_execution_goal(&repository, cp, now_ms()?)?;
    if actual.binding() != supplied.binding()
        || actual.reads() != supplied.reads()
        || actual.readset().digest() != supplied.readset().digest()
        || actual.definition_pins() != supplied.definition_pins()
    {
        return Err(SessionError::InvalidInteraction);
    }
    let mut pins = defs.definition_pins.clone();
    pins.sort_by_key(|pin| serde_json::to_vec(pin).expect("native pin serializes"));
    let config = host
        .sessions
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    if actual.definition_pins() != pins
        || actual.binding().task != defs.task.data().id
        || actual.binding().acquisition.step != defs.native.step
        || actual.binding().execution.as_str()
            != defs.compiled.context().execution_context().id().as_str()
        || actual.binding().context_digest != super::context_digest(&defs, config.consent_required)?
    {
        return Err(SessionError::StaleRevision);
    }
    for read in actual.reads() {
        let declared = defs
            .runtime
            .goal_reads
            .iter()
            .find(|r| r.condition == read.condition.as_str())
            .ok_or(SessionError::InvalidInteraction)?;
        let mapping = defs
            .mapping
            .records()
            .iter()
            .find(|m| m.reference == declared.read.mapping_reference)
            .ok_or(SessionError::InvalidInteraction)?;
        let request = ConnectorRequest {
            target: ConnectorTarget {
                connector: defs.runtime.connector.clone(),
                scope: defs.runtime.scope.clone(),
            },
            capability: declared.read.capability.clone(),
            class: mapping.class.operation_class(),
            inventory_revision: read.observation.inventory_revision.clone(),
            arguments: declared.read.arguments.clone(),
            task: defs.task.data().id.clone(),
            step: defs.native.step.clone(),
            execution: actual.binding().execution.clone(),
            correlation: NonEmptyText::new(&read.read_request.correlation)
                .map_err(|_| SessionError::InvalidInteraction)?,
        };
        if ConnectorRequestIdentity::from_request(&request)? != read.read_request {
            return Err(SessionError::StaleRevision);
        }
        // Every stored observation remains tied to the exact current registered
        // request and actual pinned normalization profile; no new provider call.
        let normalization = defs
            .normalizations
            .iter()
            .find(|n| {
                n.read.mapping_reference == declared.read.mapping_reference
                    && n.read.normalization == declared.read.normalization
                    && n.read.arguments == declared.read.arguments
                    && n.read.source == declared.read.source
                    && n.read.capability == declared.read.capability
            })
            .ok_or(SessionError::StaleRevision)?;
        let super::definitions::ReadSemantics::DirectCurrentReadProfile {
            max_age_seconds, ..
        } = normalization.document.acquisition.semantics;
        if max_age_seconds != read.max_age_seconds {
            return Err(SessionError::StaleRevision);
        }
        if read.observation.metadata.task != request.task
            || read.observation.metadata.execution != request.execution
            || read.observation.metadata.step != request.step
            || read.observation.source != declared.read.source
        {
            return Err(SessionError::InvalidInteraction);
        }
    }
    let goal = cp.goal.as_connector().ok_or(SessionError::InvalidState)?;
    let proof = ClosedLoop::verify_retained(
        actual.execution()?,
        goal.scope().clone(),
        goal.intent().clone(),
        actual.readset(),
        target::rules(&defs)?,
    )
    .map_err(|_| SessionError::InvalidInteraction)?;
    let latest_host = access.current()?;
    defs.validate_current_metadata(&latest_host)?;
    let latest_repository = access.selected_repository(&latest_host)?;
    if latest_repository.load(&cp.owner, &cp.snapshot.session)? != current
        || repository.load(&cp.owner, &cp.snapshot.session)? != current
        || now_ms()? >= cp.budget.deadline_ms
    {
        return Err(SessionError::StaleRevision);
    }
    // Re-decode age and immutable identity at the end, rather than trusting a
    // Fresh label retained from the original acquisition.
    let latest = load_pre_execution_goal(&repository, cp, now_ms()?)?;
    if latest.readset().digest() != actual.readset().digest() {
        return Err(SessionError::StaleRevision);
    }
    Ok(proof)
}
