//! Fail-closed Rust compiler-diagnostic receipts for repository hygiene (#9365).
//!
//! The parser consumes Cargo JSON messages. It never counts lint names in
//! rendered stderr, and malformed or incomplete output cannot become a clean
//! zero-finding result.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const RUST_HYGIENE_RECEIPT_SCHEMA_VERSION: &str = "rust-hygiene.v1";

const SELECTED_LINTS: [&str; 3] = ["dead_code", "unused_imports", "unused_variables"];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RustHygieneResultClassV1 {
    Success,
    PolicyFinding,
    NotProven,
    NotApplicable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RustHygieneCommandV1 {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RustHygieneToolchainV1 {
    pub cargo: String,
    pub rustc: String,
    pub clippy: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RustHygieneTargetV1 {
    pub package_id: String,
    pub name: String,
    pub kinds: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RustHygieneSpanV1 {
    pub file_name: String,
    pub line_start: u32,
    pub line_end: u32,
    pub column_start: u32,
    pub column_end: u32,
    pub byte_start: u64,
    pub byte_end: u64,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RustHygieneNativeDiagnosticV1 {
    pub code: Option<String>,
    pub level: String,
    pub message: String,
    pub target: RustHygieneTargetV1,
    pub primary_span: Option<RustHygieneSpanV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RustHygieneFindingV1 {
    pub lint_code: String,
    pub level: String,
    pub message: String,
    pub target: RustHygieneTargetV1,
    pub primary_span: Option<RustHygieneSpanV1>,
    pub instrument: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RustHygieneParseErrorV1 {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for RustHygieneParseErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "line {}: {}", self.line, self.message)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RustHygieneParsedStreamV1 {
    pub cargo_message_count: usize,
    pub compiler_message_count: usize,
    pub build_finished: bool,
    pub build_success: Option<bool>,
    pub findings: Vec<RustHygieneFindingV1>,
    pub diagnostics: Vec<RustHygieneNativeDiagnosticV1>,
    pub parse_errors: Vec<RustHygieneParseErrorV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RustHygieneReceiptInputV1 {
    pub repository_head: String,
    pub command: RustHygieneCommandV1,
    pub toolchain: RustHygieneToolchainV1,
    pub native_stdout: String,
    pub native_stderr: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RustHygieneReceiptV1 {
    pub schema_version: String,
    pub repository_head: String,
    pub command: RustHygieneCommandV1,
    pub toolchain: RustHygieneToolchainV1,
    pub result_class: RustHygieneResultClassV1,
    pub analysis_complete: bool,
    pub findings: Vec<RustHygieneFindingV1>,
    pub diagnostics: Vec<RustHygieneNativeDiagnosticV1>,
    pub parse_errors: Vec<RustHygieneParseErrorV1>,
    pub native_stdout_sha256: String,
    pub native_stderr_sha256: String,
    pub limitations: Vec<String>,
    pub claim_boundary: String,
}

pub fn parse_cargo_json_stream(input: &str) -> RustHygieneParsedStreamV1 {
    let mut parsed = RustHygieneParsedStreamV1::default();

    for (offset, line) in input.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let envelope = match serde_json::from_str::<CargoMessageEnvelope>(line) {
            Ok(envelope) => envelope,
            Err(error) => {
                parsed.parse_errors.push(RustHygieneParseErrorV1 {
                    line: offset + 1,
                    message: error.to_string(),
                });
                continue;
            }
        };

        parsed.cargo_message_count += 1;
        match envelope.reason.as_str() {
            "compiler-message" => {
                parsed.compiler_message_count += 1;
                if let Some(message) = envelope.message {
                    let target = normalize_target(envelope.package_id, envelope.target);
                    let primary_span = message
                        .spans
                        .iter()
                        .find(|span| span.is_primary)
                        .or_else(|| message.spans.first())
                        .map(normalize_span);
                    let code = message.code.map(|code| code.code);
                    let diagnostic = RustHygieneNativeDiagnosticV1 {
                        code: code.clone(),
                        level: message.level.clone(),
                        message: message.message.clone(),
                        target: target.clone(),
                        primary_span: primary_span.clone(),
                    };
                    if code.as_deref().is_some_and(is_selected_lint) {
                        parsed.findings.push(RustHygieneFindingV1 {
                            lint_code: code.clone().unwrap_or_default(),
                            level: message.level,
                            message: message.message,
                            target,
                            primary_span,
                            instrument: "clippy".into(),
                        });
                    }
                    parsed.diagnostics.push(diagnostic);
                } else {
                    parsed.parse_errors.push(RustHygieneParseErrorV1 {
                        line: offset + 1,
                        message: "compiler-message is missing its diagnostic payload".into(),
                    });
                }
            }
            "build-finished" => {
                parsed.build_finished = true;
                parsed.build_success = envelope.success;
                if envelope.success.is_none() {
                    parsed.parse_errors.push(RustHygieneParseErrorV1 {
                        line: offset + 1,
                        message: "build-finished is missing success".into(),
                    });
                }
            }
            _ => {}
        }
    }

    parsed.findings.sort_by(finding_sort_key);
    parsed.diagnostics.sort_by(diagnostic_sort_key);
    parsed
}

pub fn build_rust_hygiene_receipt(input: RustHygieneReceiptInputV1) -> RustHygieneReceiptV1 {
    let parsed = parse_cargo_json_stream(&input.native_stdout);
    let mut limitations = Vec::new();

    if parsed.cargo_message_count == 0 {
        limitations.push("missing_cargo_json_stream".into());
    }
    if !parsed.build_finished {
        limitations.push("missing_build_finished".into());
    }
    if parsed.build_success == Some(false) {
        limitations.push("cargo_build_failed".into());
    }
    if input.command.exit_code != Some(0) {
        limitations.push("instrument_exit_nonzero_or_unavailable".into());
    }
    if !parsed.parse_errors.is_empty() {
        limitations.push("malformed_or_unrecognized_cargo_json".into());
    }

    limitations.sort();
    limitations.dedup();

    let analysis_complete = limitations.is_empty() && parsed.build_success == Some(true);
    let result_class = if !analysis_complete {
        RustHygieneResultClassV1::NotProven
    } else if parsed.findings.is_empty() {
        RustHygieneResultClassV1::Success
    } else {
        RustHygieneResultClassV1::PolicyFinding
    };

    RustHygieneReceiptV1 {
        schema_version: RUST_HYGIENE_RECEIPT_SCHEMA_VERSION.into(),
        repository_head: input.repository_head,
        command: input.command,
        toolchain: input.toolchain,
        result_class,
        analysis_complete,
        findings: parsed.findings,
        diagnostics: parsed.diagnostics,
        parse_errors: parsed.parse_errors,
        native_stdout_sha256: sha256_hex(input.native_stdout.as_bytes()),
        native_stderr_sha256: sha256_hex(input.native_stderr.as_bytes()),
        limitations,
        claim_boundary: "structured rustc/Clippy item diagnostics for the exact selected lib/bin profile; no semantic liveness, deletion safety, dependency-unused, Cargo-Hawk, or Perl reachability claim".into(),
    }
}

fn is_selected_lint(code: &str) -> bool {
    SELECTED_LINTS.contains(&code)
}

fn normalize_target(package_id: String, target: Option<CargoTarget>) -> RustHygieneTargetV1 {
    match target {
        Some(target) => RustHygieneTargetV1 {
            package_id,
            name: target.name,
            kinds: target.kind,
        },
        None => RustHygieneTargetV1 { package_id, name: String::new(), kinds: Vec::new() },
    }
}

fn normalize_span(span: &CargoSpan) -> RustHygieneSpanV1 {
    RustHygieneSpanV1 {
        file_name: span.file_name.clone(),
        line_start: span.line_start,
        line_end: span.line_end,
        column_start: span.column_start,
        column_end: span.column_end,
        byte_start: span.byte_start,
        byte_end: span.byte_end,
        label: span.label.clone(),
    }
}

fn finding_sort_key(
    left: &RustHygieneFindingV1,
    right: &RustHygieneFindingV1,
) -> std::cmp::Ordering {
    finding_key(left).cmp(&finding_key(right))
}

fn finding_key(finding: &RustHygieneFindingV1) -> (&str, &str, u32, u32, &str) {
    let (path, line, column) = finding
        .primary_span
        .as_ref()
        .map(|span| (span.file_name.as_str(), span.line_start, span.column_start))
        .unwrap_or(("", 0, 0));
    (
        finding.target.package_id.as_str(),
        path,
        line,
        column,
        finding.lint_code.as_str(),
    )
}

fn diagnostic_sort_key(
    left: &RustHygieneNativeDiagnosticV1,
    right: &RustHygieneNativeDiagnosticV1,
) -> std::cmp::Ordering {
    diagnostic_key(left).cmp(&diagnostic_key(right))
}

fn diagnostic_key(diagnostic: &RustHygieneNativeDiagnosticV1) -> (&str, &str, u32, u32, &str) {
    let (path, line, column) = diagnostic
        .primary_span
        .as_ref()
        .map(|span| (span.file_name.as_str(), span.line_start, span.column_start))
        .unwrap_or(("", 0, 0));
    (
        diagnostic.target.package_id.as_str(),
        path,
        line,
        column,
        diagnostic.code.as_deref().unwrap_or(""),
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Deserialize)]
struct CargoMessageEnvelope {
    reason: String,
    #[serde(default)]
    package_id: String,
    #[serde(default)]
    target: Option<CargoTarget>,
    #[serde(default)]
    message: Option<CargoDiagnostic>,
    #[serde(default)]
    success: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CargoTarget {
    name: String,
    #[serde(default)]
    kind: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CargoDiagnostic {
    #[serde(default)]
    code: Option<CargoDiagnosticCode>,
    level: String,
    message: String,
    #[serde(default)]
    spans: Vec<CargoSpan>,
}

#[derive(Debug, Deserialize)]
struct CargoDiagnosticCode {
    code: String,
}

#[derive(Debug, Deserialize)]
struct CargoSpan {
    file_name: String,
    byte_start: u64,
    byte_end: u64,
    line_start: u32,
    line_end: u32,
    column_start: u32,
    column_end: u32,
    #[serde(default)]
    is_primary: bool,
    #[serde(default)]
    label: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn compiler_message(code: &str) -> String {
        json!({
            "reason": "compiler-message",
            "package_id": "path+file:///repo#demo@0.1.0",
            "target": {"name": "demo", "kind": ["lib"]},
            "message": {
                "code": {"code": code},
                "level": "warning",
                "message": format!("{code} diagnostic"),
                "spans": [{
                    "file_name": "src/lib.rs",
                    "byte_start": 10,
                    "byte_end": 20,
                    "line_start": 2,
                    "line_end": 2,
                    "column_start": 5,
                    "column_end": 15,
                    "is_primary": true,
                    "label": "primary"
                }]
            }
        })
        .to_string()
    }

    fn build_finished(success: bool) -> String {
        json!({"reason": "build-finished", "success": success}).to_string()
    }

    fn input(stdout: String, exit_code: Option<i32>) -> RustHygieneReceiptInputV1 {
        RustHygieneReceiptInputV1 {
            repository_head: "abc123".into(),
            command: RustHygieneCommandV1 {
                program: "cargo".into(),
                args: vec!["clippy".into(), "--message-format=json".into()],
                cwd: ".".into(),
                exit_code,
            },
            toolchain: RustHygieneToolchainV1 {
                cargo: "cargo 1.97.1".into(),
                rustc: "rustc 1.97.1".into(),
                clippy: "clippy 0.1.97".into(),
            },
            native_stdout: stdout,
            native_stderr: String::new(),
        }
    }

    #[test]
    fn clean_complete_stream_is_success() {
        let receipt = build_rust_hygiene_receipt(input(build_finished(true), Some(0)));
        assert_eq!(receipt.result_class, RustHygieneResultClassV1::Success);
        assert!(receipt.analysis_complete);
        assert!(receipt.findings.is_empty());
    }

    #[test]
    fn selected_lint_codes_become_item_findings() {
        for code in SELECTED_LINTS {
            let stdout = format!("{}\n{}", compiler_message(code), build_finished(true));
            let receipt = build_rust_hygiene_receipt(input(stdout, Some(0)));
            assert_eq!(receipt.result_class, RustHygieneResultClassV1::PolicyFinding);
            assert_eq!(receipt.findings.len(), 1);
            assert_eq!(receipt.findings[0].lint_code, code);
            assert_eq!(receipt.findings[0].target.name, "demo");
            assert_eq!(
                receipt.findings[0].primary_span.as_ref().map(|span| span.line_start),
                Some(2)
            );
        }
    }

    #[test]
    fn unknown_diagnostic_remains_visible_without_becoming_selected_finding() {
        let stdout = format!("{}\n{}", compiler_message("future_lint"), build_finished(true));
        let receipt = build_rust_hygiene_receipt(input(stdout, Some(0)));
        assert_eq!(receipt.result_class, RustHygieneResultClassV1::Success);
        assert!(receipt.findings.is_empty());
        assert_eq!(receipt.diagnostics.len(), 1);
        assert_eq!(receipt.diagnostics[0].code.as_deref(), Some("future_lint"));
    }

    #[test]
    fn malformed_output_is_not_proven_and_preserves_prior_findings() {
        let stdout = format!(
            "{}\nnot-json\n{}",
            compiler_message("dead_code"),
            build_finished(true)
        );
        let receipt = build_rust_hygiene_receipt(input(stdout, Some(0)));
        assert_eq!(receipt.result_class, RustHygieneResultClassV1::NotProven);
        assert_eq!(receipt.findings.len(), 1);
        assert_eq!(receipt.parse_errors.len(), 1);
        assert!(
            receipt
                .limitations
                .iter()
                .any(|value| value == "malformed_or_unrecognized_cargo_json")
        );
    }

    #[test]
    fn rendered_dead_code_text_cannot_create_a_finding() {
        let receipt = build_rust_hygiene_receipt(input(
            "warning: dead_code appears in rendered stderr-like text".into(),
            Some(0),
        ));
        assert_eq!(receipt.result_class, RustHygieneResultClassV1::NotProven);
        assert!(receipt.findings.is_empty());
        assert!(!receipt.parse_errors.is_empty());
    }

    #[test]
    fn missing_build_finished_is_not_proven() {
        let receipt = build_rust_hygiene_receipt(input(compiler_message("dead_code"), Some(0)));
        assert_eq!(receipt.result_class, RustHygieneResultClassV1::NotProven);
        assert_eq!(receipt.findings.len(), 1);
        assert!(
            receipt
                .limitations
                .iter()
                .any(|value| value == "missing_build_finished")
        );
    }

    #[test]
    fn nonzero_exit_is_not_proven_even_with_parseable_findings() {
        let stdout = format!("{}\n{}", compiler_message("dead_code"), build_finished(false));
        let receipt = build_rust_hygiene_receipt(input(stdout, Some(1)));
        assert_eq!(receipt.result_class, RustHygieneResultClassV1::NotProven);
        assert_eq!(receipt.findings.len(), 1);
        assert!(
            receipt
                .limitations
                .iter()
                .any(|value| value == "cargo_build_failed")
        );
    }

    #[test]
    fn native_output_digests_are_stable_and_distinct() {
        let first = build_rust_hygiene_receipt(input(build_finished(true), Some(0)));
        let mut changed = input(build_finished(true), Some(0));
        changed.native_stderr = "diagnostic context".into();
        let second = build_rust_hygiene_receipt(changed);
        assert_eq!(first.native_stdout_sha256, second.native_stdout_sha256);
        assert_ne!(first.native_stderr_sha256, second.native_stderr_sha256);
    }
}
