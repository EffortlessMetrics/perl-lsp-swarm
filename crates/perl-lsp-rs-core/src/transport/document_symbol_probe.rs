//! Bounded opt-in stream-range observations, without payload or ID hashing.

use serde_json::{Value, json};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

const RECORD_LIMIT: u64 = 256;
static ENABLED: OnceLock<bool> = OnceLock::new();
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn active() -> bool {
    #[cfg(test)]
    if CAPTURE.with(|capture| capture.borrow().is_some()) {
        return true;
    }
    *ENABLED.get_or_init(|| std::env::var("PERL_LSP_DOCUMENT_SYMBOL_PROBE").as_deref() == Ok("1"))
        && tracing::enabled!(target: "document_symbol_probe", tracing::Level::DEBUG)
}

pub(super) fn emit(stage: &'static str, metadata: impl FnOnce() -> Value) {
    #[cfg(test)]
    if CAPTURE.with(|capture| capture.borrow().is_some()) {
        CAPTURE.with(|capture| {
            let mut capture = capture.borrow_mut();
            if let Some((sequence, records)) = capture.as_mut() {
                if let Some(receipt) = bounded_receipt(*sequence, stage, metadata) {
                    records.push(receipt);
                }
                *sequence += 1;
            }
        });
        return;
    }
    if !active() {
        return;
    }
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    if let Some(receipt) = bounded_receipt(sequence, stage, metadata) {
        tracing::debug!(target: "document_symbol_probe", "{receipt}");
    }
}

fn bounded_receipt(
    sequence: u64,
    stage: &'static str,
    metadata: impl FnOnce() -> Value,
) -> Option<Value> {
    if sequence > RECORD_LIMIT {
        return None;
    }
    let receipt = if sequence == RECORD_LIMIT {
        json!({ "kind": "document_symbol_transport_probe", "stage": "record_limit_exceeded",
            "server_pid": std::process::id(), "event_sequence": sequence + 1,
            "record_limit": RECORD_LIMIT, "complete": false })
    } else {
        json!({ "kind": "document_symbol_transport_probe", "stage": stage,
            "server_pid": std::process::id(), "event_sequence": sequence + 1,
            "record_limit": RECORD_LIMIT, "metadata": metadata() })
    };
    Some(receipt)
}

#[cfg(test)]
thread_local! {
    static CAPTURE: std::cell::RefCell<Option<(u64, Vec<Value>)>> = const { std::cell::RefCell::new(None) };
}

/// Isolated current-thread observer; it never changes the process environment.
#[cfg(test)]
pub(super) fn capture<T>(operation: impl FnOnce() -> T) -> (T, Vec<Value>) {
    struct ClearCapture;
    impl Drop for ClearCapture {
        fn drop(&mut self) {
            CAPTURE.with(|capture| *capture.borrow_mut() = None);
        }
    }
    CAPTURE.with(|capture| *capture.borrow_mut() = Some((0, Vec::new())));
    let _clear = ClearCapture;
    let result = operation();
    let records = CAPTURE.with(|capture| {
        capture.borrow_mut().take().map(|(_, records)| records).unwrap_or_default()
    });
    (result, records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_budget_retains_one_overflow_marker_and_skips_omitted_metadata() {
        let (_, records) = capture(|| {
            for _ in 0..RECORD_LIMIT {
                emit("stream_read", || json!({ "accepted_bytes": 1 }));
            }
            emit("stream_read", || panic!("overflow must not construct metadata"));
            emit("stream_read", || panic!("omitted record must not construct metadata"));
        });
        assert_eq!(records.len(), RECORD_LIMIT as usize + 1);
        for (index, record) in records.iter().enumerate() {
            assert_eq!(record["event_sequence"], index + 1);
        }
        assert_eq!(records[RECORD_LIMIT as usize]["stage"], "record_limit_exceeded");
        assert_eq!(records[RECORD_LIMIT as usize]["complete"], false);
    }
}
