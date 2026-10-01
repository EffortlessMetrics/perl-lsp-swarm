//! Registry-backed literal `Function::Parameters` and `Method::Signatures`
//! declaration facts (#16808).
//!
//! This adapter owns module/version/profile detection and canonical callable
//! fact minting. It does not parse Perl, execute modules, interpret custom
//! keyword plugins, or feed a provider surface. Exact detection requires a
//! canonical descriptor plus resolved module identity whose observed version
//! satisfies the reviewed constraint. Declaration facts mint only through
//! [`signature_keyword_callable_facts`]: a detected framework, an exact
//! import profile, and a source-geometry declaration are all required.
//!
//! Shadow disposition matches the Mojo::Base / Moose siblings: output is
//! comparison/receipt material until registry-dispatch (#6821) and
//! shard-publication (#6822) land.

use crate::framework::{
    AdapterDescriptor, AdapterDetectionInput, AdapterDetectionResult, AdapterDisposition,
    AdapterId, DetectionAbsenceReason, DetectionOutcome, ModuleSelectorEvaluation,
    ModuleSelectorOutcome, UnavailableReason,
};
use crate::{
    AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, EntityId, FactId,
    FileId, InvalidationDependency, LifecyclePhase, Provenance, SemanticConfidence,
    SemanticFactEnvelope, SemanticFactKind, SemanticFactStatus, SemanticFreshness,
    SemanticProducer, SemanticProvenance, SemanticReasonCode, SourceAnchor, SourceGeneration,
};

/// Reviewed Function::Parameters 2.x family. Version 1 lax defaults are
/// outside this profile until separately reviewed.
pub const FUNCTION_PARAMETERS_VERSION_CONSTRAINT: &str = ">=2.0.0,<3.0.0";

/// Reviewed Method::Signatures identity. The current CPAN release is the
/// date-versioned `20170211` line; later unreviewed versions stay unsupported.
pub const METHOD_SIGNATURES_VERSION_CONSTRAINT: &str = ">=20170211,<20170212";

/// Stable identity of the reviewed Function::Parameters activation contract.
pub const FUNCTION_PARAMETERS_PROFILE_VERSION: &str = "function-parameters.2.v1";

/// Stable identity of the reviewed Method::Signatures activation contract.
pub const METHOD_SIGNATURES_PROFILE_VERSION: &str = "method-signatures.20170211.v1";

/// Provisional adapter identity for `use Function::Parameters`.
pub const FUNCTION_PARAMETERS_ADAPTER_ID: AdapterId = AdapterId(0x0046_5032);

/// Provisional adapter identity for `use Method::Signatures`.
pub const METHOD_SIGNATURES_ADAPTER_ID: AdapterId = AdapterId(0x004D_5331);

/// Current descriptor schema revision.
pub const SIGNATURE_KEYWORD_DESCRIPTOR_REVISION: u32 =
    crate::framework::FRAMEWORK_ADAPTER_SCHEMA_VERSION;

const CALLABLE_FACT_IDENTITY_SALT: u64 = 0x4650_4D53_4331_3100;

/// Reviewed module family that can enable literal signature keywords.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SignatureKeywordFamily {
    /// `Function::Parameters` (`fun` / `method`).
    FunctionParameters,
    /// `Method::Signatures` (`func` / `method`).
    MethodSignatures,
}

impl SignatureKeywordFamily {
    /// Exact module selector owned by this family.
    #[must_use]
    pub const fn module_name(self) -> &'static str {
        match self {
            Self::FunctionParameters => "Function::Parameters",
            Self::MethodSignatures => "Method::Signatures",
        }
    }

    /// Stable human-readable adapter name.
    #[must_use]
    pub const fn adapter_name(self) -> &'static str {
        match self {
            Self::FunctionParameters => "function-parameters",
            Self::MethodSignatures => "method-signatures",
        }
    }

    /// Reviewed version constraint for this family.
    #[must_use]
    pub const fn version_constraint(self) -> &'static str {
        match self {
            Self::FunctionParameters => FUNCTION_PARAMETERS_VERSION_CONSTRAINT,
            Self::MethodSignatures => METHOD_SIGNATURES_VERSION_CONSTRAINT,
        }
    }

    /// Reviewed activation-profile identity.
    #[must_use]
    pub const fn profile_version(self) -> &'static str {
        match self {
            Self::FunctionParameters => FUNCTION_PARAMETERS_PROFILE_VERSION,
            Self::MethodSignatures => METHOD_SIGNATURES_PROFILE_VERSION,
        }
    }

    const fn adapter_id(self) -> AdapterId {
        match self {
            Self::FunctionParameters => FUNCTION_PARAMETERS_ADAPTER_ID,
            Self::MethodSignatures => METHOD_SIGNATURES_ADAPTER_ID,
        }
    }

    /// Parse an exact module spelling, optionally followed by a reviewed version.
    ///
    /// Nested modules such as `Function::Parameters::Strict` are rejected: a
    /// trailing non-space suffix is not a version, and a trailing space plus
    /// non-version token is not a selector.
    #[must_use]
    pub fn from_module_spelling(module: &str) -> Option<(Self, Option<String>)> {
        parse_exact_module_version(module, Self::FunctionParameters)
            .or_else(|| parse_exact_module_version(module, Self::MethodSignatures))
    }

    /// Canonical descriptor for this family.
    #[must_use]
    pub fn descriptor(self) -> AdapterDescriptor {
        family_descriptor(self)
    }

    /// Detect this family from one checked input.
    #[must_use]
    pub fn detect(self, input: &AdapterDetectionInput) -> AdapterDetectionResult {
        detect_family(input, self)
    }

    /// Default reviewed keyword set for an argument-less import.
    #[must_use]
    pub const fn default_keywords(self) -> SignatureKeywordSet {
        match self {
            Self::FunctionParameters => SignatureKeywordSet::function_parameters_default(),
            Self::MethodSignatures => SignatureKeywordSet::method_signatures_default(),
        }
    }
}

/// Literal keyword admitted by a reviewed default profile.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SignatureKeyword {
    /// Function::Parameters `fun`.
    Fun,
    /// Method::Signatures `func`.
    Func,
    /// Shared `method` keyword.
    Method,
}

impl SignatureKeyword {
    /// Source spelling of the keyword.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fun => "fun",
            Self::Func => "func",
            Self::Method => "method",
        }
    }

    /// Whether this keyword declares a method rather than an ordinary function.
    #[must_use]
    pub const fn callable_kind(self) -> SignatureCallableKind {
        match self {
            Self::Fun | Self::Func => SignatureCallableKind::Function,
            Self::Method => SignatureCallableKind::Method,
        }
    }

    /// Map a source token onto a reviewed keyword.
    #[must_use]
    pub fn from_source_token(name: &str) -> Option<Self> {
        match name {
            "fun" => Some(Self::Fun),
            "func" => Some(Self::Func),
            "method" => Some(Self::Method),
            _ => None,
        }
    }
}

/// Method versus ordinary-function disposition for one declaration.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SignatureCallableKind {
    /// Ordinary named function (`fun` / `func`).
    Function,
    /// Method with an implicit invocant (`method`).
    Method,
}

/// Source-side import disposition retained with an activation site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureKeywordImportDisposition {
    /// Reviewed default or explicit keyword-tag import.
    Exact {
        /// Keywords installed by the reviewed import.
        keywords: SignatureKeywordSet,
    },
    /// Empty, customized, or otherwise unmodeled import arguments.
    Unmodeled {
        /// Normalized argument tokens retained for inspection.
        arguments: Vec<String>,
    },
}

impl SignatureKeywordImportDisposition {
    /// Whether the import spelling is inside the reviewed activation profile.
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        matches!(self, Self::Exact { .. })
    }

    /// Keywords installed by an exact import; empty when unmodeled.
    #[must_use]
    pub fn keywords(&self) -> SignatureKeywordSet {
        match self {
            Self::Exact { keywords } => *keywords,
            Self::Unmodeled { .. } => SignatureKeywordSet::none(),
        }
    }
}

/// Reviewed keyword enablement for one lexical import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignatureKeywordSet {
    fun: bool,
    func: bool,
    method: bool,
}

impl SignatureKeywordSet {
    /// No keywords enabled.
    #[must_use]
    pub const fn none() -> Self {
        Self { fun: false, func: false, method: false }
    }

    /// Function::Parameters default / `:std` / `:strict` / `:lax`.
    #[must_use]
    pub const fn function_parameters_default() -> Self {
        Self { fun: true, func: false, method: true }
    }

    /// Method::Signatures default.
    #[must_use]
    pub const fn method_signatures_default() -> Self {
        Self { fun: false, func: true, method: true }
    }

    /// Function::Parameters `fun` only.
    #[must_use]
    pub const fn fun_only() -> Self {
        Self { fun: true, func: false, method: false }
    }

    /// Function::Parameters `method` only.
    #[must_use]
    pub const fn method_only() -> Self {
        Self { fun: false, func: false, method: true }
    }

    /// Method::Signatures `func` only.
    #[must_use]
    pub const fn func_only() -> Self {
        Self { fun: false, func: true, method: false }
    }

    /// Enable one reviewed keyword.
    #[must_use]
    pub const fn with(mut self, keyword: SignatureKeyword) -> Self {
        match keyword {
            SignatureKeyword::Fun => self.fun = true,
            SignatureKeyword::Func => self.func = true,
            SignatureKeyword::Method => self.method = true,
        }
        self
    }

    /// Whether `keyword` is enabled.
    #[must_use]
    pub const fn contains(self, keyword: SignatureKeyword) -> bool {
        match keyword {
            SignatureKeyword::Fun => self.fun,
            SignatureKeyword::Func => self.func,
            SignatureKeyword::Method => self.method,
        }
    }

    /// Whether any reviewed keyword is enabled.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !self.fun && !self.func && !self.method
    }
}

/// Load-bearing source identity for one activating import site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureKeywordSiteAnchor {
    /// Caller package at the activating import.
    pub package: Option<String>,
    /// First byte of the import statement.
    pub span_start_byte: u32,
    /// Byte immediately after the import statement.
    pub span_end_byte: u32,
    /// Source generation from which this site was extracted.
    pub source_generation: SourceGeneration,
}

impl SignatureKeywordSiteAnchor {
    /// Construct a source anchor.
    #[must_use]
    pub fn new(
        package: Option<String>,
        span_start_byte: u32,
        span_end_byte: u32,
        source_generation: SourceGeneration,
    ) -> Self {
        Self { package, span_start_byte, span_end_byte, source_generation }
    }
}

/// Canonical descriptor for Function::Parameters activation.
#[must_use]
pub fn function_parameters_descriptor() -> AdapterDescriptor {
    family_descriptor(SignatureKeywordFamily::FunctionParameters)
}

/// Canonical descriptor for Method::Signatures activation.
#[must_use]
pub fn method_signatures_descriptor() -> AdapterDescriptor {
    family_descriptor(SignatureKeywordFamily::MethodSignatures)
}

/// Deterministic descriptor inventory, Function::Parameters then Method::Signatures.
#[must_use]
pub fn signature_keyword_descriptors() -> [AdapterDescriptor; 2] {
    [function_parameters_descriptor(), method_signatures_descriptor()]
}

fn family_descriptor(family: SignatureKeywordFamily) -> AdapterDescriptor {
    AdapterDescriptor::new(
        family.adapter_id(),
        family.adapter_name(),
        family.module_name(),
        Some(family.version_constraint().to_string()),
        SIGNATURE_KEYWORD_DESCRIPTOR_REVISION,
        AdapterDisposition::Shadow,
    )
}

/// Detect Function::Parameters activation from one checked input.
#[must_use]
pub fn detect_function_parameters(input: &AdapterDetectionInput) -> AdapterDetectionResult {
    detect_family(input, SignatureKeywordFamily::FunctionParameters)
}

/// Detect Method::Signatures activation from one checked input.
#[must_use]
pub fn detect_method_signatures(input: &AdapterDetectionInput) -> AdapterDetectionResult {
    detect_family(input, SignatureKeywordFamily::MethodSignatures)
}

fn detect_family(
    input: &AdapterDetectionInput,
    family: SignatureKeywordFamily,
) -> AdapterDetectionResult {
    let canonical = family_descriptor(family);
    if input.descriptor != canonical {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unsupported {
                reason: format!(
                    "input descriptor does not match the canonical {} descriptor",
                    family.module_name()
                ),
            },
        );
    }
    if input.cancellation.is_cancelled {
        return AdapterDetectionResult::for_input(input, DetectionOutcome::Cancelled);
    }
    if !input.module_observation.generation.is_known() {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unavailable { reason: UnavailableReason::MissingGeneration },
        );
    }
    if input.detector_policy_identity.trim().is_empty()
        || input.module_observation.resolver_identity.trim().is_empty()
        || input.module_observation.scope_identity.trim().is_empty()
        || input.module_observation.environment_identity.trim().is_empty()
        || input.module_observation.content_digest.trim().is_empty()
    {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unavailable { reason: UnavailableReason::InternalError },
        );
    }

    let selector = family.module_name();
    let owned: Vec<&ModuleSelectorEvaluation> = input
        .module_observation
        .evaluations
        .iter()
        .filter(|evaluation| evaluation.selector == selector)
        .collect();
    let [evaluation] = owned.as_slice() else {
        return if owned.is_empty() {
            AdapterDetectionResult::for_input(
                input,
                DetectionOutcome::Unavailable { reason: UnavailableReason::NoModulesAvailable },
            )
        } else {
            AdapterDetectionResult::for_input(
                input,
                DetectionOutcome::Conflicting {
                    conflict_descriptions: vec![format!(
                        "selector `{selector}` carries {} terminal evaluations; completeness \
                         requires exactly one",
                        owned.len()
                    )],
                },
            )
        };
    };
    let evaluation = *evaluation;

    match &evaluation.outcome {
        ModuleSelectorOutcome::Absent => AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Absent { reason: DetectionAbsenceReason::RequiredModulesMissing },
        ),
        ModuleSelectorOutcome::Unresolved { .. } | ModuleSelectorOutcome::Unavailable { .. } => {
            AdapterDetectionResult::for_input(
                input,
                DetectionOutcome::Unavailable { reason: UnavailableReason::NoModulesAvailable },
            )
        }
        ModuleSelectorOutcome::Ambiguous { .. } => AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Conflicting {
                conflict_descriptions: vec![format!(
                    "selector `{selector}` matched more than one module identity"
                )],
            },
        ),
        ModuleSelectorOutcome::Matched { activation, evidence_class } => {
            if activation.module_name != selector {
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Conflicting {
                        conflict_descriptions: vec![format!(
                            "selector `{selector}` resolved to foreign module `{}`",
                            activation.module_name
                        )],
                    },
                );
            }
            if !activation.generation.is_known()
                || activation.generation != input.module_observation.generation
            {
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unavailable { reason: UnavailableReason::MissingGeneration },
                );
            }

            let identity_confidence = evidence_class.confidence_ceiling();
            if identity_confidence != Confidence::High {
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: format!(
                            "{selector} matched with {identity_confidence:?} identity evidence; \
                             exact activation requires resolved module or import identity"
                        ),
                    },
                );
            }

            let Some(version) = &activation.observed_version else {
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: format!(
                            "{selector} activation lacks observed version evidence; the reviewed \
                             version constraint cannot be checked"
                        ),
                    },
                );
            };
            if version.generation != activation.generation {
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unavailable { reason: UnavailableReason::MissingGeneration },
                );
            }

            match crate::framework::version_constraint_matches(
                family.version_constraint(),
                &version.version,
            ) {
                Some(true) => AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Detected {
                        confidence: Confidence::High,
                        framework_version: Some(version.version.clone()),
                    },
                )
                .with_contributing_modules(vec![activation.clone()])
                .with_version_evidence(version.clone()),
                Some(false) => AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Absent {
                        reason: DetectionAbsenceReason::VersionConstraintNotSatisfied,
                    },
                )
                .with_contributing_modules(vec![activation.clone()])
                .with_version_evidence(version.clone()),
                None => AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: format!(
                            "observed {selector} version `{}` is not comparable with the reviewed \
                             constraint `{}`",
                            version.version,
                            family.version_constraint()
                        ),
                    },
                ),
            }
        }
    }
}

/// Classify Function::Parameters or Method::Signatures import arguments.
///
/// Empty `use Module ();` is unmodeled: the parser reports no arguments, so
/// callers must pass `explicit_empty` from the source spelling. Hash-ref
/// customization and `:modifiers` stay unmodeled rather than guessed.
#[must_use]
pub fn classify_signature_keyword_import(
    family: SignatureKeywordFamily,
    args: &[String],
    explicit_empty: bool,
) -> SignatureKeywordImportDisposition {
    if explicit_empty {
        return SignatureKeywordImportDisposition::Unmodeled { arguments: Vec::new() };
    }
    let tokens = normalize_import_tokens(args);
    if tokens.iter().any(|token| token.contains('{')) {
        return SignatureKeywordImportDisposition::Unmodeled { arguments: tokens };
    }
    if tokens.is_empty() {
        return SignatureKeywordImportDisposition::Exact { keywords: family.default_keywords() };
    }

    let mut keywords = SignatureKeywordSet::none();
    let mut saw_keyword_tag = false;
    for token in &tokens {
        if is_version_spelling(token) {
            continue;
        }
        match (family, token.as_str()) {
            (SignatureKeywordFamily::FunctionParameters, ":std" | ":strict" | ":lax") => {
                keywords = family.default_keywords();
                saw_keyword_tag = true;
            }
            (SignatureKeywordFamily::FunctionParameters, "fun") => {
                keywords = keywords.with(SignatureKeyword::Fun);
                saw_keyword_tag = true;
            }
            (SignatureKeywordFamily::FunctionParameters, "method") => {
                keywords = keywords.with(SignatureKeyword::Method);
                saw_keyword_tag = true;
            }
            (SignatureKeywordFamily::MethodSignatures, "func") => {
                keywords = keywords.with(SignatureKeyword::Func);
                saw_keyword_tag = true;
            }
            (SignatureKeywordFamily::MethodSignatures, "method") => {
                keywords = keywords.with(SignatureKeyword::Method);
                saw_keyword_tag = true;
            }
            (_, ":modifiers" | "before" | "after" | "around" | "augment" | "override") => {
                return SignatureKeywordImportDisposition::Unmodeled { arguments: tokens };
            }
            _ => return SignatureKeywordImportDisposition::Unmodeled { arguments: tokens },
        }
    }
    if saw_keyword_tag && !keywords.is_empty() {
        SignatureKeywordImportDisposition::Exact { keywords }
    } else if tokens.iter().all(|token| is_version_spelling(token)) {
        SignatureKeywordImportDisposition::Exact { keywords: family.default_keywords() }
    } else {
        SignatureKeywordImportDisposition::Unmodeled { arguments: tokens }
    }
}

/// Classify one `use` spelling into family, requested version, and import disposition.
///
/// `source_span` must be the original statement text so explicit-empty
/// `use Module ();` can be distinguished from a default import. A missing
/// span is unmodeled rather than guessed as exact.
#[must_use]
pub fn classify_signature_keyword_use(
    module: &str,
    args: &[String],
    source_span: Option<&str>,
) -> Option<(SignatureKeywordFamily, Option<String>, SignatureKeywordImportDisposition)> {
    let (family, mut requested_version) = SignatureKeywordFamily::from_module_spelling(module)?;
    let Some(source_span) = source_span else {
        return Some((
            family,
            requested_version,
            SignatureKeywordImportDisposition::Unmodeled {
                arguments: vec!["<invalid-source-span>".to_string()],
            },
        ));
    };
    if requested_version.is_none() {
        for token in args {
            let trimmed = token.trim();
            if is_version_spelling(trimmed) {
                requested_version = Some(trimmed.to_string());
                break;
            }
        }
    }
    let explicit_empty = source_has_explicit_empty_import(source_span, family.module_name());
    Some((
        family,
        requested_version,
        classify_signature_keyword_import(family, args, explicit_empty),
    ))
}

fn parse_exact_module_version(
    module: &str,
    family: SignatureKeywordFamily,
) -> Option<(SignatureKeywordFamily, Option<String>)> {
    let suffix = module.strip_prefix(family.module_name())?;
    if suffix.is_empty() {
        return Some((family, None));
    }
    let version = suffix.strip_prefix(' ')?;
    if is_version_spelling(version) { Some((family, Some(version.to_string()))) } else { None }
}

/// Whether `value` is a reviewed version spelling (digits, `.`, `_`).
#[must_use]
pub fn is_version_spelling(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_digit() || matches!(character, '.' | '_'))
}

fn normalize_import_tokens(args: &[String]) -> Vec<String> {
    args.iter().flat_map(|argument| expand_import_token(argument)).collect()
}

fn expand_import_token(argument: &str) -> Vec<String> {
    let trimmed = argument.trim();
    if trimmed.is_empty() || matches!(trimmed, "," | "=>" | ";") {
        return Vec::new();
    }
    if let Some(inner) = trimmed.strip_prefix("qw(").and_then(|rest| rest.strip_suffix(')')) {
        return inner
            .split_whitespace()
            .filter(|part| !part.is_empty())
            .map(|part| part.trim_matches(|ch| matches!(ch, '\'' | '"')).to_string())
            .collect();
    }
    vec![trimmed.trim_matches(|ch| matches!(ch, '\'' | '"')).to_string()]
}

/// Whether `source_span` is an explicit-empty import such as `use Module ();`.
///
/// An optional reviewed version may appear between the module name and the
/// empty list. Interior whitespace inside the parentheses is still empty.
#[must_use]
pub fn source_has_explicit_empty_import(source_span: &str, module: &str) -> bool {
    let rest = match source_span.split_once(module) {
        Some((_, rest)) => rest,
        None => source_span,
    };
    let rest = skip_optional_version_token(rest.trim_start());
    let Some(inner) = rest.strip_prefix('(') else {
        return false;
    };
    inner.trim_start().starts_with(')')
}

fn skip_optional_version_token(rest: &str) -> &str {
    let end = rest
        .find(|character: char| !character.is_ascii_digit() && !matches!(character, '.' | '_'))
        .unwrap_or(rest.len());
    if end > 0 && is_version_spelling(&rest[..end]) { rest[end..].trim_start() } else { rest }
}

/// One source-extracted literal declaration awaiting minting.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureKeywordDeclaration {
    /// Package the declaration appears in.
    pub package: Option<String>,
    /// File the declaration appears in.
    pub file_id: FileId,
    /// Source-order index among extracted declarations.
    pub declaration_index: u32,
    /// Family whose exact import enabled this keyword in scope.
    pub family: SignatureKeywordFamily,
    /// Keyword that introduced the declaration.
    pub keyword: SignatureKeyword,
    /// Canonical callable name.
    pub name: String,
    /// Name token range.
    pub name_anchor: SourceAnchor,
    /// Whole declaration range, keyword through body closer.
    pub declaration_anchor: SourceAnchor,
    /// Signature parenthesis range, when present.
    pub signature_anchor: Option<SourceAnchor>,
    /// Body brace range, when recovered.
    pub body_anchor: Option<SourceAnchor>,
    /// Accepted parameter facts at canonical-signature strength.
    pub parameters: Vec<SignatureKeywordParameter>,
    /// Why remaining parameter forms were not flattened into exact facts.
    pub parameter_limitations: Vec<String>,
    /// Extraction generation; minting requires this to match detection.
    pub source_generation: SourceGeneration,
}

impl SignatureKeywordDeclaration {
    /// Construct one source-extracted declaration carrier.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        package: Option<String>,
        file_id: FileId,
        declaration_index: u32,
        family: SignatureKeywordFamily,
        keyword: SignatureKeyword,
        name: impl Into<String>,
        name_anchor: SourceAnchor,
        declaration_anchor: SourceAnchor,
        signature_anchor: Option<SourceAnchor>,
        body_anchor: Option<SourceAnchor>,
        parameters: Vec<SignatureKeywordParameter>,
        parameter_limitations: Vec<String>,
    ) -> Self {
        Self {
            package,
            file_id,
            declaration_index,
            family,
            keyword,
            name: name.into(),
            name_anchor,
            declaration_anchor,
            signature_anchor,
            body_anchor,
            parameters,
            parameter_limitations,
            source_generation: SourceGeneration::Unknown,
        }
    }
}

/// One accepted positional, optional, or slurpy parameter.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureKeywordParameter {
    /// Variable name without sigil.
    pub name: String,
    /// Sigil (`$`, `@`, `%`).
    pub sigil: String,
    /// Canonical parameter shape.
    pub kind: SignatureParameterKind,
}

impl SignatureKeywordParameter {
    /// Construct one accepted parameter fact.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        sigil: impl Into<String>,
        kind: SignatureParameterKind,
    ) -> Self {
        Self { name: name.into(), sigil: sigil.into(), kind }
    }
}

/// Parameter shapes the canonical signature owner already admits.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SignatureParameterKind {
    /// Positional mandatory scalar.
    Positional,
    /// Optional parameter whose default was a source-literal form.
    Optional,
    /// Slurpy array or hash.
    Slurpy,
}

/// Canonical callable/method fact minted from one literal declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureKeywordCallableFact {
    /// Shared semantic identity, generation, and invalidation data.
    pub envelope: SemanticFactEnvelope,
    /// Framework family that minted the fact.
    pub family: SignatureKeywordFamily,
    /// Keyword that introduced the declaration.
    pub keyword: SignatureKeyword,
    /// Method versus ordinary function.
    pub callable_kind: SignatureCallableKind,
    /// Canonical name.
    pub name: String,
    /// Accepted parameter facts.
    pub parameters: Vec<SignatureKeywordParameter>,
    /// Name token range.
    pub name_anchor: SourceAnchor,
    /// Signature parenthesis range, when present.
    pub signature_anchor: Option<SourceAnchor>,
    /// Body brace range, when recovered.
    pub body_anchor: Option<SourceAnchor>,
    /// Reviewed activation-profile identity.
    pub profile_version: &'static str,
}

impl SignatureKeywordCallableFact {
    /// Whether this fact answers a shared name/package signature query.
    #[must_use]
    pub fn matches_signature_query(&self, package: Option<&str>, name: &str) -> bool {
        self.name == name && self.envelope.package.as_deref() == package
    }
}

/// Look up one minted callable by package and name, in source order.
///
/// This is the bounded shared semantic query used by a downstream
/// signature/navigation consumer. It does not parse framework syntax.
#[must_use]
pub fn lookup_signature_keyword_callable<'a>(
    facts: &'a [SignatureKeywordCallableFact],
    package: Option<&str>,
    name: &str,
) -> Option<&'a SignatureKeywordCallableFact> {
    facts.iter().find(|fact| fact.matches_signature_query(package, name))
}

/// Mint canonical callable facts for one detected family and one package.
///
/// Returns no facts unless detection is exact and the declaration was enabled
/// by that family's reviewed keyword set. Generation movement changes fact
/// identity through the envelope's invalidation inputs.
#[must_use]
pub fn signature_keyword_callable_facts(
    detection: &AdapterDetectionResult,
    family: SignatureKeywordFamily,
    package: Option<&str>,
    declarations: &[SignatureKeywordDeclaration],
) -> Vec<SignatureKeywordCallableFact> {
    if !detection.is_detected() {
        return Vec::new();
    }
    if detection.descriptor != family.descriptor() {
        return Vec::new();
    }
    let generation = &detection.project_generation;
    if !generation.is_known() {
        return Vec::new();
    }
    declarations
        .iter()
        .filter(|declaration| {
            declaration.family == family
                && family_admits_keyword(family, declaration.keyword)
                && declaration.package.as_deref() == package
                && declaration.source_generation == *generation
        })
        .map(|declaration| mint_callable_fact(family, generation, declaration))
        .collect()
}

const fn family_admits_keyword(family: SignatureKeywordFamily, keyword: SignatureKeyword) -> bool {
    matches!(
        (family, keyword),
        (
            SignatureKeywordFamily::FunctionParameters,
            SignatureKeyword::Fun | SignatureKeyword::Method
        ) | (
            SignatureKeywordFamily::MethodSignatures,
            SignatureKeyword::Func | SignatureKeyword::Method
        )
    )
}

fn mint_callable_fact(
    family: SignatureKeywordFamily,
    generation: &SourceGeneration,
    declaration: &SignatureKeywordDeclaration,
) -> SignatureKeywordCallableFact {
    let (fact_id, entity_id) = signature_keyword_callable_identity(
        declaration.file_id,
        declaration.declaration_index,
        generation,
    );
    let exact_parameters = declaration.parameter_limitations.is_empty();
    let boundary = if exact_parameters {
        None
    } else {
        Some(BoundaryLink::new(
            None,
            BoundaryKind::Unsupported,
            BoundaryDisposition::Degrade,
            SemanticReasonCode::GeneratedFromSource,
        ))
    };
    let envelope = SemanticFactEnvelope::new(
        fact_id,
        Some(entity_id),
        SemanticFactKind::Declaration,
        declaration.declaration_anchor,
        generation.clone(),
        None,
        declaration.package.clone(),
        LifecyclePhase::Runtime,
        SemanticProducer::FrameworkAdapter,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
        boundary,
        vec![
            InvalidationDependency::new(
                format!("source:{}", declaration.file_id.0),
                generation.clone(),
            ),
            InvalidationDependency::new(
                format!("module:{}", family.module_name()),
                generation.clone(),
            ),
        ],
        if exact_parameters {
            SemanticReasonCode::ExactSource
        } else {
            SemanticReasonCode::GeneratedFromSource
        },
    );
    SignatureKeywordCallableFact {
        envelope,
        family,
        keyword: declaration.keyword,
        callable_kind: declaration.keyword.callable_kind(),
        name: declaration.name.clone(),
        parameters: declaration.parameters.clone(),
        name_anchor: declaration.name_anchor,
        signature_anchor: declaration.signature_anchor,
        body_anchor: declaration.body_anchor,
        profile_version: family.profile_version(),
    }
}

/// Deterministic callable-fact identity for one (file, declaration, generation).
#[must_use]
pub fn signature_keyword_callable_identity(
    file_id: FileId,
    declaration_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let generation_digest = match generation {
        SourceGeneration::Known(value) => {
            value.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x1000_0000_01b3)
            })
        }
        SourceGeneration::Unknown => 0x1a2b_3c4d_5e6f_7081_u64,
    };
    let file = file_id.0.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let index = u64::from(declaration_index).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    let fact = file ^ index ^ generation_digest ^ CALLABLE_FACT_IDENTITY_SALT;
    (FactId(fact), EntityId(fact.wrapping_add(1)))
}

/// Construct a source anchor from byte offsets.
#[must_use]
pub fn signature_keyword_anchor(file_id: FileId, start: usize, end: usize) -> SourceAnchor {
    let start_byte = u32::try_from(start).unwrap_or(u32::MAX);
    let end_byte = u32::try_from(end).unwrap_or(u32::MAX);
    SourceAnchor::new(Some(AnchorId(u64::from(start_byte))), file_id, start_byte, end_byte)
}

/// Envelope status helper for tests and consumers.
#[must_use]
pub fn callable_fact_status(fact: &SignatureKeywordCallableFact) -> SemanticFactStatus {
    fact.envelope.status()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{
        AdapterCancellation, DetectionEvidenceClass, ModuleActivationIdentity,
        ModuleObservationReceipt, ModuleVersionEvidence,
    };

    fn matched(
        family: SignatureKeywordFamily,
        version: &str,
        generation: &str,
    ) -> ModuleSelectorEvaluation {
        let activation = ModuleActivationIdentity::new(
            family.module_name(),
            Some(FileId(7)),
            SourceGeneration::known(generation),
        )
        .with_observed_version(ModuleVersionEvidence::new(
            version,
            SourceGeneration::known(generation),
        ));
        ModuleSelectorEvaluation::new(
            family.module_name(),
            ModuleSelectorOutcome::Matched {
                activation,
                evidence_class: DetectionEvidenceClass::ResolvedImport,
            },
        )
    }

    fn input(
        family: SignatureKeywordFamily,
        evaluations: Vec<ModuleSelectorEvaluation>,
        generation: &str,
    ) -> AdapterDetectionInput {
        AdapterDetectionInput::new(
            family_descriptor(family),
            ModuleObservationReceipt::new(
                "module-resolver.v1",
                "root:fixture",
                "env:fixture",
                SourceGeneration::known(generation),
                "sha256:fixture",
                evaluations,
            ),
            None,
            AdapterCancellation::active(),
        )
    }

    #[test]
    fn descriptors_are_distinct_shadow_and_version_bound() {
        let first = signature_keyword_descriptors();
        let second = signature_keyword_descriptors();
        assert_eq!(first, second);
        assert_ne!(first[0].adapter_id, first[1].adapter_id);
        assert_eq!(first[0].required_module_selectors, vec!["Function::Parameters"]);
        assert_eq!(first[1].required_module_selectors, vec!["Method::Signatures"]);
        assert!(first.iter().all(|item| item.disposition == AdapterDisposition::Shadow));
    }

    #[test]
    fn default_and_empty_imports_are_distinct() {
        let default = classify_signature_keyword_import(
            SignatureKeywordFamily::FunctionParameters,
            &[],
            false,
        );
        let empty = classify_signature_keyword_import(
            SignatureKeywordFamily::FunctionParameters,
            &[],
            true,
        );
        assert_eq!(
            default,
            SignatureKeywordImportDisposition::Exact {
                keywords: SignatureKeywordSet::function_parameters_default(),
            }
        );
        assert!(!empty.is_exact());
    }

    #[test]
    fn custom_hash_and_modifiers_stay_unmodeled() {
        let hash = classify_signature_keyword_import(
            SignatureKeywordFamily::FunctionParameters,
            &["{ fun => { defaults => 'function_strict' } }".to_string()],
            false,
        );
        let modifiers = classify_signature_keyword_import(
            SignatureKeywordFamily::FunctionParameters,
            &["qw(:modifiers)".to_string()],
            false,
        );
        assert!(!hash.is_exact());
        assert!(!modifiers.is_exact());
    }

    #[test]
    fn qw_fun_enables_only_fun() {
        let disposition = classify_signature_keyword_import(
            SignatureKeywordFamily::FunctionParameters,
            &["qw(fun)".to_string()],
            false,
        );
        assert_eq!(
            disposition,
            SignatureKeywordImportDisposition::Exact { keywords: SignatureKeywordSet::fun_only() }
        );
    }

    #[test]
    fn unsupported_version_is_absent() {
        let detection = detect_function_parameters(&input(
            SignatureKeywordFamily::FunctionParameters,
            vec![matched(SignatureKeywordFamily::FunctionParameters, "1.000000", "gen-1")],
            "gen-1",
        ));
        assert_eq!(
            detection.outcome,
            DetectionOutcome::Absent {
                reason: DetectionAbsenceReason::VersionConstraintNotSatisfied
            }
        );
    }

    #[test]
    fn reviewed_versions_detect() {
        let fp = detect_function_parameters(&input(
            SignatureKeywordFamily::FunctionParameters,
            vec![matched(SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1")],
            "gen-1",
        ));
        let ms = detect_method_signatures(&input(
            SignatureKeywordFamily::MethodSignatures,
            vec![matched(SignatureKeywordFamily::MethodSignatures, "20170211", "gen-1")],
            "gen-1",
        ));
        assert!(fp.is_detected());
        assert!(ms.is_detected());
    }

    fn sample_fun_declaration(generation: &str) -> SignatureKeywordDeclaration {
        SignatureKeywordDeclaration {
            package: Some("App".to_string()),
            file_id: FileId(1),
            declaration_index: 0,
            family: SignatureKeywordFamily::FunctionParameters,
            keyword: SignatureKeyword::Fun,
            name: "add".to_string(),
            name_anchor: signature_keyword_anchor(FileId(1), 4, 7),
            declaration_anchor: signature_keyword_anchor(FileId(1), 0, 20),
            signature_anchor: None,
            body_anchor: None,
            parameters: Vec::new(),
            parameter_limitations: Vec::new(),
            source_generation: SourceGeneration::known(generation),
        }
    }

    #[test]
    fn minting_requires_detection() {
        let absent = detect_function_parameters(&input(
            SignatureKeywordFamily::FunctionParameters,
            vec![ModuleSelectorEvaluation::absent("Function::Parameters")],
            "gen-1",
        ));
        assert!(
            signature_keyword_callable_facts(
                &absent,
                SignatureKeywordFamily::FunctionParameters,
                Some("App"),
                &[sample_fun_declaration("gen-1")],
            )
            .is_empty()
        );
    }

    #[test]
    fn minting_requires_matching_family_descriptor() {
        let ms = detect_method_signatures(&input(
            SignatureKeywordFamily::MethodSignatures,
            vec![matched(SignatureKeywordFamily::MethodSignatures, "20170211", "gen-1")],
            "gen-1",
        ));
        assert!(ms.is_detected());
        assert!(
            signature_keyword_callable_facts(
                &ms,
                SignatureKeywordFamily::FunctionParameters,
                Some("App"),
                &[sample_fun_declaration("gen-1")],
            )
            .is_empty()
        );
    }

    #[test]
    fn minting_rejects_stale_declaration_generation() {
        let current = detect_function_parameters(&input(
            SignatureKeywordFamily::FunctionParameters,
            vec![matched(SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-2")],
            "gen-2",
        ));
        assert!(current.is_detected());
        assert!(
            signature_keyword_callable_facts(
                &current,
                SignatureKeywordFamily::FunctionParameters,
                Some("App"),
                &[sample_fun_declaration("gen-1")],
            )
            .is_empty(),
            "gen-1 declarations must not mint under gen-2 detection"
        );
        assert_eq!(
            signature_keyword_callable_facts(
                &current,
                SignatureKeywordFamily::FunctionParameters,
                Some("App"),
                &[sample_fun_declaration("gen-2")],
            )
            .len(),
            1
        );
    }

    #[test]
    fn minting_rejects_keyword_outside_family() {
        let current = detect_function_parameters(&input(
            SignatureKeywordFamily::FunctionParameters,
            vec![matched(SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1")],
            "gen-1",
        ));
        let mut mismatched = sample_fun_declaration("gen-1");
        mismatched.keyword = SignatureKeyword::Func;
        assert!(current.is_detected());
        assert!(
            signature_keyword_callable_facts(
                &current,
                SignatureKeywordFamily::FunctionParameters,
                Some("App"),
                &[mismatched],
            )
            .is_empty(),
            "Method::Signatures `func` must not mint under Function::Parameters"
        );

        let method_signatures = detect_method_signatures(&input(
            SignatureKeywordFamily::MethodSignatures,
            vec![matched(SignatureKeywordFamily::MethodSignatures, "20170211", "gen-1")],
            "gen-1",
        ));
        let mut foreign_fun = sample_fun_declaration("gen-1");
        foreign_fun.family = SignatureKeywordFamily::MethodSignatures;
        foreign_fun.keyword = SignatureKeyword::Fun;
        assert!(method_signatures.is_detected());
        assert!(
            signature_keyword_callable_facts(
                &method_signatures,
                SignatureKeywordFamily::MethodSignatures,
                Some("App"),
                &[foreign_fun],
            )
            .is_empty(),
            "Function::Parameters `fun` must not mint under Method::Signatures"
        );
    }

    #[test]
    fn explicit_empty_import_uses_source_geometry_not_qw_parens() {
        assert!(source_has_explicit_empty_import(
            "use Function::Parameters ();",
            "Function::Parameters"
        ));
        assert!(!source_has_explicit_empty_import(
            "use Function::Parameters qw(fun);",
            "Function::Parameters"
        ));
        assert!(!source_has_explicit_empty_import(
            "use Function::Parameters;",
            "Function::Parameters"
        ));
        assert!(source_has_explicit_empty_import(
            "use Function::Parameters 2.002006 ();",
            "Function::Parameters"
        ));
        assert!(source_has_explicit_empty_import(
            "use Function::Parameters ( );",
            "Function::Parameters"
        ));
    }

    #[test]
    fn module_spelling_rejects_nested_and_non_version_suffixes() {
        assert_eq!(
            SignatureKeywordFamily::from_module_spelling("Function::Parameters"),
            Some((SignatureKeywordFamily::FunctionParameters, None))
        );
        assert_eq!(
            SignatureKeywordFamily::from_module_spelling("Function::Parameters 2.002006"),
            Some((SignatureKeywordFamily::FunctionParameters, Some("2.002006".to_string())))
        );
        assert_eq!(
            SignatureKeywordFamily::from_module_spelling("Function::Parameters::Strict"),
            None
        );
        assert_eq!(
            SignatureKeywordFamily::from_module_spelling("Function::Parameters not-a-version"),
            None
        );
        assert_eq!(
            SignatureKeywordFamily::from_module_spelling("Method::Signatures 20170211"),
            Some((SignatureKeywordFamily::MethodSignatures, Some("20170211".to_string())))
        );
        assert_eq!(
            SignatureKeywordFamily::from_module_spelling("Method::Signatures::Simple"),
            None
        );
    }

    #[test]
    fn use_classifier_reads_version_args_and_fail_closes_missing_span() {
        let default = classify_signature_keyword_use(
            "Function::Parameters",
            &[],
            Some("use Function::Parameters;"),
        );
        assert!(matches!(
            default,
            Some((
                SignatureKeywordFamily::FunctionParameters,
                None,
                SignatureKeywordImportDisposition::Exact { .. }
            ))
        ));

        let versioned = classify_signature_keyword_use(
            "Function::Parameters",
            &["2.002006".to_string()],
            Some("use Function::Parameters 2.002006;"),
        );
        assert_eq!(
            versioned.map(|(family, version, disposition)| {
                (family, version, disposition.is_exact())
            }),
            Some((SignatureKeywordFamily::FunctionParameters, Some("2.002006".to_string()), true))
        );

        let missing_span = classify_signature_keyword_use("Function::Parameters", &[], None);
        assert!(matches!(
            missing_span,
            Some((
                SignatureKeywordFamily::FunctionParameters,
                None,
                SignatureKeywordImportDisposition::Unmodeled { .. }
            ))
        ));
        assert!(
            classify_signature_keyword_use(
                "Function::Parameters::Strict",
                &[],
                Some("use Function::Parameters::Strict;"),
            )
            .is_none()
        );
    }

    #[test]
    fn qw_func_enables_only_func() {
        let disposition = classify_signature_keyword_import(
            SignatureKeywordFamily::MethodSignatures,
            &["qw(func)".to_string()],
            false,
        );
        assert_eq!(
            disposition,
            SignatureKeywordImportDisposition::Exact { keywords: SignatureKeywordSet::func_only() }
        );
    }
}
