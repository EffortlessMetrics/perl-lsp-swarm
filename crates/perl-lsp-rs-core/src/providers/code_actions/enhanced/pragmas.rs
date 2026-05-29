//! Recommended pragma code actions for enhanced refactoring diagnostics.

use std::sync::LazyLock;

use regex::Regex;

use super::super::types::{CodeAction, CodeActionEdit, CodeActionKind};
use super::helpers::Helpers;
use crate::providers::rename::TextEdit;
use perl_parser_core::ast::SourceLocation;

static UTF8_PRAGMA_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*use\s+utf8\b").ok());
static OPEN_UTF8_PRAGMA_RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"(?mi)^\s*use\s+open\b[^\n;]*:(?:utf8|encoding\s*\(\s*utf-?8\s*\))").ok()
});

/// Build code actions for useful pragmas missing from a Perl source file.
pub(super) fn add_recommended_pragmas(source: &str, helpers: &Helpers<'_>) -> Vec<CodeAction> {
    let mut actions = Vec::new();

    // Check for missing strict and warnings
    let has_strict = source.contains("use strict");
    let has_warnings = source.contains("use warnings");

    if !has_strict || !has_warnings {
        let mut pragmas = Vec::new();
        if !has_strict {
            pragmas.push("use strict;");
        }
        if !has_warnings {
            pragmas.push("use warnings;");
        }

        let insert_pos = helpers.find_pragma_insert_position();

        actions.push(CodeAction {
            title: format!("Add missing pragmas ({})", pragmas.join(", ")),
            kind: CodeActionKind::QuickFix,
            diagnostics: Vec::new(),
            edit: CodeActionEdit {
                changes: vec![TextEdit {
                    location: SourceLocation { start: insert_pos, end: insert_pos },
                    new_text: format!("{}\n", pragmas.join("\n")),
                }],
            },
            is_preferred: true,
        });
    }

    // Add UTF-8 pragmas if missing
    let has_utf8 = UTF8_PRAGMA_RE.as_ref().is_some_and(|re| re.is_match(source));
    let has_open_utf8 = OPEN_UTF8_PRAGMA_RE.as_ref().is_some_and(|re| re.is_match(source));
    if helpers.has_non_ascii_content() && (!has_utf8 || !has_open_utf8) {
        let insert_pos = helpers.find_pragma_insert_position();
        let mut missing_pragmas = Vec::new();
        if !has_utf8 {
            missing_pragmas.push("use utf8;");
        }
        if !has_open_utf8 {
            missing_pragmas.push("use open qw(:std :utf8);");
        }

        actions.push(CodeAction {
            title: "Add UTF-8 support".to_string(),
            kind: CodeActionKind::QuickFix,
            diagnostics: Vec::new(),
            edit: CodeActionEdit {
                changes: vec![TextEdit {
                    location: SourceLocation { start: insert_pos, end: insert_pos },
                    new_text: format!("{}\n", missing_pragmas.join("\n")),
                }],
            },
            is_preferred: false,
        });
    }

    actions
}
