//! Test-only content-free timing. Labels are static source-code literals;
//! values contain only elapsed microseconds and invocation counts.
use std::{cell::RefCell, collections::BTreeMap, time::Instant};

pub(in crate::local_sessions) type Sample = BTreeMap<&'static str, (u64, u64)>;
thread_local! {
    static SAMPLES: RefCell<Sample> = const { RefCell::new(BTreeMap::new()) };
}
pub(in crate::local_sessions) struct Span {
    label: &'static str,
    started: Instant,
}
pub(in crate::local_sessions) fn span(label: &'static str) -> Span {
    Span {
        label,
        started: Instant::now(),
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        SAMPLES.with(|samples| {
            let mut samples = samples.borrow_mut();
            let sample = samples.entry(self.label).or_default();
            sample.0 = sample.0.saturating_add(1);
            sample.1 = sample.1.saturating_add(
                self.started
                    .elapsed()
                    .as_micros()
                    .try_into()
                    .unwrap_or(u64::MAX),
            );
        });
    }
}
pub(in crate::local_sessions) fn snapshot() -> Sample {
    SAMPLES.with(|samples| samples.borrow().clone())
}
pub(in crate::local_sessions) fn delta(before: &Sample) -> Sample {
    snapshot()
        .into_iter()
        .filter_map(|(label, (calls, micros))| {
            let previous = before.get(label).copied().unwrap_or_default();
            let calls = calls.saturating_sub(previous.0);
            (calls != 0).then_some((label, (calls, micros.saturating_sub(previous.1))))
        })
        .collect()
}
