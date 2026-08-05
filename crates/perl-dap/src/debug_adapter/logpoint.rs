//! Live logpoint interpolation.
//!
//! [`crate::breakpoints::interpolate_logpoint_message`] can substitute `{$var}`
//! references in a logpoint template, but until #5045 nothing supplied it with real
//! values on the live path: the output reader called `register_breakpoint_hit` (no
//! variables) and users saw the raw template `x = {$x}` instead of `x = 42`.
//!
//! The reader thread is the sole consumer of the debugger's control stream, so it
//! cannot make a blocking request/response round trip through the shared
//! recent-output buffer the way `evaluate` does — it *is* the producer of that
//! buffer. Instead it queues a framed `p` query for the scalars a template
//! references, keeps a [`PendingLogpoint`] alongside its own read loop, and folds
//! the reply lines into the template as they stream past.
//!
//! Only names matching `[A-Za-z_][A-Za-z0-9_]*` are queried, so a template can never
//! smuggle a Perl expression into the debugger command stream.

use std::collections::HashMap;

/// Prefix marking a single `name<TAB>value` reply line inside a logpoint frame.
const VALUE_PREFIX: &str = "DAPLPV:";

/// Placeholder the scalar name is substituted into in [`VALUE_QUERY`].
const NAME_SLOT: &str = "__DAP_NAME__";

/// Command that reports one scalar as exactly one line.
///
/// The escaping is the point. This is a line-oriented protocol layered on the
/// debugger's output stream, so a value that contains a newline — file contents, a
/// stack trace, any multi-line string — would otherwise arrive split across several
/// `read_line` calls, and everything after the first line would be swallowed as
/// framing noise. Escaping backslashes first, then CR and LF, keeps one reply on one
/// line; [`unescape_value`] restores the original text.
const VALUE_QUERY: &str = concat!(
    "p do { my $v = defined($",
    "__DAP_NAME__",
    ") ? \"$",
    "__DAP_NAME__",
    "\" : \"undef\"; ",
    "$v =~ s/\\\\/\\\\\\\\/g; $v =~ s/\\n/\\\\n/g; $v =~ s/\\r/\\\\r/g; ",
    "\"DAPLPV:",
    "__DAP_NAME__",
    "\\t\" . $v }"
);

/// Upper bound on debugger lines a single capture may consume *after* its begin
/// marker, before giving up and emitting the raw templates. Prevents a wedged or dead
/// debuggee from suppressing logpoint output forever.
const MAX_CAPTURE_LINES: usize = 200;

/// Upper bound on *unrecognised* lines the drain tolerates before giving up.
///
/// Value replies do not count against this: they are the frame content the drain
/// exists to swallow, so counting them would let a long frame close the drain early
/// and leak its own tail to the client.
pub(super) const MAX_DRAIN_NOISE_LINES: usize = 64;

/// Hard ceiling on *all* lines a drain may consume, value replies included.
///
/// Without it, a debuggee emitting endless `DAPLPV:`-looking output would hold the
/// drain open forever and the reader would go deaf to real debuggee output. Both
/// budgets are needed: one bounds waiting, the other bounds the frame itself.
pub(super) const MAX_DRAIN_TOTAL_LINES: usize = 512;

/// What the reader should do with a line handed to an open drain.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DrainStep {
    /// Line is adapter framing; swallow it and keep draining.
    Swallow,
    /// Frame is closed (end marker seen, or a budget ran out); swallow this line and
    /// drop the drain.
    Done,
}

/// The residual-frame filter that runs after a capture is abandoned mid-frame.
///
/// Extracted from the reader loop so its precedence and budget rules are directly
/// testable — every defect found in this logic so far has been in a rule that could
/// not be exercised where it lived.
#[derive(Debug)]
pub(super) struct LogpointDrain {
    end_marker: String,
    noise_remaining: usize,
    total_remaining: usize,
}

impl LogpointDrain {
    pub(super) fn new(end_marker: String) -> Self {
        Self {
            end_marker,
            noise_remaining: MAX_DRAIN_NOISE_LINES,
            total_remaining: MAX_DRAIN_TOTAL_LINES,
        }
    }

    /// Feed one line to the drain.
    pub(super) fn observe_line(&mut self, line: &str) -> DrainStep {
        self.total_remaining = self.total_remaining.saturating_sub(1);
        if self.total_remaining == 0 {
            return DrainStep::Done;
        }

        // A value reply is frame content, not noise: it must neither close the drain
        // (even when its text contains the end marker) nor spend the waiting budget.
        if is_value_reply(line) {
            return DrainStep::Swallow;
        }

        if line.contains(&self.end_marker) {
            return DrainStep::Done;
        }

        self.noise_remaining = self.noise_remaining.saturating_sub(1);
        if self.noise_remaining == 0 { DrainStep::Done } else { DrainStep::Swallow }
    }
}

/// Whether a debugger line is one of this protocol's value replies.
///
/// The drain that runs after a mid-frame abandonment needs the same
/// value-before-end-marker precedence as [`PendingLogpoint::observe_line`]: a value
/// whose own text contains the end marker must not be mistaken for the marker, or
/// the drain closes early and leaks the rest of the frame to the client.
pub(super) fn is_value_reply(line: &str) -> bool {
    line.contains(VALUE_PREFIX)
}

/// Upper bound on lines the capture will wait for its begin marker to appear.
///
/// Separate from [`MAX_CAPTURE_LINES`] so that a debuggee which is merely chatty
/// before answering is not mistaken for one that never will. Both phases stay
/// bounded, so neither can wedge the reader.
const MAX_WAIT_LINES: usize = 500;

/// Reverse the escaping applied by [`VALUE_QUERY`].
fn unescape_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            // Not an escape this protocol produces; keep it verbatim rather than
            // inventing a character.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// What the reader loop should do with the line it just handed to a capture.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum LogpointStep {
    /// Line belongs to the capture frame; do not forward it to the client.
    Consumed,
    /// Line is unrelated; handle it normally.
    Passthrough,
    /// Capture is complete; take the interpolated messages and drop this line.
    Finished,
    /// Capture gave up *before* its frame opened; take the interpolated messages,
    /// then handle this line normally — it was never part of the frame.
    Abandoned,
    /// Capture gave up *inside* an open frame; take the interpolated messages and
    /// drop this line. It sits between the markers, so it is adapter framing rather
    /// than debuggee output, and the reader must keep suppressing until the end
    /// marker arrives or nothing else in the frame would be filtered.
    AbandonedInFrame,
}

/// Extract the distinct, safely-queryable scalar names a logpoint template refers to.
///
/// `"a={$x} b={$y} c={$x}"` yields `["x", "y"]`. Anything that is not a plain scalar
/// reference — `{@list}`, `{$x + 1}`, `{$Pkg::x}` — is left verbatim by the
/// interpolator, so it is not queried here either.
pub(super) fn referenced_scalars(template: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut rest = template;

    while let Some(open) = rest.find('{') {
        let after_open = &rest[open + 1..];
        let Some(close) = after_open.find('}') else {
            break;
        };
        let expr = after_open[..close].trim();
        if let Some(name) = expr.strip_prefix('$')
            && is_plain_scalar_name(name)
            && !names.iter().any(|existing| existing == name)
        {
            names.push(name.to_string());
        }
        rest = &after_open[close + 1..];
    }

    names
}

/// A name is queryable only if it is a bare identifier: no sigil games, no package
/// separators, no subscripts, and therefore nothing that needs quoting when it is
/// spliced into a debugger command.
fn is_plain_scalar_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// An in-flight logpoint value query.
#[derive(Debug)]
pub(super) struct PendingLogpoint {
    begin_marker: String,
    end_marker: String,
    /// Distinct scalar names the templates reference, in first-seen order.
    names: Vec<String>,
    templates: Vec<String>,
    values: HashMap<String, String>,
    saw_begin: bool,
    lines_seen: usize,
}

impl PendingLogpoint {
    /// Build a capture for `templates`.
    ///
    /// Returns `Err(templates)` — never a bare `None` — when none of them reference a
    /// scalar the adapter can resolve. Handing the templates back is what keeps a
    /// caller from moving them in and losing them: in that case they are already
    /// their own final text and must still be emitted.
    pub(super) fn new(marker_id: u64, templates: Vec<String>) -> Result<Self, Vec<String>> {
        let mut names: Vec<String> = Vec::new();
        for template in &templates {
            for name in referenced_scalars(template) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        if names.is_empty() {
            return Err(templates);
        }

        Ok(Self {
            begin_marker: format!("DAP_LOGPOINT_BEGIN_{marker_id}"),
            end_marker: format!("DAP_LOGPOINT_END_{marker_id}"),
            names,
            templates,
            values: HashMap::new(),
            saw_begin: false,
            lines_seen: 0,
        })
    }

    /// Marker that closes this capture's frame.
    ///
    /// The reader needs it to keep filtering residual frame lines after the capture
    /// has been abandoned mid-frame.
    pub(super) fn end_marker(&self) -> &str {
        &self.end_marker
    }

    /// Debugger commands that frame and answer this capture, in write order.
    pub(super) fn query_commands(&self) -> Vec<String> {
        let mut commands = Vec::with_capacity(self.names.len() + 2);
        commands.push(format!("p \"{}\"\n", self.begin_marker));
        for name in &self.names {
            // `name` is a bare identifier (see `is_plain_scalar_name`), so the only
            // dynamic part of this command is a literal identifier.
            commands.push(format!("{}\n", VALUE_QUERY.replace(NAME_SLOT, name)));
        }
        commands.push(format!("p \"{}\"\n", self.end_marker));
        commands
    }

    /// Feed one debugger output line to the capture.
    pub(super) fn observe_line(&mut self, line: &str) -> LogpointStep {
        self.lines_seen += 1;

        if !self.saw_begin {
            if line.contains(&self.begin_marker) {
                self.saw_begin = true;
                // Restart the budget: what matters from here is how long the *answer*
                // takes, not how chatty the debuggee was before it started.
                self.lines_seen = 0;
                return LogpointStep::Consumed;
            }
            if self.lines_seen > MAX_WAIT_LINES {
                return LogpointStep::Abandoned;
            }
            return LogpointStep::Passthrough;
        }

        // A value line is checked first so that a scalar whose text happens to contain
        // the end marker cannot terminate its own frame.
        if let Some(idx) = line.find(VALUE_PREFIX) {
            let payload = &line[idx + VALUE_PREFIX.len()..];
            if let Some((name, value)) = payload.split_once('\t') {
                self.values.insert(name.to_string(), unescape_value(value.trim_end()));
                return LogpointStep::Consumed;
            }
        }

        if line.contains(&self.end_marker) {
            return LogpointStep::Finished;
        }

        if self.lines_seen > MAX_CAPTURE_LINES {
            // The debuggee never finished answering; fall back to the raw templates
            // rather than swallowing the logpoint. This line is still inside the
            // frame, so it must not reach the client.
            return LogpointStep::AbandonedInFrame;
        }

        // Everything else between the markers is adapter-internal framing noise.
        LogpointStep::Consumed
    }

    /// Interpolated messages, using whatever values arrived before completion.
    pub(super) fn into_messages(self) -> Vec<String> {
        self.templates
            .iter()
            .map(|template| {
                crate::breakpoints::interpolate_logpoint_message(template, &self.values)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The post-abandonment drain reuses this predicate to keep value-before-end-
    /// marker precedence. Without it, a late value whose text contains the frame's
    /// end marker closes the drain early and the rest of the frame — later replies
    /// and the real marker — leaks to the client as debuggee output.
    #[test]
    fn value_replies_are_recognised_regardless_of_their_text() {
        assert!(is_value_reply("DAPLPV:x\t42"), "a plain value reply is a value line");
        assert!(
            is_value_reply("DAPLPV:x\tsaw DAP_LOGPOINT_END_3 in the log"),
            "a value whose text contains an end marker is still a value line"
        );
        assert!(
            is_value_reply("  DB<4> DAPLPV:x\t42"),
            "a prompt-prefixed value reply is still a value line"
        );
        assert!(!is_value_reply("DAP_LOGPOINT_END_3"), "the end marker is not a value line");
        assert!(!is_value_reply("ordinary debuggee output"), "debuggee output is not a value line");
    }

    /// Value replies are frame content, not waiting. Counting them against the
    /// give-up budget let a frame with many scalars close its own drain early and
    /// leak the tail — later replies and the end marker — to the client.
    #[test]
    fn value_replies_do_not_spend_the_drain_waiting_budget() {
        let mut drain = LogpointDrain::new("DAP_LOGPOINT_END_9".to_string());
        for _ in 0..(MAX_DRAIN_NOISE_LINES * 2) {
            assert_eq!(
                drain.observe_line("DAPLPV:x\t42"),
                DrainStep::Swallow,
                "a value reply must never close the drain"
            );
        }
        assert_eq!(
            drain.observe_line("DAP_LOGPOINT_END_9"),
            DrainStep::Done,
            "the real end marker still closes the drain after many value replies"
        );
    }

    #[test]
    fn a_value_containing_the_end_marker_does_not_close_the_drain() {
        let mut drain = LogpointDrain::new("DAP_LOGPOINT_END_9".to_string());
        assert_eq!(
            drain.observe_line("DAPLPV:x\tsaw DAP_LOGPOINT_END_9 in the log"),
            DrainStep::Swallow,
            "a value is a value regardless of the text it carries"
        );
    }

    #[test]
    fn unrecognised_noise_still_bounds_the_drain() {
        let mut drain = LogpointDrain::new("DAP_LOGPOINT_END_9".to_string());
        let mut closed_after = None;
        for i in 1..=(MAX_DRAIN_NOISE_LINES + 5) {
            if drain.observe_line("ordinary debuggee output") == DrainStep::Done {
                closed_after = Some(i);
                break;
            }
        }
        assert_eq!(
            closed_after,
            Some(MAX_DRAIN_NOISE_LINES),
            "an end marker that never arrives must not suppress real output forever"
        );
    }

    /// The total ceiling is what stops endless value-looking output from wedging the
    /// reader once value replies no longer spend the waiting budget.
    #[test]
    fn endless_value_replies_cannot_wedge_the_drain() {
        let mut drain = LogpointDrain::new("DAP_LOGPOINT_END_9".to_string());
        let mut closed = false;
        for _ in 0..(MAX_DRAIN_TOTAL_LINES + 10) {
            if drain.observe_line("DAPLPV:x\t42") == DrainStep::Done {
                closed = true;
                break;
            }
        }
        assert!(closed, "the total ceiling must bound a debuggee that never stops replying");
    }

    #[test]
    fn referenced_scalars_collects_distinct_plain_names() {
        assert_eq!(referenced_scalars("a={$x} b={$y} c={$x}"), vec!["x", "y"]);
    }

    #[test]
    fn referenced_scalars_skips_expressions_the_interpolator_leaves_verbatim() {
        assert!(
            referenced_scalars("{@list} {%h} {$x + 1} {$Pkg::x} {$x[0]} {}").is_empty(),
            "only bare scalar identifiers are queryable"
        );
    }

    /// `concat!` cannot reference `NAME_SLOT`, so the placeholder is spelled twice:
    /// once as the constant and once inline. Renaming one silently stops
    /// substitution, and every value would come back `undef`.
    #[test]
    fn value_query_embeds_the_name_slot_constant() {
        assert_eq!(
            VALUE_QUERY.matches(NAME_SLOT).count(),
            3,
            "VALUE_QUERY must embed NAME_SLOT at every substitution site"
        );
    }

    /// A template with nothing to resolve must hand its text back rather than be
    /// swallowed: the caller moves the messages in, so a discarding `None` would
    /// silently drop a plain logpoint like `"reached here"`.
    #[test]
    fn templates_with_nothing_to_resolve_are_returned_not_dropped() -> Result<(), String> {
        let templates = vec!["plain message".to_string(), "{@list} stays put".to_string()];
        let returned = PendingLogpoint::new(1, templates.clone())
            .err()
            .ok_or("a template with no resolvable scalar must not build a capture")?;
        assert_eq!(returned, templates, "templates must come back intact");
        Ok(())
    }

    #[test]
    fn query_commands_are_framed_and_quote_free() -> Result<(), String> {
        let pending = PendingLogpoint::new(7, vec!["x={$x}".to_string()])
            .map_err(|_| "template references $x, so a capture is expected")?;
        let commands = pending.query_commands();
        assert_eq!(commands.len(), 3, "begin marker, one value query, end marker");
        assert!(commands[0].contains("DAP_LOGPOINT_BEGIN_7"), "first command opens the frame");
        assert!(commands[1].contains("DAPLPV:x"), "value query must be self-identifying");
        assert!(commands[2].contains("DAP_LOGPOINT_END_7"), "last command closes the frame");
        Ok(())
    }

    /// The query must keep a multi-line value on one line. Without the escaping, a
    /// value containing a newline arrives split across `read_line` boundaries and
    /// everything after the first segment is swallowed as framing noise.
    #[test]
    fn value_query_escapes_so_one_value_is_one_line() -> Result<(), String> {
        let pending = PendingLogpoint::new(9, vec!["x={$x}".to_string()])
            .map_err(|_| "template references $x, so a capture is expected")?;
        let query = pending.query_commands().remove(1);

        assert!(query.contains("$v =~ s/\\n/\\\\n/g"), "newlines must be escaped: {query}");
        assert!(query.contains("$v =~ s/\\r/\\\\r/g"), "carriage returns must be escaped");
        assert!(
            query.find("s/\\\\/\\\\\\\\/g") < query.find("s/\\n/\\\\n/g"),
            "backslashes must be escaped first or the escaping is ambiguous"
        );
        assert_eq!(query.matches('\n').count(), 1, "the command itself is a single line");
        Ok(())
    }

    #[test]
    fn multi_line_values_survive_the_round_trip() -> Result<(), String> {
        let mut pending = PendingLogpoint::new(11, vec!["x is {$x}".to_string()])
            .map_err(|_| "template references $x, so a capture is expected")?;
        assert_eq!(pending.observe_line("DAP_LOGPOINT_BEGIN_11"), LogpointStep::Consumed);
        // What the escaped query puts on the wire for "hello\nworld".
        assert_eq!(pending.observe_line("DAPLPV:x\thello\\nworld"), LogpointStep::Consumed);
        assert_eq!(pending.observe_line("DAP_LOGPOINT_END_11"), LogpointStep::Finished);

        assert_eq!(pending.into_messages(), vec!["x is hello\nworld".to_string()]);
        Ok(())
    }

    #[test]
    fn escaped_backslashes_are_not_mistaken_for_escapes() {
        assert_eq!(unescape_value(r"C:\\path\nfile"), "C:\\path\nfile");
        assert_eq!(unescape_value(r"literal \q stays"), r"literal \q stays");
        assert_eq!(unescape_value(r"trailing \"), r"trailing \");
    }

    /// A value whose text contains the frame's own end marker must not terminate the
    /// frame — the value line is recognized first.
    #[test]
    fn a_value_containing_the_end_marker_does_not_end_the_frame() -> Result<(), String> {
        let mut pending = PendingLogpoint::new(12, vec!["x={$x} y={$y}".to_string()])
            .map_err(|_| "template references scalars, so a capture is expected")?;
        assert_eq!(pending.observe_line("DAP_LOGPOINT_BEGIN_12"), LogpointStep::Consumed);
        assert_eq!(
            pending.observe_line("DAPLPV:x\tsaw DAP_LOGPOINT_END_12 in the log"),
            LogpointStep::Consumed,
            "a value line is a value, even when its text looks like the end marker"
        );
        assert_eq!(
            pending.observe_line("DAPLPV:y\tsecond value still arrives"),
            LogpointStep::Consumed
        );
        assert_eq!(pending.observe_line("DAP_LOGPOINT_END_12"), LogpointStep::Finished);

        assert_eq!(
            pending.into_messages(),
            vec!["x=saw DAP_LOGPOINT_END_12 in the log y=second value still arrives".to_string()]
        );
        Ok(())
    }

    /// The line budget bounds how long the *answer* may take. A debuggee that is
    /// merely chatty before the marker arrives must still get its capture.
    #[test]
    fn noise_before_the_begin_marker_does_not_consume_the_answer_budget() -> Result<(), String> {
        let mut pending = PendingLogpoint::new(13, vec!["x={$x}".to_string()])
            .map_err(|_| "template references $x, so a capture is expected")?;

        for _ in 0..(MAX_CAPTURE_LINES + 20) {
            assert_eq!(
                pending.observe_line("chatty debuggee output"),
                LogpointStep::Passthrough,
                "pre-marker noise is not part of the frame"
            );
        }

        assert_eq!(
            pending.observe_line("DAP_LOGPOINT_BEGIN_13"),
            LogpointStep::Consumed,
            "a late but valid begin marker must still open the frame"
        );
        assert_eq!(pending.observe_line("DAPLPV:x\t42"), LogpointStep::Consumed);
        assert_eq!(pending.observe_line("DAP_LOGPOINT_END_13"), LogpointStep::Finished);
        assert_eq!(pending.into_messages(), vec!["x=42".to_string()]);
        Ok(())
    }

    #[test]
    fn waiting_for_a_begin_marker_that_never_arrives_still_gives_up() -> Result<(), String> {
        let mut pending = PendingLogpoint::new(14, vec!["x={$x}".to_string()])
            .map_err(|_| "template references $x, so a capture is expected")?;

        let mut abandoned = false;
        for _ in 0..(MAX_WAIT_LINES + 5) {
            if pending.observe_line("noise") == LogpointStep::Abandoned {
                abandoned = true;
                break;
            }
        }
        assert!(abandoned, "a marker that never arrives must not wedge the reader forever");
        Ok(())
    }

    #[test]
    fn capture_folds_framed_values_into_the_template() -> Result<(), String> {
        let mut pending = PendingLogpoint::new(3, vec!["x={$x} y={$y}".to_string()])
            .map_err(|_| "template references scalars, so a capture is expected")?;

        assert_eq!(pending.observe_line("unrelated debuggee output"), LogpointStep::Passthrough);
        assert_eq!(pending.observe_line("DAP_LOGPOINT_BEGIN_3"), LogpointStep::Consumed);
        // perl5db glues its prompt onto reply lines; parsing must tolerate that.
        assert_eq!(pending.observe_line("  DB<4> DAPLPV:x\t42"), LogpointStep::Consumed);
        assert_eq!(pending.observe_line("DAPLPV:y\thello"), LogpointStep::Consumed);
        assert_eq!(pending.observe_line("DAP_LOGPOINT_END_3"), LogpointStep::Finished);

        assert_eq!(pending.into_messages(), vec!["x=42 y=hello".to_string()]);
        Ok(())
    }

    #[test]
    fn missing_value_keeps_the_original_expression() -> Result<(), String> {
        let mut pending = PendingLogpoint::new(4, vec!["x={$x}".to_string()])
            .map_err(|_| "template references $x, so a capture is expected")?;
        assert_eq!(pending.observe_line("DAP_LOGPOINT_BEGIN_4"), LogpointStep::Consumed);
        assert_eq!(pending.observe_line("DAP_LOGPOINT_END_4"), LogpointStep::Finished);
        assert_eq!(pending.into_messages(), vec!["x={$x}".to_string()]);
        Ok(())
    }

    #[test]
    fn capture_gives_up_instead_of_swallowing_the_message() -> Result<(), String> {
        let mut pending = PendingLogpoint::new(5, vec!["x={$x}".to_string()])
            .map_err(|_| "template references $x, so a capture is expected")?;
        for _ in 0..MAX_WAIT_LINES {
            assert_eq!(pending.observe_line("noise"), LogpointStep::Passthrough);
        }
        assert_eq!(
            pending.observe_line("still no marker"),
            LogpointStep::Abandoned,
            "a debuggee that never answers must not suppress the logpoint forever"
        );
        assert_eq!(pending.into_messages(), vec!["x={$x}".to_string()]);
        Ok(())
    }

    /// The other half of giving up: the frame opened and then stalled. The partial
    /// values that did arrive are still better than nothing.
    #[test]
    fn a_frame_that_opens_but_never_closes_is_abandoned_with_what_it_has() -> Result<(), String> {
        let mut pending = PendingLogpoint::new(6, vec!["x={$x} y={$y}".to_string()])
            .map_err(|_| "template references scalars, so a capture is expected")?;
        assert_eq!(pending.observe_line("DAP_LOGPOINT_BEGIN_6"), LogpointStep::Consumed);
        assert_eq!(pending.observe_line("DAPLPV:x\t42"), LogpointStep::Consumed);

        let mut abandoned = false;
        for _ in 0..(MAX_CAPTURE_LINES + 5) {
            if pending.observe_line("frame noise, no end marker") == LogpointStep::AbandonedInFrame
            {
                abandoned = true;
                break;
            }
        }
        assert!(
            abandoned,
            "an unterminated frame must not suppress the logpoint forever, and must report \
             that it gave up *inside* the frame so the reader keeps filtering it"
        );
        assert_eq!(
            pending.into_messages(),
            vec!["x=42 y={$y}".to_string()],
            "the value that did arrive is kept; the one that did not stays verbatim"
        );
        Ok(())
    }
}
