//! Lightweight value-shape inference from Perl AST patterns.
//!
//! Walks the AST to infer [`ValueShape`] approximations for variables based
//! on common Perl idioms:
//!
//! | Perl pattern                             | Inferred shape                                |
//! |------------------------------------------|-----------------------------------------------|
//! | `Foo->new(...)`                          | `Object { "Foo", Medium }`                    |
//! | `bless $ref, 'Pkg'`                      | `Object { "Pkg", Low }`                       |
//! | `$self` in method body                   | `Object { <enclosing package>, Medium }`       |
//! | `sub method($self, ...)`                 | `Object { <enclosing package>, High }`         |
//! | `my ($self) = @_`                        | `Object { <enclosing package>, Medium }`       |
//! | `DBI->connect(...)`                      | `Object { "DBI::db", Medium }`                |
//! | `$dbh->prepare(...)` after DBI connect    | `Object { "DBI::st", Medium }`                |
//! | unknown                                  | `Unknown`                                     |
//!
//! The inferrer does **not** perform full type inference — it recognises
//! syntactic patterns and assigns conservative shapes.

use crate::ast::{Node, NodeKind};
use perl_semantic_facts::{Confidence, EntityId, FileId, ValueShape};
use std::collections::HashMap;

/// Inferrer that walks an AST to produce `(EntityId, ValueShape)` pairs.
///
/// Each pair maps a variable's deterministic entity ID to its inferred shape.
pub struct ValueShapeInferrer;

impl ValueShapeInferrer {
    /// Walk the entire AST and return `(EntityId, ValueShape)` pairs for
    /// every variable whose shape can be inferred from syntactic patterns.
    pub fn infer(ast: &Node, _file_id: FileId) -> Vec<(EntityId, ValueShape)> {
        Self::infer_state(ast).results
    }

    /// Name-keyed defining-class invocant shapes visible at `position`.
    ///
    /// [`named_shapes`] is method-scoped and restored at subroutine exit, so
    /// invocants would otherwise disappear. This map copies declared invocant
    /// shapes (`sub ($self)`, `my ($self) = @_`, `my $self = shift`) from the
    /// innermost callable whose span contains `position`. File-level sites and
    /// later packages cannot reuse an exited method's `$self`.
    pub fn named_invocant_shapes_at(ast: &Node, position: usize) -> HashMap<String, ValueShape> {
        Self::infer_state_at(ast, position).invocant_shapes
    }

    /// Name-keyed shapes from the same walk as [`infer`].
    ///
    /// Last write wins for a repeated scalar name. Method-local maps are
    /// restored at subroutine exit, matching the inferrer's lexical scope.
    pub fn named_shapes(ast: &Node) -> HashMap<String, ValueShape> {
        Self::infer_state(ast).variable_shapes
    }

    /// True for the defining-class invocant names this inferrer already records.
    pub fn is_defining_class_invocant_name(name: &str) -> bool {
        is_self_like_name(name)
    }

    fn infer_state(ast: &Node) -> InferrerState {
        Self::infer_state_at(ast, usize::MAX)
    }

    fn infer_state_at(ast: &Node, position: usize) -> InferrerState {
        let mut state = InferrerState {
            current_package: "main".to_string(),
            in_method: false,
            position,
            captured_at_position: false,
            variable_shapes: HashMap::new(),
            declared_invocants: HashMap::new(),
            invocant_shapes: HashMap::new(),
            results: Vec::new(),
        };
        state.walk(ast);
        state
    }
}

/// Internal state for the recursive AST walk.
struct InferrerState {
    /// Current package context (updated when `package Foo;` is encountered).
    current_package: String,
    /// Whether we are currently inside a subroutine/method body.
    in_method: bool,
    /// Byte offset of the call site consuming invocant shapes.
    position: usize,
    /// True once the innermost callable containing [`position`](Self::position)
    /// has copied its declared invocants into [`invocant_shapes`](Self::invocant_shapes).
    captured_at_position: bool,
    /// Current lexical receiver-shape environment, keyed by scalar variable name.
    variable_shapes: HashMap<String, ValueShape>,
    /// Declared invocants in the current callable (signature / `@_` / `shift`).
    declared_invocants: HashMap<String, ValueShape>,
    /// Declared invocants from the innermost callable whose span contains
    /// [`position`](Self::position).
    invocant_shapes: HashMap<String, ValueShape>,
    /// Accumulated (EntityId, ValueShape) pairs.
    results: Vec<(EntityId, ValueShape)>,
}

impl InferrerState {
    /// Recursive AST walker.
    fn walk(&mut self, node: &Node) {
        match &node.kind {
            // Statement containers — walk children in order.
            NodeKind::Program { statements } | NodeKind::Block { statements } => {
                for stmt in statements {
                    self.walk(stmt);
                }
                return;
            }

            // `package Foo { ... }` (block form) — scoped package context.
            NodeKind::Package { name, block: Some(block), .. } => {
                let prev = self.current_package.clone();
                self.current_package = name.clone();
                self.walk(block);
                self.current_package = prev;
                return;
            }

            // `package Foo;` (semicolon form) — updates current package.
            NodeKind::Package { name, block: None, .. } => {
                self.current_package = name.clone();
                return;
            }

            // Subroutine / method body — track method-like scope, record
            // signature receivers, and keep receiver shapes local to the body.
            NodeKind::Subroutine { signature, body, .. }
            | NodeKind::Method { signature, body, .. } => {
                let prev_in_method = self.in_method;
                let prev_shapes = std::mem::take(&mut self.variable_shapes);
                let prev_declared = std::mem::take(&mut self.declared_invocants);
                self.in_method = true;
                if let Some(signature) = signature {
                    self.record_signature_receiver(signature);
                }
                self.walk(body);
                if self.span_contains_position(node) && !self.captured_at_position {
                    self.invocant_shapes = self.declared_invocants.clone();
                    self.captured_at_position = true;
                }
                self.in_method = prev_in_method;
                self.variable_shapes = prev_shapes;
                self.declared_invocants = prev_declared;
                return;
            }

            // Variable declaration with initializer:
            // `my $obj = Foo->new(...)` or `my $obj = bless ...`
            NodeKind::VariableDeclaration { variable, initializer: Some(init), .. } => {
                if self.in_method && is_argument_shift(init) {
                    self.record_self_like_variable(variable, Confidence::Medium, true);
                } else if let Some(shape) = self.infer_from_rhs(init) {
                    self.record_variable_shape(variable, shape);
                }
            }

            // List unpacking convention for invocants:
            // `my ($self) = @_`.
            NodeKind::VariableListDeclaration { variables, initializer: Some(init), .. }
                if self.in_method && is_argument_array(init) =>
            {
                if let Some(first) = variables.first() {
                    self.record_self_like_variable(first, Confidence::Medium, true);
                }
            }

            // Assignment: `$obj = Foo->new(...)` or `$obj = bless ...`
            NodeKind::Assignment { lhs, rhs, .. } => {
                if let Some(shape) = self.infer_from_rhs(rhs) {
                    self.record_variable_shape(lhs, shape);
                }
            }

            // `$self` reference inside a method body. This remains a value-shape
            // heuristic for `infer()`, but it is not a declared invocant fact.
            NodeKind::Variable { sigil, name }
                if sigil == "$" && is_self_like_name(name) && self.in_method =>
            {
                self.record_self_like_variable(node, Confidence::Medium, false);
            }

            _ => {}
        }

        // Recurse into children for all other node types.
        for child in node.children() {
            self.walk(child);
        }
    }

    /// Try to infer a [`ValueShape`] from the right-hand side of an
    /// assignment or variable declaration.
    fn infer_from_rhs(&self, rhs: &Node) -> Option<ValueShape> {
        match &rhs.kind {
            // `Foo->new(...)` — constructor call.
            NodeKind::MethodCall { object, method, .. } if method == "new" => {
                if let Some(pkg) = package_name_from_node(object) {
                    return Some(ValueShape::Object {
                        package: pkg,
                        confidence: Confidence::Medium,
                    });
                }
                None
            }

            // `DBI->connect(...)` — common DBI database handle constructor.
            NodeKind::MethodCall { object, method, .. } if method == "connect" => {
                if package_name_from_node(object).as_deref() == Some("DBI") {
                    return Some(ValueShape::Object {
                        package: "DBI::db".to_string(),
                        confidence: Confidence::Medium,
                    });
                }
                None
            }

            // `$dbh->prepare(...)` — common DBI statement handle constructor.
            NodeKind::MethodCall { object, method, .. } if method == "prepare" => {
                if self.receiver_is_dbi_database_handle(object) {
                    return Some(ValueShape::Object {
                        package: "DBI::st".to_string(),
                        confidence: Confidence::Medium,
                    });
                }
                None
            }

            // `bless $ref, 'Pkg'` — bless call.
            NodeKind::FunctionCall { name, args } if name == "bless" => {
                // Second argument is the package name.
                if let Some(pkg_node) = args.get(1)
                    && let Some(pkg) = string_value(pkg_node)
                {
                    return Some(ValueShape::Object { package: pkg, confidence: Confidence::Low });
                }
                // `bless $ref` with no explicit package — uses current package.
                if args.len() == 1 {
                    return Some(ValueShape::Object {
                        package: self.current_package.clone(),
                        confidence: Confidence::Low,
                    });
                }
                None
            }

            _ => None,
        }
    }

    fn record_signature_receiver(&mut self, signature: &Node) {
        let NodeKind::Signature { parameters } = &signature.kind else {
            return;
        };
        let Some(first) = parameters.first() else {
            return;
        };
        let Some(variable) = parameter_variable(first) else {
            return;
        };
        self.record_self_like_variable(variable, Confidence::High, true);
    }

    fn record_self_like_variable(
        &mut self,
        variable: &Node,
        confidence: Confidence,
        declared: bool,
    ) {
        let Some(name) = scalar_variable_name(variable) else {
            return;
        };
        if !is_self_like_name(name) {
            return;
        }

        let shape = ValueShape::Object { package: self.current_package.clone(), confidence };
        self.record_variable_shape(variable, shape.clone());
        if declared {
            self.declared_invocants.insert(name.to_string(), shape);
        }
    }

    fn span_contains_position(&self, node: &Node) -> bool {
        node.location.start <= self.position && self.position <= node.location.end
    }

    fn record_variable_shape(&mut self, variable: &Node, shape: ValueShape) {
        if let Some(name) = scalar_variable_name(variable) {
            self.variable_shapes.insert(name.to_string(), shape.clone());
        }
        let entity_id = entity_id_from_variable(variable);
        self.results.push((entity_id, shape));
    }

    fn receiver_is_dbi_database_handle(&self, receiver: &Node) -> bool {
        let Some(name) = scalar_variable_name(receiver) else {
            return false;
        };

        self.variable_shapes.get(name).is_some_and(
            |shape| matches!(shape, ValueShape::Object { package, .. } if package == "DBI::db"),
        )
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Extract a package name from a node that represents a class/package
/// (e.g. the `Foo` in `Foo->new`).
fn package_name_from_node(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::Identifier { name } => Some(name.clone()),
        NodeKind::String { value, .. } => normalize_package_string(value),
        _ => None,
    }
}

/// Extract a string value from a string literal node.
fn string_value(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::String { value, .. } => normalize_package_string(value),
        NodeKind::Identifier { name } => Some(name.clone()),
        _ => None,
    }
}

fn scalar_variable_name(node: &Node) -> Option<&str> {
    match &node.kind {
        NodeKind::Variable { sigil, name } if sigil == "$" => Some(name.as_str()),
        NodeKind::VariableWithAttributes { variable, .. } => scalar_variable_name(variable),
        _ => None,
    }
}

fn parameter_variable(node: &Node) -> Option<&Node> {
    match &node.kind {
        NodeKind::MandatoryParameter { variable }
        | NodeKind::OptionalParameter { variable, .. }
        | NodeKind::SlurpyParameter { variable }
        | NodeKind::NamedParameter { variable, .. } => Some(variable),
        _ => None,
    }
}

fn is_self_like_name(name: &str) -> bool {
    matches!(name, "self" | "this" | "class")
}

fn is_argument_array(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Variable { sigil, name } if sigil == "@" && name == "_")
}

fn is_argument_shift(node: &Node) -> bool {
    match &node.kind {
        NodeKind::FunctionCall { name, args } if name == "shift" => {
            args.is_empty() || args.iter().any(is_argument_array)
        }
        _ => false,
    }
}

fn normalize_package_string(value: &str) -> Option<String> {
    let normalized = value.trim().trim_matches('\'').trim_matches('"').trim();
    if normalized.is_empty() { None } else { Some(normalized.to_string()) }
}

/// Derive a deterministic [`EntityId`] from a variable node using its
/// byte-offset span.
fn entity_id_from_variable(node: &Node) -> EntityId {
    entity_id_from_node(node)
}

/// Derive a deterministic [`EntityId`] from a node's byte-offset span.
///
/// Uses a simple FNV-1a–style hash to produce a stable ID.
fn entity_id_from_node(node: &Node) -> EntityId {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0100_0000_01b3;

    let mut hash = FNV_OFFSET;
    for byte in (node.location.start as u64).to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    for byte in (node.location.end as u64).to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    EntityId(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    /// Parse Perl source and infer value shapes.
    fn parse_and_infer(code: &str) -> Vec<(EntityId, ValueShape)> {
        let mut parser = Parser::new(code);
        let ast = match parser.parse() {
            Ok(ast) => ast,
            Err(_) => return Vec::new(),
        };
        ValueShapeInferrer::infer(&ast, FileId(1))
    }

    /// Helper: find the first Object shape in results.
    fn first_object(results: &[(EntityId, ValueShape)]) -> Option<(&str, Confidence)> {
        for (_, shape) in results {
            if let ValueShape::Object { package, confidence } = shape {
                return Some((package.as_str(), *confidence));
            }
        }
        None
    }

    fn object_for_package(
        results: &[(EntityId, ValueShape)],
        expected_package: &str,
    ) -> Option<Confidence> {
        results.iter().find_map(|(_, shape)| {
            if let ValueShape::Object { package, confidence } = shape
                && package == expected_package
            {
                return Some(*confidence);
            }
            None
        })
    }

    // ── Constructor call: Foo->new(...) ─────────────────────────────────

    #[test]
    fn constructor_call_infers_object_medium() -> Result<(), String> {
        let results = parse_and_infer("my $obj = Foo->new();\n");
        let (pkg, conf) = first_object(&results).ok_or("expected Object shape from Foo->new()")?;
        assert_eq!(pkg, "Foo");
        assert_eq!(conf, Confidence::Medium);
        Ok(())
    }

    #[test]
    fn qualified_constructor_call_infers_object() -> Result<(), String> {
        let results = parse_and_infer("my $obj = My::App->new();\n");
        let (pkg, conf) =
            first_object(&results).ok_or("expected Object shape from My::App->new()")?;
        assert_eq!(pkg, "My::App");
        assert_eq!(conf, Confidence::Medium);
        Ok(())
    }

    // ── bless $ref, 'Pkg' ───────────────────────────────────────────────

    #[test]
    fn bless_with_package_infers_object_low() -> Result<(), String> {
        let code = "package Foo;\nsub new { my $self = bless {}, 'Foo'; }\n";
        let results = parse_and_infer(code);
        let (pkg, conf) =
            first_object(&results).ok_or("expected Object shape from bless {}, 'Foo'")?;
        assert_eq!(pkg, "Foo");
        assert_eq!(conf, Confidence::Low);
        Ok(())
    }

    // ── $self in method body ────────────────────────────────────────────

    #[test]
    fn self_in_method_infers_enclosing_package() -> Result<(), String> {
        let code = "package Bar;\nsub greet { my $msg = $self->name(); }\n";
        let results = parse_and_infer(code);
        // $self should be inferred as Object { Bar, Medium }
        let has_bar_medium = results.iter().any(|(_, shape)| {
            matches!(shape, ValueShape::Object { package, confidence }
                if package == "Bar" && *confidence == Confidence::Medium)
        });
        assert!(
            has_bar_medium,
            "expected $self to infer Object {{ Bar, Medium }}, got {results:?}"
        );
        Ok(())
    }

    #[test]
    fn signature_self_infers_enclosing_package_high() -> Result<(), String> {
        let code = "package Widget;\nsub render($self, $name) { return $name; }\n";
        let results = parse_and_infer(code);
        let confidence =
            object_for_package(&results, "Widget").ok_or("expected signature self shape")?;
        assert_eq!(confidence, Confidence::High);
        Ok(())
    }

    #[test]
    fn argument_unpack_self_infers_enclosing_package() -> Result<(), String> {
        let code = "package Widget;\nsub render { my ($self, $name) = @_; return $name; }\n";
        let results = parse_and_infer(code);
        let confidence =
            object_for_package(&results, "Widget").ok_or("expected @_ self unpack shape")?;
        assert_eq!(confidence, Confidence::Medium);
        Ok(())
    }

    #[test]
    fn named_shapes_expose_signature_class_and_ignore_bare_self() -> Result<(), String> {
        let source = "package Widget;\nsub new($class) { return 1; }\n$self->name;\n";
        let mut parser = Parser::new(source);
        let ast = parser.parse().map_err(|err| format!("parse failed: {err:?}"))?;
        let class_pos =
            source.find("$class").ok_or_else(|| "expected $class in fixture".to_string())?;
        let named = ValueShapeInferrer::named_invocant_shapes_at(&ast, class_pos);
        let class = named
            .get("class")
            .ok_or_else(|| format!("expected named $class invocant shape, got {named:?}"))?;
        assert!(
            matches!(class, ValueShape::Object { package, confidence }
                if package == "Widget" && *confidence == Confidence::High),
            "signature $class should be Widget/High, got {class:?}"
        );
        let file_self_pos = source
            .rfind("$self")
            .ok_or_else(|| "expected file-level $self in fixture".to_string())?;
        let at_file_self = ValueShapeInferrer::named_invocant_shapes_at(&ast, file_self_pos);
        assert!(
            !at_file_self.contains_key("self"),
            "file-level $self must not become an invocant shape: {at_file_self:?}"
        );
        Ok(())
    }

    #[test]
    fn named_invocant_shapes_at_earlier_method_ignore_later_package() -> Result<(), String> {
        let source = concat!(
            "package Animal;\n",
            "sub speak { my ($self) = @_; $self->name; }\n",
            "package Other;\n",
            "sub fetch { my ($self) = @_; $self->name; }\n",
        );
        let mut parser = Parser::new(source);
        let ast = parser.parse().map_err(|err| format!("parse failed: {err:?}"))?;
        let animal_pos =
            source.find("$self->name").ok_or_else(|| "expected Animal $self-> site".to_string())?;
        let other_pos =
            source.rfind("$self->name").ok_or_else(|| "expected Other $self-> site".to_string())?;
        let at_animal = ValueShapeInferrer::named_invocant_shapes_at(&ast, animal_pos);
        let at_other = ValueShapeInferrer::named_invocant_shapes_at(&ast, other_pos);
        assert!(
            matches!(at_animal.get("self"), Some(ValueShape::Object { package, .. }) if package == "Animal"),
            "earlier $self must stay Animal, got {at_animal:?}"
        );
        assert!(
            matches!(at_other.get("self"), Some(ValueShape::Object { package, .. }) if package == "Other"),
            "later $self must stay Other, got {at_other:?}"
        );
        Ok(())
    }

    #[test]
    fn named_invocant_shapes_at_do_not_survive_exited_method() -> Result<(), String> {
        let source = concat!(
            "package Animal {\n",
            "    sub speak { my ($self) = @_; }\n",
            "}\n",
            "$self->name;\n",
        );
        let mut parser = Parser::new(source);
        let ast = parser.parse().map_err(|err| format!("parse failed: {err:?}"))?;
        let file_self_pos =
            source.rfind("$self").ok_or_else(|| "expected file-level $self".to_string())?;
        let at_file = ValueShapeInferrer::named_invocant_shapes_at(&ast, file_self_pos);
        assert!(
            !at_file.contains_key("self"),
            "exited package-block $self must not remain an invocant: {at_file:?}"
        );
        Ok(())
    }

    #[test]
    fn named_invocant_shapes_at_ignore_unbound_numeric_self() -> Result<(), String> {
        let source = "package Animal;\nsub helper { my $self = 42; $self->name; }\n";
        let mut parser = Parser::new(source);
        let ast = parser.parse().map_err(|err| format!("parse failed: {err:?}"))?;
        let pos = source.find("$self->name").ok_or_else(|| "expected $self-> site".to_string())?;
        let named = ValueShapeInferrer::named_invocant_shapes_at(&ast, pos);
        assert!(!named.contains_key("self"), "my $self = 42 is not a declared invocant: {named:?}");
        Ok(())
    }

    // ── DBI receiver-shape idioms ───────────────────────────────────────

    #[test]
    fn dbi_connect_infers_database_handle() -> Result<(), String> {
        let results = parse_and_infer("my $dbh = DBI->connect('dbi:SQLite:dbname=:memory:');\n");
        let confidence =
            object_for_package(&results, "DBI::db").ok_or("expected DBI::db handle shape")?;
        assert_eq!(confidence, Confidence::Medium);
        Ok(())
    }

    #[test]
    fn dbh_prepare_infers_statement_handle_after_connect() -> Result<(), String> {
        let code = "my $dbh = DBI->connect('dbi:SQLite:dbname=:memory:');\nmy $sth = $dbh->prepare('select 1');\n";
        let results = parse_and_infer(code);
        let confidence =
            object_for_package(&results, "DBI::st").ok_or("expected DBI::st statement shape")?;
        assert_eq!(confidence, Confidence::Medium);
        Ok(())
    }

    #[test]
    fn prepare_on_unknown_receiver_does_not_infer_statement_handle() -> Result<(), String> {
        let results = parse_and_infer("my $sth = $thing->prepare('select 1');\n");
        assert!(
            object_for_package(&results, "DBI::st").is_none(),
            "unknown prepare receiver should not infer DBI::st: {results:?}"
        );
        Ok(())
    }

    #[test]
    fn prepare_on_dbh_name_without_known_connect_does_not_infer_statement_handle()
    -> Result<(), String> {
        let results = parse_and_infer("my $sth = $dbh->prepare('select 1');\n");
        assert!(
            object_for_package(&results, "DBI::st").is_none(),
            "$dbh naming alone should not infer DBI::st: {results:?}"
        );
        Ok(())
    }

    // ── Unknown fallback ────────────────────────────────────────────────

    #[test]
    fn plain_scalar_produces_no_shape() -> Result<(), String> {
        let results = parse_and_infer("my $x = 42;\n");
        // No Object shapes should be inferred for a plain scalar.
        assert!(first_object(&results).is_none(), "plain scalar should not produce Object shape");
        Ok(())
    }

    // ── Multiple packages ───────────────────────────────────────────────

    #[test]
    fn multiple_packages_track_context() -> Result<(), String> {
        let code = r#"
package Alpha;
sub new { my $self = bless {}, 'Alpha'; }

package Beta;
sub new { my $self = bless {}, 'Beta'; }
"#;
        let results = parse_and_infer(code);
        let has_alpha = results.iter().any(
            |(_, shape)| matches!(shape, ValueShape::Object { package, .. } if package == "Alpha"),
        );
        let has_beta = results.iter().any(
            |(_, shape)| matches!(shape, ValueShape::Object { package, .. } if package == "Beta"),
        );
        assert!(has_alpha, "expected Alpha object shape");
        assert!(has_beta, "expected Beta object shape");
        Ok(())
    }
}
