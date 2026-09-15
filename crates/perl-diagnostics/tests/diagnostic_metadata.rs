//! Registry-wide coverage for the diagnostic code registry and LSP metadata bridge.
//!
//! These tests consume [`DiagnosticCode::ALL`], so adding a registered code
//! automatically extends parse, URL, hint, tag, and serialization coverage.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use std::collections::BTreeSet;

use perl_diagnostics::catalog::diagnostic_meta;
use perl_diagnostics::codes::{DiagnosticCode, DiagnosticTag};

fn known_code_string(code: &str) -> bool {
    DiagnosticCode::parse_code(code).is_some()
}

#[test]
fn diagnostic_code_registry_is_complete_unique_and_ordered()
-> Result<(), Box<dyn std::error::Error>> {
    let code_strings = DiagnosticCode::ALL.iter().map(DiagnosticCode::as_str).collect::<Vec<_>>();
    let unique_codes = code_strings.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(unique_codes.len(), code_strings.len());
    for pair in code_strings.windows(2) {
        let previous = pair[0].strip_prefix("PL").ok_or("missing PL prefix")?.parse::<u16>()?;
        let current = pair[1].strip_prefix("PL").ok_or("missing PL prefix")?.parse::<u16>()?;
        assert!(previous < current, "{} must sort before {}", pair[0], pair[1]);
    }

    // Recurrence identities: these codes were previously omitted from copied
    // inventories (#3014, #5035, #9818). The macro guarantees membership; these
    // assertions preserve their assigned public identities.
    assert_eq!(DiagnosticCode::UnresolvedQualifiedCall.as_str(), "PL305");
    assert_eq!(DiagnosticCode::SecuritySqlInjection.as_str(), "PL607");
    assert_eq!(DiagnosticCode::SecuritySubstitutionEval.as_str(), "PL608");
    assert_eq!(DiagnosticCode::SecurityEmbeddedRegexCode.as_str(), "PL609");

    Ok(())
}

#[test]
fn diagnostic_code_registry_round_trips() -> Result<(), Box<dyn std::error::Error>> {
    for code in DiagnosticCode::ALL {
        let code_string = code.as_str();

        assert_eq!(DiagnosticCode::parse_code(code_string), Some(*code));
        assert_eq!(code_string.parse::<DiagnosticCode>()?, *code);
        assert_eq!(code_string.len(), 5);
        assert!(code_string.starts_with("PL"));

        let meta = diagnostic_meta(*code);
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
    for code in DiagnosticCode::ALL {
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
            assert!(code_string.parse::<DiagnosticCode>().is_err());
        }
    }

    assert!("ParseError".parse::<DiagnosticCode>().is_err());
    assert!("pl001".parse::<DiagnosticCode>().is_err());
    assert!(" PL001".parse::<DiagnosticCode>().is_err());
    assert!("PL001 ".parse::<DiagnosticCode>().is_err());

    Ok(())
}

#[cfg(feature = "serde")]
#[test]
fn serde_uses_stable_public_code_identity() -> Result<(), Box<dyn std::error::Error>> {
    for code in DiagnosticCode::ALL {
        let serialized = serde_json::to_string(code)?;
        assert_eq!(serialized, format!("\"{}\"", code.as_str()));

        let deserialized: DiagnosticCode = serde_json::from_str(&serialized)?;
        assert_eq!(deserialized, *code);
    }

    let rust_variant_name = serde_json::to_string("ParseError")?;
    assert!(serde_json::from_str::<DiagnosticCode>(&rust_variant_name).is_err());
    assert!(serde_json::from_str::<DiagnosticCode>("\"PL999\"").is_err());
    assert!(serde_json::from_str::<DiagnosticCode>("1").is_err());
    assert!(serde_json::from_str::<DiagnosticCode>("null").is_err());

    Ok(())
}
