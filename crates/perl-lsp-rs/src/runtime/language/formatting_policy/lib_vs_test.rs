//! Discriminating `--lib` vs `--tests` occupancy for formatting geometry types (#9618).
//!
//! Lives outside `multi_range.rs` so scanner literals and `'{'` char helpers cannot
//! satisfy or confuse the ratchet they enforce.

fn rest_at(source: &str, i: usize) -> &str {
    source.get(i..).unwrap_or("")
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

fn scan_comment_or_string(source: &str, i: usize) -> Option<usize> {
    let rest = rest_at(source, i);
    if rest.starts_with("//") {
        return Some(match rest.find('\n') {
            Some(n) => i + n + 1,
            None => source.len(),
        });
    }
    if rest.starts_with("/*") {
        return Some(match rest.get(2..).and_then(|tail| tail.find("*/")) {
            Some(n) => i + 2 + n + 2,
            None => source.len(),
        });
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

fn skip_trivia(source: &str, mut i: usize) -> usize {
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

fn skip_balanced(source: &str, start: usize, open: char, close: char) -> usize {
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
fn lib_source(source: &str) -> String {
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

fn lib_source_names_format_geometry(source: &str) -> bool {
    let lib = lib_source(source);
    lib.contains("FormatPosition") || lib.contains("FormatRange")
}

fn fn_source<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    let sig = format!("fn {name}(");
    let start = source.find(&sig)?;
    let brace = source[start..].find('{')?;
    let open = start + brace;
    let end = skip_balanced(source, open, '{', '}');
    source.get(start..end)
}

fn edit_helper_constructs_format_geometry(source: &str) -> bool {
    fn_source(source, "edit")
        .is_some_and(|edit| edit.contains("FormatPosition") && edit.contains("FormatRange"))
}

#[test]
fn module_scope_format_geometry_import_is_visible_to_lib_source() {
    let original_defect = r#"
use perl_lsp_rs_core::providers::formatting_types::{FormatPosition, FormatRange};

fn production() {}

#[cfg(test)]
mod tests {
    fn uses_them() {}
}
"#;
    assert!(
        lib_source_names_format_geometry(original_defect),
        "the #9618 unused-import shape must remain a lib-source hit"
    );
}

#[test]
fn cfg_test_format_geometry_import_is_excluded_from_lib_source() {
    let gated = r#"
fn production() {}

#[cfg(test)]
use crate::features::formatting::{FormatPosition, FormatRange};

#[cfg(test)]
mod tests {
    use super::*;
}
"#;
    assert!(
        !lib_source_names_format_geometry(gated),
        "#[cfg(test)] use at module scope must not count as --lib source"
    );
}

#[test]
fn cfg_test_item_does_not_hide_later_ungated_import() {
    let later_production = r#"
fn production_before() {}

#[cfg(test)]
mod tests {
    fn uses_them() {}
}

use crate::features::formatting::{FormatPosition, FormatRange};

fn production_after() {}
"#;
    assert!(
        lib_source_names_format_geometry(later_production),
        "ungated imports after a #[cfg(test)] item must remain --lib source"
    );
}

#[test]
fn cfg_test_marker_in_comment_or_string_does_not_truncate_lib_source() {
    let comment = r#"
fn production_before() {}
// #[cfg(test)]
use crate::features::formatting::{FormatPosition, FormatRange};
"#;
    assert!(
        lib_source_names_format_geometry(comment),
        "a comment containing #[cfg(test)] must not drop later ungated imports"
    );

    let string = r##"
fn production_before() {}
const MARKER: &str = "#[cfg(test)]";
use crate::features::formatting::{FormatPosition, FormatRange};
"##;
    assert!(
        lib_source_names_format_geometry(string),
        "a string containing #[cfg(test)] must not drop later ungated imports"
    );
}

#[test]
fn cfg_test_open_brace_char_literal_does_not_swallow_later_import() {
    let source = r#"
fn production_before() {}

#[cfg(test)]
mod tests {
    fn skip_item() {
        match ch {
            '{' => {}
        }
    }
}

use crate::features::formatting::{FormatPosition, FormatRange};
"#;
    assert!(
        lib_source_names_format_geometry(source),
        "'{{' inside a #[cfg(test)] item must not swallow a later ungated import"
    );
}

#[test]
fn occupancy_requires_the_edit_helper_not_scanner_literals() {
    let scanner_only = r#"
fn production() {}

#[cfg(test)]
mod tests {
    fn lib_source_names_format_geometry(source: &str) -> bool {
        source.contains("FormatPosition") || source.contains("FormatRange")
    }
}
"#;
    assert!(
        !edit_helper_constructs_format_geometry(scanner_only),
        "scanner/fixture literals must not satisfy occupancy without fn edit"
    );
}

#[test]
fn formatting_policy_lib_source_does_not_name_format_geometry() {
    let files = [
        ("mod.rs", include_str!("mod.rs")),
        ("multi_range.rs", include_str!("multi_range.rs")),
        ("handlers.rs", include_str!("handlers.rs")),
        ("receipt.rs", include_str!("receipt.rs")),
    ];
    for (name, source) in files {
        assert!(
            !lib_source_names_format_geometry(source),
            "{name} --lib source must not name FormatPosition/FormatRange (#9618)"
        );
    }
    assert!(
        edit_helper_constructs_format_geometry(include_str!("multi_range.rs")),
        "fn edit must still construct FormatPosition/FormatRange; scanner literals do not count"
    );
}
