//! Tests for native Perl::Critic rule behavior and registry integration.

use super::super::CriticConfig;
use super::*;
use perl_parser::Parser;
use perl_parser_core::position::{Position, Range};

struct DummyRule;

impl CriticRule for DummyRule {
    fn id(&self) -> &'static str {
        "native.test.dummy"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Syntax
    }

    fn default_severity(&self) -> Severity {
        Severity::Harsh
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        if ctx.source.contains("dummy") {
            out.push(CriticFinding {
                rule_id: self.id().to_string(),
                category: self.category(),
                severity: self.default_severity(),
                range: Range {
                    start: Position { byte: 0, line: 0, column: 0 },
                    end: Position { byte: 5, line: 0, column: 5 },
                },
                message: "dummy finding".to_string(),
                explanation: "dummy explanation".to_string(),
                suppression_key: self.id().to_string(),
                related: Vec::new(),
                fix: None,
            });
        }
    }
}

struct SecondDummyRule;

impl CriticRule for SecondDummyRule {
    fn id(&self) -> &'static str {
        "native.test.second"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Maintainability
    }

    fn default_severity(&self) -> Severity {
        Severity::Cruel
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        if ctx.source.contains("second") {
            out.push(CriticFinding {
                rule_id: self.id().to_string(),
                category: self.category(),
                severity: self.default_severity(),
                range: Range {
                    start: Position { byte: 6, line: 0, column: 6 },
                    end: Position { byte: 12, line: 0, column: 12 },
                },
                message: "second finding".to_string(),
                explanation: "second explanation".to_string(),
                suppression_key: self.id().to_string(),
                related: Vec::new(),
                fix: None,
            });
        }
    }
}

fn config_with_minimum_severity(severity: u8) -> CriticConfig {
    CriticConfig { severity, ..Default::default() }
}

fn parse_source(source: &str) -> Node {
    let mut parser = Parser::new(source);
    parser.parse().expect("test source should parse")
}

#[test]
fn native_critic_rule_contract_emits_stable_finding_shape() {
    let ast = empty_program_node();
    let config = CriticConfig::default();
    let ctx = CriticContext::new("dummy", &ast, &config);
    let mut findings = Vec::new();

    DummyRule.check(&ctx, &mut findings);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule_id, "native.test.dummy");
    assert_eq!(findings[0].suppression_key, "native.test.dummy");
    assert_eq!(findings[0].category, CriticCategory::Syntax);
    assert_eq!(findings[0].severity, Severity::Harsh);
}

#[test]
fn native_critic_finding_serializes_agent_friendly_fields() {
    let finding = CriticFinding {
        rule_id: "native.test.fixable".to_string(),
        category: CriticCategory::Style,
        severity: Severity::Cruel,
        range: Range {
            start: Position { byte: 0, line: 0, column: 0 },
            end: Position { byte: 1, line: 0, column: 1 },
        },
        message: "style issue".to_string(),
        explanation: "style explanation".to_string(),
        suppression_key: "native.test.fixable".to_string(),
        related: Vec::new(),
        fix: Some(CriticFix {
            title: "Apply style fix".to_string(),
            safety: FixSafety::Safe,
            edits: vec![CriticTextEdit {
                range: Range {
                    start: Position { byte: 0, line: 0, column: 0 },
                    end: Position { byte: 1, line: 0, column: 1 },
                },
                new_text: "x".to_string(),
            }],
        }),
    };

    let value = serde_json::to_value(&finding).expect("serialize native critic finding");

    assert_eq!(value["rule_id"], "native.test.fixable");
    assert_eq!(value["category"], "style");
    assert_eq!(value["fix"]["safety"], "safe");
    assert_eq!(value["fix"]["edits"][0]["new_text"], "x");
}

#[test]
fn native_critic_finding_converts_to_legacy_violation_shape() {
    let finding = CriticFinding {
        rule_id: "native.variables.unused_lexical".to_string(),
        category: CriticCategory::Semantic,
        severity: Severity::Stern,
        range: Range {
            start: Position { byte: 10, line: 1, column: 4 },
            end: Position { byte: 12, line: 1, column: 6 },
        },
        message: "unused lexical variable".to_string(),
        explanation: "remove or use the lexical variable".to_string(),
        suppression_key: "native.variables.unused_lexical".to_string(),
        related: Vec::new(),
        fix: None,
    };

    let violation = finding.to_violation("lib/App.pm");

    assert_eq!(violation.policy, "native.variables.unused_lexical");
    assert_eq!(violation.description, "unused lexical variable");
    assert_eq!(violation.explanation, "remove or use the lexical variable");
    assert_eq!(violation.severity, Severity::Stern);
    assert_eq!(violation.range, finding.range);
    assert_eq!(violation.file, "lib/App.pm");
}

#[test]
fn native_critic_registry_runs_rules_in_order() {
    let ast = empty_program_node();
    let config = config_with_minimum_severity(1);
    let ctx = CriticContext::new("dummy second", &ast, &config);
    let registry =
        NativeCriticRegistry::with_rules(vec![Box::new(DummyRule), Box::new(SecondDummyRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(registry.len(), 2);
    assert!(!registry.is_empty());
    assert_eq!(registry.rule_ids(), vec!["native.test.dummy", "native.test.second"]);
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].rule_id, "native.test.dummy");
    assert_eq!(findings[1].rule_id, "native.test.second");
}

#[test]
fn native_critic_registry_can_be_extended_incrementally() {
    let ast = empty_program_node();
    let config = config_with_minimum_severity(1);
    let ctx = CriticContext::new("second", &ast, &config);
    let mut registry = NativeCriticRegistry::new();

    assert!(registry.is_empty());
    registry.add_rule(Box::new(SecondDummyRule));

    let findings = registry.check(&ctx);

    assert_eq!(registry.rule_ids(), vec!["native.test.second"]);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].category, CriticCategory::Maintainability);
}

#[test]
fn native_critic_profiles_keep_recommended_lower_noise_than_strict() {
    let recommended = NativeCriticRegistry::for_profile(NativeCriticProfile::Recommended);
    let strict = NativeCriticRegistry::for_profile(NativeCriticProfile::Strict);
    let recommended_ids = recommended.rule_ids();
    let strict_ids = strict.rule_ids();

    assert!(recommended_ids.contains(&"native.testing.require_use_strict"));
    assert!(recommended_ids.contains(&"native.security.string_eval"));
    assert!(recommended_ids.contains(&"native.io.two_arg_open"));
    assert!(!recommended_ids.contains(&"native.variables.unused_lexical"));
    assert!(!recommended_ids.contains(&"native.syntax.unquoted_bareword"));
    assert!(recommended_ids.len() < strict_ids.len());
    assert_eq!(strict_ids.len(), NativeCriticRegistry::recommended().len());
}

#[test]
fn native_critic_profile_parser_accepts_stable_labels() {
    assert_eq!(
        NativeCriticProfile::parse("recommended").map(NativeCriticProfile::as_str),
        Some("recommended")
    );
    assert_eq!(
        NativeCriticProfile::parse("strict").map(NativeCriticProfile::as_str),
        Some("strict")
    );
    assert_eq!(NativeCriticProfile::parse("unknown"), None);
}

#[test]
fn native_require_use_strict_rule_emits_safe_fix_when_missing() {
    let ast = empty_program_node();
    let config = CriticConfig::default();
    let ctx = CriticContext::new("my $x = 1;\n", &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequireUseStrictRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.testing.require_use_strict");
    assert_eq!(finding.category, CriticCategory::Syntax);
    assert_eq!(finding.severity, Severity::Harsh);
    assert_eq!(finding.message, "Code does not use strict");
    assert_eq!(finding.suppression_key, "native.testing.require_use_strict");

    let fix = finding.fix.as_ref().expect("missing strict should have a safe fix");
    assert_eq!(fix.title, "Add 'use strict'");
    assert_eq!(fix.safety, FixSafety::Safe);
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(fix.edits[0].range, insertion_range());
    assert_eq!(fix.edits[0].new_text, "use strict;\n");
}

#[test]
fn native_require_use_strict_rule_accepts_exact_pragma_only() {
    let ast = empty_program_node();
    let config = CriticConfig::default();
    let exact_ctx = CriticContext::new("use strict;\nmy $x = 1;\n", &ast, &config);
    let similar_ctx = CriticContext::new("use strictures;\nmy $x = 1;\n", &ast, &config);
    let commented_ctx = CriticContext::new("# use strict;\nmy $x = 1;\n", &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequireUseStrictRule)]);

    assert!(registry.check(&exact_ctx).is_empty());
    assert_eq!(registry.check(&similar_ctx).len(), 1);
    assert_eq!(registry.check(&commented_ctx).len(), 1);
}

#[test]
fn native_require_use_warnings_rule_emits_safe_fix_when_missing() {
    let ast = empty_program_node();
    let config = CriticConfig::default();
    let ctx = CriticContext::new("use strict;\nmy $x = 1;\n", &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequireUseWarningsRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.testing.require_use_warnings");
    assert_eq!(finding.category, CriticCategory::Syntax);
    assert_eq!(finding.severity, Severity::Harsh);
    assert_eq!(finding.message, "Code does not use warnings");
    assert_eq!(finding.suppression_key, "native.testing.require_use_warnings");

    let fix = finding.fix.as_ref().expect("missing warnings should have a safe fix");
    assert_eq!(fix.title, "Add 'use warnings'");
    assert_eq!(fix.safety, FixSafety::Safe);
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(fix.edits[0].range, insertion_range());
    assert_eq!(fix.edits[0].new_text, "use warnings;\n");
}

#[test]
fn native_require_use_warnings_rule_accepts_exact_pragma_only() {
    let ast = empty_program_node();
    let config = CriticConfig::default();
    let exact_ctx = CriticContext::new("use warnings;\nmy $x = 1;\n", &ast, &config);
    let similar_ctx = CriticContext::new("use warningsx;\nmy $x = 1;\n", &ast, &config);
    let commented_ctx = CriticContext::new("# use warnings;\nmy $x = 1;\n", &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequireUseWarningsRule)]);

    assert!(registry.check(&exact_ctx).is_empty());
    assert_eq!(registry.check(&similar_ctx).len(), 1);
    assert_eq!(registry.check(&commented_ctx).len(), 1);
}

#[test]
fn native_strict_and_warnings_rules_run_together_in_order() {
    let ast = empty_program_node();
    let config = CriticConfig::default();
    let ctx = CriticContext::new("my $x = 1;\n", &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![
        Box::new(RequireUseStrictRule),
        Box::new(RequireUseWarningsRule),
    ]);

    let findings = registry.check(&ctx);

    assert_eq!(
        registry.rule_ids(),
        vec!["native.testing.require_use_strict", "native.testing.require_use_warnings"]
    );
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].rule_id, "native.testing.require_use_strict");
    assert_eq!(findings[1].rule_id, "native.testing.require_use_warnings");
}

#[test]
fn native_assignment_in_condition_rule_reports_if_assignment() {
    let source = "use strict;\nuse warnings;\nmy $x = 0;\nif ($x = 5) { print $x; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.common.assignment_in_condition");
    assert_eq!(finding.category, CriticCategory::Syntax);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(finding.message, "Assignment in condition - did you mean '=='?");
    assert_eq!(finding.suppression_key, "native.common.assignment_in_condition");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$x = 5");

    let fix = finding.fix.as_ref().expect("assignment condition should offer comparison fix");
    assert_eq!(fix.title, "Change to comparison (==)");
    assert_eq!(fix.safety, FixSafety::Suggested);
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(&source[fix.edits[0].range.start.byte..fix.edits[0].range.end.byte], "=");
    assert_eq!(fix.edits[0].new_text, "==");
    assert_eq!(finding.related.len(), 2);
}

#[test]
fn native_assignment_in_condition_rule_reports_statement_modifier_assignment() {
    let source = "use strict;\nuse warnings;\nmy $x = 0;\nprint $x if $x = 5;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule_id, "native.common.assignment_in_condition");
    assert_eq!(&source[findings[0].range.start.byte..findings[0].range.end.byte], "$x = 5");
}

#[test]
fn native_assignment_in_condition_rule_reports_while_assignment() {
    let source =
        "use strict;\nuse warnings;\nmy $x = 0;\nwhile ($x = next_value()) { print $x; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule_id, "native.common.assignment_in_condition");
    assert_eq!(
        &source[findings[0].range.start.byte..findings[0].range.end.byte],
        "$x = next_value()"
    );
}

#[test]
fn native_assignment_in_condition_rule_accepts_comparisons() {
    let source = "use strict;\nuse warnings;\nmy $x = 0;\nif ($x == 5) { print $x; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "comparison conditions should be accepted");
}

#[test]
fn native_assignment_in_condition_rule_accepts_explicitly_parenthesized_assignments() {
    let source = "use strict;\nuse warnings;\nmy $x = 0;\nif (($x = next_value())) { print $x; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

    let findings = registry.check(&ctx);

    assert!(
        findings.is_empty(),
        "double-parenthesized assignment conditions are treated as intentional"
    );
}

#[test]
fn native_assignment_in_condition_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $x = 0;\nif ($x = 5) { print $x; }\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.common.assignment_in_condition".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.common.assignment_in_condition -- intentional assignment\nuse strict;\nuse warnings;\nmy $x = 0;\nif ($x = 5) { print $x; }\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_assignment_in_condition_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $x = 0;\nif ($x = 5) { print $x; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.common.assignment_in_condition");
    assert_eq!(violations[0].description, "Assignment in condition - did you mean '=='?");
    assert_eq!(
        violations[0].explanation,
        "Assignments in conditions are usually accidental. Use '==' for numeric comparison, 'eq' for string comparison, or add parentheses if the assignment is intentional."
    );
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_printf_format_arity_rule_reports_static_mismatch() {
    let source = "use strict;\nuse warnings;\nprintf \"%s %s\", $name;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(PrintfFormatArityRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.common.printf_format_arity");
    assert_eq!(finding.category, CriticCategory::Syntax);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(finding.message, "`printf` format string has 2 specifiers but 1 argument supplied");
    assert_eq!(finding.suppression_key, "native.common.printf_format_arity");
    assert_eq!(
        &source[finding.range.start.byte..finding.range.end.byte],
        "printf \"%s %s\", $name"
    );
    assert_eq!(finding.related.len(), 1);
    assert_eq!(finding.related[0].message, "Format string contains 2 specifiers");
    assert!(finding.fix.is_none());
}

#[test]
fn native_printf_format_arity_rule_accepts_matching_and_dynamic_formats() {
    let source = "use strict;\nuse warnings;\nmy $fmt = \"%s\";\nprintf \"%s\", $name;\nprintf $fmt, $name;\nsprintf \"%s %d\", $name, $count;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(PrintfFormatArityRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "matching and dynamic formats should be accepted");
}

#[test]
fn native_printf_format_arity_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nprintf \"%s %s\", $name;\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.common.printf_format_arity".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(PrintfFormatArityRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.common.printf_format_arity -- generated format\nuse strict;\nuse warnings;\nprintf \"%s %s\", $name;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_printf_format_arity_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nsprintf \"%s %s\", $name;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(PrintfFormatArityRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.common.printf_format_arity");
    assert_eq!(
        violations[0].description,
        "`sprintf` format string has 2 specifiers but 1 argument supplied"
    );
    assert_eq!(
        violations[0].explanation,
        "Add 2 arguments to match 2 format specifiers, or adjust the format string"
    );
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_deprecated_defined_rule_reports_array_defined() {
    let source =
        "use strict;\nuse warnings;\nmy @items = (1, 2);\nif (defined @items) { print @items; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(DeprecatedDefinedRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.common.deprecated_defined");
    assert_eq!(finding.category, CriticCategory::Syntax);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(finding.message, "Use of 'defined @items' is deprecated");
    assert_eq!(finding.suppression_key, "native.common.deprecated_defined");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "defined @items");
    assert_eq!(finding.related.len(), 1);

    let fix = finding.fix.as_ref().expect("deprecated defined should offer direct fix");
    assert_eq!(fix.title, "Replace with '@items'");
    assert_eq!(fix.safety, FixSafety::Suggested);
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(fix.edits[0].new_text, "@items");
}

#[test]
fn native_deprecated_defined_rule_reports_parenthesized_hash_defined() {
    let source = "use strict;\nuse warnings;\nmy %seen = (a => 1);\nif (defined(%seen)) { print keys %seen; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(DeprecatedDefinedRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule_id, "native.common.deprecated_defined");
    assert_eq!(findings[0].message, "Use of 'defined %seen' is deprecated");
}

#[test]
fn native_deprecated_defined_rule_accepts_scalar_defined() {
    let source = "use strict;\nuse warnings;\nmy $item = 1;\nif (defined $item) { print $item; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(DeprecatedDefinedRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "scalar defined checks should be accepted");
}

#[test]
fn native_deprecated_defined_rule_composes_with_config_and_suppressions() {
    let source =
        "use strict;\nuse warnings;\nmy @items = (1, 2);\nif (defined @items) { print @items; }\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.common.deprecated_defined".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(DeprecatedDefinedRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.common.deprecated_defined -- legacy code\nuse strict;\nuse warnings;\nmy @items = (1, 2);\nif (defined @items) { print @items; }\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_deprecated_defined_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy @items = (1, 2);\ndefined @items;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(DeprecatedDefinedRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.common.deprecated_defined");
    assert_eq!(violations[0].description, "Use of 'defined @items' is deprecated");
    assert_eq!(
        violations[0].explanation,
        "Testing definedness of a whole array is deprecated because it was rarely useful and often wrong. Use the array in boolean context instead."
    );
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_undef_comparison_rule_reports_numeric_comparison_with_undef() {
    let source = "use strict;\nuse warnings;\nmy $value = maybe();\nif ($value == undef) { print $value; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndefComparisonRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.common.undef_comparison");
    assert_eq!(finding.category, CriticCategory::Syntax);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(finding.message, "Using '==' with undef -- use defined() to check first");
    assert_eq!(finding.suppression_key, "native.common.undef_comparison");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$value == undef");
    assert_eq!(finding.related.len(), 2);

    let fix = finding.fix.as_ref().expect("undef comparison should offer defined() fix");
    assert_eq!(fix.title, "Use defined() check");
    assert_eq!(fix.safety, FixSafety::Suggested);
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(fix.edits[0].new_text, "!defined($value)");
}

#[test]
fn native_undef_comparison_rule_reports_reversed_not_equal_comparison() {
    let source = "use strict;\nuse warnings;\nmy $value = maybe();\nif (undef != $value) { print $value; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndefComparisonRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule_id, "native.common.undef_comparison");
    assert_eq!(
        &source[findings[0].range.start.byte..findings[0].range.end.byte],
        "undef != $value"
    );
    assert_eq!(findings[0].fix.as_ref().expect("defined fix").edits[0].new_text, "defined($value)");
}

#[test]
fn native_undef_comparison_rule_accepts_defined_checks_and_other_comparisons() {
    let source = "use strict;\nuse warnings;\nmy $value = maybe();\nif (defined $value) { print $value; }\nif ($value == 0) { print $value; }\nif ($value eq undef) { print $value; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndefComparisonRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "only numeric ==/!= comparisons with undef should report");
}

#[test]
fn native_undef_comparison_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $value = maybe();\nif ($value == undef) { print $value; }\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.common.undef_comparison".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndefComparisonRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.common.undef_comparison -- legacy check\nuse strict;\nuse warnings;\nmy $value = maybe();\nif ($value == undef) { print $value; }\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_undef_comparison_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $value = maybe();\n$value == undef;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndefComparisonRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.common.undef_comparison");
    assert_eq!(violations[0].description, "Using '==' with undef -- use defined() to check first");
    assert_eq!(
        violations[0].explanation,
        "Numeric comparison with undef is usually wrong because undef is coerced before comparison. Use defined(...) for definedness checks, or normalize the value before comparing."
    );
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_stale_dollar_at_rule_reports_if_check_after_unlocalized_eval() {
    let source = "use strict;\nuse warnings;\neval { risky_call(); };\nif ($@) { warn $@; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(StaleDollarAtRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.common.stale_dollar_at");
    assert_eq!(finding.category, CriticCategory::Syntax);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(finding.message, "Checking $@ after eval can observe a stale error");
    assert_eq!(finding.suppression_key, "native.common.stale_dollar_at");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$@");
    assert_eq!(finding.related.len(), 1);
    assert!(finding.fix.is_none());
}

#[test]
fn native_stale_dollar_at_rule_reports_statement_modifier_after_unlocalized_eval() {
    let source = "use strict;\nuse warnings;\neval { risky_call(); };\nwarn $@ if $@;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(StaleDollarAtRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule_id, "native.common.stale_dollar_at");
    assert_eq!(&source[findings[0].range.start.byte..findings[0].range.end.byte], "$@");
}

#[test]
fn native_stale_dollar_at_rule_accepts_localized_or_return_checked_eval() {
    let source = "use strict;\nuse warnings;\n{\nlocal $@;\neval { risky_call(); };\nif ($@) { warn $@; }\n}\nmy $ok = eval { risky_call(); };\nif (!$ok) { warn 'failed'; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(StaleDollarAtRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "localized $@ and eval return checks should be accepted");
}

#[test]
fn native_stale_dollar_at_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\neval { risky_call(); };\nif ($@) { warn $@; }\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.common.stale_dollar_at".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(StaleDollarAtRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.common.stale_dollar_at -- legacy eval\nuse strict;\nuse warnings;\neval { risky_call(); };\nif ($@) { warn $@; }\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_stale_dollar_at_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\neval { risky_call(); };\nif ($@) { warn $@; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(StaleDollarAtRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.common.stale_dollar_at");
    assert_eq!(violations[0].description, "Checking $@ after eval can observe a stale error");
    assert_eq!(
        violations[0].explanation,
        "The $@ variable is global and can retain or be clobbered by unrelated exception handling. Localize $@ around eval, or check the eval return value before inspecting the error."
    );
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_unreachable_code_rule_reports_dead_statement_after_return() {
    let source = "use strict;\nuse warnings;\nsub f {\nreturn 1;\nmy $dead = 2;\n}\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnreachableCodeRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.common.unreachable_code");
    assert_eq!(finding.category, CriticCategory::Maintainability);
    assert_eq!(finding.severity, Severity::Harsh);
    assert_eq!(finding.message, "Unreachable code: this statement cannot be executed");
    assert_eq!(finding.suppression_key, "native.common.unreachable_code");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "my $dead");
    let fix = finding.fix.as_ref().expect("unreachable code should offer removal fix");
    assert_eq!(fix.title, "Remove unreachable code");
    assert_eq!(fix.safety, FixSafety::Safe);
    assert_eq!(
        &source[fix.edits[0].range.start.byte..fix.edits[0].range.end.byte],
        "my $dead = 2;\n"
    );
    assert_eq!(fix.edits[0].new_text, "");
}

#[test]
fn native_unreachable_code_rule_accepts_conditional_return_and_eval_die() {
    let source = "use strict;\nuse warnings;\nsub f {\nreturn if $cond;\nmy $live = 1;\neval { die 'caught'; };\nmy $after_eval = 2;\nreturn $live + $after_eval;\n}\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnreachableCodeRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "conditional return and caught eval die should be accepted");
}

#[test]
fn native_unreachable_code_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nsub f {\nreturn 1;\nmy $dead = 2;\n}\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.common.unreachable_code".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnreachableCodeRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.common.unreachable_code -- generated dead branch\nuse strict;\nuse warnings;\nsub f {\nreturn 1;\nmy $dead = 2;\n}\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_unreachable_code_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nsub f {\nreturn 1;\nmy $dead = 2;\n}\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnreachableCodeRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.common.unreachable_code");
    assert_eq!(violations[0].description, "Unreachable code: this statement cannot be executed");
    assert_eq!(violations[0].severity, Severity::Harsh);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_bareword_filehandle_rule_reports_open_bareword() {
    let source = "use strict;\nuse warnings;\nopen(FH, '<', 'file.txt');\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(BarewordFilehandleRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.io.bareword_filehandle");
    assert_eq!(finding.category, CriticCategory::Syntax);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(finding.message, "Bareword filehandle 'FH' should be lexical");
    assert_eq!(finding.suppression_key, "native.io.bareword_filehandle");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "FH");

    let fix = finding.fix.as_ref().expect("bareword filehandle should offer lexical fix");
    assert_eq!(fix.title, "Replace bareword filehandle 'FH' with lexical '$fh_fh'");
    assert_eq!(fix.safety, FixSafety::Suggested);
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(fix.edits[0].range, finding.range);
    assert_eq!(fix.edits[0].new_text, "my $fh_fh");
}

#[test]
fn native_bareword_filehandle_rule_accepts_lexical_and_standard_filehandles() {
    let source = "use strict;\nuse warnings;\nopen(my $fh, '<', 'file.txt');\nopen(STDOUT, '>', 'out.txt');\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(BarewordFilehandleRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "lexical and standard filehandles should be accepted");
}

#[test]
fn native_bareword_filehandle_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nopen(FH, '<', 'file.txt');\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.io.bareword_filehandle".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(BarewordFilehandleRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.io.bareword_filehandle -- legacy handle\nuse strict;\nuse warnings;\nopen(FH, '<', 'file.txt');\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_bareword_filehandle_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nopen(FH, '<', 'file.txt');\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(BarewordFilehandleRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.io.bareword_filehandle");
    assert_eq!(violations[0].description, "Bareword filehandle 'FH' should be lexical");
    assert_eq!(
        violations[0].explanation,
        "Bareword filehandles are package globals and can be accidentally reused across scopes. Use lexical filehandles for safer IO."
    );
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_two_arg_open_rule_reports_two_arg_open() {
    let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, $path);\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(TwoArgOpenRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.io.two_arg_open");
    assert_eq!(finding.category, CriticCategory::Security);
    assert_eq!(finding.severity, Severity::Harsh);
    assert_eq!(finding.message, "Two-argument open should use an explicit mode");
    assert_eq!(finding.suppression_key, "native.io.two_arg_open");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "open(my $fh, $path)");

    let fix = finding.fix.as_ref().expect("two-arg open should offer a safety fix");
    assert_eq!(fix.title, "Convert to three-argument open() for safety");
    assert_eq!(fix.safety, FixSafety::Suggested);
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(fix.edits[0].range, finding.range);
    assert_eq!(fix.edits[0].new_text, "open(my $fh, '<', $path)");
}

#[test]
fn native_two_arg_open_rule_accepts_three_arg_open() {
    let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path);\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(TwoArgOpenRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "three-argument open should be accepted");
}

#[test]
fn native_two_arg_open_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, $path);\n";
    let ast = parse_source(source);
    let excluded_config =
        CriticConfig { exclude: vec!["native.io.two_arg_open".to_string()], ..Default::default() };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(TwoArgOpenRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.io.two_arg_open -- trusted legacy filename\nuse strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, $path);\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());

    let severity_config = CriticConfig { severity: Severity::Stern as u8, ..Default::default() };
    let severity_ctx = CriticContext::new(source, &ast, &severity_config);
    assert!(registry.check(&severity_ctx).is_empty());
}

#[test]
fn native_two_arg_open_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, $path);\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(TwoArgOpenRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.io.two_arg_open");
    assert_eq!(violations[0].description, "Two-argument open should use an explicit mode");
    assert_eq!(
        violations[0].explanation,
        "Two-argument open combines mode and filename, which can allow shell interpretation when the filename is derived from input. Use three-argument open with a separate mode."
    );
    assert_eq!(violations[0].severity, Severity::Harsh);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_pipe_open_rule_reports_pipe_open_forms() {
    let source = "use strict;\nuse warnings;\nopen(my $read_fh, '-|', 'ls');\nopen(my $write_fh, '|-', 'cat');\nopen(FH, '|cmd');\nopen(my $legacy_read_fh, 'cat |');\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(PipeOpenRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 4);
    for finding in &findings {
        assert_eq!(finding.rule_id, "native.io.pipe_open");
        assert_eq!(finding.category, CriticCategory::Security);
        assert_eq!(finding.severity, Severity::Harsh);
        assert_eq!(finding.message, "Pipe-open executes a shell command");
        assert_eq!(finding.suppression_key, "native.io.pipe_open");
        assert!(finding.fix.is_none(), "pipe-open replacement is not safe to automate");
    }
    assert_eq!(
        &source[findings[0].range.start.byte..findings[0].range.end.byte],
        "open(my $read_fh, '-|', 'ls')"
    );
    assert_eq!(
        &source[findings[1].range.start.byte..findings[1].range.end.byte],
        "open(my $write_fh, '|-', 'cat')"
    );
    assert_eq!(
        &source[findings[2].range.start.byte..findings[2].range.end.byte],
        "open(FH, '|cmd')"
    );
    assert_eq!(
        &source[findings[3].range.start.byte..findings[3].range.end.byte],
        "open(my $legacy_read_fh, 'cat |')"
    );
}

#[test]
fn native_pipe_open_rule_accepts_normal_open() {
    let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path);\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(PipeOpenRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "normal three-argument open should be accepted");
}

#[test]
fn native_pipe_open_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nopen(my $fh, '-|', 'ls');\n";
    let ast = parse_source(source);
    let excluded_config =
        CriticConfig { exclude: vec!["native.io.pipe_open".to_string()], ..Default::default() };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(PipeOpenRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.io.pipe_open -- trusted command\nuse strict;\nuse warnings;\nopen(my $fh, '-|', 'ls');\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());

    let severity_config = CriticConfig { severity: Severity::Stern as u8, ..Default::default() };
    let severity_ctx = CriticContext::new(source, &ast, &severity_config);
    assert!(registry.check(&severity_ctx).is_empty());
}

#[test]
fn native_pipe_open_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nopen(my $fh, '-|', 'ls');\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(PipeOpenRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.io.pipe_open");
    assert_eq!(violations[0].description, "Pipe-open executes a shell command");
    assert_eq!(
        violations[0].explanation,
        "Pipe-open forms run a command through the shell. Prefer explicit command argument lists or IPC modules when command execution is required."
    );
    assert_eq!(violations[0].severity, Severity::Harsh);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_unchecked_open_close_rule_reports_bare_statement_calls() {
    let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path);\nclose($fh);\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UncheckedOpenCloseRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 2);
    for finding in &findings {
        assert_eq!(finding.rule_id, "native.io.unchecked_open_close");
        assert_eq!(finding.category, CriticCategory::Security);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.suppression_key, "native.io.unchecked_open_close");
        assert!(finding.fix.is_none(), "unchecked I/O fix needs caller-specific error text");
    }
    assert_eq!(findings[0].message, "open() return value should be checked");
    assert_eq!(findings[1].message, "close() return value should be checked");
    assert_eq!(
        &source[findings[0].range.start.byte..findings[0].range.end.byte],
        "open(my $fh, '<', $path)"
    );
    assert_eq!(&source[findings[1].range.start.byte..findings[1].range.end.byte], "close($fh)");
}

#[test]
fn native_unchecked_open_close_rule_accepts_or_die_checks() {
    let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path) or die $!;\nclose($fh) || die $!;\nopen(my $compact_fh, '<', $path)||die $!;\nclose($compact_fh)||die $!;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UncheckedOpenCloseRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "open/close guarded by error paths should be accepted");
}

#[test]
fn native_unchecked_open_close_rule_reports_argument_level_error_checks() {
    let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path || die 'missing path');\nclose($fh || die 'missing handle');\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UncheckedOpenCloseRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].message, "open() return value should be checked");
    assert_eq!(findings[1].message, "close() return value should be checked");
}

#[test]
fn native_unchecked_open_close_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path);\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.io.unchecked_open_close".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UncheckedOpenCloseRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let included_config = CriticConfig {
        include: vec!["native.io.unchecked_open_close".to_string()],
        ..Default::default()
    };
    let included_ctx = CriticContext::new(source, &ast, &included_config);
    assert_eq!(registry.check(&included_ctx).len(), 1);

    let other_include_config = CriticConfig {
        include: vec!["native.security.string_eval".to_string()],
        ..Default::default()
    };
    let other_include_ctx = CriticContext::new(source, &ast, &other_include_config);
    assert!(registry.check(&other_include_ctx).is_empty());

    let config = CriticConfig::default();
    let unsuppressed_ctx = CriticContext::new(source, &ast, &config);
    assert_eq!(registry.check(&unsuppressed_ctx).len(), 1);

    let suppressed_source = "## no critic native.io.unchecked_open_close -- handled by caller\nuse strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path);\n";
    let suppressed_ast = parse_source(suppressed_source);
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());

    let severity_config = CriticConfig { severity: Severity::Gentle as u8, ..Default::default() };
    let severity_ctx = CriticContext::new(source, &ast, &severity_config);
    assert!(registry.check(&severity_ctx).is_empty());
}

#[test]
fn native_unchecked_open_close_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path);\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UncheckedOpenCloseRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.io.unchecked_open_close");
    assert_eq!(violations[0].description, "open() return value should be checked");
    assert_eq!(
        violations[0].explanation,
        "open and close report I/O failures through their return value. Check the result with an explicit error path such as `or die` so failures are not silently ignored."
    );
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_qx_readpipe_rule_reports_qx_and_readpipe_forms() {
    let source = "use strict;\nuse warnings;\nmy $date = qx(date);\nmy $listing = qx/ls -la/;\nmy $hash = qx#whoami#;\nmy $pipe_delim = qx|id|;\nmy $tick_delim = qx`uname`;\nmy $pipe = readpipe($date);\nprint $listing . $pipe . $hash . $pipe_delim . $tick_delim;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(QxReadpipeRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 6);
    for finding in &findings {
        assert_eq!(finding.rule_id, "native.security.qx_readpipe");
        assert_eq!(finding.category, CriticCategory::Security);
        assert_eq!(finding.severity, Severity::Harsh);
        assert_eq!(finding.message, "qx/readpipe command execution detected");
        assert_eq!(finding.suppression_key, "native.security.qx_readpipe");
        assert!(finding.fix.is_none(), "qx/readpipe replacement is not safe to automate");
    }
    assert_eq!(&source[findings[0].range.start.byte..findings[0].range.end.byte], "qx(date)");
    assert_eq!(&source[findings[1].range.start.byte..findings[1].range.end.byte], "qx/ls -la/");
    assert_eq!(&source[findings[2].range.start.byte..findings[2].range.end.byte], "qx#whoami#");
    assert_eq!(&source[findings[3].range.start.byte..findings[3].range.end.byte], "qx|id|");
    assert_eq!(&source[findings[4].range.start.byte..findings[4].range.end.byte], "qx`uname`");
    assert_eq!(
        &source[findings[5].range.start.byte..findings[5].range.end.byte],
        "readpipe($date)"
    );
}

#[test]
fn native_qx_readpipe_rule_accepts_non_command_strings_and_calls() {
    let source = "use strict;\nuse warnings;\nmy $text = 'qx(date)';\nmy $value = read_line($text);\nprint $value;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(QxReadpipeRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "ordinary strings and non-readpipe calls should be accepted");
}

#[test]
fn native_qx_readpipe_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $out = qx(date);\nprint $out;\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.security.qx_readpipe".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(QxReadpipeRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.security.qx_readpipe -- trusted command\nuse strict;\nuse warnings;\nmy $out = readpipe('date');\nprint $out;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());

    let severity_config = CriticConfig { severity: Severity::Stern as u8, ..Default::default() };
    let severity_ctx = CriticContext::new(source, &ast, &severity_config);
    assert!(registry.check(&severity_ctx).is_empty());
}

#[test]
fn native_qx_readpipe_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $out = qx(date);\nprint $out;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(QxReadpipeRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.security.qx_readpipe");
    assert_eq!(violations[0].description, "qx/readpipe command execution detected");
    assert_eq!(
        violations[0].explanation,
        "qx and readpipe execute shell commands. Prefer explicit command argument lists or IPC modules when command execution is required."
    );
    assert_eq!(violations[0].severity, Severity::Harsh);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_backtick_exec_rule_reports_backtick_strings() {
    let source = "use strict;\nuse warnings;\nmy $out = `ls -la`;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(BacktickExecRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    for finding in &findings {
        assert_eq!(finding.rule_id, "native.security.backtick_exec");
        assert_eq!(finding.category, CriticCategory::Security);
        assert_eq!(finding.severity, Severity::Harsh);
        assert_eq!(finding.message, "Command execution detected");
        assert_eq!(finding.suppression_key, "native.security.backtick_exec");
        assert!(finding.fix.is_none(), "backtick command replacement is not safe to automate");
    }
    assert_eq!(&source[findings[0].range.start.byte..findings[0].range.end.byte], "`ls -la`");
}

#[test]
fn native_backtick_exec_rule_accepts_normal_strings() {
    let source = "use strict;\nuse warnings;\nmy $text = 'not a command';\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(BacktickExecRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "ordinary strings should be accepted");
}

#[test]
fn native_backtick_exec_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $out = `ls -la`;\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.security.backtick_exec".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(BacktickExecRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.security.backtick_exec -- trusted command\nuse strict;\nuse warnings;\nmy $out = `ls -la`;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());

    let severity_config = CriticConfig { severity: Severity::Stern as u8, ..Default::default() };
    let severity_ctx = CriticContext::new(source, &ast, &severity_config);
    assert!(registry.check(&severity_ctx).is_empty());
}

#[test]
fn native_backtick_exec_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $out = `ls -la`;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(BacktickExecRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.security.backtick_exec");
    assert_eq!(violations[0].description, "Command execution detected");
    assert_eq!(
        violations[0].explanation,
        "Backticks execute shell commands. Prefer explicit command argument lists or IPC modules when command execution is required."
    );
    assert_eq!(violations[0].severity, Severity::Harsh);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_string_eval_rule_reports_literal_and_variable_eval() {
    let source =
        "use strict;\nuse warnings;\nmy $code = 'print 1';\neval $code;\neval 'print 2';\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(StringEvalRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 2);
    for finding in &findings {
        assert_eq!(finding.rule_id, "native.security.string_eval");
        assert_eq!(finding.category, CriticCategory::Security);
        assert_eq!(finding.severity, Severity::Harsh);
        assert_eq!(finding.message, "String eval is a security risk");
        assert_eq!(finding.suppression_key, "native.security.string_eval");
        assert!(finding.fix.is_none(), "string eval replacement is not safe to automate");
    }
    assert_eq!(&source[findings[0].range.start.byte..findings[0].range.end.byte], "eval $code");
    assert_eq!(&source[findings[1].range.start.byte..findings[1].range.end.byte], "eval 'print 2'");
}

#[test]
fn native_string_eval_rule_accepts_block_eval() {
    let source = "use strict;\nuse warnings;\neval { my $x = 1; };\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(StringEvalRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "block eval should be accepted");
}

#[test]
fn native_string_eval_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $code = 'print 1';\neval $code;\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.security.string_eval".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(StringEvalRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.security.string_eval -- generated DSL\nuse strict;\nuse warnings;\nmy $code = 'print 1';\neval $code;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());

    let severity_config = CriticConfig { severity: Severity::Stern as u8, ..Default::default() };
    let severity_ctx = CriticContext::new(source, &ast, &severity_config);
    assert!(registry.check(&severity_ctx).is_empty());
}

#[test]
fn native_string_eval_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $code = 'print 1';\neval $code;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(StringEvalRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.security.string_eval");
    assert_eq!(violations[0].description, "String eval is a security risk");
    assert_eq!(
        violations[0].explanation,
        "String eval executes dynamically generated Perl code and is difficult to analyze safely. Prefer block eval for exception handling or a safer dispatch mechanism."
    );
    assert_eq!(violations[0].severity, Severity::Harsh);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_system_exec_rule_reports_system_and_exec_forms() {
    let source = "use strict;\nuse warnings;\nmy $cmd = 'ls';\nsystem($cmd);\nsystem('ls', '-la');\nexec($cmd);\nexec('ls', '-la');\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(SystemExecRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 4);
    for finding in &findings {
        assert_eq!(finding.rule_id, "native.security.system_exec");
        assert_eq!(finding.category, CriticCategory::Security);
        assert_eq!(finding.severity, Severity::Harsh);
        assert_eq!(finding.suppression_key, "native.security.system_exec");
        assert!(finding.fix.is_none(), "system/exec replacement is not safe to automate");
    }
    assert_eq!(findings[0].message, "system() executes a shell command");
    assert_eq!(findings[1].message, "system() executes a shell command");
    assert_eq!(findings[2].message, "exec() replaces the current process with a shell command");
    assert_eq!(findings[3].message, "exec() replaces the current process with a shell command");
    assert_eq!(&source[findings[0].range.start.byte..findings[0].range.end.byte], "system($cmd)");
    assert_eq!(
        &source[findings[1].range.start.byte..findings[1].range.end.byte],
        "system('ls', '-la')"
    );
    assert_eq!(&source[findings[2].range.start.byte..findings[2].range.end.byte], "exec($cmd)");
    assert_eq!(
        &source[findings[3].range.start.byte..findings[3].range.end.byte],
        "exec('ls', '-la')"
    );
}

#[test]
fn native_system_exec_rule_accepts_non_command_calls() {
    let source = "use strict;\nuse warnings;\nmy $system = 'name';\nmy $exec_ok = run_exec($system);\nprint $exec_ok;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(SystemExecRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "ordinary variables and non-system/exec calls should be accepted");
}

#[test]
fn native_system_exec_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $cmd = 'ls';\nsystem($cmd);\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.security.system_exec".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(SystemExecRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.security.system_exec -- trusted command\nuse strict;\nuse warnings;\nmy $cmd = 'ls';\nexec($cmd);\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());

    let severity_config = CriticConfig { severity: Severity::Stern as u8, ..Default::default() };
    let severity_ctx = CriticContext::new(source, &ast, &severity_config);
    assert!(registry.check(&severity_ctx).is_empty());
}

#[test]
fn native_system_exec_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $cmd = 'ls';\nsystem($cmd);\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(SystemExecRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.security.system_exec");
    assert_eq!(violations[0].description, "system() executes a shell command");
    assert_eq!(
        violations[0].explanation,
        "system and exec run external commands. Prefer explicit argument lists or IPC modules when command execution is required, and validate any user-controlled input."
    );
    assert_eq!(violations[0].severity, Severity::Harsh);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_require_pod_sections_rule_reports_incomplete_pod() {
    let source =
        "use strict;\nuse warnings;\n=head1 NAME\n\nApp::Demo - demo module\n\n=cut\n\n1;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequirePodSectionsRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.documentation.require_pod_sections");
    assert_eq!(finding.category, CriticCategory::Documentation);
    assert_eq!(finding.severity, Severity::Harsh);
    assert_eq!(finding.message, "POD is missing required =head1 DESCRIPTION section");
    assert_eq!(finding.suppression_key, "native.documentation.require_pod_sections");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "=head1 NAME");
    assert!(finding.fix.is_none(), "documentation policy should not fabricate POD content");
}

#[test]
fn native_require_pod_sections_rule_accepts_complete_pod_and_files_without_pod() {
    let complete_source = "use strict;\nuse warnings;\n=head1 NAME\n\nApp::Demo\n\n=head1 DESCRIPTION\n\nDemo.\n\n=cut\n\n1;\n";
    let complete_ast = parse_source(complete_source);
    let config = CriticConfig::default();
    let complete_ctx = CriticContext::new(complete_source, &complete_ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequirePodSectionsRule)]);

    assert!(registry.check(&complete_ctx).is_empty());

    let no_pod_source = "use strict;\nuse warnings;\npackage App::Demo;\n1;\n";
    let no_pod_ast = parse_source(no_pod_source);
    let no_pod_ctx = CriticContext::new(no_pod_source, &no_pod_ast, &config);

    assert!(
        registry.check(&no_pod_ctx).is_empty(),
        "native rule only checks existing POD sections to avoid noisy opt-in diagnostics"
    );
}

#[test]
fn native_require_pod_sections_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\n=head1 NAME\n\nApp::Demo\n\n=cut\n\n1;\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.documentation.require_pod_sections".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequirePodSectionsRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.documentation.require_pod_sections -- generated docs\nuse strict;\nuse warnings;\n=head1 NAME\n\nApp::Demo\n\n=cut\n\n1;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_require_pod_sections_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\n=head1 DESCRIPTION\n\nDemo.\n\n=cut\n\n1;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequirePodSectionsRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.documentation.require_pod_sections");
    assert_eq!(violations[0].description, "POD is missing required =head1 NAME section");
    assert_eq!(violations[0].severity, Severity::Harsh);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_recommended_registry_contains_initial_policy_bundle() {
    let source = "print 1;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::recommended();

    let findings = registry.check(&ctx);

    assert_eq!(
        registry.rule_ids(),
        vec![
            "native.testing.require_use_strict",
            "native.testing.require_use_warnings",
            "native.common.assignment_in_condition",
            "native.common.printf_format_arity",
            "native.common.deprecated_defined",
            "native.common.undef_comparison",
            "native.common.stale_dollar_at",
            "native.common.unreachable_code",
            "native.io.bareword_filehandle",
            "native.io.two_arg_open",
            "native.io.pipe_open",
            "native.io.unchecked_open_close",
            "native.security.qx_readpipe",
            "native.security.backtick_exec",
            "native.security.string_eval",
            "native.security.system_exec",
            "native.variables.unused_lexical",
            "native.variables.unused_parameter",
            "native.variables.duplicate_parameter",
            "native.variables.parameter_shadows_global",
            "native.variables.duplicate_lexical",
            "native.variables.shadowed_lexical",
            "native.regex.capture_without_match",
            "native.variables.undeclared",
            "native.variables.uninitialized",
            "native.syntax.unquoted_bareword",
            "native.documentation.require_pod_sections",
            "native.syntax.prohibit_leading_zeros"
        ]
    );
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].suppression_key, "native.testing.require_use_strict");
    assert_eq!(findings[1].suppression_key, "native.testing.require_use_warnings");
}

#[test]
fn native_unused_lexical_rule_reports_declared_but_unread_variable() {
    let source = "use strict;\nuse warnings;\nmy $unused = 1;\nprint 1;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedLexicalVariableRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.variables.unused_lexical");
    assert_eq!(finding.category, CriticCategory::Semantic);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(finding.message, "Lexical variable '$unused' is declared but never used");
    assert_eq!(finding.suppression_key, "native.variables.unused_lexical");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$unused");

    let fix = finding.fix.as_ref().expect("unused lexical should offer an intent marker");
    assert_eq!(fix.title, "Rename to '$_unused'");
    assert_eq!(fix.safety, FixSafety::Suggested);
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(fix.edits[0].range, finding.range);
    assert_eq!(fix.edits[0].new_text, "$_unused");
}

#[test]
fn native_unused_lexical_rule_accepts_used_and_intentionally_unused_variables() {
    let source = "use strict;\nuse warnings;\nmy $used = 1;\nmy $_ignored = 2;\nprint $used;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedLexicalVariableRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "used and underscore-prefixed variables should be accepted");
}

#[test]
fn native_unused_lexical_rule_reports_multiple_sigils() {
    let source = "use strict;\nuse warnings;\nmy @items = (1, 2);\nmy %seen = ();\nprint 1;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedLexicalVariableRule)]);

    let findings = registry.check(&ctx);
    let names = findings
        .iter()
        .map(|finding| &source[finding.range.start.byte..finding.range.end.byte])
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["@items", "%seen"]);
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.fix.as_ref().expect("fix").edits[0].new_text.as_str())
            .collect::<Vec<_>>(),
        vec!["@_items", "%_seen"]
    );
}

#[test]
fn native_unused_lexical_rule_composes_with_config_and_suppressions() {
    let ast = parse_source("use strict;\nuse warnings;\nmy $unused = 1;\n");
    let excluded_config = CriticConfig {
        exclude: vec!["native.variables.unused_lexical".to_string()],
        ..Default::default()
    };
    let excluded_ctx =
        CriticContext::new("use strict;\nuse warnings;\nmy $unused = 1;\n", &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedLexicalVariableRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.variables.unused_lexical -- legacy fixture\nuse strict;\nuse warnings;\nmy $unused = 1;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_unused_lexical_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $unused = 1;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedLexicalVariableRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.variables.unused_lexical");
    assert_eq!(violations[0].description, "Lexical variable '$unused' is declared but never used");
    assert_eq!(
        violations[0].explanation,
        "Remove the lexical variable, use it, or prefix it with '_' to mark it intentionally unused."
    );
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_unused_parameter_rule_reports_unread_signature_parameter() {
    let source = "use strict;\nuse warnings;\nsub helper($used, $unused) { return $used; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedParameterRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.variables.unused_parameter");
    assert_eq!(finding.category, CriticCategory::Semantic);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(finding.message, "Parameter '$unused' is never used");
    assert_eq!(finding.suppression_key, "native.variables.unused_parameter");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$unused");

    let fix = finding.fix.as_ref().expect("unused parameter should offer an intent marker");
    assert_eq!(fix.title, "Rename to '$_unused'");
    assert_eq!(fix.safety, FixSafety::Suggested);
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(fix.edits[0].range, finding.range);
    assert_eq!(fix.edits[0].new_text, "$_unused");
}

#[test]
fn native_unused_parameter_rule_accepts_used_and_intentionally_unused_parameters() {
    let source = "use strict;\nuse warnings;\nsub helper($used, $_ignored) { return $used; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedParameterRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "used and underscore-prefixed parameters should be accepted");
}

#[test]
fn native_unused_parameter_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nsub helper($used, $unused) { return $used; }\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.variables.unused_parameter".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedParameterRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.variables.unused_parameter -- fixture\nuse strict;\nuse warnings;\nsub helper($used, $unused) { return $used; }\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_unused_parameter_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nsub helper($used, $unused) { return $used; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedParameterRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.variables.unused_parameter");
    assert_eq!(violations[0].description, "Parameter '$unused' is never used");
    assert_eq!(
        violations[0].explanation,
        "Use the parameter or prefix it with '_' to mark it intentionally unused."
    );
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_duplicate_parameter_rule_reports_repeated_signature_parameter() {
    let source = "use strict;\nuse warnings;\nsub helper($arg, $arg) { return $arg; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(DuplicateParameterRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.variables.duplicate_parameter");
    assert_eq!(finding.category, CriticCategory::Semantic);
    assert_eq!(finding.severity, Severity::Gentle);
    assert_eq!(finding.message, "Parameter '$arg' appears more than once in this signature");
    assert_eq!(finding.suppression_key, "native.variables.duplicate_parameter");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$arg");

    let fix = finding.fix.as_ref().expect("duplicate parameter should offer rename");
    assert_eq!(fix.title, "Rename duplicate parameter to '$arg_2'");
    assert_eq!(fix.safety, FixSafety::Suggested);
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(fix.edits[0].range, finding.range);
    assert_eq!(fix.edits[0].new_text, "$arg_2");
}

#[test]
fn native_duplicate_parameter_rule_accepts_unique_parameters() {
    let source =
        "use strict;\nuse warnings;\nsub helper($left, $right) { return $left + $right; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(DuplicateParameterRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "unique parameters should be accepted");
}

#[test]
fn native_duplicate_parameter_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nsub helper($arg, $arg) { return $arg; }\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.variables.duplicate_parameter".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(DuplicateParameterRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no perl-lsp-critic native.variables.duplicate_parameter -- fixture\nuse strict;\nuse warnings;\nsub helper($arg, $arg) { return $arg; }\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_duplicate_parameter_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nsub helper($arg, $arg) { return $arg; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(DuplicateParameterRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.variables.duplicate_parameter");
    assert_eq!(
        violations[0].description,
        "Parameter '$arg' appears more than once in this signature"
    );
    assert_eq!(
        violations[0].explanation,
        "Remove the duplicate parameter or rename it so every signature binding is unique."
    );
    assert_eq!(violations[0].severity, Severity::Gentle);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_parameter_shadows_global_rule_reports_parameter_shadowing() {
    let source = "use strict;\nuse warnings;\nmy $name = 'outer';\nsub helper($name) { return $name; }\nprint $name;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(ParameterShadowsGlobalRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.variables.parameter_shadows_global");
    assert_eq!(finding.category, CriticCategory::Semantic);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(finding.message, "Parameter '$name' shadows an outer declaration");
    assert_eq!(finding.suppression_key, "native.variables.parameter_shadows_global");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$name");

    let fix = finding.fix.as_ref().expect("shadowing parameter should offer rename");
    assert_eq!(fix.title, "Rename parameter to '$p_name'");
    assert_eq!(fix.safety, FixSafety::Suggested);
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(fix.edits[0].range, finding.range);
    assert_eq!(fix.edits[0].new_text, "$p_name");
}

#[test]
fn native_parameter_shadows_global_rule_accepts_unique_parameters() {
    let source =
        "use strict;\nuse warnings;\nmy $outer = 1;\nsub helper($inner) { return $inner; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(ParameterShadowsGlobalRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "non-shadowing parameters should be accepted");
}

#[test]
fn native_parameter_shadows_global_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $name = 'outer';\nsub helper($name) { return $name; }\nprint $name;\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.variables.parameter_shadows_global".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(ParameterShadowsGlobalRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.variables.parameter_shadows_global -- fixture\nuse strict;\nuse warnings;\nmy $name = 'outer';\nsub helper($name) { return $name; }\nprint $name;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_parameter_shadows_global_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $name = 'outer';\nsub helper($name) { return $name; }\nprint $name;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(ParameterShadowsGlobalRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.variables.parameter_shadows_global");
    assert_eq!(violations[0].description, "Parameter '$name' shadows an outer declaration");
    assert_eq!(
        violations[0].explanation,
        "Rename the parameter or use the outer variable directly to avoid confusing scope shadowing."
    );
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_duplicate_lexical_rule_reports_same_scope_redeclaration() {
    let source = "use strict;\nuse warnings;\nmy $dup = 1;\nmy $dup = 2;\nprint $dup;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry =
        NativeCriticRegistry::with_rules(vec![Box::new(DuplicateLexicalDeclarationRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.variables.duplicate_lexical");
    assert_eq!(finding.category, CriticCategory::Semantic);
    assert_eq!(finding.severity, Severity::Gentle);
    assert_eq!(
        finding.message,
        "Lexical variable '$dup' is declared more than once in the same scope"
    );
    assert_eq!(finding.suppression_key, "native.variables.duplicate_lexical");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$dup");

    let fix = finding.fix.as_ref().expect("duplicate my should offer a safe fix");
    assert_eq!(fix.title, "Remove duplicate 'my' declaration");
    assert_eq!(fix.safety, FixSafety::Safe);
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(&source[fix.edits[0].range.start.byte..fix.edits[0].range.end.byte], "my ");
    assert_eq!(fix.edits[0].new_text, "");
}

#[test]
fn native_duplicate_lexical_rule_accepts_nested_shadowing() {
    let source = "use strict;\nuse warnings;\nmy $value = 1;\n{ my $value = 2; print $value; }\nprint $value;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry =
        NativeCriticRegistry::with_rules(vec![Box::new(DuplicateLexicalDeclarationRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "nested lexical shadowing is not same-scope duplication");
}

#[test]
fn native_duplicate_lexical_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $dup = 1;\nmy $dup = 2;\nprint $dup;\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.variables.duplicate_lexical".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry =
        NativeCriticRegistry::with_rules(vec![Box::new(DuplicateLexicalDeclarationRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no perl-lsp-critic native.variables.duplicate_lexical -- fixture\nuse strict;\nuse warnings;\nmy $dup = 1;\nmy $dup = 2;\nprint $dup;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_duplicate_lexical_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $dup = 1;\nmy $dup = 2;\nprint $dup;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry =
        NativeCriticRegistry::with_rules(vec![Box::new(DuplicateLexicalDeclarationRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.variables.duplicate_lexical");
    assert_eq!(
        violations[0].description,
        "Lexical variable '$dup' is declared more than once in the same scope"
    );
    assert_eq!(
        violations[0].explanation,
        "Remove the duplicate lexical declarator or assign to the existing lexical variable."
    );
    assert_eq!(violations[0].severity, Severity::Gentle);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_shadowed_lexical_rule_reports_inner_shadowing() {
    let source = "use strict;\nuse warnings;\nmy $value = 1;\n{ my $value = 2; print $value; }\nprint $value;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(ShadowedLexicalVariableRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.variables.shadowed_lexical");
    assert_eq!(finding.category, CriticCategory::Semantic);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(finding.message, "Lexical variable '$value' shadows an outer declaration");
    assert_eq!(finding.suppression_key, "native.variables.shadowed_lexical");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$value");

    let fix = finding.fix.as_ref().expect("shadowed lexical should offer a rename");
    assert_eq!(fix.title, "Rename to '$inner_value'");
    assert_eq!(fix.safety, FixSafety::Suggested);
    assert_eq!(fix.edits.len(), 1);
    assert_eq!(fix.edits[0].range, finding.range);
    assert_eq!(fix.edits[0].new_text, "$inner_value");
}

#[test]
fn native_shadowed_lexical_rule_accepts_unique_nested_lexicals() {
    let source = "use strict;\nuse warnings;\nmy $outer = 1;\n{ my $inner = 2; print $inner; }\nprint $outer;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(ShadowedLexicalVariableRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "unique nested lexicals should not be shadowing findings");
}

#[test]
fn native_shadowed_lexical_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $value = 1;\n{ my $value = 2; print $value; }\nprint $value;\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.variables.shadowed_lexical".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(ShadowedLexicalVariableRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.variables.shadowed_lexical -- fixture\nuse strict;\nuse warnings;\nmy $value = 1;\n{ my $value = 2; print $value; }\nprint $value;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_shadowed_lexical_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $value = 1;\n{ my $value = 2; print $value; }\nprint $value;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(ShadowedLexicalVariableRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.variables.shadowed_lexical");
    assert_eq!(violations[0].description, "Lexical variable '$value' shadows an outer declaration");
    assert_eq!(
        violations[0].explanation,
        "Rename the inner lexical variable or use the outer variable directly to avoid confusing scope shadowing."
    );
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_capture_var_rule_reports_capture_used_without_match() {
    let source = "use strict;\nuse warnings;\nmy $x = $1;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry =
        NativeCriticRegistry::with_rules(vec![Box::new(CaptureVarWithoutRegexMatchRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.regex.capture_without_match");
    assert_eq!(finding.category, CriticCategory::Semantic);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(
        finding.message,
        "Capture variable '$1' used without a preceding regex match in scope"
    );
    assert_eq!(finding.suppression_key, "native.regex.capture_without_match");
    assert_eq!(finding.fix.as_ref().map(|fix| fix.title.as_str()), None);
}

#[test]
fn native_capture_var_rule_accepts_capture_after_regex_match() {
    let source = "use strict;\nuse warnings;\nif ('hello' =~ /(ell)/) { my $x = $1; }\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry =
        NativeCriticRegistry::with_rules(vec![Box::new(CaptureVarWithoutRegexMatchRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "capture after a regex match should be accepted");
}

#[test]
fn native_capture_var_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $x = $1;\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.regex.capture_without_match".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry =
        NativeCriticRegistry::with_rules(vec![Box::new(CaptureVarWithoutRegexMatchRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.regex.capture_without_match -- fixture\nuse strict;\nuse warnings;\nmy $x = $1;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_capture_var_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $x = $1;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry =
        NativeCriticRegistry::with_rules(vec![Box::new(CaptureVarWithoutRegexMatchRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.regex.capture_without_match");
    assert_eq!(
        violations[0].description,
        "Capture variable '$1' used without a preceding regex match in scope"
    );
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_undeclared_variable_rule_reports_undeclared_use() {
    let source = "use strict;\nuse warnings;\nprint $undeclared;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndeclaredVariableRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.variables.undeclared");
    assert_eq!(finding.category, CriticCategory::Semantic);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(finding.message, "Variable '$undeclared' is used but not declared");
    assert_eq!(finding.suppression_key, "native.variables.undeclared");
    assert_eq!(
        finding.fix.as_ref().map(|fix| (fix.title.as_str(), fix.safety)),
        Some(("Change to 'my $undeclared'", FixSafety::Suggested))
    );
}

#[test]
fn native_undeclared_variable_rule_accepts_declared_variables() {
    let source = "use strict;\nuse warnings;\nmy $declared = 1;\nprint $declared;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndeclaredVariableRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "declared variables should be accepted");
}

#[test]
fn native_undeclared_variable_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nprint $undeclared;\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.variables.undeclared".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndeclaredVariableRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.variables.undeclared -- fixture\nuse strict;\nuse warnings;\nprint $undeclared;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_undeclared_variable_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nprint $undeclared;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndeclaredVariableRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.variables.undeclared");
    assert_eq!(violations[0].description, "Variable '$undeclared' is used but not declared");
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_uninitialized_variable_rule_reports_use_before_init() {
    let source = "use strict;\nuse warnings;\nmy $count;\nprint $count;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UninitializedVariableRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.variables.uninitialized");
    assert_eq!(finding.category, CriticCategory::Semantic);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(finding.message, "Variable '$count' used before initialization");
    assert_eq!(finding.suppression_key, "native.variables.uninitialized");
    assert_eq!(finding.fix.as_ref().map(|fix| fix.title.as_str()), None);
}

#[test]
fn native_uninitialized_variable_rule_accepts_initialized_variables() {
    let source = "use strict;\nuse warnings;\nmy $count = 0;\nprint $count;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UninitializedVariableRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "initialized variables should be accepted");
}

#[test]
fn native_uninitialized_variable_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $count;\nprint $count;\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.variables.uninitialized".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UninitializedVariableRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.variables.uninitialized -- fixture\nuse strict;\nuse warnings;\nmy $count;\nprint $count;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_uninitialized_variable_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $count;\nprint $count;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UninitializedVariableRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.variables.uninitialized");
    assert_eq!(violations[0].description, "Variable '$count' used before initialization");
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_unquoted_bareword_rule_reports_bareword_under_strict() {
    let source = "use strict;\nuse warnings;\nmy $x = FOO;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnquotedBarewordRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.syntax.unquoted_bareword");
    assert_eq!(finding.category, CriticCategory::Syntax);
    assert_eq!(finding.severity, Severity::Stern);
    assert_eq!(finding.message, "Bareword 'FOO' not allowed under strict");
    assert_eq!(finding.suppression_key, "native.syntax.unquoted_bareword");
    assert_eq!(
        finding.fix.as_ref().map(|fix| (fix.title.as_str(), fix.safety)),
        Some(("Quote as \"FOO\"", FixSafety::Suggested))
    );
}

#[test]
fn native_unquoted_bareword_rule_accepts_quoted_strings() {
    let source = "use strict;\nuse warnings;\nmy $x = \"FOO\";\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnquotedBarewordRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "quoted strings should be accepted");
}

#[test]
fn native_unquoted_bareword_rule_accepts_fat_comma_keys() {
    let source = "use strict;\nuse warnings;\nplan tests => 2;\nhas name => (is => 'ro');\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnquotedBarewordRule)]);

    let findings = registry.check(&ctx);

    assert!(findings.is_empty(), "fat-comma keys should be accepted as quoted strings");
}

#[test]
fn native_unquoted_bareword_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $x = FOO;\n";
    let ast = parse_source(source);
    let excluded_config = CriticConfig {
        exclude: vec!["native.syntax.unquoted_bareword".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnquotedBarewordRule)]);

    assert!(registry.check(&excluded_ctx).is_empty());

    let suppressed_source = "## no critic native.syntax.unquoted_bareword -- fixture\nuse strict;\nuse warnings;\nmy $x = FOO;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

    assert!(registry.check(&suppressed_ctx).is_empty());
}

#[test]
fn native_unquoted_bareword_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $x = FOO;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnquotedBarewordRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.syntax.unquoted_bareword");
    assert_eq!(violations[0].description, "Bareword 'FOO' not allowed under strict");
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_critic_registry_maps_findings_to_legacy_violations() {
    let ast = empty_program_node();
    let config = config_with_minimum_severity(1);
    let ctx = CriticContext::new("dummy second", &ast, &config);
    let registry =
        NativeCriticRegistry::with_rules(vec![Box::new(DummyRule), Box::new(SecondDummyRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 2);
    assert_eq!(violations[0].policy, "native.test.dummy");
    assert_eq!(violations[0].description, "dummy finding");
    assert_eq!(violations[0].file, "lib/App.pm");
    assert_eq!(violations[1].policy, "native.test.second");
    assert_eq!(violations[1].description, "second finding");
    assert_eq!(violations[1].file, "lib/App.pm");
}

#[test]
fn native_critic_registry_honors_include_and_exclude_config() {
    let ast = empty_program_node();
    let config = CriticConfig {
        severity: 1,
        include: vec!["native.test.dummy".to_string()],
        exclude: vec!["native.test.second".to_string()],
        ..Default::default()
    };
    let ctx = CriticContext::new("dummy second", &ast, &config);
    let registry =
        NativeCriticRegistry::with_rules(vec![Box::new(DummyRule), Box::new(SecondDummyRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule_id, "native.test.dummy");
}

#[test]
fn native_critic_registry_honors_minimum_severity_config() {
    let ast = empty_program_node();
    let config = CriticConfig { severity: 3, ..Default::default() };
    let ctx = CriticContext::new("dummy second", &ast, &config);
    let registry =
        NativeCriticRegistry::with_rules(vec![Box::new(DummyRule), Box::new(SecondDummyRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule_id, "native.test.dummy");
    assert_eq!(findings[0].severity, Severity::Harsh);
}

#[test]
fn native_critic_suppression_map_parses_directives_and_reasons() {
    let source = "\
## no critic native.testing.require_use_strict -- generated legacy file
## no perl-lsp-critic native.testing.require_use_warnings,native.test.second
my $x = 1;
";

    let suppressions = CriticSuppressionMap::from_source(source);

    assert_eq!(suppressions.suppressions().len(), 3);
    assert_eq!(suppressions.suppressions()[0].rule_id, "native.testing.require_use_strict");
    assert_eq!(suppressions.suppressions()[0].scope, CriticSuppressionScope::File);
    assert_eq!(suppressions.suppressions()[0].line, 0);
    assert_eq!(suppressions.suppressions()[0].reason.as_deref(), Some("generated legacy file"));
    assert_eq!(suppressions.suppressions()[1].rule_id, "native.testing.require_use_warnings");
    assert_eq!(suppressions.suppressions()[2].rule_id, "native.test.second");
}

#[test]
fn native_critic_registry_filters_suppressed_findings() {
    let ast = empty_program_node();
    let config = CriticConfig::default();
    let ctx = CriticContext::new(
        "## no critic native.testing.require_use_strict -- legacy file\nmy $x = 1;\n",
        &ast,
        &config,
    );
    let registry = NativeCriticRegistry::recommended();

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule_id, "native.testing.require_use_warnings");
}

#[test]
fn native_critic_registry_filters_suppressed_violations() {
    let ast = empty_program_node();
    let config = CriticConfig::default();
    let ctx = CriticContext::new(
        "## no perl-lsp-critic native.testing.require_use_warnings\nmy $x = 1;\n",
        &ast,
        &config,
    );
    let registry = NativeCriticRegistry::recommended();

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.testing.require_use_strict");
    assert_eq!(violations[0].file, "lib/App.pm");
}

#[test]
fn native_prohibit_leading_zeros_rule_reports_octal_literal() {
    let source = "use strict;\nuse warnings;\nchmod(0755, $file);\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(ProhibitLeadingZerosRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.rule_id, "native.syntax.prohibit_leading_zeros");
    assert_eq!(finding.category, CriticCategory::Syntax);
    assert_eq!(finding.severity, Severity::Stern);
    assert!(
        finding.message.contains("0755"),
        "message should mention the literal: {}",
        finding.message
    );
    assert!(finding.message.contains("octal"), "message should mention octal: {}", finding.message);
    assert!(
        finding.message.contains("493"),
        "message should include decimal value 493: {}",
        finding.message
    );
    assert!(
        finding.explanation.contains("evaluates to 493, not decimal 0755"),
        "explanation should compare evaluated and written values: {}",
        finding.explanation
    );
    assert!(
        finding.explanation.contains("0o755"),
        "explanation should suggest explicit octal spelling: {}",
        finding.explanation
    );
    assert_eq!(finding.suppression_key, "native.syntax.prohibit_leading_zeros");
    assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "0755");
    assert!(finding.fix.is_none(), "leading-zeros is diagnostic-only (no auto-fix)");
}

#[test]
fn native_prohibit_leading_zeros_rule_normalizes_underscored_octal_hint() {
    let source = "use strict;\nuse warnings;\nmy $mode = 0_755;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(ProhibitLeadingZerosRule)]);

    let findings = registry.check(&ctx);

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert!(
        finding.message.contains("493"),
        "message should include decimal value 493: {}",
        finding.message
    );
    assert!(
        finding.explanation.contains("0o755"),
        "explicit octal suggestion should not preserve separator after the prefix: {}",
        finding.explanation
    );
}

#[test]
fn native_prohibit_leading_zeros_rule_accepts_safe_numeric_forms() {
    // Hex, binary, float, plain zero, and invalid-octal-looking forms are
    // not silent-octal cases for this rule.
    let source = "use strict;\nuse warnings;\nmy $h = 0xFF;\nmy $b = 0b1010;\nmy $f = 0.5;\nmy $g = 00.5;\nmy $z = 0;\nmy $zero = 00;\nmy $small = 0007;\nmy $bad = 018;\nmy $also_bad = 0_8;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(ProhibitLeadingZerosRule)]);

    let findings = registry.check(&ctx);

    assert!(
        findings.is_empty(),
        "hex/binary/float/zero forms should not be flagged, got: {:?}",
        findings.iter().map(|f| &f.message).collect::<Vec<_>>()
    );
}

#[test]
fn native_prohibit_leading_zeros_rule_composes_with_config_and_suppressions() {
    let source = "use strict;\nuse warnings;\nmy $mode = 0644;\n";
    let ast = parse_source(source);

    let excluded_config = CriticConfig {
        exclude: vec!["native.syntax.prohibit_leading_zeros".to_string()],
        ..Default::default()
    };
    let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(ProhibitLeadingZerosRule)]);

    assert!(registry.check(&excluded_ctx).is_empty(), "excluded rule should produce no findings");

    let suppressed_source = "## no critic native.syntax.prohibit_leading_zeros -- intentional octal\nuse strict;\nuse warnings;\nmy $mode = 0644;\n";
    let suppressed_ast = parse_source(suppressed_source);
    let default_config = CriticConfig::default();
    let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &default_config);

    assert!(
        registry.check(&suppressed_ctx).is_empty(),
        "suppressed rule should produce no findings"
    );
}

#[test]
fn native_prohibit_leading_zeros_rule_flows_through_violation_bridge() {
    let source = "use strict;\nuse warnings;\nmy $timeout = 0300;\n";
    let ast = parse_source(source);
    let config = CriticConfig::default();
    let ctx = CriticContext::new(source, &ast, &config);
    let registry = NativeCriticRegistry::with_rules(vec![Box::new(ProhibitLeadingZerosRule)]);

    let violations = registry.check_violations(&ctx, "lib/App.pm");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].policy, "native.syntax.prohibit_leading_zeros");
    assert!(violations[0].description.contains("0300"), "description should mention the literal");
    assert_eq!(violations[0].severity, Severity::Stern);
    assert_eq!(violations[0].file, "lib/App.pm");
}
