//! Discriminating `--lib` vs `--tests` occupancy for formatting geometry types (#9618).
//!
//! Lives outside `multi_range.rs` so scanner literals and `'{'` char helpers cannot
//! satisfy or confuse the ratchet they enforce. Trivia walking is shared with
//! crate-wide all-target Clippy occupancy (#9600).

use crate::source_scan::{lib_source, skip_balanced};

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
