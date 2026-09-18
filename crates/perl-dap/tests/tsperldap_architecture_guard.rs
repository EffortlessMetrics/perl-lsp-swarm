//! Guards the active DAP architecture against revival of the superseded
//! `Devel::TSPerlDAP` bundle/shim plan.
//!
//! The archived design is allowed to be *discussed* on active surfaces — a
//! reader who meets `Devel::TSPerlDAP` in Git history or an old issue needs a
//! current page that says it is gone. What is not allowed is prescribing it:
//! telling a human or an agent to install, bundle, run, or plan around the
//! shim. These tests draw that line by section, so a supersession note passes
//! and an implementation directive fails.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(std::fs::read_to_string(repo_root().join(path))?)
}

/// Active documentation that describes the *current* product. Archive pages are
/// deliberately absent: `docs/archive/` is where the old plan is allowed to
/// live in full.
const ACTIVE_SURFACES: &[&str] = &[
    "docs/reference/CRATE_ARCHITECTURE_DAP.md",
    "book/src/architecture/dap-implementation.md",
    "book/src/architecture/lsp-implementation.md",
    "book/src/dap/implementation.md",
    "book/src/dap/security.md",
    "book/src/lsp/implementation-guide.md",
    "docs/tutorials/DAP_USER_GUIDE.md",
    "crates/perl-dap/README.md",
    "crates/perl-dap/CLAUDE.md",
];

/// Names that only ever refer to the superseded shim.
const LEGACY_SHIM_TOKENS: &[&str] =
    &["devel::tsperldap", "devel-tsperldap", "devel/tsperldap", "tsperldap.pm"];

/// Strings that are an active directive whatever prose surrounds them. Matched
/// against normalized text, so emphasis and code formatting cannot smuggle one
/// past the guard.
const OBSOLETE_DIRECTIVES: &[&str] = &[
    "--install-shim",
    "resources/perl-shim/",
    "cpan perl shim",
    "bundled fallback shim",
    "cpanm devel::tsperldap",
    "cpan devel::tsperldap",
    "install devel::tsperldap",
    "bundle devel::tsperldap",
];

/// Vocabulary that marks a section as an explicit supersession, prohibition, or
/// archive reference rather than current implementation guidance.
const SUPERSESSION_MARKERS: &[&str] = &[
    "supersed",
    "archiv",
    "historical",
    "prohibited",
    "must reject",
    "must not",
    "not ship",
    "no longer",
    "removed",
];

/// Line prefixes that make a line a runnable instruction rather than prose.
const COMMAND_PREFIXES: &[&str] =
    &["$ ", "./", "cpan ", "cpanm ", "sudo ", "perl -m", "cargo run", "> "];

/// Collapse markdown emphasis, code ticks, and case so the guard matches on
/// meaning instead of formatting. Without this, `` `Devel::TSPerlDAP` `` and
/// `**Devel::TSPerlDAP**` are three different strings and a directive can be
/// reintroduced simply by restyling it.
fn normalize(text: &str) -> String {
    let stripped: String = text.chars().filter(|c| !matches!(c, '*' | '`' | '_' | '#')).collect();
    stripped.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split a markdown document into `(heading, body)` sections. The leading
/// preamble is returned under an empty heading so it is checked too.
fn sections(content: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![(String::new(), String::new())];
    for line in content.lines() {
        if line.starts_with('#') {
            out.push((line.to_string(), String::new()));
        } else {
            let current = out.len() - 1;
            out[current].1.push_str(line);
            out[current].1.push('\n');
        }
    }
    out
}

#[test]
fn active_dap_surfaces_do_not_prescribe_the_old_shim_architecture()
-> Result<(), Box<dyn std::error::Error>> {
    for path in ACTIVE_SURFACES {
        let content = normalize(&read(path)?);
        for directive in OBSOLETE_DIRECTIVES {
            assert!(
                !content.contains(directive),
                "active DAP surface {path} revives superseded shim directive {directive:?}"
            );
        }
    }

    Ok(())
}

/// A legacy shim name may appear on an active surface only inside a section
/// that says, in the section itself, that the design is gone. This is the check
/// that distinguishes a reviewed archive reference from live architecture: an
/// architecture diagram or roadmap that routes through the shim carries no
/// supersession marker and fails here.
#[test]
fn legacy_shim_names_appear_only_in_supersession_context() -> Result<(), Box<dyn std::error::Error>>
{
    for path in ACTIVE_SURFACES {
        let content = read(path)?;
        for (heading, body) in sections(&content) {
            let section = normalize(&format!("{heading}\n{body}"));
            if !LEGACY_SHIM_TOKENS.iter().any(|token| section.contains(token)) {
                continue;
            }
            assert!(
                SUPERSESSION_MARKERS.iter().any(|marker| section.contains(marker)),
                "active DAP surface {path} names the superseded shim under heading \
                 {heading:?} without marking it superseded, archived, or prohibited; \
                 either add that context or remove the reference"
            );
        }
    }

    Ok(())
}

/// Prose about the archived shim is fine; a command a reader can paste is not.
#[test]
fn active_dap_surfaces_carry_no_runnable_shim_commands() -> Result<(), Box<dyn std::error::Error>> {
    for path in ACTIVE_SURFACES {
        let content = read(path)?;
        for line in content.lines() {
            let normalized = normalize(line);
            if !LEGACY_SHIM_TOKENS.iter().any(|token| normalized.contains(token)) {
                continue;
            }
            assert!(
                !COMMAND_PREFIXES.iter().any(|prefix| normalized.starts_with(prefix)),
                "active DAP surface {path} carries a runnable shim command: {line:?}"
            );
        }
    }

    Ok(())
}

#[test]
fn current_architecture_and_archive_state_the_transition_explicitly()
-> Result<(), Box<dyn std::error::Error>> {
    let current = read("docs/reference/CRATE_ARCHITECTURE_DAP.md")?;
    assert!(current.contains("replaces the 0.9-era greenfield design"));
    assert!(current.contains("does **not** ship or require"));
    assert!(current.contains("../archive/DAP_0_9_SHIM_DESIGN.md"));

    let archive = read("docs/archive/DAP_0_9_SHIM_DESIGN.md")?;
    assert!(archive.contains("Historical record; not product architecture"));
    assert!(archive.contains("must not be revived"));
    assert!(archive.contains("Issue #7295 owns that decision"));

    Ok(())
}
