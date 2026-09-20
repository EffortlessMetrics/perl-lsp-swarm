use crate::FieldId;

/// A descriptor for the Perl language as parsed by the native v3 engine.
///
/// Provides node kind names and field metadata for Rust-native tooling.
/// This is NOT a `tree_sitter::Language` — it does not require a C toolchain
/// and cannot be used with `tree_sitter::Parser::set_language`. For drop-in
/// tree-sitter compatibility use `tree-sitter-perl-c` instead.
///
/// # Example
///
/// ```rust
/// use tree_sitter_perl_rs::language;
///
/// let lang = language();
/// assert!(lang.node_kind_count() > 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerlLanguage {
    kind_names: &'static [&'static str],
    field_names: &'static [perl_ast::FieldId],
}

impl PerlLanguage {
    /// Returns the number of distinct node kinds in the grammar.
    pub fn node_kind_count(&self) -> usize {
        self.kind_names.len()
    }

    /// Returns all node kind names, in declaration order.
    ///
    /// The order matches the variant declaration order of [`perl_ast::NodeKind`].
    /// `ALL_KIND_NAMES` is auto-derived via `strum::VariantNames`; callers that
    /// need a sorted list should sort the returned slice themselves.
    pub fn node_kind_names(&self) -> &[&'static str] {
        self.kind_names
    }

    /// Returns `true` if the given kind name is a named (non-anonymous) node kind.
    pub fn node_kind_is_named(&self, kind: &str) -> bool {
        self.kind_names.contains(&kind)
    }

    /// Returns the stable named-field identifiers exposed by the AST.
    pub fn field_names(&self) -> &'static [FieldId] {
        self.field_names
    }

    /// Returns the field identifier for a canonical field name.
    pub fn field_id_for_name(&self, name: &str) -> Option<FieldId> {
        perl_ast::FieldId::from_name(name)
    }
}

impl Default for PerlLanguage {
    fn default() -> Self {
        LANGUAGE
    }
}

/// Returns the [`PerlLanguage`] descriptor for Rust-native tooling.
///
/// Note: This is NOT equivalent to `tree_sitter::Language`. See [`PerlLanguage`].
pub fn language() -> PerlLanguage {
    LANGUAGE
}

/// The [`PerlLanguage`] descriptor as a constant.
pub static LANGUAGE: PerlLanguage = PerlLanguage {
    kind_names: perl_ast::NodeKind::ALL_KIND_NAMES,
    field_names: perl_ast::FieldId::ALL,
};
