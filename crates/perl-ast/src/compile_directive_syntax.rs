//! Typed source structure for `use` / `no` compile directives (#8232, parent #8518).
//!
//! Legacy `module` and `args` fields on [`NodeKind::Use`] / [`No`](crate::NodeKind::No) remain
//! populated from [`CompileDirectiveSyntax::compat_module`] and
//! [`CompileDirectiveSyntax::compat_args`] so existing consumers keep compiling.

use perl_position_tracking::SourceLocation;

/// Subject of a compile directive: module identity, version-only pragma, or combined.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CompileDirectiveSubject {
    /// Ordinary module / pragma name (`use strict`, `no warnings`).
    Module(ModuleNameSyntax),
    /// Version-only directive (`use v5.36`, `use 5.036`).
    VersionPragma(VersionRequirementSyntax),
    /// Module name followed by a numeric version token (`use Foo 1.23`).
    ModuleWithVersion { module: ModuleNameSyntax, version: VersionRequirementSyntax },
}

/// Module or pragma identifier with exact source span.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModuleNameSyntax {
    /// Normalized module / pragma name (`Foo::Bar`, `strict`).
    pub name: String,
    /// Exact source span of the name (excluding trailing version tokens).
    pub span: SourceLocation,
}

/// Optional version requirement on a compile directive.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VersionRequirementSyntax {
    /// Lexical v-string style (`v5.36`, `v5.12.0`).
    VString { raw: String, normalized: String, span: SourceLocation },
    /// Plain numeric version (`1.23`, `5.036`, dotted forms).
    Numeric { raw: String, normalized: String, span: SourceLocation },
}

impl VersionRequirementSyntax {
    /// Normalized version text suitable for comparisons (not a semver guarantee).
    #[must_use]
    pub fn normalized(&self) -> &str {
        match self {
            Self::VString { normalized, .. } | Self::Numeric { normalized, .. } => {
                normalized.as_str()
            }
        }
    }

    /// Exact source spelling of the version token(s).
    #[must_use]
    pub fn raw(&self) -> &str {
        match self {
            Self::VString { raw, .. } | Self::Numeric { raw, .. } => raw.as_str(),
        }
    }

    #[must_use]
    pub fn span(&self) -> SourceLocation {
        match self {
            Self::VString { span, .. } | Self::Numeric { span, .. } => *span,
        }
    }
}

/// Whether an import / argument list is present and in what form.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImportListPresence {
    /// No argument list (`use Foo;`, `use v5.36;`).
    Absent,
    /// Explicit empty list (`use Foo ();`).
    ExplicitEmpty { paren_span: SourceLocation },
    /// Non-empty argument list.
    Present { list_span: SourceLocation },
}

impl Default for ImportListPresence {
    fn default() -> Self {
        Self::Absent
    }
}

/// One typed import argument item with source backing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImportArgumentItem {
    /// One word from a `qw(...)` list with its exact span.
    QwWord { value: String, span: SourceLocation },
    /// String literal argument (`'refs'`, `"experimental::feature"`).
    StringLiteral { raw: String, span: SourceLocation },
    /// Entire normalized `qw(...)` projection kept for legacy consumers.
    NormalizedQwList { projection: String, span: SourceLocation },
    /// Non-literal argument region represented only by its source extent.
    ExpressionBoundary { span: SourceLocation },
}

/// Typed import argument list for `use` / `no`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImportArgumentList {
    pub presence: ImportListPresence,
    pub items: Vec<ImportArgumentItem>,
}

impl Default for ImportArgumentList {
    fn default() -> Self {
        Self { presence: ImportListPresence::Absent, items: Vec::new() }
    }
}

/// Complete typed syntax for a `use` or `no` directive.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompileDirectiveSyntax {
    pub subject: CompileDirectiveSubject,
    pub arguments: ImportArgumentList,
}

impl CompileDirectiveSyntax {
    /// Placeholder for hand-built AST fixtures: subject and args mirror legacy strings only.
    #[must_use]
    pub fn from_legacy_module_and_args(module: &str, args: &[String]) -> Self {
        let module_span = SourceLocation { start: 0, end: 0 };
        let subject = classify_subject_from_legacy(module, module_span);
        let mut arguments = ImportArgumentList::default();
        if args.is_empty() {
            arguments.presence = ImportListPresence::Absent;
        } else {
            arguments.presence =
                ImportListPresence::Present { list_span: SourceLocation { start: 0, end: 0 } };
            for arg in args {
                arguments.items.push(ImportArgumentItem::NormalizedQwList {
                    projection: arg.clone(),
                    span: SourceLocation { start: 0, end: 0 },
                });
            }
        }
        Self { subject, arguments }
    }

    /// Legacy compatibility: module / pragma identity string.
    #[must_use]
    pub fn compat_module(&self) -> String {
        match &self.subject {
            CompileDirectiveSubject::Module(m) => m.name.clone(),
            CompileDirectiveSubject::VersionPragma(v) => v.raw().to_string(),
            CompileDirectiveSubject::ModuleWithVersion { module, .. } => module.name.clone(),
        }
    }

    /// Legacy compatibility: flattened argument strings (deterministic projection).
    #[must_use]
    pub fn compat_args(&self) -> Vec<String> {
        let mut out = Vec::new();
        for item in &self.arguments.items {
            match item {
                ImportArgumentItem::NormalizedQwList { projection, .. } => {
                    out.push(projection.clone())
                }
                ImportArgumentItem::QwWord { value, .. } => {
                    // If we only have discrete qw words, rebuild a normalized qw(...) group.
                    // Caller should prefer NormalizedQwList when that was the legacy shape.
                    out.push(value.clone());
                }
                ImportArgumentItem::StringLiteral { raw, .. } => out.push(raw.clone()),
                ImportArgumentItem::ExpressionBoundary { span } => {
                    out.push(format!("<expr@{}..{}>", span.start, span.end));
                }
            }
        }
        out
    }

    /// Exact span of the module / pragma name when populated by the parser.
    #[must_use]
    pub fn module_name_span(&self) -> SourceLocation {
        match &self.subject {
            CompileDirectiveSubject::Module(m) => m.span,
            CompileDirectiveSubject::VersionPragma(v) => v.span(),
            CompileDirectiveSubject::ModuleWithVersion { module, .. } => module.span,
        }
    }

    #[must_use]
    pub fn optional_version(&self) -> Option<&VersionRequirementSyntax> {
        match &self.subject {
            CompileDirectiveSubject::VersionPragma(v) => Some(v),
            CompileDirectiveSubject::ModuleWithVersion { version, .. } => Some(version),
            CompileDirectiveSubject::Module(_) => None,
        }
    }
}

fn classify_subject_from_legacy(module: &str, span: SourceLocation) -> CompileDirectiveSubject {
    if module.starts_with('v')
        && module.len() > 1
        && module.chars().skip(1).all(|c| c.is_ascii_digit() || c == '.')
    {
        return CompileDirectiveSubject::VersionPragma(VersionRequirementSyntax::VString {
            raw: module.to_string(),
            normalized: module.to_string(),
            span,
        });
    }
    if let Some((name, ver)) = module.split_once(' ') {
        let ver = ver.trim();
        if !ver.is_empty() && ver.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return CompileDirectiveSubject::ModuleWithVersion {
                module: ModuleNameSyntax { name: name.to_string(), span },
                version: VersionRequirementSyntax::Numeric {
                    raw: ver.to_string(),
                    normalized: ver.to_string(),
                    span: SourceLocation { start: 0, end: 0 },
                },
            };
        }
    }
    CompileDirectiveSubject::Module(ModuleNameSyntax { name: module.to_string(), span })
}
