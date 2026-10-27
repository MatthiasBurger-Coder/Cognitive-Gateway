//! Convert one genuine atomic root acquisition into complete Before DATA.
use super::{claim::now_ms, source::GoalSource, world_definitions::AtomicBeforeDefinitions};
use gateway_application::{
    ScopedObservationBatch,
    connectors::*,
    sessions::{scoped_snapshots::*, *},
};
use gateway_domain::*;

pub(super) fn capture(
    source: &GoalSource,
    world: &AtomicBeforeDefinitions,
    request: ConnectorRequest,
    observation: ConnectorObservation,
    batch: ScopedObservationBatch,
) -> Result<CapturedBefore, SessionError> {
    let invalid = || SessionError::InvalidInput;
    let acquisition = SnapshotAcquisition {
        request,
        observation,
        batch,
    };
    let reference = acquisition.reference().map_err(|_| invalid())?;
    let observed = |subject: &SubjectPath| {
        acquisition
            .batch
            .records()
            .observations()
            .iter()
            .find(|o| o.subject() == subject)
            .ok_or_else(invalid)
    };
    let text = |subject: &SubjectPath| match observed(subject)?.value() {
        TypedValue::String(v) => Ok(v.as_str()),
        _ => Err(invalid()),
    };
    let anchor = SnapshotAnchor {
        domain: world.profile.authority_domain.clone(),
        epoch: SourceTimestamp::new(text(&world.profile.epoch_subject)?).map_err(|_| invalid())?,
        membership: ContentDigest::new(text(&world.profile.membership_subject)?)
            .map_err(|_| invalid())?,
        acquisition: reference.clone(),
        epoch_observation: observed(&world.profile.epoch_subject)?.id().clone(),
        membership_observation: observed(&world.profile.membership_subject)?.id().clone(),
    };
    if acquisition.observation.source_revision.as_ref() != Some(&anchor.epoch) {
        return Err(invalid());
    }
    let reads = world
        .universe
        .objects
        .iter()
        .map(|member| {
            Ok(SnapshotObjectRead {
                object: member.object.clone(),
                source: member.source.clone(),
                source_kind: member.source_kind,
                acquisition: reference.clone(),
                version_observation: observed(&member.version_subject)?.id().clone(),
            })
        })
        .collect::<Result<Vec<_>, SessionError>>()?;
    source.current()?;
    let allocation = source.view.allocation();
    let snapshot = CompleteScopedSnapshot::validate(
        SnapshotBinding {
            capture: allocation.binding().clone(),
            phase: SnapshotPhase::Before,
            task: source.defs.task.data().id.clone(),
            execution: source.execution()?,
            reservation: Some(SnapshotReservation {
                reservation: allocation.reservation_id().clone(),
                fence: allocation.fence(),
            }),
        },
        world.universe.clone(),
        world.profile.clone(),
        vec![acquisition],
        reads,
        SnapshotConsistency::Atomic { anchor },
        now_ms()?,
    )
    .map_err(|_| invalid())?;
    Ok(CapturedBefore {
        snapshot,
        runtime: source.defs.runtime.reference()?,
        native_context_digest: super::context_digest(
            &source.defs,
            source.global_consent_required()?,
        )?,
    })
}
