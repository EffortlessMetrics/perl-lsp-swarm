//! Canonical identity and compatibility aliases for critic findings.
//!
//! Diagnostic codes are presentation and compatibility surfaces. They are not
//! sufficient evidence that two findings are the same logical result: some
//! public codes cover multiple syntax shapes, while some native rules cover
//! multiple public codes. Producers therefore construct checked observed
//! identities, and the registry resolves only those checked values.

use std::collections::BTreeSet;
use std::fmt;

use serde::Serialize;

/// Schema version for serialized critic identity records.
pub const CRITIC_IDENTITY_SCHEMA_VERSION: u16 = 2;

/// Producer that emitted an observed critic or diagnostic identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CriticFindingOrigin {
    /// Parser-backed built-in diagnostic using a stable `PL*` code.
    BuiltInDiagnostic,
    /// Rust-native critic rule using a `native.*` rule ID.
    NativeCritic,
    /// In-process Perl::Critic-compatible legacy policy.
    LegacyPolicy,
    /// Optional external Perl::Critic process policy.
    ExternalPerlCritic,
}

/// Reviewed syntax distinction needed when one code or rule spans several findings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CriticFindingShape {
    /// No narrower syntax distinction is required.
    #[default]
    General,
    /// Comparison against an explicit literal `undef`.
    LiteralUndefComparison,
    /// Comparison whose operand may be undefined through data flow.
    PotentiallyUndefComparison,
    /// Backtick command execution.
    Backtick,
    /// `qx` command execution.
    Qx,
    /// `readpipe` command execution.
    Readpipe,
    /// `system` process execution.
    SystemCall,
    /// `exec` process replacement.
    ExecCall,
}

/// Checked producer/code/shape identity for one observed finding.
///
/// The fields are private. Unambiguous findings use [`Self::general`]; codes
/// and rules with multiple reviewed shapes have named constructors. This keeps
/// callers from assembling arbitrary producer/code/shape triples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct CriticObservedIdentity<'a> {
    origin: CriticFindingOrigin,
    code: &'a str,
    shape: CriticFindingShape,
}

impl<'a> CriticObservedIdentity<'a> {
    /// Construct an observed identity for a producer/code pair whose contract
    /// has exactly one general shape.
    pub fn general(
        origin: CriticFindingOrigin,
        code: &'a str,
    ) -> Result<Self, CriticObservedIdentityError> {
        if CriticIdentityRegistry::requires_reviewed_shape(origin, code) {
            return Err(CriticObservedIdentityError { origin, code: code.to_string() });
        }

        Ok(Self { origin, code, shape: CriticFindingShape::General })
    }
}

impl CriticObservedIdentity<'static> {
    /// Built-in PL404 finding comparing against an explicit literal `undef`.
    #[must_use]
    pub const fn built_in_literal_undef_comparison() -> CriticObservedIdentity<'static> {
        Self::reviewed(
            CriticFindingOrigin::BuiltInDiagnostic,
            "PL404",
            CriticFindingShape::LiteralUndefComparison,
        )
    }

    /// Built-in PL404 finding inferred from a potentially undefined operand.
    #[must_use]
    pub const fn built_in_potentially_undef_comparison() -> CriticObservedIdentity<'static> {
        Self::reviewed(
            CriticFindingOrigin::BuiltInDiagnostic,
            "PL404",
            CriticFindingShape::PotentiallyUndefComparison,
        )
    }

    /// Built-in PL601 backtick finding.
    #[must_use]
    pub const fn built_in_backtick_exec() -> CriticObservedIdentity<'static> {
        Self::reviewed(
            CriticFindingOrigin::BuiltInDiagnostic,
            "PL601",
            CriticFindingShape::Backtick,
        )
    }

    /// Built-in PL601 `qx` finding.
    #[must_use]
    pub const fn built_in_qx_exec() -> CriticObservedIdentity<'static> {
        Self::reviewed(CriticFindingOrigin::BuiltInDiagnostic, "PL601", CriticFindingShape::Qx)
    }

    /// Built-in PL606 `readpipe` finding.
    #[must_use]
    pub const fn built_in_readpipe_exec() -> CriticObservedIdentity<'static> {
        Self::reviewed(
            CriticFindingOrigin::BuiltInDiagnostic,
            "PL606",
            CriticFindingShape::Readpipe,
        )
    }

    /// Built-in PL603 `system` finding.
    #[must_use]
    pub const fn built_in_system_call() -> CriticObservedIdentity<'static> {
        Self::reviewed(
            CriticFindingOrigin::BuiltInDiagnostic,
            "PL603",
            CriticFindingShape::SystemCall,
        )
    }

    /// Built-in PL604 `exec` finding.
    #[must_use]
    pub const fn built_in_exec_call() -> CriticObservedIdentity<'static> {
        Self::reviewed(
            CriticFindingOrigin::BuiltInDiagnostic,
            "PL604",
            CriticFindingShape::ExecCall,
        )
    }

    /// Native explicit-literal `undef` comparison finding.
    #[must_use]
    pub const fn native_literal_undef_comparison() -> CriticObservedIdentity<'static> {
        Self::reviewed(
            CriticFindingOrigin::NativeCritic,
            "native.common.undef_comparison",
            CriticFindingShape::LiteralUndefComparison,
        )
    }

    /// Native backtick execution finding.
    #[must_use]
    pub const fn native_backtick_exec() -> CriticObservedIdentity<'static> {
        Self::reviewed(
            CriticFindingOrigin::NativeCritic,
            "native.security.backtick_exec",
            CriticFindingShape::Backtick,
        )
    }

    /// Native `qx` execution finding.
    #[must_use]
    pub const fn native_qx_exec() -> CriticObservedIdentity<'static> {
        Self::reviewed(
            CriticFindingOrigin::NativeCritic,
            "native.security.qx_readpipe",
            CriticFindingShape::Qx,
        )
    }

    /// Native `readpipe` execution finding.
    #[must_use]
    pub const fn native_readpipe_exec() -> CriticObservedIdentity<'static> {
        Self::reviewed(
            CriticFindingOrigin::NativeCritic,
            "native.security.qx_readpipe",
            CriticFindingShape::Readpipe,
        )
    }

    /// Native `system` execution finding.
    #[must_use]
    pub const fn native_system_call() -> CriticObservedIdentity<'static> {
        Self::reviewed(
            CriticFindingOrigin::NativeCritic,
            "native.security.system_exec",
            CriticFindingShape::SystemCall,
        )
    }

    /// Native `exec` execution finding.
    #[must_use]
    pub const fn native_exec_call() -> CriticObservedIdentity<'static> {
        Self::reviewed(
            CriticFindingOrigin::NativeCritic,
            "native.security.system_exec",
            CriticFindingShape::ExecCall,
        )
    }

    const fn reviewed(
        origin: CriticFindingOrigin,
        code: &'static str,
        shape: CriticFindingShape,
    ) -> Self {
        Self { origin, code, shape }
    }
}

impl<'a> CriticObservedIdentity<'a> {
    /// Producer that emitted this finding.
    #[must_use]
    pub const fn origin(self) -> CriticFindingOrigin {
        self.origin
    }

    /// Producer-owned public code or policy spelling.
    #[must_use]
    pub const fn code(self) -> &'a str {
        self.code
    }

    /// Reviewed finding shape.
    #[must_use]
    pub const fn shape(self) -> CriticFindingShape {
        self.shape
    }
}

/// A general observed identity was requested for a code that requires a
/// producer-owned reviewed shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriticObservedIdentityError {
    origin: CriticFindingOrigin,
    code: String,
}

impl CriticObservedIdentityError {
    /// Producer whose code needs a reviewed shape.
    #[must_use]
    pub const fn origin(&self) -> CriticFindingOrigin {
        self.origin
    }

    /// Code or rule ID that cannot use the general constructor.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Display for CriticObservedIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "critic identity {:?}:{:?} requires a reviewed finding shape",
            self.origin, self.code
        )
    }
}

impl std::error::Error for CriticObservedIdentityError {}

/// Producer-owned identity disposition for one native logical finding.
///
/// The native rule registry owns the complete set of these values. The shared
/// identity registry consumes the set to prove every `(rule_id, shape)` pair is
/// represented, including each branch of combined native rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct NativeCriticIdentityDisposition {
    rule_id: &'static str,
    shape: CriticFindingShape,
}

impl NativeCriticIdentityDisposition {
    pub(crate) const fn new(rule_id: &'static str, shape: CriticFindingShape) -> Self {
        Self { rule_id, shape }
    }

    /// Native rule ID that emits this logical finding.
    #[must_use]
    pub const fn rule_id(self) -> &'static str {
        self.rule_id
    }

    /// Producer-owned logical shape emitted by the rule.
    #[must_use]
    pub const fn shape(self) -> CriticFindingShape {
        self.shape
    }

    /// Convert the producer disposition into a checked observed identity.
    #[must_use]
    pub const fn observed(self) -> CriticObservedIdentity<'static> {
        CriticObservedIdentity::reviewed(
            CriticFindingOrigin::NativeCritic,
            self.rule_id,
            self.shape,
        )
    }
}

/// Stable category of a canonical critic finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CriticIdentityCategory {
    /// Syntax or pragma policy.
    Syntax,
    /// Semantic or scope policy.
    Semantic,
    /// Security policy.
    Security,
    /// Maintainability policy.
    Maintainability,
    /// Documentation policy.
    Documentation,
}

/// Whether an identity has approved compatibility aliases or is deliberately distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CriticIdentityDisposition {
    /// Every listed observed identity is an approved alias for one logical finding.
    EquivalentAliases,
    /// No compatibility alias is approved at the current contract boundary.
    Distinct {
        /// Reviewed reason the finding must remain distinct.
        reason: &'static str,
    },
}

/// One producer/code/shape spelling of a canonical finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct CriticAlias {
    origin: CriticFindingOrigin,
    code: &'static str,
    shape: CriticFindingShape,
}

impl CriticAlias {
    const fn new(
        origin: CriticFindingOrigin,
        code: &'static str,
        shape: CriticFindingShape,
    ) -> Self {
        Self { origin, code, shape }
    }

    /// Producer that emitted this spelling.
    #[must_use]
    pub const fn origin(self) -> CriticFindingOrigin {
        self.origin
    }

    /// Stable code or policy spelling emitted by the producer.
    #[must_use]
    pub const fn code(self) -> &'static str {
        self.code
    }

    /// Reviewed syntax distinction for this alias.
    #[must_use]
    pub const fn shape(self) -> CriticFindingShape {
        self.shape
    }

    /// Checked observed identity represented by this registered alias.
    #[must_use]
    pub const fn observed(self) -> CriticObservedIdentity<'static> {
        CriticObservedIdentity::reviewed(self.origin, self.code, self.shape)
    }

}

/// Canonical identity shared by approved critic aliases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CriticIdentityEntry {
    schema_version: u16,
    canonical_id: &'static str,
    category: CriticIdentityCategory,
    disposition: CriticIdentityDisposition,
    aliases: &'static [CriticAlias],
}

impl CriticIdentityEntry {
    const fn equivalent(
        canonical_id: &'static str,
        category: CriticIdentityCategory,
        aliases: &'static [CriticAlias],
    ) -> Self {
        Self {
            schema_version: CRITIC_IDENTITY_SCHEMA_VERSION,
            canonical_id,
            category,
            disposition: CriticIdentityDisposition::EquivalentAliases,
            aliases,
        }
    }

    const fn distinct(
        canonical_id: &'static str,
        category: CriticIdentityCategory,
        reason: &'static str,
        aliases: &'static [CriticAlias],
    ) -> Self {
        Self {
            schema_version: CRITIC_IDENTITY_SCHEMA_VERSION,
            canonical_id,
            category,
            disposition: CriticIdentityDisposition::Distinct { reason },
            aliases,
        }
    }

    /// Identity schema version.
    #[must_use]
    pub const fn schema_version(self) -> u16 {
        self.schema_version
    }

    /// Stable canonical finding ID.
    #[must_use]
    pub const fn canonical_id(self) -> &'static str {
        self.canonical_id
    }

    /// Stable finding category.
    #[must_use]
    pub const fn category(self) -> CriticIdentityCategory {
        self.category
    }

    /// Alias or deliberate-distinction disposition.
    #[must_use]
    pub const fn disposition(self) -> CriticIdentityDisposition {
        self.disposition
    }

    /// Approved observed identities for this canonical finding.
    #[must_use]
    pub const fn aliases(self) -> &'static [CriticAlias] {
        self.aliases
    }
}

const BUILTIN: CriticFindingOrigin = CriticFindingOrigin::BuiltInDiagnostic;
const NATIVE: CriticFindingOrigin = CriticFindingOrigin::NativeCritic;
const LEGACY: CriticFindingOrigin = CriticFindingOrigin::LegacyPolicy;
const EXTERNAL: CriticFindingOrigin = CriticFindingOrigin::ExternalPerlCritic;
const GENERAL: CriticFindingShape = CriticFindingShape::General;

// Keep entries ordered by canonical ID. Validation pins that order so receipts
// and generated status remain byte-deterministic.
static IDENTITIES: &[CriticIdentityEntry] = &[
    CriticIdentityEntry::equivalent(
        "critic.common.assignment_in_condition",
        CriticIdentityCategory::Syntax,
        &[
            CriticAlias::new(NATIVE, "native.common.assignment_in_condition", GENERAL),
            CriticAlias::new(BUILTIN, "PL403", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.common.deprecated_defined",
        CriticIdentityCategory::Syntax,
        &[
            CriticAlias::new(NATIVE, "native.common.deprecated_defined", GENERAL),
            CriticAlias::new(BUILTIN, "PL500", GENERAL),
        ],
    ),
    CriticIdentityEntry::distinct(
        "critic.common.potentially_undef_comparison",
        CriticIdentityCategory::Syntax,
        "the native rule covers explicit literal undef comparisons, not inferred maybe-undef data flow",
        &[CriticAlias::new(BUILTIN, "PL404", CriticFindingShape::PotentiallyUndefComparison)],
    ),
    CriticIdentityEntry::equivalent(
        "critic.common.printf_format_arity",
        CriticIdentityCategory::Syntax,
        &[
            CriticAlias::new(NATIVE, "native.common.printf_format_arity", GENERAL),
            CriticAlias::new(BUILTIN, "PL405", GENERAL),
        ],
    ),
    CriticIdentityEntry::distinct(
        "critic.common.stale_dollar_at",
        CriticIdentityCategory::Syntax,
        "PL407 covers broader eval-error flow and is not an exact alias",
        &[CriticAlias::new(NATIVE, "native.common.stale_dollar_at", GENERAL)],
    ),
    CriticIdentityEntry::equivalent(
        "critic.common.undef_comparison",
        CriticIdentityCategory::Syntax,
        &[
            CriticAlias::new(
                NATIVE,
                "native.common.undef_comparison",
                CriticFindingShape::LiteralUndefComparison,
            ),
            CriticAlias::new(BUILTIN, "PL404", CriticFindingShape::LiteralUndefComparison),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.common.unreachable_code",
        CriticIdentityCategory::Maintainability,
        &[
            CriticAlias::new(NATIVE, "native.common.unreachable_code", GENERAL),
            CriticAlias::new(BUILTIN, "PL406", GENERAL),
        ],
    ),
    CriticIdentityEntry::distinct(
        "critic.documentation.require_pod_sections",
        CriticIdentityCategory::Documentation,
        "PL304 covers exported-subroutine POD rather than required module sections",
        &[CriticAlias::new(NATIVE, "native.documentation.require_pod_sections", GENERAL)],
    ),
    CriticIdentityEntry::equivalent(
        "critic.io.bareword_filehandle",
        CriticIdentityCategory::Syntax,
        &[
            CriticAlias::new(NATIVE, "native.io.bareword_filehandle", GENERAL),
            CriticAlias::new(BUILTIN, "PL400", GENERAL),
            CriticAlias::new(LEGACY, "InputOutput::ProhibitBarewordFileHandles", GENERAL),
            CriticAlias::new(EXTERNAL, "InputOutput::ProhibitBarewordFileHandles", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.io.pipe_open",
        CriticIdentityCategory::Security,
        &[
            CriticAlias::new(NATIVE, "native.io.pipe_open", GENERAL),
            CriticAlias::new(BUILTIN, "PL605", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.io.two_arg_open",
        CriticIdentityCategory::Security,
        &[
            CriticAlias::new(NATIVE, "native.io.two_arg_open", GENERAL),
            CriticAlias::new(BUILTIN, "PL401", GENERAL),
            CriticAlias::new(LEGACY, "InputOutput::ProhibitTwoArgOpen", GENERAL),
            CriticAlias::new(EXTERNAL, "InputOutput::ProhibitTwoArgOpen", GENERAL),
        ],
    ),
    CriticIdentityEntry::distinct(
        "critic.io.unchecked_open_close",
        CriticIdentityCategory::Security,
        "no built-in diagnostic has the same checked-call contract",
        &[CriticAlias::new(NATIVE, "native.io.unchecked_open_close", GENERAL)],
    ),
    CriticIdentityEntry::equivalent(
        "critic.regex.capture_without_match",
        CriticIdentityCategory::Semantic,
        &[
            CriticAlias::new(NATIVE, "native.regex.capture_without_match", GENERAL),
            CriticAlias::new(BUILTIN, "PL112", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.security.backtick_exec",
        CriticIdentityCategory::Security,
        &[
            CriticAlias::new(NATIVE, "native.security.backtick_exec", CriticFindingShape::Backtick),
            CriticAlias::new(BUILTIN, "PL601", CriticFindingShape::Backtick),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.security.exec_call",
        CriticIdentityCategory::Security,
        &[
            CriticAlias::new(NATIVE, "native.security.system_exec", CriticFindingShape::ExecCall),
            CriticAlias::new(BUILTIN, "PL604", CriticFindingShape::ExecCall),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.security.qx_exec",
        CriticIdentityCategory::Security,
        &[
            CriticAlias::new(NATIVE, "native.security.qx_readpipe", CriticFindingShape::Qx),
            CriticAlias::new(BUILTIN, "PL601", CriticFindingShape::Qx),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.security.readpipe_exec",
        CriticIdentityCategory::Security,
        &[
            CriticAlias::new(NATIVE, "native.security.qx_readpipe", CriticFindingShape::Readpipe),
            CriticAlias::new(BUILTIN, "PL606", CriticFindingShape::Readpipe),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.security.string_eval",
        CriticIdentityCategory::Security,
        &[
            CriticAlias::new(NATIVE, "native.security.string_eval", GENERAL),
            CriticAlias::new(BUILTIN, "PL600", GENERAL),
            CriticAlias::new(LEGACY, "BuiltinFunctions::ProhibitStringyEval", GENERAL),
            CriticAlias::new(EXTERNAL, "BuiltinFunctions::ProhibitStringyEval", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.security.system_call",
        CriticIdentityCategory::Security,
        &[
            CriticAlias::new(NATIVE, "native.security.system_exec", CriticFindingShape::SystemCall),
            CriticAlias::new(BUILTIN, "PL603", CriticFindingShape::SystemCall),
        ],
    ),
    CriticIdentityEntry::distinct(
        "critic.syntax.prohibit_leading_zeros",
        CriticIdentityCategory::Syntax,
        "no built-in diagnostic has the same literal policy",
        &[CriticAlias::new(NATIVE, "native.syntax.prohibit_leading_zeros", GENERAL)],
    ),
    CriticIdentityEntry::equivalent(
        "critic.syntax.unquoted_bareword",
        CriticIdentityCategory::Syntax,
        &[
            CriticAlias::new(NATIVE, "native.syntax.unquoted_bareword", GENERAL),
            CriticAlias::new(BUILTIN, "PL109", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.testing.require_use_strict",
        CriticIdentityCategory::Syntax,
        &[
            CriticAlias::new(NATIVE, "native.testing.require_use_strict", GENERAL),
            CriticAlias::new(BUILTIN, "PL100", GENERAL),
            CriticAlias::new(LEGACY, "TestingAndDebugging::RequireUseStrict", GENERAL),
            CriticAlias::new(EXTERNAL, "TestingAndDebugging::RequireUseStrict", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.testing.require_use_warnings",
        CriticIdentityCategory::Syntax,
        &[
            CriticAlias::new(NATIVE, "native.testing.require_use_warnings", GENERAL),
            CriticAlias::new(BUILTIN, "PL101", GENERAL),
            CriticAlias::new(LEGACY, "TestingAndDebugging::RequireUseWarnings", GENERAL),
            CriticAlias::new(EXTERNAL, "TestingAndDebugging::RequireUseWarnings", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.variables.duplicate_lexical",
        CriticIdentityCategory::Semantic,
        &[
            CriticAlias::new(NATIVE, "native.variables.duplicate_lexical", GENERAL),
            CriticAlias::new(BUILTIN, "PL105", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.variables.duplicate_parameter",
        CriticIdentityCategory::Semantic,
        &[
            CriticAlias::new(NATIVE, "native.variables.duplicate_parameter", GENERAL),
            CriticAlias::new(BUILTIN, "PL106", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.variables.parameter_shadows_global",
        CriticIdentityCategory::Semantic,
        &[
            CriticAlias::new(NATIVE, "native.variables.parameter_shadows_global", GENERAL),
            CriticAlias::new(BUILTIN, "PL107", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.variables.shadowed_lexical",
        CriticIdentityCategory::Semantic,
        &[
            CriticAlias::new(NATIVE, "native.variables.shadowed_lexical", GENERAL),
            CriticAlias::new(BUILTIN, "PL104", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.variables.undeclared",
        CriticIdentityCategory::Semantic,
        &[
            CriticAlias::new(NATIVE, "native.variables.undeclared", GENERAL),
            CriticAlias::new(BUILTIN, "PL103", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.variables.uninitialized",
        CriticIdentityCategory::Semantic,
        &[
            CriticAlias::new(NATIVE, "native.variables.uninitialized", GENERAL),
            CriticAlias::new(BUILTIN, "PL110", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.variables.unused_lexical",
        CriticIdentityCategory::Semantic,
        &[
            CriticAlias::new(NATIVE, "native.variables.unused_lexical", GENERAL),
            CriticAlias::new(BUILTIN, "PL102", GENERAL),
            CriticAlias::new(LEGACY, "Variables::ProhibitUnusedVariables", GENERAL),
            CriticAlias::new(EXTERNAL, "Variables::ProhibitUnusedVariables", GENERAL),
        ],
    ),
    CriticIdentityEntry::equivalent(
        "critic.variables.unused_parameter",
        CriticIdentityCategory::Semantic,
        &[
            CriticAlias::new(NATIVE, "native.variables.unused_parameter", GENERAL),
            CriticAlias::new(BUILTIN, "PL108", GENERAL),
        ],
    ),
];

/// Validation failure for the static critic identity registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CriticIdentityRegistryError {
    /// An entry has no observed identity.
    EmptyAliases {
        /// Canonical ID with no aliases.
        canonical_id: &'static str,
    },
    /// Equivalent-alias disposition has fewer than two aliases.
    EquivalentNeedsAliases {
        /// Invalid canonical ID.
        canonical_id: &'static str,
    },
    /// Distinct disposition has more than one observed identity.
    DistinctHasAliases {
        /// Invalid canonical ID.
        canonical_id: &'static str,
    },
    /// Canonical IDs are duplicated.
    DuplicateCanonicalId {
        /// Duplicated canonical ID.
        canonical_id: &'static str,
    },
    /// One producer/code/shape alias maps to multiple canonical IDs.
    DuplicateAlias {
        /// Duplicated alias.
        alias: CriticAlias,
    },
    /// Registry order is not canonical-ID lexical order.
    NonDeterministicOrder {
        /// Previous canonical ID.
        previous: &'static str,
        /// Current out-of-order canonical ID.
        current: &'static str,
    },
}

impl fmt::Display for CriticIdentityRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAliases { canonical_id } => {
                write!(f, "critic identity '{canonical_id}' has no aliases")
            }
            Self::EquivalentNeedsAliases { canonical_id } => write!(
                f,
                "critic identity '{canonical_id}' is equivalent but has fewer than two aliases"
            ),
            Self::DistinctHasAliases { canonical_id } => {
                write!(f, "critic identity '{canonical_id}' is distinct but has multiple aliases")
            }
            Self::DuplicateCanonicalId { canonical_id } => {
                write!(f, "duplicate critic canonical ID '{canonical_id}'")
            }
            Self::DuplicateAlias { alias } => write!(
                f,
                "duplicate critic alias {:?}:{}:{:?}",
                alias.origin, alias.code, alias.shape
            ),
            Self::NonDeterministicOrder { previous, current } => write!(
                f,
                "critic identity order is not deterministic: '{previous}' before '{current}'"
            ),
        }
    }
}

impl std::error::Error for CriticIdentityRegistryError {}

/// Read-only access to canonical critic finding identities.
pub struct CriticIdentityRegistry;

impl CriticIdentityRegistry {
    /// All canonical identities in deterministic order.
    #[must_use]
    pub const fn entries() -> &'static [CriticIdentityEntry] {
        IDENTITIES
    }

    /// Resolve one checked observed identity into its canonical identity.
    #[must_use]
    pub fn resolve(observed: &CriticObservedIdentity<'_>) -> Option<&'static CriticIdentityEntry> {
        Self::resolve_parts(observed.origin, observed.code, observed.shape)
    }

    /// Resolve one producer/code/shape tuple through the canonical registry.
    ///
    /// This keeps owned and borrowed identity consumers on the same registry
    /// path. Callers may retain the tuple, but they do not need to duplicate
    /// the registry's reviewed-shape constructor table.
    #[must_use]
    pub fn resolve_parts(
        origin: CriticFindingOrigin,
        code: &str,
        shape: CriticFindingShape,
    ) -> Option<&'static CriticIdentityEntry> {
        IDENTITIES.iter().find(|entry| {
            entry.aliases.iter().any(|alias| {
                alias.origin == origin && alias.code == code && alias.shape == shape
            })
        })
    }

    /// Look up one canonical ID.
    #[must_use]
    pub fn by_canonical_id(canonical_id: &str) -> Option<&'static CriticIdentityEntry> {
        IDENTITIES.iter().find(|entry| entry.canonical_id == canonical_id)
    }

    /// Approved aliases for one canonical ID.
    #[must_use]
    pub fn aliases_for(canonical_id: &str) -> Option<&'static [CriticAlias]> {
        Self::by_canonical_id(canonical_id).map(|entry| entry.aliases)
    }

    fn requires_reviewed_shape(origin: CriticFindingOrigin, code: &str) -> bool {
        IDENTITIES.iter().flat_map(|entry| entry.aliases).any(|alias| {
            alias.origin == origin
                && alias.code == code
                && alias.shape != CriticFindingShape::General
        })
    }

    /// Validate uniqueness, disposition, and deterministic ordering.
    pub fn validate() -> Result<(), CriticIdentityRegistryError> {
        let mut canonical_ids = BTreeSet::new();
        let mut aliases = BTreeSet::new();
        let mut previous = None;

        for entry in IDENTITIES {
            if let Some(previous_id) = previous
                && previous_id > entry.canonical_id
            {
                return Err(CriticIdentityRegistryError::NonDeterministicOrder {
                    previous: previous_id,
                    current: entry.canonical_id,
                });
            }
            previous = Some(entry.canonical_id);

            if !canonical_ids.insert(entry.canonical_id) {
                return Err(CriticIdentityRegistryError::DuplicateCanonicalId {
                    canonical_id: entry.canonical_id,
                });
            }
            if entry.aliases.is_empty() {
                return Err(CriticIdentityRegistryError::EmptyAliases {
                    canonical_id: entry.canonical_id,
                });
            }
            match entry.disposition {
                CriticIdentityDisposition::EquivalentAliases if entry.aliases.len() < 2 => {
                    return Err(CriticIdentityRegistryError::EquivalentNeedsAliases {
                        canonical_id: entry.canonical_id,
                    });
                }
                CriticIdentityDisposition::Distinct { .. } if entry.aliases.len() != 1 => {
                    return Err(CriticIdentityRegistryError::DistinctHasAliases {
                        canonical_id: entry.canonical_id,
                    });
                }
                _ => {}
            }

            for alias in entry.aliases {
                if !aliases.insert(*alias) {
                    return Err(CriticIdentityRegistryError::DuplicateAlias { alias: *alias });
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        BUILTIN, CRITIC_IDENTITY_SCHEMA_VERSION, CriticFindingOrigin, CriticFindingShape,
        CriticIdentityDisposition, CriticIdentityRegistry, CriticObservedIdentity, EXTERNAL,
        NATIVE,
    };
    use crate::tooling::perl_critic::{NativeCriticProfile, NativeCriticRegistry};

    fn resolve_general(
        origin: CriticFindingOrigin,
        code: &str,
    ) -> Option<&'static super::CriticIdentityEntry> {
        CriticObservedIdentity::general(origin, code)
            .ok()
            .and_then(|observed| CriticIdentityRegistry::resolve(&observed))
    }

    fn registered_native_dispositions() -> BTreeSet<(&'static str, CriticFindingShape)> {
        CriticIdentityRegistry::entries()
            .iter()
            .flat_map(|entry| entry.aliases())
            .filter(|alias| alias.origin() == CriticFindingOrigin::NativeCritic)
            .map(|alias| (alias.code(), alias.shape()))
            .collect()
    }

    fn producer_native_dispositions() -> BTreeSet<(&'static str, CriticFindingShape)> {
        NativeCriticRegistry::identity_dispositions()
            .iter()
            .map(|disposition| (disposition.rule_id(), disposition.shape()))
            .collect()
    }

    #[test]
    fn registry_validates() {
        assert!(CriticIdentityRegistry::validate().is_ok());
    }

    #[test]
    fn strict_alias_resolves_in_both_directions() {
        let canonical = resolve_general(BUILTIN, "PL100").map(|entry| entry.canonical_id());
        assert_eq!(canonical, Some("critic.testing.require_use_strict"));

        let aliases = CriticIdentityRegistry::aliases_for("critic.testing.require_use_strict");
        assert!(aliases.is_some_and(|items| {
            items.iter().any(|alias| {
                alias.origin() == NATIVE && alias.code() == "native.testing.require_use_strict"
            }) && items.iter().any(|alias| {
                alias.origin() == EXTERNAL
                    && alias.code() == "TestingAndDebugging::RequireUseStrict"
            })
        }));
    }

    #[test]
    fn general_constructor_rejects_codes_with_reviewed_shapes() {
        assert!(CriticObservedIdentity::general(BUILTIN, "PL404").is_err());
        assert!(CriticObservedIdentity::general(BUILTIN, "PL601").is_err());
        assert!(CriticObservedIdentity::general(NATIVE, "native.security.qx_readpipe").is_err());
        assert!(CriticObservedIdentity::general(NATIVE, "native.security.system_exec").is_err());
    }

    #[test]
    fn shared_public_codes_require_producer_owned_shapes() {
        let backtick =
            CriticIdentityRegistry::resolve(&CriticObservedIdentity::built_in_backtick_exec())
                .map(|entry| entry.canonical_id());
        let qx = CriticIdentityRegistry::resolve(&CriticObservedIdentity::built_in_qx_exec())
            .map(|entry| entry.canonical_id());
        assert_eq!(backtick, Some("critic.security.backtick_exec"));
        assert_eq!(qx, Some("critic.security.qx_exec"));
        assert_ne!(backtick, qx);
    }

    #[test]
    fn pl404_literal_and_data_flow_findings_remain_distinct() {
        let literal = CriticIdentityRegistry::resolve(
            &CriticObservedIdentity::built_in_literal_undef_comparison(),
        )
        .map(|entry| entry.canonical_id());
        let inferred = CriticIdentityRegistry::resolve(
            &CriticObservedIdentity::built_in_potentially_undef_comparison(),
        )
        .map(|entry| entry.canonical_id());
        let native = CriticIdentityRegistry::resolve(
            &CriticObservedIdentity::native_literal_undef_comparison(),
        )
        .map(|entry| entry.canonical_id());

        assert_eq!(literal, Some("critic.common.undef_comparison"));
        assert_eq!(native, literal);
        assert_eq!(inferred, Some("critic.common.potentially_undef_comparison"));
        assert_ne!(literal, inferred);
    }

    #[test]
    fn combined_native_rules_require_producer_owned_shapes() {
        let system = CriticIdentityRegistry::resolve(&CriticObservedIdentity::native_system_call())
            .map(|entry| entry.canonical_id());
        let exec = CriticIdentityRegistry::resolve(&CriticObservedIdentity::native_exec_call())
            .map(|entry| entry.canonical_id());
        assert_eq!(system, Some("critic.security.system_call"));
        assert_eq!(exec, Some("critic.security.exec_call"));
        assert_ne!(system, exec);
    }

    #[test]
    fn unknown_external_policy_is_not_guessed() {
        assert!(
            resolve_general(CriticFindingOrigin::ExternalPerlCritic, "Unknown::Policy").is_none()
        );
    }

    #[test]
    fn every_native_rule_and_shape_has_an_explicit_identity_disposition() {
        let catalog: BTreeSet<&str> =
            NativeCriticRegistry::for_profile(NativeCriticProfile::Strict)
                .rule_ids()
                .into_iter()
                .collect();
        let disposition_rules: BTreeSet<&str> = NativeCriticRegistry::identity_dispositions()
            .iter()
            .map(|disposition| disposition.rule_id())
            .collect();

        assert_eq!(disposition_rules, catalog);
        assert_eq!(registered_native_dispositions(), producer_native_dispositions());
    }

    #[test]
    fn native_shape_completeness_detects_a_missing_combined_rule_branch() {
        let producer = producer_native_dispositions();
        let mut registered = registered_native_dispositions();

        assert!(registered.remove(&("native.security.qx_readpipe", CriticFindingShape::Readpipe,)));
        assert_ne!(registered, producer);
        assert!(producer.difference(&registered).any(|(rule_id, shape)| {
            *rule_id == "native.security.qx_readpipe" && *shape == CriticFindingShape::Readpipe
        }));
    }

    #[test]
    fn distinct_findings_remain_explicit() {
        let disposition = CriticIdentityRegistry::by_canonical_id("critic.common.stale_dollar_at")
            .map(|entry| entry.disposition());
        assert!(matches!(disposition, Some(CriticIdentityDisposition::Distinct { .. })));
    }

    #[test]
    fn serialization_is_deterministic_and_versioned() -> Result<(), serde_json::Error> {
        let first = serde_json::to_string(CriticIdentityRegistry::entries())?;
        let second = serde_json::to_string(CriticIdentityRegistry::entries())?;
        assert_eq!(first, second);
        assert!(
            CriticIdentityRegistry::entries()
                .iter()
                .all(|entry| entry.schema_version() == CRITIC_IDENTITY_SCHEMA_VERSION)
        );
        Ok(())
    }
}
