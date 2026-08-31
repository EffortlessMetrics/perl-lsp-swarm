//! Regression contract for joined review-repair waves and stable CI subjects.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

fn project_root() -> Result<PathBuf, Box<dyn Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn read(root: &Path, path: &str) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(root.join(path))?)
}

fn assert_address_contract(skill: &str, provider: &str) -> Result<(), Box<dyn Error>> {
    for marker in [
        "### Repair-wave boundary",
        "pin one review-wave observation basis",
        "quota-limited, unavailable, or failed reviewer",
        "same underlying mechanism",
        "promote them to one failure class",
        "class-level falsifier",
        "complete governing semantic unit",
        "publishes it once",
        "do not publish per-comment pushes",
        "CLASS_REPAIR_REQUIRED",
        "REPAIR_WAVE_NOT_PROVEN",
    ] {
        assert!(
            skill.contains(marker),
            "{provider} address-review-comments must retain marker {marker:?}"
        );
    }

    let repair = skill
        .find("### Repair-wave boundary")
        .ok_or("repair-wave section is missing")?;
    let replies = skill
        .find("## Reply quality")
        .ok_or("reply-quality section is missing")?;
    assert!(
        repair < replies,
        "{provider} must join and classify the repair wave before composing replies"
    );

    Ok(())
}

fn assert_finish_contract(skill: &str, provider: &str) -> Result<(), Box<dyn Error>> {
    for marker in [
        "## Repair waves and head stabilization",
        "do not publish one commit per comment",
        "one candidate update",
        "HEAD_STABILIZED_FOR_CI",
        "candidate-owned required-check failure",
        "duplicate corroboration",
        "reviewer quota or availability",
        "one new joined repair wave",
    ] {
        assert!(
            skill.contains(marker),
            "{provider} finish-pr must retain marker {marker:?}"
        );
    }

    let waves = skill
        .find("## Repair waves and head stabilization")
        .ok_or("repair-wave convergence section is missing")?;
    let integration = skill
        .find("## Candidate and integration boundary")
        .ok_or("candidate/integration section is missing")?;
    assert!(
        waves < integration,
        "{provider} must stabilize the reviewed candidate before live integration"
    );

    Ok(())
}

#[test]
fn provider_skills_join_repair_classes_before_stabilizing_ci() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;

    let codex_address = read(
        &root,
        ".agents/skills/address-review-comments/SKILL.md",
    )?;
    let claude_address = read(
        &root,
        ".claude/skills/address-review-comments/SKILL.md",
    )?;
    let codex_finish = read(&root, ".agents/skills/finish-pr/SKILL.md")?;
    let claude_finish = read(&root, ".claude/skills/finish-pr/SKILL.md")?;

    assert_address_contract(&codex_address, "Codex")?;
    assert_address_contract(&claude_address, "Claude")?;
    assert_finish_contract(&codex_finish, "Codex")?;
    assert_finish_contract(&claude_finish, "Claude")?;

    Ok(())
}
