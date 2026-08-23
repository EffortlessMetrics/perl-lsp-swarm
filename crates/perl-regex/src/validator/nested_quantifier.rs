use crate::syntax::event::{RegexEventKind, RegexEventStream, RegexGroupKind, RegexQuantifierMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GroupFrame {
    has_backtracking_quantifier: bool,
    is_atomic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastAtom {
    None,
    Other,
    Group { has_backtracking_quantifier: bool, is_atomic: bool },
}

pub(crate) fn find_nested_quantifiers(stream: &RegexEventStream) -> Vec<usize> {
    let mut group_stack = Vec::new();
    let mut last_atom = LastAtom::None;
    let mut findings = Vec::new();

    for event in &stream.events {
        match event.kind {
            RegexEventKind::GroupOpen(kind) => {
                group_stack.push(GroupFrame {
                    has_backtracking_quantifier: false,
                    is_atomic: matches!(kind, RegexGroupKind::Atomic),
                });
                last_atom = LastAtom::None;
            }
            RegexEventKind::GroupClose(_) => {
                if let Some(frame) = group_stack.pop() {
                    let has_backtracking_quantifier =
                        frame.has_backtracking_quantifier && !frame.is_atomic;
                    if has_backtracking_quantifier && let Some(parent) = group_stack.last_mut() {
                        parent.has_backtracking_quantifier = true;
                    }
                    last_atom =
                        LastAtom::Group { has_backtracking_quantifier, is_atomic: frame.is_atomic };
                } else {
                    last_atom = LastAtom::None;
                }
            }
            RegexEventKind::Quantifier(quantifier) => {
                if let LastAtom::Group { has_backtracking_quantifier: true, .. } = last_atom
                    && quantifier.repeats_atom()
                    && !matches!(quantifier.mode, RegexQuantifierMode::Possessive)
                {
                    findings.push(event.range.start);
                }

                if quantifier.is_backtracking()
                    && let Some(parent) = group_stack.last_mut()
                    && !matches!(last_atom, LastAtom::Group { is_atomic: true, .. })
                {
                    parent.has_backtracking_quantifier = true;
                }
                last_atom = LastAtom::None;
            }
            RegexEventKind::Atom
            | RegexEventKind::Escape
            | RegexEventKind::QuotedLiteral { .. }
            | RegexEventKind::CharacterClass { .. }
            | RegexEventKind::UnicodeProperty { .. }
            | RegexEventKind::Interpolation { .. } => {
                last_atom = LastAtom::Other;
            }
            RegexEventKind::Comment(_) => {}
            RegexEventKind::ModeChange
            | RegexEventKind::Alternation
            | RegexEventKind::EmbeddedCode { .. }
            | RegexEventKind::Malformed(_) => {
                last_atom = LastAtom::None;
            }
        }
    }

    findings
}
