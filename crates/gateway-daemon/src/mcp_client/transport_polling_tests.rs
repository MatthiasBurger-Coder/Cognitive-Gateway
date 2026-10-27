use super::*;
use gateway_domain::NonEmptyText;

fn buffered_transport(pending: &[u8]) -> StdioTransport {
    let executor = Builder::new_current_thread().enable_all().build().unwrap();
    let mut child = {
        let _entered = executor.enter();
        Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap()
    };
    StdioTransport {
        input: child.stdin.take(),
        output: child.stdout.take().unwrap(),
        child,
        executor,
        pending: pending.to_vec(),
        max_frame: 1024,
        shutdown: Duration::from_millis(20),
    }
}
fn active_context() -> CallContext {
    CallContext::new(
        NonEmptyText::new("buffered-poll").unwrap(),
        Instant::now() + Duration::from_secs(1),
    )
}

#[test]
fn expired_wait_drains_complete_frames_but_preserves_active_guards_and_partial_bytes() {
    let frame = br#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#;
    let mut bytes = frame.to_vec();
    bytes.push(b'\n');
    let mut transport = buffered_transport(&bytes);
    let wait_end = Instant::now();
    assert_eq!(
        transport.receive_with_wait_deadline(&active_context(), wait_end),
        Ok(frame.to_vec())
    );
    assert!(transport.pending.is_empty());

    transport.pending = bytes.clone();
    let cancelled = active_context();
    cancelled.cancel();
    assert_eq!(
        transport.receive_with_wait_deadline(&cancelled, wait_end),
        Err(Error::Cancelled)
    );
    assert_eq!(transport.pending, bytes);
    let expired = CallContext::new(NonEmptyText::new("expired").unwrap(), Instant::now());
    assert_eq!(
        transport.receive_with_wait_deadline(&expired, wait_end),
        Err(Error::Timeout)
    );
    assert_eq!(transport.pending, bytes);
    let denied = active_context().with_guard(std::sync::Arc::new(|| {
        Err(Error::SessionDenied(
            gateway_application::connectors::ConnectorFailureCode::PermissionDenied,
        ))
    }));
    assert!(matches!(
        transport.receive_with_wait_deadline(&denied, wait_end),
        Err(Error::SessionDenied(_))
    ));
    assert_eq!(transport.pending, bytes);

    transport.pending = b"partial".to_vec();
    assert_eq!(
        transport.receive_with_wait_deadline(&active_context(), wait_end),
        Err(Error::Timeout)
    );
    assert_eq!(transport.pending, b"partial");
}

#[test]
fn an_unqualified_custom_transport_cannot_silently_skip_inventory_polling() {
    struct Legacy;
    impl Transport for Legacy {
        fn send(&mut self, _: &[u8], _: &CallContext) -> Result<(), Error> {
            panic!("polling must not manufacture a send")
        }
        fn receive(&mut self, _: &CallContext) -> Result<Vec<u8>, Error> {
            panic!("legacy blocking receive cannot implement bounded refresh")
        }
        fn close(&mut self) {}
    }
    let mut transport = Legacy;
    assert_eq!(
        transport.receive_with_wait_deadline(&active_context(), Instant::now()),
        Err(Error::Unsupported)
    );
}

#[test]
fn complete_oversized_frames_and_oversized_writes_are_denied_before_io() {
    let mut transport = buffered_transport(b"oversized\nok\n");
    transport.max_frame = 4;
    assert_eq!(
        transport.receive_with_wait_deadline(&active_context(), Instant::now()),
        Err(Error::Limit)
    );
    assert_eq!(transport.pending, b"ok\n");
    assert_eq!(
        transport.send(b"oversized", &active_context()),
        Err(Error::Limit)
    );
    assert_eq!(transport.pending, b"ok\n");
    assert_eq!(
        transport.receive_with_wait_deadline(&active_context(), Instant::now()),
        Ok(b"ok".to_vec())
    );
}
