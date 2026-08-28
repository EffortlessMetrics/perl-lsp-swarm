//! Registry-wide coverage for the diagnostic code registry and LSP metadata bridge.
//!
//! These tests derive the registered-code denominator from `parse_code` across
//! every formatted `PL000`-`PL999` slot, so newly registered codes cannot evade
//! URL, hint, tag, and round-trip coverage through a stale hand-maintained list.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use perl_diagnostics::catalog::diagnostic_meta;
use perl_diagnostics::codes::{DiagnosticCode, DiagnosticTag};

fn registered_codes() -> impl Iterator<Item = DiagnosticCode> {
    (0_u16..1000)
        .filter_map(|number| DiagnosticCode::parse_code(&format!("PL{number:03}")))
}

fn known_code_string(code: &str) -> bool {
    DiagnosticCode::parse_code(code).is_some()
}

#[test]
fn diagnostic_code_registry_round_trips() -> Result<(), Box<dyn std::error::Error>> {
    for code in registered_codes() {
        let code_string = code.as_str();

        assert_eq!(DiagnosticCode::parse_code(code_string), Some(code));
        assert!(code_string.len() == 5);
        assert!(code_string.starts_with("PL"));

        let meta = diagnostic_meta(code);
        assert_eq!(meta.code, serde_json::json!(code_string));

        let expected_url = match code {
            DiagnosticCode::SecuritySqlInjection => {
                "https://owasp.org/www-community/attacks/SQL_Injection".to_string()
            }
            _ => format!("https://docs.perl-lsp.org/errors/{code_string}"),
        };
        assert_eq!(code.documentation_url(), Some(expected_url.as_str()));
        assert_eq!(meta.desc, Some(serde_json::json!({ "href": expected_url })));
        assert!(meta.hint.is_some());
    }

    Ok(())
}

#[test]
fn diagnostic_tags_are_lsp_safe_and_deterministic() -> Result<(), Box<dyn std::error::Error>> {
    for code in registered_codes() {
        let tags = code.tags();
        assert!(tags.len() <= 1);

        for tag in tags {
            assert!(matches!(tag.to_lsp_value(), 1 | 2));
        }

        assert_eq!(
            tags.contains(&DiagnosticTag::Deprecated),
            matches!(code, DiagnosticCode::DeprecatedDefined | DiagnosticCode::DeprecatedArrayBase)
        );
        assert_eq!(
            tags.contains(&DiagnosticTag::Unnecessary),
            matches!(
                code,
                DiagnosticCode::UnusedVariable
                    | DiagnosticCode::UnusedParameter
                    | DiagnosticCode::UnusedImport
                    | DiagnosticCode::UnreachableCode
            )
        );
    }

    Ok(())
}

#[test]
fn unknown_formatted_code_strings_do_not_parse() -> Result<(), Box<dyn std::error::Error>> {
    for prefix in ["PL", "PC", "PX"] {
        for number in 0_u16..1000 {
            let code_string = format!("{prefix}{number:03}");

            if known_code_string(&code_string) {
                continue;
            }

            assert_eq!(
                DiagnosticCode::parse_code(&code_string),
                None,
                "{code_string} should not parse as a known diagnostic code"
            );
        }
    }

    Ok(())
}
