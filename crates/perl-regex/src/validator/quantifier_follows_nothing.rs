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

    for event in &stream.events {
        match event.kind {
            RegexEventKind::GroupOpen(_) => {
                last_atom = LastAtom::Nothing;
            }
            RegexEventKind::GroupClose(_) | RegexEventKind::EmbeddedCode { .. } => {
                last_atom = LastAtom::Atom;
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
                // An empty `\Q\E` quotes zero characters, so it is not a
                // quantifiable atom: perl rejects `\Q\E*a` with
                // "Quantifier follows nothing" (verified against perl 5.42).
                let text = pattern.get(event.range.start..event.range.end);
                last_atom = if text == Some(r"\Q\E") { LastAtom::Nothing } else { LastAtom::Atom };
            }
            RegexEventKind::Atom
            | RegexEventKind::Escape
            | RegexEventKind::CharacterClass { .. }
            | RegexEventKind::UnicodeProperty { .. }
            | RegexEventKind::Interpolation { .. } => {
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
