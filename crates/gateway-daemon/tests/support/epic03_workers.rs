use super::*;

fn durable_connection() -> String {
    let raw =
        std::env::var("CG_COGNITIVE_TEST_DATABASE").expect("run scripts/cognitive-test-host.py");
    let mut client = postgres::Client::connect(&raw, postgres::NoTls).unwrap();
    let schema = format!(
        "workers_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    client
        .batch_execute(&format!("CREATE SCHEMA {schema}"))
        .unwrap();
    format!("{raw} options='-c search_path={schema}'")
}
fn limits() -> FabricLimits {
    FabricLimits {
        retained_items: 4,
        per_scope_items: 3,
        concurrent_leases: 2,
        max_attempts: 3,
        max_lease_ms: 20,
        max_lifetime_ms: 200,
        max_memory_bytes: 100,
        max_compute_units: 30,
    }
}
fn durable(connection: &str) -> gateway_daemon::durable_workers::DurableScheduler {
    gateway_daemon::durable_workers::DurableScheduler::new(
        gateway_daemon::cognitive_store::CognitiveStore::connect(connection, scope("project-a"))
            .unwrap(),
        limits(),
    )
    .unwrap()
}
#[test]
#[ignore = "requires disposable PostgreSQL host; included in full qualification"]
fn durable_worker_restart_fencing_duplicate_commit_and_competing_coordinators() {
    let connection = durable_connection();
    let mut first = durable(&connection);
    let work = item();
    assert!(first.submit(work.clone(), 0).unwrap());
    let lost = first.claim(&advertisement(), 1).unwrap().unwrap();
    let late = probe().execute(&lost).unwrap();
    drop(first);
    let mut recovered = durable(&connection);
    assert_eq!(
        recovered
            .inspect(&scope("project-a"), work.id())
            .unwrap()
            .attempts,
        1
    );
    assert_eq!(recovered.recover(11).unwrap(), 1);
    assert_eq!(
        recovered.complete(&scope("project-a"), late, 11),
        Err(FabricError::StaleLease)
    );
    let mut replacement = advertisement();
    replacement.worker = reference("worker-2");
    replacement.node = reference("node-2");
    let lease = recovered.claim(&replacement, 13).unwrap().unwrap();
    assert_ne!(lease.token, lost.token);
    let mut replacement_worker = LocalCognitiveWorker::new(replacement, snapshot_probe);
    let result = replacement_worker.execute(&lease).unwrap();
    assert!(
        recovered
            .complete(&scope("project-a"), result.clone(), 14)
            .unwrap()
    );
    drop(recovered);
    let mut reloaded = durable(&connection);
    assert!(!reloaded.complete(&scope("project-a"), result, 15).unwrap());
    assert!(!reloaded.submit(work.clone(), 16).unwrap());
    assert!(reloaded.claim(&advertisement(), 17).unwrap().is_none());
    assert_eq!(
        reloaded.submit(
            WorkItem::new(WorkSpec {
                scope: scope("project-b"),
                ..spec()
            })
            .unwrap(),
            17
        ),
        Err(FabricError::ScopeMismatch)
    );
    assert_eq!(
        reloaded.inspect(&scope("project-b"), work.id()),
        Err(FabricError::ScopeMismatch)
    );
    assert_eq!(
        reloaded.claim(
            &WorkerAdvertisement {
                scope: scope("project-b"),
                ..advertisement()
            },
            17
        ),
        Err(FabricError::ScopeMismatch)
    );
    let second = WorkItem::new(WorkSpec {
        operation: reference("op-2"),
        ..spec()
    })
    .unwrap();
    reloaded.submit(second.clone(), 18).unwrap();
    let connections = [connection.clone(), connection.clone()];
    let handles: Vec<_> = connections
        .into_iter()
        .enumerate()
        .map(|(n, c)| {
            std::thread::spawn(move || {
                let mut scheduler = durable(&c);
                let mut ad = advertisement();
                ad.worker = reference(&format!("contender-{n}"));
                scheduler.claim(&ad, 19).unwrap()
            })
        })
        .collect();
    let leases: Vec<_> = handles
        .into_iter()
        .filter_map(|h| h.join().unwrap())
        .collect();
    assert_eq!(leases.len(), 1);
    let lease = &leases[0];
    reloaded
        .fail(
            &scope("project-a"),
            second.id(),
            lease.token,
            FailureReason::WorkerLost,
            20,
        )
        .unwrap();
    assert!(reloaded.claim(&advertisement(), 21).unwrap().is_none());
    let next = reloaded.claim(&advertisement(), 22).unwrap().unwrap();
    assert_ne!(next.token, lease.token);
    reloaded
        .fail(
            &scope("project-a"),
            second.id(),
            next.token,
            FailureReason::Execution,
            23,
        )
        .unwrap();
    let status = reloaded.inspect(&scope("project-a"), second.id()).unwrap();
    assert_eq!(
        status.state,
        WorkState::Failed {
            reason: FailureReason::AttemptsExhausted
        }
    );
    let mut changed = limits();
    changed.max_attempts = 2;
    let mut mismatch = gateway_daemon::durable_workers::DurableScheduler::new(
        gateway_daemon::cognitive_store::CognitiveStore::connect(&connection, scope("project-a"))
            .unwrap(),
        changed,
    )
    .unwrap();
    assert_eq!(mismatch.recover(24), Err(FabricError::Storage));
    if let Ok(path) = std::env::var("CG03_DURABLE_WORKER_OUTPUT") {
        std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({"schema_version":1,"status":"PASS","coordinator_restart":true,"late_results_fenced":true,
            "competing_coordinators_single_claim":true,"duplicate_commits":0,"completed":reloaded.inspect(&scope("project-a"),work.id()).unwrap(),"exhausted":status})).unwrap()).unwrap();
    }
    // Hash corruption and malformed journals fail before any work can dispatch.
    let mut client = postgres::Client::connect(&connection, postgres::NoTls).unwrap();
    client
        .execute("UPDATE cg_cognitive_journals SET digest='corrupt'", &[])
        .unwrap();
    assert_eq!(
        reloaded.claim(&advertisement(), 24),
        Err(FabricError::Storage)
    );
    client
        .execute(
            "UPDATE cg_cognitive_journals SET payload='not-json',digest=$1",
            &[&digest(b"not-json")],
        )
        .unwrap();
    assert_eq!(reloaded.recover(25), Err(FabricError::Storage));
}

#[test]
fn bounded_process_limits_hung_cpu_memory_output_and_runtime_identity() {
    use gateway_daemon::{
        bounded_process::{BoundedProcess, ProcessError},
        worker_fabric::ProcessCognitiveWorker,
    };
    let root = std::env::temp_dir().join(format!("cg-process-limits-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let script = root.join("worker.py");
    std::fs::write(&script,"import sys,time,json\nr=json.load(sys.stdin)\nif r['mode']=='hang': time.sleep(10)\nif r['mode']=='cpu':\n while True: pass\nif r['mode']=='memory': x=bytearray(500_000_000)\nif r['mode']=='output': print('x'*10000)\nif r['mode']=='empty': sys.exit(0)\nif r['mode']=='good': print(json.dumps({'proposal':r['value'],'has_credentials':any('PASSWORD' in x or x=='HOME' for x in __import__('os').environ)}))\n").unwrap();
    let process = BoundedProcess {
        interpreter: "/usr/bin/python3".into(),
        script: script.clone(),
        work_root: root.clone(),
    };
    let good = process
        .run(
            "execute",
            b"{\"mode\":\"good\",\"value\":42}",
            1000,
            134_217_728,
            1,
            1024,
        )
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&good).unwrap();
    assert_eq!(result["proposal"], 42);
    assert_eq!(result["has_credentials"], false);
    for mode in ["hang", "cpu", "memory", "output"] {
        let input = serde_json::json!({"mode":mode}).to_string();
        assert_eq!(
            process.run(
                "execute",
                input.as_bytes(),
                if mode == "cpu" { 3000 } else { 100 },
                134_217_728,
                1,
                1024
            ),
            Err(ProcessError::Failed)
        );
    }
    assert_eq!(
        process.run(
            "execute",
            b"{\"mode\":\"empty\"}",
            1000,
            134_217_728,
            1,
            1024
        ),
        Err(ProcessError::Output)
    );
    assert_eq!(
        process.run("execute", b"{}", 0, 134_217_728, 1, 1024),
        Err(ProcessError::InvalidLimits)
    );
    let unavailable = BoundedProcess {
        work_root: root.join("missing"),
        ..process.clone()
    };
    assert_eq!(
        unavailable.run("execute", b"{}", 1000, 134_217_728, 1, 1024),
        Err(ProcessError::Unavailable)
    );
    let runtime = reference(&format!(
        "sha256-{}",
        digest(&std::fs::read(&script).unwrap())
    ));
    let ad = WorkerAdvertisement {
        runtimes: BTreeSet::from([runtime.clone()]),
        memory_bytes: 134_217_728,
        compute_units: 1,
        ..advertisement()
    };
    let work = WorkItem::new(WorkSpec {
        runtime,
        snapshot: b"{\"mode\":\"good\",\"value\":42}".to_vec(),
        budget: WorkBudget {
            lease_ms: 1000,
            deadline_ms: 3000,
            memory_bytes: 134_217_728,
            compute_units: 1,
            max_result_bytes: 1024,
            ..spec().budget
        },
        ..spec()
    })
    .unwrap();
    let lease = WorkLease {
        item: work,
        worker: ad.worker.clone(),
        node: ad.node.clone(),
        token: 1,
        attempt: 1,
        expires_ms: 1000,
    };
    let mut worker = ProcessCognitiveWorker {
        advertisement: ad,
        process: process.clone(),
        clock: || 0,
    };
    assert_eq!(worker.advertisement().worker, reference("worker-1"));
    let proof = worker.execute(&lease).unwrap();
    assert_eq!(proof.work_id, lease.item.id());
    worker.clock = || 1000;
    assert_eq!(worker.execute(&lease), Err(FailureReason::Timeout));
    worker.clock = || 0;
    std::fs::write(&script, b"changed script").unwrap();
    assert_eq!(worker.execute(&lease), Err(FailureReason::InvalidResult));
    std::fs::remove_file(&script).unwrap();
    assert_eq!(worker.execute(&lease), Err(FailureReason::Execution));
    std::fs::remove_dir_all(root).unwrap();
}
