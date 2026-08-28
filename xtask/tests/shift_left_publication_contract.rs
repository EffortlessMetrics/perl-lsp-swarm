//! Regression contract for issue #13047: the provider-native publication skills and
//! the pull-request front door retain one ordered shift-left proof envelope.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const TEMPLATE_PATH: &str = ".github/PULL_REQUEST_TEMPLATE.md";
const PROVIDER_SKILLS: &[(&str, &str)] = &[
    ("Codex", ".agents/skills/publish-pr/SKILL.md"),
    ("Claude", ".claude/skills/publish-pr/SKILL.md"),
];
const REVIEW_INDEX_HEADINGS: &[&str] = &[
    "## Claim",
    "## Controlling issue",
    "## Governing contract",
    "## Changed production path",
    "## Proof",
    "## Test hardening",
    "## Simplification",
    "## Deviations",
    "## Claim Boundary",
    "## Non-goals",
    "## Risk and rollback",
    "## Review index",
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

fn contract_error(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn prose(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn exact_heading_positions(document: &str, heading: &str) -> Vec<usize> {
    let mut offset = 0;
    let mut positions = Vec::new();
    for line in document.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        if content == heading {
            positions.push(offset);
        }
        offset += line.len();
    }
    positions
}

fn validate_ordered_headings(
    document: &str,
    subject: &str,
    headings: &[&str],
) -> Result<(), String> {
    let mut previous = None;
    for &heading in headings {
        let positions = exact_heading_positions(document, heading);
        if positions.len() != 1 {
            return Err(format!(
                "{subject}: expected exactly one heading {heading:?}, found {}",
                positions.len()
            ));
        }
        let position = positions
            .first()
            .copied()
            .ok_or_else(|| format!("{subject}: missing heading {heading:?}"))?;
        if previous.is_some_and(|prior| position <= prior) {
            return Err(format!(
                "{subject}: heading {heading:?} is out of the shift-left review order"
            ));
        }
        previous = Some(position);
    }
    Ok(())
}

fn section<'a>(document: &'a str, heading: &str) -> Result<&'a str, String> {
    let positions = exact_heading_positions(document, heading);
    if positions.len() != 1 {
        return Err(format!(
            "template: expected exactly one section {heading:?}, found {}",
            positions.len()
        ));
    }
    let start = positions
        .first()
        .copied()
        .ok_or_else(|| format!("template: missing section {heading:?}"))?
        + heading.len();
    let tail = &document[start..];
    let content = tail
        .strip_prefix("\r\n")
        .or_else(|| tail.strip_prefix('\n'))
        .unwrap_or(tail);
    let end = content.find("\n## ").unwrap_or(content.len());
    Ok(&content[..end])
}

fn require_phrases(text: &str, subject: &str, phrases: &[&str]) -> Result<(), String> {
    let normalized = prose(text);
    for &phrase in phrases {
        let expected = prose(phrase);
        if !normalized.contains(&expected) {
            return Err(format!(
                "{subject}: missing shift-left semantic marker {phrase:?}"
            ));
        }
    }
    Ok(())
}

fn validate_template(template: &str) -> Result<(), String> {
    validate_ordered_headings(template, TEMPLATE_PATH, REVIEW_INDEX_HEADINGS)?;
    require_phrases(
        template,
        TEMPLATE_PATH,
        &[
            "a pr owns one coherent acceptance and rollback claim",
            "do not fabricate evidence",
            "i ran the cheapest discriminating proof first",
            "focused and affected proof covers the candidate s changed semantic subjects",
        ],
    )?;
    require_phrases(
        section(template, "## Changed production path")?,
        "Changed production path",
        &["real user protocol runtime route", "changed behavior"],
    )?;
    require_phrases(
        section(template, "## Proof")?,
        "Proof",
        &[
            "exact focused commands tests fixtures external oracle and observed results",
            "distinguish pass fail not run not_proven",
        ],
    )?;
    require_phrases(
        section(template, "## Test hardening")?,
        "Test hardening",
        &[
            "realistic wrong implementation",
            "negative stale failure recovery or opposite direction control",
        ],
    )?;
    require_phrases(
        section(template, "## Simplification")?,
        "Simplification",
        &[
            "duplicate authority scaffolding overbroad api repeated validation or dead compatibility",
        ],
    )?;
    require_phrases(
        section(template, "## Claim Boundary")?,
        "Claim Boundary",
        &["provably true", "explicitly out of scope"],
    )?;
    require_phrases(
        section(template, "## Non-goals")?,
        "Non-goals",
        &["unrun proof unsupported cases and remaining work"],
    )?;
    require_phrases(
        section(template, "## Risk and rollback")?,
        "Risk and rollback",
        &[
            "failure modes compatibility support effects",
            "reverted or disabled",
        ],
    )?;
    require_phrases(
        section(template, "## Review index")?,
        "Review index",
        &[
            "governing contract key production seams proof generated artifacts and high risk files",
        ],
    )?;
    Ok(())
}

fn validate_provider_skill(provider: &str, skill: &str) -> Result<(), String> {
    validate_ordered_headings(skill, provider, REVIEW_INDEX_HEADINGS)?;
    require_phrases(
        skill,
        provider,
        &[
            "the order is load bearing",
            "trace the changed production path",
            "focused and affected proof",
            "pass fail not run not_proven",
            "realistic wrong implementation",
            "negative stale failure recovery or opposite direction controls",
            "simplify before publication",
            "bound the claim and non goals",
            "risk rollback and review locations",
        ],
    )?;
    if skill.contains("## What this establishes")
        || skill.contains("## What this does not establish")
    {
        return Err(format!(
            "{provider}: retired establishment headings diverge from the PR front door"
        ));
    }
    Ok(())
}

#[test]
fn publication_contract_is_current() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let template = read(&root, TEMPLATE_PATH)?;
    validate_template(&template).map_err(contract_error)?;

    for &(provider, path) in PROVIDER_SKILLS {
        let skill = read(&root, path)?;
        validate_provider_skill(provider, &skill).map_err(contract_error)?;
    }
    Ok(())
}

#[test]
fn ratchet_rejects_missing_or_reordered_template_sections()
    -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let template = read(&root, TEMPLATE_PATH)?;

    let missing_hardening = template.replacen("## Test hardening\n", "", 1);
    assert_ne!(
        missing_hardening, template,
        "missing-section mutation fixture must apply"
    );
    assert!(
        validate_template(&missing_hardening).is_err(),
        "removing test hardening must fail the publication contract"
    );

    let reordered = template
        .replacen("## Proof", "## __publication_contract_swap__", 1)
        .replacen("## Review index", "## Proof", 1)
        .replacen("## __publication_contract_swap__", "## Review index", 1);
    assert_ne!(reordered, template, "reordering mutation fixture must apply");
    assert!(
        validate_template(&reordered).is_err(),
        "moving proof behind the review index must fail the publication contract"
    );
    Ok(())
}

#[test]
fn ratchet_rejects_weakened_proof_and_provider_drift()
    -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let template = read(&root, TEMPLATE_PATH)?;
    let weakened = template.replacen(
        "Distinguish pass/fail/not-run/NOT_PROVEN.",
        "List the result.",
        1,
    );
    assert_ne!(weakened, template, "proof mutation fixture must apply");
    assert!(
        validate_template(&weakened).is_err(),
        "collapsing typed proof states must fail the publication contract"
    );

    let codex = read(&root, ".agents/skills/publish-pr/SKILL.md")?;
    let drifted = codex.replacen("## Claim Boundary", "## What this establishes", 1);
    assert_ne!(drifted, codex, "provider drift mutation fixture must apply");
    assert!(
        validate_provider_skill("Codex", &drifted).is_err(),
        "restoring a retired provider-only boundary heading must fail"
    );
    Ok(())
}
