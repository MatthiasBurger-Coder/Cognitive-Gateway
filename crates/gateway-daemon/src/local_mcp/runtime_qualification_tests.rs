// Deterministic fault cases included only inside runtime's existing test module.
struct PendingHost {
    started: Sender<RequestContext>,
}
impl CodexApplicationPort for PendingHost {
    fn execute(&self, _: &str, _: &Value) -> Value {
        contracts::failure("CG_UNSUPPORTED_CAPABILITY")
    }
    fn execute_with_context(
        &self,
        operation: &str,
        request: &Value,
        context: &RequestContext,
    ) -> Value {
        let _ = self.started.send(context.clone());
        let until = Instant::now() + Duration::from_secs(2);
        while context.check() != Err(gateway_application::codex::FacadeError::Cancelled) {
            assert!(
                Instant::now() < until,
                "pending application must receive cancellation"
            );
            std::thread::yield_now();
        }
        self.execute(operation, request)
    }
}

#[test]
fn malformed_error_payloads_are_discarded_before_bounded_correlated_emission() {
    let (mut server, mut wire) = setup(100, false, vec![]);
    server.limits.output_bytes = 1024;
    let private = "PRIVATE_ERROR_CANARY".repeat(256);
    for (index, data) in [json!(&private), json!([&private])].into_iter().enumerate() {
        let correlation = format!("safe-error-{index}");
        server.emit(
            &mut wire,
            json!({"jsonrpc":"2.0","id":index,"error":{"code":-32602,"message":"Invalid parameters","data":data}}),
            Duration::from_secs(1), &correlation, Instant::now(),
        ).unwrap();
        let response = wire.output.last().unwrap();
        assert_eq!(
            response["error"]["data"],
            json!({"correlation_id":correlation})
        );
        assert!(!response.to_string().contains("PRIVATE_ERROR_CANARY"));
        assert!(response.to_string().len() < server.limits.output_bytes);
    }
    assert_eq!(wire.output.len(), 2);
    assert_eq!(server.metrics.failed, 2);
    assert_eq!(server.metrics.completed, 2);
    assert_eq!(server.metrics.rejected, 0);
}
#[test]
fn lost_completion_cancellation_panic_and_timeout_replies_never_restart_work() {
    for (delay, panic, faults, deadline, expected) in [
        (20, false, vec![], 30_000, None),
        (
            100,
            false,
            vec![Fault::Frame(
                json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":42}}),
            )],
            30_000,
            Some("CG_CANCELLED"),
        ),
        (0, true, vec![], 30_000, Some("CG_INTERNAL_ERROR")),
        (100, false, vec![Fault::Wait], 5, Some("CG_TIMEOUT")),
    ] {
        let (mut server, mut wire) = setup(delay, panic, faults);
        if matches!(expected, Some("CG_CANCELLED" | "CG_TIMEOUT")) {
            let (started, receiver) = mpsc::channel();
            server.application = Box::new(PendingHost { started });
            wire.started = receiver;
        }
        server.limits.request_timeout_ms = deadline;
        wire.write_error = true;
        assert_eq!(serve(&mut server, &mut wire), Err(TransportError::Io));
        assert_eq!(wire.output.len(), 1);
        if let Some(code) = expected {
            assert_eq!(wire.output[0]["error"]["data"]["code"], code);
        } else {
            assert!(wire.output[0]["result"].is_object());
        }
        if let Some(context) = wire.context.take().or_else(|| wire.started.try_recv().ok()) {
            assert_eq!(
                context.check(),
                Err(gateway_application::codex::FacadeError::Cancelled)
            );
        }
        assert!(
            wire.started.try_recv().is_err(),
            "one application dispatch at most"
        );
        assert_eq!(server.metrics.completed, 1);
        assert_eq!(server.metrics.failed, 1);
    }
}
