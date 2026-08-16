//! Ratchet for RIPR new-gap placement authority (epic #9112).
//!
//! #9112 moved diff-scoped RIPR new-gap enforcement to the exact staged-tree commit
//! boundary, over the one `git write-tree` candidate OID established by #3786. The
//! earlier categorical ruling — that RIPR has no exact staged input and therefore
//! structurally belongs after the commit tier — is retired.
//!
//! That ruling is easy to reintroduce by paraphrase, because it reads like a durable
//! invariant rather than the migration snapshot it actually was. Any agent contract that
//! restates it teaches fresh sessions to skip, resist, or later undo the accepted
//! migration. These tests fail if it comes back, and fail if the surfaces stop naming
//! #9112 as the placement authority.

use std::fs;
use std::path::{Path, PathBuf};

/// Every surface an orchestrator or claim lane reads for proof-ladder placement.
const CONTRACT_SURFACES: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    ".claude/skills/change-graph/SKILL.md",
    ".agents/skills/change-graph/SKILL.md",
    ".claude/skills/prove-before-push/SKILL.md",
    ".agents/skills/prove-before-push/SKILL.md",
];

/// Assertive forms of the retired ruling, in normalized prose.
///
/// These are deliberately the *claim* shapes, not every mention of RIPR and the commit
/// tier: the surfaces must stay free to describe the current non-blocking migration
/// state and the commit-tier budget.
const RETIRED_RULINGS: &[(&str, &str)] = &[
    ("ripr has no exact staged", "denies the #3786 staged-tree subject #9112 builds on"),
    ("ripr had no exact staged", "denies the #3786 staged-tree subject #9112 builds on"),
    ("no ripr in the pre-commit", "restates the retired categorical exclusion"),
    ("no ripr in the commit tier", "restates the retired categorical exclusion"),
    ("ripr does not move into pre-commit", "contradicts the accepted #9112 target"),
    ("ripr does not move into the commit tier", "contradicts the accepted #9112 target"),
    (
        "do not put cargo compilation or ripr in the commit tier",
        "couples RIPR to the Cargo-compilation exclusion #9112 retired",
    ),
    (
        "do not move cargo compilation or ripr into the commit tier",
        "couples RIPR to the Cargo-compilation exclusion #9112 retired",
    ),
];

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn read(root: &Path, path: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(fs::read_to_string(root.join(path))?)
}

/// Collapse markdown emphasis, quoting, and wrapping so a paraphrase split across lines
/// cannot slip past a substring match.
fn prose(text: &str) -> String {
    text.replace('`', "")
        .replace('*', "")
        .replace('"', "")
        .replace('\u{201c}', "")
        .replace('\u{201d}', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[test]
fn contract_surfaces_do_not_restate_retired_ripr_placement() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root()?;
    let mut violations = Vec::new();

    for surface in CONTRACT_SURFACES {
        let text = prose(&read(&root, surface)?);
        for (ruling, why) in RETIRED_RULINGS {
            if text.contains(ruling) {
                violations.push(format!("{surface}: contains {ruling:?} — {why}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "retired RIPR placement ruling reintroduced; #9112 is the current authority:\n  {}",
        violations.join("\n  ")
    );
    Ok(())
}

#[test]
fn proof_ladder_surfaces_name_the_current_placement_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let mut missing = Vec::new();

    for surface in CONTRACT_SURFACES {
        if !read(&root, surface)?.contains("#9112") {
            missing.push(*surface);
        }
    }

    assert!(
        missing.is_empty(),
        "these surfaces describe the proof ladder without naming #9112 as the RIPR \
         placement authority, so a reader cannot tell current migration state from \
         settled invariant: {missing:?}"
    );
    Ok(())
}

#[test]
fn pre_push_surfaces_do_not_claim_rival_new_gap_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let mut violations = Vec::new();

    // #7365 remains real work, but is scoped to pre-push affected proof and Changie. It
    // must not be described as owning diff-scoped RIPR new-gap placement again.
    for surface in
        [".claude/skills/prove-before-push/SKILL.md", ".agents/skills/prove-before-push/SKILL.md"]
    {
        let text = prose(&read(&root, surface)?);
        if text.contains("7365 owns completion of one executable local path that includes diff-scoped ripr")
        {
            violations.push(format!(
                "{surface}: re-scopes #7365 to own diff-scoped RIPR placement beside #9112"
            ));
        }
    }

    assert!(violations.is_empty(), "{}", violations.join("\n  "));
    Ok(())
}
