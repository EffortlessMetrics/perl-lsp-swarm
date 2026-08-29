//! Contract tests for exact Emacs stock-discovery observations (#13610).
//!
//! These tests prove source-registration facts only. They do not launch Emacs,
//! promote editor support, or treat absence of a built-in entry as server
//! incompatibility.

#![expect(clippy::expect_used, clippy::panic)]

use xtask::editor_client_compat::ClientSourceState;
use xtask::emacs_stock_discovery::{RegistrationEntry, checked_baseline, render_checked_json};
use xtask::emacs_subject_manifest::SubjectClientKind;

#[test]
fn checked_baseline_is_complete_and_deterministic() {
    let baseline = checked_baseline();
    baseline.validate().expect("checked baseline must validate");

    let first = render_checked_json().expect("first rendering");
    let second = render_checked_json().expect("second rendering");
    assert_eq!(
        first, second,
        "identical exact observations must render byte-stably"
    );
    assert!(
        first.ends_with('\n'),
        "machine projection must have one terminal newline"
    );
}

#[test]
fn released_and_source_rows_stay_independent_for_both_clients() {
    let baseline = checked_baseline();
    let dimensions = baseline
        .observations
        .iter()
        .map(|row| (row.client_kind, row.source_state, row.subject_id.as_str()))
        .collect::<Vec<_>>();

    assert_eq!(
        dimensions,
        vec![
            (
                SubjectClientKind::ExternalEglot,
                ClientSourceState::Released,
                "released_eglot_gnu_elpa_1_24",
            ),
            (
                SubjectClientKind::ExternalEglot,
                ClientSourceState::UpstreamSource,
                "source_eglot_emacs_f4f249a2",
            ),
            (
                SubjectClientKind::LspMode,
                ClientSourceState::Released,
                "released_lsp_mode_10_0_0",
            ),
            (
                SubjectClientKind::LspMode,
                ClientSourceState::UpstreamSource,
                "source_lsp_mode_e15b8205",
            ),
        ],
        "a current source candidate must not overwrite or relabel the released subject"
    );
}

#[test]
fn exact_rows_require_manual_registration_for_perllsp() {
    let baseline = checked_baseline();
    for row in baseline.observations {
        assert!(
            row.observation_complete,
            "{} must be a complete source observation",
            row.subject_id
        );
        assert!(
            !row.manual_registration_injected,
            "{} must not contain repository setup",
            row.subject_id
        );
        assert!(
            !row.perllsp_present,
            "{} unexpectedly contains a stock perllsp entry",
            row.subject_id
        );
        assert!(
            row.entries.iter().all(|entry| {
                entry.server_id.as_deref() != Some("perllsp")
                    && entry.command.first().map(String::as_str) != Some("perllsp")
            }),
            "perllsp absence must be derived from the exact observed entries"
        );
    }
}

#[test]
fn eglot_rows_retain_the_existing_perl_language_server_contact() {
    let baseline = checked_baseline();
    for row in baseline
        .observations
        .iter()
        .filter(|row| row.client_kind == SubjectClientKind::ExternalEglot)
    {
        assert_eq!(row.entries.len(), 1);
        let entry = &row.entries[0];
        assert_eq!(entry.entry_id, "perl_language_server");
        assert_eq!(
            entry.modes.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["perl-mode", "cperl-mode"]
        );
        assert_eq!(
            entry
                .command
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec![
                "perl",
                "-MPerl::LanguageServer",
                "-e",
                "Perl::LanguageServer::run",
            ]
        );
    }
}

#[test]
fn lsp_mode_rows_retain_competing_clients_and_priority_order() {
    let baseline = checked_baseline();
    for row in baseline
        .observations
        .iter()
        .filter(|row| row.client_kind == SubjectClientKind::LspMode)
    {
        let observed = row
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.entry_id.as_str(),
                    entry.server_id.as_deref(),
                    entry.priority,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            observed,
            vec![
                ("perlnavigator", Some("perlnavigator"), Some(0)),
                ("pls", Some("pls"), Some(-1)),
                (
                    "perl_language_server",
                    Some("perl-language-server"),
                    Some(-2),
                ),
            ],
            "lsp-mode selection context must not be flattened to one generic Perl client"
        );
    }
}

#[test]
fn incomplete_observation_cannot_report_absence() {
    let mut baseline = checked_baseline();
    baseline.observations[0].observation_complete = false;
    let error = baseline
        .validate()
        .expect_err("incomplete source inventory must fail closed");
    assert!(error.to_string().contains("incomplete observation"));
}

#[test]
fn manual_registration_cannot_manufacture_stock_presence() {
    let mut baseline = checked_baseline();
    baseline.observations[0].manual_registration_injected = true;
    let error = baseline
        .validate()
        .expect_err("manual setup must not satisfy stock discovery");
    assert!(error.to_string().contains("manual registration"));
}

#[test]
fn hidden_perllsp_entry_must_change_the_derived_presence_field() {
    let mut baseline = checked_baseline();
    baseline.observations[0].entries.push(RegistrationEntry {
        entry_id: "perllsp".to_string(),
        modes: vec!["perl-mode".to_string(), "cperl-mode".to_string()],
        language_id: Some("perl".to_string()),
        command: vec!["perllsp".to_string(), "--stdio".to_string()],
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
    let error = baseline
        .validate()
        .expect_err("floating upstream source must fail");
    assert!(error.to_string().contains("40-hex"));
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
    let error = baseline
        .validate()
        .expect_err("priority drift must be explicit");
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
    let error = baseline
        .validate()
        .expect_err("missing PLS row must fail closed");
    assert!(error.to_string().contains("three Perl clients"));
}