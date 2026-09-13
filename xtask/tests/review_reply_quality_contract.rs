//! Regression contract for reasoned inline review dispositions.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

fn project_root() -> Result<PathBuf, Box<dyn Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn assert_reasoned_reply<'a>(skill: &'a str, provider: &str) -> Result<&'a str, Box<dyn Error>> {
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

    let start = skill
        .find("## Reply quality")
        .ok_or("review-response rules are missing the reply-quality section")?;
    let procedure = skill
        .find("\n## Procedure")
        .ok_or("review-response rules are missing the procedure section")?;
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
fn review_response_surfaces_require_reasoned_inline_replies() -> Result<(), Box<dyn Error>> {
    let root = project_root()?;
    let codex = fs::read_to_string(root.join(".agents/skills/address-review-comments/SKILL.md"))?;
    let claude = fs::read_to_string(root.join(".claude/skills/address-review-comments/SKILL.md"))?;
    let droid = fs::read_to_string(root.join(".factory/rules/droid-review.md"))?;
    let github_surfaces = fs::read_to_string(root.join("docs/agents/GITHUB_SURFACES.md"))?;
    let threads = fs::read_to_string(root.join("scripts/reviews/threads"))?;

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

    for marker in [
        "## Finding disposition",
        "concise engineering decision record",
        "<judgment, architectural reason",
        "Evaluate the concern and the suggested repair separately",
        "Do not blindly agree",
        "not reflexively defend the candidate",
        "repair the owning seam",
        "A bare `fixed`",
    ] {
        assert!(
            github_surfaces.contains(marker),
            "shared GitHub-surface rules must retain marker {marker:?}"
        );
    }

    let reasoned_template = concat!(
        "--reply 'Disposition: fixed\n\n",
        "<concise judgment: what failed, why this boundary owns it, and what changed>\n\n",
        "Evidence: <claim-bounded evidence>'",
    );
    assert!(
        threads.contains(reasoned_template),
        "the sanctioned thread helper must emit the reasoned reply template"
    );
    assert!(
        !threads.contains("Disposition: fixed\nEvidence:"),
        "the sanctioned thread helper must not emit the old labels-only template"
    );

    Ok(())
}
