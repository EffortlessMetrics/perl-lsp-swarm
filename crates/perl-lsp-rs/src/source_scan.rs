//! Shared source occupancy helpers for `--lib` vs `--all-targets` proof.
//!
//! Lives at crate root so scanner literals cannot satisfy a formatting-policy
//! occupancy ratchet, and so all-target Clippy tests reuse one trivia walker.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PanicFamilyLint {
    UnwrapUsed,
    ExpectUsed,
    Panic,
}

impl PanicFamilyLint {
    const fn as_str(self) -> &'static str {
        match self {
            Self::UnwrapUsed => "clippy::unwrap_used",
            Self::ExpectUsed => "clippy::expect_used",
            Self::Panic => "clippy::panic",
        }
    }

    /// Clippy accepts raw-identifier lint spellings (`clippy::r#panic`);
    /// the ratchet must read them the same as the bare names.
    const fn as_raw_str(self) -> &'static str {
        match self {
            Self::UnwrapUsed => "clippy::r#unwrap_used",
            Self::ExpectUsed => "clippy::r#expect_used",
            Self::Panic => "clippy::r#panic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuppressionKind {
    Allow,
    Expect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuppressionScope {
    Inner,
    Outer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PanicFamilySuppression {
    pub kind: SuppressionKind,
    pub scope: SuppressionScope,
    pub lints: Vec<PanicFamilyLint>,
    pub has_reason: bool,
    /// True when an outer attribute decorates `mod` / `impl` / `trait`.
    /// Those items are still blankets even with `expect` + `reason`.
    pub decorates_wide_item: bool,
}

impl PanicFamilySuppression {
    pub(crate) fn is_forbidden(&self) -> bool {
        match self.scope {
            SuppressionScope::Inner => true,
            SuppressionScope::Outer => {
                self.kind == SuppressionKind::Allow || !self.has_reason || self.decorates_wide_item
            }
        }
    }
}

pub(crate) fn rest_at(source: &str, i: usize) -> &str {
    source.get(i..).unwrap_or("")
}

fn ident_continues(source: &str, ident_len: usize) -> bool {
    rest_at(source, ident_len)
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn starts_with_keyword(rest: &str, kw: &str) -> bool {
    rest.starts_with(kw) && !ident_continues(rest, kw.len())
}

/// Skip remaining outer attributes, visibility, and `unsafe`/`const`/`auto`/
/// `default` prefixes, then report whether the decorated item is `mod`,
/// `impl`, or `trait` (a module-wide blanket, not a narrow function).
fn following_item_is_wide(source: &str, mut i: usize) -> bool {
    i = skip_outer_attributes(source, i);
    i = skip_visibility(source, i);
    i = skip_trivia(source, i);
    loop {
        let rest = rest_at(source, i);
        let prefix_len = if starts_with_keyword(rest, "unsafe") {
            6
        } else if starts_with_keyword(rest, "auto") {
            4
        } else if starts_with_keyword(rest, "default") {
            7
        } else if starts_with_keyword(rest, "const") {
            5
        } else {
            break;
        };
        i = skip_trivia(source, i + prefix_len);
    }
    let rest = rest_at(source, i);
    starts_with_keyword(rest, "mod")
        || starts_with_keyword(rest, "impl")
        || starts_with_keyword(rest, "trait")
}

fn scan_char_literal(source: &str, i: usize) -> Option<usize> {
    let rest = rest_at(source, i);
    if !rest.starts_with('\'') {
        return None;
    }
    let mut chars = rest.char_indices().skip(1);
    let (_, first) = chars.next()?;
    if first == '\\' {
        let _escaped = chars.next()?;
        let (off, quote) = chars.next()?;
        if quote == '\'' {
            return Some(i + off + quote.len_utf8());
        }
        return None;
    }
    let (off, quote) = chars.next()?;
    if quote == '\'' { Some(i + off + quote.len_utf8()) } else { None }
}

pub(crate) fn scan_comment_or_string(source: &str, i: usize) -> Option<usize> {
    let rest = rest_at(source, i);
    if rest.starts_with("//") {
        return Some(match rest.find('\n') {
            Some(n) => i + n + 1,
            None => source.len(),
        });
    }
    if rest.starts_with("/*") {
        // Rust block comments nest; skip to the matching outer `*/` so a
        // panic-family attribute after an inner close stays comment text.
        let bytes = rest.as_bytes();
        let mut depth = 0usize;
        let mut j = 0usize;
        while j + 1 < bytes.len() {
            if bytes[j] == b'/' && bytes[j + 1] == b'*' {
                depth += 1;
                j += 2;
            } else if bytes[j] == b'*' && bytes[j + 1] == b'/' {
                depth = depth.saturating_sub(1);
                j += 2;
                if depth == 0 {
                    return Some(i + j);
                }
            } else {
                j += 1;
            }
        }
        return Some(source.len());
    }
    if let Some(end) = scan_char_literal(source, i) {
        return Some(end);
    }
    if rest.starts_with('r') {
        let bytes = rest.as_bytes();
        let mut hashes = 0;
        let mut j = 1;
        while j < bytes.len() && bytes[j] == b'#' {
            hashes += 1;
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b'"' {
            j += 1;
            let close = format!("\"{}", "#".repeat(hashes));
            return rest
                .get(j..)
                .and_then(|tail| tail.find(&close))
                .map(|n| i + j + n + close.len());
        }
    }
    if rest.starts_with('"') {
        let mut escaped = false;
        for (off, ch) in rest.char_indices().skip(1) {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                return Some(i + off + ch.len_utf8());
            }
        }
        return Some(source.len());
    }
    None
}

pub(crate) fn skip_trivia(source: &str, mut i: usize) -> usize {
    while i < source.len() {
        let rest = rest_at(source, i);
        if rest.starts_with(|ch: char| ch.is_whitespace()) {
            i += rest.chars().next().map_or(0, char::len_utf8);
            continue;
        }
        match scan_comment_or_string(source, i) {
            Some(end) if end > i => i = end,
            _ => break,
        }
    }
    i
}

pub(crate) fn skip_balanced(source: &str, start: usize, open: char, close: char) -> usize {
    let mut i = start;
    let mut depth = 0_i32;
    while i < source.len() {
        if let Some(end) = scan_comment_or_string(source, i) {
            i = end;
            continue;
        }
        let Some(ch) = rest_at(source, i).chars().next() else { break };
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            i += ch.len_utf8();
            if depth == 0 {
                return i;
            }
            continue;
        }
        i += ch.len_utf8();
    }
    i
}

fn skip_outer_attributes(source: &str, mut i: usize) -> usize {
    loop {
        i = skip_trivia(source, i);
        if rest_at(source, i).starts_with("#[") {
            i += 1;
            i = skip_balanced(source, i, '[', ']');
            continue;
        }
        break;
    }
    i
}

fn skip_visibility(source: &str, i: usize) -> usize {
    if !rest_at(source, i).starts_with("pub") {
        return i;
    }
    let after_pub = i + 3;
    let rest = rest_at(source, after_pub);
    if rest.starts_with(|ch: char| ch.is_ascii_alphanumeric() || ch == '_') {
        return i;
    }
    if rest.starts_with('(') {
        return skip_balanced(source, after_pub, '(', ')');
    }
    after_pub
}

fn skip_item(source: &str, mut i: usize) -> usize {
    i = skip_trivia(source, i);
    i = skip_visibility(source, i);
    i = skip_trivia(source, i);
    while i < source.len() {
        if let Some(end) = scan_comment_or_string(source, i) {
            i = end;
            continue;
        }
        let Some(ch) = rest_at(source, i).chars().next() else { break };
        match ch {
            '{' => return skip_balanced(source, i, '{', '}'),
            '(' => i = skip_balanced(source, i, '(', ')'),
            '[' => i = skip_balanced(source, i, '[', ']'),
            ';' => return i + 1,
            _ => i += ch.len_utf8(),
        }
    }
    i
}

/// Source rustc compiles for `--lib`: drop each `#[cfg(test)]` item, keep
/// later ungated code, and ignore the marker inside comments or strings.
pub(crate) fn lib_source(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < source.len() {
        if let Some(end) = scan_comment_or_string(source, i) {
            out.push_str(&source[i..end]);
            i = end;
            continue;
        }
        if rest_at(source, i).starts_with("#[cfg(test)]") {
            i += "#[cfg(test)]".len();
            i = skip_outer_attributes(source, i);
            i = skip_item(source, i);
            continue;
        }
        let Some(ch) = rest_at(source, i).chars().next() else { break };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// True when the attribute body names a lint group that covers the whole
/// panic family: `clippy::restriction` contains `unwrap_used`, `expect_used`,
/// and `panic`, and the bare `warnings` group suppresses every Clippy warning
/// including them. Bare group spellings require `at_list_boundary` (true when
/// only trivia separates the name from the list's `(`, a `,`, or the start of
/// the body) so names like `sandbox_restrictions` cannot match; the
/// tool-qualified `clippy::restriction` is unambiguous anywhere.
fn group_covers_panic_family(body: &str, i: usize, at_list_boundary: bool) -> bool {
    let rest = rest_at(body, i);
    if rest.starts_with("clippy::restriction")
        && !ident_continues(rest, "clippy::restriction".len())
    {
        return true;
    }
    if !at_list_boundary {
        return false;
    }
    if rest.starts_with("restriction") && !ident_continues(rest, "restriction".len()) {
        return true;
    }
    rest.starts_with("warnings") && !ident_continues(rest, "warnings".len())
}

fn panic_family_lints_in(body: &str) -> Vec<PanicFamilyLint> {
    const FAMILY: [PanicFamilyLint; 3] =
        [PanicFamilyLint::UnwrapUsed, PanicFamilyLint::ExpectUsed, PanicFamilyLint::Panic];
    let mut lints = Vec::new();
    let mut i = 0;
    // Trivia (whitespace, comments, string literals) never updates this, so a
    // rustfmt-style multiline element such as `allow(\n    warnings,\n)`
    // still reads as a list element start.
    let mut at_list_boundary = true;
    while i < body.len() {
        if let Some(end) = scan_comment_or_string(body, i) {
            i = end;
            continue;
        }
        let rest = rest_at(body, i);
        let ch = rest.chars().next().unwrap_or('\0');
        if ch.is_whitespace() {
            i += ch.len_utf8();
            continue;
        }
        let group_hit = group_covers_panic_family(body, i, at_list_boundary);
        for lint in FAMILY {
            if lints.contains(&lint) {
                continue;
            }
            let name = lint.as_str();
            let raw_name = lint.as_raw_str();
            let named = (rest.starts_with(name) && !ident_continues(rest, name.len()))
                || (rest.starts_with(raw_name) && !ident_continues(rest, raw_name.len()));
            if named || group_hit {
                lints.push(lint);
            }
        }
        at_list_boundary = matches!(ch, '(' | ',');
        i += ch.len_utf8();
    }
    lints
}

fn attr_has_reason(body: &str) -> bool {
    let mut i = 0;
    while i < body.len() {
        if let Some(end) = scan_comment_or_string(body, i) {
            i = end;
            continue;
        }
        let rest = rest_at(body, i);
        if rest.starts_with("reason") && !ident_continues(rest, 6) {
            let after = skip_trivia(body, i + 6);
            if rest_at(body, after).starts_with('=') {
                return true;
            }
        }
        let Some(ch) = rest.chars().next() else { break };
        i += ch.len_utf8();
    }
    false
}

fn attr_kind(body: &str) -> Option<SuppressionKind> {
    let mut i = 0;
    let mut found_expect = false;
    while i < body.len() {
        if let Some(end) = scan_comment_or_string(body, i) {
            i = end;
            continue;
        }
        let rest = rest_at(body, i);
        if rest.starts_with("allow") && !ident_continues(rest, 5) {
            return Some(SuppressionKind::Allow);
        }
        if rest.starts_with("expect") && !ident_continues(rest, 6) {
            found_expect = true;
        }
        let Some(ch) = rest.chars().next() else { break };
        i += ch.len_utf8();
    }
    found_expect.then_some(SuppressionKind::Expect)
}

/// Live `allow`/`expect` attributes that name panic-family Clippy lints.
pub(crate) fn panic_family_suppressions(source: &str) -> Vec<PanicFamilySuppression> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < source.len() {
        if let Some(end) = scan_comment_or_string(source, i) {
            i = end;
            continue;
        }
        let rest = rest_at(source, i);
        let (scope, attr_start) = if rest.starts_with("#![") {
            (SuppressionScope::Inner, i + 2)
        } else if rest.starts_with("#[") {
            (SuppressionScope::Outer, i + 1)
        } else {
            let Some(ch) = rest.chars().next() else { break };
            i += ch.len_utf8();
            continue;
        };
        let attr_end = skip_balanced(source, attr_start, '[', ']');
        let Some(body) = source.get(attr_start + 1..attr_end.saturating_sub(1)) else {
            i = attr_end.max(i + 1);
            continue;
        };
        let lints = panic_family_lints_in(body);
        if !lints.is_empty()
            && let Some(kind) = attr_kind(body)
        {
            out.push(PanicFamilySuppression {
                kind,
                scope,
                lints,
                has_reason: attr_has_reason(body),
                decorates_wide_item: scope == SuppressionScope::Outer
                    && following_item_is_wide(source, attr_end),
            });
        }
        i = attr_end.max(i + 1);
    }
    out
}
