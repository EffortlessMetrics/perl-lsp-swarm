use crate::syntax::event::{RegexEventKind, RegexEventStream, RegexGroupKind};

use super::{
    analysis::{RegexDiagnostic, RegexDiagnosticCode},
    config::RegexValidationConfig,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupKind {
    Other,
    Lookbehind,
    BranchReset,
}

struct OpenGroup {
    kind: GroupKind,
    branch_count: usize,
}

/// O(1)-per-event nesting state. Depth is maintained by counters, not by
/// rescanning [`OpenGroup`] frames on each [`RegexEventKind::GroupOpen`].
struct GroupNesting {
    frames: Vec<OpenGroup>,
    lookbehind_depth: usize,
    branch_reset_depth: usize,
    /// Push, pop, and top-frame branch bumps. Each is O(1) and independent of
    /// nesting depth.
    frame_ops: usize,
    /// Frames visited by a full stack walk used to compute depth. Running
    /// counters keep this at zero.
    depth_scan_visits: usize,
}

impl GroupNesting {
    fn new() -> Self {
        Self {
            frames: Vec::new(),
            lookbehind_depth: 0,
            branch_reset_depth: 0,
            frame_ops: 0,
            depth_scan_visits: 0,
        }
    }

    fn open(&mut self, kind: GroupKind) -> usize {
        let depth = match kind {
            GroupKind::Lookbehind => {
                let depth = self.lookbehind_depth;
                self.lookbehind_depth = self.lookbehind_depth.saturating_add(1);
                depth
            }
            GroupKind::BranchReset => {
                let depth = self.branch_reset_depth;
                self.branch_reset_depth = self.branch_reset_depth.saturating_add(1);
                depth
            }
            GroupKind::Other => 0,
        };
        self.frames.push(OpenGroup {
            kind,
            branch_count: match kind {
                GroupKind::BranchReset => 1,
                GroupKind::Lookbehind | GroupKind::Other => 0,
            },
        });
        self.frame_ops = self.frame_ops.saturating_add(1);
        depth
    }

    fn close(&mut self) {
        self.frame_ops = self.frame_ops.saturating_add(1);
        match self.frames.pop().map(|frame| frame.kind) {
            Some(GroupKind::Lookbehind) => {
                self.lookbehind_depth = self.lookbehind_depth.saturating_sub(1);
            }
            Some(GroupKind::BranchReset) => {
                self.branch_reset_depth = self.branch_reset_depth.saturating_sub(1);
            }
            Some(GroupKind::Other) | None => {}
        }
    }

    fn bump_branch(&mut self) -> Option<usize> {
        self.frame_ops = self.frame_ops.saturating_add(1);
        match self.frames.last_mut() {
            Some(OpenGroup { kind: GroupKind::BranchReset, branch_count }) => {
                *branch_count = branch_count.saturating_add(1);
                Some(*branch_count)
            }
            Some(OpenGroup { kind: GroupKind::Lookbehind | GroupKind::Other, .. }) | None => None,
        }
    }
}

/// Counted work for the complexity walk. `frame_ops` is O(events); restored
/// O(depth) stack scans would accumulate in `depth_scan_visits`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ComplexityWork {
    pub(crate) events: usize,
    pub(crate) frame_ops: usize,
    pub(crate) depth_scan_visits: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComplexityScan {
    pub(crate) diagnostics: Vec<RegexDiagnostic>,
    pub(crate) work: ComplexityWork,
    pub(crate) lookbehind_depth: usize,
    pub(crate) branch_reset_depth: usize,
    pub(crate) open_frames: usize,
}

pub(crate) fn find_complexity_diagnostics(
    stream: &RegexEventStream,
    config: &RegexValidationConfig,
) -> Vec<RegexDiagnostic> {
    scan_complexity(stream, config).diagnostics
}

pub(crate) fn scan_complexity(
    stream: &RegexEventStream,
    config: &RegexValidationConfig,
) -> ComplexityScan {
    let mut nesting = GroupNesting::new();
    let mut unicode_property_count = 0usize;
    let mut emitted_unicode_limit = false;
    let mut emitted_lookbehind_limit = false;
    let mut emitted_branch_reset_nesting_limit = false;
    let mut emitted_branch_reset_branch_limit = false;
    let mut diagnostics = Vec::new();
    let mut events = 0usize;

    for event in &stream.events {
        events = events.saturating_add(1);
        match event.kind {
            RegexEventKind::UnicodeProperty { .. } => {
                unicode_property_count = unicode_property_count.saturating_add(1);
                emit_once(
                    &mut emitted_unicode_limit,
                    &mut diagnostics,
                    unicode_property_count > config.max_unicode_properties,
                    RegexDiagnosticCode::UnicodePropertyLimit,
                    event.range,
                    config.max_unicode_properties,
                );
            }
            RegexEventKind::GroupOpen(kind) => {
                let group_kind = match kind {
                    RegexGroupKind::Lookbehind | RegexGroupKind::NegativeLookbehind => {
                        GroupKind::Lookbehind
                    }
                    RegexGroupKind::BranchReset => GroupKind::BranchReset,
                    _ => GroupKind::Other,
                };
                let depth = nesting.open(group_kind);
                match group_kind {
                    GroupKind::Lookbehind => emit_once(
                        &mut emitted_lookbehind_limit,
                        &mut diagnostics,
                        depth >= config.max_nesting,
                        RegexDiagnosticCode::LookbehindNestingLimit,
                        event.range,
                        config.max_nesting,
                    ),
                    GroupKind::BranchReset => emit_once(
                        &mut emitted_branch_reset_nesting_limit,
                        &mut diagnostics,
                        depth >= config.max_nesting,
                        RegexDiagnosticCode::BranchResetNestingLimit,
                        event.range,
                        config.max_nesting,
                    ),
                    GroupKind::Other => {}
                }
            }
            RegexEventKind::Alternation => {
                if let Some(branch_count) = nesting.bump_branch() {
                    emit_once(
                        &mut emitted_branch_reset_branch_limit,
                        &mut diagnostics,
                        branch_count > config.max_branch_reset_branches,
                        RegexDiagnosticCode::BranchResetBranchLimit,
                        event.range,
                        config.max_branch_reset_branches,
                    );
                }
            }
            RegexEventKind::GroupClose(_) => {
                nesting.close();
            }
            _ => {}
        }
    }

    ComplexityScan {
        diagnostics,
        work: ComplexityWork {
            events,
            frame_ops: nesting.frame_ops,
            depth_scan_visits: nesting.depth_scan_visits,
        },
        lookbehind_depth: nesting.lookbehind_depth,
        branch_reset_depth: nesting.branch_reset_depth,
        open_frames: nesting.frames.len(),
    }
}

fn emit_once(
    emitted: &mut bool,
    diagnostics: &mut Vec<RegexDiagnostic>,
    should_emit: bool,
    code: RegexDiagnosticCode,
    range: super::analysis::RegexRange,
    limit: usize,
) {
    if !should_emit || *emitted {
        return;
    }
    diagnostics.push(RegexDiagnostic::new(code, range, Some(limit)));
    *emitted = true;
}
