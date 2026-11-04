//! C2 owner-attribution bench (#17154).
//!
//! Each fixture under `benchmarks/ripr/owners/` is a triple: `<stem>.pm` (the
//! head-version source), `<stem>.diff` (a one-hunk unified diff whose `+` lines
//! land at the head positions recorded in its `@@` header), and
//! `<stem>.expected.json` (the staged `lib/` path plus the expected
//! `(kind, qualified_name)` owner — or `null` for the unowned negative).
//!
//! This test stages each fixture alone under a fresh `target/ripr-c2/` root,
//! builds the packet through BOTH the library (`build_ripr_facts_packet` with
//! `diff`) and the `perllsp --ripr-facts --ripr-diff` CLI, and scores
//! attribution precision = correctly-attributed changes / emitted changes.
//!
//! Threshold (C2): overall precision >= 0.98 on EACH leg. Positive fixtures
//! must emit exactly one change pointing at the labeled owner; the negative
//! fixture (`unowned_top_level`) asserts the emitter's documented unowned
//! behavior — NO change fact plus an `unattributable_change` limitation
//! (`changes.rs`: hunks outside every owner are surfaced, never attributed).
//!
//! Fixture honesty: the staged `.pm` is the diff's head side, so every `+`
//! line must appear in it and the `+++ b/` marker must name the staged path;
//! both are asserted at load so a rotting fixture fails here rather than as a
//! mysterious attribution miss.
//!
//! On success each leg prints the per-fixture score table plus a one-line
//! `MATCH` summary so reference baselines capture values, not just pass/fail.
//! The harness captures passing-test output, so baselines must disable
//! capture: `cargo test -p perl-ripr-facts --locked --test owner_attribution
//! -- --nocapture`.

#![deny(clippy::map_err_ignore)]
// Cohort C0 activation (#12598); see src/lib.rs.
// Score output is the bench's baselining contract (cf. E1/A1/P3 MATCH lines).
#![allow(clippy::print_stdout)]

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must_some_with, must_with};

/// C2 bar from #17154: change owner-attribution precision must be >= 98%.
const MIN_PRECISION: f64 = 0.98;

/// Focused classes: C2 scores `changes[]` against `owners[]`; the oracles /
/// relations / boundaries slices only add scan cost and unrelated limitations.
const C2_FACT_CLASSES: &str = "files,owners,changes,limitations,provenance";

/// Labeled C2 fixture stems. `<stem>.pm`, `<stem>.diff`, and
/// `<stem>.expected.json` live in `benchmarks/ripr/owners/`; the `.pm` is
/// staged under `<root>/lib/` (the conventional discovery location).
const FIXTURES: &[&str] = &[
    "sub_owner",
    "package_block_owner",
    "method_owner",
    "second_sub_owner",
    "nested_sub_owner",
    "unowned_top_level",
];

/// One labeled fixture: head source, one-hunk diff, and the expected owner
/// (`None` = the hunk is outside every owner and must stay unattributed).
struct LabeledOwnerFixture {
    stem: String,
    staged_rel: String,
    source: String,
    diff: String,
    expected_owner: Option<(String, String)>,
}

fn c2_fixture_dir() -> String {
    format!("{}/../../benchmarks/ripr/owners", env!("CARGO_MANIFEST_DIR"))
}

/// Load + honesty-check one fixture triple: the `+++ b/` marker must name the
/// staged path and every added line must appear in the staged head source.
fn c2_load_labeled(stem: &str) -> LabeledOwnerFixture {
    let dir = c2_fixture_dir();
    let source = must_with(
        std::fs::read_to_string(format!("{dir}/{stem}.pm")),
        format!("read C2 fixture {stem}.pm"),
    );
    let diff = must_with(
        std::fs::read_to_string(format!("{dir}/{stem}.diff")),
        format!("read C2 fixture {stem}.diff"),
    );
    let expected_raw = must_with(
        std::fs::read_to_string(format!("{dir}/{stem}.expected.json")),
        format!("read C2 expectation {stem}.expected.json"),
    );
    let expected_json: serde_json::Value =
        must_with(serde_json::from_str(&expected_raw), format!("parse {stem}.expected.json"));
    let staged_rel = must_some_with(
        expected_json["staged_path"].as_str(),
        format!("{stem}.expected.json names a staged_path"),
    )
    .to_owned();
    let expected_owner = if expected_json["expected_owner"].is_null() {
        None
    } else {
        let kind = must_some_with(
            expected_json["expected_owner"]["kind"].as_str(),
            format!("{stem}.expected.json names an owner kind"),
        )
        .to_owned();
        let qualified = must_some_with(
            expected_json["expected_owner"]["qualified_name"].as_str(),
            format!("{stem}.expected.json names an owner qualified_name"),
        )
        .to_owned();
        Some((kind, qualified))
    };

    assert!(
        diff.contains(&format!("+++ b/{staged_rel}")),
        "C2 fixture `{stem}`: the diff must touch the staged file `+++ b/{staged_rel}`"
    );
    for added in diff.lines().filter_map(|line| line.strip_prefix('+')) {
        if added.starts_with('+') {
            continue; // the `+++ b/` file marker, not an added line
        }
        assert!(
            source.contains(added),
            "C2 fixture `{stem}`: added line `{added}` must appear in the staged head source"
        );
    }

    LabeledOwnerFixture { stem: stem.to_owned(), staged_rel, source, diff, expected_owner }
}

/// Stage one fixture alone under a pid-namespaced root (#17272 lesson) with a
/// per-leg tag: the lib and CLI tests run in one process (same pid), so the
/// tag keeps their fixture trees disjoint under parallel execution.
fn c2_stage_single(fixture: &LabeledOwnerFixture, leg: &str) -> String {
    let root = format!("target/ripr-c2/{}-{leg}-pid{}", fixture.stem, std::process::id());
    let _ = std::fs::remove_dir_all(&root);
    let staged = format!("{root}/{}", fixture.staged_rel);
    let parent = must_some_with(
        std::path::Path::new(&staged).parent(),
        format!("C2 fixture `{}`: staged file has a parent dir", fixture.stem),
    );
    must_with(
        std::fs::create_dir_all(parent),
        format!("mkdir C2 fixture parent for {}", fixture.stem),
    );
    must_with(
        std::fs::write(&staged, &fixture.source),
        format!("stage C2 fixture {}", fixture.stem),
    );
    root
}

fn c2_library_packet(root: &str, diff: &str, stem: &str) -> serde_json::Value {
    must_with(
        build_ripr_facts_packet(&RiprFactsRequest {
            schema: "ripr-perl-facts-v1",
            root,
            base: Some("origin/main"),
            head: Some("HEAD"),
            fact_classes: C2_FACT_CLASSES,
            diff: Some(diff),
        }),
        format!("assemble C2 packet for staged root {root} ({stem})"),
    )
}

/// Resolve the canonical `perllsp` binary, building it on demand (mirrors
/// `perl-lsp-rs/tests/support.rs`: a stale prebuilt binary must never stand in
/// for current sources; the build result is cached per test process).
fn c2_perllsp_binary() -> String {
    if let Ok(env_bin) = std::env::var("PERL_LSP_BIN") {
        return env_bin;
    }
    static CACHED_BINARY: std::sync::OnceLock<Result<String, String>> = std::sync::OnceLock::new();
    must_with(
        CACHED_BINARY.get_or_init(c2_compile_perllsp_once).clone(),
        "resolve the canonical perllsp test binary",
    )
}

/// Single build-and-resolve attempt for [`c2_perllsp_binary`]. Errors are
/// cached alongside success: a failing build is equally deterministic within
/// one test executable, and retrying it per call only multiplies the cost.
fn c2_compile_perllsp_once() -> Result<String, String> {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("perl-ripr-facts must live below the workspace root")?;
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| workspace.join("target"));
    let binary = target.join("debug").join(if cfg!(windows) { "perllsp.exe" } else { "perllsp" });
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = std::process::Command::new(cargo)
        .current_dir(workspace)
        .args(["build", "-p", "perllsp", "--bin", "perllsp", "--locked"])
        .status()
        .map_err(|error| format!("spawning cargo build for perllsp failed: {error}"))?;
    if !status.success() {
        return Err("building canonical perllsp test binary failed".to_string());
    }
    if !binary.is_file() {
        return Err(format!("canonical perllsp test binary missing: {}", binary.display()));
    }
    Ok(binary.to_string_lossy().into_owned())
}

/// Build the packet through the real `perllsp --ripr-facts --ripr-diff` CLI
/// (e2e style of `cli_smoke.rs`): stage `change.diff`, spawn the binary with
/// the staged root as cwd, and read back `--ripr-out`.
fn c2_cli_packet(root: &str, fixture: &LabeledOwnerFixture) -> serde_json::Value {
    must_with(
        std::fs::write(format!("{root}/change.diff"), &fixture.diff),
        format!("stage C2 diff for {}", fixture.stem),
    );
    let absolute =
        must_with(std::fs::canonicalize(root), format!("canonicalize C2 staged root {root}"));
    let output = must_with(
        std::process::Command::new(c2_perllsp_binary())
            .current_dir(&absolute)
            .args([
                "--ripr-facts",
                "--ripr-root",
                ".",
                "--ripr-diff",
                "change.diff",
                "--ripr-fact-classes",
                C2_FACT_CLASSES,
                "--ripr-out",
                "out.json",
            ])
            .output(),
        format!("spawn perllsp --ripr-facts for C2 fixture {}", fixture.stem),
    );
    assert!(
        output.status.success(),
        "C2 fixture `{}`: perllsp --ripr-facts must exit 0, stderr: {}",
        fixture.stem,
        String::from_utf8_lossy(&output.stderr)
    );
    let packet_raw = must_with(
        std::fs::read_to_string(absolute.join("out.json")),
        format!("read CLI packet for C2 fixture {}", fixture.stem),
    );
    must_with(
        serde_json::from_str(&packet_raw),
        format!("parse CLI packet for C2 fixture {}", fixture.stem),
    )
}

/// Score one fixture's packet: structural asserts plus the emitted/correct
/// attribution counts feeding overall precision. A labeled-but-unresolvable
/// expected owner (e.g. the sub was renamed away) scores a MISS, not a panic,
/// so owner drift fails the benchmark through the precision bar.
fn c2_score_packet(
    packet: &serde_json::Value,
    fixture: &LabeledOwnerFixture,
    leg: &str,
    report: &mut String,
) -> (usize, usize) {
    let stem = fixture.stem.as_str();
    let file_id = format!("file:{}", fixture.staged_rel);
    let changes = must_some_with(
        packet["changes"].as_array(),
        format!("C2 {leg} fixture `{stem}`: packet carries changes[]"),
    );
    // No-orphan guard: the staged root holds exactly one fixture, so every
    // emitted change must belong to its file.
    for (index, change) in changes.iter().enumerate() {
        assert_eq!(
            change["file_id"].as_str(),
            Some(file_id.as_str()),
            "C2 {leg} fixture `{stem}`: change {index} must belong to {file_id}"
        );
    }
    // A supplied diff must clear the no-diff caveat on every fixture.
    let limitation_ids: Vec<&str> = must_some_with(
        packet["limitations"].as_array(),
        format!("C2 {leg} fixture `{stem}`: packet carries limitations[]"),
    )
    .iter()
    .filter_map(|limitation| limitation["limitation_id"].as_str())
    .collect();
    assert!(
        !limitation_ids.contains(&"no-diff-supplied"),
        "C2 {leg} fixture `{stem}`: a supplied diff must clear no-diff-supplied, got {limitation_ids:?}"
    );

    let Some((expected_kind, expected_qualified)) = fixture.expected_owner.as_ref() else {
        // Negative control: the hunk sits outside every owner, so the emitter
        // must surface it as unattributable — never attribute it, never drop it.
        assert!(
            changes.is_empty(),
            "C2 {leg} fixture `{stem}`: unowned hunk must yield zero changes, got {changes:?}"
        );
        let unattributable = must_some_with(
            packet["limitations"].as_array(),
            format!("C2 {leg} fixture `{stem}`: packet carries limitations[]"),
        )
        .iter()
        .find(|limitation| {
            limitation["kind"].as_str() == Some("unattributable_change")
                && limitation["limitation_id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("unattributable-change:"))
        });
        assert!(
            unattributable.is_some(),
            "C2 {leg} fixture `{stem}`: limitations[] must hold an unattributable_change entry, got {limitation_ids:?}"
        );
        report
            .push_str(&format!("{stem} | unowned | zero changes + unattributable_change | n/a\n"));
        return (0, 0);
    };

    // Exactly one hunk in, exactly one change out: extras mean a spurious
    // attribution and zero means a silent drop — both fail louder than the
    // precision fraction could.
    assert_eq!(
        changes.len(),
        1,
        "C2 {leg} fixture `{stem}`: one hunk must yield exactly one change, got {}\n{report}",
        changes.len()
    );
    let actual_owner = must_some_with(
        changes[0]["owner_id"].as_str(),
        format!("C2 {leg} fixture `{stem}`: the change carries an owner_id"),
    )
    .to_owned();
    let owners = must_some_with(
        packet["owners"].as_array(),
        format!("C2 {leg} fixture `{stem}`: packet carries owners[]"),
    );
    let labeled_owner_id = owners.iter().find_map(|owner| {
        let owner_id = owner["owner_id"].as_str()?;
        let wanted_prefix =
            format!("owner:{}:{expected_kind}:{expected_qualified}:", fixture.staged_rel);
        (owner["kind"].as_str() == Some(expected_kind.as_str())
            && owner_id.starts_with(&wanted_prefix))
        .then(|| owner_id.to_owned())
    });
    match labeled_owner_id {
        Some(want) if want == actual_owner => {
            report.push_str(&format!("{stem} | {want} | {actual_owner} | correct\n"));
            (1, 1)
        }
        Some(want) => {
            report.push_str(&format!("{stem} | {want} | {actual_owner} | WRONG OWNER\n"));
            (1, 0)
        }
        None => {
            report.push_str(&format!(
                "{stem} | ({expected_kind}, {expected_qualified}) missing from owners[] | {actual_owner} | MISS\n"
            ));
            (1, 0)
        }
    }
}

/// Drive every fixture through one packet-producing leg and assert the C2 bar.
fn c2_assert_precision_for_leg(
    leg: &str,
    packet_for: &dyn Fn(&str, &LabeledOwnerFixture) -> serde_json::Value,
) {
    let mut emitted = 0usize;
    let mut correct = 0usize;
    let mut report = String::from("fixture | expected owner | actual owner | verdict\n");

    for stem in FIXTURES {
        let fixture = c2_load_labeled(stem);
        let root = c2_stage_single(&fixture, leg);
        let packet = packet_for(&root, &fixture);
        let _ = std::fs::remove_dir_all(&root);
        let (fixture_emitted, fixture_correct) =
            c2_score_packet(&packet, &fixture, leg, &mut report);
        emitted += fixture_emitted;
        correct += fixture_correct;
    }

    #[allow(clippy::cast_precision_loss)]
    let precision = correct as f64 / emitted as f64;
    report.push_str(&format!("overall | | | {correct}/{emitted} = {precision:.3}\n"));
    assert!(
        precision >= MIN_PRECISION,
        "C2 {leg} precision {precision:.3} ({correct}/{emitted}) is below the {MIN_PRECISION} bar\n{report}"
    );
    print!("{report}");
    println!(
        "C2 {leg} | precision={correct}/{emitted} = {precision:.3} bar={MIN_PRECISION} | MATCH"
    );
}

#[test]
fn owner_attribution_library_meets_c2_bar() {
    c2_assert_precision_for_leg("lib", &|root, fixture| {
        c2_library_packet(root, &fixture.diff, &fixture.stem)
    });
}

#[test]
fn owner_attribution_cli_meets_c2_bar() {
    c2_assert_precision_for_leg("cli", &|root, fixture| c2_cli_packet(root, fixture));
}
