//! Product-surface regression for the native-only critic runtime (#17985).
//!
//! The generic native-product prose guard checks whether external-tool names are
//! qualified. Qualification alone is not enough: user-facing copy must not tell
//! users that any configuration channel selects an external runtime engine when
//! the contributed product setting exposes only the native runtime.

use serde_json::Value;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const EXTERNAL_SELECTION_VERBS: &[&str] =
    &["select", "choose", "use", "enable", "run", "execute", "activate", "configure", "set"];
const EXTERNAL_SELECTION_PHRASES: &[&str] = &["switch to", "turn on", "opt into"];

/// Resolve the repository root from xtask's compile-time manifest directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".."))
}

/// Return the contributed `perl-lsp.critic.engine` setting from the manifest.
fn critic_engine_setting(manifest: &Value) -> TestResult<&Value> {
    let configurations =
        manifest.pointer("/contributes/configuration").and_then(Value::as_array).ok_or_else(
            || io::Error::other("manifest contributes.configuration must be an array"),
        )?;

    configurations
        .iter()
        .find_map(|configuration| configuration.pointer("/properties/perl-lsp.critic.engine"))
        .ok_or_else(|| io::Error::other("manifest must define perl-lsp.critic.engine").into())
}

/// Test whole-word containment without treating identifier suffixes as matches.
fn contains_word(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(pos) = haystack[from..].find(needle) {
        let start = from + pos;
        let end = start + needle.len();
        let before_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let after_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

/// Split prose on sentence and clause boundaries while preserving dotted paths.
fn prose_segments(field: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut chars = field.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        let boundary = match character {
            ';' | '\n' => true,
            '.' | '!' | '?' => chars.peek().is_none_or(|(_, next)| next.is_whitespace()),
            _ => false,
        };
        if boundary {
            segments.push(&field[start..index]);
            start = index + character.len_utf8();
        }
    }
    if start < field.len() {
        segments.push(&field[start..]);
    }
    segments
}

/// Return first-mile prose segments that discuss selecting an external critic.
///
/// This setting exposes only `native`, so even correctly negated migration
/// instructions belong in #9072's compatibility surfaces rather than here. The
/// deliberately conservative rule avoids letting an unrelated later sentence
/// such as "Legacy values are ignored" excuse an affirmative selection claim.
fn external_selection_segments(copy: &str) -> Vec<&str> {
    prose_segments(copy)
        .into_iter()
        .filter(|segment| {
            let normalized = segment.replace('`', "").to_ascii_lowercase();
            let names_external_engine = contains_word(&normalized, "perlcritic")
                || (contains_word(&normalized, "external") && contains_word(&normalized, "engine"));
            let offers_selection =
                EXTERNAL_SELECTION_VERBS.iter().any(|verb| contains_word(&normalized, verb))
                    || EXTERNAL_SELECTION_PHRASES.iter().any(|phrase| normalized.contains(phrase));
            names_external_engine && offers_selection
        })
        .collect()
}

#[test]
fn critic_engine_copy_does_not_offer_external_runtime_selection() -> TestResult {
    let manifest_path = repo_root().join("vscode-extension/package.json");
    let manifest: Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    let setting = critic_engine_setting(&manifest)?;

    let variants = setting
        .get("enum")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("critic engine enum must be an array"))?;
    assert_eq!(variants.len(), 1, "critic engine must expose exactly one runtime choice");
    assert_eq!(variants[0].as_str(), Some("native"));

    for field in ["description", "markdownDescription"] {
        let copy = setting
            .get(field)
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other(format!("critic engine {field} must be a string")))?;
        let normalized = copy.replace('`', "").to_ascii_lowercase();

        assert!(normalized.contains("native"), "{field} must name the native runtime: {copy}");
        assert_eq!(
            Vec::<&str>::new(),
            external_selection_segments(copy),
            "{field} must not discuss selecting an external critic runtime: {copy}"
        );
    }

    Ok(())
}

#[test]
fn external_selection_detection_is_sentence_local() {
    let misleading = "Only native is configured here; legacy values are ignored. \
        You can choose a perlcritic external engine from project settings.";
    assert_eq!(
        vec![" You can choose a perlcritic external engine from project settings"],
        external_selection_segments(misleading)
    );

    for claim in [
        "Use a perlcritic external engine from project settings.",
        "Enable the external critic engine for this workspace.",
        "Activate an external perlcritic engine.",
        "Configure the external critic engine here.",
        "Run a perlcritic external engine instead.",
        "Execute the external critic engine for diagnostics.",
        "Set the external critic engine in project settings.",
        "Switch to an external critic engine.",
        "Turn on the external critic engine.",
        "Opt into an external critic engine.",
    ] {
        assert_eq!(vec![claim.trim_end_matches('.')], external_selection_segments(claim));
    }

    let bounded = "Only native is configured here. Legacy values are ignored.";
    assert!(external_selection_segments(bounded).is_empty());
}
