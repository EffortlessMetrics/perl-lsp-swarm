#![deny(clippy::map_err_ignore)]

use perl_regex::analyzer::{
    FeatureState, ModifierSequence, PerlVersion, RegexLanguageProfile, RegexOperator,
};
use perl_regex::validator::RegexDiagnosticCode;
use perl_regex::{RegexAnalyzer, RegexValidator};

fn effective_xx(
    profile: RegexLanguageProfile,
) -> Result<perl_regex::analyzer::EffectiveModifiers, Box<dyn std::error::Error>> {
    let sequence = ModifierSequence::new("xx", 0).ok_or("modifier sequence")?;
    Ok(RegexAnalyzer::analyze_modifiers(RegexOperator::Match, sequence, profile).effective)
}

fn profile_544(enhanced_xx: FeatureState) -> RegexLanguageProfile {
    RegexLanguageProfile::new(Some(PerlVersion::new(5, 44)), enhanced_xx)
}

#[test]
fn enhanced_xx_warns_on_unspaced_comment_introducer() -> Result<(), Box<dyn std::error::Error>> {
    let modifiers = effective_xx(profile_544(FeatureState::Enabled))?;
    let pattern = "[#oops]";
    let analysis = RegexValidator::new().analyze_with_modifiers(pattern, modifiers);
    let hash_offset = pattern.find('#').ok_or("hash")?;
    assert!(
        analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == RegexDiagnosticCode::EnhancedXxUnspacedCommentIntroducer
                && diagnostic.range.start == hash_offset
        }),
        "expected unspaced `#` warning at the octothorpe"
    );
    Ok(())
}

#[test]
fn ordinary_xx_without_enhanced_feature_emits_no_enhanced_class_warnings()
-> Result<(), Box<dyn std::error::Error>> {
    let modifiers = effective_xx(profile_544(FeatureState::Disabled))?;
    let pattern = "[#still literal]";
    let analysis = RegexValidator::new().analyze_with_modifiers(pattern, modifiers);
    assert!(
        analysis.diagnostics.iter().all(|diagnostic| {
            !matches!(
                diagnostic.code,
                RegexDiagnosticCode::EnhancedXxUnspacedCommentIntroducer
                    | RegexDiagnosticCode::EnhancedXxHashInsideComment
                    | RegexDiagnosticCode::EnhancedXxBracketInsideComment
            )
        }),
        "ordinary /xx must not apply enhanced_xx class warnings"
    );
    Ok(())
}

#[test]
fn unknown_enhanced_profile_withholds_enhanced_class_semantics()
-> Result<(), Box<dyn std::error::Error>> {
    let modifiers = effective_xx(profile_544(FeatureState::Unknown))?;
    let pattern = "[#not a comment]";
    let analysis = RegexValidator::new().analyze_with_modifiers(pattern, modifiers);
    assert!(
        analysis.diagnostics.iter().all(|diagnostic| {
            !matches!(
                diagnostic.code,
                RegexDiagnosticCode::EnhancedXxUnspacedCommentIntroducer
                    | RegexDiagnosticCode::EnhancedXxHashInsideComment
                    | RegexDiagnosticCode::EnhancedXxBracketInsideComment
            )
        }),
        "unknown enhanced_xx must not emit enhanced-class warnings"
    );
    Ok(())
}

#[test]
fn enhanced_xx_emits_bracket_inside_comment_warning() -> Result<(), Box<dyn std::error::Error>> {
    let modifiers = effective_xx(profile_544(FeatureState::Enabled))?;
    let pattern = "[a # note ] oops\n]";
    let analysis = RegexValidator::new().analyze_with_modifiers(pattern, modifiers);
    let bracket_offset = pattern.find(']').ok_or("first bracket in comment")?;
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == RegexDiagnosticCode::EnhancedXxBracketInsideComment
            && diagnostic.range.start == bracket_offset
    }));
    Ok(())
}

#[test]
fn enhanced_xx_emits_hash_inside_comment_warning() -> Result<(), Box<dyn std::error::Error>> {
    let modifiers = effective_xx(profile_544(FeatureState::Enabled))?;
    let pattern = "[a # one # two\n]";
    let analysis = RegexValidator::new().analyze_with_modifiers(pattern, modifiers);
    let first_hash = pattern.find('#').ok_or("hash")?;
    let second_hash = pattern[first_hash + 1..].find('#').ok_or("second hash")? + first_hash + 1;
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == RegexDiagnosticCode::EnhancedXxHashInsideComment
            && diagnostic.range.start == second_hash
    }));
    Ok(())
}

#[test]
fn enhanced_xx_spaced_comment_introducer_avoids_unspaced_warning()
-> Result<(), Box<dyn std::error::Error>> {
    let modifiers = effective_xx(profile_544(FeatureState::Enabled))?;
    let pattern = "[a # spaced comment\nb]";
    let analysis = RegexValidator::new().analyze_with_modifiers(pattern, modifiers);
    assert!(
        analysis.diagnostics.iter().all(|diagnostic| {
            diagnostic.code != RegexDiagnosticCode::EnhancedXxUnspacedCommentIntroducer
        }),
        "spaced `#` comment introducer must not raise the unspaced warning"
    );
    Ok(())
}
