use gateway_domain::{ContextScopeId, ReferenceId, worker_fabric::*};
use std::collections::BTreeSet;
fn spec() -> WorkSpec {
    WorkSpec {
        version: WORK_VERSION,
        scope: ContextScopeId::new("project-a").unwrap(),
        trace: ReferenceId::new("trace-1").unwrap(),
        operation: ReferenceId::new("op-1").unwrap(),
        kind: WorkKind::Model,
        snapshot: vec![1, 2, 3],
        sources: BTreeSet::from([ReferenceId::new("source-1").unwrap()]),
        runtime: ReferenceId::new("runtime-1").unwrap(),
        model: Some(ReferenceId::new("model-1").unwrap()),
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
#[test]
fn identity_survives_transport_and_rejects_forgery() {
    let item = WorkItem::new(spec()).unwrap();
    assert_eq!(item.snapshot_digest(), digest(&[1, 2, 3]));
    assert_eq!(item.spec(), &spec());
    assert_eq!(item.id(), WorkItem::new(spec()).unwrap().id());
    let copy: WorkItem = serde_json::from_slice(&serde_json::to_vec(&item).unwrap()).unwrap();
    assert_eq!(copy, item);
    copy.validate().unwrap();
    for field in ["id", "snapshot_digest"] {
        let mut json = serde_json::to_value(&item).unwrap();
        json[field] = serde_json::json!("forged");
        assert_eq!(
            serde_json::from_value::<WorkItem>(json).unwrap().validate(),
            Err(FabricError::InvalidContract)
        );
    }
}
#[test]
fn every_invalid_snapshot_and_budget_is_refused() {
    for mode in 0..15 {
        let mut spec = spec();
        match mode {
            0 => spec.version = 2,
            1 => spec.snapshot.clear(),
            2 => spec.snapshot = vec![0; MAX_SNAPSHOT_BYTES + 1],
            3 => spec.sources.clear(),
            4 => {
                spec.sources = (0..65)
                    .map(|n| ReferenceId::new(format!("source-{n}")).unwrap())
                    .collect()
            }
            5 => spec.budget.max_attempts = 0,
            6 => spec.budget.max_attempts = 65,
            7 => spec.budget.lease_ms = 0,
            8 => spec.budget.deadline_ms = 0,
            9 => spec.budget.memory_bytes = 0,
            10 => spec.budget.compute_units = 0,
            11 => spec.budget.max_result_bytes = 0,
            12 => spec.budget.max_result_bytes = MAX_SNAPSHOT_BYTES + 1,
            13 => spec.budget.compute_units = u64::MAX,
            _ => spec.model = None,
        }
        assert_eq!(WorkItem::new(spec), Err(FabricError::InvalidContract));
    }
}
