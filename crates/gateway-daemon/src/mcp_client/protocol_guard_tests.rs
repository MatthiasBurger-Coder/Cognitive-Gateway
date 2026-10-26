use super::*;
use std::sync::Mutex;

struct Peer {
    frames: Arc<Mutex<Vec<Value>>>,
    response: Option<Vec<u8>>,
}
impl Transport for Peer {
    fn send(&mut self, bytes: &[u8], _: &CallContext) -> Result<(), Error> {
        // Deliberately no guard here: the protocol must guard before entry.
        let value: Value = serde_json::from_slice(bytes).unwrap();
        if let Some(id) = value.get("id") {
            self.response = Some(
                serde_json::to_vec(&json!({
                    "jsonrpc":"2.0", "id":id, "result":{}
                }))
                .unwrap(),
            );
        }
        self.frames.lock().unwrap().push(value);
        Ok(())
    }
    fn receive(&mut self, _: &CallContext) -> Result<Vec<u8>, Error> {
        self.response.take().ok_or(Error::Timeout)
    }
    fn close(&mut self) {}
}

fn context() -> CallContext {
    CallContext::new(
        NonEmptyText::new("protocol-guard").unwrap(),
        Instant::now() + Duration::from_secs(1),
    )
}

#[test]
fn rejected_guard_never_marks_an_attempt_and_counter_is_checked_and_persistent() {
    let frames = Arc::new(Mutex::new(Vec::new()));
    let mut session = Session::new(Limits::default(), Arc::new(()), "1.0.0".into());
    session.transport = Some(Box::new(Peer {
        frames: frames.clone(),
        response: None,
    }));
    let denied = context().with_guard(Arc::new(|| {
        Err(Error::SessionDenied(ConnectorFailureCode::PermissionDenied))
    }));
    assert_eq!(
        session.rpc("ping", json!({}), &denied),
        Err(Error::SessionDenied(ConnectorFailureCode::PermissionDenied))
    );
    assert_eq!(session.send_attempts, 0);
    assert!(!session.sent);
    assert!(frames.lock().unwrap().is_empty());

    session.rpc("ping", json!({}), &context()).unwrap();
    session
        .send(json!({"jsonrpc":"2.0", "method":"notice"}), &context())
        .unwrap();
    assert_eq!(session.send_attempts, 2);
    session.close(SessionState::Disconnected, &context());
    assert_eq!(
        session.send_attempts, 2,
        "closing cannot erase prior attempts"
    );
    session.transport = Some(Box::new(Peer {
        frames: frames.clone(),
        response: None,
    }));
    session.send_attempts = u64::MAX;
    assert_eq!(
        session.rpc("ping", json!({}), &context()),
        Err(Error::Limit)
    );
    assert!(!session.sent);
    assert_eq!(session.send_attempts, u64::MAX);
    assert_eq!(frames.lock().unwrap().len(), 2);
}

struct RevokeConnecting(Arc<AtomicBool>);
impl Observer for RevokeConnecting {
    fn record(&self, event: Event) {
        if event.kind == EventKind::State(SessionState::Connecting) {
            self.0.store(false, Ordering::Release);
        }
    }
}
struct Factory(Arc<AtomicBool>);
impl TransportFactory for Factory {
    fn open(&self, _: &CallContext, _: &Limits) -> Result<Box<dyn Transport>, Error> {
        self.0.store(true, Ordering::Release);
        Err(Error::Unavailable)
    }
}

#[test]
fn observer_revocation_before_open_does_not_enter_transport_factory() {
    let live = Arc::new(AtomicBool::new(true));
    let opened = Arc::new(AtomicBool::new(false));
    let admission = live.clone();
    let guarded = context().with_guard(Arc::new(move || {
        if admission.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(Error::SessionDenied(ConnectorFailureCode::PermissionDenied))
        }
    }));
    let mut session = Session::new(
        Limits::default(),
        Arc::new(RevokeConnecting(live)),
        "1.0.0".into(),
    );
    assert_eq!(
        session.connect(&Factory(opened.clone()), &guarded),
        Err(Error::SessionDenied(ConnectorFailureCode::PermissionDenied))
    );
    assert!(!opened.load(Ordering::Acquire));
    assert_eq!(session.send_attempts, 0);
}
