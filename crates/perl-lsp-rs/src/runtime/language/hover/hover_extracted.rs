use super::Value;

/// Intermediate result from phase-1 hover extraction (under document lock).
///
/// The document lock must be released before calling module resolution to avoid
/// deadlock, so we extract what we need first and resolve afterwards.
pub(super) enum HoverExtracted {
    /// Hover content fully built (symbol, builtin, or token hover).
    Complete(Value),
    /// A `use Module` was found; module name needs resolution without lock.
    /// Carries (module_name, doc_text, doc_uri, doc_offset) for use lib / FindBin wiring.
    UseModule(String, String, String, usize),
    /// Cursor is on a `->method()` call where the method belongs to an inherited or
    /// role-composed ancestor class. Carries
    /// (receiver_pkg, method_name, doc_uri, dynamic_fallback).
    /// Phase 2 resolves the hover using the workspace index BFS (same logic as
    /// `inherited_method_definition_location` in navigation.rs).
    ///
    /// `dynamic_fallback` carries an in-file `AUTOLOAD` card when the same-file
    /// analyzer could only answer with a dynamic boundary. Such an answer must not
    /// pre-empt phase 2: the receiver's ancestor may live in another file, where an
    /// exact method outranks `AUTOLOAD` in Perl's dispatch order, and the same-file
    /// class model cannot see it. Phase 2 therefore runs first and this card is used
    /// only if the workspace lookup yields nothing at all — for instance when the
    /// index has not settled.
    InheritedMethod(String, String, String, Option<Value>),
    /// Cursor is on a package-name token (contains `::`) that was not handled by an
    /// earlier semantic or `use` path. Carries (package_name, doc_text, doc_uri, doc_offset).
    /// Phase 2 resolves it via `build_module_hover` (same as `UseModule`).
    PossiblePackage(String, String, String, usize),
    /// Nothing hoverable at this position.
    None,
}
