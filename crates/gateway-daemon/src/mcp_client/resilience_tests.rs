use super::*;
use gateway_application::connectors::*;
fn context() -> CallContext {
    CallContext::new(
        NonEmptyText::new("health").unwrap(),
        Instant::now() + Duration::from_secs(10),
    )
}
fn failure(code: ConnectorFailureCode) -> ConnectorFailure {
    ConnectorFailure {
        code,
        effect: ConnectorEffect::NotDispatched,
        correlation: context().correlation,
    }
}
fn runtime() -> Resilience {
    Resilience::new(
        ResilienceConfig {
            failure_threshold: 2,
            open_for: Duration::from_secs(1),
        },
        Arc::new(()),
    )
}
fn expire(runtime: &Resilience) {
    runtime.state.lock().unwrap().open_until = Some(Instant::now() - Duration::from_secs(1));
}
#[test]
fn configuration_rejects_zero_and_excessive_thresholds_and_durations() {
    assert!(ResilienceConfig::default().validate().is_ok());
    for (failure_threshold, open_for) in [
        (0, Duration::from_secs(1)),
        (1025, Duration::from_secs(1)),
        (1, Duration::ZERO),
        (1, Duration::from_secs(3601)),
    ] {
        assert_eq!(
            ResilienceConfig {
                failure_threshold,
                open_for
            }
            .validate(),
            Err(Error::Configuration)
        );
    }
}
#[test]
fn circuit_opens_rejects_without_io_probes_and_recovers() {
    let runtime = runtime();
    let context = context();
    for _ in 0..2 {
        assert_eq!(
            runtime
                .run::<()>(&context, || Err(failure(ConnectorFailureCode::Unavailable)))
                .unwrap_err()
                .code,
            ConnectorFailureCode::Unavailable
        );
    }
    assert_eq!(runtime.snapshot().health, ConnectorHealth::Unavailable);
    assert_eq!(runtime.snapshot().circuit, CircuitState::Open);
    assert_eq!(
        runtime
            .run::<()>(&context, || panic!("open circuit performed I/O"))
            .unwrap_err()
            .effect,
        ConnectorEffect::NotDispatched
    );
    assert_eq!(runtime.snapshot().rejected, 1);
    assert!(runtime.snapshot().retry_after > Duration::ZERO);
    expire(&runtime);
    runtime
        .run(&context, || {
            assert_eq!(runtime.snapshot().circuit, CircuitState::HalfOpen);
            assert_eq!(
                runtime
                    .run::<()>(&context, || panic!("parallel probe"))
                    .unwrap_err()
                    .code,
                ConnectorFailureCode::Backpressure
            );
            Ok(())
        })
        .unwrap();
    let snapshot = runtime.snapshot();
    assert_eq!(
        (snapshot.attempts, snapshot.failed, snapshot.succeeded),
        (3, 2, 1)
    );
    assert_eq!(
        (snapshot.circuit, snapshot.health),
        (CircuitState::Closed, ConnectorHealth::Healthy)
    );
    assert!(!snapshot.in_flight);
}
#[test]
fn failed_or_denied_half_open_probe_cannot_close_circuit() {
    for code in [
        ConnectorFailureCode::Unavailable,
        ConnectorFailureCode::PermissionDenied,
    ] {
        let runtime = runtime();
        for _ in 0..2 {
            let _ = runtime.run::<()>(&context(), || Err(failure(ConnectorFailureCode::Timeout)));
        }
        expire(&runtime);
        let _ = runtime.run::<()>(&context(), || Err(failure(code)));
        assert_eq!(runtime.snapshot().circuit, CircuitState::Open);
    }
    let runtime = runtime();
    let _ = runtime.run::<()>(&context(), || Err(failure(ConnectorFailureCode::Cancelled)));
    assert_eq!(runtime.snapshot().consecutive_failures, 0);
}
#[test]
fn cancellation_rate_limit_and_panics_do_not_leave_busy_state() {
    let runtime = runtime();
    let context = context();
    let cancelled = context.clone();
    cancelled.cancel();
    assert_eq!(
        runtime
            .run::<()>(&cancelled, || panic!("cancelled I/O"))
            .unwrap_err()
            .code,
        ConnectorFailureCode::Cancelled
    );
    let context = super::tests::context();
    runtime.rate_limited(Duration::from_secs(2));
    runtime.rate_limited(Duration::from_secs(1));
    assert!(runtime.snapshot().retry_after > Duration::from_secs(1));
    assert_eq!(
        runtime
            .run::<()>(&context, || panic!("rate-limited I/O"))
            .unwrap_err()
            .code,
        ConnectorFailureCode::RateLimited
    );
    runtime.state.lock().unwrap().rate_until = Some(Instant::now() - Duration::from_secs(1));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || runtime.run::<()>(&context, || panic!("test unwind"))
        ))
        .is_err()
    );
    assert!(!runtime.snapshot().in_flight);
    runtime.run(&context, || Ok(())).unwrap();
    assert_eq!(runtime.snapshot().health, ConnectorHealth::Healthy);
}
