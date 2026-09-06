//! Walk a parsed Rust file and collect tautological assertion macros.

use super::detect::{Detection, RuleId, classify_assert_condition_in, classify_assert_eq};
use super::expr::{TypeEnv, bind_binding_pat, bind_pat_type};
use syn::parse::{ParseStream, Parser};
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{
    Expr, ExprClosure, ExprForLoop, ExprIf, ExprMacro, ExprWhile, File, ImplItemFn, ItemFn,
    ItemMacro, Local, Macro, StmtMacro, TraitItemFn,
};

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
    let mut visitor = AssertionVisitor { path, findings: Vec::new(), env: Vec::new() };
    visitor.visit_file(file);
    visitor.findings.sort();
    visitor.findings
}

struct AssertionVisitor<'a> {
    path: &'a str,
    findings: Vec<Finding>,
    env: Vec<TypeEnv>,
}

impl AssertionVisitor<'_> {
    fn current_env(&self) -> TypeEnv {
        self.env.last().cloned().unwrap_or_default()
    }

    fn push_scope(&mut self) {
        if let Some(env) = self.env.last_mut() {
            env.push_scope();
        }
    }

    fn pop_scope(&mut self) {
        if let Some(env) = self.env.last_mut() {
            env.pop_scope();
        }
    }

    fn bind_current(&mut self, pat: &syn::Pat) {
        if let Some(env) = self.env.last_mut() {
            bind_binding_pat(env, pat);
        }
    }

    fn push_fn_env(&mut self, inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>) {
        let mut env = TypeEnv::new();
        for input in inputs {
            if let syn::FnArg::Typed(typed) = input {
                bind_pat_type(&mut env, &typed.pat, &typed.ty);
            }
        }
        self.env.push(env);
    }
}

impl<'ast> Visit<'ast> for AssertionVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        self.push_fn_env(&node.sig.inputs);
        syn::visit::visit_item_fn(self, node);
        self.env.pop();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
        self.push_fn_env(&node.sig.inputs);
        syn::visit::visit_impl_item_fn(self, node);
        self.env.pop();
    }

    fn visit_trait_item_fn(&mut self, node: &'ast TraitItemFn) {
        if let Some(block) = &node.default {
            self.push_fn_env(&node.sig.inputs);
            syn::visit::visit_block(self, block);
            self.env.pop();
        }
    }

    fn visit_expr_closure(&mut self, node: &'ast ExprClosure) {
        let mut env = self.current_env();
        env.push_scope();
        for input in &node.inputs {
            bind_binding_pat(&mut env, input);
        }
        self.env.push(env);
        syn::visit::visit_expr_closure(self, node);
        self.env.pop();
    }

    fn visit_block(&mut self, node: &'ast syn::Block) {
        self.push_scope();
        syn::visit::visit_block(self, node);
        self.pop_scope();
    }

    fn visit_local(&mut self, node: &'ast Local) {
        syn::visit::visit_local(self, node);
        self.bind_current(&node.pat);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast ExprForLoop) {
        for attr in &node.attrs {
            self.visit_attribute(attr);
        }
        if let Some(label) = &node.label {
            self.visit_label(label);
        }
        self.visit_expr(&node.expr);
        self.push_scope();
        self.bind_current(&node.pat);
        self.visit_block(&node.body);
        self.pop_scope();
    }

    fn visit_expr_if(&mut self, node: &'ast ExprIf) {
        for attr in &node.attrs {
            self.visit_attribute(attr);
        }
        match &*node.cond {
            Expr::Let(expr_let) => {
                self.visit_expr(&expr_let.expr);
                self.push_scope();
                self.bind_current(&expr_let.pat);
                self.visit_block(&node.then_branch);
                self.pop_scope();
            }
            other => {
                self.visit_expr(other);
                self.visit_block(&node.then_branch);
            }
        }
        if let Some((_, else_expr)) = &node.else_branch {
            self.visit_expr(else_expr);
        }
    }

    fn visit_expr_while(&mut self, node: &'ast ExprWhile) {
        for attr in &node.attrs {
            self.visit_attribute(attr);
        }
        if let Some(label) = &node.label {
            self.visit_label(label);
        }
        match &*node.cond {
            Expr::Let(expr_let) => {
                self.visit_expr(&expr_let.expr);
                self.push_scope();
                self.bind_current(&expr_let.pat);
                self.visit_block(&node.body);
                self.pop_scope();
            }
            other => {
                self.visit_expr(other);
                self.visit_block(&node.body);
            }
        }
    }

    fn visit_arm(&mut self, node: &'ast syn::Arm) {
        self.push_scope();
        self.bind_current(&node.pat);
        syn::visit::visit_arm(self, node);
        self.pop_scope();
    }

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
                self.push(classify_assert_condition_in(&expr, &self.current_env()), mac);
            }
            return;
        }
        if ASSERT_EQ_MACROS.iter().any(|candidate| name == *candidate)
            && let Some((left, right)) = parse_assert_eq_args(mac.tokens.clone())
        {
            self.push(classify_assert_eq(&left, &right), mac);
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
                assert_eq!(1, 1);
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

    #[test]
    fn scan_skips_stateful_receivers_and_non_reflexive_eq_but_keeps_path_tautologies() {
        let source = r#"
            fn probe(value: Option<u8>, mut probe: Probe) {
                assert!(value.is_some() || value.is_none());
                assert!(counter().is_some() || counter().is_none());
                assert!(probe.is_some() || probe.is_none());
                assert_eq!(f32::NAN, f32::NAN);
                assert_eq!(1, 1);
                assert_eq!(1.0, 1.0);
            }
            struct Probe { n: u8 }
            impl Probe {
                fn is_some(&mut self) -> bool { self.n += 1; false }
                fn is_none(&self) -> bool { false }
            }
            fn counter() -> Option<u8> { None }
        "#;
        assert_eq!(
            rules(source),
            vec![RuleId::OptionSomeOrNone, RuleId::AssertEqIdentical, RuleId::AssertEqIdentical]
        );
    }

    #[test]
    fn option_parameter_shadowed_by_untyped_or_custom_binding_is_skipped() {
        let source = r#"
            fn probe(value: Option<u8>, mut probe: Probe) {
                assert!(value.is_some() || value.is_none());
                let value = probe;
                assert!(value.is_some() || value.is_none());
                let value: Probe = Probe { n: 0 };
                assert!(value.is_some() || value.is_none());
            }
            fn restore(value: Option<u8>) {
                {
                    let value = Probe { n: 0 };
                    assert!(value.is_some() || value.is_none());
                }
                assert!(value.is_some() || value.is_none());
            }
            struct Probe { n: u8 }
            impl Probe {
                fn is_some(&mut self) -> bool { self.n += 1; false }
                fn is_none(&self) -> bool { false }
            }
        "#;
        assert_eq!(
            rules(source),
            vec![RuleId::OptionSomeOrNone, RuleId::OptionSomeOrNone],
            "{:?}",
            rules(source)
        );
    }

    #[test]
    fn block_local_option_does_not_classify_outer_custom_binding() {
        let source = r#"
            fn probe(value: Probe) {
                {
                    let value: Option<u8> = None;
                    assert!(value.is_some() || value.is_none());
                }
                assert!(value.is_some() || value.is_none());
            }
            struct Probe { n: u8 }
            impl Probe {
                fn is_some(&self) -> bool { false }
                fn is_none(&self) -> bool { false }
            }
        "#;
        assert_eq!(rules(source), vec![RuleId::OptionSomeOrNone], "{:?}", rules(source));
    }

    #[test]
    fn for_if_let_while_let_and_match_bindings_shadow_option_parameters() {
        let source = r#"
            fn probe(value: Option<u8>, probes: [Probe; 1], probe: Probe) {
                for value in probes {
                    assert!(value.is_some() || value.is_none());
                }
                if let value = probe {
                    assert!(value.is_some() || value.is_none());
                }
                while let value = probe {
                    assert!(value.is_some() || value.is_none());
                    break;
                }
                match probe {
                    value => assert!(value.is_some() || value.is_none()),
                }
                assert!(value.is_some() || value.is_none());
            }
            struct Probe { n: u8 }
            impl Probe {
                fn is_some(&self) -> bool { false }
                fn is_none(&self) -> bool { false }
            }
        "#;
        assert_eq!(rules(source), vec![RuleId::OptionSomeOrNone], "{:?}", rules(source));
    }

    #[test]
    fn closure_inherits_option_ascription_unless_parameter_shadows() {
        let source = r#"
            fn capture(value: Option<u8>) {
                let _f = || assert!(value.is_some() || value.is_none());
            }
            fn shadow_capture(value: Option<u8>) {
                let _f = |value: Probe| assert!(value.is_some() || value.is_none());
            }
            struct Probe { n: u8 }
            impl Probe {
                fn is_some(&self) -> bool { false }
                fn is_none(&self) -> bool { false }
            }
        "#;
        assert_eq!(rules(source), vec![RuleId::OptionSomeOrNone], "{:?}", rules(source));
    }

    #[test]
    fn deref_and_field_predicates_are_not_tautologies() {
        let source = r#"
            fn probe(value: Toggle, item: Toggle, ready: bool) {
                assert!(*value || !*value);
                assert!(item.flag || !item.flag);
                assert!(ready || !ready);
            }
            struct Toggle { flag: bool }
        "#;
        assert_eq!(rules(source), vec![RuleId::PredicateOrNegation], "{:?}", rules(source));
    }
}
