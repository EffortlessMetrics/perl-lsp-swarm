use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SECTION_HEADING: &str = "## Mutation admission";
const SEMANTIC_HEADING: &str = "### Semantic key";
const MECHANICAL_HEADING: &str = "### Mechanical key";
const JOIN_HEADING: &str = "### Same-subject join";

const PROVIDER_BUILD_SKILLS: &[(&str, &str)] = &[
    ("codex", ".agents/skills/build-candidate/SKILL.md"),
    ("claude", ".claude/skills/build-candidate/SKILL.md"),
];

const PREAMBLE_REQUIREMENTS: &[(&str, &[&str])] = &[
    (
        "direct and delegated pre-mutation boundary",
        &[
            "before the accountable root edits the candidate directly or delegates any candidate mutation",
            "before the main claude thread edits the candidate directly or delegates any candidate mutation",
        ],
    ),
    ("one admission", &["retain one admission", "keep one admission"]),
];

const SEMANTIC_REQUIREMENTS: &[(&str, &[&str])] = &[
    ("claim", &["acceptance-and-rollback claim"]),
    ("semantic owner", &["semantic owner"]),
    ("authority", &["governing authority"]),
    ("facts and contradictions", &["current facts and contradictions"]),
    ("production seam", &["production or observable seam"]),
    ("acceptance surface", &["acceptance surface"]),
    ("first falsifier", &["cheapest first falsifier"]),
    ("negative control", &["realistic negative control"]),
    ("proof ceiling", &["proof ceiling"]),
    ("NOT_PROVEN boundary", &["explicit `not_proven` boundary"]),
    ("deferred proof", &["deferred broader proof"]),
    ("next or backward route", &["named next or backward route", "named next/backward route"]),
];

const MECHANICAL_REQUIREMENTS: &[(&str, &[&str])] = &[
    ("repository identity", &["repository, common-dir, and remote identity"]),
    ("issue and claim identity", &["issue and claim identity"]),
    ("candidate branch", &["candidate branch"]),
    ("expected head and base", &["expected head and base"]),
    ("worktree", &["worktree"]),
    ("one writer", &["one writer"]),
    ("intended mutation", &["intended mutation"]),
    ("postcondition", &["required postcondition"]),
    (
        "writer preflight",
        &["writer-preflight/admission decision", "writer admission/preflight decision"],
    ),
];

const JOIN_REQUIREMENTS: &[(&str, &[&str])] = &[
    ("same-subject join", &["must identify the same exact claim/candidate/writer boundary"]),
    ("semantic/mechanical separation", &["does not establish mechanical safety"]),
    (
        "mechanical/semantic separation",
        &["does not establish authority to implement another claim"],
    ),
    (
        "direct and delegated join",
        &[
            "direct root edits and delegated writer edits use the same join",
            "direct main-thread edits and delegated writer edits use the same join",
        ],
    ),
    ("midstream coverage", &["entry midstream does not bypass admission"]),
    ("read-only precursor", &["read-only research may precede admission"]),
    ("fresh mechanical identity", &["re-derive or revalidate volatile mechanical identity"]),
    (
        "pre-mutation refusal",
        &["do not mutate when either key is missing, stale, contradictory, or cross-subject"],
    ),
    ("anti-inference", &["do not infer either key"]),
    ("second-candidate refusal", &["mint a second candidate"]),
    ("prepare-issue route", &["prepare-issue"]),
    ("prepare-proof route", &["prepare-proof"]),
    ("writer-admission route", &["routes to writer admission/preflight"]),
    ("writer collision", &["writer_collision"]),
    ("unsafe worktree", &["unsafe_worktree"]),
    ("blocked", &["`blocked`"]),
    ("not proven", &["`not_proven`"]),
    ("runtime-local", &["runtime-local"]),
    ("durable exception", &["unless it changes durable claim, authority, or proof state"]),
    ("non-stage", &["not a stage record"]),
    ("non-database", &["second work database"]),
    ("non-lease/scheduler/frontier", &["does not create a lease, scheduler, or tracked frontier"]),
];

const SEMANTIC_FIXTURE_BODY: &str = r#"Carry the acceptance-and-rollback claim, semantic owner, governing authority, current
facts and contradictions, production or observable seam, acceptance surface, cheapest
first falsifier, realistic negative control, proof ceiling, explicit `NOT_PROVEN`
boundary, deferred broader proof, and named next or backward route."#;

const MECHANICAL_FIXTURE_BODY: &str = r#"Carry repository, common-dir, and remote identity; issue and claim identity; candidate
branch; expected head and base; worktree; one writer; intended mutation; required
postcondition; and the writer-preflight/admission decision."#;

const JOIN_FIXTURE_BODY: &str = r#"Both keys must identify the same exact claim/candidate/writer boundary. Semantic
authority does not establish mechanical safety. Mechanical safety does not establish
authority to implement another claim. Direct root edits and delegated writer edits use
the same join, and entry midstream does not bypass admission.

Read-only research may precede admission. Re-derive or revalidate volatile mechanical
identity. Do not mutate when either key is missing, stale, contradictory, or
cross-subject. Do not infer either key or mint a second candidate.

Changed scope routes to `prepare-issue`; weak proof routes to `prepare-proof`; missing
or stale mechanical evidence routes to writer admission/preflight. Refuse with
`WRITER_COLLISION`, `UNSAFE_WORKTREE`, `BLOCKED`, or `NOT_PROVEN` as applicable.

Keep this runtime-local unless it changes durable claim, authority, or proof state. It
is not a stage record or second work database, and does not create a lease, scheduler,
or tracked frontier."#;

fn repo_root() -> io::Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest directory has no repository parent"))
}

fn is_h2_heading(trimmed: &str) -> bool {
    trimmed.starts_with("## ") && !trimmed.starts_with("### ")
}

fn is_h3_heading(trimmed: &str) -> bool {
    trimmed.starts_with("### ")
}

/// Classify each line as prose or not. A line is prose when it is outside
/// fenced code blocks and is not indented code. Fence recognition follows the
/// CommonMark fence shapes used in admission docs: three or more backticks or
/// tildes with up to three leading spaces, closing fences carrying no info
/// string. Tab stops expand to four columns for indented-code detection. This
/// is a documented approximation for flat admission prose, not a full
/// CommonMark parser: it only narrows which lines count as headings, and
/// section bodies still include code-block text.
fn prose_flags(text: &str) -> Vec<bool> {
    let mut flags = Vec::with_capacity(text.lines().count());
    let mut fence: Option<(char, usize)> = None;

    for line in text.lines() {
        let is_prose = match fence {
            Some((marker, run)) => {
                if closes_fence(line, marker, run) {
                    fence = None;
                }
                false
            }
            None => match fence_delimiter(line) {
                Some((marker, run)) => {
                    fence = Some((marker, run));
                    false
                }
                None => leading_indent(line) < 4,
            },
        };
        flags.push(is_prose);
    }

    flags
}

fn leading_indent(line: &str) -> usize {
    let mut columns = 0;
    for ch in line.chars() {
        match ch {
            ' ' => columns += 1,
            '\t' => columns += 4 - (columns % 4),
            _ => break,
        }
    }
    columns
}

fn fence_delimiter(line: &str) -> Option<(char, usize)> {
    if leading_indent(line) > 3 {
        return None;
    }
    let body = line.trim_start();
    let marker = body.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let run = body.chars().take_while(|&ch| ch == marker).count();
    if run < 3 {
        return None;
    }
    if body[run..].trim().contains(marker) {
        return None;
    }
    Some((marker, run))
}

fn closes_fence(line: &str, marker: char, run: usize) -> bool {
    if leading_indent(line) > 3 {
        return false;
    }
    let body = line.trim_start();
    let closing_run = body.chars().take_while(|&ch| ch == marker).count();
    closing_run >= run && body[closing_run..].trim().is_empty()
}

fn h2_section(text: &str, heading: &str) -> Option<String> {
    let flags = prose_flags(text);
    let mut in_section = false;
    let mut lines = Vec::new();

    for (line, prose) in text.lines().zip(flags) {
        let trimmed = line.trim();
        if prose && is_h2_heading(trimmed) {
            if trimmed.eq_ignore_ascii_case(heading) {
                // A repeated section heading is treated as continuation here;
                // validate() reports duplicate occurrences explicitly so the
                // concatenated content cannot mask them.
                in_section = true;
            } else if in_section {
                break;
            }
            continue;
        }
        if in_section {
            lines.push(line);
        }
    }

    in_section.then(|| lines.join("\n"))
}

fn h2_headings(text: &str) -> Vec<&str> {
    let flags = prose_flags(text);
    text.lines()
        .zip(flags)
        .filter_map(|(line, prose)| prose.then_some(line.trim()))
        .filter(|trimmed| is_h2_heading(trimmed))
        .filter_map(|trimmed| trimmed.strip_prefix("## "))
        .collect()
}

fn h2_heading_count(text: &str, heading: &str) -> usize {
    let flags = prose_flags(text);
    text.lines()
        .zip(flags)
        .filter(|&(line, prose)| {
            prose && is_h2_heading(line.trim()) && line.trim().eq_ignore_ascii_case(heading)
        })
        .count()
}

fn h3_headings(text: &str) -> Vec<&str> {
    let flags = prose_flags(text);
    text.lines()
        .zip(flags)
        .filter_map(|(line, prose)| prose.then_some(line.trim()))
        .filter(|trimmed| is_h3_heading(trimmed))
        .filter_map(|trimmed| trimmed.strip_prefix("### "))
        .collect()
}

fn h3_heading_count(text: &str, heading: &str) -> usize {
    let flags = prose_flags(text);
    text.lines()
        .zip(flags)
        .filter(|&(line, prose)| {
            prose && is_h3_heading(line.trim()) && line.trim().eq_ignore_ascii_case(heading)
        })
        .count()
}

// Body attribution: when a required subsection heading is absent, its intended
// body is attributed to the preceding subsection. That cannot produce a false
// green: validate_subsection hard-fails on a heading count of zero before any
// marker evaluation. Only real Markdown headings (never fenced or indented
// code) open, close, or bound a subsection.
fn h3_section(text: &str, heading: &str) -> Option<String> {
    let flags = prose_flags(text);
    let mut found = false;
    let mut in_section = false;
    let mut lines = Vec::new();

    for (line, prose) in text.lines().zip(flags) {
        let trimmed = line.trim();

        if prose && is_h3_heading(trimmed) {
            if in_section {
                break;
            }
            if !found && trimmed.eq_ignore_ascii_case(heading) {
                found = true;
                in_section = true;
            }
            continue;
        }

        if in_section {
            lines.push(line);
        }
    }

    found.then(|| lines.join("\n"))
}

fn h3_preamble(text: &str) -> String {
    let flags = prose_flags(text);
    text.lines()
        .zip(flags)
        .take_while(|(line, prose)| !(*prose && is_h3_heading(line.trim())))
        .map(|(line, _)| line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn validate_requirements(scope: &str, text: &str, requirements: &[(&str, &[&str])]) -> Vec<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();

    requirements
        .iter()
        .filter(|&(_, alternatives)| {
            !alternatives.iter().any(|&term| normalized.contains(&term.to_ascii_lowercase()))
        })
        .map(|&(label, alternatives)| {
            format!("{scope} is missing {label}; expected one of: {}", alternatives.join(", "))
        })
        .collect()
}

fn validate_subsection(
    section: &str,
    heading: &str,
    scope: &str,
    requirements: &[(&str, &[&str])],
    found_headings: &str,
) -> Vec<String> {
    match h3_heading_count(section, heading) {
        0 => vec![format!(
            "missing subsection '{heading}' inside '{SECTION_HEADING}'; found h3 headings: [{found_headings}]"
        )],
        1 => h3_section(section, heading)
            .map(|subsection| validate_requirements(scope, &subsection, requirements))
            .unwrap_or_else(|| {
                vec![format!("could not read subsection '{heading}' inside '{SECTION_HEADING}'")]
            }),
        count => vec![format!(
            "subsection '{heading}' occurs {count} times inside '{SECTION_HEADING}'; it must occur exactly once; found h3 headings: [{found_headings}]"
        )],
    }
}

fn validate(text: &str) -> Vec<String> {
    let section_count = h2_heading_count(text, SECTION_HEADING);
    if section_count == 0 {
        let found = h2_headings(text).join("; ");
        return vec![format!("missing section '{SECTION_HEADING}'; found h2 headings: [{found}]")];
    }

    let Some(section) = h2_section(text, SECTION_HEADING) else {
        return vec![format!("could not read section '{SECTION_HEADING}'")];
    };

    let mut failures = Vec::new();
    if section_count > 1 {
        failures.push(format!(
            "section '{SECTION_HEADING}' occurs {section_count} times; it must occur exactly once"
        ));
    }

    let found_headings = h3_headings(&section).join("; ");
    failures.extend(validate_requirements(
        "mutation admission preamble",
        &h3_preamble(&section),
        PREAMBLE_REQUIREMENTS,
    ));

    for (heading, scope, requirements) in [
        (SEMANTIC_HEADING, "semantic key", SEMANTIC_REQUIREMENTS),
        (MECHANICAL_HEADING, "mechanical key", MECHANICAL_REQUIREMENTS),
        (JOIN_HEADING, "same-subject join", JOIN_REQUIREMENTS),
    ] {
        failures.extend(validate_subsection(
            &section,
            heading,
            scope,
            requirements,
            &found_headings,
        ));
    }

    failures
}

fn fixture_with_sections(semantic: &str, mechanical: &str, join: &str) -> String {
    format!(
        r#"
## Mutation admission
Before the accountable root edits the candidate directly or delegates any candidate
mutation, retain one admission.

### Semantic key
{semantic}

### Mechanical key
{mechanical}

### Same-subject join
{join}

## Procedure
Later content.
"#
    )
}

fn fixture() -> String {
    fixture_with_sections(SEMANTIC_FIXTURE_BODY, MECHANICAL_FIXTURE_BODY, JOIN_FIXTURE_BODY)
}

#[test]
fn provider_build_skills_join_semantic_and_mechanical_admission() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let mut failures = Vec::new();

    for &(provider, relative_path) in PROVIDER_BUILD_SKILLS {
        let path = root.join(relative_path);
        let text = fs::read_to_string(&path)?;
        for error in validate(&text) {
            failures.push(format!("{provider} ({}): {error}", path.display()));
        }
    }

    assert!(
        failures.is_empty(),
        "provider-native mutation admission contract failed:\n{}",
        failures.join("\n")
    );
    Ok(())
}

#[test]
fn provider_wording_may_differ_without_changing_the_contract() {
    let text = fixture()
        .replace("the accountable root", "the main Claude thread")
        .replace("retain one admission", "keep one admission")
        .replace("Direct root edits", "Direct main-thread edits")
        .replace("writer-preflight/admission", "writer admission/preflight")
        .replace("named next or backward route", "named next/backward route");

    assert!(validate(&text).is_empty());
}

#[test]
fn markers_outside_the_admission_section_do_not_count() {
    let decoy = fixture().replacen(SECTION_HEADING, "## Decoy admission", 1);
    let text = format!("{decoy}\n{SECTION_HEADING}\nretain one admission\n\n## Procedure\n");
    let errors = validate(&text);

    assert!(errors.iter().any(|error| error.contains("missing subsection '### Semantic key'")));
    assert!(errors.iter().any(|error| error.contains("missing subsection '### Mechanical key'")));
    assert!(
        errors.iter().any(|error| error.contains("missing subsection '### Same-subject join'"))
    );
}

#[test]
fn two_keys_without_a_same_subject_join_fail_closed() {
    let text = fixture().replace(
        "Both keys must identify the same exact claim/candidate/writer boundary.",
        "Both keys are present.",
    );

    assert!(
        validate(&text)
            .iter()
            .any(|error| error.contains("same-subject join is missing same-subject join"))
    );
}

#[test]
fn delegated_only_admission_does_not_cover_direct_root_mutation() {
    let text = fixture().replace(
        "Before the accountable root edits the candidate directly or delegates any candidate\nmutation",
        "Before delegating any candidate mutation",
    );

    assert!(validate(&text).iter().any(|error| {
        error.contains("mutation admission preamble is missing direct and delegated pre-mutation")
    }));
}

#[test]
fn cross_subject_input_without_pre_mutation_refusal_fails_closed() {
    let text = fixture().replace(
        "Do not mutate when either key is missing, stale, contradictory, or\ncross-subject.",
        "A cross-subject key is recorded after mutation.",
    );

    assert!(
        validate(&text)
            .iter()
            .any(|error| error.contains("same-subject join is missing pre-mutation refusal"))
    );
}

#[test]
fn swapped_semantic_and_mechanical_bodies_fail_closed() {
    let text =
        fixture_with_sections(MECHANICAL_FIXTURE_BODY, SEMANTIC_FIXTURE_BODY, JOIN_FIXTURE_BODY);
    let errors = validate(&text);

    assert!(errors.iter().any(|error| error.contains("semantic key is missing claim")));
    assert!(
        errors.iter().any(|error| error.contains("mechanical key is missing repository identity"))
    );
}

#[test]
fn marker_in_the_wrong_key_does_not_count() {
    let semantic = format!("{SEMANTIC_FIXTURE_BODY}\n\nOne writer appears only under this key.");
    let mechanical = MECHANICAL_FIXTURE_BODY.replace("one writer; ", "");
    let text = fixture_with_sections(&semantic, &mechanical, JOIN_FIXTURE_BODY);

    assert!(
        validate(&text).iter().any(|error| error.contains("mechanical key is missing one writer"))
    );
}

#[test]
fn accepted_claim_in_the_mechanical_key_does_not_count() {
    let semantic = SEMANTIC_FIXTURE_BODY.replace("acceptance-and-rollback claim, ", "");
    let mechanical = format!(
        "{MECHANICAL_FIXTURE_BODY}\n\nThe acceptance-and-rollback claim appears here only."
    );
    let text = fixture_with_sections(&semantic, &mechanical, JOIN_FIXTURE_BODY);

    assert!(validate(&text).iter().any(|error| error.contains("semantic key is missing claim")));
}

#[test]
fn missing_required_subsection_fails_even_when_its_markers_exist_elsewhere() {
    let text = format!(
        r#"
## Mutation admission
Before the accountable root edits the candidate directly or delegates any candidate
mutation, retain one admission.

### Semantic key
{SEMANTIC_FIXTURE_BODY}

{MECHANICAL_FIXTURE_BODY}

### Same-subject join
{JOIN_FIXTURE_BODY}

## Procedure
Later content.
"#
    );
    let errors = validate(&text);

    assert!(errors.iter().any(|error| error.contains("missing subsection '### Mechanical key'")));
    assert!(
        errors
            .iter()
            .any(|error| error.contains("found h3 headings: [Semantic key; Same-subject join]"))
    );
}

#[test]
fn duplicated_required_subsection_fails_closed() {
    let text = format!(
        r#"
## Mutation admission
Before the accountable root edits the candidate directly or delegates any candidate
mutation, retain one admission.

### Semantic key
{SEMANTIC_FIXTURE_BODY}

### Mechanical key
{MECHANICAL_FIXTURE_BODY}

### Mechanical key
{MECHANICAL_FIXTURE_BODY}

### Same-subject join
{JOIN_FIXTURE_BODY}

## Procedure
Later content.
"#
    );
    let errors = validate(&text);

    assert!(
        errors.iter().any(|error| error.contains("subsection '### Mechanical key' occurs 2 times"))
    );
}

#[test]
fn missing_admission_section_fails_closed() {
    let errors = validate("## Procedure\nBuild the candidate.");

    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].starts_with(&format!("missing section '{SECTION_HEADING}'")),
        "unexpected error: {}",
        errors[0]
    );
    assert!(
        errors[0].contains("found h2 headings: [Procedure]"),
        "diagnostics must point at the heading actually found: {}",
        errors[0]
    );
}

#[test]
fn duplicate_admission_sections_fail_closed_with_duplicate_diagnostic() {
    let text = format!(
        r#"
## Mutation admission
Before the accountable root edits the candidate directly or delegates any candidate
mutation, retain one admission.

### Semantic key
{SEMANTIC_FIXTURE_BODY}

## Mutation admission

### Mechanical key
{MECHANICAL_FIXTURE_BODY}

### Same-subject join
{JOIN_FIXTURE_BODY}

## Procedure
Later content.
"#
    );
    let errors = validate(&text);

    assert_eq!(errors.len(), 1, "unexpected errors: {:?}", errors);
    assert_eq!(
        errors[0],
        "section '## Mutation admission' occurs 2 times; it must occur exactly once"
    );
}

#[test]
fn headings_and_markers_only_inside_fenced_code_fail_closed() {
    let text = format!(
        r#"
## Mutation admission
Before the accountable root edits the candidate directly or delegates any candidate
mutation, retain one admission.

```
### Semantic key
{SEMANTIC_FIXTURE_BODY}

### Mechanical key
{MECHANICAL_FIXTURE_BODY}

### Same-subject join
{JOIN_FIXTURE_BODY}
```

## Procedure
Later content.
"#
    );
    let errors = validate(&text);

    assert!(errors.iter().any(|error| error.contains("missing subsection '### Semantic key'")));
    assert!(errors.iter().any(|error| error.contains("missing subsection '### Mechanical key'")));
    assert!(
        errors.iter().any(|error| error.contains("missing subsection '### Same-subject join'"))
    );
}

#[test]
fn headings_and_markers_only_inside_indented_code_fail_closed() {
    let text = format!(
        r#"
## Mutation admission
Before the accountable root edits the candidate directly or delegates any candidate
mutation, retain one admission.

    ### Semantic key
    {SEMANTIC_FIXTURE_BODY}

    ### Mechanical key
    {MECHANICAL_FIXTURE_BODY}

    ### Same-subject join
    {JOIN_FIXTURE_BODY}

## Procedure
Later content.
"#
    );
    let errors = validate(&text);

    assert!(errors.iter().any(|error| error.contains("missing subsection '### Semantic key'")));
    assert!(errors.iter().any(|error| error.contains("missing subsection '### Mechanical key'")));
    assert!(
        errors.iter().any(|error| error.contains("missing subsection '### Same-subject join'"))
    );
}
