/// Returns `true` when a source file should be skipped wholesale by the print-macro check.
///
/// Files with a file-level `#![allow(clippy::print_stderr)]` or
/// `#![allow(clippy::print_stdout)]` attribute have been explicitly opted out of the
/// rule (e.g. `cli.rs` in the LSP binary crate). The attribute must appear in the
/// first 30 lines of the file (the module-doc / crate-doc block).
pub(super) fn file_has_print_allow(lines: &[String]) -> bool {
    lines.iter().take(30).any(|line| line_has_inner_print_allow_attr(line))
}

fn line_has_inner_print_allow_attr(line: &str) -> bool {
    line.contains("#![allow(")
        && (line.contains("clippy::print_stderr")
            || line.contains("clippy::print_stdout")
            || line.contains("clippy::print_"))
}

pub(super) fn line_has_outer_print_allow_attr(line: &str) -> bool {
    line.contains("#[allow(")
        && (line.contains("clippy::print_stderr")
            || line.contains("clippy::print_stdout")
            || line.contains("clippy::print_"))
}

pub(super) fn line_is_whole_line_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

#[derive(Default)]
pub(super) struct PrintAllowScope {
    pending_attr: bool,
    active_brace_depth: usize,
}

impl PrintAllowScope {
    pub(super) fn note_attribute(&mut self) {
        self.pending_attr = true;
    }

    pub(super) fn allows_current_line(&self) -> bool {
        self.pending_attr || self.active_brace_depth > 0
    }

    pub(super) fn observe_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() || line_is_whole_line_comment(line) || trimmed.starts_with("#[") {
            return;
        }

        if self.active_brace_depth > 0 {
            self.apply_brace_delta(line);
            return;
        }

        if self.pending_attr {
            self.pending_attr = false;
            let delta = brace_delta(line);
            if delta > 0 {
                self.active_brace_depth = delta as usize;
            }
        }
    }

    fn apply_brace_delta(&mut self, line: &str) {
        let delta = brace_delta(line);
        if delta.is_negative() {
            self.active_brace_depth = self.active_brace_depth.saturating_sub(delta.unsigned_abs());
        } else {
            self.active_brace_depth = self.active_brace_depth.saturating_add(delta as usize);
        }
    }
}

fn brace_delta(line: &str) -> isize {
    let opens = line.chars().filter(|ch| *ch == '{').count() as isize;
    let closes = line.chars().filter(|ch| *ch == '}').count() as isize;
    opens - closes
}
