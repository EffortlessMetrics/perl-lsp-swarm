use crate::validator::{RegexDiagnostic, RegexDiagnosticCode, RegexRange};

const PERL_5_14: PerlVersion = PerlVersion::new(5, 14);
const PERL_5_22: PerlVersion = PerlVersion::new(5, 22);
const PERL_5_26: PerlVersion = PerlVersion::new(5, 26);
const PERL_5_44: PerlVersion = PerlVersion::new(5, 44);
const ENHANCED_XX_FEATURE: &str = "enhanced_xx";

/// Regex-family operator that owns a modifier sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RegexOperator {
    /// Bare match form, `/.../`.
    BareMatch,
    /// Explicit match form, `m/.../`.
    Match,
    /// Compiled regex form, `qr/.../`.
    QuoteRegex,
    /// Substitution form, `s/.../.../`.
    Substitution,
    /// Transliteration form, `tr/.../.../`.
    Transliteration,
    /// Transliteration alias, `y/.../.../`.
    TransliterationAlias,
}

impl RegexOperator {
    const fn is_transliteration(self) -> bool {
        matches!(self, Self::Transliteration | Self::TransliterationAlias)
    }
}

/// Perl language version used to interpret a modifier sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PerlVersion {
    /// Major Perl version component.
    pub major: u16,
    /// Minor Perl version component.
    pub minor: u16,
}

impl PerlVersion {
    /// Construct a Perl language version.
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}

/// State of a feature that changes regex semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FeatureState {
    /// The feature is enabled at this source interval.
    Enabled,
    /// The feature is known to be disabled.
    Disabled,
    /// The parser/project cannot establish the feature state.
    Unknown,
}

/// Small language profile supplied by parser or project authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RegexLanguageProfile {
    /// Effective Perl language version, when known.
    pub perl_version: Option<PerlVersion>,
    /// Effective `feature "enhanced_xx"` state.
    pub enhanced_xx: FeatureState,
}

impl RegexLanguageProfile {
    /// Construct a profile with explicit version and feature state.
    #[must_use]
    pub const fn new(perl_version: Option<PerlVersion>, enhanced_xx: FeatureState) -> Self {
        Self { perl_version, enhanced_xx }
    }

    /// Construct a profile whose version and feature state are both unknown.
    #[must_use]
    pub const fn unknown() -> Self {
        Self { perl_version: None, enhanced_xx: FeatureState::Unknown }
    }
}

/// Raw modifier spelling with its exact source range.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ModifierSequence {
    /// Ordered raw spelling, including repetition and unknown characters.
    pub raw: String,
    /// Exact range of the raw sequence in the caller's source coordinate space.
    pub range: RegexRange,
}

impl ModifierSequence {
    /// Construct a sequence at `start`, returning `None` on offset overflow.
    #[must_use]
    pub fn new(raw: impl Into<String>, start: usize) -> Option<Self> {
        let raw = raw.into();
        let end = start.checked_add(raw.len())?;
        Some(Self { raw, range: RegexRange { start, end } })
    }
}

/// One source-backed modifier token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ModifierToken {
    /// Modifier character exactly as written.
    pub value: char,
    /// Exact source range of this character.
    pub range: RegexRange,
}

/// Effective extended-whitespace mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExtendedMode {
    /// Extended mode is disabled.
    Off,
    /// `/x` semantics.
    Extended,
    /// `/xx` semantics, optionally changed by Perl 5.44 `enhanced_xx`.
    ExtraExtended {
        /// Feature state actually available for the selected profile.
        enhanced: FeatureState,
    },
}

/// Effective character-set interpretation for regex escapes and classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CharacterSetMode {
    /// No explicit `/a`, `/d`, `/l`, or `/u` modifier.
    Default,
    /// `/a` ASCII-safe mode.
    Ascii,
    /// `/aa` stricter ASCII mode.
    AsciiRestricted,
    /// `/d` platform/default mode.
    Depends,
    /// `/l` locale mode.
    Locale,
    /// `/u` Unicode mode.
    Unicode,
    /// Mutually exclusive character-set modes were combined.
    Conflict,
}

/// Effective capture default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CaptureMode {
    /// Ordinary unnamed groups capture by default.
    CapturingByDefault,
    /// `/n` makes ordinary unnamed groups non-capturing by default.
    NonCapturingByDefault,
}

/// Transliteration-specific modifier semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct TransliterationModifiers {
    /// `/c` complements the search list.
    pub complement: bool,
    /// `/d` deletes unmatched search characters.
    pub delete: bool,
    /// `/s` squashes duplicate replacement characters.
    pub squash: bool,
    /// `/r` returns a transformed copy instead of mutating the target.
    pub non_destructive: bool,
}

/// Effective semantics derived from a lossless modifier sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct EffectiveModifiers {
    /// `/i` case-insensitive matching.
    pub case_insensitive: bool,
    /// `/m` line-oriented anchors.
    pub multiline: bool,
    /// `/s` dot-matches-newline mode for regex operators.
    pub single_line: bool,
    /// `/x` or `/xx` mode.
    pub extended: ExtendedMode,
    /// `/a`, `/aa`, `/d`, `/l`, or `/u` mode.
    pub character_set: CharacterSetMode,
    /// `/n` capture default.
    pub captures: CaptureMode,
    /// `/p` match-string preservation.
    pub preserve_match: bool,
    /// `/o` compile-once request.
    pub compile_once: bool,
    /// `/g` global matching/substitution.
    pub global: bool,
    /// `/c` match-position preservation, set only for `m//gc`.
    ///
    /// Substitution `/c` and match `/c` without `/g` are accepted by Perl but have
    /// no effect, so they leave this `false` and report `ModifierHasNoEffect`.
    pub keep_match_position: bool,
    /// Number of `/e` evaluation layers for substitution.
    pub substitution_evaluation_depth: usize,
    /// `/r` non-destructive substitution result.
    pub non_destructive: bool,
    /// Transliteration-specific meanings of `/c`, `/d`, `/s`, and `/r`.
    pub transliteration: TransliterationModifiers,
}

impl Default for EffectiveModifiers {
    fn default() -> Self {
        Self {
            case_insensitive: false,
            multiline: false,
            single_line: false,
            extended: ExtendedMode::Off,
            character_set: CharacterSetMode::Default,
            captures: CaptureMode::CapturingByDefault,
            preserve_match: false,
            compile_once: false,
            global: false,
            keep_match_position: false,
            substitution_evaluation_depth: 0,
            non_destructive: false,
            transliteration: TransliterationModifiers::default(),
        }
    }
}

/// Requirement imposed by a modifier spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ModifierRequirementKind {
    /// Minimum Perl version required by the modifier form.
    PerlVersion(PerlVersion),
    /// Named feature required for the requested semantic variant.
    Feature(&'static str),
}

/// Whether the supplied profile satisfies a modifier requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RequirementDisposition {
    /// The requirement is satisfied.
    Satisfied,
    /// The profile establishes that the requirement is not satisfied.
    Unsatisfied,
    /// The profile does not carry enough information to decide.
    Unknown,
}

/// Source-backed version or feature requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ModifierRequirement {
    /// Modifier range that created this requirement.
    pub range: RegexRange,
    /// Required version or feature.
    pub kind: ModifierRequirementKind,
    /// Disposition under the supplied profile.
    pub disposition: RequirementDisposition,
}

/// Lossless modifier analysis for one operator and language profile.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ModifierAnalysis {
    /// Operator whose semantics were applied.
    pub operator: RegexOperator,
    /// Original raw sequence.
    pub sequence: ModifierSequence,
    /// One token per raw character in source order.
    pub tokens: Vec<ModifierToken>,
    /// Derived effective semantics.
    pub effective: EffectiveModifiers,
    /// Version and feature requirements, including unknown dispositions.
    pub requirements: Vec<ModifierRequirement>,
    /// Typed conformance diagnostics in source order.
    pub diagnostics: Vec<RegexDiagnostic>,
}

pub(crate) fn analyze_modifiers(
    operator: RegexOperator,
    sequence: ModifierSequence,
    profile: RegexLanguageProfile,
) -> ModifierAnalysis {
    let tokens = tokenize(&sequence);
    let mut effective = EffectiveModifiers::default();
    let mut requirements = Vec::new();
    let mut diagnostics = Vec::new();
    let mut x_count = 0usize;
    let mut character_tokens = Vec::new();
    let mut keep_position_tokens = Vec::new();

    for token in &tokens {
        let modifier = token.value;
        if !is_known_modifier(modifier) {
            push_diagnostic(&mut diagnostics, RegexDiagnosticCode::UnknownModifier, token.range);
            continue;
        }
        if !is_allowed(operator, modifier) {
            push_diagnostic(
                &mut diagnostics,
                RegexDiagnosticCode::ModifierNotAllowedForOperator,
                token.range,
            );
            continue;
        }

        if operator.is_transliteration() {
            match modifier {
                'c' => effective.transliteration.complement = true,
                'd' => effective.transliteration.delete = true,
                's' => effective.transliteration.squash = true,
                'r' => {
                    effective.transliteration.non_destructive = true;
                    record_version_requirement(
                        token.range,
                        PERL_5_14,
                        profile,
                        &mut requirements,
                        &mut diagnostics,
                    );
                }
                _ => {}
            }
            continue;
        }

        match modifier {
            'i' => effective.case_insensitive = true,
            'm' => effective.multiline = true,
            's' => effective.single_line = true,
            'x' => {
                x_count = x_count.saturating_add(1);
                match x_count {
                    1 => effective.extended = ExtendedMode::Extended,
                    2 => {
                        // Before 5.26 the second `x` is not `/xx`; the admitted
                        // behavior stays plain `/x` rather than extra-extended.
                        if record_version_requirement(
                            token.range,
                            PERL_5_26,
                            profile,
                            &mut requirements,
                            &mut diagnostics,
                        ) {
                            effective.extended = ExtendedMode::ExtraExtended {
                                enhanced: enhanced_xx_state(
                                    token.range,
                                    profile,
                                    &mut requirements,
                                    &mut diagnostics,
                                ),
                            };
                        }
                    }
                    _ => {}
                }
            }
            'a' | 'd' | 'l' | 'u' => {
                if record_version_requirement(
                    token.range,
                    PERL_5_14,
                    profile,
                    &mut requirements,
                    &mut diagnostics,
                ) {
                    character_tokens.push(*token);
                }
            }
            'n' => {
                if record_version_requirement(
                    token.range,
                    PERL_5_22,
                    profile,
                    &mut requirements,
                    &mut diagnostics,
                ) {
                    effective.captures = CaptureMode::NonCapturingByDefault;
                }
            }
            'p' => effective.preserve_match = true,
            'o' => effective.compile_once = true,
            'g' => effective.global = true,
            // `/c` is resolved after the sequence is known: it only preserves a
            // match position on match operators, and only together with `/g`.
            'c' => keep_position_tokens.push(*token),
            'e' => {
                effective.substitution_evaluation_depth =
                    effective.substitution_evaluation_depth.saturating_add(1);
            }
            'r' => {
                if record_version_requirement(
                    token.range,
                    PERL_5_14,
                    profile,
                    &mut requirements,
                    &mut diagnostics,
                ) {
                    effective.non_destructive = true;
                }
            }
            _ => {}
        }
    }

    // Perl only preserves `pos` for `m//gc`. `s///c` is accepted with a
    // "meaningless" warning and does not keep a substitution match position, and
    // match `/c` without `/g` is likewise inert. Transliteration `/c` never
    // reaches here: it is complement, modeled in `effective.transliteration`.
    let keeps_position =
        matches!(operator, RegexOperator::BareMatch | RegexOperator::Match) && effective.global;
    effective.keep_match_position = keeps_position && !keep_position_tokens.is_empty();
    if !keeps_position {
        for token in &keep_position_tokens {
            push_diagnostic(
                &mut diagnostics,
                RegexDiagnosticCode::ModifierHasNoEffect,
                token.range,
            );
        }
    }

    effective.character_set = character_set_mode(&character_tokens, &mut diagnostics);
    diagnostics
        .sort_by_key(|diagnostic| (diagnostic.range.start, diagnostic.range.end, diagnostic.code));
    diagnostics.dedup_by(|left, right| left.code == right.code && left.range == right.range);

    ModifierAnalysis { operator, sequence, tokens, effective, requirements, diagnostics }
}

fn tokenize(sequence: &ModifierSequence) -> Vec<ModifierToken> {
    sequence
        .raw
        .char_indices()
        .map(|(relative, value)| {
            let start = sequence.range.start.saturating_add(relative);
            ModifierToken {
                value,
                range: RegexRange { start, end: start.saturating_add(value.len_utf8()) },
            }
        })
        .collect()
}

fn is_known_modifier(modifier: char) -> bool {
    matches!(
        modifier,
        'i' | 'm' | 's' | 'x' | 'g' | 'a' | 'd' | 'l' | 'u' | 'n' | 'p' | 'r' | 'c' | 'o' | 'e'
    )
}

fn is_allowed(operator: RegexOperator, modifier: char) -> bool {
    match operator {
        RegexOperator::BareMatch | RegexOperator::Match => matches!(
            modifier,
            'i' | 'm' | 's' | 'x' | 'g' | 'a' | 'd' | 'l' | 'u' | 'n' | 'p' | 'c' | 'o'
        ),
        RegexOperator::QuoteRegex => {
            matches!(modifier, 'i' | 'm' | 's' | 'x' | 'a' | 'd' | 'l' | 'u' | 'n' | 'p' | 'o')
        }
        RegexOperator::Substitution => matches!(
            modifier,
            'i' | 'm' | 's' | 'x' | 'g' | 'a' | 'd' | 'l' | 'u' | 'n' | 'p' | 'r' | 'c' | 'o' | 'e'
        ),
        RegexOperator::Transliteration | RegexOperator::TransliterationAlias => {
            matches!(modifier, 'c' | 'd' | 's' | 'r')
        }
    }
}

/// Derive the effective character-set mode from the `/a`, `/d`, `/l`, and `/u` tokens.
///
/// Perl constrains both variety and multiplicity: the modes are mutually exclusive,
/// `/a` may appear at most twice (`/aa`), and `/d`, `/l`, and `/u` may appear only
/// once. Each violation is reported at the exact token that introduces it, so a
/// third `a` or a second `d`/`l`/`u` is diagnosed rather than silently deduplicated.
fn character_set_mode(
    tokens: &[ModifierToken],
    diagnostics: &mut Vec<RegexDiagnostic>,
) -> CharacterSetMode {
    let mut distinct = Vec::new();
    let mut a_count = 0usize;

    for token in tokens {
        let repetition_limit = if token.value == 'a' { 2 } else { 1 };
        let seen = tokens
            .iter()
            .take_while(|earlier| earlier.range.start < token.range.start)
            .filter(|earlier| earlier.value == token.value)
            .count();
        if seen >= repetition_limit {
            push_diagnostic(
                diagnostics,
                RegexDiagnosticCode::RepeatedCharacterSetModifier,
                token.range,
            );
        } else if token.value == 'a' {
            a_count = a_count.saturating_add(1);
        }

        if !distinct.contains(&token.value) {
            if !distinct.is_empty() {
                push_diagnostic(
                    diagnostics,
                    RegexDiagnosticCode::ConflictingCharacterSetModifiers,
                    token.range,
                );
            }
            distinct.push(token.value);
        }
    }

    if distinct.len() > 1 {
        return CharacterSetMode::Conflict;
    }
    match distinct.first().copied() {
        Some('a') if a_count >= 2 => CharacterSetMode::AsciiRestricted,
        Some('a') => CharacterSetMode::Ascii,
        Some('d') => CharacterSetMode::Depends,
        Some('l') => CharacterSetMode::Locale,
        Some('u') => CharacterSetMode::Unicode,
        _ => CharacterSetMode::Default,
    }
}

/// Record a version requirement and report whether the selected profile admits it.
///
/// Returns `true` when the effect may be applied to [`EffectiveModifiers`]. An
/// unsatisfied requirement withholds the effect so a consumer reading `effective`
/// cannot apply behavior the selected Perl could not compile. The requested form is
/// never lost: it remains in [`ModifierAnalysis::sequence`] and the token stream.
fn record_version_requirement(
    range: RegexRange,
    minimum: PerlVersion,
    profile: RegexLanguageProfile,
    requirements: &mut Vec<ModifierRequirement>,
    diagnostics: &mut Vec<RegexDiagnostic>,
) -> bool {
    let disposition = match profile.perl_version {
        Some(version) if version >= minimum => RequirementDisposition::Satisfied,
        Some(_) => RequirementDisposition::Unsatisfied,
        None => RequirementDisposition::Unknown,
    };
    requirements.push(ModifierRequirement {
        range,
        kind: ModifierRequirementKind::PerlVersion(minimum),
        disposition,
    });
    if disposition == RequirementDisposition::Unsatisfied {
        push_diagnostic(diagnostics, RegexDiagnosticCode::ModifierRequiresPerlVersion, range);
        return false;
    }
    true
}

fn enhanced_xx_state(
    range: RegexRange,
    profile: RegexLanguageProfile,
    requirements: &mut Vec<ModifierRequirement>,
    diagnostics: &mut Vec<RegexDiagnostic>,
) -> FeatureState {
    match profile.perl_version {
        Some(version) if version < PERL_5_44 => {
            if profile.enhanced_xx == FeatureState::Enabled {
                requirements.push(ModifierRequirement {
                    range,
                    kind: ModifierRequirementKind::Feature(ENHANCED_XX_FEATURE),
                    disposition: RequirementDisposition::Unsatisfied,
                });
                push_diagnostic(diagnostics, RegexDiagnosticCode::ModifierRequiresFeature, range);
            }
            FeatureState::Disabled
        }
        Some(_) => match profile.enhanced_xx {
            FeatureState::Enabled => {
                requirements.push(ModifierRequirement {
                    range,
                    kind: ModifierRequirementKind::Feature(ENHANCED_XX_FEATURE),
                    disposition: RequirementDisposition::Satisfied,
                });
                FeatureState::Enabled
            }
            FeatureState::Disabled => FeatureState::Disabled,
            FeatureState::Unknown => {
                requirements.push(ModifierRequirement {
                    range,
                    kind: ModifierRequirementKind::Feature(ENHANCED_XX_FEATURE),
                    disposition: RequirementDisposition::Unknown,
                });
                FeatureState::Unknown
            }
        },
        None => {
            if profile.enhanced_xx != FeatureState::Disabled {
                requirements.push(ModifierRequirement {
                    range,
                    kind: ModifierRequirementKind::Feature(ENHANCED_XX_FEATURE),
                    disposition: RequirementDisposition::Unknown,
                });
            }
            // An enabling pragma alone does not prove enhanced `/xx`: the 5.44
            // version boundary is unestablished, so the state stays `Unknown`
            // rather than silently selecting the newest semantics.
            match profile.enhanced_xx {
                FeatureState::Disabled => FeatureState::Disabled,
                FeatureState::Enabled | FeatureState::Unknown => FeatureState::Unknown,
            }
        }
    }
}

fn push_diagnostic(
    diagnostics: &mut Vec<RegexDiagnostic>,
    code: RegexDiagnosticCode,
    range: RegexRange,
) {
    diagnostics.push(RegexDiagnostic::new(code, range, None));
}
