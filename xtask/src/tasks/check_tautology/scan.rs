//! Walk a parsed Rust file and collect tautological assertion macros.

use super::detect::{Detection, RuleId, classify_assert_condition_in, classify_assert_eq};
use super::expr::{
    PreludeShadow, QueryKind, TypeEnv, bind_binding_pat, bind_pat_type, ident_unraw,
    import_prefix_is_std_namespace, imported_path_is_std_enum_or_ctor, peel,
};
use syn::parse::{ParseStream, Parser};
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{
    BinOp, Expr, ExprClosure, ExprForLoop, ExprIf, ExprMacro, ExprWhile, File, Ident, ImplItemFn,
    Item, ItemFn, ItemImpl, ItemMacro, ItemMod, ItemTrait, Local, Macro, Signature, Stmt,
    StmtMacro, TraitItemFn, UseTree,
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
    let mut visitor = AssertionVisitor {
        path,
        findings: Vec::new(),
        env: Vec::new(),
        module_shadows: Vec::new(),
        crate_root_shadows: PreludeShadow::default(),
    };
    visitor.visit_file(file);
    visitor.findings.sort();
    visitor.findings
}

struct AssertionVisitor<'a> {
    path: &'a str,
    findings: Vec<Finding>,
    env: Vec<TypeEnv>,
    module_shadows: Vec<PreludeShadow>,
    crate_root_shadows: PreludeShadow,
}

impl AssertionVisitor<'_> {
    fn current_env(&self) -> TypeEnv {
        self.env.last().cloned().unwrap_or_else(|| {
            TypeEnv::with_module_and_crate_root(
                self.current_shadows(),
                self.crate_root_shadows.clone(),
            )
        })
    }

    fn current_shadows(&self) -> PreludeShadow {
        self.module_shadows.last().cloned().unwrap_or_default()
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

    /// Walk an `if`/`while` condition left-to-right so each `let` is in scope
    /// for later `&&` operands and for the then/body block, but not for `else`.
    fn visit_condition_with_lets(&mut self, expr: &Expr) {
        match peel(expr) {
            Expr::Let(expr_let) => {
                for attr in &expr_let.attrs {
                    self.visit_attribute(attr);
                }
                self.visit_expr(&expr_let.expr);
                self.bind_current(&expr_let.pat);
            }
            Expr::Binary(binary) if matches!(binary.op, BinOp::And(_)) => {
                self.visit_condition_with_lets(&binary.left);
                self.visit_condition_with_lets(&binary.right);
            }
            other => self.visit_expr(other),
        }
    }

    fn push_fn_env(&mut self, sig: &Signature) {
        let mut shadows = self.current_shadows();
        untrust_generic_params(&sig.generics, &mut shadows);
        let mut env = TypeEnv::with_module_and_crate_root(shadows, self.crate_root_shadows.clone());
        for input in &sig.inputs {
            if let syn::FnArg::Typed(typed) = input {
                bind_pat_type(&mut env, &typed.pat, &typed.ty);
            }
        }
        self.env.push(env);
    }

    fn push_generic_shadows(&mut self, generics: &syn::Generics) {
        let mut shadows = self.current_shadows();
        untrust_generic_params(generics, &mut shadows);
        self.module_shadows.push(shadows);
    }
}

impl<'ast> Visit<'ast> for AssertionVisitor<'_> {
    fn visit_file(&mut self, node: &'ast File) {
        let crate_root = collect_shadows(node.items.iter(), None);
        self.crate_root_shadows = crate_root.clone();
        self.module_shadows.push(crate_root);
        syn::visit::visit_file(self, node);
        self.module_shadows.pop();
    }

    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        if let Some((_, items)) = &node.content {
            self.module_shadows.push(collect_shadows(items.iter(), Some(&self.crate_root_shadows)));
            syn::visit::visit_item_mod(self, node);
            self.module_shadows.pop();
        }
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        self.push_generic_shadows(&node.generics);
        syn::visit::visit_item_impl(self, node);
        self.module_shadows.pop();
    }

    fn visit_item_trait(&mut self, node: &'ast ItemTrait) {
        self.push_generic_shadows(&node.generics);
        syn::visit::visit_item_trait(self, node);
        self.module_shadows.pop();
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        self.push_fn_env(&node.sig);
        syn::visit::visit_item_fn(self, node);
        self.env.pop();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
        self.push_fn_env(&node.sig);
        syn::visit::visit_impl_item_fn(self, node);
        self.env.pop();
    }

    fn visit_trait_item_fn(&mut self, node: &'ast TraitItemFn) {
        if let Some(block) = &node.default {
            self.push_fn_env(&node.sig);
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
        let extra = collect_shadows(
            node.stmts.iter().filter_map(|stmt| match stmt {
                Stmt::Item(item) => Some(item),
                _ => None,
            }),
            Some(&self.crate_root_shadows),
        );
        let applied = !extra.is_empty();
        let previous = if applied {
            let merged = self.current_shadows().merged(&extra);
            self.module_shadows.push(merged.clone());
            self.env.last_mut().map(|env| env.replace_shadows(merged))
        } else {
            None
        };
        self.push_scope();
        syn::visit::visit_block(self, node);
        self.pop_scope();
        if applied {
            if let (Some(env), Some(prev)) = (self.env.last_mut(), previous) {
                env.replace_shadows(prev);
            }
            self.module_shadows.pop();
        }
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
        self.push_scope();
        self.visit_condition_with_lets(&node.cond);
        self.visit_block(&node.then_branch);
        self.pop_scope();
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
        self.push_scope();
        self.visit_condition_with_lets(&node.cond);
        self.visit_block(&node.body);
        self.pop_scope();
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

fn collect_shadows<'a>(
    items: impl IntoIterator<Item = &'a Item>,
    crate_root: Option<&PreludeShadow>,
) -> PreludeShadow {
    let items: Vec<&Item> = items.into_iter().collect();
    let mut shadows = PreludeShadow::default();
    collect_namespace_shadows(&items, &mut shadows);
    let module_ns = shadows.clone();
    let crate_ns_owned = crate_root.cloned().unwrap_or_else(|| module_ns.clone());
    for item in &items {
        match item {
            Item::Struct(item) => note_ident(&item.ident, &mut shadows),
            Item::Enum(item) => note_ident(&item.ident, &mut shadows),
            Item::Union(item) => note_ident(&item.ident, &mut shadows),
            Item::Type(item) => note_ident(&item.ident, &mut shadows),
            Item::Trait(item) => note_ident(&item.ident, &mut shadows),
            Item::TraitAlias(item) => note_ident(&item.ident, &mut shadows),
            Item::Fn(item) => note_ctor_ident(&item.sig.ident, &mut shadows),
            Item::Mod(item) => note_ident(&item.ident, &mut shadows),
            Item::Const(item) => note_ctor_ident(&item.ident, &mut shadows),
            Item::Static(item) => note_ctor_ident(&item.ident, &mut shadows),
            _ => {}
        }
    }
    for item in &items {
        if let Item::Use(item_use) = item {
            collect_use(
                &item_use.tree,
                item_use.leading_colon.is_some(),
                &[],
                &mut shadows,
                &module_ns,
                &crate_ns_owned,
            );
        }
    }
    shadows
}

#[derive(Clone, Copy)]
enum AliasPass {
    /// Untrust bound `std`/`core` when the imported path is not the extern crate.
    NonRealSource,
    /// Untrust a syntactic `std`/`core` retarget when that source name is already
    /// untrusted (`mod std; use std as core`).
    RealSourceIfShadowed,
}

fn collect_namespace_shadows(items: &[&Item], shadows: &mut PreludeShadow) {
    for item in items {
        match item {
            Item::Mod(item_mod) => untrust_namespace_ident(&item_mod.ident, shadows),
            Item::ExternCrate(ext) => {
                if let Some((_, rename)) = &ext.rename {
                    untrust_namespace_alias(
                        &[],
                        &ext.ident,
                        rename,
                        shadows,
                        AliasPass::NonRealSource,
                    );
                }
            }
            Item::Use(item_use) => {
                collect_use_namespace_aliases(
                    &item_use.tree,
                    &[],
                    shadows,
                    AliasPass::NonRealSource,
                );
            }
            _ => {}
        }
    }
    for item in items {
        match item {
            Item::ExternCrate(ext) => {
                if let Some((_, rename)) = &ext.rename {
                    untrust_namespace_alias(
                        &[],
                        &ext.ident,
                        rename,
                        shadows,
                        AliasPass::RealSourceIfShadowed,
                    );
                }
            }
            Item::Use(item_use) => {
                collect_use_namespace_aliases(
                    &item_use.tree,
                    &[],
                    shadows,
                    AliasPass::RealSourceIfShadowed,
                );
            }
            _ => {}
        }
    }
}

fn collect_use_namespace_aliases(
    tree: &UseTree,
    prefix: &[String],
    shadows: &mut PreludeShadow,
    pass: AliasPass,
) {
    match tree {
        UseTree::Rename(rename) => {
            untrust_namespace_alias(prefix, &rename.ident, &rename.rename, shadows, pass);
        }
        UseTree::Path(path) => {
            let mut next = prefix.to_vec();
            next.push(ident_unraw(&path.ident));
            collect_use_namespace_aliases(&path.tree, &next, shadows, pass);
        }
        UseTree::Group(group) => {
            for item in &group.items {
                collect_use_namespace_aliases(item, prefix, shadows, pass);
            }
        }
        UseTree::Name(name) if matches!(pass, AliasPass::NonRealSource) => {
            if name.ident == "self" {
                // `use custom::std::{self}` binds `std`; `use std::{self}` is the real crate.
                if prefix.len() >= 2 {
                    if let Some(last) = prefix.last() {
                        shadows.untrust_namespace(last);
                    }
                }
            } else if !prefix.is_empty() {
                untrust_namespace_ident(&name.ident, shadows);
            }
        }
        UseTree::Name(_) | UseTree::Glob(_) => {}
    }
}

fn untrust_namespace_ident(ident: &Ident, shadows: &mut PreludeShadow) {
    let name = ident_unraw(ident);
    if matches!(name.as_str(), "std" | "core") {
        shadows.untrust_namespace(&name);
    }
}

/// `use std as core` stays trusted when `std` is the extern crate. `use custom as std`,
/// `use custom::std as std`, and `use std as core` after a local `mod std` untrust the bound name.
fn untrust_namespace_alias(
    prefix: &[String],
    ident: &Ident,
    rename: &Ident,
    shadows: &mut PreludeShadow,
    pass: AliasPass,
) {
    let bound = ident_unraw(rename);
    if !matches!(bound.as_str(), "std" | "core") {
        return;
    }
    let source = ident_unraw(ident);
    let real = imported_path_is_real_std_or_core_crate(prefix, &source);
    match pass {
        AliasPass::NonRealSource => {
            if !real {
                shadows.untrust_namespace(&bound);
            }
        }
        AliasPass::RealSourceIfShadowed => {
            if real {
                let source_ns = if source == "self" {
                    prefix.first().map(String::as_str).unwrap_or("")
                } else {
                    source.as_str()
                };
                if shadows.namespace_untrusted(source_ns) {
                    shadows.untrust_namespace(&bound);
                }
            }
        }
    }
}

fn imported_path_is_real_std_or_core_crate(prefix: &[String], ident: &str) -> bool {
    match prefix {
        [] => matches!(ident, "std" | "core"),
        [ns] if ident == "self" => matches!(ns.as_str(), "std" | "core"),
        _ => false,
    }
}

fn note_ident(ident: &Ident, shadows: &mut PreludeShadow) {
    match ident_unraw(ident).as_str() {
        "Option" => shadows.untrust_type(QueryKind::Option),
        "Result" => shadows.untrust_type(QueryKind::Result),
        "Some" | "None" | "Ok" | "Err" => shadows.untrust_ctor(ident_unraw(ident)),
        _ => {}
    }
}

fn note_ctor_ident(ident: &Ident, shadows: &mut PreludeShadow) {
    let name = ident_unraw(ident);
    if matches!(name.as_str(), "Some" | "None" | "Ok" | "Err") {
        shadows.untrust_ctor(name);
    }
}

fn untrust_generic_params(generics: &syn::Generics, shadows: &mut PreludeShadow) {
    for param in &generics.params {
        match param {
            syn::GenericParam::Type(ty) => note_ident(&ty.ident, shadows),
            syn::GenericParam::Const(c) => note_ctor_ident(&c.ident, shadows),
            syn::GenericParam::Lifetime(_) => {}
        }
    }
}

fn collect_use(
    tree: &UseTree,
    rooted: bool,
    prefix: &[String],
    shadows: &mut PreludeShadow,
    module_ns: &PreludeShadow,
    crate_ns: &PreludeShadow,
) {
    match tree {
        UseTree::Path(path) => {
            let mut next = prefix.to_vec();
            next.push(ident_unraw(&path.ident));
            collect_use(&path.tree, rooted, &next, shadows, module_ns, crate_ns);
        }
        UseTree::Name(name) => {
            let mut full = prefix.to_vec();
            full.push(ident_unraw(&name.ident));
            untrust_imported(
                rooted,
                &full,
                &ident_unraw(&name.ident),
                shadows,
                module_ns,
                crate_ns,
            );
        }
        UseTree::Rename(rename) => {
            let mut full = prefix.to_vec();
            full.push(ident_unraw(&rename.ident));
            untrust_imported(
                rooted,
                &full,
                &ident_unraw(&rename.rename),
                shadows,
                module_ns,
                crate_ns,
            );
        }
        UseTree::Glob(_) => {
            if !import_prefix_is_std_namespace(rooted, prefix, module_ns, crate_ns) {
                shadows.untrust_all_prelude();
            }
        }
        UseTree::Group(group) => {
            for tree in &group.items {
                collect_use(tree, rooted, prefix, shadows, module_ns, crate_ns);
            }
        }
    }
}

fn untrust_imported(
    rooted: bool,
    full: &[String],
    bound: &str,
    shadows: &mut PreludeShadow,
    module_ns: &PreludeShadow,
    crate_ns: &PreludeShadow,
) {
    if imported_path_is_std_enum_or_ctor(rooted, full, module_ns, crate_ns) {
        return;
    }
    match bound {
        "Option" => shadows.untrust_type(QueryKind::Option),
        "Result" => shadows.untrust_type(QueryKind::Result),
        "Some" | "None" | "Ok" | "Err" => shadows.untrust_ctor(bound),
        _ => {}
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
            fn probe(value: Option<u8>, result: Result<(), ()>) {
                assert!(value.is_some() || value.is_none());
                debug_assert!(result.is_ok() || result.is_err());
                assert!(true || !true, "still a tautology");
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
    fn let_chain_bindings_shadow_option_parameters() {
        let source = r#"
            fn probe(value: Option<u8>, probe: Probe) {
                if let _ready = true && let value = probe {
                    assert!(value.is_some() || value.is_none());
                } else {
                    assert!(value.is_some() || value.is_none());
                }
                while let _ready = true && let value = probe {
                    assert!(value.is_some() || value.is_none());
                    break;
                }
                assert!(value.is_some() || value.is_none());
            }
            struct Probe { n: u8 }
            impl Probe {
                fn is_some(&self) -> bool { false }
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
    fn let_chain_later_operands_see_earlier_shadowing_bindings() {
        let source = r#"
            fn probe(value: Option<u8>, probe: Probe) {
                if let value = probe && { assert!(value.is_some() || value.is_none()); true } {
                    let _ = 0;
                }
                while let value = probe && { assert!(value.is_some() || value.is_none()); true } {
                    break;
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
        assert!(rules(source).is_empty(), "{:?}", rules(source));
    }

    #[test]
    fn overloaded_not_and_neg_are_not_tautologies() {
        let source = r#"
            use std::cell::Cell;
            use std::ops::{Neg, Not};

            #[derive(Clone, Copy)]
            struct Flag(Cell<u8>);

            impl Not for Flag {
                type Output = bool;
                fn not(self) -> bool {
                    self.0.set(self.0.get().wrapping_add(1));
                    false
                }
            }

            impl Neg for Flag {
                type Output = bool;
                fn neg(self) -> bool {
                    self.0.set(self.0.get().wrapping_add(1));
                    false
                }
            }

            fn probe(x: Flag, ready: bool) {
                assert!(!x || !!x);
                assert!(-x || !-x);
                assert!(ready || !ready);
                assert!(true || !true);
            }
        "#;
        assert_eq!(rules(source), vec![RuleId::PredicateOrNegation], "{:?}", rules(source));
    }

    #[test]
    fn custom_option_result_ascriptions_are_skipped_std_option_ascription_is_retained() {
        let source = r#"
            fn skip_custom_option(value: custom::Option<u8>) {
                assert!(value.is_some() || value.is_none());
            }
            fn skip_custom_result(value: custom::Result<(), ()>) {
                assert!(value.is_ok() || value.is_err());
            }
            fn skip_qualified_custom_ctors() {
                assert!(custom::Option::Some(1).is_some() || custom::Option::Some(1).is_none());
                assert!(custom::Result::Ok(()).is_ok() || custom::Result::Ok(()).is_err());
            }
            fn retain_std_option(value: std::option::Option<u8>) {
                assert!(value.is_some() || value.is_none());
            }
            fn retain_core_option(value: core::option::Option<u8>) {
                assert!(value.is_some() || value.is_none());
            }
            mod custom {
                pub struct Option<T>(T);
                impl<T> Option<T> {
                    #[allow(non_snake_case)]
                    pub fn Some(value: T) -> Self { Self(value) }
                    pub fn is_some(&self) -> bool { false }
                    pub fn is_none(&self) -> bool { false }
                }
                pub struct Result<T, E>(core::marker::PhantomData<(T, E)>);
                impl<T, E> Result<T, E> {
                    #[allow(non_snake_case)]
                    pub fn Ok(_value: T) -> Self { Self(core::marker::PhantomData) }
                    pub fn is_ok(&self) -> bool { false }
                    pub fn is_err(&self) -> bool { false }
                }
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
    fn local_option_type_import_and_ctor_shadows_are_skipped_std_option_is_retained() {
        let source = r#"
            struct Option;
            impl Option {
                fn is_some(&self) -> bool { false }
                fn is_none(&self) -> bool { false }
            }
            fn skip_local_option(x: Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_std_option(value: std::option::Option<u8>) {
                assert!(value.is_some() || value.is_none());
            }
            mod inner {
                fn retain_prelude_in_child(value: Option<u8>) {
                    assert!(value.is_some() || value.is_none());
                }
            }
        "#;
        assert_eq!(
            rules(source),
            vec![RuleId::OptionSomeOrNone, RuleId::OptionSomeOrNone],
            "{:?}",
            rules(source)
        );

        let imported = r#"
            use custom::Option;
            fn skip_imported(x: Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_std(value: std::option::Option<u8>) {
                assert!(value.is_some() || value.is_none());
            }
            mod custom {
                pub struct Option;
            }
        "#;
        assert_eq!(rules(imported), vec![RuleId::OptionSomeOrNone], "{:?}", rules(imported));

        let ctor = r#"
            struct Some(u8);
            impl Some {
                fn is_some(&self) -> bool { false }
                fn is_none(&self) -> bool { false }
            }
            fn skip_local_some() {
                assert!(Some(1).is_some() || Some(1).is_none());
            }
            fn retain_std() {
                assert!(
                    std::option::Option::Some(1).is_some()
                        || std::option::Option::Some(1).is_none()
                );
            }
        "#;
        assert_eq!(rules(ctor), vec![RuleId::OptionSomeOrNone], "{:?}", rules(ctor));

        let local_result = r#"
            struct Result;
            impl Result {
                fn is_ok(&self) -> bool { false }
                fn is_err(&self) -> bool { false }
            }
            fn skip_local_result(x: Result) {
                assert!(x.is_ok() || x.is_err());
            }
            fn retain_std(value: std::result::Result<(), ()>) {
                assert!(value.is_ok() || value.is_err());
            }
        "#;
        assert_eq!(rules(local_result), vec![RuleId::ResultOkOrErr], "{:?}", rules(local_result));
    }

    #[test]
    fn generic_option_params_are_skipped_std_option_is_retained() {
        let source = r#"
            trait Query {
                fn is_some(&self) -> bool;
                fn is_none(&self) -> bool;
            }
            fn check<Option: Query>(x: Option) {
                assert!(x.is_some() || x.is_none());
            }
            struct Holder;
            impl<Option: Query> Holder {
                fn check(x: Option) {
                    assert!(x.is_some() || x.is_none());
                }
            }
            trait HolderTrait<Option: Query> {
                fn check(x: Option) {
                    assert!(x.is_some() || x.is_none());
                }
            }
            fn retain(value: std::option::Option<u8>) {
                assert!(value.is_some() || value.is_none());
            }
        "#;
        assert_eq!(rules(source), vec![RuleId::OptionSomeOrNone], "{:?}", rules(source));
    }

    #[test]
    fn module_and_value_ctor_shadows_are_skipped_std_option_is_retained() {
        let source = r#"
            struct Probe;
            impl Probe {
                fn is_some(&self) -> bool { false }
                fn is_none(&self) -> bool { false }
            }
            #[allow(non_snake_case)]
            mod Option {
                pub fn Some(_value: u8) -> super::Probe { super::Probe }
            }
            fn skip_module_ctor() {
                assert!(Option::Some(1).is_some() || Option::Some(1).is_none());
            }
            fn retain_std() {
                assert!(
                    std::option::Option::Some(1).is_some()
                        || std::option::Option::Some(1).is_none()
                );
            }
        "#;
        assert_eq!(rules(source), vec![RuleId::OptionSomeOrNone], "{:?}", rules(source));

        let const_ctor = r#"
            struct Probe;
            impl Probe {
                fn is_some(&self) -> bool { false }
                fn is_none(&self) -> bool { false }
            }
            #[allow(non_upper_case_globals)]
            const Some: fn(u8) -> Probe = |_| Probe;
            fn skip_const_ctor() {
                assert!(Some(1).is_some() || Some(1).is_none());
            }
            fn retain_std() {
                assert!(
                    std::option::Option::Some(1).is_some()
                        || std::option::Option::Some(1).is_none()
                );
            }
        "#;
        assert_eq!(rules(const_ctor), vec![RuleId::OptionSomeOrNone], "{:?}", rules(const_ctor));

        let let_ctor = r#"
            struct Probe;
            impl Probe {
                fn is_some(&self) -> bool { false }
                fn is_none(&self) -> bool { false }
            }
            fn skip_let_ctor() {
                #[allow(non_snake_case)]
                let Some = |_: u8| Probe;
                assert!(Some(1).is_some() || Some(1).is_none());
            }
            fn retain_std() {
                assert!(
                    std::option::Option::Some(1).is_some()
                        || std::option::Option::Some(1).is_none()
                );
            }
        "#;
        assert_eq!(rules(let_ctor), vec![RuleId::OptionSomeOrNone], "{:?}", rules(let_ctor));
    }

    #[test]
    fn parent_item_shadows_are_not_in_scope_in_child_modules() {
        let source = r#"
            struct Option<T>(T);
            impl<T> Option<T> {
                fn is_some(&self) -> bool { false }
                fn is_none(&self) -> bool { false }
            }
            fn skip_parent(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_std_parent(value: std::option::Option<u8>) {
                assert!(value.is_some() || value.is_none());
            }
            mod child {
                fn retain_prelude(value: Option<u8>) {
                    assert!(value.is_some() || value.is_none());
                }
                fn retain_std(value: std::option::Option<u8>) {
                    assert!(value.is_some() || value.is_none());
                }
            }
            mod imported {
                use super::Option;
                fn skip_imported(x: Option<u8>) {
                    assert!(x.is_some() || x.is_none());
                }
                fn retain_std(value: std::option::Option<u8>) {
                    assert!(value.is_some() || value.is_none());
                }
            }
        "#;
        assert_eq!(
            rules(source),
            vec![
                RuleId::OptionSomeOrNone,
                RuleId::OptionSomeOrNone,
                RuleId::OptionSomeOrNone,
                RuleId::OptionSomeOrNone,
            ],
            "{:?}",
            rules(source)
        );
    }

    #[test]
    fn local_std_core_namespace_aliases_are_skipped_real_std_is_retained() {
        let source = r#"
            mod std {
                pub mod option {
                    pub struct Option;
                    impl Option {
                        pub fn is_some(&self) -> bool { false }
                        pub fn is_none(&self) -> bool { false }
                    }
                }
            }
            fn skip_aliased_std(x: std::option::Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn skip_global_std(x: ::std::option::Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
            mod child {
                fn retain_extern_std(x: std::option::Option<u8>) {
                    assert!(x.is_some() || x.is_none());
                }
            }
        "#;
        assert_eq!(
            rules(source),
            vec![RuleId::OptionSomeOrNone, RuleId::OptionSomeOrNone],
            "{:?}",
            rules(source)
        );

        let core_alias = r#"
            mod custom {
                pub mod option {
                    pub struct Option;
                    impl Option {
                        pub fn is_some(&self) -> bool { false }
                        pub fn is_none(&self) -> bool { false }
                    }
                }
            }
            use custom as core;
            fn skip_core_alias(x: core::option::Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(rules(core_alias), vec![RuleId::OptionSomeOrNone], "{:?}", rules(core_alias));

        let extern_alias = r#"
            extern crate custom as std;
            fn skip_extern_std(x: std::option::Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(
            rules(extern_alias),
            vec![RuleId::OptionSomeOrNone],
            "{:?}",
            rules(extern_alias)
        );

        let imported_std = r#"
            mod custom {
                pub mod std {
                    pub mod option {
                        pub struct Option;
                        impl Option {
                            pub fn is_some(&self) -> bool { false }
                            pub fn is_none(&self) -> bool { false }
                        }
                    }
                }
            }
            use custom::std;
            fn skip_imported_std(x: std::option::Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(
            rules(imported_std),
            vec![RuleId::OptionSomeOrNone],
            "{:?}",
            rules(imported_std)
        );

        let raw_std = r#"
            mod custom {
                pub mod r#std {
                    pub mod option {
                        pub struct Option;
                        impl Option {
                            pub fn is_some(&self) -> bool { false }
                            pub fn is_none(&self) -> bool { false }
                        }
                    }
                }
            }
            use custom::r#std;
            fn skip_raw_std(x: std::option::Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(rules(raw_std), vec![RuleId::OptionSomeOrNone], "{:?}", rules(raw_std));

        let grouped_self = r#"
            mod custom {
                pub mod std {
                    pub mod option {
                        pub struct Option;
                        impl Option {
                            pub fn is_some(&self) -> bool { false }
                            pub fn is_none(&self) -> bool { false }
                        }
                    }
                }
            }
            use custom::std::{self};
            fn skip_grouped_self(x: std::option::Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(
            rules(grouped_self),
            vec![RuleId::OptionSomeOrNone],
            "{:?}",
            rules(grouped_self)
        );

        let same_name = r#"
            mod custom {
                pub mod std {
                    pub mod option {
                        pub struct Option;
                        impl Option {
                            pub fn is_some(&self) -> bool { false }
                            pub fn is_none(&self) -> bool { false }
                        }
                    }
                }
            }
            use custom::std as std;
            fn skip_same_name(x: std::option::Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(rules(same_name), vec![RuleId::OptionSomeOrNone], "{:?}", rules(same_name));

        let grouped_same_name = r#"
            mod custom {
                pub mod std {
                    pub mod option {
                        pub struct Option;
                        impl Option {
                            pub fn is_some(&self) -> bool { false }
                            pub fn is_none(&self) -> bool { false }
                        }
                    }
                }
            }
            use custom::{std as std};
            fn skip_grouped_same_name(x: std::option::Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(
            rules(grouped_same_name),
            vec![RuleId::OptionSomeOrNone],
            "{:?}",
            rules(grouped_same_name)
        );

        let same_name_core = r#"
            mod custom {
                pub mod core {
                    pub mod option {
                        pub struct Option;
                        impl Option {
                            pub fn is_some(&self) -> bool { false }
                            pub fn is_none(&self) -> bool { false }
                        }
                    }
                }
            }
            use custom::core as core;
            fn skip_same_name_core(x: core::option::Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(
            rules(same_name_core),
            vec![RuleId::OptionSomeOrNone],
            "{:?}",
            rules(same_name_core)
        );

        let std_as_core_after_mod = r#"
            mod std {
                pub mod option {
                    pub struct Option;
                    impl Option {
                        pub fn is_some(&self) -> bool { false }
                        pub fn is_none(&self) -> bool { false }
                    }
                }
            }
            use std as core;
            fn skip_std_as_core(x: core::option::Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(
            rules(std_as_core_after_mod),
            vec![RuleId::OptionSomeOrNone],
            "{:?}",
            rules(std_as_core_after_mod)
        );

        let grouped_std_as_core = r#"
            mod std {
                pub mod option {
                    pub struct Option;
                    impl Option {
                        pub fn is_some(&self) -> bool { false }
                        pub fn is_none(&self) -> bool { false }
                    }
                }
            }
            use std::{self as core};
            fn skip_grouped_std_as_core(x: core::option::Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(
            rules(grouped_std_as_core),
            vec![RuleId::OptionSomeOrNone],
            "{:?}",
            rules(grouped_std_as_core)
        );

        let chained = r#"
            mod custom {
                pub mod option {
                    pub struct Option;
                    impl Option {
                        pub fn is_some(&self) -> bool { false }
                        pub fn is_none(&self) -> bool { false }
                    }
                }
            }
            use custom as std;
            use std as core;
            fn skip_chained(x: core::option::Option) {
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(rules(chained), vec![RuleId::OptionSomeOrNone], "{:?}", rules(chained));

        let real_std_as_core = r#"
            use std as core;
            fn retain_real_core_alias(x: core::option::Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(
            rules(real_std_as_core),
            vec![RuleId::OptionSomeOrNone],
            "{:?}",
            rules(real_std_as_core)
        );

        let real_std = r#"
            fn retain_std(x: std::option::Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(rules(real_std), vec![RuleId::OptionSomeOrNone], "{:?}", rules(real_std));
    }

    #[test]
    fn block_local_option_type_is_skipped_outer_prelude_is_retained() {
        let source = r#"
            fn check() {
                struct Option;
                impl Option {
                    fn is_some(&self) -> bool { false }
                    fn is_none(&self) -> bool { false }
                }
                let x: Option = Option;
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
        "#;
        assert_eq!(rules(source), vec![RuleId::OptionSomeOrNone], "{:?}", rules(source));

        let nested = r#"
            fn check() {
                {
                    struct Option;
                    impl Option {
                        fn is_some(&self) -> bool { false }
                        fn is_none(&self) -> bool { false }
                    }
                    let x: Option = Option;
                    assert!(x.is_some() || x.is_none());
                }
                let y: Option<u8> = None;
                assert!(y.is_some() || y.is_none());
            }
        "#;
        assert_eq!(rules(nested), vec![RuleId::OptionSomeOrNone], "{:?}", rules(nested));

        let imported = r#"
            fn check() {
                use custom::Option;
                let x: Option = Option;
                assert!(x.is_some() || x.is_none());
            }
            fn retain_prelude(x: Option<u8>) {
                assert!(x.is_some() || x.is_none());
            }
            mod custom {
                pub struct Option;
                impl Option {
                    fn is_some(&self) -> bool { false }
                    fn is_none(&self) -> bool { false }
                }
            }
        "#;
        assert_eq!(rules(imported), vec![RuleId::OptionSomeOrNone], "{:?}", rules(imported));
    }

    #[test]
    fn pointer_casts_are_skipped_numeric_casts_are_retained() {
        let source = r#"
            fn probe() {
                assert_eq!(&1 as *const i32, &1 as *const i32);
                assert_eq!(&1 as *mut i32, &1 as *mut i32);
                assert_eq!(1 as i32, 1 as i32);
                assert_eq!(1, 1);
            }
        "#;
        assert_eq!(
            rules(source),
            vec![RuleId::AssertEqIdentical, RuleId::AssertEqIdentical],
            "{:?}",
            rules(source)
        );
    }
}
