//! Contract tests for the exact-source lsp-mode perllsp client patch (#13614).
//!
//! These tests exercise a frozen, attested source-tree shape. They never run a
//! real lsp-mode workspace, submit upstream, or promote a released client row.

#![expect(clippy::expect_used)]

use std::collections::BTreeMap;
use xtask::emacs_lsp_mode_upstream_patch::{
    AttestedUpstreamFile, AttestedUpstreamTree, BASE_COMMIT, BASE_REPOSITORY,
    BASE_TREE_SHA1, CLIENT_CATALOG_PATH, LSP_MODE_PATH, MKDOCS_PATH,
    NEW_CLIENT_CONTENT, NEW_CLIENT_PATH, checked_packet, render_checked_json,
};

const PACKAGE_ANCHOR: &str = concat!(
    "     lsp-ocaml lsp-odin lsp-openscad lsp-pascal lsp-perl lsp-perlnavigator\n",
    "      lsp-php lsp-pls lsp-postgres lsp-prisma",
);
const CATALOG_ANCHOR: &str = r#"  {
    "name": "perl",
    "full-name": "Perl",
    "server-name": "Perl::LanguageServer",
    "server-url": "https://github.com/richterger/Perl-LanguageServer",
    "installation": "cpan Perl::LanguageServer",
    "debugger": "Not available"
  },
  {
    "name": "perlnavigator",
"#;
const MKDOCS_ANCHOR: &str = concat!(
    "    - Perl (PLS): page/lsp-pls.md\n",
    "    - Perl (Perl::LanguageServer): page/lsp-perl.md\n",
    "    - Perl (Navigator): page/lsp-perlnavigator.md\n",
);

fn file(blob_sha1: &str, content: impl Into<String>) -> AttestedUpstreamFile {
    AttestedUpstreamFile {
        blob_sha1: blob_sha1.to_string(),
        content: content.into(),
    }
}

fn attested_tree() -> AttestedUpstreamTree {
    let mut files = BTreeMap::new();
    files.insert(
        LSP_MODE_PATH.to_string(),
        file(
            "9575875cd4c7ef49ab0bd8e5473e44f73c4a0c7d",
            format!("header\n{PACKAGE_ANCHOR}\nfooter\n"),
        ),
    );
    files.insert(
        CLIENT_CATALOG_PATH.to_string(),
        file(
            "913c2724516bc92b807c0d1d79a8c9147c855d10",
            format!("[\n{CATALOG_ANCHOR}    \"full-name\": \"Perl Navigator\"\n  }}\n]\n"),
        ),
    );
    files.insert(
        MKDOCS_PATH.to_string(),
        file(
            "b4bcf68c800f13de02b98f414c21e2eb1b729edc",
            format!("nav:\n{MKDOCS_ANCHOR}"),
        ),
    );
    files.insert(
        "clients/lsp-perl.el".to_string(),
        file(
            "28569f7ecf22a5b02762976ef338693c655679ad",
            "legacy Perl::LanguageServer client\n",
        ),
    );
    files.insert(
        "clients/lsp-perlnavigator.el".to_string(),
        file(
            "51cbc768c960c40433d61299cc837b94f7830423",
            "Perl Navigator client\n",
        ),
    );
    files.insert(
        "clients/lsp-pls.el".to_string(),
        file(
            "f5437fbf739dd661d0850ede25ad7ef73dbf81d4",
            "PLS client\n",
        ),
    );
    AttestedUpstreamTree {
        repository: BASE_REPOSITORY.to_string(),
        commit: BASE_COMMIT.to_string(),
        tree_sha1: BASE_TREE_SHA1.to_string(),
        files,
    }
}

#[test]
fn checked_packet_is_exact_content_addressed_and_deterministic() {
    let packet = checked_packet().expect("checked packet");
    packet.validate().expect("checked packet validates");

    assert!(packet.packet_id.starts_with("lsp_mode_patch_"));
    assert!(packet.patch_sha256.starts_with("sha256:"));
    assert_eq!(packet.base_repository, BASE_REPOSITORY);
    assert_eq!(packet.base_commit, BASE_COMMIT);
    assert_eq!(packet.base_tree_sha1, BASE_TREE_SHA1);
    assert!(!packet.external_action_authorized);

    let first = render_checked_json().expect("first render");
    let second = render_checked_json().expect("second render");
    assert_eq!(first, second, "packet rendering must be byte-stable");
}

#[test]
fn exact_patch_adds_every_upstream_consumption_surface() {
    let packet = checked_packet().expect("checked packet");
    let input = attested_tree();
    let output = packet
        .apply_to_attested_tree(&input)
        .expect("exact patch applies");

    assert_eq!(
        output.get(NEW_CLIENT_PATH).map(String::as_str),
        Some(NEW_CLIENT_CONTENT)
    );
    assert!(
        output
            .get(LSP_MODE_PATH)
            .is_some_and(|content| content.contains("lsp-perl lsp-perllsp lsp-perlnavigator"))
    );
    assert!(
        output.get(CLIENT_CATALOG_PATH).is_some_and(|content| {
            content.contains("\"name\": \"perllsp\"")
                && content.contains("cargo install perllsp")
                && content.contains("https://github.com/EffortlessMetrics/perl-lsp")
        })
    );
    assert!(
        output
            .get(MKDOCS_PATH)
            .is_some_and(|content| content.contains("page/lsp-perllsp.md"))
    );
}

#[test]
fn new_client_has_exact_command_activation_mode_and_selection_shape() {
    let packet = checked_packet().expect("checked packet");
    let client = packet.added_files.first().expect("new client");
    let content = &client.content;

    assert!(content.contains("(lambda () (list lsp-perllsp-executable \"--stdio\"))"));
    assert!(content.contains(":activation-fn (lsp-activate-on \"perl\")"));
    assert!(content.contains(":major-modes '(perl-mode cperl-mode)"));
    assert!(content.contains(":priority 1"));
    assert!(content.contains(":server-id 'perllsp"));
    assert!(content.contains("(lsp-consistency-check lsp-perllsp)"));
    assert!(!content.contains("perl-ts-mode"));
    assert!(!content.contains("lsp-dependency"));
    assert!(!content.contains("download-server-fn"));
    assert!(!content.contains("lsp-register-custom-settings"));
}

#[test]
fn existing_perl_clients_are_preserved_as_fallbacks() {
    let packet = checked_packet().expect("checked packet");
    let input = attested_tree();
    let output = packet
        .apply_to_attested_tree(&input)
        .expect("exact patch applies");

    for path in [
        "clients/lsp-perlnavigator.el",
        "clients/lsp-pls.el",
        "clients/lsp-perl.el",
    ] {
        assert_eq!(
            output.get(path),
            input.files.get(path).map(|file| &file.content),
            "fallback client {path} must remain byte-identical"
        );
    }

    let priorities = packet
        .existing_perl_clients
        .iter()
        .map(|client| (client.server_id.as_str(), client.priority))
        .collect::<Vec<_>>();
    assert_eq!(
        priorities,
        vec![
            ("perlnavigator", 0),
            ("pls", -1),
            ("perl-language-server", -2),
        ]
    );
}

#[test]
fn coexistence_matrix_keeps_disable_absence_and_third_party_cases() {
    let packet = checked_packet().expect("checked packet");
    let cases = packet
        .selection_cases
        .iter()
        .map(|case| (case.case_id.as_str(), case.expected_disposition.as_str()))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(
        cases.get("perllsp_with_perlnavigator"),
        Some(&"perllsp_selected_priority_1_over_0")
    );
    assert_eq!(
        cases.get("perllsp_absent"),
        Some(&"existing_installed_client_remains_eligible")
    );
    assert_eq!(
        cases.get("perllsp_explicitly_disabled"),
        Some(&"existing_enabled_client_remains_eligible")
    );
    assert_eq!(
        cases.get("perl_ts_mode"),
        Some(&"base_perllsp_client_not_eligible")
    );
}

#[test]
fn wrong_tree_or_file_identity_fails_closed() {
    let packet = checked_packet().expect("checked packet");
    let mut input = attested_tree();
    input.tree_sha1 = "0000000000000000000000000000000000000000".to_string();
    let error = packet
        .apply_to_attested_tree(&input)
        .expect_err("wrong upstream tree must fail");
    assert!(error.to_string().contains("exact base"));

    let mut input = attested_tree();
    let lsp_mode = input
        .files
        .get_mut(LSP_MODE_PATH)
        .expect("lsp-mode fixture");
    lsp_mode.blob_sha1 = "0000000000000000000000000000000000000000".to_string();
    let error = packet
        .apply_to_attested_tree(&input)
        .expect_err("wrong edited-file blob must fail");
    assert!(error.to_string().contains("wrong blob"));
}

#[test]
fn stale_missing_or_duplicate_anchor_fails_closed() {
    let packet = checked_packet().expect("checked packet");
    let mut input = attested_tree();
    let lsp_mode = input
        .files
        .get_mut(LSP_MODE_PATH)
        .expect("lsp-mode fixture");
    lsp_mode.content = lsp_mode.content.replace("lsp-perlnavigator", "lsp-perl-nav");
    let error = packet
        .apply_to_attested_tree(&input)
        .expect_err("moved package-list anchor must fail");
    assert!(error.to_string().contains("appear once"));

    let mut input = attested_tree();
    let mkdocs = input.files.get_mut(MKDOCS_PATH).expect("mkdocs fixture");
    mkdocs.content.push_str(MKDOCS_ANCHOR);
    let error = packet
        .apply_to_attested_tree(&input)
        .expect_err("ambiguous docs anchor must fail");
    assert!(error.to_string().contains("appear once"));
}

#[test]
fn already_present_client_cannot_be_called_a_fresh_patch() {
    let packet = checked_packet().expect("checked packet");
    let mut input = attested_tree();
    input.files.insert(
        NEW_CLIENT_PATH.to_string(),
        file(
            "0000000000000000000000000000000000000000",
            NEW_CLIENT_CONTENT,
        ),
    );
    let error = packet
        .apply_to_attested_tree(&input)
        .expect_err("existing client must block add-file patch");
    assert!(error.to_string().contains("already exists"));
}

#[test]
fn missing_fallback_client_cannot_pass_coexistence() {
    let packet = checked_packet().expect("checked packet");
    let mut input = attested_tree();
    let _ = input.files.remove("clients/lsp-pls.el");
    let error = packet
        .apply_to_attested_tree(&input)
        .expect_err("missing fallback client must fail");
    assert!(error.to_string().contains("missing fallback client"));
}

#[test]
fn command_mode_priority_or_package_mutation_is_rejected() {
    let mut packet = checked_packet().expect("checked packet");
    let client = packet.added_files.first_mut().expect("new client");
    client.content = client.content.replace("\"--stdio\"", "\"--socket\"");
    let error = packet.validate().expect_err("stdio is required");
    assert!(error.to_string().contains("reviewed lsp-perllsp client"));

    let mut packet = checked_packet().expect("checked packet");
    let client = packet.added_files.first_mut().expect("new client");
    client.content = client.content.replace(
        ":major-modes '(perl-mode cperl-mode)",
        ":major-modes '(perl-mode cperl-mode perl-ts-mode)",
    );
    let error = packet
        .validate()
        .expect_err("third-party mode cannot enter base client");
    assert!(error.to_string().contains("reviewed lsp-perllsp client"));

    let mut packet = checked_packet().expect("checked packet");
    let client = packet.added_files.first_mut().expect("new client");
    client.content = client.content.replace(":priority 1", ":priority 0");
    let error = packet
        .validate()
        .expect_err("perllsp cannot tie Perl Navigator while claiming default selection");
    assert!(error.to_string().contains("reviewed lsp-perllsp client"));

    let mut packet = checked_packet().expect("checked packet");
    let replacement = packet
        .replacements
        .iter_mut()
        .find(|replacement| replacement.source.path == LSP_MODE_PATH)
        .expect("package replacement");
    replacement.after = replacement.after.replace("lsp-perllsp ", "");
    let error = packet
        .validate()
        .expect_err("client file without package loading must fail");
    assert!(error.to_string().contains("replacements must remain exact"));
}

#[test]
fn docs_or_navigation_omission_is_rejected() {
    for path in [CLIENT_CATALOG_PATH, MKDOCS_PATH] {
        let mut packet = checked_packet().expect("checked packet");
        let replacement = packet
            .replacements
            .iter_mut()
            .find(|replacement| replacement.source.path == path)
            .expect("docs replacement");
        replacement.after = replacement.before.clone();
        let error = packet
            .validate()
            .expect_err("every upstream consumption surface is required");
        assert!(error.to_string().contains("replacements must remain exact"));
    }
}

#[test]
fn external_action_or_claim_widening_is_rejected() {
    let mut packet = checked_packet().expect("checked packet");
    packet.external_action_authorized = true;
    let error = packet
        .validate()
        .expect_err("repository packet cannot submit upstream");
    assert!(error.to_string().contains("cannot authorize"));

    let mut packet = checked_packet().expect("checked packet");
    let _ = packet.limitations.pop();
    let error = packet
        .validate()
        .expect_err("host/release limitation cannot disappear");
    assert!(error.to_string().contains("limitations"));
}

#[test]
fn patch_or_correspondence_mutation_invalidates_content_identity() {
    let mut packet = checked_packet().expect("checked packet");
    packet.unified_diff.push_str("# changed\n");
    let error = packet
        .validate()
        .expect_err("patch bytes must be exact");
    assert!(error.to_string().contains("unified patch bytes"));

    let mut packet = checked_packet().expect("checked packet");
    packet.proposed_pr_body.push_str(" changed");
    let error = packet
        .validate()
        .expect_err("correspondence mutation must invalidate packet id");
    assert!(error.to_string().contains("content-address"));
}