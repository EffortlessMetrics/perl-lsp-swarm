use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SECTION_HEADING: &str = "## Shift-left claim admission";
const PROVIDER_ENTRY_SKILLS: &[(&str, &str)] = &[
    ("codex", ".agents/skills/deliver-pr/SKILL.md"),
    ("claude", ".claude/skills/deliver-pr/SKILL.md"),
];
/// How a requirement's markers must appear inside the admission section.
///
/// The kind is part of the table rather than a label comparison inside the
/// validator so that a requirement cannot silently acquire a branch that
/// always succeeds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Match {
    /// The marker must appear as positive guidance (not under negation).
    Positive,
    /// The marker is itself a prohibition, so it must simply be present.
    Prohibition,
    /// The marker must be refused inside the same sentence that names an
    /// unresolved admission fact.
    ConditionalRefusal,
    /// The marker must be negated *and* tied to the runtime-local boundary.
    Boundary,
}

const REQUIREMENTS: &[(Match, &str, &[&str])] = &[
    (
        Match::Positive,
        "a pre-mutation boundary",
        &["before the first delegated mutation", "before delegating a mutation"],
    ),
    (
        Match::Positive,
        "direct candidate edits are inside the boundary",
        &["direct candidate edit", "editing the candidate directly"],
    ),
    (Match::Positive, "one coherent claim", &["acceptance-and-rollback claim", "coherent claim"]),
    (Match::Positive, "the semantic owner", &["semantic owner"]),
    (Match::Positive, "current governing authority", &["current authority", "governing authority"]),
    (
        Match::Positive,
        "current facts and contradictions",
        &["source-backed facts", "current facts"],
    ),
    (Match::Positive, "material contradictions", &["contradictions"]),
    (Match::Positive, "the production or observable seam", &["production seam", "observable seam"]),
    (Match::Positive, "the acceptance surface", &["acceptance surface"]),
    (Match::Positive, "the cheapest check", &["cheapest"]),
    (Match::Positive, "the first falsifier", &["first falsifier", "earliest falsifier"]),
    (
        Match::Positive,
        "a realistic wrong or negative control",
        &["wrong implementation", "negative control", "current defect"],
    ),
    (Match::Positive, "the proof ceiling", &["proof ceiling"]),
    (Match::Positive, "an explicit NOT_PROVEN boundary", &["not_proven"]),
    (
        Match::Positive,
        "deferred broader proof",
        &["broader proof is deferred", "broader proof to defer", "defer broader proof"],
    ),
    (Match::Positive, "one mutation owner", &["mutation owner"]),
    (Match::Positive, "one writer", &["one writer"]),
    (
        Match::Positive,
        "a named next or backward route",
        &["next or backward route", "next/backward route", "named next"],
    ),
    (Match::Positive, "the earliest missing judgment", &["earliest missing judgment"]),
    (Match::Positive, "read-only pre-admission research", &["read-only research"]),
    (Match::Positive, "the prepare-issue repair route", &["prepare-issue"]),
    (Match::Positive, "the prepare-proof repair route", &["prepare-proof"]),
    (
        Match::Positive,
        "an unresolved falsifier repair condition",
        &["first falsifier is unresolved", "earliest falsifier is unresolved"],
    ),
    (Match::Prohibition, "an anti-inference rule", &["do not infer"]),
    (
        Match::ConditionalRefusal,
        "candidate refusal before admission",
        &["mint a candidate", "create a candidate", "begin a candidate"],
    ),
    (Match::Positive, "the runtime-local boundary", &["runtime-local"]),
    (
        Match::Positive,
        "the durable-state exception",
        &["runtime-local unless it changes durable claim, authority, or proof state"],
    ),
    (Match::Boundary, "the non-stage boundary", &["stage record"]),
    (Match::Boundary, "the non-lease boundary", &["lease"]),
    (Match::Boundary, "the non-scheduler boundary", &["scheduler"]),
    (Match::Boundary, "the non-frontier boundary", &["tracked frontier"]),
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

    if in_section { Some(lines.join("\n")) } else { None }
}

fn visible_markdown(text: &str) -> String {
    let mut visible = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut in_comment = false;

    for line in text.lines() {
        let trimmed = line.trim();
        let fence_run = trimmed.chars().next().and_then(|character| {
            if character != '`' && character != '~' {
                return None;
            }
            let length = trimmed.chars().take_while(|candidate| *candidate == character).count();
            let suffix_is_whitespace = trimmed.chars().skip(length).all(char::is_whitespace);
            (length >= 3).then_some((character, length, suffix_is_whitespace))
        });
        if let Some((character, length)) = fence {
            if fence_run.is_some_and(|(candidate, candidate_length, suffix_is_whitespace)| {
                suffix_is_whitespace && candidate == character && candidate_length >= length
            }) {
                fence = None;
            }
            continue;
        }
        if let Some((character, length, _)) = fence_run {
            fence = Some((character, length));
            continue;
        }

        let mut remaining = line;
        while !remaining.is_empty() {
            if in_comment {
                let Some(end) = remaining.find("-->") else {
                    remaining = "";
                    continue;
                };
                in_comment = false;
                remaining = &remaining[end + 3..];
            } else if let Some(start) = remaining.find("<!--") {
                visible.push(&remaining[..start]);
                in_comment = true;
                remaining = &remaining[start + 4..];
            } else {
                visible.push(remaining);
                break;
            }
        }
        visible.push("\n");
    }

    visible.concat()
}

fn is_negated_at(line: &str, marker_start: usize, marker: &str) -> bool {
    let sentence_start =
        line[..marker_start].rfind(['.', '!', '?', ';']).map_or(0, |index| index + 1);
    let sentence_end = line[marker_start + marker.len()..]
        .find(['.', '!', '?', ';'])
        .map_or(line.len(), |index| marker_start + marker.len() + index);
    // Anchor the prefix on a trailing word boundary. Trimming alone drops the space the
    // negation tokens end with, so a negation sitting immediately before the marker
    // ("do not create a candidate") would read as positive guidance and the refusal
    // requirement would reject valid provider prose.
    let prefix = format!("{} ", line[sentence_start..marker_start].trim().to_ascii_lowercase());
    let suffix = line[marker_start + marker.len()..sentence_end].trim().to_ascii_lowercase();

    let local_negation = [
        "not ",
        "not a ",
        "never ",
        "without ",
        "does not ",
        "do not ",
        "no ",
        "omit ",
        "omitted",
        "missing ",
    ]
    .iter()
    .any(|negation| prefix.ends_with(negation) || suffix.starts_with(negation));
    let coordinated_negation =
        prefix.find("not a ").is_some_and(|start| !prefix[start..].contains(" but "));
    let modal_negation = [
        "must not ",
        "mustn't ",
        "should not ",
        "shouldn't ",
        "may not ",
        "cannot ",
        "can't ",
        "do not ",
        "don't ",
        "does not ",
        "doesn't ",
    ]
    .iter()
    .any(|negation| prefix.contains(negation));

    local_negation
        || coordinated_negation
        || modal_negation
        || suffix.starts_with("must not ")
        || suffix.starts_with("mustn't ")
        || suffix.starts_with("should not ")
        || suffix.starts_with("shouldn't ")
        || suffix.starts_with("may not ")
        || suffix.starts_with("cannot ")
        || suffix.starts_with("can't ")
        || suffix.starts_with("do not ")
        || suffix.starts_with("don't ")
        || suffix.starts_with("does not ")
        || suffix.starts_with("doesn't ")
        || suffix.starts_with("is not ")
        || suffix.starts_with("isn't ")
        || suffix.starts_with("isn’t ")
        || suffix.starts_with("are not ")
        || suffix.starts_with("aren't ")
        || suffix.starts_with("aren’t ")
        || suffix.starts_with("not required")
        || suffix.starts_with("not needed")
        || suffix.starts_with("is omitted")
        || suffix.contains(" optional")
        || suffix.starts_with("doesn't matter")
        || suffix.starts_with("doesn’t matter")
        || suffix.starts_with("may be omitted")
        || suffix.starts_with("can be omitted")
}

fn has_unnegated_marker(text: &str, marker: &str) -> bool {
    text.match_indices(marker).any(|(start, _)| !is_negated_at(text, start, marker))
}

/// The refusal must be one piece of conditional guidance, not two unrelated
/// statements that happen to share a section. An unresolved admission fact
/// somewhere in the section plus a candidate prohibition somewhere else would
/// otherwise let a provider permit candidates on unresolved facts while the
/// control-plane check stayed green.
fn has_conditional_candidate_refusal(text: &str, alternatives: &[&str]) -> bool {
    alternatives.iter().any(|&marker| {
        text.match_indices(marker).any(|(start, _)| {
            if !is_negated_at(text, start, marker) {
                return false;
            }
            sentence_around(text, start, marker.len()).contains("unresolved")
        })
    })
}

/// The sentence containing `[start, start + length)`, bounded by terminators.
fn sentence_around(text: &str, start: usize, length: usize) -> &str {
    let sentence_start = text[..start].rfind(['.', '!', '?', ';']).map_or(0, |index| index + 1);
    let sentence_end = text[start + length..]
        .find(['.', '!', '?', ';'])
        .map_or(text.len(), |index| start + length + index);
    &text[sentence_start..sentence_end]
}

fn has_coordinated_boundary(text: &str, marker: &str) -> bool {
    text.match_indices(marker).next().is_some()
        && text.match_indices(marker).all(|(start, _)| {
            if !is_negated_at(text, start, marker) {
                return false;
            }
            let sentence_start = text[..start].rfind(['.', '!', '?']).map_or(0, |index| index + 1);
            let previous_start = text[..sentence_start.saturating_sub(1)]
                .rfind(['.', '!', '?'])
                .map_or(0, |index| index + 1);
            let sentence_end = text[start + marker.len()..]
                .find(['.', '!', '?'])
                .map_or(text.len(), |index| start + marker.len() + index);
            let current = &text[sentence_start..sentence_end];
            let previous = &text[previous_start..sentence_start];
            [current, previous].iter().any(|context| {
                context.contains("runtime-local") && context.contains("durable claim")
            })
        })
}

fn validate_claim_admission(text: &str) -> Vec<String> {
    let text = visible_markdown(text);
    let Some(section) = h2_section(&text, SECTION_HEADING) else {
        return vec![format!("missing section '{SECTION_HEADING}'")];
    };
    let section = section.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
    let mut errors = Vec::new();

    for &(kind, label, alternatives) in REQUIREMENTS {
        let present = match kind {
            Match::ConditionalRefusal => has_conditional_candidate_refusal(&section, alternatives),
            // A prohibition carries its own negation, so polarity analysis is
            // skipped -- but presence is still required, or the requirement
            // would be vacuous.
            Match::Prohibition => alternatives.iter().any(|&term| section.contains(term)),
            Match::Boundary => {
                alternatives.iter().any(|&term| has_coordinated_boundary(&section, term))
            }
            Match::Positive => {
                alternatives.iter().any(|&term| has_unnegated_marker(&section, term))
            }
        };
        if !present {
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

    let errors = validate_claim_admission(text);
    assert!(errors.is_empty(), "{errors:?}");
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
    assert!(errors.iter().any(|error| error.contains("direct candidate edits")));
    assert!(errors.iter().any(|error| error.contains("acceptance surface")));
    assert!(errors.iter().any(|error| error.contains("proof ceiling")));
    assert!(errors.iter().any(|error| error.contains("prepare-issue")));
    assert!(errors.iter().any(|error| error.contains("durable-state exception")));
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
    assert!(errors.iter().any(|error| error.contains("proof ceiling")));
    assert!(errors.iter().any(|error| error.contains("NOT_PROVEN")));
    assert!(errors.iter().any(|error| error.contains("prepare-issue")));
    assert!(errors.iter().any(|error| error.contains("prepare-proof")));
}

#[test]
fn permission_to_infer_fails_the_anti_inference_requirement() {
    // The section is otherwise complete; only the prohibitive rule is turned
    // into permission. Before the requirement was typed, this passed.
    let text = r#"
## Shift-left claim admission
Before delegating a mutation or editing the candidate directly, retain a coherent claim
and its semantic owner. Name the governing authority, current facts and contradictions,
and observable seam. State the acceptance surface and choose the cheapest earliest
falsifier, including a negative control. State the proof ceiling, what stays
`NOT_PROVEN`, and which broader proof to defer. Name the mutation owner, one writer, the
earliest missing judgment, and the named next or backward route. Read-only research may
precede this boundary. When the earliest falsifier is unresolved, you may infer the
missing facts, and must not create a candidate; route through `prepare-issue` or
`prepare-proof`. Keep this runtime-local unless it changes durable claim, authority, or
proof state. It is not a stage record, lease, scheduler, or tracked frontier.

## Entry route
Later content.
"#;

    let errors = validate_claim_admission(text);
    assert!(
        errors.iter().any(|error| error.contains("an anti-inference rule")),
        "expected the anti-inference requirement to fail closed: {errors:?}"
    );
}

#[test]
fn negation_adjacent_to_the_marker_is_read_as_a_refusal() {
    // The most direct phrasing of the refusal puts the negation immediately before the
    // marker, with no intervening words. That must count as a refusal, or the contract
    // rejects valid provider guidance.
    let text = r#"
## Shift-left claim admission
Before delegating a mutation or editing the candidate directly, retain a coherent claim
and its semantic owner. Name the governing authority, current facts and contradictions,
and observable seam. State the acceptance surface and choose the cheapest earliest
falsifier, including a negative control. State the proof ceiling, what stays
`NOT_PROVEN`, and which broader proof to defer. Name the mutation owner, one writer, the
earliest missing judgment, and the named next or backward route. Read-only research may
precede this boundary. When the earliest falsifier is unresolved, do not infer the
missing facts. When the earliest falsifier is unresolved, do not create a candidate.
Route through `prepare-issue` or `prepare-proof`. Keep this runtime-local unless it
changes durable claim, authority, or proof state. It is not a stage record, lease,
scheduler, or tracked frontier.

## Entry route
Later content.
"#;

    let errors = validate_claim_admission(text);
    assert!(errors.is_empty(), "valid provider guidance was rejected: {errors:?}");
}

#[test]
fn unrelated_candidate_prohibition_does_not_satisfy_conditional_refusal() {
    // "unresolved" and the candidate prohibition are in separate, unrelated
    // sentences: unresolved facts explicitly permit a candidate here.
    let text = r#"
## Shift-left claim admission
Before delegating a mutation or editing the candidate directly, retain a coherent claim
and its semantic owner. Name the governing authority, current facts and contradictions,
and observable seam. State the acceptance surface and choose the cheapest earliest
falsifier, including a negative control. State the proof ceiling, what stays
`NOT_PROVEN`, and which broader proof to defer. Name the mutation owner, one writer, the
earliest missing judgment, and the named next or backward route. Read-only research may
precede this boundary. When the earliest falsifier is unresolved, you may still create a
candidate; do not infer the missing facts, and route through `prepare-issue` or
`prepare-proof`. After the lane is closed, do not reopen or create a candidate. Keep this
runtime-local unless it changes durable claim, authority, or proof state. It is not a
stage record, lease, scheduler, or tracked frontier.

## Entry route
Later content.
"#;

    let errors = validate_claim_admission(text);
    assert!(
        errors.iter().any(|error| error.contains("candidate refusal before admission")),
        "expected the conditional refusal requirement to fail closed: {errors:?}"
    );
}

#[test]
fn missing_admission_section_fails_closed() {
    let errors = validate_claim_admission("## Entry route\n- `prepare-issue`");
    assert_eq!(errors, vec![format!("missing section '{SECTION_HEADING}'")]);
}

#[test]
fn hidden_markdown_decoys_do_not_satisfy_the_admission_section() {
    let text = r#"
## Shift-left claim admission
Before the first delegated mutation, retain a coherent claim and its owner.
<!-- acceptance surface, first falsifier, proof ceiling, NOT_PROVEN, prepare-proof,
prepare-issue, mutation owner, one writer, runtime-local, stage record, lease,
scheduler, tracked frontier -->
~~~text
direct candidate edit current authority production seam negative control
broader proof is deferred earliest missing judgment next/backward route
~~~
## Entry route
Later content.
"#;

    let errors = validate_claim_admission(text);
    assert!(errors.iter().any(|error| error.contains("acceptance surface")));
    assert!(errors.iter().any(|error| error.contains("proof ceiling")));
    assert!(errors.iter().any(|error| error.contains("prepare-issue")));
}

#[test]
fn mismatched_markdown_fences_keep_decoys_hidden() {
    let text = r#"
## Shift-left claim admission
Before the first delegated mutation, retain a coherent claim and its owner.
```text
direct candidate edit current authority production seam negative control
broader proof is deferred earliest missing judgment next/backward route
~~~
acceptance surface proof ceiling NOT_PROVEN prepare-issue prepare-proof
mutation owner one writer runtime-local durable claim stage record lease scheduler
tracked frontier
```
## Entry route
Later content.
"#;

    let errors = validate_claim_admission(text);
    assert!(errors.iter().any(|error| error.contains("acceptance surface")));
    assert!(errors.iter().any(|error| error.contains("proof ceiling")));
    assert!(errors.iter().any(|error| error.contains("prepare-issue")));
}

#[test]
fn markdown_fence_with_trailing_text_does_not_close_a_block() {
    let text = r#"
## Shift-left claim admission
Before the first delegated mutation, retain a coherent claim and its owner.
```text
```still-code
direct candidate edit current authority production seam negative control
broader proof is deferred earliest missing judgment next/backward route
acceptance surface proof ceiling NOT_PROVEN prepare-issue prepare-proof
mutation owner one writer runtime-local durable claim stage record lease scheduler
tracked frontier
```
## Entry route
Later content.
"#;

    let errors = validate_claim_admission(text);
    assert!(errors.iter().any(|error| error.contains("acceptance surface")));
    assert!(errors.iter().any(|error| error.contains("proof ceiling")));
    assert!(errors.iter().any(|error| error.contains("prepare-issue")));
}

#[test]
fn contrast_clause_does_not_negate_a_later_marker() {
    let text = r#"
## Shift-left claim admission
Before the first delegated mutation, retain a coherent claim and its semantic owner.
The boundary is not a stage record, but this is a stage record.
## Entry route
Later content.
"#;

    let errors = validate_claim_admission(text);
    assert!(errors.iter().any(|error| error.contains("non-stage boundary")), "{errors:?}");
}

#[test]
fn boundary_markers_must_be_tied_to_the_runtime_boundary() {
    let text = r#"
## Shift-left claim admission
Before the first delegated mutation, retain a coherent claim and its semantic owner.
Keep this runtime-local unless it changes durable claim, authority, or proof state.
This unrelated sentence separates the boundary concepts.
The boundary is not a stage record, lease, scheduler, or tracked frontier.
## Entry route
Later content.
"#;

    let errors = validate_claim_admission(text);
    assert!(errors.iter().any(|error| error.contains("non-stage boundary")), "{errors:?}");
    assert!(errors.iter().any(|error| error.contains("non-lease boundary")));
    assert!(errors.iter().any(|error| error.contains("non-scheduler boundary")));
    assert!(errors.iter().any(|error| error.contains("non-frontier boundary")));
}

#[test]
fn negated_markdown_obligations_fail_closed() {
    let text = r#"
## Shift-left claim admission
Before the first delegated mutation, retain a coherent claim and semantic owner.
The acceptance surface doesn’t matter, the proof ceiling may be omitted, and no
negative control is needed. Current authority and production seam are optional.
The mutation owner is optional, one writer is not required. It is not a stage
record, lease, scheduler, or tracked frontier.
## Entry route
Later content.
"#;

    let errors = validate_claim_admission(text);
    assert!(errors.iter().any(|error| error.contains("acceptance surface")));
    assert!(errors.iter().any(|error| error.contains("proof ceiling")));
    assert!(errors.iter().any(|error| error.contains("first falsifier")));
    assert!(errors.iter().any(|error| error.contains("current governing authority")));
    assert!(errors.iter().any(|error| error.contains("one mutation owner")));
    assert!(errors.iter().any(|error| error.contains("one writer")));
    assert!(errors.iter().any(|error| error.contains("non-stage boundary")), "{errors:?}");
    assert!(errors.iter().any(|error| error.contains("non-lease boundary")));
    assert!(errors.iter().any(|error| error.contains("non-scheduler boundary")));
    assert!(errors.iter().any(|error| error.contains("non-frontier boundary")));
}

#[test]
fn modal_and_contracted_negation_cannot_satisfy_requirements() {
    let text = r#"
## Shift-left claim admission
Before the first delegated mutation, retain a coherent claim and semantic owner.
The lane must not route through prepare-issue. When the earliest falsifier is
unresolved, do not infer facts or mint a candidate.
The acceptance surface must not include acceptance surface, and one writer
isn’t required. Name current authority, source-backed facts and contradictions,
production seam, a cheapest first falsifier and negative control, proof ceiling,
NOT_PROVEN, broader proof is deferred, mutation owner, earliest missing judgment,
next or backward route, read-only research, prepare-proof, and the unresolved
first falsifier. Keep this runtime-local unless it changes durable claim, authority,
or proof state; it is not a stage record, lease, scheduler, or tracked frontier.
## Entry route
Later content.
"#;

    let errors = validate_claim_admission(text);
    assert!(errors.iter().any(|error| error.contains("acceptance surface")), "{errors:?}");
    assert!(errors.iter().any(|error| error.contains("prepare-issue")), "{errors:?}");
    // The refusal is genuine conditional guidance, so modal negation elsewhere
    // in the section must not knock it out.
    assert!(!errors.iter().any(|error| error.contains("candidate refusal")), "{errors:?}");
    assert!(errors.iter().any(|error| error.contains("one writer")), "{errors:?}");
}
