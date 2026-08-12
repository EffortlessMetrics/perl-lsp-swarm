//! Bounded, priority-aware retention for asynchronous external-peer events.
//!
//! Individual peer frames are bounded by the shared Content-Length framer, but
//! a peer can send an arbitrary number of valid events while the editor is slow
//! or absent. This queue therefore owns the aggregate retention contract:
//!
//! - output is loss-tolerant and may be truncated or evicted with an observable
//!   console notice;
//! - source facts and breakpoint snapshots are replaceable only when their
//!   identity proves that the newer event supersedes the older one;
//! - stopped and terminated events outrank stream/state traffic;
//! - an event that cannot be represented after lower-priority degradation
//!   closes the peer with a typed resource-limit outcome.

use std::collections::VecDeque;
use std::mem::size_of;

use crate::model::{DebugEvent, DebugSource, OutputCategory, StopReason};

const DEFAULT_MAX_EVENTS: usize = 256;
const DEFAULT_MAX_BYTES: usize = 1024 * 1024;
const MAX_SINGLE_OUTPUT_BYTES: usize = 64 * 1024;
const CRITICAL_EVENT_RESERVE: usize = 2;
const CRITICAL_BYTE_RESERVE: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventClass {
    Output,
    Replaceable,
    Lifecycle,
    Stopped,
    Terminated,
}

#[derive(Debug)]
struct BufferedEvent {
    event: DebugEvent,
    bytes: usize,
}

/// Result of attempting to retain one translated peer event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PushOutcome {
    /// Event retained without discarding any user-observable information.
    Buffered,
    /// Event retained or deliberately dropped after observable degradation.
    Degraded,
    /// The queue could not preserve the event under its reviewed envelope.
    ResourceLimit(String),
}

/// Bounded queue used by the external-peer reader thread and DAP frontend.
#[derive(Debug)]
pub(super) struct PeerEventBuffer {
    queue: VecDeque<BufferedEvent>,
    retained_bytes: usize,
    max_events: usize,
    max_bytes: usize,
    dropped_output_events: usize,
    dropped_output_bytes: usize,
    dropped_state_events: usize,
    dropped_lifecycle_events: usize,
    terminated_seen: bool,
}

impl Default for PeerEventBuffer {
    fn default() -> Self {
        Self::with_limits(DEFAULT_MAX_EVENTS, DEFAULT_MAX_BYTES)
    }
}

impl PeerEventBuffer {
    fn with_limits(max_events: usize, max_bytes: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            retained_bytes: 0,
            max_events: max_events.max(1),
            max_bytes: max_bytes.max(1),
            dropped_output_events: 0,
            dropped_output_bytes: 0,
            dropped_state_events: 0,
            dropped_lifecycle_events: 0,
            terminated_seen: false,
        }
    }

    /// Retain one event, applying the documented priority/degradation policy.
    pub(super) fn push(&mut self, mut event: DebugEvent) -> PushOutcome {
        let class = event_class(&event);
        // `Terminated` is the final event in the session stream. Once accepted,
        // discard all later peer traffic rather than exposing output/state after
        // the terminal transition or allowing a second terminal event.
        if self.terminated_seen {
            return PushOutcome::Degraded;
        }

        self.coalesce_replaceable(&event);
        let mut degraded = false;
        if let DebugEvent::Output { output, .. } = &mut event {
            let original_len = output.len();
            truncate_utf8(output, MAX_SINGLE_OUTPUT_BYTES);
            let removed = original_len.saturating_sub(output.len());
            if removed > 0 {
                self.dropped_output_bytes = self.dropped_output_bytes.saturating_add(removed);
                degraded = true;
            }
        }

        let bytes = estimated_event_bytes(&event);
        if bytes > self.max_bytes {
            return self.reject_oversized(event, bytes);
        }

        while !self.fits(class, bytes) {
            let Some(index) = self.eviction_candidate(class) else {
                return if class == EventClass::Output {
                    self.record_dropped_output(&event);
                    PushOutcome::Degraded
                } else {
                    PushOutcome::ResourceLimit(format!(
                        "external peer event buffer exhausted while retaining {} event ({} events, {} bytes retained; limits: {} events, {} bytes)",
                        class_name(class),
                        self.queue.len(),
                        self.retained_bytes,
                        self.max_events,
                        self.max_bytes
                    ))
                };
            };
            self.evict(index);
            degraded = true;
        }

        if class == EventClass::Terminated {
            self.terminated_seen = true;
        }
        self.retained_bytes = self.retained_bytes.saturating_add(bytes);
        self.queue.push_back(BufferedEvent { event, bytes });
        if degraded { PushOutcome::Degraded } else { PushOutcome::Buffered }
    }

    /// Replace queued traffic with an observable terminal resource-limit state.
    pub(super) fn force_resource_limit(&mut self, reason: &str) {
        self.clear_counting_drops();
        let notice = DebugEvent::Output {
            category: OutputCategory::Console,
            output: format!("[perl-dap] external debugger peer disconnected: {reason}\n"),
        };
        let _ = self.push(notice);
        let _ = self.push(DebugEvent::Terminated { exit_code: None });
    }

    /// Drain retained events and prepend a single loss receipt when degradation occurred.
    pub(super) fn drain(&mut self) -> Vec<DebugEvent> {
        let mut events = Vec::with_capacity(self.queue.len().saturating_add(1));
        if self.has_loss_receipt() {
            events.push(DebugEvent::Output {
                category: OutputCategory::Console,
                output: format!(
                    "[perl-dap] external peer event stream degraded: {} output event(s) dropped, {} output byte(s) truncated or dropped, {} replaceable state event(s) superseded, {} lifecycle event(s) superseded\n",
                    self.dropped_output_events,
                    self.dropped_output_bytes,
                    self.dropped_state_events,
                    self.dropped_lifecycle_events
                ),
            });
        }
        events.extend(self.queue.drain(..).map(|entry| entry.event));
        self.retained_bytes = 0;
        self.dropped_output_events = 0;
        self.dropped_output_bytes = 0;
        self.dropped_state_events = 0;
        self.dropped_lifecycle_events = 0;
        events
    }

    fn has_loss_receipt(&self) -> bool {
        self.dropped_output_events > 0
            || self.dropped_output_bytes > 0
            || self.dropped_state_events > 0
            || self.dropped_lifecycle_events > 0
    }

    fn coalesce_replaceable(&mut self, incoming: &DebugEvent) {
        let Some(index) = self.queue.iter().position(|entry| supersedes(incoming, &entry.event))
        else {
            return;
        };
        self.remove_without_loss(index);
    }

    fn reject_oversized(&mut self, event: DebugEvent, bytes: usize) -> PushOutcome {
        if matches!(event, DebugEvent::Output { .. }) {
            self.record_dropped_output(&event);
            PushOutcome::Degraded
        } else {
            PushOutcome::ResourceLimit(format!(
                "external peer {} event requires {bytes} retained bytes, exceeding the {}-byte event-buffer limit",
                class_name(event_class(&event)),
                self.max_bytes
            ))
        }
    }

    fn record_dropped_output(&mut self, event: &DebugEvent) {
        self.dropped_output_events = self.dropped_output_events.saturating_add(1);
        if let DebugEvent::Output { output, .. } = event {
            self.dropped_output_bytes = self.dropped_output_bytes.saturating_add(output.len());
        }
    }

    fn fits(&self, class: EventClass, bytes: usize) -> bool {
        let (max_events, max_bytes) =
            if matches!(class, EventClass::Stopped | EventClass::Terminated) {
                (self.max_events, self.max_bytes)
            } else {
                let event_reserve = CRITICAL_EVENT_RESERVE.min(self.max_events / 4);
                let byte_reserve = CRITICAL_BYTE_RESERVE.min(self.max_bytes / 4);
                (
                    self.max_events.saturating_sub(event_reserve).max(1),
                    self.max_bytes.saturating_sub(byte_reserve).max(1),
                )
            };
        self.queue.len() < max_events && self.retained_bytes.saturating_add(bytes) <= max_bytes
    }

    fn eviction_candidate(&self, incoming: EventClass) -> Option<usize> {
        let priorities: &[EventClass] = match incoming {
            EventClass::Output => &[EventClass::Output],
            EventClass::Replaceable => &[EventClass::Output],
            EventClass::Lifecycle => &[EventClass::Output, EventClass::Replaceable],
            EventClass::Stopped => {
                &[EventClass::Output, EventClass::Replaceable, EventClass::Lifecycle]
            }
            EventClass::Terminated => &[
                EventClass::Output,
                EventClass::Replaceable,
                EventClass::Lifecycle,
                EventClass::Stopped,
            ],
        };

        priorities.iter().find_map(|class| {
            self.queue.iter().position(|entry| event_class(&entry.event) == *class)
        })
    }

    fn remove_without_loss(&mut self, index: usize) {
        let Some(entry) = self.queue.remove(index) else {
            return;
        };
        self.retained_bytes = self.retained_bytes.saturating_sub(entry.bytes);
    }

    fn evict(&mut self, index: usize) {
        let Some(entry) = self.queue.remove(index) else {
            return;
        };
        self.retained_bytes = self.retained_bytes.saturating_sub(entry.bytes);
        match entry.event {
            DebugEvent::Output { output, .. } => {
                self.dropped_output_events = self.dropped_output_events.saturating_add(1);
                self.dropped_output_bytes = self.dropped_output_bytes.saturating_add(output.len());
            }
            DebugEvent::SourceFacts { .. } | DebugEvent::BreakpointsChanged { .. } => {
                self.dropped_state_events = self.dropped_state_events.saturating_add(1);
            }
            DebugEvent::Initialized | DebugEvent::Continued { .. } | DebugEvent::Stopped { .. } => {
                self.dropped_lifecycle_events = self.dropped_lifecycle_events.saturating_add(1);
            }
            DebugEvent::Terminated { .. } => {}
        }
    }

    fn clear_counting_drops(&mut self) {
        while !self.queue.is_empty() {
            self.evict(0);
        }
        self.retained_bytes = 0;
    }

    #[cfg(test)]
    fn retained_events(&self) -> usize {
        self.queue.len()
    }

    #[cfg(test)]
    fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

fn event_class(event: &DebugEvent) -> EventClass {
    match event {
        DebugEvent::Output { .. } => EventClass::Output,
        DebugEvent::SourceFacts { .. } | DebugEvent::BreakpointsChanged { .. } => {
            EventClass::Replaceable
        }
        DebugEvent::Initialized | DebugEvent::Continued { .. } => EventClass::Lifecycle,
        DebugEvent::Stopped { .. } => EventClass::Stopped,
        DebugEvent::Terminated { .. } => EventClass::Terminated,
    }
}

fn class_name(class: EventClass) -> &'static str {
    match class {
        EventClass::Output => "output",
        EventClass::Replaceable => "replaceable-state",
        EventClass::Lifecycle => "lifecycle",
        EventClass::Stopped => "stopped",
        EventClass::Terminated => "terminated",
    }
}

fn supersedes(incoming: &DebugEvent, existing: &DebugEvent) -> bool {
    match (incoming, existing) {
        (
            DebugEvent::SourceFacts { source: incoming_source, .. },
            DebugEvent::SourceFacts { source: existing_source, .. },
        ) => incoming_source == existing_source,
        (DebugEvent::BreakpointsChanged { .. }, DebugEvent::BreakpointsChanged { .. }) => true,
        _ => false,
    }
}

fn truncate_utf8(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }
    let mut boundary = max_bytes.min(text.len());
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text.truncate(boundary);
}

fn estimated_event_bytes(event: &DebugEvent) -> usize {
    let base = size_of::<DebugEvent>();
    match event {
        DebugEvent::Initialized | DebugEvent::Terminated { .. } => base,
        DebugEvent::Continued { .. } => base,
        DebugEvent::Stopped { reason, position, .. } => {
            base.saturating_add(stop_reason_bytes(reason)).saturating_add(
                position.as_ref().map_or(0, |position| debug_source_bytes(&position.source)),
            )
        }
        DebugEvent::Output { output, .. } => base.saturating_add(output.capacity()),
        DebugEvent::BreakpointsChanged { breakpoints } => {
            let mut bytes = base.saturating_add(
                breakpoints
                    .capacity()
                    .saturating_mul(size_of::<crate::model::ResolvedBreakpoint>()),
            );
            for breakpoint in breakpoints {
                bytes = bytes
                    .saturating_add(debug_source_bytes(&breakpoint.actual_position.source))
                    .saturating_add(breakpoint.message.as_ref().map_or(0, String::capacity));
            }
            bytes
        }
        DebugEvent::SourceFacts { source, facts } => {
            let mut bytes = base
                .saturating_add(debug_source_bytes(source))
                .saturating_add(
                    facts.breakable_line_candidates.capacity().saturating_mul(size_of::<u32>()),
                )
                .saturating_add(
                    facts
                        .subroutines
                        .capacity()
                        .saturating_mul(size_of::<crate::model::DebugFunctionSymbol>()),
                );
            for subroutine in &facts.subroutines {
                bytes = bytes
                    .saturating_add(subroutine.name.capacity())
                    .saturating_add(debug_source_bytes(&subroutine.source));
            }
            bytes
        }
    }
}

fn stop_reason_bytes(reason: &StopReason) -> usize {
    match reason {
        StopReason::Unknown(raw) => raw.capacity(),
        _ => 0,
    }
}

fn debug_source_bytes(source: &DebugSource) -> usize {
    let path_bytes = source.path.as_os_str().to_string_lossy().len().saturating_mul(2);
    size_of::<DebugSource>()
        .saturating_add(path_bytes)
        .saturating_add(source.name.as_ref().map_or(0, String::capacity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DebugPosition, ResolvedBreakpoint, SourceDebugFacts, ThreadId};
    use std::path::PathBuf;

    fn output(size: usize) -> DebugEvent {
        DebugEvent::Output { category: OutputCategory::Stdout, output: "x".repeat(size) }
    }

    fn stopped() -> DebugEvent {
        DebugEvent::Stopped {
            reason: StopReason::Breakpoint,
            thread_id: ThreadId(1),
            position: None,
        }
    }

    fn source_facts(path: &str, line: u32) -> DebugEvent {
        let source = DebugSource::from_path(path);
        DebugEvent::SourceFacts {
            source,
            facts: SourceDebugFacts {
                breakable_line_candidates: vec![line],
                subroutines: Vec::new(),
            },
        }
    }

    fn breakpoint_snapshot(line: u32) -> DebugEvent {
        let source = DebugSource::from_path("/work/app.pl");
        DebugEvent::BreakpointsChanged {
            breakpoints: vec![ResolvedBreakpoint {
                id: i64::from(line),
                verified: true,
                actual_position: DebugPosition { source, line, column: None },
                message: None,
            }],
        }
    }

    #[test]
    fn output_flood_stays_within_count_and_byte_limits() {
        let mut buffer = PeerEventBuffer::with_limits(12, 1024);
        for _ in 0..500 {
            assert!(!matches!(buffer.push(output(80)), PushOutcome::ResourceLimit(_)));
        }
        assert!(buffer.retained_events() <= 12);
        assert!(buffer.retained_bytes() <= 1024);
        let drained = buffer.drain();
        assert!(
            matches!(drained.first(), Some(DebugEvent::Output { output, .. }) if output.contains("degraded"))
        );
    }

    #[test]
    fn output_flood_cannot_evict_stopped_or_terminated() {
        let mut buffer = PeerEventBuffer::with_limits(10, 2048);
        for _ in 0..100 {
            let _ = buffer.push(output(128));
        }
        assert!(!matches!(buffer.push(stopped()), PushOutcome::ResourceLimit(_)));
        for _ in 0..100 {
            let _ = buffer.push(output(128));
        }
        assert!(!matches!(
            buffer.push(DebugEvent::Terminated { exit_code: Some(7) }),
            PushOutcome::ResourceLimit(_)
        ));
        let drained = buffer.drain();
        assert!(drained.iter().any(|event| matches!(event, DebugEvent::Stopped { .. })));
        assert!(
            drained
                .iter()
                .any(|event| matches!(event, DebugEvent::Terminated { exit_code: Some(7) }))
        );
    }

    #[test]
    fn source_facts_only_coalesce_for_the_same_source_identity() {
        let mut buffer = PeerEventBuffer::with_limits(16, 4096);
        assert_eq!(buffer.push(source_facts("/work/a.pl", 1)), PushOutcome::Buffered);
        assert_eq!(buffer.push(source_facts("/work/a.pl", 2)), PushOutcome::Buffered);
        assert_eq!(buffer.push(source_facts("/work/b.pl", 3)), PushOutcome::Buffered);
        let drained = buffer.drain();
        let facts = drained
            .iter()
            .filter_map(|event| match event {
                DebugEvent::SourceFacts { source, facts } => {
                    Some((source.path.clone(), facts.breakable_line_candidates.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(facts.len(), 2);
        assert!(facts.contains(&(PathBuf::from("/work/a.pl"), vec![2])));
        assert!(facts.contains(&(PathBuf::from("/work/b.pl"), vec![3])));
    }

    #[test]
    fn breakpoint_snapshot_replaces_its_session_predecessor() {
        let mut buffer = PeerEventBuffer::with_limits(16, 4096);
        assert_eq!(buffer.push(breakpoint_snapshot(1)), PushOutcome::Buffered);
        assert_eq!(buffer.push(breakpoint_snapshot(2)), PushOutcome::Buffered);
        let drained = buffer.drain();
        let snapshots = drained
            .iter()
            .filter_map(|event| match event {
                DebugEvent::BreakpointsChanged { breakpoints } => Some(breakpoints),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0][0].actual_position.line, 2);
    }

    #[test]
    fn oversized_non_stream_event_is_a_resource_limit() {
        let mut buffer = PeerEventBuffer::with_limits(8, 128);
        let event = source_facts(&format!("/work/{}.pl", "x".repeat(256)), 1);
        assert!(matches!(buffer.push(event), PushOutcome::ResourceLimit(_)));
    }

    #[test]
    fn terminated_is_unique_across_drains() {
        let mut buffer = PeerEventBuffer::with_limits(8, 2048);
        assert_eq!(buffer.push(DebugEvent::Terminated { exit_code: None }), PushOutcome::Buffered);
        let first = buffer.drain();
        assert_eq!(
            first.iter().filter(|event| matches!(event, DebugEvent::Terminated { .. })).count(),
            1
        );
        assert_eq!(
            buffer.push(DebugEvent::Terminated { exit_code: Some(1) }),
            PushOutcome::Degraded
        );
        assert!(buffer.drain().is_empty());
    }

    #[test]
    fn no_event_is_exposed_after_termination() {
        let mut buffer = PeerEventBuffer::with_limits(8, 2048);
        assert_eq!(buffer.push(DebugEvent::Terminated { exit_code: None }), PushOutcome::Buffered);
        assert_eq!(buffer.push(output(64)), PushOutcome::Degraded);
        assert_eq!(buffer.push(stopped()), PushOutcome::Degraded);

        let drained = buffer.drain();
        assert_eq!(drained.len(), 1);
        assert!(matches!(drained[0], DebugEvent::Terminated { .. }));
    }
}
