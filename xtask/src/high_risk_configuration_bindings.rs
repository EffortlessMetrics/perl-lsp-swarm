//! Exact, function-bound AST witnesses for the CA02B projection.
//!
//! A witness is a complete expression, not a bag of identifiers. Its syntax must
//! remain in the specified production function. Comments, tests and other
//! functions cannot satisfy it. The projection records unresolved runtime
//! obligations separately; source agreement is not first-effect execution proof.
use std::{collections::BTreeMap, error::Error, fs, path::Path};
use syn::visit::{self, Visit};

#[path = "../../crates/perl-lsp-rs-core/src/configuration_authority/high_risk_model.rs"]
mod model;
use model::{Projection, Witness};

type CheckResult<T = ()> = Result<T, Box<dyn Error>>;
const PROJECTION: &str = "fixtures/configuration_authority/high_risk_bindings.v1.json";

fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => Some(
            path.path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join("::"),
        ),
        _ => None,
    }
}

fn test_only(attrs: &[syn::Attribute]) -> bool {
    fn mentions_test(meta: &syn::Meta) -> bool {
        match meta {
            syn::Meta::Path(path) => path.is_ident("test"),
            syn::Meta::List(list) => list
                .parse_args_with(
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                )
                .map_or(true, |children| children.iter().any(mentions_test)),
            syn::Meta::NameValue(_) => false,
        }
    }
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
            // Conservative eligibility, not compiler-resolved cfg evaluation:
            // any test-dependent condition is outside this source witness.
            || ((attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr")) && mentions_test(&attr.meta))
    })
}

fn functions<'a>(
    items: &'a [syn::Item],
    prefix: &str,
    found: &mut BTreeMap<String, Vec<&'a syn::Block>>,
) -> CheckResult {
    for item in items {
        match item {
            syn::Item::Fn(function) if !test_only(&function.attrs) => {
                let id = format!("{prefix}{}", function.sig.ident);
                found.entry(id).or_default().push(&function.block);
            }
            syn::Item::Impl(implementation) if !test_only(&implementation.attrs) => {
                let Some(owner) = type_name(&implementation.self_ty) else { continue };
                let trait_name = implementation
                    .trait_
                    .as_ref()
                    .map(|(path, _)| {
                        format!(
                            "{}::",
                            path.segments
                                .iter()
                                .map(|s| s.ident.to_string())
                                .collect::<Vec<_>>()
                                .join("::")
                        )
                    })
                    .unwrap_or_default();
                for item in &implementation.items {
                    if let syn::ImplItem::Fn(function) = item {
                        if test_only(&function.attrs) {
                            continue;
                        }
                        let id = format!("{prefix}{owner}::{trait_name}{}", function.sig.ident);
                        found.entry(id).or_default().push(&function.block);
                    }
                }
            }
            syn::Item::Mod(module) if !test_only(&module.attrs) => {
                if let Some((_, items)) = &module.content {
                    functions(items, &format!("{prefix}{}::", module.ident), found)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

struct ExpressionMatch<'a> {
    expected: &'a syn::Expr,
    count: usize,
}
impl<'ast> Visit<'ast> for ExpressionMatch<'_> {
    fn visit_local(&mut self, node: &'ast syn::Local) {
        if !test_only(&node.attrs) {
            visit::visit_local(self, node);
        }
    }
    fn visit_arm(&mut self, node: &'ast syn::Arm) {
        if !test_only(&node.attrs) {
            visit::visit_arm(self, node);
        }
    }
    fn visit_field_value(&mut self, node: &'ast syn::FieldValue) {
        if !test_only(&node.attrs) {
            visit::visit_field_value(self, node);
        }
    }
    fn visit_stmt_macro(&mut self, node: &'ast syn::StmtMacro) {
        if !test_only(&node.attrs) {
            visit::visit_stmt_macro(self, node);
        }
    }
    fn visit_expr(&mut self, expression: &'ast syn::Expr) {
        let attrs: &[syn::Attribute] = match expression {
            syn::Expr::Array(node) => &node.attrs,
            syn::Expr::Assign(node) => &node.attrs,
            syn::Expr::Async(node) => &node.attrs,
            syn::Expr::Await(node) => &node.attrs,
            syn::Expr::Binary(node) => &node.attrs,
            syn::Expr::Block(node) => &node.attrs,
            syn::Expr::Break(node) => &node.attrs,
            syn::Expr::Call(node) => &node.attrs,
            syn::Expr::Cast(node) => &node.attrs,
            syn::Expr::Closure(node) => &node.attrs,
            syn::Expr::Const(node) => &node.attrs,
            syn::Expr::Continue(node) => &node.attrs,
            syn::Expr::Field(node) => &node.attrs,
            syn::Expr::ForLoop(node) => &node.attrs,
            syn::Expr::Group(node) => &node.attrs,
            syn::Expr::If(node) => &node.attrs,
            syn::Expr::Index(node) => &node.attrs,
            syn::Expr::Infer(node) => &node.attrs,
            syn::Expr::Let(node) => &node.attrs,
            syn::Expr::Lit(node) => &node.attrs,
            syn::Expr::Loop(node) => &node.attrs,
            syn::Expr::Macro(node) => &node.attrs,
            syn::Expr::Match(node) => &node.attrs,
            syn::Expr::MethodCall(node) => &node.attrs,
            syn::Expr::Paren(node) => &node.attrs,
            syn::Expr::Path(node) => &node.attrs,
            syn::Expr::Range(node) => &node.attrs,
            syn::Expr::RawAddr(node) => &node.attrs,
            syn::Expr::Reference(node) => &node.attrs,
            syn::Expr::Repeat(node) => &node.attrs,
            syn::Expr::Return(node) => &node.attrs,
            syn::Expr::Struct(node) => &node.attrs,
            syn::Expr::Try(node) => &node.attrs,
            syn::Expr::TryBlock(node) => &node.attrs,
            syn::Expr::Tuple(node) => &node.attrs,
            syn::Expr::Unary(node) => &node.attrs,
            syn::Expr::Unsafe(node) => &node.attrs,
            syn::Expr::While(node) => &node.attrs,
            syn::Expr::Yield(node) => &node.attrs,
            // Unparsed or future syntax cannot establish a production binding.
            _ => return,
        };
        if test_only(attrs) {
            return;
        }
        if expression == self.expected {
            self.count += 1;
        }
        visit::visit_expr(self, expression);
    }
    // A nested helper's expression cannot stand in for the enclosing function.
    fn visit_item(&mut self, _: &'ast syn::Item) {}
}

fn check_witness(source: &str, witness: &Witness) -> CheckResult {
    let parsed = syn::parse_file(source)?;
    if let Some(identity) = witness.function.strip_prefix("@serde-field:") {
        let (owner, member) = identity.split_once('.').ok_or("invalid serde field identity")?;
        let structure = parsed
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Struct(item) if item.ident == owner => Some(item),
                _ => None,
            })
            .ok_or("missing serde structure")?;
        let derives_deserialize =
            structure.attrs.iter().filter(|attr| attr.path().is_ident("derive")).any(|attr| {
                attr.parse_args_with(
                    syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
                )
                .is_ok_and(|paths| {
                    paths.iter().any(|path| {
                        path.segments.last().is_some_and(|segment| segment.ident == "Deserialize")
                    })
                })
            });
        let field = structure
            .fields
            .iter()
            .find(|field| field.ident.as_ref().is_some_and(|name| name == member))
            .ok_or("missing serde field")?;
        if !derives_deserialize
            || structure.attrs.iter().any(|attr| {
                !(attr.path().is_ident("doc")
                    || attr.path().is_ident("derive")
                    || attr.path().is_ident("non_exhaustive")
                    || (attr.path().is_ident("serde")
                        && attr
                            .parse_args::<syn::Path>()
                            .is_ok_and(|path| path.is_ident("default"))))
            })
            || field.ty != syn::parse_str::<syn::Type>(&witness.expression)?
            || field.attrs.iter().any(|attr| !attr.path().is_ident("doc"))
        {
            return Err("serde field parse/storage contract changed".into());
        }
        return Ok(());
    }
    let mut owners = BTreeMap::new();
    functions(&parsed.items, "", &mut owners)?;
    if let Some(identity) = witness.function.strip_prefix("@no-key:") {
        let bodies = owners.get(identity).ok_or("missing removed-key parser")?;
        let mut finder = RemovedKey { key: &witness.expression, found: false };
        for body in bodies {
            finder.visit_block(body);
        }
        return if finder.found {
            Err("removed configuration key is parsed again".into())
        } else {
            Ok(())
        };
    }
    let function = owners
        .get(&witness.function)
        .ok_or_else(|| format!("missing function {}", witness.function))?;
    let expected = syn::parse_str::<syn::Expr>(&witness.expression)?;
    let mut matcher = ExpressionMatch { expected: &expected, count: 0 };
    for body in function {
        if matches!(&expected, syn::Expr::Block(block) if &block.block == *body) {
            matcher.count += 1;
        }
        matcher.visit_block(body);
    }
    if matcher.count != 1 {
        return Err(format!(
            "{}: expected exactly one coupled expression, found {}",
            witness.function, matcher.count
        )
        .into());
    }
    Ok(())
}

struct RemovedKey<'a> {
    key: &'a str,
    found: bool,
}

struct StorageUse<'a> {
    member: &'a str,
    writes: bool,
    reads: bool,
}

// matches! accepts a pattern, not an expression list. Consume the complete
// grammar so malformed macro tokens cannot establish storage evidence.
struct MatchesInput {
    expression: syn::Expr,
    guard: Option<syn::Expr>,
}
impl syn::parse::Parse for MatchesInput {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let expression = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        syn::Pat::parse_multi_with_leading_vert(input)?;
        let guard = if input.peek(syn::Token![if]) {
            input.parse::<syn::Token![if]>()?;
            Some(input.parse()?)
        } else {
            None
        };
        if input.peek(syn::Token![,]) {
            input.parse::<syn::Token![,]>()?;
        }
        if !input.is_empty() {
            return Err(input.error("unexpected matches! tokens"));
        }
        Ok(Self { expression, guard })
    }
}
impl StorageUse<'_> {
    // A place is written, not read. Its evaluated receiver/index expressions
    // can still read configuration, so do not discard the entire left operand.
    fn visit_assignment_place(&mut self, place: &syn::Expr) {
        match place {
            syn::Expr::Field(field) => {
                if matches!(&field.member, syn::Member::Named(name) if name == self.member) {
                    self.writes = true;
                }
                self.visit_assignment_place(&field.base);
            }
            syn::Expr::Index(index) => {
                self.visit_assignment_place(&index.expr);
                self.visit_expr(&index.index);
            }
            syn::Expr::Paren(paren) => self.visit_assignment_place(&paren.expr),
            syn::Expr::Group(group) => self.visit_assignment_place(&group.expr),
            syn::Expr::Tuple(tuple) => {
                for element in &tuple.elems {
                    self.visit_assignment_place(element);
                }
            }
            syn::Expr::Array(array) => {
                for element in &array.elems {
                    self.visit_assignment_place(element);
                }
            }
            syn::Expr::Struct(record) => {
                for field in &record.fields {
                    self.visit_assignment_place(&field.expr);
                }
            }
            syn::Expr::Unary(unary) => self.visit_expr(&unary.expr),
            syn::Expr::Call(_) | syn::Expr::MethodCall(_) => self.visit_expr(place),
            _ => {}
        }
    }
}
impl<'ast> Visit<'ast> for StorageUse<'_> {
    fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
        self.visit_assignment_place(&node.left);
        self.visit_expr(&node.right);
    }
    fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
        if matches!(
            node.op,
            syn::BinOp::AddAssign(_)
                | syn::BinOp::SubAssign(_)
                | syn::BinOp::MulAssign(_)
                | syn::BinOp::DivAssign(_)
                | syn::BinOp::RemAssign(_)
                | syn::BinOp::BitXorAssign(_)
                | syn::BinOp::BitAndAssign(_)
                | syn::BinOp::BitOrAssign(_)
                | syn::BinOp::ShlAssign(_)
                | syn::BinOp::ShrAssign(_)
        ) {
            self.visit_assignment_place(&node.left);
        }
        visit::visit_expr_binary(self, node);
    }
    fn visit_field_value(&mut self, node: &'ast syn::FieldValue) {
        if matches!(&node.member, syn::Member::Named(name) if name == self.member) {
            self.writes = true;
        }
        visit::visit_field_value(self, node);
    }
    fn visit_expr_field(&mut self, node: &'ast syn::ExprField) {
        if matches!(&node.member, syn::Member::Named(name) if name == self.member) {
            self.reads = true;
        }
        visit::visit_expr_field(self, node);
    }
    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        if node.mac.path.is_ident("matches")
            && let Ok(arguments) = syn::parse2::<MatchesInput>(node.mac.tokens.clone())
        {
            self.visit_expr(&arguments.expression);
            if let Some(guard) = &arguments.guard {
                self.visit_expr(guard);
            }
        }
    }
}

fn check_storage_join(row: &model::Row, projection: &Projection) -> CheckResult {
    if row.kind != "active" {
        return Ok(());
    }
    let (_, member) = row.rust_field.split_once('.').ok_or("invalid canonical storage identity")?;
    let mut has_write = false;
    let mut has_read = false;
    for (references, writer) in [(&row.writers, true), (&row.consumers, false)] {
        for reference in references {
            let witness = projection.witnesses.get(reference).ok_or("unknown joined witness")?;
            if witness.path.ends_with(".ts") || witness.function.starts_with('@') {
                continue;
            }
            let expression: syn::Expr = syn::parse_str(&witness.expression)?;
            let mut usage = StorageUse { member, writes: false, reads: false };
            usage.visit_expr(&expression);
            if writer {
                has_write |= usage.writes;
            } else {
                has_read |= usage.reads;
            }
        }
    }
    if !has_write || (!row.consumers.is_empty() && !has_read) {
        return Err(format!(
            "{}: witness does not join canonical storage to writer/consumer",
            row.id
        )
        .into());
    }
    Ok(())
}
impl RemovedKey<'_> {
    fn inspect_string(&mut self, value: &str) {
        // Conservative syntax evidence, including JSON-pointer path segments.
        // This does not resolve aliases or infer separately declared serde types.
        self.found |= value == self.key
            || (value.starts_with('/')
                && value
                    .split('/')
                    .skip(1)
                    .any(|segment| segment.replace("~1", "/").replace("~0", "~") == self.key));
    }
    fn inspect_tokens(&mut self, tokens: proc_macro2::TokenStream) {
        for token in tokens {
            match token {
                proc_macro2::TokenTree::Group(group) => self.inspect_tokens(group.stream()),
                proc_macro2::TokenTree::Literal(literal) => {
                    if let Ok(syn::Lit::Str(value)) =
                        syn::parse_str::<syn::Lit>(&literal.to_string())
                    {
                        self.inspect_string(&value.value());
                    }
                }
                _ => {}
            }
        }
    }
}
impl<'ast> Visit<'ast> for RemovedKey<'_> {
    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        if let syn::Meta::List(list) = &node.meta {
            self.inspect_tokens(list.tokens.clone());
        }
        visit::visit_attribute(self, node);
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.inspect_string(&literal.value());
    }
    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.inspect_tokens(node.tokens.clone());
    }
}

fn low_risk_schema_id(family: &str, key: &str) -> Option<&'static str> {
    match (family, key) {
        ("workspace", "discoveryExtensions") => Some("workspace.discovery_extra_extensions"),
        ("workspace", "discoverySkippedDirs") => Some("workspace.discovery_extra_skipped_dirs"),
        ("formatting", "maximumLineLength") => Some("formatting.maximum_line_length"),
        ("formatting", "indentColumns") => Some("formatting.indent_columns"),
        ("formatting", "tabs") => Some("formatting.tabs"),
        ("formatting", "openingBraceOnNewLine") => Some("formatting.opening_brace_on_new_line"),
        ("formatting", "cuddledElse") => Some("formatting.cuddled_else"),
        ("formatting", "spaceAfterKeyword") => Some("formatting.space_after_keyword"),
        ("formatting", "addTrailingCommas") => Some("formatting.add_trailing_commas"),
        ("formatting", "verticalAlignment") => Some("formatting.vertical_alignment"),
        ("formatting", "blockCommentIndentation") => Some("formatting.block_comment_indentation"),
        _ => None,
    }
}

fn schema_coverage(document: &serde_json::Value, projection: &Projection) -> CheckResult {
    fn visit(
        value: &serde_json::Value,
        pointer: &str,
        family: &str,
        projection: &Projection,
    ) -> CheckResult {
        if let Some(properties) = value.get("properties").and_then(serde_json::Value::as_object) {
            for (key, child) in properties {
                let next = format!("{pointer}/properties/{key}");
                let low_risk = low_risk_schema_id(family, key);
                if let Some(id) = low_risk {
                    if !projection.remaining_low_risk.iter().any(|remaining| remaining == id) {
                        return Err(format!("unowned low-risk schema field {id}").into());
                    }
                    continue;
                }
                visit(child, &next, family, projection)?;
            }
        } else if !projection.rows.iter().any(|row| {
            row.projections.iter().any(|field| {
                field.path == "schemas/perllsp-settings.schema.json"
                    && field.pointer == pointer
                    && field.present
            })
        }) {
            return Err(
                format!("schema field lacks runtime/deprecation disposition: {pointer}").into()
            );
        }
        Ok(())
    }
    for family in ["aiCompletion", "critic", "perlcritic", "limits", "formatting", "workspace"] {
        let pointer = format!("/properties/perl/properties/{family}");
        if let Some(value) = document.pointer(&pointer) {
            visit(value, &pointer, family, projection)?;
        }
    }
    Ok(())
}

fn docs_coverage(text: &str, projection: &Projection) -> CheckResult {
    for line in text.lines().filter(|line| line.starts_with('#')) {
        let Some((_, rest)) = line.split_once('`') else {
            continue;
        };
        let Some((id, _)) = rest.split_once('`') else {
            continue;
        };
        let Some(setting) = id.strip_prefix("perl.") else {
            continue;
        };
        let Some((family, _)) = setting.split_once('.') else {
            continue;
        };
        if !matches!(
            family,
            "aiCompletion"
                | "critic"
                | "perlcritic"
                | "limits"
                | "formatting"
                | "workspace"
                | "testRunner"
        ) {
            continue;
        }
        let (_, key) = setting.split_once('.').ok_or("missing documented field")?;
        if let Some(low_risk) = low_risk_schema_id(family, key) {
            if !projection.remaining_low_risk.iter().any(|id| id == low_risk) {
                return Err(
                    format!("documented low-risk field lacks explicit remainder: {id}").into()
                );
            }
            continue;
        }
        if family == "testRunner" {
            let explicitly_removed =
                rest.split_once('`').is_some_and(|(_, suffix)| suffix.trim() == "(removed)");
            if !explicitly_removed
                || !projection.rows.iter().any(|row| {
                    row.id == setting
                        && row.kind == "removed"
                        && row.disposition == "remove_false_contract"
                })
            {
                return Err(format!(
                    "documented runner field is not an explicit owned retirement: {id}"
                )
                .into());
            }
            continue;
        }
        let pointer =
            format!("/properties/perl/properties/{}", setting.replace('.', "/properties/"));
        if !projection
            .rows
            .iter()
            .any(|row| row.projections.iter().any(|field| field.pointer == pointer))
        {
            return Err(format!("documented high-risk field lacks disposition: {id}").into());
        }
    }
    Ok(())
}

pub fn check(root: &Path, typescript: Option<&Path>) -> CheckResult {
    let projection: Projection = serde_json::from_str(&fs::read_to_string(root.join(PROJECTION))?)?;
    model::validate(&projection)?;
    let package: serde_json::Value = serde_json::from_str(&fs::read_to_string(
        root.join("vscode-extension").join("package.json"),
    )?)?;
    let configurations = package
        .pointer("/contributes/configuration")
        .and_then(serde_json::Value::as_array)
        .ok_or("missing extension configuration sections")?;
    for section in configurations {
        if section.get("properties").and_then(serde_json::Value::as_object).is_some_and(
            |properties| properties.keys().any(|key| key.starts_with("perl-lsp.testRunner")),
        ) {
            return Err("removed runner contribution resurrected".into());
        }
    }
    docs_coverage(
        &fs::read_to_string(root.join("docs").join("reference").join("CONFIGURATION_SCHEMA.md"))?,
        &projection,
    )?;
    schema_coverage(
        &serde_json::from_str(&fs::read_to_string(
            root.join("schemas").join("perllsp-settings.schema.json"),
        )?)?,
        &projection,
    )?;
    for (id, witness) in &projection.witnesses {
        if witness.path.ends_with(".ts") {
            continue;
        }
        check_witness(&fs::read_to_string(root.join(&witness.path))?, witness)
            .map_err(|error| format!("witness {id}: {error}"))?;
    }
    if projection.witnesses.values().any(|witness| witness.path.ends_with(".ts")) {
        let compiler = typescript
            .ok_or("TypeScript compiler module path required for client adapter witnesses")?;
        let status = std::process::Command::new("node")
            .arg(root.join("scripts").join("ci").join("check_high_risk_client_bindings.cjs"))
            .arg(root)
            .arg(compiler)
            .status()?;
        if !status.success() {
            return Err("client adapter AST proof failed or unavailable".into());
        }
    }
    for row in &projection.rows {
        check_storage_join(row, &projection)?;
        for reference in row.writers.iter().chain(&row.consumers) {
            if !projection.witnesses.contains_key(reference) {
                return Err(format!("{}: missing witness {reference}", row.id).into());
            }
        }
        for field in &row.projections {
            let document: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(root.join(&field.path))?)?;
            if document.pointer(&field.pointer).is_some() != field.present {
                return Err(format!(
                    "{}: projection drift {}{}",
                    row.id, field.path, field.pointer
                )
                .into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn witness() -> Witness {
        Witness {
            path: "fixture.rs".into(),
            function: "Config::update".into(),
            expression: "if let Some(v) = input.get(\"engine\") { self.engine = v; }".into(),
        }
    }
    #[test]
    fn parser_key_destination_and_function_are_one_binding() -> CheckResult {
        let source = "impl Config { fn update(&mut self, input: Value) { if let Some(v) = input.get(\"engine\") { self.engine = v; } } }";
        check_witness(source, &witness())?;
        for changed in [
            source.replace("get(\"engine\")", "get(\"other\")"),
            source.replace("self.engine = v", "self.other = v"),
            source.replace("fn update", "fn another"),
        ] {
            let changed = format!(
                "{changed}\n// input.get(\"engine\"); self.engine = v;\n#[cfg(test)] mod tests {{ fn update() {{ if let Some(v) = input.get(\"engine\") {{ self.engine = v; }} }} }}"
            );
            if check_witness(&changed, &witness()).is_ok() {
                return Err("accepted moved key, destination or function".into());
            }
        }
        Ok(())
    }
    #[test]
    fn disconnected_consumer_cannot_be_replaced_by_comment_or_other_function() -> CheckResult {
        let witness = Witness {
            path: "fixture.rs".into(),
            function: "consume".into(),
            expression: "run(config.engine)".into(),
        };
        check_witness("fn consume(){ run(config.engine); }", &witness)?;
        if check_witness("fn consume(){ run(default_engine); /* run(config.engine) */ } fn elsewhere(){ run(config.engine); }", &witness).is_ok() { return Err("accepted disconnected consumer".into()); }
        if check_witness("fn consume(){ #[cfg(test)] { run(config.engine); } }", &witness).is_ok() {
            return Err("test-only consumer satisfied production binding".into());
        }
        Ok(())
    }

    #[test]
    fn compound_test_gates_and_serde_container_mapping_cannot_satisfy_bindings() -> CheckResult {
        let consumer = Witness {
            path: "fixture.rs".into(),
            function: "consume".into(),
            expression: "run(config.engine)".into(),
        };
        check_witness("fn consume(){ run(config.engine); }", &consumer)?;
        for source in [
            "#[cfg(all(test, unix))] fn consume(){ run(config.engine); }",
            "fn consume(){ #[cfg(any(test, feature=\"fixture\"))] { run(config.engine); } }",
            // Eligibility deliberately excludes every test-dependent shape;
            // ignoring all not(...) lists would admit double-negated test code.
            "#[cfg(not(test))] fn consume(){ run(config.engine); }",
            "#[cfg(not(not(test)))] fn consume(){ run(config.engine); }",
        ] {
            if check_witness(source, &consumer).is_ok() {
                return Err("compound test gate satisfied production binding".into());
            }
        }
        let field = Witness {
            path: "fixture.rs".into(),
            function: "@serde-field:Config.target_version".into(),
            expression: "Option<String>".into(),
        };
        let source = "#[derive(serde::Deserialize)] #[serde(default)] struct Config { target_version: Option<String> }";
        check_witness(source, &field)?;
        for mapping in ["rename_all=\"camelCase\"", "from=\"Other\"", "transparent"] {
            let changed = source.replace("#[serde(default)]", &format!("#[serde({mapping})]"));
            if check_witness(&changed, &field).is_ok() {
                return Err("serde container mapping drift accepted".into());
            }
        }
        Ok(())
    }

    #[test]
    fn test_gated_initializers_arms_and_field_values_are_not_production_bindings() -> CheckResult {
        let witness = Witness {
            path: "fixture.rs".into(),
            function: "consume".into(),
            expression: "run(config.engine)".into(),
        };
        for body in [
            "ATTR let value = run(config.engine);",
            "ATTR let Some(value) = input else { run(config.engine); return; };",
            "match input { ATTR Some(_) => run(config.engine), _ => (), }",
            "let value = Record { ATTR field: run(config.engine) };",
            "ATTR return run(config.engine);",
            "ATTR loop { run(config.engine); break; }",
            "ATTR while ready { run(config.engine); }",
            "ATTR for value in values { run(config.engine); }",
            "ATTR async { run(config.engine); };",
            "ATTR unsafe { run(config.engine); }",
        ] {
            let positive = format!("fn consume() {{ {} }}", body.replace("ATTR", ""));
            check_witness(&positive, &witness)?;
            for attribute in
                ["#[cfg(test)]", "#[cfg(all(test, unix))]", "#[cfg_attr(test, allow(unused))]"]
            {
                let changed = format!(
                    "fn consume() {{ {} }}\n// run(config.engine)\nfn elsewhere() {{ run(config.engine); }}",
                    body.replace("ATTR", attribute)
                );
                if check_witness(&changed, &witness).is_ok() {
                    return Err(format!("test-gated binding accepted: {body} {attribute}").into());
                }
            }
        }
        Ok(())
    }

    #[test]
    fn documented_high_risk_families_and_explicit_low_risk_retirements_are_checked() -> CheckResult
    {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).parent().ok_or("missing repository root")?;
        let projection: Projection =
            serde_json::from_str(&fs::read_to_string(root.join(PROJECTION))?)?;
        for heading in [
            "#### `perl.formatting.engine`",
            "#### `perl.perlcritic.enabled`",
            "#### `perl.formatting.indentColumns`",
            "#### `perl.workspace.discoveryExtensions`",
            "#### `perl.testRunner.command` (removed)",
            "#### `perl.testRunner.args` (removed)",
        ] {
            docs_coverage(heading, &projection)?;
        }
        for heading in [
            "#### `perl.formatting.unboundNewField`",
            "#### `perl.perlcritic.unboundNewField`",
            "#### `perl.testRunner.unboundNewField` (removed)",
            "#### `perl.testRunner.command`",
            "#### `perl.testRunner.args`",
        ] {
            if docs_coverage(heading, &projection).is_ok() {
                return Err(format!("unowned or resurrected docs field accepted: {heading}").into());
            }
        }
        let mut missing = projection.clone();
        missing.remaining_low_risk.retain(|id| id != "formatting.indent_columns");
        if docs_coverage("#### `perl.formatting.indentColumns`", &missing).is_ok() {
            return Err("unowned low-risk docs field accepted".into());
        }
        Ok(())
    }

    #[test]
    fn matches_storage_reads_parse_patterns_and_guards_without_accepting_residue() -> CheckResult {
        fn reads(source: &str) -> CheckResult<bool> {
            let expression = syn::parse_str::<syn::Expr>(source)?;
            let mut usage = StorageUse { member: "engine", writes: false, reads: false };
            usage.visit_expr(&expression);
            Ok(usage.reads)
        }
        for source in [
            "matches!(config.engine, _)",
            "matches!(config.engine, Some(_))",
            "matches!(config.engine, Some(value @ 1..=3) | None,)",
            "matches!(config.engine, | Some(_) | None)",
            "matches!(other, Some(value) if config.engine.accepts(value),)",
        ] {
            if !reads(source)? {
                return Err(format!("valid matches! read missed: {source}").into());
            }
            let without_read = source.replace("config.engine", "default_engine");
            if reads(&format!("{without_read} /* config.engine */"))? {
                return Err("comment supplied missing macro storage read".into());
            }
        }
        for source in [
            "matches!(other, Some(engine))",
            "matches!(config.engine, Some(_) trailing)",
            "matches!(config.engine, Some(_) if)",
            "matches!(config.engine, Some(_), extra)",
        ] {
            if reads(source)? {
                return Err(format!(
                    "pattern binding or malformed macro supplied storage read: {source}"
                )
                .into());
            }
        }
        Ok(())
    }

    #[test]
    fn assignment_places_do_not_supply_consumer_reads() -> CheckResult {
        for (source, writes, reads) in [
            ("config.engine = default_engine", true, false),
            ("(config.engine) = default_engine", true, false),
            ("(config.engine, other) = values", true, false),
            ("[config.engine, other] = values", true, false),
            ("Record { field: config.engine } = value", true, false),
            ("config.engine.option = value", true, false),
            ("config.engine[0] = value", true, false),
            ("config.engine[0].option = value", true, false),
            ("config.engine.options[0] = value", true, false),
            ("config.engine[config.engine.index] = value", true, true),
            ("config.engine.receiver().option = value", false, true),
            ("receiver(config.engine)[0] = value", false, true),
            ("config.engine.option += value", true, true),
            ("config.engine[0] += value", true, true),
            ("config.engine = previous.engine", true, true),
            ("config.other = config.engine", false, true),
            ("receiver(config.engine).other = value", false, true),
            ("values[config.engine] = value", false, true),
            ("*config.engine = value", false, true),
            ("config.engine += value", true, true),
            ("run(config.engine)", false, true),
        ] {
            let expression = syn::parse_str::<syn::Expr>(source)?;
            let mut usage = StorageUse { member: "engine", writes: false, reads: false };
            usage.visit_expr(&expression);
            if (usage.writes, usage.reads) != (writes, reads) {
                return Err(
                    format!("incorrect assignment read/write classification: {source}").into()
                );
            }
        }
        Ok(())
    }

    #[test]
    fn removed_key_literals_are_rejected_across_parser_syntax() -> CheckResult {
        let witness = Witness {
            path: "fixture.rs".into(),
            function: "@no-key:parse".into(),
            expression: "testRunner".into(),
        };
        for body in [
            "let value = input.get(\"testRunner\");",
            "let value = input[\"testRunner\"] ;",
            "let value = input.get_mut(\"testRunner\");",
            "let value = input.remove(\"testRunner\");",
            "let value = input.pointer(\"/testRunner/enabled\");",
            "let value = json!({\"testRunner\": false});",
            "let value = input.get(r#\"testRunner\"#);",
            "#[derive(Deserialize)] struct Payload { #[serde(rename = \"testRunner\")] runner: bool }",
        ] {
            let source = format!("fn parse() {{ {body} }}");
            if check_witness(&source, &witness).is_ok() {
                return Err(format!("removed key literal accepted: {body}").into());
            }
            check_witness(&source.replace("testRunner", "otherSetting"), &witness)?;
        }
        check_witness(
            "fn parse() { /* input.get(\"testRunner\") */ } fn elsewhere() { input.get(\"testRunner\"); }",
            &witness,
        )?;
        Ok(())
    }

    #[test]
    fn required_merge_surface_executes_tests_and_actual_bindings() -> CheckResult {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).parent().ok_or("missing repository root")?;
        let workflow: serde_yaml_ng::Value = serde_yaml_ng::from_str(&fs::read_to_string(
            root.join(".github").join("workflows").join("ci.yml"),
        )?)?;
        let job = workflow
            .get("jobs")
            .and_then(|jobs| jobs.get("check-all-targets"))
            .ok_or("missing required merge surface")?;
        if job.get("continue-on-error").is_some() {
            return Err("binding job cannot be advisory".into());
        }
        let steps = job
            .get("steps")
            .and_then(serde_yaml_ng::Value::as_sequence)
            .ok_or("missing gate steps")?;
        fn validate_steps(steps: &[serde_yaml_ng::Value]) -> CheckResult {
            let expected = [
                ("uses", "./.github/actions/setup-vscode-toolchain"),
                ("run", "cargo test -p xtask --bin high-risk-configuration-bindings --locked"),
                (
                    "run",
                    "cargo run -p xtask --bin high-risk-configuration-bindings --locked -- . vscode-extension/node_modules/typescript",
                ),
            ];
            let mut previous = None;
            for (key, value) in expected {
                let matches: Vec<_> = steps
                    .iter()
                    .enumerate()
                    .filter(|(_, step)| {
                        step.get(key).and_then(serde_yaml_ng::Value::as_str) == Some(value)
                    })
                    .collect();
                let (index, step) =
                    matches.first().copied().ok_or("missing binding execution step")?;
                if matches.len() != 1
                    || previous.is_some_and(|earlier| earlier >= index)
                    || step.get("if").is_some()
                    || step.get("continue-on-error").is_some()
                {
                    return Err(
                        "binding setup/execution is optional, duplicated or out of order".into()
                    );
                }
                if key == "uses" && step.get("with").is_some() {
                    return Err("binding setup must retain locked install defaults".into());
                }
                previous = Some(index);
            }
            Ok(())
        }
        validate_steps(steps)?;
        let mut without_execution = steps.clone();
        without_execution.retain(|step| {
            !step.get("run").and_then(serde_yaml_ng::Value::as_str).is_some_and(|command| {
                command.starts_with("cargo run -p xtask --bin high-risk-configuration-bindings")
            })
        });
        if validate_steps(&without_execution).is_ok() {
            return Err("missing actual execution accepted".into());
        }
        let mut reordered = steps.clone();
        reordered.reverse();
        if validate_steps(&reordered).is_ok() {
            return Err("execution before setup accepted".into());
        }
        for guard in ["if", "continue-on-error"] {
            let mut optional = steps.clone();
            let step = optional
                .iter_mut()
                .find(|step| {
                    step.get("run").and_then(serde_yaml_ng::Value::as_str).is_some_and(|command| {
                        command.starts_with(
                            "cargo run -p xtask --bin high-risk-configuration-bindings",
                        )
                    })
                })
                .and_then(serde_yaml_ng::Value::as_mapping_mut)
                .ok_or("missing actual execution mapping")?;
            step.insert(
                serde_yaml_ng::Value::String(guard.into()),
                serde_yaml_ng::Value::Bool(true),
            );
            if validate_steps(&optional).is_ok() {
                return Err(format!("optional execution accepted: {guard}").into());
            }
        }
        Ok(())
    }

    #[test]
    fn new_schema_field_requires_a_runtime_or_retirement_disposition() -> CheckResult {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).parent().ok_or("missing repository root")?;
        let projection: Projection =
            serde_json::from_str(&fs::read_to_string(root.join(PROJECTION))?)?;
        let mut schema: serde_json::Value = serde_json::from_str(&fs::read_to_string(
            root.join("schemas").join("perllsp-settings.schema.json"),
        )?)?;
        schema_coverage(&schema, &projection)?;
        schema
            .pointer_mut("/properties/perl/properties/aiCompletion/properties")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or("AI schema properties missing")?
            .insert("unboundNewField".into(), serde_json::json!({"type":"string"}));
        if schema_coverage(&schema, &projection).is_ok() {
            return Err("schema-only field escaped disposition check".into());
        }
        docs_coverage("#### `perl.limits.completionCap`", &projection)?;
        if docs_coverage("#### `perl.limits.unboundNewField`", &projection).is_ok() {
            return Err("docs-only field escaped disposition check".into());
        }
        Ok(())
    }
}
