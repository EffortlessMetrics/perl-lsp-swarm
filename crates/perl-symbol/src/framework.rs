//! Object-framework activation mapping shared by workspace indexing and semantic extraction.
//!
//! [`classify_framework_module`] is the single authority for whether a `use`d module
//! spelling activates Moo/Moose/Role::Tiny DSL semantics on the enclosing package.

use crate::SymbolKind;

/// Classification of Moo/Moose framework variant detected via `use` statements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameworkKind {
    /// `use Moo;` or `use Mouse;`
    Moo,
    /// `use Moo::Role;` or `use Mouse::Role;`
    MooRole,
    /// `use Moose;`
    Moose,
    /// `use Moose::Role;`
    MooseRole,
    /// `use Role::Tiny;` — the package is a role
    RoleTiny,
    /// `use Role::Tiny::With;` — the package consumes roles
    RoleTinyWith,
    /// `use Class::Tiny;` or `use Class::Tiny::RW;`
    ClassTiny,
}

/// Classify a `use`d module name as an object-framework activation.
///
/// Returns `None` for modules that do not activate an object framework,
/// including `Class::Accessor`, which is tracked separately because it does
/// not import the Moo/Moose DSL keywords.
#[must_use]
pub fn classify_framework_module(module: &str) -> Option<FrameworkKind> {
    match module {
        "Moo" | "Mouse" => Some(FrameworkKind::Moo),
        "Moo::Role" | "Mouse::Role" => Some(FrameworkKind::MooRole),
        "Moose" => Some(FrameworkKind::Moose),
        "Moose::Role" => Some(FrameworkKind::MooseRole),
        "Role::Tiny" => Some(FrameworkKind::RoleTiny),
        "Role::Tiny::With" => Some(FrameworkKind::RoleTinyWith),
        _ => None,
    }
}

/// Map a framework activation to the namespace [`SymbolKind`] used by LSP projections.
#[must_use]
pub fn namespace_symbol_kind_for_framework(kind: FrameworkKind) -> SymbolKind {
    match kind {
        FrameworkKind::Moo
        | FrameworkKind::Moose
        | FrameworkKind::RoleTinyWith
        | FrameworkKind::ClassTiny => SymbolKind::Class,
        FrameworkKind::MooRole | FrameworkKind::MooseRole | FrameworkKind::RoleTiny => {
            SymbolKind::Role
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moo_role_maps_to_role_namespace_kind() {
        assert_eq!(
            namespace_symbol_kind_for_framework(
                classify_framework_module("Moo::Role").expect("Moo::Role activates")
            ),
            SymbolKind::Role
        );
    }

    #[test]
    fn class_tiny_maps_to_class_namespace_kind() {
        assert_eq!(
            namespace_symbol_kind_for_framework(FrameworkKind::ClassTiny),
            SymbolKind::Class
        );
    }
}
