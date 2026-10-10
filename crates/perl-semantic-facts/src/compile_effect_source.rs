/// Source construct that produced a compile effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum CompileEffectSourceKind {
    /// `package` declaration.
    PackageDecl,
    /// `sub` declaration.
    SubDecl,
    /// `method` declaration.
    MethodDecl,
    /// Variable declaration.
    VariableDecl,
    /// `use` directive.
    UseDirective,
    /// `no` directive.
    NoDirective,
    /// `require` directive.
    RequireDirective,
    /// Compile-time phase block.
    PhaseBlock,
    /// Symbolic-reference dereference.
    SymbolicReferenceDeref,
    /// Assignment expression.
    Assignment,
    /// Typeglob assignment.
    TypeglobAssignment,
    /// Derived HIR scope graph fact.
    ScopeGraph,
    /// Derived HIR stash graph fact.
    StashGraph,
    /// Derived compile-environment fact.
    CompileEnvironment,
}
