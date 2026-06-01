use super::policy::line_is_whole_line_comment;

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
