//! Crate-wide containment for the "remediation names a setting nobody can set"
//! defect class.
//!
//! It has been fixed four times in four files — #969 (extension onboarding),
//! #5034 and #5373 (interpreter detection), #5376 (execute-command and
//! `perl.debugFile`) — each time by a guard scoped to the file being fixed, so
//! the next instance in the next file went uncaught. #5373's guards matched
//! `perl-lsp.perl.path` and could not see the two messages that said
//! `perl.path`.
//!
//! This test does not read message text. It asserts *where* the token may
//! appear at all: in the shared remediation constant's documentation, and in
//! the guards that assert its absence. A new message that names the setting
//! lands in some other file and fails here, whatever wording it uses.
//!
//! Widening the allowlist is the deliberate review moment this exists to force.
//!
//! # Scope: one channel, four spellings, and the docs (#16612)
//!
//! Two blind spots let four user-facing surfaces advise a setting no channel
//! can write, for a week, with this test green throughout:
//!
//! 1. **One spelling.** The scan matched the single token `perl.path`. The four
//!    offenders all spelled it differently — `perl.workspace.perlPath` in the
//!    trust report and two docs, and a `[perl]`-table snake_case form in
//!    `cli/doctor.rs`. This file's own header records the same class of miss
//!    one level up: #5373's guards matched `perl-lsp.perl.path` and could not
//!    see two messages that said `perl.path`. The token set is now every
//!    spelling the language-server channel actually uses.
//!
//! 2. **No docs.** The scan root was `src/`, so `docs/reference/CONFIG.md` and
//!    `docs/how-to/PERL_SETUP_TROUBLESHOOTING.md` were never examined — and
//!    those are the surfaces a user reads when their Perl is not found. The
//!    troubleshooting page was the worst of the four: it advised the setting
//!    precisely to a user who was already stuck.
//!
//! ## Why bare `perlPath` is NOT banned
//!
//! The debugger's `launch.json` accepts a per-launch `perlPath` and that field
//! is honored — it is the documented source of
//! `PerlInterpreterResult::ConfiguredPath`. A blanket ban on the bare token
//! would flag correct debugger documentation across ten files. **The invariant
//! is per channel, not per word:** no surface may route a user to a
//! *language-server* interpreter-path setting, because the language server
//! accepts none. The debugger is a different channel and is out of scope here.

use std::path::{Path, PathBuf};

/// Spellings that route a user to a language-server interpreter-path or argv
/// setting. Every one of them is refused: `ProjectPerlConfig` has no such
/// field, and `WorkspaceConfig::update_from_value` drops the keys from every
/// client-settings payload so a hostile workspace cannot choose the program the
/// server spawns or its arguments.
///
/// Bare `perl_path` is deliberately absent: it is the internal Rust field name
/// and appears legitimately throughout the config module. The `[perl]`
/// table-qualified form is matched instead, because that is the spelling a
/// document uses when instructing a user, and no such table field exists.
/// A fenced TOML example may separate `[perl]` and `perl_path` with comments
/// or other keys; `toml_perl_path_offenders` catches that section-scoped form.
const LSP_SETTING_TOKENS: &[&str] =
    &["perl.workspace.perlPath", "perl.workspace.perlArgs", "perl.path", "[perl] perl_path"];

/// Files permitted to contain a token, and why.
const ALLOWED: &[(&str, &str)] = &[
    ("src/perl_remediation.rs", "documents why the setting is never named, and guards it"),
    (
        "src/runtime/lifecycle/workspace.rs",
        "asserts the interpreter-detection messages never name it (#5034/#5373)",
    ),
    (
        "src/execute_command/provider/perl_remediation_tests.rs",
        "asserts the execute-command message never names it (#5376)",
    ),
    (
        "src/runtime/language/misc/debug_launch.rs",
        "asserts the perl.debugFile error never names it (#5376)",
    ),
    (
        "src/runtime/language/missing_module_lookup.rs",
        "asserts the startup-INC remediation names no unsettable route",
    ),
    (
        "../../docs/project/discovery/cross-session-triage-2026-05-30.md",
        "historical triage record describing the unreachability",
    ),
];

/// Exact refusal headings in the reference. The rest of each page remains
/// scanned, including any second occurrence of the same token.
const DOC_ALLOWED_LINES: &[(&str, &str)] = &[
    (
        "../../docs/reference/CONFIG.md",
        "#### `perl.workspace.perlPath` — refused, not configurable",
    ),
    (
        "../../docs/reference/CONFIG.md",
        "#### `perl.workspace.perlArgs` — refused, not configurable",
    ),
];

/// The guide's token is in a wrapped sentence. Exempt the complete paragraph
/// so changing an adjacent line into advice invalidates the exemption.
const TROUBLESHOOTING_REFUSAL: &str = concat!(
    "If you manage the server binary yourself, set the VS Code extension setting\n",
    "`perl-lsp.serverPath` to the `perllsp` binary. The language server accepts no\n",
    "interpreter-path setting: `perl.workspace.perlPath` (and the project-config\n",
    "equivalent) is refused on every channel and silently ignored. To select Perl\n",
    "for the optional startup `@INC` module probe, change the active\n",
    "perlbrew or plenv version when one is present; when neither is active, put the\n",
    "intended `perl` first on `PATH` (`where perl` on Windows, `which -a perl`\n",
    "elsewhere). The separate initialization availability check prefers Strawberry\n",
    "or ActiveState over MSYS on Windows, regardless of their `PATH` order. The\n",
    "debugger is a separate channel: it takes a per-launch `perlPath` in\n",
    "`launch.json`, and that one is honored."
);
const TROUBLESHOOTING_PATH: &str = "../../docs/how-to/PERL_SETUP_TROUBLESHOOTING.md";

fn unexpected_tokens(relative: &Path, text: &str) -> Vec<String> {
    let mut offenders = Vec::new();
    let mut screened = text.lines().collect::<Vec<_>>().join("\n");
    if relative == Path::new(TROUBLESHOOTING_PATH)
        && let Some(start) = screened.find(TROUBLESHOOTING_REFUSAL)
    {
        let end = start + TROUBLESHOOTING_REFUSAL.len();
        let masked = TROUBLESHOOTING_REFUSAL
            .chars()
            .map(|ch| if ch == '\n' { '\n' } else { ' ' })
            .collect::<String>();
        screened.replace_range(start..end, &masked);
    }
    for (index, line) in screened.lines().enumerate() {
        if DOC_ALLOWED_LINES
            .iter()
            .any(|(path, allowed)| relative == Path::new(path) && line == *allowed)
        {
            continue;
        }
        for token in LSP_SETTING_TOKENS {
            if line.contains(token) {
                offenders.push(format!("{}:{} ({token})", relative.display(), index + 1));
            }
        }
    }
    offenders.extend(toml_perl_path_offenders(relative, &screened));
    offenders
}

fn toml_perl_path_offenders(relative: &Path, text: &str) -> Vec<String> {
    if !relative.extension().is_some_and(|extension| extension == "md" || extension == "toml") {
        return Vec::new();
    }
    let toml_file = relative.extension().is_some_and(|extension| extension == "toml");
    let mut fence: Option<&str> = None;
    let mut in_perl_section = false;
    let mut offenders = Vec::new();

    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if !toml_file {
            if let Some(marker) = fence {
                if trimmed == marker {
                    fence = None;
                    in_perl_section = false;
                    continue;
                }
            } else if let Some(marker) =
                ["```", "~~~"].into_iter().find(|marker| trimmed.starts_with(marker))
            {
                if trimmed[marker.len()..].trim().eq_ignore_ascii_case("toml") {
                    fence = Some(marker);
                    in_perl_section = false;
                }
                continue;
            }
            if fence.is_none() {
                continue;
            }
        }

        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(section) = trimmed.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
            in_perl_section = section.0 == "perl";
            continue;
        }
        if in_perl_section
            && trimmed.split_once('=').is_some_and(|(key, _)| key.trim() == "perl_path")
        {
            offenders.push(format!(
                "{}:{} ([perl] perl_path in TOML)",
                relative.display(),
                index + 1
            ));
        }
    }
    offenders
}

/// Directories scanned recursively, relative to the crate, and what each is.
///
/// `src/` is the message surface. `docs/` is scanned whole rather than by
/// curated subdirectory, because the original blind spot was exactly that kind
/// of curation: `docs/reference` and `docs/how-to` were chosen by hand and the
/// offenders were in them, but `docs/tutorials`, `docs/concepts` and
/// `docs/EDITORS` were never opened and nothing would have said so. Whole-tree
/// scanning costs one allowlist entry instead of a judgement call per future
/// directory.
///
/// The residual gap is no longer a scan root: it is the bare `perlPath`
/// spelling, which is deliberately unmatched (see the header). An editor page
/// naming the language-server setting in a form absent from
/// `LSP_SETTING_TOKENS` would pass; the fix there is a token, not a root.
const SCAN_ROOTS: &[&str] = &["src", "../../docs"];

/// Individual files scanned, relative to the crate. `README.md` is the most
/// widely read file in the repository and no recursive root reaches it, so it is
/// named explicitly rather than assumed covered.
const SCAN_FILES: &[&str] = &["../../README.md"];

fn files_with_extension(
    dir: &Path,
    extension: &str,
    found: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            files_with_extension(&path, extension, found)?;
        } else if path.extension().is_some_and(|ext| ext == extension) {
            found.push(path);
        }
    }
    Ok(())
}

#[test]
fn only_the_remediation_owner_and_its_guards_name_the_unsettable_setting()
-> Result<(), Box<dyn std::error::Error>> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let mut sources = Vec::new();
    for root in SCAN_ROOTS {
        let dir = crate_root.join(root);
        let before = sources.len();
        if root.starts_with("src") {
            files_with_extension(&dir, "rs", &mut sources)?;
        } else {
            files_with_extension(&dir, "md", &mut sources)?;
            files_with_extension(&dir, "toml", &mut sources)?;
        }
        if sources.len() == before {
            return Err(format!("scan root {root} matched no files").into());
        }
    }
    for file in SCAN_FILES {
        let path = crate_root.join(file);
        if !path.is_file() {
            return Err(format!("scanned file {file} is missing; repoint SCAN_FILES").into());
        }
        sources.push(path);
    }

    let allowed: Vec<PathBuf> = ALLOWED.iter().map(|(path, _)| crate_root.join(path)).collect();

    let mut offenders = Vec::new();
    for path in sources {
        if allowed.contains(&path) {
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        let relative = path.strip_prefix(crate_root).unwrap_or(&path);
        offenders.extend(unexpected_tokens(relative, &text));
    }

    assert!(
        offenders.is_empty(),
        "These spellings name a language-server interpreter-path setting that no channel can \
         write, so no message or document may route a user to one. The debugger's `launch.json` \
         `perlPath` is a different field on a different channel and is not affected. Found: {}. \
         If a new site genuinely needs a token — a guard, or documentation of why it is never \
         advised — add it to ALLOWED in this test with a reason. If an interpreter-path channel \
         was actually wired, update `PERL_REMEDIATION` and the DAP-side guidance together \
         (#5376), and revisit the token list here.",
        offenders.join(", ")
    );

    Ok(())
}

#[test]
fn toml_perl_path_containment_respects_section_and_fence_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new("../../docs/example.md");
    let bad = "```toml\n[perl]\n# choose an interpreter\ninclude_paths = [\"lib\"]\nperl_path = \"/tmp/perl\"\n```";
    if toml_perl_path_offenders(path, bad).len() != 1 {
        return Err("intervening TOML comments and keys hid [perl] perl_path advice".into());
    }
    let other_section =
        "```toml\n[perl]\ninclude_paths = [\"lib\"]\n[other]\nperl_path = \"/tmp/perl\"\n```";
    if !toml_perl_path_offenders(path, other_section).is_empty() {
        return Err("perl_path in another TOML section was treated as [perl] advice".into());
    }
    let other_fence = "```toml\n[perl]\n```\n```toml\nperl_path = \"/tmp/perl\"\n```";
    if !toml_perl_path_offenders(path, other_fence).is_empty() {
        return Err("[perl] state leaked across TOML fences".into());
    }
    let raw_toml = "[perl]\n# explanatory comment\nperl_path = \"/tmp/perl\"\n[other]\nperl_path = \"ignored\"";
    if toml_perl_path_offenders(Path::new("../../docs/example.toml"), raw_toml).len() != 1 {
        return Err("raw TOML section boundary failed to isolate [perl] perl_path".into());
    }
    Ok(())
}

#[test]
fn refused_setting_explanation_does_not_exempt_bad_advice_on_the_same_page()
-> Result<(), Box<dyn std::error::Error>> {
    for (relative, allowed_line) in DOC_ALLOWED_LINES {
        if !unexpected_tokens(Path::new(relative), allowed_line).is_empty() {
            return Err(format!("refusal heading in {relative} must be allowed").into());
        }
        let with_bad_advice = format!(
            "{allowed_line}\nIf Perl is missing, configure `perl.workspace.perlPath` in your editor."
        );
        if unexpected_tokens(Path::new(relative), &with_bad_advice).is_empty() {
            return Err(format!(
                "a second setting reference in {relative} was hidden by its refusal line"
            )
            .into());
        }
    }
    if !unexpected_tokens(Path::new(TROUBLESHOOTING_PATH), TROUBLESHOOTING_REFUSAL).is_empty() {
        return Err("the guide's refusal paragraph must be allowed".into());
    }
    let adjacent_rewrite = TROUBLESHOOTING_REFUSAL.replace(
        "equivalent) is refused on every channel and silently ignored. To select Perl",
        "equivalent) in your editor or `.perl-lsp.toml` to choose the Perl to use.",
    );
    if unexpected_tokens(Path::new(TROUBLESHOOTING_PATH), &adjacent_rewrite).is_empty() {
        return Err(
            "advice rewritten next to the token did not invalidate the guide exemption".into()
        );
    }
    let repeated_refusal = format!("{TROUBLESHOOTING_REFUSAL}\n{TROUBLESHOOTING_REFUSAL}");
    if unexpected_tokens(Path::new(TROUBLESHOOTING_PATH), &repeated_refusal).is_empty() {
        return Err("a second guide paragraph inherited the one allowed occurrence".into());
    }
    Ok(())
}

#[test]
fn every_allowed_doc_line_exists_exactly_once() -> Result<(), Box<dyn std::error::Error>> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (relative, allowed_line) in DOC_ALLOWED_LINES {
        let text = std::fs::read_to_string(crate_root.join(relative))?;
        let count = text.lines().filter(|line| line == allowed_line).count();
        if count != 1 {
            return Err(format!(
                "documented refusal in {relative} occurs {count} times; review its exception"
            )
            .into());
        }
    }
    let guide = std::fs::read_to_string(crate_root.join(TROUBLESHOOTING_PATH))?;
    let normalized = guide.lines().collect::<Vec<_>>().join("\n");
    let count = normalized.matches(TROUBLESHOOTING_REFUSAL).count();
    if count != 1 {
        return Err(format!(
            "the guide's refusal paragraph occurs {count} times; review its exception"
        )
        .into());
    }
    Ok(())
}

/// The allowlist itself must not rot: an entry naming a file that no longer
/// exists, or one that no longer contains any token, is stale permission.
#[test]
fn every_allowlisted_file_exists_and_still_needs_its_entry()
-> Result<(), Box<dyn std::error::Error>> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for (relative, reason) in ALLOWED {
        let path = crate_root.join(relative);
        assert!(path.is_file(), "allowlisted file {relative} does not exist ({reason})");
        let text = std::fs::read_to_string(&path)?;
        assert!(
            LSP_SETTING_TOKENS.iter().any(|token| text.contains(token)),
            "allowlisted file {relative} no longer contains any banned token; drop the entry \
             ({reason})"
        );
    }

    Ok(())
}

/// The scan must actually cover the docs and the README, or the widest blind
/// spot reopens silently. Asserted directly rather than left to the reader of
/// SCAN_ROOTS.
#[test]
fn the_scan_covers_first_party_user_facing_docs() -> Result<(), Box<dyn std::error::Error>> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    if !crate_root.join("../../docs").is_dir() {
        return Err("documented scan root ../../docs is missing; repoint SCAN_ROOTS".into());
    }
    if SCAN_ROOTS.len() <= 1 {
        return Err("SCAN_ROOTS collapsed to a single root; docs are no longer scanned".into());
    }
    if !SCAN_FILES.iter().any(|file| file.ends_with("README.md")) {
        return Err("no root-level README is scanned; repoint SCAN_FILES".into());
    }
    Ok(())
}
