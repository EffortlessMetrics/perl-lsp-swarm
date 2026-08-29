//! Contract tests for the exact-source Eglot registration patch (#13613).
//!
//! The packet is repository preparation only. Tests never contact or mutate
//! upstream and cannot promote source, accepted, released, or host evidence.

#![expect(clippy::expect_used)]

use xtask::emacs_eglot_upstream_patch::{
    AFTER_ANCHOR, BASE_BLOB_SHA1, BASE_COMMIT, BASE_PATH, BASE_TREE_SHA1, BEFORE_ANCHOR,
    UNIFIED_DIFF, checked_packet, render_checked_json,
};

fn source_fixture() -> String {
    format!(
        "(defcustom eglot-server-programs\n  '(\n{BEFORE_ANCHOR}    (markdown-mode . (\"marksman\")))\n  \"fixture\")\n"
    )
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
    let patched = packet.apply_to_source(&source).expect("exact patch applies");

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
        .apply_to_source(&stale)
        .expect_err("moved upstream contact must block application");
    assert!(error.to_string().contains("appear once"));

    let duplicate = format!("{}{}", source_fixture(), source_fixture());
    let error = packet
        .apply_to_source(&duplicate)
        .expect_err("ambiguous duplicate anchor must block application");
    assert!(error.to_string().contains("appear once"));
}

#[test]
fn missing_stdio_is_rejected() {
    let mut packet = checked_packet().expect("checked packet");
    packet.after_anchor = packet.after_anchor.replace("--stdio", "--socket");
    let error = packet
        .validate()
        .expect_err("perllsp must use the stdio transport");
    assert!(error.to_string().contains("reviewed selector"));
}

#[test]
fn legacy_fallback_cannot_be_removed() {
    let mut packet = checked_packet().expect("checked packet");
    packet.after_anchor = packet
        .after_anchor
        .replace("Perl::LanguageServer::run", "removed_legacy_fallback");
    let error = packet
        .validate()
        .expect_err("legacy fallback removal must fail");
    assert!(error.to_string().contains("reviewed selector"));
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
    let error = packet
        .validate()
        .expect_err("reversed alternative order must fail");
    assert!(error.to_string().contains("reviewed selector"));
}

#[test]
fn both_builtin_modes_must_use_language_id_perl() {
    let mut packet = checked_packet().expect("checked packet");
    packet.after_anchor = packet.after_anchor.replace(
        "(cperl-mode :language-id \"perl\")",
        "(cperl-mode :language-id \"cperl\")",
    );
    let error = packet
        .validate()
        .expect_err("cperl must not become the protocol language id");
    assert!(error.to_string().contains("reviewed selector"));
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
    let error = packet
        .validate()
        .expect_err("third-party mode must remain a separate support subject");
    assert!(error.to_string().contains("reviewed selector"));
}

#[test]
fn stale_base_identity_is_rejected() {
    let mut packet = checked_packet().expect("checked packet");
    packet.base.commit = "0000000000000000000000000000000000000000".to_string();
    let error = packet
        .validate()
        .expect_err("patch must not float or apply to another source");
    assert!(error.to_string().contains("exact audited Eglot source"));
}

#[test]
fn external_action_cannot_be_authorized_by_the_packet() {
    let mut packet = checked_packet().expect("checked packet");
    packet.external_action_authorized = true;
    let error = packet
        .validate()
        .expect_err("repository preparation cannot submit upstream");
    assert!(error.to_string().contains("cannot authorize"));
}

#[test]
fn content_or_claim_mutation_invalidates_packet_identity() {
    let mut packet = checked_packet().expect("checked packet");
    packet.proposed_pr_body.push_str(" changed");
    let error = packet
        .validate()
        .expect_err("content mutation must invalidate the packet id");
    assert!(error.to_string().contains("content-address"));

    let mut packet = checked_packet().expect("checked packet");
    let _ = packet.limitations.pop();
    let error = packet
        .validate()
        .expect_err("claim ceiling cannot silently broaden");
    assert!(error.to_string().contains("limitations"));
}

#[test]
fn applying_the_same_packet_twice_is_not_success() {
    let packet = checked_packet().expect("checked packet");
    let patched = packet
        .apply_to_source(&source_fixture())
        .expect("first application");
    let error = packet
        .apply_to_source(&patched)
        .expect_err("already-patched source must not count as a fresh apply");
    assert!(error.to_string().contains("appear once"));
}