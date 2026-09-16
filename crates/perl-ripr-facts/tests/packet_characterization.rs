//! Characterization proof for the `ripr-perl-facts-v1` packet (#9271).
//!
//! This module pins the **exact serialized bytes** `build_ripr_facts_packet`
//! produces today, over a representative fixture workspace, before
//! `crates/perl-ripr-facts/src/{lib,emitter}.rs` are split into
//! single-responsibility internal modules. The split (#9271) must be
//! behavior-preserving; these goldens are the falsifier: if the split changes
//! any ID recipe, sort order, fingerprint input, status semantic, or error
//! string, one of these tests goes red.
//!
//! ## What the fixture root is, and why no path normalization is needed
//!
//! `RiprFactsRequest.root` must be a repo-relative path
//! (`validate_ripr_facts_path` rejects absolute paths, `./`, and `..`) — so
//! every fixture here lives under a **fixed, hardcoded** `target/`-relative
//! directory name (e.g. `target/ripr-characterization/no-diff`), never
//! `std::env::temp_dir()`. That means the packet's `root.repo_relative` field,
//! every `file_id`/`owner_id`/`test_id` (which embed the relative path), and
//! the `packet_id` are already host-independent and deterministic — there is
//! no OS temp-dir path embedded in the packet to strip or normalize. Do not
//! change these fixtures to use a system tempdir; that would reintroduce a
//! non-deterministic path into the golden and force exactly the normalization
//! this comment says is unnecessary today.
//!
//! The only other source of non-determinism the packet schema could carry is
//! a timestamp — the schema has none (`packet_fingerprint`/`packet_id` are
//! content-derived), and file `digest`s are SHA-256 over fixed fixture bytes.
//! So the golden comparison below is a **plain, unmodified**
//! `assert_eq!(actual_json, golden_json)` — no field is stripped, redacted, or
//! projected before comparing, which also means the negative control test
//! below (a single perturbed fact) is not accidentally masked by a
//! normalization step.

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use perl_ripr_facts::{RiprFactsError, RiprFactsRequest, build_ripr_facts_packet, run_cli};
use perl_tdd_support::{must, must_some};

const DEFAULT_FACT_CLASSES: &str = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,verify_commands,limitations,provenance";

/// `lib/App.pm`: a package with two subs (`discount`, `risky`), one of which
/// contains an `eval { }` dynamic boundary. 19 lines, 0-based line numbers
/// annotated for the diff hunks in [`CHARACTERIZATION_DIFF`] below:
///
/// ```text
/// 0  use strict;
/// 1  use warnings;
/// 2
/// 3  package App;
/// 4
/// 5  sub discount {
/// 6      my ($amount) = @_;
/// 7      if ($amount > 100) {
/// 8          return $amount * 0.9;
/// 9      }
/// 10     return $amount;
/// 11 }
/// 12
/// 13 sub risky {
/// 14     eval { die "boom" };
/// 15     return 1;
/// 16 }
/// 17
/// 18 1;
/// ```
const APP_PM: &str = "use strict;\nuse warnings;\n\npackage App;\n\nsub discount {\n    my ($amount) = @_;\n    if ($amount > 100) {\n        return $amount * 0.9;\n    }\n    return $amount;\n}\n\nsub risky {\n    eval { die \"boom\" };\n    return 1;\n}\n\n1;\n";

/// `lib/Other.pm`: a second package, referenced only by a bare (unqualified)
/// call from `t/other.t` — exercises the conservative `file_proximity`
/// relation branch (a bare call after `use` is not provably a direct call).
const OTHER_PM: &str = "package Other;\n\nsub other_sub {\n    return 1;\n}\n\n1;\n";

/// `t/app.t`: Test::More file with three oracle kinds (`is`, `ok`, `like`),
/// both calls package-qualified (`App::discount`, `App::risky`) so the
/// relation emitter proves two `direct_owner_call` relations.
const APP_T: &str = "use strict;\nuse warnings;\nuse Test::More;\n\nuse App;\n\nis(App::discount(50), 50, 'no discount under threshold');\nis(App::discount(200), 180, 'discount applied above threshold');\nok(App::risky(), 'risky returns true');\nlike(App::discount(200), qr/^\\d+(\\.\\d+)?$/, 'discount looks numeric');\n\ndone_testing;\n";

/// `t/other.t`: bare (unqualified) call after `use Other;` — the
/// `file_proximity` relation branch.
const OTHER_T: &str = "use strict;\nuse warnings;\nuse Test::More;\n\nuse Other;\n\nok(other_sub(), 'bare call after use');\n\ndone_testing;\n";

/// A unified diff exercising every `changes[]` limitation branch in one pass:
/// - hunk 1 (lines 0-0, before `package App;`): outside every owner →
///   `unattributable-change`.
/// - hunk 2 (inside `sub discount`, lines 8-8): a valid `predicate_boundary`
///   change.
/// - hunk 3 (inside `sub risky`, lines 14-15): a valid `exception_path`
///   change whose first added line (`eval { warn 'test' };`) also trips the
///   `eval_or_string_code` dynamic boundary → `diff-dynamic-boundary`.
/// - hunk 4 (`lib/Missing.pm`, a path the fixture never creates): →
///   `diff-file-not-found`.
///
/// The diff is opaque caller-supplied text (`build_ripr_facts_packet` never
/// runs git or reads these files to verify it) — only the head-file line
/// numbers need to line up with the real fixture file's owner ranges to steer
/// which branch each hunk lands in; the *added-line content* is free text
/// chosen to hit a specific `behavior_hint`/boundary pattern.
const CHARACTERIZATION_DIFF: &str = "+++ b/lib/App.pm\n@@ -1,1 +1,2 @@\n use strict;\n+use POSIX;\n@@ -8,1 +9,2 @@\n         return $amount * 0.9;\n+    if ($amount >= 500) {\n@@ -14,1 +15,3 @@\n     eval { die \"boom\" };\n+    eval { warn 'test' };\n+    die \"boom\" if $x < 0;\n+++ b/lib/Missing.pm\n@@ -1,1 +1,2 @@\n package Missing;\n+sub gone { 1 }\n";

/// Write the two-package, two-test fixture under `root` (created fresh).
fn write_fixture(root: &str) -> std::io::Result<()> {
    let _ = std::fs::remove_dir_all(root);
    std::fs::create_dir_all(format!("{root}/lib"))?;
    std::fs::create_dir_all(format!("{root}/t"))?;
    std::fs::write(format!("{root}/lib/App.pm"), APP_PM)?;
    std::fs::write(format!("{root}/lib/Other.pm"), OTHER_PM)?;
    std::fs::write(format!("{root}/t/app.t"), APP_T)?;
    std::fs::write(format!("{root}/t/other.t"), OTHER_T)?;
    Ok(())
}

fn cleanup(root: &str) {
    let _ = std::fs::remove_dir_all(root);
}

/// Build the packet over the fixture at `root`, requesting every fact class,
/// with `diff` forwarded verbatim (`None` in the no-diff golden).
fn build_fixture_packet(root: &str, diff: Option<&str>) -> serde_json::Value {
    must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: Some("origin/main"),
        head: Some("HEAD"),
        fact_classes: DEFAULT_FACT_CLASSES,
        diff,
    }))
}

#[test]
fn packet_characterization_no_diff_matches_golden() -> std::io::Result<()> {
    let root = "target/ripr-characterization/no-diff";
    write_fixture(root)?;
    let packet = build_fixture_packet(root, None);
    cleanup(root);

    let actual = must(serde_json::to_string_pretty(&packet));
    let golden = include_str!("golden/ripr_facts_v1_no_diff.json");
    assert_eq!(
        actual,
        golden.trim_end_matches('\n'),
        "no-diff packet drifted from the pre-split golden"
    );

    assert_eq!(
        must_some(packet["packet_fingerprint"].as_str()),
        "sha256:32e2d25ccdd806bf479d9953065844a1c1faa3f1772233315fed5fb13598328c",
        "packet_fingerprint literal drifted from the pre-split golden"
    );
    Ok(())
}

#[test]
fn packet_characterization_with_diff_matches_golden() -> std::io::Result<()> {
    let root = "target/ripr-characterization/with-diff";
    write_fixture(root)?;
    let packet = build_fixture_packet(root, Some(CHARACTERIZATION_DIFF));
    cleanup(root);

    let actual = must(serde_json::to_string_pretty(&packet));
    let golden = include_str!("golden/ripr_facts_v1_with_diff.json");
    assert_eq!(
        actual,
        golden.trim_end_matches('\n'),
        "with-diff packet drifted from the pre-split golden"
    );

    assert_eq!(
        must_some(packet["packet_fingerprint"].as_str()),
        "sha256:8fc37c160dc0bb48ce47c7eb4544202a5413ccce1b0f3c5c0456b176820ea3dd",
        "packet_fingerprint literal drifted from the pre-split golden"
    );
    Ok(())
}

/// Negative control: proves the golden comparison above is not vacuous. A
/// deliberately perturbed clone of the no-diff packet (one extra fact
/// appended to `files[]`) must NOT equal the golden bytes. If this test ever
/// passes with `assert_ne!` failing (i.e. the perturbed packet matches), the
/// golden comparison above is not discriminating anything.
#[test]
fn perturbed_packet_does_not_match_golden() -> std::io::Result<()> {
    let root = "target/ripr-characterization/perturb";
    write_fixture(root)?;
    let mut packet = build_fixture_packet(root, None);
    cleanup(root);

    let files = must_some(packet["files"].as_array_mut());
    files.push(serde_json::json!({
        "file_id": "file:lib/Injected.pm",
        "path": "lib/Injected.pm",
        "role": ["source"],
        "digest": "sha256:0000000000000000000000000000000000000000000000000000000000000",
        "package_names": [],
        "provenance_refs": [],
    }));

    let perturbed = must(serde_json::to_string_pretty(&packet));
    let golden = include_str!("golden/ripr_facts_v1_no_diff.json");
    assert_ne!(
        perturbed,
        golden.trim_end_matches('\n'),
        "a packet with an injected extra file fact must not match the golden — \
         if it does, the golden comparison is vacuous"
    );

    // Same control over ordering: reversing `owners[]` must also not match.
    let mut reordered = build_fixture_packet(root, None);
    let owners = must_some(reordered["owners"].as_array_mut());
    owners.reverse();
    if owners.len() > 1 {
        let reordered_json = must(serde_json::to_string_pretty(&reordered));
        assert_ne!(
            reordered_json,
            golden.trim_end_matches('\n'),
            "a packet with reordered owners[] must not match the golden"
        );
    }
    Ok(())
}

// ── Malformed-input / error-path characterization ──
//
// Pins `RiprFactsError` variant shape/equality and `run_cli` exit codes —
// separate from the packet-byte goldens above because these assert on typed
// error values and process exit codes, not on serialized JSON.

#[test]
fn build_packet_rejects_unsupported_schema_with_typed_error() {
    let error = must_some(
        build_ripr_facts_packet(&RiprFactsRequest {
            schema: "ripr-perl-facts-v2",
            root: ".",
            base: None,
            head: None,
            fact_classes: "files",
            diff: None,
        })
        .err(),
    );
    assert_eq!(
        error,
        RiprFactsError::UnsupportedSchema { schema: "ripr-perl-facts-v2".to_string() }
    );
    assert_eq!(
        error.to_string(),
        "unsupported schema `ripr-perl-facts-v2`; expected `ripr-perl-facts-v1`"
    );
}

#[test]
fn build_packet_rejects_invalid_root_with_typed_error() {
    let error = must_some(
        build_ripr_facts_packet(&RiprFactsRequest {
            schema: "ripr-perl-facts-v1",
            root: "/abs/path",
            base: None,
            head: None,
            fact_classes: "files",
            diff: None,
        })
        .err(),
    );
    assert!(matches!(error, RiprFactsError::InvalidRoot(_)));
}

#[test]
fn build_packet_rejects_unknown_fact_class_with_typed_error() {
    let error = must_some(
        build_ripr_facts_packet(&RiprFactsRequest {
            schema: "ripr-perl-facts-v1",
            root: ".",
            base: None,
            head: None,
            fact_classes: "not_a_real_class",
            diff: None,
        })
        .err(),
    );
    assert!(matches!(error, RiprFactsError::InvalidFactClasses(_)));
}

#[test]
fn run_cli_exits_nonzero_on_missing_subcommand() {
    let rc = run_cli(vec!["perl-ripr-facts".to_string()]);
    assert_eq!(rc, 1, "missing `ripr-facts` subcommand must exit 1");
}

#[test]
fn run_cli_exits_nonzero_on_unknown_flag() {
    let rc = run_cli(vec![
        "perl-ripr-facts".to_string(),
        "ripr-facts".to_string(),
        "--not-a-flag".to_string(),
        "x".to_string(),
    ]);
    assert_eq!(rc, 1, "an unknown flag must exit 1");
}

#[test]
fn run_cli_exits_nonzero_on_missing_diff_file() {
    let root = "target/ripr-characterization/missing-diff";
    let _ = std::fs::remove_dir_all(root);
    must(std::fs::create_dir_all(root));
    let rc = run_cli(vec![
        "perl-ripr-facts".to_string(),
        "ripr-facts".to_string(),
        "--schema".to_string(),
        "ripr-perl-facts-v1".to_string(),
        "--root".to_string(),
        root.to_string(),
        "--diff".to_string(),
        format!("{root}/does-not-exist.diff"),
        "--out".to_string(),
        format!("{root}/out.json"),
    ]);
    cleanup(root);
    assert_eq!(rc, 1, "a --diff path that does not exist must exit 1, not panic");
}

#[test]
fn run_cli_exits_zero_and_writes_partial_packet_on_valid_invocation() -> std::io::Result<()> {
    let root = "target/ripr-characterization/cli-success";
    write_fixture(root)?;
    let out = format!("{root}/out.json");
    let rc = run_cli(vec![
        "perl-ripr-facts".to_string(),
        "ripr-facts".to_string(),
        "--schema".to_string(),
        "ripr-perl-facts-v1".to_string(),
        "--root".to_string(),
        root.to_string(),
        "--fact-classes".to_string(),
        "files,owners".to_string(),
        "--out".to_string(),
        out.clone(),
    ]);
    assert_eq!(rc, 0);
    let written: serde_json::Value = serde_json::from_slice(&std::fs::read(&out)?)?;
    assert_eq!(written["packet_status"], serde_json::json!("partial"));
    cleanup(root);
    Ok(())
}
