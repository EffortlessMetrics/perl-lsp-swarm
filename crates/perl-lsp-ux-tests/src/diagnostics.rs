//! Diagnostics-focused helpers for UX harness orchestration.

use crate::LspEvent;
use crate::client::EventSource;
use serde_json::Value;
use std::time::Duration;

/// Event-driven helper for diagnostics events in the UX harness queue.
pub struct DiagnosticsTracker;

impl DiagnosticsTracker {
    /// Return the most recent diagnostics payload seen for `uri`.
    pub fn latest_for_uri(events: &[LspEvent], uri: &str) -> Option<Vec<Value>> {
        events.iter().rev().find_map(|event| match event {
            LspEvent::Diagnostics { uri: event_uri, diagnostics, .. } if event_uri == uri => {
                Some(diagnostics.clone())
            }
            _ => None,
        })
    }

    /// Count diagnostics payloads seen for `uri`.
    pub fn count_for_uri(events: &[LspEvent], uri: &str) -> usize {
        events
            .iter()
            .filter(|event| {
                matches!(event, LspEvent::Diagnostics { uri: event_uri, .. } if event_uri == uri)
            })
            .count()
    }

    /// Return the newest diagnostics payload after `already_seen` matching
    /// events for `uri`.
    pub fn latest_for_uri_after_count(
        events: &[LspEvent],
        uri: &str,
        already_seen: usize,
    ) -> Option<Vec<Value>> {
        events
            .iter()
            .filter_map(|event| match event {
                LspEvent::Diagnostics { uri: event_uri, diagnostics, .. } if event_uri == uri => {
                    Some(diagnostics)
                }
                _ => None,
            })
            .skip(already_seen)
            .last()
            .cloned()
    }

    /// Wait until diagnostics for `uri` satisfy `predicate`, returning the
    /// matching payload. Returns `None` on timeout.
    ///
    /// `events` is the observation source to block on — normally the scenario's
    /// `UxClient`. Events are never consumed by the wait, so a later waiter
    /// still sees everything this one matched on.
    ///
    /// # Timeout behaviour
    ///
    /// The wait is event-driven: `predicate` is re-evaluated when the server
    /// publishes something, not on a timer, so `timeout` is a pure outer bound
    /// rather than a rounding term. A matching payload already buffered returns
    /// without ever consulting the deadline.
    pub fn wait_for_uri_matching<F>(
        events: &impl EventSource,
        uri: &str,
        timeout: Duration,
        mut predicate: F,
    ) -> Option<Vec<Value>>
    where
        F: FnMut(&[Value]) -> bool,
    {
        events
            .wait_for_events(timeout, |observed| {
                Self::latest_for_uri(observed, uri).filter(|diagnostics| predicate(diagnostics))
            })
            .ok()
    }

    /// Wait until at least one diagnostics payload newer than `already_seen`
    /// matching events is available for `uri`.
    pub fn wait_for_uri_after_count(
        events: &impl EventSource,
        uri: &str,
        already_seen: usize,
        timeout: Duration,
    ) -> Option<Vec<Value>> {
        events
            .wait_for_events(timeout, |observed| {
                Self::latest_for_uri_after_count(observed, uri, already_seen)
            })
            .ok()
    }
}

#[cfg(test)]
mod tests {
    // The #3021 `unwrap_used` expectation is gone: these waits no longer poll a
    // hand-rolled provider, so they no longer need to unwrap shared test state.
    use super::DiagnosticsTracker;
    use crate::LspEvent;
    use crate::observation::Inbox;
    use serde_json::{Value, json};
    use std::thread;
    use std::time::{Duration, Instant};

    /// A generous outer bound. Every passing wait below must finish far inside
    /// it — a test that only passes by consuming it has proved nothing.
    const GENEROUS: Duration = Duration::from_secs(30);

    /// Build the raw notification the server actually sends, so these waits are
    /// driven through the same decode path production uses.
    fn publish(uri: &str, diagnostics: Vec<Value>) -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": { "uri": uri, "version": 1, "diagnostics": diagnostics },
        })
    }

    #[test]
    fn latest_for_uri_prefers_most_recent_payload() {
        let events = vec![
            LspEvent::Diagnostics {
                uri: "file:///a.pl".to_string(),
                version: Some(1),
                diagnostics: vec![json!({"message": "old"})],
            },
            LspEvent::Diagnostics {
                uri: "file:///b.pl".to_string(),
                version: Some(1),
                diagnostics: vec![json!({"message": "other"})],
            },
            LspEvent::Diagnostics {
                uri: "file:///a.pl".to_string(),
                version: Some(2),
                diagnostics: vec![json!({"message": "new"})],
            },
        ];

        let latest = DiagnosticsTracker::latest_for_uri(&events, "file:///a.pl");
        assert_eq!(latest, Some(vec![json!({"message": "new"})]));
    }

    #[test]
    fn latest_for_uri_returns_none_when_no_match() {
        let events = vec![LspEvent::Diagnostics {
            uri: "file:///b.pl".to_string(),
            version: Some(1),
            diagnostics: vec![json!({"message": "other"})],
        }];
        let latest = DiagnosticsTracker::latest_for_uri(&events, "file:///a.pl");
        assert!(latest.is_none());
    }

    #[test]
    fn count_for_uri_counts_only_matching_diagnostics() {
        let events = vec![
            LspEvent::Diagnostics {
                uri: "file:///a.pl".to_string(),
                version: Some(1),
                diagnostics: vec![json!({"message": "old"})],
            },
            LspEvent::Diagnostics {
                uri: "file:///b.pl".to_string(),
                version: Some(1),
                diagnostics: vec![json!({"message": "other"})],
            },
            LspEvent::Diagnostics {
                uri: "file:///a.pl".to_string(),
                version: Some(2),
                diagnostics: vec![json!({"message": "new"})],
            },
        ];

        assert_eq!(DiagnosticsTracker::count_for_uri(&events, "file:///a.pl"), 2);
    }

    #[test]
    fn latest_for_uri_after_count_returns_newer_payload() {
        let events = vec![
            LspEvent::Diagnostics {
                uri: "file:///a.pl".to_string(),
                version: Some(1),
                diagnostics: vec![json!({"message": "old"})],
            },
            LspEvent::Diagnostics {
                uri: "file:///a.pl".to_string(),
                version: Some(2),
                diagnostics: vec![json!({"message": "new"})],
            },
        ];

        let latest = DiagnosticsTracker::latest_for_uri_after_count(&events, "file:///a.pl", 1);

        assert_eq!(latest, Some(vec![json!({"message": "new"})]));
    }

    #[test]
    fn latest_for_uri_after_count_returns_none_without_newer_payload() {
        let events = vec![LspEvent::Diagnostics {
            uri: "file:///a.pl".to_string(),
            version: Some(1),
            diagnostics: vec![json!({"message": "old"})],
        }];

        let latest = DiagnosticsTracker::latest_for_uri_after_count(&events, "file:///a.pl", 1);

        assert!(latest.is_none());
    }

    /// An already-buffered matching payload returns without ever consulting
    /// the deadline — there is no poll interval to round up to.
    #[test]
    fn wait_for_uri_matching_returns_on_immediate_match() {
        let inbox = Inbox::new();
        inbox.push_event(publish("file:///a.pl", vec![]));

        let started = Instant::now();
        let result =
            DiagnosticsTracker::wait_for_uri_matching(&inbox, "file:///a.pl", GENEROUS, |diags| {
                diags.is_empty()
            });

        assert_eq!(result, Some(vec![]), "expected immediate match on empty diagnostics");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a buffered match must not depend on the deadline, took {:?}",
            started.elapsed()
        );
    }

    /// A live stream that never satisfies the predicate reports the bound.
    #[test]
    fn wait_for_uri_matching_returns_none_on_timeout() {
        let inbox = Inbox::new();
        inbox.push_event(publish("file:///a.pl", vec![json!({"message": "err"})]));

        let result = DiagnosticsTracker::wait_for_uri_matching(
            &inbox,
            "file:///a.pl",
            Duration::from_millis(120),
            |diags| diags.is_empty(),
        );

        assert!(result.is_none(), "expected None when predicate never matches within timeout");
    }

    /// The wait wakes on the *publication that clears the file*, not on a timer.
    ///
    /// The clearing notification is published from another thread after the
    /// waiter is already blocked, so a wait that depended on a poll interval
    /// would be measurably slower than one driven by the event itself.
    #[test]
    fn wait_for_uri_matching_returns_when_diagnostics_clear_later() {
        let inbox = Inbox::new();
        inbox.push_event(publish("file:///a.pl", vec![json!({"message": "err"})]));

        let publisher = inbox.clone();
        let clearing = thread::spawn(move || {
            publisher.push_event(publish("file:///a.pl", vec![]));
        });

        let result =
            DiagnosticsTracker::wait_for_uri_matching(&inbox, "file:///a.pl", GENEROUS, |diags| {
                diags.is_empty()
            });
        let _ = clearing.join();

        assert_eq!(result, Some(vec![]), "expected empty payload when diagnostics clear");
    }

    /// Traffic for another file wakes the waiter but cannot satisfy it.
    #[test]
    fn wait_for_uri_matching_ignores_other_uris() {
        let inbox = Inbox::new();
        inbox.push_event(publish("file:///b.pl", vec![]));

        let result = DiagnosticsTracker::wait_for_uri_matching(
            &inbox,
            "file:///a.pl",
            Duration::from_millis(120),
            |diags| diags.is_empty(),
        );

        assert!(result.is_none(), "should not match events for a different URI");
    }

    /// A newer publication for the file must supersede the already-seen ones.
    #[test]
    fn wait_for_uri_after_count_wakes_on_the_newer_publication() {
        let inbox = Inbox::new();
        inbox.push_event(publish("file:///a.pl", vec![json!({"message": "old"})]));

        let publisher = inbox.clone();
        let later = thread::spawn(move || {
            publisher.push_event(publish("file:///a.pl", vec![json!({"message": "new"})]));
        });

        let result =
            DiagnosticsTracker::wait_for_uri_after_count(&inbox, "file:///a.pl", 1, GENEROUS);
        let _ = later.join();

        assert_eq!(result, Some(vec![json!({"message": "new"})]));
    }
}
