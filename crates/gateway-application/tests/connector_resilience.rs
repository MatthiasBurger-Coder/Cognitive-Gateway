use gateway_application::{connector_resilience::*, connectors::*};
use gateway_domain::{CapabilityId, NonEmptyText};
use std::time::Duration;
fn failure(code: ConnectorFailureCode, effect: ConnectorEffect) -> ConnectorFailure {
    ConnectorFailure {
        code,
        effect,
        correlation: NonEmptyText::new("request").unwrap(),
    }
}
fn read() -> RecoverySupport {
    RecoverySupport {
        retry: RetrySafety::ReadOnly,
        ..Default::default()
    }
}
#[test]
fn bounded_exponential_backoff_obeys_remaining_time_and_provider_lower_bound() {
    let policy = RetryPolicy::new(32, Duration::from_millis(10), Duration::from_secs(1)).unwrap();
    let error = failure(
        ConnectorFailureCode::RateLimited,
        ConnectorEffect::NoMutation,
    );
    for (attempts, ms) in [(1, 10), (2, 20), (7, 640), (8, 1000), (31, 1000)] {
        assert_eq!(
            policy.decide(&error, &read(), attempts, Duration::from_secs(10), None),
            RetryDecision::RetryAfter(Duration::from_millis(ms))
        );
    }
    assert_eq!(
        policy.decide(
            &error,
            &read(),
            1,
            Duration::from_secs(10),
            Some(Duration::from_secs(4))
        ),
        RetryDecision::RetryAfter(Duration::from_secs(4))
    );
    for (attempts, remaining) in [(0, 10), (32, 10), (u32::MAX, 10), (1, 0)] {
        assert_eq!(
            policy.decide(
                &error,
                &read(),
                attempts,
                Duration::from_secs(remaining),
                None
            ),
            RetryDecision::Stop(ConnectorFailureCode::BudgetExhausted)
        );
    }
    assert_eq!(
        policy.decide(
            &error,
            &read(),
            1,
            Duration::from_secs(2),
            Some(Duration::from_secs(2))
        ),
        RetryDecision::Stop(ConnectorFailureCode::BudgetExhausted)
    );
    assert_eq!(
        RetryPolicy::default().decide(&error, &read(), 3, Duration::from_secs(10), None),
        RetryDecision::Stop(ConnectorFailureCode::BudgetExhausted)
    );
}
#[test]
fn unsafe_acknowledged_and_uncertain_effects_never_replay() {
    let policy = RetryPolicy::default();
    for support in [
        RecoverySupport::default(),
        RecoverySupport {
            retry: RetrySafety::Idempotent,
            ..Default::default()
        },
    ] {
        assert_eq!(
            policy.decide(
                &failure(
                    ConnectorFailureCode::Timeout,
                    ConnectorEffect::NotDispatched
                ),
                &support,
                1,
                Duration::from_secs(10),
                None
            ),
            RetryDecision::Stop(ConnectorFailureCode::RetryUnsafe)
        );
    }
    for idempotency in [
        ProviderIdempotency::Idempotent,
        ProviderIdempotency::Keyed {
            argument: NonEmptyText::new("request_key").unwrap(),
        },
    ] {
        let support = RecoverySupport {
            retry: RetrySafety::Idempotent,
            idempotency,
            read_back: ReadBackSupport::Capability(CapabilityId::new("effect.read").unwrap()),
        };
        for effect in [
            ConnectorEffect::Unknown,
            ConnectorEffect::Acknowledged,
            ConnectorEffect::NotDispatched,
        ] {
            let decision = policy.decide(
                &failure(ConnectorFailureCode::Timeout, effect),
                &support,
                1,
                Duration::from_secs(10),
                None,
            );
            assert_eq!(
                decision,
                match effect {
                    ConnectorEffect::Unknown => RetryDecision::Reconcile(support.read_back.clone()),
                    ConnectorEffect::Acknowledged =>
                        RetryDecision::Stop(ConnectorFailureCode::RetryUnsafe),
                    _ => RetryDecision::RetryAfter(Duration::from_millis(50)),
                }
            );
        }
    }
    assert_eq!(
        policy.decide(
            &failure(
                ConnectorFailureCode::MutationUncertain,
                ConnectorEffect::NotDispatched
            ),
            &read(),
            1,
            Duration::from_secs(10),
            None
        ),
        RetryDecision::Reconcile(ReadBackSupport::Unsupported)
    );
}
#[test]
fn only_transient_failures_are_retryable_and_configuration_is_bounded() {
    let policy = RetryPolicy::default();
    for code in [
        ConnectorFailureCode::Unavailable,
        ConnectorFailureCode::Timeout,
        ConnectorFailureCode::Backpressure,
    ] {
        assert!(matches!(
            policy.decide(
                &failure(code, ConnectorEffect::NoMutation),
                &read(),
                1,
                Duration::from_secs(10),
                None
            ),
            RetryDecision::RetryAfter(_)
        ));
    }
    for code in [
        ConnectorFailureCode::Cancelled,
        ConnectorFailureCode::AuthenticationFailed,
        ConnectorFailureCode::PermissionDenied,
        ConnectorFailureCode::ProtocolError,
        ConnectorFailureCode::SchemaMismatch,
        ConnectorFailureCode::BudgetExhausted,
        ConnectorFailureCode::InventoryChanged,
    ] {
        assert_eq!(
            policy.decide(
                &failure(code, ConnectorEffect::NoMutation),
                &read(),
                1,
                Duration::from_secs(10),
                None
            ),
            RetryDecision::Stop(code)
        );
    }
    for (attempts, initial, maximum) in [(0, 1, 1), (33, 1, 1), (1, 0, 1), (1, 2, 1), (1, 1, 61)] {
        assert_eq!(
            RetryPolicy::new(
                attempts,
                Duration::from_secs(initial),
                Duration::from_secs(maximum)
            ),
            Err(ConnectorFailureCode::BudgetExhausted)
        );
    }
}
