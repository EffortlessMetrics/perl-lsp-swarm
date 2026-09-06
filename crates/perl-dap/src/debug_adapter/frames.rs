//! Stack frame management: stack trace parsing, scopes.

use super::{
    DEBUGGER_QUERY_WAIT_MS, DapMessage, DebugAdapter, HashMap, Scope, ScopesArguments,
    ScopesResponseBody, Source, StackFrame, StackTraceArguments, Value, Write, json,
    lock_or_recover,
};
use crate::parse_origin::{DebuggerOutputOrigin, OriginatedParseInput, ParseIdentity};
use std::collections::HashSet;

const FRAME_ID_MODULUS: i32 = 100_000;

#[derive(Clone)]
struct StoppedFrameAuthority {
    adapter_generation: u64,
    stopped_generation: u64,
    top: StackFrame,
    snapshot: Vec<StackFrame>,
}

enum FramedStackQuery {
    Sent { begin: String, end: String },
    Failed,
    Unavailable,
    Rejected,
}

#[cfg(test)]
fn clear_rejected_framed_snapshot(session: &mut Option<super::DebugSession>) {
    if let Some(session) = session {
        session.stack_frames.clear();
        session.stack_frame_arguments.clear();
    }
}

impl DebugAdapter {
    fn stopped_frame_authority(
        session: &Option<super::DebugSession>,
        adapter_generation: u64,
    ) -> Option<StoppedFrameAuthority> {
        let session = session.as_ref()?;
        if session.state != crate::debug_adapter::DebugState::Stopped {
            return None;
        }
        let expected_id = session.stopped_generation.max(1);
        if expected_id >= FRAME_ID_MODULUS as u64 {
            return None;
        }
        let top = session.stack_frames.first()?.clone();
        if top.id != expected_id as i32 {
            return None;
        }
        Some(StoppedFrameAuthority {
            adapter_generation,
            stopped_generation: session.stopped_generation,
            top,
            snapshot: session.stack_frames.clone(),
        })
    }

    fn stopped_frame_authority_is_current(
        session: &Option<super::DebugSession>,
        adapter_generation: u64,
        authority: &StoppedFrameAuthority,
    ) -> bool {
        session.as_ref().is_some_and(|session| {
            adapter_generation == authority.adapter_generation
                && session.state == crate::debug_adapter::DebugState::Stopped
                && session.stopped_generation == authority.stopped_generation
        })
    }

    fn capture_stopped_frame_authority(&self) -> (bool, Option<StoppedFrameAuthority>) {
        // Keep the adapter generation stable while pairing it with the process
        // session. Replacement paths advance this gate before swapping sessions.
        let generation =
            lock_or_recover(&self.termination_state, "debug_adapter.termination_state");
        let session = lock_or_recover(&self.session, "debug_adapter.session");
        (session.is_some(), Self::stopped_frame_authority(&session, generation.generation))
    }

    fn with_current_stopped_session<R>(
        &self,
        authority: &StoppedFrameAuthority,
        apply: impl FnOnce(&mut super::DebugSession) -> R,
    ) -> Option<R> {
        // Lock in the same generation-first order used by replacement paths.
        // Holding both guards makes the final currentness check and mutation one
        // atomic admission with respect to resume/replacement state changes.
        let generation =
            lock_or_recover(&self.termination_state, "debug_adapter.termination_state");
        let mut session = lock_or_recover(&self.session, "debug_adapter.session");
        if !Self::stopped_frame_authority_is_current(&session, generation.generation, authority) {
            return None;
        }
        session.as_mut().map(apply)
    }

    fn send_current_framed_stack_query(
        &self,
        authority: &StoppedFrameAuthority,
    ) -> FramedStackQuery {
        self.with_current_stopped_session(authority, |session| {
            let Some(stdin) = session.process.stdin.as_mut() else {
                return FramedStackQuery::Unavailable;
            };
            let commands = vec!["T".to_string()];
            match self.send_framed_debugger_commands(stdin, &commands) {
                Ok((begin, end)) => FramedStackQuery::Sent { begin, end },
                Err(error) => {
                    tracing::warn!(%error, "Failed to send framed stackTrace command, falling back");
                    let _ = stdin.write_all(b"T\n");
                    let _ = stdin.flush();
                    FramedStackQuery::Failed
                }
            }
        })
        .unwrap_or(FramedStackQuery::Rejected)
    }

    fn promote_framed_stack_if_current(
        &self,
        authority: &StoppedFrameAuthority,
        frames: &[StackFrame],
        arguments: &HashMap<i32, Vec<String>>,
    ) -> bool {
        self.with_current_stopped_session(authority, |session| {
            session.stack_frame_arguments = arguments.clone();
            session.stack_frames = frames.to_vec();
        })
        .is_some()
    }

    fn clear_rejected_framed_snapshot_if_current(&self, authority: &StoppedFrameAuthority) {
        let _ = self.with_current_stopped_session(authority, |session| {
            session.stack_frames.clear();
            session.stack_frame_arguments.clear();
        });
    }

    fn reconcile_framed_stack(
        authoritative_top: &StackFrame,
        framed_frames: Vec<StackFrame>,
        arguments: HashMap<i32, Vec<String>>,
    ) -> Option<(Vec<StackFrame>, HashMap<i32, Vec<String>>)> {
        let (mut rebound_frames, rebound_arguments) = Self::rebind_generation_frame_ids(
            framed_frames,
            arguments,
            Some(authoritative_top.id),
        )?;
        if let Some(top) = rebound_frames.first_mut() {
            top.name = authoritative_top.name.clone();
            top.source = authoritative_top.source.clone();
            top.line = authoritative_top.line;
            top.column = authoritative_top.column;
            top.end_line = authoritative_top.end_line;
            top.end_column = authoritative_top.end_column;
        }
        Some((rebound_frames, rebound_arguments))
    }

    fn rebind_generation_frame_ids(
        frames: Vec<StackFrame>,
        arguments: HashMap<i32, Vec<String>>,
        current_frame_id: Option<i32>,
    ) -> Option<(Vec<StackFrame>, HashMap<i32, Vec<String>>)> {
        let base_id = current_frame_id.or_else(|| frames.first().map(|frame| frame.id));
        if base_id.is_some_and(|id| !(0..FRAME_ID_MODULUS).contains(&id))
            || frames.len() > FRAME_ID_MODULUS as usize
        {
            return None;
        }
        let mut used_ids = HashSet::new();
        let mut rebound_arguments = HashMap::new();
        let mut rebound_frames = Vec::with_capacity(frames.len());

        for (index, mut frame) in frames.into_iter().enumerate() {
            let original_id = frame.id;
            let index = i32::try_from(index).unwrap_or(i32::MAX);
            let mut candidate = if index == 0 {
                base_id
                    .map(|base| base.rem_euclid(FRAME_ID_MODULUS))
                    .unwrap_or_else(|| original_id.rem_euclid(FRAME_ID_MODULUS))
            } else {
                base_id
                    .map(|base| {
                        base.rem_euclid(FRAME_ID_MODULUS)
                            .wrapping_add(index)
                            .rem_euclid(FRAME_ID_MODULUS)
                    })
                    .unwrap_or_else(|| original_id.rem_euclid(FRAME_ID_MODULUS))
            };
            let mut attempts = 0;
            while !used_ids.insert(candidate) {
                candidate = (candidate + 1).rem_euclid(FRAME_ID_MODULUS);
                attempts += 1;
                if attempts >= FRAME_ID_MODULUS {
                    return None;
                }
            }
            frame.id = candidate;
            if let Some(values) = arguments.get(&original_id) {
                rebound_arguments.insert(candidate, values.clone());
            }
            rebound_frames.push(frame);
        }

        Some((rebound_frames, rebound_arguments))
    }

    /// Return the only frame the native scope path may inspect.
    ///
    /// `stack_frames[0]` is the frame captured for the current stopped
    /// suspension.  Do not infer identity from source/line or accept another
    /// frame merely because its id is numerically valid; the typed frame
    /// authority in #9045/#9046 will replace this compatibility floor.
    pub(super) fn exact_current_stopped_frame_id(&self, requested: i64) -> Option<i32> {
        let frame_id = i32::try_from(requested).ok().filter(|id| *id >= 0)?;
        let session = lock_or_recover(&self.session, "debug_adapter.session");
        let session = session.as_ref()?;
        if session.state != crate::debug_adapter::DebugState::Stopped {
            return None;
        }
        session.stack_frames.first().filter(|frame| frame.id == frame_id).map(|frame| frame.id)
    }

    /// Handle stackTrace request
    pub(super) fn handle_stack_trace(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        // Identity before any debugger query (#8294): when an execution
        // context is live, the request must name exactly that context. With no
        // live context, the pre-existing honest empty-list response is kept.
        if let Err(rejection) = self.validated_live_thread_id(
            "stackTrace",
            seq,
            request_seq,
            arguments.as_ref().and_then(|v| v.get("threadId")).and_then(Value::as_i64),
        ) {
            return rejection;
        }
        let args: Option<StackTraceArguments> =
            arguments.and_then(|v| serde_json::from_value(v).ok());
        let start_frame =
            args.as_ref().and_then(|value| value.start_frame).unwrap_or(0).max(0) as usize;
        let levels = args.as_ref().and_then(|value| value.levels).unwrap_or(0);
        let requested_count = if levels <= 0 { None } else { Some(levels as usize) };
        let mut framed_output_lines = None;
        let (session_was_present, authority) = self.capture_stopped_frame_authority();
        let mut promotion_rejected = session_was_present && authority.is_none();

        // Ask the debugger for an explicit stack snapshot only when a current
        // stopped-frame authority was captured for this request.
        if let Some(authority) = authority.as_ref() {
            match self.send_current_framed_stack_query(authority) {
                FramedStackQuery::Sent { begin, end } => {
                    framed_output_lines = self.capture_framed_debugger_output(
                        &begin,
                        &end,
                        DEBUGGER_QUERY_WAIT_MS * 8,
                    );
                }
                FramedStackQuery::Failed => {
                    Self::wait_for_debugger_output_window(DEBUGGER_QUERY_WAIT_MS as u32);
                }
                FramedStackQuery::Unavailable => {}
                FramedStackQuery::Rejected => promotion_rejected = true,
            }
        }

        let parsed_frames = if let (Some(lines), Some(authority)) =
            (framed_output_lines.as_ref(), authority.as_ref())
        {
            let output = lines.join("\n");
            let identity = ParseIdentity::new()
                .with_operation_id_from_i64(request_seq)
                .with_suspension_generation(authority.stopped_generation);
            let input = OriginatedParseInput::new(
                DebuggerOutputOrigin::DebuggerControlPayload,
                identity,
                &output,
            );
            let (parsed_frames, frame_arguments) = Self::parse_stack_frames_from_text(input);
            let visible_frames = Self::filter_user_visible_frames(parsed_frames);
            let rebound =
                Self::reconcile_framed_stack(&authority.top, visible_frames, frame_arguments);
            let Some((framed_frames, frame_arguments)) = rebound else {
                // An unencodable generation or exhausted frame namespace is a
                // hard rejection, not an empty debugger snapshot. Clear both
                // authorities only when this request still owns the current
                // suspension. A later suspension must not be cleared by an
                // older framed query that completed after it.
                self.clear_rejected_framed_snapshot_if_current(authority);
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: true,
                    command: "stackTrace".to_string(),
                    body: Some(json!({ "stackFrames": [], "totalFrames": 0 })),
                    message: None,
                };
            };
            if framed_frames.is_empty() {
                // The framed T output contained only internal debugger frames (e.g.
                // `@ = DB::DB called from file '...' line N` at top-level stops) or
                // none at all.  These are filtered out by filter_user_visible_frames.
                //
                // Do NOT fall back to snapshot parsing here: the snapshot buffer
                // contains the entire session history, including the initial implicit
                // stop context line (e.g. line 4 in a 7-line fixture), which appears
                // BEFORE the current breakpoint context line (e.g. line 5).
                // Snapshot-based parsing returns frames in output order, so the FIRST
                // frame would be the stale line-4 context, not the current line-5 stop.
                //
                // The output reader already parsed the most recent context line and
                // stored it in session.stack_frames.  Returning an empty vec here
                // causes the caller to fall through to that authoritative source.
                Vec::new()
            } else {
                if !self.promote_framed_stack_if_current(
                    authority,
                    &framed_frames,
                    &frame_arguments,
                ) {
                    promotion_rejected = true;
                    Vec::new()
                } else {
                    framed_frames
                }
            }
        } else {
            // Snapshot buffer is unreliable when framed transport fails: it holds
            // the full session history so snapshot-based parsing returns frames in
            // buffer order — the stale pre-stop context line appears before the
            // current stop line, producing a wrong first frame.  Return empty so
            // the caller falls through to session.stack_frames, which the output
            // reader populates with the authoritative current-stop frame.
            Vec::new()
        };

        let stack_frames = if promotion_rejected {
            Vec::new()
        } else if !parsed_frames.is_empty() {
            parsed_frames
        } else if let Some(authority) = authority.as_ref() {
            self.with_current_stopped_session(authority, |_| {
                Self::filter_user_visible_frames(authority.snapshot.clone())
            })
            .unwrap_or_default()
        } else if !session_was_present
            && let Some(pid) = *lock_or_recover(&self.attached_pid, "debug_adapter.attached_pid")
        {
            vec![StackFrame {
                id: Self::i64_to_i32_saturating(i64::from(pid)),
                name: format!("attached::process::{pid}"),
                source: Source {
                    name: Some(format!("pid:{pid}")),
                    path: format!("pid://{pid}"),
                    source_reference: None,
                },
                line: 1,
                column: 1,
                end_line: None,
                end_column: None,
            }]
        } else {
            // No active session — return honest empty list per DAP spec
            Vec::new()
        };
        // Capture full depth before pagination so totalFrames reports the real
        // stack depth, not the size of the paginated window (DAP spec §StackTraceResponse:
        // "totalFrames: The total number of frames available in the stack").
        let total_frames = stack_frames.len();
        let stack_frames = Self::paginate_stack_frames(stack_frames, start_frame, requested_count);

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "stackTrace".to_string(),
            body: Some(json!({
                "stackFrames": stack_frames,
                "totalFrames": total_frames
            })),
            message: None,
        }
    }

    /// Handle scopes request
    pub fn handle_scopes(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let args: ScopesArguments = match arguments.and_then(|v| serde_json::from_value(v).ok()) {
            Some(a) => a,
            None => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "scopes".to_string(),
                    body: None,
                    message: Some("Missing frameId".to_string()),
                };
            }
        };

        let Some(frame_id) = self.exact_current_stopped_frame_id(args.frame_id) else {
            return DapMessage::Response {
                seq,
                request_seq,
                success: true,
                command: "scopes".to_string(),
                body: Some(json!({ "scopes": [] })),
                message: None,
            };
        };

        // AC8.3: Hierarchical scope inspection
        // Use VariableReference codec to encode scope refs into disjoint wire bands.
        use crate::debug_adapter::var_ref::{ScopeKind, VariableReference};
        let Some(locals_ref) =
            VariableReference::Scope { frame_id, kind: ScopeKind::Locals }.encode()
        else {
            return DapMessage::Response {
                seq,
                request_seq,
                success: true,
                command: "scopes".to_string(),
                body: Some(json!({ "scopes": [] })),
                message: None,
            };
        };
        let arguments = lock_or_recover(&self.session, "debug_adapter.session")
            .as_ref()
            .and_then(|session| session.stack_frame_arguments.get(&frame_id))
            .cloned()
            .unwrap_or_default();

        let mut scopes = vec![Scope {
            name: "Locals".to_string(),
            presentation_hint: Some("locals".to_string()),
            variables_reference: i64::from(locals_ref),
            expensive: false,
            named_variables: None,
            indexed_variables: None,
        }];
        if !arguments.is_empty()
            && let Some(arguments_ref) =
                (VariableReference::Scope { frame_id, kind: ScopeKind::Arguments }).encode()
        {
            scopes.push(Scope {
                name: "Arguments".to_string(),
                presentation_hint: Some("arguments".to_string()),
                variables_reference: i64::from(arguments_ref),
                expensive: false,
                named_variables: None,
                indexed_variables: Some(arguments.len() as i64),
            });
        }

        let scopes_body = ScopesResponseBody { scopes };

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "scopes".to_string(),
            body: serde_json::to_value(&scopes_body).ok(),
            message: None,
        }
    }
}

impl DebugAdapter {
    fn paginate_stack_frames(
        stack_frames: Vec<StackFrame>,
        start_frame: usize,
        levels: Option<usize>,
    ) -> Vec<StackFrame> {
        let iter = stack_frames.into_iter().skip(start_frame);
        match levels {
            Some(limit) => iter.take(limit).collect(),
            None => iter.collect(),
        }
    }
}

#[cfg(test)]
mod pagination_tests {
    use super::*;
    use crate::debug_adapter::DebugState;
    use std::fmt::Debug;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    fn require_eq<T: Debug + PartialEq>(
        actual: &T,
        expected: &T,
        context: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if actual != expected {
            return Err(format!("{context}: actual={actual:?}, expected={expected:?}").into());
        }
        Ok(())
    }

    fn make_frame(id: i32, name: &str) -> StackFrame {
        StackFrame {
            id,
            name: name.to_string(),
            source: Source {
                name: Some("test.pl".to_string()),
                path: "/tmp/test.pl".to_string(),
                source_reference: None,
            },
            line: id,
            column: 1,
            end_line: None,
            end_column: None,
        }
    }

    /// Regression: paginate_stack_frames used to be called BEFORE capturing the
    /// full depth, so totalFrames reported the slice length instead of the full
    /// stack depth.  This unit test locks the correct invariant:
    ///   totalFrames == pre-pagination length >= paginated-window length
    #[test]
    fn total_frames_is_pre_pagination_length() -> Result<(), Box<dyn std::error::Error>> {
        let all_frames: Vec<StackFrame> = (1..=5).map(|i| make_frame(i, "main::step")).collect();
        let total_before = all_frames.len();

        // Paginate to window of 2, starting at offset 0.
        let paginated = DebugAdapter::paginate_stack_frames(all_frames, 0, Some(2));

        assert_eq!(paginated.len(), 2, "paginated window should be 2");
        assert_eq!(total_before, 5, "total_frames must be full depth (5)");
        assert!(
            total_before >= paginated.len(),
            "total_frames ({total_before}) must be >= paginated len ({})",
            paginated.len()
        );
        Ok(())
    }

    #[test]
    fn generation_rebinding_keeps_multi_frame_ids_and_arguments_aligned()
    -> Result<(), Box<dyn std::error::Error>> {
        let frames = vec![make_frame(0, "inner"), make_frame(1, "outer")];
        let arguments =
            HashMap::from([(0, vec!["inner_arg".to_string()]), (1, vec!["outer_arg".to_string()])]);

        // The generation id (2) intentionally collides with the parser's
        // second frame id; rebinding must repair that collision as well as
        // preserve each frame's captured arguments.
        let (rebound, rebound_arguments) =
            DebugAdapter::rebind_generation_frame_ids(frames, arguments, Some(2))
                .ok_or("valid generation rebinding unexpectedly rejected")?;

        let ids = rebound.iter().map(|frame| frame.id).collect::<Vec<_>>();
        assert_eq!(ids, vec![2, 3], "every visible frame needs a unique stop-bound id");
        assert_eq!(rebound_arguments.get(&2), Some(&vec!["inner_arg".to_string()]));
        assert_eq!(rebound_arguments.get(&3), Some(&vec!["outer_arg".to_string()]));
        Ok(())
    }

    #[test]
    fn framed_stack_trace_preserves_authoritative_current_stop()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut authoritative = make_frame(41, "main::scorecard_frame");
        authoritative.source.path = "/tmp/scorecard.pl".to_string();
        authoritative.line = 7;
        authoritative.column = 5;

        let mut framed_current = make_frame(0, "main::scorecard_frame");
        framed_current.source.path = "/tmp/scorecard.pl".to_string();
        framed_current.line = 11;
        let arguments = HashMap::from([(0, vec!["$marker".to_string()])]);

        let (reconciled, reconciled_arguments) =
            DebugAdapter::reconcile_framed_stack(&authoritative, vec![framed_current], arguments)
                .ok_or("valid framed stack reconciliation unexpectedly rejected")?;
        let top = reconciled.first().ok_or("reconciled stack omitted current frame")?;

        require_eq(&top.id, &41, "top frame id")?;
        require_eq(&top.source.path.as_str(), &"/tmp/scorecard.pl", "top frame source")?;
        require_eq(&top.line, &7, "framed caller line replaced the current stop")?;
        require_eq(&top.column, &5, "top frame column")?;
        require_eq(
            &reconciled_arguments.get(&41),
            &Some(&vec!["$marker".to_string()]),
            "top frame arguments",
        )?;
        Ok(())
    }

    #[test]
    fn framed_stack_trace_retains_callers_with_unique_ids_and_arguments()
    -> Result<(), Box<dyn std::error::Error>> {
        let authoritative = make_frame(99_999, "main::inner");
        let frames = vec![make_frame(0, "main::inner"), make_frame(1, "main::outer")];
        let arguments =
            HashMap::from([(0, vec!["inner_arg".to_string()]), (1, vec!["outer_arg".to_string()])]);

        let (reconciled, reconciled_arguments) =
            DebugAdapter::reconcile_framed_stack(&authoritative, frames, arguments)
                .ok_or("valid framed stack reconciliation unexpectedly rejected")?;

        require_eq(&reconciled.len(), &2, "visible stack depth")?;
        let top = reconciled.first().ok_or("reconciled stack omitted current frame")?;
        let caller = reconciled.get(1).ok_or("reconciled stack omitted caller frame")?;
        require_eq(&top.id, &99_999, "current frame id")?;
        require_eq(&top.name, &"main::inner".to_string(), "current frame name")?;
        require_eq(&caller.id, &0, "caller frame id")?;
        require_eq(
            &reconciled_arguments.get(&99_999),
            &Some(&vec!["inner_arg".to_string()]),
            "current frame arguments",
        )?;
        require_eq(
            &reconciled_arguments.get(&0),
            &Some(&vec!["outer_arg".to_string()]),
            "caller frame arguments",
        )?;
        Ok(())
    }

    #[test]
    fn framed_stack_promotion_requires_same_stopped_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(7, "main::inner")]);
        let authority = {
            let mut guard = lock_or_recover(&adapter.session, "test.seed_generation");
            let session = guard.as_mut().ok_or("test session was not seeded")?;
            session.stopped_generation = 7;
            drop(guard);
            adapter
                .capture_stopped_frame_authority()
                .1
                .ok_or("stopped authority was not captured")?
        };

        {
            let mut session = lock_or_recover(&adapter.session, "test.running_generation");
            let session = session.as_mut().ok_or("test session was not seeded")?;
            session.state = DebugState::Running;
        }
        require_eq(
            &adapter.with_current_stopped_session(&authority, |_| ()).is_some(),
            &false,
            "running session accepted framed promotion",
        )?;

        {
            let mut session = lock_or_recover(&adapter.session, "test.changed_generation");
            let session = session.as_mut().ok_or("test session was not seeded")?;
            session.state = DebugState::Stopped;
            session.stopped_generation = 8;
            session.stack_frames = vec![make_frame(8, "main::inner")];
        }
        require_eq(
            &adapter.with_current_stopped_session(&authority, |_| ()).is_some(),
            &false,
            "later suspension accepted prior framed promotion",
        )?;

        {
            let mut session = lock_or_recover(&adapter.session, "test.terminated_generation");
            let session = session.as_mut().ok_or("test session was not seeded")?;
            session.state = DebugState::Terminated;
        }
        require_eq(
            &adapter.with_current_stopped_session(&authority, |_| ()).is_some(),
            &false,
            "terminated session accepted framed promotion",
        )?;
        Ok(())
    }

    #[test]
    fn running_stack_trace_sends_no_framed_query() -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(1, "main::inner")]);
        {
            let mut guard = lock_or_recover(&adapter.session, "test.running_stack_trace");
            let session = guard.as_mut().ok_or("test session was not seeded")?;
            session.state = DebugState::Running;
        }

        let before_queries = adapter.debugger_query_count_for_test();
        let response = adapter.handle_stack_trace(1, 1, None);
        require_eq(
            &adapter.debugger_query_count_for_test(),
            &before_queries,
            "running stackTrace wrote a debugger query",
        )?;
        let DapMessage::Response { body: Some(body), .. } = response else {
            return Err("running stackTrace response omitted its body".into());
        };
        require_eq(
            &body.get("stackFrames"),
            &Some(&json!([])),
            "running stackTrace exposed stopped frames",
        )?;
        Ok(())
    }

    fn wait_for_framed_query(adapter: &DebugAdapter) -> Result<(), Box<dyn std::error::Error>> {
        for _ in 0..100 {
            if adapter.debugger_query_count_for_test() >= 1 {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(1));
        }
        Err("stackTrace did not issue its framed query".into())
    }

    #[test]
    fn delayed_stack_trace_rejects_session_that_resumes_before_reply()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = Arc::new(DebugAdapter::new());
        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(1, "main::old")]);
        {
            let mut guard = lock_or_recover(&adapter.session, "test.delayed_running_session");
            let session = guard.as_mut().ok_or("test session was not seeded")?;
            session.stopped_generation = 1;
        }

        let worker = Arc::clone(&adapter);
        let request = thread::spawn(move || worker.handle_stack_trace(1, 1, None));
        wait_for_framed_query(&adapter)?;
        {
            let mut guard = lock_or_recover(&adapter.session, "test.resume_during_query");
            let session = guard.as_mut().ok_or("test session disappeared")?;
            session.state = DebugState::Running;
        }
        adapter.push_recent_output_line_for_test("\"DAP_BEGIN_1\"");
        adapter.push_recent_output_line_for_test("# 0 main::stale at /tmp/stale.pl line 9");
        adapter.push_recent_output_line_for_test("\"DAP_END_1\"");

        let response = request.join().map_err(|_| "stackTrace worker panicked")?;
        let DapMessage::Response { body: Some(body), .. } = response else {
            return Err("delayed stackTrace response omitted its body".into());
        };
        require_eq(
            &body.get("stackFrames"),
            &Some(&json!([])),
            "resumed session accepted delayed framed promotion",
        )?;
        Ok(())
    }

    #[test]
    fn delayed_stack_trace_rejects_equal_generation_replacement_session()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = Arc::new(DebugAdapter::new());
        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(1, "main::old")]);
        {
            let mut guard = lock_or_recover(&adapter.session, "test.delayed_prior_session");
            let session = guard.as_mut().ok_or("prior test session was not seeded")?;
            session.stopped_generation = 1;
        }

        let worker = Arc::clone(&adapter);
        let request = thread::spawn(move || worker.handle_stack_trace(1, 1, None));
        wait_for_framed_query(&adapter)?;
        adapter.begin_session_generation();
        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(1, "main::replacement")]);
        {
            let mut guard = lock_or_recover(&adapter.session, "test.delayed_replacement_session");
            let session = guard.as_mut().ok_or("replacement session was not seeded")?;
            session.stopped_generation = 1;
        }
        adapter.push_recent_output_line_for_test("\"DAP_BEGIN_1\"");
        adapter.push_recent_output_line_for_test("# 0 main::stale at /tmp/stale.pl line 9");
        adapter.push_recent_output_line_for_test("\"DAP_END_1\"");

        let response = request.join().map_err(|_| "stackTrace worker panicked")?;
        let DapMessage::Response { body: Some(body), .. } = response else {
            return Err("replacement stackTrace response omitted its body".into());
        };
        require_eq(
            &body.get("stackFrames"),
            &Some(&json!([])),
            "replaced session accepted delayed framed promotion",
        )?;
        let guard = lock_or_recover(&adapter.session, "test.delayed_replacement_check");
        let session = guard.as_ref().ok_or("replacement session disappeared")?;
        require_eq(
            &session.stack_frames.first().map(|frame| frame.name.clone()),
            &Some("main::replacement".to_string()),
            "delayed query changed replacement top frame",
        )?;
        Ok(())
    }

    #[test]
    fn unavailable_framed_refresh_retains_current_generation_callers()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        adapter.seed_stopped_session_with_frames_for_test(vec![
            make_frame(7, "main::inner"),
            make_frame(8, "main::outer"),
        ]);
        {
            let mut guard = lock_or_recover(&adapter.session, "test.unavailable_framed_refresh");
            let session = guard.as_mut().ok_or("test session was not seeded")?;
            session.stopped_generation = 7;
            let _ = session.process.stdin.take();
        }

        let response = adapter.handle_stack_trace(1, 1, None);
        let DapMessage::Response { body: Some(body), .. } = response else {
            return Err("fallback stackTrace response omitted its body".into());
        };
        let frames = body
            .get("stackFrames")
            .and_then(Value::as_array)
            .ok_or("fallback stackTrace body omitted stackFrames")?;
        require_eq(&frames.len(), &2, "fallback stack depth")?;
        require_eq(
            &frames.get(1).and_then(|frame| frame.get("name")),
            &Some(&json!("main::outer")),
            "fallback omitted current-generation caller",
        )?;
        require_eq(&body.get("totalFrames"), &Some(&json!(2)), "fallback totalFrames")?;
        Ok(())
    }

    #[test]
    fn replacement_session_with_equal_stop_generation_rejects_prior_authority()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        let top = make_frame(7, "main::inner");
        adapter.seed_stopped_session_with_frames_for_test(vec![top.clone()]);
        {
            let mut guard = lock_or_recover(&adapter.session, "test.prior_session");
            let session = guard.as_mut().ok_or("prior test session was not seeded")?;
            session.stopped_generation = 7;
        }
        let prior_authority = adapter
            .capture_stopped_frame_authority()
            .1
            .ok_or("prior stopped authority was not captured")?;

        adapter.begin_session_generation();
        adapter.seed_stopped_session_with_frames_for_test(vec![top.clone()]);
        {
            let mut guard = lock_or_recover(&adapter.session, "test.replacement_session");
            let session = guard.as_mut().ok_or("replacement test session was not seeded")?;
            session.stopped_generation = 7;
            session.stack_frame_arguments.insert(7, vec!["replacement".to_string()]);
        }

        let before_queries = adapter.debugger_query_count_for_test();
        require_eq(
            &matches!(
                adapter.send_current_framed_stack_query(&prior_authority),
                FramedStackQuery::Rejected
            ),
            &true,
            "prior authority queried a replacement session",
        )?;
        require_eq(
            &adapter.debugger_query_count_for_test(),
            &before_queries,
            "rejected replacement query wrote to the debugger",
        )?;

        let stale_frames = vec![make_frame(7, "main::stale")];
        let stale_arguments = HashMap::from([(7, vec!["stale".to_string()])]);
        require_eq(
            &adapter.promote_framed_stack_if_current(
                &prior_authority,
                &stale_frames,
                &stale_arguments,
            ),
            &false,
            "prior authority promoted into a replacement session",
        )?;
        adapter.clear_rejected_framed_snapshot_if_current(&prior_authority);

        let guard = lock_or_recover(&adapter.session, "test.replacement_session_check");
        let session = guard.as_ref().ok_or("replacement test session was cleared")?;
        require_eq(
            &session.stack_frames,
            &vec![top],
            "prior authority changed replacement frames",
        )?;
        require_eq(
            &session.stack_frame_arguments.get(&7),
            &Some(&vec!["replacement".to_string()]),
            "prior authority changed replacement arguments",
        )?;
        Ok(())
    }

    #[test]
    fn stopped_frame_authority_rejects_malformed_or_exhausted_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(7, "main::inner")]);
        {
            let mut session = lock_or_recover(&adapter.session, "test.malformed_generation");
            let session = session.as_mut().ok_or("test session was not seeded")?;
            session.stopped_generation = 7;
            session.stack_frames = vec![make_frame(8, "main::inner")];
        }
        let adapter_generation = adapter.current_session_generation();
        let session = lock_or_recover(&adapter.session, "test.malformed_generation_check");
        require_eq(
            &DebugAdapter::stopped_frame_authority(&session, adapter_generation).is_none(),
            &true,
            "mismatched generation-derived frame id was admitted",
        )?;
        drop(session);

        {
            let mut session = lock_or_recover(&adapter.session, "test.exhausted_generation");
            let session = session.as_mut().ok_or("test session was not seeded")?;
            session.stopped_generation = FRAME_ID_MODULUS as u64;
            session.stack_frames = vec![make_frame(i32::MAX, "main::inner")];
        }
        let adapter_generation = adapter.current_session_generation();
        let session = lock_or_recover(&adapter.session, "test.exhausted_generation_check");
        require_eq(
            &DebugAdapter::stopped_frame_authority(&session, adapter_generation).is_none(),
            &true,
            "exhausted generation was admitted",
        )?;
        Ok(())
    }

    #[test]
    fn rejected_prior_query_cannot_clear_a_later_suspension()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(7, "main::inner")]);
        let prior_authority = {
            let mut guard = lock_or_recover(&adapter.session, "test.prior_authority");
            let session = guard.as_mut().ok_or("test session was not seeded")?;
            session.stopped_generation = 7;
            drop(guard);
            adapter
                .capture_stopped_frame_authority()
                .1
                .ok_or("prior stopped authority was not captured")?
        };

        {
            let mut guard = lock_or_recover(&adapter.session, "test.later_suspension");
            let session = guard.as_mut().ok_or("test session was not seeded")?;
            session.stopped_generation = 8;
            session.stack_frames = vec![make_frame(8, "main::inner")];
            session.stack_frame_arguments.insert(8, vec!["later_arg".to_string()]);
        }
        adapter.clear_rejected_framed_snapshot_if_current(&prior_authority);

        let guard = lock_or_recover(&adapter.session, "test.later_suspension_check");
        let session = guard.as_ref().ok_or("test session was cleared unexpectedly")?;
        let top = session.stack_frames.first().ok_or("later suspension frame was cleared")?;
        require_eq(&top.id, &8, "later suspension frame id")?;
        require_eq(
            &session.stack_frame_arguments.get(&8),
            &Some(&vec!["later_arg".to_string()]),
            "later suspension arguments",
        )?;
        Ok(())
    }

    #[test]
    fn generation_rebinding_fails_closed_past_scope_frame_id_ceiling()
    -> Result<(), Box<dyn std::error::Error>> {
        let frames = vec![make_frame(7, "inner"), make_frame(8, "outer")];
        let arguments =
            HashMap::from([(7, vec!["inner_arg".to_string()]), (8, vec!["outer_arg".to_string()])]);
        let (near_ceiling, near_arguments) = DebugAdapter::rebind_generation_frame_ids(
            frames.clone(),
            arguments.clone(),
            Some(99_999),
        )
        .ok_or("near-ceiling generation rebinding unexpectedly rejected")?;
        let near_ids = near_ceiling.iter().map(|frame| frame.id).collect::<Vec<_>>();
        assert_eq!(near_ids, vec![99_999, 0]);
        assert_eq!(near_arguments.get(&0), Some(&vec!["outer_arg".to_string()]));

        let rejected = DebugAdapter::rebind_generation_frame_ids(frames, arguments, Some(100_000));
        assert!(rejected.is_none());
        Ok(())
    }

    #[test]
    fn rejected_framed_snapshot_clears_prior_frames_and_arguments()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(1, "prior")]);
        adapter.seed_stack_frame_arguments_for_test(1, vec!["prior_arg".to_string()]);
        let mut session = lock_or_recover(&adapter.session, "test.rejected_snapshot");

        clear_rejected_framed_snapshot(&mut session);

        let session = session.as_ref().ok_or("test session was cleared unexpectedly")?;
        assert!(session.stack_frames.is_empty());
        assert!(session.stack_frame_arguments.is_empty());
        Ok(())
    }

    /// startFrame beyond the stack depth: paginated slice is empty, but the
    /// pre-pagination total is still the real depth.
    #[test]
    fn total_frames_with_start_frame_beyond_depth() -> Result<(), Box<dyn std::error::Error>> {
        let all_frames: Vec<StackFrame> = (1..=3).map(|i| make_frame(i, "main::step")).collect();
        let total_before = all_frames.len();

        let paginated = DebugAdapter::paginate_stack_frames(all_frames, 10, Some(2));

        assert_eq!(paginated.len(), 0, "paginated slice beyond depth should be empty");
        assert_eq!(total_before, 3, "total_frames must still report full depth when start > depth");
        Ok(())
    }

    /// No pagination (None levels): total_frames == paginated length (no difference).
    #[test]
    fn total_frames_no_pagination_unchanged() -> Result<(), Box<dyn std::error::Error>> {
        let all_frames: Vec<StackFrame> = (1..=4).map(|i| make_frame(i, "main::step")).collect();
        let total_before = all_frames.len();

        let paginated = DebugAdapter::paginate_stack_frames(all_frames, 0, None);

        assert_eq!(paginated.len(), total_before, "no pagination: total == paginated");
        Ok(())
    }

    #[test]
    fn arguments_scope_is_advertised_and_paginates_captured_values()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(7, "main::run")]);
        adapter.seed_stack_frame_arguments_for_test(
            7,
            vec!["$first".to_string(), "[1, 2]".to_string(), "\"a,b\"".to_string()],
        );

        let scopes = adapter.handle_scopes(1, 1, Some(json!({ "frameId": 7 })));
        let DapMessage::Response { body: Some(body), .. } = scopes else {
            return Err("scopes response did not contain a body".into());
        };
        let scope_values =
            body.get("scopes").and_then(Value::as_array).ok_or("scopes body was not an array")?;
        assert_eq!(scope_values.len(), 2);
        assert!(scope_values.iter().all(|scope| {
            scope.get("name") != Some(&json!("Package"))
                && scope.get("name") != Some(&json!("Globals"))
        }));
        let arguments_scope = scope_values
            .iter()
            .find(|scope| scope.get("name") == Some(&json!("Arguments")))
            .ok_or("Arguments scope was not advertised")?;
        assert_eq!(arguments_scope.get("variablesReference"), Some(&json!(74)));
        assert_eq!(arguments_scope.get("indexedVariables"), Some(&json!(3)));
        assert_eq!(arguments_scope.get("namedVariables"), None);

        let variables = adapter.handle_variables(
            2,
            2,
            Some(json!({ "variablesReference": 74, "start": 1, "count": 1 })),
        );
        let DapMessage::Response { body: Some(body), .. } = variables else {
            return Err("variables response did not contain a body".into());
        };
        assert_eq!(body.get("totalVariables"), Some(&json!(3)));
        let values = body
            .get("variables")
            .and_then(Value::as_array)
            .ok_or("variables body was not an array")?;
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].get("name"), Some(&json!("arg1")));
        assert_eq!(values[0].get("value"), Some(&json!("[1, 2]")));
        Ok(())
    }

    #[test]
    fn scopes_without_exact_current_stopped_frame_are_empty()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();

        for frame_id in [-1_i64, 8, i64::from(i32::MAX) + 1] {
            let response = adapter.handle_scopes(1, 1, Some(json!({ "frameId": frame_id })));
            let DapMessage::Response { body: Some(body), .. } = response else {
                return Err("invalid frame response did not contain a body".into());
            };
            assert_eq!(body.get("scopes"), Some(&json!([])));
        }

        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(7, "main::run")]);
        for frame_id in [6_i64, 8] {
            let response = adapter.handle_scopes(1, 1, Some(json!({ "frameId": frame_id })));
            let DapMessage::Response { body: Some(body), .. } = response else {
                return Err("non-current frame response did not contain a body".into());
            };
            assert_eq!(body.get("scopes"), Some(&json!([])));
        }
        Ok(())
    }

    #[test]
    fn scopes_without_a_stopped_session_are_empty() -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        let response = adapter.handle_scopes(1, 1, Some(json!({ "frameId": 7 })));
        let DapMessage::Response { body: Some(body), .. } = response else {
            return Err("no-session response did not contain a body".into());
        };
        assert_eq!(body.get("scopes"), Some(&json!([])));

        adapter.seed_running_session_for_test();
        let response = adapter.handle_scopes(1, 1, Some(json!({ "frameId": 7 })));
        let DapMessage::Response { body: Some(body), .. } = response else {
            return Err("running-session response did not contain a body".into());
        };
        assert_eq!(body.get("scopes"), Some(&json!([])));
        Ok(())
    }

    #[test]
    fn scopes_from_terminated_session_are_empty() -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(1, "main::run")]);
        {
            let mut guard = lock_or_recover(&adapter.session, "test.terminated_session");
            let session = guard.as_mut().ok_or("test session was not seeded")?;
            session.state = DebugState::Terminated;
        }

        let response = adapter.handle_scopes(1, 1, Some(json!({ "frameId": 1 })));
        let DapMessage::Response { body: Some(body), .. } = response else {
            return Err("terminated-session response did not contain a body".into());
        };
        assert_eq!(body.get("scopes"), Some(&json!([])));
        Ok(())
    }

    #[test]
    fn prior_suspension_scope_reference_cannot_revive_after_new_stop()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::debug_adapter::var_ref::{ScopeKind, VariableReference};

        let adapter = DebugAdapter::new();
        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(1, "main::run")]);
        let old_scope = VariableReference::Scope { frame_id: 1, kind: ScopeKind::Locals }
            .encode()
            .ok_or("old scope reference did not encode")?;

        // The next stop has a new generation-derived frame id, even when the
        // visible source/line and logical frame are otherwise unchanged.
        {
            let mut guard = lock_or_recover(&adapter.session, "test.new_stop_generation");
            let session = guard.as_mut().ok_or("test session was not seeded")?;
            session.stopped_generation = 2;
            session.stack_frames = vec![make_frame(2, "main::run")];
        }

        let stale =
            adapter.handle_variables(1, 1, Some(json!({ "variablesReference": old_scope })));
        let DapMessage::Response { body: Some(body), .. } = stale else {
            return Err("stale scope response did not contain a body".into());
        };
        assert_eq!(body.get("variables"), Some(&json!([])));
        Ok(())
    }
}
