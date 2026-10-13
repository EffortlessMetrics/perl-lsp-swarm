//! Contract tests for the exact-source Eglot registration patch (#13613).
//!
//! The packet is repository preparation only. Tests never contact or mutate
//! upstream and cannot promote source, accepted, released, or host evidence.

#![expect(clippy::expect_used)]

use std::fs;
use std::process::Command;

use xtask::emacs_eglot_upstream_patch::{
    AFTER_ANCHOR, BASE_BLOB_SHA1, BASE_COMMIT, BASE_PATH, BASE_TREE_SHA1, BEFORE_ANCHOR,
    UNIFIED_DIFF, checked_packet, render_checked_json,
};

fn source_fixture() -> String {
    format!(
        "(defcustom eglot-server-programs\n  '(\n{BEFORE_ANCHOR}    (markdown-mode . (\"marksman\")))\n  \"fixture\")\n"
    )
}

/// Declared and payload-derived shape of the single hunk in `UNIFIED_DIFF`.
struct HunkShape {
    start_old: u32,
    old_count: u32,
    start_new: u32,
    new_count: u32,
    context_before: u32,
    context_after: u32,
    removed: u32,
    added: u32,
}

/// Parse the single hunk of a prepared unified diff into its declared header
/// ranges and its actual payload line classes.
fn hunk_shape(diff: &str) -> HunkShape {
    let mut lines = diff.lines();
    assert!(lines.next().expect("diff file header").starts_with("--- "));
    assert!(lines.next().expect("diff file header").starts_with("+++ "));
    let header = lines.next().expect("diff hunk header");
    assert!(header.starts_with("@@ -") && header.ends_with(" @@"), "{header}");
    let ranges = header.trim_start_matches("@@ -").trim_end_matches(" @@");
    let (old_range, new_range) = ranges.split_once(' ').expect("hunk old/new ranges");
    let (start_old, old_count) = old_range.split_once(',').expect("hunk old range");
    let (start_new, new_count) = new_range.split_once(',').expect("hunk new range");
    let mut shape = HunkShape {
        start_old: start_old.parse().expect("hunk old start"),
        old_count: old_count.parse().expect("hunk old count"),
        start_new: start_new.parse().expect("hunk new start"),
        new_count: new_count.parse().expect("hunk new count"),
        context_before: 0,
        context_after: 0,
        removed: 0,
        added: 0,
    };
    let mut payload_seen = false;
    for line in lines {
        match line.chars().next().expect("nonempty hunk payload line") {
            '+' => {
                payload_seen = true;
                shape.added += 1;
            }
            '-' => {
                payload_seen = true;
                shape.removed += 1;
            }
            ' ' => {
                if payload_seen {
                    shape.context_after += 1;
                } else {
                    shape.context_before += 1;
                }
            }
            other => unreachable!("unexpected unified-diff marker {other:?}"),
        }
    }
    shape
}

/// Reconstruct the pre-image (context + removed lines) that `UNIFIED_DIFF`
/// applies to, so the prepared artifact can be exercised offline.
fn unified_diff_pre_image() -> String {
    let mut pre_image = String::new();
    for line in UNIFIED_DIFF.lines().skip(3) {
        let marker = line.chars().next().expect("nonempty diff line");
        if marker == '+' {
            continue;
        }
        pre_image.push_str(&line[1..]);
        pre_image.push('\n');
    }
    pre_image
}

/// Zero-context rendition of the same replacement: what `UNIFIED_DIFF` would
/// be if the standard context were stripped.
fn zero_context_rendition() -> String {
    let shape = hunk_shape(UNIFIED_DIFF);
    let old_start = shape.start_old + shape.context_before;
    let new_start = shape.start_new + shape.context_before;
    let mut rendition = format!(
        "--- a/{BASE_PATH}\n+++ b/{BASE_PATH}\n@@ -{old_start},{} +{new_start},{} @@\n",
        shape.removed, shape.added
    );
    for line in UNIFIED_DIFF.lines().skip(3) {
        let marker = line.chars().next().expect("nonempty diff line");
        if marker == ' ' {
            continue;
        }
        rendition.push_str(line);
        rendition.push('\n');
    }
    rendition
}

#[test]
fn checked_packet_is_exact_content_addressed_and_deterministic() {
    let packet = checked_packet().expect("checked packet");
    packet.validate().expect("checked packet validates");

    assert!(packet.packet_id.starts_with("eglot_patch_"));
    assert_eq!(packet.base.commit, BASE_COMMIT);
    assert_eq!(packet.base.tree_sha1, BASE_TREE_SHA1);
    assert_eq!(packet.base.path, BASE_PATH);
    assert_eq!(packet.base.blob_sha1, BASE_BLOB_SHA1);
    assert_eq!(packet.unified_diff, UNIFIED_DIFF);
    assert!(!packet.external_action_authorized);

    let first = render_checked_json().expect("first render");
    let second = render_checked_json().expect("second render");
    assert_eq!(first, second, "packet rendering must be byte-stable");
}

#[test]
fn exact_anchor_applies_once_and_preserves_surrounding_source() {
    let packet = checked_packet().expect("checked packet");
    let source = source_fixture();
    let patched = packet.apply_to_unverified_source(&source).expect("exact patch applies");

    assert_eq!(patched.matches(AFTER_ANCHOR).count(), 1);
    assert!(!patched.contains(BEFORE_ANCHOR));
    assert!(patched.contains("(markdown-mode . (\"marksman\"))"));
    assert!(patched.contains("(\"perllsp\" \"--stdio\")"));
    assert!(patched.contains("Perl::LanguageServer::run"));
}

#[test]
fn stale_or_duplicate_anchor_fails_closed() {
    let packet = checked_packet().expect("checked packet");
    let stale = source_fixture().replace("cperl-mode", "cperl-ts-mode");
    let error = packet
        .apply_to_unverified_source(&stale)
        .expect_err("moved upstream contact must block application");
    assert!(error.to_string().contains("appear once"));

    let duplicate = format!("{}{}", source_fixture(), source_fixture());
    let error = packet
        .apply_to_unverified_source(&duplicate)
        .expect_err("ambiguous duplicate anchor must block application");
    assert!(error.to_string().contains("appear once"));
}

#[test]
fn missing_stdio_is_rejected() {
    let mut packet = checked_packet().expect("checked packet");
    packet.after_anchor = packet.after_anchor.replace("--stdio", "--socket");
    let error = packet.validate().expect_err("perllsp must use the stdio transport");
    assert!(error.to_string().contains("perllsp --stdio alternative is missing"));
}

#[test]
fn legacy_fallback_cannot_be_removed() {
    let mut packet = checked_packet().expect("checked packet");
    packet.after_anchor =
        packet.after_anchor.replace("Perl::LanguageServer::run", "removed_legacy_fallback");
    let error = packet.validate().expect_err("legacy fallback removal must fail");
    assert!(error.to_string().contains("legacy Perl::LanguageServer fallback is missing"));
}

#[test]
fn perllsp_cannot_follow_the_ubiquitous_perl_fallback() {
    let mut packet = checked_packet().expect("checked packet");
    packet.after_anchor = concat!(
        "    (((perl-mode :language-id \"perl\")\n",
        "      (cperl-mode :language-id \"perl\"))\n",
        "     . ,(eglot-alternatives\n",
        "         '((\"perl\" \"-MPerl::LanguageServer\" \"-e\"\n",
        "            \"Perl::LanguageServer::run\")\n",
        "           (\"perllsp\" \"--stdio\"))))\n",
    )
    .to_string();
    let error = packet.validate().expect_err("reversed alternative order must fail");
    assert!(error.to_string().contains("perllsp must precede the ubiquitous perl fallback"));
}

#[test]
fn both_builtin_modes_must_use_language_id_perl() {
    let mut packet = checked_packet().expect("checked packet");
    packet.after_anchor = packet
        .after_anchor
        .replace("(cperl-mode :language-id \"perl\")", "(cperl-mode :language-id \"cperl\")");
    let error = packet.validate().expect_err("cperl must not become the protocol language id");
    assert!(error.to_string().contains("cperl-mode must explicitly negotiate language ID perl"));
}

#[test]
fn third_party_perl_mode_cannot_enter_the_core_patch() {
    let mut packet = checked_packet().expect("checked packet");
    packet.after_anchor = packet.after_anchor.replace(
        "(cperl-mode :language-id \"perl\"))",
        concat!(
            "(cperl-mode :language-id \"perl\")\n",
            "      (perl-ts-mode :language-id \"perl\"))"
        ),
    );
    let error =
        packet.validate().expect_err("third-party mode must remain a separate support subject");
    assert!(
        error.to_string().contains("third-party perl-ts-mode is outside the core upstream patch")
    );
}

#[test]
fn stale_base_identity_is_rejected() {
    let mut packet = checked_packet().expect("checked packet");
    packet.base.commit = "0000000000000000000000000000000000000000".to_string();
    let error = packet.validate().expect_err("patch must not float or apply to another source");
    assert!(error.to_string().contains("exact audited Eglot source"));
}

#[test]
fn external_action_cannot_be_authorized_by_the_packet() {
    let mut packet = checked_packet().expect("checked packet");
    packet.external_action_authorized = true;
    let error = packet.validate().expect_err("repository preparation cannot submit upstream");
    assert!(error.to_string().contains("cannot authorize"));
}

#[test]
fn content_or_claim_mutation_invalidates_packet_identity() {
    let mut packet = checked_packet().expect("checked packet");
    packet.proposed_pr_body.push_str(" changed");
    let error = packet.validate().expect_err("content mutation must invalidate the packet id");
    assert!(error.to_string().contains("content-address"));

    let mut packet = checked_packet().expect("checked packet");
    let _ = packet.limitations.pop();
    let error = packet.validate().expect_err("claim ceiling cannot silently broaden");
    assert!(error.to_string().contains("limitations"));
}

#[test]
fn applying_the_same_packet_twice_is_not_success() {
    let packet = checked_packet().expect("checked packet");
    let patched = packet.apply_to_unverified_source(&source_fixture()).expect("first application");
    let error = packet
        .apply_to_unverified_source(&patched)
        .expect_err("already-patched source must not count as a fresh apply");
    assert!(error.to_string().contains("appear once"));
}

/// Git blob SHA-1 of `source_fixture()` as produced by `git hash-object`
/// offline; the test asserts the Rust computation agrees with Git.
const FIXTURE_BLOB_SHA1: &str = "8693186cc014cf6ee96ae8714fc5c064a2892267";

#[test]
fn blob_verification_rejects_unaudited_bytes_with_intact_anchor() {
    let packet = checked_packet().expect("checked packet");

    // The anchor still appears exactly once, but the bytes are not the
    // declared subject: byte-exactness must fail before any replacement.
    let tampered = source_fixture().replace("marksman", "org-mode");
    let error = packet
        .verify_source_blob(&tampered)
        .expect_err("unaudited source bytes must fail blob verification");
    assert!(error.to_string().contains("refusing to patch unaudited source"));

    // The Rust Git-blob computation must agree with `git hash-object` for
    // the same bytes.
    let mut declared = checked_packet().expect("checked packet");
    declared.base.blob_sha1 = FIXTURE_BLOB_SHA1.to_string();
    declared
        .verify_source_blob(&source_fixture())
        .expect("byte-exact source must verify against its Git blob identity");

    // The packet's own declared subject stays pinned to the audited blob.
    assert_eq!(packet.base.blob_sha1, BASE_BLOB_SHA1);
}

#[test]
fn verified_application_rejects_unaudited_bytes_before_anchoring() {
    let packet = checked_packet().expect("checked packet");
    let tampered = source_fixture().replace("marksman", "org-mode");
    let error = packet
        .apply_to_verified_source(&tampered)
        .expect_err("verified application must reject unaudited bytes first");
    assert!(error.to_string().contains("Git blob"));
}

#[test]
fn unified_diff_declares_standard_context_with_accurate_hunk_counts() {
    let shape = hunk_shape(UNIFIED_DIFF);

    // Plain `git apply` refuses zero-context hunks unless `--unidiff-zero` is
    // passed; the prepared upstream artifact must carry the standard three
    // lines of context on each side so consumers can apply it ordinarily.
    assert_eq!(shape.context_before, 3, "context before the contact");
    assert_eq!(shape.context_after, 3, "context after the contact");
    let context = shape.context_before + shape.context_after;
    assert_eq!(shape.old_count, context + shape.removed, "old hunk count");
    assert_eq!(shape.new_count, context + shape.added, "new hunk count");
    assert_eq!(shape.removed, 2, "the exact current Perl contact is two lines");
    assert_eq!(shape.added, 6, "the reviewed replacement is six lines");
    assert_eq!(shape.start_new, shape.start_old, "replacement stays at its position");
    // The declared subject carries the Perl contact at lines 347-348; with
    // three context lines the hunk must start at line 344.
    assert_eq!(shape.start_old + shape.context_before, 347, "contact line position");

    // The diff's old side must be the exact reviewed contact plus context,
    // not an approximate paraphrase.
    let pre_image = unified_diff_pre_image();
    assert_eq!(pre_image.matches(BEFORE_ANCHOR).count(), 1);
    assert!(!pre_image.contains(AFTER_ANCHOR));
}

#[test]
fn unified_diff_applies_with_ordinary_git_apply_without_unidiff_zero() {
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let repo = workspace.path().join("repo");
    fs::create_dir_all(repo.join("lisp").join("progmodes")).expect("create package directory");
    fs::write(repo.join("lisp").join("progmodes").join("eglot.el"), unified_diff_pre_image())
        .expect("write pinned diff pre-image");
    fs::write(repo.join("prepared.diff"), UNIFIED_DIFF).expect("write prepared diff");
    fs::write(repo.join("zero-context.diff"), zero_context_rendition())
        .expect("write zero-context control diff");

    let run_git = |args: &[&str]| {
        let output = Command::new("git")
            .args(["-c", "core.autocrlf=false"])
            .args(args)
            .current_dir(&repo)
            .output()
            .expect("spawn git");
        (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned())
    };

    let (init_ok, init_stderr) = run_git(&["init", "-q"]);
    assert!(init_ok, "git init failed: {init_stderr}");

    // Ordinary application must accept the prepared artifact...
    let (apply_ok, apply_stderr) = run_git(&["apply", "--check", "prepared.diff"]);
    assert!(apply_ok, "ordinary git apply --check must accept the prepared diff: {apply_stderr}");

    // ...while the zero-context rendition of the same replacement must stay
    // rejected, proving the assertion above is not vacuous.
    let (control_ok, control_stderr) = run_git(&["apply", "--check", "zero-context.diff"]);
    assert!(!control_ok, "zero-context hunks must still require --unidiff-zero: {control_stderr}");

    // The real application produces exactly the reviewed replacement.
    let (applied, applied_stderr) = run_git(&["apply", "prepared.diff"]);
    assert!(applied, "prepared diff must apply: {applied_stderr}");
    let patched = fs::read_to_string(repo.join("lisp").join("progmodes").join("eglot.el"))
        .expect("read patched source");
    assert_eq!(patched.matches(AFTER_ANCHOR).count(), 1);
    assert!(!patched.contains(BEFORE_ANCHOR));
}
