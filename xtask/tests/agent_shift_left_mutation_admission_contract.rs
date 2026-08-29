use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SECTION_HEADING: &str = "## Mutation admission";
const PROVIDER_BUILD_SKILLS: &[(&str, &str)] = &[
    ("codex", ".agents/skills/build-candidate/SKILL.md"),
    ("claude", ".claude/skills/build-candidate/SKILL.md"),
];
const REQUIREMENTS: &[(&str, &[&str])] = &[
    (
        "direct and delegated pre-mutation boundary",
        &[
            "before the accountable root edits the candidate directly or delegates any candidate mutation",
            "before the main claude thread edits the candidate directly or delegates any candidate mutation",
        ],
    ),
    ("one admission", &["retain one admission", "keep one admission"]),
    ("semantic key", &["semantic key"]),
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
    (
        "next or backward route",
        &["named next or backward route", "named next/backward route"],
    ),
    ("mechanical key", &["mechanical key"]),
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
    (
        "same-subject join",
        &["must identify the same exact claim/candidate/writer boundary"],
    ),
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
    (
        "fresh mechanical identity",
        &["re-derive or revalidate volatile mechanical identity"],
    ),
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
    (
        "durable exception",
        &["unless it changes durable claim, authority, or proof state"],
    ),
    ("non-stage", &["not a stage record"]),
    ("non-database", &["second work database"]),
    (
        "non-lease/scheduler/frontier",
        &["does not create a lease, scheduler, or tracked frontier"],
    ),
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

    in_section.then(|| lines.join("\n"))
}

fn validate(text: &str) -> Vec<String> {
    let Some(section) = h2_section(text, SECTION_HEADING) else {
        return vec![format!("missing section '{SECTION_HEADING}'")];
    };
    let section = section
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();

    REQUIREMENTS
        .iter()
        .filter_map(|&(label, alternatives)| {
            (!alternatives.iter().any(|&term| section.contains(term))).then(|| {
                format!(
                    "mutation admission is missing {label}; expected one of: {}",
                    alternatives.join(", ")
                )
            })
        })
        .collect()
}

fn fixture() -> String {
    r#"
## Mutation admission
Before the accountable root edits the candidate directly or delegates any candidate
mutation, retain one admission.

### Semantic key
Carry the acceptance-and-rollback claim, semantic owner, governing authority, current
facts and contradictions, production or observable seam, acceptance surface, cheapest
first falsifier, realistic negative control, proof ceiling, explicit `NOT_PROVEN`
boundary, deferred broader proof, and named next or backward route.

### Mechanical key
Carry repository, common-dir, and remote identity; issue and claim identity; candidate
branch; expected head and base; worktree; one writer; intended mutation; required
postcondition; and the writer-preflight/admission decision.

### Same-subject join
Both keys must identify the same exact claim/candidate/writer boundary. Semantic
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
or tracked frontier.

## Procedure
Later content.
"#
    .to_owned()
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

    assert!(errors.iter().any(|error| error.contains("same-subject join")));
    assert!(errors.iter().any(|error| error.contains("pre-mutation refusal")));
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
            .any(|error| error.contains("same-subject join"))
    );
}

#[test]
fn delegated_only_admission_does_not_cover_direct_root_mutation() {
    let text = fixture().replace(
        "Before the accountable root edits the candidate directly or delegates any candidate\nmutation",
        "Before delegating any candidate mutation",
    );

    assert!(
        validate(&text)
            .iter()
            .any(|error| error.contains("direct and delegated pre-mutation"))
    );
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
            .any(|error| error.contains("pre-mutation refusal"))
    );
}

#[test]
fn missing_admission_section_fails_closed() {
    assert_eq!(
        validate("## Procedure\nBuild the candidate."),
        vec![format!("missing section '{SECTION_HEADING}'")]
    );
}
