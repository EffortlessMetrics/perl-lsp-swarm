use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SECTION_HEADING: &str = "## Shift-left claim admission";
const PROVIDER_ENTRY_SKILLS: &[(&str, &str)] = &[
    ("codex", ".agents/skills/deliver-pr/SKILL.md"),
    ("claude", ".claude/skills/deliver-pr/SKILL.md"),
];
const REQUIREMENTS: &[(&str, &[&str])] = &[
    (
        "a pre-mutation boundary",
        &[
            "before the first delegated mutation",
            "before delegating a mutation",
        ],
    ),
    (
        "direct candidate edits are inside the boundary",
        &["direct candidate edit", "editing the candidate directly"],
    ),
    (
        "one coherent claim",
        &["acceptance-and-rollback claim", "coherent claim"],
    ),
    ("the semantic owner", &["semantic owner"]),
    (
        "current governing authority",
        &["current authority", "governing authority"],
    ),
    (
        "current facts and contradictions",
        &["source-backed facts", "current facts"],
    ),
    ("material contradictions", &["contradictions"]),
    (
        "the production or observable seam",
        &["production seam", "observable seam"],
    ),
    ("the acceptance surface", &["acceptance surface"]),
    ("the cheapest check", &["cheapest"]),
    (
        "the first falsifier",
        &["first falsifier", "earliest falsifier"],
    ),
    (
        "a realistic wrong or negative control",
        &[
            "wrong implementation",
            "negative control",
            "current defect",
        ],
    ),
    ("the proof ceiling", &["proof ceiling"]),
    ("an explicit NOT_PROVEN boundary", &["not_proven"]),
    (
        "deferred broader proof",
        &[
            "broader proof is deferred",
            "broader proof to defer",
            "defer broader proof",
        ],
    ),
    ("one mutation owner", &["mutation owner"]),
    ("one writer", &["one writer"]),
    (
        "a named next or backward route",
        &[
            "next or backward route",
            "next/backward route",
            "named next",
        ],
    ),
    (
        "the earliest missing judgment",
        &["earliest missing judgment"],
    ),
    ("read-only pre-admission research", &["read-only research"]),
    ("the prepare-issue repair route", &["prepare-issue"]),
    ("the prepare-proof repair route", &["prepare-proof"]),
    (
        "an unresolved falsifier repair condition",
        &[
            "first falsifier is unresolved",
            "earliest falsifier is unresolved",
        ],
    ),
    ("an anti-inference rule", &["do not infer"]),
    (
        "candidate refusal before admission",
        &["mint a candidate", "create a candidate", "begin a candidate"],
    ),
    ("the runtime-local boundary", &["runtime-local"]),
    (
        "the durable-state exception",
        &["runtime-local unless it changes durable claim, authority, or proof state"],
    ),
    ("the non-stage boundary", &["stage record"]),
    ("the non-lease boundary", &["lease"]),
    ("the non-scheduler boundary", &["scheduler"]),
    ("the non-frontier boundary", &["tracked frontier"]),
];

fn repo_root() -> io::Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest directory has no repository parent"))
}

fn h2_section(text: &str, heading: &str) -> Option<String> {
    let mut in_section = false;
    let mut lines = Vec::new();

    for line in text.lines() {
        if line.trim() == heading {
            in_section = true;
            continue;
        }
        if in_section && line.trim_start().starts_with("## ") {
            break;
        }
        if in_section {
            lines.push(line);
        }
    }

    if in_section {
        Some(lines.join("\n"))
    } else {
        None
    }
}

fn validate_claim_admission(text: &str) -> Vec<String> {
    let Some(section) = h2_section(text, SECTION_HEADING) else {
        return vec![format!("missing section '{SECTION_HEADING}'")];
    };
    let section = section
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let mut errors = Vec::new();

    for &(label, alternatives) in REQUIREMENTS {
        if !alternatives.iter().any(|&term| section.contains(term)) {
            errors.push(format!(
                "claim admission is missing {label}; expected one of: {}",
                alternatives.join(", ")
            ));
        }
    }

    errors
}

#[test]
fn provider_entry_skills_encode_shift_left_claim_admission() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let mut failures = Vec::new();

    for &(provider, relative_path) in PROVIDER_ENTRY_SKILLS {
        let path = root.join(relative_path);
        let text = fs::read_to_string(&path)?;
        for error in validate_claim_admission(&text) {
            failures.push(format!("{provider} ({}): {error}", path.display()));
        }
    }

    assert!(
        failures.is_empty(),
        "provider-native shift-left entry contract failed:\n{}",
        failures.join("\n")
    );
    Ok(())
}

#[test]
fn semantic_alternatives_do_not_require_byte_identical_provider_prose() {
    let text = r#"
## Shift-left claim admission
Before delegating a mutation or editing the candidate directly, retain a coherent claim
and its semantic owner. Name the governing authority, current facts and contradictions,
and observable seam. State the acceptance surface and choose the cheapest earliest
falsifier, including a negative control. State the proof ceiling, what stays
`NOT_PROVEN`, and which broader proof to defer. Name the mutation owner, one writer, the
earliest missing judgment, and the named next or backward route. Read-only research may
precede this boundary. When the earliest falsifier is unresolved, do not infer missing
facts or create a candidate; route through `prepare-issue` or `prepare-proof`.
Keep this runtime-local unless it changes durable claim, authority, or proof state. It
is not a stage record, lease, scheduler, or tracked frontier.

## Entry route
Later content.
"#;

    assert!(validate_claim_admission(text).is_empty());
}

#[test]
fn markers_elsewhere_do_not_satisfy_the_admission_section() {
    let text = r#"
Editing the candidate directly, the acceptance surface, first falsifier, proof ceiling,
`NOT_PROVEN`, mutation owner, one writer, earliest missing judgment, current authority,
source-backed facts, contradictions, production seam, broader proof is deferred,
read-only research, prepare-issue, prepare-proof, earliest falsifier is unresolved, do
not infer, mint a candidate, runtime-local unless it changes durable claim, authority,
or proof state, stage record, lease, scheduler, and tracked frontier are here.

## Shift-left claim admission
Before the first delegated mutation, retain an acceptance-and-rollback claim and its
semantic owner. Choose the cheapest negative control and the next/backward route.

## Entry route
Later content.
"#;

    let errors = validate_claim_admission(text);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("direct candidate edits"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("acceptance surface"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("proof ceiling"))
    );
    assert!(errors.iter().any(|error| error.contains("prepare-issue")));
    assert!(
        errors
            .iter()
            .any(|error| error.contains("durable-state exception"))
    );
}

#[test]
fn missing_proof_and_backward_route_semantics_fail_closed() {
    let text = r#"
## Shift-left claim admission
Before the first delegated mutation or direct candidate edit, retain an
acceptance-and-rollback claim and its semantic owner, current authority, source-backed
facts, contradictions, production seam, and acceptance surface. Choose the cheapest
first falsifier against a wrong implementation. Name the mutation owner, one writer,
the earliest missing judgment, and the next or backward route. Read-only research may
precede the boundary. When the first falsifier is unresolved, do not infer missing
facts or mint a candidate. Keep this runtime-local unless it changes durable claim,
authority, or proof state; it is not a stage record, lease, scheduler, or tracked
frontier.
"#;

    let errors = validate_claim_admission(text);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("proof ceiling"))
    );
    assert!(errors.iter().any(|error| error.contains("NOT_PROVEN")));
    assert!(errors.iter().any(|error| error.contains("prepare-issue")));
    assert!(errors.iter().any(|error| error.contains("prepare-proof")));
}

#[test]
fn missing_admission_section_fails_closed() {
    let errors = validate_claim_admission("## Entry route\n- `prepare-issue`");
    assert_eq!(errors, vec![format!("missing section '{SECTION_HEADING}'")]);
}
