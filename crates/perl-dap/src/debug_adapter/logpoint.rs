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

/// Upper bound on debugger lines a single capture may consume before giving up and
/// emitting the raw templates. Prevents a wedged or dead debuggee from suppressing
/// logpoint output forever.
const MAX_CAPTURE_LINES: usize = 200;

/// What the reader loop should do with the line it just handed to a capture.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum LogpointStep {
    /// Line belongs to the capture frame; do not forward it to the client.
    Consumed,
    /// Line is unrelated; handle it normally.
    Passthrough,
    /// Capture is complete; take the interpolated messages and drop this line.
    Finished,
    /// Capture gave up waiting; take the interpolated messages, then handle this
    /// line normally — it was never part of the frame.
    Abandoned,
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
    /// Build a capture for `templates`, or `None` when none of them reference a
    /// scalar the adapter can resolve (in which case the templates are already their
    /// own final text).
    pub(super) fn new(marker_id: u64, templates: Vec<String>) -> Option<Self> {
        let mut names: Vec<String> = Vec::new();
        for template in &templates {
            for name in referenced_scalars(template) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        if names.is_empty() {
            return None;
        }

        Some(Self {
            begin_marker: format!("DAP_LOGPOINT_BEGIN_{marker_id}"),
            end_marker: format!("DAP_LOGPOINT_END_{marker_id}"),
            names,
            templates,
            values: HashMap::new(),
            saw_begin: false,
            lines_seen: 0,
        })
    }

    /// Debugger commands that frame and answer this capture, in write order.
    pub(super) fn query_commands(&self) -> Vec<String> {
        let mut commands = Vec::with_capacity(self.names.len() + 2);
        commands.push(format!("p \"{}\"\n", self.begin_marker));
        for name in &self.names {
            // `name` is a bare identifier (see `is_plain_scalar_name`), so this is a
            // literal command, not user-controlled Perl.
            commands.push(format!(
                "p \"{VALUE_PREFIX}{name}\\t\" . (defined(${name}) ? ${name} : \"undef\")\n"
            ));
        }
        commands.push(format!("p \"{}\"\n", self.end_marker));
        commands
    }

    /// Feed one debugger output line to the capture.
    pub(super) fn observe_line(&mut self, line: &str) -> LogpointStep {
        self.lines_seen += 1;
        if self.lines_seen > MAX_CAPTURE_LINES {
            // The debuggee never answered; fall back to the raw templates rather than
            // swallowing the logpoint.
            return LogpointStep::Abandoned;
        }

        if !self.saw_begin {
            if line.contains(&self.begin_marker) {
                self.saw_begin = true;
                return LogpointStep::Consumed;
            }
            return LogpointStep::Passthrough;
        }

        if line.contains(&self.end_marker) {
            return LogpointStep::Finished;
        }

        if let Some(idx) = line.find(VALUE_PREFIX) {
            let payload = &line[idx + VALUE_PREFIX.len()..];
            if let Some((name, value)) = payload.split_once('\t') {
                self.values.insert(name.to_string(), value.to_string());
            }
        }

        // Everything between the markers is adapter-internal framing noise.
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

    #[test]
    fn referenced_scalars_collects_distinct_plain_names() {
        assert_eq!(referenced_scalars("a={$x} b={$y} c={$x}"), vec!["x", "y"]);
    }

    #[test]
    fn referenced_scalars_skips_expressions_the_interpolator_leaves_verbatim() {
        assert!(referenced_scalars("{@list} {%h} {$x + 1} {$Pkg::x} {$x[0]} {}").is_empty());
    }

    #[test]
    fn no_capture_when_template_has_nothing_to_resolve() {
        assert!(PendingLogpoint::new(1, vec!["plain message".to_string()]).is_none());
    }

    #[test]
    fn query_commands_are_framed_and_quote_free() -> Result<(), String> {
        let pending = PendingLogpoint::new(7, vec!["x={$x}".to_string()])
            .ok_or("template references $x, so a capture is expected")?;
        let commands = pending.query_commands();
        assert_eq!(commands.len(), 3, "begin marker, one value query, end marker");
        assert!(commands[0].contains("DAP_LOGPOINT_BEGIN_7"));
        assert!(commands[1].contains("DAPLPV:x"), "value query must be self-identifying");
        assert!(commands[2].contains("DAP_LOGPOINT_END_7"));
        Ok(())
    }

    #[test]
    fn capture_folds_framed_values_into_the_template() -> Result<(), String> {
        let mut pending = PendingLogpoint::new(3, vec!["x={$x} y={$y}".to_string()])
            .ok_or("template references scalars, so a capture is expected")?;

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
            .ok_or("template references $x, so a capture is expected")?;
        assert_eq!(pending.observe_line("DAP_LOGPOINT_BEGIN_4"), LogpointStep::Consumed);
        assert_eq!(pending.observe_line("DAP_LOGPOINT_END_4"), LogpointStep::Finished);
        assert_eq!(pending.into_messages(), vec!["x={$x}".to_string()]);
        Ok(())
    }

    #[test]
    fn capture_gives_up_instead_of_swallowing_the_message() -> Result<(), String> {
        let mut pending = PendingLogpoint::new(5, vec!["x={$x}".to_string()])
            .ok_or("template references $x, so a capture is expected")?;
        for _ in 0..MAX_CAPTURE_LINES {
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
}
