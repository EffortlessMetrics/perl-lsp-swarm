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
            RegexEventKind::Atom
            | RegexEventKind::Escape
            | RegexEventKind::QuotedLiteral { .. }
            | RegexEventKind::CharacterClass { .. }
            | RegexEventKind::UnicodeProperty { .. }
            | RegexEventKind::Interpolation { .. } => {
                last_atom = LastAtom::Atom;
            }
            RegexEventKind::Comment(_) => {}
            RegexEventKind::ModeChange
            | RegexEventKind::Alternation
            | RegexEventKind::Malformed(_) => {
                last_atom = LastAtom::Nothing;
            }
        }
    }

    findings
}
