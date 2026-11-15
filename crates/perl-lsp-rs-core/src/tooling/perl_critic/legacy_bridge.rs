//! Temporary same-crate adapters for the native critic extraction (#7410).
//!
//! The native declarations no longer own legacy-output or LSP projection.
//! Public inherent methods remain available only while their types are defined
//! in this crate. At the actual package cut, disposition the legacy methods and
//! replace the LSP method with an adapter free function; these impl blocks
//! cannot follow foreign types into the extracted native package.
//!
//! This module adds no evaluator or runtime engine. The registry bridge still
//! calls the existing filtered registry exactly once. Ordinary product paths
//! continue to use NativeCriticService, not this compatibility surface.

use super::{CriticContext, CriticFinding, NativeCriticRegistry, Severity, Violation};

impl Severity {
    /// Converts this severity to a `DiagnosticSeverity` for LSP reporting.
    ///
    /// Perl::Critic severity 5 ([`Severity::Gentle`], the most severe bucket)
    /// maps to [`lsp_types::DiagnosticSeverity::ERROR`]; severity 1
    /// ([`Severity::Brutal`], the least severe bucket) maps to
    /// [`lsp_types::DiagnosticSeverity::HINT`]. See the type-level docs for
    /// why the variant names run opposite to the numbers.
    ///
    /// This is the single source of truth for the perlcritic-to-LSP severity
    /// mapping. Call it rather than re-deriving the `match` at a call site.
    #[cfg(feature = "lsp-compat")]
    pub fn to_diagnostic_severity(self) -> lsp_types::DiagnosticSeverity {
        match self {
            Self::Gentle => lsp_types::DiagnosticSeverity::ERROR,
            Self::Stern | Self::Harsh => lsp_types::DiagnosticSeverity::WARNING,
            Self::Cruel => lsp_types::DiagnosticSeverity::INFORMATION,
            Self::Brutal => lsp_types::DiagnosticSeverity::HINT,
        }
    }

    /// Converts this severity to a numeric severity level (for non-LSP contexts).
    #[cfg(not(feature = "lsp-compat"))]
    pub fn to_severity_level(self) -> u8 {
        self as u8
    }
}

impl CriticFinding {
    /// Convert this native finding into the existing violation shape.
    ///
    /// This is the bridge that lets native rules flow through current LSP,
    /// execute-command, and diagnostic consumers while those consumers still
    /// expect `Violation` values.
    #[must_use]
    pub fn to_violation(&self, file: impl Into<String>) -> Violation {
        Violation {
            policy: self.rule_id.clone(),
            description: self.message.clone(),
            explanation: self.explanation.clone(),
            severity: self.severity,
            range: self.range,
            file: file.into(),
        }
    }
}

impl NativeCriticRegistry {
    /// Run all registered rules and return current legacy violation values.
    ///
    /// This keeps native rule execution single-sourced while callers migrate
    /// from `Violation` consumers to richer native finding/code-action data.
    #[must_use]
    pub fn check_violations(
        &self,
        ctx: &CriticContext<'_>,
        file: impl Into<String>,
    ) -> Vec<Violation> {
        let file = file.into();
        self.check(ctx).into_iter().map(|finding| finding.to_violation(file.clone())).collect()
    }
}

#[cfg(test)]
#[cfg(feature = "lsp-compat")]
mod severity_direction_tests {
    use super::*;

    /// Perl::Critic scores violations 1 (least severe) to 5 (most severe):
    /// <https://metacpan.org/pod/Perl::Critic::Violation> -- "an integer
    /// ranging from 1 to 5, where 5 is the 'most' severe".
    ///
    /// The `Gentle`/`Brutal` variant names are `perlcritic` *threshold* names
    /// and run in the opposite direction, which makes the mapping look
    /// inverted at a glance. This test pins the numeric direction so that a
    /// well-meaning "fix" of the apparent inversion fails loudly here rather
    /// than silently shipping backwards diagnostics to users.
    #[test]
    fn numeric_severity_five_is_most_severe_and_maps_to_error() {
        assert_eq!(
            Severity::from_number(5).to_diagnostic_severity(),
            lsp_types::DiagnosticSeverity::ERROR,
            "perlcritic severity 5 is the MOST severe and must surface as an LSP Error"
        );
        assert_eq!(
            Severity::from_number(1).to_diagnostic_severity(),
            lsp_types::DiagnosticSeverity::HINT,
            "perlcritic severity 1 is the LEAST severe and must surface as an LSP Hint"
        );
    }

    #[test]
    fn severity_mapping_is_monotonic_in_the_numeric_score() {
        // Higher perlcritic number == worse code == more prominent LSP
        // severity. `DiagnosticSeverity` numbers run the other way (ERROR is
        // 1), so descending perlcritic scores must be non-decreasing here.
        let ordered: Vec<lsp_types::DiagnosticSeverity> =
            (1..=5).rev().map(|n| Severity::from_number(n).to_diagnostic_severity()).collect();

        assert_eq!(
            ordered,
            vec![
                lsp_types::DiagnosticSeverity::ERROR,       // 5
                lsp_types::DiagnosticSeverity::WARNING,     // 4
                lsp_types::DiagnosticSeverity::WARNING,     // 3
                lsp_types::DiagnosticSeverity::INFORMATION, // 2
                lsp_types::DiagnosticSeverity::HINT,        // 1
            ],
            "perlcritic 5..=1 must map to a non-increasing severity ramp"
        );
    }

    #[test]
    fn threshold_variant_names_carry_their_documented_numbers() {
        // `--gentle` is severity 5, `--brutal` is severity 1.
        assert_eq!(Severity::Gentle as u8, 5);
        assert_eq!(Severity::Stern as u8, 4);
        assert_eq!(Severity::Harsh as u8, 3);
        assert_eq!(Severity::Cruel as u8, 2);
        assert_eq!(Severity::Brutal as u8, 1);
    }
}

#[cfg(test)]
mod compatibility_tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::tooling::perl_critic::{
        CriticCategory, CriticConfig, CriticFindingShape, CriticFix, CriticRelatedInformation,
        CriticRule, CriticTextEdit, FixSafety,
    };
    use perl_parser_core::Parser;
    use perl_parser_core::position::{Position, Range};

    fn finding(id: &str, severity: Severity) -> CriticFinding {
        let range = Range {
            start: Position { byte: 0, line: 0, column: 0 },
            end: Position { byte: 0, line: 0, column: 0 },
        };
        CriticFinding {
            rule_id: id.to_string(),
            category: CriticCategory::Syntax,
            severity,
            range,
            message: "native message".to_string(),
            explanation: "native explanation".to_string(),
            suppression_key: id.to_string(),
            observed_shape: CriticFindingShape::General,
            related: vec![CriticRelatedInformation {
                range,
                message: "related evidence".to_string(),
            }],
            fix: Some(CriticFix {
                title: "suggested change".to_string(),
                safety: FixSafety::Suggested,
                edits: vec![CriticTextEdit { range, new_text: "replacement".to_string() }],
            }),
        }
    }

    #[test]
    fn legacy_projection_preserves_fields_without_consuming_native_metadata() {
        let mut native = finding("native.testing.require_use_strict", Severity::Harsh);
        native.range = Range {
            start: Position { byte: 4, line: 1, column: 2 },
            end: Position { byte: 9, line: 1, column: 7 },
        };
        let original = native.clone();
        let legacy = native.to_violation("lib/Example.pm");
        assert_eq!(legacy.policy, original.rule_id);
        assert_eq!(legacy.description, original.message);
        assert_eq!(legacy.explanation, original.explanation);
        assert_eq!(legacy.severity, original.severity);
        assert_eq!(legacy.range, original.range);
        assert_eq!(legacy.file, "lib/Example.pm");
        assert_eq!(native, original, "legacy projection must not consume richer native evidence");
        assert_eq!(native.related.len(), 1);
        assert!(native.fix.is_some());
    }

    struct ProbeRule {
        id: &'static str,
        severity: Severity,
        calls: Arc<AtomicUsize>,
    }

    impl CriticRule for ProbeRule {
        fn id(&self) -> &'static str {
            self.id
        }

        fn category(&self) -> CriticCategory {
            CriticCategory::Syntax
        }

        fn default_severity(&self) -> Severity {
            self.severity
        }

        fn check(&self, _ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            out.push(finding(self.id, self.severity));
        }
    }

    #[test]
    fn legacy_registry_bridge_keeps_filtering_order_and_one_evaluation()
    -> Result<(), Box<dyn std::error::Error>> {
        const WARNINGS: &str = "native.testing.require_use_warnings";
        const ASSIGNMENT: &str = "native.common.assignment_in_condition";
        const STRICT: &str = "native.testing.require_use_strict";
        let suppressed = "## no critic native.testing.require_use_strict\n1;\n";
        let cases = [
            ("1;\n", 1, vec![], vec![], vec![WARNINGS, ASSIGNMENT, STRICT], [1, 1, 1]),
            ("1;\n", 4, vec![], vec![], vec![ASSIGNMENT], [1, 1, 1]),
            ("1;\n", 1, vec![STRICT], vec![], vec![STRICT], [0, 0, 1]),
            ("1;\n", 1, vec![], vec![ASSIGNMENT], vec![WARNINGS, STRICT], [1, 0, 1]),
            (suppressed, 1, vec![], vec![], vec![WARNINGS, ASSIGNMENT], [1, 1, 1]),
            ("1;\n", 5, vec![], vec![], vec![], [1, 1, 1]),
            ("1;\n", 1, vec![STRICT], vec![STRICT], vec![], [0, 0, 0]),
        ];
        for (source, severity, include, exclude, expected_ids, expected_calls) in cases {
            let config = CriticConfig {
                severity,
                include: include.into_iter().map(str::to_string).collect(),
                exclude: exclude.into_iter().map(str::to_string).collect(),
                ..CriticConfig::default()
            };
            let ast = Parser::new(source).parse()?;
            let context = CriticContext::new(source, &ast, &config);
            let calls: Vec<_> = (0..3).map(|_| Arc::new(AtomicUsize::new(0))).collect();
            let rules: Vec<Box<dyn CriticRule>> = [
                (WARNINGS, Severity::Harsh),
                (ASSIGNMENT, Severity::Stern),
                (STRICT, Severity::Harsh),
            ]
            .into_iter()
            .zip(&calls)
            .map(|((id, severity), count)| {
                Box::new(ProbeRule { id, severity, calls: Arc::clone(count) }) as Box<dyn CriticRule>
            })
            .collect();
            let registry = NativeCriticRegistry::with_rules(rules);
            let native = registry.check(&context);
            assert_eq!(
                native.iter().map(|row| row.rule_id.as_str()).collect::<Vec<_>>(),
                expected_ids,
            );
            for (count, expected) in calls.iter().zip(expected_calls) {
                assert_eq!(count.load(Ordering::SeqCst), expected);
                count.store(0, Ordering::SeqCst);
            }
            let legacy = registry.check_violations(&context, "lib/Example.pm");
            assert_eq!(
                legacy.iter().map(|row| row.policy.as_str()).collect::<Vec<_>>(),
                expected_ids,
            );
            for (count, expected) in calls.iter().zip(expected_calls) {
                assert_eq!(
                    count.load(Ordering::SeqCst),
                    expected,
                    "the bridge must evaluate each admitted rule once, not replay evaluation",
                );
            }
            let expected: Vec<_> =
                native.iter().map(|row| row.to_violation("lib/Example.pm")).collect();
            assert_eq!(serde_json::to_value(&legacy)?, serde_json::to_value(&expected)?);
        }
        Ok(())
    }

    #[cfg(not(feature = "lsp-compat"))]
    #[test]
    fn non_lsp_numeric_projection_keeps_all_five_scores() {
        for number in 1..=5 {
            assert_eq!(Severity::from_number(number).to_severity_level(), number);
        }
    }
}
