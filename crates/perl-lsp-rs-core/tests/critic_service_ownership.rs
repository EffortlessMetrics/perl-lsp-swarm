//! Sole-owner inventory gate for the native critic service (#9062).
//!
//! After the #9062 cutover, exactly one production site may compose the
//! native critic pipeline (registry construction, candidate collection,
//! post-merge policy, canonical normalization): the service itself, plus the
//! settled #7475 seam that defines it and the registry implementation it
//! calls through. Every diagnostic/action transport must consume
//! [`NativeCriticService::analyze`] instead.
//!
//! This gate walks the production source trees of both owning packages and
//! fails closed if any composition entry point appears outside the
//! allowlist. A restored consumer-side pipeline — two paths snapshotting
//! configuration at different times, re-running semantic work to recover
//! findings, or flattening metadata differently — turns this red instead of
//! silently reintroducing the split the issue closed.

use std::fs;
use std::path::{Path, PathBuf};

/// Composition entry points reserved to the service and the seam modules.
///
/// `BuiltInAnalyzer::new(` is included deliberately: the legacy analyzer is a
/// second critic evaluator that runs outside the service, its accepted-state
/// and currentness gates, canonical normalization, and work receipt. After the
/// #9062 cutover no diagnostic or action transport may reach it; only the
/// #6969 command adapter is still allowed to, and that allowance is temporary.
const SERVICE_ONLY_COMPOSITION: [&str; 7] = [
    "native_finding_candidates(",
    "normalize_with_native_policy(",
    "NativeCriticPolicy::new(",
    "for_profile_with_config(",
    ".check_unfiltered(",
    "built_in_observation_candidates(",
    "BuiltInAnalyzer::new(",
];

/// Files allowed to contain composition entry points, with the reason each
/// allowance exists. Paths are relative to the workspace `crates/` directory
/// using forward slashes so the table reads identically on every platform.
const ALLOWED_SITES: [(&str, &str); 4] = [
    (
        "perl-lsp-rs-core/src/tooling/perl_critic/service.rs",
        "#9062: the one protocol-neutral service",
    ),
    (
        "perl-lsp-rs-core/src/tooling/perl_critic/semantic.rs",
        "#7475: the settled normalization/policy seam the service composes",
    ),
    (
        "perl-lsp-rs-core/src/tooling/perl_critic/native/native_registry.rs",
        "the registry implementation itself (definition and internal check())",
    ),
    (
        "perl-lsp-rs/src/execute_command/provider.rs",
        "#6969 pending: the perl.runCritic command adapter cuts over separately",
    ),
];

/// Walk `dir` for Rust sources, reporting the exact unreadable path instead of
/// unwinding. An instrument failure must stay distinguishable from an ownership
/// violation, so it surfaces as a contextual error rather than a bare panic.
fn collect_rust_sources(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir)
        .map_err(|error| format!("source directory {} must be readable: {error}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, out)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// Strip each `#[cfg(test)]`-gated item so inline test modules do not count
/// as production call sites.
///
/// Only the gated items themselves are removed, never the remainder of the
/// file: Rust permits production items after a test module, and a production
/// composition pipeline placed there must stay visible to this gate. A
/// `#[cfg(test)]` attribute followed by a brace-delimited item (`mod`,
/// `fn`, `impl`, …) is stripped through its balanced closing brace; one
/// attached to a semicolon-terminated item (`use`, extern crate) strips
/// through that statement's `;`.
///
/// Brace balance is delimiter-aware (review #12067): line comments, nestable
/// block comments, regular/raw/byte strings, and char literals contribute no
/// depth, so a literal `{` inside a gated item cannot leak one level of depth
/// into the walk and swallow following production items out of the audited
/// surface. Apostrophes are treated as lifetime markers unless they close a
/// well-formed char-literal shape (`'x'`, `'\n'`, `'\''`, `'{'`, multibyte).
fn production_portion(source: &str) -> String {
    const ATTR: &str = "#[cfg(test)]";
    let mut kept = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(index) = rest.find(ATTR) {
        kept.push_str(&rest[..index]);
        let after = &rest[index + ATTR.len()..];
        let bytes = after.as_bytes();
        let mut cursor = 0;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        // First significant `{` or `;` decides whether the gated item is
        // brace-delimited or statement-terminated.
        while cursor < bytes.len() && bytes[cursor] != b'{' && bytes[cursor] != b';' {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            rest = "";
            break;
        }
        if bytes[cursor] == b';' {
            rest = &after[cursor + 1..];
            continue;
        }
        match closing_brace(bytes, cursor) {
            Some(end) => rest = &after[end..],
            None => {
                // Unterminated item: conservatively drop nothing further.
                rest = "";
                break;
            }
        }
    }
    kept.push_str(rest);
    kept
}

/// Byte offset just past the `}` balancing the opening `{` at `open`.
///
/// Delimiter-aware: `//` line comments, nestable `/* */` block comments, and
/// the bodies of string/char literals are consumed without contributing
/// depth. Returns `None` when the braces never rebalance.
fn closing_brace(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let mut nesting = 1usize;
                i += 2;
                while i < bytes.len() && nesting > 0 {
                    match (bytes[i], bytes.get(i + 1)) {
                        (b'*', Some(b'/')) => {
                            nesting -= 1;
                            i += 2;
                        }
                        (b'/', Some(b'*')) => {
                            nesting += 1;
                            i += 2;
                        }
                        _ => i += 1,
                    }
                }
            }
            b'"' => i = skip_regular_string(bytes, i),
            b'\'' => i = skip_quote_or_lifetime(bytes, i),
            b'r' => {
                if let Some((hashes, quote)) = raw_string_hashes(bytes, i) {
                    i = skip_raw_string(bytes, quote, hashes);
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    None
}

/// Number of `#` in a raw-string introducer starting at `start` (`r"`, `r#"`,
/// `r##"`), with the byte offset of its opening `"`. `None` when `start` does
/// not begin a raw string (plain identifier such as `return` or `r#type`).
fn raw_string_hashes(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut hashes = 0usize;
    let mut j = start + 1;
    while j < bytes.len() && bytes[j] == b'#' {
        hashes += 1;
        j += 1;
    }
    if j < bytes.len() && bytes[j] == b'"' { Some((hashes, j)) } else { None }
}

/// Byte offset past the closing quote of the regular string whose `"` sits at
/// `open`; backslash escapes (including `\"`) do not terminate it.
fn skip_regular_string(bytes: &[u8], open: usize) -> usize {
    let mut j = open + 1;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' => j += 2,
            b'"' => return j + 1,
            _ => j += 1,
        }
    }
    j
}

/// Byte offset past a raw string's terminator (`"` followed by as many `#` as
/// its introducer carried); raw strings have no escapes.
fn skip_raw_string(bytes: &[u8], open: usize, hashes: usize) -> usize {
    let mut j = open + 1;
    while j < bytes.len() {
        if bytes[j] == b'"' {
            let mut k = j + 1;
            let mut seen = 0usize;
            while k < bytes.len() && bytes[k] == b'#' && seen < hashes {
                seen += 1;
                k += 1;
            }
            if seen == hashes {
                return k;
            }
        }
        j += 1;
    }
    j
}

/// Byte offset past the apostrophe at `start`, whichever construct it opens:
/// an escaped char literal (`'\n'`, `'\''`, `'\u{7}'`), a simple or multibyte
/// char literal (`'x'`, `'{'`, `'é'`), or a lifetime/loop label whose
/// following identifier characters are consumed verbatim.
fn skip_quote_or_lifetime(bytes: &[u8], start: usize) -> usize {
    let len = bytes.len();
    let Some(&first) = bytes.get(start + 1) else {
        return len;
    };
    if first == b'\\' {
        // Escape form: consume through the next unescaped closing quote.
        let mut j = start + 2;
        while j < len {
            match bytes[j] {
                b'\\' => j += 2,
                b'\'' => return j + 1,
                _ => j += 1,
            }
        }
        return j.min(len);
    }
    if !first.is_ascii_alphanumeric() && first != b'_' && first != b'\'' && first != b'\n' {
        // Simple single-unit literal. Multibyte content is measured by its
        // UTF-8 lead byte so `'é'` closes at the right quote.
        let width = if first >= 0xF0 {
            4
        } else if first >= 0xE0 {
            3
        } else if first >= 0xC0 {
            2
        } else {
            1
        };
        if bytes.get(start + 1 + width) == Some(&b'\'') {
            return start + 2 + width;
        }
    }
    // No well-formed literal shape: lifetime or label — consume its name.
    let mut j = start + 1;
    while j < len && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
        j += 1;
    }
    j.max(start + 1)
}

#[test]
fn test_only_items_are_stripped_but_later_production_stays_scanned() {
    let source = concat!(
        "fn ordinary() {}\n",
        "#[cfg(test)]\nmod tests {\n",
        "    fn hidden() { normalize_with_native_policy(unreachable()); }\n",
        "}\n",
        "#[cfg(test)]\nfn helper() { native_finding_candidates(x); }\n",
        "// production composition placed after the test module (#9062 review)\n",
        "fn pipeline_after_tests() { NativeCriticPolicy::new(a, b, c, d); }\n",
    );

    let production = production_portion(source);

    assert!(
        !production.contains("normalize_with_native_policy("),
        "composition inside a test module stays excluded"
    );
    assert!(
        !production.contains("native_finding_candidates("),
        "multiple gated items are each stripped"
    );
    assert!(
        production.contains("NativeCriticPolicy::new("),
        "a production composition call after a test module must be visible to the gate"
    );
}

#[test]
fn gated_use_statements_strip_without_swallowing_following_items() {
    let source = concat!(
        "#[cfg(test)]\n",
        "use super::CriticConfig;\n",
        "fn production_fn() { .check_unfiltered(ctx); }\n",
    );

    let production = production_portion(source);

    assert!(!production.contains("use super::CriticConfig"), "gated use strips");
    assert!(
        production.contains(".check_unfiltered("),
        "production items after a gated statement stay scanned"
    );
}

#[test]
fn brace_inside_string_literal_cannot_swallow_later_production_items() {
    // Exact falsifier from review #12067: the gated test item holds a literal
    // `{` in a string. The delimiter-aware walk must not leak that brace into
    // the depth count; a byte-blind counter would end one level too deep,
    // swallow every following production item into the stripped span, and
    // hide a forbidden composition call placed after this module.
    let source = concat!(
        "#[cfg(test)]\nmod tests {\n",
        "    const OPENER: &str = \"{\";\n",
        "    fn hidden() { normalize_with_native_policy(unreachable()); }\n",
        "}\n",
        "// production composition deliberately placed after the test module\n",
        "fn pipeline_after_tests() { NativeCriticPolicy::new(a, b, c, d); }\n",
    );

    let production = production_portion(source);

    assert!(
        !production.contains("normalize_with_native_policy("),
        "the genuinely gated item stays excluded"
    );
    assert!(
        production.contains("NativeCriticPolicy::new("),
        "an unmatched brace inside a string literal must not swallow later \
         production items out of the audited surface"
    );
}

#[test]
fn raw_strings_comments_and_char_literals_do_not_skew_the_strip_span() {
    // Adjacent delimiter families the scanner must distinguish inside one
    // gated module: a nested block comment, a raw string containing braces
    // and quotes, brace char literals, and a lifetime apostrophe.
    let source = concat!(
        "#[cfg(test)]\nmod tests {\n",
        "    /* /* { */ still comment } */\n",
        r##"    const RAW: &str = r#"{ not a real brace }"#;"##,
        "\n    const OPEN: char = '{';\n",
        "    const CLOSE: char = '}';\n",
        "    fn hidden<'a>(x: &'a str) { normalize_with_native_policy(unreachable()); }\n",
        "}\n",
        "// production composition deliberately placed after the test module\n",
        "fn pipeline_after_tests() { built_in_observation_candidates(x, y); }\n",
    );

    let production = production_portion(source);

    assert!(
        !production.contains("normalize_with_native_policy("),
        "the genuinely gated item stays excluded despite exotic delimiters"
    );
    assert!(
        production.contains("built_in_observation_candidates("),
        "the production composition after the gated module stays scanned"
    );
}

#[test]
fn the_native_critic_pipeline_is_composed_only_by_its_service() -> Result<(), String> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let mut sources = Vec::new();
    let base = Path::new(manifest_dir);
    let crates_dir = base
        .parent()
        .ok_or_else(|| format!("manifest dir {manifest_dir} must have a parent"))?
        .to_path_buf();
    for crate_src in [base.join("src"), crates_dir.join("perl-lsp-rs").join("src")] {
        assert!(
            crate_src.is_dir(),
            "owning package source tree {} must exist",
            crate_src.display()
        );
        collect_rust_sources(&crate_src, &mut sources)?;
    }
    assert!(
        sources.len() > 100,
        "the inventory must scan a real source tree; found only {} files",
        sources.len()
    );

    let mut violations = Vec::new();
    for path in &sources {
        let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        // Test-only module files are not production call sites.
        if file_name == "tests.rs" || file_name.starts_with("test_") {
            continue;
        }

        // Workspace-relative path: everything from the `crates` component on.
        let mut seen_crates = false;
        let mut parts: Vec<&std::ffi::OsStr> = Vec::new();
        for component in path.components() {
            let raw = component.as_os_str();
            if seen_crates {
                parts.push(raw);
            } else if raw == "crates" {
                seen_crates = true;
                parts.push(raw);
            }
        }
        let Some(relative_path) =
            parts.split_first().map(|(_, tail)| tail.iter().collect::<PathBuf>())
        else {
            violations.push(format!(
                "{}: source file outside both owning crates cannot happen",
                path.display()
            ));
            continue;
        };
        let relative = relative_path.to_string_lossy().replace('\\', "/");

        let source = fs::read_to_string(path).map_err(|error| {
            format!("production source {} must be readable: {error}", path.display())
        })?;
        let production = production_portion(&source);
        for token in SERVICE_ONLY_COMPOSITION {
            if production.contains(token) {
                let allowance = ALLOWED_SITES.iter().find(|(allowed, _)| *allowed == relative);
                if allowance.is_none() {
                    violations.push(format!(
                        "{relative} composes `{token}` outside the native critic service (#9062)"
                    ));
                }
            }
        }
    }

    // The allowlist itself must stay honest: every entry still exists, so a
    // moved/renamed file cannot silently keep covering composition sites.
    for (allowed, reason) in ALLOWED_SITES {
        let absolute = crates_dir.join(allowed);
        assert!(absolute.is_file(), "allowlisted site {allowed} ({reason}) must exist");
    }

    assert!(
        violations.is_empty(),
        "native critic composition ownership violated:\n{}",
        violations.join("\n")
    );
    Ok(())
}
