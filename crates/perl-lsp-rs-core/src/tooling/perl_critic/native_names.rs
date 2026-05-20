//! Identifier rename helpers for native critic quick-fix suggestions.
//!
//! Keep name-shaping logic centralized so rules can share consistent fix naming
//! conventions without reimplementing sigil parsing.

pub(super) fn shadowed_lexical_name(name: &str) -> String {
    let (sigil, base_name) = split_sigil(name);
    format!("{sigil}inner_{base_name}")
}

pub(super) fn numbered_duplicate_name(name: &str) -> String {
    let (sigil, base_name) = split_sigil(name);
    format!("{sigil}{base_name}_2")
}

pub(super) fn parameter_shadow_name(name: &str) -> String {
    let (sigil, base_name) = split_sigil(name);
    format!("{sigil}p_{base_name}")
}

pub(super) fn prefixed_unused_name(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(sigil @ ('$' | '@' | '%' | '&' | '*')) => {
            let rest = chars.as_str();
            format!("{sigil}_{rest}")
        }
        _ => format!("_{name}"),
    }
}

pub(super) fn bareword_filehandle_lexical_name(name: &str) -> String {
    format!("${}_fh", name.to_lowercase())
}

fn split_sigil(name: &str) -> (&str, &str) {
    let bare = name.trim_start_matches(['$', '@', '%', '&', '*']);
    let sigil_len = name.len() - bare.len();
    (&name[..sigil_len], bare)
}
