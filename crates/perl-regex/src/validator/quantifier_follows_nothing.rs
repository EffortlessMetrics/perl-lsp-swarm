use crate::syntax::event::{RegexEventKind, RegexEventStream};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastAtom {
    /// No quantifiable atom precedes the next quantifier in this alternation
    /// branch: pattern start, just after `|`, or just after a group opener.
    Nothing,
    /// A quantifiable atom precedes the next quantifier.
    Atom,
    /// A quantifier was just consumed. A further quantifier here is a nested
    /// quantifier (reported separately), not one following nothing.
    Quantified,
}

/// One open group in the detector-local group stack.
#[derive(Debug, Clone, Copy)]
struct GroupFrameInfo {
    /// This group is the condition of an enclosing conditional, so its
    /// closer starts the conditional's first branch rather than supplying
    /// a quantifiable atom.
    is_condition_child: bool,
    /// A conditional whose assertion/code condition has not yet appeared:
    /// `(?(?=...)...)` opens as a bare `(?`, so the condition is the next
    /// direct child group or embedded-code block. A simple condition such
    /// as `(?(1)...)` is consumed by the opener itself and never awaits.
    awaiting_condition: bool,
}

/// Locate `*`, `+`, and `?` quantifiers with no preceding atom, mirroring
/// Perl's `Quantifier follows nothing in regex` compile error.
///
/// The event stream already suppresses quantifier interpretation inside
/// character classes, `\Q...\E` literals, escapes, and comments, so those
/// guards are inherited rather than reimplemented. A `{m,n}` form at a
/// follows-nothing position is a literal in Perl (verified against
/// perl 5.42: `/{2}abc/` compiles), so brace quantifiers are treated as
/// literal atoms there instead of findings.
pub(crate) fn find_quantifiers_following_nothing(
    stream: &RegexEventStream,
    pattern: &str,
) -> Vec<usize> {
    let mut last_atom = LastAtom::Nothing;
    let mut findings = Vec::new();
    let mut groups: Vec<GroupFrameInfo> = Vec::new();

    for event in &stream.events {
        match event.kind {
            RegexEventKind::GroupOpen(_) => {
                let opener = pattern.get(event.range.start..event.range.end);
                let awaiting_condition = opener == Some("(?");
                let is_condition_child =
                    groups.last().is_some_and(|frame| frame.awaiting_condition);
                if let Some(frame) = groups.last_mut() {
                    frame.awaiting_condition = false;
                }
                // Only a conditional opener (`(?(` with a bare `(?` range)
                // awaits a separate condition construct.
                let is_conditional_opener =
                    pattern.get(event.range.start..).is_some_and(|rest| rest.starts_with("(?("));
                groups.push(GroupFrameInfo {
                    is_condition_child,
                    awaiting_condition: awaiting_condition && is_conditional_opener,
                });
                last_atom = LastAtom::Nothing;
            }
            RegexEventKind::GroupClose(_) => {
                let popped = groups.pop();
                last_atom = if popped.is_some_and(|frame| frame.is_condition_child) {
                    // The condition closer does not authorize a quantifier:
                    // perl rejects `(?(?=a)*b|c)` (verified perl 5.42).
                    LastAtom::Nothing
                } else {
                    LastAtom::Atom
                };
            }
            RegexEventKind::EmbeddedCode { .. } => {
                if groups.last().is_some_and(|frame| frame.awaiting_condition) {
                    if let Some(frame) = groups.last_mut() {
                        frame.awaiting_condition = false;
                    }
                    last_atom = LastAtom::Nothing;
                } else {
                    last_atom = LastAtom::Atom;
                }
            }
            RegexEventKind::Quantifier(_) => {
                let is_brace = pattern.as_bytes().get(event.range.start) == Some(&b'{');
                match last_atom {
                    LastAtom::Nothing if is_brace => {
                        last_atom = LastAtom::Atom;
                    }
                    LastAtom::Nothing => {
                        findings.push(event.range.start);
                        last_atom = LastAtom::Quantified;
                    }
                    LastAtom::Atom | LastAtom::Quantified => {
                        last_atom = LastAtom::Quantified;
                    }
                }
            }
            RegexEventKind::QuotedLiteral { .. } => {
                // An empty `\Q\E` quotes zero characters and preserves the
                // preceding state: perl accepts `a\Q\E*` (`*` repeats `a`)
                // and rejects `\Q\E*a` at branch start (both perl 5.42).
                let text = pattern.get(event.range.start..event.range.end);
                if text != Some(r"\Q\E") {
                    last_atom = LastAtom::Atom;
                }
            }
            RegexEventKind::Atom
            | RegexEventKind::Escape
            | RegexEventKind::CharacterClass { .. }
            | RegexEventKind::UnicodeProperty { .. }
            | RegexEventKind::Interpolation { .. } => {
                if let Some(frame) = groups.last_mut() {
                    frame.awaiting_condition = false;
                }
                last_atom = LastAtom::Atom;
            }
            // A malformed construct (e.g. an unmatched `)`) is reported on its
            // own; it does not erase the preceding atom. Perl reports only
            // "Unmatched )" for `a)**b`, never a follows-nothing quantifier.
            RegexEventKind::Comment(_) | RegexEventKind::Malformed(_) => {}
            RegexEventKind::ModeChange | RegexEventKind::Alternation => {
                last_atom = LastAtom::Nothing;
            }
        }
    }

    findings
}
