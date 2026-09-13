//! Contract tests for exact Emacs stock-discovery observations (#13610).
//!
//! These tests prove source-registration facts only. They do not launch Emacs,
//! promote editor support, or treat absence of a built-in entry as server
//! incompatibility.

#![expect(clippy::expect_used)]

use xtask::editor_client_compat::ClientSourceState;
use xtask::emacs_stock_discovery::{RegistrationEntry, checked_baseline, render_checked_json};
use xtask::emacs_subject_manifest::SubjectClientKind;

#[test]
fn checked_baseline_is_complete_and_deterministic() {
    let baseline = checked_baseline();
    baseline.validate().expect("checked baseline must validate");

    let first = render_checked_json().expect("first rendering");
    let second = render_checked_json().expect("second rendering");
    assert_eq!(first, second, "identical exact observations must render byte-stably");
    assert!(first.ends_with('\n'), "machine projection must have one terminal newline");
}

#[test]
fn released_and_source_rows_stay_independent_for_both_clients() {
    let baseline = checked_baseline();
    let dimensions = baseline
        .observations
        .iter()
        .map(|row| (row.client_kind, row.source_state, row.observation_id.as_str()))
        .collect::<Vec<_>>();

    assert_eq!(
        dimensions,
        vec![
            (
                SubjectClientKind::ExternalEglot,
                ClientSourceState::Released,
                "eglot_released_1_24_stock_registry",
            ),
            (
                SubjectClientKind::ExternalEglot,
                ClientSourceState::UpstreamSource,
                "eglot_source_f4f249a2_stock_registry",
            ),
            (
                SubjectClientKind::LspMode,
                ClientSourceState::Released,
                "lsp_mode_released_10_0_0_clients",
            ),
            (
                SubjectClientKind::LspMode,
                ClientSourceState::UpstreamSource,
                "lsp_mode_source_e15b8205_clients",
            ),
        ],
        "a current source observation must not overwrite or relabel a released row"
    );
}

#[test]
fn exact_rows_bind_commit_tree_and_complete_search_scope() {
    let baseline = checked_baseline();
    for row in baseline.observations {
        assert_eq!(row.commit.len(), 40, "commit must be exact");
        assert_eq!(row.tree_sha1.len(), 40, "tree must be exact");
        assert!(
            row.observation_complete,
            "{} must be a complete source observation",
            row.observation_id
        );
        assert!(
            !row.search_scope.is_empty(),
            "{} must retain the exhaustive search scope",
            row.observation_id
        );
        assert!(
            !row.observed_files.is_empty(),
            "{} must retain exact source blobs",
            row.observation_id
        );
    }
}

#[test]
fn exact_rows_require_manual_registration_for_perllsp() {
    let baseline = checked_baseline();
    for row in baseline.observations {
        assert!(
            !row.manual_registration_injected,
            "{} must not contain repository setup",
            row.observation_id
        );
        assert!(
            !row.perllsp_present,
            "{} unexpectedly contains a stock perllsp entry",
            row.observation_id
        );
        assert!(
            row.entries.iter().all(|entry| {
                entry.entry_id != "perllsp"
                    && entry.server_id.as_deref() != Some("perllsp")
                    && entry.command_shape.first().is_none_or(|program| program != "perllsp")
            }),
            "perllsp absence must be derived from the exact observed entries"
        );
    }
}

#[test]
fn eglot_rows_retain_the_existing_perl_language_server_contact() {
    let baseline = checked_baseline();
    baseline.validate().expect("checked baseline must validate");
    for row in baseline
        .observations
        .iter()
        .filter(|row| row.client_kind == SubjectClientKind::ExternalEglot)
    {
        assert_eq!(row.entries.len(), 1);
        let entry = row.entries.first().expect("Eglot Perl contact");
        assert_eq!(entry.entry_id, "perl_language_server");
        assert_eq!(
            entry.major_modes.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["perl-mode", "cperl-mode"]
        );
        assert!(entry.activation_language.is_none());
        assert_eq!(
            entry.command_shape.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["perl", "-MPerl::LanguageServer", "-e", "Perl::LanguageServer::run",]
        );
    }
}

#[test]
fn lsp_mode_rows_retain_competing_clients_and_priority_order() {
    let baseline = checked_baseline();
    baseline.validate().expect("checked baseline must validate");
    for row in
        baseline.observations.iter().filter(|row| row.client_kind == SubjectClientKind::LspMode)
    {
        let observed = row
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.entry_id.as_str(),
                    entry.server_id.as_deref(),
                    entry.priority,
                    entry.activation_language.as_deref(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            observed,
            vec![
                ("perlnavigator", Some("perlnavigator"), Some(0), Some("perl"),),
                ("pls", Some("pls"), Some(-1), Some("perl")),
                ("perl_language_server", Some("perl-language-server"), Some(-2), None,),
            ],
            "lsp-mode selection context must not be flattened to one generic Perl client"
        );

        // Self-sufficient against validator drift: assert the exact
        // registration facts per entry, mirroring the Eglot branch.
        let navigator = row
            .entries
            .iter()
            .find(|entry| entry.entry_id == "perlnavigator")
            .expect("perlnavigator row");
        assert!(navigator.major_modes.is_empty(), "Perl Navigator activates by language id");
        assert_eq!(
            navigator.command_shape.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["managed_or_configured:perlnavigator", "--stdio"],
        );
        let pls = row.entries.iter().find(|entry| entry.entry_id == "pls").expect("pls row");
        assert!(pls.major_modes.is_empty(), "PLS activates by language id");
        assert_eq!(
            pls.command_shape.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["configured:lsp-pls-executable", "configured:lsp-pls-arguments..."],
        );
        let legacy = row
            .entries
            .iter()
            .find(|entry| entry.entry_id == "perl_language_server")
            .expect("perl_language_server row");
        assert_eq!(
            legacy.major_modes.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["perl-mode", "cperl-mode"],
        );
        assert_eq!(
            legacy.command_shape.iter().map(String::as_str).collect::<Vec<_>>(),
            vec![
                "configured:lsp-perl-language-server-path",
                "-MPerl::LanguageServer",
                "-e",
                "Perl::LanguageServer::run",
                "--",
                "--port {port} --version {client-version}",
            ],
        );
    }
}

#[test]
fn incomplete_observation_cannot_report_absence() {
    let mut baseline = checked_baseline();
    baseline.observations[0].observation_complete = false;
    let error = baseline.validate().expect_err("incomplete source inventory must fail closed");
    assert!(error.to_string().contains("incomplete observation"));
}

#[test]
fn manual_registration_cannot_manufacture_stock_presence() {
    let mut baseline = checked_baseline();
    baseline.observations[0].manual_registration_injected = true;
    let error = baseline.validate().expect_err("manual setup must not satisfy stock discovery");
    assert!(error.to_string().contains("manual registration"));
}

#[test]
fn hidden_perllsp_entry_must_change_the_derived_presence_field() {
    let mut baseline = checked_baseline();
    baseline.observations[0].entries.push(RegistrationEntry {
        entry_id: "perllsp".to_string(),
        major_modes: vec!["perl-mode".to_string(), "cperl-mode".to_string()],
        activation_language: None,
        command_shape: vec!["perllsp".to_string(), "--stdio".to_string()],
        server_id: Some("perllsp".to_string()),
        priority: None,
    });
    let error = baseline
        .validate()
        .expect_err("perllsp_present=false cannot survive an observed perllsp entry");
    assert!(
        error.to_string().contains("perllsp_present")
            || error.to_string().contains("one Perl contact")
    );
}

#[test]
fn floating_source_ref_is_rejected() {
    let mut baseline = checked_baseline();
    baseline.observations[1].commit = "master".to_string();
    let error = baseline.validate().expect_err("floating upstream source must fail");
    assert!(error.to_string().contains("40-hex"));
}

#[test]
fn missing_tree_identity_cannot_support_an_absence_claim() {
    let mut baseline = checked_baseline();
    baseline.observations[3].tree_sha1.clear();
    let error = baseline.validate().expect_err("absence must bind the complete repository tree");
    assert!(error.to_string().contains("tree_sha1"));
}

#[test]
fn changed_lsp_mode_priority_is_visible() {
    let mut baseline = checked_baseline();
    let row = baseline
        .observations
        .iter_mut()
        .find(|row| row.client_kind == SubjectClientKind::LspMode)
        .expect("lsp-mode row");
    row.entries[0].priority = Some(-3);
    let error = baseline.validate().expect_err("priority drift must be explicit");
    assert!(error.to_string().contains("priority order"));
}

#[test]
fn missing_competing_client_is_not_a_complete_lsp_mode_observation() {
    let mut baseline = checked_baseline();
    let row = baseline
        .observations
        .iter_mut()
        .find(|row| row.client_kind == SubjectClientKind::LspMode)
        .expect("lsp-mode row");
    row.entries.remove(1);
    let error = baseline.validate().expect_err("missing PLS row must fail closed");
    assert!(error.to_string().contains("three Perl clients"));
}

#[test]
fn swapped_shape_valid_commit_is_rejected() {
    let mut baseline = checked_baseline();
    baseline.observations[0].commit = "0123456789abcdef0123456789abcdef01234567".to_string();
    let error =
        baseline.validate().expect_err("a replaced-but-shape-valid commit must fail closed");
    assert!(
        error.to_string().contains("audited"),
        "swapped commit must be bound to the audited revision: {error}"
    );
}

#[test]
fn swapped_shape_valid_tree_is_rejected() {
    let mut baseline = checked_baseline();
    baseline.observations[3].tree_sha1 = "0123456789abcdef0123456789abcdef01234567".to_string();
    let error = baseline.validate().expect_err("a replaced-but-shape-valid tree must fail closed");
    assert!(
        error.to_string().contains("audited"),
        "swapped tree must be bound to the audited tree: {error}"
    );
}

#[test]
fn swapped_shape_valid_blob_is_rejected() {
    let mut baseline = checked_baseline();
    baseline.observations[2].observed_files[0].git_blob_sha1 =
        "0123456789abcdef0123456789abcdef01234567".to_string();
    let error = baseline.validate().expect_err("a replaced-but-shape-valid blob must fail closed");
    assert!(
        error.to_string().contains("audited"),
        "swapped blob must be bound to the audited identity: {error}"
    );
}

#[test]
fn unaudited_observation_id_is_rejected() {
    let mut baseline = checked_baseline();
    baseline.observations[0].observation_id = "unaudited_row".to_string();
    let error =
        baseline.validate().expect_err("an observation no audited record owns must fail closed");
    assert!(error.to_string().contains("unaudited observation_id"));
}

#[test]
fn released_rows_bind_canonical_manifest_subjects() {
    let baseline = checked_baseline();
    let subject_ids = baseline
        .observations
        .iter()
        .map(|row| (row.source_state, row.subject_id.as_deref(), row.observation_id.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        subject_ids,
        vec![
            (
                ClientSourceState::Released,
                Some("released_eglot_gnu_elpa_1_24"),
                "eglot_released_1_24_stock_registry",
            ),
            (ClientSourceState::UpstreamSource, None, "eglot_source_f4f249a2_stock_registry",),
            (
                ClientSourceState::Released,
                Some("released_lsp_mode_melpa_stable_10_0_0"),
                "lsp_mode_released_10_0_0_clients",
            ),
            (ClientSourceState::UpstreamSource, None, "lsp_mode_source_e15b8205_clients",),
        ],
        "released rows must join the canonical checked-manifest subjects; \
         upstream-source rows must not manufacture aliases"
    );
}

#[test]
fn alias_subject_id_cannot_replace_the_canonical_manifest_id() {
    let mut baseline = checked_baseline();
    baseline.observations[2].subject_id = Some("lsp_mode_released_10_0_0_clients".to_string());
    let error = baseline
        .validate()
        .expect_err("a manufactured alias must not replace the canonical manifest subject id");
    assert!(
        error.to_string().contains("canonical checked-manifest subject"),
        "alias join must fail closed: {error}"
    );
}

#[test]
fn corrupted_server_id_is_rejected() {
    let mut baseline = checked_baseline();
    let row = baseline
        .observations
        .iter_mut()
        .find(|row| row.client_kind == SubjectClientKind::LspMode)
        .expect("lsp-mode row");
    row.entries[0].server_id = Some("bogus".to_string());
    let error = baseline.validate().expect_err("corrupted registration facts must fail closed");
    assert!(error.to_string().contains("server id"), "server-id drift must be explicit: {error}");
}
