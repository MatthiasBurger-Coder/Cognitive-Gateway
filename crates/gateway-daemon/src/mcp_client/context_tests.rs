use super::*;
use std::sync::Mutex;

fn context() -> CallContext {
    CallContext::new(
        NonEmptyText::new("guard-regression").unwrap(),
        Instant::now() + Duration::from_secs(2),
    )
}

fn recording_guard(
    calls: &Arc<Mutex<Vec<usize>>>,
    position: usize,
    denied: Option<usize>,
) -> Arc<dyn CallGuard> {
    let calls = calls.clone();
    Arc::new(move || {
        calls.lock().unwrap().push(position);
        if denied == Some(position) {
            Err(Error::SessionDenied(ConnectorFailureCode::ScopeMismatch))
        } else {
            Ok(())
        }
    })
}

#[test]
fn composed_guards_keep_regular_then_final_order_and_short_circuit_every_denial() {
    for denied in [None, Some(0), Some(1), Some(2), Some(3)] {
        let calls = Arc::new(Mutex::new(Vec::new()));
        // Adding a regular guard after a final guard must not let the final
        // authorization run before repricing/current-policy callbacks.
        let guarded = context()
            .with_final_guard(recording_guard(&calls, 2, denied))
            .with_guard(recording_guard(&calls, 0, denied))
            .with_final_guard(recording_guard(&calls, 3, denied))
            .with_guard(recording_guard(&calls, 1, denied));
        match denied {
            Some(position) => {
                let error = guarded.check().unwrap_err();
                assert_eq!(
                    error,
                    Error::SessionDenied(ConnectorFailureCode::ScopeMismatch)
                );
                let projected = error.failure(&guarded.correlation, ConnectorEffect::NoMutation);
                assert_eq!(projected.code, ConnectorFailureCode::ScopeMismatch);
                assert_eq!(projected.effect, ConnectorEffect::NoMutation);
                assert_eq!(*calls.lock().unwrap(), (0..=position).collect::<Vec<_>>());
            }
            None => {
                guarded.check().unwrap();
                assert_eq!(*calls.lock().unwrap(), vec![0, 1, 2, 3]);
            }
        }
    }
}

#[test]
fn final_guard_observes_revocation_from_regular_callback_on_each_check() {
    let live = Arc::new(AtomicBool::new(true));
    let revoked_by_callback = live.clone();
    let live_at_final_check = live.clone();
    let guarded = context()
        .with_final_guard(Arc::new(move || {
            if live_at_final_check.load(Ordering::Acquire) {
                Ok(())
            } else {
                Err(Error::SessionDenied(ConnectorFailureCode::PermissionDenied))
            }
        }))
        .with_guard(Arc::new(move || {
            revoked_by_callback.store(false, Ordering::Release);
            Ok(())
        }));
    for _ in 0..2 {
        live.store(true, Ordering::Release);
        assert_eq!(
            guarded.check(),
            Err(Error::SessionDenied(ConnectorFailureCode::PermissionDenied))
        );
    }
}

#[test]
fn cancellation_and_deadline_are_checked_again_after_successful_callbacks() {
    for final_guard in [false, true] {
        let original = context();
        let cancel_from_callback = original.clone();
        let called = Arc::new(AtomicBool::new(false));
        let callback_called = called.clone();
        let cancel: Arc<dyn CallGuard> = Arc::new(move || {
            callback_called.store(true, Ordering::Release);
            cancel_from_callback.cancel();
            Ok(())
        });
        let guarded = if final_guard {
            original.with_final_guard(cancel)
        } else {
            original.with_guard(cancel)
        };
        assert_eq!(guarded.check(), Err(Error::Cancelled));
        assert!(called.load(Ordering::Acquire));
        assert_eq!(original.check(), Err(Error::Cancelled));

        let mut original = context();
        original.deadline = Instant::now() + Duration::from_millis(250);
        let deadline = original.deadline;
        let called = Arc::new(AtomicBool::new(false));
        let callback_called = called.clone();
        let expire: Arc<dyn CallGuard> = Arc::new(move || {
            callback_called.store(true, Ordering::Release);
            std::thread::sleep(
                deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
            );
            Ok(())
        });
        let guarded = if final_guard {
            original.with_final_guard(expire)
        } else {
            original.with_guard(expire)
        };
        assert_eq!(guarded.check(), Err(Error::Timeout));
        assert!(called.load(Ordering::Acquire));
    }
}

#[test]
fn already_cancelled_or_expired_context_never_enters_authority_callbacks() {
    let called = Arc::new(AtomicBool::new(false));
    let callback_called = called.clone();
    let guarded = context().with_guard(Arc::new(move || {
        callback_called.store(true, Ordering::Release);
        Ok(())
    }));
    guarded.cancel();
    assert_eq!(guarded.check(), Err(Error::Cancelled));
    assert!(!called.load(Ordering::Acquire));
    let expired_calls = Arc::new(Mutex::new(Vec::new()));
    let mut expired = context().with_final_guard(recording_guard(&expired_calls, 0, None));
    expired.deadline = Instant::now();
    assert_eq!(expired.check(), Err(Error::Timeout));
    assert!(expired_calls.lock().unwrap().is_empty());
}
