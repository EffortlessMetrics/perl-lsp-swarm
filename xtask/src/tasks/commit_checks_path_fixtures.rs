//! Exact source spans of audited, pure path-detector test inputs.
//!
//! Unknown owners, indirect inputs, opaque macros and invalid Rust fail closed.
use std::ops::Range;
use syn::{Expr, Lit, parse::Parser, punctuated::Punctuated, spanned::Spanned, visit::Visit};

const HYGIENE: &str = "crates/perl-ci-hygiene/src/main.rs";
const CHECKS: &str = "xtask/src/tasks/commit_checks.rs";
const SELF: &str = "xtask/src/tasks/commit_checks_path_fixtures.rs";

pub(super) fn fixture_spans(path: &str, source: &str) -> Vec<Range<usize>> {
    if !matches!(path, HYGIENE | CHECKS | SELF) {
        return Vec::new();
    }
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let mut hazards = Hazards { depth: 0, found: false };
    hazards.visit_file(&file);
    if hazards.found {
        return Vec::new();
    }
    let mut visitor = Fixtures { path, test: false, asserted: false, spans: Vec::new() };
    visitor.visit_file(&file);
    // Audited comment examples have abstract ellipsis components, never concrete tails.
    // Token spans exclude string contents (including raw strings containing //).
    if (path == HYGIENE || path == CHECKS)
        && let Ok(tokens) = source.parse::<proc_macro2::TokenStream>()
    {
        let mut literals = Vec::new();
        literal_spans(tokens, &mut literals);
        let mut offset = 0;
        for line in source.split_inclusive('\n') {
            if let Some(start) = line.find("//") {
                let comment = offset + start;
                if !literals.iter().any(|r| r.contains(&comment)) {
                    let comment_text = &line[start..];
                    if path == CHECKS {
                        // Backticks delimit the exact abstract examples in the
                        // regex documentation. Concrete suffixes are not examples.
                        let ticks: Vec<_> =
                            comment_text.match_indices('`').map(|(i, _)| i).collect();
                        for pair in ticks.chunks_exact(2) {
                            let token = &comment_text[pair[0] + 1..pair[1]];
                            let parts: Vec<_> = token.split('\\').collect();
                            if matches!(
                                parts.as_slice(),
                                ["c:", "users", "..."]
                                    | ["C:", "Users", "..."]
                                    | ["C:", "Users", "John Doe", "..."]
                            ) {
                                let candidate = comment + pair[0]..comment + pair[1] + 1;
                                if !literals.iter().any(|literal| {
                                    literal.start < candidate.end && candidate.start < literal.end
                                }) {
                                    visitor.spans.push(candidate);
                                }
                            }
                        }
                    } else {
                        // The hygiene comment uses an ellipsis username followed
                        // by a concrete platform tail. Mask only that prefix.
                        for word in comment_text.split_whitespace() {
                            let mut parts = word.split('\\');
                            if matches!(
                                (parts.next(), parts.next(), parts.next(), parts.next()),
                                (Some("C:"), Some("Users"), Some("..."), Some(_))
                            ) {
                                let prefix_len = 2 + 1 + 5 + 1 + 3 + 1;
                                for (index, _) in comment_text.match_indices(word) {
                                    let candidate = comment + index..comment + index + prefix_len;
                                    if !literals.iter().any(|literal| {
                                        literal.start < candidate.end
                                            && candidate.start < literal.end
                                    }) {
                                        visitor.spans.push(candidate);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            offset += line.len();
        }
    }
    visitor.spans
}

fn literal_spans(tokens: proc_macro2::TokenStream, spans: &mut Vec<Range<usize>>) {
    for token in tokens {
        match token {
            proc_macro2::TokenTree::Literal(lit) => spans.push(lit.span().byte_range()),
            proc_macro2::TokenTree::Group(group) => literal_spans(group.stream(), spans),
            _ => {}
        }
    }
}

struct Fixtures<'a> {
    path: &'a str,
    test: bool,
    asserted: bool,
    spans: Vec<Range<usize>>,
}
impl Fixtures<'_> {
    fn literal(&mut self, expr: &Expr) {
        if let Expr::Lit(lit) = expr
            && matches!(lit.lit, Lit::Str(_))
        {
            self.spans.push(lit.span().byte_range());
        }
    }
}
fn protected(name: &syn::Ident) -> bool {
    [
        "has_machine_specific_home_path",
        "has_machine_specific_users_path",
        "evaluate_forbidden_paths",
        "fixture_spans",
        "assert",
        "assert_eq",
        "assert_ne",
    ]
    .iter()
    .any(|candidate| name == *candidate)
}
fn safe_import(tree: &syn::UseTree) -> bool {
    match tree {
        syn::UseTree::Path(path) => safe_import(&path.tree),
        syn::UseTree::Name(name) => !protected(&name.ident),
        syn::UseTree::Rename(rename) => !protected(&rename.rename),
        syn::UseTree::Group(group) => group.items.iter().all(safe_import),
        syn::UseTree::Glob(_) => false,
    }
}
// This is an intentionally conservative hazard scan, not a Rust name resolver.
// Only canonical root definitions and the existing exact parent glob are audited.
struct Hazards {
    depth: usize,
    found: bool,
}
impl<'ast> Visit<'ast> for Hazards {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if self.depth != 0 && protected(&item.sig.ident) {
            self.found = true;
        }
        self.depth += 1;
        syn::visit::visit_item_fn(self, item);
        self.depth -= 1;
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        self.depth += 1;
        syn::visit::visit_item_mod(self, item);
        self.depth -= 1;
    }
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let parent_glob = item.leading_colon.is_none()
            && matches!(&item.tree,
            syn::UseTree::Path(path) if path.ident == "super" && matches!(&*path.tree, syn::UseTree::Glob(_)));
        if !parent_glob && !safe_import(&item.tree) {
            self.found = true;
        }
    }
    fn visit_item_foreign_mod(&mut self, _: &'ast syn::ItemForeignMod) {
        // Foreign bindings have unsupported resolution; do not infer purity.
        self.found = true;
    }
    fn visit_item_macro(&mut self, _: &'ast syn::ItemMacro) {
        self.found = true;
    }
    fn visit_pat_ident(&mut self, item: &'ast syn::PatIdent) {
        if protected(&item.ident) {
            self.found = true;
        }
        syn::visit::visit_pat_ident(self, item);
    }
    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        if protected(&item.ident) {
            self.found = true;
        }
        syn::visit::visit_item_const(self, item);
    }
    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        if protected(&item.ident) {
            self.found = true;
        }
        syn::visit::visit_item_static(self, item);
    }
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        if protected(&item.ident) {
            self.found = true;
        }
        syn::visit::visit_item_struct(self, item);
    }
    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        if protected(item.rename.as_ref().map_or(&item.ident, |(_, ident)| ident)) {
            self.found = true;
        }
    }
}
impl<'ast> Visit<'ast> for Fixtures<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let previous = self.test;
        self.test = node.attrs.iter().any(|attr| attr.path().is_ident("test"));
        syn::visit::visit_item_fn(self, node);
        self.test = previous;
    }
    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if !self.test
            || !["assert", "assert_eq", "assert_ne"].iter().any(|name| node.path.is_ident(name))
        {
            return;
        }
        if let Ok(args) =
            Punctuated::<Expr, syn::Token![,]>::parse_terminated.parse2(node.tokens.clone())
        {
            let previous = self.asserted;
            self.asserted = true;
            // Only assertion operands, never diagnostic formatting arguments.
            let count = if node.path.is_ident("assert") { 1 } else { 2 };
            for arg in args.iter().take(count) {
                self.visit_expr(arg);
            }
            self.asserted = previous;
        }
    }
    fn visit_expr_closure(&mut self, _node: &'ast syn::ExprClosure) {
        // Deferred executable bodies are not direct assertion operands.
    }
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if self.test
            && let Expr::Path(callee) = &*node.func
        {
            if self.asserted
                && self.path == HYGIENE
                && ["has_machine_specific_home_path", "has_machine_specific_users_path"]
                    .iter()
                    .any(|name| callee.path.is_ident(name))
                && let Some(arg) = node.args.first()
            {
                self.literal(arg);
            }
            if self.asserted
                && self.path == SELF
                && callee.path.is_ident("fixture_spans")
                && let Some(arg) = node.args.iter().nth(1)
            {
                self.literal(arg);
            }
            if self.path == CHECKS
                && callee.path.is_ident("evaluate_forbidden_paths")
                && let Some(Expr::MethodCall(iter)) = node.args.iter().nth(1)
                && iter.method == "into_iter"
                && iter.args.is_empty()
                && let Expr::Array(array) = &*iter.receiver
            {
                for element in &array.elems {
                    if let Expr::Tuple(tuple) = element
                        && tuple.elems.len() == 2
                        && tuple
                            .elems
                            .iter()
                            .all(|e| matches!(e, Expr::Lit(l) if matches!(l.lit, Lit::Str(_))))
                        && let Some(input) = tuple.elems.iter().nth(1)
                    {
                        self.literal(input);
                    }
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_direct_asserted_inputs_in_known_owner() {
        assert_eq!(fixture_spans(HYGIENE, r##"#[test] fn t() { assert!(has_machine_specific_home_path("/home/alice/work", &re)); let leak = "/home/bob/work"; }"##).len(), 1);
        assert!(fixture_spans("src/lib.rs", r##"#[test] fn t() { assert!(has_machine_specific_home_path("/home/alice/work", &re)); }"##).is_empty());
        assert!(
            fixture_spans(
                HYGIENE,
                r##"fn t() { assert!(has_machine_specific_home_path("/home/alice/work", &re)); }"##
            )
            .is_empty()
        );
        assert!(fixture_spans(HYGIENE, r##"#[test] fn t() { let p = "/home/alice/work"; assert!(has_machine_specific_home_path(p, &re)); opaque!("/home/bob/work"); }"##).is_empty());
        assert!(fixture_spans(HYGIENE, r##"#[test] fn t() { assert!(std::path::Path::new("/home/alice/work").exists()); }"##).is_empty());
        assert!(fixture_spans(HYGIENE, r##"#[test] fn broken( { "/home/alice/work" "##).is_empty());
    }
    #[test]
    fn exact_ranges_leave_neighboring_leaks_visible() {
        assert_eq!(fixture_spans(HYGIENE, r##"#[test] fn t() { assert!(has_machine_specific_home_path("/home/alice/work", &re)); let leak = "/home/bob/work"; }"##).iter().map(|r| r.len()).collect::<Vec<_>>(), vec![18]);
        assert!(fixture_spans(HYGIENE, r##"#[test] fn t() { let has_machine_specific_home_path = other; assert!(has_machine_specific_home_path("/home/alice/work", &re)); }"##).is_empty());
        assert!(fixture_spans(HYGIENE, r##"#[test] fn t() { let p = "/home/alice/work"; let alias = p; let closure = || p; let s = Thing { p }; let f = format!("{p}"); assert!(has_machine_specific_home_path(alias, &re)); }"##).is_empty());
        assert!(fixture_spans(HYGIENE, r##"#[test] fn t() { assert!(true, "{}", has_machine_specific_home_path("/home/alice/work", &re)); }"##).is_empty());
        assert_eq!(fixture_spans(CHECKS, r##"#[test] fn t() { let result = evaluate_forbidden_paths(&re, [("file", "/home/alice/work")].into_iter()); }"##).len(), 1);
        assert!(fixture_spans(CHECKS, r##"#[test] fn t() { let files = [("file", "/home/alice/work")]; let result = evaluate_forbidden_paths(&re, files.into_iter()); }"##).is_empty());
    }
    #[test]
    fn ancestor_and_sibling_redirects_fail_closed() {
        assert!(fixture_spans(SELF, r##"mod tests { fn fixture_spans(_: &str,s:&str)->std::path::PathBuf{s.into()} #[test] fn t(){assert!(std::fs::read(fixture_spans("x","/home/alice/private")).is_ok());}}"##).is_empty());
        assert!(fixture_spans(HYGIENE, r##"mod tests { use attacker::has_machine_specific_home_path; #[test] fn t() { assert!(has_machine_specific_home_path("/home/alice/work", &re)); } }"##).is_empty());
        assert!(fixture_spans(HYGIENE, r##"use attacker::assert; #[test] fn t() { assert!(has_machine_specific_home_path("/home/alice/work", &re)); }"##).is_empty());
        assert!(fixture_spans(HYGIENE, r##"macro_rules! assert { ($e:expr) => { std::fs::read($e) } } #[test] fn t() { assert!(has_machine_specific_home_path("/home/alice/work", &re)); }"##).is_empty());
        assert!(fixture_spans(HYGIENE, r##"use attacker::*; #[test] fn t() { assert!(has_machine_specific_home_path("/home/alice/work", &re)); }"##).is_empty());
        assert_eq!(fixture_spans(HYGIENE, r##"fn has_machine_specific_home_path(_: &str, _: &()) -> bool { true } mod tests { use super::*; use color_eyre::eyre::ensure; #[test] fn t() { assert!(has_machine_specific_home_path("/home/alice/work", &())); } }"##).len(), 1);
    }
    #[test]
    fn foreign_bindings_fail_closed() {
        assert!(fixture_spans(SELF, r##"mod tests { unsafe extern "C" { fn fixture_spans(p: *const u8); } #[test] fn t() { assert!(fixture_spans("x", "/home/alice/private").is_empty()); } }"##).is_empty());
    }
    #[test]
    fn only_audited_abstract_comment_tokens() {
        assert_eq!(
            fixture_spans(
                CHECKS,
                r##"// `c:\users\...` (lowercase drive/segment) `C:\Users\...`
fn t() {}"##
            )
            .len(),
            2
        );
        assert_eq!(
            fixture_spans(
                CHECKS,
                r##"// (`C:\Users\John Doe\...`, common on Windows)
fn t() {}"##
            )
            .len(),
            1
        );
        assert!(
            fixture_spans(
                CHECKS,
                r##"// `C:\Users\John Doe\project` and `C:\Users\alice\...`
fn t() {}"##
            )
            .is_empty()
        );
        assert!(
            fixture_spans(CHECKS, r##"fn t() { let s = r"// `C:\Users\John Doe\...`"; }"##)
                .is_empty()
        );
        assert_eq!(
            fixture_spans(
                CHECKS,
                r##"// `C:\Users\John Doe\...` and C:\Users\alice\private
fn t() {}"##
            )
            .len(),
            1
        );
    }
    #[test]
    fn block_comment_markers_do_not_admit_later_literals() {
        assert!(
            fixture_spans(CHECKS, r##"/* // */ fn t() { let p = r"`C:\Users\John Doe\...`"; }"##)
                .is_empty()
        );
        assert!(
            fixture_spans(CHECKS, r##"/* // */ fn t() { let p = "`C:\\Users\\John Doe\\...`"; }"##)
                .is_empty()
        );
        assert!(
            fixture_spans(HYGIENE, r##"/* // */ fn t() { let p = r"C:\Users\...\AppData"; }"##)
                .is_empty()
        );
    }
    #[test]
    fn comment_placeholder_is_not_a_real_user_or_string() {
        assert_eq!(
            fixture_spans(
                HYGIENE,
                r##"// C:\Users\...\AppData
fn t() {}"##
            )
            .len(),
            1
        );
        assert!(
            fixture_spans(
                HYGIENE,
                r##"// C:\Users\alice\AppData
fn t() {}"##
            )
            .is_empty()
        );
        assert!(
            fixture_spans(HYGIENE, r##"fn t() { let s = r"// C:\Users\...\AppData"; }"##)
                .is_empty()
        );
    }
}
