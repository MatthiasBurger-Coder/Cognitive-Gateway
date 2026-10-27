use super::*;

fn failure(code: ConnectorFailureCode, effect: ConnectorEffect) -> ConnectorFailure {
    ConnectorFailure {
        code,
        effect,
        correlation: NonEmptyText::new("diagnostic-scope").unwrap(),
    }
}

#[test]
fn connector_failures_preserve_content_free_session_taxonomy() {
    use ConnectorFailureCode as C;
    use SessionError as S;
    let cases = [
        (C::Unavailable, S::Unavailable),
        (C::AuthenticationFailed, S::AuthorityDenied),
        (C::CredentialUnavailable, S::Unavailable),
        (C::CredentialScopeDenied, S::ScopeDenied),
        (C::SecretDisclosure, S::AuthorityDenied),
        (C::PermissionDenied, S::AuthorityDenied),
        (C::Timeout, S::Unavailable),
        (C::Cancelled, S::Unavailable),
        (C::ProtocolError, S::InvalidInput),
        (C::IncompatibleVersion, S::UnsupportedCapability),
        (C::SchemaMismatch, S::InvalidInput),
        (C::RateLimited, S::Unavailable),
        (C::NotFound, S::Unavailable),
        (C::StaleResult, S::StaleRevision),
        (C::ConflictingResult, S::InvalidInput),
        (C::MutationUncertain, S::OutcomeUnknown),
        (C::RetryUnsafe, S::UnsupportedCapability),
        (C::ScopeMismatch, S::ScopeDenied),
        (C::InventoryChanged, S::StaleRevision),
        (C::BudgetExhausted, S::LimitExceeded),
        (C::Backpressure, S::Unavailable),
    ];
    for (code, expected) in cases {
        for effect in [ConnectorEffect::NotDispatched, ConnectorEffect::NoMutation] {
            assert_eq!(
                session_error(failure(code, effect)),
                expected,
                "{code:?}/{effect:?}"
            );
        }
    }
    assert_eq!(S::InvalidInput.code(), "CG_INVALID_INPUT");
    assert_eq!(S::AuthorityDenied.code(), "CG_POLICY_DENIED");
}

#[test]
fn uncertain_effect_takes_priority_over_schema_or_other_diagnostic_codes() {
    for code in [
        ConnectorFailureCode::SchemaMismatch,
        ConnectorFailureCode::PermissionDenied,
        ConnectorFailureCode::BudgetExhausted,
    ] {
        for effect in [ConnectorEffect::Unknown, ConnectorEffect::Acknowledged] {
            assert_eq!(
                session_error(failure(code, effect)),
                SessionError::OutcomeUnknown
            );
        }
    }
    assert_eq!(
        session_error(failure(
            ConnectorFailureCode::MutationUncertain,
            ConnectorEffect::NotDispatched
        )),
        SessionError::OutcomeUnknown
    );
}
