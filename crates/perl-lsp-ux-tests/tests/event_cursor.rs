// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use perl_lsp_ux_tests::{CapturedMessage, CursorOwnerMismatch, Inbox, ServerMessageCursor};
use serde_json::{Value, json};
use std::time::Duration;

fn notification(method: &str) -> Value {
    json!({ "jsonrpc": "2.0", "method": method })
}

#[test]
fn captured_events_since_is_immutable_across_drain() -> anyhow::Result<()> {
    let inbox = Inbox::new();
    inbox.push_event(notification("first"));
    let cursor = inbox.checkpoint();
    inbox.push_event(notification("second"));
    let _ = inbox.drain_events();
    inbox.push_event(notification("third"));
    let after: Vec<CapturedMessage> = inbox.captured_events_since(&cursor)?;
    anyhow::ensure!(after.len() == 1, "expected one post-cursor event, got {after:?}");
    anyhow::ensure!(
        after[0].value["method"] == "third",
        "post-cursor identity must track capture order, not queue slots"
    );
    Ok(())
}

#[test]
fn drain_events_after_respects_cursor() -> anyhow::Result<()> {
    let inbox = Inbox::new();
    inbox.push_event(notification("keep"));
    let cursor = inbox.checkpoint();
    inbox.push_event(notification("drop"));
    let drained = inbox.drain_events_after(&cursor)?;
    anyhow::ensure!(
        drained.len() == 1 && drained[0]["method"] == "drop",
        "drain_after must remove only post-cursor payloads"
    );
    let retained = inbox.captured_events_since(&inbox.origin_cursor())?;
    anyhow::ensure!(
        retained.len() == 1 && retained[0].value["method"] == "keep",
        "pre-cursor payload must remain retained"
    );
    Ok(())
}

#[test]
fn foreign_cursor_rejects_projection() -> anyhow::Result<()> {
    let left = Inbox::new();
    let right = Inbox::new();
    left.push_event(notification("left"));
    let cursor = left.checkpoint();
    anyhow::ensure!(
        matches!(right.captured_events_since(&cursor), Err(CursorOwnerMismatch)),
        "cross-inbox cursor must not project events"
    );
    Ok(())
}

#[test]
fn wait_for_after_observes_new_event_after_checkpoint() -> anyhow::Result<()> {
    let inbox = Inbox::new();
    inbox.push_event(notification("stale"));
    let cursor = inbox.checkpoint();
    let _ = inbox.drain_events();
    inbox.push_event(notification("fresh"));
    let matched = inbox.wait_for_events_after(&cursor, Duration::from_secs(1), |events| {
        events.iter().find(|message| message.value["method"] == "fresh").map(|message| message.id)
    })?;
    anyhow::ensure!(matched.get() >= 1, "fresh event must carry a post-checkpoint id");
    Ok(())
}
