use super::*;

#[allow(dead_code)]
#[path = "../../../tests/support/complete_scoped_snapshot_fixture.rs"]
mod fixture;

#[test]
fn canonical_reload_retains_the_selected_native_consistency_profile() {
    for input in [
        fixture::Fixture::atomic(),
        fixture::Fixture::versioned(),
        fixture::Fixture::bracketed(),
    ] {
        let complete = input.validate().unwrap();
        let encoded = complete.encode().unwrap();
        let loaded = CompleteScopedSnapshot::decode(
            &encoded,
            &input.binding,
            &input.universe,
            &input.profile,
            input.now_ms,
        )
        .unwrap();
        assert_eq!(loaded.consistency(), &input.consistency);
        assert_eq!(loaded.consistency(), complete.consistency());
        assert_eq!(loaded.digest(), complete.digest());
        assert_eq!(loaded.encode().unwrap(), encoded);
    }
}
