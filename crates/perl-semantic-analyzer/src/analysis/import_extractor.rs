//! Import specification extractor for `use` and `require` statements.
//!
//! Directive `use` / `require` meaning comes from
//! [`perl_parser_core::hir::CompileEnvironment::import_specs`]. This module
//! does not reclassify flattened `Use.args`. It only overlays patterns the
//! current HIR projection does not emit: `require Module; Module->import(...)`
//! pairing and standalone `ClassName->import(@names)`.
//!
//! # Supported Patterns
//!
//! | Perl source                              | `ImportKind`          | `ImportSymbols`              | Source  |
//! |------------------------------------------|-----------------------|------------------------------|---------|
//! | `use Module qw(a b)`                     | `UseExplicitList`     | `Explicit(["a", "b"])`       | HIR     |
//! | `use Module ()`                          | `UseEmpty`            | `None`                       | HIR     |
//! | `use Module ':tag'`                      | `UseTag`              | `Tags(["tag"])`              | HIR     |
//! | `use Module` (bare)                      | `Use`                 | `Default`                    | HIR     |
//! | `use constant { FOO => 1 }`              | `UseConstant`         | `Explicit(["FOO"])`          | HIR     |
//! | `use constant PI => 3.14`                | `UseConstant`         | `Explicit(["PI"])`           | HIR     |
//! | `require Module`                         | `Require`             | `Default`                    | HIR     |
//! | `require Module; Module->import(...)`    | `RequireThenImport`   | `Explicit([...])` / `Default`| overlay |
//! | `require $var`                           | `DynamicRequire`      | `Dynamic`                    | HIR     |
//! | `Foo->import(@names)` (standalone)       | `ManualImport`        | `Dynamic`                    | overlay |
use crate::ast::{Node, NodeKind};
use perl_parser_core::hir::{HirFile, lower_ast};
use perl_semantic_facts::{
    AnchorId, Confidence, FileId, ImportKind, ImportSpec, ImportSymbols, Provenance,
};

/// Extractor that projects HIR import facts and overlays non-directive import
/// patterns into [`ImportSpec`] entries.
pub struct ImportExtractor;

impl ImportExtractor {
    /// Project canonical HIR import facts, then overlay patterns HIR does not emit.
    ///
    /// Each spec carries the supplied `file_id` and an `anchor_id` derived from
    /// the statement's byte-offset span.
    pub fn extract(ast: &Node, file_id: FileId) -> Vec<ImportSpec> {
        Self::extract_from_hir(&lower_ast(ast), ast, file_id)
    }

    /// Project [`HirFile::compile_environment`] import facts for `file_id`.
    ///
    /// Callers that already lowered HIR should use this entry so analyzer
    /// specs come from the same compile-environment projection rather than a
    /// second argument classifier.
    pub fn extract_from_hir(hir: &HirFile, ast: &Node, file_id: FileId) -> Vec<ImportSpec> {
        let mut out = hir.compile_environment.import_specs(file_id);
        Self::overlay_non_directive_imports(ast, file_id, &mut out);
        out
    }

    // ── Overlay for patterns current HIR import_specs do not emit ────────

    fn overlay_non_directive_imports(node: &Node, file_id: FileId, out: &mut Vec<ImportSpec>) {
        if let Some(spec) = Self::try_classify_standalone_class_import(node, file_id)
            && !Self::already_has_span(out, spec.span_start_byte)
        {
            out.push(spec);
        }

        match &node.kind {
            NodeKind::Program { statements } | NodeKind::Block { statements } => {
                Self::overlay_require_then_import(statements, file_id, out);
            }
            _ => {}
        }

        for child in node.children() {
            Self::overlay_non_directive_imports(child, file_id, out);
        }
    }

    fn already_has_span(specs: &[ImportSpec], span_start_byte: Option<u32>) -> bool {
        specs.iter().any(|spec| spec.span_start_byte == span_start_byte)
    }

    /// Detect a standalone `ClassName->import(...)` call where `ClassName`
    /// is a static identifier (not a variable).
    ///
    /// Returns `None` when:
    /// - The node is not a `MethodCall`.
    /// - The method name is not `"import"`.
    /// - The object is a variable (those are covered by `walk_statements`).
    /// - The argument list is entirely static (fully explicit symbols) — those
    ///   are already captured by `walk_statements` when preceded by `require`.
    ///
    /// Returns an `ImportSpec` with `ImportSymbols::Dynamic` when the
    /// argument list contains any dynamic argument (e.g. `@names`, `$names`).
    /// Returns `None` when all arguments are static strings or `qw(...)` lists
    /// (those produce `Explicit` specs through `walk_statements`).
    fn try_classify_standalone_class_import(node: &Node, file_id: FileId) -> Option<ImportSpec> {
        let (object, method, args) = match &node.kind {
            NodeKind::MethodCall { object, method, args } => (object, method, args),
            _ => return None,
        };

        if method != "import" {
            return None;
        }

        // Only static class names (Identifier nodes), not variables.
        let class_name = match &object.kind {
            NodeKind::Identifier { name } => name.as_str(),
            _ => return None,
        };

        // Classify the argument list.
        let symbols = Self::extract_import_call_symbols(args);

        // Only emit evidence when the arguments are Dynamic (unknown at
        // compile time). Explicit/tag lists are precise and do not need
        // the conservative "any bareword might be imported" treatment.
        if !matches!(symbols, ImportSymbols::Dynamic) {
            return None;
        }

        let anchor_id = Self::anchor_from_node(node);
        Some(ImportSpec {
            module: class_name.to_string(),
            // ManualImport distinguishes this from a `use Foo` statement —
            // it is a `Class->import(...)` method call, not a `use` declaration.
            kind: ImportKind::ManualImport,
            symbols,
            provenance: Provenance::DynamicBoundary,
            confidence: Confidence::Low,
            file_id: Some(file_id),
            anchor_id: Some(anchor_id),
            scope_id: None,
            span_start_byte: Some(node.location.start.min(u32::MAX as usize) as u32),
        })
    }

    /// Overlay `require Module; Module->import(...)` onto HIR Require rows.
    ///
    /// Standalone `require` and `require $var` stay as HIR facts. This only
    /// promotes a paired import call and replaces a path-like HIR Require row
    /// in place so the quoted-require overlay does not duplicate.
    fn overlay_require_then_import(
        statements: &[Node],
        file_id: FileId,
        out: &mut Vec<ImportSpec>,
    ) {
        let mut consumed: std::collections::HashSet<usize> = std::collections::HashSet::new();

        for (i, stmt) in statements.iter().enumerate() {
            if consumed.contains(&i) {
                continue;
            }

            let expr = Self::unwrap_expression_statement(stmt);

            let (require_node, require_args) = match &expr.kind {
                NodeKind::FunctionCall { name, args } if name == "require" => (stmt, args),
                _ => continue,
            };

            if Self::is_dynamic_require(require_args) {
                consumed.insert(i);
                continue;
            }

            let module_name = match Self::extract_require_module_name(require_args) {
                Some(name) => name,
                None => continue,
            };

            let import_spec = statements.get(i + 1).and_then(|next_stmt| {
                let next_expr = Self::unwrap_expression_statement(next_stmt);
                Self::try_match_import_call(next_expr, &module_name)
            });

            if let Some((symbols, _import_node)) = import_spec {
                let require_span = Self::some_span_start(require_node);
                let expr_span = Self::some_span_start(expr);
                let confidence = Self::confidence_for_symbols(&symbols);
                let provenance = if matches!(symbols, ImportSymbols::Dynamic) {
                    Provenance::ExactAst
                } else {
                    Provenance::LiteralRequireImport
                };
                if let Some(existing) = out.iter_mut().find(|spec| {
                    (spec.span_start_byte == require_span || spec.span_start_byte == expr_span)
                        && matches!(spec.kind, ImportKind::Require | ImportKind::RequireThenImport)
                }) {
                    existing.kind = ImportKind::RequireThenImport;
                    existing.module = module_name;
                    existing.symbols = symbols;
                    existing.confidence = confidence;
                    existing.provenance = provenance;
                } else {
                    let anchor_id = Self::anchor_from_node(require_node);
                    out.push(ImportSpec {
                        module: module_name,
                        kind: ImportKind::RequireThenImport,
                        symbols,
                        provenance,
                        confidence,
                        file_id: Some(file_id),
                        anchor_id: Some(anchor_id),
                        scope_id: None,
                        span_start_byte: require_span,
                    });
                }
                consumed.insert(i);
                consumed.insert(i + 1);
            } else {
                consumed.insert(i);
            }
        }
    }

    fn some_span_start(node: &Node) -> Option<u32> {
        Some(node.location.start.min(u32::MAX as usize) as u32)
    }

    // ── Require helpers ────────────────────────────────────────────────

    /// Unwrap an `ExpressionStatement` to get the inner expression node.
    /// Returns the node itself if it is not an `ExpressionStatement`.
    fn unwrap_expression_statement(node: &Node) -> &Node {
        match &node.kind {
            NodeKind::ExpressionStatement { expression } => expression,
            _ => node,
        }
    }

    /// Check whether a `require` call's arguments indicate a dynamic require
    /// (i.e. `require $var`).
    fn is_dynamic_require(args: &[Node]) -> bool {
        match args.first() {
            Some(arg) => matches!(&arg.kind, NodeKind::Variable { .. }),
            None => false,
        }
    }

    /// Extract the module name from a `require` call's arguments.
    ///
    /// Handles:
    /// - `require Foo::Bar` → `"Foo::Bar"` (Identifier)
    /// - `require "Foo/Bar.pm"` → `"Foo::Bar"` (String, path-to-module conversion)
    fn extract_require_module_name(args: &[Node]) -> Option<String> {
        let arg = args.first()?;
        match &arg.kind {
            NodeKind::Identifier { name } => Some(name.clone()),
            NodeKind::String { value, .. } => {
                // "Foo/Bar.pm" → "Foo::Bar"
                let cleaned = value.trim_matches('\'').trim_matches('"').trim();
                let module = cleaned.trim_end_matches(".pm").replace('/', "::");
                Some(module)
            }
            _ => None,
        }
    }

    /// Try to match a `Module->import(...)` method call node.
    ///
    /// Returns `Some((symbols, node))` if the node is a `MethodCall` with
    /// method `"import"` and the object matches `expected_module`.
    fn try_match_import_call<'a>(
        node: &'a Node,
        expected_module: &str,
    ) -> Option<(ImportSymbols, &'a Node)> {
        let (object, method, args) = match &node.kind {
            NodeKind::MethodCall { object, method, args } => (object, method, args),
            _ => return None,
        };

        if method != "import" {
            return None;
        }

        // The object must be an Identifier matching the module name.
        let obj_name = match &object.kind {
            NodeKind::Identifier { name } => name.as_str(),
            _ => return None,
        };

        if obj_name != expected_module {
            return None;
        }

        // Extract imported symbols from the argument list.
        let symbols = Self::extract_import_call_symbols(args);
        Some((symbols, node))
    }

    /// Extract [`ImportSymbols`] from the argument list of a `Module->import(...)` call.
    fn extract_import_call_symbols(args: &[Node]) -> ImportSymbols {
        if args.is_empty() {
            return ImportSymbols::Default;
        }

        let mut names: Vec<String> = Vec::new();
        let mut tags: Vec<String> = Vec::new();
        let mut has_dynamic_arg = false;

        for arg in args {
            has_dynamic_arg |= Self::collect_import_arg_symbols(arg, &mut names, &mut tags);
        }

        if has_dynamic_arg {
            return ImportSymbols::Dynamic;
        }

        if names.is_empty() && tags.is_empty() {
            return ImportSymbols::Default;
        }

        if !tags.is_empty() && names.is_empty() {
            return ImportSymbols::Tags(tags);
        }

        if !tags.is_empty() && !names.is_empty() {
            return ImportSymbols::Mixed { tags, names };
        }

        ImportSymbols::Explicit(names)
    }

    /// Collect symbol names and tags from a single argument node of an
    /// `import(...)` call.
    ///
    /// Returns `true` when the argument is dynamic or unsupported and should
    /// prevent the import site from claiming exact symbol names.
    fn collect_import_arg_symbols(
        arg: &Node,
        names: &mut Vec<String>,
        tags: &mut Vec<String>,
    ) -> bool {
        match &arg.kind {
            NodeKind::String { value, .. } => {
                let bare = value.trim_matches('\'').trim_matches('"');
                if let Some(tag) = bare.strip_prefix(':') {
                    tags.push(tag.to_string());
                } else if !bare.is_empty() {
                    names.push(bare.to_string());
                }
                false
            }
            NodeKind::Identifier { name } => {
                // Handle qw(...) stored as raw identifier string.
                if let Some(inner) = Self::parse_qw_content(name) {
                    for word in inner.split_whitespace() {
                        if let Some(tag) = word.strip_prefix(':') {
                            tags.push(tag.to_string());
                        } else {
                            names.push(word.to_string());
                        }
                    }
                } else if let Some(tag) = name.strip_prefix(':') {
                    tags.push(tag.to_string());
                } else if !name.is_empty() {
                    names.push(name.clone());
                }
                false
            }
            NodeKind::Variable { .. } => {
                // `Foo->import(@names)` / `Foo->import($name)` is dynamic:
                // do not guess exact imported symbols.
                true
            }
            NodeKind::ArrayLiteral { elements } => {
                // qw(...) in expression context → ArrayLiteral of String nodes
                let mut has_dynamic_arg = false;
                for el in elements {
                    has_dynamic_arg |= Self::collect_import_arg_symbols(el, names, tags);
                }
                has_dynamic_arg
            }
            _ => true,
        }
    }

    fn confidence_for_symbols(symbols: &ImportSymbols) -> Confidence {
        if matches!(symbols, ImportSymbols::Dynamic) { Confidence::Low } else { Confidence::High }
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    /// Derive an [`AnchorId`] from a node's byte-offset span.
    fn anchor_from_node(node: &Node) -> AnchorId {
        // Use the start byte offset as a deterministic anchor ID.
        // This is unique per use-statement within a file.
        AnchorId(node.location.start as u64)
    }

    /// Extract the inner content of a `qw<delim>...</delim>` string.
    ///
    /// Handles all Perl-legal qw delimiters: `qw(a b)`, `qw[a b]`, `qw{a b}`,
    /// `qw<a b>`, `qw/a b/`, and the space-before-delimiter forms such as
    /// `qw (a b)`, `qw [a b]`, etc.
    ///
    /// Returns `Some("a b c")` for any valid qw form, `None` otherwise.
    /// Rejects `qwfoo` (alphanumeric or underscore immediately after `qw`).
    fn parse_qw_content(s: &str) -> Option<&str> {
        Self::parse_quote_operator_content(s, "qw")
    }

    /// Generic quote-operator content extractor.
    ///
    /// Delegates to the canonical implementation in
    /// `perl_parser_core::parse_quote_operator_content`.
    fn parse_quote_operator_content<'a>(s: &'a str, operator: &str) -> Option<&'a str> {
        perl_parser_core::parse_quote_operator_content(s, operator)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    /// Parse Perl source and extract import specs.
    fn parse_and_extract(code: &str) -> Vec<ImportSpec> {
        let mut parser = Parser::new(code);
        let ast = match parser.parse() {
            Ok(ast) => ast,
            Err(_) => return Vec::new(),
        };
        ImportExtractor::extract(&ast, FileId(1))
    }

    fn hir_constant_symbols(code: &str) -> Result<ImportSymbols, String> {
        let mut parser = Parser::new(code);
        let ast = parser.parse().map_err(|error| format!("fixture must parse: {error:?}"))?;
        let hir = perl_parser_core::hir::lower_ast(&ast);
        let specs = hir.compile_environment.import_specs(FileId(1));
        specs
            .into_iter()
            .find(|spec| spec.module == "constant")
            .map(|spec| spec.symbols)
            .ok_or_else(|| "HIR produced no constant ImportSpec".to_string())
    }

    // ── use Module qw(a b) → UseExplicitList ────────────────────────────

    #[test]
    fn test_use_explicit_list_qw() -> Result<(), String> {
        let specs = parse_and_extract("use List::Util qw(first reduce any);");
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "List::Util");
        assert_eq!(spec.kind, ImportKind::UseExplicitList);
        if let ImportSymbols::Explicit(names) = &spec.symbols {
            assert!(names.contains(&"first".to_string()), "missing 'first' in {names:?}");
            assert!(names.contains(&"reduce".to_string()), "missing 'reduce' in {names:?}");
            assert!(names.contains(&"any".to_string()), "missing 'any' in {names:?}");
        } else {
            return Err(format!("expected Explicit, got {:?}", spec.symbols));
        }
        assert_eq!(spec.file_id, Some(FileId(1)));
        assert!(spec.anchor_id.is_some());
        Ok(())
    }

    #[test]
    fn test_use_explicit_list_quoted_strings() -> Result<(), String> {
        let specs = parse_and_extract("use Exporter 'import';");
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "Exporter");
        assert_eq!(spec.kind, ImportKind::UseExplicitList);
        if let ImportSymbols::Explicit(names) = &spec.symbols {
            assert!(names.contains(&"import".to_string()), "missing 'import' in {names:?}");
        } else {
            return Err(format!("expected Explicit, got {:?}", spec.symbols));
        }
        Ok(())
    }

    // ── use Module () → UseEmpty ────────────────────────────────────────
    //
    // HIR `classify_import_args` distinguishes `use Module;` from `use Module ()`
    // when both flatten to empty args, using the same span-length heuristic
    // previously owned by this extractor.

    #[test]
    fn test_use_empty_parens() -> Result<(), String> {
        let specs = parse_and_extract("use POSIX ();");
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "POSIX");
        assert_eq!(spec.kind, ImportKind::UseEmpty);
        assert_eq!(spec.symbols, ImportSymbols::None);
        Ok(())
    }

    // ── use Module ':tag' → UseTag ──────────────────────────────────────

    #[test]
    fn test_use_tag_single() -> Result<(), String> {
        let specs = parse_and_extract("use POSIX ':sys_wait_h';");
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "POSIX");
        assert_eq!(spec.kind, ImportKind::UseTag);
        if let ImportSymbols::Tags(tags) = &spec.symbols {
            assert!(tags.contains(&"sys_wait_h".to_string()), "missing tag in {tags:?}");
        } else {
            return Err(format!("expected Tags, got {:?}", spec.symbols));
        }
        Ok(())
    }

    #[test]
    fn test_use_tag_in_qw() -> Result<(), String> {
        let specs = parse_and_extract("use Fcntl qw(:flock);");
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "Fcntl");
        assert_eq!(spec.kind, ImportKind::UseTag);
        if let ImportSymbols::Tags(tags) = &spec.symbols {
            assert!(tags.contains(&"flock".to_string()), "missing tag in {tags:?}");
        } else {
            return Err(format!("expected Tags, got {:?}", spec.symbols));
        }
        Ok(())
    }

    // ── use Module (bare) → Use/Default ─────────────────────────────────

    #[test]
    fn test_use_bare() -> Result<(), String> {
        let specs = parse_and_extract("use strict;");
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "strict");
        assert_eq!(spec.kind, ImportKind::Use);
        assert_eq!(spec.symbols, ImportSymbols::Default);
        Ok(())
    }

    #[test]
    fn test_use_bare_qualified() -> Result<(), String> {
        let specs = parse_and_extract("use Data::Dumper;");
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "Data::Dumper");
        assert_eq!(spec.kind, ImportKind::Use);
        assert_eq!(spec.symbols, ImportSymbols::Default);
        Ok(())
    }

    // ── use constant → UseConstant ──────────────────────────────────────

    #[test]
    fn test_use_constant_scalar() -> Result<(), String> {
        let specs = parse_and_extract("use constant PI => 3.14;");
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "constant");
        assert_eq!(spec.kind, ImportKind::UseConstant);
        if let ImportSymbols::Explicit(names) = &spec.symbols {
            assert!(names.contains(&"PI".to_string()), "missing 'PI' in {names:?}");
        } else {
            return Err(format!("expected Explicit, got {:?}", spec.symbols));
        }
        Ok(())
    }

    #[test]
    fn test_use_constant_hash_ref() -> Result<(), String> {
        let specs = parse_and_extract("use constant { FOO => 1, BAR => 2 };");
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "constant");
        assert_eq!(spec.kind, ImportKind::UseConstant);
        if let ImportSymbols::Explicit(names) = &spec.symbols {
            assert!(names.contains(&"FOO".to_string()), "missing 'FOO' in {names:?}");
            assert!(names.contains(&"BAR".to_string()), "missing 'BAR' in {names:?}");
        } else {
            return Err(format!("expected Explicit, got {:?}", spec.symbols));
        }
        Ok(())
    }

    #[test]
    fn test_use_constant_quoted_scalar() -> Result<(), String> {
        let specs = parse_and_extract("use constant 'HTTP_OK' => 200;");
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "constant");
        assert_eq!(spec.kind, ImportKind::UseConstant);
        if let ImportSymbols::Explicit(names) = &spec.symbols {
            assert!(names.contains(&"HTTP_OK".to_string()), "missing 'HTTP_OK' in {names:?}");
        } else {
            return Err(format!("expected Explicit, got {:?}", spec.symbols));
        }
        Ok(())
    }

    #[test]
    fn test_use_constant_quoted_hash_ref() -> Result<(), String> {
        let specs = parse_and_extract(r#"use constant { 'FOO' => 1, "BAR" => 2 };"#);
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "constant");
        assert_eq!(spec.kind, ImportKind::UseConstant);
        if let ImportSymbols::Explicit(names) = &spec.symbols {
            assert!(names.contains(&"FOO".to_string()), "missing 'FOO' in {names:?}");
            assert!(names.contains(&"BAR".to_string()), "missing 'BAR' in {names:?}");
        } else {
            return Err(format!("expected Explicit, got {:?}", spec.symbols));
        }
        Ok(())
    }

    #[test]
    fn test_use_constant_plus_hash_ref() -> Result<(), String> {
        let source = "use constant +{ FOO => 1, BAR => 2 };";
        let specs = parse_and_extract(source);
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "constant");
        assert_eq!(spec.kind, ImportKind::UseConstant);
        assert_eq!(
            spec.symbols,
            hir_constant_symbols(source)?,
            "plus-hash constant form must follow HIR, not a local argument classifier"
        );
        Ok(())
    }

    #[test]
    fn test_use_constant_hash_ref_nested_fat_comma_agrees_with_hir() -> Result<(), String> {
        let source = "use constant { FOO => { nested => 1 }, BAR => 2 };";
        let specs = parse_and_extract(source);
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "constant");
        assert_eq!(spec.kind, ImportKind::UseConstant);
        assert_eq!(
            spec.symbols,
            hir_constant_symbols(source)?,
            "nested constant values must follow HIR, not a local nested-hash skipper"
        );
        Ok(())
    }

    #[test]
    fn test_use_constant_empty() -> Result<(), String> {
        let specs = parse_and_extract("use constant;");
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "constant");
        assert_eq!(spec.kind, ImportKind::UseConstant);
        assert_eq!(spec.symbols, ImportSymbols::None);
        Ok(())
    }

    // ── Version pragmas are skipped ─────────────────────────────────────

    #[test]
    fn test_version_pragma_skipped() -> Result<(), String> {
        let specs = parse_and_extract("use 5.036;");
        assert!(specs.is_empty(), "version pragma should not produce ImportSpec");
        Ok(())
    }

    #[test]
    fn test_vstring_pragma_skipped() -> Result<(), String> {
        let specs = parse_and_extract("use v5.38;");
        assert!(specs.is_empty(), "v-string pragma should not produce ImportSpec");
        Ok(())
    }

    // ── Multiple use statements ─────────────────────────────────────────

    #[test]
    fn test_multiple_use_statements() -> Result<(), String> {
        let code = r#"
use strict;
use warnings;
use List::Util qw(first any);
use POSIX ();
use constant MAX => 100;
"#;
        let specs = parse_and_extract(code);
        assert_eq!(specs.len(), 5, "expected 5 ImportSpecs, got {}", specs.len());

        // strict — bare
        assert_eq!(specs[0].module, "strict");
        assert_eq!(specs[0].kind, ImportKind::Use);

        // warnings — bare
        assert_eq!(specs[1].module, "warnings");
        assert_eq!(specs[1].kind, ImportKind::Use);

        // List::Util — explicit list
        assert_eq!(specs[2].module, "List::Util");
        assert_eq!(specs[2].kind, ImportKind::UseExplicitList);

        // POSIX — empty
        assert_eq!(specs[3].module, "POSIX");
        assert_eq!(specs[3].kind, ImportKind::UseEmpty);

        // constant — use constant
        assert_eq!(specs[4].module, "constant");
        assert_eq!(specs[4].kind, ImportKind::UseConstant);

        Ok(())
    }

    // ── Anchor and file_id are populated ────────────────────────────────

    #[test]
    fn test_anchor_and_file_id_populated() -> Result<(), String> {
        let specs = parse_and_extract("use Foo::Bar qw(baz);");
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.file_id, Some(FileId(1)));
        assert!(spec.anchor_id.is_some(), "anchor_id should be populated");
        assert_eq!(spec.provenance, Provenance::ExactAst);
        assert_eq!(spec.confidence, Confidence::High);
        Ok(())
    }

    // ── Nested use in package block ─────────────────────────────────────

    #[test]
    fn test_use_inside_package_block() -> Result<(), String> {
        let code = r#"
package MyModule;
use Exporter 'import';
our @EXPORT = qw(foo);
1;
"#;
        let specs = parse_and_extract(code);
        let exporter_spec =
            specs.iter().find(|s| s.module == "Exporter").ok_or("expected Exporter ImportSpec")?;

        assert_eq!(exporter_spec.kind, ImportKind::UseExplicitList);
        if let ImportSymbols::Explicit(names) = &exporter_spec.symbols {
            assert!(names.contains(&"import".to_string()));
        } else {
            return Err(format!("expected Explicit, got {:?}", exporter_spec.symbols));
        }
        Ok(())
    }

    // ── Mixed tags and names ────────────────────────────────────────────

    #[test]
    fn test_use_mixed_tags_and_names() -> Result<(), String> {
        let specs = parse_and_extract("use Fcntl qw(:flock LOCK_EX LOCK_NB);");
        let spec = specs.first().ok_or("expected at least one ImportSpec")?;

        assert_eq!(spec.module, "Fcntl");
        assert_eq!(spec.kind, ImportKind::UseExplicitList);
        if let ImportSymbols::Mixed { tags, names } = &spec.symbols {
            assert!(tags.contains(&"flock".to_string()), "missing tag 'flock' in {tags:?}");
            assert!(names.contains(&"LOCK_EX".to_string()), "missing 'LOCK_EX' in {names:?}");
            assert!(names.contains(&"LOCK_NB".to_string()), "missing 'LOCK_NB' in {names:?}");
        } else {
            return Err(format!("expected Mixed, got {:?}", spec.symbols));
        }
        Ok(())
    }

    // ── require Module → Require ────────────────────────────────────────

    #[test]
    fn test_require_bare_module() -> Result<(), String> {
        let specs = parse_and_extract("require Foo::Bar;");
        let spec = specs
            .iter()
            .find(|s| s.module == "Foo::Bar")
            .ok_or("expected ImportSpec for Foo::Bar")?;

        assert_eq!(spec.kind, ImportKind::Require);
        assert_eq!(spec.symbols, ImportSymbols::Default);
        assert_eq!(spec.provenance, Provenance::ExactAst);
        assert_eq!(spec.confidence, Confidence::High);
        assert_eq!(spec.file_id, Some(FileId(1)));
        assert!(spec.anchor_id.is_some(), "anchor_id should be populated");
        Ok(())
    }

    // ── require Module; Module->import(...) → RequireThenImport ─────────

    #[test]
    fn test_require_then_import_with_qw() -> Result<(), String> {
        let code = r#"
require Foo::Bar;
Foo::Bar->import(qw(alpha beta));
"#;
        let specs = parse_and_extract(code);
        let spec = specs
            .iter()
            .find(|s| s.module == "Foo::Bar")
            .ok_or("expected ImportSpec for Foo::Bar")?;

        assert_eq!(spec.kind, ImportKind::RequireThenImport);
        if let ImportSymbols::Explicit(names) = &spec.symbols {
            assert!(names.contains(&"alpha".to_string()), "missing 'alpha' in {names:?}");
            assert!(names.contains(&"beta".to_string()), "missing 'beta' in {names:?}");
        } else {
            return Err(format!("expected Explicit, got {:?}", spec.symbols));
        }
        // Fully literal import list → LiteralRequireImport provenance.
        assert_eq!(spec.provenance, Provenance::LiteralRequireImport);
        assert_eq!(spec.confidence, Confidence::High);
        Ok(())
    }

    #[test]
    fn test_require_then_import_bare() -> Result<(), String> {
        let code = r#"
require Some::Module;
Some::Module->import();
"#;
        let specs = parse_and_extract(code);
        let spec = specs
            .iter()
            .find(|s| s.module == "Some::Module")
            .ok_or("expected ImportSpec for Some::Module")?;

        assert_eq!(spec.kind, ImportKind::RequireThenImport);
        assert_eq!(spec.symbols, ImportSymbols::Default);
        Ok(())
    }

    #[test]
    fn test_require_then_import_quoted_strings() -> Result<(), String> {
        let code = r#"
require Foo::Bar;
Foo::Bar->import('alpha', 'beta');
"#;
        let specs = parse_and_extract(code);
        let spec = specs
            .iter()
            .find(|s| s.module == "Foo::Bar")
            .ok_or("expected ImportSpec for Foo::Bar")?;

        assert_eq!(spec.kind, ImportKind::RequireThenImport);
        if let ImportSymbols::Explicit(names) = &spec.symbols {
            assert!(names.contains(&"alpha".to_string()), "missing 'alpha' in {names:?}");
            assert!(names.contains(&"beta".to_string()), "missing 'beta' in {names:?}");
        } else {
            return Err(format!("expected Explicit, got {:?}", spec.symbols));
        }
        // Fully literal quoted-string import → LiteralRequireImport provenance.
        assert_eq!(spec.provenance, Provenance::LiteralRequireImport);
        assert_eq!(spec.confidence, Confidence::High);
        Ok(())
    }

    #[test]
    fn test_require_then_import_dynamic_symbol_list() -> Result<(), String> {
        let code = r#"
require Foo::Bar;
Foo::Bar->import(@names);
"#;
        let specs = parse_and_extract(code);
        let spec = specs
            .iter()
            .find(|s| s.module == "Foo::Bar")
            .ok_or("expected ImportSpec for Foo::Bar")?;

        assert_eq!(spec.kind, ImportKind::RequireThenImport);
        assert_eq!(spec.symbols, ImportSymbols::Dynamic);
        assert_eq!(spec.confidence, Confidence::Low);
        Ok(())
    }

    // ── require $var → DynamicRequire ───────────────────────────────────

    #[test]
    fn test_require_dynamic_variable() -> Result<(), String> {
        let specs = parse_and_extract("require $module;");
        let spec = specs
            .iter()
            .find(|s| s.kind == ImportKind::DynamicRequire)
            .ok_or("expected DynamicRequire ImportSpec")?;

        assert_eq!(spec.module, "");
        assert_eq!(spec.symbols, ImportSymbols::Dynamic);
        // DynamicRequire must use DynamicBoundary provenance (Q5 architectural decision):
        // the module identity is not statically known, so we cannot claim ExactAst.
        assert_eq!(spec.provenance, Provenance::DynamicBoundary);
        assert_eq!(spec.confidence, Confidence::Low);
        assert_eq!(spec.file_id, Some(FileId(1)));
        assert!(spec.anchor_id.is_some(), "anchor_id should be populated");
        Ok(())
    }

    // ── Mixed use and require statements ────────────────────────────────

    #[test]
    fn test_mixed_use_and_require() -> Result<(), String> {
        let code = r#"
use strict;
use warnings;
require Foo::Bar;
Foo::Bar->import(qw(baz));
require $dynamic;
"#;
        let specs = parse_and_extract(code);

        // strict — bare use
        let strict_spec =
            specs.iter().find(|s| s.module == "strict").ok_or("expected strict ImportSpec")?;
        assert_eq!(strict_spec.kind, ImportKind::Use);

        // warnings — bare use
        let warnings_spec =
            specs.iter().find(|s| s.module == "warnings").ok_or("expected warnings ImportSpec")?;
        assert_eq!(warnings_spec.kind, ImportKind::Use);

        // Foo::Bar — require then import
        let foo_spec =
            specs.iter().find(|s| s.module == "Foo::Bar").ok_or("expected Foo::Bar ImportSpec")?;
        assert_eq!(foo_spec.kind, ImportKind::RequireThenImport);
        if let ImportSymbols::Explicit(names) = &foo_spec.symbols {
            assert!(names.contains(&"baz".to_string()), "missing 'baz' in {names:?}");
        } else {
            return Err(format!("expected Explicit, got {:?}", foo_spec.symbols));
        }

        // dynamic require
        let dyn_spec = specs
            .iter()
            .find(|s| s.kind == ImportKind::DynamicRequire)
            .ok_or("expected DynamicRequire ImportSpec")?;
        assert_eq!(dyn_spec.symbols, ImportSymbols::Dynamic);

        Ok(())
    }

    // ── require with string path → Require ──────────────────────────────

    #[test]
    fn test_require_string_path() -> Result<(), String> {
        let specs = parse_and_extract(r#"require "Foo/Bar.pm";"#);
        assert_eq!(specs.len(), 1, "standalone quoted require must keep one HIR row: {specs:?}");
        let spec = specs.first().ok_or("expected ImportSpec for quoted require")?;

        assert_eq!(spec.kind, ImportKind::Require);
        assert_eq!(spec.symbols, ImportSymbols::Default);
        assert_ne!(
            spec.module, "Foo::Bar",
            "standalone quoted require must keep HIR path identity, not a reconstructed module name"
        );
        Ok(())
    }

    // ── standalone ClassName->import(@names) — Case 3 (PR-B) ────────────

    #[test]
    fn standalone_class_dynamic_import_produces_dynamic_spec() -> Result<(), String> {
        // `Foo->import(@names)` — static class, dynamic arg list.
        // Should produce one ImportSpec with ImportSymbols::Dynamic and
        // ImportKind::ManualImport (not Use — it's a method call, not a `use` statement).
        let specs = parse_and_extract(r#"Foo->import(@names);"#);
        let spec = specs
            .iter()
            .find(|s| s.module == "Foo" && matches!(s.symbols, ImportSymbols::Dynamic))
            .ok_or("expected Dynamic ImportSpec for Foo")?;

        assert_eq!(spec.provenance, Provenance::DynamicBoundary);
        assert_eq!(spec.confidence, Confidence::Low);
        assert_eq!(
            spec.kind,
            ImportKind::ManualImport,
            "Class->import(@names) must use ManualImport, not Use"
        );
        Ok(())
    }

    #[test]
    fn standalone_class_explicit_import_produces_no_dynamic_spec() -> Result<(), String> {
        // `Foo->import('bar')` — static class, static arg list.
        // Should NOT produce a Dynamic ImportSpec (explicit symbols only).
        let specs = parse_and_extract(r#"Foo->import('bar');"#);
        let dynamic_specs: Vec<_> =
            specs.iter().filter(|s| matches!(s.symbols, ImportSymbols::Dynamic)).collect();

        assert!(dynamic_specs.is_empty(), "explicit import args must not produce a Dynamic spec");
        Ok(())
    }

    #[test]
    fn variable_class_import_does_not_produce_standalone_spec() -> Result<(), String> {
        // `$var->import(@names)` — variable object, not a static class name.
        // The standalone extractor should not match variable-object calls.
        let specs = parse_and_extract(r#"$var->import(@names);"#);
        // Variable-object calls are handled by require+import pair logic, not
        // the standalone path. Without a require, this should produce no spec.
        let standalone_dynamic: Vec<_> = specs
            .iter()
            .filter(|s| matches!(s.symbols, ImportSymbols::Dynamic) && s.module.is_empty())
            .collect();

        // The standalone extractor only handles Identifier objects, so this
        // should produce nothing via the standalone path.
        assert!(
            standalone_dynamic.is_empty(),
            "variable-class import without require must not produce standalone Dynamic spec"
        );
        Ok(())
    }

    // ── parse_quote_operator_content seam-proof unit tests ─────────────
    //
    // These tests provide direct static evidence for RIPR activation analysis
    // of the `parse_quote_operator_content` function.  Each test pins ONE
    // decision boundary so that a single-line mutation causes exactly that
    // test to fail.

    #[test]
    fn parse_quote_operator_content_compact_paren() {
        // Boundary: '(' => ')' arm in the match
        assert_eq!(
            ImportExtractor::parse_quote_operator_content("qw(foo bar)", "qw"),
            Some("foo bar")
        );
    }

    #[test]
    fn parse_quote_operator_content_compact_bracket() {
        // Boundary: '[' => ']' arm in the match
        assert_eq!(
            ImportExtractor::parse_quote_operator_content("qw[foo bar]", "qw"),
            Some("foo bar")
        );
    }

    #[test]
    fn parse_quote_operator_content_compact_brace() {
        // Boundary: '{' => '}' arm in the match
        assert_eq!(
            ImportExtractor::parse_quote_operator_content("qw{foo bar}", "qw"),
            Some("foo bar")
        );
    }

    #[test]
    fn parse_quote_operator_content_compact_slash() {
        // Boundary: other => other self-close arm
        assert_eq!(
            ImportExtractor::parse_quote_operator_content("qw/foo bar/", "qw"),
            Some("foo bar")
        );
    }

    #[test]
    fn parse_quote_operator_content_space_before_paren() {
        // Boundary: trim_start() handles leading whitespace
        assert_eq!(
            ImportExtractor::parse_quote_operator_content("qw (foo bar)", "qw"),
            Some("foo bar")
        );
    }

    #[test]
    fn parse_quote_operator_content_space_before_bracket() {
        // Boundary: trim_start() + '[' => ']' mapping
        assert_eq!(
            ImportExtractor::parse_quote_operator_content("qw [foo bar]", "qw"),
            Some("foo bar")
        );
    }

    #[test]
    fn parse_quote_operator_content_space_before_slash() {
        // Boundary: trim_start() + self-close
        assert_eq!(
            ImportExtractor::parse_quote_operator_content("qw /foo bar/", "qw"),
            Some("foo bar")
        );
    }

    #[test]
    fn parse_quote_operator_content_alphanumeric_delimiter_rejected() {
        // Boundary: alphanumeric guard — `qwfoo` must be rejected
        assert_eq!(ImportExtractor::parse_quote_operator_content("qwfoo", "qw"), None);
    }

    #[test]
    fn parse_quote_operator_content_underscore_delimiter_rejected() {
        // Boundary: underscore guard — `qw_foo_` must be rejected
        assert_eq!(ImportExtractor::parse_quote_operator_content("qw_foo_", "qw"), None);
    }

    #[test]
    fn parse_quote_operator_content_wrong_operator_returns_none() {
        // Boundary: strip_prefix — wrong operator prefix returns None
        assert_eq!(ImportExtractor::parse_quote_operator_content("qq(foo bar)", "qw"), None);
    }

    #[test]
    fn parse_quote_operator_content_mismatched_close_returns_none() {
        // Boundary: ends_with(close) check — mismatched close returns None
        assert_eq!(ImportExtractor::parse_quote_operator_content("qw(foo bar]", "qw"), None);
    }
}
