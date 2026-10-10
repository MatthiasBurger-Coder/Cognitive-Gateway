//! Bounded single-worker dispatch. A lost observation never restarts application work.
use super::*;
use gateway_application::codex::RequestContext;
use serde::{Deserialize, Serialize};
use std::sync::mpsc;
use std::time::Instant;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLimits {
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub requests: usize,
    pub request_timeout_ms: u64,
    pub idle_timeout_ms: u64,
    pub write_timeout_ms: u64,
}
impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            input_bytes: MAX_FRAME_BYTES,
            output_bytes: MAX_FRAME_BYTES,
            requests: MAX_REQUESTS,
            request_timeout_ms: 30_000,
            idle_timeout_ms: 300_000,
            write_timeout_ms: 2_000,
        }
    }
}
impl RuntimeLimits {
    pub fn validate(&self) -> Result<(), TransportError> {
        if !(1024..=MAX_FRAME_BYTES).contains(&self.input_bytes)
            || !(1024..=MAX_FRAME_BYTES).contains(&self.output_bytes)
            || !(1..=MAX_REQUESTS).contains(&self.requests)
            || [
                self.request_timeout_ms,
                self.idle_timeout_ms,
                self.write_timeout_ms,
            ]
            .iter()
            .any(|v| !(1..=300_000).contains(v))
        {
            return Err(TransportError::Limit);
        }
        Ok(())
    }
}
#[derive(Clone, Default, Serialize)]
pub struct Metrics {
    pub completed: u64,
    pub failed: u64,
    pub rejected: u64,
    pub cancelled: u64,
    pub timed_out: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    None,
    Validation,
    Policy,
    Application,
    Runtime,
    Transport,
}
/// Fixed classes only; no diagnostic text, IDs, URIs, reasons or payloads retained.
pub fn failure_class(response: &Value) -> FailureClass {
    if let Some(code) = response["error"]["data"]["code"].as_str() {
        return match code {
            "CG_TIMEOUT" | "CG_CANCELLED" | "CG_LIMIT_EXCEEDED" | "CG_OVERLOADED" => {
                FailureClass::Runtime
            }
            "CG_INTERNAL_ERROR" => FailureClass::Application,
            "CG_SCOPE_DENIED"
            | "CG_SENSITIVITY_DENIED"
            | "CG_POLICY_DENIED"
            | "CG_CONSENT_REQUIRED"
            | "CG_EVIDENCE_REQUIRED" => FailureClass::Policy,
            "CG_PARSE_ERROR"
            | "CG_INVALID_REQUEST"
            | "CG_UNKNOWN_METHOD"
            | "CG_INVALID_PARAMS"
            | "CG_SESSION_UNAVAILABLE" => FailureClass::Validation,
            _ => FailureClass::Transport,
        };
    }
    if response.get("error").is_some() {
        return FailureClass::Validation;
    }
    match response["result"]["structuredContent"]["diagnostics"][0]["code"].as_str() {
        Some(
            "CG_POLICY_DENIED"
            | "CG_CONSENT_REQUIRED"
            | "CG_EVIDENCE_REQUIRED"
            | "CG_SCOPE_DENIED"
            | "CG_SENSITIVITY_DENIED",
        ) => FailureClass::Policy,
        Some(
            "CG_INVALID_REQUEST"
            | "CG_INVALID_INPUT"
            | "CG_UNSUPPORTED_VERSION"
            | "CG_UNKNOWN_OPERATION",
        ) => FailureClass::Validation,
        Some(_) => FailureClass::Application,
        None => FailureClass::None,
    }
}
/// Stop serialization as soon as a byte limit would be exceeded.
pub(super) fn bounded_json(value: &Value, max: usize) -> Result<Vec<u8>, TransportError> {
    struct Buffer {
        bytes: Vec<u8>,
        max: usize,
    }
    impl std::io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.bytes.len().saturating_add(bytes.len()) > self.max {
                return Err(std::io::ErrorKind::FileTooLarge.into());
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Buffer {
        bytes: Vec::new(),
        max,
    };
    serde_json::to_writer(&mut output, value).map_err(|_| TransportError::Limit)?;
    Ok(output.bytes)
}
pub(super) fn diagnostic(id: Value, code: &'static str, correlation: &str) -> Value {
    let mut error = rpc_error(id, -32000, "Local runtime rejected request");
    error["error"]["data"] = json!({"code":code,"class":"runtime","correlation_id":correlation,"retry":false,"outcome":"unknown_if_dispatched","task_observation":"detached"});
    error
}
struct CancelOnDrop(RequestContext);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
impl Server {
    pub fn with_limits(mut self, limits: RuntimeLimits) -> Result<Self, TransportError> {
        limits.validate()?;
        self.limits = limits;
        Ok(self)
    }
    pub fn health(&self) -> Value {
        json!({"healthy":self.phase != Phase::Closed,"ready":self.phase == Phase::Ready,
            "application":"requires_admitted_host","max_in_flight":1,"queue_capacity":0,
            "disconnect":"cancel_invocation_detach_task_observation","automatic_retry":false,
            "limits":self.limits,"metrics":self.metrics})
    }
    fn emit(
        &mut self,
        transport: &mut impl Transport,
        mut response: Value,
        timeout: Duration,
        correlation: &str,
        started: Instant,
    ) -> Result<(), TransportError> {
        if let Some(result) = response.get_mut("result") {
            result["_meta"] = json!({"cg/correlation_id":correlation});
        } else if response["error"].is_object() {
            if !response["error"]["data"].is_object() {
                response["error"]["data"] = json!({});
            }
            response["error"]["data"]["correlation_id"] = json!(correlation);
        }
        let (bytes, class) = match bounded_json(&response, self.limits.output_bytes - 1) {
            Ok(bytes) => (bytes, failure_class(&response)),
            Err(_) => {
                self.metrics.rejected += 1;
                let mut fallback =
                    diagnostic(response["id"].clone(), "CG_LIMIT_EXCEEDED", correlation);
                let bytes = match bounded_json(&fallback, self.limits.output_bytes - 1) {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        fallback["id"] = Value::Null;
                        bounded_json(&fallback, self.limits.output_bytes - 1)?
                    }
                };
                (bytes, FailureClass::Runtime)
            }
        };
        self.metrics.completed += 1;
        self.metrics.failed += u64::from(class != FailureClass::None);
        eprintln!(
            "{}",
            json!({"event":"local_call_finished","correlation_id":correlation,
            "failure_class":class,"elapsed_ms":started.elapsed().as_millis()})
        );
        transport.send(bytes, timeout)
    }
    fn interruption(
        &mut self,
        context: &RequestContext,
        error: gateway_application::codex::FacadeError,
    ) -> &'static str {
        if error == gateway_application::codex::FacadeError::Cancelled {
            self.metrics.cancelled += 1;
        } else {
            self.metrics.timed_out += 1;
        }
        context.cancel();
        error.code()
    }
    pub(super) fn bounded_serve(
        &mut self,
        transport: &mut impl Transport,
        read_timeout: Duration,
        write_timeout: Duration,
    ) -> Result<(), TransportError> {
        let read_timeout = read_timeout.min(Duration::from_millis(self.limits.idle_timeout_ms));
        let write_timeout = write_timeout.min(Duration::from_millis(self.limits.write_timeout_ms));
        let mut sequence = 0u64;
        while self.phase != Phase::Closed {
            let Some(frame) = transport.receive(read_timeout)? else {
                break;
            };
            if frame.len().saturating_add(1) > self.limits.input_bytes {
                return Err(TransportError::Limit);
            }
            sequence += 1;
            if sequence > self.limits.requests as u64 {
                return Err(TransportError::Limit);
            }
            static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            let correlation = format!(
                "cg-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
                NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            let started = Instant::now();
            let decoded = decode::decode(&frame).ok();
            let application_call = decoded.as_ref().is_some_and(|v| {
                security::credential_free(v)
                    && (v["method"] == "tools/call"
                        || (v["method"] == "resources/read"
                            && !v["params"]["uri"].as_str().is_some_and(|u| {
                                u.starts_with("cg://contracts/") || u == "cg://runtime/health"
                            })))
            });
            if !application_call {
                if let Some(response) = self.handle(&frame) {
                    self.emit(transport, response, write_timeout, &correlation, started)?;
                }
                continue;
            }
            let id = decoded.as_ref().unwrap()["id"].clone();
            let context = RequestContext::new(
                correlation.clone(),
                started + Duration::from_millis(self.limits.request_timeout_ms),
            );
            let _cancel_on_exit = CancelOnDrop(context.clone());
            let mut rejected_ids = BTreeSet::new();
            let mut worker = std::mem::replace(self, Server::new(self.binding.clone()));
            // Preserve configuration for the observer; worker alone owns application state.
            self.limits = worker.limits.clone();
            self.metrics = worker.metrics.clone();
            worker.context = Some(context.clone());
            let (tx, rx) = mpsc::sync_channel(1);
            std::thread::Builder::new()
                .spawn(move || {
                    let response = worker.handle(&frame);
                    worker.context = None;
                    let _ = tx.send((worker, response));
                })
                .map_err(|_| TransportError::Io)?;
            loop {
                match rx.recv_timeout(
                    Duration::from_millis(1)
                        .min(context.deadline.saturating_duration_since(Instant::now())),
                ) {
                    Ok((worker, response)) => {
                        let metrics = self.metrics.clone();
                        *self = worker;
                        self.metrics = metrics;
                        self.seen.extend(rejected_ids);
                        if let Err(error) = context.check() {
                            let code = self.interruption(&context, error);
                            self.emit(
                                transport,
                                diagnostic(id, code, &correlation),
                                write_timeout,
                                &correlation,
                                started,
                            )?;
                            self.phase = Phase::Closed;
                        } else if let Some(response) = response {
                            self.emit(transport, response, write_timeout, &correlation, started)?;
                        }
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        context.cancel();
                        self.emit(
                            transport,
                            diagnostic(id, "CG_INTERNAL_ERROR", &correlation),
                            write_timeout,
                            &correlation,
                            started,
                        )?;
                        self.phase = Phase::Closed;
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if let Err(error) = context.check() {
                    let code = self.interruption(&context, error);
                    self.emit(
                        transport,
                        diagnostic(id, code, &correlation),
                        write_timeout,
                        &correlation,
                        started,
                    )?;
                    self.phase = Phase::Closed;
                    break;
                }
                match transport.receive(
                    Duration::from_millis(2)
                        .min(context.deadline.saturating_duration_since(Instant::now())),
                ) {
                    Ok(Some(frame)) => {
                        sequence += 1;
                        if sequence > self.limits.requests as u64 {
                            return Err(TransportError::Limit);
                        }
                        if frame.len().saturating_add(1) > self.limits.input_bytes {
                            context.cancel();
                            return Err(TransportError::Limit);
                        }
                        let value = decode::decode(&frame)
                            .ok()
                            .filter(security::credential_free);
                        if let Some(value) = value {
                            if value["jsonrpc"] == "2.0"
                                && value["method"] == "notifications/cancelled"
                                && value.get("id").is_none()
                                && params_object(
                                    &value,
                                    &["jsonrpc", "method", "params"],
                                    &["jsonrpc", "method", "params"],
                                )
                                && params_object(
                                    &value["params"],
                                    &["requestId", "reason"],
                                    &["requestId"],
                                )
                                && value["params"].get("reason").is_none_or(Value::is_string)
                                && value["params"]["requestId"] == id
                            {
                                context.cancel();
                                self.metrics.cancelled += 1;
                                self.emit(
                                    transport,
                                    diagnostic(id, "CG_CANCELLED", &correlation),
                                    write_timeout,
                                    &correlation,
                                    started,
                                )?;
                                self.phase = Phase::Closed;
                                break;
                            }
                            if let Some(other_id) = value.get("id").filter(|id| request_id(id)) {
                                rejected_ids.insert(other_id.to_string());
                                self.metrics.rejected += 1;
                                self.emit(
                                    transport,
                                    diagnostic(
                                        other_id.clone(),
                                        "CG_OVERLOADED",
                                        &format!("{correlation}-busy"),
                                    ),
                                    write_timeout,
                                    &correlation,
                                    started,
                                )?;
                            }
                        }
                    }
                    Ok(None) => {
                        context.cancel();
                        self.phase = Phase::Closed;
                        break;
                    }
                    Err(TransportError::Timeout) => {}
                    Err(error) => {
                        context.cancel();
                        return Err(error);
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::mpsc::{Receiver, Sender};
    struct Host {
        started: Sender<RequestContext>,
        delay: Duration,
        panic: bool,
    }
    impl CodexApplicationPort for Host {
        fn execute(&self, _: &str, _: &Value) -> Value {
            contracts::failure("CG_UNSUPPORTED_CAPABILITY")
        }
        fn execute_with_context(
            &self,
            operation: &str,
            request: &Value,
            context: &RequestContext,
        ) -> Value {
            self.started.send(context.clone()).unwrap();
            if self.panic {
                panic!("fault injected");
            }
            if self.delay.is_zero() {
                context.cancel();
            }
            std::thread::sleep(self.delay);
            self.execute(operation, request)
        }
    }
    enum Fault {
        Frame(Value),
        Eof,
        Error(TransportError),
        Oversize,
        Wait,
    }
    struct Wire {
        first: Option<Vec<u8>>,
        faults: VecDeque<Fault>,
        started: Receiver<RequestContext>,
        context: Option<RequestContext>,
        output: Vec<Value>,
        write_error: bool,
    }
    impl Transport for Wire {
        fn receive(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, TransportError> {
            if let Some(frame) = self.first.take() {
                return Ok(Some(frame));
            }
            if self.output.iter().any(|v| v["id"] == 42) && self.faults.is_empty() {
                return Ok(None);
            }
            if self.context.is_none() {
                self.context = self.started.recv_timeout(Duration::from_secs(1)).ok();
            }
            match self.faults.pop_front().unwrap_or(Fault::Wait) {
                Fault::Frame(value) => Ok(Some(value.to_string().into_bytes())),
                Fault::Eof => Ok(None),
                Fault::Error(error) => Err(error),
                Fault::Oversize => Ok(Some(vec![b' '; MAX_FRAME_BYTES])),
                Fault::Wait => {
                    std::thread::sleep(timeout);
                    Err(TransportError::Timeout)
                }
            }
        }
        fn send(&mut self, frame: Vec<u8>, _: Duration) -> Result<(), TransportError> {
            self.output.push(serde_json::from_slice(&frame).unwrap());
            if self.write_error {
                Err(TransportError::Io)
            } else {
                Ok(())
            }
        }
    }
    fn setup(delay: u64, panic: bool, faults: Vec<Fault>) -> (Server, Wire) {
        let (started, receiver) = mpsc::channel();
        let mut server = Server::with_application(
            super::super::tests::binding(),
            Box::new(Host {
                started,
                delay: Duration::from_millis(delay),
                panic,
            }),
        );
        server.phase = Phase::Ready;
        let request: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/codex-v1/registry.inspect.request.json"
        ))
        .unwrap();
        let first = json!({"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"name":"cg_registry_inspect_v1","arguments":request}}).to_string().into_bytes();
        (
            server,
            Wire {
                first: Some(first),
                faults: faults.into(),
                started: receiver,
                context: None,
                output: vec![],
                write_error: false,
            },
        )
    }
    fn serve(server: &mut Server, wire: &mut Wire) -> Result<(), TransportError> {
        server.serve(wire, Duration::from_secs(1), Duration::from_secs(1))
    }
    #[test]
    fn active_cancel_disconnect_errors_and_deadline_are_bounded() {
        for fault in [
            Fault::Frame(
                json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":42,"reason":"private text"}}),
            ),
            Fault::Eof,
            Fault::Error(TransportError::Io),
            Fault::Oversize,
        ] {
            let (mut server, mut wire) = setup(100, false, vec![fault]);
            let started = Instant::now();
            let _ = serve(&mut server, &mut wire);
            assert!(started.elapsed() < Duration::from_secs(1));
            assert_eq!(
                wire.context.unwrap().check(),
                Err(gateway_application::codex::FacadeError::Cancelled)
            );
            assert_eq!(server.phase, Phase::Closed);
            assert!(
                !serde_json::to_string(&wire.output)
                    .unwrap()
                    .contains("private text")
            );
        }
        let (mut server, mut wire) = setup(100, false, vec![Fault::Wait]);
        server.limits.request_timeout_ms = 5;
        serve(&mut server, &mut wire).unwrap();
        assert_eq!(wire.output[0]["error"]["data"]["code"], "CG_TIMEOUT");
        assert_eq!(server.metrics.timed_out, 1);
        assert_eq!(
            wire.context.unwrap().check(),
            Err(gateway_application::codex::FacadeError::Cancelled)
        );
    }
    #[test]
    fn host_cancellation_is_distinguished_and_error_response_is_always_bounded() {
        let (mut server, mut wire) = setup(0, false, vec![]);
        serve(&mut server, &mut wire).unwrap();
        assert_eq!(wire.output[0]["error"]["data"]["code"], "CG_CANCELLED");
        assert_eq!(server.metrics.cancelled, 1);
        server.limits.output_bytes = 1024;
        server
            .emit(
                &mut wire,
                result(
                    json!("\u{0001}".repeat(128)),
                    json!({"large":"x".repeat(2048)}),
                ),
                Duration::from_secs(1),
                &"c".repeat(128),
                Instant::now(),
            )
            .unwrap();
        assert!(wire.output.last().unwrap().to_string().len() < 1024);
        assert!(wire.output.last().unwrap()["id"].is_null());
        assert_eq!(server.metrics.failed, 2);
        for error in [
            TransportError::Io,
            TransportError::Frame,
            TransportError::Limit,
            TransportError::Timeout,
        ] {
            assert!(error.code().starts_with("CG_"));
        }
    }
    #[test]
    fn overload_and_unknown_cancellation_do_not_restart_work() {
        let (mut server, mut wire) = setup(
            100,
            false,
            vec![
                Fault::Frame(json!({"jsonrpc":"2.0","id":43,"method":"ping"})),
                Fault::Frame(
                    json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":99}}),
                ),
                Fault::Frame(
                    json!({"jsonrpc":"2.0","id":"sk-proj-FAKE_CREDENTIAL_0123456789","method":"ping"}),
                ),
                Fault::Frame(json!({"jsonrpc":"2.0","id":{},"method":"ping"})),
                Fault::Frame(json!({"not":"rpc"})),
                Fault::Wait,
            ],
        );
        serve(&mut server, &mut wire).unwrap();
        assert_eq!(wire.output.len(), 2);
        assert_eq!(wire.output[0]["error"]["data"]["code"], "CG_OVERLOADED");
        assert!(wire.output[1]["result"].is_object());
        assert!(server.seen.contains("43"));
        assert_eq!(server.metrics.rejected, 1);
        assert_eq!(server.metrics.completed, 2);
    }
    #[test]
    fn panic_write_failure_and_frame_budget_close_without_retry() {
        let (mut server, mut wire) = setup(0, true, vec![]);
        serve(&mut server, &mut wire).unwrap();
        assert_eq!(wire.output[0]["error"]["data"]["code"], "CG_INTERNAL_ERROR");
        let (mut server, mut wire) = setup(
            100,
            false,
            vec![Fault::Frame(
                json!({"jsonrpc":"2.0","id":43,"method":"ping"}),
            )],
        );
        wire.write_error = true;
        assert_eq!(serve(&mut server, &mut wire), Err(TransportError::Io));
        assert_eq!(
            wire.context.unwrap().check(),
            Err(gateway_application::codex::FacadeError::Cancelled)
        );
        for faults in [vec![], vec![Fault::Wait]] {
            let (mut server, mut wire) = setup(10, false, faults);
            server.limits.requests = 0;
            assert_eq!(serve(&mut server, &mut wire), Err(TransportError::Limit));
        }
        let (mut server, mut wire) = setup(
            100,
            false,
            vec![Fault::Frame(
                json!({"jsonrpc":"2.0","id":43,"method":"ping"}),
            )],
        );
        server.limits.requests = 1;
        assert_eq!(serve(&mut server, &mut wire), Err(TransportError::Limit));
    }
    #[test]
    fn limits_health_output_and_failure_classes_are_explicit() {
        let defaults = RuntimeLimits::default();
        defaults.validate().unwrap();
        for limits in [
            RuntimeLimits {
                input_bytes: 0,
                ..defaults.clone()
            },
            RuntimeLimits {
                output_bytes: MAX_FRAME_BYTES + 1,
                ..defaults.clone()
            },
            RuntimeLimits {
                requests: 0,
                ..defaults.clone()
            },
            RuntimeLimits {
                request_timeout_ms: 0,
                ..defaults.clone()
            },
        ] {
            assert_eq!(limits.validate(), Err(TransportError::Limit));
            assert!(super::super::tests::ready().with_limits(limits).is_err());
        }
        let mut server = super::super::tests::ready().with_limits(defaults).unwrap();
        assert_eq!(server.health()["ready"], true);
        let request = json!({"jsonrpc":"2.0","id":98,"method":"resources/read","params":{"uri":"cg://runtime/health"}});
        assert!(
            server.handle(request.to_string().as_bytes()).unwrap()["result"]["contents"][0]["text"]
                .as_str()
                .unwrap()
                .contains("queue_capacity")
        );
        assert_eq!(
            failure_class(&json!({"error":{"data":{"code":"other"}}})),
            FailureClass::Transport
        );
        assert_eq!(
            failure_class(&json!({"error":{}})),
            FailureClass::Validation
        );
        for (code, expected) in [
            ("CG_POLICY_DENIED", FailureClass::Policy),
            ("CG_INVALID_INPUT", FailureClass::Validation),
            ("CG_INTERNAL_ERROR", FailureClass::Application),
        ] {
            assert_eq!(
                failure_class(&json!({"result":{"structuredContent":contracts::failure(code)}})),
                expected
            );
        }
        let mut wire = Wire {
            first: None,
            faults: vec![Fault::Eof].into(),
            started: mpsc::channel().1,
            context: None,
            output: vec![],
            write_error: false,
        };
        server.limits.output_bytes = 1024;
        server
            .emit(
                &mut wire,
                result(json!(1), json!({"large":"x".repeat(2048)})),
                Duration::from_secs(1),
                "safe",
                Instant::now(),
            )
            .unwrap();
        assert_eq!(wire.output[0]["error"]["data"]["code"], "CG_LIMIT_EXCEEDED");
        assert!(!server.health().to_string().contains("large"));
    }
}
