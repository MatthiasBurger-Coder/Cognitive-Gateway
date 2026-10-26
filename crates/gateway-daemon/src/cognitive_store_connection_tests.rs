use super::*;

#[test]
fn shared_connection_clones_read_fresh_committed_rows_and_preserve_scope() {
    let Ok(connection) = std::env::var("CG_COGNITIVE_TEST_DATABASE") else {
        return;
    };
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let scope = ContextScopeId::new(format!("connection-sharing-{nonce}")).unwrap();
    let first = CognitiveStore::connect(&connection, scope.clone()).unwrap();
    let shared = first.clone();
    let backend = |store: &CognitiveStore| -> i32 {
        store
            .client
            .lock()
            .unwrap()
            .query_one("SELECT pg_backend_pid()", &[])
            .unwrap()
            .get(0)
    };
    assert_eq!(backend(&first), backend(&shared));
    assert_eq!(shared.scope(), &scope);
    first
        .transact::<u64, _, StoreError>("fresh-row", &0, |n| {
            *n = 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        shared
            .transact::<u64, _, StoreError>("fresh-row", &0, |n| Ok(*n))
            .unwrap(),
        1
    );
    let independent = CognitiveStore::connect(&connection, scope.clone()).unwrap();
    assert_ne!(backend(&independent), backend(&shared));
    independent
        .transact::<u64, _, StoreError>("fresh-row", &0, |n| {
            *n = 2;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        shared
            .transact::<u64, _, StoreError>("fresh-row", &0, |n| Ok(*n))
            .unwrap(),
        2,
        "sharing a connection must not cache authority or journal rows"
    );
    let other = CognitiveStore::connect(
        &connection,
        ContextScopeId::new(format!("other-{nonce}")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        other
            .transact::<u64, _, StoreError>("fresh-row", &0, |n| Ok(*n))
            .unwrap(),
        0
    );
}
