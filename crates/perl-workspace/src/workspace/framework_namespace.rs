//! Reclassify indexed package namespaces using shared object-framework activation rules.
//!
//! Document symbols apply this upgrade inside [`SymbolExtractor`]; workspace indexing
//! must mirror it so `workspace/symbol` reports the same Class/Role kinds (#18342).

use perl_parser_core::ast::{Node, NodeKind};
use perl_symbol::{
    FrameworkKind, SymbolKind, classify_framework_module, namespace_symbol_kind_for_framework,
};
use std::collections::HashMap;

/// Map each package name to the namespace kind implied by its `use` statements.
#[must_use]
pub(super) fn framework_namespace_kinds_by_package(ast: &Node) -> HashMap<String, SymbolKind> {
    let mut collector = FrameworkNamespaceCollector::default();
    collector.visit(ast);
    collector.into_symbol_kinds()
}

#[derive(Default)]
struct FrameworkNamespaceCollector {
    current_package: String,
    kinds: HashMap<String, FrameworkKind>,
}

impl FrameworkNamespaceCollector {
    fn visit(&mut self, node: &Node) {
        match &node.kind {
            NodeKind::Program { statements } => {
                for statement in statements {
                    self.visit(statement);
                }
            }
            NodeKind::Package { name, block, .. } => {
                let previous = self.current_package.clone();
                self.current_package.clone_from(name);
                if let Some(body) = block {
                    self.visit(body);
                    self.current_package = previous;
                }
            }
            NodeKind::Use { module, .. } => {
                self.record_use_module(module);
            }
            _ => {
                node.for_each_child(|child| self.visit(child));
            }
        }
    }

    fn record_use_module(&mut self, module: &str) {
        if let Some(kind) = classify_framework_module(module) {
            self.kinds.insert(self.current_package.clone(), kind);
            return;
        }
        if matches!(module, "Class::Tiny" | "Class::Tiny::RW") {
            self.kinds.insert(self.current_package.clone(), FrameworkKind::ClassTiny);
        }
    }

    fn into_symbol_kinds(self) -> HashMap<String, SymbolKind> {
        self.kinds
            .into_iter()
            .map(|(package, kind)| (package, namespace_symbol_kind_for_framework(kind)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    use crate::workspace::workspace_index::{LspWorkspaceSymbol, WorkspaceIndex};

    #[test]
    fn moo_role_package_indexes_as_role_in_workspace_symbols()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "package age { use Moo::Role; sub helper { 1 } }";
        let uri = Url::parse("file:///workspace/role-symbol-0.pm")?;
        let index = WorkspaceIndex::new();
        index.index_file(uri, source.to_string())?;

        let matches: Vec<_> =
            index.search_symbols("age").into_iter().filter(|symbol| symbol.name == "age").collect();
        assert_eq!(matches.len(), 1, "expected one namespace symbol for age");
        assert_eq!(
            matches[0].kind,
            SymbolKind::Role,
            "workspace index must classify Moo::Role packages as Role (#18342)"
        );
        assert_eq!(LspWorkspaceSymbol::from(&matches[0]).kind, 11);
        Ok(())
    }

    #[test]
    fn plain_package_stays_module_kind_in_workspace_symbols()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "package age { use strict; sub helper { 1 } }";
        let uri = Url::parse("file:///workspace/plain-symbol.pm")?;
        let index = WorkspaceIndex::new();
        index.index_file(uri, source.to_string())?;

        let matches: Vec<_> =
            index.search_symbols("age").into_iter().filter(|symbol| symbol.name == "age").collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].kind, SymbolKind::Package);
        assert_eq!(LspWorkspaceSymbol::from(&matches[0]).kind, 2);
        Ok(())
    }
}
