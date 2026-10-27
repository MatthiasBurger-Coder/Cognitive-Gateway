//! Shared native DATA adapter. The caller supplies the actual live coordinator
//! allocation and a genuinely compiled native context. No mint/dispatch exists.
use crate::complete_snapshot_fixture::{self as snapshot, Fixture};
use gateway_application::{
    context_application::CompiledStep,
    sessions::{scoped_snapshots::*, *},
};
use gateway_domain::*;

pub fn capture(
    f: &mut Fixture,
    allocation: &SessionResourceAllocation,
    compiled: &CompiledStep,
    runtime: SnapshotPin,
    now_ms: u64,
) -> CapturedBefore {
    assert_eq!(allocation.binding().scope, f.universe.pin.scope);
    assert_eq!(compiled.basis().scope, allocation.binding().scope);
    assert_eq!(compiled.policy().step, allocation.binding().step);
    let context = compiled.context().execution_context();
    f.binding = SnapshotBinding {
        capture: allocation.binding().clone(),
        phase: SnapshotPhase::Before,
        task: context.task().id().clone(),
        execution: snapshot::text(context.id().as_str()),
        reservation: Some(SnapshotReservation {
            reservation: allocation.reservation_id().clone(),
            fence: allocation.fence(),
        }),
    };
    f.now_ms = now_ms;
    let count = f.acquisitions.len();
    let max_at = f
        .acquisitions
        .iter()
        .map(|a| a.observation.acquired_at.seconds())
        .max()
        .unwrap();
    for i in 0..count {
        let old = &f.acquisitions[i];
        let old_at = old.observation.acquired_at.seconds();
        let at = (now_ms / 1000) as i64 - (max_at - old_at);
        let native = snapshot::native_read_in_scope(
            allocation.binding().scope.clone(),
            old.request.correlation.as_str(),
            old.observation.metadata.object_reference.as_str(),
            old.observation.source_revision.as_ref().unwrap().as_str(),
            at,
            old.request.arguments.clone(),
            old.observation
                .records
                .observations()
                .iter()
                .map(|o| (o.subject().clone(), o.value().clone()))
                .collect(),
        );
        let mut actual = SnapshotAcquisition {
            request: native.request,
            observation: native.observation,
            batch: native.batch,
        };
        actual.request.task = f.binding.task.clone();
        actual.request.step = allocation.binding().step.clone();
        actual.request.execution = f.binding.execution.clone();
        actual.observation.metadata.task = actual.request.task.clone();
        actual.observation.metadata.step = actual.request.step.clone();
        actual.observation.metadata.execution = actual.request.execution.clone();
        f.replace_acquisition(i, actual);
    }
    // Match the production digest owner: FULL native CompiledStep plus actual
    // global consent overlay. The test producer is explicitly readonly/noConsent.
    let compiled: serde_json::Value = serde_json::from_str(&compiled.to_json().unwrap()).unwrap();
    let body = serde_json::json!({"contract":"cg.connector-observed-context/1.0",
        "compiled":compiled,"effective_source_policy":{"consent_required":false}});
    CapturedBefore {
        snapshot: f.validate().unwrap(),
        runtime,
        native_context_digest: context_content_digest(&serde_json::to_vec(&body).unwrap()),
    }
}
