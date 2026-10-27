use super::*;
use std::{collections::VecDeque, sync::Mutex};
struct Peer {
    input: VecDeque<Result<Vec<u8>, Error>>,
    output: Arc<Mutex<Vec<Value>>>,
    send_error: Option<Error>,
    delay_past_receive_deadline: bool,
}
impl Transport for Peer {
    fn send(&mut self, frame: &[u8], _: &CallContext) -> Result<(), Error> {
        self.output
            .lock()
            .unwrap()
            .push(serde_json::from_slice(frame).unwrap());
        self.send_error.map_or(Ok(()), Err)
    }
    fn receive(&mut self, context: &CallContext) -> Result<Vec<u8>, Error> {
        let frame = self.input.pop_front().unwrap_or(Err(Error::Timeout));
        if self.delay_past_receive_deadline && frame.is_ok() {
            std::thread::sleep(
                context.deadline.saturating_duration_since(Instant::now())
                    + Duration::from_millis(1),
            );
        }
        frame
    }
    fn receive_with_wait_deadline(
        &mut self,
        context: &CallContext,
        wait_deadline: Instant,
    ) -> Result<Vec<u8>, Error> {
        if self.delay_past_receive_deadline {
            std::thread::sleep(
                wait_deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
            );
        }
        context.check()?;
        self.input.pop_front().unwrap_or(Err(Error::Timeout))
    }
    fn close(&mut self) {}
}
fn context() -> CallContext {
    CallContext::new(
        NonEmptyText::new("protocol-test").unwrap(),
        Instant::now() + Duration::from_secs(1),
    )
}
fn session(values: Vec<Value>) -> (Session, Arc<Mutex<Vec<Value>>>) {
    let output = Arc::new(Mutex::new(Vec::new()));
    let mut session = Session::new(Limits::default(), Arc::new(()), "1.0.0".into());
    session.transport = Some(Box::new(Peer {
        input: values
            .into_iter()
            .map(|v| Ok(serde_json::to_vec(&v).unwrap()))
            .collect(),
        output: output.clone(),
        send_error: None,
        delay_past_receive_deadline: false,
    }));
    session.state = SessionState::Ready;
    (session, output)
}
#[test]
fn malformed_rpc_envelopes_and_error_details_are_rejected() {
    let invalid = [
        json!([]),
        json!({}),
        json!({"jsonrpc":"1.0","id":1,"result":{}}),
        json!({"jsonrpc":"2.0","id":1,"result":{},"error":{}}),
        json!({"jsonrpc":"2.0","id":1}),
        json!({"jsonrpc":"2.0","id":"1","result":{}}),
        json!({"jsonrpc":"2.0","id":1,"result":[]}),
        json!({"jsonrpc":"2.0","id":1,"error":{"code":"bad","message":"secret"}}),
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-1}}),
        json!({"jsonrpc":"2.0","method":false}),
        json!({"jsonrpc":"2.0","method":"ping","id":null}),
        json!({"jsonrpc":"2.0","method":"ping","result":{}}),
    ];
    for value in invalid {
        let (mut s, _) = session(vec![value.clone()]);
        assert_eq!(
            s.rpc("ping", json!({}), &context()),
            Err(Error::Protocol),
            "{value}"
        );
    }
    let (mut s, _) = session(vec![
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"secret","data":"secret"}}),
    ]);
    assert_eq!(
        s.rpc("ping", json!({}), &context()),
        Err(Error::Remote(-32000))
    );
}
#[test]
fn cancellation_id_rules_and_no_initialize_cancellation() {
    for method in ["initialize", "tools/call"] {
        let (mut s, writes) = session(vec![]);
        assert_eq!(s.rpc(method, json!({}), &context()), Err(Error::Timeout));
        let writes = writes.lock().unwrap();
        assert_eq!(writes.len(), if method == "initialize" { 1 } else { 2 });
        if method != "initialize" {
            assert_eq!(writes[1]["params"]["requestId"], writes[0]["id"]);
        }
    }
    let (mut s, _) = session(vec![
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}),
    ]);
    assert_eq!(
        s.rpc("tools/call", json!({}), &context()),
        Err(Error::Cancelled)
    );
    let (mut s, _) = session(vec![
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":999}}),
        json!({"jsonrpc":"2.0","id":1,"result":{}}),
    ]);
    s.rpc("ping", json!({}), &context()).unwrap();
}
#[test]
fn queued_notifications_invalidate_before_any_effect_write() {
    let (mut s, output) = session(vec![
        json!({"jsonrpc":"2.0","method":"notifications/resources/list_changed"}),
    ]);
    s.inventory = Some(ProtocolInventory::default());
    assert_eq!(s.refresh(&context()), Err(Error::InventoryChanged));
    assert!(s.inventory.is_none());
    assert!(output.lock().unwrap().is_empty());
    let (mut s, _) = session(vec![json!({"jsonrpc":"2.0","id":0,"result":{}})]);
    assert_eq!(s.refresh(&context()), Err(Error::Protocol));
    let (mut s, output) = session(vec![
        json!({"jsonrpc":"2.0","method":"notifications/progress","params":{}}),
    ]);
    s.refresh(&context()).unwrap();
    assert!(output.lock().unwrap().is_empty());
}
#[test]
fn frame_event_id_and_write_limits_are_explicit() {
    let (mut s, _) = session(vec![]);
    s.next_id = u64::MAX;
    assert_eq!(s.rpc("ping", json!({}), &context()), Err(Error::Limit));
    let (mut s, _) = session(vec![]);
    s.limits.frame_bytes = 1;
    assert_eq!(s.rpc("ping", json!({}), &context()), Err(Error::Limit));
    assert!(!s.sent);
    assert_eq!(s.send(json!({}), &context()), Err(Error::Limit));
    let (mut s, _) = session(vec![
        json!({"jsonrpc":"2.0","id":1,"result":{"large":"x".repeat(500)}}),
    ]);
    s.limits.frame_bytes = 100;
    assert_eq!(s.rpc("ping", json!({}), &context()), Err(Error::Limit));
    let (mut s, _) = session(vec![]);
    s.transport = None;
    assert_eq!(s.send(json!({}), &context()), Err(Error::InvalidState));
    assert_eq!(
        s.rpc("ping", json!({}), &context()),
        Err(Error::InvalidState)
    );
    assert_eq!(s.response(1, &context()), Err(Error::InvalidState));
    let (mut s, _) = session(vec![
        json!({"jsonrpc":"2.0","method":"notifications/progress"});
        4
    ]);
    s.limits.events_per_request = 3;
    assert_eq!(s.rpc("ping", json!({}), &context()), Err(Error::Limit));
    s.state = SessionState::Disabled;
    struct Closed;
    impl TransportFactory for Closed {
        fn open(&self, _: &CallContext, _: &Limits) -> Result<Box<dyn Transport>, Error> {
            Err(Error::Unavailable)
        }
    }
    assert_eq!(s.connect(&Closed, &context()), Err(Error::InvalidState));
}
struct Init(Value);
impl TransportFactory for Init {
    fn open(&self, _: &CallContext, _: &Limits) -> Result<Box<dyn Transport>, Error> {
        Ok(Box::new(Peer {
            input: VecDeque::from([Ok(serde_json::to_vec(
                &json!({"jsonrpc":"2.0","id":1,"result":self.0}),
            )
            .unwrap())]),
            output: Arc::new(Mutex::new(vec![])),
            send_error: None,
            delay_past_receive_deadline: false,
        }))
    }
}
#[test]
fn initialize_validates_capabilities_and_manifest_version() {
    let base = json!({"protocolVersion":MCP_VERSION,"serverInfo":{"name":"peer","version":"1.0.0"},"capabilities":{}});
    for (path, value, expected) in [
        ("serverInfo", json!({}), Error::Protocol),
        ("capabilities", json!([]), Error::Protocol),
        ("capabilities", json!({"tools":false}), Error::Protocol),
        (
            "serverInfo",
            json!({"name":"peer","version":"2.0.0"}),
            Error::IncompatibleVersion,
        ),
    ] {
        let mut input = base.clone();
        input[path] = value;
        let mut s = Session::new(Limits::default(), Arc::new(()), "1.0.0".into());
        assert_eq!(s.connect(&Init(input), &context()), Err(expected));
        assert_eq!(s.state, SessionState::Degraded);
    }
}
#[test]
fn discovery_rejects_invalid_pages_and_cursor_cycles_atomically() {
    let invalid = [
        json!({}),
        json!({"tools":[{}]}),
        json!({"tools":[{"name":"t","inputSchema":false}]}),
        json!({"tools":[],"nextCursor":null}),
        json!({"tools":[],"nextCursor":""}),
    ];
    for page in invalid {
        let (mut s, _) = session(vec![json!({"jsonrpc":"2.0","id":1,"result":page})]);
        s.capabilities = json!({"tools":{}});
        assert!(s.discover(&context()).is_err());
        assert!(s.inventory.is_none());
    }
    let (mut s, _) = session(vec![
        json!({"jsonrpc":"2.0","id":1,"result":{"tools":[],"nextCursor":"same"}}),
        json!({"jsonrpc":"2.0","id":2,"result":{"tools":[],"nextCursor":"same"}}),
    ]);
    s.capabilities = json!({"tools":{}});
    assert_eq!(s.discover(&context()), Err(Error::Limit));
}

#[test]
fn received_inventory_invalidation_survives_expired_refresh_poll() {
    let (mut session, output) = session(vec![]);
    session.transport = Some(Box::new(Peer {
        input: VecDeque::from([Ok(serde_json::to_vec(&json!({
            "jsonrpc":"2.0","method":"notifications/tools/list_changed"
        }))
        .unwrap())]),
        output: output.clone(),
        send_error: None,
        delay_past_receive_deadline: true,
    }));
    session.inventory = Some(ProtocolInventory::default());
    assert_eq!(session.refresh(&context()), Err(Error::InventoryChanged));
    assert!(session.inventory.is_none());
    assert!(output.lock().unwrap().is_empty());
}
