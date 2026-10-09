//! Shared native↔built-in overlap-collapse policy (#5088, #11918, #17241).
//!
//! The native perlcritic engine and the built-in lints intentionally emit the
//! same fact under two identity spaces; both transports therefore apply the
//! same collapse at equal `(range, severity)`, preferring built-in `PL*` codes:
//!
//! - push applies it to the internal diagnostic list before conversion
//!   (`runtime::diagnostics::dedup_overlapping_diagnostics`);
//! - pull applies the identical policy, through the shared predicates here, to
//!   the projected LSP rows, so a fact push collapsed is not double-reported
//!   by `textDocument/diagnostic` (#17241 F2).

use lsp_types::Diagnostic as LspDiagnostic;
use lsp_types::NumberOrString;

/// Returns `true` if the code string looks like a native-critic code (not a
/// `PL*` code).
pub(crate) fn is_native_critic_code(code: Option<&str>) -> bool {
    !code.is_some_and(|c| c.starts_with("PL"))
}

/// Whether one `(PL* code, native rule id)` pair is a reviewed alias whose
/// duplicate prevention moved upstream into the normalized critic seam
/// (#11918).
///
/// The table lists exactly the reviewed alias pairs of the migrated producer
/// cohort, in both orders: PL404 (literal shape) with the undef-comparison
/// alias; PL601 with the backtick alias and, for the qx shape, the
/// qx/readpipe alias; PL606 (readpipe shape) with the qx/readpipe alias; and
/// PL603/PL604 with the system/exec rule that owns both shapes. Every other
/// overlap pair keeps the transport-level coincidence dedup until its own
/// producers migrate, so unrelated rows never lose their existing collapse
/// behavior to this exemption.
pub(crate) fn is_upstream_merged_alias_pair(a_code: Option<&str>, b_code: Option<&str>) -> bool {
    let forward = matches!(
        (a_code, b_code),
        (Some("PL404"), Some("native.common.undef_comparison"))
            | (
                Some("PL601"),
                Some("native.security.backtick_exec" | "native.security.qx_readpipe")
            )
            | (Some("PL606"), Some("native.security.qx_readpipe"))
            | (Some("PL603" | "PL604"), Some("native.security.system_exec"))
    );
    let reverse = matches!(
        (b_code, a_code),
        (Some("PL404"), Some("native.common.undef_comparison"))
            | (
                Some("PL601"),
                Some("native.security.backtick_exec" | "native.security.qx_readpipe")
            )
            | (Some("PL606"), Some("native.security.qx_readpipe"))
            | (Some("PL603" | "PL604"), Some("native.security.system_exec"))
    );
    forward || reverse
}

/// Whether two diagnostics at the same `(range, severity)` collapse into one
/// row: exactly one of the two codes must be a native-critic code, and the
/// pair must not be a reviewed upstream-merged alias (#11918).
pub(crate) fn codes_collapse_pair(a_code: Option<&str>, b_code: Option<&str>) -> bool {
    (is_native_critic_code(a_code) ^ is_native_critic_code(b_code))
        && !is_upstream_merged_alias_pair(a_code, b_code)
}

/// Wire code of an LSP diagnostic, when it carries a string code.
fn lsp_code_str(code: Option<&NumberOrString>) -> Option<&str> {
    match code {
        Some(NumberOrString::String(code_str)) => Some(code_str),
        _ => None,
    }
}

/// Collapse native↔built-in overlap pairs in a projected LSP row list.
///
/// Mirrors the push path's internal-list collapse: stable sort so `PL*` rows
/// come first at equal `(range, severity)`, then drop the native twin of each
/// adjacent collapse pair. Two distinct `PL*` rows sharing a range and
/// severity (e.g. PL100 vs PL101 at the same insertion point) never collapse,
/// because the pair predicate requires exactly one native code.
pub(crate) fn collapse_overlapping_lsp_diagnostics(diagnostics: &mut Vec<LspDiagnostic>) {
    diagnostics.sort_by(|a, b| {
        let a_native = is_native_critic_code(lsp_code_str(a.code.as_ref()));
        let b_native = is_native_critic_code(lsp_code_str(b.code.as_ref()));
        (a.range.start, a.range.end, a.severity, a_native).cmp(&(
            b.range.start,
            b.range.end,
            b.severity,
            b_native,
        ))
    });
    diagnostics.dedup_by(|a, b| {
        a.range == b.range
            && a.severity == b.severity
            && codes_collapse_pair(lsp_code_str(a.code.as_ref()), lsp_code_str(b.code.as_ref()))
    });
}

#[cfg(test)]
mod tests {
    use super::{
        codes_collapse_pair, collapse_overlapping_lsp_diagnostics, is_upstream_merged_alias_pair,
    };
    use lsp_types::{
        Diagnostic as LspDiagnostic, DiagnosticSeverity, NumberOrString, Position, Range,
    };

    fn pl(code: &'static str) -> Option<&'static str> {
        Some(code)
    }

    fn row(range: Range, severity: DiagnosticSeverity, code: &str) -> LspDiagnostic {
        LspDiagnostic {
            range,
            severity: Some(severity),
            code: Some(NumberOrString::String(code.to_string())),
            code_description: None,
            source: Some("perl-lsp".to_string()),
            message: "message".to_string(),
            related_information: None,
            tags: None,
            data: None,
        }
    }

    fn codes(diagnostics: &[LspDiagnostic]) -> Vec<String> {
        diagnostics
            .iter()
            .map(|d| match d.code.as_ref() {
                Some(NumberOrString::String(code)) => code.clone(),
                _ => String::new(),
            })
            .collect()
    }

    #[test]
    fn upstream_merged_alias_exemption_covers_exactly_the_reviewed_pairs() {
        // #11918: the transport XOR retirement is keyed to the exact reviewed
        // alias pairs, not a cross-product of cohort codes, so unrelated
        // overlap pairs keep their pre-existing coincidence dedup.
        for (a, b) in [
            (pl("PL404"), pl("native.common.undef_comparison")),
            (pl("PL601"), pl("native.security.backtick_exec")),
            (pl("PL601"), pl("native.security.qx_readpipe")),
            (pl("PL606"), pl("native.security.qx_readpipe")),
            (pl("PL603"), pl("native.security.system_exec")),
            (pl("PL604"), pl("native.security.system_exec")),
        ] {
            assert!(
                is_upstream_merged_alias_pair(a, b) && is_upstream_merged_alias_pair(b, a),
                "reviewed alias pair {a:?}/{b:?} must be exempt in both orders"
            );
        }
        for (a, b) in [
            (pl("PL404"), pl("native.security.system_exec")),
            (pl("PL603"), pl("native.security.qx_readpipe")),
            (pl("PL606"), pl("native.security.backtick_exec")),
            (pl("PL100"), pl("native.security.system_exec")),
            (pl("PL404"), pl("PL603")),
            (pl("native.common.undef_comparison"), pl("native.security.system_exec")),
            (pl("PL603"), None),
        ] {
            assert!(
                !is_upstream_merged_alias_pair(a, b) && !is_upstream_merged_alias_pair(b, a),
                "unrelated pair {a:?}/{b:?} must keep the transport coincidence dedup"
            );
        }
    }

    #[test]
    fn pair_predicate_requires_exactly_one_native_code() {
        let range = || Range::new(Position::new(0, 0), Position::new(0, 5));
        let same_severity = DiagnosticSeverity::WARNING;
        let a = row(range(), same_severity, "PL100");
        let b = row(range(), same_severity, "native.testing.require_use_strict");
        assert!(codes_collapse_pair(lsp_row_code(&a), lsp_row_code(&b),));
        // Two distinct PL* rows at the same (range, severity) never collapse.
        let other_pl = row(range(), same_severity, "PL101");
        assert!(!codes_collapse_pair(lsp_row_code(&a), lsp_row_code(&other_pl),));
        // Severity mismatch never collapses (checked by the callers).
    }

    fn lsp_row_code(diagnostic: &LspDiagnostic) -> Option<&str> {
        match diagnostic.code.as_ref() {
            Some(NumberOrString::String(code)) => Some(code),
            _ => None,
        }
    }

    #[test]
    fn lsp_collapse_prefers_builtin_row_and_keeps_distinct_builtins() {
        let range = Range::new(Position::new(0, 0), Position::new(0, 1));
        let mut rows = vec![
            row(range, DiagnosticSeverity::WARNING, "native.testing.require_use_strict"),
            row(range, DiagnosticSeverity::WARNING, "PL101"),
            row(range, DiagnosticSeverity::WARNING, "native.testing.require_use_warnings"),
            row(range, DiagnosticSeverity::WARNING, "PL100"),
        ];
        collapse_overlapping_lsp_diagnostics(&mut rows);
        let mut surviving = codes(&rows);
        surviving.sort();
        assert_eq!(
            surviving,
            vec!["PL100".to_string(), "PL101".to_string()],
            "both built-in rows survive; both native twins collapse"
        );
    }

    #[test]
    fn lsp_collapse_keeps_severity_mismatched_twins() {
        // PL406 (Hint) and native.common.unreachable_code (Warning) carry the
        // same fact at different severities; the reviewed #5088 policy is
        // severity-exact, so both stay until a severity owning ruling lands.
        let range = Range::new(Position::new(40, 0), Position::new(40, 21));
        let mut rows = vec![
            row(range, DiagnosticSeverity::WARNING, "native.common.unreachable_code"),
            row(range, DiagnosticSeverity::HINT, "PL406"),
        ];
        collapse_overlapping_lsp_diagnostics(&mut rows);
        assert_eq!(codes(&rows).len(), 2);
    }

    #[test]
    fn lsp_collapse_keeps_distinct_ranges_and_uncollapsible_codes() {
        let range_a = Range::new(Position::new(1, 0), Position::new(1, 5));
        let range_b = Range::new(Position::new(2, 0), Position::new(2, 5));
        let mut rows = vec![
            row(range_a, DiagnosticSeverity::WARNING, "native.io.unchecked_open_close"),
            row(range_b, DiagnosticSeverity::WARNING, "PL401"),
        ];
        collapse_overlapping_lsp_diagnostics(&mut rows);
        assert_eq!(codes(&rows).len(), 2, "different ranges never collapse");
    }
}
