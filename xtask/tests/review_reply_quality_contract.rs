//! Regression contract for reasoned inline review dispositions.

use std::error::Error;
use std::fs;
use std::io;
use std::path::PathBuf;

fn project_root() -> Result<PathBuf, Box<dyn Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn marker_index(skill: &str, marker: &str, provider: &str) -> Result<usize, io::Error> {
    skill.find(marker).ok_or_else(|| {
        io::Error::other(format!(
            "{provider} review-response rules must retain marker {marker:?}"
        ))
    })
}

fn assert_reasoned_reply<'a>(
    skill: &'a str,
    provider: &str,
) -> Result<&'a str, Box<dyn Error>> {
    for marker in [
        "## Reply quality",
        "concise engineering decision record",
        "Do not blindly agree",
        "Do not reflexively defend",
        "right about the failure and wrong about the",
        "repair the owning",
        "A bare `fixed`",
        "<judgment, architectural reason",
        "reply must answer the comment in its inline context",
        "put the concise reasoned judgment between them",
    ] {
        assert!(
            skill.contains(marker),
            "{provider} review-response rules must retain marker {marker:?}"
        );
    }

    let start = marker_index(skill, "## Reply quality", provider)?;
    let procedure = marker_index(skill, "\n## Procedure", provider)?;
    assert!(
        start < procedure,
        "{provider} must establish reply judgment before the mutation procedure"
    );
    assert!(
        !skill.contains("5. Compose the canonical human reply"),
        "{provider} must not regress to the old labels-only reply instruction"
    );

    Ok(&skill[start..procedure])
}

#[test]
fn provider_rules_require_reasoned_inline_replies() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;
    let codex = fs::read_to_string(root.join(".agents/skills/address-review-comments/SKILL.md"))?;
    let claude =
        fs::read_to_string(root.join(".claude/skills/address-review-comments/SKILL.md"))?;
    let droid = fs::read_to_string(root.join(".factory/rules/droid-review.md"))?;

    let codex_section = assert_reasoned_reply(&codex, "Codex")?;
    let claude_section = assert_reasoned_reply(&claude, "Claude")?;
    assert_eq!(
        codex_section, claude_section,
        "Claude and Codex must share the same reply-quality judgment contract"
    );

    for marker in [
        "## Replying to Inline Findings",
        "concise engineering decision record",
        "Do not blindly agree",
        "reflexively defend",
        "repair the owning seam",
        "A bare `fixed`",
        "<judgment, architectural reason",
    ] {
        assert!(
            droid.contains(marker),
            "Droid review-response rules must retain marker {marker:?}"
        );
    }

    Ok(())
}
