//! Source-backed regex hover from retained canonical analysis (#7048).

use perl_parser_core::syntax::quote::RegexFamilyOperator;
use perl_parser_core::{RegexAnalysisAvailability, RegexAnalysisRecord, RegexAnalysisTable};
use perl_regex::analyzer::{
    CaptureSyntax, CharacterSetMode, ExtendedMode, PatternControlEffect, PatternControlKind,
};

const MAX_CAPTURE_LINES: usize = 24;

/// Render hover markdown for a cursor inside a regex-family construct, if the
/// retained table owns that offset.
#[must_use]
pub fn hover_projection_at(
    source: &str,
    table: &RegexAnalysisTable,
    offset: usize,
) -> Option<String> {
    if !table.source_matches(source) {
        return None;
    }
    let record = table.find_at_offset(offset)?;
    Some(render_record(source, record, offset))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CursorZone {
    Replacement,
    Modifier,
    PatternFact,
    Summary,
}

fn cursor_zone(record: &RegexAnalysisRecord, source: &str, offset: usize) -> CursorZone {
    if record.replacement_range().is_some_and(|range| range.start <= offset && offset < range.end) {
        return CursorZone::Replacement;
    }
    if record.modifier_range().is_some_and(|range| range.start <= offset && offset < range.end) {
        return CursorZone::Modifier;
    }
    if record.pattern_range().is_some_and(|range| range.start <= offset && offset < range.end)
        && pattern_fact_at(record, source, offset).is_some()
    {
        return CursorZone::PatternFact;
    }
    CursorZone::Summary
}

fn pattern_fact_at(
    record: &RegexAnalysisRecord,
    source: &str,
    offset: usize,
) -> Option<PatternFactFocus> {
    let pattern = record.pattern.as_ref()?;
    for declaration in &pattern.controls.captures.declarations {
        let Some(source_start) = record.pattern_range().map(|range| range.start) else {
            continue;
        };
        let map = |range: perl_regex::validator::RegexRange| -> Option<(usize, usize)> {
            let start = source_start.checked_add(range.start)?;
            let end = source_start.checked_add(range.end)?;
            (start <= end && end <= source.len()).then_some((start, end))
        };
        if declaration
            .name_range
            .and_then(map)
            .is_some_and(|(start, end)| start <= offset && offset < end)
            || map(declaration.group_range)
                .is_some_and(|(start, end)| start <= offset && offset < end)
        {
            return Some(PatternFactFocus::Capture(declaration.id));
        }
    }
    for fact in &pattern.controls.facts {
        let Some(source_range) = fact.source_range else {
            continue;
        };
        if source_range.start <= offset && offset < source_range.end {
            return Some(PatternFactFocus::Control(fact.id));
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PatternFactFocus {
    Capture(perl_regex::analyzer::CaptureId),
    Control(perl_regex::analyzer::PatternControlId),
}

fn render_record(source: &str, record: &RegexAnalysisRecord, offset: usize) -> String {
    let zone = cursor_zone(record, source, offset);
    let mut lines = Vec::new();
    lines.push(format!("**{}**", operator_heading(record)));
    match zone {
        CursorZone::Replacement => {
            lines.push("Substitution replacement (not regex syntax)".to_string());
            if let Some(text) = replacement_spelling(source, record) {
                lines.push(format!("Replacement: `{}`", escape_inline_code(&text)));
            }
            return lines.join("\n\n");
        }
        CursorZone::Modifier => {
            if let Some(raw) = modifier_spelling(source, record) {
                lines.push(format!("Modifier sequence: `{}`", escape_inline_code(&raw)));
            }
            if let Some(modifiers) = &record.modifiers {
                lines.push("Effective modifiers:".to_string());
                for line in describe_effective_modifiers(&modifiers.effective) {
                    lines.push(format!("- {line}"));
                }
            }
            return lines.join("\n\n");
        }
        CursorZone::PatternFact => {
            if let Some(focus) = pattern_fact_at(record, source, offset) {
                render_pattern_fact(source, record, focus, &mut lines);
                return lines.join("\n\n");
            }
        }
        CursorZone::Summary => {}
    }

    if let Some(text) = pattern_spelling(source, record) {
        lines.push(format!("Pattern: `{}`", escape_inline_code(&text)));
    }
    if let Some(raw) = modifier_spelling(source, record) {
        lines.push(format!("Modifiers: `{}`", escape_inline_code(&raw)));
    }
    if let Some(modifiers) = &record.modifiers {
        lines.push("Effective modifiers:".to_string());
        for line in describe_effective_modifiers(&modifiers.effective) {
            lines.push(format!("- {line}"));
        }
    }
    render_capture_section(record, &mut lines);
    render_limitations(record, &mut lines);
    lines.join("\n\n")
}

fn render_pattern_fact(
    source: &str,
    record: &RegexAnalysisRecord,
    focus: PatternFactFocus,
    lines: &mut Vec<String>,
) {
    let Some(pattern) = &record.pattern else {
        return;
    };
    match focus {
        PatternFactFocus::Capture(id) => {
            let Some(declaration) =
                pattern.controls.captures.declarations.iter().find(|d| d.id == id)
            else {
                return;
            };
            lines.push("Capture declaration".to_string());
            if let Some(name) = &declaration.name {
                lines.push(format!("Access: `$+{{{}}}`", escape_inline_code(name)));
                if let Some(name_range) = declaration.name_range {
                    if let Some(spelling) = slice_at_pattern_offset(source, record, name_range) {
                        lines.push(format!("Name: `{}`", escape_inline_code(&spelling)));
                    }
                }
            } else if let Some(number) = declaration.number {
                lines.push(format!("Access: `${number}`"));
            }
            lines.push(format!("Syntax: `{}`", capture_syntax_label(declaration.syntax)));
            if let Some(body) = slice_at_pattern_offset(source, record, declaration.body_range) {
                lines.push(format!("Body: `{}`", escape_inline_code(&body)));
            }
        }
        PatternFactFocus::Control(id) => {
            let Some(fact) = pattern.controls.facts.iter().find(|f| f.id == id) else {
                return;
            };
            lines.push("Pattern construct".to_string());
            lines.push(format!("Kind: {}", control_kind_label(&fact.kind)));
            if matches!(
                fact.effect,
                PatternControlEffect::DynamicExecution | PatternControlEffect::DynamicPattern
            ) {
                lines.push(
                    "Analysis is limited around dynamic execution or runtime pattern text."
                        .to_string(),
                );
            }
            if let Some(operand) = fact
                .source_operand_range
                .or(fact.source_range)
                .and_then(|range| source.get(range.start..range.end))
            {
                lines.push(format!("Spelling: `{}`", escape_inline_code(operand)));
            }
            render_reference_targets(pattern, fact, lines);
        }
    }
}

fn render_reference_targets(
    pattern: &perl_parser_core::RetainedRegexPatternAnalysis,
    fact: &perl_regex::analyzer::PatternControlFact,
    lines: &mut Vec<String>,
) {
    let targets = match &fact.resolution {
        perl_regex::analyzer::PatternControlResolution::Resolved { targets } => targets.as_slice(),
        perl_regex::analyzer::PatternControlResolution::ProfileDependent { known_targets }
        | perl_regex::analyzer::PatternControlResolution::DynamicUnknown { known_targets }
        | perl_regex::analyzer::PatternControlResolution::StructuralUnknown { known_targets } => {
            known_targets.as_slice()
        }
        _ => return,
    };
    if targets.is_empty() {
        return;
    }
    lines.push("Targets:".to_string());
    for target in targets {
        let Some(declaration) =
            pattern.controls.captures.declarations.iter().find(|d| d.id == *target)
        else {
            continue;
        };
        if let Some(name) = &declaration.name {
            lines.push(format!("- `$+{{{}}}`", escape_inline_code(name)));
        } else if let Some(number) = declaration.number {
            lines.push(format!("- `${number}`"));
        }
    }
}

fn render_capture_section(record: &RegexAnalysisRecord, lines: &mut Vec<String>) {
    let Some(pattern) = &record.pattern else {
        return;
    };
    let declarations = &pattern.controls.captures.declarations;
    if declarations.is_empty() {
        return;
    }
    lines.push("Captures:".to_string());
    let mut shown = 0usize;
    for declaration in declarations {
        if shown >= MAX_CAPTURE_LINES {
            let remaining = declarations.len().saturating_sub(shown);
            lines.push(format!("- … and {remaining} more capture(s)"));
            break;
        }
        if let Some(name) = &declaration.name {
            lines.push(format!("- `$+{{{}}}`", escape_inline_code(name)));
        } else if let Some(number) = declaration.number {
            lines.push(format!("- `${number}`"));
        } else {
            lines.push("- `(capture)`".to_string());
        }
        shown += 1;
    }
}

fn render_limitations(record: &RegexAnalysisRecord, lines: &mut Vec<String>) {
    if record.availability != RegexAnalysisAvailability::Analyzed {
        lines.push(format!("Analysis: {} (no regex-body facts)", record.availability.as_str()));
        return;
    }
    if record.has_embedded_code() {
        lines.push(
            "Embedded code or evaluated substitution regions limit static analysis here."
                .to_string(),
        );
    }
    let Some(pattern) = &record.pattern else {
        return;
    };
    if !pattern.controls.status.is_complete() {
        lines.push("Pattern analysis is locally incomplete.".to_string());
    }
    if !record.is_complete() {
        lines.push("Some retained facts are approximate or profile-dependent.".to_string());
    }
}

fn operator_heading(record: &RegexAnalysisRecord) -> String {
    match record.operator {
        None => "Regex-family operator".to_string(),
        Some(op) => match op {
            RegexFamilyOperator::BareMatch => "Regex (bare `/.../`)".to_string(),
            RegexFamilyOperator::Match => "Regex match (`m//`)".to_string(),
            RegexFamilyOperator::QuoteRegex => "Quote regex (`qr//`)".to_string(),
            RegexFamilyOperator::Substitution => "Substitution (`s///`)".to_string(),
            RegexFamilyOperator::Transliteration => "Transliteration (`tr///`)".to_string(),
            RegexFamilyOperator::TransliterationAlias => "Transliteration (`y///`)".to_string(),
            _ => "Regex-family operator".to_string(),
        },
    }
}

fn pattern_spelling(source: &str, record: &RegexAnalysisRecord) -> Option<String> {
    let range = record.pattern_range()?;
    source.get(range.start..range.end).map(str::to_string)
}

fn replacement_spelling(source: &str, record: &RegexAnalysisRecord) -> Option<String> {
    let range = record.replacement_range()?;
    source.get(range.start..range.end).map(str::to_string)
}

fn modifier_spelling(source: &str, record: &RegexAnalysisRecord) -> Option<String> {
    let range = record.modifier_range()?;
    source.get(range.start..range.end).map(str::to_string)
}

fn slice_at_pattern_offset(
    source: &str,
    record: &RegexAnalysisRecord,
    body_relative: perl_regex::validator::RegexRange,
) -> Option<String> {
    let mapped = record.map_pattern_range(body_relative)?;
    source.get(mapped.start..mapped.end).map(str::to_string)
}

fn capture_syntax_label(syntax: CaptureSyntax) -> &'static str {
    match syntax {
        CaptureSyntax::Unnamed => "unnamed group",
        CaptureSyntax::NamedAngle => "(?<name>...)",
        CaptureSyntax::NamedQuote => "(?'name'...)",
        CaptureSyntax::PythonNamed => "(?P<name>...)",
        _ => "capture group",
    }
}

fn control_kind_label(kind: &PatternControlKind) -> String {
    match kind {
        PatternControlKind::NumericBackreference { number, .. } => {
            format!("backreference ${number}")
        }
        PatternControlKind::NamedBackreference { name, .. } => {
            format!("backreference $+{{{name}}}")
        }
        PatternControlKind::NamedSubpatternCall { name, .. } => format!("subpattern (?&{name})"),
        PatternControlKind::NumberedSubpatternCall { number, .. } => {
            format!("subpattern (?({number}))")
        }
        other => other.as_str().replace('_', " "),
    }
}

fn describe_effective_modifiers(
    effective: &perl_regex::analyzer::EffectiveModifiers,
) -> Vec<String> {
    let mut lines = Vec::new();
    if effective.case_insensitive {
        lines.push("/i case-insensitive".to_string());
    }
    if effective.multiline {
        lines.push("/m multiline anchors".to_string());
    }
    if effective.single_line {
        lines.push("/s dot matches newline".to_string());
    }
    match effective.extended {
        ExtendedMode::Off => {}
        ExtendedMode::Extended => lines.push("/x extended whitespace".to_string()),
        ExtendedMode::ExtraExtended { enhanced } => {
            lines.push(format!(
                "/xx extra-extended whitespace (enhanced_xx: {})",
                feature_state_label(enhanced)
            ));
        }
        _ => {}
    }
    match effective.character_set {
        CharacterSetMode::Default => {}
        CharacterSetMode::Ascii => lines.push("/a ASCII rules".to_string()),
        CharacterSetMode::AsciiRestricted => lines.push("/aa restricted ASCII rules".to_string()),
        CharacterSetMode::Depends => lines.push("/d default charset".to_string()),
        CharacterSetMode::Locale => lines.push("/l locale charset".to_string()),
        CharacterSetMode::Unicode => lines.push("/u Unicode rules".to_string()),
        CharacterSetMode::Conflict => lines.push("conflicting charset modifiers".to_string()),
        _ => {}
    }
    if matches!(effective.captures, perl_regex::analyzer::CaptureMode::NonCapturingByDefault) {
        lines.push("/n non-capturing groups by default".to_string());
    }
    if effective.preserve_match {
        lines.push("/p preserve match string".to_string());
    }
    if effective.compile_once {
        lines.push("/o compile once".to_string());
    }
    if effective.global {
        lines.push("/g global".to_string());
    }
    if effective.keep_match_position {
        lines.push("/c keep match position (with /g)".to_string());
    }
    if effective.substitution_evaluation_depth > 0 {
        lines.push(format!(
            "/e{} substitution evaluation",
            "e".repeat(effective.substitution_evaluation_depth)
        ));
    }
    if effective.non_destructive {
        lines.push("/r non-destructive result".to_string());
    }
    if lines.is_empty() {
        lines.push("(default modifier semantics)".to_string());
    }
    lines
}

fn feature_state_label(state: perl_regex::analyzer::FeatureState) -> &'static str {
    match state {
        perl_regex::analyzer::FeatureState::Enabled => "enabled",
        perl_regex::analyzer::FeatureState::Disabled => "disabled",
        perl_regex::analyzer::FeatureState::Unknown => "unknown",
        _ => "unknown",
    }
}

fn escape_inline_code(text: &str) -> String {
    text.replace('`', "\\`")
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser::Parser;
    use perl_parser_core::RetainedRegexSession;
    use std::sync::Arc;

    fn parse_with_retention(source: &str) -> Arc<RegexAnalysisTable> {
        let session = RetainedRegexSession::begin(source);
        let mut parser = Parser::new(source);
        match parser.parse() {
            Ok(mut ast) => Arc::new(session.finish(Some(&mut ast))),
            Err(_) => Arc::new(session.finish(None)),
        }
    }

    #[test]
    fn xx_and_x_modifiers_render_differently() {
        let source = "my $a = m/foo/x; my $b = m/foo/xx;\n";
        let table = parse_with_retention(source);
        let x_hover = hover_projection_at(source, &table, source.find("m/foo/x").unwrap() + 3)
            .unwrap_or_default();
        let xx_hover = hover_projection_at(source, &table, source.find("m/foo/xx").unwrap() + 3)
            .unwrap_or_default();
        assert!(x_hover.contains("/x extended whitespace"), "{x_hover}");
        assert!(xx_hover.contains("/xx extra-extended"), "{xx_hover}");
        assert_ne!(x_hover, xx_hover);
    }

    #[test]
    fn named_capture_uses_plus_brace_access_syntax() {
        let source = "my $re = /(?<foo>bar)/;\n";
        let table = parse_with_retention(source);
        let offset = source.find("foo").unwrap();
        let hover = hover_projection_at(source, &table, offset).unwrap_or_default();
        assert!(hover.contains("$+{foo}"), "{hover}");
        assert!(!hover.contains("${foo}"), "{hover}");
    }

    #[test]
    fn replacement_hover_does_not_claim_regex_syntax() {
        let source = "my $x = $s =~ s/(a)/(b)/;\n";
        let table = parse_with_retention(source);
        let offset = source.find("(b)").unwrap() + 1;
        let hover = hover_projection_at(source, &table, offset).unwrap_or_default();
        assert!(hover.contains("not regex syntax"), "{hover}");
    }

    #[test]
    fn stale_table_fails_closed() {
        let source = "my $re = /a/;\n";
        let table = parse_with_retention(source);
        assert!(hover_projection_at("other", &table, 1).is_none());
    }
}
