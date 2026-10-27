use super::*;
use gateway_application::connector_resilience::*;

#[test]
fn explicit_rate_limit_is_distinct_from_timeout_and_honors_cooldown() {
    for mode in ["rate-once", "rate-default", "rate-invalid"] {
        let host = host(mode);
        ready(&host);
        let error = host.invoke(&request()).unwrap_err();
        assert_eq!(
            error.code,
            if mode == "rate-invalid" {
                ConnectorFailureCode::ProtocolError
            } else {
                ConnectorFailureCode::RateLimited
            }
        );
        assert_eq!(error.effect, ConnectorEffect::NoMutation);
        if mode != "rate-invalid" {
            let health = host.health_snapshot();
            assert_eq!(health.health, ConnectorHealth::Degraded);
            assert!(health.retry_after > Duration::ZERO);
            let next = host.invoke(&request()).unwrap_err();
            assert_eq!(next.code, ConnectorFailureCode::RateLimited);
            assert_eq!(next.effect, ConnectorEffect::NotDispatched);
        }
    }
}
#[test]
fn connector_load_is_rejected_without_queueing_or_parallel_wire_effects() {
    let host = Arc::new(host("slow"));
    ready(&host);
    let worker = {
        let host = host.clone();
        std::thread::spawn(move || host.invoke(&request()))
    };
    let until = Instant::now() + Duration::from_secs(3);
    while !host.health_snapshot().in_flight {
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    let workers: Vec<_> = (0..16)
        .map(|_| {
            let host = host.clone();
            std::thread::spawn(move || host.invoke(&request()))
        })
        .collect();
    for rejected in workers {
        let error = rejected.join().unwrap().unwrap_err();
        assert_eq!(
            (error.code, error.effect),
            (
                ConnectorFailureCode::Backpressure,
                ConnectorEffect::NotDispatched
            )
        );
    }
    assert!(worker.join().unwrap().is_ok());
    let health = host.health_snapshot();
    assert_eq!(health.rejected, 16);
    assert!(!health.in_flight);
    assert_eq!(health.health, ConnectorHealth::Healthy);
}
#[test]
fn persistent_real_peer_failures_open_the_host_circuit_and_emit_health() {
    let events = Arc::new(Events::default());
    let host = build(
        "version",
        Limits::default(),
        Arc::new(Authority::default()),
        events.clone(),
    );
    for _ in 0..5 {
        assert_eq!(
            host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
                .unwrap_err()
                .code,
            ConnectorFailureCode::IncompatibleVersion
        );
    }
    let before = host.health_snapshot();
    assert_eq!(before.circuit, CircuitState::Open);
    assert_eq!(before.health, ConnectorHealth::Unavailable);
    assert_eq!(
        host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
            .unwrap_err()
            .code,
        ConnectorFailureCode::Unavailable
    );
    assert_eq!(host.health_snapshot().attempts, before.attempts);
    assert!(events.0.lock().unwrap().iter().any(|event| matches!(
        event.kind,
        EventKind::Health(HealthSnapshot {
            circuit: CircuitState::Open,
            ..
        })
    )));
    // Operator cleanup remains possible while the circuit is open.
    host.lifecycle(&target(), ConnectorLifecycleCommand::Disable)
        .unwrap();
}

#[test]
fn rate_limits_on_bootstrap_discovery_and_health_apply_host_cooldown() {
    for mode in ["rate-init", "rate-discovery", "rate-health"] {
        let host = host(mode);
        let error = if mode == "rate-init" {
            host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
                .unwrap_err()
        } else {
            host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
                .unwrap();
            if mode == "rate-discovery" {
                host.discover(&target()).unwrap_err()
            } else {
                host.discover(&target()).unwrap();
                host.lifecycle(&target(), ConnectorLifecycleCommand::Health)
                    .unwrap_err()
            }
        };
        assert_eq!(error.code, ConnectorFailureCode::RateLimited);
        assert!(host.health_snapshot().retry_after > Duration::ZERO);
        assert_eq!(
            host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
                .unwrap_err()
                .code,
            ConnectorFailureCode::RateLimited
        );
    }
}

#[test]
fn canonical_recovery_metadata_flows_through_the_production_mapping_adapter() {
    let host = canonical_host(Arc::new(Authority::default()), "normal");
    host.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
        .unwrap();
    let inventory = host.discover(&target()).unwrap();
    let mut request = request();
    request.inventory_revision = inventory.revision;
    let support = host.recovery_support(&request, None).unwrap();
    assert_eq!(support.retry, RetrySafety::ReadOnly);
    assert_eq!(support.idempotency, ProviderIdempotency::Idempotent);
    assert_eq!(support.read_back, ReadBackSupport::Unsupported);
}
