use perl_ast::ast::{Node, NodeKind};
use std::ops::Range;

/// A statically observed `->import::into(...)` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportIntoCall {
    /// Source range of the call.
    pub range: Range<usize>,
    /// The module expression receiving `import::into`.
    pub source: ImportIntoSource,
    /// The destination package information proven from the first argument.
    pub target: ImportIntoTarget,
}

/// The statically known source module, or an expression that must remain dynamic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportIntoSource {
    /// A bare or qualified package name, such as `strict` or `Foo::Bar`.
    Package(String),
    /// A variable or other expression whose package cannot be proven statically.
    Dynamic,
}

/// The destination package information available without executing Perl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportIntoTarget {
    /// `import::into(N)` with a statically known caller depth.
    CallerDepth(u32),
    /// A literal package name supplied as the destination.
    Package(String),
    /// A caller expression, dynamic value, or missing destination.
    Dynamic,
}

/// Find every `->import::into(...)` call in an AST.
///
/// This is deliberately an observation API. It does not execute `import`, infer
/// the caller, or turn an arbitrary imported module into an exact pragma state.
/// Consumers can use the source and target facts to recognize safe common cases
/// while preserving a dynamic boundary for the rest.
#[must_use]
pub fn find_import_into_calls(ast: &Node) -> Vec<ImportIntoCall> {
    let mut calls = Vec::new();
    collect_import_into_calls(ast, &mut calls);
    calls
}

fn collect_import_into_calls(node: &Node, calls: &mut Vec<ImportIntoCall>) {
    if let NodeKind::MethodCall { object, method, args } = &node.kind
        && method == "import::into"
    {
        calls.push(ImportIntoCall {
            range: node.location.start()..node.location.end(),
            source: source_from_node(object),
            target: target_from_args(args),
        });
    }

    node.for_each_child(|child| collect_import_into_calls(child, calls));
}

fn source_from_node(node: &Node) -> ImportIntoSource {
    match &node.kind {
        NodeKind::Identifier { name } if is_package_name(name) => {
            ImportIntoSource::Package(name.clone())
        }
        _ => ImportIntoSource::Dynamic,
    }
}

fn target_from_args(args: &[Node]) -> ImportIntoTarget {
    match args.first().map(|node| &node.kind) {
        Some(NodeKind::Number { value }) => value
            .parse::<u32>()
            .map(ImportIntoTarget::CallerDepth)
            .unwrap_or(ImportIntoTarget::Dynamic),
        Some(NodeKind::String { value, .. }) if is_package_name(value) => {
            ImportIntoTarget::Package(value.clone())
        }
        _ => ImportIntoTarget::Dynamic,
    }
}

fn is_package_name(value: &str) -> bool {
    !value.is_empty()
        && value.split("::").all(|part| {
            !part.is_empty() && part.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        })
}
