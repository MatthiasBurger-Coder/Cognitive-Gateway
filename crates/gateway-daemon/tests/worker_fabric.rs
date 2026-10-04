use gateway_application::{
    ports::outbound::{CognitiveSchedulerPort, CognitiveWorkerPort},
    worker_fabric::{FabricLimits, ReferenceScheduler, dispatch_one},
};
use gateway_daemon::worker_fabric::{LocalCognitiveWorker, WorkerProposal, snapshot_probe};
use gateway_domain::{ContextScopeId, ReferenceId, worker_fabric::*};
use std::collections::BTreeSet;

fn reference(s: &str) -> ReferenceId {
    ReferenceId::new(s).unwrap()
}
fn scope(s: &str) -> ContextScopeId {
    ContextScopeId::new(s).unwrap()
}
fn spec() -> WorkSpec {
    WorkSpec {
        version: WORK_VERSION,
        scope: scope("project-a"),
        trace: reference("trace-1"),
        operation: reference("operation-1"),
        kind: WorkKind::Model,
        snapshot: b"immutable input".to_vec(),
        sources: BTreeSet::from([reference("source-revision-1")]),
        runtime: reference("runtime-v1"),
        model: Some(reference("model-digest-v1")),
        budget: WorkBudget {
            max_attempts: 2,
            lease_ms: 10,
            retry_delay_ms: 2,
            deadline_ms: 100,
            memory_bytes: 20,
            compute_units: 5,
            max_result_bytes: 256,
        },
    }
}
fn item() -> WorkItem {
    WorkItem::new(spec()).unwrap()
}
fn advertisement() -> WorkerAdvertisement {
    WorkerAdvertisement {
        worker: reference("worker-1"),
        node: reference("node-1"),
        scope: scope("project-a"),
        kinds: BTreeSet::from([
            WorkKind::Retrieval,
            WorkKind::Pattern,
            WorkKind::Evaluation,
            WorkKind::Model,
        ]),
        runtimes: BTreeSet::from([reference("runtime-v1")]),
        models: BTreeSet::from([reference("model-digest-v1")]),
        slots: 1,
        memory_bytes: 40,
        compute_units: 10,
    }
}
fn scheduler() -> ReferenceScheduler {
    ReferenceScheduler::new(FabricLimits {
        retained_items: 4,
        per_scope_items: 3,
        concurrent_leases: 2,
        max_attempts: 3,
        max_lease_ms: 20,
        max_lifetime_ms: 200,
        max_memory_bytes: 100,
        max_compute_units: 30,
    })
    .unwrap()
}
fn probe() -> impl CognitiveWorkerPort {
    LocalCognitiveWorker::new(advertisement(), snapshot_probe)
}
fn submitted() -> (ReferenceScheduler, WorkItem) {
    let mut scheduler = scheduler();
    let item = item();
    assert!(scheduler.submit(item.clone(), 0).unwrap());
    (scheduler, item)
}

#[test]
fn immutable_identity_and_transport_tampering() {
    let original = item();
    assert_eq!(original, WorkItem::new(spec()).unwrap());
    let bytes = serde_json::to_vec(&original).unwrap();
    let roundtrip: WorkItem = serde_json::from_slice(&bytes).unwrap();
    roundtrip.validate().unwrap();
    for field in ["scope", "trace", "operation", "runtime"] {
        let mut changed = spec();
        match field {
            "scope" => changed.scope = scope("project-b"),
            "trace" => changed.trace = reference("trace-2"),
            "operation" => changed.operation = reference("op-2"),
            _ => changed.runtime = reference("runtime-v2"),
        }
        assert_ne!(original.id(), WorkItem::new(changed).unwrap().id());
    }
    let mut json = serde_json::to_value(&original).unwrap();
    json["spec"]["snapshot"] = serde_json::json!([9, 8, 7]);
    let tampered = serde_json::from_value::<WorkItem>(json).unwrap();
    assert_eq!(
        scheduler().submit(tampered, 0),
        Err(FabricError::InvalidContract)
    );
    let mut json = serde_json::to_value(original).unwrap();
    json["approve_procedure"] = serde_json::json!(true);
    assert!(serde_json::from_value::<WorkItem>(json).is_err());
    let mut changed = spec();
    changed.budget.compute_units = u64::MAX;
    assert_eq!(WorkItem::new(changed), Err(FabricError::InvalidContract));
}

#[test]
fn duplicate_delivery_commits_only_once_and_retains_trace_provenance() {
    let (mut s, item) = submitted();
    assert!(!s.submit(item.clone(), 0).unwrap());
    let lease = s.claim(&advertisement(), 1).unwrap().unwrap();
    let result = probe().execute(&lease).unwrap();
    assert!(s.complete(&scope("project-a"), result.clone(), 2).unwrap());
    assert!(!s.complete(&scope("project-a"), result.clone(), 3).unwrap());
    assert!(!s.submit(item.clone(), 100).unwrap());
    assert!(s.claim(&advertisement(), 100).unwrap().is_none());
    let status = s.inspect(&scope("project-a"), item.id()).unwrap();
    assert_eq!(status.trace, item.spec().trace);
    assert_eq!(status.snapshot_digest, item.snapshot_digest());
    assert_eq!(status.result, Some(result));
    assert_eq!(status.attempts, 1);
    assert_eq!(s.metrics().completions, 1);
    assert_eq!(s.metrics().duplicates, 3);
}

#[test]
fn lost_node_timeout_fences_old_results_and_exhausts_bounded_retries() {
    let (mut s, item) = submitted();
    let first = s.claim(&advertisement(), 0).unwrap().unwrap();
    let late = probe().execute(&first).unwrap();
    assert_eq!(s.recover(10).unwrap(), 1);
    assert_eq!(
        s.complete(&scope("project-a"), late.clone(), 10),
        Err(FabricError::StaleLease)
    );
    assert!(s.claim(&advertisement(), 11).unwrap().is_none());
    let mut replacement = advertisement();
    replacement.worker = reference("worker-2");
    replacement.node = reference("node-2");
    let second = s.claim(&replacement, 12).unwrap().unwrap();
    assert_ne!(first.token, second.token);
    assert_eq!(
        s.complete(&scope("project-a"), late, 12),
        Err(FabricError::StaleLease)
    );
    assert_eq!(
        s.fail(
            &scope("project-a"),
            item.id(),
            first.token,
            FailureReason::Execution,
            12
        ),
        Err(FabricError::StaleLease)
    );
    assert_eq!(s.recover(22).unwrap(), 1);
    let status = s.inspect(&scope("project-a"), item.id()).unwrap();
    assert_eq!(
        status.state,
        WorkState::Failed {
            reason: FailureReason::AttemptsExhausted
        }
    );
    assert_eq!(status.attempts, 2);
    assert_eq!(status.reserved_compute_units, 10);
    assert!(s.claim(&replacement, 30).unwrap().is_none());
    assert_eq!(s.metrics().recoveries, 2);
}

#[test]
fn explicit_failure_recovers_to_success_and_preserves_failure_evidence() {
    let (mut s, item) = submitted();
    let mut failing = LocalCognitiveWorker::new(
        advertisement(),
        |_: &WorkItem| -> Result<WorkerProposal, FailureReason> { Err(FailureReason::WorkerLost) },
    );
    assert!(dispatch_one(&mut s, &mut failing, 0, || 1).unwrap());
    assert!(dispatch_one(&mut s, &mut probe(), 3, || 4).unwrap());
    let status = s.inspect(&scope("project-a"), item.id()).unwrap();
    assert_eq!(status.state, WorkState::Completed);
    assert_eq!(status.attempts, 2);
    assert_eq!(status.last_failure, Some(FailureReason::WorkerLost));
    assert_eq!(s.metrics().failures, 1);
}

#[test]
fn project_isolation_at_queue_result_inspection_and_worker_boundaries() {
    let (mut s, item) = submitted();
    let mut other = advertisement();
    other.scope = scope("project-b");
    assert!(s.claim(&other, 0).unwrap().is_none());
    assert_eq!(
        s.inspect(&other.scope, item.id()),
        Err(FabricError::ScopeMismatch)
    );
    let lease = s.claim(&advertisement(), 0).unwrap().unwrap();
    assert_eq!(
        LocalCognitiveWorker::new(other.clone(), snapshot_probe).execute(&lease),
        Err(FailureReason::InvalidResult)
    );
    let result = probe().execute(&lease).unwrap();
    assert_eq!(
        s.complete(&other.scope, result.clone(), 1),
        Err(FabricError::ScopeMismatch)
    );
    assert_eq!(
        s.fail(
            &other.scope,
            item.id(),
            lease.token,
            FailureReason::Execution,
            1
        ),
        Err(FabricError::ScopeMismatch)
    );
    assert_eq!(s.claim(&other, 1), Err(FabricError::InvalidWorker));
    assert!(s.complete(&scope("project-a"), result, 1).unwrap());
}

#[test]
fn provenance_and_result_budget_forgery_cannot_commit() {
    let (mut s, item) = submitted();
    let lease = s.claim(&advertisement(), 0).unwrap().unwrap();
    let good = probe().execute(&lease).unwrap();
    let mut variants = Vec::new();
    let mut r = good.clone();
    r.trace = reference("other-trace");
    variants.push(r);
    let mut r = good.clone();
    r.scope = scope("project-b");
    variants.push(r);
    let mut r = good.clone();
    r.provenance.model = Some(reference("unqualified"));
    variants.push(r);
    let mut r = good.clone();
    r.provenance.runtime = reference("wrong-runtime");
    variants.push(r);
    let mut r = good.clone();
    r.provenance.worker = reference("imposter");
    variants.push(r);
    let mut r = good.clone();
    r.provenance.node = reference("wrong-node");
    variants.push(r);
    let mut r = good.clone();
    r.provenance.sources.clear();
    variants.push(r);
    let mut r = good.clone();
    r.provenance.snapshot_digest = "wrong".into();
    variants.push(r);
    let mut r = good.clone();
    r.compute_units = 6;
    variants.push(r);
    let mut r = good.clone();
    r.proposal = vec![0; 257];
    variants.push(r);
    let mut r = good.clone();
    r.proposal.clear();
    variants.push(r);
    for r in variants {
        assert_eq!(
            s.complete(&scope("project-a"), r, 1),
            Err(FabricError::InvalidResult)
        );
    }
    assert_eq!(s.metrics().invalid_results, 11);
    assert!(
        s.inspect(&scope("project-a"), item.id())
            .unwrap()
            .result
            .is_none()
    );
    assert!(s.complete(&scope("project-a"), good.clone(), 2).unwrap());
    let mut conflict = good;
    conflict.proposal = vec![3];
    assert_eq!(
        s.complete(&scope("project-a"), conflict, 2),
        Err(FabricError::StaleLease)
    );
}

#[test]
fn backpressure_bounds_live_queue_and_completed_idempotency_history() {
    let mut s = scheduler();
    for n in 0..3 {
        let mut spec = spec();
        spec.operation = reference(&format!("op-{n}"));
        let item = WorkItem::new(spec).unwrap();
        s.submit(item, 0).unwrap();
    }
    let mut fourth = spec();
    fourth.operation = reference("op-4");
    assert_eq!(
        s.submit(WorkItem::new(fourth.clone()).unwrap(), 0),
        Err(FabricError::Backpressure)
    );
    assert!(dispatch_one(&mut s, &mut probe(), 0, || 1).unwrap());
    assert_eq!(
        s.submit(WorkItem::new(fourth.clone()).unwrap(), 1),
        Err(FabricError::Backpressure)
    );
    fourth.scope = scope("project-b");
    s.submit(WorkItem::new(fourth).unwrap(), 1).unwrap();
    let mut fifth = spec();
    fifth.scope = scope("project-c");
    assert_eq!(
        s.submit(WorkItem::new(fifth).unwrap(), 1),
        Err(FabricError::Backpressure)
    );
    assert_eq!(s.metrics().backpressure, 3);
}

#[test]
fn capability_selection_and_aggregate_worker_resources_are_bounded() {
    let (mut s, _) = submitted();
    for n in 2..4 {
        let mut spec = spec();
        spec.operation = reference(&format!("op-{n}"));
        s.submit(WorkItem::new(spec).unwrap(), 0).unwrap();
    }
    for mode in 0..4 {
        let mut ad = advertisement();
        match mode {
            0 => ad.models.clear(),
            1 => ad.runtimes = BTreeSet::from([reference("runtime-v2")]),
            2 => ad.kinds = BTreeSet::from([WorkKind::Retrieval]),
            _ => ad.memory_bytes = 19,
        }
        assert!(s.claim(&ad, 0).unwrap().is_none());
    }
    let mut ad = advertisement();
    ad.slots = 3;
    ad.memory_bytes = 39;
    assert!(s.claim(&ad, 0).unwrap().is_some());
    assert!(s.claim(&ad, 0).unwrap().is_none());
    ad.memory_bytes = 40;
    ad.compute_units = 9;
    assert!(s.claim(&ad, 0).unwrap().is_none());
    ad.compute_units = 10;
    assert!(s.claim(&ad, 0).unwrap().is_some());
    ad.worker = reference("worker-2");
    assert!(s.claim(&ad, 0).unwrap().is_none());
}

#[test]
fn deadlines_clocks_and_slow_worker_completion_fail_closed() {
    let (mut s, item) = submitted();
    let lease = s.claim(&advertisement(), 1).unwrap().unwrap();
    let result = probe().execute(&lease).unwrap();
    assert_eq!(
        s.complete(&scope("project-a"), result, 11),
        Err(FabricError::StaleLease)
    );
    assert_eq!(s.recover(10), Err(FabricError::ClockRegression));
    assert_eq!(s.recover(100).unwrap(), 1);
    assert_eq!(
        s.inspect(&scope("project-a"), item.id()).unwrap().state,
        WorkState::Failed {
            reason: FailureReason::Deadline
        }
    );
    let (mut s, _) = submitted();
    assert_eq!(
        dispatch_one(&mut s, &mut probe(), 0, || 10),
        Err(FabricError::StaleLease)
    );
    let mut expired = spec();
    expired.budget.deadline_ms = 1;
    assert_eq!(
        s.submit(WorkItem::new(expired).unwrap(), 10),
        Err(FabricError::InvalidContract)
    );
}

#[test]
fn all_work_kinds_roundtrip_through_replaceable_ports() {
    for kind in [
        WorkKind::Retrieval,
        WorkKind::Pattern,
        WorkKind::Evaluation,
        WorkKind::Model,
    ] {
        let mut spec = spec();
        spec.kind = kind;
        let item = WorkItem::new(spec).unwrap();
        let mut s = scheduler();
        s.submit(item.clone(), 0).unwrap();
        assert!(dispatch_one(&mut s, &mut probe(), 0, || 1).unwrap());
        assert_eq!(
            s.inspect(&scope("project-a"), item.id()).unwrap().state,
            WorkState::Completed
        );
    }
}

#[test]
fn invalid_limits_and_work_budgets_are_rejected_before_dispatch() {
    let limits = FabricLimits {
        retained_items: 4,
        per_scope_items: 3,
        concurrent_leases: 2,
        max_attempts: 3,
        max_lease_ms: 20,
        max_lifetime_ms: 200,
        max_memory_bytes: 100,
        max_compute_units: 30,
    };
    for mode in 0..10 {
        let mut l = limits.clone();
        match mode {
            0 => l.retained_items = 0,
            1 => l.per_scope_items = 0,
            2 => l.per_scope_items = 5,
            3 => l.concurrent_leases = 0,
            4 => l.max_attempts = 0,
            5 => l.max_attempts = 65,
            6 => l.max_lease_ms = 0,
            7 => l.max_lifetime_ms = 0,
            8 => l.max_memory_bytes = 0,
            _ => l.max_compute_units = 0,
        }
        assert!(matches!(
            ReferenceScheduler::new(l),
            Err(FabricError::InvalidContract)
        ));
    }
    for mode in 0..5 {
        let mut changed = spec();
        match mode {
            0 => changed.budget.deadline_ms = 201,
            1 => changed.budget.max_attempts = 4,
            2 => changed.budget.lease_ms = 21,
            3 => changed.budget.memory_bytes = 101,
            _ => changed.budget.compute_units = 16,
        }
        assert_eq!(
            scheduler().submit(WorkItem::new(changed).unwrap(), 0),
            Err(FabricError::InvalidContract)
        );
    }
    let mut s = scheduler();
    assert_eq!(
        s.inspect(&scope("project-a"), "missing"),
        Err(FabricError::UnknownWork)
    );
    assert_eq!(
        s.fail(
            &scope("project-a"),
            "missing",
            1,
            FailureReason::Execution,
            0
        ),
        Err(FabricError::UnknownWork)
    );
    let mut ad = advertisement();
    ad.slots = 0;
    assert_eq!(s.claim(&ad, 0), Err(FabricError::InvalidWorker));
}

#[test]
fn local_adapter_rejects_invalid_leases_and_handler_budget_overruns() {
    let (mut s, _) = submitted();
    let lease = s.claim(&advertisement(), 0).unwrap().unwrap();
    for mode in 0..6 {
        let mut changed = lease.clone();
        match mode {
            0 => changed.worker = reference("wrong-worker"),
            1 => changed.node = reference("wrong-node"),
            2 => changed.token = 0,
            3 => changed.attempt = 0,
            4 => changed.attempt = 3,
            _ => {
                let mut json = serde_json::to_value(&changed.item).unwrap();
                json["id"] = serde_json::json!("tampered");
                changed.item = serde_json::from_value(json).unwrap();
            }
        }
        assert_eq!(probe().execute(&changed), Err(FailureReason::InvalidResult));
    }
    for mode in 0..3 {
        let mut worker = LocalCognitiveWorker::new(advertisement(), move |_: &WorkItem| {
            Ok(WorkerProposal {
                bytes: if mode == 0 {
                    vec![]
                } else if mode == 1 {
                    vec![0; 257]
                } else {
                    vec![1]
                },
                compute_units: if mode == 2 { 6 } else { 1 },
            })
        });
        assert_eq!(worker.execute(&lease), Err(FailureReason::InvalidResult));
    }
}

#[test]
fn retained_fabric_evidence_is_correlated_and_reproducible() {
    let (mut s, item) = submitted();
    let first = s.claim(&advertisement(), 0).unwrap().unwrap();
    let stale = probe().execute(&first).unwrap();
    s.recover(10).unwrap();
    assert_eq!(
        s.complete(&scope("project-a"), stale, 10),
        Err(FabricError::StaleLease)
    );
    let lease = s.claim(&advertisement(), 12).unwrap().unwrap();
    let result = probe().execute(&lease).unwrap();
    s.complete(&scope("project-a"), result.clone(), 13).unwrap();
    assert!(!s.complete(&scope("project-a"), result, 14).unwrap());
    let status = s.inspect(&scope("project-a"), item.id()).unwrap();
    assert_eq!(status.attempts, 2);
    assert_eq!(status.reserved_compute_units, 10);
    assert_eq!(s.metrics().recoveries, 1);
    assert_eq!(s.metrics().completions, 1);
    if let Ok(path) = std::env::var("CG29_FABRIC_OUTPUT") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": WORK_VERSION, "work_item": item, "status": status,
                "dispatches": s.metrics().dispatches, "recoveries": s.metrics().recoveries,
                "completions": s.metrics().completions, "duplicates": s.metrics().duplicates
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn transport_dispatch_cannot_complete_a_different_assignment() {
    struct MisboundWorker;
    impl CognitiveWorkerPort for MisboundWorker {
        fn advertisement(&self) -> WorkerAdvertisement {
            advertisement()
        }
        fn execute(&mut self, lease: &WorkLease) -> Result<WorkResult, FailureReason> {
            let mut result = probe().execute(lease)?;
            result.work_id = "different-work".into();
            Ok(result)
        }
    }
    let (mut s, item) = submitted();
    assert_eq!(
        dispatch_one(&mut s, &mut MisboundWorker, 0, || 1),
        Err(FabricError::InvalidResult)
    );
    let status = s.inspect(&scope("project-a"), item.id()).unwrap();
    assert_eq!(status.last_failure, Some(FailureReason::InvalidResult));
    assert_eq!(status.state, WorkState::Queued { ready_ms: 3 });
}

#[path = "support/epic03_workers.rs"]
mod complete;
