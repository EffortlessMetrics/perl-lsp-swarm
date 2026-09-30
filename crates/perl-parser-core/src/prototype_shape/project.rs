//! Single projector from raw prototype text to [`PrototypeShape`].

use super::{
    PrototypeBoundary, PrototypeCompleteness, PrototypeDefault, PrototypeRecovery,
    PrototypeReferent, PrototypeShape, PrototypeSlot, PrototypeSlotKind, PrototypeSyntaxClass,
    digest,
};

/// Project raw prototype text (no surrounding parentheses) into a shape.
#[must_use]
pub fn project_prototype_shape(raw: &str) -> PrototypeShape {
    let mut slots = Vec::new();
    let mut optional_boundary = None;
    let mut optional = false;
    let mut recovery = None;
    let mut topic_seen = false;
    let mut index = 0usize;

    while index < raw.len() {
        let ch = match next_char(raw, index) {
            Some(ch) => ch,
            None => break,
        };
        let ch_len = ch.len_utf8();
        if ch.is_ascii_whitespace() {
            index = index.saturating_add(ch_len);
            continue;
        }
        if !is_prototype_sigil(ch) {
            note_recovery(&mut recovery, PrototypeRecovery::InvalidCharacter);
            index = index.saturating_add(ch_len);
            continue;
        }
        match ch {
            ';' => {
                if optional_boundary.is_some() {
                    note_recovery(&mut recovery, PrototypeRecovery::DuplicateOptionalBoundary);
                } else {
                    optional_boundary =
                        Some(PrototypeBoundary { start: index, end: index.saturating_add(ch_len) });
                    optional = true;
                }
                index = index.saturating_add(ch_len);
            }
            ']' => {
                note_recovery(&mut recovery, PrototypeRecovery::UnexpectedCloser);
                index = index.saturating_add(ch_len);
            }
            '\\' => match project_reference(raw, index) {
                Ok((slot_kind, end)) => {
                    let slot_raw = slice_raw(raw, index, end);
                    slots.push(PrototypeSlot {
                        kind: slot_kind,
                        optional,
                        default: PrototypeDefault::None,
                        raw: slot_raw,
                        start: index,
                        end,
                    });
                    index = end;
                }
                Err((reason, end)) => {
                    note_recovery(&mut recovery, reason);
                    index = end.max(index.saturating_add(ch_len));
                }
            },
            '_' => {
                let after = index.saturating_add(ch_len);
                if topic_seen || !topic_default_is_legal(raw, after) {
                    note_recovery(&mut recovery, PrototypeRecovery::InvalidTopicDefaultPosition);
                }
                topic_seen = true;
                slots.push(PrototypeSlot {
                    kind: PrototypeSlotKind::TopicDefaultScalar,
                    optional,
                    default: PrototypeDefault::TopicVariable,
                    raw: "_".to_string(),
                    start: index,
                    end: after,
                });
                index = after;
            }
            other => {
                let kind = match other {
                    '$' => Some(PrototypeSlotKind::Scalar),
                    '@' => Some(PrototypeSlotKind::ArraySlurpy),
                    '%' => Some(PrototypeSlotKind::HashSlurpy),
                    '&' => Some(PrototypeSlotKind::Code),
                    '*' => Some(PrototypeSlotKind::Glob),
                    '+' => Some(PrototypeSlotKind::ScalarOrReference),
                    '[' => {
                        note_recovery(&mut recovery, PrototypeRecovery::UnsupportedForm);
                        None
                    }
                    _ => {
                        note_recovery(&mut recovery, PrototypeRecovery::InvalidCharacter);
                        None
                    }
                };
                if let Some(kind) = kind {
                    let end = index.saturating_add(ch_len);
                    slots.push(PrototypeSlot {
                        kind,
                        optional,
                        default: PrototypeDefault::None,
                        raw: other.to_string(),
                        start: index,
                        end,
                    });
                    index = end;
                } else {
                    index = index.saturating_add(ch_len);
                }
            }
        }
    }

    if slot_follows_unbackslashed_slurpy(&slots) {
        note_recovery(&mut recovery, PrototypeRecovery::SlotAfterSlurpy);
    }

    let completeness = match recovery {
        None => PrototypeCompleteness::Exact,
        Some(reason) => PrototypeCompleteness::Recovered { reason },
    };
    let syntax_class = syntax_class(&slots);
    let semantic_digest =
        digest::PrototypeSemanticDigest::from_shape(&completeness, syntax_class, &slots);
    PrototypeShape {
        raw: raw.to_string(),
        slots,
        optional_boundary,
        completeness,
        syntax_class,
        semantic_digest,
    }
}

/// Inner text of a `:prototype(...)` attribute (`prototype($$)` → `$$`).
#[must_use]
pub fn raw_from_attribute(attr: &str) -> Option<&str> {
    attr.strip_prefix("prototype(").and_then(|rest| rest.strip_suffix(')'))
}

/// Return true if `c` is admitted in an old-style prototype (`perlsub`).
///
/// ASCII whitespace is formatting, not a slot. Tab, newline, CR, and form-feed
/// are therefore valid, matching Perl's prototype character class.
#[must_use]
pub(crate) const fn is_prototype_char(c: char) -> bool {
    c.is_ascii_whitespace() || is_prototype_sigil(c)
}

fn project_reference(
    raw: &str,
    start: usize,
) -> Result<(PrototypeSlotKind, usize), (PrototypeRecovery, usize)> {
    let slash_end = start.saturating_add('\\'.len_utf8());
    let Some(next) = next_significant(raw, slash_end) else {
        return Err((PrototypeRecovery::DanglingBackslash, raw.len()));
    };
    if next.ch == '[' {
        return project_group(raw, start, next.index.saturating_add(next.ch.len_utf8()));
    }
    let referent = match referent_from_char(next.ch) {
        Some(referent) => referent,
        None => {
            return Err((
                PrototypeRecovery::DanglingBackslash,
                next.index.saturating_add(next.ch.len_utf8()),
            ));
        }
    };
    Ok((PrototypeSlotKind::ReferenceTo(referent), next.index.saturating_add(next.ch.len_utf8())))
}

fn project_group(
    raw: &str,
    _start: usize,
    mut index: usize,
) -> Result<(PrototypeSlotKind, usize), (PrototypeRecovery, usize)> {
    let mut referents = Vec::new();
    while index < raw.len() {
        let Some(ch) = next_char(raw, index) else {
            break;
        };
        if ch.is_ascii_whitespace() {
            index = index.saturating_add(ch.len_utf8());
            continue;
        }
        if ch == ']' {
            let end = index.saturating_add(ch.len_utf8());
            if referents.is_empty() {
                return Err((PrototypeRecovery::EmptyGroup, end));
            }
            return Ok((PrototypeSlotKind::GroupedReference(referents), end));
        }
        match referent_from_char(ch) {
            Some(referent) => {
                referents.push(referent);
                index = index.saturating_add(ch.len_utf8());
            }
            None => {
                return Err((
                    PrototypeRecovery::UnsupportedForm,
                    index.saturating_add(ch.len_utf8()),
                ));
            }
        }
    }
    Err((PrototypeRecovery::UnclosedGroup, raw.len()))
}

struct SignificantChar {
    index: usize,
    ch: char,
}

fn next_char(raw: &str, index: usize) -> Option<char> {
    raw.get(index..)?.chars().next()
}

fn next_significant(raw: &str, mut index: usize) -> Option<SignificantChar> {
    while index < raw.len() {
        let ch = next_char(raw, index)?;
        if !ch.is_ascii_whitespace() {
            return Some(SignificantChar { index, ch });
        }
        index = index.saturating_add(ch.len_utf8());
    }
    None
}

fn topic_default_is_legal(raw: &str, after: usize) -> bool {
    match next_significant(raw, after).map(|item| item.ch) {
        None | Some(';' | '@' | '%') => true,
        Some(_) => false,
    }
}

fn referent_from_char(ch: char) -> Option<PrototypeReferent> {
    match ch {
        '$' => Some(PrototypeReferent::Scalar),
        '@' => Some(PrototypeReferent::Array),
        '%' => Some(PrototypeReferent::Hash),
        '&' => Some(PrototypeReferent::Code),
        '*' => Some(PrototypeReferent::Glob),
        _ => None,
    }
}

const fn is_prototype_sigil(ch: char) -> bool {
    matches!(ch, '$' | '@' | '%' | '&' | '*' | '\\' | ';' | '+' | '_' | '[' | ']')
}

fn slot_follows_unbackslashed_slurpy(slots: &[PrototypeSlot]) -> bool {
    slots.iter().enumerate().any(|(index, slot)| {
        matches!(slot.kind, PrototypeSlotKind::ArraySlurpy | PrototypeSlotKind::HashSlurpy)
            && index.saturating_add(1) < slots.len()
    })
}

fn note_recovery(slot: &mut Option<PrototypeRecovery>, reason: PrototypeRecovery) {
    if slot.is_none() {
        *slot = Some(reason);
    }
}

fn slice_raw(raw: &str, start: usize, end: usize) -> String {
    raw.get(start..end).unwrap_or_default().to_string()
}

fn syntax_class(slots: &[PrototypeSlot]) -> PrototypeSyntaxClass {
    if slots.is_empty() {
        return PrototypeSyntaxClass::Nullary;
    }
    if let Some(first) = slots.first()
        && !first.optional
        && matches!(first.kind, PrototypeSlotKind::Code)
    {
        return PrototypeSyntaxClass::BlockTaking;
    }
    PrototypeSyntaxClass::Ordinary
}
