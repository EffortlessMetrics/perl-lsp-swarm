//! Walk a parsed Rust file and collect tautological assertion macros.

use super::detect::{Detection, RuleId, classify_assert_condition, classify_assert_eq};
use syn::parse::{ParseStream, Parser};
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Expr, ExprMacro, File, ItemMacro, Macro, StmtMacro};

const ASSERT_MACROS: &[&str] = &["assert", "debug_assert"];
const ASSERT_EQ_MACROS: &[&str] = &["assert_eq", "debug_assert_eq"];

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Finding {
    pub path: String,
    pub line: u32,
    pub rule: RuleId,
    pub shape: &'static str,
}

impl Finding {
    pub fn render(&self) -> String {
        format!("{}:{}: tautology:{}: {}", self.path, self.line, self.rule.as_str(), self.shape)
    }
}

pub fn scan_file(path: &str, source: &str) -> Result<Vec<Finding>, String> {
    let file = syn::parse_file(source).map_err(|error| error.to_string())?;
    Ok(scan_ast(path, &file))
}

pub fn scan_ast(path: &str, file: &File) -> Vec<Finding> {
    let mut visitor = AssertionVisitor { path, findings: Vec::new() };
    visitor.visit_file(file);
    visitor.findings.sort();
    visitor.findings
}

struct AssertionVisitor<'a> {
    path: &'a str,
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for AssertionVisitor<'_> {
    fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
        self.inspect_macro(&node.mac);
        syn::visit::visit_expr_macro(self, node);
    }

    fn visit_stmt_macro(&mut self, node: &'ast StmtMacro) {
        self.inspect_macro(&node.mac);
        syn::visit::visit_stmt_macro(self, node);
    }

    fn visit_item_macro(&mut self, node: &'ast ItemMacro) {
        self.inspect_macro(&node.mac);
        syn::visit::visit_item_macro(self, node);
    }
}

impl AssertionVisitor<'_> {
    fn inspect_macro(&mut self, mac: &Macro) {
        let Some(name) = mac.path.segments.last().map(|segment| segment.ident.clone()) else {
            return;
        };
        if ASSERT_MACROS.iter().any(|candidate| name == *candidate) {
            if let Some(expr) = parse_assert_condition(mac.tokens.clone()) {
                self.push(classify_assert_condition(&expr), mac);
            }
            return;
        }
        if ASSERT_EQ_MACROS.iter().any(|candidate| name == *candidate) {
            if let Some((left, right)) = parse_assert_eq_args(mac.tokens.clone()) {
                self.push(classify_assert_eq(&left, &right), mac);
            }
        }
    }

    fn push(&mut self, detection: Option<Detection>, mac: &Macro) {
        let Some(detection) = detection else {
            return;
        };
        let line = match u32::try_from(mac.path.span().start().line) {
            Ok(line) if line > 0 => line,
            _ => detection.line,
        };
        self.findings.push(Finding {
            path: self.path.to_string(),
            line,
            rule: detection.rule,
            shape: detection.rule.shape(),
        });
    }
}

fn parse_assert_condition(tokens: proc_macro2::TokenStream) -> Option<Expr> {
    fn parser(input: ParseStream<'_>) -> syn::Result<Expr> {
        let expr: Expr = input.parse()?;
        if input.peek(syn::Token![,]) {
            let _: syn::Token![,] = input.parse()?;
            let _: proc_macro2::TokenStream = input.parse()?;
        }
        Ok(expr)
    }
    parser.parse2(tokens).ok()
}

fn parse_assert_eq_args(tokens: proc_macro2::TokenStream) -> Option<(Expr, Expr)> {
    fn parser(input: ParseStream<'_>) -> syn::Result<(Expr, Expr)> {
        let left: Expr = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        let right: Expr = input.parse()?;
        if input.peek(syn::Token![,]) {
            let _: syn::Token![,] = input.parse()?;
            let _: proc_macro2::TokenStream = input.parse()?;
        }
        Ok((left, right))
    }
    parser.parse2(tokens).ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::super::detect::RuleId;
    use super::scan_file;

    fn rules(source: &str) -> Vec<RuleId> {
        scan_file("probe.rs", source).expect("parse fixture").into_iter().map(|f| f.rule).collect()
    }

    #[test]
    fn scans_assert_macros_and_skips_non_assert_tokens() {
        let source = r#"
            fn probe(value: Option<u8>, result: Result<(), ()>, ready: bool) {
                assert!(value.is_some() || value.is_none());
                debug_assert!(result.is_ok() || result.is_err());
                assert!(ready || !ready, "still a tautology");
                assert_eq!(ready, ready);
                let _ = value.is_some() || value.is_none();
                if result.is_ok() || result.is_err() {
                    let _ = ready;
                }
            }
        "#;
        assert_eq!(
            rules(source),
            vec![
                RuleId::OptionSomeOrNone,
                RuleId::ResultOkOrErr,
                RuleId::PredicateOrNegation,
                RuleId::AssertEqIdentical,
            ]
        );
    }

    #[test]
    fn comment_and_string_lookalikes_are_not_assertions() {
        let source = r#"
            fn probe() {
                // assert!(value.is_some() || value.is_none());
                let _ = "assert!(value.is_some() || value.is_none())";
            }
        "#;
        assert!(rules(source).is_empty());
    }

    #[test]
    fn clone_method_oracles_are_false_negatives_independent_idents_stay_green() {
        let source = r#"
            fn probe(value: Flag, ready: bool) {
                // Method `.clone()` is a false-negative allowance, not a repair recipe.
                assert_eq!(value, value.clone());
                assert_eq!(Flag::On, Flag::On.clone());
                assert_eq!(Mode::Socket { port: 1 }, Mode::Socket { port: 1 }.clone());
                let left = Flag::On;
                let right = Flag::On;
                assert_eq!(left, right);
                debug_assert_eq!(ready, !ready);
            }
            #[derive(Clone, PartialEq, Debug)]
            enum Flag { On }
            #[derive(Clone, PartialEq, Debug)]
            enum Mode { Socket { port: u16 } }
        "#;
        assert!(rules(source).is_empty(), "{:?}", rules(source));
    }

    #[test]
    fn unparsable_source_is_an_instrument_error() {
        let error = scan_file("broken.rs", "fn oops( {").expect_err("must fail");
        assert!(!error.is_empty());
    }
}
