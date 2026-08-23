use super::variable_cache::VariableCache;
use crate::types::StackFrame;
use std::collections::HashMap;
use std::process::Child;

/// Active debug session
pub(super) struct DebugSession {
    /// Perl debugger process
    pub(super) process: Child,
    /// Current execution state
    pub(super) state: DebugState,
    /// Stack frames
    pub(super) stack_frames: Vec<StackFrame>,
    /// Best-effort arguments captured from verbose stack output, keyed by frame id.
    pub(super) stack_frame_arguments: HashMap<i32, Vec<String>>,
    /// Variables in current scope, including root scopes and child expansions.
    pub(super) variable_cache: VariableCache,
    /// Thread ID
    pub(super) thread_id: i32,
    /// Last resume command issued while running.
    pub(super) last_resume_mode: ResumeMode,
    /// Monotonic stopped-suspension authority used to prevent old frame ids
    /// from becoming valid again when the debugger reuses a numeric frame id.
    pub(super) stopped_generation: u64,
}

#[derive(Debug, Clone, PartialEq)]
// Terminated is recorded by shutdown paths even when some targets only observe Running/Stopped.
#[allow(dead_code)]
pub(super) enum DebugState {
    Running,
    Stopped,
    Terminated,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ResumeMode {
    Continue,
    /// Like `Continue` but auto-continues past any non-breakpoint stop.
    /// Used when `configurationDone` runs with `stopOnEntry: false` to
    /// silently skip the debugger's implicit first-line stop and run to
    /// the first user-set breakpoint.
    RunToBreakpoint,
    Next,
    StepIn,
    StepOut,
    Goto,
    Unknown,
}
