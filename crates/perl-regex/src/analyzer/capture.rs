use std::collections::BTreeMap;

use crate::{
    analyzer::{CaptureMode, EffectiveModifiers, ExtendedMode, FeatureState, RegexLanguageProfile},
    syntax::event::{
        RegexEmbeddedCodeKind, RegexEventBudget, RegexEventKind, RegexExtendedMode,
        RegexGroupKind, RegexModeState, parse_regex_events,
    },
    validator::{RegexAnalysisBudget, RegexRange},
};

const NAMED_CAPTURE_MIN_VERSION: (u16, u16) = (5, 10);

/// Stable identifier for one capture declaration in an analysis result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CaptureId(u32);

impl CaptureId {
    /// Return the zero-based declaration index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// Source spelling used to declare a capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CaptureSyntax {
    /// Ordinary unnamed capturing group `( ... )`.
    Unnamed,
    /// Perl angle-bracket named capture `(?<name> ... )`.
    NamedAngle,
    /// Perl quote named capture `(?'name' ... )`.
    NamedQuote,
    /// Python-compatible named capture `(?P<name> ... )`.
    PythonNamed,
}

/// Confidence that the declaration's source range is exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CaptureSourceConfidence {
    /// Opening and closing group ranges are both source-backed.
    Exact,
    /// The declaration was retained from malformed or truncated input.
    Recovered,
}

/// Confidence that the declaration's numeric capture identity is exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CaptureNumberConfidence {
    /// The capture number follows from the complete static prefix.
    Exact,
    /// Earlier interpolation or runtime-supplied pattern text can change numbering.
    DynamicUnknown,
}

/// Confidence that the capture name is valid for the supplied language profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CaptureProfileConfidence {
    /// The modeled profile establishes the name/form inside the supported subset.
    Exact,
    /// Version or UTF-8 state is unknown, or the Unicode spelling is outside the
    /// analyzer's conservative exact subset.
    ProfileDependent,
    /// The supplied profile establishes that the form is unavailable.
    Incompatible,
}

/// Local confidence dimensions for one capture declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct CaptureConfidence {
    /// Source-range confidence.
    pub source: CaptureSourceConfidence,
    /// Numbering confidence.
    pub number: CaptureNumberConfidence,
    /// Language-profile confidence.
    pub profile: CaptureProfileConfidence,
}

/// One source-backed capture declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CaptureDeclaration {
    /// Stable declaration identity within this analysis.
    pub id: CaptureId,
    /// Capture name, when named.
    pub name: Option<String>,
    /// One-based Perl capture number when statically known.
    pub number: Option<u32>,
    /// Full group range, including parentheses.
    pub group_range: RegexRange,
    /// Exact name-token range for a named capture.
    pub name_range: Option<RegexRange>,
    /// Group body range, excluding declaration prefix and closing parenthesis.
    pub body_range: RegexRange,
    /// Declaration syntax.
    pub syntax: CaptureSyntax,
    /// Local confidence dimensions.
    pub confidence: CaptureConfidence,
}

/// All declarations sharing one named-capture spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NamedCaptureFamily {
    /// Named-capture spelling.
    pub name: String,
    /// Declarations in source order; duplicate names remain distinct.
    pub declarations: Vec<CaptureId>,
}

/// Stable capture-analysis diagnostic identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum CaptureDiagnosticCode {
    /// Capture name is empty or invalid in the modeled exact subset.
    InvalidName,
    /// Named capture or branch reset requires Perl 5.10 or newer.
    RequiresPerlVersion,
    /// Non-ASCII source spelling requires source UTF-8 semantics.
    RequiresSourceUtf8,
}

impl CaptureDiagnosticCode {
    /// Stable machine token for later LSP/catalog projection.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidName => "invalid_capture_name",
            Self::RequiresPerlVersion => "capture_requires_perl_version",
            Self::RequiresSourceUtf8 => "capture_requires_source_utf8",
        }
    }
}

/// One source-backed capture-analysis diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CaptureDiagnostic {
    /// Stable diagnostic identity.
    pub code: CaptureDiagnosticCode,
    /// Exact source range responsible for the diagnostic.
    pub range: RegexRange,
    /// Required Perl version where applicable.
    pub required_perl_version: Option<(u16, u16)>,
}

/// Source/profile facts needed to interpret capture names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct CaptureLanguageProfile {
    /// Regex version/feature profile from modifier analysis.
    pub regex: RegexLanguageProfile,
    /// Whether source text is interpreted with UTF-8 identifier semantics.
    pub source_utf8: FeatureState,
}

impl CaptureLanguageProfile {
    /// Construct an explicit capture profile.
    #[must_use]
    pub const fn new(regex: RegexLanguageProfile, source_utf8: FeatureState) -> Self {
        Self { regex, source_utf8 }
    }

    /// Construct an unknown profile for compatibility projections.
    #[must_use]
    pub const fn unknown() -> Self {
        Self { regex: RegexLanguageProfile::unknown(), source_utf8: FeatureState::Unknown }
    }
}

/// Completeness and recovery status for capture analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct CaptureAnalysisStatus {
    /// Runtime-supplied/interpolated pattern text made later numbering unknown.
    pub dynamic: bool,
    /// Malformed or truncated group structure was observed.
    pub malformed: bool,
    /// Deterministic structural analysis stopped at a declared budget.
    pub exhausted: Option<RegexAnalysisBudget>,
}

impl CaptureAnalysisStatus {
    /// Whether capture declarations and numbering are complete for the modeled subset.
    #[must_use]
    pub const fn is_complete(self) -> bool {
        !self.dynamic && !self.malformed && self.exhausted.is_none()
    }
}

/// Canonical capture declarations, families, diagnostics, and status.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CaptureAnalysis {
    /// All ordinary and named capture declarations in source order.
    pub declarations: Vec<CaptureDeclaration>,
    /// Named families in lexical name order; declarations remain source ordered.
    pub named_families: Vec<NamedCaptureFamily>,
    /// Capture-specific diagnostics in source order.
    pub diagnostics: Vec<CaptureDiagnostic>,
    /// Dynamic/recovery/budget status.
    pub status: CaptureAnalysisStatus,
}

/// Legacy named-capture projection retained for API compatibility.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureGroup {
    pub name: String,
    pub index: usize,
    pub pattern: String,
}

#[derive(Debug, Clone, Copy)]
enum OpenFrame {
    Capture { declaration_index: usize },
    BranchReset {
        base: Option<u32>,
        max_next: Option<u32>,
    },
    Other,
}

pub(crate) fn analyze_captures(
    pattern: &str,
    modifiers: EffectiveModifiers,
    profile: CaptureLanguageProfile,
) -> CaptureAnalysis {
    let stream = parse_regex_events(pattern, initial_mode(modifiers));
    let mut declarations = Vec::new();
    let mut diagnostics = Vec::new();
    let mut stack = Vec::new();
    let mut next_number = Some(1u32);
    let mut dynamic = false;
    let mut malformed = stream.malformed;

    for event in &stream.events {
        match event.kind {
            RegexEventKind::GroupOpen(kind) => match kind {
                RegexGroupKind::Capturing => {
                    let declaration_index = declarations.len();
                    declarations.push(new_declaration(
                        declaration_index,
                        None,
                        allocate_number(&mut next_number),
                        event.range,
                        None,
                        CaptureSyntax::Unnamed,
                        CaptureProfileConfidence::Exact,
                        pattern.len(),
                    ));
                    stack.push(OpenFrame::Capture { declaration_index });
                }
                RegexGroupKind::NamedCapture { name_range } => {
                    let raw_name = pattern.get(name_range.start..name_range.end).unwrap_or_default();
                    let syntax = named_syntax(pattern, name_range);
                    let profile_confidence = named_capture_profile(
                        raw_name,
                        name_range,
                        profile,
                        &mut diagnostics,
                    );
                    if raw_name.is_empty() || !basic_name_shape(raw_name) {
                        diagnostics.push(CaptureDiagnostic {
                            code: CaptureDiagnosticCode::InvalidName,
                            range: name_range,
                            required_perl_version: None,
                        });
                        next_number = None;
                        stack.push(OpenFrame::Other);
                        malformed = true;
                    } else {
                        let declaration_index = declarations.len();
                        declarations.push(new_declaration(
                            declaration_index,
                            Some(raw_name.to_string()),
                            allocate_number(&mut next_number),
                            event.range,
                            Some(name_range),
                            syntax,
                            profile_confidence,
                            pattern.len(),
                        ));
                        stack.push(OpenFrame::Capture { declaration_index });
                    }
                }
                RegexGroupKind::BranchReset => {
                    let profile_confidence = form_version_confidence(
                        event.range,
                        profile,
                        &mut diagnostics,
                    );
                    if profile_confidence == CaptureProfileConfidence::Incompatible {
                        next_number = None;
                    }
                    stack.push(OpenFrame::BranchReset {
                        base: next_number,
                        max_next: next_number,
                    });
                }
                _ => stack.push(OpenFrame::Other),
            },
            RegexEventKind::Alternation => {
                if let Some(OpenFrame::BranchReset { base, max_next }) = stack.last_mut() {
                    *max_next = merge_branch_next(*max_next, next_number);
                    next_number = *base;
                }
            }
            RegexEventKind::GroupClose(_) => {
                if let Some(frame) = stack.pop() {
                    match frame {
                        OpenFrame::Capture { declaration_index } => {
                            if let Some(declaration) = declarations.get_mut(declaration_index) {
                                declaration.group_range.end = event.range.end;
                                declaration.body_range.end = event.range.start;
                                declaration.confidence.source = CaptureSourceConfidence::Exact;
                            }
                        }
                        OpenFrame::BranchReset { max_next, .. } => {
                            next_number = merge_branch_next(max_next, next_number);
                        }
                        OpenFrame::Other => {}
                    }
                }
            }
            RegexEventKind::Interpolation
            | RegexEventKind::EmbeddedCode {
                kind: RegexEmbeddedCodeKind::Deferred,
                ..
            } => {
                dynamic = true;
                next_number = None;
            }
            RegexEventKind::Malformed(_) => {
                malformed = true;
                next_number = None;
            }
            _ => {}
        }
    }

    for frame in stack {
        if let OpenFrame::Capture { declaration_index } = frame
            && let Some(declaration) = declarations.get_mut(declaration_index)
        {
            declaration.confidence.source = CaptureSourceConfidence::Recovered;
        }
    }
    if dynamic {
        for declaration in &mut declarations {
            if declaration.number.is_none() {
                declaration.confidence.number = CaptureNumberConfidence::DynamicUnknown;
            }
        }
    }

    diagnostics.sort_by_key(|diagnostic| {
        (diagnostic.range.start, diagnostic.range.end, diagnostic.code)
    });
    diagnostics.dedup_by(|left, right| {
        left.code == right.code && left.range == right.range
    });

    CaptureAnalysis {
        named_families: named_families(&declarations),
        declarations,
        diagnostics,
        status: CaptureAnalysisStatus {
            dynamic,
            malformed,
            exhausted: stream.exhausted.map(map_budget),
        },
    }
}

pub(crate) fn extract_named_captures(pattern: &str) -> Vec<CaptureGroup> {
    analyze_captures(
        pattern,
        EffectiveModifiers::default(),
        CaptureLanguageProfile::unknown(),
    )
    .declarations
    .into_iter()
    .filter_map(|declaration| {
        let name = declaration.name?;
        let number = declaration.number?;
        if declaration.confidence.source != CaptureSourceConfidence::Exact
            || declaration.confidence.profile == CaptureProfileConfidence::Incompatible
        {
            return None;
        }
        let index = usize::try_from(number).ok()?;
        let pattern = pattern
            .get(declaration.body_range.start..declaration.body_range.end)?
            .to_string();
        Some(CaptureGroup { name, index, pattern })
    })
    .collect()
}

fn new_declaration(
    index: usize,
    name: Option<String>,
    number: Option<u32>,
    open_range: RegexRange,
    name_range: Option<RegexRange>,
    syntax: CaptureSyntax,
    profile: CaptureProfileConfidence,
    pattern_len: usize,
) -> CaptureDeclaration {
    CaptureDeclaration {
        id: CaptureId(u32::try_from(index).unwrap_or(u32::MAX)),
        name,
        number,
        group_range: RegexRange { start: open_range.start, end: pattern_len },
        name_range,
        body_range: RegexRange { start: open_range.end, end: pattern_len },
        syntax,
        confidence: CaptureConfidence {
            source: CaptureSourceConfidence::Recovered,
            number: if number.is_some() {
                CaptureNumberConfidence::Exact
            } else {
                CaptureNumberConfidence::DynamicUnknown
            },
            profile,
        },
    }
}

fn allocate_number(next_number: &mut Option<u32>) -> Option<u32> {
    let number = *next_number;
    *next_number = number.and_then(|value| value.checked_add(1));
    number
}

fn merge_branch_next(accumulated: Option<u32>, current: Option<u32>) -> Option<u32> {
    match (accumulated, current) {
        (Some(left), Some(right)) => Some(left.max(right)),
        _ => None,
    }
}

fn named_families(declarations: &[CaptureDeclaration]) -> Vec<NamedCaptureFamily> {
    let mut families = BTreeMap::<String, Vec<CaptureId>>::new();
    for declaration in declarations {
        if let Some(name) = &declaration.name {
            families.entry(name.clone()).or_default().push(declaration.id);
        }
    }
    families
        .into_iter()
        .map(|(name, declarations)| NamedCaptureFamily { name, declarations })
        .collect()
}

fn named_syntax(pattern: &str, name_range: RegexRange) -> CaptureSyntax {
    if name_range.start >= 4
        && pattern
            .get(name_range.start - 4..name_range.start)
            .is_some_and(|prefix| prefix == "(?P<")
    {
        CaptureSyntax::PythonNamed
    } else if name_range.start >= 3
        && pattern
            .get(name_range.start - 3..name_range.start)
            .is_some_and(|prefix| prefix == "(?'")
    {
        CaptureSyntax::NamedQuote
    } else {
        CaptureSyntax::NamedAngle
    }
}

fn named_capture_profile(
    name: &str,
    range: RegexRange,
    profile: CaptureLanguageProfile,
    diagnostics: &mut Vec<CaptureDiagnostic>,
) -> CaptureProfileConfidence {
    let version = form_version_confidence(range, profile, diagnostics);
    if version == CaptureProfileConfidence::Incompatible {
        return version;
    }
    if name.is_ascii() {
        return version;
    }
    match profile.source_utf8 {
        FeatureState::Disabled => {
            diagnostics.push(CaptureDiagnostic {
                code: CaptureDiagnosticCode::RequiresSourceUtf8,
                range,
                required_perl_version: None,
            });
            CaptureProfileConfidence::Incompatible
        }
        FeatureState::Unknown => CaptureProfileConfidence::ProfileDependent,
        FeatureState::Enabled => {
            if conservative_unicode_name(name) {
                version
            } else {
                CaptureProfileConfidence::ProfileDependent
            }
        }
    }
}

fn form_version_confidence(
    range: RegexRange,
    profile: CaptureLanguageProfile,
    diagnostics: &mut Vec<CaptureDiagnostic>,
) -> CaptureProfileConfidence {
    match profile.regex.perl_version {
        Some(version)
            if (version.major, version.minor) < NAMED_CAPTURE_MIN_VERSION =>
        {
            diagnostics.push(CaptureDiagnostic {
                code: CaptureDiagnosticCode::RequiresPerlVersion,
                range,
                required_perl_version: Some(NAMED_CAPTURE_MIN_VERSION),
            });
            CaptureProfileConfidence::Incompatible
        }
        Some(_) => CaptureProfileConfidence::Exact,
        None => CaptureProfileConfidence::ProfileDependent,
    }
}

fn basic_name_shape(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first.is_ascii_digit() || first == '-' || !(first == '_' || first.is_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

fn conservative_unicode_name(name: &str) -> bool {
    basic_name_shape(name)
}

fn initial_mode(modifiers: EffectiveModifiers) -> RegexModeState {
    let extended = match modifiers.extended {
        ExtendedMode::Off => RegexExtendedMode::Off,
        ExtendedMode::Extended => RegexExtendedMode::Extended,
        ExtendedMode::ExtraExtended { .. } => RegexExtendedMode::ExtraExtended,
    };
    RegexModeState {
        extended,
        captures_by_default: matches!(modifiers.captures, CaptureMode::CapturingByDefault),
    }
}

fn map_budget(budget: RegexEventBudget) -> RegexAnalysisBudget {
    match budget {
        RegexEventBudget::Events => RegexAnalysisBudget::Events,
        RegexEventBudget::Nesting => RegexAnalysisBudget::Nesting,
        RegexEventBudget::Steps => RegexAnalysisBudget::Steps,
    }
}
