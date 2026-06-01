use serde::{Deserialize, Serialize};

use super::indentation_style_from_line;

/// Prepared context for inline completion suggestions and future AI handoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedInlineCompletionContext {
    /// Prefix on the current line up to the request position.
    pub prefix: String,
    /// Full current line with trailing newline removed.
    pub current_line: String,
    /// Closest previous non-empty line, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_non_empty_line: Option<String>,
    /// Nearest enclosing subroutine name, if one can be inferred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_function: Option<String>,
    /// Nearest package declaration before the cursor, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_package: Option<String>,
    /// Nearby variables, ordered from closest to farthest.
    pub variables: Vec<String>,
    /// Imported modules or pragmas visible before the cursor.
    pub imports: Vec<String>,
}

/// Request-local facts supplied by the LSP runtime.
///
/// The deterministic provider remains usable with only source text, but the
/// runtime can pass workspace-derived facts here so inline completion can
/// prefer project-aware suggestions without depending on runtime state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InlineCompletionEnvironment {
    /// Modules reachable from the current document's effective `@INC`.
    pub available_modules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticInlineContext {
    pub(crate) lexical_scope: InlineLexicalScope,
    pub(crate) package: Option<String>,
    pub(crate) enclosing_sub: Option<String>,
    pub(crate) expected_syntax: ExpectedSyntax,
    pub(crate) visible_variables: Vec<VariableFact>,
    pub(crate) receiver_hint: Option<ReceiverHint>,
    pub(crate) dbi_receiver_kind: Option<DbiReceiverKind>,
    pub(crate) imported_modules: Vec<ModuleFact>,
    pub(crate) available_modules: Vec<ModuleFact>,
    pub(crate) current_package_methods: Vec<MethodFact>,
    pub(crate) has_done_testing_call: bool,
    pub(crate) file_role: FileRole,
    pub(crate) style: InlineStyleContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InlineLexicalScope {
    File,
    Subroutine(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpectedSyntax {
    EmptyStatement,
    UseModule,
    MethodName,
    LexicalVariableName,
    PackageName,
    BlessArguments,
    ReturnExpression,
    GuardCondition,
    ConditionExpression,
    LoopBinding,
    TestAssertionArguments,
    ShebangInterpreter,
    SubroutineBody,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VariableFact {
    pub(crate) sigil: VariableSigil,
    pub(crate) name: String,
}

impl VariableFact {
    pub(super) fn from_perl_variable(variable: &str) -> Option<Self> {
        let mut chars = variable.chars();
        let sigil = VariableSigil::from_char(chars.next()?)?;
        let name: String = chars.collect();
        (!name.is_empty()).then_some(Self { sigil, name })
    }

    pub(super) fn as_perl_variable(&self) -> String {
        format!("{}{}", self.sigil.as_char(), self.name)
    }

    pub(super) fn is_scalar_self(&self) -> bool {
        self.sigil == VariableSigil::Scalar && self.name == "self"
    }

    pub(super) fn is_scalar(&self) -> bool {
        self.sigil == VariableSigil::Scalar
    }

    pub(super) fn is_array(&self) -> bool {
        self.sigil == VariableSigil::Array
    }

    pub(super) fn is_hash(&self) -> bool {
        self.sigil == VariableSigil::Hash
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VariableSigil {
    Scalar,
    Array,
    Hash,
}

impl VariableSigil {
    fn from_char(ch: char) -> Option<Self> {
        match ch {
            '$' => Some(Self::Scalar),
            '@' => Some(Self::Array),
            '%' => Some(Self::Hash),
            _ => None,
        }
    }

    fn as_char(self) -> char {
        match self {
            Self::Scalar => '$',
            Self::Array => '@',
            Self::Hash => '%',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModuleFact {
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MethodFact {
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReceiverHint {
    SelfReceiver,
    Variable(VariableFact),
    Package(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DbiReceiverKind {
    DatabaseHandle,
    StatementHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileRole {
    Module,
    Script,
    Test,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InlineStyleContext {
    pub(crate) indentation: IndentationStyle,
    pub(crate) language_prelude: LanguagePreludeStyle,
    pub(crate) sub_argument_style: SubArgumentStyle,
    pub(crate) constructor_style: ConstructorStyle,
    pub(crate) test_framework: TestFramework,
}

impl InlineStyleContext {
    pub(super) fn unknown(context: &PreparedInlineCompletionContext) -> Self {
        Self {
            indentation: indentation_style_from_line(context.current_line.as_str()),
            language_prelude: LanguagePreludeStyle::from_imports(&context.imports),
            sub_argument_style: SubArgumentStyle::Unknown,
            constructor_style: ConstructorStyle::Unknown,
            test_framework: TestFramework::from_imports(&context.imports),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndentationStyle {
    Spaces(usize),
    Tabs,
    Mixed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LanguagePreludeStyle {
    ModernPerl,
    StrictWarnings,
    StrictOnly,
    WarningsOnly,
    Unknown,
}

impl LanguagePreludeStyle {
    pub(super) fn from_imports(imports: &[String]) -> Self {
        if imports.iter().any(|import| import == "Modern::Perl") {
            return Self::ModernPerl;
        }

        let has_strict = imports.iter().any(|import| import == "strict");
        let has_warnings = imports.iter().any(|import| import == "warnings");
        match (has_strict, has_warnings) {
            (true, true) => Self::StrictWarnings,
            (true, false) => Self::StrictOnly,
            (false, true) => Self::WarningsOnly,
            (false, false) => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubArgumentStyle {
    AtUnderscore,
    Shift,
    Signature,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConstructorStyle {
    BlessHashReturnSelf,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestFramework {
    Test2V0,
    TestMore,
    Unknown,
}

impl TestFramework {
    pub(super) fn from_imports(imports: &[String]) -> Self {
        if imports.iter().any(|import| import == "Test2::V0") {
            return Self::Test2V0;
        }
        if imports.iter().any(|import| import == "Test::More") {
            return Self::TestMore;
        }
        Self::Unknown
    }
}
