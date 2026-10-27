use super::tests::{dispatch, mapper, wire};
use super::*;
use gateway_application::connector_resilience as recovery;
#[test]
fn recovery_metadata_is_current_bound_and_explicitly_unsupported_by_default() {
    let mut mapper = mapper();
    let wire = wire();
    let mut request = dispatch(&mapper, &wire, false).request;
    let support = mapper.recovery_support(&request, None, &wire).unwrap();
    assert_eq!(support.retry, recovery::RetrySafety::ReadOnly);
    assert_eq!(
        support.idempotency,
        recovery::ProviderIdempotency::Idempotent
    );
    assert_eq!(support.read_back, recovery::ReadBackSupport::Unsupported);
    request.inventory_revision = gateway_domain::ContentDigest::new("b".repeat(64)).unwrap();
    assert_eq!(
        mapper.recovery_support(&request, None, &wire),
        Err(MappingError::StaleInventory)
    );
    for (retry, idempotency, key) in [
        (RetrySafety::Never, Idempotency::NonIdempotent, None),
        (
            RetrySafety::Idempotent,
            Idempotency::ProviderKey,
            Some("query".to_owned()),
        ),
    ] {
        mapper.document.0.records[0].retry_safety = retry;
        mapper.document.0.records[0].idempotency = idempotency;
        mapper.document.0.records[0].idempotency_key = key;
        let request = dispatch(&mapper, &wire, false).request;
        let support = mapper.recovery_support(&request, None, &wire).unwrap();
        assert_eq!(
            support.retry,
            if retry == RetrySafety::Never {
                recovery::RetrySafety::Never
            } else {
                recovery::RetrySafety::Idempotent
            }
        );
        assert_eq!(
            support.idempotency,
            if idempotency == Idempotency::NonIdempotent {
                recovery::ProviderIdempotency::Unsupported
            } else {
                recovery::ProviderIdempotency::Keyed {
                    argument: NonEmptyText::new("query").unwrap(),
                }
            }
        );
    }
}
